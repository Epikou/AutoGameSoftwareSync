//! Lot 1.0.1 : modes de la manette (desactivee / me demander / toujours), detection passive, liberation pendant
//! un jeu ou en arriere-plan, migration des reglages, journal (copie, hauteur), deplacement d'un jeu.
use agss::config::{Config, Game};
use agss::gamepad::*;
use agss::padctl::{self, Phase, PromptCtx, Prompter};
use agss::settings::{PadMode, Settings, LOG_HEIGHT_MAX, LOG_HEIGHT_MIN};
use agss::Ctx;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Source simulee qui COMPTE ce qui lui est demande : lectures (`poll`), presences, passages en mode Open.
struct Counting {
    polls: Arc<AtomicUsize>,
    presences: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
    access: Access,
}

fn info() -> PadInfo {
    PadInfo { id: "sim:1".into(), name: "DualSense".into(), conn: Connection::Usb }
}

impl PadSource for Counting {
    fn set_access(&mut self, a: Access) {
        if a == Access::Open && self.access != Access::Open {
            self.opens.fetch_add(1, Ordering::SeqCst);
        }
        self.access = a;
    }
    fn presence(&mut self, _now: u64) -> Vec<PadInfo> {
        self.presences.fetch_add(1, Ordering::SeqCst);
        vec![info()]
    }
    fn poll(&mut self, _now: u64) -> Vec<PadPoll> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        vec![PadPoll { info: info(), readable: true, states: vec![PadState::default()] }]
    }
}

struct Rig {
    svc: GamepadService,
    polls: Arc<AtomicUsize>,
    presences: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
    game: Arc<AtomicBool>,
    fg: Arc<AtomicBool>,
    logs: Arc<Mutex<Vec<String>>>,
}

fn rig(mode: PadMode) -> Rig {
    let (polls, presences, opens) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (p2, pr2, o2) = (polls.clone(), presences.clone(), opens.clone());
    let game = Arc::new(AtomicBool::new(false));
    let fg = Arc::new(AtomicBool::new(true));
    let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (l2, fg2) = (logs.clone(), fg.clone());
    let shared = PadShared::new(mode, game.clone());
    let svc = spawn_ctl(
        shared,
        move |_| vec![Box::new(Counting { polls: p2, presences: pr2, opens: o2, access: Access::Open }) as Box<dyn PadSource>],
        || {},
        move || fg2.load(Ordering::SeqCst),
        move |m| l2.lock().unwrap().push(m.to_string()),
    );
    Rig { svc, polls, presences, opens, game, fg, logs }
}

fn wait(secs: u64, f: impl Fn() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn has_log(r: &Rig, s: &str) -> bool {
    r.logs.lock().unwrap().iter().any(|m| m.contains(s))
}

#[test]
fn desactivee_n_ouvre_ni_ne_liste_ni_ne_lit_jamais() {
    let r = rig(PadMode::Disabled);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(r.polls.load(Ordering::SeqCst), 0, "aucune lecture");
    assert_eq!(r.presences.load(Ordering::SeqCst), 0, "aucune enumeration, meme passive");
    assert_eq!(r.opens.load(Ordering::SeqCst), 0, "aucune ouverture");
    assert!(r.svc.status().is_empty() && r.svc.presence().is_empty());
    assert_eq!(r.svc.phase(), Phase::Disabled);
}

#[test]
fn me_demander_detecte_passivement_sans_ouvrir_ni_lire_avant_accord() {
    let r = rig(PadMode::Ask);
    assert!(wait(5, || !r.svc.presence().is_empty()), "presence detectee passivement");
    std::thread::sleep(Duration::from_millis(800));
    assert!(r.presences.load(Ordering::SeqCst) > 0);
    assert_eq!(r.polls.load(Ordering::SeqCst), 0, "aucune lecture de touches avant acceptation");
    assert_eq!(r.opens.load(Ordering::SeqCst), 0, "aucune ouverture avant acceptation");
    assert!(r.svc.status().is_empty(), "pas de mode manette tant que non accepte");
    // Reponse « Oui » : la manette s'ouvre et est lue.
    r.svc.shared.set_session_yes(true);
    assert!(wait(5, || !r.svc.status().is_empty()), "manette ouverte apres Oui");
    assert_eq!(r.opens.load(Ordering::SeqCst), 1);
    assert!(r.polls.load(Ordering::SeqCst) > 0);
}

#[test]
fn toujours_activee_libere_pendant_un_jeu_puis_reprend() {
    let r = rig(PadMode::Always);
    assert!(wait(5, || !r.svc.status().is_empty()));
    r.game.store(true, Ordering::SeqCst);
    assert!(wait(5, || r.svc.status().is_empty()), "manette liberee quand un jeu tourne");
    std::thread::sleep(Duration::from_millis(200));
    let frozen = r.polls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(r.polls.load(Ordering::SeqCst), frozen, "plus aucune lecture pendant le jeu");
    assert!(has_log(&r, "[Manette] liberee (en cours d'utilisation)"));
    assert!(matches!(r.svc.phase(), Phase::Released(padctl::Why::GameRunning)));
    assert_eq!(r.svc.presence().first().map(|p| p.name.as_str()), Some("DualSense"), "nom conserve pour l'infobulle");
    r.game.store(false, Ordering::SeqCst);
    assert!(wait(5, || !r.svc.status().is_empty()), "reprise a la fin du jeu");
    assert!(has_log(&r, "[Manette] reprise"));
    assert_eq!(r.opens.load(Ordering::SeqCst), 2);
}

#[test]
fn arriere_plan_libere_la_manette_et_retour_au_premier_plan_reprend() {
    let r = rig(PadMode::Always);
    assert!(wait(5, || !r.svc.status().is_empty()));
    r.fg.store(false, Ordering::SeqCst);
    assert!(wait(5, || r.svc.status().is_empty()));
    std::thread::sleep(Duration::from_millis(200));
    let frozen = r.polls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(r.polls.load(Ordering::SeqCst), frozen, "aucune lecture en arriere-plan");
    assert!(has_log(&r, "arriere-plan"));
    r.fg.store(true, Ordering::SeqCst);
    assert!(wait(5, || !r.svc.status().is_empty()));
    assert!(has_log(&r, "[Manette] reprise"));
}

#[test]
fn hub_les_acces_off_et_passif_n_appellent_pas_poll() {
    let (polls, presences, opens) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut hub = Hub::new(vec![Box::new(Counting { polls: polls.clone(), presences: presences.clone(), opens: opens.clone(), access: Access::Open })]);
    hub.set_access(Access::Off);
    for t in 0..5 {
        let o = hub.tick(t * 100);
        assert!(o.presence.is_none() && o.navs.is_empty() && o.changes.is_empty());
    }
    assert_eq!((polls.load(Ordering::SeqCst), presences.load(Ordering::SeqCst)), (0, 0));
    hub.set_access(Access::Passive);
    let o = hub.tick(600);
    assert_eq!(o.presence.unwrap().len(), 1);
    assert_eq!((polls.load(Ordering::SeqCst), presences.load(Ordering::SeqCst)), (0, 1));
    hub.set_access(Access::Open);
    hub.tick(700);
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    assert_eq!(hub.status().len(), 1);
    hub.set_access(Access::Off);
    assert!(hub.status().is_empty(), "quitter Open vide l'etat (poignees fermees)");
}

#[test]
fn identifiants_depuis_les_noms_de_peripheriques() {
    assert_eq!(parse_vid_pid(r"\\?\HID#VID_054C&PID_0CE6&MI_03#8&1a2b3c4d&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"), Some((0x054c, 0x0ce6)));
    assert_eq!(parse_vid_pid(r"\\?\HID#{00001124-0000-1000-8000-00805f9b34fb}_VID&0002054c_PID&0ce6#9&abc&0&0000#{4d1e55b2}"), Some((0x054c, 0x0ce6)));
    assert_eq!(parse_vid_pid(r"\\?\HID#ACPI0000#1"), None);
}

#[test]
fn reglage_migration_et_defauts() {
    let t = TempDir::new().unwrap();
    // Nouvelle installation : « Me demander ».
    assert_eq!(Settings::default().pad_mode, PadMode::Ask);
    assert_eq!(Settings::load_from(t.path()).unwrap().pad_mode, PadMode::Ask);
    // Reglages existants sans le champ (la manette etait toujours active) : « Toujours activee », rien ne change.
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr","big_mode":false}"#).unwrap();
    let s = Settings::load_from(t.path()).unwrap();
    assert_eq!(s.pad_mode, PadMode::Always);
    assert!((s.log_height - 320.0).abs() < 0.1);
    // Le choix persiste.
    let mut s2 = Settings::default();
    s2.pad_mode = PadMode::Disabled;
    s2.log_height = 480.0;
    std::fs::write(t.path().join("settings.json"), serde_json::to_string(&s2).unwrap()).unwrap();
    let r = Settings::load_from(t.path()).unwrap();
    assert_eq!((r.pad_mode, r.log_height), (PadMode::Disabled, 480.0));
    // Hauteur du journal bornee.
    s2.log_height = 99999.0;
    std::fs::write(t.path().join("settings.json"), serde_json::to_string(&s2).unwrap()).unwrap();
    assert_eq!(Settings::load_from(t.path()).unwrap().log_height, LOG_HEIGHT_MAX);
    s2.log_height = 1.0;
    std::fs::write(t.path().join("settings.json"), serde_json::to_string(&s2).unwrap()).unwrap();
    assert_eq!(Settings::load_from(t.path()).unwrap().log_height, LOG_HEIGHT_MIN);
}

#[test]
fn question_report_et_memorisation_de_la_case() {
    let mut p = Prompter::default();
    let pad = [info()];
    let blocked = PromptCtx { tutorial: true, ..Default::default() };
    assert!(p.on_presence(&pad, &blocked).is_none(), "differee pendant le tutoriel");
    assert!(p.on_presence(&pad, &PromptCtx { hidden: true, ..Default::default() }).is_none(), "differee fenetre cachee");
    assert!(p.on_presence(&pad, &PromptCtx { game_running: true, ..Default::default() }).is_none(), "differee pendant un jeu");
    assert!(p.on_presence(&pad, &PromptCtx::default()).is_some());
    assert!(p.on_presence(&pad, &PromptCtx::default()).is_none(), "une fois par connexion");
    assert_eq!(padctl::apply_answer(true, true), (PadMode::Always, true));
    assert_eq!(padctl::apply_answer(false, true), (PadMode::Disabled, false));
    assert_eq!(padctl::apply_answer(false, false), (PadMode::Ask, false));
}

#[test]
fn journal_copie_respecte_le_filtre_actif() {
    use agss::vm::{log_copy_text, parse_log_line, LogFilter};
    let lines = ["20261025-010101-000 [Jeu A] sauvegarde effectuee", "20261025-010102-000 [Jeu B] ERREUR boum", "20261025-010103-000 [Manette] reprise"];
    let entries: Vec<_> = lines.iter().filter_map(|l| parse_log_line(l)).collect();
    let tz = agss::timefmt::FixedTz(0);
    let all = log_copy_text(&entries, &LogFilter::default(), agss::i18n::Lang::Fr, &tz);
    assert_eq!(all.lines().count(), 3);
    let f = LogFilter { game: Some("Jeu B".into()), ..Default::default() };
    let one = log_copy_text(&entries, &f, agss::i18n::Lang::Fr, &tz);
    assert_eq!(one.lines().count(), 1);
    assert!(one.contains("[Jeu B] ERREUR boum"));
}

#[test]
fn deplacer_un_jeu_met_a_jour_la_config_sans_rien_deplacer() {
    let t = TempDir::new().unwrap();
    let ctx = Ctx::new(t.path().join("data"));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let old = t.path().join("old");
    let new = t.path().join("new");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(old.join("a.sav"), "1").unwrap();
    let exe = t.path().join("Jeu2").join("jeu2.exe");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "x").unwrap();
    let cfg = Config { games: vec![Game { name: "Jeu".into(), path: old.clone(), exe: Some("jeu.exe".into()), dest_key: None, ..Default::default() }], dest: None };
    cfg.save(&ctx).unwrap();
    // Chemins inexistants : refus, rien ne change.
    assert!(agss::actions::relocate_game(&ctx, "Jeu", None, Some(&agss::actions::SaveTarget { path: t.path().join("absent"), is_file: false, filter: None })).is_err());
    assert!(agss::actions::relocate_game(&ctx, "Jeu", Some(&t.path().join("absent.exe")), None).is_err());
    assert_eq!(Config::load(&ctx).unwrap().games[0].path, old);
    // Deplacement valide : config mise a jour, aucun fichier deplace ni supprime.
    agss::actions::relocate_game(&ctx, "Jeu", Some(&exe), Some(&agss::actions::SaveTarget { path: new.clone(), is_file: false, filter: None })).unwrap();
    let g = &Config::load(&ctx).unwrap().games[0];
    assert_eq!((g.path.clone(), g.exe.clone(), g.dest_key.clone()), (new.clone(), Some("jeu2.exe".to_string()), None));
    assert_eq!(Settings::load(&ctx).unwrap().per_game.get("Jeu").and_then(|p| p.exe_path.clone()), Some(exe));
    assert!(old.join("a.sav").is_file(), "l'ancien dossier n'est pas touche");
}
