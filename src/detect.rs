//! Detection des jeux installes (Steam, Epic, GOG) + rapprochement avec le catalogue des jeux.
//! Les parseurs sont purs (testables avec des fixtures) ; seules `detect_all` & co touchent la machine.
use crate::catalog::Catalog;
#[cfg(windows)]
use crate::registry;
use crate::Res;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    Steam,
    Epic,
    Gog,
    Xbox,
    Ubisoft,
    Ea,
    Itch,
    Lutris,
}

impl Store {
    pub fn label(&self) -> &'static str {
        match self {
            Store::Steam => "Steam",
            Store::Epic => "Epic",
            Store::Gog => "GOG",
            Store::Xbox => "Xbox",
            Store::Ubisoft => "Ubisoft",
            Store::Ea => "EA",
            Store::Itch => "itch",
            Store::Lutris => "Lutris",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub name: String,
    pub store: Store,
    /// appid Steam, AppName Epic ou gameID GOG.
    pub id: String,
    pub install_dir: PathBuf,
    /// Nom d'exe (sans chemin) si connu de facon fiable.
    pub exe: Option<String>,
}

// ---------------------------------------------------------------- VDF (Valve KeyValues)

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(v) => v.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            _ => None,
        }
    }
}

struct Lexer<'a> {
    c: std::iter::Peekable<std::str::Chars<'a>>,
}

#[derive(Debug, PartialEq)]
enum Tok {
    S(String),
    Open,
    Close,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Res<Option<Tok>> {
        loop {
            match self.c.peek().copied() {
                None => return Ok(None),
                Some(ch) if ch.is_whitespace() || ch == '\u{feff}' => {
                    self.c.next();
                }
                Some('/') => {
                    // commentaire // jusqu'a la fin de ligne
                    self.c.next();
                    if self.c.peek() == Some(&'/') {
                        while let Some(&ch) = self.c.peek() {
                            if ch == '\n' {
                                break;
                            }
                            self.c.next();
                        }
                    } else {
                        return Err("caractere '/' inattendu dans le VDF".into());
                    }
                }
                Some('{') => {
                    self.c.next();
                    return Ok(Some(Tok::Open));
                }
                Some('}') => {
                    self.c.next();
                    return Ok(Some(Tok::Close));
                }
                Some('"') => {
                    self.c.next();
                    let mut s = String::new();
                    loop {
                        match self.c.next() {
                            None => return Err("chaine VDF non terminee".into()),
                            Some('"') => break,
                            Some('\\') => match self.c.next() {
                                Some('n') => s.push('\n'),
                                Some('t') => s.push('\t'),
                                Some('\\') => s.push('\\'),
                                Some('"') => s.push('"'),
                                Some(o) => {
                                    s.push('\\');
                                    s.push(o);
                                }
                                None => return Err("echappement VDF tronque".into()),
                            },
                            Some(ch) => s.push(ch),
                        }
                    }
                    return Ok(Some(Tok::S(s)));
                }
                Some(ch) => return Err(format!("caractere inattendu '{}' dans le VDF", ch).into()),
            }
        }
    }
}

fn parse_map(lx: &mut Lexer, nested: bool) -> Res<Vdf> {
    let mut out = Vec::new();
    loop {
        match lx.next()? {
            None if nested => return Err("accolade fermante manquante".into()),
            None => return Ok(Vdf::Map(out)),
            Some(Tok::Close) if nested => return Ok(Vdf::Map(out)),
            Some(Tok::Close) => return Err("accolade fermante en trop".into()),
            Some(Tok::Open) => return Err("accolade ouvrante inattendue".into()),
            Some(Tok::S(key)) => match lx.next()? {
                Some(Tok::S(v)) => out.push((key, Vdf::Str(v))),
                Some(Tok::Open) => out.push((key, parse_map(lx, true)?)),
                _ => return Err(format!("valeur manquante pour la cle '{}'", key).into()),
            },
        }
    }
}

pub fn parse_vdf(text: &str) -> Res<Vdf> {
    parse_map(&mut Lexer { c: text.chars().peekable() }, false)
}

// ---------------------------------------------------------------- Steam

/// Chemins des bibliotheques listees dans libraryfolders.vdf (ancien et nouveau format).
pub fn parse_library_folders(text: &str) -> Res<Vec<PathBuf>> {
    let v = parse_vdf(text)?;
    let root = v.get("libraryfolders").ok_or("libraryfolders.vdf : racine 'libraryfolders' absente")?;
    let mut out = Vec::new();
    if let Vdf::Map(items) = root {
        for (k, val) in items {
            if k.parse::<u32>().is_err() {
                continue;
            }
            let p = match val {
                Vdf::Str(s) => Some(s.as_str()),
                m => m.get("path").and_then(|p| p.as_str()),
            };
            if let Some(p) = p {
                out.push(PathBuf::from(p));
            }
        }
    }
    Ok(out)
}

/// appmanifest_*.acf -> (appid, nom, installdir)
pub fn parse_acf(text: &str) -> Res<(String, String, String)> {
    let v = parse_vdf(text)?;
    let s = v.get("AppState").ok_or("acf : 'AppState' absent")?;
    let g = |k: &str| s.get(k).and_then(|x| x.as_str()).map(|x| x.to_string()).ok_or(format!("acf : champ '{}' absent", k));
    Ok((g("appid")?, g("name")?, g("installdir")?))
}

/// Dossier d'installation de Steam : registre HKCU puis chemins par defaut.
#[cfg(not(windows))]
pub fn steam_root() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    crate::proton::steam_root_candidates(&home).into_iter().find(|p| p.join("steamapps").is_dir())
}

#[cfg(windows)]
pub fn steam_root() -> Option<PathBuf> {
    if let Some(v) = registry::read_string(registry::Hive::CurrentUser, "Software\\Valve\\Steam", "SteamPath") {
        let p = PathBuf::from(v.trim().replace('/', "\\"));
        if p.is_dir() {
            return Some(p);
        }
    }
    ["C:\\Program Files (x86)\\Steam", "C:\\Program Files\\Steam"].iter().map(PathBuf::from).find(|p| p.is_dir())
}

/// Toutes les bibliotheques Steam existantes (la principale en premier).
pub fn steam_libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    #[cfg(not(windows))]
    {
        // Bibliotheques sur supports amovibles (carte SD du Steam Deck) : /run/media/<utilisateur>/<volume>.
        let mounts = crate::proton::list_media_mounts(Path::new("/run/media"));
        libs.extend(crate::proton::steam_media_libraries(&mounts));
    }
    if let Ok(t) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        if let Ok(v) = parse_library_folders(&t) {
            for p in v {
                let same = |l: &PathBuf| {
                    #[cfg(windows)]
                    {
                        l.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy())
                    }
                    #[cfg(not(windows))]
                    {
                        l == &p || l.canonicalize().ok().map(|a| Some(a) == p.canonicalize().ok()).unwrap_or(false)
                    }
                };
                if !libs.iter().any(same) && p.is_dir() {
                    libs.push(p);
                }
            }
        }
    }
    libs
}

pub fn steam_games(libs: &[PathBuf]) -> Vec<Installed> {
    let mut out = Vec::new();
    for lib in libs {
        let apps = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&apps) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_string();
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(e.path()) else { continue };
            let Ok((id, name, dir)) = parse_acf(&t) else { continue };
            if name.starts_with("Steamworks Common") || name.starts_with("Steam Linux Runtime") || name.starts_with("Proton") {
                continue;
            }
            out.push(Installed { name, store: Store::Steam, id, install_dir: apps.join("common").join(dir), exe: None });
        }
    }
    out
}

// ---------------------------------------------------------------- Epic

pub fn parse_epic_item(json: &str) -> Res<Installed> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("fichier .item invalide : {}", e))?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    let name = s("DisplayName").ok_or("DisplayName absent")?;
    let loc = s("InstallLocation").ok_or("InstallLocation absent")?;
    let exe = s("LaunchExecutable").filter(|e| !e.is_empty()).map(|e| e.rsplit(['/', '\\']).next().unwrap_or(&e).to_string());
    Ok(Installed {
        name,
        store: Store::Epic,
        id: s("AppName").or_else(|| s("CatalogItemId")).unwrap_or_default(),
        install_dir: PathBuf::from(loc),
        exe,
    })
}

pub fn epic_games(catalogs_dir: &Path) -> Vec<Installed> {
    let Ok(rd) = std::fs::read_dir(catalogs_dir) else { return vec![] };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "item").unwrap_or(false))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| parse_epic_item(&t).ok())
        .collect()
}

pub const EPIC_CATALOGS: &str = "C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Catalogs";

// ---------------------------------------------------------------- GOG

/// Sortie textuelle de `reg query HKLM\SOFTWARE\WOW6432Node\GOG.com\Games /s` (format de reference
/// pour les tests ; la detection reelle lit le registre directement, voir `gog_games`).
pub fn parse_gog_reg(output: &str) -> Vec<Installed> {
    let mut out = Vec::new();
    let mut cur: HashMap<String, String> = HashMap::new();
    let mut flush = |cur: &mut HashMap<String, String>| {
        if let Some(g) = gog_from_values(cur) {
            out.push(g);
        }
        cur.clear();
    };
    for line in output.lines() {
        if line.trim_start().starts_with("HKEY_") {
            flush(&mut cur);
        } else if let Some(i) = line.find("REG_SZ") {
            cur.insert(line[..i].trim().to_string(), line[i + 6..].trim().to_string());
        }
    }
    flush(&mut cur);
    out
}

/// Valeurs d'une sous-cle `...\GOG.com\Games\<id>` (gameName, gameID, path, exe) -> jeu installe.
pub fn gog_from_values(cur: &HashMap<String, String>) -> Option<Installed> {
    let (name, id, path) = (cur.get("gameName")?, cur.get("gameID")?, cur.get("path")?);
    let exe = cur
        .get("exe")
        .map(|e| e.rsplit(['/', '\\']).next().unwrap_or(e).to_string())
        .filter(|e| e.to_lowercase().ends_with(".exe"));
    Some(Installed { name: name.clone(), store: Store::Gog, id: id.clone(), install_dir: PathBuf::from(path), exe })
}

/// goggame-<id>.info : exe principal (playTasks isPrimary).
pub fn parse_gog_info_exe(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let tasks = v.get("playTasks")?.as_array()?;
    let t = tasks.iter().find(|t| t.get("isPrimary").and_then(|x| x.as_bool()).unwrap_or(false)).or_else(|| tasks.first())?;
    let p = t.get("path")?.as_str()?;
    Some(p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())
}

pub fn gog_games() -> Vec<Installed> {
    #[cfg(not(windows))]
    {
        Vec::new()
    }
    #[cfg(windows)]
    gog_games_windows()
}

#[cfg(windows)]
fn gog_games_windows() -> Vec<Installed> {
    // Lecture directe du registre (aucun sous-processus) : une sous-cle par jeu.
    const GOG_KEY: &str = "SOFTWARE\\WOW6432Node\\GOG.com\\Games";
    let mut v: Vec<Installed> = registry::subkeys(registry::Hive::LocalMachine, GOG_KEY)
        .iter()
        .filter_map(|sub| {
            let path = format!("{}\\{}", GOG_KEY, sub);
            let mut vals = HashMap::new();
            for k in ["gameName", "gameID", "path", "exe"] {
                if let Some(s) = registry::read_string(registry::Hive::LocalMachine, &path, k) {
                    vals.insert(k.to_string(), s);
                }
            }
            gog_from_values(&vals)
        })
        .collect();
    for g in &mut v {
        if g.exe.is_none() {
            if let Ok(rd) = std::fs::read_dir(&g.install_dir) {
                for e in rd.filter_map(|e| e.ok()) {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    if n.starts_with("goggame-") && n.ends_with(".info") {
                        if let Some(x) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_gog_info_exe(&t)) {
                            g.exe = Some(x);
                        }
                    }
                }
            }
        }
    }
    v
}

// ---------------------------------------------------------------- Global

pub struct Detection {
    pub steam_libraries: Vec<PathBuf>,
    pub games: Vec<Installed>,
}

// ---------------------------------------------------------------- Heroic (Linux : Epic, GOG et jeux ajoutes a la main)

/// Dossiers de configuration de Heroic (installation native puis Flatpak).
pub fn heroic_config_dirs(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".config/heroic"), home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic")]
}

/// Jeux installes avec Heroic : Epic (`legendaryConfig/legendary/installed.json`) et GOG (`gog_store/installed.json`).
/// Lecture seule ; fichier absent ou illisible = aucun jeu.
pub fn heroic_games(home: &Path) -> Vec<Installed> {
    let mut out: Vec<Installed> = Vec::new();
    let read = |p: PathBuf| -> Option<serde_json::Value> { serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok() };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string()).filter(|x| !x.is_empty());
    let exe_of = |e: Option<String>| e.map(|e| e.rsplit(['/', '\\']).next().unwrap_or(&e).to_string()).filter(|e| !e.is_empty());
    for dir in heroic_config_dirs(home) {
        if let Some(serde_json::Value::Object(m)) = read(dir.join("legendaryConfig/legendary/installed.json")) {
            for (id, g) in m {
                let (Some(name), Some(path)) = (s(&g, "title"), s(&g, "install_path")) else { continue };
                out.push(Installed { name, store: Store::Epic, id: s(&g, "app_name").unwrap_or(id), install_dir: PathBuf::from(path), exe: exe_of(s(&g, "executable")) });
            }
        }
        if let Some(v) = read(dir.join("gog_store/installed.json")) {
            for g in v.get("installed").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                let (Some(id), Some(path)) = (s(&g, "appName"), s(&g, "install_path")) else { continue };
                let p = PathBuf::from(&path);
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| id.clone());
                let exe = std::fs::read_to_string(p.join(format!("goggame-{}.info", id))).ok().and_then(|t| parse_gog_info_exe(&t));
                out.push(Installed { name, store: Store::Gog, id, install_dir: p, exe });
            }
        }
    }
    out.retain(|g| !is_non_game(g));
    out
}

pub fn detect_all() -> Detection {
    let libs = steam_root().map(|r| steam_libraries(&r)).unwrap_or_default();
    let mut games = steam_games(&libs);
    games.extend(epic_games(Path::new(EPIC_CATALOGS)));
    games.extend(gog_games());
    #[cfg(not(windows))]
    if let Some(h) = dirs::home_dir() {
        games.extend(heroic_games(&h));
    }
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Detection { steam_libraries: libs, games }
}

/// Filtre des "non-elements" listes par les lanceurs (moteurs, plugins, clients...).
pub fn is_non_game(g: &Installed) -> bool {
    let n = g.name.to_lowercase();
    let id = g.id.to_lowercase();
    const KEYWORDS: [&str; 11] = ["unreal engine", "plugin", "discord", "epic games launcher", "launcher", "redistributable", "directx", "easyanticheat", "easy anti-cheat", "battleye", "visual c++"];
    n == "fab" || n.starts_with("fab ") || KEYWORDS.iter().any(|k| n.contains(k)) || id.starts_with("ue_") || id.starts_with("fabplugin") || id.contains("plugin")
}

/// Heuristique : plus gros .exe a la racine du dossier d'installation (hors installeurs/crash handlers).
pub fn guess_exe(dir: &Path) -> Option<String> {
    const JUNK: [&str; 8] = ["unins", "crash", "redist", "setup", "vc_", "dxsetup", "dotnet", "uninstall"];
    let mut best: Option<(u64, String)> = None;
    for e in std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()) {
        let n = e.file_name().to_string_lossy().to_string();
        let l = n.to_lowercase();
        if !l.ends_with(".exe") || JUNK.iter().any(|j| l.contains(j)) {
            continue;
        }
        let sz = e.metadata().map(|m| m.len()).unwrap_or(0);
        if best.as_ref().map(|b| sz > b.0).unwrap_or(true) {
            best = Some((sz, n));
        }
    }
    // Linux : jeu natif (aucun .exe) -> programme ELF principal du dossier.
    #[cfg(not(windows))]
    if best.is_none() {
        return crate::linuxapps::main_executable(dir, &[]).and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
    }
    best.map(|b| b.1)
}

// ---------------------------------------------------------------- Rapprochement catalog

pub fn norm_name(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

pub struct CatalogIndex<'a> {
    by_steam: HashMap<u64, &'a String>,
    by_gog: HashMap<u64, &'a String>,
    by_name: HashMap<String, &'a String>,
}

impl<'a> CatalogIndex<'a> {
    pub fn new(m: &'a Catalog) -> Self {
        let mut i = CatalogIndex { by_steam: HashMap::new(), by_gog: HashMap::new(), by_name: HashMap::new() };
        for (k, e) in m {
            if let Some(id) = e.steam.as_ref().and_then(|s| s.id) {
                i.by_steam.entry(id).or_insert(k);
            }
            if let Some(id) = e.gog.as_ref().and_then(|s| s.id) {
                i.by_gog.entry(id).or_insert(k);
            }
            i.by_name.entry(norm_name(k)).or_insert(k);
        }
        i
    }
    /// Rapproche par id de boutique (Steam/GOG) puis par nom normalise.
    pub fn find(&self, g: &Installed) -> Option<&'a String> {
        let by_id = match g.store {
            Store::Steam => g.id.parse::<u64>().ok().and_then(|id| self.by_steam.get(&id)),
            Store::Gog => g.id.parse::<u64>().ok().and_then(|id| self.by_gog.get(&id)),
            _ => None,
        };
        by_id.copied().or_else(|| self.by_name.get(&norm_name(&g.name)).copied())
    }
}
