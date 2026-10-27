//! 1.2.2 : planification (calcul pur, rattrapage, DST, fichiers verrouilles, moteur), reconnaissance par nom d'exe generique,
//! libelles de l'ajout manuel et contraste des libelles.
use agss::backup::{self, Plan};
use agss::config::Game;
use agss::gamedb::{Db, DbGame, GameDb};
use agss::gamedetect::*;
use agss::i18n::{tr, Lang, K};
use agss::process::ProcInfo;
use agss::schedule::*;
use agss::timefmt::{FixedTz, TzProvider};
use agss::watch::{Logger, WatchCfg, Watcher};
use agss::Ctx;
/// Verrou exclusif sur un fichier tant que la valeur retournee vit (Windows : partage refuse ; Linux : flock).
#[cfg(windows)]
fn hold_lock(p: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).share_mode(1).open(p).unwrap()
}
#[cfg(not(windows))]
fn hold_lock(p: &Path) -> std::fs::File {
    use std::os::unix::io::AsRawFd;
    let f = std::fs::OpenOptions::new().read(true).write(true).open(p).unwrap();
    assert_eq!(unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    f
}
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const DAY: i64 = 86_400;
const T0: i64 = 20_000 * DAY; // minuit UTC d'un jour quelconque
const UTC: FixedTz = FixedTz(0);

fn hu(h: u32, m: u32, s: u32) -> u32 {
    h * 3600 + m * 60 + s
}

fn hm(h: i64, m: i64, s: i64) -> i64 {
    h * 3600 + m * 60 + s
}

// ------------------------------------------------------------------ calcul pur

#[test]
fn heures_fixes_prochaine_echeance_execution_et_pas_de_doublon() {
    let s = Schedule::fixed(vec![hu(18, 0, 0), hu(12, 0, 0)]);
    assert!(s.is_valid());
    // Avant midi : rien, prochaine = 12:00:00.
    let e = evaluate(&s, T0 + hm(6, 0, 0), T0 + hm(10, 0, 0), &UTC).unwrap();
    assert_eq!((e.due.clone(), e.next_utc), (None, T0 + hm(12, 0, 0)));
    // A 12:00:00 pile : echeance, label a la seconde, prochaine = 18:00:00.
    let e = evaluate(&s, T0 + hm(6, 0, 0), T0 + hm(12, 0, 0), &UTC).unwrap();
    let d = e.due.unwrap();
    assert_eq!(d.label, "heure fixe 12:00:00");
    assert!(!d.catch_up);
    assert_eq!(e.next_utc, T0 + hm(18, 0, 0));
    // Une fois executee (ancre = maintenant) : plus d'echeance a la seconde suivante.
    let e = evaluate(&s, T0 + hm(12, 0, 0), T0 + hm(12, 0, 1), &UTC).unwrap();
    assert!(e.due.is_none());
    // Apres 18:00 : la prochaine est 12:00 le lendemain.
    let e = evaluate(&s, T0 + hm(18, 0, 0), T0 + hm(19, 0, 0), &UTC).unwrap();
    assert_eq!(e.next_utc, T0 + DAY + hm(12, 0, 0));
}

#[test]
fn rattrapage_au_demarrage_une_seule_execution() {
    let s = Schedule::fixed(vec![hu(12, 0, 0), hu(18, 0, 0)]);
    // PC eteint de 13:00 a 09:00 le lendemain : l'echeance de 18:00 (et 12:00 du lendemain pas encore) est rattrapee UNE fois.
    let e = evaluate(&s, T0 + hm(13, 0, 0), T0 + DAY + hm(9, 0, 0), &UTC).unwrap();
    let d = e.due.unwrap();
    assert_eq!(d.label, "heure fixe 18:00:00");
    assert!(d.catch_up);
    // Ancre mise a jour : plus rien tant que la prochaine echeance n'est pas la.
    assert!(evaluate(&s, T0 + DAY + hm(9, 0, 0), T0 + DAY + hm(9, 0, 5), &UTC).unwrap().due.is_none());
    // Plusieurs jours d'absence : toujours une seule execution.
    let e = evaluate(&s, T0, T0 + 10 * DAY, &UTC).unwrap();
    assert!(e.due.unwrap().catch_up);
}

#[test]
fn intervalle_chronometre_rattrapage_et_horloge_qui_recule() {
    let s = Schedule::interval(900);
    let e = evaluate(&s, 1000, 1899, &UTC).unwrap();
    assert_eq!((e.due.is_none(), e.next_utc), (true, 1900));
    let e = evaluate(&s, 1000, 1900, &UTC).unwrap();
    let d = e.due.unwrap();
    assert_eq!(d.label, "intervalle 00:15:00");
    assert!(!d.catch_up);
    assert_eq!(e.next_utc, 1900 + 900);
    // Longue absence : rattrapage signale, une seule execution.
    assert!(evaluate(&s, 1000, 1000 + 5 * DAY, &UTC).unwrap().due.unwrap().catch_up);
    // Horloge ramenee en arriere : aucune execution fantome.
    assert!(evaluate(&s, 50_000, 1000, &UTC).unwrap().due.is_none());
    assert_eq!(next_run_utc(&s, 1000, 1500, &UTC), Some(1900));
    assert_eq!(next_run_utc(&s, 1000, 5000, &UTC), Some(5000));
}

/// Fuseau a heure d'ete simule : +1 h entre `start` et `end` (instants UTC en secondes).
struct Dst {
    base: i32,
    start: i64,
    end: i64,
}
impl TzProvider for Dst {
    fn utc_offset_secs(&self, utc_ms: u64) -> i32 {
        let s = (utc_ms / 1000) as i64;
        if s >= self.start && s < self.end { self.base + 3600 } else { self.base }
    }
}

#[test]
fn changement_d_heure_ni_doublon_ni_saut_absurde() {
    // Heure d'ete du jour 3 (01:00 UTC) au jour 6 : saut avant (heure locale inexistante) puis retour arriere (heure doublee).
    let tz = Dst { base: 0, start: T0 + 3 * DAY + hm(1, 0, 0), end: T0 + 6 * DAY + hm(1, 0, 0) };
    for t in [hm(1, 30, 0), hm(2, 30, 0), hm(12, 0, 0)] {
        let slots = fixed_slots(&[t as u32], T0, T0 + 9 * DAY, &tz);
        assert!((8..=9).contains(&slots.len()), "t={} -> {} slots", t, slots.len());
        for w in slots.windows(2) {
            let gap = w[1] - w[0];
            assert!((23 * 3600..=25 * 3600).contains(&gap), "t={} ecart absurde {} s", t, gap);
        }
    }
    // Simulation seconde par minute : exactement une execution par jour, jamais deux a moins de 20 h.
    let s = Schedule::fixed(vec![hu(1, 30, 0)]);
    let (mut anchor, mut runs) = (T0, Vec::new());
    let mut now = T0;
    while now < T0 + 9 * DAY {
        now += 60;
        if evaluate(&s, anchor, now, &tz).unwrap().due.is_some() {
            runs.push(now);
            anchor = now;
        }
    }
    assert!((8..=9).contains(&runs.len()), "{:?}", runs);
    for w in runs.windows(2) {
        assert!(w[1] - w[0] >= 20 * 3600, "doublon: {:?}", w);
    }
}

#[test]
fn validation_formulaire_limites_et_cli() {
    assert!(Schedule::interval(10).is_valid());
    assert!(Schedule::interval(86_400).is_valid());
    assert_eq!(Schedule::interval(9).validate(), Err(SchedError::BadInterval));
    assert_eq!(Schedule::interval(86_401).validate(), Err(SchedError::BadInterval));
    assert!(Schedule::interval(59).is_short() && !Schedule::interval(60).is_short());
    assert_eq!(Schedule::fixed(vec![]).validate(), Err(SchedError::NoTime));
    assert_eq!(Schedule::fixed(vec![86_400]).validate(), Err(SchedError::BadTime));
    assert_eq!(Schedule::fixed(vec![hu(12, 0, 0), hu(12, 0, 0), 5]).normalized().times_secs, vec![5, hu(12, 0, 0)]);
    // Sans exe : une planification valide est obligatoire ; avec exe : facultative.
    assert!(!form_ok("", None));
    assert!(!form_ok("  ", Some(&Schedule::interval(5))));
    assert!(form_ok("", Some(&Schedule::interval(15))));
    assert!(form_ok("game.exe", None));
    // HH:MM:SS
    assert_eq!(hms(hm(12, 5, 9) as u32), "12:05:09");
    assert_eq!(parse_hms("00:15:00"), Some(900));
    assert_eq!(parse_hms("18:30"), Some(hm(18, 30, 0) as u32));
    assert_eq!(parse_hms("12:60:00"), None);
    assert_eq!(parse_hms("abc"), None);
    // CLI
    assert_eq!(from_cli(Some("00:15:00"), &[]).unwrap().unwrap(), Schedule::interval(900));
    let at = ["18:00:00".to_string(), "12:00:00".to_string()];
    assert_eq!(from_cli(None, &at).unwrap().unwrap().times_secs, vec![hm(12, 0, 0) as u32, hm(18, 0, 0) as u32]);
    assert!(from_cli(Some("00:00:05"), &[]).is_err());
    assert!(from_cli(Some("00:15:00"), &at).is_err());
    assert!(from_cli(None, &["25:00:00".to_string()]).is_err());
    assert_eq!(from_cli(None, &[]).unwrap(), None);
}

#[test]
fn config_json_retrocompatible_et_status_additif() {
    // Ancien config.json sans `schedule`.
    let g: Game = serde_json::from_str(r#"{"name":"G","path":"C:/s"}"#).unwrap();
    assert!(g.schedule.is_none());
    assert!(!serde_json::to_string(&g).unwrap().contains("schedule"));
    let mut g2 = g.clone();
    g2.schedule = Some(Schedule { last_run_utc: Some(123), ..Schedule::fixed(vec![43_200, 64_800]) });
    let j = serde_json::to_string(&g2).unwrap();
    assert!(j.contains("\"mode\":\"fixed\"") && j.contains("times_secs") && j.contains("last_run_utc"), "{}", j);
    assert_eq!(serde_json::from_str::<Game>(&j).unwrap(), g2);
    let iv = serde_json::to_string(&Schedule::interval(900)).unwrap();
    assert!(iv.contains("\"mode\":\"interval\"") && iv.contains("900"), "{}", iv);
    // status.json : l'entree `schedule` est optionnelle (anciens fichiers lisibles).
    let e = agss::statusfile::ScheduleEntry { mode: "interval".into(), next_run_utc: Some("2026-01-01T00:00:00Z".into()) };
    assert!(serde_json::to_string(&e).unwrap().contains("next_run_utc"));
}

// ------------------------------------------------------------------ fichiers verrouilles + moteur

#[test]
fn fichier_verrouille_detecte_et_decision() {
    let t = TempDir::new().unwrap();
    let f = t.path().join("save.dat");
    std::fs::write(&f, "x").unwrap();
    let g = Game { name: "G".into(), path: t.path().to_path_buf(), ..Default::default() };
    assert!(!file_locked(&f) && !any_locked(&g));
    {
        let _h = hold_lock(&f);
        assert!(file_locked(&f) && any_locked(&g));
        // Un fichier absent n'est pas « verrouille ».
        assert!(!file_locked(&t.path().join("absent.dat")));
    }
    assert!(!any_locked(&g));
    assert_eq!(guard(true, true), Guard::PostponeRestore);
    assert_eq!(guard(true, false), Guard::Run);
    assert_eq!(guard(false, true), Guard::Run, "la sauvegarde est toujours faite");
}

struct Pc {
    ctx: Ctx,
    game: Game,
    log: Logger,
}

fn pc(root: &Path, name: &str, sched: Option<Schedule>) -> Pc {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.into();
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "G".into(), path: root.join(format!("saves-{}", name)), exe: None, schedule: sched, ..Default::default() };
    let log = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
    Pc { ctx, game, log }
}

fn write(p: &Path, c: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}

const CFG: WatchCfg = WatchCfg { stable_ms: 10_000, resync_ms: 60_000, max_tries: 3, safety_ms: 2000, retry_ms: 1000, retry_max_ms: 30_000 };

#[test]
fn moteur_synchro_planifiee_sans_exe_journal_et_une_seule_fois() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    std::fs::create_dir_all(&dest).unwrap(); // 2.0 : la surveillance ne cree jamais le coffre lui-meme
    let a = pc(t.path(), "A", Some(Schedule::interval(10)));
    write(&a.game.path.join("save.dat"), "v1");
    let mut w = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), CFG, &a.log);
    w.set_schedule_anchor(0, 1000);
    assert!(w.tick_schedule(1005, &UTC).is_empty(), "pas encore l'echeance");
    assert_eq!(w.tick_schedule(1010, &UTC), vec!["G".to_string()]);
    assert!(w.tick_schedule(1011, &UTC).is_empty(), "une seule fois par echeance");
    assert_eq!(backup::list_versions(&dest, &a.game).len(), 1);
    let log = std::fs::read_to_string(a.ctx.data_dir.join("logs").join("watch.log")).unwrap();
    assert!(log.contains("[G] synchro planifiee (intervalle 00:00:10)"), "{}", log);
    assert!(log.contains("prochaine synchro planifiee"), "{}", log);
    // Prochaine echeance 10 s plus tard.
    assert_eq!(w.tick_schedule(1020, &UTC), vec!["G".to_string()]);
}

#[test]
fn restauration_reportee_si_verrouille_puis_faite_sinon() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = pc(t.path(), "A", Some(Schedule::interval(10)));
    let b = pc(t.path(), "B", None);
    write(&a.game.path.join("save.dat"), "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    write(&b.game.path.join("save.dat"), "v2-B");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Restore);
    let mut w = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), CFG, &a.log);
    w.set_schedule_anchor(0, 1000);
    {
        let _lock = hold_lock(&a.game.path.join("save.dat"));
        assert!(w.tick_schedule(1010, &UTC).is_empty(), "restauration reportee");
    }
    assert_eq!(std::fs::read_to_string(a.game.path.join("save.dat")).unwrap(), "v1", "rien ecrase");
    let log = std::fs::read_to_string(a.ctx.data_dir.join("logs").join("watch.log")).unwrap();
    assert!(log.contains("restauration reportee : fichiers en cours d'utilisation"), "{}", log);
    // Echeance suivante, fichier libere : restauration (snapshot de securite compris).
    assert_eq!(w.tick_schedule(1020, &UTC), vec!["G".to_string()]);
    assert_eq!(std::fs::read_to_string(a.game.path.join("save.dat")).unwrap(), "v2-B");
    // Local modifie + fichier verrouille en ecriture : la SAUVEGARDE est faite quand meme.
    write(&a.game.path.join("save.dat"), "v3-A");
    {
        let _lock = hold_lock(&a.game.path.join("save.dat"));
        assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Backup);
        assert_eq!(w.tick_schedule(1030, &UTC), vec!["G".to_string()]);
    }
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::InSync);
}

// ------------------------------------------------------------------ reconnaissance : exe generiques

fn p(exe: &str, path: &str) -> ProcInfo {
    ProcInfo { pid: 1, exe: exe.to_lowercase(), path: Some(PathBuf::from(path)), start: 1 }
}

fn db(exe: &str) -> Vec<DbHit> {
    match exe {
        "eldenring.exe" => vec![DbHit { name: "Elden Ring".into(), install_dirs: vec!["ELDEN RING".into()] }],
        "app2.exe" => (0..4).map(|i| DbHit { name: format!("Jeu {}", i), install_dirs: vec![format!("dir{}", i)] }).collect(),
        // Tout nom generique « existe » dans la base (mauvaise donnee) : il ne doit jamais suffire.
        e if GENERIC_EXES.contains(&e) || EXCLUDED_EXES.contains(&e) => vec![DbHit { name: "NODE: The Last Favor of the Antarii".into(), install_dirs: vec![] }],
        _ => vec![],
    }
}

#[cfg(windows)]
#[test]
fn exe_generiques_jamais_reconnus_par_leur_seul_nom_detection() {
    let roots: Vec<String> = vec![];
    let known: Vec<String> = vec![];
    let e = Env { db: &db, launcher_roots: &roots, known_exes: &known };
    for x in ["node.exe", "python.exe", "pythonw.exe", "java.exe", "javaw.exe", "dotnet.exe", "electron.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "conhost.exe", "code.exe", "chrome.exe", "msedge.exe", "firefox.exe", "explorer.exe", "steam.exe", "Node.EXE"] {
        assert!(is_generic_name(x), "{}", x);
        assert!(classify(&p(x, &format!(r"C:\Program Files\Outil\{}", x)), &e).is_none(), "{} ne doit pas etre propose", x);
    }
    // Un vrai jeu distinctif est accepte.
    let c = classify(&p("EldenRing.exe", r"D:\Steam\steamapps\common\ELDEN RING\Game\eldenring.exe"), &e).unwrap();
    assert_eq!(c.name, "Elden Ring");
    assert!(!is_generic_name("eldenring.exe"));
    // Nom trop ambigu dans la base : refuse sans correspondance du dossier, accepte avec.
    assert!(classify(&p("app2.exe", r"C:\Truc\Inconnu\app2.exe"), &e).is_none());
    assert_eq!(classify(&p("app2.exe", r"C:\Truc\dir2\app2.exe"), &e).unwrap().name, "Jeu 2");
}

#[test]
fn exe_generiques_jamais_reconnus_par_leur_seul_nom_ajout_manuel() {
    let node = DbGame { name: "NODE: The Last Favor of the Antarii".into(), exes: vec!["node.exe".into(), "python.exe".into()], paths: vec!["<winLocalAppData>/Node".into()], ..Default::default() };
    let distinct = DbGame { name: "Vrai Jeu".into(), exes: vec!["vraijeu.exe".into()], paths: vec!["<winLocalAppData>/VraiJeu".into()], ..Default::default() };
    let gdb = GameDb::from_db(Db { format: 1, generated: String::new(), games: vec![node, distinct] }, vec![]);
    let ex = |_: &Path| false;
    for x in ["node.exe", "NODE.EXE", "python.exe", r"C:\Program Files\nodejs\node.exe"] {
        let full = PathBuf::from(x);
        let r = agss::autofill::lookup(&gdb, x, if full.is_absolute() { Some(full.as_path()) } else { None }, &[], &[], &ex);
        assert!(r.is_empty(), "{} -> {:?}", x, r);
    }
    let r = agss::autofill::lookup(&gdb, "vraijeu.exe", None, &[], &[], &ex);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].game_name, "Vrai Jeu");
    // Base reelle embarquee : node.exe / java.exe / cmd.exe ne proposent rien.
    let t = TempDir::new().unwrap();
    let real = agss::gamedb::embedded(t.path());
    for x in ["node.exe", "python.exe", "java.exe", "javaw.exe", "dotnet.exe", "electron.exe", "cmd.exe", "chrome.exe"] {
        assert!(agss::autofill::lookup(&real, x, None, &[], &[], &ex).is_empty(), "{}", x);
    }
}

// ------------------------------------------------------------------ libelles et contraste

#[test]
fn libelles_ajout_manuel_et_carte_profils_integres_dans_les_5_langues() {
    let want: [(Lang, &str, &str, &str, &str); 5] = [
        (Lang::Fr, "Exe du jeu/émulateur/programme", "Nom du dossier partagé", "Dossier ou fichier à sauvegarder", "Profils intégrés"),
        (Lang::En, "Game/emulator/program exe", "Shared folder name", "Folder or file to back up", "Built-in profiles"),
        (Lang::Ru, "Exe игры/эмулятора/программы", "Название общей папки", "Папка или файл для сохранения", "Встроенные профили"),
        (Lang::Es, "Exe del juego/emulador/programa", "Nombre de la carpeta compartida", "Carpeta o archivo a guardar", "Perfiles integrados"),
        (Lang::Zh, "游戏/模拟器/程序的 exe", "共享文件夹名称", "要备份的文件夹或文件", "内置配置文件"),
    ];
    for (l, exe, name, path, prof) in want {
        // Libelle de base (Windows) ; sous Linux, la variante dit « programme » (tests/v32.rs).
        assert_eq!(agss::i18n::tr_raw(l, K::AddExe), exe);
        assert_eq!(tr(l, K::AddName), name);
        assert_eq!(tr(l, K::AddSavePath), path);
        assert_eq!(tr(l, K::ProfTitle), prof);
        for k in [K::SchedTitle, K::SchedToggle, K::SchedFixed, K::SchedInterval, K::SchedAddTime, K::SchedRemove, K::SchedNeed, K::SchedBadInterval, K::SchedShort, K::SchedNoTime, K::SchedNext, K::SchedHelp, K::TipSched] {
            assert!(!tr(l, k).trim().is_empty());
        }
        assert!(tr(l, K::SchedNext).contains("{}"));
    }
}

#[test]
fn contraste_des_libelles_au_moins_4_5_dans_tous_les_themes() {
    use agss::theme::{contrast_ratio, palette};
    for dark in [true, false] {
        for accent in [[0x4c, 0x8d, 0xf6], [220, 40, 40], [40, 180, 90], [250, 200, 0], [150, 60, 200]] {
            let p = palette(dark, accent);
            for bg in [p.panel_bg, p.widget_bg] {
                let r = contrast_ratio(p.text, bg);
                assert!(r >= 4.5, "dark={} accent={:?} ratio {:.2}", dark, accent, r);
            }
        }
    }
}
