//! Tests v0.3 : logique de l'interface (view-model), restore --version, reglages, i18n, config.
use agss::actions;
use agss::backup;
use agss::config::{Config, Game};
use agss::detect::{self, Installed, Store};
use agss::i18n::{tr, Lang, ALL_KEYS};
use agss::process::ProcessChecker;
use agss::settings::{Settings, ThemeMode};
use agss::vm::{self, AppModel, Confirm, GameStatus, Level, Page, ThemeProbe};
use agss::watch::{EventKind, InstanceLock, Logger, WatchCfg, Watcher};
use agss::{Ctx, Res};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

struct FakeProc(Arc<AtomicBool>);
impl ProcessChecker for FakeProc {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct M {
    ctx: Ctx,
    game: Game,
    running: Arc<AtomicBool>,
    dest: PathBuf,
}

fn machine(root: &Path, n: &str) -> M {
    let running = Arc::new(AtomicBool::new(false));
    let mut ctx = Ctx::new(root.join(format!("data-{}", n)));
    ctx.machine = n.into();
    ctx.procs = Box::new(FakeProc(running.clone()));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "G".into(), path: root.join(format!("saves-{}", n)), exe: Some("g.exe".into()), dest_key: None, ..Default::default() };
    M { ctx, game, running, dest: root.join("cloud") }
}

fn write(p: &Path, c: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}
fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}
fn bk(m: &M, content: &str) {
    write(&m.game.path.join("save.dat"), content);
    backup::backup(&m.ctx, &m.game, &m.dest).unwrap();
}

// ---------------------------------------------------------------- i18n

#[test]
fn i18n_toutes_les_cles_ont_fr_et_en_coherents() {
    for k in ALL_KEYS {
        let (fr, en) = (tr(Lang::Fr, *k), tr(Lang::En, *k));
        assert!(!fr.trim().is_empty() && !en.trim().is_empty(), "{:?} vide", k);
        // memes marqueurs de format dans les deux langues
        for m in ["{}", "{0}", "{1}"] {
            assert_eq!(fr.matches(m).count(), en.matches(m).count(), "{:?} marqueur {} different", k, m);
        }
    }
    assert!(ALL_KEYS.len() > 80);
}

// ---------------------------------------------------------------- navigation / theme

#[test]
fn navigation_pages_circulaire() {
    assert_eq!(Page::Games.next(), Page::Add);
    assert_eq!(Page::Settings.next(), Page::Games);
    assert_eq!(Page::Games.prev(), Page::Settings);
}

struct Probe(Option<bool>);
impl ThemeProbe for Probe {
    fn system_is_light(&self) -> Option<bool> {
        self.0
    }
}

#[test]
fn theme_suivre_windows_abstrait() {
    assert!(vm::resolve_dark(ThemeMode::System, &Probe(Some(false))));
    assert!(!vm::resolve_dark(ThemeMode::System, &Probe(Some(true))));
    assert!(vm::resolve_dark(ThemeMode::System, &Probe(None)), "inconnu => sombre");
    assert!(vm::resolve_dark(ThemeMode::Dark, &Probe(Some(true))));
    assert!(!vm::resolve_dark(ThemeMode::Light, &Probe(Some(false))));
    let out = "\r\nHKEY_CURRENT_USER\\...\\Personalize\r\n    AppsUseLightTheme    REG_DWORD    0x1\r\n";
    assert_eq!(vm::parse_light_theme(out), Some(true));
    assert_eq!(vm::parse_light_theme(&out.replace("0x1", "0x0")), Some(false));
    assert_eq!(vm::parse_light_theme("rien"), None);
}

// ---------------------------------------------------------------- etats

#[test]
fn etats_des_jeux() {
    let t = TempDir::new().unwrap();
    let a = machine(t.path(), "A");
    write(&a.game.path.join("save.dat"), "v1");
    assert_eq!(vm::compute_status(&a.ctx, &a.game, None), GameStatus::NoDest);
    assert_eq!(vm::compute_status(&a.ctx, &a.game, Some(&a.dest)), GameStatus::Never);
    backup::backup(&a.ctx, &a.game, &a.dest).unwrap();
    assert_eq!(vm::compute_status(&a.ctx, &a.game, Some(&a.dest)), GameStatus::UpToDate);
    write(&a.game.path.join("save.dat"), "v2");
    assert_eq!(vm::compute_status(&a.ctx, &a.game, Some(&a.dest)), GameStatus::LocalChanged);
    a.running.store(true, Ordering::SeqCst);
    assert_eq!(vm::compute_status(&a.ctx, &a.game, Some(&a.dest)), GameStatus::Running);
    assert!(GameStatus::Running.blocks_actions());
    a.running.store(false, Ordering::SeqCst);

    // cloud plus recent : B met a jour apres avoir connu v2
    backup::backup(&a.ctx, &a.game, &a.dest).unwrap();
    let b = machine(t.path(), "B");
    backup::restore(&b.ctx, &b.game, &a.dest).unwrap();
    write(&b.game.path.join("save.dat"), "v3");
    backup::backup(&b.ctx, &b.game, &a.dest).unwrap();
    assert_eq!(vm::compute_status(&a.ctx, &a.game, Some(&a.dest)), GameStatus::CloudNewer);
    // conflit
    write(&a.game.path.join("save.dat"), "v3-A");
    assert_eq!(vm::compute_status(&a.ctx, &a.game, Some(&a.dest)), GameStatus::Conflict);
}

// ---------------------------------------------------------------- confirmations

#[test]
fn rien_ne_s_execute_sans_confirmation() {
    let t = TempDir::new().unwrap();
    let a = machine(t.path(), "A");
    write(&a.game.path.join("save.dat"), "v1");
    let mut cfg = Config::default();
    cfg.games.push(a.game.clone());
    cfg.dest = Some(a.dest.clone());
    cfg.save(&a.ctx).unwrap();
    let mut m = AppModel::new(a.ctx).unwrap();

    assert!(m.confirm_pending().is_err(), "aucune demande => aucune action");
    assert!(!a.dest.exists());

    m.request(Confirm::Backup { game: "G".into() });
    m.cancel();
    assert!(m.confirm.is_none());
    assert!(m.confirm_pending().is_err());
    assert!(!a.dest.exists(), "annule => rien ecrit");

    m.request(Confirm::Backup { game: "G".into() });
    assert!(m.confirm_pending().is_ok());
    assert!(!backup::list_versions(&a.dest, &a.game).is_empty());

    // messages en langage simple, dans les deux langues
    for c in [
        Confirm::Backup { game: "G".into() },
        Confirm::Restore { game: "G".into(), version: None },
        Confirm::Restore { game: "G".into(), version: Some("20260101-000000-000".into()) },
        Confirm::Prune { game: "G".into(), count: 3 },
        Confirm::Import { path: "x.json".into() },
        Confirm::Remove { game: "G".into() },
    ] {
        for l in [Lang::Fr, Lang::En] {
            let msg = c.message(l);
            assert!(!msg.contains("{}") && !msg.contains("{0}") && !msg.contains("{1}"), "{}", msg);
            assert!(msg.contains('G') || msg.contains("x.json"));
        }
    }
}

#[test]
fn retirer_un_jeu_ne_supprime_aucun_fichier() {
    let t = TempDir::new().unwrap();
    let a = machine(t.path(), "A");
    bk(&a, "v1");
    let mut cfg = Config::default();
    cfg.games.push(a.game.clone());
    cfg.dest = Some(a.dest.clone());
    cfg.save(&a.ctx).unwrap();
    let mut m = AppModel::new(a.ctx).unwrap();
    m.request(Confirm::Remove { game: "G".into() });
    m.confirm_pending().unwrap();
    assert!(m.config.games.is_empty());
    assert_eq!(read(&a.game.path.join("save.dat")), "v1");
    assert!(!backup::list_versions(&a.dest, &a.game).is_empty());
}

// ---------------------------------------------------------------- restore --version / prune

#[test]
fn restore_version_precise_avec_toutes_les_protections() {
    let t = TempDir::new().unwrap();
    let a = machine(t.path(), "A");
    for v in ["v1", "v2", "v3"] {
        bk(&a, v);
    }
    let versions = backup::list_versions(&a.dest, &a.game);
    assert_eq!(versions.len(), 3); // v1, v2 et v3 (la plus recente = active) : un dossier unique chacune
    let v1 = versions[0].file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(read(&versions[0].join("files/save.dat")), "v1");

    // refus si le jeu tourne : rien ne change
    a.running.store(true, Ordering::SeqCst);
    let err = backup::restore_version(&a.ctx, &a.game, &a.dest, &v1).unwrap_err().to_string();
    assert!(err.contains("REFUS"), "{}", err);
    assert_eq!(read(&a.game.path.join("save.dat")), "v3");
    assert!(!a.ctx.data_dir.join("safety").exists());
    a.running.store(false, Ordering::SeqCst);

    // version inconnue / tentative de traversee : refus
    for bad in ["nope", "..", "..\\..", "current", ""] {
        assert!(backup::restore_version(&a.ctx, &a.game, &a.dest, bad).is_err(), "{}", bad);
    }
    assert_eq!(read(&a.game.path.join("save.dat")), "v3");

    // restauration reelle : snapshot safety, contenu v1
    let r = backup::restore_version(&a.ctx, &a.game, &a.dest, &v1).unwrap();
    assert_eq!(read(&a.game.path.join("save.dat")), "v1");
    let safety = r.safety_dir.expect("snapshot safety");
    assert_eq!(read(&safety.join("files/save.dat")), "v3");

    // la version restauree devient la version ACTIVE (nouvelle sauvegarde, sans conflit), v3 reste dans l'historique
    let out = backup::backup(&a.ctx, &a.game, &a.dest).unwrap();
    assert!(matches!(out, backup::BackupOutcome::UpToDate), "{:?}", out);
    let newest = backup::list_versions(&a.dest, &a.game).pop().unwrap();
    assert_eq!(read(&newest.join("files/save.dat")), "v1");
    assert!(matches!(backup::plan(&a.ctx, &a.game, &a.dest).unwrap(), backup::Plan::InSync));
    let all: Vec<String> = backup::list_versions(&a.dest, &a.game).iter().map(|v| read(&v.join("files/save.dat"))).collect();
    assert!(all.contains(&"v3".to_string()), "{:?}", all);
}

#[test]
fn elagage_respecte_le_plancher_de_2() {
    let t = TempDir::new().unwrap();
    let mut a = machine(t.path(), "A");
    a.ctx.keep_versions = 50;
    for i in 1..=8 {
        bk(&a, &format!("v{}", i));
    }
    assert_eq!(backup::list_versions(&a.dest, &a.game).len(), 8);
    // apercu : ne supprime rien, meme avec keep=0 (plancher 2 + l'active)
    assert_eq!(backup::prune_plan(&a.dest, &a.game, 0).len(), 5);
    assert_eq!(backup::list_versions(&a.dest, &a.game).len(), 8);
    // reglage par jeu : 0 ou 1 => 2
    a.ctx.keep_overrides.insert("G".into(), 0);
    assert_eq!(backup::keep_for(&a.ctx, "G"), 2);
    // Un dossier etranger dans saves/ n'est JAMAIS touche par l'elagage.
    let stranger = a.dest.join("g/saves/pas-une-sauvegarde-agss");
    std::fs::create_dir_all(&stranger).unwrap();
    std::fs::write(stranger.join("x.txt"), "a moi").unwrap();
    assert_eq!(actions::prune_preview(&a.ctx, &a.game, &a.dest).len(), 5);
    let done = actions::do_prune(&a.ctx, &a.game, &a.dest);
    assert_eq!(done.len(), 5);
    let left = backup::list_versions(&a.dest, &a.game);
    assert_eq!(left.len(), 3, "2 anciennes + l'active");
    assert_eq!(read(&left[2].join("files/save.dat")), "v8");
    assert_eq!(read(&left[1].join("files/save.dat")), "v7");
    assert_eq!(std::fs::read_to_string(stranger.join("x.txt")).unwrap(), "a moi");
}

// ---------------------------------------------------------------- reglages

#[test]
fn reglages_par_jeu_plancher_non_contournable() {
    let t = TempDir::new().unwrap();
    let mut a = machine(t.path(), "A");
    let mut s = Settings::default();
    assert_eq!(s.set_keep("G", 0), 2);
    assert_eq!(s.set_keep("G", 1), 2);
    assert_eq!(s.set_keep("G", 25), 25);
    s.set_keep("H", 1);
    s.apply_to_ctx(&mut a.ctx);
    assert_eq!(backup::keep_for(&a.ctx, "G"), 25);
    assert_eq!(backup::keep_for(&a.ctx, "H"), 2);
    assert_eq!(backup::keep_for(&a.ctx, "autre"), backup::DEFAULT_KEEP);

    // un settings.json trafique (keep=0) est corrige au chargement
    std::fs::write(a.ctx.data_dir.join("settings.json"), r#"{"per_game":{"G":{"keep_versions":0,"stable_secs":0,"resync_secs":1}}}"#).unwrap();
    let l = Settings::load(&a.ctx).unwrap();
    let g = &l.per_game["G"];
    assert_eq!((g.keep_versions, g.stable_secs, g.resync_secs), (Some(2), Some(1), Some(10)));
    assert_eq!(l.lang, Lang::En, "English par defaut (premiere installation)");
    assert_eq!(l.theme, agss::settings::ThemeMode::Dark);
    assert_eq!(l.accent, [2, 132, 99]);
    assert!(l.tutorial_seen, "un settings.json deja existant ne declenche pas le premier lancement");
    assert!(l.notify_global);
}

#[test]
fn notifications_globales_et_par_jeu() {
    let mut s = Settings::default();
    assert!(s.notify_for("G"));
    s.notify_global = false;
    assert!(!s.notify_for("G"));
    s.game_mut("G").notify = Some(true);
    assert!(s.notify_for("G") && !s.notify_for("H"));
}

#[test]
fn settings_aller_retour() {
    let t = TempDir::new().unwrap();
    let a = machine(t.path(), "A");
    let mut s = Settings::default();
    s.lang = Lang::En;
    s.theme = ThemeMode::Light;
    s.accent = [1, 2, 3];
    s.big_mode = true;
    s.set_keep("G", 7);
    s.save(&a.ctx).unwrap();
    assert_eq!(Settings::load(&a.ctx).unwrap(), s);
}

// ---------------------------------------------------------------- import / export

#[test]
fn export_import_config() {
    let t = TempDir::new().unwrap();
    let a = machine(t.path(), "A");
    let mut cfg = Config::default();
    cfg.games.push(a.game.clone());
    cfg.dest = Some(a.dest.clone());
    cfg.save(&a.ctx).unwrap();
    let mut s = Settings::default();
    s.set_keep("G", 5);
    s.save(&a.ctx).unwrap();
    let file = t.path().join("export.json");
    actions::export_config(&a.ctx, &file).unwrap();

    // autre machine, config differente
    let b = machine(t.path(), "B");
    let mut other = Config::default();
    other.games.push(Game { name: "Autre".into(), path: "x".into(), exe: None, dest_key: None, ..Default::default() });
    other.save(&b.ctx).unwrap();
    let bak = actions::import_config(&b.ctx, &file).unwrap();
    let imported = Config::load(&b.ctx).unwrap();
    assert_eq!(imported.games, vec![a.game.clone()]);
    assert_eq!(imported.dest, Some(a.dest.clone()));
    assert_eq!(Settings::load(&b.ctx).unwrap().per_game["G"].keep_versions, Some(5));
    // l'ancienne config est conservee
    assert!(read(&bak.join("config.json")).contains("Autre"));
}

#[test]
fn import_invalide_ne_modifie_rien() {
    let t = TempDir::new().unwrap();
    let a = machine(t.path(), "A");
    let mut cfg = Config::default();
    cfg.games.push(a.game.clone());
    cfg.save(&a.ctx).unwrap();
    let before = read(&a.ctx.data_dir.join("config.json"));

    let bad = t.path().join("bad.json");
    std::fs::write(&bad, "pas du json").unwrap();
    assert!(actions::import_config(&a.ctx, &bad).is_err());
    // doublon de jeu
    let dup = t.path().join("dup.json");
    std::fs::write(&dup, r#"{"format":1,"config":{"games":[{"name":"X","path":"a"},{"name":"x","path":"b"}]},"settings":{}}"#).unwrap();
    assert!(actions::import_config(&a.ctx, &dup).is_err());
    // mauvais format
    let fmt = t.path().join("fmt.json");
    std::fs::write(&fmt, r#"{"format":99,"config":{"games":[]},"settings":{}}"#).unwrap();
    assert!(actions::import_config(&a.ctx, &fmt).is_err());
    assert_eq!(read(&a.ctx.data_dir.join("config.json")), before);
    assert!(!a.ctx.data_dir.join("imports-backup").exists());
    // un fichier importe avec keep_versions=0 est corrige
    let low = t.path().join("low.json");
    std::fs::write(&low, r#"{"format":1,"config":{"games":[]},"settings":{"per_game":{"G":{"keep_versions":0}}}}"#).unwrap();
    actions::import_config(&a.ctx, &low).unwrap();
    assert_eq!(Settings::load(&a.ctx).unwrap().per_game["G"].keep_versions, Some(2));
}

// ---------------------------------------------------------------- taille

#[test]
fn calcul_de_taille() {
    let t = TempDir::new().unwrap();
    write(&t.path().join("a/x.bin"), &"x".repeat(1000));
    write(&t.path().join("a/b/y.bin"), &"y".repeat(24));
    assert_eq!(actions::dir_size(&t.path().join("a")), 1024);
    assert_eq!(actions::dir_size(&t.path().join("absent")), 0);
    assert_eq!(actions::game_size(t.path(), &Game { name: "a".into(), path: std::path::PathBuf::new(), exe: None, dest_key: None, ..Default::default() }), 1024);
    assert_eq!(actions::format_bytes(0), "0 o");
    assert_eq!(actions::format_bytes(1024), "1.0 Ko");
    assert_eq!(actions::format_bytes(5 * 1024 * 1024 + 512 * 1024), "5.5 Mo");
}

// ---------------------------------------------------------------- journal

#[test]
fn journal_parse_et_filtre() {
    let lines = [
        "20261022-182959-789 watch demarre : 1 jeu(x)",
        "20261022-182959-861 [Fake] sauvegarde effectuee",
        "20261022-183000-000 [Fake] CONFLIT : les deux versions sont conservees",
        "20261022-183001-000 [Autre] ERREUR sauvegarde (essai 1) : boom",
        "ligne invalide",
    ];
    let e: Vec<_> = lines.iter().filter_map(|l| vm::parse_log_line(l)).collect();
    assert_eq!(e.len(), 4);
    assert_eq!(e[0].game, None);
    assert_eq!(e[1].game.as_deref(), Some("Fake"));
    assert_eq!((e[1].level, e[2].level, e[3].level), (Level::Info, Level::Warn, Level::Error));

    let f = |lvl, g: Option<&str>, txt: &str| {
        let flt = vm::LogFilter { min_level: lvl, game: g.map(|s| s.to_string()), text: txt.into() };
        vm::filter_log(&e, &flt).len()
    };
    assert_eq!(f(None, None, ""), 4);
    assert_eq!(f(Some(Level::Warn), None, ""), 2);
    assert_eq!(f(Some(Level::Error), None, ""), 1);
    assert_eq!(f(None, Some("Fake"), ""), 2);
    assert_eq!(f(None, None, "SAUVEGARDE"), 2);
    assert_eq!(f(Some(Level::Warn), Some("Fake"), "conflit"), 1);
}

#[test]
fn journal_lu_depuis_le_fichier_avec_rotation() {
    let t = TempDir::new().unwrap();
    let mut l = Logger::new(t.path().join("logs/watch.log"), false);
    l.max_bytes = 200;
    l.keep = 2;
    for i in 0..20 {
        l.log(&format!("[G] evenement {}", i));
    }
    let e = vm::read_log(t.path(), 1000);
    assert!(!e.is_empty() && e.iter().all(|x| x.game.as_deref() == Some("G")));
    assert!(e.last().unwrap().text.contains("19"));
}

// ---------------------------------------------------------------- detection : non-jeux

#[test]
fn filtre_des_non_jeux() {
    let mk = |name: &str, id: &str| Installed { name: name.into(), store: Store::Epic, id: id.into(), install_dir: "x".into(), exe: None };
    for (n, id) in [("Unreal Engine", "UE_5.8"), ("Fab UE Plugin", "FabPlugin_5.8"), ("Discord", "abc"), ("Epic Games Launcher", "x")] {
        assert!(detect::is_non_game(&mk(n, id)), "{}", n);
    }
    for (n, id) in [("Hades II", "Fennel"), ("Hogwarts Legacy", "fa42"), ("Celeste", "1")] {
        assert!(!detect::is_non_game(&mk(n, id)), "{}", n);
    }
}

// ---------------------------------------------------------------- watch : delais par jeu, evenements, verrous

#[test]
fn watch_delais_par_jeu_et_evenements() {
    let t = TempDir::new().unwrap();
    let m = machine(t.path(), "A");
    std::fs::create_dir_all(&m.dest).unwrap(); // 2.0 : la surveillance ne cree jamais le coffre lui-meme
    write(&m.game.path.join("save.dat"), "v1");
    let log = Logger::new(t.path().join("w.log"), false);
    let events: RefCell<Vec<(String, EventKind)>> = RefCell::new(vec![]);
    let cfg = WatchCfg { stable_ms: 60_000, resync_ms: 600_000, max_tries: 3, safety_ms: 2000, retry_ms: 1000, retry_max_ms: 30_000 };
    let mut w = Watcher::new(&m.ctx, vec![m.game.clone()], m.dest.clone(), cfg, &log);
    w.timing.insert("G".into(), (Some(2_000), None)); // ce jeu : 2 s de stabilite au lieu de 60
    w.on_event = Some(Box::new(|e| events.borrow_mut().push((e.game, e.kind))));
    w.startup(0);
    assert_eq!(events.borrow().len(), 1, "evenement de sauvegarde au demarrage");
    assert_eq!(events.borrow()[0], ("G".to_string(), EventKind::BackedUp));

    m.running.store(true, Ordering::SeqCst);
    w.tick(1_000);
    write(&m.game.path.join("save.dat"), "v2-plus-long");
    m.running.store(false, Ordering::SeqCst);
    w.tick(2_000);
    w.tick(3_000);
    assert_eq!(read(&backup::list_versions(&m.dest, &m.game).pop().unwrap().join("files/save.dat")), "v1");
    w.tick(4_500);
    assert_eq!(read(&backup::list_versions(&m.dest, &m.game).pop().unwrap().join("files/save.dat")), "v2-plus-long", "delai par jeu applique");
    assert_eq!(events.borrow().len(), 2);
}

#[test]
fn verrous_gui_et_watch_independants() {
    let t = TempDir::new().unwrap();
    let w = InstanceLock::acquire(t.path(), &|_| true).unwrap();
    let g = InstanceLock::acquire_named(t.path(), "gui.lock", &|_| true).unwrap();
    assert!(InstanceLock::acquire_named(t.path(), "gui.lock", &|_| true).is_err(), "2e GUI refusee");
    assert!(InstanceLock::acquire(t.path(), &|_| true).is_err(), "2e watch refuse");
    drop(g);
    drop(w);
    assert!(!t.path().join("gui.lock").exists() && !t.path().join("watch.lock").exists());
}
