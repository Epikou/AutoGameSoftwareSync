//! Detection de processus (abstraite pour pouvoir la simuler en test).
//!
//! L'implementation reelle utilise directement l'API Windows (Toolhelp / OpenProcess) : AUCUN
//! sous-processus (`tasklist` ouvrait une fenetre de terminal a chaque verification, car l'exe est
//! en sous-systeme "windows").
use crate::Res;

pub trait ProcessChecker {
    /// Ok(true) si un processus portant ce nom d'exe tourne. Err = impossible de savoir
    /// (l'appelant doit alors considerer le element comme "en cours").
    fn is_running(&self, exe: &str) -> Res<bool>;
}

/// Implementation reelle : instantane Toolhelp32 de la liste des processus.
pub struct ToolhelpChecker;

#[cfg(windows)]
impl ProcessChecker for ToolhelpChecker {
    fn is_running(&self, exe: &str) -> Res<bool> {
        let wanted = normalize_exe_name(exe);
        if wanted.is_empty() {
            return Err("nom d'exe vide".into());
        }
        let mut found = false;
        enum_process_names(|name| {
            if names_match(name, &wanted) {
                found = true;
                false // stop
            } else {
                true
            }
        })?;
        Ok(found)
    }
}

/// Nom d'exe a comparer : dernier composant d'un chemin eventuel, espaces retires, minuscules.
pub fn normalize_exe_name(exe: &str) -> String {
    let base = exe.trim().rsplit(['/', '\\']).next().unwrap_or("");
    base.trim().to_lowercase()
}

/// `szExeFile` (UTF-16, termine par un NUL dans un tampon de 260) correspond-il au nom voulu
/// (deja normalise) ? Comparaison insensible a la casse, sur le nom COMPLET (pas de troncature).
pub fn names_match(sz_exe_file: &[u16], wanted_normalized: &str) -> bool {
    let end = sz_exe_file.iter().position(|&c| c == 0).unwrap_or(sz_exe_file.len());
    String::from_utf16_lossy(&sz_exe_file[..end]).to_lowercase() == wanted_normalized
}

#[cfg(windows)]
/// Parcourt les processus ; `f` renvoie false pour arreter. Err si la liste est indisponible.
fn enum_process_names(mut f: impl FnMut(&[u16]) -> bool) -> Res<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return Err(format!("liste des processus indisponible (erreur {})", GetLastError()).into());
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut res: Res<()> = Ok(());
        let mut ok = Process32FirstW(snap, &mut pe) != 0;
        loop {
            if !ok {
                let e = GetLastError();
                if e != ERROR_NO_MORE_FILES {
                    res = Err(format!("parcours des processus interrompu (erreur {})", e).into());
                }
                break;
            }
            if !f(&pe.szExeFile) {
                break;
            }
            ok = Process32NextW(snap, &mut pe) != 0;
        }
        CloseHandle(snap);
        res
    }
}

#[cfg(windows)]
/// Le processus `pid` existe-t-il encore ? En cas de doute (acces refuse, erreur inattendue)
/// on repond `true` : on ne vole jamais un verrou d'instance a un processus qui peut etre vivant.
pub fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE};
    if pid == 0 {
        return false;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            // PID inexistant => ERROR_INVALID_PARAMETER ; tout autre echec (ex. acces refuse) => doute => vivant.
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        // Un processus termine dont un handle reste ouvert ailleurs est "signale" : il est mort.
        let alive = WaitForSingleObject(h, 0) == WAIT_TIMEOUT;
        CloseHandle(h);
        alive
    }
}

/// Un processus vu par l'enumeration : pid, nom d'exe (minuscules), chemin complet si lisible, heure de demarrage (FILETIME, 100 ns).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub exe: String,
    pub path: Option<std::path::PathBuf>,
    pub start: u64,
}

/// Processus avec son parent (pid) et, sous Linux, la presence d'un terminal de controle (indices de la detection).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcExt {
    pub info: ProcInfo,
    pub ppid: u32,
    pub tty: bool,
}

/// Liste des processus avec chemin et heure de demarrage (voir `list_processes_ext`).
pub fn list_processes() -> Res<Vec<ProcInfo>> {
    Ok(list_processes_ext()?.into_iter().map(|p| p.info).collect())
}

/// Heure de demarrage d'un processus (unite de `ProcInfo::start`) convertie en ms depuis 1970 ; None si inconnue.
#[cfg(windows)]
pub fn start_unix_ms(start: u64) -> Option<u64> {
    // FILETIME : intervalles de 100 ns depuis 1601.
    (start / 10_000).checked_sub(11_644_473_600_000).filter(|_| start > 0)
}

/// Heure de demarrage d'un processus (ticks depuis le demarrage du systeme) convertie en ms depuis 1970.
#[cfg(not(windows))]
pub fn start_unix_ms(start: u64) -> Option<u64> {
    let btime = std::fs::read_to_string("/proc/stat").ok().and_then(|s| crate::procscan::parse_btime(&s))?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let hz = if hz > 0 { hz as u64 } else { 100 };
    Some(btime * 1000 + start * 1000 / hz)
}

#[cfg(windows)]
/// Liste des processus avec chemin, heure de demarrage et parent : une seule capture Toolhelp, aucun sous-processus. Les
/// processus dont le chemin n'est pas lisible (acces refuse) ont `path: None`.
pub fn list_processes_ext() -> Res<Vec<ProcExt>> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return Err(format!("liste des processus indisponible (erreur {})", GetLastError()).into());
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut pe) != 0;
        while ok {
            let end = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
            let exe = String::from_utf16_lossy(&pe.szExeFile[..end]).to_lowercase();
            let (mut path, mut start) = (None, 0u64);
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pe.th32ProcessID);
            if !h.is_null() {
                let mut buf = [0u16; 1024];
                let mut n = buf.len() as u32;
                if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut n) != 0 {
                    path = Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])));
                }
                let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = std::mem::zeroed();
                if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0 {
                    start = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64;
                }
                CloseHandle(h);
            }
            out.push(ProcExt { info: ProcInfo { pid: pe.th32ProcessID, exe, path, start }, ppid: pe.th32ParentProcessID, tty: false });
            ok = Process32NextW(snap, &mut pe) != 0;
        }
        let e = GetLastError();
        CloseHandle(snap);
        if e != ERROR_NO_MORE_FILES && out.is_empty() {
            return Err(format!("parcours des processus interrompu (erreur {})", e).into());
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ Linux : lecture de /proc

#[cfg(not(windows))]
impl ProcessChecker for ToolhelpChecker {
    fn is_running(&self, exe: &str) -> Res<bool> {
        let wanted = normalize_exe_name(exe);
        if wanted.is_empty() {
            return Err("nom d'exe vide".into());
        }
        let procs = crate::procscan::scan(std::path::Path::new("/proc")).map_err(|e| format!("liste des processus indisponible : {}", e))?;
        Ok(procs.iter().any(|p| crate::procscan::matches_wanted(p, &wanted)))
    }
}

/// Le processus `pid` existe-t-il encore ? En cas de doute (acces refuse) on repond `true`.
#[cfg(not(windows))]
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // signal 0 : aucun signal envoye, seulement la verification d'existence (EPERM = existe mais appartient a un autre).
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if r == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Liste des processus avec chemin, heure de demarrage (ticks depuis le boot), parent et terminal, via /proc.
#[cfg(not(windows))]
pub fn list_processes_ext() -> Res<Vec<ProcExt>> {
    let procs = crate::procscan::scan(std::path::Path::new("/proc")).map_err(|e| format!("liste des processus indisponible : {}", e))?;
    Ok(procs
        .iter()
        .map(|p| ProcExt { info: ProcInfo { pid: p.pid, exe: crate::procscan::display_exe(p), path: crate::procscan::display_path(p), start: p.start }, ppid: p.ppid, tty: p.tty })
        .collect())
}
