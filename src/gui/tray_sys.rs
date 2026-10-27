//! Icone de la zone de notification, par plateforme. Windows : `tray-icon` directement (boucle de messages de la fenetre).
//! Linux (feature `tray`) : l'icone vit dans un thread dedie qui execute la boucle GTK (StatusNotifier / appindicator) ;
//! l'interface lui envoie des commandes. Sans la feature, ou si le bureau n'offre pas de zone de notification, il n'y a
//! pas d'icone : `spawn` renvoie `None` et l'application fonctionne normalement (fermer la fenetre quitte alors l'application).
#![allow(unused_imports, dead_code)]
use super::*;

/// Operations dont l'interface a besoin sur l'icone, identiques sur toutes les plateformes.
pub trait TrayLike {
    fn set_icon_rgba(&self, rgba: Vec<u8>, w: u32, h: u32);
    fn set_tip(&self, tip: &str);
}

/// Case « Detection » du menu de l'icone.
pub trait DetectLike {
    fn set_detect(&self, checked: bool, text: &str);
}

#[cfg(windows)]
pub type TrayHandle = tray_icon::TrayIcon;
#[cfg(windows)]
pub type DetectHandle = tray_icon::menu::CheckMenuItem;

#[cfg(windows)]
impl TrayLike for tray_icon::TrayIcon {
    fn set_icon_rgba(&self, rgba: Vec<u8>, w: u32, h: u32) {
        if let Ok(icon) = tray_icon::Icon::from_rgba(rgba, w, h) {
            let _ = self.set_icon(Some(icon));
        }
    }
    fn set_tip(&self, tip: &str) {
        let _ = self.set_tooltip(Some(tip));
    }
}

#[cfg(windows)]
impl DetectLike for tray_icon::menu::CheckMenuItem {
    fn set_detect(&self, checked: bool, text: &str) {
        self.set_checked(checked);
        self.set_text(text);
    }
}

// ------------------------------------------------------------------ Linux

#[cfg(not(windows))]
pub enum Cmd {
    Icon(Vec<u8>, u32, u32),
    Tip(String),
    Detect(bool, String),
}

#[cfg(not(windows))]
pub struct TrayHandle {
    tx: Sender<Cmd>,
}

#[cfg(not(windows))]
pub struct DetectHandle {
    tx: Sender<Cmd>,
}

#[cfg(not(windows))]
impl TrayLike for TrayHandle {
    fn set_icon_rgba(&self, rgba: Vec<u8>, w: u32, h: u32) {
        let _ = self.tx.send(Cmd::Icon(rgba, w, h));
    }
    fn set_tip(&self, tip: &str) {
        let _ = self.tx.send(Cmd::Tip(tip.to_string()));
    }
}

#[cfg(not(windows))]
impl DetectLike for DetectHandle {
    fn set_detect(&self, checked: bool, text: &str) {
        let _ = self.tx.send(Cmd::Detect(checked, text.to_string()));
    }
}

/// Sans la feature `tray` : pas d'icone.
#[cfg(all(not(windows), not(has_tray)))]
pub fn spawn(_shared: &Arc<Shared>, _lang: Lang, _ectx: &egui::Context, _detect_on: bool) -> Option<(TrayHandle, DetectHandle)> {
    None
}

#[cfg(all(not(windows), has_tray))]
pub fn spawn(shared: &Arc<Shared>, lang: Lang, ectx: &egui::Context, detect_on: bool) -> Option<(TrayHandle, DetectHandle)> {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
    let (tx, rx) = channel::<Cmd>();
    let (ready_tx, ready_rx) = channel::<bool>();
    let (sh, c) = (shared.clone(), ectx.clone());
    let spawned = std::thread::Builder::new().name("agss-tray".into()).spawn(move || {
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if gtk::init().is_err() {
                return None;
            }
            let menu = Menu::new();
            let open = MenuItem::new(tr(lang, K::TrayOpen), true, None);
            let pause = MenuItem::new(tr(lang, K::TrayPause), true, None);
            let resume = MenuItem::new(tr(lang, K::TrayResume), true, None);
            let quit = MenuItem::new(tr(lang, K::TrayQuit), true, None);
            let det = CheckMenuItem::new(tr(lang, if detect_on { K::TrayDetectOn } else { K::TrayDetectOff }), true, detect_on, None);
            menu.append_items(&[&open, &pause, &resume, &det, &PredefinedMenuItem::separator(), &quit]).ok()?;
            let (rgba, w, h) = super::tray_icon_rgba();
            let tray = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(APP_TITLE).with_icon(Icon::from_rgba(rgba, w, h).ok()?).build().ok()?;
            let (oid, pid, rid, qid, did) = (open.id().clone(), pause.id().clone(), resume.id().clone(), quit.id().clone(), det.id().clone());
            MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
                if ev.id == oid {
                    winutil::show_window(APP_TITLE);
                } else if ev.id == pid {
                    sh.paused.store(true, Ordering::SeqCst);
                    sh.status_dirty.store(true, Ordering::SeqCst);
                } else if ev.id == rid {
                    sh.paused.store(false, Ordering::SeqCst);
                    sh.status_dirty.store(true, Ordering::SeqCst);
                } else if ev.id == did {
                    if let Ok((_, line)) = crate::settings::toggle_detect(&sh.data_dir, "menu") {
                        Logger::new(sh.data_dir.join("logs").join("watch.log"), false).log(&line);
                    }
                    sh.settings_dirty.store(true, Ordering::SeqCst);
                    sh.status_dirty.store(true, Ordering::SeqCst);
                } else if ev.id == qid {
                    sh.quit();
                }
                c.request_repaint();
            }));
            TrayIconEvent::set_event_handler(Some(|_ev: TrayIconEvent| {}));
            Some((tray, det))
        }));
        let (tray, det) = match built {
            Ok(Some(x)) => x,
            _ => {
                let _ = ready_tx.send(false);
                return;
            }
        };
        let _ = ready_tx.send(true);
        gtk::glib::timeout_add_local(Duration::from_millis(150), move || {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    Cmd::Icon(rgba, w, h) => {
                        if let Ok(i) = Icon::from_rgba(rgba, w, h) {
                            let _ = tray.set_icon(Some(i));
                        }
                    }
                    Cmd::Tip(t) => {
                        let _ = tray.set_tooltip(Some(t.as_str()));
                    }
                    Cmd::Detect(on, text) => {
                        det.set_checked(on);
                        det.set_text(text);
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });
        gtk::main();
    });
    if spawned.is_err() {
        return None;
    }
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(true) => Some((TrayHandle { tx: tx.clone() }, DetectHandle { tx })),
        _ => None,
    }
}
