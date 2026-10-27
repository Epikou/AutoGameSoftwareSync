//! Icone de la zone de notification : construction (Windows : menu natif) et mise a jour de son etat.
use super::*;


#[cfg(not(windows))]
pub(super) fn build_tray(shared: &Arc<Shared>, lang: Lang, _accent: [u8; 3], ectx: &egui::Context, detect_on: bool) -> Option<(tray_sys::TrayHandle, tray_sys::DetectHandle)> {
    tray_sys::spawn(shared, lang, ectx, detect_on)
}

#[cfg(windows)]
pub(super) fn build_tray(shared: &Arc<Shared>, lang: Lang, accent: [u8; 3], ectx: &egui::Context, detect_on: bool) -> Option<(tray_sys::TrayHandle, tray_sys::DetectHandle)> {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
    let menu = Menu::new();
    let open = MenuItem::new(tr(lang, K::TrayOpen), true, None);
    // Deux actions EXPLICITES et idempotentes (toujours actives) : pas d etat de menu a tenir a jour, « Reprendre » ne depend de rien.
    let pause = MenuItem::new(tr(lang, K::TrayPause), true, None);
    let resume = MenuItem::new(tr(lang, K::TrayResume), true, None);
    let quit = MenuItem::new(tr(lang, K::TrayQuit), true, None);
    // Detection des jeux : MEME reglage que l interrupteur des Reglages (settings.json = source de verite unique).
    let det = CheckMenuItem::new(tr(lang, if detect_on { K::TrayDetectOn } else { K::TrayDetectOff }), true, detect_on, None);
    menu.append_items(&[&open, &pause, &resume, &det, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let _ = accent;
    let (rgba, w, h) = tray_icon_rgba();
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_TITLE)
        .with_icon(Icon::from_rgba(rgba, w, h).ok()?)
        .build()
        .ok()?;
    let (oid, pid, rid, qid, did) = (open.id().clone(), pause.id().clone(), resume.id().clone(), quit.id().clone(), det.id().clone());
    let (sh, c) = (shared.clone(), ectx.clone());
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        if ev.id == oid {
            winutil::show_window(APP_TITLE);
            c.request_repaint();
        } else if ev.id == pid {
            sh.paused.store(true, Ordering::SeqCst);
            sh.status_dirty.store(true, Ordering::SeqCst);
            c.request_repaint();
        } else if ev.id == rid {
            sh.paused.store(false, Ordering::SeqCst);
            sh.status_dirty.store(true, Ordering::SeqCst);
            c.request_repaint();
        } else if ev.id == did {
            // Bascule depuis la valeur PERSISTEE ; l interface relit le reglage et remet la case a jour a l image suivante.
            if let Ok((_, line)) = crate::settings::toggle_detect(&sh.data_dir, "menu") {
                Logger::new(sh.data_dir.join("logs").join("watch.log"), false).log(&line);
            }
            sh.settings_dirty.store(true, Ordering::SeqCst);
            sh.status_dirty.store(true, Ordering::SeqCst);
            c.request_repaint();
        } else if ev.id == qid {
            sh.quit();
        }
    }));
    let c2 = ectx.clone();
    TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { .. } = ev {
            winutil::show_window(APP_TITLE);
            c2.request_repaint();
        }
    }));
    Some((tray, det))
}

impl App {
    /// Entrees de l'icone de la zone de notification, a l'instant `now_ms`.
    pub(super) fn tray_inputs(&self, now_ms: u64) -> crate::trayicon::TrayInputs {
        let tr = self.shared.tracker.lock().map(|t| (t.conflict(), t.error_recent(now_ms))).unwrap_or((false, false));
        let seen = crate::perf::STREAMING_SEEN_MS.load(Ordering::SeqCst);
        let st = self.shared.state();
        let running = matches!(st, WatchState::Running | WatchState::Other(_));
        crate::trayicon::TrayInputs {
            error: tr.1,
            conflict: tr.0,
            unreachable: self.shared.dest_reach.load(Ordering::SeqCst) == 2 || crate::sharedfolder::tray_attention(self.share_state),
            on_demand: seen > 0 && now_ms.saturating_sub(seen) < crate::trayicon::ON_DEMAND_RECENT_MS,
            syncing: crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) > 0 || self.op_progress.is_some(),
            paused: !running || self.shared.paused.load(Ordering::SeqCst),
        }
    }

    /// Met a jour l'icone et l'infobulle de la zone de notification (uniquement quand l'etat ou l'image change).
    pub(super) fn update_tray(&mut self) {
        self.sync_tray_detect();
        use crate::trayicon::{anim_frame, render, state_of, TrayState};
        let Some(tray) = &self._tray else { return };
        let now = crate::util::now_ms();
        let state = state_of(&self.tray_inputs(now));
        let frame = anim_frame(state, now);
        let upd = self.shared.update_latest.lock().ok().and_then(|l| l.clone());
        let upd_tag = crate::update::banner(upd.as_ref(), crate::update::current_version(), self.model.settings.ignored_update.as_deref(), self.model.settings.update_mode() != crate::settings::UpdateMode::Never).map(|r| r.tag.clone());
        let key = (state, frame, self.model.settings.lang, upd_tag.clone());
        if self.tray_last.as_ref() == Some(&key) {
            return;
        }
        self.tray_last = Some(key);
        let (base, w, h) = tray_icon_rgba();
        {
            use tray_sys::TrayLike;
            tray.set_icon_rgba(render(&base, w, state, frame), w, h);
        }
        let k = match state {
            TrayState::Error => K::TrayStError,
            TrayState::Conflict => K::TrayStConflict,
            TrayState::Unreachable => if self.share_not_configured() { K::TrayStNoShare } else { K::TrayStUnreachable },
            TrayState::OnDemand => K::TrayStOnDemand,
            TrayState::Syncing => K::TrayStSyncing,
            TrayState::Paused => K::TrayStPaused,
            TrayState::Ok => K::TrayStOk,
        };
        let mut text = format!("{} - {}", APP_SHORT, self.t(k));
        if let Some(t) = &upd_tag {
            text.push_str(&self.t(K::UpdTray).replace("{}", t));
        }
        {
            use tray_sys::TrayLike;
            tray.set_tip(text.as_str());
        }
        // Diagnostic (verification manuelle) : l'etat courant est ecrit dans un fichier quand AGSS_TRAY_DEBUG est defini.
        if std::env::var_os("AGSS_TRAY_DEBUG").is_some() {
            let _ = std::fs::write(self.shared.data_dir.join("tray-state.txt"), format!("{:?} frame={} {}", state, frame, text));
        }
    }
}
