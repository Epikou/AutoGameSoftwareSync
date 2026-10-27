//! 2.0 lot 12 : Nouveau PC (migration). Appareils du coffre, traduction des emplacements entre PC (autre utilisateur,
//! Windows -> Linux / Proton, Linux -> Windows), etats (pret, a installer, a choisir, conflit, deja la), recuperation en
//! lot puis annulation sans perte, garde « programme en cours », recuperation differee a l'installation, inventaire
//! cloud-safe des logiciels.
use agss::backup;
use agss::config::{Config, Game};
use agss::detect::{Installed, Store};
use agss::hints::KnownFolders;
use agss::migrate::{self as mg, inventory as inv, Here, Known, Link, Outcome, Status, Tr};
use agss::process::ProcessChecker;
use agss::{Ctx, Res};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

struct Procs(Arc<AtomicBool>);
impl ProcessChecker for Procs {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct Pc {
    ctx: Ctx,
    running: Arc<AtomicBool>,
}

fn pc(root: &Path, name: &str) -> Pc {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    let running = Arc::new(AtomicBool::new(false));
    ctx.procs = Box::new(Procs(running.clone()));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    Pc { ctx, running }
}

fn write(p: &Path, b: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b).unwrap();
}

/// Dossiers connus d'un utilisateur Windows simule `<racine>/Users/<nom>`.
fn win_kf(root: &Path, user: &str) -> KnownFolders {
    let u = root.join("Users").join(user);
    KnownFolders { local_app_data: Some(u.join("AppData").join("Local")), roaming_app_data: Some(u.join("AppData").join("Roaming")), local_low: Some(u.join("AppData").join("LocalLow")), documents: Some(u.join("Documents")), saved_games: Some(u.join("Saved Games")) }
}

/// L'ancien PC protege `name` (dossier `dir`, programme `exe`) ; les indices du meta.json sont reecrits avec `tpl`
/// (gabarit universel) et `os` (comme sur l'appareil d'origine).
fn protect(p: &Pc, vault: &Path, name: &str, dir: &Path, exe: Option<&str>, tpl: Option<&str>, os: &str) -> Game {
    let g = Game { name: name.into(), path: dir.to_path_buf(), exe: exe.map(String::from), ..Default::default() };
    backup::backup(&p.ctx, &g, vault).unwrap();
    let root = agss::cloud::list_dest_games(vault).into_iter().find(|d| d.name == name).unwrap().dirs[0].clone();
    for e in std::fs::read_dir(root.join("saves")).unwrap().flatten() {
        let mp = e.path().join("meta.json");
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&mp).unwrap()).unwrap();
        match tpl {
            Some(t) => v["save_path_tpl"] = serde_json::Value::String(t.to_string()),
            None => {
                v.as_object_mut().unwrap().remove("save_path_tpl");
            }
        }
        v["main_hint"]["os"] = serde_json::Value::String(os.to_string());
        std::fs::write(&mp, serde_json::to_string(&v).unwrap()).unwrap();
    }
    g
}

fn device<'a>(ds: &'a [mg::Device], name: &str) -> &'a mg::Device {
    ds.iter().find(|d| d.name == name).unwrap_or_else(|| panic!("appareil {} absent : {:?}", name, ds))
}

fn plan_of<'a>(plans: &'a [mg::Plan], name: &str) -> &'a mg::Plan {
    plans.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{} absent : {:?}", name, plans.iter().map(|p| (&p.name, p.status)).collect::<Vec<_>>()))
}

/// Toutes les donnees d'un dossier : (chemin relatif, octets).
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).map(|e| (e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read(e.path()).unwrap())).collect();
    v.sort();
    v
}

// ================================================================== appareils

#[test]
fn appareils_du_coffre_avec_dates_nombres_et_marques() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let old = pc(tmp.path(), "ANCIEN");
    let deck = pc(tmp.path(), "DECK");
    for (p, n) in [(&old, "Celeste"), (&old, "Hades"), (&deck, "Balatro")] {
        let d = tmp.path().join(format!("src-{}", n));
        write(&d.join("slot.dat"), n.as_bytes());
        protect(p, &vault, n, &d, Some(&format!("{}.exe", n)), None, "windows");
    }
    // DECK a seulement recupere Celeste : sa marque de position compte aussi
    let key = agss::cloud::game_key("Celeste");
    assert!(agss::home::note_position(&vault, "DECK", &key, "v1", 1_700_000_000_000));
    let ds = mg::devices(&vault, "NOUVEAU", &HashSet::new());
    assert_eq!(ds.len(), 2);
    let a = device(&ds, "ANCIEN");
    assert_eq!((a.items(), a.todo), (2, 2));
    assert!(a.last_ms.is_some());
    let d = device(&ds, "DECK");
    assert_eq!(d.items(), 2, "Balatro + la marque de Celeste : {:?}", d);
    assert!(!a.same_as_here);
    // deja sur ce PC : plus a faire
    let mut local = HashSet::new();
    local.insert(agss::cloud::game_key("Hades"));
    assert_eq!(device(&mg::devices(&vault, "NOUVEAU", &local), "ANCIEN").todo, 1);
    // PC reinstalle sous le meme nom : l'ancien appareil reste propose
    assert!(device(&mg::devices(&vault, "ancien", &HashSet::new()), "ANCIEN").same_as_here);
    // coffre vide ou inexistant : aucun appareil
    assert!(mg::devices(&tmp.path().join("neuf"), "X", &HashSet::new()).is_empty());
}

// ================================================================== traduction des emplacements

#[test]
fn traduction_vers_un_autre_nom_d_utilisateur() {
    let tmp = TempDir::new().unwrap();
    let alice = win_kf(tmp.path(), "Alice");
    let bob = win_kf(tmp.path(), "Bob");
    let src = alice.local_app_data.clone().unwrap().join("Team Cherry").join("Hollow Knight");
    let tpl = agss::hints::to_template(&src, &alice).expect("gabarit universel");
    assert_eq!(tpl, "%LOCALAPPDATA%/Team Cherry/Hollow Knight");
    assert!(!tpl.contains("Alice"), "aucun nom d'utilisateur dans un gabarit");
    let mut here = Here::empty(true);
    here.kf = bob.clone();
    assert_eq!(mg::translate(&tpl, "Hollow Knight", Some("hollow_knight.exe"), &here), Tr::Path(bob.local_app_data.unwrap().join("Team Cherry").join("Hollow Knight")));
    // Documents redirige (OneDrive) : le gabarit suit le dossier connu de ce PC
    here.kf.documents = Some(tmp.path().join("OneDrive").join("Documents"));
    assert_eq!(mg::translate("%DOCUMENTS%/My Games/Skyrim", "Skyrim", Some("skyrim.exe"), &here), Tr::Path(tmp.path().join("OneDrive").join("Documents").join("My Games").join("Skyrim")));
    // gabarit qui sortirait de sa base, base inconnue : refuse
    assert_eq!(mg::translate("%APPDATA%/../../x", "x", None, &here), Tr::Unknown);
    assert_eq!(mg::translate("%INCONNU%/x", "x", None, &here), Tr::Unknown);
}

#[test]
fn traduction_windows_vers_linux_et_proton() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home").join("bob");
    let mut here = Here::empty(false);
    here.kf = KnownFolders { local_app_data: Some(home.join(".local").join("share")), roaming_app_data: Some(home.join(".config")), local_low: None, documents: Some(home.join("Documents")), saved_games: None };
    // programme Windows sans prefixe Proton : il faut l'installer d'abord
    assert_eq!(mg::translate("%APPDATA%/Jeu", "Jeu", Some("Jeu.exe"), &here), Tr::NeedProgram);
    // prefixe ou le dossier existe deja
    let pfx = tmp.path().join("steam").join("steamapps").join("compatdata").join("4242").join("pfx");
    let inside = pfx.join("drive_c").join("users").join("steamuser").join("AppData").join("Roaming").join("Jeu");
    std::fs::create_dir_all(&inside).unwrap();
    here.prefixes = vec![pfx.clone()];
    assert_eq!(mg::translate("%APPDATA%/Jeu", "Jeu", Some("Jeu.exe"), &here), Tr::Path(inside.clone()));
    // jeu Steam installe (AppID connu) dont le dossier n'existe pas encore : dans SON prefixe
    here.prefixes.clear();
    here.steam_libs = vec![tmp.path().join("steam")];
    here.launchers = vec![Installed { name: "Jeu".into(), store: Store::Steam, id: "4242".into(), install_dir: tmp.path().join("steam").join("steamapps").join("common").join("Jeu"), exe: None }];
    let want = pfx.join("drive_c").join("users").join("steamuser").join("AppData").join("Local").join("Studio").join("Jeu");
    assert_eq!(mg::translate("%LOCALAPPDATA%/Studio/Jeu", "Jeu", Some("Jeu.exe"), &here), Tr::Path(want));
    // logiciel sans programme Windows (reglages, dossier perso) : dossiers natifs de l'utilisateur Linux
    assert_eq!(mg::translate("%APPDATA%/Code/User", "Visual Studio Code", None, &here), Tr::Path(home.join(".config").join("Code").join("User")));
    assert_eq!(mg::translate("%DOCUMENTS%/Notes", "Notes", None, &here), Tr::Path(home.join("Documents").join("Notes")));
    // pas d'equivalent natif (LocalLow) : a choisir
    assert_eq!(mg::translate("%LOCALLOW%/Studio/Jeu", "Jeu", None, &here), Tr::Unknown);
}

#[test]
fn traduction_linux_vers_windows() {
    let tmp = TempDir::new().unwrap();
    // Sauvegarde ecrite sous Linux par un jeu Windows lance par Proton : gabarit Windows
    let pfx_path = PathBuf::from("/home/alice/.local/share/Steam/steamapps/compatdata/70/pfx/drive_c/users/steamuser/AppData/Roaming/Jeu/Saves");
    let tpl = agss::proton::template_from_prefix_path(&pfx_path).expect("gabarit depuis un prefixe");
    assert_eq!(tpl, "%APPDATA%/Jeu/Saves");
    let bob = win_kf(tmp.path(), "Bob");
    let mut here = Here::empty(true);
    here.kf = bob.clone();
    assert_eq!(mg::translate(&tpl, "Jeu", Some("Jeu.exe"), &here), Tr::Path(bob.roaming_app_data.clone().unwrap().join("Jeu").join("Saves")));
    // Reglages d'un logiciel Linux (~/.config = %APPDATA% par convention) : retrouves dans AppData\Roaming
    assert_eq!(mg::translate("%APPDATA%/obs-studio", "OBS Studio", None, &here), Tr::Path(bob.roaming_app_data.unwrap().join("obs-studio")));
}

// ================================================================== verification complete d'un coffre

/// Coffre a deux appareils : l'ancien PC (utilisateur Alice) a 6 elements ; le nouveau PC (Bob) est vide.
struct World {
    tmp: TempDir,
    vault: PathBuf,
    _old: Pc,
    new: Pc,
    bob: KnownFolders,
}

fn world() -> World {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let old = pc(tmp.path(), "ANCIEN");
    let new = pc(tmp.path(), "NOUVEAU");
    let alice = win_kf(tmp.path(), "Alice");
    let bob = win_kf(tmp.path(), "Bob");
    let la = alice.local_app_data.clone().unwrap();
    let ra = alice.roaming_app_data.clone().unwrap();
    let hk = la.join("Team Cherry").join("Hollow Knight");
    write(&hk.join("user1.dat"), b"\x00\x01partie hollow");
    write(&hk.join("sub").join("deep.bin"), &[7u8; 3000]);
    protect(&old, &vault, "Hollow Knight", &hk, Some("hollow_knight.exe"), Some("%LOCALAPPDATA%/Team Cherry/Hollow Knight"), "windows");
    let ce = la.join("Celeste");
    write(&ce.join("0.celeste"), b"chapitre 7");
    protect(&old, &vault, "Celeste", &ce, Some("Celeste.exe"), Some("%LOCALAPPDATA%/Celeste"), "windows");
    let er = ra.join("EldenRing");
    write(&er.join("ER0000.sl2"), b"elden");
    protect(&old, &vault, "Elden Ring", &er, Some("eldenring.exe"), Some("%APPDATA%/EldenRing"), "windows");
    let un = tmp.path().join("D").join("Jeux").join("JeuUnity");
    write(&un.join("save.json"), b"{}");
    protect(&old, &vault, "Jeu Unity", &un, Some("JeuUnity.exe"), None, "windows");
    let st = ra.join("StardewValley");
    write(&st.join("Saves").join("Ferme"), b"ancienne ferme");
    protect(&old, &vault, "Stardew Valley", &st, Some("Stardew Valley.exe"), Some("%APPDATA%/StardewValley"), "windows");
    let docs = alice.documents.clone().unwrap();
    write(&docs.join("cv.txt"), b"mon cv");
    protect(&old, &vault, "Documents", &docs, None, None, "windows");
    World { tmp, vault, _old: old, new, bob }
}

/// Le nouveau PC : programmes installes (sauf Elden Ring), Documents connu par son nom stable.
fn here_of(w: &World) -> Here {
    let mut here = Here::empty(true);
    here.kf = w.bob.clone();
    here.apps = vec!["Hollow Knight".into(), "Celeste".into(), "Jeu Unity".into(), "Stardew Valley".into()];
    here.personal = vec![Known { stable: "Documents".into(), path: w.bob.documents.clone().unwrap(), is_file: false, filter: None, exclude: None, os_specific: false }];
    here
}

#[test]
fn verification_etats_pret_installer_choisir_conflit_et_deja_la() {
    let w = world();
    let bob_la = w.bob.local_app_data.clone().unwrap();
    let bob_ra = w.bob.roaming_app_data.clone().unwrap();
    // Celeste : deja present ici, identique ; Stardew : deja present ici et PLUS RECENT (conflit)
    write(&bob_la.join("Celeste").join("0.celeste"), b"chapitre 7");
    let fresh = bob_ra.join("StardewValley").join("Saves").join("Ferme");
    write(&fresh, b"ma ferme d'ici, plus recente");
    filetime_bump(&fresh);
    let here = here_of(&w);
    let ds = mg::devices(&w.vault, "NOUVEAU", &HashSet::new());
    let plans = mg::check(&w.vault, device(&ds, "ANCIEN"), &HashSet::new(), &here, None, &|_, _| {});
    assert_eq!(plans.len(), 6);
    let hk = plan_of(&plans, "Hollow Knight");
    assert_eq!(hk.status, Status::Ready);
    assert_eq!(hk.main.as_ref().unwrap().path, bob_la.join("Team Cherry").join("Hollow Knight"), "dossier traduit vers l'utilisateur Bob");
    assert!(hk.checked);
    assert_eq!(plan_of(&plans, "Celeste").status, Status::AlreadyHere);
    let er = plan_of(&plans, "Elden Ring");
    assert_eq!(er.status, Status::NotInstalled, "programme pas encore installe");
    assert!(er.checked, "la copie est gardee et remise a l'installation");
    let un = plan_of(&plans, "Jeu Unity");
    assert_eq!(un.status, Status::Choose, "aucun indice d'emplacement : a choisir");
    assert!(un.checked);
    let sv = plan_of(&plans, "Stardew Valley");
    assert_eq!(sv.status, Status::Conflict);
    assert!(!sv.checked, "tout est coche sauf les conflits");
    assert!(sv.local_newest_ms.is_some());
    let docs = plan_of(&plans, "Documents");
    assert_eq!(docs.status, Status::Ready);
    assert_eq!(docs.main.as_ref().unwrap().path, w.bob.documents.clone().unwrap(), "dossier perso retrouve par son nom stable");
    assert_eq!(docs.personal.as_deref(), Some("Documents"));
    // taille totale des elements coches (conflit exclu)
    let total: u64 = plans.iter().filter(|p| p.checked).map(|p| p.bytes).sum();
    assert_eq!(mg::checked_bytes(&plans), total);
    assert!(total > 3000);
    // emplacement choisi au selecteur guide : reverifie
    let mut un = un.clone();
    let chosen = w.tmp.path().join("E").join("JeuUnity");
    mg::choose_place(&mut un, chosen.clone(), false);
    assert_eq!(un.status, Status::Ready);
    assert_eq!(un.main.unwrap().path, chosen);
    // Elden Ring installe ensuite : pret
    let mut here2 = here_of(&w);
    here2.apps.push("ELDEN RING".into());
    let plans2 = mg::check(&w.vault, device(&ds, "ANCIEN"), &HashSet::new(), &here2, None, &|_, _| {});
    assert_eq!(plan_of(&plans2, "Elden Ring").status, Status::Ready);
}

/// Date de modification d'un fichier dans le futur (plus recent que la version du coffre).
fn filetime_bump(p: &Path) {
    let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600)).unwrap();
}

// ================================================================== recuperation en lot, annulation, garde

#[test]
fn recuperation_en_lot_puis_arret_et_annulation_sans_aucune_perte() {
    let w = world();
    let bob_la = w.bob.local_app_data.clone().unwrap();
    // Hollow Knight : une ancienne partie est deja la (elle doit survivre dans la copie de securite, puis revenir)
    let hk_dir = bob_la.join("Team Cherry").join("Hollow Knight");
    write(&hk_dir.join("user1.dat"), b"vieille partie locale");
    write(&hk_dir.join("perso.txt"), b"fichier local seulement");
    // plus anciens que la version du coffre : pret (pas un conflit)
    for f in ["user1.dat", "perso.txt"] {
        let h = std::fs::OpenOptions::new().write(true).open(hk_dir.join(f)).unwrap();
        h.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86400)).unwrap();
    }
    let before_hk = snapshot(&hk_dir);
    let vault_before = snapshot(&w.vault);
    let here = here_of(&w);
    let ds = mg::devices(&w.vault, "NOUVEAU", &HashSet::new());
    let mut plans = mg::check(&w.vault, device(&ds, "ANCIEN"), &HashSet::new(), &here, None, &|_, _| {});
    // deux elements prets : Documents puis Hollow Knight (ordre alphabetique)
    assert_eq!(plan_of(&plans, "Hollow Knight").status, Status::Ready, "partie locale plus ancienne : prete");
    plans.retain(|p| p.name == "Hollow Knight" || p.name == "Documents");
    assert_eq!(plans.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Documents", "Hollow Knight"]);
    let mut ctx = Ctx::new(w.new.ctx.data_dir.clone());
    ctx.machine = "NOUVEAU".into();
    ctx.procs = Box::new(Procs(w.new.running.clone()));
    // arret demande apres le premier element : le second n'est pas touche
    let run = mg::Run::new();
    let r2 = run.clone();
    mg::run(&mut ctx, &w.vault, &plans, "ANCIEN", &run, &move |i| {
        if i == 1 {
            r2.request_cancel();
        }
    });
    let st = run.state.lock().unwrap();
    assert!(st.finished && st.cancelled);
    assert_eq!(st.results.len(), 1, "annulable entre deux elements");
    let first = &plans[0];
    assert_eq!(st.results[0].outcome, Outcome::Restored, "{:?}", st.results[0].outcome);
    let untouched = &plans[1];
    let ut_dir = untouched.main.as_ref().unwrap().path.clone();
    let ut_before: Vec<(String, Vec<u8>)> = if untouched.name == "Hollow Knight" { before_hk.clone() } else { vec![] };
    assert_eq!(snapshot(&ut_dir), ut_before, "element suivant non touche");
    // l'element recupere est lie a ce PC pour la synchro future
    let cfg = Config::load(&ctx).unwrap();
    assert_eq!(cfg.games.len(), 1);
    assert_eq!(cfg.games[0].name, first.name);
    assert!(cfg.games[0].exe.is_some() || cfg.games[0].schedule.is_some(), "sans programme : copie planifiee");
    // le coffre n'a pas bouge
    assert_eq!(snapshot(&w.vault), vault_before);
    // tout recuperer, puis annuler : retour a l'identique, octet par octet
    let results_first = st.results.clone();
    drop(st);
    let _ = mg::undo(&ctx, &w.vault, &results_first);
    let run = mg::Run::new();
    mg::run(&mut ctx, &w.vault, &plans, "ANCIEN", &run, &|_| {});
    let st = run.state.lock().unwrap();
    assert_eq!(st.results.iter().filter(|r| r.outcome == Outcome::Restored).count(), 2);
    // Hollow Knight remis : fichiers du coffre ecrits, fichier local seulement garde
    assert_eq!(std::fs::read(hk_dir.join("user1.dat")).unwrap(), b"\x00\x01partie hollow");
    assert_eq!(std::fs::read(hk_dir.join("sub").join("deep.bin")).unwrap(), vec![7u8; 3000]);
    assert_eq!(std::fs::read(hk_dir.join("perso.txt")).unwrap(), b"fichier local seulement");
    let hk_res = st.results.iter().find(|r| r.name == "Hollow Knight").unwrap();
    assert!(hk_res.ticket.as_ref().unwrap().safety_dir.is_some(), "copie de securite avant de remettre");
    let rep = mg::undo(&ctx, &w.vault, &st.results);
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    assert_eq!(snapshot(&hk_dir), before_hk, "annulation : etat d'avant, octet par octet");
    assert!(!w.bob.documents.clone().unwrap().join("cv.txt").exists(), "fichier ajoute retire par l'annulation");
    assert!(Config::load(&ctx).unwrap().games.is_empty(), "elements retires de la configuration");
    assert_eq!(snapshot(&w.vault), vault_before, "le coffre n'est jamais modifie");
}

#[test]
fn jamais_pendant_que_le_programme_tourne() {
    let w = world();
    let here = here_of(&w);
    let ds = mg::devices(&w.vault, "NOUVEAU", &HashSet::new());
    let plans = mg::check(&w.vault, device(&ds, "ANCIEN"), &HashSet::new(), &here, None, &|_, _| {});
    let hk = plan_of(&plans, "Hollow Knight").clone();
    assert_eq!(mg::running_programs(&[hk.clone()], &|e| e == "hollow_knight.exe"), ["Hollow Knight"]);
    assert!(mg::running_programs(&[hk.clone()], &|_| false).is_empty());
    w.new.running.store(true, Ordering::SeqCst);
    let r = mg::apply(&w.new.ctx, &w.vault, &hk, "ANCIEN");
    assert_eq!(r.outcome, Outcome::Busy);
    assert!(!hk.main.as_ref().unwrap().path.exists(), "rien n'est ecrit");
    assert!(Config::load(&w.new.ctx).unwrap().games.is_empty());
    w.new.running.store(false, Ordering::SeqCst);
    assert_eq!(mg::apply(&w.new.ctx, &w.vault, &hk, "ANCIEN").outcome, Outcome::Restored);
}

#[test]
fn conflit_lie_sans_rien_ecraser_et_les_deux_versions_gardees() {
    let w = world();
    let bob_ra = w.bob.roaming_app_data.clone().unwrap();
    let fresh = bob_ra.join("StardewValley").join("Saves").join("Ferme");
    write(&fresh, b"ma ferme d'ici, plus recente");
    filetime_bump(&fresh);
    let here = here_of(&w);
    let ds = mg::devices(&w.vault, "NOUVEAU", &HashSet::new());
    let plans = mg::check(&w.vault, device(&ds, "ANCIEN"), &HashSet::new(), &here, None, &|_, _| {});
    let mut sv = plan_of(&plans, "Stardew Valley").clone();
    assert_eq!(sv.status, Status::Conflict);
    sv.checked = true;
    let r = mg::apply(&w.new.ctx, &w.vault, &sv, "ANCIEN");
    assert_eq!(r.outcome, Outcome::Linked);
    assert_eq!(std::fs::read(&fresh).unwrap(), b"ma ferme d'ici, plus recente", "la version d'ici n'est jamais ecrasee");
    // la synchro suivante garde les deux : branche de conflit, aucune version supprimee
    let g = Config::load(&w.new.ctx).unwrap().games.remove(0);
    assert_eq!(backup::plan(&w.new.ctx, &g, &w.vault).unwrap(), backup::Plan::Conflict);
}

// ================================================================== recuperation differee

#[test]
fn recuperation_differee_quand_le_programme_est_installe() {
    let w = world();
    let mut here = here_of(&w);
    let ds = mg::devices(&w.vault, "NOUVEAU", &HashSet::new());
    let plans = mg::check(&w.vault, device(&ds, "ANCIEN"), &HashSet::new(), &here, None, &|_, _| {});
    let er = plan_of(&plans, "Elden Ring").clone();
    assert_eq!(mg::apply(&w.new.ctx, &w.vault, &er, "ANCIEN").outcome, Outcome::Deferred);
    let p = mg::load_pending(&w.new.ctx.data_dir);
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].name.as_str(), p[0].source.as_str()), ("Elden Ring", "ANCIEN"));
    let dest = w.bob.roaming_app_data.clone().unwrap().join("EldenRing");
    assert!(!dest.exists(), "rien n'est cree tant que le programme n'est pas la");
    // pas encore installe : aucune carte
    assert!(mg::pending_ready(&w.vault, &w.new.ctx.data_dir, &HashSet::new(), &here).is_empty());
    // le programme apparait : la carte propose de recuperer, la recuperation remet la version
    here.apps.push("Elden Ring".into());
    let ready = mg::pending_ready(&w.vault, &w.new.ctx.data_dir, &HashSet::new(), &here);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].1.status, Status::Ready);
    assert_eq!(mg::apply(&w.new.ctx, &w.vault, &ready[0].1, "ANCIEN").outcome, Outcome::Restored);
    assert_eq!(std::fs::read(dest.join("ER0000.sl2")).unwrap(), b"elden");
    assert!(mg::load_pending(&w.new.ctx.data_dir).is_empty(), "plus rien en attente");
    // un element ajoute a la main entre-temps est oublie
    let er2 = plan_of(&plans, "Elden Ring").clone();
    mg::add_pending(&w.new.ctx.data_dir, &er2, "ANCIEN").unwrap();
    let mut local = HashSet::new();
    local.insert(er2.key.clone());
    assert!(mg::pending_ready(&w.vault, &w.new.ctx.data_dir, &local, &here).is_empty());
    assert!(mg::load_pending(&w.new.ctx.data_dir).is_empty());
}

// ================================================================== liens et inventaire

#[test]
fn pages_des_boutiques_et_recherche_web() {
    assert_eq!(Link::Steam("367520".into()).url("Hollow Knight"), "https://store.steampowered.com/app/367520");
    assert_eq!(Link::Steam("367520".into()).store(), Some("Steam"));
    assert!(Link::Gog("1".into()).url("The Witcher 3").starts_with("https://www.gog.com/"));
    assert!(Link::Epic("x".into()).url("Hades").starts_with("https://store.epicgames.com/"));
    let web = Link::Web.url("Mon Logiciel & Co");
    assert!(web.starts_with("https://duckduckgo.com/?q=Mon+Logiciel+%26+Co+download"), "{}", web);
    assert_eq!(Link::Web.store(), None);
    // identifiant note par l'ancien PC
    let i = inv::build("ANCIEN", "20261026", "windows", vec![], vec![], vec![inv::GameRef { name: "Hades".into(), store: "epic".into(), id: "Min".into() }], &[]);
    assert_eq!(mg::link_for("Hades", None, Some(&i), None), Link::Epic("Min".into()));
    assert_eq!(mg::link_for("Inconnu", None, Some(&i), None), Link::Web);
}

#[test]
fn inventaire_cloud_safe_un_par_jour_trois_gardes_sans_chemin() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let users = vec!["alice".to_string()];
    let mk = |day: &str| {
        inv::build(
            "ANCIEN",
            day,
            "windows",
            vec!["Mozilla Firefox".into(), "C:\\Users\\alice\\Outil".into(), "Dossier d'alice".into(), "7-Zip".into(), "7-zip".into()],
            vec!["Mozilla.Firefox".into(), "7zip.7zip".into(), "pas un id".into()],
            vec![inv::GameRef { name: "Hollow Knight".into(), store: "Steam".into(), id: "367520".into() }],
            &users,
        )
    };
    // aucun chemin, aucun nom d'utilisateur, doublons retires
    let i = mk("20261020");
    assert_eq!(i.apps, ["7-Zip", "Mozilla Firefox"]);
    assert_eq!(i.winget, ["7zip.7zip", "Mozilla.Firefox"]);
    assert_eq!(i.games[0].store, "steam");
    let text = serde_json::to_string(&i).unwrap();
    assert!(!text.to_lowercase().contains("alice") && !text.contains(":\\"), "{}", text);
    // un fichier par jour, jamais reecrit
    assert!(inv::needs_today(&vault, "ANCIEN", "20261020"));
    assert!(inv::write(&vault, &i).unwrap());
    let f = inv::device_dir(&vault, "ANCIEN").join("20261020.json");
    let first = std::fs::read(&f).unwrap();
    let mut changed = mk("20261020");
    changed.apps.push("Autre".into());
    assert!(!inv::write(&vault, &changed).unwrap(), "deja ecrit aujourd'hui : rien");
    assert_eq!(std::fs::read(&f).unwrap(), first, "jamais reecrit");
    assert!(!inv::needs_today(&vault, "ANCIEN", "20261020"));
    // les inventaires d'un autre appareil ne sont jamais touches
    let other = inv::build("DECK", "20261001", "linux", vec!["GIMP".into()], vec![], vec![], &[]);
    assert!(inv::write(&vault, &other).unwrap());
    for d in ["20261021", "20261022", "20261023", "20261024"] {
        assert!(inv::write(&vault, &mk(d)).unwrap());
    }
    let mut left: Vec<String> = std::fs::read_dir(inv::device_dir(&vault, "ANCIEN")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    left.sort();
    assert_eq!(left, ["20261022.json", "20261023.json", "20261024.json"], "les 3 plus recents");
    assert!(inv::device_dir(&vault, "DECK").join("20261001.json").exists());
    assert_eq!(inv::latest(&vault, "ANCIEN").unwrap().day, "20261024");
    // le dossier des inventaires n'est pas un element du coffre
    assert!(agss::cloud::list_dest_games(&vault).is_empty());
    assert!(mg::devices(&vault, "X", &HashSet::new()).is_empty());
    // winget : identifiants lus dans l'export, commandes montrees (rien n'est lance)
    let export = r#"{"Sources":[{"Packages":[{"PackageIdentifier":"Mozilla.Firefox"},{"PackageIdentifier":"Valve.Steam"},{"PackageIdentifier":"bad id"}]}]}"#;
    assert_eq!(inv::parse_winget_export(export), ["Mozilla.Firefox", "Valve.Steam"]);
    assert!(inv::parse_winget_export("pas du json").is_empty());
    assert_eq!(inv::winget_lines(&["Valve.Steam".into(), "x & del".into()]), ["winget install --exact --id Valve.Steam"]);
}

#[test]
fn indices_de_l_emplacement_principal_ecrits_par_les_nouvelles_versions() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    let p = pc(tmp.path(), "ANCIEN");
    let dir = tmp.path().join("Bureau");
    write(&dir.join("a.lnk"), b"x");
    write(&dir.join("b.txt"), b"y");
    let g = Game { name: "Desktop shortcuts".into(), path: dir.clone(), save_filter: Some("*.lnk;*.url".into()), ..Default::default() };
    backup::backup(&p.ctx, &g, &vault).unwrap();
    let m = backup::active_meta(&vault, &g).unwrap();
    let h = m.main_hint.clone().expect("indices ecrits");
    assert_eq!(h.filter.as_deref(), Some("*.lnk;*.url"));
    assert!(!h.is_file);
    assert_eq!(h.os, agss::store::this_os());
    assert_eq!(mg::source_os(&m), agss::store::this_os());
    // un meta.json d'une version precedente (sans indices) reste lisible
    let mut v: serde_json::Value = serde_json::to_value(&m).unwrap();
    v.as_object_mut().unwrap().remove("main_hint");
    let old: agss::store::Meta = serde_json::from_value(v).unwrap();
    assert!(old.main_hint.is_none());
    assert_eq!(mg::source_os(&old), "windows", "deduit des raccourcis .lnk");
    // element propre au systeme venu d'un autre systeme : a choisir
    let mut here = Here::empty(!cfg!(windows));
    here.personal = vec![Known { stable: "Desktop shortcuts".into(), path: tmp.path().join("ici"), is_file: false, filter: Some("*.desktop".into()), exclude: None, os_specific: true }];
    let dg = agss::cloud::list_dest_games(&vault).remove(0);
    let plan = mg::plan_item(&vault, &dg, &here, None).unwrap();
    assert_eq!(plan.status, Status::Choose);
    // meme systeme : retrouve, avec le filtre de ce PC
    let mut here = Here::empty(cfg!(windows));
    here.personal = vec![Known { stable: "Desktop shortcuts".into(), path: tmp.path().join("ici"), is_file: false, filter: Some("*.lnk;*.url".into()), exclude: None, os_specific: true }];
    let plan = mg::plan_item(&vault, &dg, &here, None).unwrap();
    assert_eq!(plan.status, Status::Ready);
    assert_eq!(plan.main.as_ref().unwrap().filter.as_deref(), Some("*.lnk;*.url"));
    let g2 = mg::to_game(&plan).unwrap();
    assert_eq!(g2.save_filter.as_deref(), Some("*.lnk;*.url"));
    assert!(g2.schedule.is_some(), "sans programme : copie chaque jour a midi");
    // evenement d'usage en tranches seulement
    assert_eq!(mg::usage_props("end", 23, 2, 75), [("step", "end"), ("items", "21+"), ("remaining", "1-5"), ("duration", "1-3m")]);
}
