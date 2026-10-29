//! Utilitaires specifiques a Windows : console parente, icones d'exe, theme systeme,
//! demarrage automatique, notifications. Tout est "best effort" : un echec ne doit jamais
//! gener le reste de l'application.
use crate::registry::{delete_value, read_dword, value_exists, write_string, Hive};
use crate::vm::ThemeProbe;
use std::path::Path;

/// Nom de la valeur dans HKCU\Software\Microsoft\Windows\CurrentVersion\Run
pub const RUN_VALUE: &str = "AutoGameSoftwareSync";

// ------------------------------------------------------------------ console parente (CLI depuis un terminal)

/// L'exe est en sous-systeme "windows" (pas de console au double-clic). Quand on est lance depuis
/// un terminal avec des arguments (mode CLI), on s'attache a la console du parent et on rebranche
/// stdout/stderr/stdin SEULEMENT s'ils ne sont pas deja valides (redirections/pipes preservees).
pub fn attach_parent_console() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::*;
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        let fix = |id: u32, name: &str, write: bool| {
            let h = GetStdHandle(id);
            if h.is_null() || h == INVALID_HANDLE_VALUE {
                let f = std::fs::OpenOptions::new().read(!write).write(write).open(name);
                if let Ok(f) = f {
                    use std::os::windows::io::IntoRawHandle;
                    SetStdHandle(id, f.into_raw_handle() as _);
                }
            }
        };
        fix(STD_OUTPUT_HANDLE, "CONOUT$", true);
        fix(STD_ERROR_HANDLE, "CONOUT$", true);
        fix(STD_INPUT_HANDLE, "CONIN$", false);
    }
}

// ------------------------------------------------------------------ theme systeme

/// Theme Windows lu directement dans le registre (AppsUseLightTheme : 1 = clair, 0 = sombre).
pub struct RegThemeProbe;

impl ThemeProbe for RegThemeProbe {
    fn system_is_light(&self) -> Option<bool> {
        read_dword(Hive::CurrentUser, THEME_KEY, "AppsUseLightTheme").map(|v| v != 0)
    }
}

const THEME_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";

// ------------------------------------------------------------------ demarrage automatique (cle Run de HKCU)

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn run_key_command(exe: &Path) -> String {
    // Lance l'interface directement dans la zone de notification, surveillance active.
    format!("\"{}\" --tray", exe.display())
}

pub fn autostart_enabled() -> bool {
    value_exists(Hive::CurrentUser, RUN_KEY, RUN_VALUE)
}

pub fn set_autostart(enable: bool, exe: &Path) -> bool {
    if enable {
        write_string(Hive::CurrentUser, RUN_KEY, RUN_VALUE, &run_key_command(exe))
    } else {
        delete_value(Hive::CurrentUser, RUN_KEY, RUN_VALUE)
    }
}

/// Ouvre une URL avec le navigateur par defaut (ShellExecute : aucun sous-processus visible).
pub fn open_url(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let op: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let u: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), u.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

// ------------------------------------------------------------------ fenetre (afficher / masquer)

fn find_window(title: &str) -> Option<windows_sys::Win32::Foundation::HWND> {
    // Fenetre de premier niveau de CE processus portant ce titre (pas celle d'un autre programme,
    // p. ex. un Explorateur ouvert sur un dossier du meme nom).
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId};
    struct Q {
        title: Vec<u16>,
        pid: u32,
        found: HWND,
    }
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> i32 {
        let q = &mut *(l as *mut Q);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid != q.pid {
            return 1;
        }
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        if n > 0 && buf[..n as usize] == q.title[..] {
            q.found = h;
            return 0;
        }
        1
    }
    let mut q = Q { title: title.encode_utf16().collect(), pid: std::process::id(), found: std::ptr::null_mut() };
    unsafe {
        EnumWindows(Some(cb), &mut q as *mut Q as LPARAM);
    }
    if q.found.is_null() { None } else { Some(q.found) }
}

/// Une demande d'affichage (tray, 2e lancement) a eu lieu : le masquage de demarrage doit s'effacer.
static SHOW_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn show_window(title: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    SHOW_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(h) = find_window(title) {
        unsafe {
            clear_hidden_start_styles(h);
            ShowWindow(h, SW_SHOW);
            ShowWindow(h, SW_RESTORE);
            SetForegroundWindow(h);
        }
    }
}

pub fn hide_window(title: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if let Some(h) = find_window(title) {
        unsafe {
            ShowWindow(h, SW_HIDE);
        }
    }
}

const OFFSCREEN: i32 = -32000;

/// Etape 1 du demarrage masque (avant la fin de la 1re image) : eframe 0.31 force l'affichage de la
/// fenetre apres la 1re image. On la rend donc invisible a l'ecran (hors ecran, sans bouton dans la
/// barre des taches, sans prise de focus) pour que cet affichage force ne se voie pas.
pub fn hidden_start_prepare(title: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if let Some(h) = find_window(title) {
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize);
            SetWindowPos(h, std::ptr::null_mut(), OFFSCREEN, OFFSCREEN, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Etape 2 : masque reellement (SW_HIDE, ni reduction ni activation), remet la fenetre au centre
/// de l'ecran et ses styles normaux pour un affichage ulterieur. Sans effet si l'utilisateur a
/// deja demande l'affichage entre-temps.
pub fn hidden_start_finish(title: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if SHOW_REQUESTED.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Some(h) = find_window(title) {
        unsafe {
            ShowWindow(h, SW_HIDE);
            clear_hidden_start_styles(h);
        }
    }
}

unsafe fn clear_hidden_start_styles(h: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
    let mask = (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize;
    if ex & mask != 0 {
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex & !mask);
    }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if GetWindowRect(h, &mut r) != 0 && r.left <= OFFSCREEN / 2 {
        let (w, hh) = (r.right - r.left, r.bottom - r.top);
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        SetWindowPos(h, std::ptr::null_mut(), ((sw - w) / 2).max(0), ((sh - hh) / 2).max(0), 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

/// La fenetre de ce processus est-elle visible ? (verifications manuelles / tests)
pub fn own_window_visible(title: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    find_window(title).map(|h| unsafe { IsWindowVisible(h) != 0 }).unwrap_or(false)
}

/// La fenetre au premier plan appartient-elle a ce processus ?
pub fn own_window_is_foreground() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            return false;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(h, &mut pid);
        pid == std::process::id()
    }
}

// ------------------------------------------------------------------ icone d'un exe

/// Icone (32x32 en general) d'un exe, en RGBA. None si impossible.
pub fn exe_icon_rgba(exe: &Path) -> Option<(Vec<u8>, usize, usize)> {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::Shell::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if !exe.is_file() {
        return None;
    }
    let wide: Vec<u16> = exe.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        // 2.0.3 : icone en 96 px (nette sur une tuile de 48 a 60 px, ecrans haute densite compris) ; sinon 32 px du shell.
        let mut big: windows_sys::Win32::UI::WindowsAndMessaging::HICON = std::ptr::null_mut();
        let n = PrivateExtractIconsW(wide.as_ptr(), 0, 96, 96, &mut big, std::ptr::null_mut(), 1, 0);
        if n >= 1 && n != u32::MAX && !big.is_null() {
            info.hIcon = big;
        } else {
            let r = SHGetFileInfoW(wide.as_ptr(), 0, &mut info, std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
            if r == 0 || info.hIcon.is_null() {
                return None;
            }
        }
        let mut ii: ICONINFO = std::mem::zeroed();
        let mut out = None;
        if GetIconInfo(info.hIcon, &mut ii) != 0 {
            let mut bm: BITMAP = std::mem::zeroed();
            if !ii.hbmColor.is_null() && GetObjectW(ii.hbmColor as _, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut _) != 0 {
                let (w, h) = (bm.bmWidth as usize, bm.bmHeight as usize);
                let mut bi: BITMAPINFO = std::mem::zeroed();
                bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bi.bmiHeader.biWidth = w as i32;
                bi.bmiHeader.biHeight = -(h as i32); // top-down
                bi.bmiHeader.biPlanes = 1;
                bi.bmiHeader.biBitCount = 32;
                bi.bmiHeader.biCompression = BI_RGB;
                let mut buf = vec![0u8; w * h * 4];
                let dc = GetDC(std::ptr::null_mut());
                let ok = GetDIBits(dc, ii.hbmColor, 0, h as u32, buf.as_mut_ptr() as *mut _, &mut bi, DIB_RGB_COLORS);
                ReleaseDC(std::ptr::null_mut(), dc);
                if ok != 0 {
                    let all_transparent = buf.chunks(4).all(|p| p[3] == 0);
                    for p in buf.chunks_mut(4) {
                        p.swap(0, 2); // BGRA -> RGBA
                        if all_transparent {
                            p[3] = 255;
                        }
                    }
                    out = Some((buf, w, h));
                }
            }
            if !ii.hbmColor.is_null() {
                DeleteObject(ii.hbmColor as _);
            }
            if !ii.hbmMask.is_null() {
                DeleteObject(ii.hbmMask as _);
            }
        }
        DestroyIcon(info.hIcon);
        out
    }
}

// ------------------------------------------------------------------ lancement / elevation

/// Lance un programme (ShellExecute « open », sans console) avec des arguments.
pub fn open_exe(path: &Path, args: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (op, f, a) = (w("open"), w(&path.display().to_string()), w(args));
    let dir = path.parent().map(|p| w(&p.display().to_string()));
    let r = unsafe { ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), f.as_ptr(), if args.is_empty() { std::ptr::null() } else { a.as_ptr() }, dir.as_ref().map(|d| d.as_ptr()).unwrap_or(std::ptr::null()), SW_SHOWNORMAL) };
    r as isize > 32
}

#[derive(Debug, PartialEq, Eq)]
pub enum ElevateResult {
    Finished(u32),
    /// L'utilisateur a refuse la fenetre UAC.
    Cancelled,
    Failed(u32),
}

/// Relance CE programme avec des droits eleves (UAC, verbe `runas`) et attend la fin (au plus `timeout_ms`).
pub fn run_elevated_and_wait(args: &str, timeout_ms: u32) -> ElevateResult {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let exe = std::env::current_exe().unwrap_or_default();
    let (verb, file, params) = (w("runas"), w(&exe.display().to_string()), w(args));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            let e = GetLastError();
            return if e == 1223 { ElevateResult::Cancelled } else { ElevateResult::Failed(e) };
        }
        let h = info.hProcess;
        if h.is_null() {
            return ElevateResult::Failed(0);
        }
        let r = if WaitForSingleObject(h, timeout_ms) == WAIT_OBJECT_0 {
            let mut code = 0u32;
            GetExitCodeProcess(h, &mut code);
            ElevateResult::Finished(code)
        } else {
            ElevateResult::Failed(258) // WAIT_TIMEOUT
        };
        CloseHandle(h);
        r
    }
}

/// Valeur de demarrage automatique ancienne (voir `legacy`) ou actuelle dans la cle Run de HKCU.
pub fn run_entry(name: &str) -> Option<String> {
    crate::registry::read_string(Hive::CurrentUser, RUN_KEY, name)
}

pub fn remove_run_entry(name: &str) -> bool {
    delete_value(Hive::CurrentUser, RUN_KEY, name)
}

// ------------------------------------------------------------------ lancement reduit (regles de demarrage)

/// Ligne de commande Windows : programme entre guillemets puis arguments.
pub fn build_cmdline(path: &Path, args: &str) -> String {
    if args.trim().is_empty() {
        format!("\"{}\"", path.display())
    } else {
        format!("\"{}\" {}", path.display(), args)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchHow {
    /// Demande « reduit sans activer » transmise au programme (il peut l'ignorer).
    Minimized,
    /// Lancement ordinaire (option desactivee, ou repli : programme necessitant l'elevation, etc.).
    Normal,
    Failed,
}

/// Lance un programme en DEMANDANT une fenetre reduite et non activee : CreateProcess avec
/// STARTF_USESHOWWINDOW + SW_SHOWMINNOACTIVE. C'est une demande : certains programmes l'ignorent.
/// Repli sur ShellExecute si CreateProcess echoue (p. ex. programme exigeant les droits admin).
pub fn launch_program(path: &Path, args: &str, minimized: bool) -> LaunchHow {
    if !minimized {
        return if open_exe(path, args) { LaunchHow::Normal } else { LaunchHow::Failed };
    }
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let mut cmd = w(&build_cmdline(path, args)); // CreateProcessW peut modifier ce tampon
    let dir = path.parent().map(|p| w(&p.display().to_string()));
    let ok = unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = SW_SHOWMINNOACTIVE as u16;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let r = CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            dir.as_ref().map(|d| d.as_ptr()).unwrap_or(std::ptr::null()),
            &si,
            &mut pi,
        );
        if r != 0 {
            CloseHandle(pi.hProcess);
            CloseHandle(pi.hThread);
        }
        r != 0
    };
    if ok {
        LaunchHow::Minimized
    } else if open_exe(path, args) {
        LaunchHow::Normal
    } else {
        LaunchHow::Failed
    }
}

/// Ouvre un dossier dans l'Explorateur Windows (ShellExecute « open »).
pub fn open_folder(dir: &Path) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (op, d) = (w("open"), w(&dir.display().to_string()));
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), d.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

/// Migration des anciennes entrees Run de CETTE application (`AutoGameSoftwareSync`, `AutoGameSoftwareSync`) vers le nouvel exe sous le nom
/// `AutoGameSoftwareSync` (arguments conserves). Une entree dont la commande ne vise pas notre ancien exe n'est pas touchee.
/// Retourne les lignes de journal.
pub fn migrate_run_entries(new_exe: &Path) -> Vec<String> {
    let mut log = Vec::new();
    if run_entry(RUN_VALUE).is_some() {
        return log;
    }
    for name in crate::legacy::RUN_VALUES {
        let Some(cmd) = run_entry(name) else { continue };
        match crate::startup::rewrite_run_command(&cmd, new_exe) {
            Some(new_cmd) => {
                if write_string(Hive::CurrentUser, RUN_KEY, RUN_VALUE, &new_cmd) {
                    remove_run_entry(name);
                    log.push(format!("[Migration] ancienne entree de demarrage remplacee par '{}' : {}", RUN_VALUE, new_cmd));
                    break;
                }
            }
            None => log.push(format!("[Migration] ancienne entree de demarrage laissee telle quelle (ne vise pas notre ancien programme)")),
        }
    }
    log
}

/// Dossier connu de Windows (SHGetKnownFolderPath : suit les redirections, ex. Documents sous OneDrive).
pub fn known_folder(id: &windows_sys::core::GUID) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p);
        if hr != 0 || p.is_null() {
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        CoTaskMemFree(p as *const _);
        Some(std::path::PathBuf::from(s))
    }
}

/// Passe la fenetre au premier plan ; retourne vrai si Windows l'a accordee (verrou de focus sinon : on ne force rien).
pub fn bring_to_front(title: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    show_window(title);
    std::thread::sleep(std::time::Duration::from_millis(150));
    unsafe { find_window(title).map(|h| GetForegroundWindow() == h).unwrap_or(false) }
}

/// Processus qui ont une VRAIE fenetre (visible, sans proprietaire, pas une fenetre d'outil, avec un titre, au moins
/// 200 x 150) : apprentissage des programmes inconnus (2.0.3). Lecture seule.
pub fn visible_window_pids() -> std::collections::HashSet<u32> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> i32 {
        let set = &mut *(l as *mut std::collections::HashSet<u32>);
        if IsWindowVisible(h) == 0 || !GetWindow(h, GW_OWNER).is_null() {
            return 1;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW != 0 || GetWindowTextLengthW(h) == 0 {
            return 1;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(h, &mut r) == 0 || r.right - r.left < 200 || r.bottom - r.top < 150 {
            return 1;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(h, &mut pid);
        if pid != 0 {
            set.insert(pid);
        }
        1
    }
    let mut set: std::collections::HashSet<u32> = std::collections::HashSet::new();
    unsafe {
        EnumWindows(Some(cb), &mut set as *mut _ as LPARAM);
    }
    set
}

/// Fait clignoter le bouton de la barre des taches (repli sans insister quand le premier plan est refuse).
pub fn flash_taskbar(title: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FlashWindowEx, FLASHWINFO, FLASHW_TIMERNOFG, FLASHW_TRAY};
    if let Some(h) = find_window(title) {
        unsafe {
            let fi = FLASHWINFO { cbSize: std::mem::size_of::<FLASHWINFO>() as u32, hwnd: h, dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG, uCount: 4, dwTimeout: 0 };
            FlashWindowEx(&fi);
        }
    }
}

/// Une application (hors bureau et barre des taches) occupe-t-elle tout l'ecran au premier plan ? (plein ecran / sans bordure)
pub fn foreground_is_fullscreen() -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect};
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            return false;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(h, cls.as_mut_ptr(), 64) as usize;
        let class = String::from_utf16_lossy(&cls[..n]);
        if ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"].contains(&class.as_str()) {
            return false;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(h, &mut r) == 0 {
            return false;
        }
        let mon = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

/// Les animations de l'interface sont-elles activees dans Windows (Parametres > Accessibilite > Effets visuels) ?
pub fn client_animations_enabled() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    unsafe {
        let mut on: i32 = 1;
        if SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on as *mut i32 as *mut _, 0) == 0 {
            return true;
        }
        on != 0
    }
}

// ------------------------------------------------------------------ informations de version d'un exe

/// (ProductName, FileDescription) des informations de version d'un exe (None si absentes ou illisibles).
pub fn exe_version_strings(exe: &Path) -> (Option<String>, Option<String>) {
    let mut v = version_fields(exe, &["ProductName", "FileDescription"]).into_iter();
    (v.next().flatten(), v.next().flatten())
}

/// (CompanyName, ProductName) des informations de version d'un exe (indices de la detection).
pub fn exe_company_product(exe: &Path) -> (Option<String>, Option<String>) {
    let mut v = version_fields(exe, &["CompanyName", "ProductName"]).into_iter();
    (v.next().flatten(), v.next().flatten())
}

/// Champs texte des informations de version d'un exe, dans l'ordre demande (None si absents ou illisibles).
fn version_fields(exe: &Path, names: &[&str]) -> Vec<Option<String>> {
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let none = || names.iter().map(|_| None).collect::<Vec<Option<String>>>();
    if !exe.is_file() {
        return none();
    }
    let wide: Vec<u16> = exe.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
        if size == 0 {
            return none();
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr() as *mut _) == 0 {
            return none();
        }
        let query = |sub: &str| -> Option<(*const u16, u32)> {
            let w: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
            let mut p: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            if VerQueryValueW(buf.as_ptr() as *const _, w.as_ptr(), &mut p, &mut len) == 0 || p.is_null() || len == 0 {
                None
            } else {
                Some((p as *const u16, len))
            }
        };
        let langs: Vec<(u16, u16)> = match query("\\VarFileInfo\\Translation") {
            Some((p, bytes)) => (0..(bytes as usize / 4)).map(|i| (*p.add(i * 2), *p.add(i * 2 + 1))).collect(),
            None => vec![(0x0409, 0x04B0)],
        };
        let get = |name: &str| -> Option<String> {
            for (l, c) in &langs {
                if let Some((p, n)) = query(&format!("\\StringFileInfo\\{:04x}{:04x}\\{}", l, c, name)) {
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n as usize)).trim_end_matches('\0').trim().to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
            None
        };
        names.iter().map(|n| get(n)).collect()
    }
}
