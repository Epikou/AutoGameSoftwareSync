//! Utilitaires : horodatage, noms surs, hash, ecriture atomique.
use crate::Res;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn mtime_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// "AAAAMMJJ-HHMMSS-mmm" en UTC, triable lexicographiquement.
pub fn timestamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    // Algorithme de Howard Hinnant (jours -> date civile)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}", y, m, d, sod / 3600, (sod % 3600) / 60, sod % 60, ms % 1000)
}

/// Nom de jeu -> nom de dossier valide sous Windows.
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "_".into() } else { s }
}

pub fn hash_file(p: &Path) -> Res<String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("ouverture {} : {}", p.display(), e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn hash_bytes(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

pub fn hex_digest(b: &[u8]) -> String {
    hex(b)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// Ecriture atomique : fichier temporaire voisin puis renommage.
pub fn write_atomic(path: &Path, data: &[u8]) -> Res<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".agss-tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp); // notre propre fichier temporaire
        return Err(format!("renommage atomique {} : {}", path.display(), e).into());
    }
    Ok(())
}

// ------------------------------------------------------------------ calendrier civil (UTC)

/// ms depuis l'epoch -> (annee, mois, jour, heure, minute, seconde, ms), UTC.
pub fn civil_from_ms(ms: i64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let milli = ms.rem_euclid(1000) as u32;
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    if m <= 2 {
        y += 1;
    }
    (y, m, d, (sod / 3600) as u32, ((sod % 3600) / 60) as u32, (sod % 60) as u32, milli)
}

/// (annee, mois, jour, heure, minute, seconde, ms) UTC -> ms depuis l'epoch.
pub fn ms_from_civil(y: i64, m: u32, d: u32, h: u32, mi: u32, s: u32, ms: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    ((days * 86400 + h as i64 * 3600 + mi as i64 * 60 + s as i64) * 1000) + ms as i64
}

/// "AAAAMMJJ-HHMMSS-mmm" (UTC) -> ms. None si le format ne correspond pas.
pub fn parse_timestamp(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[8] != b'-' || b[15] != b'-' {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<u32> {
        let t = s.get(a..z)?;
        if t.bytes().all(|c| c.is_ascii_digit()) { t.parse().ok() } else { None }
    };
    let (y, mo, d, h, mi, se, ms) = (num(0, 4)?, num(4, 6)?, num(6, 8)?, num(9, 11)?, num(11, 13)?, num(13, 15)?, num(16, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    let v = ms_from_civil(y as i64, mo, d, h, mi, se, ms);
    if v < 0 { None } else { Some(v as u64) }
}

static ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Identifiant unique de sauvegarde (32 hex) : horloge ns + pid + compteur + sel.
pub fn new_id(salt: &str) -> String {
    let ns = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let c = ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    hash_bytes(format!("{}|{}|{}|{}", ns, std::process::id(), c, salt).as_bytes())[..32].to_string()
}

/// Nom de la machine ecrit dans les metadonnees (Windows : COMPUTERNAME ; Linux : nom d'hote).
#[cfg(not(windows))]
pub fn machine_name() -> String {
    // Linux : variable AUTOGAMESOFTWARESYNC_MACHINE (choix explicite), sinon le nom d'hote. Les noms d'hote par defaut
    // des consoles et distributions (« steamdeck » sur TOUS les Steam Deck...) recoivent un suffixe court et stable tire
    // de /etc/machine-id : deux appareils ne se confondent jamais dans le dossier partage.
    if let Some(n) = std::env::var("AUTOGAMESOFTWARESYNC_MACHINE").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        return n;
    }
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "machine-inconnue".into());
    let mid = std::fs::read_to_string("/etc/machine-id").or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id")).unwrap_or_default();
    linux_machine_name(&host, &mid)
}

/// Nom de machine Linux : le nom d'hote, suffixe de 4 caracteres de `machine-id` s'il s'agit d'un nom par defaut partage
/// par beaucoup d'appareils (steamdeck, localhost, bazzite...).
pub fn linux_machine_name(host: &str, machine_id: &str) -> String {
    const GENERIC: &[&str] = &["steamdeck", "localhost", "localhost.localdomain", "bazzite", "steamos", "chimeraos", "holoiso", "nobara", "fedora", "ubuntu", "archlinux", "linux", "deck", "machine-inconnue"];
    let h = host.trim();
    let id: String = machine_id.trim().chars().filter(|c| c.is_ascii_hexdigit()).take(4).collect();
    if GENERIC.contains(&h.to_lowercase().as_str()) && id.len() == 4 {
        format!("{}-{}", h, id)
    } else {
        h.to_string()
    }
}

#[cfg(windows)]
pub fn machine_name() -> String {
    let n = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    n.unwrap_or_else(|| "machine-inconnue".into())
}
