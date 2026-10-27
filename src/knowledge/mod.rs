//! Connaissance intelligente : trouve tout seul ou un programme (jeu ou logiciel) garde ses sauvegardes et ses reglages
//! quand ni la base embarquee ni les profils ne le savent. Quatre couches, chacune utilisee seulement si la precedente n'a
//! rien donne :
//! 1. `launchers` : caches locaux des lanceurs (Steam, GOG Galaxy, Epic) et liste des logiciels installes ;
//! 2. `deduce` : deduction locale (infos de version, nom du dossier, editeur) dans les dossiers perso ;
//! 3. `learn` : apprentissage des ecritures pendant que le programme tourne ;
//! 4. `web` : recherche web discrete, verifiee sur le disque.
//!
//! Un emplacement accepte par l'utilisateur part dans une file locale (`queue`), envoyee au serveur avec le consentement
//! aux donnees d'usage ; les emplacements confirmes par plusieurs installations forment la base maison (`home`), consultee
//! juste apres la base embarquee et les profils.
pub mod appinfo;
pub mod deduce;
pub mod home;
pub mod launchers;
pub mod learn;
pub mod paths;
pub mod queue;
pub mod sqlite;
pub mod store;
pub mod web;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Couche qui a trouve l'emplacement.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Launcher,
    Deduce,
    Learn,
    Web,
    /// Base maison (emplacements confirmes par d'autres installations).
    Home,
}

impl Layer {
    pub fn code(self) -> &'static str {
        match self {
            Layer::Launcher => "launcher",
            Layer::Deduce => "deduce",
            Layer::Learn => "learn",
            Layer::Web => "web",
            Layer::Home => "home",
        }
    }
}

/// Sauvegardes ou reglages.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Save,
    Config,
}

/// Un emplacement trouve, verifie sur CE PC.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    /// Gabarit generique (`%APPDATA%/Studio/Jeu`), absent si le chemin n'est sous aucun dossier de base.
    #[serde(default)]
    pub template: Option<String>,
    pub kind: Kind,
    pub layer: Layer,
    /// Confiance 0-100.
    pub confidence: u8,
    /// Detail lisible pour le journal (« Steam : sauvegarde cloud automatique »...). Jamais affiche tel quel.
    #[serde(default)]
    pub source: String,
}

/// Niveau de confiance affiche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    High,
    Medium,
    Low,
}

pub fn level(confidence: u8) -> Level {
    match confidence {
        80..=100 => Level::High,
        60..=79 => Level::Medium,
        _ => Level::Low,
    }
}

/// Fenetre de fraicheur : un dossier n'est retenu que s'il contient un fichier modifie depuis ce nombre de jours.
pub const RECENT_DAYS: u64 = 180;

/// Ce que l'on sait d'un programme (rien n'est tape a la main : exe, infos de version, lanceurs).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Program {
    /// Cle de cache : nom de l'exe en minuscules.
    pub key: String,
    pub exe: String,
    #[serde(default)]
    pub exe_path: Option<PathBuf>,
    /// Nom affiche (base, carte de detection ou nom deduit de l'exe).
    pub name: String,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub install_dir: Option<PathBuf>,
    #[serde(default)]
    pub steam_appid: Option<u64>,
    #[serde(default)]
    pub steam_name: Option<String>,
    /// Editeur donne par un lanceur ou la liste des logiciels installes.
    #[serde(default)]
    pub publisher: Option<String>,
    /// Developpeur donne par un lanceur (souvent le nom du dossier « studio » des sauvegardes).
    #[serde(default)]
    pub developer: Option<String>,
    /// Titre donne par un lanceur ou la liste des logiciels installes.
    #[serde(default)]
    pub title: Option<String>,
    /// Logiciel (reglages) plutot que jeu (sauvegardes).
    #[serde(default)]
    pub app: bool,
    /// Linux : prefixe Proton / Wine du programme Windows.
    #[serde(default)]
    pub prefix: Option<PathBuf>,
}

/// Noms de produit generiques (moteur, environnement d'execution) : jamais pris pour le nom du programme.
const GENERIC_PRODUCTS: &[&str] = &[
    "unity", "unityplayer", "unrealengine", "unreal", "bootstrappackagedgame", "godotengine", "godot", "electron", "java", "javaplatformse", "openjdkplatformbinary", "nwjs", "chromium", "python",
    "microsoftwindowsoperatingsystem", "windows", "launcher", "game", "application", "app", "setup", "client", "nodejs", "cefsharp", "qt", "monogame", "xna", "renpy", "love", "gamemaker", "rpgmaker",
];

/// Editeurs generiques (moteur, outil) : ne designent pas l'editeur du programme.
const GENERIC_COMPANIES: &[&str] = &["unitytechnologies", "unitytechnologiesaps", "defaultcompany", "epicgames", "epicgamesinc", "godotengine", "electron", "github", "githubinc", "openjsfoundation", "thequtcompany", "theqtcompany", "nwjs", "yoyogames"];

/// Nom compare sans casse, espaces ni ponctuation.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

const STOP: &[&str] = &["the", "of", "and", "a", "an", "de", "la", "le", "les", "du", "edition", "remastered", "definitive", "goty", "enhanced", "deluxe", "complete", "x64", "x86", "win64", "win32", "64", "32"];

fn tokens(s: &str) -> Vec<String> {
    // Les chiffres seuls sont gardes : ils distinguent les suites (« Graveyard Keeper 2 »).
    s.split(|c: char| !c.is_alphanumeric()).map(|t| t.to_lowercase()).filter(|t| (t.len() >= 2 || t.chars().all(|c| c.is_ascii_digit())) && !t.is_empty() && !STOP.contains(&t.as_str())).collect()
}

/// Reste d'un nom plus long qui designe un AUTRE produit (suite numerotee, demo, serveur, editeur...).
fn other_product(rest: &str) -> bool {
    rest.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) || ["demo", "server", "dedicated", "editor", "beta", "playtest", "launcher", "tool", "mod", "sdk", "test"].iter().any(|w| rest.contains(w))
}

fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Correspondance floue mais PRUDENTE entre un nom de dossier et un nom de programme : 100 identique (sans casse ni
/// ponctuation), 85 l'un commence / finit par l'autre (longueurs proches), 80 memes mots, 70 une seule faute de frappe.
pub fn name_match(dir: &str, name: &str) -> Option<u8> {
    let (a, b) = (norm(dir), norm(name));
    if a.chars().count() < 3 || b.chars().count() < 3 {
        return None;
    }
    if a == b {
        return Some(100);
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    let ratio = la.min(lb) as f32 / la.max(lb) as f32;
    if la.min(lb) >= 6 && ratio >= 0.6 {
        let (long, short) = if la >= lb { (&a, &b) } else { (&b, &a) };
        // « graveyardkeeper2demo » commence par « graveyardkeeper » mais c'est un autre produit.
        if long.starts_with(short.as_str()) && !other_product(&long[short.len()..]) {
            return Some(85);
        }
        if long.ends_with(short.as_str()) && !other_product(&long[..long.len() - short.len()]) {
            return Some(85);
        }
    }
    let (ta, tb) = (tokens(dir), tokens(name));
    let same_set = |x: &[String], y: &[String]| x.iter().all(|t| y.contains(t)) && y.iter().all(|t| x.contains(t));
    if tb.len() >= 2 && same_set(&ta, &tb) {
        return Some(80);
    }
    let digits = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    if la.min(lb) >= 7 && digits(&a) == digits(&b) && levenshtein(&a, &b) <= 1 {
        return Some(70);
    }
    None
}

/// Meilleure correspondance d'un nom de dossier avec une liste de noms.
pub fn best_match(dir: &str, names: &[String]) -> Option<u8> {
    names.iter().filter_map(|n| name_match(dir, n)).max()
}

/// Nom nettoye : symboles de marque retires, suffixes techniques d'exe retires (`-Win64-Shipping`, `_x64`...).
pub fn clean_name(s: &str) -> String {
    let mut t: String = s.replace(['™', '®', '©'], "").trim().to_string();
    let low = t.to_lowercase();
    for suf in ["-win64-shipping", "-wingdk-shipping", "-win32-shipping", "-shipping", "_x64", "-x64", "_win64", "-win64", "_dx11", "_dx12", " (64-bit)", " (32-bit)", " x64"] {
        if low.ends_with(suf) && low.len() > suf.len() + 2 {
            t.truncate(t.len() - suf.len());
            break;
        }
    }
    t.trim().to_string()
}

/// Nom d'entreprise sans forme juridique (« Team Cherry Pty Ltd » -> « Team Cherry »).
pub fn clean_company(s: &str) -> String {
    let mut t = clean_name(s);
    loop {
        let low = t.to_lowercase();
        let mut cut = None;
        for suf in [", inc.", ", inc", " inc.", " inc", " llc", " ltd.", " ltd", " limited", " gmbh", " s.a.", " sa", " sas", " sarl", " co.", " corporation", " corp.", " corp", " ab", " oy", " b.v.", " bv", " pty", " s.r.o.", " srl", " k.k.", " co., ltd", ","] {
            if low.ends_with(suf) && low.len() > suf.len() + 1 {
                cut = Some(t.len() - suf.len());
                break;
            }
        }
        match cut {
            Some(n) => t = t[..n].trim().to_string(),
            None => break,
        }
    }
    t
}

impl Program {
    pub fn key_of(exe: &str) -> String {
        crate::hints::exe_file_name(exe).to_lowercase()
    }

    /// Noms candidats du programme (dedoublonnes, generiques retires), du plus fiable au moins fiable.
    pub fn names(&self) -> Vec<String> {
        let stem = crate::hints::exe_file_name(&self.exe);
        let stem = stem.rsplit_once('.').filter(|(_, e)| e.len() <= 4).map(|(s, _)| s.to_string()).unwrap_or(stem);
        let dir = self.install_dir.as_ref().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string());
        let mut out: Vec<String> = Vec::new();
        for n in [Some(self.name.clone()), self.product.clone(), self.steam_name.clone(), self.title.clone(), dir, Some(stem)].into_iter().flatten() {
            let c = clean_name(&n);
            let k = norm(&c);
            if k.chars().count() < 3 || GENERIC_PRODUCTS.contains(&k.as_str()) || crate::gamedetect::is_generic_name(&format!("{}.exe", c.to_lowercase())) {
                continue;
            }
            if !out.iter().any(|o| norm(o) == k) {
                out.push(c);
            }
        }
        out
    }

    /// Editeurs candidats (infos de version, lanceurs), formes juridiques retirees.
    pub fn companies(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in [self.company.clone(), self.developer.clone(), self.publisher.clone()].into_iter().flatten() {
            for v in [clean_company(&c), c.trim().to_string()] {
                let k = norm(&v);
                if k.chars().count() < 2 || GENERIC_COMPANIES.contains(&k.as_str()) {
                    continue;
                }
                if !out.iter().any(|o| norm(o) == k) {
                    out.push(v);
                }
            }
        }
        out
    }

    /// Nom a utiliser pour une recherche : le plus parlant (produit ou nom Steam avant le nom de l'exe).
    pub fn search_name(&self) -> Option<String> {
        self.names().into_iter().next()
    }

    /// Dossiers de base de ce programme (Linux : bases Windows dans son prefixe Proton s'il en a un).
    pub fn folders(&self) -> paths::Folders {
        let f = paths::Folders::real();
        match &self.prefix {
            Some(p) => f.with_prefix(p),
            None => f,
        }
    }

    /// Decrit un programme depuis la machine (lecture seule) : infos de version, dossier d'installation, AppID Steam,
    /// prefixe Proton (Linux).
    pub fn describe(exe_path: Option<&Path>, exe: &str, name: &str, app: bool) -> Program {
        let exe_name = crate::hints::exe_file_name(exe);
        let full = exe_path.filter(|p| p.is_absolute()).map(Path::to_path_buf).or_else(|| Some(PathBuf::from(exe.trim().trim_matches('"'))).filter(|p| p.is_absolute()));
        let (company, product) = full.as_deref().filter(|p| p.is_file()).map(crate::winutil::exe_company_product).unwrap_or((None, None));
        let mut p = Program { key: exe_name.to_lowercase(), exe: exe_name, exe_path: full.clone(), name: name.trim().to_string(), company, product, app, ..Default::default() };
        if let Some(f) = &full {
            p.install_dir = install_dir_of(f);
            if let Some((steamapps, dir)) = steam_install(f) {
                p.install_dir = Some(steamapps.join("common").join(&dir));
                p.steam_appid = crate::autofill::steam_app_id(&steamapps, &dir);
                if let Some(id) = p.steam_appid {
                    p.steam_name = std::fs::read_to_string(steamapps.join(format!("appmanifest_{}.acf", id))).ok().and_then(|t| crate::detect::parse_acf(&t).ok()).map(|x| x.1);
                    #[cfg(not(windows))]
                    if p.exe.to_lowercase().ends_with(".exe") {
                        let libs = crate::detect::steam_root().map(|r| crate::detect::steam_libraries(&r)).unwrap_or_default();
                        p.prefix = crate::proton::find_prefix(&libs, id);
                    }
                }
            }
        }
        p
    }
}

/// `.../steamapps/common/<dossier>/...` -> (dossier steamapps, nom du dossier d'installation).
pub fn steam_install(exe: &Path) -> Option<(PathBuf, String)> {
    let mut cur = exe.parent();
    while let Some(d) = cur {
        let parent = d.parent()?;
        if parent.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("common")).unwrap_or(false) {
            let steamapps = parent.parent()?;
            if steamapps.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("steamapps")).unwrap_or(false) {
                return Some((steamapps.to_path_buf(), d.file_name()?.to_string_lossy().to_string()));
            }
        }
        cur = d.parent();
    }
    None
}

/// Dossier d'installation probable : dossier de l'exe, en remontant les sous-dossiers techniques (`bin`, `Win64`...).
pub fn install_dir_of(exe: &Path) -> Option<PathBuf> {
    const TECH: &[&str] = &["bin", "bin64", "bin32", "binaries", "win64", "win32", "x64", "x86", "windows", "windowsnoeditor", "release", "retail", "shipping", "wingdk"];
    let mut d = exe.parent()?.to_path_buf();
    for _ in 0..4 {
        let n = d.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if TECH.contains(&n.as_str()) {
            match d.parent() {
                Some(p) => d = p.to_path_buf(),
                None => break,
            }
        } else {
            break;
        }
    }
    // Unreal : <Jeu>/<Projet>/Binaries/Win64 -> <Jeu>.
    if let Some(p) = d.parent() {
        if exe.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("binaries")) && p.join("Engine").is_dir() {
            d = p.to_path_buf();
        }
    }
    Some(d)
}

/// Sauvegarde ou reglage d'apres le chemin (mots usuels), sinon selon la nature du programme.
pub fn kind_for(path_text: &str, app: bool) -> Kind {
    let l = path_text.to_lowercase();
    let save = ["save", "saves", "savegame", "savegames", "savedata", "saved games", "profiles", "slot", "remote", "storage", "progress"];
    let conf = ["config", "configs", "settings", "prefs", "preferences", "options", "cfg", ".ini", "keybind"];
    let last = l.rsplit(['/', '\\']).next().unwrap_or("");
    if save.iter().any(|w| last.contains(w)) {
        return Kind::Save;
    }
    if conf.iter().any(|w| last.contains(w)) {
        return Kind::Config;
    }
    if conf.iter().any(|w| l.contains(w)) {
        return Kind::Config;
    }
    if save.iter().any(|w| l.contains(w)) {
        return Kind::Save;
    }
    if app {
        Kind::Config
    } else {
        Kind::Save
    }
}

/// Garde un seul emplacement par chemin (meilleure confiance) et retire ceux contenus dans un autre retenu ; trie par
/// confiance decroissante.
pub fn dedupe(mut v: Vec<Found>) -> Vec<Found> {
    v.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    let mut out: Vec<Found> = Vec::new();
    let key = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    for f in v {
        let k = key(&f.path);
        if out.iter().any(|o| {
            let ok = key(&o.path);
            ok == k || k.starts_with(&format!("{}/", ok)) || ok.starts_with(&format!("{}/", k))
        }) {
            continue;
        }
        out.push(f);
    }
    out.truncate(6);
    out
}

/// Couches 1 puis 2 (rapides, locales, sans reseau). Vide = rien trouve.
pub fn find_quick(p: &Program, now_ms: u64) -> Vec<Found> {
    let mut p = p.clone();
    let env = launchers::Env::real();
    launchers::enrich(&mut p, &env);
    let f = p.folders();
    let l1 = launchers::find(&p, &f, &env);
    if !l1.is_empty() {
        return dedupe(l1);
    }
    dedupe(deduce::find(&p, &f, now_ms, &deduce::RealRegistry))
}

/// Base maison puis couches 1 a 4 sans attendre : 1 et 2 calculees, 3 et 4 lues dans le cache local (resultats
/// d'apprentissage ou de recherche deja faits, re-verifies sur le disque).
pub fn find_local(p: &Program, data_dir: &Path, now_ms: u64) -> Vec<Found> {
    let h = home::find(p, &p.folders(), &home::Base::load(data_dir));
    if !h.is_empty() {
        return dedupe(h);
    }
    let q = find_quick(p, now_ms);
    if !q.is_empty() {
        return q;
    }
    dedupe(store::Cache::load(data_dir).known(&p.key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correspondance_prudente() {
        assert_eq!(name_match("StardewValley", "Stardew Valley"), Some(100));
        assert_eq!(name_match("Hollow Knight", "hollow_knight"), Some(100));
        assert_eq!(name_match("HogwartsLegacy", "Hogwarts Legacy: L'Héritage de Poudlard"), None, "trop different");
        assert_eq!(name_match("Hogwarts Legacy", "HogwartsLegacy"), Some(100));
        assert_eq!(name_match("Factorio", "Factorio Server"), None, "ratio trop faible : autre programme");
        assert_eq!(name_match("Witcher3", "The Witcher 3"), Some(85));
        assert_eq!(name_match("Satisfactory", "Satisfactori"), Some(70));
        assert_eq!(name_match("Cache", "Cachet"), None);
        assert_eq!(name_match("Microsoft", "Minecraft"), None);
        assert_eq!(name_match("ab", "ab"), None, "trop court");
        assert_eq!(name_match("Elden Ring", "ELDEN RING™"), Some(100));
        // Autres produits du meme nom : suite, demo, serveur.
        assert_eq!(name_match("Graveyard Keeper 2 Demo", "Graveyard Keeper"), None);
        assert_eq!(name_match("Graveyard Keeper 2", "Graveyard Keeper"), None);
        assert_eq!(name_match("ValheimServer", "Valheim"), None);
        assert_eq!(name_match("Ring Elden", "Elden Ring"), Some(80), "memes mots");
        assert_eq!(name_match("Graveyard Keeper", "Graveyard Keeper 2"), None, "la suite ne prend pas le premier");
    }

    #[test]
    fn noms_et_editeurs() {
        let p = Program { exe: "Palworld-Win64-Shipping.exe".into(), name: "Palworld-Win64-Shipping".into(), product: Some("Unreal Engine".into()), company: Some("Pocketpair, Inc.".into()), install_dir: Some(PathBuf::from("/x/Palworld")), ..Default::default() };
        assert_eq!(p.names(), vec!["Palworld".to_string()]);
        assert_eq!(p.companies(), vec!["Pocketpair".to_string(), "Pocketpair, Inc.".to_string()]);
        assert_eq!(clean_company("Team Cherry Pty Ltd"), "Team Cherry");
        assert_eq!(clean_company("CD PROJEKT S.A."), "CD PROJEKT");
        let u = Program { exe: "game.exe".into(), name: "game".into(), company: Some("DefaultCompany".into()), product: Some("UnityPlayer".into()), ..Default::default() };
        assert!(u.names().is_empty());
        assert!(u.companies().is_empty());
    }

    #[test]
    fn nature_de_l_emplacement() {
        assert_eq!(kind_for("%APPDATA%/StardewValley/Saves", false), Kind::Save);
        assert_eq!(kind_for("%LOCALAPPDATA%/X/Saved/Config/Windows", false), Kind::Config);
        assert_eq!(kind_for("%APPDATA%/Code/User", true), Kind::Config);
        assert_eq!(kind_for("%LOCALLOW%/Team Cherry/Hollow Knight", false), Kind::Save);
    }

    #[test]
    fn dedoublonnage() {
        let f = |p: &str, c: u8| Found { path: PathBuf::from(p), template: None, kind: Kind::Save, layer: Layer::Deduce, confidence: c, source: String::new() };
        let v = dedupe(vec![f("/a/x", 60), f("/a/x/saves", 80), f("/b", 70), f("/a/x", 90)]);
        assert_eq!(v.iter().map(|x| (x.path.to_string_lossy().to_string(), x.confidence)).collect::<Vec<_>>(), vec![("/a/x".to_string(), 90), ("/b".to_string(), 70)]);
    }

    #[test]
    fn dossier_d_installation() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("steamapps").join("common").join("Jeu").join("bin").join("x64").join("jeu.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert_eq!(install_dir_of(&exe), Some(t.path().join("steamapps").join("common").join("Jeu")));
        assert_eq!(steam_install(&exe), Some((t.path().join("steamapps"), "Jeu".to_string())));
    }
}
