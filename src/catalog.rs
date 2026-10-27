//! Catalogue des jeux : lecture, recherche, resolution des             next = cur.iter().filter_map(|b| join_existing(b, part)).collect();laceholders.
use crate::{Ctx, Res};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Deserialize, Debug, Default)]
pub struct When {
    #[serde(default)]
    pub os: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct FileEntry {
    #[serde(default)]
    pub when: Vec<When>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct Steam {
    #[serde(default)]
    pub id: Option<u64>,
}

#[derive(Deserialize, Debug, Default)]
pub struct Entry {
    #[serde(default)]
    pub files: HashMap<String, Option<FileEntry>>,
    #[serde(default, rename = "installDir")]
    pub install_dir: HashMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    pub steam: Option<Steam>,
    #[serde(default)]
    pub gog: Option<Steam>,
    /// Executables de lancement (cles de `launch`, tous systemes : on ne garde que les .exe).
    #[serde(default)]
    pub launch: HashMap<String, serde::de::IgnoredAny>,
}

pub type Catalog = BTreeMap<String, Entry>;

pub fn cache_path(ctx: &Ctx) -> PathBuf {
    ctx.data_dir.join("catalog.yaml")
}

pub fn parse(bytes: &[u8]) -> Res<Catalog> {
    let m: Catalog = serde_yaml::from_slice(bytes).map_err(|e| format!("catalog invalide : {}", e))?;
    if m.len() < 100 {
        return Err(format!("catalog suspect ({} elements seulement) : rejete", m.len()).into());
    }
    Ok(m)
}

pub fn load_cached(ctx: &Ctx) -> Res<Catalog> {
    let p = cache_path(ctx);
    let b = std::fs::read(&p).map_err(|_| "pas de cache : lancez d'abord `AutoGameSoftwareSync catalog update`".to_string())?;
    parse(&b)
}

/// Recherche insensible a la casse : exacts d'abord, puis sous-chaines.
pub fn search<'a>(m: &'a Catalog, q: &str) -> Vec<(&'a String, &'a Entry)> {
    let ql = q.to_lowercase();
    let mut exact = Vec::new();
    let mut part = Vec::new();
    for (k, v) in m {
        let kl = k.to_lowercase();
        if kl == ql {
            exact.push((k, v));
        } else if kl.contains(&ql) {
            part.push((k, v));
        }
    }
    exact.extend(part);
    exact
}

/// Chemins Windows (patterns bruts) d'une entree.
pub fn windows_patterns(e: &Entry) -> Vec<String> {
    let mut v: Vec<String> = e
        .files
        .iter()
        .filter(|(_, fe)| match fe {
            None => true,
            Some(fe) => fe.when.is_empty() || fe.when.iter().any(|w| w.os.as_deref().map_or(true, |o| o == "windows")),
        })
        .map(|(p, _)| p.clone())
        .collect();
    v.sort();
    v
}

#[derive(Debug, Default)]
pub struct Resolved {
    /// Chemins concrets (peuvent contenir des jokers * ?).
    pub paths: Vec<String>,
    /// Placeholders non resolus (le chemin correspondant est ignore).
    pub unresolved: Vec<String>,
}

fn env_path(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|s| !s.is_empty())
}

/// Informations de boutique utilisees pour resoudre <root>, <base>, <storeGameId>.
#[derive(Debug, Default, Clone)]
pub struct StoreCtx {
    /// Bibliotheques Steam reelles (la principale en premier).
    pub steam_libs: Vec<PathBuf>,
    /// Dossier d'installation detecte du jeu (toutes boutiques).
    pub install_dir: Option<PathBuf>,
    /// Identifiant boutique detecte du jeu.
    pub game_id: Option<String>,
}

impl StoreCtx {
    /// Detection reelle des bibliotheques Steam de la machine.
    pub fn auto() -> Self {
        StoreCtx { steam_libs: crate::detect::steam_root().map(|r| crate::detect::steam_libraries(&r)).unwrap_or_default(), ..Default::default() }
    }
}

fn s_(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(windows)]
const SEP: &str = "\\";
#[cfg(windows)]
const DOUBLE_SEP: &str = "\\\\";
#[cfg(not(windows))]
const SEP: &str = "/";
#[cfg(not(windows))]
const DOUBLE_SEP: &str = "//";

/// Linux : prefixe Proton du jeu (AppID connu, bibliotheques Steam detectees).
#[cfg(not(windows))]
fn proton_prefix(entry: &Entry, sc: &StoreCtx) -> Option<PathBuf> {
    let appid = sc.game_id.as_deref().and_then(|g| g.parse::<u64>().ok()).or_else(|| entry.steam.as_ref().and_then(|s| s.id))?;
    crate::proton::find_prefix(&sc.steam_libs, appid)
}

/// Linux : les marqueurs Windows du catalogue pointent dans le prefixe Proton du jeu (vide si inconnu).
#[cfg(not(windows))]
fn linux_candidates(ph: &str, entry: &Entry, sc: &StoreCtx) -> Option<Vec<String>> {
    let known = ["home", "winAppData", "winLocalAppData", "winDocuments", "winPublic", "winProgramData", "winDir", "osUserName"];
    if !known.contains(&ph) {
        return None;
    }
    let pfx = proton_prefix(entry, sc);
    Some(match (ph, pfx) {
        ("osUserName", _) => vec![crate::proton::PROTON_USER.to_string()],
        (_, Some(p)) => crate::proton::placeholder_base(ph, &p).map(|b| vec![s_(&b)]).unwrap_or_default(),
        _ => vec![],
    })
}

/// Linux : un chemin Windows en dur (`C:\...`) est traduit dans le prefixe ; un chemin deja Linux est conserve.
#[cfg(not(windows))]
fn linux_finish(p: &str, entry: &Entry, sc: &StoreCtx) -> Option<String> {
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        let pfx = proton_prefix(entry, sc)?;
        return crate::proton::translate_windows_path(p, &pfx).map(|x| s_(&x));
    }
    Some(p.to_string())
}

/// Valeurs candidates d'un placeholder (vide = non resolu).
fn candidates(ph: &str, entry: &Entry, game_name: &str, sc: &StoreCtx) -> Vec<String> {
    #[cfg(not(windows))]
    if let Some(v) = linux_candidates(ph, entry, sc) {
        return v;
    }
    let s = |p: Option<PathBuf>| p.map(|p| vec![p.to_string_lossy().into_owned()]).unwrap_or_default();
    match ph {
        "home" => s(dirs::home_dir()),
        "winAppData" => s(dirs::data_dir()),
        "winLocalAppData" => s(dirs::data_local_dir()),
        "winDocuments" => s(dirs::document_dir()),
        "winPublic" => env_path("PUBLIC").map(|p| vec![p]).unwrap_or_else(|| vec!["C:\\Users\\Public".into()]),
        "winProgramData" => env_path("ProgramData").into_iter().collect(),
        "winDir" => env_path("SystemRoot").into_iter().collect(),
        "osUserName" => env_path("USERNAME").into_iter().collect(),
        "storeUserId" => vec!["*".into()],
        "storeGameId" => sc
            .game_id
            .clone()
            .or_else(|| entry.steam.as_ref().and_then(|s| s.id).map(|i| i.to_string()))
            .or_else(|| entry.gog.as_ref().and_then(|s| s.id).map(|i| i.to_string()))
            .into_iter()
            .collect(),
        "root" => {
            let mut v: Vec<String> = sc.steam_libs.iter().map(|p| s_(p)).collect();
            if let Some(p) = sc.install_dir.as_ref().and_then(|d| d.parent()) {
                // Hors Steam, le "root" est le dossier parent de l'installation.
                if !sc.steam_libs.iter().any(|l| p.starts_with(l)) {
                    v.push(s_(p));
                }
            }
            v
        }
        "game" => {
            if entry.install_dir.is_empty() {
                vec![game_name.to_string()]
            } else {
                entry.install_dir.keys().cloned().collect()
            }
        }
        "base" => {
            if let Some(d) = &sc.install_dir {
                return vec![s_(d)];
            }
            let games = candidates("game", entry, game_name, sc);
            let mut v = Vec::new();
            for r in &sc.steam_libs {
                for g in &games {
                    v.push(s_(&r.join("steamapps").join("common").join(g)));
                }
            }
            // On ne garde que les dossiers qui existent (sinon le premier, pour l'affichage).
            let existing: Vec<String> = v.iter().filter(|p| std::path::Path::new(p).is_dir()).cloned().collect();
            if existing.is_empty() { v.into_iter().take(1).collect() } else { existing }
        }
        _ => vec![],
    }
}

pub fn resolve(entry: &Entry, game_name: &str) -> Resolved {
    resolve_with(entry, game_name, &StoreCtx::auto())
}

pub fn resolve_with(entry: &Entry, game_name: &str, sc: &StoreCtx) -> Resolved {
    let mut res = Resolved::default();
    for pat in windows_patterns(entry) {
        let mut results = vec![pat.replace('/', SEP)];
        let mut ok = true;
        // Remplace chaque placeholder <xxx> (produit cartesien des candidats).
        while let Some(cur) = results.first().cloned() {
            let (Some(a), Some(b)) = (cur.find('<'), cur.find('>')) else { break };
            if b < a {
                break;
            }
            let ph = cur[a + 1..b].to_string();
            let cands = candidates(&ph, entry, game_name, sc);
            if cands.is_empty() {
                if !res.unresolved.contains(&ph) {
                    res.unresolved.push(ph);
                }
                ok = false;
                break;
            }
            let tag = format!("<{}>", ph);
            results = results
                .iter()
                .flat_map(|r| cands.iter().map(|c| r.replacen(&tag, &c.replace('/', SEP), 1)).collect::<Vec<_>>())
                .collect();
        }
        if ok {
            for r in results {
                let r = r.replace(DOUBLE_SEP, SEP);
                #[cfg(not(windows))]
                let Some(r) = linux_finish(&r, entry, sc) else { continue };
                if !res.paths.contains(&r) {
                    res.paths.push(r);
                }
            }
        }
    }
    res
}

fn wild_match(p: &[char], t: &[char]) -> bool {
    match (p.first(), t.first()) {
        (None, None) => true,
        (Some(&'*'), _) => wild_match(&p[1..], t) || (!t.is_empty() && wild_match(p, &t[1..])),
        (Some(&'?'), Some(_)) => wild_match(&p[1..], &t[1..]),
        (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => wild_match(&p[1..], &t[1..]),
        _ => false,
    }
}

/// Developpe un chemin contenant des jokers en chemins existants.
pub fn expand_glob(pattern: &str) -> Vec<PathBuf> {
    let sep_char = SEP.chars().next().unwrap_or('\\');
    let mut parts = pattern.split(sep_char);
    let Some(first) = parts.next() else { return vec![] };
    let start = if first.ends_with(':') {
        format!("{}{}", first, SEP)
    } else if first.is_empty() && !cfg!(windows) {
        SEP.to_string()
    } else {
        first.to_string()
    };
    let mut cur = vec![PathBuf::from(start)];
    for part in parts.filter(|p| !p.is_empty()) {
        let mut next = Vec::new();
        if part.contains('*') || part.contains('?') {
            let pc: Vec<char> = part.chars().collect();
            for base in &cur {
                if let Ok(rd) = std::fs::read_dir(base) {
                    for e in rd.filter_map(|e| e.ok()) {
                        let n: Vec<char> = e.file_name().to_string_lossy().chars().collect();
                        if wild_match(&pc, &n) {
                            next.push(e.path());
                        }
                    }
                }
            }
        } else {
            next = cur.iter().filter_map(|b| join_existing(b, part)).collect();
        }
        cur = next;
        if cur.is_empty() {
            break;
        }
    }
    cur.retain(|p| p.exists());
    cur
}

/// "1,3", "tous"/"all"/"a" -> indices 0-based. Vide -> aucun choix (annulation).
pub fn parse_selection(input: &str, n: usize) -> Res<Vec<usize>> {
    let t = input.trim().to_lowercase();
    if t.is_empty() {
        return Ok(vec![]);
    }
    if ["tous", "all", "a", "t", "*"].contains(&t.as_str()) {
        return Ok((0..n).collect());
    }
    let mut out = Vec::new();
    for part in t.split([',', ' ']).filter(|p| !p.is_empty()) {
        let i: usize = part.parse().map_err(|_| format!("choix invalide : '{}'", part))?;
        if i == 0 || i > n {
            return Err(format!("numero hors limites : {} (1..{})", i, n).into());
        }
        if !out.contains(&(i - 1)) {
            out.push(i - 1);
        }
    }
    Ok(out)
}

/// Dossiers trop generiques pour etre sauvegardes en entier (home, Documents, AppData, racine de disque...).
pub fn is_generic_root(p: &std::path::Path) -> bool {
    let norm = |x: &std::path::Path| x.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    let n = norm(p);
    if p.parent().is_none() || n.ends_with(':') {
        return true;
    }
    let mut set: Vec<String> = Vec::new();
    for d in [dirs::home_dir(), dirs::document_dir(), dirs::data_dir(), dirs::data_local_dir(), dirs::desktop_dir()].into_iter().flatten() {
        set.push(norm(&d));
    }
    if let Some(h) = dirs::home_dir() {
        set.push(norm(&h.join("Saved Games")));
        set.push(norm(&h.join("AppData")));
        set.push(norm(&h.join("AppData").join("LocalLow")));
    }
    if let Some(l) = dirs::data_local_dir() {
        set.push(norm(&l.join("Packages")));
    }
    set.push("c:\\users\\public".into());
    set.contains(&n)
}

/// Catalog a utiliser : le cache developpeur (`catalog update`) s'il existe, sinon la base EMBARQUEE (+ overrides locales).
pub fn load_best(ctx: &Ctx) -> Res<Catalog> {
    if cache_path(ctx).is_file() {
        return load_cached(ctx);
    }
    Ok(crate::gamedb::embedded(&ctx.data_dir).to_catalog())
}

/// `base/part` s'il existe. Sous Linux le systeme de fichiers distingue la casse alors que Windows (et donc les
/// chemins du catalogue) non : on cherche aussi le composant sans tenir compte de la casse.
fn join_existing(base: &std::path::Path, part: &str) -> Option<PathBuf> {
    let p = base.join(part);
    if p.exists() {
        return Some(p);
    }
    #[cfg(not(windows))]
    {
        let low = part.to_lowercase();
        return std::fs::read_dir(base).ok()?.filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().to_lowercase() == low).map(|e| e.path());
    }
    #[cfg(windows)]
    None
}
