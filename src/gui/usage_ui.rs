//! Donnees d'usage dans l'interface : question au premier lancement et aux mises a jour (deux boutons de meme poids),
//! carte des Reglages (interrupteur, identifiant, « Voir ce qui est envoye », « Effacer mes donnees », confidentialite),
//! fenetre du contenu de la file, et thread d'envoi (une fois par jour au plus, jamais pendant un programme detecte).
use super::*;
use crate::gui::ui::{self as u, Btn};
use crate::usage;

impl Shared {
    /// Thread d'envoi. Dossier de donnees de test : rien (sauf verification manuelle explicite AGSS_USAGE_LIVE=1).
    pub(super) fn spawn_usage_thread(self: &Arc<Self>) {
        let live = std::env::var("AGSS_USAGE_LIVE").as_deref() == Ok("1");
        if crate::appdirs::data_dir_from_env().is_some() && !live {
            return;
        }
        let Some(http) = usage::Http::real() else { return };
        let me = self.clone();
        let _ = std::thread::Builder::new().name("agss-usage".into()).spawn(move || {
            let lg = Logger::new(me.data_dir.join("logs").join("watch.log"), false);
            let mut wait: u64 = if live { 5 } else { 120 };
            loop {
                for _ in 0..wait {
                    std::thread::sleep(Duration::from_secs(1));
                    if me.quitting.load(Ordering::SeqCst) {
                        return;
                    }
                    if me.usage_kick.swap(false, Ordering::SeqCst) {
                        break;
                    }
                }
                wait = 600;
                let consent = usage::consent();
                let busy = || me.knowledge_busy() || me.quitting.load(Ordering::SeqCst);
                let now = crate::util::now_ms();
                if !busy() && usage::snapshot_if_due(&me.data_dir, consent, now) {
                    lg.log("[Usage] releve quotidien ajoute a la file");
                }
                let o = usage::run_once(&me.data_dir, consent, now, &http, &busy);
                if o.sent_events + o.dropped_events + o.sent_records + o.erased > 0 || o.base_entries.is_some() || o.error.is_some() {
                    lg.log(&format!(
                        "[Usage] envoi : {} evenement(s), {} refuse(s), {} emplacement(s) accepte(s), {} effacement(s){}{}",
                        o.sent_events,
                        o.dropped_events,
                        o.sent_records,
                        o.erased,
                        o.base_entries.map(|n| format!(", base maison : {} entree(s)", n)).unwrap_or_default(),
                        o.error.as_deref().map(|e| format!(" ; echec : {}", e)).unwrap_or_default()
                    ));
                }
            }
        });
    }
}

impl App {
    /// Enregistre le choix (reglages, etat du processus) ; retrait = file videe.
    pub(super) fn set_usage_consent(&mut self, yes: bool, from: &str) {
        let s = &mut self.model.settings;
        s.usage_consent = Some(yes);
        s.usage_asked = Some(usage::ask_key(crate::update::current_version()));
        // la question est repondue : plus de rappel (une prochaine version pourra la reposer)
        s.usage_late = usage::LATE_NONE;
        s.usage_late_ms = 0;
        self.late_card_open = false;
        let _ = self.model.save_settings();
        usage::set_consent(Some(yes));
        if yes {
            // Identifiant cree tout de suite : il s'affiche dans Reglages (droits d'acces et d'effacement).
            usage::install_id(&self.shared.data_dir, crate::util::now_ms());
            let s = &self.model.settings;
            usage::push(&self.shared.data_dir, usage::app_start_event(s.lang, s.expert_mode, self.start_hidden, self.big_now, crate::util::now_ms()), Some(true));
            self.shared.usage_kick.store(true, Ordering::SeqCst);
        } else {
            usage::withdraw(&self.shared.data_dir);
        }
        self.pad_log(&format!("[Usage] donnees d'usage {} ({})", if yes { "acceptees" } else { "refusees" }, from));
    }

    /// La question des statistiques est-elle encore sans reponse pour cette version ?
    pub(super) fn usage_unanswered(&self) -> bool {
        usage::needs_question(self.model.settings.usage_asked.as_deref(), crate::update::current_version())
    }

    /// Fin de l'assistant (ou de « Quoi de neuf ») : sans reponse, la question reviendra UNE fois, plus tard (carte discrete).
    pub(super) fn usage_schedule_late(&mut self) {
        let next = usage::late_after_wizard(self.usage_unanswered(), self.model.settings.usage_late);
        if next != self.model.settings.usage_late {
            self.model.settings.usage_late = next;
            self.model.settings.usage_late_ms = crate::util::now_ms();
            self.late_set_this_session = true;
            let _ = self.model.save_settings();
            self.pad_log("[Usage] question des statistiques sans reponse : rappel unique programme");
        }
    }

    /// Ligne compacte et discrete : « Aider l'auteur avec des statistiques anonymes ? » + « Oui, j'aide » (vert, mis en avant) +
    /// « Non merci » (gris) + « Voir ce qui est envoye ». RIEN n'est pre-selectionne (consentement explicite, RGPD) et aucun
    /// bouton n'a le focus d'office : tant qu'on n'a pas clique, rien n'est envoye. Une fois repondu, un petit rappel du choix.
    pub(super) fn consent_line(&mut self, ui: &mut egui::Ui) {
        let tk = u::tokens(ui.ctx());
        let big = self.big_now;
        let fs = u::tokens::FS_SMALL + if big { 2.0 } else { 0.5 };
        if !self.usage_unanswered() {
            if let Some(c) = self.model.settings.usage_consent {
                ui.label(RichText::new(self.t(if c { K::VConsentChosenYes } else { K::VConsentChosenNo })).size(fs).color(u::col(if c { tk.ok } else { tk.faint })));
            }
            return;
        }
        let mut answer: Option<bool> = None;
        let mut see = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = u::tokens::SP_S;
            let q = ui.label(RichText::new(self.t(K::VConsentTitle)).size(fs).color(u::col(tk.weak)));
            q.on_hover_text(format!("{}\n\n{}", self.t(K::VConsentBody), self.t(K::VConsentNever)));
            let h = if big { 30.0 } else { 24.0 };
            let yes_fill = tk.ok;
            let yes = egui::Button::new(RichText::new(self.t(K::BtnUsageYes)).size(fs).strong().color(u::col(crate::theme::text_on(yes_fill)))).fill(u::col(yes_fill)).corner_radius(u::tokens::R_SM).min_size(egui::vec2(0.0, h));
            let r = ui.add(yes);
            if self.deco(ui, r, Some(K::TipUsageYes)).clicked() {
                answer = Some(true);
            }
            let no = egui::Button::new(RichText::new(self.t(K::BtnUsageNo)).size(fs).color(u::col(tk.weak))).fill(u::col(tk.widget)).corner_radius(u::tokens::R_SM).min_size(egui::vec2(0.0, h));
            let r = ui.add(no);
            if self.deco(ui, r, Some(K::TipUsageNo)).clicked() {
                answer = Some(false);
            }
            let r = ui.add(egui::Label::new(RichText::new(self.t(K::BtnUsageView)).size(fs).color(u::col(tk.weak)).underline()).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
            if self.deco(ui, r, Some(K::TipUsageView)).clicked() {
                see = true;
            }
        });
        if see {
            self.consent_info = true;
        }
        if let Some(a) = answer {
            self.set_usage_consent(a, "question discrete");
        }
    }

    /// Rappel unique (2.0.1) : l'assistant s'est termine sans reponse ; la question revient en carte discrete en bas de l'accueil,
    /// une seule fois (marquee « montree » des l'affichage), puis plus jamais.
    pub(super) fn usage_late_tick(&mut self) {
        if self.late_card_open {
            return;
        }
        let s = &self.model.settings;
        let wizard_pending = crate::onboarding::pending(&s.onboarding) || self.wizard.active;
        if usage::late_card_due(self.usage_unanswered(), s.usage_late, wizard_pending, self.late_set_this_session, s.usage_late_ms, crate::util::now_ms()) && self.model.confirm.is_none() && self.pad_prompt.is_none() {
            self.late_card_open = true;
            self.model.settings.usage_late = usage::LATE_SHOWN;
            let _ = self.model.save_settings();
            self.pad_log("[Usage] question des statistiques : rappel unique affiche");
        }
    }

    /// La carte du rappel doit-elle etre dessinee maintenant (accueil, aucune fiche ouverte) ?
    pub(super) fn usage_late_visible(&self) -> bool {
        self.late_card_open && self.model.page == Page::Games && self.model.selected.is_none() && !self.wizard.active
    }

    /// Carte discrete du rappel, en bas de l'accueil.
    pub(super) fn usage_late_card(&mut self, ui: &mut egui::Ui) {
        ui.add_space(u::tokens::SP_XS);
        self.consent_line(ui);
        ui.add_space(u::tokens::SP_XS);
    }

    /// Fenetre du detail (noms CONNUS envoyes, jamais les noms tapes, fichiers, dossiers perso, chemins, appareil, IP).
    pub(super) fn consent_info_window(&mut self, ctx: &egui::Context) {
        if !self.consent_info {
            return;
        }
        let tk = u::tokens(ctx);
        let mut close = false;
        let mut view = false;
        self.focus.set_modal_scope(true);
        egui::Modal::new(egui::Id::new("consent_info")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading(self.t(K::VConsentTitle));
            ui.add_space(u::tokens::SP_S);
            ui.label(RichText::new(self.t(K::VConsentBody)).color(u::col(tk.text)));
            ui.add_space(u::tokens::SP_XS);
            ui.label(RichText::new(self.t(K::VConsentNever)).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_M);
            ui.horizontal_wrapped(|ui| {
                let r = u::button(ui, Btn::Secondary, self.t(K::VConsentSeeSent));
                if self.deco(ui, r, Some(K::TipUsageView)).clicked() {
                    view = true;
                }
                let r = ui.link(self.t(K::UsagePrivacyLink));
                if self.deco(ui, r, Some(K::TipUsagePrivacy)).clicked() {
                    winutil::open_url(usage::PRIVACY_URL);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = u::button(ui, Btn::Primary, self.t(K::BtnClose));
                    if self.deco(ui, r, None).clicked() {
                        close = true;
                    }
                });
            });
        });
        self.focus.set_modal_scope(false);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if view {
            self.load_usage_view(ctx);
        }
        if close {
            self.consent_info = false;
        }
    }

    /// Contenu de « Voir ce qui est envoye », calcule dans un thread (le releve quotidien lit le dossier partage).
    pub(super) fn load_usage_view(&mut self, ctx: &egui::Context) {
        if self.usage_view.is_none() {
            self.usage_view = Some("…".into());
        }
        let (tx, c, dd) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::UsageText(usage::pending_text(&dd, true)));
            c.request_repaint();
        });
    }

    /// Fenetre « Ce qui est envoye » et confirmation de l'effacement.
    pub(super) fn usage_windows(&mut self, ctx: &egui::Context) {
        self.consent_info_window(ctx);
        if let Some(text) = self.usage_view.clone() {
            let mut open = true;
            let mut close = false;
            egui::Window::new(self.t(K::UsageViewTitle)).id(egui::Id::new("usage_view")).collapsible(false).resizable(true).default_size([680.0, 420.0]).open(&mut open).show(ctx, |ui| {
                ui.label(RichText::new(self.t(K::UsageViewHint)).small());
                ui.add_space(4.0);
                egui::ScrollArea::both().max_height(ui.available_height() - 36.0).auto_shrink([false, false]).show(ui, |ui| {
                    let mut t = text.as_str();
                    ui.add(egui::TextEdit::multiline(&mut t).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                });
                ui.horizontal(|ui| {
                    let b = ui.button(self.t(K::BtnRefresh));
                    if self.deco(ui, b, Some(K::TipUsageView)).clicked() {
                        self.load_usage_view(ui.ctx());
                    }
                    let b = ui.button(self.t(K::BtnClose));
                    if self.deco(ui, b, None).clicked() {
                        close = true;
                    }
                });
            });
            if !open || close {
                self.usage_view = None;
            }
        }
        if self.usage_erase_ask {
            let mut done: Option<bool> = None;
            self.focus.set_modal_scope(true);
            egui::Modal::new(egui::Id::new("usage_erase")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.heading(self.t(K::BtnUsageErase));
                ui.add_space(6.0);
                ui.label(self.t(K::ConfUsageErase));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let b = ui.button(self.t(K::BtnCancel));
                    if self.deco(ui, b, Some(K::TipCancel)).clicked() {
                        done = Some(false);
                    }
                    let b = ui.button(RichText::new(self.t(K::BtnConfirm)).strong());
                    if self.deco(ui, b, Some(K::TipUsageErase)).clicked() {
                        done = Some(true);
                    }
                });
            });
            self.focus.set_modal_scope(false);
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            match done {
                Some(true) => {
                    self.usage_erase_ask = false;
                    let msg = match usage::request_erase(&self.shared.data_dir, crate::util::now_ms()) {
                        Some(_) => {
                            self.shared.usage_kick.store(true, Ordering::SeqCst);
                            self.pad_log("[Usage] effacement des donnees demande (nouvel identifiant cree)");
                            K::UsageErased
                        }
                        None => K::UsageNothingSent,
                    };
                    if self.usage_view.is_some() {
                        self.load_usage_view(ctx);
                    }
                    self.say(self.t(msg).to_string(), true);
                }
                Some(false) => self.usage_erase_ask = false,
                None => {}
            }
        }
    }
}
