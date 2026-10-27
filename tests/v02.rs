//! Tests v0.2 : detection (parseurs sur fixtures), resolution <root>/<base>/<storeGameId>, watch.
use agss::config::Game;
use agss::detect::{self, CatalogIndex, Store};
use agss::catalog::{self, Catalog, StoreCtx};
use agss::process::ProcessChecker;
use agss::watch::{InstanceLock, Logger, WatchCfg, Watcher};
use agss::{backup, Ctx, Res};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

// ---------------------------------------------------------------- fixtures

const LIBFOLDERS_NEW: &str = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"1234"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
	"contentstatsid"		"123"
}
"#;

const LIBFOLDERS_OLD: &str = r#"
"LibraryFolders"
{
	"TimeNextStatsReport"		"1234"
	"1"		"E:\\Jeux\\Steam"
}
"#;

const ACF: &str = r#"
"AppState"
{
	"appid"		"504230"
	"Universe"		"1"
	"name"		"Celeste"
	"installdir"		"Celeste"
	"StateFlags"		"4"
}
"#;

const EPIC_ITEM: &str = r#"{"FormatVersion":0,"DisplayName":"Hades II","AppName":"Fennel","CatalogItemId":"abc","InstallLocation":"C:\\Games\\Epic\\HadesII","LaunchExecutable":"Ship/Hades2.exe"}"#;

const GOG_REG: &str = r#"
HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games\1207658930

HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games\1207658930
    gameID    REG_SZ    1207658930
    gameName    REG_SZ    The Witcher 3
    path    REG_SZ    C:\GOG Games\Witcher 3
    exe    REG_SZ    C:\GOG Games\Witcher 3\bin\x64\witcher3.exe

HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games\1
    gameID    REG_SZ    1
    gameName    REG_SZ    Sans Exe
    path    REG_SZ    C:\GOG Games\Sans Exe
"#;

const GOG_INFO: &str = r#"{"gameId":"1","name":"X","playTasks":[{"isPrimary":false,"path":"manual.exe"},{"isPrimary":true,"path":"bin/Game.exe"}]}"#;

#[test]
fn vdf_libraryfolders_nouveau_et_ancien_format() {
    let n = detect::parse_library_folders(LIBFOLDERS_NEW).unwrap();
    assert_eq!(n, vec![Path::new("C:\\Program Files (x86)\\Steam"), Path::new("D:\\SteamLibrary")]);
    let o = detect::parse_library_folders(LIBFOLDERS_OLD).unwrap();
    assert_eq!(o, vec![Path::new("E:\\Jeux\\Steam")]);
    assert!(detect::parse_vdf("\"a\" {").is_err());
    assert!(detect::parse_library_folders("\"autre\" { }").is_err());
}

#[test]
fn acf_parse() {
    assert_eq!(detect::parse_acf(ACF).unwrap(), ("504230".into(), "Celeste".into(), "Celeste".into()));
    assert!(detect::parse_acf("\"AppState\" { \"appid\" \"1\" }").is_err());
}

#[test]
fn epic_item_et_gog_parse() {
    let e = detect::parse_epic_item(EPIC_ITEM).unwrap();
    assert_eq!((e.name.as_str(), e.id.as_str(), e.exe.as_deref()), ("Hades II", "Fennel", Some("Hades2.exe")));
    assert_eq!(e.install_dir, Path::new("C:\\Games\\Epic\\HadesII"));
    assert!(detect::parse_epic_item("pas du json").is_err());

    let g = detect::parse_gog_reg(GOG_REG);
    assert_eq!(g.len(), 2);
    assert_eq!(g[0].id, "1207658930");
    assert_eq!(g[0].exe.as_deref(), Some("witcher3.exe"));
    assert_eq!(g[1].exe, None);
    assert_eq!(detect::parse_gog_info_exe(GOG_INFO).as_deref(), Some("Game.exe"));
}

fn write(p: &Path, c: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}

#[test]
fn decouverte_steam_et_epic_sur_dossiers_temporaires() {
    let t = TempDir::new().unwrap();
    let lib1 = t.path().join("Steam");
    let lib2 = t.path().join("Lib2");
    write(&lib1.join("steamapps/appmanifest_504230.acf"), ACF);
    write(&lib1.join("steamapps/appmanifest_228980.acf"), "\"AppState\" { \"appid\" \"228980\" \"name\" \"Steamworks Common Redistributables\" \"installdir\" \"x\" }");
    write(&lib2.join("steamapps/appmanifest_1.acf"), "\"AppState\" { \"appid\" \"1\" \"name\" \"Autre\" \"installdir\" \"Autre\" }");
    let vdf = format!("\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} }}", lib1.display().to_string().replace('\\', "\\\\"), lib2.display().to_string().replace('\\', "\\\\"));
    write(&lib1.join("steamapps/libraryfolders.vdf"), &vdf);

    let libs = detect::steam_libraries(&lib1);
    assert_eq!(libs.len(), 2);
    let games = detect::steam_games(&libs);
    let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains(&"Celeste") && names.contains(&"Autre"));
    assert!(!names.iter().any(|n| n.starts_with("Steamworks")));
    let celeste = games.iter().find(|g| g.name == "Celeste").unwrap();
    assert_eq!(celeste.install_dir, lib1.join("steamapps/common/Celeste"));

    let em = t.path().join("Catalogs");
    write(&em.join("a.item"), EPIC_ITEM);
    write(&em.join("junk.item"), "{");
    write(&em.join("b.txt"), EPIC_ITEM);
    assert_eq!(detect::epic_games(&em).len(), 1);
}

fn catalog_fixture() -> Catalog {
    serde_yaml::from_str(
        r#"
Celeste:
  steam: {id: 504230}
  installDir: {Celeste: {}}
  files:
    '<base>/Saves':
      tags: [save]
    '<root>/userdata/<storeUserId>/<storeGameId>/remote':
      tags: [save]
Hades II:
  files:
    '<winDocuments>/Hades2':
      tags: [save]
Witcher Third:
  gog: {id: 1207658930}
  files: {}
"#,
    )
    .unwrap()
}

#[test]
fn rapprochement_catalog_par_id_puis_par_nom() {
    let m = catalog_fixture();
    let idx = CatalogIndex::new(&m);
    let steam = detect::Installed { name: "Nom different".into(), store: Store::Steam, id: "504230".into(), install_dir: "x".into(), exe: None };
    assert_eq!(idx.find(&steam).unwrap(), "Celeste"); // par appid
    let epic = detect::Installed { name: "HADES  II".into(), store: Store::Epic, id: "Fennel".into(), install_dir: "x".into(), exe: None };
    assert_eq!(idx.find(&epic).unwrap(), "Hades II"); // par nom normalise
    let gog = detect::Installed { name: "The Witcher 3".into(), store: Store::Gog, id: "1207658930".into(), install_dir: "x".into(), exe: None };
    assert_eq!(idx.find(&gog).unwrap(), "Witcher Third"); // par id GOG
    let inconnu = detect::Installed { name: "Zzz".into(), store: Store::Steam, id: "9".into(), install_dir: "x".into(), exe: None };
    assert!(idx.find(&inconnu).is_none());
}

#[cfg(windows)]
#[test]
fn placeholders_root_base_storegameid_avec_vraies_bibliotheques() {
    let t = TempDir::new().unwrap();
    let lib2 = t.path().join("Lib2");
    std::fs::create_dir_all(lib2.join("steamapps/common/Celeste")).unwrap();
    let m = catalog_fixture();
    let sc = StoreCtx { steam_libs: vec![t.path().join("Lib1"), lib2.clone()], install_dir: None, game_id: None };
    let r = catalog::resolve_with(&m["Celeste"], "Celeste", &sc);
    let base = lib2.join("steamapps/common/Celeste/Saves").display().to_string().replace('/', "\\");
    assert!(r.paths.contains(&base), "{:?}", r.paths); // <base> = bibliotheque secondaire existante
    assert!(r.paths.iter().any(|p| p.ends_with("userdata\\*\\504230\\remote")), "{:?}", r.paths); // <storeGameId> via catalog
    assert!(r.unresolved.is_empty(), "{:?}", r.unresolved);

    // Avec un dossier d'installation detecte (Epic/GOG), <base> = install_dir.
    let sc2 = StoreCtx { steam_libs: vec![], install_dir: Some("C:\\Games\\Celeste".into()), game_id: Some("77".into()) };
    let r2 = catalog::resolve_with(&m["Celeste"], "Celeste", &sc2);
    assert!(r2.paths.contains(&"C:\\Games\\Celeste\\Saves".to_string()), "{:?}", r2.paths);
    assert!(r2.paths.iter().any(|p| p.contains("\\77\\")));
    // Sans aucune info : root/base non resolus et dit.
    let r3 = catalog::resolve_with(&m["Celeste"], "Celeste", &StoreCtx::default());
    assert!(r3.unresolved.contains(&"root".to_string()));
}

#[cfg(windows)]
#[test]
fn selection_et_dossiers_generiques() {
    assert_eq!(catalog::parse_selection("1, 3", 3).unwrap(), vec![0, 2]);
    assert_eq!(catalog::parse_selection("tous", 3).unwrap(), vec![0, 1, 2]);
    assert!(catalog::parse_selection("", 3).unwrap().is_empty());
    assert!(catalog::parse_selection("4", 3).is_err());
    assert!(catalog::parse_selection("x", 3).is_err());
    assert!(catalog::is_generic_root(&dirs::document_dir().unwrap()));
    assert!(catalog::is_generic_root(Path::new("C:\\")));
    assert!(!catalog::is_generic_root(&dirs::document_dir().unwrap().join("MonJeu")));
}

#[test]
fn guess_exe_prend_le_plus_gros_hors_installeurs() {
    let t = TempDir::new().unwrap();
    write(&t.path().join("Game.exe"), &"x".repeat(1000));
    write(&t.path().join("unins000.exe"), &"x".repeat(5000));
    write(&t.path().join("Small.exe"), "x");
    assert_eq!(detect::guess_exe(t.path()).as_deref(), Some("Game.exe"));
}

// ---------------------------------------------------------------- watch

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
    log: Logger,
}

fn machine(root: &Path, n: &str, exe: Option<&str>) -> M {
    let running = Arc::new(AtomicBool::new(false));
    let mut ctx = Ctx::new(root.join(format!("data-{}", n)));
    ctx.machine = n.into();
    ctx.procs = Box::new(FakeProc(running.clone()));
    let log = Logger::new(root.join(format!("data-{}/watch.log", n)), false);
    let game = Game { name: "G".into(), path: root.join(format!("saves-{}", n)), exe: exe.map(|e| e.to_string()), dest_key: None, ..Default::default() };
    M { ctx, game, running, log }
}
fn save(m: &M) -> String {
    std::fs::read_to_string(m.game.path.join("save.dat")).unwrap()
}
fn gg() -> Game {
    Game { name: "G".into(), path: std::path::PathBuf::new(), exe: None, dest_key: None, ..Default::default() }
}
/// Contenu de la version ACTIVE (la plus recente) dans la destination.
fn cloud(dest: &Path) -> String {
    let v = backup::list_versions(dest, &gg()).pop().expect("une sauvegarde");
    std::fs::read_to_string(v.join("files/save.dat")).unwrap()
}
const CFG: WatchCfg = WatchCfg { stable_ms: 10_000, resync_ms: 60_000, max_tries: 3, safety_ms: 2000, retry_ms: 1000, retry_max_ms: 30_000 };

#[test]
fn watch_backup_a_la_fermeture_apres_stabilite() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    std::fs::create_dir_all(&dest).unwrap(); // 2.0 : la surveillance ne cree jamais le coffre lui-meme
    let m = machine(t.path(), "A", Some("g.exe"));
    write(&m.game.path.join("save.dat"), "v1");
    let mut w = Watcher::new(&m.ctx, vec![m.game.clone()], dest.clone(), CFG, &m.log);
    w.startup(0);
    assert_eq!(cloud(&dest), "v1");

    m.running.store(true, Ordering::SeqCst);
    w.tick(1_000); // lancement detecte
    write(&m.game.path.join("save.dat"), "v2");
    w.tick(2_000);
    assert_eq!(cloud(&dest), "v1", "jamais de backup pendant que le jeu tourne");

    m.running.store(false, Ordering::SeqCst);
    w.tick(3_000); // fermeture detectee
    w.tick(8_000);
    assert_eq!(cloud(&dest), "v1", "pas encore stable");
    write(&m.game.path.join("save.dat"), "v3-ecriture-tardive"); // le jeu finit d'ecrire
    w.tick(9_000); // changement => compteur remis a zero
    w.tick(18_000);
    assert_eq!(cloud(&dest), "v1", "9 s de stabilite < 10 s");
    w.tick(19_000);
    assert_eq!(cloud(&dest), "v3-ecriture-tardive");
    assert_eq!(backup::list_versions(&dest, &gg()).len(), 2, "v1 puis v3 : un dossier unique par sauvegarde");
}

#[test]
fn watch_ne_restaure_jamais_un_jeu_en_cours_puis_restaure_apres_fermeture() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A", Some("g.exe"));
    let b = machine(t.path(), "B", Some("g.exe"));
    write(&a.game.path.join("save.dat"), "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap(); // B connait v1
    write(&a.game.path.join("save.dat"), "v2-depuis-A");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();

    b.running.store(true, Ordering::SeqCst); // jeu deja lance sur B
    let mut w = Watcher::new(&b.ctx, vec![b.game.clone()], dest.clone(), CFG, &b.log);
    w.startup(0);
    for t in [1_000, 100_000, 200_000, 300_000] {
        w.tick(t);
    }
    assert_eq!(save(&b), "v1", "jeu en cours : intouche");
    assert!(!b.ctx.data_dir.join("safety").exists());

    b.running.store(false, Ordering::SeqCst);
    w.tick(400_000); // fermeture
    w.tick(405_000);
    assert_eq!(save(&b), "v1", "stabilite pas atteinte");
    w.tick(410_000);
    assert_eq!(save(&b), "v2-depuis-A", "restaure apres fermeture + stabilite");
    assert!(b.ctx.data_dir.join("safety").exists(), "snapshot safety fait");
}

#[test]
fn watch_restaure_au_demarrage_et_a_la_resynchro_les_jeux_fermes() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A", Some("g.exe"));
    let b = machine(t.path(), "B", Some("g.exe"));
    write(&a.game.path.join("save.dat"), "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    let mut w = Watcher::new(&b.ctx, vec![b.game.clone()], dest.clone(), CFG, &b.log);
    w.startup(0); // B n'a rien : restauration du cloud
    assert_eq!(save(&b), "v1");

    write(&a.game.path.join("save.dat"), "v2");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    w.tick(30_000);
    assert_eq!(save(&b), "v1", "pas encore l'heure de resynchroniser");
    w.tick(61_000);
    assert_eq!(save(&b), "v2");
}

#[test]
fn watch_conflit_garde_les_deux_sans_restauration_automatique() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A", Some("g.exe"));
    let b = machine(t.path(), "B", Some("g.exe"));
    write(&a.game.path.join("save.dat"), "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    write(&a.game.path.join("save.dat"), "v2-A");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    write(&b.game.path.join("save.dat"), "v2-B"); // les deux ont diverge
    let mut w = Watcher::new(&b.ctx, vec![b.game.clone()], dest.clone(), CFG, &b.log);
    w.startup(0);
    let mut seen = vec![cloud(&dest)];
    for v in backup::list_versions(&dest, &gg()) {
        seen.push(std::fs::read_to_string(v.join("files/save.dat")).unwrap());
    }
    assert!(seen.contains(&"v2-A".to_string()) && seen.contains(&"v2-B".to_string()), "{:?}", seen);
}

#[test]
fn watch_ignore_les_jeux_sans_exe() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let m = machine(t.path(), "A", None);
    write(&m.game.path.join("save.dat"), "v1");
    let mut w = Watcher::new(&m.ctx, vec![m.game.clone()], dest.clone(), CFG, &m.log);
    w.startup(0);
    w.tick(1_000_000);
    assert!(!dest.exists() || !dest.join("G").exists(), "pas d'exe => aucune action automatique");
}

#[test]
fn instance_unique_et_verrou_perime() {
    let t = TempDir::new().unwrap();
    let l1 = InstanceLock::acquire(t.path(), &|_| true).unwrap();
    assert!(InstanceLock::acquire(t.path(), &|_| true).is_err(), "2e instance refusee");
    drop(l1);
    let l2 = InstanceLock::acquire(t.path(), &|_| true).unwrap(); // libere => ok
    drop(l2);
    std::fs::write(t.path().join("watch.lock"), "999999").unwrap(); // verrou d'un watch mort
    let l3 = InstanceLock::acquire(t.path(), &|_| false).unwrap();
    drop(l3);
    assert!(!t.path().join("watch.lock").exists());
    // vrai test de pid : notre propre processus est vivant, un pid absurde non
    assert!(agss::watch::pid_alive(std::process::id()));
    assert!(!agss::watch::pid_alive(4_000_000_000));
}

#[test]
fn logs_avec_rotation() {
    let t = TempDir::new().unwrap();
    let mut l = Logger::new(t.path().join("logs/w.log"), false);
    l.max_bytes = 100;
    l.keep = 2;
    for i in 0..30 {
        l.log(&format!("ligne numero {}", i));
    }
    assert!(t.path().join("logs/w.log").exists());
    assert!(t.path().join("logs/w.log.1").exists());
    assert!(t.path().join("logs/w.log.2").exists());
    assert!(!t.path().join("logs/w.log.3").exists());
}
