//! Sources REELLES du scan (lecture seule, emplacements cibles) : lanceurs en plus de `detect` (Xbox, Ubisoft, EA, itch,
//! Lutris), logiciels installes (cles Uninstall / raccourcis `.desktop`), dossiers perso, elements Windows, couches de
//! connaissance locales avec un registre mis en cache pour tout le scan.
use super::{AppEntry, LastScan, Personal, ScanEnv, SysItem};
use crate::config::Game;
use crate::detect::{Installed, Store};
use crate::knowledge::{self, paths::Folders, Found, Program};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Environnement reel du scan. `games` : elements deja configures ; `vault` : coffre (jamais propose).
pub fn real_env(data_dir: &Path, games: Vec<Game>, vault: Option<PathBuf>) -> ScanEnv {
    // Diagnostic des captures (jamais dans l'interface) : `AGSS_SCAN_SIM=<fichier.json>` remplace les logiciels et jeux installes par
    // une liste de DEMONSTRATION (aucune donnee reelle de l'utilisateur ; profils et bibliotheques reelles ignores).
    if let Some(sim) = sim_file() {
        return sim_env(data_dir, games, vault, sim);
    }
    let folders = Folders::real();
    let steam_libs = crate::detect::steam_root().map(|r| crate::detect::steam_libraries(&r)).unwrap_or_default();
    let mut profiles = crate::profiles::load_embedded();
    let (custom, _) = crate::profiles::load_custom(data_dir, &profiles);
    profiles.extend(custom);
    let dd = data_dir.to_path_buf();
    let reg = Arc::new(CachedRegistry::default());
    let home_base = Arc::new(knowledge::home::Base::load(data_dir));
    let cache = Arc::new(knowledge::store::Cache::load(data_dir));
    let lenv = knowledge::launchers::Env::real();
    let libs2 = steam_libs.clone();
    ScanEnv {
        data_dir: dd.clone(),
        personal: personal_dirs(&folders),
        system: system_items(&folders),
        prefs: pref_offers(data_dir),
        folders,
        launcher_games: Box::new(launcher_games),
        apps: Box::new(installed_apps),
        steam_libs,
        profiles,
        vars: crate::profiles::Vars::real(),
        db: crate::gamedb::embedded(data_dir),
        locate: Box::new(move |p: &Program| locate(p, &lenv, &home_base, &cache, &*reg)),
        games,
        vault,
        last: LastScan::load(data_dir),
        now_ms: crate::util::now_ms(),
        db_probe: true,
        prefix_of: Box::new(move |id| if cfg!(windows) { None } else { crate::proton::find_prefix(&libs2, id) }),
    }
}

/// Base maison, couche 1 (lanceurs), couche 2 (deduction, registre en cache), puis resultats deja appris / trouves.
fn locate(p: &Program, env: &knowledge::launchers::Env, home: &knowledge::home::Base, cache: &knowledge::store::Cache, reg: &CachedRegistry) -> Vec<Found> {
    let mut p = p.clone();
    knowledge::launchers::enrich(&mut p, env);
    let f = p.folders();
    let h = knowledge::home::find(&p, &f, home);
    if !h.is_empty() {
        return knowledge::dedupe(h);
    }
    let l1 = knowledge::launchers::find(&p, &f, env);
    if !l1.is_empty() {
        return knowledge::dedupe(l1);
    }
    let d = knowledge::deduce::find(&p, &f, crate::util::now_ms(), reg);
    if !d.is_empty() {
        return knowledge::dedupe(d);
    }
    if p.key.is_empty() {
        return vec![];
    }
    knowledge::dedupe(cache.known(&p.key))
}

/// Registre de l'utilisateur lu une seule fois par cle pendant le scan (la deduction le parcourt pour chaque logiciel).
#[derive(Default)]
pub struct CachedRegistry {
    map: Mutex<HashMap<String, Vec<String>>>,
}

impl knowledge::deduce::Registry for CachedRegistry {
    fn subkeys(&self, path: &str) -> Vec<String> {
        let k = path.to_lowercase();
        if let Some(v) = self.map.lock().ok().and_then(|m| m.get(&k).cloned()) {
            return v;
        }
        let v = knowledge::deduce::RealRegistry.subkeys(path);
        if let Ok(mut m) = self.map.lock() {
            m.insert(k, v.clone());
        }
        v
    }
}

// ------------------------------------------------------------------ PC de demonstration (captures)

fn sim_file() -> Option<serde_json::Value> {
    let p = std::env::var("AGSS_SCAN_SIM").ok()?;
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// Environnement de DEMONSTRATION : logiciels et jeux de la liste, dossiers perso / Windows pris sous `AGSS_KNOWLEDGE_ROOT`.
fn sim_env(data_dir: &Path, games: Vec<Game>, vault: Option<PathBuf>, sim: serde_json::Value) -> ScanEnv {
    let folders = Folders::real();
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
    let apps: Vec<AppEntry> = sim
        .get("apps")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().map(|e| AppEntry { name: text(e, "name").unwrap_or_default(), publisher: text(e, "publisher"), location: text(e, "location").map(PathBuf::from), exe: text(e, "exe").map(PathBuf::from), system: e.get("system").and_then(|x| x.as_bool()).unwrap_or(false) }).collect())
        .unwrap_or_default();
    let installed: Vec<Installed> = sim
        .get("games")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .map(|e| Installed {
                    name: text(e, "name").unwrap_or_default(),
                    store: match text(e, "store").as_deref() {
                        Some("epic") => Store::Epic,
                        Some("gog") => Store::Gog,
                        _ => Store::Steam,
                    },
                    id: text(e, "id").unwrap_or_default(),
                    install_dir: PathBuf::from(text(e, "install_dir").unwrap_or_default()),
                    exe: text(e, "exe"),
                })
                .collect()
        })
        .unwrap_or_default();
    ScanEnv {
        data_dir: data_dir.to_path_buf(),
        personal: personal_dirs(&folders),
        system: system_items(&folders),
        prefs: pref_offers(data_dir),
        folders,
        launcher_games: Box::new(move || installed.clone()),
        apps: Box::new(move || apps.clone()),
        steam_libs: vec![],
        profiles: vec![],
        vars: crate::profiles::Vars::real(),
        db: crate::gamedb::embedded(data_dir),
        locate: Box::new(|_p: &Program| vec![]),
        games,
        vault,
        last: LastScan::default(),
        now_ms: crate::util::now_ms(),
        db_probe: true,
        prefix_of: Box::new(|_| None),
    }
}

// ------------------------------------------------------------------ dossiers perso et elements Windows

/// Dossiers perso de CE PC (dossiers connus du systeme : Documents redirige vers OneDrive compris ; XDG sous Linux).
pub fn personal_dirs(f: &Folders) -> Vec<(Personal, PathBuf)> {
    if let Some(root) = Folders::test_root() {
        let u = root.join("Users").join(Folders::test_user());
        return [(Personal::Documents, "Documents"), (Personal::Pictures, "Pictures"), (Personal::Desktop, "Desktop"), (Personal::Music, "Music"), (Personal::Videos, "Videos"), (Personal::Downloads, "Downloads")]
            .into_iter()
            .map(|(k, n)| (k, u.join(n)))
            .collect();
    }
    let _ = f;
    let mut v = Vec::new();
    for (k, d) in [
        (Personal::Documents, dirs::document_dir()),
        (Personal::Pictures, dirs::picture_dir()),
        (Personal::Desktop, dirs::desktop_dir()),
        (Personal::Music, dirs::audio_dir()),
        (Personal::Videos, dirs::video_dir()),
        (Personal::Downloads, dirs::download_dir()),
    ] {
        if let Some(d) = d {
            if !v.iter().any(|(_, x): &(Personal, PathBuf)| *x == d) {
                v.push((k, d));
            }
        }
    }
    v
}

/// Elements « Windows » (fichiers seulement) ; Linux : raccourcis du bureau et lanceurs du menu de l'utilisateur.
pub fn system_items(f: &Folders) -> Vec<(SysItem, PathBuf, bool, Option<String>)> {
    let mut v = Vec::new();
    let desktop = if Folders::test_root().is_some() { f.profile.as_ref().map(|p| p.join("Desktop")) } else { dirs::desktop_dir() };
    if cfg!(windows) {
        if let Some(d) = desktop {
            v.push((SysItem::DesktopShortcuts, d, false, Some("*.lnk;*.url".to_string())));
        }
        if let Some(r) = &f.roaming {
            let win = r.join("Microsoft").join("Windows");
            v.push((SysItem::StartMenu, win.join("Start Menu").join("Programs"), false, None));
            v.push((SysItem::Taskbar, r.join("Microsoft").join("Internet Explorer").join("Quick Launch").join("User Pinned").join("TaskBar"), false, Some("*.lnk".to_string())));
            v.push((SysItem::Wallpaper, win.join("Themes").join("TranscodedWallpaper"), true, None));
        }
    } else {
        if let Some(d) = desktop {
            v.push((SysItem::DesktopShortcuts, d, false, Some("*.desktop".to_string())));
        }
        if let Some(x) = &f.xdg_data {
            v.push((SysItem::StartMenu, x.join("applications"), false, Some("*.desktop".to_string())));
        }
    }
    v
}

/// Reglages Windows proposes (exports locaux mis a jour) ; aucun sous Linux.
pub fn pref_offers(data_dir: &Path) -> Vec<crate::winprefs::Offer> {
    let Some(s) = crate::winprefs::real() else { return vec![] };
    // Variables d'environnement : seulement en mode expert.
    let expert = crate::settings::Settings::load_from(data_dir).map(|s| s.expert_mode).unwrap_or(false);
    crate::winprefs::offers(data_dir, &*s).into_iter().filter(|o| expert || !o.kind.expert_only()).collect()
}

// ------------------------------------------------------------------ lanceurs

/// Jeux installes : Steam, Epic, GOG (+ Heroic sous Linux) puis Xbox, Ubisoft, EA, itch, Lutris.
pub fn launcher_games() -> Vec<Installed> {
    let mut v = crate::detect::detect_all().games;
    #[cfg(windows)]
    {
        v.extend(xbox_games());
        v.extend(ubisoft_games());
        v.extend(ea_games());
    }
    v.extend(itch_games());
    #[cfg(not(windows))]
    v.extend(lutris_games());
    let mut seen = std::collections::HashSet::new();
    v.retain(|g| seen.insert(crate::gamedetect::norm_path(&g.install_dir)));
    v
}

/// Xbox (Game Pass PC) : `<lecteur>:\XboxGames\<Jeu>\Content` (nom et exe lus dans `MicrosoftGame.config`).
#[cfg(windows)]
pub fn xbox_games() -> Vec<Installed> {
    let mut out = Vec::new();
    for l in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\XboxGames", l as char));
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let content = e.path().join("Content");
            if !content.is_dir() {
                continue;
            }
            let cfg = std::fs::read_to_string(content.join("MicrosoftGame.config")).unwrap_or_default();
            let (name, exe) = parse_ms_game_config(&cfg);
            out.push(Installed { name: name.unwrap_or_else(|| e.file_name().to_string_lossy().to_string()), store: Store::Xbox, id: String::new(), install_dir: content, exe });
        }
    }
    out
}

/// `MicrosoftGame.config` : (DefaultDisplayName, premier Executable Name).
pub fn parse_ms_game_config(xml: &str) -> (Option<String>, Option<String>) {
    let attr = |key: &str| -> Option<String> {
        let i = xml.find(key)? + key.len();
        let rest = &xml[i..];
        let q = rest.find('"')? + 1;
        let end = rest[q..].find('"')?;
        Some(rest[q..q + end].trim().to_string()).filter(|s| !s.is_empty() && !s.starts_with("ms-resource"))
    };
    let exe = attr("<Executable Name=").map(|e| e.rsplit(['\\', '/']).next().unwrap_or(&e).to_string());
    (attr("DefaultDisplayName="), exe)
}

/// Ubisoft Connect : `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<id>` (InstallDir).
#[cfg(windows)]
pub fn ubisoft_games() -> Vec<Installed> {
    use crate::registry::{read_string, subkeys, Hive};
    let key = "SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs";
    subkeys(Hive::LocalMachine, key)
        .into_iter()
        .filter_map(|id| {
            let dir = PathBuf::from(read_string(Hive::LocalMachine, &format!("{}\\{}", key, id), "InstallDir")?.trim().trim_end_matches(['/', '\\']).replace('/', "\\"));
            let name = dir.file_name()?.to_string_lossy().to_string();
            dir.is_dir().then_some(Installed { name, store: Store::Ubisoft, id, install_dir: dir, exe: None })
        })
        .collect()
}

/// EA app / Origin : `HKLM\SOFTWARE\(WOW6432Node\)Electronic Arts\<Jeu>` (« Install Dir »).
#[cfg(windows)]
pub fn ea_games() -> Vec<Installed> {
    use crate::registry::{read_string, subkeys, Hive};
    let mut out = Vec::new();
    for key in ["SOFTWARE\\WOW6432Node\\Electronic Arts", "SOFTWARE\\Electronic Arts"] {
        for sub in subkeys(Hive::LocalMachine, key) {
            let k = format!("{}\\{}", key, sub);
            let Some(dir) = read_string(Hive::LocalMachine, &k, "Install Dir").or_else(|| read_string(Hive::LocalMachine, &k, "InstallDir")) else { continue };
            let dir = PathBuf::from(dir.trim().trim_end_matches(['/', '\\']));
            let low = sub.to_lowercase();
            if !dir.is_dir() || low.contains("ea desktop") || low.contains("ea core") || low == "origin" {
                continue;
            }
            out.push(Installed { name: sub, store: Store::Ea, id: String::new(), install_dir: dir, exe: None });
        }
    }
    out
}

/// itch : dossier d'installation par defaut (`%APPDATA%\itch\apps\<jeu>`, `~/.config/itch/apps/<jeu>`).
pub fn itch_games() -> Vec<Installed> {
    let root = if cfg!(windows) { dirs::data_dir().map(|d| d.join("itch").join("apps")) } else { dirs::config_dir().map(|d| d.join("itch").join("apps")) };
    let Some(root) = root else { return vec![] };
    let Ok(rd) = std::fs::read_dir(&root) else { return vec![] };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| Installed { name: e.file_name().to_string_lossy().to_string(), store: Store::Itch, id: String::new(), install_dir: e.path(), exe: None })
        .collect()
}

/// Lutris (Linux) : table `games` de `~/.local/share/lutris/pga.db` (jeux installes, hors Steam deja vus).
#[cfg(not(windows))]
pub fn lutris_games() -> Vec<Installed> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let mut out = Vec::new();
    for db in [home.join(".local/share/lutris/pga.db"), home.join(".var/app/net.lutris.Lutris/data/lutris/pga.db")] {
        let Ok(d) = knowledge::sqlite::Db::open(&db) else { continue };
        for row in d.table("games").unwrap_or_default() {
            let txt = |k: &str| row.get(k).and_then(|v| v.as_text()).map(|s| s.to_string()).filter(|s| !s.is_empty());
            if row.get("installed").and_then(|v| v.as_i64()).unwrap_or(0) == 0 || txt("runner").as_deref() == Some("steam") {
                continue;
            }
            let (Some(name), Some(dir)) = (txt("name"), txt("directory")) else { continue };
            let dir = PathBuf::from(dir);
            if dir.is_dir() {
                out.push(Installed { name, store: Store::Lutris, id: txt("slug").unwrap_or_default(), install_dir: dir, exe: None });
            }
        }
    }
    out
}

// ------------------------------------------------------------------ logiciels installes

/// Logiciels installes : cles Uninstall (Windows, hors composants systeme et mises a jour) ; raccourcis `.desktop` (Linux).
pub fn installed_apps() -> Vec<AppEntry> {
    #[cfg(windows)]
    {
        uninstall_apps()
    }
    #[cfg(not(windows))]
    {
        desktop_apps()
    }
}

#[cfg(windows)]
fn uninstall_apps() -> Vec<AppEntry> {
    use crate::registry::{read_dword, read_string, subkeys, Hive};
    let mut out = Vec::new();
    for (hive, key) in [
        (Hive::LocalMachine, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (Hive::LocalMachine, "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (Hive::CurrentUser, "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ] {
        for sub in subkeys(hive, key) {
            let k = format!("{}\\{}", key, sub);
            let Some(name) = read_string(hive, &k, "DisplayName").map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) else { continue };
            // Composants systeme, mises a jour, correctifs : jamais listes.
            if read_dword(hive, &k, "SystemComponent") == Some(1) || read_string(hive, &k, "ParentKeyName").is_some() || read_string(hive, &k, "ReleaseType").map(|t| t.to_lowercase().contains("update") || t.to_lowercase().contains("hotfix")).unwrap_or(false) {
                continue;
            }
            // Entrees Steam (« Steam App 123 ») : le jeu est deja vu dans sa bibliotheque.
            if sub.to_lowercase().starts_with("steam app ") {
                continue;
            }
            let clean = |s: String| -> Option<PathBuf> {
                let s = s.trim().trim_matches('"').to_string();
                let s = s.split(',').next().unwrap_or("").trim().trim_matches('"').to_string();
                Some(PathBuf::from(s)).filter(|p| p.is_absolute())
            };
            let icon = read_string(hive, &k, "DisplayIcon").and_then(clean).filter(|p| {
                let l = p.to_string_lossy().to_lowercase();
                l.ends_with(".exe") && !l.contains("unins") && !l.contains("setup") && !l.contains("install")
            });
            let location = read_string(hive, &k, "InstallLocation").and_then(clean).filter(|p| crate::gamedetect::norm_path(p).trim_end_matches('/').matches('/').count() >= 2);
            out.push(AppEntry { name, publisher: read_string(hive, &k, "Publisher").filter(|p| !p.trim().is_empty()), location, exe: icon, system: false });
        }
    }
    out
}

#[cfg(not(windows))]
fn desktop_apps() -> Vec<AppEntry> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let data = crate::linuxapps::data_dirs(&home, std::env::var("XDG_DATA_HOME").ok().as_deref(), std::env::var("XDG_DATA_DIRS").ok().as_deref());
    let mut out = Vec::new();
    for e in crate::linuxapps::load_entries(&data) {
        let file = e.file.to_string_lossy().to_string();
        let stem = e.file.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let exe = crate::linuxapps::exec_target(&e.exec).and_then(|t| match t {
            crate::linuxapps::Target::Program(p) if p.starts_with('/') => Some(PathBuf::from(p)),
            crate::linuxapps::Target::Program(p) => crate::linuxapps::which(&p, &std::env::var("PATH").unwrap_or_default()),
            _ => None,
        });
        let system = file.starts_with("/usr/share/applications") && (stem.starts_with("org.gnome.") || stem.starts_with("org.kde.") || stem.starts_with("gnome-") || stem.starts_with("kde") || stem.contains("settings") || stem.contains("debian") || stem.contains("ubuntu"));
        out.push(AppEntry { name: e.name.clone(), publisher: None, location: exe.as_deref().and_then(Path::parent).map(Path::to_path_buf), exe, system });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_xbox() {
        let x = r#"<Game configVersion="1"><ExecutableList><Executable Name="Content\Game\Binaries\Hellblade2.exe" Id="Game" /></ExecutableList><ShellVisuals DefaultDisplayName="Senua's Saga: Hellblade II" PublisherDisplayName="Xbox Game Studios"/></Game>"#;
        assert_eq!(parse_ms_game_config(x), (Some("Senua's Saga: Hellblade II".into()), Some("Hellblade2.exe".into())));
        assert_eq!(parse_ms_game_config("<ShellVisuals DefaultDisplayName=\"ms-resource:Name\"/>"), (None, None));
        assert_eq!(parse_ms_game_config(""), (None, None));
    }
}
