//! 2.0.3 : jeux installes dans Playnite pas encore proteges. L'extension Playnite (1.2.0) ecrit `playnite-library.json` dans
//! le dossier de donnees (a cote de `status.json`), d'un coup (fichier temporaire puis remplacement) et avec un numero de
//! format : pour chaque jeu INSTALLE, identifiant Playnite, nom, source, dossier d'installation, programmes connus,
//! identifiants de plateforme. L'appli ne fait que LIRE ce fichier.
//!
//! Pas de lecture directe de la bibliotheque de Playnite (`%AppData%\Playnite\library\*.db`) : c'est une base LiteDB
//! (format binaire interne de .NET, pages et index BSON propres a LiteDB) sans bibliotheque Rust maintenue ; la lire
//! demanderait de reecrire un lecteur de ce format interne, fragile a chaque version de Playnite. Seule la voie de
//! l'extension est gardee (si le fichier est absent : aucune proposition Playnite).
use crate::config::Game;
use crate::detect::{Installed, Store};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Fichier ecrit par l'extension dans le dossier de donnees.
pub const FILE: &str = "playnite-library.json";
/// Format compris (un format plus recent n'ajoute que des champs : il reste lu).
pub const MIN_SCHEMA: u32 = 1;

/// Un jeu installe dans Playnite.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LibGame {
    /// Identifiant Playnite (GUID).
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// Source / bibliotheque (« Steam », « GOG », « Epic », vide pour un jeu ajoute a la main).
    #[serde(default)]
    pub source: String,
    /// Extension de bibliotheque de Playnite (GUID), vide pour un jeu ajoute a la main.
    #[serde(default)]
    pub plugin_id: String,
    #[serde(default)]
    pub install_dir: String,
    /// Programmes des actions de jeu (noms de fichiers ou chemins).
    #[serde(default)]
    pub exes: Vec<String>,
    /// Identifiant du jeu dans sa bibliotheque (appid Steam, identifiant GOG, nom d'application Epic...).
    #[serde(default)]
    pub game_id: String,
    #[serde(default)]
    pub steam_appid: Option<u64>,
    /// Plateformes Playnite (« PC (Windows) », « Nintendo Switch »...).
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Jeu masque par l'utilisateur dans Playnite (extension 1.2.0 corrigee) : jamais propose.
    #[serde(default)]
    pub hidden: bool,
    /// ROM d'un jeu emule et son emulateur (extension 1.2.0 corrigee) : toleres, pas exploites en 2.0.3 (jeux emules ignores).
    #[serde(default)]
    pub roms: Vec<String>,
    #[serde(default)]
    pub emulator: Option<LibEmulator>,
}

/// Emulateur d'un jeu emule (champs optionnels du contrat, lus pour memoire).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LibEmulator {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub install_dir: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub exe: String,
}

#[derive(Deserialize)]
struct LibFile {
    schema: u32,
    /// Date de generation (ISO 8601 UTC), optionnelle : une liste de plus de 30 jours est perimee (ignoree).
    #[serde(default)]
    generated_utc: String,
    #[serde(default)]
    games: Vec<LibGame>,
}

/// Au-dela de cet age (d'apres `generated_utc`), la liste est perimee : Playnite ne l'a plus mise a jour.
pub const MAX_AGE_MS: u64 = 30 * 86_400_000;

/// Date ISO 8601 UTC (`2026-10-29T12:00:00Z`, fraction de seconde toleree) -> ms UTC. None si illisible.
pub fn parse_utc_ms(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|x| x.split('.').next().unwrap_or("").parse::<i64>().ok());
    let (h, mi, sec) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..61).contains(&sec) {
        return None;
    }
    // Jours depuis 1970-01-01 (algorithme civil de H. Hinnant).
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60 + sec;
    u64::try_from(secs).ok().map(|s| s * 1000)
}

/// Dossier d'installation utilisable : ni vide, ni variable Playnite non resolue (« {InstallDir}... »), ni chemin tronque
/// (« \dossier » sans lecteur ; un partage reseau « \\NAS\... » reste valable).
pub fn install_dir_ok(dir: &str) -> bool {
    let d = dir.trim();
    !d.is_empty() && !d.starts_with('{') && !(d.starts_with('\\') && !d.starts_with("\\\\"))
}

/// Lecture tolerante : None si vide, invalide, format inconnu (trop ancien) ou liste perimee (`generated_utc` de plus de
/// 30 jours par rapport a `now_ms`). Les champs inconnus sont ignores ; les jeux masques dans Playnite sont retires.
pub fn parse_at(text: &str, now_ms: u64) -> Option<Vec<LibGame>> {
    let f: LibFile = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    if f.schema < MIN_SCHEMA {
        return None;
    }
    if let Some(at) = parse_utc_ms(&f.generated_utc).filter(|_| !f.generated_utc.trim().is_empty()) {
        if now_ms.saturating_sub(at) > MAX_AGE_MS {
            return None;
        }
    }
    Some(f.games.into_iter().filter(|g| !g.name.trim().is_empty() && !g.hidden).collect())
}

pub fn parse(text: &str) -> Option<Vec<LibGame>> {
    parse_at(text, crate::util::now_ms())
}

/// Lit le fichier `path` (vide si absent ou invalide).
pub fn read_from(path: &Path) -> Vec<LibGame> {
    read_from_opt(path).unwrap_or_default()
}

/// Meme lecture, mais distingue « fichier absent ou illisible » (None : l'extension est peut-etre en train de le remplacer,
/// l'appelant garde alors son dernier compte) d'une liste lue (Some, eventuellement vide).
pub fn read_from_opt(path: &Path) -> Option<Vec<LibGame>> {
    std::fs::read_to_string(path).ok().and_then(|t| parse(&t))
}

/// Jeu PC (ou assimile) : au moins un programme, ou aucune plateforme renseignee, ou une plateforme PC. Un jeu emule (ROM
/// lancee par un emulateur : pas de programme, plateforme console) n'est jamais propose : son dossier d'installation est
/// le dossier de ROMs partage et une recherche par nom dans la base PC donnerait un faux emplacement.
pub fn is_pc_game(g: &LibGame) -> bool {
    if g.exes.iter().any(|e| !e.trim().is_empty()) || g.platforms.is_empty() {
        return true;
    }
    g.platforms.iter().any(|p| {
        let p = p.to_lowercase();
        p.contains("pc") || p.contains("windows") || p.contains("linux") || p.contains("mac") || p.contains("steamos")
    })
}

/// Jeux Playnite du dossier de donnees. Playnite n'existe que sous Windows : toujours vide ailleurs.
pub fn read(data_dir: &Path) -> Vec<LibGame> {
    if !cfg!(windows) {
        return vec![];
    }
    read_from(&data_dir.join(FILE))
}

/// Nom de fichier d'un programme (« Game.exe » pour « C:\Jeux\Game.exe »), en minuscules.
fn exe_file(e: &str) -> String {
    e.rsplit(['/', '\\']).next().unwrap_or(e).trim().trim_matches('"').to_lowercase()
}

/// Boutique correspondant a la source Playnite (le scan s'en sert pour la base : appid Steam, identifiant GOG).
pub fn store_of(g: &LibGame) -> Store {
    let s = g.source.to_lowercase();
    if s.contains("steam") || g.steam_appid.is_some() {
        Store::Steam
    } else if s.contains("gog") {
        Store::Gog
    } else if s.contains("epic") {
        Store::Epic
    } else if s.contains("xbox") || s.contains("microsoft") {
        Store::Xbox
    } else if s.contains("ubisoft") || s.contains("uplay") {
        Store::Ubisoft
    } else if s.contains("ea app") || s.contains("origin") || s == "ea" {
        Store::Ea
    } else {
        Store::Itch
    }
}

/// Jeu Playnite vu comme un jeu installe (meme traitement que ceux des lanceurs : profils, base, connaissance locale).
pub fn to_installed(g: &LibGame) -> Installed {
    let store = store_of(g);
    let id = match store {
        Store::Steam => g.steam_appid.map(|a| a.to_string()).unwrap_or_else(|| g.game_id.clone()),
        Store::Gog | Store::Epic => g.game_id.clone(),
        _ => String::new(),
    };
    let exe = g.exes.iter().map(|e| e.rsplit(['/', '\\']).next().unwrap_or(e).trim().trim_matches('"').to_string()).find(|e| e.to_lowercase().ends_with(".exe") || cfg!(not(windows)) && !e.is_empty());
    Installed { name: g.name.trim().to_string(), store, id, install_dir: std::path::PathBuf::from(g.install_dir.trim()), exe }
}

/// Le jeu Playnite est-il deja protege (meme nom, ou un de ses programmes est celui d'un element) ?
pub fn is_protected(g: &LibGame, games: &[Game]) -> bool {
    let key = crate::cloud::game_key(&g.name);
    let exes: Vec<String> = g.exes.iter().map(|e| exe_file(e)).filter(|e| !e.is_empty()).collect();
    games.iter().any(|x| {
        (!key.is_empty() && (crate::cloud::game_key(&x.name) == key || crate::cloud::game_key(x.shown()) == key)) || x.exe.as_deref().map(|e| exes.contains(&exe_file(e))).unwrap_or(false)
    })
}

/// Jeux installes dans Playnite pas encore proteges et proposables (jeux PC seulement : quelque chose a proteger ou a
/// apprendre ; sans doublon de nom), dans l'ordre alphabetique. C'est aussi le compte de la carte de l'accueil.
pub fn not_protected<'a>(lib: &'a [LibGame], games: &[Game]) -> Vec<&'a LibGame> {
    let mut seen = std::collections::HashSet::new();
    let mut v: Vec<&LibGame> = lib.iter().filter(|g| install_dir_ok(&g.install_dir) && !g.hidden && is_pc_game(g) && !is_protected(g, games) && !crate::detect::is_non_game(&to_installed(g))).filter(|g| seen.insert(crate::cloud::game_key(&g.name))).collect();
    v.sort_by_key(|g| g.name.to_lowercase());
    v
}

/// Jeux Playnite proposables qu'AUCUN lanceur n'a deja vus (meme nom) : les lanceurs passent avant Playnite dans le scan
/// (section, etiquette et programme du lanceur conserves).
pub fn not_seen_by_launchers<'a>(lib: &'a [LibGame], games: &[Game], launcher_names: &[String]) -> Vec<&'a LibGame> {
    let seen: std::collections::HashSet<String> = launcher_names.iter().map(|n| crate::cloud::game_key(n)).collect();
    not_protected(lib, games).into_iter().filter(|g| !seen.contains(&crate::cloud::game_key(&g.name))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_tolerante() {
        assert!(parse("").is_none());
        assert!(parse("{").is_none());
        assert!(parse(r#"{"schema":0,"games":[]}"#).is_none());
        let v = parse("\u{feff}{\"schema\":2,\"extra\":1,\"games\":[{\"name\":\"Celeste\",\"install_dir\":\"C:/G/Celeste\",\"new_field\":true},{\"name\":\"  \"}]}").unwrap();
        assert_eq!(v.len(), 1, "nom vide ignore, champs inconnus acceptes");
        assert_eq!(v[0].name, "Celeste");
    }

    #[test]
    fn date_iso_dossier_et_liste_perimee() {
        assert_eq!(parse_utc_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc_ms("2026-10-29T12:00:00Z"), Some(1_793_275_200_000));
        assert_eq!(parse_utc_ms("2000-03-01T00:00:00.5Z"), Some(951_868_800_000));
        assert!(parse_utc_ms("hier").is_none() && parse_utc_ms("2026-13-01T00:00:00Z").is_none());
        assert!(install_dir_ok("C:\\Jeux\\X") && install_dir_ok("\\\\NAS\\jeux\\X") && install_dir_ok("/home/a/x"));
        assert!(!install_dir_ok("") && !install_dir_ok("{InstallDir}\\x") && !install_dir_ok("\\Jeux\\X"));
        let t = |gen: &str| format!(r#"{{"schema":1,"generated_utc":"{}","games":[{{"name":"A","install_dir":"C:/a"}}]}}"#, gen);
        let now = parse_utc_ms("2026-10-29T12:00:00Z").unwrap();
        assert_eq!(parse_at(&t("2026-10-01T12:00:00Z"), now).map(|v| v.len()), Some(1), "28 jours : valide");
        assert!(parse_at(&t("2026-09-01T12:00:00Z"), now).is_none(), "plus de 30 jours : perimee");
        assert_eq!(parse_at(&t(""), now).map(|v| v.len()), Some(1), "sans date : valide");
        assert_eq!(parse_at(&t("n'importe quoi"), now).map(|v| v.len()), Some(1), "date illisible : toleree");
    }
}
