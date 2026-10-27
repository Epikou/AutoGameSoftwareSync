//! Nouveau PC (2.0, lot 12) : tout retrouver apres un formatage ou sur une nouvelle machine, en 4 ecrans (ancien PC,
//! verification, recuperation, bilan). Logique testable ici ; l'ecran est dans `gui/migrate_ui.rs`.
//!
//! - Appareils du coffre : suffixe des dossiers de versions (`saves/<horodatage>-<appareil>`) et marques de position
//!   (`.agss-devices`) ; aucun fichier de donnees n'est lu.
//! - Verification : l'emplacement d'origine (gabarit universel du meta.json) est traduit vers CE PC (autre nom
//!   d'utilisateur, prefixes Proton / Wine sous Linux, dossiers perso connus, couches de connaissance locales en repli).
//!   Etats : pret, programme pas encore installe (recuperation differee), emplacement a choisir, deja present ici et plus
//!   recent (conflit : on garde les deux), deja identique.
//! - Recuperation en lot : jamais pendant que le programme concerne tourne, copie de securite avant chaque element (moteur
//!   existant), element lie a ce PC pour la synchro future, annulable entre deux elements, et annulable ensuite en bloc.
//! - Recuperation differee : `<donnees>/migration/pending.json`, revu quand le programme apparait.
//! - Inventaire des logiciels de l'ancien PC : voir `inventory`.
use crate::backup::{self, UndoTicket};
use crate::cloud::{self, DestGame};
use crate::config::{Config, Game, Location};
use crate::detect::{Installed, Store};
use crate::hints::KnownFolders;
use crate::home::Kind;
use crate::store::{self, Meta};
use crate::{util, Ctx, Res};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub mod inventory;

/// Un dossier perso de plus de 500 Mo garde 3 versions (meme regle que le scan).
pub const BIG_FOLDER: u64 = crate::weight::DEFAULT_LIMIT_MB as u64 * crate::weight::MB;

// ================================================================== appareils du coffre

/// Appareil vu dans le coffre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    /// Cles des elements du coffre ecrits ou tenus par cet appareil.
    pub keys: Vec<String>,
    /// Elements pas encore sur ce PC (ce qui sera propose).
    pub todo: usize,
    /// Derniere copie connue (ms UTC) : dossier de version le plus recent, ou marque de position.
    pub last_ms: Option<u64>,
    /// Meme nom que ce PC (cas d'un PC reinstalle sous le meme nom).
    pub same_as_here: bool,
}

impl Device {
    pub fn items(&self) -> usize {
        self.keys.len()
    }
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// Appareils qui ont ecrit ou tiennent des elements du coffre, le plus recent d'abord. `local_keys` : cles deja sur ce PC.
pub fn devices(vault: &Path, own: &str, local_keys: &HashSet<String>) -> Vec<Device> {
    let mut map: BTreeMap<String, (String, BTreeSet<String>, Option<u64>)> = BTreeMap::new();
    let games = cloud::list_dest_games(vault);
    let known: HashSet<String> = games.iter().map(|g| g.key.clone()).collect();
    for dg in &games {
        for root in &dg.dirs {
            for d in subdirs(&root.join(cloud::SAVES_DIR)) {
                let n = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !cloud::is_save_dir_name(&n) {
                    continue;
                }
                let Some(m) = crate::machinecolor::machine_of_dirname(&n) else { continue };
                let e = map.entry(m.to_lowercase()).or_insert_with(|| (m.to_string(), BTreeSet::new(), None));
                e.1.insert(dg.key.clone());
                if let Some(ms) = util::parse_timestamp(&n) {
                    e.2 = Some(e.2.map_or(ms, |x: u64| x.max(ms)));
                }
            }
        }
    }
    // Marques de position : un appareil qui a seulement recupere une version tient aussi l'element.
    for d in subdirs(&vault.join(crate::home::DEVICES_DIR)) {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for f in rd.filter_map(|e| e.ok()) {
            let n = f.file_name().to_string_lossy().to_string();
            let Some((key, _)) = n.strip_suffix(".json").and_then(|s| s.split_once('~')) else { continue };
            if !known.contains(key) {
                continue;
            }
            let v: Option<serde_json::Value> = std::fs::read_to_string(f.path()).ok().and_then(|t| serde_json::from_str(&t).ok());
            let machine = v.as_ref().and_then(|v| v.get("device")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            if machine.trim().is_empty() {
                continue;
            }
            let at = v.as_ref().and_then(|v| v.get("at_ms")).and_then(|x| x.as_u64());
            let e = map.entry(machine.to_lowercase()).or_insert_with(|| (machine.clone(), BTreeSet::new(), None));
            e.1.insert(key.to_string());
            if let Some(ms) = at {
                e.2 = Some(e.2.map_or(ms, |x: u64| x.max(ms)));
            }
        }
    }
    let mut v: Vec<Device> = map
        .into_values()
        .map(|(name, keys, last)| {
            let todo = keys.iter().filter(|k| !local_keys.contains(*k) && !crate::winprefs::kind_of_key(k).map(|x| x.device_specific()).unwrap_or(false)).count();
            Device { same_as_here: name.eq_ignore_ascii_case(own), name, keys: keys.into_iter().collect(), todo, last_ms: last }
        })
        .filter(|d| !d.keys.is_empty())
        .collect();
    v.sort_by(|a, b| b.last_ms.cmp(&a.last_ms).then(b.keys.len().cmp(&a.keys.len())).then(a.name.cmp(&b.name)));
    v
}

/// Cles des elements de la configuration de ce PC (nom normalise et dossier du coffre).
pub fn local_keys(cfg: &Config) -> HashSet<String> {
    cfg.games.iter().map(cloud::dest_key).chain(cfg.games.iter().map(|g| cloud::game_key(&g.name))).collect()
}

// ================================================================== ce PC

/// Dossier perso ou element systeme connu par son nom STABLE (cle du coffre identique sur tous les appareils).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    pub stable: String,
    pub path: PathBuf,
    pub is_file: bool,
    pub filter: Option<String>,
    pub exclude: Option<String>,
    /// Propre au systeme (raccourcis, menu Demarrer, fond d'ecran) : pas repris d'un systeme a l'autre.
    pub os_specific: bool,
}

/// Ce que ce PC sait de lui-meme (injectable : tests sur un PC simule Windows ou Linux).
pub struct Here {
    pub windows: bool,
    pub kf: KnownFolders,
    /// Prefixes Wine / Proton (Linux).
    pub prefixes: Vec<PathBuf>,
    pub steam_libs: Vec<PathBuf>,
    /// Programmes installes par les lanceurs.
    pub launchers: Vec<Installed>,
    /// Logiciels installes (noms).
    pub apps: Vec<String>,
    pub personal: Vec<Known>,
    pub db: Option<Arc<crate::gamedb::GameDb>>,
    /// Couches de connaissance locales (lot 2) : emplacements trouves pour (nom, programme).
    pub locate: Option<Box<dyn Fn(&str, Option<&str>) -> Vec<PathBuf> + Send + Sync>>,
    /// Le programme tourne-t-il ?
    pub running: Box<dyn Fn(&str) -> bool + Send + Sync>,
}

impl Here {
    /// PC vide (tests) : aucun programme, aucun dossier connu.
    pub fn empty(windows: bool) -> Here {
        Here { windows, kf: KnownFolders::default(), prefixes: vec![], steam_libs: vec![], launchers: vec![], apps: vec![], personal: vec![], db: None, locate: None, running: Box::new(|_| false) }
    }

    pub fn os(&self) -> &'static str {
        if self.windows {
            "windows"
        } else {
            "linux"
        }
    }

    /// Ce PC (lecture seule). A appeler hors de l'interface : lanceurs, logiciels installes, prefixes.
    pub fn real(data_dir: &Path) -> Here {
        use crate::scan::sources;
        let folders = crate::knowledge::paths::Folders::real();
        let steam_libs = crate::detect::steam_root().map(|r| crate::detect::steam_libraries(&r)).unwrap_or_default();
        let launchers = sources::launcher_games();
        let apps: Vec<String> = sources::installed_apps().into_iter().filter(|a| !a.system).map(|a| a.name).collect();
        let mut personal: Vec<Known> = sources::personal_dirs(&folders)
            .into_iter()
            .map(|(k, p)| Known { stable: crate::scan::personal_name(k).to_string(), exclude: (k == crate::scan::Personal::Desktop).then(|| "desktop.ini".to_string()), path: p, is_file: false, filter: None, os_specific: false })
            .collect();
        for (k, p, is_file, filter) in sources::system_items(&folders) {
            if p.as_os_str().is_empty() {
                continue;
            }
            personal.push(Known { stable: crate::scan::sys_name(k).to_string(), path: p, is_file, filter, exclude: None, os_specific: true });
        }
        personal.extend(pref_known(data_dir, cfg!(windows)));
        let dd = data_dir.to_path_buf();
        Here {
            windows: cfg!(windows),
            kf: KnownFolders::real(),
            prefixes: if cfg!(windows) { vec![] } else { crate::proton::wine_prefixes_cached() },
            steam_libs,
            launchers,
            apps,
            personal,
            db: Some(crate::gamedb::embedded(data_dir)),
            locate: Some(Box::new(move |name: &str, exe: Option<&str>| {
                let p = crate::knowledge::Program::describe(None, exe.unwrap_or(""), name, false);
                if p.key.is_empty() && name.trim().is_empty() {
                    return vec![];
                }
                crate::knowledge::find_local(&p, &dd, util::now_ms()).into_iter().map(|f| f.path).collect()
            })),
            running: Box::new(|exe: &str| !matches!(crate::process::ProcessChecker::is_running(&crate::process::ToolhelpChecker, exe), Ok(false))),
        }
    }
}

/// Reglages Windows de ce PC, retrouves par leur nom stable (dossier de travail local). Les elements propres a un appareil
/// (mode d'alimentation, liste « A refaire ») ne sont jamais repris d'un autre appareil.
pub fn pref_known(data_dir: &Path, windows: bool) -> Vec<Known> {
    if !windows {
        return vec![];
    }
    crate::winprefs::Kind::ALL
        .iter()
        .filter(|k| !k.device_specific())
        .map(|k| Known { stable: k.stable().to_string(), path: crate::winprefs::staging(data_dir, *k), is_file: false, filter: None, exclude: None, os_specific: true })
        .collect()
}

/// Liste « A refaire » (imprimantes, lecteurs reseau, Wi-Fi) notee par l'appareil `device` dans le coffre, si cet appareil
/// la protegeait. Lecture seule : rien n'est remis.
pub fn todo_list_of(vault: &Path, device: &Device) -> Option<crate::winprefs::TodoList> {
    let pre = crate::winprefs::device_prefix(crate::winprefs::Kind::TodoList);
    let key = device.keys.iter().find(|k| k.starts_with(&pre) && k.len() > pre.len())?;
    let probe = Game { name: crate::winprefs::Kind::TodoList.stable().into(), dest_key: Some(key.clone()), ..Default::default() };
    let snap = cloud::scan_dest(vault, &probe);
    let v = backup::active_version(&snap)?;
    let pf = crate::winprefs::PrefFile::read(&v.dir.join("files").join(crate::winprefs::FILE))?;
    pf.todo.filter(|t| !t.is_empty())
}

/// Deux noms designent-ils le meme programme ? (identiques une fois normalises, ou l'un contient l'autre a partir de 4
/// caracteres).
pub fn same_name(a: &str, b: &str) -> bool {
    let (x, y) = (crate::gamedb::norm(a), crate::gamedb::norm(b));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    x == y || (x.len() >= 4 && y.len() >= 4 && (x.contains(&y) || y.contains(&x)))
}

fn is_windows_exe(exe: Option<&str>) -> bool {
    exe.map(|e| e.trim().to_lowercase().ends_with(".exe")).unwrap_or(false)
}

/// Resultat de la traduction d'un emplacement d'origine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tr {
    /// Emplacement sur ce PC (il peut ne pas encore exister).
    Path(PathBuf),
    /// Programme Windows sous Linux sans prefixe Proton / Wine : il faut d'abord l'installer (et le lancer une fois).
    NeedProgram,
    /// Gabarit inconnu ici.
    Unknown,
}

/// Prefixe Proton du programme `name` / `exe` s'il est installe par Steam (AppID connu par le lanceur ou la base).
fn steam_prefix(name: &str, exe: Option<&str>, here: &Here) -> Option<PathBuf> {
    let mut ids: Vec<u64> = here.launchers.iter().filter(|i| i.store == Store::Steam && (same_name(&i.name, name) || exe.map(|e| i.exe.as_deref().map(|x| x.eq_ignore_ascii_case(e)).unwrap_or(false)).unwrap_or(false))).filter_map(|i| i.id.parse().ok()).collect();
    if let Some(db) = &here.db {
        ids.extend(db.find_by_name(name).iter().filter_map(|g| g.steam));
    }
    ids.into_iter().find_map(|id| crate::proton::find_prefix(&here.steam_libs, id))
}

/// Gabarit universel (`%APPDATA%/Jeu`) -> emplacement sur ce PC. Un programme Windows sous Linux va dans son prefixe
/// Proton / Wine (dossier existant, sinon prefixe Steam du programme) ; sinon le gabarit est developpe avec les dossiers
/// de cet utilisateur (autre nom d'utilisateur : meme gabarit, autre dossier).
pub fn translate(tpl: &str, name: &str, exe: Option<&str>, here: &Here) -> Tr {
    if is_windows_exe(exe) && !here.windows {
        if let Some(p) = crate::proton::find_in_prefixes(tpl, &here.prefixes).into_iter().next() {
            return Tr::Path(p);
        }
        if let Some(p) = steam_prefix(name, exe, here).and_then(|pfx| crate::proton::translate_template(tpl, &pfx)) {
            return Tr::Path(p);
        }
        return Tr::NeedProgram;
    }
    match crate::hints::expand(tpl, &here.kf) {
        Some(p) => Tr::Path(p),
        None => Tr::Unknown,
    }
}

/// Le programme est-il installe sur ce PC ? (pas de programme = rien a installer)
pub fn program_installed(name: &str, exe: Option<&str>, here: &Here, place_exists: bool) -> bool {
    let Some(exe) = exe.map(str::trim).filter(|e| !e.is_empty()) else { return true };
    if place_exists || (here.running)(exe) {
        return true;
    }
    if here.launchers.iter().any(|i| same_name(&i.name, name) || i.exe.as_deref().map(|x| x.eq_ignore_ascii_case(exe)).unwrap_or(false)) {
        return true;
    }
    let stem = exe.rsplit(['/', '\\']).next().unwrap_or(exe).trim_end_matches(".exe").trim_end_matches(".EXE");
    here.apps.iter().any(|a| same_name(a, name) || (stem.len() >= 4 && same_name(a, stem)))
}

// ================================================================== verification

/// Etat d'un element sur ce PC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Emplacement retrouve : la derniere version sera remise (copie de securite avant).
    Ready,
    /// Programme pas encore installe : la copie reste dans le coffre, on la remettra a l'installation.
    NotInstalled,
    /// Emplacement a choisir (selecteur guide).
    Choose,
    /// Deja present ici et plus recent : conflit, on garde les deux (decoche par defaut).
    Conflict,
    /// Deja present ici, identique : simplement lie.
    AlreadyHere,
}

/// Un emplacement de ce PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub path: PathBuf,
    pub is_file: bool,
    pub filter: Option<String>,
    pub exclude: Option<String>,
}

/// Emplacement supplementaire d'un element (id stable, comme sur l'ancien PC).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extra {
    pub id: String,
    pub label: String,
    pub is_file: bool,
    pub filter: Option<String>,
    pub exclude: Option<String>,
    /// None : pas retrouve ici (ses fichiers restent dans le coffre).
    pub path: Option<PathBuf>,
}

/// Lien vers la page d'un programme (boutique si l'identifiant est connu, sinon recherche web).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Steam(String),
    Gog(String),
    Epic(String),
    Web,
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

impl Link {
    /// Adresse a ouvrir dans le navigateur. Seul le nom du programme part dans l'adresse (jamais de chemin).
    pub fn url(&self, name: &str) -> String {
        match self {
            Link::Steam(id) if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() => format!("https://store.steampowered.com/app/{}", id),
            Link::Gog(_) => format!("https://www.gog.com/en/games?query={}", enc(name)),
            Link::Epic(_) => format!("https://store.epicgames.com/browse?q={}", enc(name)),
            _ => format!("https://duckduckgo.com/?q={}", enc(&format!("{} download", name))),
        }
    }

    /// Nom de la boutique (bouton) ; None = recherche web.
    pub fn store(&self) -> Option<&'static str> {
        match self {
            Link::Steam(_) => Some("Steam"),
            Link::Gog(_) => Some("GOG"),
            Link::Epic(_) => Some("Epic"),
            Link::Web => None,
        }
    }
}

/// Lien de la page d'un programme : identifiant note par l'ancien PC (inventaire), sinon base des programmes connus.
pub fn link_for(name: &str, exe: Option<&str>, inv: Option<&inventory::Inventory>, db: Option<&crate::gamedb::GameDb>) -> Link {
    if let Some(g) = inv.and_then(|i| i.games.iter().find(|g| same_name(&g.name, name))) {
        match g.store.as_str() {
            "steam" => return Link::Steam(g.id.clone()),
            "gog" => return Link::Gog(g.id.clone()),
            "epic" => return Link::Epic(g.id.clone()),
            _ => {}
        }
    }
    if let Some(db) = db {
        let mut c = db.find_by_name(name);
        if c.is_empty() {
            if let Some(e) = exe {
                c = db.find_by_exe(e);
            }
        }
        if let Some(g) = c.first() {
            if let Some(s) = g.steam {
                return Link::Steam(s.to_string());
            }
            if let Some(gg) = g.gog {
                return Link::Gog(gg.to_string());
            }
        }
    }
    Link::Web
}

/// Ce qu'on prevoit pour un element du coffre.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Dossier de l'element dans le coffre.
    pub key: String,
    /// Nom d'origine (identique sur tous les appareils).
    pub name: String,
    /// Nom affiche (traduction d'un dossier perso), rempli par l'interface.
    pub display: Option<String>,
    pub exe: Option<String>,
    pub kind: Kind,
    pub status: Status,
    pub main: Option<Place>,
    pub extras: Vec<Extra>,
    /// Nom stable d'un dossier perso / element systeme reconnu.
    pub personal: Option<String>,
    pub files: usize,
    pub bytes: u64,
    /// Version qui sera remise : date (ms UTC) et appareil qui l'a ecrite.
    pub saved_ms: u64,
    pub writer: String,
    /// Fichier le plus recent deja present ici (ms), pour un conflit.
    pub local_newest_ms: Option<u64>,
    pub link: Link,
    pub checked: bool,
    /// Fichiers de l'emplacement principal dans la version (chemin, taille, empreinte) : comparaison avec ce PC.
    pub main_files: Vec<(String, u64, String)>,
    pub latest_mtime_ms: u64,
    /// Emplacements trouves par les couches de connaissance (selecteur guide).
    pub suggestions: Vec<PathBuf>,
}

/// Systeme de l'appareil qui a ecrit la version : indice note, sinon deduit des fichiers (raccourcis Windows, lanceurs
/// `.desktop`). Vide = inconnu.
pub fn source_os(meta: &Meta) -> String {
    if let Some(h) = &meta.main_hint {
        if !h.os.is_empty() {
            return h.os.clone();
        }
    }
    let low: Vec<String> = meta.files.iter().map(|f| f.path.to_lowercase()).collect();
    if low.iter().any(|p| p.ends_with(".lnk") || p.ends_with(".url") || p.ends_with("desktop.ini") || p == "transcodedwallpaper") {
        "windows".into()
    } else if low.iter().any(|p| p.ends_with(".desktop")) {
        "linux".into()
    } else {
        String::new()
    }
}

/// L'emplacement principal est-il un fichier unique ? Indice note, sinon : un seul fichier, au nom du dernier element du
/// gabarit.
fn main_is_file(meta: &Meta) -> bool {
    if let Some(h) = &meta.main_hint {
        return h.is_file;
    }
    let main: Vec<&store::FileInfo> = meta.files.iter().filter(|f| store::split_loc(&f.path).0.is_none()).collect();
    match (main.as_slice(), meta.save_path_tpl.as_deref()) {
        ([one], Some(t)) => !one.path.contains('/') && t.rsplit(['/', '\\']).next().map(|l| l.eq_ignore_ascii_case(&one.path)).unwrap_or(false),
        _ => false,
    }
}

/// Fichier le plus recent sous un emplacement (fichiers acceptes par le filtre) : (nombre, ms).
fn local_files(place: &Place) -> Vec<(String, u64, u64)> {
    let g = Game { name: "x".into(), path: place.path.clone(), save_is_file: place.is_file, save_filter: place.filter.clone(), save_exclude: place.exclude.clone(), ..Default::default() };
    let t = store::Target::of(&g);
    let mut out = Vec::new();
    if !t.root.is_dir() {
        return out;
    }
    for e in walkdir::WalkDir::new(&t.root).follow_links(false).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        let Ok(rel) = e.path().strip_prefix(&t.root) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.ends_with(".agss-tmp") || !t.accepts(&rel) {
            continue;
        }
        let md = e.metadata().ok();
        out.push((rel, md.as_ref().map(|m| m.len()).unwrap_or(0), md.and_then(|m| m.modified().ok()).map(util::mtime_ms).unwrap_or(0)));
        if out.len() > 200_000 {
            break;
        }
    }
    out
}

/// Compare l'emplacement de ce PC a la version du coffre : vide = pret ; identique = deja la ; plus recent ici = conflit ;
/// sinon pret (le contenu actuel part dans la copie de securite avant d'etre remplace).
pub fn compare_local(place: &Place, main_files: &[(String, u64, String)], latest_mtime_ms: u64) -> (Status, Option<u64>) {
    let local = local_files(place);
    if local.is_empty() {
        return (Status::Ready, None);
    }
    let newest = local.iter().map(|x| x.2).max();
    let mut a: Vec<(String, u64)> = local.iter().map(|x| (x.0.to_lowercase(), x.1)).collect();
    let mut b: Vec<(String, u64)> = main_files.iter().map(|x| (x.0.to_lowercase(), x.1)).collect();
    a.sort();
    b.sort();
    if a == b {
        let root = if place.is_file { place.path.parent().map(Path::to_path_buf).unwrap_or_default() } else { place.path.clone() };
        let same = main_files.iter().all(|(p, _, h)| util::hash_file(&root.join(p)).map(|x| &x == h).unwrap_or(false));
        if same {
            return (Status::AlreadyHere, newest);
        }
    }
    if newest.map(|n| n > latest_mtime_ms + 2000).unwrap_or(false) {
        (Status::Conflict, newest)
    } else {
        (Status::Ready, newest)
    }
}

/// Plan d'un element du coffre pour ce PC. None : rien a recuperer (aucune version complete).
pub fn plan_item(vault: &Path, dg: &DestGame, here: &Here, inv: Option<&inventory::Inventory>) -> Option<Plan> {
    let probe = Game { name: dg.name.clone(), dest_key: Some(dg.key.clone()), ..Default::default() };
    let snap = cloud::scan_dest(vault, &probe);
    let active = backup::active_version(&snap)?;
    let mut meta = active.meta.clone();
    // Indices absents de la version active (ancienne version de l'appli) : version complete la plus recente.
    if meta.save_path_tpl.is_none() && meta.exe.is_none() {
        if let Some(m) = cloud::latest_complete_meta(dg) {
            meta.save_path_tpl = m.save_path_tpl;
            meta.exe = m.exe;
            meta.game_name = meta.game_name.or(m.game_name);
            if meta.main_hint.is_none() {
                meta.main_hint = m.main_hint;
            }
        }
    }
    let name = dg.name.clone();
    let exe = meta.exe.clone().filter(|e| !e.trim().is_empty());
    let src_os = source_os(&meta);
    let is_file = main_is_file(&meta);
    let (filter, exclude) = meta.main_hint.as_ref().map(|h| (h.filter.clone(), h.exclude.clone())).unwrap_or((None, None));
    let mut status: Option<Status> = None;
    let mut main: Option<Place> = None;
    let mut personal: Option<String> = None;
    let mut system = false;
    // Dossiers perso et elements systeme : retrouves par leur nom stable.
    if exe.is_none() {
        if let Some(k) = here.personal.iter().find(|k| cloud::game_key(&k.stable) == dg.key) {
            personal = Some(k.stable.clone());
            system = k.os_specific;
            if k.os_specific && !src_os.is_empty() && src_os != here.os() {
                status = Some(Status::Choose);
            } else {
                main = Some(Place { path: k.path.clone(), is_file: k.is_file, filter: k.filter.clone().or(filter.clone()), exclude: k.exclude.clone().or(exclude.clone()) });
            }
        }
    }
    let mut need_program = false;
    if main.is_none() && status.is_none() {
        if let Some(t) = meta.save_path_tpl.as_deref() {
            match translate(t, &name, exe.as_deref(), here) {
                Tr::Path(p) => main = Some(Place { path: p, is_file, filter: filter.clone(), exclude: exclude.clone() }),
                Tr::NeedProgram => need_program = true,
                Tr::Unknown => {}
            }
        }
    }
    // Couches de connaissance locales : un seul emplacement trouve = retenu ; plusieurs = proposes au selecteur.
    let mut suggestions = Vec::new();
    if main.is_none() && status.is_none() && !need_program {
        if let Some(loc) = &here.locate {
            suggestions = loc(&name, exe.as_deref());
            if suggestions.len() == 1 && !is_file {
                main = Some(Place { path: suggestions[0].clone(), is_file: false, filter: filter.clone(), exclude: exclude.clone() });
            }
        }
    }
    let place_exists = main.as_ref().map(|m| if m.is_file { m.path.is_file() } else { m.path.is_dir() }).unwrap_or(false);
    let installed = !need_program && program_installed(&name, exe.as_deref(), here, place_exists);
    let main_files: Vec<(String, u64, String)> = meta.files.iter().filter_map(|f| match store::split_loc(&f.path) {
        (None, rel) => Some((rel.to_string(), f.size, f.hash.clone())),
        _ => None,
    }).collect();
    let mut local_newest = None;
    let status = match status {
        Some(s) => s,
        None if !installed => Status::NotInstalled,
        None => match &main {
            None => Status::Choose,
            Some(p) => {
                let (s, n) = compare_local(p, &main_files, meta.latest_mtime_ms);
                local_newest = n;
                s
            }
        },
    };
    let extras: Vec<Extra> = meta
        .locations
        .iter()
        .map(|l| {
            let path = l.path_tpl.as_deref().and_then(|t| match translate(t, &name, exe.as_deref(), here) {
                Tr::Path(p) => Some(p),
                _ => None,
            });
            Extra { id: l.id.clone(), label: l.label.clone(), is_file: l.is_file, filter: l.filter.clone(), exclude: l.exclude.clone(), path }
        })
        .collect();
    let in_db = here.db.as_ref().map(|db| !db.find_by_name(&name).is_empty() || exe.as_deref().map(|e| !db.find_by_exe(e).is_empty()).unwrap_or(false)).unwrap_or(false);
    let kind = if exe.is_none() {
        Kind::Folder
    } else if in_db || here.launchers.iter().any(|i| same_name(&i.name, &name)) || inv.map(|i| i.games.iter().any(|g| same_name(&g.name, &name))).unwrap_or(false) {
        Kind::Game
    } else {
        Kind::App
    };
    let link = link_for(&name, exe.as_deref(), inv, here.db.as_deref());
    Some(Plan {
        key: dg.key.clone(),
        name,
        display: None,
        exe,
        kind,
        // Elements Windows / systeme : toujours decoches (choix de l'utilisateur).
        checked: status != Status::Conflict && !system,
        status,
        main,
        extras,
        personal,
        files: meta.files.len(),
        bytes: meta.files.iter().map(|f| f.size).sum(),
        saved_ms: meta.sort_ms(),
        writer: meta.machine.clone(),
        local_newest_ms: local_newest,
        link,
        main_files,
        latest_mtime_ms: meta.latest_mtime_ms,
        suggestions,
    })
}

/// Verification de tous les elements de l'appareil `device` absents de ce PC. `step(i, n)` : progression.
pub fn check(vault: &Path, device: &Device, local: &HashSet<String>, here: &Here, inv: Option<&inventory::Inventory>, step: &dyn Fn(usize, usize)) -> Vec<Plan> {
    let games = cloud::list_dest_games(vault);
    // Reglages Windows : jamais sur un autre systeme ; un element propre a un appareil n'est jamais repris ailleurs.
    let todo: Vec<&DestGame> = games
        .iter()
        .filter(|g| device.keys.contains(&g.key) && !local.contains(&g.key))
        .filter(|g| match crate::winprefs::kind_of_key(&g.key) {
            Some(k) => here.windows && !k.device_specific(),
            None => true,
        })
        .collect();
    let n = todo.len();
    let mut out = Vec::new();
    for (i, dg) in todo.into_iter().enumerate() {
        step(i, n);
        if let Some(p) = plan_item(vault, dg, here, inv) {
            out.push(p);
        }
    }
    step(n, n);
    // Ordre : pret, deja la, a choisir, conflit, a installer ; puis par nom.
    out.sort_by(|a, b| status_rank(a.status).cmp(&status_rank(b.status)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn status_rank(s: Status) -> u8 {
    match s {
        Status::Ready => 0,
        Status::AlreadyHere => 1,
        Status::Choose => 2,
        Status::Conflict => 3,
        Status::NotInstalled => 4,
    }
}

/// Emplacement choisi a la main (selecteur guide) : l'element est reverifie (vide, identique, plus recent).
pub fn choose_place(plan: &mut Plan, path: PathBuf, is_file: bool) {
    let (filter, exclude) = plan.main.as_ref().map(|m| (m.filter.clone(), m.exclude.clone())).unwrap_or((None, None));
    let place = Place { path, is_file, filter, exclude };
    let (s, n) = compare_local(&place, &plan.main_files, plan.latest_mtime_ms);
    plan.main = Some(place);
    plan.status = s;
    plan.local_newest_ms = n;
    plan.checked = s != Status::Conflict;
}

/// Taille totale des elements coches.
pub fn checked_bytes(plans: &[Plan]) -> u64 {
    plans.iter().filter(|p| p.checked).map(|p| p.bytes).sum()
}

/// Programmes concernes qui tournent (elements coches qui seront remis) : la recuperation attend leur fermeture.
pub fn running_programs(plans: &[Plan], running: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut v: Vec<String> = plans.iter().filter(|p| p.checked && p.status == Status::Ready).filter_map(|p| p.exe.as_deref().filter(|e| running(e)).map(|_| p.display.clone().unwrap_or_else(|| p.name.clone()))).collect();
    v.dedup();
    v
}

// ================================================================== recuperation

/// Resultat d'un element.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Derniere version remise (copie de securite faite avant), element lie a ce PC.
    Restored,
    /// Lie sans rien ecrire (deja identique, ou conflit : les deux versions sont gardees).
    Linked,
    /// Programme pas encore installe : remis quand il apparaitra.
    Deferred,
    /// Emplacement non choisi : reste a faire.
    NoPlace,
    /// Le programme tourne : rien n'a ete touche.
    Busy,
    /// Deja protege ici (par un autre element).
    Already(String),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct ItemResult {
    pub key: String,
    pub name: String,
    pub shown: String,
    pub outcome: Outcome,
    /// Element ajoute a la configuration (pour l'annulation).
    pub game: Option<Game>,
    pub ticket: Option<UndoTicket>,
    pub link: Link,
    pub bytes: u64,
}

/// Etat d'une recuperation en cours (partage avec l'interface).
#[derive(Default)]
pub struct RunState {
    pub total: usize,
    pub index: usize,
    pub current: Option<String>,
    pub results: Vec<ItemResult>,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub finished: bool,
    pub cancelled: bool,
    /// Progression de l'element en cours (fichiers, octets, phase).
    pub progress: Option<Arc<crate::perf::Progress>>,
}

#[derive(Default)]
pub struct Run {
    pub state: Mutex<RunState>,
    /// Arret demande : pris en compte entre deux elements.
    pub cancel: AtomicBool,
}

impl Run {
    pub fn new() -> Arc<Run> {
        Arc::new(Run::default())
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Element a ajouter a la configuration de ce PC (lie au dossier du coffre, emplacements supplementaires aux memes ids).
pub fn to_game(plan: &Plan) -> Option<Game> {
    let main = plan.main.as_ref()?;
    let mut g = Game {
        name: plan.name.clone(),
        path: main.path.clone(),
        exe: plan.exe.clone(),
        dest_key: if cloud::game_key(&plan.name) == plan.key { None } else { Some(plan.key.clone()) },
        display_name: plan.display.clone().filter(|d| !d.trim().is_empty() && *d != plan.name),
        save_is_file: main.is_file,
        save_filter: main.filter.clone(),
        save_exclude: main.exclude.clone(),
        ..Default::default()
    };
    for e in &plan.extras {
        if let Some(p) = &e.path {
            g.extra_locations.push(Location { id: e.id.clone(), label: e.label.clone(), path: p.clone(), is_file: e.is_file, filter: e.filter.clone(), exclude: e.exclude.clone(), enabled: true });
        }
    }
    if g.exe.is_none() {
        g.schedule = Some(crate::scan::default_schedule());
    }
    Some(g)
}

fn result(plan: &Plan, outcome: Outcome) -> ItemResult {
    ItemResult { key: plan.key.clone(), name: plan.name.clone(), shown: plan.display.clone().unwrap_or_else(|| plan.name.clone()), outcome, game: None, ticket: None, link: plan.link.clone(), bytes: plan.bytes }
}

/// Ajoute l'element a la configuration (apres la remise des fichiers pour un fichier unique, qui doit exister).
fn link_game(ctx: &Ctx, g: &Game, big: bool) -> Res<()> {
    crate::actions::add_game(ctx, g.clone())?;
    if big {
        let mut s = crate::settings::Settings::load(ctx).unwrap_or_default();
        s.game_mut(&g.name).keep_versions = Some(crate::scan::BIG_KEEP);
        let _ = s.save(ctx);
    }
    Ok(())
}

/// Recupere UN element : programme ferme, copie de securite avant (moteur), element lie a ce PC. Rien n'est supprime.
/// Un element recupere sort de la liste des recuperations differees.
pub fn apply(ctx: &Ctx, vault: &Path, plan: &Plan, source: &str) -> ItemResult {
    let r = apply_inner(ctx, vault, plan, source);
    if matches!(r.outcome, Outcome::Restored | Outcome::Linked) {
        let _ = remove_pending(&ctx.data_dir, &plan.key);
    }
    r
}

fn apply_inner(ctx: &Ctx, vault: &Path, plan: &Plan, source: &str) -> ItemResult {
    let mut r = result(plan, Outcome::NoPlace);
    match plan.status {
        Status::NotInstalled => {
            r.outcome = match add_pending(&ctx.data_dir, plan, source) {
                Ok(()) => Outcome::Deferred,
                Err(e) => Outcome::Failed(e.to_string()),
            };
            return r;
        }
        Status::Choose if plan.main.is_none() => return r,
        _ => {}
    }
    let Some(g) = to_game(plan) else { return r };
    let cfg = match Config::load(ctx) {
        Ok(c) => c,
        Err(e) => {
            r.outcome = Outcome::Failed(e.to_string());
            return r;
        }
    };
    if cfg.find(&g.name).is_some() || crate::actions::config_has_key(&cfg, &g.name) {
        r.outcome = Outcome::Already(g.name.clone());
        return r;
    }
    if let Some(o) = cfg.games.iter().find(|o| crate::actions::targets_overlap(o, &g)) {
        r.outcome = Outcome::Already(o.shown().to_string());
        return r;
    }
    let big = plan.personal.is_some() && crate::weight::is_heavy(plan.bytes);
    if matches!(plan.status, Status::AlreadyHere | Status::Conflict) {
        // Rien n'est ecrit : identique, ou plus recent ici (la prochaine synchro garde les deux versions).
        r.outcome = match link_game(ctx, &g, big) {
            Ok(()) => {
                r.game = Some(g);
                Outcome::Linked
            }
            Err(e) => Outcome::Failed(e.to_string()),
        };
        return r;
    }
    if let Some(exe) = g.exe.as_deref() {
        if !matches!(ctx.procs.is_running(exe), Ok(false)) {
            r.outcome = Outcome::Busy;
            return r;
        }
    }
    match backup::restore_undoable(ctx, &g, vault) {
        Ok((_, ticket)) => {
            r.ticket = Some(ticket);
            match link_game(ctx, &g, big) {
                Ok(()) => {
                    r.game = Some(g);
                    r.outcome = Outcome::Restored;
                }
                Err(e) => {
                    // Les fichiers remis restent (copie de securite d'avant gardee) ; l'annulation les retire.
                    r.game = Some(g);
                    r.outcome = Outcome::Failed(e.to_string());
                }
            }
        }
        Err(e) if e.to_string().contains("REFUS") => r.outcome = Outcome::Busy,
        Err(e) => r.outcome = Outcome::Failed(e.to_string()),
    }
    r
}

/// Recuperation en lot (thread de l'interface) : un element apres l'autre, arret possible entre deux elements.
/// `before(i)` : appele avant chaque element (tests).
pub fn run(ctx: &mut Ctx, vault: &Path, plans: &[Plan], source: &str, h: &Run, before: &dyn Fn(usize)) {
    let todo: Vec<&Plan> = plans.iter().filter(|p| p.checked).collect();
    if let Ok(mut s) = h.state.lock() {
        s.total = todo.len();
        s.bytes_total = todo.iter().filter(|p| p.status == Status::Ready).map(|p| p.bytes).sum();
    }
    // Diagnostic des captures (jamais dans l'interface) : pause entre deux elements pour voir la progression.
    let slow = std::env::var("AGSS_MIGRATE_SLOW_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0).min(10_000);
    for (i, p) in todo.iter().enumerate() {
        before(i);
        if h.cancel.load(Ordering::SeqCst) {
            if let Ok(mut s) = h.state.lock() {
                s.cancelled = true;
            }
            break;
        }
        let prog = crate::perf::Progress::new();
        ctx.progress = Some(prog.clone());
        if let Ok(mut s) = h.state.lock() {
            s.index = i;
            s.current = Some(p.display.clone().unwrap_or_else(|| p.name.clone()));
            s.progress = Some(prog);
        }
        if slow > 0 {
            std::thread::sleep(std::time::Duration::from_millis(slow));
            if h.cancel.load(Ordering::SeqCst) {
                if let Ok(mut s) = h.state.lock() {
                    s.cancelled = true;
                    s.current = None;
                }
                break;
            }
        }
        let r = apply(ctx, vault, p, source);
        crate::perf::log(ctx, &format!("[Nouveau PC] {} : {:?}", p.name, r.outcome));
        if let Ok(mut s) = h.state.lock() {
            if p.status == Status::Ready {
                s.bytes_done += p.bytes;
            }
            s.results.push(r);
            s.current = None;
        }
    }
    ctx.progress = None;
    if let Ok(mut s) = h.state.lock() {
        s.finished = true;
        s.progress = None;
        if h.cancel.load(Ordering::SeqCst) && s.results.len() < s.total {
            s.cancelled = true;
        }
    }
}

/// Bilan de l'annulation d'une recuperation.
#[derive(Clone, Debug, Default)]
pub struct UndoReport {
    pub undone: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Annule une recuperation (dans l'ordre inverse) : fichiers d'avant remis a l'identique (copie de securite verifiee, et
/// copie de securite de l'etat actuel avant), elements retires de la configuration (aucun fichier supprime), recuperations
/// differees oubliees. Le coffre n'est jamais modifie.
pub fn undo(ctx: &Ctx, vault: &Path, results: &[ItemResult]) -> UndoReport {
    let mut rep = UndoReport::default();
    for r in results.iter().rev() {
        let res: Res<()> = (|| {
            match (&r.outcome, &r.game) {
                (Outcome::Restored, Some(g)) | (Outcome::Failed(_), Some(g)) => {
                    if let Some(t) = &r.ticket {
                        backup::undo_restore(ctx, g, Some(vault), t)?;
                    }
                    let _ = crate::actions::remove_game(ctx, &g.name);
                }
                (Outcome::Linked, Some(g)) => crate::actions::remove_game(ctx, &g.name)?,
                (Outcome::Deferred, _) => remove_pending(&ctx.data_dir, &r.key)?,
                _ => return Ok(()),
            }
            rep.undone.push(r.name.clone());
            Ok(())
        })();
        if let Err(e) = res {
            rep.failed.push((r.name.clone(), e.to_string()));
        }
    }
    rep
}

// ================================================================== recuperation differee

/// Element a remettre quand son programme sera installe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub exe: Option<String>,
    /// Appareil d'origine.
    pub source: String,
    pub since_ms: u64,
}

pub fn pending_path(data_dir: &Path) -> PathBuf {
    data_dir.join("migration").join("pending.json")
}

pub fn load_pending(data_dir: &Path) -> Vec<Pending> {
    std::fs::read_to_string(pending_path(data_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_pending(data_dir: &Path, v: &[Pending]) -> Res<()> {
    util::write_atomic(&pending_path(data_dir), serde_json::to_string_pretty(v)?.as_bytes())
}

pub fn add_pending(data_dir: &Path, plan: &Plan, source: &str) -> Res<()> {
    let mut v = load_pending(data_dir);
    v.retain(|p| p.key != plan.key);
    v.push(Pending { key: plan.key.clone(), name: plan.name.clone(), display: plan.display.clone(), exe: plan.exe.clone(), source: source.to_string(), since_ms: util::now_ms() });
    save_pending(data_dir, &v)
}

pub fn remove_pending(data_dir: &Path, key: &str) -> Res<()> {
    let mut v = load_pending(data_dir);
    let n = v.len();
    v.retain(|p| p.key != key);
    if v.len() != n {
        save_pending(data_dir, &v)?;
    }
    Ok(())
}

/// Elements differes dont le programme est maintenant la : plan pret a appliquer (carte « Recuperer »). Ceux deja ajoutes
/// a la main sur ce PC sont oublies.
pub fn pending_ready(vault: &Path, data_dir: &Path, local: &HashSet<String>, here: &Here) -> Vec<(Pending, Plan)> {
    let pend = load_pending(data_dir);
    if pend.is_empty() {
        return vec![];
    }
    let games = cloud::list_dest_games(vault);
    let mut out = Vec::new();
    for p in &pend {
        if local.contains(&p.key) {
            let _ = remove_pending(data_dir, &p.key);
            continue;
        }
        let Some(dg) = games.iter().find(|g| g.key == p.key) else { continue };
        if let Some(mut plan) = plan_item(vault, dg, here, None) {
            if matches!(plan.status, Status::Ready | Status::AlreadyHere | Status::Conflict) {
                plan.display = p.display.clone();
                plan.checked = true;
                out.push((p.clone(), plan));
            }
        }
    }
    out
}

// ================================================================== bilan

/// Chiffres du bilan : (recuperes, a terminer).
pub fn summary(results: &[ItemResult]) -> (usize, usize) {
    let ok = results.iter().filter(|r| matches!(r.outcome, Outcome::Restored | Outcome::Linked)).count();
    (ok, results.len() - ok)
}

/// Proprietes de l'evenement d'usage `migration` (tranches seulement).
pub fn usage_props(step: &'static str, items: usize, remaining: usize, secs: u64) -> [(&'static str, &'static str); 4] {
    [("step", step), ("items", crate::usage::bucket_count(items)), ("remaining", crate::usage::bucket_count(remaining)), ("duration", crate::usage::bucket_duration(secs))]
}
