//! Notifications Windows envoyees par l'application elle-meme (plus aucun PowerShell).
//!
//! Choix : `Shell_NotifyIconW` avec `NIF_INFO` (« balloon »), que Windows 10/11 affiche comme une
//! notification (toast) systeme. Un toast WinRT demanderait une identite d'application (AUMID /
//! raccourci) pour un exe non package et une dependance lourde ; le balloon n'exige rien de tel.
//! Une icone de notification TEMPORAIRE (fenetre message-only dans un thread dedie) est ajoutee pour
//! chaque notification puis retiree apres quelques secondes d'inactivite.
//! L'envoi est abstrait (`Notifier`) pour les tests ; le texte est pur (`notification_text`).
use crate::i18n::{tr, Lang, K};
use crate::watch::{EventKind, WatchEvent};
use std::path::Path;
#[cfg(windows)]
use std::sync::mpsc::{channel, Sender};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

pub trait Notifier {
    fn notify(&self, title: &str, body: &str);
}

/// Adresse a ouvrir au clic sur la derniere notification (« Quoi de neuf ? » apres une mise a jour), valable 30 s.
static CLICK_URL: std::sync::Mutex<Option<(String, std::time::Instant)>> = std::sync::Mutex::new(None);

fn clear_click_url() {
    if let Ok(mut g) = CLICK_URL.lock() {
        *g = None;
    }
}

/// Notification dont le clic ouvre `url` dans le navigateur (au lieu d afficher la fenetre).
pub fn notify_with_url(n: &dyn Notifier, title: &str, body: &str, url: &str) {
    n.notify(title, body); // efface l adresse precedente
    if let Ok(mut g) = CLICK_URL.lock() {
        *g = Some((url.to_string(), std::time::Instant::now()));
    }
}

/// Adresse liee a la derniere notification, si elle date de moins de 30 s (consommee).
pub fn take_click_url() -> Option<String> {
    let v = CLICK_URL.lock().ok().and_then(|mut g| g.take());
    v.filter(|(_, at)| at.elapsed() < std::time::Duration::from_secs(30)).map(|(u, _)| u)
}

/// Texte (titre, corps) d'une notification, dans la langue choisie.
pub fn notification_text(ev: &WatchEvent, lang: Lang) -> (String, String) {
    match ev.kind {
        EventKind::BackedUp => (tr(lang, K::NotifBackedUp).to_string(), ev.game.clone()),
        EventKind::Restored => (tr(lang, K::NotifRestored).to_string(), ev.game.clone()),
        EventKind::Conflict => (tr(lang, K::NotifConflict).to_string(), ev.game.clone()),
        EventKind::ConflictResolved => (tr(lang, K::NotifConflictResolved).to_string(), ev.game.clone()),
        EventKind::Error => (tr(lang, K::NotifError).to_string(), format!("{} : {}", ev.game, ev.detail)),
    }
}

/// Envoie la notification d'un evenement de surveillance si les reglages (global + par jeu) l'autorisent.
/// Les reglages sont relus a chaque evenement. Renvoie true si une notification a ete emise.
pub fn deliver(n: &dyn Notifier, data_dir: &Path, lang: Lang, ev: &WatchEvent) -> bool {
    let s = crate::settings::Settings::load_from(data_dir).unwrap_or_default();
    // « Conflit resolu » a son propre reglage (actif par defaut meme si les autres notifications sont coupees).
    let allowed = if ev.kind == EventKind::ConflictResolved { s.notify_conflict_resolved } else { s.notify_for(&ev.game) };
    if !allowed {
        return false;
    }
    // Nom AFFICHE du jeu (More > renommer) dans la notification.
    let mut ev2 = WatchEvent { game: ev.game.clone(), kind: ev.kind, detail: ev.detail.clone() };
    if let Some(g) = crate::config::Config::load(&crate::Ctx::new(data_dir.to_path_buf())).ok().and_then(|c| c.find(&ev.game).map(|g| g.shown().to_string())) {
        ev2.game = g;
    }
    let (title, body) = notification_text(&ev2, lang);
    n.notify(&title, &body);
    true
}

// ------------------------------------------------------------------ moins de notifications (2.0.3)

/// Ce que devient un evenement de la surveillance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Notification tout de suite.
    Now,
    /// Un programme suivi ou detecte tourne : gardee, puis regroupee en UNE notification a sa fermeture.
    Hold,
    /// Aucune notification (la tuile de l'accueil suffit).
    Silent,
}

/// Politique des notifications. `has_program` : l'element a un programme (sa copie suit la fermeture du programme) ;
/// `busy` : un programme suivi ou detecte tourne en ce moment. Pure.
///
/// - copie faite : notifiee seulement pour un element a programme, a sa fermeture (c'est LA notification de fin
///   d'utilisation) ; si un AUTRE programme suivi tourne encore, elle est gardee et regroupee (« A jour : ... ») ; les
///   copies de dossiers, de reglages ou planifiees sont silencieuses ;
/// - version recue, conflit, conflit regle, probleme : jamais pendant l'utilisation (gardes puis regroupes).
pub fn route(kind: EventKind, has_program: bool, busy: bool) -> Route {
    match kind {
        EventKind::BackedUp if !has_program => Route::Silent,
        _ if busy => Route::Hold,
        _ => Route::Now,
    }
}

/// File des notifications gardees et problemes deja signales (un meme probleme n'est notifie qu'une fois par session).
#[derive(Default, Debug)]
pub struct Gate {
    held: Vec<WatchEvent>,
    errors: std::collections::HashSet<String>,
}

impl Gate {
    /// Evenement a notifier tout de suite (None : garde, silencieux ou deja signale).
    pub fn offer(&mut self, ev: &WatchEvent, has_program: bool, busy: bool) -> Option<WatchEvent> {
        if ev.kind == EventKind::Error && !self.errors.insert(format!("{}\u{1}{}", ev.game, ev.detail)) {
            return None;
        }
        match route(ev.kind, has_program, busy) {
            Route::Now => Some(ev.clone()),
            Route::Silent => None,
            Route::Hold => {
                // « Conflit regle » remplace « version recue » du meme element (meme fait, un seul message).
                if ev.kind == EventKind::ConflictResolved {
                    self.held.retain(|h| !(h.game == ev.game && h.kind == EventKind::Restored));
                }
                let dup = self.held.iter().any(|h| h.game == ev.game && (h.kind == ev.kind || ev.kind == EventKind::Restored && h.kind == EventKind::ConflictResolved));
                if !dup {
                    self.held.push(ev.clone());
                }
                None
            }
        }
    }

    /// Plus rien ne tourne : les evenements gardes, a regrouper en une seule notification.
    pub fn flush(&mut self, busy: bool) -> Vec<WatchEvent> {
        if busy {
            return vec![];
        }
        std::mem::take(&mut self.held)
    }

    pub fn held(&self) -> usize {
        self.held.len()
    }
}

/// Texte de LA notification qui regroupe les evenements gardes : un seul evenement = son texte habituel ; sinon un titre et
/// une ligne par sorte (mis a jour, a verifier, probleme), noms affiches separes par des virgules (3 au plus, puis « +N »).
pub fn grouped_text(held: &[WatchEvent], lang: Lang) -> Option<(String, String)> {
    if held.is_empty() {
        return None;
    }
    if held.len() == 1 {
        return Some(notification_text(&held[0], lang));
    }
    let names = |f: &dyn Fn(EventKind) -> bool| -> Option<String> {
        let v: Vec<&str> = held.iter().filter(|e| f(e.kind)).map(|e| e.game.as_str()).collect();
        if v.is_empty() {
            return None;
        }
        let mut s = v.iter().take(3).copied().collect::<Vec<_>>().join(", ");
        if v.len() > 3 {
            s.push_str(&format!(" +{}", v.len() - 3));
        }
        Some(s)
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(s) = names(&|k| matches!(k, EventKind::Restored | EventKind::ConflictResolved | EventKind::BackedUp)) {
        lines.push(tr(lang, K::VNotifHeldUpdated).replace("{}", &s));
    }
    if let Some(s) = names(&|k| k == EventKind::Conflict) {
        lines.push(tr(lang, K::VNotifHeldConflict).replace("{}", &s));
    }
    if let Some(s) = names(&|k| k == EventKind::Error) {
        lines.push(tr(lang, K::VNotifHeldError).replace("{}", &s));
    }
    Some((tr(lang, K::VNotifHeldTitle).to_string(), lines.join("\n")))
}

/// Envoi avec la politique 2.0.3 : reglages relus (global + par element), nom affiche, puis `Gate`. Vrai si une
/// notification est partie tout de suite.
pub fn deliver_gated(n: &dyn Notifier, data_dir: &Path, lang: Lang, ev: &WatchEvent, busy: bool, gate: &mut Gate) -> bool {
    let s = crate::settings::Settings::load_from(data_dir).unwrap_or_default();
    let allowed = if ev.kind == EventKind::ConflictResolved { s.notify_conflict_resolved } else { s.notify_for(&ev.game) };
    if !allowed {
        return false;
    }
    let cfg = crate::config::Config::load(&crate::Ctx::new(data_dir.to_path_buf())).ok();
    let g = cfg.as_ref().and_then(|c| c.find(&ev.game));
    let has_program = g.map(|g| !g.exe.as_deref().map(str::trim).unwrap_or("").is_empty()).unwrap_or(false);
    let shown = WatchEvent { game: g.map(|g| g.shown().to_string()).unwrap_or_else(|| ev.game.clone()), kind: ev.kind, detail: ev.detail.clone() };
    match gate.offer(&shown, has_program, busy) {
        Some(e) => {
            let (title, body) = notification_text(&e, lang);
            n.notify(&title, &body);
            true
        }
        None => false,
    }
}

/// Fin d'utilisation : UNE notification pour tout ce qui a ete garde. Vrai si elle est partie.
pub fn flush_gated(n: &dyn Notifier, lang: Lang, busy: bool, gate: &mut Gate) -> bool {
    let held = gate.flush(busy);
    match grouped_text(&held, lang) {
        Some((t, b)) => {
            n.notify(&t, &b);
            true
        }
        None => false,
    }
}

#[cfg(windows)]
mod imp {
use super::*;
/// Implementation reelle (Windows).
pub struct WinNotifier;

/// Dernier resultat de Shell_NotifyIconW (0 = jamais appele, 1 = accepte par Windows, -1 = refuse) : diagnostic.
pub static LAST_RESULT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// 1 si l icone de l application (ressource de l exe) a ete chargee et jointe a la notification, -1 sinon (icone generique), 0 jamais appele.
pub static ICON_RESULT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

static TX: OnceLock<Mutex<Sender<(String, String)>>> = OnceLock::new();

impl Notifier for WinNotifier {
    fn notify(&self, title: &str, body: &str) {
        super::clear_click_url();
        let tx = TX.get_or_init(|| {
            let (tx, rx) = channel::<(String, String)>();
            let _ = std::thread::Builder::new().name("agss-notify".into()).spawn(move || worker(rx));
            Mutex::new(tx)
        });
        if let Ok(t) = tx.lock() {
            let _ = t.send((title.to_string(), body.to_string()));
        }
    }
}

fn wide_buf<const N: usize>(s: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (i, c) in s.encode_utf16().take(N - 1).enumerate() {
        out[i] = c;
    }
    out
}

fn worker(rx: std::sync::mpsc::Receiver<(String, String)>) {
    use windows_sys::Win32::UI::Shell::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let class: Vec<u16> = "AutoGameSoftwareSyncNotifyClass".encode_utf16().chain(std::iter::once(0)).collect();
        let name: Vec<u16> = "AutoGameSoftwareSyncNotify".encode_utf16().chain(std::iter::once(0)).collect();
        let hinst0 = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(notify_wndproc);
        wc.hInstance = hinst0;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        // Fenetre message-only : invisible, appartient a CE thread (qui reste vivant).
        let hwnd = CreateWindowExW(0, class.as_ptr(), name.as_ptr(), 0, 0, 0, 0, 0, HWND_MESSAGE, std::ptr::null_mut(), hinst0, std::ptr::null());
        if hwnd.is_null() {
            return;
        }
        let mut added = false;
        let mut idle_ms = 0u64;
        loop {
            // Pompe les messages de la fenetre (aucun traitement particulier).
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok((title, body)) => {
                    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                    nid.hWnd = hwnd;
                    nid.uID = 1;
                    nid.uFlags = NIF_ICON | NIF_TIP | NIF_INFO | NIF_MESSAGE;
                    nid.uCallbackMessage = CALLBACK_MSG;
                    // Icone de l application (ressource n 1 de l exe) : petite pour la zone de notification, grande pour la bulle.
                    let hinst = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
                    let small = LoadImageW(hinst, 1usize as *const u16, IMAGE_ICON, GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_DEFAULTCOLOR) as HICON;
                    let large = LoadImageW(hinst, 1usize as *const u16, IMAGE_ICON, GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON), LR_DEFAULTCOLOR) as HICON;
                    let have_icon = !small.is_null() && !large.is_null();
                    nid.hIcon = if !small.is_null() { small } else { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
                    nid.hBalloonIcon = large;
                    ICON_RESULT.store(if have_icon { 1 } else { -1 }, std::sync::atomic::Ordering::SeqCst);
                    nid.szTip = wide_buf::<128>("AutoGameSoftwareSync");
                    nid.szInfoTitle = wide_buf::<64>(&title);
                    nid.szInfo = wide_buf::<256>(&body);
                    nid.dwInfoFlags = if have_icon { NIIF_USER | NIIF_LARGE_ICON } else { NIIF_INFO };
                    if !added {
                        added = Shell_NotifyIconW(NIM_ADD, &nid) != 0;
                    } else if Shell_NotifyIconW(NIM_MODIFY, &nid) == 0 {
                        added = Shell_NotifyIconW(NIM_ADD, &nid) != 0;
                    }
                    LAST_RESULT.store(if added { 1 } else { -1 }, std::sync::atomic::Ordering::SeqCst);
                    idle_ms = 0;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    idle_ms += 100;
                    // Icone retiree apres 15 s sans notification (le toast reste dans le centre de notifications).
                    if added && idle_ms >= 15_000 {
                        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                        nid.hWnd = hwnd;
                        nid.uID = 1;
                        Shell_NotifyIconW(NIM_DELETE, &nid);
                        added = false;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

// ------------------------------------------------------------------ clic sur la bulle

/// Action au clic sur une bulle (l'application la fixe au demarrage : ouvrir la fenetre sur la proposition).
static ON_CLICK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn set_click_handler(f: impl Fn() + Send + Sync + 'static) {
    let _ = ON_CLICK.set(Box::new(f));
}

/// Message de rappel de l'icone de notification (WM_APP + 1) et evenement « clic sur la bulle ».
const CALLBACK_MSG: u32 = 0x8000 + 1;
const NIN_BALLOONUSERCLICK_EV: u32 = 0x0405;

unsafe extern "system" fn notify_wndproc(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    if msg == CALLBACK_MSG && (lparam as u32 & 0xFFFF) == NIN_BALLOONUSERCLICK_EV {
        if let Some(f) = ON_CLICK.get() {
            f();
        }
        return 0;
    }
    windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
}
}
#[cfg(windows)]
pub use imp::*;

// ------------------------------------------------------------------ Linux : notifications D-Bus (freedesktop)

#[cfg(not(windows))]
mod imp {
    use super::Notifier;
    use std::sync::OnceLock;

    /// Implementation reelle (Linux) : service de notifications du bureau, via D-Bus (Rust pur).
    pub struct WinNotifier;

    /// Dernier resultat d'envoi (0 = jamais, 1 = accepte, -1 = refuse) : diagnostic.
    pub static LAST_RESULT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    /// Sans objet sous Linux (l'icone est celle du fichier `.desktop`) ; garde pour l'interface commune.
    pub static ICON_RESULT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

    static ON_CLICK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    static ON_FAIL: OnceLock<Box<dyn Fn(&str) + Send + Sync>> = OnceLock::new();
    static OPEN_LABEL: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    /// Un echec a deja ete signale (une seule ligne de journal par session).
    static FAIL_SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// Action declenchee quand l'utilisateur clique sur une notification.
    pub fn set_click_handler(f: impl Fn() + Send + Sync + 'static) {
        let _ = ON_CLICK.set(Box::new(f));
    }

    /// Journal d'un echec d'envoi (aucun service de notifications : mode jeu de SteamOS, session minimale...).
    pub fn set_failure_handler(f: impl Fn(&str) + Send + Sync + 'static) {
        let _ = ON_FAIL.set(Box::new(f));
    }

    /// Libelle du bouton « Ouvrir » (langue de l'interface).
    pub fn set_open_label(s: &str) {
        if let Ok(mut g) = OPEN_LABEL.lock() {
            if *g != s {
                *g = s.to_string();
            }
        }
    }

    impl Notifier for WinNotifier {
        fn notify(&self, title: &str, body: &str) {
            super::clear_click_url();
            let (t, b) = (title.to_string(), body.to_string());
            let open = OPEN_LABEL.lock().map(|g| g.clone()).unwrap_or_default();
            let open = if open.is_empty() { "Open".to_string() } else { open };
            let _ = std::thread::Builder::new().name("agss-notify".into()).spawn(move || {
                // Icone et regroupement : raccourci AutoGameSoftwareSync.desktop installe par install-linux.sh.
                // Clic sur la notification (action « default ») : la fenetre s'ouvre, comme la bulle Windows.
                let r = notify_rust::Notification::new()
                    .appname("AutoGameSoftwareSync")
                    .summary(&t)
                    .body(&b)
                    .icon("AutoGameSoftwareSync")
                    .hint(notify_rust::Hint::DesktopEntry("AutoGameSoftwareSync".into()))
                    .action("default", &open)
                    .timeout(8000)
                    .show();
                match r {
                    Ok(h) => {
                        LAST_RESULT.store(1, std::sync::atomic::Ordering::SeqCst);
                        h.wait_for_action(|a| {
                            if a == "default" {
                                if let Some(f) = ON_CLICK.get() {
                                    f();
                                }
                            }
                        });
                    }
                    Err(e) => {
                        LAST_RESULT.store(-1, std::sync::atomic::Ordering::SeqCst);
                        if !FAIL_SAID.swap(true, std::sync::atomic::Ordering::SeqCst) {
                            if let Some(f) = ON_FAIL.get() {
                                f(&format!("[Notifications] aucun service de notifications du bureau ({}) : les notifications sont ignorees, l'appli continue normalement", e));
                            }
                        }
                    }
                }
            });
        }
    }
}
#[cfg(not(windows))]
pub use imp::*;
