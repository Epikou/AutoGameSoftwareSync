//! 2.0 lot 8 : le coffre (detection des dossiers de synchronisation sur un PC SIMULE : variables, registre, fichiers de
//! configuration, lecteurs, montages ; sous-dossier AutoGameSoftwareSync ; elements d'un autre appareil et reprise dans un
//! vrai coffre temporaire) et le selecteur de dossier guide (raccourcis, dossier de depart, suggestions, apercu,
//! avertissements). UNIQUEMENT des dossiers temporaires et des PC simules : ni le vrai registre ni le vrai disque.
use agss::backup;
use agss::config::{Config, Game, State};
use agss::i18n::{tr, Lang, K};
use agss::knowledge::paths::Folders;
use agss::picker::{self as pk, Shortcut, Warning, Why};
use agss::process::ProcessChecker;
use agss::vaults::{self as vl, Candidate, DriveKind, Kind, SimEnv};
use agss::{Ctx, Res};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn kinds(v: &[Candidate]) -> Vec<Kind> {
    v.iter().map(|c| c.kind).collect()
}

fn find(v: &[Candidate], k: Kind) -> Vec<&Candidate> {
    v.iter().filter(|c| c.kind == k).collect()
}

// ================================================================== detection des coffres (Windows simule)

fn win_pc() -> SimEnv {
    let mut e = SimEnv::windows_pc("C:\\Users\\Joueur");
    e.add_drive("C:\\", DriveKind::Fixed, "Windows", "NTFS");
    e.set_var("APPDATA", "C:\\Users\\Joueur\\AppData\\Roaming");
    e.set_var("LOCALAPPDATA", "C:\\Users\\Joueur\\AppData\\Local");
    e
}

#[test]
fn google_drive_en_streaming_sur_un_lecteur_virtuel() {
    let mut e = win_pc();
    e.add_drive("G:\\", DriveKind::Fixed, "Google Drive", "FAT32");
    e.add_dir("G:\\Mon Drive");
    let v = vl::detect(&e);
    assert_eq!(v.len(), 1, "{:?}", v);
    assert_eq!(v[0].kind, Kind::GoogleDrive);
    assert_eq!(v[0].streaming, Some(true), "lecteur virtuel de Drive = mode a la demande");
    assert!(vl::needs_offline_tip(&v[0]));
    // « My Drive » (interface en anglais) sur le meme lecteur
    let mut e = win_pc();
    e.add_drive("G:\\", DriveKind::Fixed, "Google Drive", "FAT32");
    e.add_dir("G:\\My Drive");
    assert_eq!(find(&vl::detect(&e), Kind::GoogleDrive).len(), 1);
    // lecteur G: jamais enumere (lecteur absent de la liste) mais dossier present : propose quand meme
    let mut e = win_pc();
    e.add_dir("G:\\Mon Drive");
    assert_eq!(find(&vl::detect(&e), Kind::GoogleDrive).len(), 1);
}

#[test]
fn google_drive_lettre_du_registre_et_autre_langue() {
    let mut e = win_pc();
    e.add_drive("H:\\", DriveKind::Fixed, "Google Drive", "FAT32");
    e.add_dir("H:\\Mi unidad");
    e.set_reg("HKCU\\Software\\Google\\DriveFS\\Share", "MountPoint", "H:");
    let v = vl::detect(&e);
    let g = find(&v, Kind::GoogleDrive);
    assert_eq!(g.len(), 1, "{:?}", v);
    assert!(g[0].path.to_string_lossy().contains("Mi unidad"));
    // lecteur de Drive dont le dossier a un nom inconnu : la racine du lecteur est proposee
    let mut e = win_pc();
    e.add_drive("I:\\", DriveKind::Fixed, "Google Drive", "FAT32");
    e.add_dir("I:\\Autre nom");
    let v = vl::detect(&e);
    assert_eq!(find(&v, Kind::GoogleDrive).len(), 1);
    assert_eq!(find(&v, Kind::GoogleDrive)[0].streaming, Some(true));
}

#[test]
fn google_drive_en_miroir_dans_le_dossier_perso() {
    let mut e = win_pc();
    e.add_dir("C:\\Users\\Joueur\\Google Drive");
    let v = vl::detect(&e);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].kind, Kind::GoogleDrive);
    assert_eq!(v[0].streaming, Some(false), "dossier local = mode miroir : pas de conseil");
    assert!(!vl::needs_offline_tip(&v[0]));
}

#[test]
fn onedrive_variables_et_registre() {
    let mut e = win_pc();
    e.set_var("OneDrive", "C:\\Users\\Joueur\\OneDrive");
    e.set_var("OneDriveConsumer", "C:\\Users\\Joueur\\OneDrive");
    e.set_var("OneDriveCommercial", "C:\\Users\\Joueur\\OneDrive - Acme");
    e.add_dir("C:\\Users\\Joueur\\OneDrive");
    e.add_dir("C:\\Users\\Joueur\\OneDrive - Acme");
    let v = vl::detect(&e);
    let o = find(&v, Kind::OneDrive);
    assert_eq!(o.len(), 2, "le meme dossier cite deux fois n'apparait qu'une fois : {:?}", o);
    assert!(o.iter().any(|c| c.work), "OneDriveCommercial = compte professionnel");
    assert!(o.iter().any(|c| !c.work));
    // registre : comptes OneDrive
    let mut e = win_pc();
    e.set_reg("HKCU\\Software\\Microsoft\\OneDrive\\Accounts\\Business1", "UserFolder", "D:\\OD Travail");
    e.set_reg("HKCU\\Software\\Microsoft\\OneDrive\\Accounts\\Personal", "UserFolder", "D:\\OD Perso");
    e.add_dir("D:\\OD Travail");
    e.add_dir("D:\\OD Perso");
    let o = vl::detect(&e);
    assert_eq!(find(&o, Kind::OneDrive).len(), 2);
    assert!(find(&o, Kind::OneDrive).iter().any(|c| c.work && c.path.to_string_lossy().contains("Travail")));
    // variable qui pointe vers un dossier absent : rien n'est propose
    let mut e = win_pc();
    e.set_var("OneDrive", "C:\\Users\\Joueur\\OneDrive");
    assert!(vl::detect(&e).is_empty());
}

#[test]
fn dropbox_info_json_personnel_et_professionnel() {
    let mut e = win_pc();
    e.add_file("C:\\Users\\Joueur\\AppData\\Local\\Dropbox\\info.json", r#"{"personal":{"path":"D:\\Dropbox","host":1},"business":{"path":"D:\\Dropbox (Acme)"}}"#);
    e.add_dir("D:\\Dropbox");
    e.add_dir("D:\\Dropbox (Acme)");
    let v = vl::detect(&e);
    let d = find(&v, Kind::Dropbox);
    assert_eq!(d.len(), 2);
    assert!(d.iter().any(|c| c.work));
    // info.json dans %APPDATA% aussi ; json casse : ignore
    let mut e = win_pc();
    e.add_file("C:\\Users\\Joueur\\AppData\\Roaming\\Dropbox\\info.json", r#"{"personal":{"path":"E:\\Dropbox"}}"#);
    e.add_dir("E:\\Dropbox");
    assert_eq!(find(&vl::detect(&e), Kind::Dropbox).len(), 1);
    let mut e = win_pc();
    e.add_file("C:\\Users\\Joueur\\AppData\\Roaming\\Dropbox\\info.json", "{ pas du json");
    assert!(vl::detect(&e).is_empty());
}

#[test]
fn autres_services_icloud_mega_pcloud_nextcloud_syncthing() {
    let mut e = win_pc();
    for d in ["iCloudDrive", "MEGA", "pCloud Sync", "Nextcloud"] {
        e.add_dir(&format!("C:\\Users\\Joueur\\{}", d));
    }
    // Nextcloud : dossier supplementaire lu dans nextcloud.cfg
    e.add_file("C:\\Users\\Joueur\\AppData\\Roaming\\Nextcloud\\nextcloud.cfg", "[Accounts]\n0\\Folders\\1\\localPath=D:/NC Jeux/\n");
    e.add_dir("D:\\NC Jeux");
    // Syncthing : dossiers lus dans config.xml (le 2e n'existe pas)
    e.add_file("C:\\Users\\Joueur\\AppData\\Local\\Syncthing\\config.xml", "<configuration><folder id=\"a\" label=\"Jeux\" path=\"D:\\Sync Jeux\" type=\"sendreceive\"></folder><folder id=\"b\" path=\"D:\\Absent\"></folder></configuration>");
    e.add_dir("D:\\Sync Jeux");
    // pCloud Drive (lecteur virtuel)
    e.add_drive("P:\\", DriveKind::Fixed, "pCloud Drive", "FAT32");
    let v = vl::detect(&e);
    assert_eq!(find(&v, Kind::ICloud).len(), 1);
    assert_eq!(find(&v, Kind::Mega).len(), 1);
    assert_eq!(find(&v, Kind::PCloud).len(), 2, "dossier de synchro et lecteur virtuel");
    assert_eq!(find(&v, Kind::Nextcloud).len(), 2);
    assert_eq!(find(&v, Kind::Syncthing).len(), 1);
    // ordre d'affichage : suit Kind::ORDER
    let ks = kinds(&v);
    let mut sorted = ks.clone();
    sorted.sort_by_key(|k| Kind::ORDER.iter().position(|x| x == k).unwrap());
    assert_eq!(ks, sorted);
}

#[test]
fn lecteurs_reseau_nas_et_usb() {
    let mut e = win_pc();
    e.add_drive("Z:\\", DriveKind::Network, "NAS", "NTFS");
    e.add_drive("E:\\", DriveKind::Removable, "CLE", "exFAT");
    e.add_drive("D:\\", DriveKind::Fixed, "Donnees", "NTFS");
    e.add_drive("F:\\", DriveKind::Optical, "", "");
    let v = vl::detect(&e);
    assert_eq!(kinds(&v), [Kind::Network, Kind::Usb], "les disques fixes et optiques ne sont pas des coffres tout trouves : {:?}", v);
    assert_eq!(v[0].path, PathBuf::from("Z:\\"));
}

#[test]
fn pc_sans_rien_et_ordre_google_drive_en_tete() {
    assert!(vl::detect(&win_pc()).is_empty());
    let mut e = win_pc();
    e.add_drive("Z:\\", DriveKind::Network, "NAS", "NTFS");
    e.add_dir("C:\\Users\\Joueur\\Dropbox");
    e.add_drive("G:\\", DriveKind::Fixed, "Google Drive", "FAT32");
    e.add_dir("G:\\Mon Drive");
    e.set_var("OneDrive", "C:\\Users\\Joueur\\OneDrive");
    e.add_dir("C:\\Users\\Joueur\\OneDrive");
    let v = vl::detect(&e);
    assert_eq!(kinds(&v), [Kind::GoogleDrive, Kind::OneDrive, Kind::Dropbox, Kind::Network]);
}

// ================================================================== Linux simule

#[test]
fn linux_dossiers_usuels_et_points_de_montage() {
    let mut e = SimEnv::linux_pc("/home/deck");
    e.add_dir("/home/deck/Nextcloud");
    e.add_dir("/home/deck/Dropbox");
    e.add_dir("/home/deck/Sync");
    e.add_dir("/home/deck/MEGA");
    e.add_mount("/mnt/nas", "cifs");
    e.add_mount("/run/media/deck/CLE", "vfat");
    e.add_mount("/boot/efi", "vfat");
    e.add_mount("/", "ext4");
    let v = vl::detect(&e);
    assert_eq!(kinds(&v), [Kind::Dropbox, Kind::Mega, Kind::Nextcloud, Kind::Syncthing, Kind::Network, Kind::Usb], "{:?}", v);
    assert!(v.iter().all(|c| c.streaming.is_none()), "pas de mode streaming sous Linux");
}

#[test]
fn linux_dropbox_info_et_syncthing_config() {
    let mut e = SimEnv::linux_pc("/home/a");
    e.add_file("/home/a/.dropbox/info.json", r#"{"personal":{"path":"/data/Dropbox"}}"#);
    e.add_dir("/data/Dropbox");
    e.add_file("/home/a/.local/state/syncthing/config.xml", "<configuration><folder id=\"x\" path=\"~/Parties\"></folder></configuration>");
    e.add_dir("/home/a/Parties");
    let v = vl::detect(&e);
    assert_eq!(find(&v, Kind::Dropbox)[0].path, PathBuf::from("/data/Dropbox"));
    assert_eq!(find(&v, Kind::Syncthing)[0].path, PathBuf::from("/home/a/Parties"));
}

#[test]
fn type_du_coffre_actuel_d_apres_son_chemin() {
    let mut e = win_pc();
    e.add_drive("G:\\", DriveKind::Fixed, "Google Drive", "FAT32");
    e.add_dir("G:\\Mon Drive");
    e.add_dir("C:\\Users\\Joueur\\Dropbox");
    let found = vl::detect(&e);
    assert_eq!(vl::kind_of(Path::new("G:\\Mon Drive\\AutoGameSoftwareSync"), &found), (Kind::GoogleDrive, Some(true)));
    assert_eq!(vl::kind_of(Path::new("C:\\Users\\Joueur\\Dropbox"), &found).0, Kind::Dropbox);
    assert_eq!(vl::kind_of(Path::new("D:\\Jeux\\Coffre"), &found), (Kind::Other, None));
    // un frere dont le nom commence pareil n'est pas « dedans »
    assert_eq!(vl::kind_of(Path::new("C:\\Users\\Joueur\\Dropbox2"), &found).0, Kind::Other);
}

#[test]
fn candidats_serialises_en_json_pour_les_verifications_manuelles() {
    let c = Candidate { kind: Kind::GoogleDrive, path: PathBuf::from("X:\\Mon Drive"), streaming: Some(true), work: false };
    let j = serde_json::to_string(&vec![c.clone()]).unwrap();
    let back: Vec<Candidate> = serde_json::from_str(&j).unwrap();
    assert_eq!(back, vec![c]);
    // champs facultatifs absents
    let back: Vec<Candidate> = serde_json::from_str(r#"[{"kind":"Usb","path":"E:\\"}]"#).unwrap();
    assert_eq!(back[0].streaming, None);
    assert!(!back[0].work);
}

// ================================================================== sous-dossier du coffre

#[test]
fn sous_dossier_autogamesoftwaresync() {
    let tmp = TempDir::new().unwrap();
    let drive = tmp.path().join("Mon Drive");
    std::fs::create_dir_all(&drive).unwrap();
    // coffre neuf : sous-dossier cree dedans, marqueur ecrit
    let v = vl::prepare(&drive).unwrap();
    assert_eq!(v, drive.join("AutoGameSoftwareSync"));
    assert!(v.is_dir());
    let marker = v.join(vl::MARKER);
    assert!(marker.is_file());
    // deuxieme appel : meme dossier, le marqueur n'est pas reecrit
    let before = std::fs::read(&marker).unwrap();
    std::fs::write(&marker, b"modifie a la main").unwrap();
    assert_eq!(vl::prepare(&drive).unwrap(), v);
    assert_eq!(std::fs::read(&marker).unwrap(), b"modifie a la main", "jamais ecrase");
    let _ = before;
    // choisir le sous-dossier lui-meme : pas d'imbrication
    assert_eq!(vl::prepare(&v).unwrap(), v);
    assert!(!v.join("AutoGameSoftwareSync").exists());
    // dossier inexistant : refuse, rien cree
    let nope = tmp.path().join("absent");
    assert!(vl::prepare(&nope).is_err());
    assert!(!nope.exists());
}

#[test]
fn ancien_dossier_partage_deja_rempli_n_est_pas_imbrique() {
    let tmp = TempDir::new().unwrap();
    let old = tmp.path().join("Saves");
    // un element de l'ancien format (dossier avec `saves/`)
    std::fs::create_dir_all(old.join("hollowknight").join("saves")).unwrap();
    assert_eq!(vl::target_for(&old), old, "les copies existantes restent a leur place");
    assert_eq!(vl::prepare(&old).unwrap(), old);
    assert!(!old.join("AutoGameSoftwareSync").exists());
    // dossier vide : sous-dossier ; sous-dossier deja la : repris
    let empty = tmp.path().join("vide");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(vl::target_for(&empty), empty.join("AutoGameSoftwareSync"));
    std::fs::create_dir_all(empty.join("AutoGameSoftwareSync")).unwrap();
    assert_eq!(vl::target_for(&empty), empty.join("AutoGameSoftwareSync"));
    // casse differente du nom
    let low = tmp.path().join("autogamesoftwaresync");
    std::fs::create_dir_all(&low).unwrap();
    assert_eq!(vl::target_for(&low), low);
}

#[test]
fn espace_libre_connu_pour_un_dossier_existant() {
    let tmp = TempDir::new().unwrap();
    let f = vl::free_space(tmp.path());
    assert!(f.map(|b| b > 0).unwrap_or(false), "espace libre : {:?}", f);
}

// ================================================================== elements venant d'un autre appareil + reprise

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

struct Pc {
    ctx: Ctx,
    root: PathBuf,
}

fn pc(root: &Path, name: &str) -> Pc {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    Pc { ctx, root: root.to_path_buf() }
}

/// L'appareil `p` protege `name` (fichier `slot.dat`) dans `vault` ; `tpl` : gabarit de dossier ecrit dans le meta.json.
fn protect(p: &Pc, vault: &Path, name: &str, content: &str, tpl: Option<&str>) {
    let dir = p.root.join(format!("src-{}-{}", p.ctx.machine, name.replace(' ', "")));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("slot.dat"), content).unwrap();
    let g = Game { name: name.into(), path: dir, exe: Some(format!("{}.exe", name.replace(' ', ""))), ..Default::default() };
    backup::backup(&p.ctx, &g, vault).unwrap();
    // gabarit universel : ecrit apres coup (les indices ne font pas partie de l'empreinte du contenu)
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
        std::fs::write(&mp, serde_json::to_string(&v).unwrap()).unwrap();
    }
}

fn keys_of(cfg: &Config) -> HashSet<String> {
    cfg.games.iter().map(|g| agss::cloud::game_key(&g.name)).collect()
}

#[test]
fn elements_trouves_d_un_autre_appareil_dans_le_coffre() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let epigos = pc(tmp.path(), "EPIGOS");
    let deck = pc(tmp.path(), "DECK");
    protect(&epigos, &vault, "Hollow Knight", "niveau 3", Some("%LOCALAPPDATA%/Team Cherry/Hollow Knight"));
    protect(&epigos, &vault, "Celeste", "chap 2", Some("%LOCALAPPDATA%/Celeste"));
    protect(&deck, &vault, "Stardew Valley", "an 2", None);
    let me = "EPIKOU";
    let f = vl::found_from_others(&vault, me, &HashSet::new());
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].machine, "EPIGOS", "l'appareil le plus fourni en premier");
    assert_eq!(f[0].items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Celeste", "Hollow Knight"]);
    assert_eq!(f[1].machine, "DECK");
    // ce qui est deja sur ce PC n'est pas propose
    let mut keys = HashSet::new();
    keys.insert(agss::cloud::game_key("Celeste"));
    let f = vl::found_from_others(&vault, me, &keys);
    assert_eq!(f[0].items.len(), 1);
    // ses propres copies ne comptent pas comme « autre appareil »
    assert!(vl::found_from_others(&vault, "EPIGOS", &HashSet::new()).iter().all(|d| d.machine != "EPIGOS"));
    assert_eq!(vl::other_devices(&vault, me), ["DECK", "EPIGOS"]);
    // coffre neuf ou inexistant : rien
    assert!(vl::found_from_others(&tmp.path().join("neuf"), me, &HashSet::new()).is_empty());
}

#[test]
fn reprise_lie_et_restaure_sans_rien_ecraser() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let epigos = pc(tmp.path(), "EPIGOS");
    protect(&epigos, &vault, "Hollow Knight", "niveau 3", Some("%LOCALAPPDATA%/Team Cherry/Hollow Knight"));
    protect(&epigos, &vault, "Celeste", "chap 2", Some("%LOCALAPPDATA%/Celeste"));
    protect(&epigos, &vault, "Sans Indice", "x", None);
    // les deux dernieres sont sur un dossier qui contient deja une partie locale (a ne pas ecraser)
    let me = pc(tmp.path(), "EPIKOU");
    let local = tmp.path().join("local");
    std::fs::create_dir_all(local.join("Celeste")).unwrap();
    std::fs::write(local.join("Celeste").join("ma-partie.dat"), "chap 9 (la mienne)").unwrap();
    let kf = agss::hints::KnownFolders { local_app_data: Some(local.clone()), ..Default::default() };
    let found = vl::found_from_others(&vault, "EPIKOU", &HashSet::new());
    assert_eq!(found[0].items.len(), 3);
    let rep = vl::resume(&me.ctx, &vault, &found[0].items, &kf, &[]);
    assert_eq!(rep.added, ["Celeste", "Hollow Knight"].map(String::from).to_vec(), "{:?}", rep);
    assert_eq!(rep.restored, ["Hollow Knight"], "restaure seulement ce qui etait absent ou vide : {:?}", rep);
    assert_eq!(rep.to_finish, ["Sans Indice"], "sans emplacement connu : a terminer a la main");
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    // Hollow Knight : dossier cree, derniere version remise
    assert_eq!(std::fs::read_to_string(local.join("Team Cherry").join("Hollow Knight").join("slot.dat")).unwrap(), "niveau 3");
    // Celeste : la partie locale est intacte et rien n'a ete ajoute dedans
    assert_eq!(std::fs::read_to_string(local.join("Celeste").join("ma-partie.dat")).unwrap(), "chap 9 (la mienne)");
    assert!(!local.join("Celeste").join("slot.dat").exists());
    // la configuration contient les deux elements, lies a leur dossier du coffre
    let cfg = Config::load(&me.ctx).unwrap();
    assert_eq!(cfg.games.len(), 2);
    assert!(keys_of(&cfg).contains("hollowknight") && keys_of(&cfg).contains("celeste"));
    assert!(cfg.games.iter().all(|g| g.dest_key.is_none()), "meme nom = meme dossier du coffre");
    // rien n'a ete supprime ni modifie dans le coffre (versions toujours la)
    assert_eq!(agss::cloud::list_dest_games(&vault).len(), 3);
    // reprise relancee : plus rien a ajouter (deja la)
    let again = vl::resume(&me.ctx, &vault, &found[0].items, &kf, &[]);
    assert!(again.added.is_empty(), "{:?}", again);
}

#[test]
fn reprise_un_nom_different_reste_lie_au_meme_dossier_du_coffre() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let epigos = pc(tmp.path(), "EPIGOS");
    protect(&epigos, &vault, "Hollow Knight", "n", Some("%APPDATA%/HK"));
    let me = pc(tmp.path(), "EPIKOU");
    let kf = agss::hints::KnownFolders { roaming_app_data: Some(tmp.path().join("roaming")), ..Default::default() };
    std::fs::create_dir_all(tmp.path().join("roaming")).unwrap();
    // la cle du coffre peut differer du nom (dossier ecrit avec un autre nom) : le lien est conserve
    let dg = agss::cloud::list_dest_games(&vault).remove(0);
    let item = vl::Item { key: dg.key.clone(), name: dg.name.clone() };
    let rep = vl::resume(&me.ctx, &vault, &[item], &kf, &[]);
    assert_eq!(rep.added.len(), 1);
    let cfg = Config::load(&me.ctx).unwrap();
    assert_eq!(agss::cloud::dest_key(&cfg.games[0]), dg.key);
}

// ================================================================== selecteur de dossier : raccourcis et depart

fn windows_folders(root: &Path) -> Folders {
    let f = Folders::under(root);
    for d in [&f.local, &f.roaming, &f.locallow, &f.documents, &f.saved_games, &f.profile].into_iter().flatten() {
        std::fs::create_dir_all(d).unwrap();
    }
    f
}

fn exists(p: &Path) -> bool {
    p.exists()
}

#[test]
fn raccourcis_windows_et_linux_avec_proton() {
    let tmp = TempDir::new().unwrap();
    let f = windows_folders(tmp.path());
    std::fs::create_dir_all(f.documents.as_ref().unwrap().join("My Games")).unwrap();
    let s = pk::shortcuts(&f, &[], true, &exists);
    let k: Vec<Shortcut> = s.iter().map(|x| x.kind).collect();
    assert_eq!(k, [Shortcut::SavedGames, Shortcut::MyGames, Shortcut::Documents, Shortcut::AppDataRoaming, Shortcut::AppDataLocal, Shortcut::LocalLow]);
    // un dossier absent n'est pas propose
    std::fs::remove_dir(f.saved_games.as_ref().unwrap()).unwrap();
    assert!(!pk::shortcuts(&f, &[], true, &exists).iter().any(|x| x.kind == Shortcut::SavedGames));
    // Linux + prefixes Proton
    let pfx = tmp.path().join("steam/compatdata/1245620/pfx");
    std::fs::create_dir_all(agss::proton::user_dir(&pfx).join("AppData")).unwrap();
    let lf = Folders { xdg_data: Some(tmp.path().join("home/.local/share")), xdg_config: Some(tmp.path().join("home/.config")), documents: Some(tmp.path().join("home/Documents")), home: Some(tmp.path().join("home")), ..Default::default() };
    for d in [lf.xdg_data.as_ref(), lf.xdg_config.as_ref(), lf.documents.as_ref()].into_iter().flatten() {
        std::fs::create_dir_all(d).unwrap();
    }
    let s = pk::shortcuts(&lf, &[pfx], false, &exists);
    let k: Vec<Shortcut> = s.iter().map(|x| x.kind).collect();
    assert_eq!(k, [Shortcut::ConfigDir, Shortcut::ShareDir, Shortcut::Documents, Shortcut::Proton, Shortcut::Home]);
    assert_eq!(s[3].tag, "1245620", "le prefixe Proton est nomme d'apres l'identifiant du jeu");
}

#[test]
fn dossier_de_depart_le_plus_probable() {
    let tmp = TempDir::new().unwrap();
    let f = windows_folders(tmp.path());
    let local = f.local.clone().unwrap();
    let low = f.locallow.clone().unwrap();
    let hint = |names: &[&str], found: &[PathBuf]| pk::Hint { names: names.iter().map(|s| s.to_string()).collect(), found: found.to_vec() };
    // rien de connu : AppData (Local)
    assert_eq!(pk::start_dir(&hint(&["Celeste"], &[]), &f, &[], true, &exists), local);
    // %LOCALAPPDATA%\<Editeur> existe : on y est
    std::fs::create_dir_all(local.join("Team Cherry")).unwrap();
    assert_eq!(pk::start_dir(&hint(&["Team Cherry, Inc.", "Hollow Knight"], &[]), &f, &[], true, &exists), local.join("Team Cherry"), "forme juridique retiree");
    // l'editeur est cherche avant le produit, et Local avant LocalLow
    std::fs::create_dir_all(low.join("Hollow Knight")).unwrap();
    assert_eq!(pk::start_dir(&hint(&["Team Cherry", "Hollow Knight"], &[]), &f, &[], true, &exists), local.join("Team Cherry"));
    assert_eq!(pk::start_dir(&hint(&["Hollow Knight"], &[]), &f, &[], true, &exists), low.join("Hollow Knight"));
    // sans espaces
    std::fs::create_dir_all(f.roaming.as_ref().unwrap().join("SuperGiant")).unwrap();
    assert_eq!(pk::start_dir(&hint(&["Super Giant"], &[]), &f, &[], true, &exists), f.roaming.clone().unwrap().join("SuperGiant"));
    // un emplacement trouve par la connaissance passe avant tout
    let found = tmp.path().join("trouve");
    std::fs::create_dir_all(&found).unwrap();
    assert_eq!(pk::start_dir(&hint(&["Team Cherry"], &[found.clone(), local.clone()]), &f, &[], true, &exists), found);
    // trouve mais absent : on continue
    assert_eq!(pk::start_dir(&hint(&["Team Cherry"], &[tmp.path().join("absent")]), &f, &[], true, &exists), local.join("Team Cherry"));
    // sans AppData Local : Roaming, puis Documents
    let mut g = f.clone();
    g.local = None;
    assert_eq!(pk::start_dir(&hint(&[], &[]), &g, &[], true, &exists), f.roaming.clone().unwrap());
    g.roaming = None;
    assert_eq!(pk::start_dir(&hint(&[], &[]), &g, &[], true, &exists), f.documents.clone().unwrap());
}

#[test]
fn dossier_de_depart_linux_et_proton() {
    let tmp = TempDir::new().unwrap();
    let lf = Folders { xdg_data: Some(tmp.path().join("share")), xdg_config: Some(tmp.path().join("config")), home: Some(tmp.path().to_path_buf()), ..Default::default() };
    std::fs::create_dir_all(lf.xdg_data.as_ref().unwrap()).unwrap();
    std::fs::create_dir_all(lf.xdg_config.as_ref().unwrap()).unwrap();
    let hint = |n: &str| pk::Hint { names: vec![n.into()], found: vec![] };
    assert_eq!(pk::start_dir(&hint("Celeste"), &lf, &[], false, &exists), tmp.path().join("share"), "sinon ~/.local/share");
    std::fs::create_dir_all(tmp.path().join("config").join("Celeste")).unwrap();
    assert_eq!(pk::start_dir(&hint("Celeste"), &lf, &[], false, &exists), tmp.path().join("config").join("Celeste"));
    // dossier de l'editeur dans le prefixe Proton d'un jeu Windows
    let pfx = tmp.path().join("compat/1/pfx");
    let hk = agss::proton::user_dir(&pfx).join("AppData").join("LocalLow").join("Team Cherry");
    std::fs::create_dir_all(&hk).unwrap();
    assert_eq!(pk::start_dir(&hint("Team Cherry"), &lf, &[pfx], false, &exists), hk);
}

#[test]
fn dossiers_recents_modifies_par_le_programme() {
    let tmp = TempDir::new().unwrap();
    let mk = |n: &str| {
        let p = tmp.path().join(n);
        std::fs::create_dir_all(&p).unwrap();
        p
    };
    let (a, b, c) = (mk("a"), mk("b"), mk("c"));
    std::fs::write(c.join("f.sav"), "x").unwrap();
    let games = vec![
        Game { name: "A".into(), path: a.clone(), ..Default::default() },
        Game { name: "B".into(), path: b.clone(), ..Default::default() },
        Game { name: "C".into(), path: c.join("f.sav"), save_is_file: true, ..Default::default() },
        Game { name: "Jamais".into(), path: mk("d"), ..Default::default() },
        Game { name: "Absent".into(), path: tmp.path().join("absent"), ..Default::default() },
    ];
    let mut st = State::default();
    st.last_backup_ms.insert("A".into(), 100);
    st.last_restore_ms.insert("B".into(), 300);
    st.last_sync_ms.insert("C".into(), 200);
    st.last_backup_ms.insert("Absent".into(), 999);
    let r = pk::recent_dirs(&games, &st, 3, &exists);
    assert_eq!(r.iter().map(|s| s.path.clone()).collect::<Vec<_>>(), vec![b.clone(), c.clone(), a.clone()], "le plus recent d'abord, un fichier devient son dossier, ni jamais copie ni absent");
    assert!(r.iter().all(|s| s.why == Why::Recent));
    assert_eq!(pk::recent_dirs(&games, &st, 1, &exists).len(), 1);
    // suggestions : trouves d'abord (3 au plus), puis recents, sans doublon
    let found = vec![a.clone(), tmp.path().join("absent"), b.clone()];
    let s = pk::suggestions(&found, r, &exists);
    assert_eq!(s.iter().map(|x| (x.path.clone(), x.why)).collect::<Vec<_>>(), vec![(a, Why::Found), (b, Why::Found), (c, Why::Recent)]);
}

#[test]
fn chemin_en_miettes_et_filtre_expert() {
    let tmp = TempDir::new().unwrap();
    let f = Folders::under(tmp.path());
    let p = f.locallow.clone().unwrap().join("Team Cherry").join("Hollow Knight");
    let (base, rest) = pk::friendly_parts(&p, &f);
    assert_eq!(base, Some(Shortcut::LocalLow), "la base la plus precise gagne");
    assert_eq!(rest, ["Team Cherry", "Hollow Knight"]);
    let (base, rest) = pk::friendly_parts(Path::new("D:\\Jeux\\X"), &f);
    assert_eq!(base, None);
    assert_eq!(rest.last().unwrap(), "X");
    assert_eq!(pk::friendly_parts(&f.documents.clone().unwrap().join("My Games").join("Fallout"), &f).0, Some(Shortcut::MyGames));
    // filtre du mode expert : extensions pour la boite du systeme
    assert_eq!(pk::filter_exts("*.sav ; Profil*.dat, *.SAV, nom"), ["sav", "dat"]);
    assert!(pk::filter_exts("").is_empty());
}

// ================================================================== apercu et avertissements

fn write(p: &Path, content: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

#[test]
fn apercu_fichiers_taille_et_date() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path().join("Celeste");
    write(&d.join("a.sav"), &[0u8; 1000]);
    write(&d.join("sous").join("b.sav"), &[0u8; 2000]);
    let pv = pk::scan(&d, 100);
    assert!(pv.exists && !pv.is_file && !pv.truncated);
    assert_eq!((pv.files, pv.bytes, pv.sensitive), (2, 3000, 0));
    let age = agss::util::now_ms().saturating_sub(pv.newest_ms.unwrap());
    assert!(age < 60_000, "derniere modification recente : {} ms", age);
    // fichier seul
    let one = pk::scan(&d.join("a.sav"), 100);
    assert!(one.is_file && one.files == 1 && one.bytes == 1000);
    // absent
    assert!(!pk::scan(&tmp.path().join("rien"), 100).exists);
    // plafond : apercu arrete, nombres = minimums
    for i in 0..30 {
        write(&d.join("many").join(format!("{}.dat", i)), b"x");
    }
    let cut = pk::scan(&d, 10);
    assert!(cut.truncated && cut.files == 10);
}

#[test]
fn apercu_ignore_la_liste_noire() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path().join("App");
    write(&d.join("settings.json"), b"{}");
    write(&d.join(".ssh").join("id_rsa"), b"cle");
    write(&d.join("Profile").join("Cookies"), b"c");
    write(&d.join("secret.pem"), b"p");
    let pv = pk::scan(&d, 100);
    assert_eq!(pv.files, 1, "seul settings.json compte");
    assert!(pv.sensitive >= 3, "{:?}", pv);
}

fn ctx_for<'a>(f: &'a Folders, vault: Option<&'a Path>, windows: bool) -> pk::WarnCtx<'a> {
    pk::WarnCtx { folders: f, vault, windows, system_dirs: if windows { vec![PathBuf::from("C:\\Windows"), PathBuf::from("C:\\Program Files"), PathBuf::from("C:\\Program Files (x86)")] } else { vec![] } }
}

fn pv(files: u64, bytes: u64) -> pk::Preview {
    pk::Preview { exists: true, files, bytes, newest_ms: Some(1), ..Default::default() }
}

#[test]
fn avertissement_dossier_enorme_a_partir_de_2_go() {
    let f = Folders::under(Path::new("C:\\t"));
    let c = ctx_for(&f, None, true);
    let p = Path::new("D:\\Jeux\\Gros");
    const GO: u64 = 1024 * 1024 * 1024;
    assert!(pk::warnings(p, &pv(10, 500 * 1024 * 1024), &c).is_empty(), "au seuil de l'alerte poids (500 Mo) : rien");
    assert_eq!(pk::warnings(p, &pv(10, 500 * 1024 * 1024 + 1), &c), [Warning::Heavy { bytes: 500 * 1024 * 1024 + 1, at_least: false }], "au-dessus du seuil : alerte poids");
    assert_eq!(pk::warnings(p, &pv(10, 2 * GO - 1), &c), [Warning::Heavy { bytes: 2 * GO - 1, at_least: false }], "juste en dessous de 2 Go : lourd, pas encore enorme");
    assert_eq!(pk::warnings(p, &pv(10, 2 * GO), &c), [Warning::Huge { bytes: 2 * GO, at_least: false }]);
    // apercu interrompu avec 20 000 fichiers : « au moins »
    let mut big = pv(pk::PREVIEW_MAX_FILES as u64, GO);
    big.truncated = true;
    assert!(matches!(pk::warnings(p, &big, &c)[..], [Warning::Huge { at_least: true, .. }]));
    // le texte annonce la taille
    assert!(tr(Lang::Fr, K::VWarnHuge).contains("{}") && tr(Lang::Fr, K::VWarnHuge).contains("très gros"));
}

#[test]
fn avertissement_dossier_systeme_et_dossier_trop_large() {
    let tmp = TempDir::new().unwrap();
    let f = Folders::under(tmp.path());
    let c = ctx_for(&f, None, true);
    let ok = pv(3, 100);
    for p in ["C:\\Windows", "C:\\Windows\\System32\\config", "C:\\Program Files\\Steam", "C:\\Program Files (x86)", "D:\\", "C:\\$Recycle.Bin"] {
        assert_eq!(pk::warnings(Path::new(p), &ok, &c), [Warning::System], "{}", p);
    }
    // dossier perso, AppData entier : trop large (pas « systeme »)
    for p in [f.profile.clone().unwrap(), f.local.clone().unwrap(), f.roaming.clone().unwrap(), f.locallow.clone().unwrap()] {
        assert_eq!(pk::warnings(&p, &ok, &c), [Warning::TooBroad], "{}", p.display());
    }
    // un vrai dossier de jeu : aucun avertissement
    assert!(pk::warnings(&f.local.clone().unwrap().join("Celeste"), &ok, &c).is_empty());
    assert!(pk::warnings(Path::new("C:\\Users\\Joueur\\Documents\\My Games\\X"), &ok, &c).is_empty());
    // Linux
    let lc = ctx_for(&f, None, false);
    for p in ["/", "/usr/share", "/etc", "/var/log", "/boot"] {
        assert_eq!(pk::warnings(Path::new(p), &ok, &lc), [Warning::System], "{}", p);
    }
    assert!(pk::warnings(Path::new("/run/media/deck/CLE/Saves"), &ok, &lc).is_empty(), "un disque USB monte n'est pas un dossier systeme");
    assert!(pk::warnings(Path::new("/home/deck/.local/share/Celeste"), &ok, &lc).is_empty());
}

#[test]
fn avertissement_liste_noire_et_coffre_lui_meme() {
    let tmp = TempDir::new().unwrap();
    let f = Folders::under(tmp.path());
    let vault = PathBuf::from("G:\\Mon Drive\\AutoGameSoftwareSync");
    let c = ctx_for(&f, Some(&vault), true);
    // le dossier lui-meme est un emplacement de la liste noire (profil de navigateur)
    let chrome = f.local.clone().unwrap().join("Google").join("Chrome").join("User Data");
    assert_eq!(pk::warnings(&chrome, &pv(5, 10), &c), [Warning::Sensitive]);
    assert_eq!(pk::warnings(Path::new("C:\\Users\\Joueur\\.ssh"), &pv(1, 10), &c), [Warning::Sensitive]);
    // il contient des fichiers de la liste noire : ils seront ignores
    let mut inside = pv(5, 10);
    inside.sensitive = 2;
    assert_eq!(pk::warnings(Path::new("D:\\Donnees\\App"), &inside, &c), [Warning::SensitiveInside(2)]);
    // le coffre lui-meme ou un dossier a l'interieur : refuse
    let w = pk::warnings(&vault, &pv(5, 10), &c);
    assert!(w.contains(&Warning::IsVault) && w.iter().any(|x| x.blocks()));
    let w = pk::warnings(&vault.join("hollowknight"), &pv(5, 10), &c);
    assert!(w.contains(&Warning::IsVault));
    assert!(!pk::warnings(Path::new("G:\\Mon Drive\\Autre"), &pv(5, 10), &c).contains(&Warning::IsVault));
    // seul « coffre lui-meme » bloque
    assert!(!Warning::Huge { bytes: 1, at_least: false }.blocks() && !Warning::System.blocks() && !Warning::Sensitive.blocks());
    // dossier vide
    let empty = pk::Preview { exists: true, ..Default::default() };
    assert_eq!(pk::warnings(Path::new("D:\\Vide"), &empty, &c), [Warning::Empty]);
}

// ================================================================== textes et absence de regression

#[test]
fn textes_du_coffre_et_du_selecteur_dans_les_5_langues() {
    use agss::i18n::{tr_raw, SIMPLE_KEYS};
    let keys = [
        K::VVaultPickTitle, K::VVaultPickBody, K::VVaultDetected, K::VVaultChoose, K::VVaultNetwork, K::VVaultUsb, K::VVaultOther, K::VVaultNoneFound, K::VVaultNote,
        K::VVaultReady, K::VVaultStreamTag, K::VVaultStreamTitle, K::VVaultStreamBody, K::VVaultFoundOne, K::VVaultFoundMany, K::VVaultFoundBody, K::VVaultFoundYes,
        K::VVaultResumeReport, K::VVaultChangeTitle, K::VVaultChangeBody, K::VPickTitleFor, K::VPickPreviewOne, K::VPickPreviewMany, K::VWarnHuge, K::VWarnSystem,
        K::VWarnBroad, K::VWarnSensitive, K::VWarnSensitiveInside, K::VWarnVault, K::PickFilterHint,
    ];
    for l in Lang::ALL {
        for k in keys {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr_raw(l, k) };
            assert!(!s.trim().is_empty(), "{:?} vide en {}", k, l.code());
            if !matches!(k, K::PickFilterHint) {
                assert!(SIMPLE_KEYS.contains(&k), "{:?} doit etre du mode simple", k);
            }
        }
        // remplacements attendus par l'interface
        let many = if l == Lang::Zh { agss::i18n_zh::tr(K::VVaultFoundMany) } else { tr_raw(l, K::VVaultFoundMany) };
        assert!(many.contains("{0}") && many.contains("{1}"), "{}", l.code());
        let one = if l == Lang::Zh { agss::i18n_zh::tr(K::VVaultFoundOne) } else { tr_raw(l, K::VVaultFoundOne) };
        assert!(one.contains("{1}") && !one.contains("{0}"), "{}", l.code());
        let pm = if l == Lang::Zh { agss::i18n_zh::tr(K::VPickPreviewMany) } else { tr_raw(l, K::VPickPreviewMany) };
        assert!(pm.contains("{0}") && pm.contains("{1}") && pm.contains("{2}"), "{}", l.code());
    }
    // textes de reference
    assert_eq!(tr(Lang::Fr, K::VVaultFoundMany).replace("{0}", "12").replace("{1}", "EPIGOS"), "On a trouvé 12 éléments de EPIGOS : tu veux les retrouver ici ?");
    assert_eq!(tr(Lang::Fr, K::VPickTitleFor).replace("{}", "Celeste"), "Où Celeste range-t-il ses données ?");
    assert!(tr(Lang::Fr, K::VVaultStreamBody).contains("Disponible hors connexion") && tr(Lang::Fr, K::VVaultStreamBody).contains("miroir"));
}

#[test]
fn aucun_selecteur_du_systeme_direct_pour_choisir_un_emplacement() {
    // Tous les ecrans qui choisissent un EMPLACEMENT passent par le selecteur guide ; seuls restent les dialogues du
    // systeme du selecteur lui-meme, du choix de dossier du coffre, des programmes (.exe), de l'export / import et du champ
    // du chemin saisi a la main du mode expert (Reglages).
    let gui = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("gui");
    for f in ["manual_ui.rs", "program_form_ui.rs", "detect_ui.rs", "fiche_ui.rs", "detail_ui.rs", "common.rs", "profiles_ui.rs"] {
        let s = std::fs::read_to_string(gui.join(f)).unwrap();
        assert!(!s.contains(".pick_folder()"), "{} ouvre encore la boite du systeme pour un dossier", f);
        assert!(f == "manual_ui.rs" || s.contains("open_picker"), "{} n utilise pas le selecteur guide", f);
    }
    let picker = std::fs::read_to_string(gui.join("picker_ui.rs")).unwrap();
    assert!(picker.contains("set_directory"), "la boite du systeme doit s'ouvrir dans le dossier le plus probable");
}

#[test]
fn le_coffre_n_utilise_que_des_noms_propres_dans_les_tuiles() {
    for k in Kind::ORDER {
        match k {
            Kind::Network | Kind::Usb | Kind::Other => assert!(k.brand().is_none()),
            _ => assert!(k.brand().is_some(), "{:?}", k),
        }
        assert!(!k.code().is_empty());
    }
}

#[test]
fn google_drive_dossier_declare_dans_la_configuration_de_drive() {
    // « Mon Drive » en miroir, dans un dossier choisi par l'utilisateur : lu dans la base de preferences de Drive
    let mut e = win_pc();
    e.add_dir("D:\\Mon Google");
    e.drivefs_roots.push(PathBuf::from("D:\\Mon Google"));
    let v = vl::detect(&e);
    assert_eq!(v.len(), 1, "{:?}", v);
    assert_eq!((v[0].kind, v[0].streaming), (Kind::GoogleDrive, Some(false)));
    // declare sous un lecteur virtuel de Drive : mode a la demande
    let mut e = win_pc();
    e.add_drive("G:\\", DriveKind::Fixed, "Google Drive", "FAT32");
    e.add_dir("G:\\Mon Drive");
    e.drivefs_roots.push(PathBuf::from("G:\\Mon Drive"));
    let v = vl::detect(&e);
    assert_eq!(v.len(), 1, "le meme dossier n'apparait qu'une fois : {:?}", v);
    assert_eq!(v[0].streaming, Some(true));
    // base absente ou illisible : rien, sans erreur
    assert!(vl::drivefs_roots_from_db(Path::new("Z:\\n-existe-pas\\root_preference_sqlite.db")).is_empty());
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("x.db"), b"pas une base").unwrap();
    assert!(vl::drivefs_roots_from_db(&tmp.path().join("x.db")).is_empty());
}
