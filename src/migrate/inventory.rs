//! Inventaire des logiciels d'un appareil, dans le coffre (nouveau PC : liste a reinstaller, pages des boutiques, winget).
//!
//! `<coffre>/.agss-inventory/<appareil>/<AAAAMMJJ>.json` : ecrit UNE fois par appareil et par jour (jamais reecrit ni
//! renomme : cloud-safe), seulement par l'appareil concerne ; ses 3 fichiers les plus recents sont gardes, les plus anciens
//! retires (jamais ceux d'un autre appareil). Contenu : noms des logiciels installes, identifiants winget quand ils sont
//! connus, programmes des lanceurs (boutique + identifiant). AUCUN chemin, aucun nom d'utilisateur. Le dossier commence par
//! un point : il est ignore par la liste des elements du coffre.
use crate::{util, Res};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DIR: &str = ".agss-inventory";
pub const KEEP: usize = 3;
/// Plafonds (un inventaire reste petit).
pub const MAX_APPS: usize = 1500;
pub const MAX_GAMES: usize = 1500;

/// Programme d'un lanceur.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GameRef {
    pub name: String,
    /// `steam`, `epic`, `gog`, `xbox`, `ubisoft`, `ea`, `itch`, `lutris`.
    pub store: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Inventory {
    pub device: String,
    /// AAAAMMJJ.
    pub day: String,
    /// `windows` ou `linux`.
    #[serde(default)]
    pub os: String,
    /// Logiciels installes (noms).
    #[serde(default)]
    pub apps: Vec<String>,
    /// Identifiants winget (Windows).
    #[serde(default)]
    pub winget: Vec<String>,
    #[serde(default)]
    pub games: Vec<GameRef>,
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(80).collect()
}

/// Dossier des inventaires d'un appareil.
pub fn device_dir(vault: &Path, device: &str) -> PathBuf {
    vault.join(DIR).join(clean(device))
}

/// Nom de fichier d'un jour (`20261026.json`).
pub fn file_name(day: &str) -> String {
    format!("{}.json", day)
}

fn is_day_file(n: &str) -> bool {
    n.len() == 13 && n.ends_with(".json") && n[..8].bytes().all(|b| b.is_ascii_digit())
}

/// Jour (AAAAMMJJ, UTC) d'un instant.
pub fn day_of(ms: u64) -> String {
    util::timestamp(ms).chars().take(8).collect()
}

/// L'inventaire du jour reste-t-il a ecrire ?
pub fn needs_today(vault: &Path, device: &str, day: &str) -> bool {
    vault.is_dir() && !device_dir(vault, device).join(file_name(day)).exists()
}

/// Nom utilisable : ni chemin, ni nom d'utilisateur, ni texte de controle ; longueur raisonnable.
pub fn clean_name(s: &str, user_names: &[String]) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.len() > 120 || t.contains(['\\', '/', ':', '%', '$']) || t.chars().any(|c| c.is_control()) {
        return None;
    }
    let low = t.to_lowercase();
    if user_names.iter().any(|u| u.len() >= 3 && low.contains(&u.to_lowercase())) {
        return None;
    }
    Some(t.to_string())
}

/// Identifiant winget plausible (`Editeur.Produit`, lettres, chiffres, `.`, `-`, `_`, `+`).
pub fn valid_winget_id(s: &str) -> bool {
    let s = s.trim();
    (3..=128).contains(&s.len()) && s.contains('.') && !s.starts_with('.') && !s.ends_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Identifiants du fichier produit par `winget export` (`Sources[].Packages[].PackageIdentifier`).
pub fn parse_winget_export(json: &str) -> Vec<String> {
    let v: serde_json::Value = match serde_json::from_str(json.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut out: Vec<String> = Vec::new();
    for src in v.get("Sources").and_then(|s| s.as_array()).into_iter().flatten() {
        for p in src.get("Packages").and_then(|s| s.as_array()).into_iter().flatten() {
            if let Some(id) = p.get("PackageIdentifier").and_then(|x| x.as_str()) {
                if valid_winget_id(id) && !out.iter().any(|x| x.eq_ignore_ascii_case(id)) {
                    out.push(id.to_string());
                }
            }
        }
    }
    out.sort_by_key(|a| a.to_lowercase());
    out
}

/// Inventaire nettoye (noms sans chemin ni nom d'utilisateur, doublons retires, tries, plafonnes).
pub fn build(device: &str, day: &str, os: &str, apps: Vec<String>, winget: Vec<String>, games: Vec<GameRef>, user_names: &[String]) -> Inventory {
    let mut a: Vec<String> = apps.iter().filter_map(|n| clean_name(n, user_names)).collect();
    a.sort_by_key(|x| x.to_lowercase());
    a.dedup_by(|x, y| x.eq_ignore_ascii_case(y));
    a.truncate(MAX_APPS);
    let mut w: Vec<String> = winget.into_iter().filter(|x| valid_winget_id(x)).collect();
    w.sort_by_key(|x| x.to_lowercase());
    w.dedup_by(|x, y| x.eq_ignore_ascii_case(y));
    let mut g: Vec<GameRef> = games
        .into_iter()
        .filter_map(|g| {
            let name = clean_name(&g.name, user_names)?;
            let id: String = g.id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).take(80).collect();
            Some(GameRef { name, store: g.store.to_lowercase(), id })
        })
        .collect();
    g.sort_by(|x, y| x.name.to_lowercase().cmp(&y.name.to_lowercase()));
    g.dedup_by(|x, y| x.name.eq_ignore_ascii_case(&y.name) && x.store == y.store);
    g.truncate(MAX_GAMES);
    Inventory { device: device.to_string(), day: day.to_string(), os: os.to_string(), apps: a, winget: w, games: g }
}

/// Ecrit l'inventaire du jour (jamais d'ecrasement : Ok(false) s'il existe deja), puis ne garde que les 3 plus recents de
/// CET appareil.
pub fn write(vault: &Path, inv: &Inventory) -> Res<bool> {
    if !vault.is_dir() {
        return Err("coffre injoignable".into());
    }
    let dir = device_dir(vault, &inv.device);
    let file = dir.join(file_name(&inv.day));
    if file.exists() {
        return Ok(false);
    }
    crate::store::write_new(&file, serde_json::to_string_pretty(inv)?.as_bytes())?;
    prune(&dir);
    Ok(true)
}

/// Retire les inventaires au-dela des 3 plus recents (fichiers `AAAAMMJJ.json` du dossier de cet appareil seulement).
pub fn prune(dir: &Path) -> Vec<PathBuf> {
    let mut days: Vec<String> = std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| is_day_file(n)).collect()).unwrap_or_default();
    days.sort();
    days.reverse();
    let mut removed = Vec::new();
    for n in days.into_iter().skip(KEEP) {
        let p = dir.join(&n);
        if std::fs::remove_file(&p).is_ok() {
            removed.push(p);
        }
    }
    removed
}

/// Inventaire le plus recent d'un appareil (lisible), s'il existe.
pub fn latest(vault: &Path, device: &str) -> Option<Inventory> {
    let dir = device_dir(vault, device);
    let mut days: Vec<String> = std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| is_day_file(n)).collect();
    days.sort();
    days.reverse();
    days.into_iter().find_map(|n| std::fs::read_to_string(dir.join(n)).ok().and_then(|t| serde_json::from_str(&t).ok()))
}

/// Commandes a lancer pour tout reinstaller avec winget (une ligne par logiciel ; rien n'est lance ici).
pub fn winget_lines(ids: &[String]) -> Vec<String> {
    ids.iter().filter(|i| valid_winget_id(i)).map(|i| format!("winget install --exact --id {}", i)).collect()
}

// ------------------------------------------------------------------ collecte (vrai PC)

/// `winget export` (Windows) : identifiants des logiciels reconnus. Fenetre cachee, 90 s au plus, aucune question posee
/// (`--disable-interactivity`) ; tout echec = liste vide.
#[cfg(windows)]
pub fn winget_ids(tmp_dir: &Path) -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let _ = std::fs::create_dir_all(tmp_dir);
    let out = tmp_dir.join(format!("winget-export-{}.json", util::now_ms()));
    let child = std::process::Command::new("winget")
        .args(["export", "--output"])
        .arg(&out)
        .arg("--disable-interactivity")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn();
    let Ok(mut child) = child else { return vec![] };
    let t0 = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t0.elapsed() < std::time::Duration::from_secs(90) => std::thread::sleep(std::time::Duration::from_millis(300)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let ids = std::fs::read_to_string(&out).map(|t| parse_winget_export(&t)).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    ids
}

#[cfg(not(windows))]
pub fn winget_ids(_tmp_dir: &Path) -> Vec<String> {
    vec![]
}

fn store_code(s: crate::detect::Store) -> &'static str {
    use crate::detect::Store;
    match s {
        Store::Steam => "steam",
        Store::Epic => "epic",
        Store::Gog => "gog",
        Store::Xbox => "xbox",
        Store::Ubisoft => "ubisoft",
        Store::Ea => "ea",
        Store::Itch => "itch",
        Store::Lutris => "lutris",
    }
}

/// Inventaire de CE PC (lecture seule ; a appeler dans un thread : lanceurs, logiciels installes, winget).
pub fn collect(device: &str, data_dir: &Path) -> Inventory {
    use crate::scan::sources;
    let apps: Vec<String> = sources::installed_apps().into_iter().filter(|a| !a.system).map(|a| a.name).collect();
    let games: Vec<GameRef> = sources::launcher_games().into_iter().filter(|g| !crate::detect::is_non_game(g)).map(|g| GameRef { name: g.name, store: store_code(g.store).into(), id: g.id }).collect();
    let winget = winget_ids(&data_dir.join("tmp"));
    let users = crate::knowledge::paths::Folders::real().user_names();
    build(device, &day_of(util::now_ms()), crate::store::this_os(), apps, winget, games, &users)
}
