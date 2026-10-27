//! Donnees d'usage (pour l'auteur seul, avec un consentement EXPLICITE) et echanges avec le serveur de l'appli :
//! - identifiant d'installation aleatoire (UUID v4) dans `<donnees>/usage-id`, renouvele tous les 13 mois ;
//! - file locale `<donnees>/usage-queue.jsonl` (200 evenements au plus, abandonnes apres 7 jours sans envoi reussi) ;
//! - envoi groupe 1 fois par jour au plus, delai reseau 5 s, jamais pendant l'utilisation d'un programme ;
//! - liste blanche STRICTE (meme liste que le serveur, `server/src/spec.js`) : aucune valeur libre, sauf les titres
//!   CONNUS (base embarquee, profils integres, base maison) ;
//! - envoi de la file des emplacements acceptes (`knowledge::queue`) et telechargement de la base maison validee.
//!
//! Sans consentement : rien n'est mis en file ni envoye. Consentement refuse : la file est videe et le serveur n'est
//! jamais contacte (seule une demande « Effacer mes donnees » faite par l'utilisateur part).
use crate::config::Game;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

/// Adresse du serveur : constante de compilation (`AGSS_USAGE_URL` au moment du build), sinon le serveur de l'auteur.
pub const SERVER: &str = match option_env!("AGSS_USAGE_URL") {
    Some(u) => u,
    None => "https://agss-usage.agss-usage-server.workers.dev",
};
/// Texte public sur la confidentialite.
pub const PRIVACY_URL: &str = "https://github.com/Epikou/AutoGameSoftwareSync/blob/main/PRIVACY.md";

pub const QUEUE_FILE: &str = "usage-queue.jsonl";
pub const ID_FILE: &str = "usage-id";
pub const STATE_FILE: &str = "usage-state.json";
pub const MAX_QUEUE: usize = 200;
pub const MAX_AGE_DAYS: u64 = 7;
/// 13 mois.
pub const ID_RENEW_DAYS: u64 = 396;
pub const TIMEOUT_S: u64 = 5;
pub const MAX_BODY: usize = 16 * 1024;
pub const MAX_EVENTS: usize = 100;
pub const MAX_RECORDS: usize = 50;
pub const MAX_KNOWN: usize = 100;
const DAY_MS: u64 = 86_400_000;
/// Nouvel essai apres un echec d'envoi (sans depasser un envoi reussi par jour).
const RETRY_MS: u64 = 6 * 3_600_000;

// ------------------------------------------------------------------ consentement (etat global du processus)

static CONSENT: AtomicU8 = AtomicU8::new(0);

/// Consentement courant (renseigne par l'interface a partir de `settings.json`).
pub fn set_consent(c: Option<bool>) {
    CONSENT.store(
        match c {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        },
        Ordering::SeqCst,
    );
}

pub fn consent() -> Option<bool> {
    match CONSENT.load(Ordering::SeqCst) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// « majeure.mineure » : la question est reposee a chaque mise a jour de ce niveau.
pub fn ask_key(version: &str) -> String {
    version.split(['.', '-']).take(2).collect::<Vec<_>>().join(".")
}

/// Faut-il poser la question (premier lancement, ou mise a jour depuis la derniere question) ?
pub fn needs_question(asked: Option<&str>, version: &str) -> bool {
    asked != Some(ask_key(version).as_str())
}

// ------------------------------------------------------------------ rappel unique de la question (2.0.1)
//
// Le consentement est EXPLICITE (RGPD, arret Planet49 : jamais de pre-cochage) : tant qu'on n'a pas clique « Oui, j'aide »,
// rien n'est envoye. La question se pose en bas de la derniere etape de l'assistant ; si l'utilisateur termine sans choisir,
// elle revient UNE seule fois, plus tard, en carte discrete en bas de l'accueil, puis plus jamais.

/// Rien de programme.
pub const LATE_NONE: u8 = 0;
/// Rappel a montrer plus tard (prochain lancement, ou apres 24 h).
pub const LATE_PENDING: u8 = 1;
/// Rappel montre : plus jamais.
pub const LATE_SHOWN: u8 = 2;

/// Delai apres lequel le rappel peut revenir dans la meme session (24 h).
pub const LATE_AFTER_MS: u64 = DAY_MS;

/// Fin de l'assistant (ou de « Quoi de neuf ») sans reponse : programme le rappel (une seule fois). Pur.
pub fn late_after_wizard(needs_question: bool, late: u8) -> u8 {
    if needs_question && late == LATE_NONE {
        LATE_PENDING
    } else {
        late
    }
}

/// Au lancement : une question restee sans reponse alors que l'assistant n'est plus a faire est programmee (une seule fois).
/// Un rappel deja montre ne revient jamais. Pur.
pub fn late_at_start(needs_question: bool, late: u8, wizard_pending: bool) -> u8 {
    if needs_question && late == LATE_NONE && !wizard_pending {
        LATE_PENDING
    } else {
        late
    }
}

/// La carte discrete doit-elle s'afficher maintenant ? Jamais pendant l'assistant ; pas dans la session qui l'a programmee
/// (sauf apres 24 h). Pur.
pub fn late_card_due(needs_question: bool, late: u8, wizard_pending: bool, set_this_session: bool, set_at_ms: u64, now_ms: u64) -> bool {
    needs_question && late == LATE_PENDING && !wizard_pending && (!set_this_session || now_ms.saturating_sub(set_at_ms) >= LATE_AFTER_MS)
}

// ------------------------------------------------------------------ liste blanche

#[derive(Clone, Copy, Debug)]
pub enum Rule {
    E(&'static [&'static str]),
    Version,
    Known,
}

const N: &[&str] = &["0", "1-5", "6-20", "21+"];
const DUR: &[&str] = &["<10s", "10-30s", "30-60s", "1-3m", "3-10m", "10m+"];
const AGE: &[&str] = &["<1h", "1h-1d", "1-7d", "7-30d", "30d+"];
const YN: &[&str] = &["yes", "no"];

/// Evenements autorises et leurs proprietes (ETUDE §11.4, plus `known_items`).
pub const EVENTS: &[(&str, &[(&str, Rule)])] = &[
    ("app_start", &[("version", Rule::Version), ("os", Rule::E(&["windows", "linux"])), ("os_build", Rule::E(&["win10", "win11", "steamos", "linux", "other"])), ("arch", Rule::E(&["x86_64", "aarch64", "x86", "other"])), ("lang", Rule::E(&["en", "fr", "ru", "es", "zh"])), ("mode", Rule::E(&["simple", "expert"])), ("source", Rule::E(&["manual", "startup", "tray"])), ("big", Rule::E(YN))]),
    ("daily_state", &[("items", Rule::E(N)), ("devices", Rule::E(&["1", "2", "3+"])), ("vault", Rule::E(&["gdrive", "onedrive", "dropbox", "network", "usb", "other", "none"])), ("detection", Rule::E(&["on", "off"])), ("update_mode", Rule::E(&["auto", "notify", "off"])), ("theme", Rule::E(&["system", "dark", "light"]))]),
    ("known_items", &[("items", Rule::Known)]),
    ("onboarding_step", &[("step", Rule::E(&["1", "2", "3", "4", "5", "6"])), ("action", Rule::E(&["done", "later", "back"])), ("duration", Rule::E(DUR))]),
    ("onboarding_done", &[("duration", Rule::E(DUR)), ("items", Rule::E(N)), ("migration", Rule::E(YN))]),
    ("item_added", &[("source", Rule::E(&["scan", "detection", "manual", "drop", "profile"])), ("category", Rule::E(&["game", "app", "folder", "windows"])), ("multi", Rule::E(YN))]),
    ("detection_answer", &[("answer", Rule::E(&["yes", "no", "later"])), ("reason", Rule::E(&["launcher", "base", "profile", "emulator", "other"])), ("score", Rule::E(&["<70", "70-79", "80-89", "90-100"]))]),
    ("scan_done", &[("duration", Rule::E(DUR)), ("proposed", Rule::E(N)), ("accepted", Rule::E(N))]),
    ("time_travel", &[("action", Rule::E(&["opened", "version_chosen", "restored", "cancelled"])), ("age", Rule::E(AGE))]),
    ("conflict", &[("action", Rule::E(&["shown", "resolved"])), ("choice", Rule::E(&["this_pc", "other_device", "none"]))]),
    ("migration", &[("step", Rule::E(&["start", "end"])), ("items", Rule::E(N)), ("remaining", Rule::E(N)), ("duration", Rule::E(DUR))]),
    ("sync_error", &[("category", Rule::E(&["vault_unreachable", "file_locked", "disk_full", "permissions", "other"]))]),
    ("update", &[("from", Rule::Version), ("to", Rule::Version), ("result", Rule::E(&["ok", "rollback", "signature_ko", "hash_ko", "download_failed"]))]),
    ("setting_changed", &[("key", Rule::E(&["reactivity", "detection", "update_mode", "theme", "expert_mode", "startup"])), ("value", Rule::E(&["on", "off", "instant", "normal", "slow", "custom", "system", "dark", "light", "auto", "notify"]))]),
    ("support_click", &[("place", Rule::E(&["popup", "bar", "settings", "tutorial"]))]),
];

/// Un seul par jour (le plus recent remplace l'ancien, sauf `app_start` : le premier compte).
const ONCE_A_DAY: &[&str] = &["app_start", "daily_state", "known_items"];

pub fn is_version(s: &str) -> bool {
    let (main, pre) = match s.split_once('-') {
        Some((m, p)) => (m, Some(p)),
        None => (s, None),
    };
    let parts: Vec<&str> = main.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
        && pre.map(|p| !p.is_empty() && p.len() <= 16 && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.')).unwrap_or(true)
}

pub fn is_uuid_v4(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && s.chars().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() && !c.is_ascii_uppercase() })
        && b[14] == b'4'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
}

/// Texte court affichable (titre connu) : ni controle, ni separateur de chemin, ni adresse, ni lecteur.
pub fn clean_text(s: &str, max: usize) -> bool {
    let t = s.trim();
    !t.is_empty() && t.len() == s.len() && s.chars().count() <= max && !s.chars().any(|c| c.is_control() || c == '\\' || c == '/' || c == '@') && !(s.len() >= 2 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic())
}

fn known_ok(v: &Value) -> Result<(), String> {
    let arr = v.as_array().ok_or("known_items: liste attendue")?;
    if arr.len() > MAX_KNOWN {
        return Err("known_items: trop de titres".into());
    }
    for it in arr {
        let o = it.as_object().ok_or("known_items: element invalide")?;
        if o.keys().any(|k| !["title", "type", "multi"].contains(&k.as_str())) {
            return Err("known_items: champ inconnu".into());
        }
        if !o.get("title").and_then(Value::as_str).map(|t| clean_text(t, 80)).unwrap_or(false) {
            return Err("known_items: titre invalide".into());
        }
        if !matches!(o.get("type").and_then(Value::as_str), Some("game" | "app")) {
            return Err("known_items: type invalide".into());
        }
        if let Some(m) = o.get("multi") {
            if !matches!(m.as_str(), Some("yes" | "no")) {
                return Err("known_items: multi invalide".into());
            }
        }
    }
    Ok(())
}

/// Jour UTC AAAA-MM-JJ.
pub fn utc_day(ms: u64) -> String {
    let (y, m, d, ..) = crate::util::civil_from_ms(ms as i64);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn day_index(day: &str) -> Option<i64> {
    let p: Vec<&str> = day.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return None;
    }
    let (y, m, d) = (p[0].parse::<i64>().ok()?, p[1].parse::<u32>().ok()?, p[2].parse::<u32>().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(crate::util::ms_from_civil(y, m, d, 0, 0, 0, 0) / DAY_MS as i64)
}

/// Un evenement de la file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub day: String,
    #[serde(default)]
    pub props: Map<String, Value>,
    /// Date de mise en file (ms) : sert seulement a l'abandon apres 7 jours, jamais envoyee.
    #[serde(default)]
    pub at: u64,
}

impl Event {
    pub fn new(name: &str, props: &[(&str, &str)], now_ms: u64) -> Event {
        Event { name: name.to_string(), day: utc_day(now_ms), props: props.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect(), at: now_ms }
    }

    /// Forme envoyee au serveur (sans la date de mise en file).
    pub fn wire(&self) -> Value {
        json!({ "name": self.name, "day": self.day, "props": self.props })
    }
}

/// Verification de la liste blanche (identique au serveur).
pub fn validate(e: &Event, now_ms: u64) -> Result<(), String> {
    let spec = EVENTS.iter().find(|(n, _)| *n == e.name).map(|(_, s)| *s).ok_or_else(|| format!("evenement inconnu : {}", e.name))?;
    let d = day_index(&e.day).ok_or("jour invalide")?;
    let off = d - (now_ms / DAY_MS) as i64;
    if !(-8..=1).contains(&off) {
        return Err("jour hors fenetre".into());
    }
    for (k, v) in &e.props {
        let rule = spec.iter().find(|(n, _)| n == k).map(|(_, r)| *r).ok_or_else(|| format!("propriete inconnue : {}.{}", e.name, k))?;
        match rule {
            Rule::E(vals) => {
                if !v.as_str().map(|s| vals.contains(&s)).unwrap_or(false) {
                    return Err(format!("valeur refusee : {}.{}", e.name, k));
                }
            }
            Rule::Version => {
                if !v.as_str().map(is_version).unwrap_or(false) {
                    return Err(format!("version invalide : {}.{}", e.name, k));
                }
            }
            Rule::Known => known_ok(v)?,
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ tranches

pub fn bucket_count(n: usize) -> &'static str {
    match n {
        0 => "0",
        1..=5 => "1-5",
        6..=20 => "6-20",
        _ => "21+",
    }
}

pub fn bucket_devices(n: usize) -> &'static str {
    match n {
        0 | 1 => "1",
        2 => "2",
        _ => "3+",
    }
}

pub fn bucket_duration(secs: u64) -> &'static str {
    match secs {
        0..=9 => "<10s",
        10..=29 => "10-30s",
        30..=59 => "30-60s",
        60..=179 => "1-3m",
        180..=599 => "3-10m",
        _ => "10m+",
    }
}

pub fn bucket_age(ms: u64) -> &'static str {
    match ms / 60_000 {
        0..=59 => "<1h",
        60..=1439 => "1h-1d",
        1440..=10079 => "1-7d",
        10080..=43199 => "7-30d",
        _ => "30d+",
    }
}

pub fn bucket_score(s: u8) -> &'static str {
    match s {
        0..=69 => "<70",
        70..=79 => "70-79",
        80..=89 => "80-89",
        _ => "90-100",
    }
}

/// Categorie d'une erreur de synchro (d'apres son texte, qui n'est jamais envoye).
pub fn error_category(detail: &str) -> &'static str {
    let l = detail.to_lowercase();
    if ["os error 32", "os error 33", "verrouill", "locked", "being used by another process", "utilise par un autre"].iter().any(|w| l.contains(w)) {
        "file_locked"
    } else if ["os error 112", "os error 28", "espace", "disk full", "no space", "not enough space"].iter().any(|w| l.contains(w)) {
        "disk_full"
    } else if ["os error 5)", "os error 13", "access is denied", "acces refuse", "accès refusé", "permission denied"].iter().any(|w| l.contains(w)) {
        "permissions"
    } else if ["injoignable", "unreachable", "introuvable", "os error 53", "os error 67", "os error 2)", "os error 3)", "network", "reseau", "réseau"].iter().any(|w| l.contains(w)) {
        "vault_unreachable"
    } else {
        "other"
    }
}

/// Type de dossier partage d'apres son chemin (le chemin lui-meme n'est jamais envoye). `removable` : lecteur amovible.
pub fn vault_kind(path: Option<&Path>, removable: bool) -> &'static str {
    let Some(p) = path else { return "none" };
    let s = p.to_string_lossy().replace('\\', "/").to_lowercase();
    if s.contains("google drive") || s.contains("googledrive") || s.contains("mon drive") || s.contains("my drive") || s.contains("drivefs") || s.contains("google-drive") {
        "gdrive"
    } else if s.contains("onedrive") {
        "onedrive"
    } else if s.contains("dropbox") {
        "dropbox"
    } else if s.starts_with("//") || s.contains("/gvfs/") || s.contains("smb") || s.contains("nfs") || s.contains("/nas") {
        "network"
    } else if removable || s.starts_with("/media/") || s.starts_with("/run/media/") {
        "usb"
    } else {
        "other"
    }
}

#[cfg(windows)]
fn is_removable(p: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    if b.len() < 2 || b[1] != b':' {
        return false;
    }
    let root: Vec<u16> = std::ffi::OsStr::new(&format!("{}:\\", &s[..1])).encode_wide().chain(std::iter::once(0)).collect();
    // DRIVE_REMOVABLE = 2
    unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) == 2 }
}

#[cfg(not(windows))]
fn is_removable(_: &Path) -> bool {
    false
}

#[cfg(windows)]
pub fn os_build() -> &'static str {
    let b = crate::registry::read_string(crate::registry::Hive::LocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "CurrentBuildNumber").and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    match b {
        22000.. => "win11",
        10240..=21999 => "win10",
        _ => "other",
    }
}

#[cfg(not(windows))]
pub fn os_build() -> &'static str {
    let t = std::fs::read_to_string("/etc/os-release").unwrap_or_default().to_lowercase();
    if t.lines().any(|l| l.trim() == "id=steamos" || l.trim() == "id=\"steamos\"") {
        "steamos"
    } else {
        "linux"
    }
}

pub fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "x86" => "x86",
        _ => "other",
    }
}

// ------------------------------------------------------------------ identifiant d'installation

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct IdFile {
    id: String,
    created_ms: u64,
}

/// UUID v4 aleatoire (generateur du systeme ; jamais lie au nom de l'appareil).
pub fn new_uuid() -> String {
    let mut b = [0u8; 16];
    if getrandom::getrandom(&mut b).is_err() {
        // Repli tres improbable : empreinte de l'heure precise et du processus.
        let seed = format!("{:?}|{}|{:?}", std::time::SystemTime::now(), std::process::id(), std::thread::current().id());
        let h = crate::util::hash_bytes(seed.as_bytes());
        for (i, x) in b.iter_mut().enumerate() {
            *x = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0);
        }
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{:02x}", x)).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn read_id(data_dir: &Path) -> Option<IdFile> {
    std::fs::read_to_string(data_dir.join(ID_FILE)).ok().and_then(|t| serde_json::from_str::<IdFile>(&t).ok()).filter(|f| is_uuid_v4(&f.id))
}

fn write_id(data_dir: &Path, f: &IdFile) {
    let _ = std::fs::create_dir_all(data_dir);
    let _ = crate::util::write_atomic(&data_dir.join(ID_FILE), serde_json::to_string(f).unwrap_or_default().as_bytes());
}

/// Identifiant courant (cree au besoin ; renouvele apres 13 mois).
pub fn install_id(data_dir: &Path, now_ms: u64) -> String {
    match read_id(data_dir) {
        Some(f) if now_ms.saturating_sub(f.created_ms) < ID_RENEW_DAYS * DAY_MS && f.created_ms <= now_ms + DAY_MS => f.id,
        _ => reset_id(data_dir, now_ms),
    }
}

/// Identifiant existant (affichage), sans en creer.
pub fn current_id(data_dir: &Path) -> Option<String> {
    read_id(data_dir).map(|f| f.id)
}

/// Nouvel identifiant (apres « Effacer mes donnees » ou au bout de 13 mois).
pub fn reset_id(data_dir: &Path, now_ms: u64) -> String {
    let f = IdFile { id: new_uuid(), created_ms: now_ms };
    write_id(data_dir, &f);
    f.id
}

// ------------------------------------------------------------------ file locale

static FILE_LOCK: Mutex<()> = Mutex::new(());

pub fn queue_path(data_dir: &Path) -> PathBuf {
    data_dir.join(QUEUE_FILE)
}

pub fn read_queue(data_dir: &Path) -> Vec<Event> {
    std::fs::read_to_string(queue_path(data_dir)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn write_queue(data_dir: &Path, v: &[Event]) {
    let p = queue_path(data_dir);
    if v.is_empty() {
        let _ = std::fs::remove_file(&p);
        return;
    }
    let mut s = String::new();
    for e in v {
        if let Ok(l) = serde_json::to_string(e) {
            s.push_str(&l);
            s.push('\n');
        }
    }
    let _ = std::fs::create_dir_all(data_dir);
    let _ = crate::util::write_atomic(&p, s.as_bytes());
}

/// Nettoyage : evenements de plus de 7 jours abandonnes, 200 au plus (les plus anciens partent).
pub fn prune(mut v: Vec<Event>, now_ms: u64) -> Vec<Event> {
    v.retain(|e| now_ms.saturating_sub(e.at) <= MAX_AGE_DAYS * DAY_MS && validate(e, now_ms).is_ok());
    if v.len() > MAX_QUEUE {
        let cut = v.len() - MAX_QUEUE;
        v.drain(..cut);
    }
    v
}

/// Ajoute un evenement a la file : SEULEMENT avec le consentement, et s'il respecte la liste blanche.
pub fn push(data_dir: &Path, e: Event, consent: Option<bool>) -> bool {
    if consent != Some(true) || validate(&e, e.at).is_err() {
        return false;
    }
    let now = e.at;
    let _g = FILE_LOCK.lock();
    let mut v = prune(read_queue(data_dir), now);
    if ONCE_A_DAY.contains(&e.name.as_str()) {
        if let Some(i) = v.iter().position(|x| x.name == e.name && x.day == e.day) {
            if e.name == "app_start" {
                return false;
            }
            v.remove(i);
        }
    }
    v.push(e);
    write_queue(data_dir, &prune(v, now));
    true
}

/// Evenement de l'interface (consentement courant du processus).
pub fn record(data_dir: &Path, name: &str, props: &[(&str, &str)]) -> bool {
    push(data_dir, Event::new(name, props, crate::util::now_ms()), consent())
}

pub fn clear_queue(data_dir: &Path) {
    let _g = FILE_LOCK.lock();
    write_queue(data_dir, &[]);
}

// ------------------------------------------------------------------ lots

fn envelope(id: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("v".into(), json!(1));
    m.insert("install_id".into(), json!(id));
    m.insert("version".into(), json!(crate::update::current_version()));
    m
}

/// Lots d'evenements (16 Ko et 100 evenements au plus chacun) : (indices dans `events`, corps JSON). Un evenement trop
/// gros pour un lot a lui seul est ignore.
pub fn event_batches(id: &str, events: &[Event]) -> Vec<(Vec<usize>, Vec<u8>)> {
    let mut env = envelope(id);
    env.insert("os".into(), json!(os_name()));
    chunks(env, "events", events.iter().map(Event::wire).collect(), MAX_EVENTS)
}

fn chunks(env: Map<String, Value>, key: &str, items: Vec<Value>, max_n: usize) -> Vec<(Vec<usize>, Vec<u8>)> {
    let body = |idx: &[usize]| {
        let mut m = env.clone();
        m.insert(key.into(), Value::Array(idx.iter().map(|i| items[*i].clone()).collect()));
        serde_json::to_vec(&Value::Object(m)).unwrap_or_default()
    };
    let mut out = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for i in 0..items.len() {
        cur.push(i);
        if cur.len() > max_n || body(&cur).len() > MAX_BODY {
            cur.pop();
            if !cur.is_empty() {
                out.push((cur.clone(), body(&cur)));
            }
            cur = vec![i];
            if body(&cur).len() > MAX_BODY {
                cur.clear(); // trop gros seul : ignore
            }
        }
    }
    if !cur.is_empty() {
        out.push((cur.clone(), body(&cur)));
    }
    out
}

/// Emplacement accepte acceptable pour le serveur (meme regles).
pub fn record_ok(r: &crate::knowledge::queue::Record) -> bool {
    let t = |s: &Option<String>, max: usize| s.as_deref().map(|x| clean_text(x, max)).unwrap_or(true);
    (r.program.is_some() || r.exe.is_some()) && t(&r.program, 100) && t(&r.publisher, 100) && t(&r.exe, 80) && is_version(&r.app_version) && crate::knowledge::home::valid(&crate::knowledge::home::Entry { program: r.program.clone(), publisher: r.publisher.clone(), exe: r.exe.clone(), template: r.template.clone(), kind: r.kind, n: 0, validated: false }) && !r.template.contains(':') && !r.template.contains('@')
}

/// Empreinte d'un emplacement de la file (deja envoye ?).
pub fn record_key(r: &crate::knowledge::queue::Record) -> String {
    crate::util::hash_bytes(serde_json::to_string(r).unwrap_or_default().as_bytes()).chars().take(16).collect()
}

// ------------------------------------------------------------------ transport

/// Acces au serveur (simule dans les tests : AUCUN envoi reel pendant les tests).
pub trait Transport {
    fn post(&self, path: &str, body: &[u8]) -> Result<u16, String>;
    fn get(&self, path: &str) -> Result<(u16, Vec<u8>), String>;
}

pub struct Http {
    base: String,
    agent: ureq::Agent,
}

impl Http {
    /// Transport reel vers `SERVER` ; jamais dans un binaire de test.
    pub fn real() -> Option<Http> {
        if cfg!(test) {
            return None;
        }
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(TIMEOUT_S)).user_agent(&format!("AutoGameSoftwareSync/{}", crate::update::current_version())).build();
        Some(Http { base: SERVER.trim_end_matches('/').to_string(), agent })
    }
}

impl Transport for Http {
    fn post(&self, path: &str, body: &[u8]) -> Result<u16, String> {
        match self.agent.post(&format!("{}{}", self.base, path)).set("content-type", "application/json").send_bytes(body) {
            Ok(r) => Ok(r.status()),
            Err(ureq::Error::Status(c, _)) => Ok(c),
            Err(e) => Err(e.to_string()),
        }
    }

    fn get(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
        use std::io::Read;
        match self.agent.get(&format!("{}{}", self.base, path)).call() {
            Ok(r) => {
                let st = r.status();
                let mut buf = Vec::new();
                r.into_reader().take(8_000_000).read_to_end(&mut buf).map_err(|e| e.to_string())?;
                Ok((st, buf))
            }
            Err(ureq::Error::Status(c, _)) => Ok((c, Vec::new())),
            Err(e) => Err(e.to_string()),
        }
    }
}

// ------------------------------------------------------------------ etat et envoi

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct State {
    #[serde(default)]
    pub last_send_ms: u64,
    #[serde(default)]
    pub last_try_ms: u64,
    #[serde(default)]
    pub last_base_ms: u64,
    #[serde(default)]
    pub last_base_try_ms: u64,
    /// Jour du dernier releve quotidien (`daily_state`, `known_items`).
    #[serde(default)]
    pub snapshot_day: String,
    /// Empreintes des emplacements acceptes deja envoyes.
    #[serde(default)]
    pub sent_knowledge: Vec<String>,
    /// Identifiants dont l'effacement reste a demander au serveur (hors ligne au moment du clic).
    #[serde(default)]
    pub pending_erase: Vec<String>,
}

impl State {
    pub fn load(data_dir: &Path) -> State {
        std::fs::read_to_string(data_dir.join(STATE_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self, data_dir: &Path) {
        let _ = std::fs::create_dir_all(data_dir);
        let _ = crate::util::write_atomic(&data_dir.join(STATE_FILE), serde_json::to_string_pretty(self).unwrap_or_default().as_bytes());
    }
}

/// « Effacer mes donnees » : demande de suppression de l'identifiant actuel (envoyee des que possible), file videe,
/// nouvel identifiant. Retourne l'ancien identifiant.
pub fn request_erase(data_dir: &Path, now_ms: u64) -> Option<String> {
    let old = current_id(data_dir)?;
    let mut st = State::load(data_dir);
    if !st.pending_erase.contains(&old) {
        st.pending_erase.push(old.clone());
    }
    st.save(data_dir);
    clear_queue(data_dir);
    reset_id(data_dir, now_ms);
    Some(old)
}

/// Retrait du consentement : plus rien n'est envoye, la file est videe.
pub fn withdraw(data_dir: &Path) {
    clear_queue(data_dir);
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub sent_events: usize,
    pub dropped_events: usize,
    pub sent_records: usize,
    pub erased: usize,
    pub base_entries: Option<usize>,
    pub skipped: Option<&'static str>,
    pub error: Option<String>,
}

/// Un passage du thread d'envoi : effacements en attente, base maison (une fois par jour), puis evenements et
/// emplacements acceptes (une fois par jour, consentement donne, aucun programme en cours d'utilisation).
pub fn run_once(data_dir: &Path, consent: Option<bool>, now_ms: u64, t: &dyn Transport, busy: &dyn Fn() -> bool) -> Outcome {
    let mut out = Outcome::default();
    let mut st = State::load(data_dir);
    // Effacements demandes par l'utilisateur (quel que soit le consentement).
    if !st.pending_erase.is_empty() {
        let mut keep = Vec::new();
        for id in std::mem::take(&mut st.pending_erase) {
            let body = serde_json::to_vec(&json!({ "install_id": id })).unwrap_or_default();
            match t.post("/v1/erase", &body) {
                Ok(c) if (200..300).contains(&c) || c == 400 => out.erased += 1,
                _ => keep.push(id),
            }
        }
        st.pending_erase = keep;
        st.save(data_dir);
    }
    if consent == Some(false) {
        withdraw(data_dir);
        out.skipped = Some("consentement refuse");
        return out;
    }
    if consent != Some(true) {
        out.skipped = Some("consentement non donne");
        return out;
    }
    if busy() {
        out.skipped = Some("programme en cours d'utilisation");
        return out;
    }
    // Base maison validee : une fois par jour (6 h apres un echec).
    if now_ms.saturating_sub(st.last_base_ms) >= DAY_MS && now_ms.saturating_sub(st.last_base_try_ms) >= RETRY_MS {
        st.last_base_try_ms = now_ms;
        match t.get("/v1/knowledge/base") {
            Ok((200, body)) => match crate::knowledge::home::Base::parse(&String::from_utf8_lossy(&body)) {
                Some(b) => {
                    out.base_entries = Some(b.entries.len());
                    let _ = b.save(data_dir);
                    st.last_base_ms = now_ms;
                }
                None => out.error = Some("base maison illisible".into()),
            },
            Ok((c, _)) => out.error = Some(format!("base maison : code {}", c)),
            Err(e) => out.error = Some(e),
        }
        st.save(data_dir);
    }
    // Evenements et emplacements : une fois par jour au plus.
    if now_ms.saturating_sub(st.last_send_ms) < DAY_MS || now_ms.saturating_sub(st.last_try_ms) < RETRY_MS {
        out.skipped = Some("deja envoye aujourd'hui");
        return out;
    }
    st.last_try_ms = now_ms;
    st.save(data_dir);
    let id = install_id(data_dir, now_ms);
    let mut failed = false;
    {
        // File lue puis relachee pendant l'envoi (l'interface peut ajouter des evenements entre-temps).
        let events = {
            let _g = FILE_LOCK.lock();
            prune(read_queue(data_dir), now_ms)
        };
        let batches = event_batches(&id, &events);
        let in_batch: std::collections::HashSet<usize> = batches.iter().flat_map(|(i, _)| i.iter().copied()).collect();
        let mut done: Vec<usize> = Vec::new();
        for (idx, body) in batches {
            match t.post("/v1/events", &body) {
                Ok(c) if (200..300).contains(&c) => {
                    out.sent_events += idx.len();
                    done.extend(idx);
                }
                // Refuse par le serveur (format) : jamais renvoye.
                Ok(400) | Ok(413) => {
                    out.dropped_events += idx.len();
                    done.extend(idx);
                }
                Ok(c) => {
                    out.error = Some(format!("evenements : code {}", c));
                    failed = true;
                    break;
                }
                Err(e) => {
                    out.error = Some(e);
                    failed = true;
                    break;
                }
            }
        }
        // Retires de la file : envoyes, refuses, ou trop gros pour tout lot.
        let gone: Vec<&Event> = events.iter().enumerate().filter(|(i, _)| done.contains(i) || !in_batch.contains(i)).map(|(_, e)| e).collect();
        let _g = FILE_LOCK.lock();
        let left: Vec<Event> = prune(read_queue(data_dir), now_ms).into_iter().filter(|e| !gone.contains(&e)).collect();
        write_queue(data_dir, &left);
    }
    if !failed {
        let recs: Vec<crate::knowledge::queue::Record> = crate::knowledge::queue::read_all(data_dir).into_iter().filter(|r| !st.sent_knowledge.contains(&record_key(r))).collect();
        let (ok, bad): (Vec<_>, Vec<_>) = recs.into_iter().partition(record_ok);
        st.sent_knowledge.extend(bad.iter().map(record_key));
        let items: Vec<Value> = ok.iter().map(|r| serde_json::to_value(r).unwrap_or(Value::Null)).collect();
        for (idx, body) in chunks(envelope(&id), "records", items, MAX_RECORDS) {
            match t.post("/v1/knowledge", &body) {
                Ok(c) if (200..300).contains(&c) || c == 400 || c == 413 => {
                    if (200..300).contains(&c) {
                        out.sent_records += idx.len();
                    }
                    st.sent_knowledge.extend(idx.iter().map(|i| record_key(&ok[*i])));
                }
                Ok(c) => {
                    out.error = Some(format!("emplacements : code {}", c));
                    failed = true;
                    break;
                }
                Err(e) => {
                    out.error = Some(e);
                    failed = true;
                    break;
                }
            }
        }
        st.sent_knowledge.dedup();
        if st.sent_knowledge.len() > 5000 {
            let cut = st.sent_knowledge.len() - 5000;
            st.sent_knowledge.drain(..cut);
        }
    }
    if !failed {
        st.last_send_ms = now_ms;
    }
    st.save(data_dir);
    out
}

// ------------------------------------------------------------------ releve quotidien

/// Titre connu d'une synchro : profil integre, base embarquee ou base maison. Jamais le nom tape par l'utilisateur
/// (seul le titre de la base est envoye). (titre, logiciel ?)
pub fn known_title(g: &Game, db: &crate::gamedb::GameDb, profiles: &[crate::profiles::Profile], home: &crate::knowledge::home::Base) -> Option<(String, bool)> {
    let exe = g.exe.as_deref().map(crate::hints::exe_file_name).map(|e| e.to_lowercase()).filter(|e| !e.is_empty());
    for p in profiles {
        let by_exe = exe.as_deref().map(|x| p.detection.exes.iter().any(|e| crate::hints::exe_file_name(e).eq_ignore_ascii_case(x))).unwrap_or(false);
        if by_exe || p.name.eq_ignore_ascii_case(g.name.trim()) {
            return Some((p.name.clone(), matches!(p.category.as_str(), "app" | "config")));
        }
    }
    if let Some(x) = exe.as_deref().filter(|x| !crate::gamedetect::is_generic_name(x)) {
        let hits = db.find_by_exe(x);
        if !hits.is_empty() && hits.len() <= 3 && hits.iter().all(|h| crate::gamedb::norm(&h.name) == crate::gamedb::norm(&hits[0].name)) {
            return Some((hits[0].name.clone(), false));
        }
    }
    let byname = db.find_by_name(&g.name);
    if let Some(h) = byname.first().filter(|h| crate::gamedb::norm(&h.name) == crate::gamedb::norm(&g.name) && crate::gamedb::norm(&g.name).len() >= 3) {
        return Some((h.name.clone(), false));
    }
    home.title_for(&g.name, g.exe.as_deref())
}

/// Appareils qui ont ecrit une version de cette synchro dans le dossier partage.
pub fn devices_of(dest: &Path, g: &Game) -> usize {
    let mut seen: Vec<String> = Vec::new();
    for root in crate::cloud::roots_for(dest, g) {
        let Ok(rd) = std::fs::read_dir(root.join("saves")) else { continue };
        for e in rd.flatten().take(5000) {
            if let Some(m) = crate::machinecolor::machine_of_dirname(&e.file_name().to_string_lossy()) {
                if !seen.iter().any(|s| s.eq_ignore_ascii_case(m)) {
                    seen.push(m.to_string());
                }
            }
        }
    }
    seen.len()
}

/// `known_items` : titres connus proteges (100 au plus, dedoublonnes).
pub fn known_items_event(items: &[(String, bool, bool)], now_ms: u64) -> Event {
    let mut list: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (title, app, multi) in items {
        let k = crate::gamedb::norm(title);
        if !clean_text(title, 80) || seen.contains(&k) || list.len() >= MAX_KNOWN {
            continue;
        }
        seen.push(k);
        list.push(json!({ "title": title, "type": if *app { "app" } else { "game" }, "multi": if *multi { "yes" } else { "no" } }));
    }
    let mut e = Event::new("known_items", &[], now_ms);
    e.props.insert("items".into(), Value::Array(list));
    e
}

/// Releve quotidien depuis les fichiers de l'appli (config, reglages, dossier partage) : `daily_state` et `known_items`.
pub fn snapshot(data_dir: &Path, now_ms: u64) -> Vec<Event> {
    let ctx = crate::Ctx::new(data_dir.to_path_buf());
    let cfg = crate::config::Config::load(&ctx).unwrap_or_default();
    let s = crate::settings::Settings::load_from(data_dir).unwrap_or_default();
    let dest = cfg.dest.clone().filter(|d| !d.as_os_str().is_empty());
    let reach = dest.as_deref().filter(|d| d.is_dir());
    let devices = reach.map(|d| crate::cloud::machines_seen(d).len()).unwrap_or(1);
    let theme = match s.theme {
        crate::settings::ThemeMode::System => "system",
        crate::settings::ThemeMode::Dark => "dark",
        crate::settings::ThemeMode::Light => "light",
    };
    let vault = vault_kind(dest.as_deref(), dest.as_deref().map(is_removable).unwrap_or(false));
    let state = Event::new(
        "daily_state",
        &[("items", bucket_count(cfg.games.len())), ("devices", bucket_devices(devices)), ("vault", vault), ("detection", if s.detect_enabled { "on" } else { "off" }), ("update_mode", match s.update_mode() { crate::settings::UpdateMode::Auto => "auto", crate::settings::UpdateMode::Ask => "notify", crate::settings::UpdateMode::Never => "off" }), ("theme", theme)],
        now_ms,
    );
    let db = crate::gamedb::embedded(data_dir);
    let profiles = crate::profiles::load_embedded();
    let home = crate::knowledge::home::Base::load(data_dir);
    let items: Vec<(String, bool, bool)> = cfg.games.iter().filter_map(|g| known_title(g, &db, &profiles, &home).map(|(t, app)| (t, app, reach.map(|d| devices_of(d, g) > 1).unwrap_or(false)))).collect();
    vec![state, known_items_event(&items, now_ms)]
}

/// Releve quotidien mis en file si ce n'est pas deja fait aujourd'hui (consentement donne).
pub fn snapshot_if_due(data_dir: &Path, consent: Option<bool>, now_ms: u64) -> bool {
    if consent != Some(true) {
        return false;
    }
    let mut st = State::load(data_dir);
    let today = utc_day(now_ms);
    if st.snapshot_day == today {
        return false;
    }
    for e in snapshot(data_dir, now_ms) {
        push(data_dir, e, consent);
    }
    st.snapshot_day = today;
    st.save(data_dir);
    true
}

/// `app_start` (une fois par jour).
pub fn app_start_event(lang: crate::i18n::Lang, expert: bool, startup: bool, big: bool, now_ms: u64) -> Event {
    let l = match lang {
        crate::i18n::Lang::Fr => "fr",
        crate::i18n::Lang::En => "en",
        crate::i18n::Lang::Ru => "ru",
        crate::i18n::Lang::Es => "es",
        crate::i18n::Lang::Zh => "zh",
    };
    let v = crate::update::current_version();
    Event::new("app_start", &[("version", v), ("os", os_name()), ("os_build", os_build()), ("arch", arch()), ("lang", l), ("mode", if expert { "expert" } else { "simple" }), ("source", if startup { "startup" } else { "manual" }), ("big", if big { "yes" } else { "no" })], now_ms)
}

/// Texte lisible de ce qui est en attente d'envoi (fenetre « Voir ce qui est envoye ») : identifiant, evenements tels
/// qu'ils partiront, emplacements acceptes pas encore envoyes.
/// `preview` : ajoute le releve quotidien qui sera mis en file juste avant le prochain envoi (lecture du dossier
/// partage : a appeler hors du thread de l'interface).
pub fn pending_text(data_dir: &Path, preview: bool) -> String {
    let mut s = pending_queues(data_dir);
    let now = crate::util::now_ms();
    if preview && State::load(data_dir).snapshot_day != utc_day(now) {
        s.push_str("\n(daily_state / known_items)\n");
        for e in snapshot(data_dir, now) {
            s.push_str(&serde_json::to_string(&e.wire()).unwrap_or_default());
            s.push('\n');
        }
    }
    s
}

fn pending_queues(data_dir: &Path) -> String {
    let mut s = String::new();
    s.push_str(&format!("install_id: {}\n", current_id(data_dir).unwrap_or_else(|| "-".into())));
    s.push_str(&format!("version: {} · os: {}\n\n", crate::update::current_version(), os_name()));
    let q = read_queue(data_dir);
    s.push_str(&format!("{} ({})\n", QUEUE_FILE, q.len()));
    for e in &q {
        s.push_str(&serde_json::to_string(&e.wire()).unwrap_or_default());
        s.push('\n');
    }
    let st = State::load(data_dir);
    let recs: Vec<_> = crate::knowledge::queue::read_all(data_dir).into_iter().filter(|r| !st.sent_knowledge.contains(&record_key(r))).collect();
    s.push_str(&format!("\n{} ({})\n", crate::knowledge::queue::FILE, recs.len()));
    for r in &recs {
        s.push_str(&serde_json::to_string(r).unwrap_or_default());
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiant_uuid_v4() {
        for _ in 0..50 {
            assert!(is_uuid_v4(&new_uuid()));
        }
        assert_ne!(new_uuid(), new_uuid());
        assert!(!is_uuid_v4("3f2b8c1e-7a4d-1e9b-8c2a-1d5e6f7a8b9c"));
        assert!(!is_uuid_v4("3F2B8C1E-7A4D-4E9B-8C2A-1D5E6F7A8B9C"));
    }

    #[test]
    fn versions_et_jours() {
        assert!(is_version("1.4.0") && is_version("1.4.0-dev") && !is_version("1.4") && !is_version("1.4.0; x") && !is_version("1.4.0-DEV"));
        assert_eq!(utc_day(0), "1970-01-01");
        assert_eq!(ask_key("1.4.0-dev"), "1.4");
        assert!(needs_question(None, "1.4.0") && needs_question(Some("1.3"), "1.4.0") && !needs_question(Some("1.4"), "1.4.2"));
    }
}
