//! Surveillance par EVENEMENTS du dossier partage (ReadDirectoryChangesW, recursif, API Windows directe, aucune crate).
//! Un thread dedie envoie les chemins relatifs modifies ; un dossier partage indisponible est reessaye toutes les 5 s
//! (aucune erreur fatale). Le tri par jeu et le debounce sont dans `reactive`.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchMsg {
    /// Chemin relatif (separateur `\`) d'un fichier ou dossier cree / modifie / renomme / supprime.
    Changed(String),
    /// Tampon deborde ou dossier (re)connecte : des evenements ont pu etre perdus, tout reverifier.
    Rescan,
}

pub struct DirWatch {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl DirWatch {
    /// Demarre la surveillance de `root`. `log` recoit les changements d'etat (disponible / indisponible).
    pub fn spawn(root: PathBuf, tx: Sender<WatchMsg>, log: impl Fn(&str) + Send + 'static) -> DirWatch {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let join = std::thread::spawn(move || run(&root, &tx, &s2, &log));
        DirWatch { stop, join: Some(join) }
    }
}

impl Drop for DirWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn sleep_stoppable(stop: &AtomicBool, ms: u64) {
    let mut left = ms;
    while left > 0 && !stop.load(Ordering::SeqCst) {
        let s = left.min(200);
        std::thread::sleep(Duration::from_millis(s));
        left -= s;
    }
}

fn run(root: &Path, tx: &Sender<WatchMsg>, stop: &AtomicBool, log: &dyn Fn(&str)) {
    let mut was_down = false;
    while !stop.load(Ordering::SeqCst) {
        match watch_once(root, tx, stop, &mut was_down, log) {
            Ok(()) => {}
            Err(why) => {
                if !was_down {
                    log(&format!("[Surveillance] dossier partage indisponible pour la surveillance par evenements ({}) : reessai toutes les 5 s, le filet de secours continue", why));
                    was_down = true;
                }
            }
        }
        sleep_stoppable(stop, 5000);
    }
}

#[cfg(windows)]
/// Ouvre le dossier et traite les evenements jusqu'a l'arret ou une erreur (dossier disparu...).
fn watch_once(root: &Path, tx: &Sender<WatchMsg>, stop: &AtomicBool, was_down: &mut bool, log: &dyn Fn(&str)) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_IO_INCOMPLETE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    let wide: Vec<u16> = root.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let h = CreateFileW(wide.as_ptr(), FILE_LIST_DIRECTORY, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED, std::ptr::null_mut());
        if h == INVALID_HANDLE_VALUE {
            return Err(format!("code {}", GetLastError()));
        }
        let ev = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
        if ev.is_null() {
            CloseHandle(h);
            return Err("evenement impossible".into());
        }
        if *was_down {
            log("[Surveillance] dossier partage de nouveau disponible : surveillance par evenements reprise");
            *was_down = false;
            // Des changements ont pu arriver pendant l'indisponibilite : une verification generale.
            let _ = tx.send(WatchMsg::Rescan);
        }
        let mut buf = vec![0u32; 16 * 1024]; // 64 Ko, aligne DWORD
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE;
        let mut result: Result<(), String> = Ok(());
        'outer: while !stop.load(Ordering::SeqCst) {
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = ev;
            ResetEvent(ev);
            let ok = ReadDirectoryChangesW(h, buf.as_mut_ptr() as *mut _, (buf.len() * 4) as u32, 1, filter, std::ptr::null_mut(), &mut ov, None);
            if ok == 0 {
                result = Err(format!("lecture impossible (code {})", GetLastError()));
                break;
            }
            loop {
                if stop.load(Ordering::SeqCst) {
                    CancelIoEx(h, &ov);
                    let mut n = 0u32;
                    GetOverlappedResult(h, &ov, &mut n, 1); // attend la fin de l'annulation (le tampon doit rester valide)
                    break 'outer;
                }
                if WaitForSingleObject(ev, 300) == WAIT_OBJECT_0 {
                    break;
                }
            }
            let mut n = 0u32;
            if GetOverlappedResult(h, &ov, &mut n, 0) == 0 {
                let e = GetLastError();
                if e == ERROR_IO_INCOMPLETE {
                    continue;
                }
                result = Err(format!("dossier perdu (code {})", e));
                break;
            }
            if n == 0 {
                let _ = tx.send(WatchMsg::Rescan); // debordement du tampon
                continue;
            }
            let base = buf.as_ptr() as *const u8;
            let mut off = 0usize;
            loop {
                let rec = base.add(off) as *const FILE_NOTIFY_INFORMATION;
                let len = ((*rec).FileNameLength / 2) as usize;
                let name = std::slice::from_raw_parts((*rec).FileName.as_ptr(), len);
                let _ = tx.send(WatchMsg::Changed(String::from_utf16_lossy(name)));
                let next = (*rec).NextEntryOffset as usize;
                if next == 0 || off + next >= n as usize {
                    break;
                }
                off += next;
            }
        }
        CloseHandle(ev);
        CloseHandle(h);
        result
    }
}

// ------------------------------------------------------------------ Linux : inotify

/// Profondeur maximale surveillee sous la racine (racine / dossier de jeu / sauvegarde / sous-dossier).
#[cfg(not(windows))]
const MAX_DEPTH: usize = 4;

#[cfg(not(windows))]
fn add_tree(fd: i32, dir: &Path, depth: usize, map: &mut std::collections::HashMap<i32, PathBuf>) {
    use std::os::unix::ffi::OsStrExt;
    let mask = libc::IN_MODIFY | libc::IN_CLOSE_WRITE | libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return };
    let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), mask) };
    if wd < 0 {
        return;
    }
    map.insert(wd, dir.to_path_buf());
    if depth + 1 >= MAX_DEPTH {
        return;
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.filter_map(|e| e.ok()) {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                add_tree(fd, &e.path(), depth + 1, map);
            }
        }
    }
}

/// Chemin relatif a `root`, avec le separateur `\` attendu par `reactive`.
#[cfg(not(windows))]
pub fn relative_backslash(root: &Path, full: &Path) -> Option<String> {
    let rel = full.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    if parts.is_empty() { None } else { Some(parts.join("\\")) }
}

#[cfg(not(windows))]
fn watch_once(root: &Path, tx: &Sender<WatchMsg>, stop: &AtomicBool, was_down: &mut bool, log: &dyn Fn(&str)) -> Result<(), String> {
    use std::collections::HashMap;
    if !root.is_dir() {
        return Err("dossier introuvable".into());
    }
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        return Err(format!("inotify indisponible ({})", std::io::Error::last_os_error()));
    }
    let mut map: HashMap<i32, PathBuf> = HashMap::new();
    add_tree(fd, root, 0, &mut map);
    if map.is_empty() {
        unsafe { libc::close(fd) };
        return Err("surveillance impossible (limite inotify ou droits)".into());
    }
    if *was_down {
        log("[Surveillance] dossier partage de nouveau disponible : surveillance par evenements reprise");
        *was_down = false;
        let _ = tx.send(WatchMsg::Rescan);
    }
    let mut buf = vec![0u8; 64 * 1024];
    let mut result: Result<(), String> = Ok(());
    while !stop.load(Ordering::SeqCst) {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, 300) };
        if r < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            result = Err("attente impossible".into());
            break;
        }
        if r == 0 {
            if !root.is_dir() {
                result = Err("dossier perdu".into());
                break;
            }
            continue;
        }
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n <= 0 {
            continue;
        }
        let n = n as usize;
        let mut off = 0usize;
        let hdr = std::mem::size_of::<libc::inotify_event>();
        while off + hdr <= n {
            let ev = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off) as *const libc::inotify_event) };
            let name_len = ev.len as usize;
            let name = if name_len > 0 {
                let raw = &buf[off + hdr..(off + hdr + name_len).min(n)];
                String::from_utf8_lossy(raw.split(|&c| c == 0).next().unwrap_or(&[])).to_string()
            } else {
                String::new()
            };
            off += hdr + name_len;
            if ev.mask & libc::IN_Q_OVERFLOW != 0 {
                let _ = tx.send(WatchMsg::Rescan);
                continue;
            }
            if ev.mask & libc::IN_IGNORED != 0 {
                map.remove(&ev.wd);
                continue;
            }
            let Some(dir) = map.get(&ev.wd).cloned() else { continue };
            let full = if name.is_empty() { dir.clone() } else { dir.join(&name) };
            if dir == root && name.is_empty() && ev.mask & (libc::IN_DELETE_SELF | libc::IN_MOVE_SELF) != 0 {
                result = Err("dossier perdu".into());
            }
            if let Some(rel) = relative_backslash(root, &full) {
                let _ = tx.send(WatchMsg::Changed(rel));
            }
            if ev.mask & libc::IN_ISDIR != 0 && ev.mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0 {
                let depth = full.strip_prefix(root).map(|p| p.components().count()).unwrap_or(MAX_DEPTH);
                add_tree(fd, &full, depth, &mut map);
                let _ = tx.send(WatchMsg::Rescan);
            }
        }
        if result.is_err() {
            break;
        }
    }
    unsafe { libc::close(fd) };
    result
}

#[cfg(all(test, not(windows)))]
mod linux_tests {
    use super::*;

    #[test]
    fn relative_path_uses_backslashes() {
        assert_eq!(relative_backslash(Path::new("/r"), Path::new("/r/Jeu/abc/f.bin")).as_deref(), Some("Jeu\\abc\\f.bin"));
        assert_eq!(relative_backslash(Path::new("/r"), Path::new("/r")), None);
        assert_eq!(relative_backslash(Path::new("/r"), Path::new("/autre/x")), None);
    }

    #[test]
    fn inotify_reports_a_new_file() {
        let t = std::env::temp_dir().join(format!("agss-inotify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("Jeu")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let w = DirWatch::spawn(t.clone(), tx, |_| {});
        std::thread::sleep(Duration::from_millis(600));
        std::fs::write(t.join("Jeu").join("meta.json"), b"x").unwrap();
        let mut seen = false;
        for _ in 0..30 {
            if let Ok(WatchMsg::Changed(p)) = rx.recv_timeout(Duration::from_millis(200)) {
                if p == "Jeu\\meta.json" {
                    seen = true;
                    break;
                }
            }
        }
        drop(w);
        let _ = std::fs::remove_dir_all(&t);
        assert!(seen, "l'evenement du nouveau fichier doit arriver");
    }
}
