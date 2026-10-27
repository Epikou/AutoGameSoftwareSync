//! Heure corrigee par internet. L'horloge Windows peut deriver (ou etre fausse) : on mesure
//! l'ecart avec un serveur de temps (NTP/UDP 123, repli sur l'en-tete HTTP `Date` d'un serveur
//! HTTPS), on le memorise, et `saved_at` = heure locale + ecart. Jamais bloquant (timeouts courts,
//! thread d'arriere-plan), hors ligne = horloge systeme + une ligne de journal. Aucune donnee
//! personnelle n'est envoyee (un paquet NTP standard, ou une requete HEAD sans cookie).
//! IMPORTANT : l'horloge ne sert qu'a l'affichage « version la plus recente » ; aucune decision de
//! synchronisation n'en depend (voir backup::decide).
use crate::util;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

pub trait Clock: Send + Sync {
    /// Horloge systeme (ms depuis l'epoch).
    fn system_ms(&self) -> u64;
    /// Ecart connu (ms) a AJOUTER a l'horloge systeme pour obtenir l'heure exacte (0 si inconnu).
    fn offset_ms(&self) -> i64;
    /// Heure corrigee (ms UTC).
    fn corrected_ms(&self) -> u64 {
        (self.system_ms() as i64 + self.offset_ms()).max(0) as u64
    }
}

static OFFSET: AtomicI64 = AtomicI64::new(0);
static OFFSET_KNOWN: AtomicBool = AtomicBool::new(false);

/// Memorise l'ecart mesure (global au processus).
pub fn set_offset(ms: i64) {
    OFFSET.store(ms, Ordering::SeqCst);
    OFFSET_KNOWN.store(true, Ordering::SeqCst);
}

pub fn offset_known() -> bool {
    OFFSET_KNOWN.load(Ordering::SeqCst)
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn system_ms(&self) -> u64 {
        util::now_ms()
    }
    fn offset_ms(&self) -> i64 {
        OFFSET.load(Ordering::SeqCst)
    }
}

// ------------------------------------------------------------------ NTP (pur)

const NTP_UNIX_DELTA_SECS: i64 = 2_208_988_800;

fn ntp_ts_to_unix_ms(b: &[u8]) -> i64 {
    let secs = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64;
    let frac = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as i64;
    (secs - NTP_UNIX_DELTA_SECS) * 1000 + (frac * 1000) / (1i64 << 32)
}

fn unix_ms_to_ntp_ts(ms: u64) -> [u8; 8] {
    let secs = (ms / 1000) as i64 + NTP_UNIX_DELTA_SECS;
    let frac = ((ms % 1000) as i64 * (1i64 << 32)) / 1000;
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&(secs as u32).to_be_bytes());
    out[4..].copy_from_slice(&(frac as u32).to_be_bytes());
    out
}

/// Paquet de requete NTP (client, v3) avec l'heure d'emission `t1_ms` (Unix ms).
pub fn ntp_request(t1_ms: u64) -> [u8; 48] {
    let mut p = [0u8; 48];
    p[0] = 0x1B; // LI=0, VN=3, mode=3 (client)
    p[40..48].copy_from_slice(&unix_ms_to_ntp_ts(t1_ms));
    p
}

/// Ecart (ms) a ajouter a l'horloge locale, depuis une reponse NTP :
/// ((T2 - T1) + (T3 - T4)) / 2. None si la reponse est invalide (taille, mode, strate, horodatage nul).
pub fn ntp_offset_ms(t1_ms: u64, t4_ms: u64, resp: &[u8]) -> Option<i64> {
    if resp.len() < 48 {
        return None;
    }
    let mode = resp[0] & 0x07;
    let li = resp[0] >> 6;
    let stratum = resp[1];
    if mode != 4 && mode != 5 || li == 3 || stratum == 0 {
        return None;
    }
    if resp[32..40].iter().all(|b| *b == 0) || resp[40..48].iter().all(|b| *b == 0) {
        return None;
    }
    let t2 = ntp_ts_to_unix_ms(&resp[32..40]);
    let t3 = ntp_ts_to_unix_ms(&resp[40..48]);
    Some(((t2 - t1_ms as i64) + (t3 - t4_ms as i64)) / 2)
}

// ------------------------------------------------------------------ en-tete HTTP `Date` (pur)

/// `Tue, 15 Nov 1994 08:12:31 GMT` -> ms Unix.
pub fn parse_http_date(s: &str) -> Option<u64> {
    let s = s.trim();
    let rest = s.split_once(", ").map(|x| x.1).unwrap_or(s);
    let mut it = rest.split_whitespace();
    let day: u32 = it.next()?.parse().ok()?;
    let mon = it.next()?;
    let year: i64 = it.next()?.parse().ok()?;
    let hms = it.next()?;
    let tz = it.next().unwrap_or("GMT");
    if tz != "GMT" && tz != "UTC" {
        return None;
    }
    let m = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|x| x.eq_ignore_ascii_case(mon))? as u32 + 1;
    let mut p = hms.split(':');
    let (h, mi, se): (u32, u32, u32) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    if !(1..=31).contains(&day) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let v = util::ms_from_civil(year, m, day, h, mi, se.min(59), 0);
    if v < 0 { None } else { Some(v as u64) }
}

// ------------------------------------------------------------------ mesure reelle (reseau)

/// Source de mesure abstraite (tests : sources simulees).
pub trait TimeSource {
    /// Ecart (ms) a ajouter a l'horloge locale, et nom de la source. Err = injoignable.
    fn measure(&self) -> Result<(i64, String), String>;
}

pub struct NetTimeSource;

const NTP_SERVERS: [&str; 2] = ["time.windows.com:123", "pool.ntp.org:123"];
const HTTPS_FALLBACKS: [&str; 2] = ["https://www.microsoft.com", "https://www.google.com"];

fn ntp_query(server: &str) -> Result<i64, String> {
    use std::net::{ToSocketAddrs, UdpSocket};
    let addr = server.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("adresse introuvable")?;
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok();
    sock.set_write_timeout(Some(std::time::Duration::from_secs(3))).ok();
    let t1 = util::now_ms();
    sock.send_to(&ntp_request(t1), addr).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 128];
    let (n, _) = sock.recv_from(&mut buf).map_err(|e| e.to_string())?;
    let t4 = util::now_ms();
    ntp_offset_ms(t1, t4, &buf[..n]).ok_or_else(|| "reponse NTP invalide".to_string())
}

fn http_date_query(url: &str) -> Result<i64, String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(4)).build();
    let t1 = util::now_ms();
    let resp = agent.head(url).call().map_err(|e| e.to_string())?;
    let t4 = util::now_ms();
    let d = resp.header("Date").and_then(parse_http_date).ok_or("en-tete Date absent")?;
    // Resolution d'une seconde : on compare a l'instant median de l'echange (+500 ms de demi-seconde).
    Ok(d as i64 + 500 - ((t1 + t4) / 2) as i64)
}

impl TimeSource for NetTimeSource {
    fn measure(&self) -> Result<(i64, String), String> {
        let mut errs = Vec::new();
        for s in NTP_SERVERS {
            match ntp_query(s) {
                Ok(o) => return Ok((o, format!("NTP {}", s))),
                Err(e) => errs.push(format!("{} : {}", s, e)),
            }
        }
        for u in HTTPS_FALLBACKS {
            match http_date_query(u) {
                Ok(o) => return Ok((o, format!("HTTPS Date {}", u))),
                Err(e) => errs.push(format!("{} : {}", u, e)),
            }
        }
        Err(errs.join(" ; "))
    }
}

/// Une tentative : met a jour l'ecart global et renvoie la ligne de journal a ecrire.
pub fn sync_once(src: &dyn TimeSource) -> String {
    match src.measure() {
        Ok((off, name)) => {
            set_offset(off);
            format!("[Horloge] ecart avec l'heure internet : {} ms (source : {})", off, name)
        }
        Err(_) => "[Horloge] hors ligne ou serveur de temps injoignable : horloge systeme utilisee".to_string(),
    }
}

/// Fichier de memorisation de l'ecart.
fn offset_file(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("clock.json")
}

/// Recharge l'ecart memorise (utile hors ligne au demarrage).
pub fn load_saved_offset(data_dir: &std::path::Path) -> Option<i64> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(offset_file(data_dir)).ok()?).ok()?;
    let o = v.get("offset_ms")?.as_i64()?;
    set_offset(o);
    Some(o)
}

/// Demarre la synchronisation de l'horloge en arriere-plan : une mesure au demarrage puis toutes les
/// 6 h (10 min apres un echec). Ne bloque jamais l'appelant.
pub fn spawn_sync(data_dir: std::path::PathBuf, log: impl Fn(&str) + Send + 'static) {
    let _ = std::thread::Builder::new().name("agss-clock".into()).spawn(move || {
        if let Some(o) = load_saved_offset(&data_dir) {
            log(&format!("[Horloge] ecart memorise recharge : {} ms", o));
        }
        loop {
            let line = sync_once(&NetTimeSource);
            let ok = line.contains("ecart avec l'heure internet");
            log(&line);
            if ok {
                let _ = util::write_atomic(&offset_file(&data_dir), format!("{{\"offset_ms\":{},\"measured_ms\":{}}}", OFFSET.load(Ordering::SeqCst), util::now_ms()).as_bytes());
            }
            std::thread::sleep(std::time::Duration::from_secs(if ok { 6 * 3600 } else { 600 }));
        }
    });
}
