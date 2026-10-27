//! 2.0 lot 6 : accueil en tuiles. Tri par priorite, resume global (bandeau), etat d'une tuile, etat de synchro ENTRE
//! APPAREILS (logique pure + vrai coffre temporaire a deux appareils), filtres, navigation au clavier, marques de position
//! (ecrites une fois, ignorees par le reste), pause d'un element (compatibilite des reglages), tuile sur un vrai egui.
//! UNIQUEMENT des dossiers temporaires.
use agss::backup;
use agss::config::{Config, Game};
use agss::gui::ui::{self, anim::Motion, State, TileIcon, Tokens};
use agss::home::{self, Ago, DeviceSync, Dir, Facts, Filter, Head, Headline, Kind, MainAction, Row, Watch, Why};
use agss::i18n::{tr, Lang, K};
use agss::process::ProcessChecker;
use agss::settings::Settings;
use agss::vm::{self, GameStatus};
use agss::{Ctx, Res};
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn row(name: &str, st: State, recent: u64) -> Row {
    Row { name: name.into(), shown: name.into(), state: st, recent_ms: recent }
}

fn facts(status: GameStatus) -> Facts {
    let g = Game { name: "Celeste".into(), path: PathBuf::from("C:/Saves/Celeste"), exe: Some("Celeste.exe".into()), ..Default::default() };
    Facts::bare(&g, Kind::Game, status)
}

fn head(machine: &str, id: &str, hash: &str) -> Option<Head> {
    Some(Head { machine: machine.into(), id: id.into(), hash: hash.into(), saved_ms: 1_000 })
}

// ================================================================== tri

#[test]
fn tri_par_priorite_puis_recent_puis_alphabetique() {
    let mut rows = vec![
        row("Zelda", State::Safe, 10),
        row("Minecraft", State::Safe, 50),
        row("Factorio", State::Incoming, 5),
        row("Hollow Knight", State::Busy, 1),
        row("Elden Ring", State::Todo, 2),
        row("Dolphin", State::Problem, 0),
        row("Celeste", State::Paused, 3),
        row("alpha", State::Safe, 10),
    ];
    home::sort_rows(&mut rows);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // a verifier (probleme puis a faire) > en cours > version recue > en pause > a l'abri (recent d'abord, puis A-Z sans casse)
    assert_eq!(names, ["Dolphin", "Elden Ring", "Hollow Knight", "Factorio", "Celeste", "Minecraft", "alpha", "Zelda"]);
    // Stable et deterministe : un second tri ne change rien.
    let again = rows.clone();
    home::sort_rows(&mut rows);
    assert_eq!(rows, again);
    // Activite recente = la plus recente de la derniere synchro et de la version du coffre.
    let mut f = facts(GameStatus::UpToDate);
    f.last_sync_ms = Some(500);
    f.head = Some(Head { saved_ms: 900, ..Default::default() });
    assert_eq!(home::recent_ms(&f), 900);
}

// ================================================================== bandeau

#[test]
fn resume_global_un_seul_message_le_plus_important() {
    use State::*;
    let s = home::summary(&[Safe, Todo], false, true, Watch::Running, None);
    assert_eq!((s.headline, s.action, s.state), (Headline::NoVault, Some(MainAction::ChooseVault), Todo));
    let s = home::summary(&[Safe, Problem], true, false, Watch::Running, None);
    assert_eq!((s.headline, s.action, s.state), (Headline::VaultUnreachable, None, Problem), "le coffre injoignable passe avant le reste");
    assert_eq!(home::summary(&[], true, true, Watch::Running, None).headline, Headline::Empty);
    let s = home::summary(&[Safe], true, true, Watch::Failed, None);
    assert_eq!((s.headline, s.action), (Headline::WatchFailed, Some(MainAction::Restart)));
    let s = home::summary(&[Safe, Todo], true, true, Watch::Paused, None);
    assert_eq!((s.headline, s.action, s.state), (Headline::WatchPaused, Some(MainAction::Resume), Paused));
    let s = home::summary(&[Safe, Todo, Problem, Busy, Todo], true, true, Watch::Running, Some(7));
    assert_eq!((s.headline, s.action, s.state, s.total, s.last_copy_ms), (Headline::ToCheck(3), Some(MainAction::SyncAll), Problem, 5, Some(7)));
    assert_eq!(home::summary(&[Safe, Todo], true, true, Watch::Running, None).state, Todo);
    assert_eq!(home::summary(&[Safe, Busy, Incoming], true, true, Watch::Running, None).headline, Headline::Copying(1));
    assert_eq!(home::summary(&[Safe, Incoming, Paused], true, true, Watch::Running, None).headline, Headline::Incoming(1));
    let s = home::summary(&[Safe, Safe, Paused], true, true, Watch::Starting, Some(3));
    assert_eq!((s.headline, s.action, s.state), (Headline::AllSafe, Some(MainAction::SyncAll), Safe), "demarrage : rien a signaler");
}

// ================================================================== etat d'une tuile

#[test]
fn etat_de_chaque_tuile() {
    let me = "PC-DESKTOP";
    let st = |s: GameStatus| home::item_state(&facts(s), None, true, me);
    let mut ok = facts(GameStatus::UpToDate);
    ok.last_sync_ms = Some(42);
    assert_eq!(home::item_state(&ok, None, true, me), (State::Safe, Why::Safe(Some(42))));
    assert_eq!(st(GameStatus::Never), (State::Todo, Why::Pending));
    assert_eq!(st(GameStatus::LocalChanged), (State::Todo, Why::Pending));
    assert_eq!(st(GameStatus::Running), (State::Paused, Why::InUse));
    assert_eq!(st(GameStatus::Conflict), (State::Problem, Why::Conflict));
    assert_eq!(st(GameStatus::NoDest), (State::Paused, Why::WaitingVault));
    assert_eq!(st(GameStatus::Error("os error 32 : fichier verrouillé".into())), (State::Problem, Why::Locked));
    assert_eq!(st(GameStatus::Error("acces refuse".into())), (State::Problem, Why::Problem("acces refuse".into())));
    // Version recue : de quel appareil.
    let mut inc = facts(GameStatus::CloudNewer);
    inc.head = head("ALLY", "v2", "h2");
    assert_eq!(home::item_state(&inc, None, true, me), (State::Incoming, Why::NewerFrom("ALLY".into())));
    inc.head = head("pc-desktop", "v2", "h2");
    assert_eq!(home::item_state(&inc, None, true, me), (State::Incoming, Why::NewerFrom(String::new())), "pas « de PC-DESKTOP » pour soi-meme");
    // Copie en cours (pourcentage), pause par l'utilisateur, coffre injoignable.
    assert_eq!(home::item_state(&ok, Some(Some(0.643)), true, me), (State::Busy, Why::Copying(Some(64))));
    assert_eq!(home::item_state(&ok, Some(None), true, me), (State::Busy, Why::Copying(None)));
    let mut p = facts(GameStatus::LocalChanged);
    p.user_paused = true;
    assert_eq!(home::item_state(&p, None, true, me), (State::Paused, Why::PausedByUser));
    assert_eq!(home::item_state(&ok, None, false, me), (State::Paused, Why::WaitingVault), "coffre injoignable : calme, le bandeau explique");
    // En cours d'utilisation passe avant tout (le programme tourne).
    let mut r = facts(GameStatus::Running);
    r.user_paused = true;
    assert_eq!(home::item_state(&r, None, false, me).1, Why::InUse);
}

// ================================================================== synchro entre appareils (logique)

#[test]
fn etat_de_synchro_entre_appareils() {
    let me = "PC-DESKTOP";
    // Seul appareil.
    let mut f = facts(GameStatus::UpToDate);
    f.head = head("PC-DESKTOP", "v1", "h1");
    f.writers = vec![("PC-DESKTOP".into(), "v1".into(), "h1".into(), 1)];
    assert_eq!(home::device_sync(&f, me, true), DeviceSync::OnlyHere);
    // L'autre appareil a remis cette version (marque de position) : a jour sur 2 appareils.
    f.marks = vec![("ALLY".into(), "v1".into())];
    assert_eq!(home::device_sync(&f, me, true), DeviceSync::UpToDateOn(2));
    assert_eq!(home::devices_at_head(&f, me), ["ALLY", "PC-DESKTOP"]);
    // Sa marque designe une version plus ancienne : en attente sur ALLY.
    f.marks = vec![("ALLY".into(), "v0".into())];
    assert_eq!(home::device_sync(&f, me, true), DeviceSync::Waiting(vec!["ALLY".into()]));
    // L'autre appareil a ecrit une version au MEME contenu (sa derniere) : a jour aussi, sans marque.
    f.marks.clear();
    f.writers.push(("ALLY".into(), "v1b".into(), "h1".into(), 2));
    assert_eq!(home::device_sync(&f, me, true), DeviceSync::UpToDateOn(2));
    // ... mais si sa DERNIERE version a un autre contenu, il n'est pas a jour.
    f.writers.push(("ALLY".into(), "v1c".into(), "h9".into(), 3));
    assert_eq!(home::device_sync(&f, me, true), DeviceSync::Waiting(vec!["ALLY".into()]));
    // Trois appareils, casse des noms ignoree.
    f.writers.clear();
    f.marks = vec![("ally".into(), "v1".into()), ("STEAMDECK-1a2b".into(), "v1".into())];
    assert_eq!(home::device_sync(&f, me, true), DeviceSync::UpToDateOn(3));
    // Plus recent sur l'autre appareil / ici / dans le coffre.
    let mut n = facts(GameStatus::CloudNewer);
    n.head = head("ALLY", "v2", "h2");
    assert_eq!(home::device_sync(&n, me, true), DeviceSync::NewerOn("ALLY".into()));
    n.head = head("PC-DESKTOP", "v2", "h2");
    assert_eq!(home::device_sync(&n, me, true), DeviceSync::NewerInVault);
    let mut l = facts(GameStatus::LocalChanged);
    l.head = head("ALLY", "v2", "h2");
    assert_eq!(home::device_sync(&l, me, true), DeviceSync::NewerHere);
    // Jamais copie, rien dans le coffre : seulement ici. Conflit. Coffre injoignable : on ne dit rien.
    assert_eq!(home::device_sync(&facts(GameStatus::Never), me, true), DeviceSync::OnlyHere);
    assert_eq!(home::device_sync(&facts(GameStatus::Conflict), me, true), DeviceSync::Conflict);
    assert_eq!(home::device_sync(&f, me, false), DeviceSync::Unknown);
    // Programme ouvert : etat local inconnu ; on ne dit que ce qui est sur pour les autres.
    let mut r = facts(GameStatus::Running);
    r.head = head("ALLY", "v1", "h1");
    assert_eq!(home::device_sync(&r, me, true), DeviceSync::Unknown);
    r.marks = vec![("STEAMDECK".into(), "v1".into())];
    assert_eq!(home::device_sync(&r, me, true), DeviceSync::UpToDateOn(2));
    r.marks.push(("PC-DESKTOP".into(), "v1".into()));
    assert_eq!(home::device_sync(&r, me, true), DeviceSync::UpToDateOn(3), "sa propre marque compte pendant que le programme tourne");
    // Jamais copie ici mais une version existe ailleurs : plus recent la-bas.
    let mut nv = facts(GameStatus::Never);
    nv.head = head("ALLY", "v1", "h1");
    assert_eq!(home::device_sync(&nv, me, true), DeviceSync::NewerOn("ALLY".into()));
}

// ================================================================== synchro entre appareils (vrai coffre, 2 appareils)

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

struct Pc {
    ctx: Ctx,
    game: Game,
}

fn pc(root: &Path, name: &str) -> Pc {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "Hollow Knight".into(), path: root.join(format!("saves-{}", name)), exe: None, ..Default::default() };
    std::fs::create_dir_all(&game.path).unwrap();
    Pc { ctx, game }
}

fn gather_one(p: &Pc, dest: &Path) -> Facts {
    let st = vm::compute_status(&p.ctx, &p.game, Some(dest));
    let cfg = Config { games: vec![p.game.clone()], dest: Some(dest.to_path_buf()) };
    let mut m = HashMap::new();
    m.insert(p.game.name.clone(), st);
    home::gather(&p.ctx, &cfg, &Settings::default(), &m).remove(0)
}

#[test]
fn deux_appareils_dans_un_vrai_coffre_temporaire() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("coffre");
    std::fs::create_dir_all(&dest).unwrap();
    let a = pc(tmp.path(), "PC-DESKTOP");
    let b = pc(tmp.path(), "ALLY");
    // A protege sa partie.
    std::fs::write(a.game.path.join("slot1.dat"), "niveau 3").unwrap();
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    let fa = gather_one(&a, &dest);
    assert_eq!(fa.status, GameStatus::UpToDate);
    assert_eq!(fa.head.as_ref().unwrap().machine, "PC-DESKTOP");
    assert_eq!(home::device_sync(&fa, "PC-DESKTOP", true), DeviceSync::OnlyHere);
    // B voit la version de A : plus recent sur PC-DESKTOP.
    let fb = gather_one(&b, &dest);
    assert_eq!(fb.status, GameStatus::CloudNewer);
    assert_eq!(home::item_state(&fb, None, true, "ALLY"), (State::Incoming, Why::NewerFrom("PC-DESKTOP".into())));
    assert_eq!(home::device_sync(&fb, "ALLY", true), DeviceSync::NewerOn("PC-DESKTOP".into()));
    // B remet la version (aucune version ecrite), puis note sa position (fil d'etat de l'interface).
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    let mut sb = HashMap::new();
    sb.insert(b.game.name.clone(), vm::compute_status(&b.ctx, &b.game, Some(&dest)));
    assert_eq!(sb[&b.game.name], GameStatus::UpToDate);
    assert_eq!(home::note_positions(&dest, "ALLY", &[b.game.clone()], &sb), 1);
    assert_eq!(home::note_positions(&dest, "ALLY", &[b.game.clone()], &sb), 0, "marque ecrite UNE fois");
    // Vu de A comme de B : a jour sur 2 appareils.
    assert_eq!(home::device_sync(&gather_one(&a, &dest), "PC-DESKTOP", true), DeviceSync::UpToDateOn(2));
    assert_eq!(home::device_sync(&gather_one(&b, &dest), "ALLY", true), DeviceSync::UpToDateOn(2));
    // A modifie et recopie : A attend ALLY, B voit plus recent sur PC-DESKTOP.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(a.game.path.join("slot1.dat"), "niveau 4").unwrap();
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    assert_eq!(home::device_sync(&gather_one(&a, &dest), "PC-DESKTOP", true), DeviceSync::Waiting(vec!["ALLY".into()]));
    assert_eq!(home::device_sync(&gather_one(&b, &dest), "ALLY", true), DeviceSync::NewerOn("PC-DESKTOP".into()));
    // B remet la nouvelle version : sa marque est remplacee (l'ancienne retiree), a jour sur 2 appareils.
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    sb.insert(b.game.name.clone(), vm::compute_status(&b.ctx, &b.game, Some(&dest)));
    assert_eq!(home::note_positions(&dest, "ALLY", &[b.game.clone()], &sb), 1);
    let key = agss::cloud::dest_key(&b.game);
    assert_eq!(home::positions(&dest, &key).len(), 1, "une seule marque par appareil et par element");
    assert_eq!(home::device_sync(&gather_one(&a, &dest), "PC-DESKTOP", true), DeviceSync::UpToDateOn(2));
    // Les marques ne genent rien : liste des elements du coffre, appareils vus, versions.
    assert!(dest.join(home::DEVICES_DIR).is_dir());
    let dg = agss::cloud::list_dest_games(&dest);
    assert_eq!(dg.len(), 1, "le dossier des marques n'est pas un element : {:?}", dg.iter().map(|d| &d.name).collect::<Vec<_>>());
    assert_eq!(agss::cloud::machines_seen(&dest), ["PC-DESKTOP"]);
    assert_eq!(backup::list_versions(&dest, &a.game).len(), 2);
    assert_eq!(gather_one(&a, &dest).versions, 2);
}

#[test]
fn synchroniser_maintenant_un_element_sans_programme() {
    use agss::watch::{Logger, WatchCfg, Watcher};
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("coffre");
    std::fs::create_dir_all(&dest).unwrap();
    let a = pc(tmp.path(), "PC-DESKTOP");
    std::fs::write(a.game.path.join("notes.txt"), "v1").unwrap();
    let log = Logger::new(a.ctx.data_dir.join("logs").join("watch.log"), false);
    let triggers = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Option<String>>::new()));
    let mut w = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), WatchCfg::default(), &log);
    w.set_triggers(triggers.clone());
    w.startup(0);
    w.tick(1_000);
    w.tick(60_000);
    assert!(backup::list_versions(&dest, &a.game).is_empty(), "sans programme : jamais de synchro automatique");
    // Demande explicite (menu « ... » de l'accueil) : copie faite comme une synchro planifiee.
    triggers.lock().unwrap().push(Some(a.game.name.clone()));
    w.tick(61_000);
    w.tick(62_000);
    assert_eq!(backup::list_versions(&dest, &a.game).len(), 1, "« Synchroniser maintenant » copie un element sans programme");
    assert_eq!(vm::compute_status(&a.ctx, &a.game, Some(&dest)), GameStatus::UpToDate);
}

#[test]
fn marques_de_position_robustes() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path();
    assert!(!home::note_position(&dest.join("absent"), "PC", "jeu", "v1", 1), "coffre absent : rien");
    assert!(!home::note_position(dest, "", "jeu", "v1", 1));
    assert!(home::note_position(dest, "PC/..\\bad", "jeu", "v1:2", 1));
    assert!(home::note_position(dest, "PC/..\\bad", "autre", "v1", 1));
    let p = home::positions(dest, "jeu");
    assert_eq!(p, [("PC/..\\bad".to_string(), "v1:2".to_string())], "nom exact relu dans la marque");
    // Le nom du dossier et du fichier sont nettoyes (aucune traversee).
    for e in walkdir::WalkDir::new(dest).into_iter().filter_map(|e| e.ok()) {
        let rel = e.path().strip_prefix(dest).unwrap().to_string_lossy().to_string();
        assert!(!rel.contains(".."), "{}", rel);
    }
    assert_eq!(home::mark_file_name("hollowknight", "2026/10:26"), "hollowknight~2026_10_26.json");
    // Element sans marque.
    assert!(home::positions(dest, "rien").is_empty());
}

// ================================================================== filtres, categories

#[test]
fn filtres_et_compteurs() {
    let kinds = [Kind::Game, Kind::Game, Kind::App, Kind::Folder, Kind::Game];
    assert_eq!(home::filter_counts(&kinds), [5, 3, 1, 1]);
    assert_eq!(home::filter_counts(&[]), [0, 0, 0, 0]);
    assert!(Filter::All.matches(Kind::Folder) && Filter::Games.matches(Kind::Game) && !Filter::Games.matches(Kind::App));
    assert!(Filter::Apps.matches(Kind::App) && Filter::Folders.matches(Kind::Folder) && !Filter::Folders.matches(Kind::Game));
    assert_eq!(Filter::default(), Filter::All);
    // Categories : profil, base des jeux, sans programme = dossier.
    let mut g = Game { name: "Visual Studio Code".into(), exe: Some("Code.exe".into()), ..Default::default() };
    assert_eq!(home::kind_of(&g, Some("app"), false), Kind::App);
    assert_eq!(home::kind_of(&g, Some("config"), true), Kind::App);
    assert_eq!(home::kind_of(&g, None, true), Kind::Game);
    g.exe = Some("  ".into());
    assert_eq!(home::kind_of(&g, None, false), Kind::Folder);
    // Base reelle : un jeu connu par son nom est un jeu, meme sans programme ; un dossier perso est un dossier.
    let tmp = TempDir::new().unwrap();
    let ctx = Ctx::new(tmp.path().to_path_buf());
    let cfg = Config {
        games: vec![
            Game { name: "Documents".into(), path: tmp.path().join("docs"), ..Default::default() },
            Game { name: "Visual Studio Code".into(), path: tmp.path().join("code"), exe: Some("Code.exe".into()), ..Default::default() },
            Game { name: "Hollow Knight".into(), path: tmp.path().join("hk"), ..Default::default() },
        ],
        dest: None,
    };
    let f = home::gather(&ctx, &cfg, &Settings::default(), &HashMap::new());
    let k: Vec<Kind> = f.iter().map(|x| x.kind).collect();
    assert_eq!(k, [Kind::Folder, Kind::App, Kind::Game]);
    assert!(f.iter().all(|x| x.head.is_none() && x.status == GameStatus::Never));
}

// ================================================================== navigation au clavier, mise en page, durees

#[test]
fn navigation_au_clavier_dans_la_grille() {
    // 7 tuiles (6 + « ajouter ») sur 3 colonnes :  0 1 2 / 3 4 5 / 6
    let n = 7;
    assert_eq!(home::grid_neighbor(0, 3, n, Dir::Right), Some(1));
    assert_eq!(home::grid_neighbor(2, 3, n, Dir::Right), Some(3), "passe a la ligne suivante");
    assert_eq!(home::grid_neighbor(6, 3, n, Dir::Right), None);
    assert_eq!(home::grid_neighbor(3, 3, n, Dir::Left), Some(2));
    assert_eq!(home::grid_neighbor(0, 3, n, Dir::Left), None);
    assert_eq!(home::grid_neighbor(1, 3, n, Dir::Up), None, "premiere ligne : on remonte vers les filtres");
    assert_eq!(home::grid_neighbor(4, 3, n, Dir::Up), Some(1));
    assert_eq!(home::grid_neighbor(1, 3, n, Dir::Down), Some(4));
    assert_eq!(home::grid_neighbor(5, 3, n, Dir::Down), Some(6), "derniere ligne incomplete : derniere tuile");
    assert_eq!(home::grid_neighbor(6, 3, n, Dir::Down), Some(6));
    assert_eq!(home::grid_neighbor(4, 3, n, Dir::Home), Some(0));
    assert_eq!(home::grid_neighbor(1, 3, n, Dir::End), Some(6));
    assert_eq!(home::grid_neighbor(0, 0, 1, Dir::Down), Some(0), "colonnes nulles : pas de panique");
    assert_eq!(home::grid_neighbor(0, 3, 0, Dir::Down), None);
    assert_eq!(home::grid_neighbor(99, 3, n, Dir::Left), Some(5), "indice hors grille ramene a la fin");
}

#[test]
fn mise_en_page_de_1280_au_plein_ecran() {
    let (w, h) = ui::tokens::tile_size(false);
    let (wb, hb) = ui::tokens::tile_size(true);
    assert!(h >= 160.0 && hb > h && wb > w, "place pour la ligne des appareils");
    // 1280 x 800 (Steam Deck, Legion Go) : grand format, au moins 5 tuiles par ligne ; plein ecran 1920 : davantage.
    let deck = ui::tokens::grid_columns(1280.0 - 40.0, true);
    let full = ui::tokens::grid_columns(1920.0 - 40.0, false);
    assert!(deck >= 5 && full >= 9, "{} {}", deck, full);
    // La largeur remplit la ligne sans deformer les tuiles (+35 % au plus).
    let tw = home::tile_width(1000.0, 5, w, 12.0);
    assert!(tw >= w && tw <= w * 1.35 && 5.0 * tw + 4.0 * 12.0 <= 1000.0);
    assert_eq!(home::tile_width(100.0, 1, w, 12.0), w, "jamais plus petite que la tuile nominale");
}

#[test]
fn durees_relatives() {
    let m = 60_000;
    assert_eq!(home::ago(10_000, 10_000), Ago::JustNow);
    assert_eq!(home::ago(59_000, 0), Ago::JustNow);
    assert_eq!(home::ago(5 * m, 0), Ago::Min(5));
    assert_eq!(home::ago(61 * m, 0), Ago::Hour(1));
    assert_eq!(home::ago(25 * 60 * m, 0), Ago::Yesterday);
    assert_eq!(home::ago(72 * 60 * m, 0), Ago::Day(3));
    assert_eq!(home::ago(0, 5 * m), Ago::JustNow, "horloge en avance : jamais negatif");
}

#[test]
fn couleur_des_initiales_stable() {
    assert_eq!(home::initial_color("Celeste"), home::initial_color("celeste"));
    let all: std::collections::HashSet<[u8; 3]> = ["Celeste", "Elden Ring", "Factorio", "OBS Studio", "Minecraft", "Dolphin", "Portal 2", "Hades"].iter().map(|n| home::initial_color(n)).collect();
    assert!(all.len() >= 4, "couleurs variees");
}

// ================================================================== pause d'un element, copie en cours

#[test]
fn pause_d_un_element_compatible_avec_les_anciens_reglages() {
    let mut s = Settings::default();
    assert!(!s.is_paused("Celeste"));
    s.game_mut("Celeste").keep_versions = Some(5);
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("\"paused\""), "rien d'ecrit tant que rien n'est en pause (anciens fichiers identiques)");
    s.game_mut("Celeste").paused = true;
    assert!(s.is_paused("Celeste"));
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert!(back.is_paused("Celeste") && !back.is_paused("Hades"));
    // Ancien fichier sans le champ : pas en pause.
    let old: agss::settings::GameSettings = serde_json::from_str("{\"keep_versions\": 3}").unwrap();
    assert!(!old.paused);
}

#[test]
fn copie_en_cours_connue_par_element() {
    let tmp = TempDir::new().unwrap();
    let ctx = Ctx::new(tmp.path().to_path_buf());
    {
        let _op = agss::perf::Op::start(&ctx, "Sauvegarde", "Jeu de test lot 6");
        assert!(agss::perf::busy_games().iter().any(|g| g == "Jeu de test lot 6"));
    }
    assert!(!agss::perf::busy_games().iter().any(|g| g == "Jeu de test lot 6"), "retire a la fin");
    {
        let _op = agss::perf::Op::start(&ctx, "Analyse", "Analyse lot 6").with_min(std::time::Duration::from_secs(5));
        assert!(!agss::perf::busy_games().iter().any(|g| g == "Analyse lot 6"), "une simple analyse n'est pas une copie");
    }
}

// ================================================================== vocabulaire

#[test]
fn vocabulaire_de_l_accueil() {
    assert_eq!(tr(Lang::Fr, K::VNavHome), "Accueil");
    assert_eq!(tr(Lang::Fr, K::VNavAdd), "Ajouter");
    assert_eq!(tr(Lang::Fr, K::VNavSettings), "Réglages");
    assert_eq!(tr(Lang::Fr, K::VSyncAllNow), "Tout synchroniser maintenant");
    assert_eq!(tr(Lang::Fr, K::VUpToDateOn).replace("{}", "2"), "À jour sur 2 appareils");
    assert_eq!(tr(Lang::Fr, K::VNewerOn).replace("{}", "ALLY"), "Plus récent sur ALLY");
    assert_eq!(tr(Lang::Fr, K::VFindOnPc), "Chercher sur mon PC");
    assert_eq!(tr(Lang::Fr, K::VVaultUnreachable), "Ton coffre est injoignable");
    assert_eq!(tr(Lang::Fr, K::VAllSafe), "Tout est à l'abri");
    // Phrases d'etat courtes (12 mots au plus, ETUDE §6.1).
    for k in [K::VUpToDateOn, K::VNewerOn, K::VNewerHere, K::VOnlyHere, K::VWaitingOn, K::VPausedPlaying, K::VWaitingVault, K::VNoVault, K::VProtectionPaused] {
        for l in Lang::ALL {
            let n = tr(l, k).split_whitespace().count();
            assert!(n <= 12, "{:?} {} : {} mots", k, l.code(), n);
        }
    }
}

// ================================================================== tuile sur un vrai egui

fn draw_tile(name: &str, sync: Option<&str>) -> (bool, egui::Rect) {
    let ctx = egui::Context::default();
    let mut out = (false, egui::Rect::NOTHING);
    for t in [0.0, 1.0] {
        let raw = egui::RawInput { time: Some(t), screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))), ..Default::default() };
        let _ = ctx.run(raw, |ctx| {
            ui::install(ctx, Tokens::default_dark(), false, Motion::default());
            egui::CentralPanel::default().show(ctx, |ui| {
                let spec = ui::TileSpec { name, icon: TileIcon::Initial("C", [0x6a, 0x2d, 0x4a]), state: State::Safe, status: "À l'abri · il y a 5 min", progress: None, index: 0, epoch: 1, sync: sync.map(|s| (s, Some([0x2a, 0x9d, 0x8f]))), width: Some(200.0), heavy: None };
                let r = ui::tile(ui, "t", &spec);
                out = (r.elided, r.response.rect);
            });
        });
    }
    out
}

#[test]
fn tuile_avec_ligne_des_appareils_et_texte_complet_en_infobulle() {
    let (elided, rect) = draw_tile("Celeste", Some("À jour sur 2 appareils"));
    assert!(!elided, "texte court : rien n'est raccourci");
    assert_eq!(rect.width(), 200.0, "largeur imposee par la grille");
    assert_eq!(rect.height(), ui::tokens::tile_size(false).1);
    let (elided, _) = draw_tile("The Legend of Heroes: Trails through Daybreak II Deluxe Edition Ultimate", Some("À jour ici · en attente sur STEAMDECK-1a2b, ALLY"));
    assert!(elided, "texte trop long : signale, l'accueil montre alors le texte complet en infobulle");
}
