//! Fenetres modales : question « mode manette ? », confirmation, rappel de soutien.
use super::*;

impl App {
    /// Petite fenetre modale non agressive : Oui / Non + « Ne plus me le rappeler ». Clavier et souris ;
    /// la manette n'est PAS ouverte tant que l'utilisateur n'a pas accepte.
    pub(super) fn pad_prompt_modal(&mut self, ctx: &egui::Context) {
        let Some(info) = self.pad_prompt.clone() else { return };
        let mut answer: Option<bool> = None;
        let mut remember = self.pad_prompt_remember;
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let modal = egui::Modal::new(egui::Id::new("pad_prompt")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(self.t(K::PadAskTitle));
            ui.add_space(6.0);
            ui.label(self.t(K::PadAskText));
            ui.label(RichText::new(info.display()).small().weak());
            ui.add_space(8.0);
            self.toggle(ui, &mut remember, self.t(K::PadAskRemember), K::TipPadAskRemember);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let yes = ui.button(RichText::new(self.t(K::OptYes)).strong());
                if self.deco(ui, yes, Some(K::TipPadAskYes)).clicked() {
                    answer = Some(true);
                }
                let no = ui.button(self.t(K::OptNo));
                if self.deco(ui, no, Some(K::TipPadAskNo)).clicked() {
                    answer = Some(false);
                }
            });
        });
        self.pad_prompt_remember = remember;
        if answer.is_none() && (esc || modal.should_close()) {
            answer = Some(false); // Echap = Non, sans memoriser
            remember = false;
        }
        if let Some(yes) = answer {
            let (mode, session_yes) = crate::padctl::apply_answer(yes, remember);
            self.pad_prompt = None;
            self.pad.shared.set_session_yes(session_yes);
            if mode != self.model.settings.pad_mode {
                self.model.settings.pad_mode = mode;
                let _ = self.model.save_settings();
            }
            self.pad.shared.set_mode(mode);
            self.pad_log(&format!("[Manette] reponse : {}{}", if yes { "oui" } else { "non" }, if remember { " (choix memorise)" } else { "" }));
        }
    }

    // ------------------------------------------------------------ dialogue de confirmation

    pub(super) fn confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(c) = self.model.confirm.clone() else { return };
        let mut msg = c.message(self.model.settings.lang);
        if let Some(orig) = c.game_name() {
            if let Some(g) = self.model.game(orig) {
                if g.shown() != orig && !matches!(c, Confirm::Rename { .. }) {
                    msg = msg.replace(orig, g.shown());
                }
            }
        }
        let mut confirmed = false;
        let mut cancelled = false;
        self.focus.set_modal_scope(true);
        let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(self.t(K::ConfTitle));
            ui.add_space(8.0);
            ui.label(msg);
            if matches!(c, Confirm::Restore { .. } | Confirm::Resolve { keep_mine: false, .. }) {
                if let Some(p) = &self.preview {
                    self.preview_ui(ui, p);
                }
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                // Le bouton "Annuler" a le focus par defaut (securite, et navigation manette).
                let cancel = ui.button(self.t(K::BtnCancel));
                if !self.pad_mode && !ui.memory(|m| m.has_focus(cancel.id)) && ui.memory(|m| m.focused().is_none()) {
                    cancel.request_focus();
                }
                self.focus.set_default(cancel.id); // mode manette : « Annuler » selectionne par defaut
                if self.deco(ui, cancel, Some(K::TipCancel)).clicked() {
                    cancelled = true;
                }
                let ok = ui.button(RichText::new(self.t(K::BtnConfirm)).strong());
                if self.deco(ui, ok, Some(K::TipConfirm)).clicked() {
                    confirmed = true;
                }
            });
        });
        self.focus.set_modal_scope(false);
        if modal.should_close() {
            cancelled = true;
        }
        if confirmed || cancelled {
            self.preview = None;
        }
        if confirmed {
            self.model.confirm = None;
            // « Retirer de la liste » : la fiche ouverte du jeu se ferme aussi (aucun etat orphelin).
            if let Confirm::Remove { game } = &c {
                if self.model.selected.as_deref() == Some(game.as_str()) {
                    self.model.selected = None;
                }
                self.history = None;
                self.history_for = None;
                self.link_open = false;
            }
            self.run_confirmed(c, ctx);
        } else if cancelled {
            self.model.cancel();
        }
    }
}

impl App {
    /// Compte les jeux AJOUTES (compteur persistant du rappel de soutien).
    pub(super) fn update_support_counter(&mut self) {
        let n = self.model.config.games.len();
        let added = crate::support::newly_added(self.games_seen, n);
        self.games_seen = n;
        if added > 0 {
            self.model.settings.support_added = self.model.settings.support_added.saturating_add(added);
            let _ = self.model.save_settings();
        }
    }

    /// Rappel de soutien (popup drole, en douceur, jamais pendant le tutoriel / une operation / fenetre cachee).
    pub(super) fn support_popup(&mut self, ctx: &egui::Context) {
        use crate::support;
        let s = &self.model.settings;
        if self.support.is_none() {
            let busy = self.op_progress.is_some() || crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) > 0;
            // (jamais par-dessus le premier lancement ni « Nouveau PC »)
            let mut b = support::Blockers { tutorial: self.wizard.active || self.migrate.open, busy, window_hidden: false, dialog_open: self.model.confirm.is_some() };
            let now = crate::util::now_ms();
            // La visibilite de la fenetre n'est interrogee que si tout le reste autorise l'affichage.
            if !support::should_show(s.support_reminders, s.support_added, s.support_last_ms, s.support_long, now, &b) {
                return;
            }
            b.window_hidden = !winutil::own_window_visible(APP_TITLE);
            if !support::should_show(s.support_reminders, s.support_added, s.support_last_ms, s.support_long, now, &b) {
                return; // reporte : rien n'est consomme
            }
            self.model.settings.support_added = 0;
            self.model.settings.support_last_ms = now;
            let _ = self.model.save_settings();
            self.support = Some(Instant::now());
        }
        let Some(since) = self.support else { return };
        let alpha = (since.elapsed().as_secs_f32() / 0.45).min(1.0);
        if alpha < 1.0 {
            ctx.request_repaint();
        }
        let mut close = false;
        let mut coffee = false;
        self.focus.set_modal_scope(true);
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Modal::new(egui::Id::new("support")).show(ctx, |ui| {
            ui.set_opacity(alpha);
            ui.set_max_width(420.0);
            ui.heading(self.t(K::SupTitle));
            ui.add_space(6.0);
            ui.label(self.t(K::SupBody));
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if self.coffee_button(ui, false) {
                    coffee = true;
                }
                let b = ui.button(self.t(K::BtnPropLater));
                let r = self.deco(ui, b, Some(K::TipSupLater));
                self.focus.set_default(r.id);
                if r.clicked() {
                    close = true;
                }
            });
        });
        self.focus.set_modal_scope(false);
        if esc || coffee {
            close = true;
        }
        if coffee {
            self.model.settings.support_long = true; // frequence reduite apres un clic sur le cafe
            let _ = self.model.save_settings();
        }
        self.support = if close { None } else { Some(since) };
    }
}
