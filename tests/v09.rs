//! v0.9 : demarrage masque, option « Demarrer reduit », cache d'empreintes, durees au Journal, progression.
//! UNIQUEMENT des dossiers temporaires.
use agss::backup;
use agss::config::Game;
use agss::hashcache::{entry_valid, RACY_MS};
use agss::perf::{self, Phase, Progress};
use agss::process::ProcessChecker;
use agss::settings::StartupRuleCfg;
use agss::startup::*;
use agss::uilogic::{hidden_start_step, HideStep, HIDE_LAST_FRAME};
use agss::{store, util, Ctx, Res};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(false)
    }
}

fn machine(root: &Path, name: &str) -> (Ctx, Game) {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "TestGame".into(), path: root.join(format!("saves-{}", name)), exe: Some("testgame.exe".into()), dest_key: None, ..Default::default() };
    (ctx, game)
}

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}
fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}
fn pb(s: &str) -> PathBuf {
    PathBuf::from(s)
}
fn set_mtime(p: &Path, t: SystemTime) {
    std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
}
fn hour_ago() -> SystemTime {
    SystemTime::now() - Duration::from_secs(3600)
}

// ------------------------------------------------------------------ demarrage masque

#[test]
fn demarrage_masque_decision_pure() {
    // Pas de --tray : on ne touche a rien.
    for f in 0..10 {
        assert_eq!(hidden_start_step(false, f), HideStep::None);
    }
    // --tray : preparation a l'image 0, masquage aux images suivantes (eframe force l'affichage apres l'image 0), puis rien.
    assert_eq!(hidden_start_step(true, 0), HideStep::Prepare);
    for f in 1..=HIDE_LAST_FRAME {
        assert_eq!(hidden_start_step(true, f), HideStep::Finish);
    }
    assert_eq!(hidden_start_step(true, HIDE_LAST_FRAME + 1), HideStep::None);
    assert_eq!(hidden_start_step(true, u32::MAX), HideStep::None);
}

// ------------------------------------------------------------------ « Demarrer reduit »

fn gdrive() -> Tool {
    Tool { id: "gdrive".into(), kind: ToolKind::GoogleDrive, name: "Google Drive".into(), exe: pb("C:\\gd.exe"), args: String::new() }
}

#[test]
fn demarrer_reduit_dans_les_regles_et_le_xml() {
    let self_exe = pb("C:\\Apps\\AutoGameSoftwareSync.exe");
    // Defaut : reduit. Les arguments des anciens parametres sont inchanges.
    let r = build_rules(&[gdrive()], &[], &self_exe, &|_| true);
    assert_eq!(r[0].args, "--tray", "la tache AutoGameSoftwareSync garde --tray");
    assert!(!r[0].minimized);
    assert!(r[1].minimized);
    assert_eq!(r[1].args, "--launch-tool gdrive");
    // Option decochee : le lanceur demande un lancement normal.
    let cfg = vec![StartupRuleCfg { id: "gdrive".into(), name: String::new(), exe: None, delay_secs: 20, enabled: true, minimized: false }];
    let r = build_rules(&[gdrive()], &cfg, &self_exe, &|_| true);
    assert_eq!(r[1].args, "--launch-tool gdrive --normal");
    assert!(task_xml(&r[1], "PC\\moi").contains("--launch-tool gdrive --normal"));
    // Programme personnalise : reduit = lance via AutoGameSoftwareSync (CreateProcess + SW_SHOWMINNOACTIVE) ; sinon directement.
    let exe = pb("D:\\x\\outil.exe");
    let mk = |mn: bool| vec![StartupRuleCfg { id: "custom-1".into(), name: "Outil".into(), exe: Some(exe.clone()), delay_secs: 5, enabled: true, minimized: mn }];
    let on = build_rules(&[], &mk(true), &self_exe, &|_| true);
    assert_eq!((on[1].command.clone(), on[1].args.as_str()), (self_exe.clone(), "--launch-program \"D:\\x\\outil.exe\""));
    let off = build_rules(&[], &mk(false), &self_exe, &|_| true);
    assert_eq!((off[1].command.clone(), off[1].args.as_str()), (exe.clone(), ""));
    // Le plan d'application recree la tache avec la nouvelle commande (idempotent : meme resultat deux fois).
    let p1 = plan_apply(&["AutoGameSoftwareSync\\custom-1".to_string()], &on);
    let p2 = plan_apply(&["AutoGameSoftwareSync\\custom-1".to_string()], &on);
    assert_eq!(p1, p2);
    assert!(p1.create.iter().any(|r| r.id == "custom-1" && r.minimized));
}

#[test]
fn ancienne_config_sans_champ_minimized_demarre_reduit() {
    let c: StartupRuleCfg = serde_json::from_str(r#"{"id":"gdrive","delay_secs":20,"enabled":true}"#).unwrap();
    assert!(c.minimized, "ON par defaut");
    let c2: StartupRuleCfg = serde_json::from_str(r#"{"id":"gdrive","delay_secs":20,"enabled":true,"minimized":false}"#).unwrap();
    assert!(!c2.minimized);
}

#[test]
fn ligne_de_commande_de_lancement() {
    assert_eq!(agss::winutil::build_cmdline(Path::new("C:\\a b\\x.exe"), ""), "\"C:\\a b\\x.exe\"");
    assert_eq!(agss::winutil::build_cmdline(Path::new("C:\\a b\\x.exe"), "/background"), "\"C:\\a b\\x.exe\" /background");
}

// ------------------------------------------------------------------ cache d'empreintes

#[test]
fn cache_regle_de_validite() {
    let t0 = 1_000_000u64;
    assert!(entry_valid(10, t0, 10, t0, t0 + RACY_MS), "inchange et stable au moment du calcul");
    assert!(!entry_valid(11, t0, 10, t0, t0 + RACY_MS), "taille differente");
    assert!(!entry_valid(10, t0 + 1, 10, t0, t0 + RACY_MS), "date differente");
    assert!(!entry_valid(10, t0, 10, t0, t0 + RACY_MS - 1), "fichier trop recent au moment du calcul : jamais fiable");
}

#[test]
fn cache_ne_masque_jamais_une_modification() {
    let t = TempDir::new().unwrap();
    let (ctx, _) = machine(t.path(), "A");
    let dir = t.path().join("saves");
    write(&dir, "a.dat", "AAAA");
    let a = dir.join("a.dat");
    set_mtime(&a, hour_ago());
    let (f1, _) = store::scan_cached(&ctx, &dir).unwrap();
    assert_eq!(f1[0].hash, util::hash_bytes(b"AAAA"));
    assert_eq!(ctx.hash_cache.len(), 1);
    // Fichier inchange : meme resultat (venant du cache) et identique a un vrai calcul.
    let (f1b, _) = store::scan_cached(&ctx, &dir).unwrap();
    assert_eq!(f1b, f1);
    assert_eq!(store::scan(&dir).unwrap().0, f1);
    // Contenu modifie, MEME taille, date differente : nouveau hash.
    std::fs::write(&a, "BBBB").unwrap();
    set_mtime(&a, hour_ago() + Duration::from_secs(5));
    let (f2, _) = store::scan_cached(&ctx, &dir).unwrap();
    assert_eq!(f2[0].hash, util::hash_bytes(b"BBBB"));
    // Taille differente : nouveau hash.
    std::fs::write(&a, "CCCCC").unwrap();
    set_mtime(&a, hour_ago() + Duration::from_secs(5));
    let (f3, _) = store::scan_cached(&ctx, &dir).unwrap();
    assert_eq!(f3[0].hash, util::hash_bytes(b"CCCCC"));
}

#[test]
fn cache_ignore_un_fichier_modifie_juste_apres_le_calcul() {
    let t = TempDir::new().unwrap();
    let (ctx, _) = machine(t.path(), "A");
    let dir = t.path().join("saves");
    write(&dir, "a.dat", "AAAA");
    let a = dir.join("a.dat");
    let md = std::fs::metadata(&a).unwrap().modified().unwrap();
    store::scan_cached(&ctx, &dir).unwrap(); // fichier tout neuf : memorise mais jamais cru
    std::fs::write(&a, "BBBB").unwrap();
    set_mtime(&a, md); // meme date, meme taille, contenu different
    let (f, _) = store::scan_cached(&ctx, &dir).unwrap();
    assert_eq!(f[0].hash, util::hash_bytes(b"BBBB"), "course d'horloge : on recalcule");
}

#[test]
fn cache_persiste_et_un_fichier_illisible_est_ignore() {
    let t = TempDir::new().unwrap();
    let (ctx, _) = machine(t.path(), "A");
    let dir = t.path().join("saves");
    write(&dir, "a.dat", "AAAA");
    let a = dir.join("a.dat");
    set_mtime(&a, hour_ago());
    let (f, _) = store::scan_cached(&ctx, &dir).unwrap();
    let mtime = util::mtime_ms(std::fs::metadata(&a).unwrap().modified().unwrap());
    // Un autre contexte (autre demarrage) retrouve l'entree sur disque.
    let ctx2 = Ctx::new(ctx.data_dir.clone());
    assert_eq!(ctx2.hash_cache.lookup(&a, 4, mtime), Some(f[0].hash.clone()));
    // Fichier de cache corrompu : cache vide, resultats corrects.
    std::fs::write(ctx.data_dir.join("hashcache.json"), "{pas du json").unwrap();
    let ctx3 = Ctx::new(ctx.data_dir.clone());
    assert!(ctx3.hash_cache.is_empty());
    assert_eq!(store::scan_cached(&ctx3, &dir).unwrap().0, f);
    // Oubli explicite.
    ctx3.hash_cache.forget(&a);
    assert_eq!(ctx3.hash_cache.lookup(&a, 4, mtime), None);
}

// ------------------------------------------------------------------ durees au Journal

#[test]
fn formats_du_resume_de_duree() {
    let s = perf::format_summary("Restauration", "G", &[("scan".to_string(), Duration::from_millis(1500)), ("copie".to_string(), Duration::from_millis(300))], Duration::from_secs(2), 15, 3 * 1024 * 1024, false);
    assert_eq!(s, "[Perf] Restauration G : scan 1.5 s, copie 0.3 s, total 2.0 s (15 fichiers, 3.0 Mo)");
}

#[test]
fn sauvegarde_et_restauration_ecrivent_leurs_durees_au_journal() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, ga) = machine(t.path(), "A");
    write(&ga.path, "save.dat", "v1");
    write(&ga.path, "sub/x.dat", "xx");
    backup::backup(&a, &ga, &dest).unwrap();
    let (b, gb) = machine(t.path(), "B");
    write(&gb.path, "save.dat", "local");
    backup::restore(&b, &gb, &dest).unwrap();
    let la = read(&a.data_dir.join("logs").join("watch.log"));
    assert!(la.contains("[Perf] Sauvegarde TestGame"), "{}", la);
    assert!(la.contains("scan local") && la.contains("total") && la.contains("(2 fichiers"), "{}", la);
    let lb = read(&b.data_dir.join("logs").join("watch.log"));
    assert!(lb.contains("[Perf] Restauration TestGame"), "{}", lb);
    for step in ["lecture du dossier partage", "lecture + verification de la source", "snapshot de securite verifie", "ecriture dans le dossier de sauvegardes"] {
        assert!(lb.contains(step), "etape manquante : {} dans {}", step, lb);
    }
    assert_eq!(read(&gb.path.join("save.dat")), "v1");
    assert_eq!(read(&gb.path.join("sub/x.dat")), "xx");
}

// ------------------------------------------------------------------ progression + annulation

#[test]
fn progression_restauration_comptes_et_phases() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, ga) = machine(t.path(), "A");
    write(&ga.path, "a.dat", "1234");
    write(&ga.path, "b.dat", "56789");
    backup::backup(&a, &ga, &dest).unwrap();
    let (mut b, gb) = machine(t.path(), "B");
    write(&gb.path, "a.dat", "old");
    let p = Progress::new();
    b.progress = Some(p.clone());
    backup::restore(&b, &gb, &dest).unwrap();
    let v = p.view();
    assert_eq!(v.phase, Phase::Write, "derniere phase : ecriture");
    assert_eq!((v.files_done, v.files_total, v.bytes_done), (2, 2, 9));
    assert!(!v.cancellable, "apres le point de non-retour");
    assert!(!v.streaming);
}

#[test]
fn annulation_avant_ecriture_ne_modifie_rien() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, ga) = machine(t.path(), "A");
    write(&ga.path, "save.dat", "cloud");
    backup::backup(&a, &ga, &dest).unwrap();
    let (mut b, gb) = machine(t.path(), "B");
    write(&gb.path, "save.dat", "local-precieux");
    let p = Progress::new();
    p.request_cancel();
    b.progress = Some(p.clone());
    let err = backup::restore(&b, &gb, &dest).unwrap_err().to_string();
    assert!(err.contains(perf::CANCELLED), "{}", err);
    assert_eq!(read(&gb.path.join("save.dat")), "local-precieux");
    assert!(!b.data_dir.join("safety").exists(), "aucune copie de securite partielle");
    let tmp = b.data_dir.join("tmp");
    assert!(!tmp.exists() || std::fs::read_dir(&tmp).unwrap().next().is_none(), "dossier de travail nettoye");
    // Sans demande d'annulation, la meme restauration aboutit.
    b.progress = Some(Progress::new());
    backup::restore(&b, &gb, &dest).unwrap();
    assert_eq!(read(&gb.path.join("save.dat")), "cloud");
}

#[test]
fn annulation_ignoree_apres_le_point_de_non_retour() {
    let p = Progress::new();
    p.request_cancel();
    assert!(p.check_cancel().is_err());
    p.lock();
    assert!(p.check_cancel().is_ok(), "ecriture finale : on ne laisse jamais des fichiers a moitie restaures");
    assert!(!p.view().cancellable);
}

// ------------------------------------------------------------------ mode flux (fichiers a la demande)

#[test]
fn detection_mode_flux() {
    assert!(perf::attrs_say_on_demand(0x0040_0000), "RECALL_ON_DATA_ACCESS");
    assert!(perf::attrs_say_on_demand(0x0000_1000), "OFFLINE");
    assert!(perf::attrs_say_on_demand(0x0004_0000), "RECALL_ON_OPEN");
    assert!(!perf::attrs_say_on_demand(0x20), "ARCHIVE seul : fichier normal");
    // Lecture lente mesuree : 2 Mo en 10 s = trop lent ; 2 Mo en 0.1 s = normal ; tout petit fichier : pas de conclusion.
    assert!(perf::read_is_slow(2 * 1024 * 1024, Duration::from_secs(10)));
    assert!(!perf::read_is_slow(2 * 1024 * 1024, Duration::from_millis(100)));
    assert!(!perf::read_is_slow(10 * 1024, Duration::from_secs(1)));
    assert!(perf::STREAMING_WARNING.contains("mode miroir") && perf::STREAMING_WARNING.contains("hors ligne"));
}
