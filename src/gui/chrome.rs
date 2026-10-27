//! Cadre de la fenetre : sonde du dossier partage, grand format automatique, barre d'etat du bas.
use super::*;

impl App {
    /// Sonde (toutes les ~8 s, en arriere-plan) : le dossier partage est-il joignable ?
    pub(super) fn probe_dest(&mut self, ctx: &egui::Context) {
        if self.dest_probe_at.elapsed() < Duration::from_secs(8) {
            return;
        }
        self.dest_probe_at = Instant::now();
        let d = self.model.dest();
        let (tx, c, sh) = (self.tx.clone(), ctx.clone(), self.shared.clone());
        std::thread::spawn(move || {
            let v = d.map(|d| d.is_dir());
            sh.dest_reach.store(match v { None => 0, Some(true) => 1, Some(false) => 2 }, Ordering::SeqCst);
            let _ = tx.send(Msg::DestOk(v));
            c.request_repaint();
        });
    }

    /// Mode manette : la fenetre passe maximisee (comme le bouton agrandir) et revient a sa taille d'origine ensuite.
    pub(super) fn update_window_mode(&mut self, ctx: &egui::Context) {
        let want = uilogic::want_maximized(self.model.settings.big_choice(), self.pad_mode);
        // Diagnostic (captures) : taille imposee par AGSS_UI_SIZE, jamais maximisee (redemandee apres l'ouverture : a la
        // creation, Windows limite la fenetre a l'ecran).
        if let Some((w, h)) = std::env::var("AGSS_UI_SIZE").ok().and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))) {
            if self.frame_no == 3 || self.frame_no == 12 {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            }
            return;
        }
        if want == self.max_state.prev_want {
            return;
        }
        let is_max = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
        let visible = winutil::own_window_visible(APP_TITLE);
        match uilogic::max_step(&mut self.max_state, want, is_max, visible) {
            uilogic::MaxAction::Maximize => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true)),
            uilogic::MaxAction::Restore => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false)),
            uilogic::MaxAction::None => {}
        }
    }

    /// Barre du bas : surveillance a gauche, etat de la manette au centre, cafe a droite.
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let tiny = w < uilogic::TINY_W;
        let h = ui.spacing().interact_size.y.max(28.0);
        let (full, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let pads = self.pad.status();
        let phase = self.pad.phase();
        let presence = self.pad.presence();
        if let Some(p) = pads.first() {
            self.pad_last_name = Some(p.info.display());
        }
        let released_name = presence.first().map(|p| p.display()).or_else(|| self.pad_last_name.clone());
        let bar = crate::padctl::bar_state(phase, released_name.as_deref());
        // Detectee (mode Me demander, pas encore acceptee) : famille affichee mais grisee.
        let kind = if pads.is_empty() && matches!(phase, crate::padctl::Phase::Ask { .. }) && !presence.is_empty() {
            uilogic::pad_kind(&presence.iter().map(|i| gamepad::PadStatus { info: i.clone(), readable: true }).collect::<Vec<_>>())
        } else {
            uilogic::pad_kind(&pads)
        };
        // droite : cafe
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(full).layout(egui::Layout::right_to_left(egui::Align::Center)));
        self.coffee_button_sized(&mut right, w < 560.0, 0.82);
        // gauche : surveillance
        let st = self.shared.state();
        let paused = self.shared.paused.load(Ordering::SeqCst);
        let no_share = self.share_not_configured();
        // Mode simple : vocabulaire 2.0 (« Protection active », « coffre ») ; mode expert : termes techniques d'origine.
        let ex = self.model.settings.expert_mode;
        let k = |simple: K, expert: K| if ex { expert } else { simple };
        let (txt, col) = match (&st, paused) {
            _ if no_share => (self.t(k(K::VNoVault, K::WatchNoShare)), Color32::from_rgb(230, 170, 40)),
            (WatchState::Running, false) => (self.t(k(K::VProtectionOn, K::WatchRunning)), Color32::from_rgb(60, 180, 100)),
            (WatchState::Running, true) => (self.t(k(K::VProtectionPaused, K::WatchPaused)), Color32::from_rgb(230, 170, 40)),
            (WatchState::Stopped, _) => (self.t(k(K::VProtectionStopped, K::WatchStopped)), Color32::GRAY),
            (WatchState::Other(_), _) => (self.t(k(K::VProtectionOn, K::WatchOther)), Color32::from_rgb(230, 170, 40)),
            (WatchState::Error(_), _) => (self.t(k(K::VProtectionStopped, K::WatchStopped)), Color32::from_rgb(220, 80, 80)),
        };
        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(full).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if tiny {
            let (r, resp) = left.allocate_exact_size(egui::vec2(h, h), egui::Sense::hover());
            paint_dot(left.painter(), r, col);
            resp.on_hover_text(txt);
        } else {
            let r = left.label(RichText::new(txt).color(col));
            self.tip(r, K::TipWatchState, false);
        }
        // centre : etat de la manette (4 libelles) ; mode de connexion et aide Steam en infobulle
        let (label, active) = match kind {
            uilogic::PadKind::DualSense => (self.t(K::PadDualSense), true),
            uilogic::PadKind::Xbox => (self.t(K::PadXbox), true),
            uilogic::PadKind::Other => (self.t(K::PadOther), true),
            uilogic::PadKind::None => (self.t(K::PadNone), false),
        };
        #[cfg(windows)]
        let (label, active) = match &bar {
            crate::padctl::BarState::Disabled => (self.t(K::PadBarOff), false),
            crate::padctl::BarState::Released(_) => (self.t(K::PadBarReleased), false),
            crate::padctl::BarState::Normal => (label, active && !pads.is_empty()),
        };
        // Linux / SteamOS : Steam Input gere deja les manettes (clavier + souris), pas de lecture directe.
        #[cfg(not(windows))]
        let (label, active) = {
            let _ = (label, active, &pads);
            (self.t(K::PadBarSteam), true)
        };
        let font = egui::TextStyle::Body.resolve(ui.style());
        let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x;
        let cw = if tiny { h } else { h + tw + 6.0 };
        let crect = egui::Rect::from_center_size(full.center(), egui::vec2(cw, h));
        let resp = ui.interact(crect, egui::Id::new("pad_state"), egui::Sense::hover());
        let colr = if active { rgb(self.palette.accent) } else { Color32::GRAY };
        paint_pad(ui.painter(), egui::Rect::from_min_size(crect.min, egui::vec2(h, h)).shrink(3.0), colr, rgb(self.palette.panel_bg));
        if !tiny {
            ui.painter().text(egui::pos2(crect.min.x + h + 2.0, crect.center().y), egui::Align2::LEFT_CENTER, label, font, if active { rgb(self.palette.text) } else { Color32::GRAY });
        }
        #[cfg(not(windows))]
        let mut tip = self.t(K::PadSteamTip).to_string();
        #[cfg(windows)]
        let mut tip = self.t(K::TipPadState).to_string();
        if let crate::padctl::BarState::Released(n) = &bar {
            tip = self.t(K::PadReleasedTip).replace("{}", n);
        }
        for p in &pads {
            tip.push('\n');
            tip.push_str(&p.info.display());
            if !p.readable {
                tip.push_str(" - ");
                tip.push_str(self.t(K::PadUnreadable));
            }
        }
        if phase == crate::padctl::Phase::Active && gamepad::needs_steam_hint(&pads) {
            tip.push('\n');
            tip.push_str(self.t(K::SetPadHelp));
        }
        resp.on_hover_text(tip);
        self.last_pad_label = Some((kind, active));
    }
}
