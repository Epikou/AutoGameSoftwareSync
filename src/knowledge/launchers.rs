//! Couche 1 : lanceurs, en LECTURE LOCALE seulement (rien n'est redistribue) :
//! - Steam : `appcache/appinfo.vdf`, sections `ufs/savefiles` et `ufs/rootoverrides` (sauvegarde cloud automatique) par
//!   AppID, et dossier `userdata/<compte>/<appid>/remote` (stockage cloud) ; sous Linux, bases Windows dans le prefixe
//!   Proton du jeu, ou regles Linux natives ;
//! - GOG Galaxy : table `CloudSavesLocations` de sa base locale (`goggame-<id>.info` du dossier d'installation) ;
//! - Epic : manifestes `.item` (dossier d'installation, identifiant du catalogue) et cache du catalogue
//!   (`CloudSaveFolder`) ;
//! - liste des logiciels installes (cles Uninstall : titre, editeur, dossier) pour enrichir la deduction.
use super::appinfo::{self, AppUfs, SaveRule};
use super::paths::{self, Folders, INSTALL};
use super::{kind_for, Found, Layer, Program};
use std::path::{Path, PathBuf};

/// Emplacements des fichiers des lanceurs (injectables pour les tests).
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub steam_root: Option<PathBuf>,
    pub epic_manifests: Option<PathBuf>,
    pub epic_catalog: Option<PathBuf>,
    pub gog_db: Option<PathBuf>,
    /// Lire la liste des logiciels installes (registre) : faux en test.
    pub uninstall: bool,
}

impl Env {
    pub fn real() -> Env {
        let pd = PathBuf::from("C:\\ProgramData");
        Env {
            steam_root: crate::detect::steam_root(),
            epic_manifests: Some(pd.join("Epic").join("EpicGamesLauncher").join("Data").join("Manifests")).filter(|_| cfg!(windows)),
            epic_catalog: Some(pd.join("Epic").join("EpicGamesLauncher").join("Data").join("Catalog").join("catcache.bin")).filter(|_| cfg!(windows)),
            gog_db: Some(pd.join("GOG.com").join("Galaxy").join("storage").join("galaxy-2.0.db")).filter(|_| cfg!(windows)),
            uninstall: cfg!(windows),
        }
    }
}

/// Complete le programme avec ce que savent les lanceurs (titre, editeur, developpeur). Lecture seule.
pub fn enrich(p: &mut Program, env: &Env) {
    if let (Some(id), Some(root)) = (p.steam_appid, env.steam_root.as_ref()) {
        if let Some(u) = steam_ufs(root, id as u32) {
            if p.steam_name.is_none() {
                p.steam_name = u.name.clone();
            }
            if p.developer.is_none() {
                p.developer = u.developer.clone();
            }
            if p.publisher.is_none() {
                p.publisher = u.publisher.clone();
            }
        }
    }
    if let Some(item) = p.exe_path.as_deref().and_then(|e| epic_item_for(e, env)) {
        if p.title.is_none() {
            p.title = Some(item.title.clone());
        }
        if p.install_dir.is_none() {
            p.install_dir = Some(item.install.clone());
        }
        if let Some(c) = epic_catalog_entry(env, &item.catalog_id) {
            if p.developer.is_none() {
                p.developer = c.developer;
            }
        }
    }
    #[cfg(windows)]
    if env.uninstall {
        if let Some(e) = p.exe_path.as_deref().and_then(uninstall_entry_for) {
            if p.title.is_none() {
                p.title = Some(e.name).filter(|n| !n.trim().is_empty());
            }
            if p.publisher.is_none() {
                p.publisher = e.publisher;
            }
        }
    }
}

/// Emplacements donnes par les lanceurs, verifies sur le disque (le dossier existe et contient au moins un fichier).
pub fn find(p: &Program, f: &Folders, env: &Env) -> Vec<Found> {
    let mut out = Vec::new();
    if let (Some(id), Some(root)) = (p.steam_appid, env.steam_root.as_ref()) {
        out.extend(steam_locations(p, f, root, id as u32));
    }
    if out.is_empty() {
        out.extend(gog_locations(p, f, env));
    }
    if out.is_empty() {
        out.extend(epic_locations(p, f, env));
    }
    out
}

fn has_any_file(p: &Path) -> bool {
    if p.is_file() {
        return true;
    }
    walkdir::WalkDir::new(p).max_depth(4).into_iter().filter_map(|e| e.ok()).take(2000).any(|e| e.file_type().is_file())
}

/// Gabarit -> emplacements existants (avec au moins un fichier) ; jamais le dossier d'installation entier.
fn resolve(tpl: &str, f: &Folders, install: Option<&Path>, kind_app: bool, conf: u8, source: &str) -> Vec<Found> {
    let Some((ph, rest)) = paths::split_template(tpl) else { return vec![] };
    if rest.is_empty() {
        return vec![];
    }
    paths::expand(tpl, f, install)
        .into_iter()
        .filter(|x| has_any_file(x))
        .map(|x| {
            let template = if ph.eq_ignore_ascii_case(INSTALL) { Some(tpl.to_string()) } else { paths::to_generic(&x, f).map(|t| generic_ids(&t, tpl)) };
            Found { kind: kind_for(tpl, kind_app), template, path: x, layer: Layer::Launcher, confidence: conf, source: source.to_string() }
        })
        .collect()
}

/// Remet les jokers du gabarit d'origine a la place des identifiants de compte du chemin reel.
fn generic_ids(real_tpl: &str, pattern: &str) -> String {
    let (Some((a, ra)), Some((b, rb))) = (paths::split_template(real_tpl), paths::split_template(pattern)) else { return real_tpl.to_string() };
    if !a.eq_ignore_ascii_case(&b) || ra.len() != rb.len() {
        return real_tpl.to_string();
    }
    let v: Vec<String> = ra.iter().zip(rb.iter()).map(|(x, y)| if y.contains('*') { y.clone() } else { x.clone() }).collect();
    format!("{}/{}", a, v.join("/"))
}

// ------------------------------------------------------------------ Steam

fn steam_ufs(root: &Path, appid: u32) -> Option<AppUfs> {
    let a = appinfo::cached(&root.join("appcache").join("appinfo.vdf"))?;
    Some(appinfo::ufs_of(&a.app(appid)?))
}

/// Racine `ufs` -> base de gabarit.
pub fn steam_root_base(root: &str) -> Option<&'static str> {
    Some(match root.to_lowercase().as_str() {
        "winmydocuments" => "%DOCUMENTS%",
        "winappdatalocal" => "%LOCALAPPDATA%",
        "winappdatalocallow" => "%LOCALLOW%",
        "winappdataroaming" => "%APPDATA%",
        "winsavedgames" => "%SAVEDGAMES%",
        "gameinstall" => INSTALL,
        "linuxhome" => "$HOME",
        "linuxxdgdatahome" => "$XDG_DATA_HOME",
        "linuxxdgconfighome" => "$XDG_CONFIG_HOME",
        _ => return None,
    })
}

/// Gabarits d'une application Steam pour ce PC : regles Windows (Windows, ou Linux avec un prefixe Proton), sinon regles
/// Linux natives (racines remplacees par `rootoverrides`). Les identifiants de compte deviennent des jokers.
pub fn steam_templates(u: &AppUfs, windows_rules: bool) -> Vec<String> {
    let mut out = Vec::new();
    let plat_ok = |r: &SaveRule, want: &str| r.platforms.is_empty() || r.platforms.iter().any(|p| p.eq_ignore_ascii_case(want) || p.eq_ignore_ascii_case("all"));
    for r in &u.savefiles {
        let mut root = r.root.clone();
        let mut path = r.path.clone();
        if windows_rules {
            if !plat_ok(r, "Windows") || r.root.to_lowercase().starts_with("linux") || r.root.to_lowercase().starts_with("mac") {
                continue;
            }
        } else {
            // Regle native Linux, ou regle Windows transposee par un remplacement de racine pour Linux.
            let ov = u.overrides.iter().find(|o| o.root.eq_ignore_ascii_case(&r.root) && o.os.eq_ignore_ascii_case("linux"));
            if let Some(o) = ov {
                root = o.new_root.clone();
                for (find, rep) in &o.transforms {
                    path = path.replace(find, rep);
                }
                if !o.add_path.is_empty() {
                    path = format!("{}/{}", o.add_path.trim_end_matches('/'), path);
                }
            } else if !(r.root.to_lowercase().starts_with("linux") || (plat_ok(r, "Linux") && r.root.eq_ignore_ascii_case("gameinstall"))) {
                continue;
            }
        }
        let Some(base) = steam_root_base(&root) else { continue };
        let p = path.replace("{64BitSteamID}", "*").replace("{Steam3AccountID}", "*").replace('\\', "/");
        let p = p.trim_matches('/');
        if p.is_empty() || p.split('/').any(|c| c == "..") {
            continue; // jamais un dossier de base entier (ni le dossier d'installation entier)
        }
        let t = format!("{}/{}", base, p);
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn steam_locations(p: &Program, f: &Folders, root: &Path, appid: u32) -> Vec<Found> {
    let mut out = Vec::new();
    if let Some(u) = steam_ufs(root, appid) {
        let windows_rules = cfg!(windows) || p.prefix.is_some();
        for t in steam_templates(&u, windows_rules) {
            out.extend(resolve(&t, f, p.install_dir.as_deref(), p.app, 95, "Steam : sauvegarde cloud automatique"));
        }
    }
    if out.is_empty() {
        // Stockage cloud de Steam (jeux qui ecrivent eux-memes dans le dossier de Steam).
        let ud = root.join("userdata");
        if let Ok(rd) = std::fs::read_dir(&ud) {
            for e in rd.filter_map(|e| e.ok()) {
                let acc = e.file_name().to_string_lossy().to_string();
                if acc.is_empty() || !acc.chars().all(|c| c.is_ascii_digit()) || acc == "0" {
                    continue;
                }
                let remote = e.path().join(appid.to_string()).join("remote");
                if remote.is_dir() && has_any_file(&remote) {
                    out.push(Found { path: remote, template: Some(format!("%STEAMUSERDATA%/*/{}/remote", appid)), kind: super::Kind::Save, layer: Layer::Launcher, confidence: 88, source: "Steam : stockage cloud".into() });
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------ GOG Galaxy

/// Identifiant GOG d'un dossier d'installation (`goggame-<id>.info`), en remontant de 2 niveaux au plus.
pub fn gog_id(dir: &Path) -> Option<u64> {
    let mut cur = Some(dir);
    for _ in 0..3 {
        let d = cur?;
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.filter_map(|e| e.ok()) {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if let Some(id) = n.strip_prefix("goggame-").and_then(|r| r.strip_suffix(".info")) {
                    if let Ok(v) = id.parse() {
                        return Some(v);
                    }
                }
            }
        }
        cur = d.parent();
    }
    None
}

/// `<?APPLICATION_DATA_LOCAL?>/Studio/Jeu` -> gabarit.
pub fn gog_template(location: &str) -> Option<String> {
    let l = location.trim();
    let (ph, rest) = l.strip_prefix("<?").and_then(|r| r.split_once("?>"))?;
    let base = match ph.to_uppercase().as_str() {
        "INSTALL" => {
            let r = rest.replace('\\', "/");
            let r = r.trim_matches('/');
            if r.is_empty() || r.split('/').any(|c| c == "..") {
                return None;
            }
            return Some(format!("{}/{}", INSTALL, r));
        }
        "DOCUMENTS" => "%DOCUMENTS%",
        "APPLICATION_DATA_LOCAL" => "%LOCALAPPDATA%",
        "APPLICATION_DATA_LOCAL_LOW" => "%LOCALLOW%",
        "APPLICATION_DATA_ROAMING" => "%APPDATA%",
        "SAVED_GAMES" => "%SAVEDGAMES%",
        "USER_HOME" => "%USERPROFILE%",
        _ => return None,
    };
    paths::normalize(&format!("{}{}", base, rest))
}

/// (emplacement, nom) des dossiers de sauvegarde cloud d'un produit GOG.
pub fn gog_cloud_rows(db: &super::sqlite::Db, product: u64) -> Vec<(String, String)> {
    db.table("CloudSavesLocations")
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.get("productId").and_then(|v| v.as_i64()) == Some(product as i64))
        .filter_map(|r| Some((r.get("location")?.as_text()?.to_string(), r.get("name").and_then(|v| v.as_text()).unwrap_or("").to_string())))
        .collect()
}

fn gog_locations(p: &Program, f: &Folders, env: &Env) -> Vec<Found> {
    let Some(dir) = p.install_dir.as_deref().or_else(|| p.exe_path.as_deref().and_then(Path::parent)) else { return vec![] };
    let Some(id) = gog_id(dir) else { return vec![] };
    let Some(dbp) = env.gog_db.as_ref().filter(|d| d.is_file()) else { return vec![] };
    let Ok(db) = super::sqlite::Db::open(dbp) else { return vec![] };
    let mut out = Vec::new();
    for (loc, name) in gog_cloud_rows(&db, id) {
        let Some(t) = gog_template(&loc) else { continue };
        for mut x in resolve(&t, f, Some(dir), p.app, 93, "GOG Galaxy : sauvegarde cloud") {
            let n = name.to_lowercase();
            if n.contains("setting") || n.contains("config") {
                x.kind = super::Kind::Config;
            } else if n.contains("save") {
                x.kind = super::Kind::Save;
            }
            out.push(x);
        }
    }
    out
}

// ------------------------------------------------------------------ Epic

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicItem {
    pub title: String,
    pub install: PathBuf,
    pub catalog_id: String,
}

pub fn parse_epic_manifest(json: &str) -> Option<EpicItem> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string()).filter(|x| !x.is_empty());
    Some(EpicItem { title: s("DisplayName")?, install: PathBuf::from(s("InstallLocation")?), catalog_id: s("CatalogItemId")? })
}

fn under(child: &Path, parent: &Path) -> bool {
    let (c, p) = (crate::gamedetect::norm_path(child), crate::gamedetect::norm_path(parent));
    let p = p.trim_end_matches('/').to_string();
    p.matches('/').count() >= 1 && c.starts_with(&format!("{}/", p))
}

/// Manifeste Epic dont le dossier d'installation contient l'exe (le plus specifique : plusieurs titres partagent un dossier).
pub fn epic_item_for(exe: &Path, env: &Env) -> Option<EpicItem> {
    let dir = env.epic_manifests.as_ref()?;
    let rd = std::fs::read_dir(dir).ok()?;
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("item")).unwrap_or(false))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| parse_epic_manifest(&t))
        .filter(|i| under(exe, &i.install))
        .max_by_key(|i| i.install.as_os_str().len())
}

/// Decodage base64 standard (le cache du catalogue Epic est du JSON encode). Caracteres inconnus ignores.
pub fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EpicCatalog {
    pub cloud_folder: Option<String>,
    pub developer: Option<String>,
}

/// Entree du cache du catalogue (JSON, eventuellement en base64) pour un identifiant d'article.
pub fn epic_catalog_lookup(raw: &str, catalog_id: &str) -> Option<EpicCatalog> {
    let t = raw.trim_start();
    let json: String = if t.starts_with('[') || t.starts_with('{') { t.to_string() } else { String::from_utf8_lossy(&base64_decode(t)).to_string() };
    let v: serde_json::Value = serde_json::from_str(&json).ok()?;
    let arr = v.as_array().cloned().unwrap_or_else(|| vec![v]);
    let item = arr.into_iter().find(|i| i.get("id").and_then(|x| x.as_str()) == Some(catalog_id))?;
    let attr = |k: &str| item.get("customAttributes").and_then(|c| c.get(k)).and_then(|c| c.get("value")).and_then(|x| x.as_str()).map(|x| x.to_string()).filter(|x| !x.trim().is_empty());
    Some(EpicCatalog { cloud_folder: attr("CloudSaveFolder"), developer: item.get("developer").and_then(|x| x.as_str()).map(|x| x.to_string()).filter(|x| !x.is_empty()) })
}

fn epic_catalog_entry(env: &Env, catalog_id: &str) -> Option<EpicCatalog> {
    let raw = std::fs::read_to_string(env.epic_catalog.as_ref()?).ok()?;
    epic_catalog_lookup(&raw, catalog_id)
}

/// `{AppData}/Studio/Saved/SaveGames/{EpicID}/` -> gabarit.
pub fn epic_template(folder: &str) -> Option<String> {
    let f = folder.trim();
    let low = f.to_lowercase();
    for (ph, base) in [("{appdata}", "%LOCALAPPDATA%"), ("{userdir}", "%DOCUMENTS%"), ("{usersavedgames}", "%SAVEDGAMES%"), ("{userprofile}", "%USERPROFILE%"), ("{installdir}", INSTALL)] {
        if let Some(rest) = low.strip_prefix(ph).map(|_| &f[ph.len()..]) {
            if base == INSTALL {
                let r = rest.replace('\\', "/").replace("{EpicID}", "*");
                let r = r.trim_matches('/');
                if r.is_empty() || r.split('/').any(|c| c == "..") {
                    return None;
                }
                return Some(format!("{}/{}", INSTALL, r));
            }
            return paths::normalize(&format!("{}{}", base, rest));
        }
    }
    None
}

fn epic_locations(p: &Program, f: &Folders, env: &Env) -> Vec<Found> {
    let Some(item) = p.exe_path.as_deref().and_then(|e| epic_item_for(e, env)) else { return vec![] };
    let Some(cat) = epic_catalog_entry(env, &item.catalog_id) else { return vec![] };
    let Some(t) = cat.cloud_folder.as_deref().and_then(epic_template) else { return vec![] };
    resolve(&t, f, Some(&item.install), p.app, 93, "Epic : sauvegarde cloud")
}

// ------------------------------------------------------------------ logiciels installes (Windows)

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UninstallEntry {
    pub name: String,
    pub publisher: Option<String>,
    pub location: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

/// L'entree correspond-elle a cet exe ? (exe sous son dossier d'installation, ou exe de son icone).
pub fn uninstall_matches(e: &UninstallEntry, exe: &Path) -> bool {
    if let Some(i) = &e.icon {
        if crate::gamedetect::norm_path(i) == crate::gamedetect::norm_path(exe) {
            return true;
        }
    }
    match &e.location {
        // Jamais une racine generique (« C:\ », « C:\Program Files ») : au moins 2 niveaux sous le lecteur.
        Some(l) => crate::gamedetect::norm_path(l).trim_end_matches('/').matches('/').count() >= 2 && under(exe, l),
        None => false,
    }
}

#[cfg(windows)]
fn uninstall_entries() -> Vec<UninstallEntry> {
    use crate::registry::{read_string, subkeys, Hive};
    let mut out = Vec::new();
    for (hive, key) in [
        (Hive::LocalMachine, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (Hive::LocalMachine, "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (Hive::CurrentUser, "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ] {
        for sub in subkeys(hive, key) {
            let k = format!("{}\\{}", key, sub);
            let Some(name) = read_string(hive, &k, "DisplayName") else { continue };
            let clean = |s: String| -> Option<PathBuf> {
                let s = s.trim().trim_matches('"').to_string();
                let s = s.split(',').next().unwrap_or("").trim().trim_matches('"').to_string();
                Some(PathBuf::from(s)).filter(|p| p.is_absolute())
            };
            out.push(UninstallEntry { name, publisher: read_string(hive, &k, "Publisher").filter(|p| !p.trim().is_empty()), location: read_string(hive, &k, "InstallLocation").and_then(clean), icon: read_string(hive, &k, "DisplayIcon").and_then(clean) });
        }
    }
    out
}

/// Entree de la liste des logiciels installes correspondant a l'exe (la plus specifique).
#[cfg(windows)]
pub fn uninstall_entry_for(exe: &Path) -> Option<UninstallEntry> {
    uninstall_entries().into_iter().filter(|e| uninstall_matches(e, exe)).max_by_key(|e| e.location.as_ref().map(|l| l.as_os_str().len()).unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Vdf;

    #[test]
    fn regles_steam_windows_et_linux() {
        let u = AppUfs {
            savefiles: vec![
                SaveRule { root: "WinAppDataLocal".into(), path: "Pal/Saved/SaveGames/{64BitSteamID}".into(), pattern: "*".into(), platforms: vec!["Windows".into()], recursive: true },
                SaveRule { root: "WinMyDocuments".into(), path: "My Games/X".into(), pattern: "*".into(), platforms: vec![], recursive: true },
                SaveRule { root: "gameinstall".into(), path: "".into(), pattern: "*.sav".into(), platforms: vec![], recursive: false },
                SaveRule { root: "LinuxXdgDataHome".into(), path: "jeu".into(), pattern: "*".into(), platforms: vec!["Linux".into()], recursive: true },
                SaveRule { root: "MacAppSupport".into(), path: "jeu".into(), pattern: "*".into(), platforms: vec!["MacOS".into()], recursive: true },
            ],
            overrides: vec![appinfo::RootOverride { root: "WinMyDocuments".into(), os: "Linux".into(), new_root: "LinuxHome".into(), add_path: ".local/share".into(), use_instead: true, transforms: vec![("My Games".into(), "my-games".into())] }],
            ..Default::default()
        };
        assert_eq!(steam_templates(&u, true), vec!["%LOCALAPPDATA%/Pal/Saved/SaveGames/*".to_string(), "%DOCUMENTS%/My Games/X".to_string()]);
        assert_eq!(steam_templates(&u, false), vec!["$HOME/.local/share/my-games/X".to_string(), "$XDG_DATA_HOME/jeu".to_string()]);
    }

    #[test]
    fn steam_de_bout_en_bout() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        std::fs::create_dir_all(root.join("appcache")).unwrap();
        let m = |v: Vec<(&str, Vdf)>| Vdf::Map(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        let st = |s: &str| Vdf::Str(s.to_string());
        let app = m(vec![("common", m(vec![("name", st("Palworld")), ("developer", st("Pocketpair"))])), ("ufs", m(vec![("savefiles", m(vec![("0", m(vec![("root", st("WinAppDataLocal")), ("path", st("Pal/Saved/SaveGames/{64BitSteamID}")), ("pattern", st("*"))]))]))]))]);
        std::fs::write(root.join("appcache").join("appinfo.vdf"), appinfo::writer::appinfo(29, &[(1623730, app)])).unwrap();
        let f = Folders::under(t.path());
        let save = f.local.as_ref().unwrap().join("Pal").join("Saved").join("SaveGames").join("76561198000000001");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::write(save.join("Level.sav"), b"x").unwrap();
        let env = Env { steam_root: Some(root.clone()), ..Default::default() };
        let mut p = Program { exe: "Palworld-Win64-Shipping.exe".into(), steam_appid: Some(1623730), ..Default::default() };
        enrich(&mut p, &env);
        assert_eq!(p.steam_name.as_deref(), Some("Palworld"));
        assert_eq!(p.developer.as_deref(), Some("Pocketpair"));
        // Ce test ecrit sous un dossier temporaire : les bases Windows sont simulees, valable sur toutes les plateformes.
        let p_win = Program { prefix: Some(PathBuf::from("/x")), ..p.clone() };
        let found = find(if cfg!(windows) { &p } else { &p_win }, &f, &env);
        assert_eq!(found.len(), 1, "{:?}", found);
        assert_eq!(found[0].path, save);
        assert_eq!(found[0].template.as_deref(), Some("%LOCALAPPDATA%/Pal/Saved/SaveGames/*"), "identifiant de compte retire");
        assert_eq!(found[0].layer, Layer::Launcher);
        // Dossier vide : rien (verification sur le disque).
        std::fs::remove_file(save.join("Level.sav")).unwrap();
        assert!(find(if cfg!(windows) { &p } else { &p_win }, &f, &env).is_empty());
        // Stockage cloud de Steam.
        let remote = root.join("userdata").join("12345").join("1623730").join("remote");
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::write(remote.join("s.dat"), b"x").unwrap();
        let found = find(if cfg!(windows) { &p } else { &p_win }, &f, &env);
        assert_eq!(found.iter().map(|x| x.path.clone()).collect::<Vec<_>>(), vec![remote]);
    }

    #[test]
    fn gabarits_gog_et_epic() {
        assert_eq!(gog_template("<?APPLICATION_DATA_LOCAL_LOW?>/Tiny Trinket Games/Zoria - Age of Shattering/Saves").as_deref(), Some("%LOCALLOW%/Tiny Trinket Games/Zoria - Age of Shattering/Saves"));
        assert_eq!(gog_template("<?APPLICATION_DATA_LOCAL_LOW?>\\\\mc2games\\\\Mystic Academy Escape Room").as_deref(), Some("%LOCALLOW%/mc2games/Mystic Academy Escape Room"));
        assert_eq!(gog_template("<?DOCUMENTS?>\\The Witcher\\saves").as_deref(), Some("%DOCUMENTS%/The Witcher/saves"));
        assert_eq!(gog_template("<?INSTALL?>/save").as_deref(), Some("%INSTALLDIR%/save"));
        assert_eq!(gog_template("<?INSTALL?>"), None, "jamais le dossier d'installation entier");
        assert_eq!(gog_template("<?WEIRD?>/x"), None);
        assert_eq!(epic_template("{AppData}/HogwartsLegacy/Saved/SaveGames/{EpicID}/").as_deref(), Some("%LOCALAPPDATA%/HogwartsLegacy/Saved/SaveGames/*"));
        assert_eq!(epic_template("{UserDir}/My Games/FalloutNV_Epic/").as_deref(), Some("%DOCUMENTS%/My Games/FalloutNV_Epic"));
        assert_eq!(epic_template("{appdata}/../locallow/No Code/").as_deref(), Some("%LOCALLOW%/No Code"));
        assert_eq!(epic_template("{UserSavedGames}/Studio/X").as_deref(), Some("%SAVEDGAMES%/Studio/X"));
        assert_eq!(epic_template("{InstallDir}/Saves").as_deref(), Some("%INSTALLDIR%/Saves"));
        assert_eq!(epic_template("C:/x"), None);
    }

    #[test]
    fn catalogue_epic_en_base64() {
        let json = r#"[{"id":"abc","developer":"Avalanche","customAttributes":{"CloudSaveFolder":{"type":"STRING","value":"{AppData}/HogwartsLegacy/Saved/SaveGames/{EpicID}/"}}},{"id":"zzz"}]"#;
        // Encodage base64 de reference (aucune dependance) : on encode a la main.
        let enc = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let b = json.as_bytes();
            let mut s = String::new();
            for ch in b.chunks(3) {
                let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
                for i in 0..4 {
                    if i <= ch.len() {
                        s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
                    } else {
                        s.push('=');
                    }
                }
            }
            s
        };
        assert_eq!(base64_decode(&enc), json.as_bytes());
        let c = epic_catalog_lookup(&enc, "abc").unwrap();
        assert_eq!(c.developer.as_deref(), Some("Avalanche"));
        assert_eq!(c.cloud_folder.as_deref(), Some("{AppData}/HogwartsLegacy/Saved/SaveGames/{EpicID}/"));
        assert_eq!(epic_catalog_lookup(json, "zzz"), Some(EpicCatalog::default()), "JSON en clair accepte");
        assert!(epic_catalog_lookup(&enc, "nope").is_none());
    }

    #[test]
    fn epic_de_bout_en_bout() {
        let t = tempfile::tempdir().unwrap();
        let man = t.path().join("Manifests");
        std::fs::create_dir_all(&man).unwrap();
        let inst = t.path().join("Epic").join("Hogwarts");
        let exe = inst.join("Phoenix").join("Binaries").join("Win64").join("HogwartsLegacy.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        let inst_json = inst.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(man.join("A.item"), format!(r#"{{"DisplayName":"Hogwarts Legacy","InstallLocation":"{}","CatalogItemId":"abc"}}"#, inst_json)).unwrap();
        std::fs::write(man.join("B.item"), r#"{"DisplayName":"Autre","InstallLocation":"C:\\\\Ailleurs","CatalogItemId":"x"}"#).unwrap();
        let cat = t.path().join("catcache.bin");
        std::fs::write(&cat, r#"[{"id":"abc","customAttributes":{"CloudSaveFolder":{"value":"{AppData}/HogwartsLegacy/Saved/SaveGames/{EpicID}/"}}}]"#).unwrap();
        let f = Folders::under(t.path());
        let save = f.local.as_ref().unwrap().join("HogwartsLegacy").join("Saved").join("SaveGames").join("5894eec90d3c4b9cac0accb22b5d2530");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::write(save.join("HL-00-10.sav"), b"x").unwrap();
        let env = Env { epic_manifests: Some(man), epic_catalog: Some(cat), ..Default::default() };
        let mut p = Program { exe: "HogwartsLegacy.exe".into(), exe_path: Some(exe), ..Default::default() };
        enrich(&mut p, &env);
        assert_eq!(p.title.as_deref(), Some("Hogwarts Legacy"));
        let found = find(&p, &f, &env);
        assert_eq!(found.len(), 1, "{:?}", found);
        assert_eq!(found[0].path, save);
        assert_eq!(found[0].template.as_deref(), Some("%LOCALAPPDATA%/HogwartsLegacy/Saved/SaveGames/*"));
    }

    #[test]
    fn liste_des_logiciels_installes() {
        let e = UninstallEntry { name: "Obsidian".into(), publisher: Some("Obsidian.md".into()), location: Some(PathBuf::from("C:\\Users\\x\\AppData\\Local\\Programs\\Obsidian")), icon: None };
        if cfg!(windows) {
            assert!(uninstall_matches(&e, Path::new("C:\\Users\\x\\AppData\\Local\\Programs\\Obsidian\\Obsidian.exe")));
            assert!(!uninstall_matches(&e, Path::new("C:\\Users\\x\\AppData\\Local\\Programs\\Autre\\a.exe")));
            let racine = UninstallEntry { location: Some(PathBuf::from("C:\\Program Files")), ..e.clone() };
            assert!(!uninstall_matches(&racine, Path::new("C:\\Program Files\\Truc\\t.exe")), "racine generique refusee");
            let icone = UninstallEntry { location: None, icon: Some(PathBuf::from("D:\\Apps\\T\\t.exe")), ..e };
            assert!(uninstall_matches(&icone, Path::new("d:\\apps\\t\\T.EXE")));
        }
    }

    #[test]
    fn identifiant_gog() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("GOG Games").join("Witcher").join("bin");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(t.path().join("GOG Games").join("Witcher").join("goggame-1207658924.info"), b"{}").unwrap();
        assert_eq!(gog_id(&d), Some(1207658924));
        assert_eq!(gog_id(t.path()), None);
    }
}
