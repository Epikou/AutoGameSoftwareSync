//! Dates lisibles en HEURE LOCALE de l'utilisateur, selon la langue choisie.
//! FR : `31/12/2027 02:24:15` ; EN : `2027-12-31 02:24:15`. Le fuseau est abstrait (`TzProvider`)
//! pour pouvoir tester la conversion UTC -> local sans dependre de la machine.
use crate::i18n::Lang;
use crate::util;

pub trait TzProvider {
    /// Decalage (secondes) de l'heure locale par rapport a UTC a l'instant `utc_ms` (heure d'ete comprise).
    fn utc_offset_secs(&self, utc_ms: u64) -> i32;
}

/// Fuseau fixe (tests).
pub struct FixedTz(pub i32);
impl TzProvider for FixedTz {
    fn utc_offset_secs(&self, _utc_ms: u64) -> i32 {
        self.0
    }
}

/// Fuseau reel de Windows (regles d'heure d'ete de la date concernee).
pub struct WinTz;
#[cfg(not(windows))]
impl TzProvider for WinTz {
    fn utc_offset_secs(&self, utc_ms: u64) -> i32 {
        let t: libc::time_t = (utc_ms / 1000) as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let ok = unsafe { libc::localtime_r(&t, &mut tm) };
        if ok.is_null() { 0 } else { tm.tm_gmtoff as i32 }
    }
}

#[cfg(windows)]
impl TzProvider for WinTz {
    fn utc_offset_secs(&self, utc_ms: u64) -> i32 {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
        let (y, mo, d, h, mi, s, ms) = util::civil_from_ms(utc_ms as i64);
        let utc = SYSTEMTIME { wYear: y as u16, wMonth: mo as u16, wDayOfWeek: 0, wDay: d as u16, wHour: h as u16, wMinute: mi as u16, wSecond: s as u16, wMilliseconds: ms as u16 };
        let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
        let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };
        if ok == 0 {
            return 0;
        }
        let l = util::ms_from_civil(local.wYear as i64, local.wMonth as u32, local.wDay as u32, local.wHour as u32, local.wMinute as u32, local.wSecond as u32, 0);
        let u = util::ms_from_civil(y, mo, d, h, mi, s, 0);
        ((l - u) / 1000) as i32
    }
}

/// Date et heure locales formatees selon la langue.
pub fn format_local(utc_ms: u64, lang: Lang, tz: &dyn TzProvider) -> String {
    let off = tz.utc_offset_secs(utc_ms) as i64;
    let (y, mo, d, h, mi, s, _) = util::civil_from_ms(utc_ms as i64 + off * 1000);
    match lang {
        Lang::Fr | Lang::Es => format!("{:02}/{:02}/{:04} {:02}:{:02}:{:02}", d, mo, y, h, mi, s),
        Lang::Ru => format!("{:02}.{:02}.{:04} {:02}:{:02}:{:02}", d, mo, y, h, mi, s),
        Lang::En | Lang::Zh => format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s),
    }
}

/// Idem avec le fuseau reel de la machine.
pub fn format_now_tz(utc_ms: u64, lang: Lang) -> String {
    format_local(utc_ms, lang, &WinTz)
}

/// ISO 8601 locale avec decalage (`2027-12-31T02:24:15+01:00`) : champ `saved_at_local` des meta.
pub fn iso_local(utc_ms: u64, tz: &dyn TzProvider) -> String {
    let off = tz.utc_offset_secs(utc_ms) as i64;
    let (y, mo, d, h, mi, s, _) = util::civil_from_ms(utc_ms as i64 + off * 1000);
    let sign = if off < 0 { '-' } else { '+' };
    let a = off.abs();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}", y, mo, d, h, mi, s, sign, a / 3600, (a % 3600) / 60)
}

/// Libelle lisible d'un nom de version (`20261023-001136-166-PC-DESKTOP`, `versions/20261022-...`,
/// `current`) : date locale si le nom commence par un horodatage, sinon le nom tel quel.
pub fn version_label(name: &str, lang: Lang, tz: &dyn TzProvider) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    match util::parse_timestamp(base) {
        Some(ms) => format_local(ms, lang, tz),
        None => name.to_string(),
    }
}

/// Horodatage de journal (`AAAAMMJJ-HHMMSS-mmm`, UTC) -> date locale lisible (inchange si invalide).
pub fn log_ts_label(ts: &str, lang: Lang, tz: &dyn TzProvider) -> String {
    match util::parse_timestamp(ts) {
        Some(ms) => format_local(ms, lang, tz),
        None => ts.to_string(),
    }
}
