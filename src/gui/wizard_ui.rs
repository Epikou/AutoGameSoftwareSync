//! Premier lancement 2.0 (lot 10) : assistant PLEIN ECRAN de 6 etapes qui CONFIGURENT vraiment l'appli en avancant
//! (remplace l'ancien tutoriel « projecteur » qui montrait sans configurer). Chaque etape reutilise un ecran existant :
//! 1 langue (drapeaux dessines) + donnees d'usage (choix du lot 3), 2 coffre (lot 8, coffres EXISTANTS proposes en premier,
//! reprise des elements d'un autre appareil), 3 appareil (nom + couleur), 4 scan et protection (lot 9), 5 ajouts libres
//! (glisser-deposer, ajout manuel, selecteur guide), 6 reglages essentiels puis PREMIERE COPIE reelle avec progression et
//! coche finale animee. Precedent / Suivant / « Plus tard » partout, reprise a l'etape memorisee, relance depuis les
//! Reglages, ecran « Quoi de neuf » pour une mise a jour. Logique pure et testee : `crate::onboarding`.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph, State};
use crate::onboarding::{self as ob, CopyState, Moved, Nav};

/// Cle du selecteur de dossier guide de l'etape 5.
const PICK_KEY: &str = "wizard_folder";

/// Premiere copie de l'etape 6 (copies demandees a la surveillance, etats relus regulierement).
struct FirstCopy {
    names: Vec<String>,
    requested_at: Instant,
    last_refresh: Instant,
    /// Instant (horloge de l'interface) ou tout s'est pose : coche animee.
    settled_at: Option<f64>,
}

pub struct WizardUi {
    /// Assistant a afficher pendant cette session (etat persistant : `settings.onboarding`).
    pub active: bool,
    /// Montre au moins une fois pendant cette session : la question des donnees d'usage attend le prochain lancement, et
    /// la garde « pas pendant un jeu » ne le cache plus.
    pub shown_this_session: bool,
    /// Sens de la derniere transition (1 = suivant, -1 = retour).
    dir: f32,
    began: Instant,
    step_at: Instant,
    block_at: Option<Instant>,
    blocked: bool,
    /// Etape 4 : avancer des que l'ajout en lot demande par « Protéger la sélection » est fini.
    advance_after_add: bool,
    /// Etape 5 : ajout en cours (thread) et noms ajoutes pendant cette etape.
    adding: Option<Arc<Mutex<Option<crate::scan::AddReport>>>>,
    added: Vec<String>,
    /// Etape 6 : premiere copie.
    copy: Option<FirstCopy>,
    /// Diagnostic des captures (`AGSS_UI_WIZARD_DEMO=consent:yes,vault:N,scan-add,copy`), jamais dans l'interface.
    demo: Vec<String>,
    demo_t: Option<f64>,
}

impl WizardUi {
    pub fn new(active: bool) -> WizardUi {
        WizardUi {
            active,
            shown_this_session: false,
            dir: 1.0,
            began: Instant::now(),
            step_at: Instant::now(),
            block_at: None,
            blocked: false,
            advance_after_add: false,
            adding: None,
            added: Vec::new(),
            copy: None,
            demo: std::env::var("AGSS_UI_WIZARD_DEMO").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
            demo_t: None,
        }
    }
}

impl App {
    /// Garde « jamais par-dessus un jeu en cours » : tant qu'un programme suivi ou detecte tourne, l'assistant ne s'ouvre pas
    /// (l'accueil normal reste dessous, la fenetre n'est jamais mise au premier plan). Une fois montre, il reste.
    pub(super) fn wizard_blocked(&mut self) -> bool {
        if self.wizard.shown_this_session {
            return false;
        }
        let due = self.wizard.block_at.map(|t| t.elapsed() > Duration::from_secs(2)).unwrap_or(true);
        if due {
            self.wizard.block_at = Some(Instant::now());
            let running = self.shared.games_running.load(Ordering::SeqCst) || self.shared.det_running.load(Ordering::SeqCst);
            self.wizard.blocked = !ob::may_show(running, false);
        }
        self.wizard.blocked
    }

    /// « Refaire la configuration » (Reglages) : les 6 etapes depuis le debut, choix actuels repris par les ecrans.
    pub(super) fn wizard_restart(&mut self, ctx: &egui::Context) {
        ob::restart(&mut self.model.settings.onboarding);
        let _ = self.model.save_settings();
        let demo = std::mem::take(&mut self.wizard.demo);
        self.wizard = WizardUi::new(true);
        self.wizard.demo = demo;
        // ecran du coffre repris de zero (coffre actuel et coffres existants relus)
        self.vault_start_detect(ctx);
        self.wizard_enter(ctx, ob::WELCOME);
        self.pad_log("[Premier lancement] assistant relance depuis les Reglages");
    }

    /// Une image de l'assistant : ecran plein, puis les fenetres dont les etapes ont besoin.
    pub(super) fn wizard_frame(&mut self, ctx: &egui::Context) {
        if !self.wizard.shown_this_session {
            self.wizard.shown_this_session = true;
            self.wizard.began = Instant::now();
            self.wizard.step_at = Instant::now();
            let step = self.model.settings.onboarding.step;
            self.wizard_enter(ctx, step);
        }
        self.wizard_poll(ctx);
        self.wizard_demo(ctx);
        self.scan_demo_start(ctx);
        let bg = u::col(u::tokens(ctx).panel);
        // « C'est un nouveau PC ? Tout récupérer » (etape 2) : la migration s'affiche a la place de l'assistant, qui reprend ensuite.
        let migrating = self.migrate.open;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(bg).inner_margin(egui::Margin::symmetric(18, 14))).show(ctx, |ui| if migrating { self.migrate_screen(ui, ctx) } else { self.wizard_screen(ui, ctx) });
        self.pad_prompt_modal(ctx);
        self.weight_overlay(ctx);
        self.vault_overlay(ctx);
        self.picker_overlay(ctx);
        self.confirm_modal(ctx);
        self.fiche_overlays(ctx);
        self.usage_windows(ctx);
        self.focus.set_modal_scope(false);
        let modal = self.model.confirm.is_some() || self.vault.confirm_open() || self.picker.open.is_some() || self.pad_prompt.is_some();
        if self.focus.end_frame(ctx, 100 + self.model.settings.onboarding.step, modal, self.pad_mode) {
            ctx.request_repaint();
        }
    }

    /// Entree sur un ecran : prepare ce dont il a besoin (detection des coffres, scan, premiere copie).
    fn wizard_enter(&mut self, ctx: &egui::Context, step: u8) {
        match step {
            ob::VAULT => {
                if !self.vault_detect_started() {
                    self.vault_start_detect(ctx);
                }
            }
            // appareils deja vus dans le coffre (synchronisation entre appareils)
            ob::DEVICE => self.load_dest_games(ctx),
            // etape 5 : le scan (s'il n'a pas tourne) fournit les ajouts en un clic
            ob::SCAN | ob::EXTRAS => self.scan_wizard_ensure(ctx),
            ob::FINISH => {
                if self.wizard.copy.is_none() {
                    self.wizard_start_copy(ctx);
                }
            }
            _ => {}
        }
    }

    /// Resultats des threads de l'assistant (ajouts de l'etape 5) et avancee automatique apres l'ajout de l'etape 4.
    fn wizard_poll(&mut self, ctx: &egui::Context) {
        if let Some(slot) = self.wizard.adding.clone() {
            if let Some(rep) = slot.lock().ok().and_then(|mut g| g.take()) {
                self.wizard.adding = None;
                self.after_change(ctx);
                if let Ok(mut q) = self.shared.sync_requests.lock() {
                    q.extend(rep.added.iter().map(|n| Some(n.clone())));
                }
                for n in &rep.added {
                    let shown = self.model.game(n).map(|g| g.shown().to_string()).unwrap_or_else(|| n.clone());
                    self.toasts.push(u::anim::now(ctx), &self.t(K::VWizAdded).replace("{}", &shown), "", State::Safe, None);
                    self.wizard.added.push(shown);
                }
                for (name, why) in &rep.skipped {
                    let s = match why {
                        crate::scan::Skip::Already(o) => self.t(K::VScanSkipAlready).replace("{0}", name).replace("{1}", o),
                        crate::scan::Skip::Overlap(o) => self.t(K::VScanSkipOverlap).replace("{0}", name).replace("{1}", o),
                        crate::scan::Skip::Missing => self.t(K::VScanSkipMissing).replace("{}", name),
                        crate::scan::Skip::Error(_) => self.t(K::VScanSkipError).replace("{}", name),
                    };
                    self.toasts.push(u::anim::now(ctx), self.t(K::VScanNotAdded), &s, State::Todo, None);
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
        }
        if self.wizard.advance_after_add && self.model.settings.onboarding.step == ob::SCAN {
            let (_, _, adding, report, _) = self.scan_wizard_status();
            if !adding && report.is_some() {
                self.wizard.advance_after_add = false;
                self.wizard_nav(ctx, Nav::Continue);
            }
        }
    }

    /// L'etape courante a-t-elle vraiment configure quelque chose ?
    fn wizard_did(&self, step: u8) -> bool {
        match step {
            ob::VAULT => self.model.dest().is_some(),
            ob::SCAN => self.scan_wizard_status().3.map(|n| n > 0).unwrap_or(false) || !self.model.config.games.is_empty(),
            _ => true,
        }
    }

    /// Bouton de bas de page : etat memorise, donnees d'usage, ecran suivant ou fin.
    fn wizard_nav(&mut self, ctx: &egui::Context, nav: Nav) {
        let cur = self.model.settings.onboarding.step;
        let did = self.wizard_did(cur);
        if let Some(st) = ob::usage_step(cur) {
            crate::usage::record(&self.shared.data_dir, "onboarding_step", &[("step", st), ("action", ob::usage_action(nav)), ("duration", crate::usage::bucket_duration(self.wizard.step_at.elapsed().as_secs()))]);
        }
        let moved = ob::navigate(&mut self.model.settings.onboarding, nav, did);
        let _ = self.model.save_settings();
        self.wizard.step_at = Instant::now();
        self.wizard.dir = if nav == Nav::Back { -1.0 } else { 1.0 };
        match moved {
            Moved::To(s) => {
                self.pad_log(&format!("[Premier lancement] etape {} ({})", s, ob::usage_action(nav)));
                self.wizard_enter(ctx, s);
            }
            Moved::Finished => self.wizard_finish(ctx),
            Moved::Stay => {}
        }
    }

    /// Fin du parcours : accueil, cascade des tuiles, donnees d'usage.
    fn wizard_finish(&mut self, ctx: &egui::Context) {
        // Statistiques sans reponse : la question reviendra UNE fois, plus tard, en carte discrete (jamais d'envoi sans clic).
        self.usage_schedule_late();
        let items = crate::usage::bucket_count(self.model.config.games.len());
        crate::usage::record(&self.shared.data_dir, "onboarding_done", &[("duration", crate::usage::bucket_duration(self.wizard.began.elapsed().as_secs())), ("items", items), ("migration", if self.migrate.did_migrate { "yes" } else { "no" })]);
        self.model.settings.tutorial_seen = true;
        self.model.settings.tutorial_step = 0;
        self.model.settings.onboarding.finished = true;
        let _ = self.model.save_settings();
        self.wizard.active = false;
        self.scan.embedded = false;
        self.model.page = Page::Games;
        self.on_page_enter(Page::Games, ctx);
        self.home.epoch += 1;
        self.last_status = Instant::now() - Duration::from_secs(60);
        self.toasts.push(u::anim::now(ctx), self.t(K::VWizReady), self.t(K::VTaglineSub), State::Safe, None);
        self.pad_log("[Premier lancement] assistant termine");
    }

    /// « Terminer plus tard » : l'assistant se ferme et reprendra a cette etape au prochain lancement.
    fn wizard_close_for_now(&mut self, ctx: &egui::Context) {
        self.wizard.active = false;
        self.scan.embedded = false;
        self.model.page = Page::Games;
        self.on_page_enter(Page::Games, ctx);
        self.home.epoch += 1;
        self.toasts.push(u::anim::now(ctx), self.t(K::VWizResumeLater), "", State::Paused, None);
        self.pad_log(&format!("[Premier lancement] ferme a l'etape {} (reprise au prochain lancement)", self.model.settings.onboarding.step));
    }

    // ------------------------------------------------------------ ecran

    fn wizard_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let o = self.model.settings.onboarding.clone();
        let step = o.step;
        let full = ui.max_rect();
        let col_w = full.width().min(if big { 1080.0 } else { 900.0 });
        let x0 = full.center().x - col_w / 2.0;
        let head_h = if big { 64.0 } else { 52.0 };
        let foot_h = if big { 76.0 } else { 62.0 };
        let head = egui::Rect::from_min_size(egui::pos2(full.left(), full.top()), egui::vec2(full.width(), head_h));
        // Statistiques (2.0.1) : une ligne discrete en bas de la DERNIERE etape, rien de pre-selectionne.
        let consent_h = if self.wizard_is_last(step) { if big { 46.0 } else { 38.0 } } else { 0.0 };
        let body = egui::Rect::from_min_max(egui::pos2(x0, head.bottom() + 4.0), egui::pos2(x0 + col_w, full.bottom() - foot_h - 4.0 - consent_h));
        let consent_rect = egui::Rect::from_min_max(egui::pos2(x0, body.bottom()), egui::pos2(x0 + col_w, full.bottom() - foot_h - 4.0));
        let foot = egui::Rect::from_min_max(egui::pos2(x0, full.bottom() - foot_h), egui::pos2(x0 + col_w, full.bottom()));

        // --- en-tete : points de progression qui s'etirent, « Etape k sur n », « Terminer plus tard »
        let mut close = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(head), |ui| {
            let seq = ob::sequence(o.plan);
            let pos = ob::position(o.plan, step);
            let (dot, cur_w, gap) = if big { (10.0, 36.0, 10.0) } else { (8.0, 30.0, 8.0) };
            let total_w: f32 = seq.iter().map(|s| if *s == step { cur_w } else { dot }).sum::<f32>() + gap * (seq.len().saturating_sub(1)) as f32;
            let mut x = head.center().x - total_w / 2.0;
            let y = head.top() + 14.0;
            let m = u::anim::motion(ctx);
            for s in &seq {
                let target = if *s == step { cur_w } else { dot };
                let w = ctx.animate_value_with_time(egui::Id::new(("wiz_dot", *s)), target, m.dur(u::tokens::DUR_NORMAL) as f32);
                let fill = if *s == step { tk.accent_hover } else if o.done & ob::bit(*s) != 0 || *s < step { tk.accent } else if tk.dark { [0x4a, 0x4a, 0x4a] } else { [0xc8, 0xcc, 0xc9] };
                let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, dot));
                ui.painter().rect_filled(r, dot / 2.0, u::col(fill));
                if (w - target).abs() > 0.5 {
                    u::anim::need_transition(ctx);
                }
                x += w + gap;
            }
            let label = match pos {
                Some((k, n)) => self.t(K::VWizStepOf).replace("{0}", &k.to_string()).replace("{1}", &n.to_string()),
                None => self.t(K::VNewTitle).to_string(),
            };
            ui.painter().text(egui::pos2(head.center().x, y + dot + 8.0), egui::Align2::CENTER_TOP, label, u::font(u::tokens::FS_SMALL * if big { 1.2 } else { 1.0 }), u::col(tk.faint));
            let br = egui::Rect::from_min_max(egui::pos2(head.right() - 260.0, head.top()), egui::pos2(head.right(), head.top() + head_h - 8.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::right_to_left(egui::Align::Min)), |ui| {
                let r = u::button_with(ui, Btn::Ghost, self.t(K::VWizClose), None, false);
                if self.deco(ui, r, Some(K::TipWizClose)).clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.wizard_close_for_now(ctx);
            return;
        }

        // --- contenu (glissement horizontal de 24 px entre etapes, fondu seul en mouvement reduit)
        let dir = self.wizard.dir;
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            u::step_transition(ui, "wizard", step as u64, dir, |ui| {
                if step == ob::SCAN {
                    // le scan a sa propre zone de defilement (resultats + resume collant)
                    self.wizard_step_scan(ui, ctx);
                } else {
                    egui::ScrollArea::vertical().id_salt(("wizard_body", step)).auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_max_width((ui.available_width() - 12.0).max(200.0));
                        match step {
                            ob::WHATS_NEW => self.wizard_whats_new(ui, ctx),
                            ob::WELCOME => self.wizard_step_welcome(ui, ctx),
                            ob::VAULT => self.wizard_step_vault(ui, ctx),
                            ob::DEVICE => self.wizard_step_device(ui, ctx),
                            ob::EXTRAS => self.wizard_step_extras(ui, ctx),
                            _ => self.wizard_step_finish(ui, ctx),
                        }
                        ui.add_space(u::tokens::SP_L);
                    });
                }
            });
        });

        if consent_h > 0.0 {
            ui.scope_builder(egui::UiBuilder::new().max_rect(consent_rect.shrink2(egui::vec2(4.0, 6.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| self.consent_line(ui));
        }

        // --- bas de page : Retour · Plus tard · action principale
        let mut nav: Option<Nav> = None;
        let mut protect = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(foot), |ui| {
            ui.painter().line_segment([foot.left_top(), foot.right_top()], egui::Stroke::new(1.0_f32, u::col(tk.line)));
            ui.add_space(if big { 14.0 } else { 10.0 });
            ui.horizontal(|ui| {
                let can_back = ob::prev(o.plan, o.whats_new, step).is_some();
                let r = ui.add_enabled_ui(can_back, |ui| u::button_with(ui, Btn::Ghost, self.t(K::VWizBack), Some(Glyph::ChevronLeft), big)).inner;
                if self.deco(ui, r, None).clicked() && can_back {
                    nav = Some(Nav::Back);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (label, enabled, is_protect) = self.wizard_primary(step);
                    let r = ui.add_enabled_ui(enabled, |ui| u::button_with(ui, Btn::Primary, &label, if is_protect { Some(Glyph::Shield) } else { None }, true)).inner;
                    let r = self.deco(ui, r, None);
                    if !self.focus_from_nav && r.enabled() {
                        self.focus.set_default(r.id);
                    }
                    if r.clicked() && enabled {
                        if is_protect {
                            protect = true;
                        } else {
                            nav = Some(Nav::Continue);
                        }
                    }
                    if step != ob::WHATS_NEW {
                        ui.add_space(u::tokens::SP_S);
                        let r = u::button_with(ui, Btn::Ghost, self.t(K::VWizLater), None, big);
                        if self.deco(ui, r, None).clicked() {
                            nav = Some(Nav::Later);
                        }
                    }
                });
            });
        });
        if protect {
            self.wizard.advance_after_add = true;
            self.scan_wizard_protect(ctx);
            // rien a ajouter (aucune case cochee) : pas d'attente
            if !self.scan_wizard_status().2 {
                self.wizard.advance_after_add = false;
            }
        }
        if let Some(n) = nav {
            self.wizard_nav(ctx, n);
        }
    }

    /// Dernier ecran du parcours ? (la ligne des statistiques y est affichee.)
    fn wizard_is_last(&self, step: u8) -> bool {
        let plan = self.model.settings.onboarding.plan;
        if step == ob::WHATS_NEW {
            ob::sequence(plan).is_empty()
        } else {
            ob::next(plan, step).is_none()
        }
    }

    /// Libelle de l'action principale, actif ou non, et s'il s'agit de « Protéger la sélection » (etape 4).
    fn wizard_primary(&self, step: u8) -> (String, bool, bool) {
        let last = ob::next(self.model.settings.onboarding.plan, step).is_none();
        match step {
            ob::SCAN => {
                let (started, finished, adding, report, sel) = self.scan_wizard_status();
                if adding || self.wizard.advance_after_add {
                    (self.t(K::VScanAdding).to_string(), false, false)
                } else if started && finished && report.is_none() && sel > 0 {
                    (self.t(K::VScanProtect).replace("{}", &sel.to_string()), true, true)
                } else {
                    (self.t(K::VWizContinue).to_string(), true, false)
                }
            }
            ob::FINISH => (self.t(K::VWizSeeHome).to_string(), true, false),
            _ if last => (self.t(K::VWizSeeHome).to_string(), true, false),
            _ => (self.t(K::VWizContinue).to_string(), true, false),
        }
    }

    /// Titre et sous-titre d'une etape (centres).
    fn wizard_title(&self, ui: &mut egui::Ui, title: &str, sub: &str) {
        let tk = u::tokens(ui.ctx());
        let big = self.big_now;
        ui.vertical_centered(|ui| {
            ui.add_space(u::tokens::SP_S);
            let w = ui.available_width();
            let g = u::layout(ui, title, if big { 32.0 } else { u::tokens::FS_TITLE }, u::col(tk.text), w, 2);
            let (r, _) = ui.allocate_exact_size(egui::vec2(g.size().x.min(w), g.size().y), egui::Sense::hover());
            ui.painter().galley(r.min, g, u::col(tk.text));
            if !sub.is_empty() {
                ui.add_space(u::tokens::SP_XS);
                ui.set_max_width(w.min(680.0));
                ui.label(RichText::new(sub).size(if big { u::tokens::FS_BODY * 1.15 } else { u::tokens::FS_BODY + 1.0 }).color(u::col(tk.weak)));
            }
            ui.add_space(u::tokens::SP_L);
        });
    }

    // ------------------------------------------------------------ « Quoi de neuf » (mise a jour)

    fn wizard_whats_new(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        ui.vertical_centered(|ui| {
            ui.add_space(u::tokens::SP_M);
            let s = if big { 76.0 } else { 64.0 };
            let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
            let age = u::anim::age_of(ctx, egui::Id::new("wiz_new_shield"), 1);
            let p = u::anim::run(ctx, age, u::anim::motion(ctx).dur(0.6));
            u::status_banner::paint_shield(ui.painter(), r, State::Safe, p);
        });
        self.wizard_title(ui, self.t(K::VNewTitle), self.t(K::VNewBody));
        let rows = [(Glyph::Home, K::VNew1), (Glyph::Clock, K::VNew2), (Glyph::Drive, K::VNew3), (Glyph::Eye, K::VNew4), (Glyph::ArrowDown, K::VNew5), (Glyph::Pc, K::VNew6)];
        u::card(ui, None, |ui| {
            for (i, (g, k)) in rows.iter().enumerate() {
                u::cascade_in(ui, ("wiz_new", i), i, 1, |ui| {
                    ui.horizontal(|ui| {
                        let s = if big { 30.0 } else { 24.0 };
                        let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                        ui.painter().rect_filled(r, 7.0, u::col(tk.accent_soft));
                        u::paint_glyph(ui.painter(), r.shrink(s * 0.2), *g, u::col(tk.accent_hover));
                        ui.add_space(u::tokens::SP_S);
                        ui.label(RichText::new(self.t(*k)).size(if big { u::tokens::FS_BODY * 1.2 } else { u::tokens::FS_BODY }).color(u::col(tk.text)));
                    });
                });
                ui.add_space(u::tokens::SP_S);
            }
        });
        ui.add_space(u::tokens::SP_M);
        let names: Vec<&str> = ob::sequence(self.model.settings.onboarding.plan).iter().map(|s| self.t(step_name(*s))).collect();
        ui.vertical_centered(|ui| {
            let txt = if names.is_empty() { self.t(K::VNewNoSteps).to_string() } else { self.t(K::VNewSteps).replace("{}", &names.join(" \u{b7} ")) };
            ui.label(RichText::new(txt).strong().color(u::col(tk.weak)));
        });
    }

    // ------------------------------------------------------------ etape 1 : bienvenue et langue

    fn wizard_step_welcome(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        ui.vertical_centered(|ui| {
            ui.add_space(u::tokens::SP_XS);
            let w = ui.available_width().min(660.0);
            self.welcome_animation(ui, w, if big { 118.0 } else { 158.0 });
        });
        self.wizard_title(ui, self.t(K::VTagline), self.t(K::VWelcomeBody));
        // Langue : drapeaux dessines (5 langues).
        let mut chosen: Option<Lang> = None;
        ui.vertical_centered(|ui| {
            let (cw, ch) = if big { (128.0, 92.0) } else { (112.0, 90.0) };
            let gap = u::tokens::SP_S;
            let n = Lang::ALL.len() as f32;
            let row_w = cw * n + gap * (n - 1.0);
            let (row, _) = ui.allocate_exact_size(egui::vec2(row_w.min(ui.available_width()), ch), egui::Sense::hover());
            let cw = ((row.width() - gap * (n - 1.0)) / n).min(cw);
            for (i, l) in Lang::ALL.into_iter().enumerate() {
                let rect = egui::Rect::from_min_size(egui::pos2(row.left() + i as f32 * (cw + gap), row.top()), egui::vec2(cw, ch));
                let resp = ui.interact(rect, egui::Id::new(("wiz_lang", i)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                let resp = self.deco(ui, resp, Some(K::TipLangCard));
                let selected = self.model.settings.lang == l;
                let m = u::anim::motion(ctx);
                let hov = ctx.animate_bool_with_time(resp.id.with("h"), resp.hovered(), m.dur(u::tokens::DUR_FAST) as f32);
                let p = ui.painter();
                let fill = if selected { crate::theme::mix(tk.card2, tk.accent, 0.18) } else { crate::theme::mix(tk.card2, tk.widget, hov * 0.6) };
                p.rect_filled(rect, u::tokens::R_LG, u::col(fill));
                p.rect_stroke(rect, u::tokens::R_LG, egui::Stroke::new(if selected { 2.5_f32 } else { 1.0_f32 }, u::col(if selected { tk.accent } else { crate::theme::mix(tk.line, tk.accent, hov) })), egui::StrokeKind::Inside);
                let fh = ch * 0.42;
                let flag = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 12.0 + fh / 2.0), egui::vec2(fh * 1.5, fh));
                paint_flag(l, p, flag);
                p.text(egui::pos2(rect.center().x, rect.bottom() - 12.0), egui::Align2::CENTER_BOTTOM, self.lang_label(l), u::font(if big { 17.0 } else { 15.0 }), u::col(tk.text));
                if resp.has_focus() {
                    u::focus_ring(ui, rect, u::tokens::R_LG);
                }
                if resp.clicked() && !selected {
                    chosen = Some(l);
                }
            }
        });
        if let Some(l) = chosen {
            self.model.settings.lang = l;
            let _ = self.model.save_settings();
            self.pad_log(&format!("[Premier lancement] langue {}", l.code()));
        }
    }

    // ------------------------------------------------------------ etape 2 : le coffre

    fn wizard_step_vault(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.vault_ready_shown() {
            self.wizard_title(ui, self.t(K::VWizName2), "");
        } else {
            self.wizard_title(ui, self.t(K::VVaultPickTitle), self.t(K::VVaultPickBody));
        }
        let mut acts = super::vault_ui::VaultActs::default();
        self.vault_contents(ui, ctx, true, &mut acts);
        let keep = acts.keep;
        self.vault_handle(ctx, acts, false);
        if keep {
            self.wizard_nav(ctx, Nav::Continue);
        }
    }

    // ------------------------------------------------------------ etape 3 : cet appareil

    fn wizard_step_device(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        self.wizard_title(ui, self.t(K::VWizDeviceTitle), self.t(K::VWizDeviceBody));
        let me = self.model.ctx.machine.clone();
        let cur = self.machine_rgb(&me);
        let mut pick: Option<[u8; 3]> = None;
        u::card(ui, None, |ui| {
            ui.label(RichText::new(self.t(K::VDeviceName)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_XS);
            // nom en grand, sur sa couleur (comme sur les tuiles et la frise)
            let fs = if big { 30.0 } else { 24.0 };
            let fg = crate::theme::text_on(cur);
            let g = u::layout_line(ui, &me, fs, u::col(fg));
            let pad = egui::vec2(18.0, 8.0);
            let (r, _) = ui.allocate_exact_size(g.size() + pad * 2.0, egui::Sense::hover());
            let m = u::anim::motion(ctx);
            let c = ctx.animate_value_with_time(egui::Id::new("wiz_dev_c"), cur[0] as f32 + cur[1] as f32 * 256.0 + cur[2] as f32 * 65536.0, m.dur(u::tokens::DUR_FAST) as f32);
            let _ = c;
            ui.painter().rect_filled(r, r.height() / 2.0, u::col(cur));
            ui.painter().galley(r.min + pad, g, u::col(fg));
            ui.add_space(u::tokens::SP_XS);
            ui.label(RichText::new(self.t(K::VDeviceNameFrom)).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
            ui.add_space(u::tokens::SP_M);
            ui.label(RichText::new(self.t(K::VDeviceColor)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_XS);
            let n = if self.model.settings.expert_mode { crate::machinecolor::PALETTE.len() } else { 8 };
            ui.horizontal_wrapped(|ui| {
                let s = if big { 38.0 } else { 30.0 };
                for (i, c) in crate::machinecolor::PALETTE.iter().take(n).chain(std::iter::once(&cur).filter(|c| !crate::machinecolor::PALETTE.iter().take(n).any(|p| p == *c))).enumerate() {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::click());
                    let resp = self.deco(ui, resp.on_hover_cursor(egui::CursorIcon::PointingHand), Some(K::TipDeviceColor));
                    let on = *c == cur;
                    ui.painter().circle_filled(r.center(), s * 0.42, u::col(*c));
                    if on {
                        ui.painter().circle_stroke(r.center(), s * 0.5 - 1.0, egui::Stroke::new(2.0_f32, u::col(tk.text)));
                        let t = crate::theme::text_on(*c);
                        u::paint_glyph(ui.painter(), r.shrink(s * 0.28), Glyph::Check, u::col(t));
                    }
                    if resp.has_focus() {
                        u::focus_ring(ui, r, s / 2.0);
                    }
                    if resp.clicked() && !on {
                        pick = Some(*c);
                    }
                    let _ = i;
                }
            });
        });
        if let Some(c) = pick {
            self.model.settings.machine_colors.insert(me.clone(), c);
            let _ = self.model.save_settings();
        }
        ui.add_space(u::tokens::SP_M);
        // Synchronisation entre appareils : l'autre argument principal.
        let others: Vec<String> = self.machines.iter().filter(|m| !m.eq_ignore_ascii_case(&me)).cloned().collect();
        u::card(ui, None, |ui| {
            ui.horizontal(|ui| {
                let s = if big { 34.0 } else { 28.0 };
                let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                ui.painter().rect_filled(r, 8.0, u::col(tk.accent_soft));
                u::paint_glyph(ui.painter(), r.shrink(s * 0.2), Glyph::Pc, u::col(tk.accent_hover));
                ui.add_space(u::tokens::SP_S);
                ui.label(RichText::new(self.t(K::VWizSyncTitle)).strong().size(if big { u::tokens::FS_H2 } else { u::tokens::FS_H3 + 1.0 }).color(u::col(tk.text)));
            });
            ui.add_space(u::tokens::SP_XS);
            ui.label(RichText::new(self.t(K::VWizSyncBody)).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_S);
            if others.is_empty() {
                ui.label(RichText::new(self.t(K::VWizSyncAlone)).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(self.t(K::VWizSyncOthers).replace("{}", "")).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                    u::device_pill(ui, &me, cur);
                    for m in &others {
                        u::device_pill(ui, m, self.machine_rgb(m));
                    }
                });
            }
        });
    }

    // ------------------------------------------------------------ etape 4 : le scan et la protection

    fn wizard_step_scan(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        self.wizard_title(ui, self.t(K::VWizScanTitle), self.t(K::VScanPcHint));
        if self.model.dest().is_none() {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(self.t(K::VWizScanNoVault)).size(u::tokens::FS_SMALL).color(u::col(tk.orange)));
            });
            ui.add_space(u::tokens::SP_S);
        }
        self.scan_wizard_ensure(ctx);
        self.scan.embedded = true;
        self.scan_screen(ui, ctx);
        self.scan.embedded = false;
    }

    // ------------------------------------------------------------ etape 5 : autre chose a proteger ?

    fn wizard_step_extras(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        self.wizard_title(ui, self.t(K::VWizExtrasTitle), self.t(K::VWizExtrasBody));
        // Depot sur la fenetre : un dossier est protege tout de suite ; un programme (ou son raccourci) ouvre le formulaire
        // d'ajout ci-dessous, prerempli (emplacement trouve tout seul ou choisi avec le selecteur guide).
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for d in dropped {
            if d.is_dir() {
                self.wizard_add_items(ctx, vec![crate::scan::folder_item(&d)], "drop");
            } else if let Some(e) = self.resolve_program(&d) {
                self.add_start_program(e);
            }
        }
        if let Some(p) = self.take_picked(PICK_KEY) {
            if !p.is_file {
                self.wizard_add_items(ctx, vec![crate::scan::folder_item(&p.path)], "manual");
            }
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        // Zone « dossier » : depot n'importe ou dans la fenetre, ou selecteur guide.
        let h = if big { 92.0 } else { 76.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
        let stroke = if hovering { egui::Stroke::new(2.5_f32, u::col(tk.accent)) } else { egui::Stroke::new(1.5_f32, u::col(tk.line)) };
        ui.painter().rect_filled(rect, 14.0, u::col(if hovering { crate::theme::mix(tk.card2, tk.accent, 0.12) } else { tk.card }));
        dashed_rect(ui.painter(), rect.shrink(1.0), stroke);
        let mut choose = false;
        let mut program = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(18.0, 10.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let s = if big { 40.0 } else { 32.0 };
            let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
            u::paint_glyph(ui.painter(), r, Glyph::Plus, u::col(if hovering { tk.accent_hover } else { tk.weak }));
            ui.add_space(u::tokens::SP_S);
            ui.label(RichText::new(self.t(K::VWizDropHint)).size(if big { u::tokens::FS_BODY * 1.2 } else { u::tokens::FS_BODY }).color(u::col(tk.weak)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = u::button_with(ui, Btn::Secondary, self.t(K::VWizChooseFolder), Some(Glyph::Folder), big);
                if self.deco(ui, r, None).clicked() {
                    choose = true;
                }
                let r = u::button_with(ui, Btn::Secondary, self.t(K::VWizChooseProgram), Some(Glyph::Window), big);
                if self.deco(ui, r, None).clicked() {
                    program = true;
                }
                if self.wizard.adding.is_some() {
                    ui.add(egui::Spinner::new());
                }
            });
        });
        if choose {
            self.open_picker(ctx, PICK_KEY, crate::gui::picker_ui::PickOpts::default());
        }
        if program {
            if let Some(p) = Self::program_dialog(self.model.settings.lang).pick_file() {
                if let Some(e) = self.resolve_program(&p) {
                    self.add_start_program(e);
                }
            }
        }
        // Ajouts en un clic proposes par le scan (Documents, Bureau, raccourcis du Bureau, fond d'ecran).
        let quick = self.scan_quick_items();
        if !quick.is_empty() {
            ui.add_space(u::tokens::SP_M);
            let mut add: Option<crate::scan::Item> = None;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(self.t(K::VWizQuickAdd)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                for (it, shown) in &quick {
                    let r = u::button_with(ui, Btn::Ghost, &format!("+ {}", shown), None, false);
                    let r = self.deco(ui, r, None);
                    let r = match self.scan_size_of(&it.key) {
                        Some(s) => r.on_hover_text(self.bytes(s.bytes)),
                        None => r,
                    };
                    if r.clicked() && self.wizard.adding.is_none() {
                        add = Some(it.clone());
                    }
                }
            });
            if let Some(it) = add {
                self.wizard_add_items(ctx, vec![it], "manual");
            }
        }
        if !self.wizard.added.is_empty() {
            ui.add_space(u::tokens::SP_S);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(self.t(K::VWizAddedHere).replace("{}", "")).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                for n in self.wizard.added.clone() {
                    let _ = u::chip(ui, &n, None, true);
                }
            });
        }
        // Programme choisi ou depose : formulaire d'ajout existant (emplacement trouve tout seul ou choisi avec le selecteur
        // guide, alertes de doublon), affiche seulement a ce moment-la.
        if !self.man_exe.trim().is_empty() || self.pending_add.is_some() || self.pending_overlap.is_some() {
            ui.add_space(u::tokens::SP_M);
            self.add_form(ui, ctx);
        }
    }

    /// Ajout d'elements (dossier depose ou choisi, ajout rapide) dans un thread ; bilan repris par `wizard_poll`.
    fn wizard_add_items(&mut self, ctx: &egui::Context, items: Vec<crate::scan::Item>, source: &'static str) {
        if self.wizard.adding.is_some() || items.is_empty() {
            return;
        }
        // Alerte poids (2.0.1) : taille mesuree en fond, choix demande si l'element est lourd.
        self.weight_gate_items(ctx, items, source);
    }

    /// Suite de `wizard_add_items` une fois le poids verifie (ou choisi) : `keep` = versions choisies (cle de l'element).
    pub(super) fn wizard_add_items_checked(&mut self, ctx: &egui::Context, items: Vec<crate::scan::Item>, source: &'static str, keep: HashMap<String, usize>) {
        if self.wizard.adding.is_some() || items.is_empty() {
            return;
        }
        let sizes: HashMap<String, crate::scan::Size> = items.iter().filter_map(|i| self.scan_size_of(&i.key).map(|s| (i.key.clone(), s))).collect();
        let shown: HashMap<String, String> = items.iter().map(|i| (i.key.clone(), self.wizard_item_name(i))).collect();
        let slot: Arc<Mutex<Option<crate::scan::AddReport>>> = Arc::new(Mutex::new(None));
        self.wizard.adding = Some(slot.clone());
        let (dd, c) = (self.shared.data_dir.clone(), ctx.clone());
        std::thread::spawn(move || {
            let cx = make_ctx(&dd);
            let rep = crate::scan::add_items_with_keep(&cx, &items, &sizes, &|it| shown.get(&it.key).cloned(), source, &keep);
            if let Ok(mut s) = slot.lock() {
                *s = Some(rep);
            }
            c.request_repaint();
        });
    }

    fn wizard_item_name(&self, it: &crate::scan::Item) -> String {
        self.scan_quick_items().into_iter().find(|(i, _)| i.key == it.key).map(|(_, s)| s).unwrap_or_else(|| it.name.clone())
    }

    // ------------------------------------------------------------ etape 6 : reglages essentiels et premiere copie

    fn wizard_step_finish(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        self.wizard_title(ui, self.t(K::VWizFinishTitle), self.t(K::VWizFinishBody));
        u::card(ui, None, |ui| {
            // Demarrer avec l'ordinateur (Windows : regle de demarrage de l'appli ; Linux : autostart de la session).
            let mut start = self.wizard_startup_enabled();
            if u::toggle(ui, &mut start, self.t(K::VStartWithPc)).changed() {
                self.wizard_set_startup(start);
            }
            ui.label(RichText::new(self.t(K::VStartHint)).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
            ui.add_space(u::tokens::SP_S);
            let mut det = self.model.settings.detect_enabled;
            if u::toggle(ui, &mut det, self.t(K::VSuggestNew)).changed() {
                self.model.settings.detect_enabled = det;
                let _ = self.model.save_settings();
                self.pad_log(&format!("[Detection] detection {} (premier lancement)", if det { "activee" } else { "desactivee" }));
                crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "detection"), ("value", if det { "on" } else { "off" })]);
            }
            ui.add_space(u::tokens::SP_S);
            let mut auto = self.model.settings.update_mode() == crate::settings::UpdateMode::Auto;
            if u::toggle(ui, &mut auto, self.t(K::VAutoUpdates)).changed() {
                let m = if auto { crate::settings::UpdateMode::Auto } else { crate::settings::UpdateMode::Ask };
                self.model.settings.set_update_mode(m);
                let _ = self.model.save_settings();
                crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "update_mode"), ("value", if auto { "auto" } else { "notify" })]);
            }
        });
        ui.add_space(u::tokens::SP_M);
        u::card(ui, Some(self.t(K::VWizName6)), |ui| self.wizard_first_copy(ui, ctx, big));
    }

    pub(super) fn wizard_startup_enabled(&self) -> bool {
        #[cfg(windows)]
        {
            self.model.settings.startup_rules.iter().find(|c| c.id == crate::startup::ID_APP).map(|c| c.enabled).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            winutil::autostart_enabled()
        }
    }

    fn wizard_set_startup(&mut self, on: bool) {
        #[cfg(windows)]
        {
            self.set_rule_cfg(crate::startup::ID_APP, |c| {
                c.enabled = on;
                c.minimized = true;
            });
            self.model.request(Confirm::ApplyStartup);
        }
        #[cfg(not(windows))]
        {
            let exe = std::env::current_exe().unwrap_or_default();
            winutil::set_autostart(on, &exe);
        }
        crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "startup"), ("value", if on { "on" } else { "off" })]);
    }

    /// Lance la premiere copie de tous les elements (file de la surveillance, meme moteur que « Synchroniser maintenant »).
    fn wizard_start_copy(&mut self, ctx: &egui::Context) {
        if self.model.dest().is_none() || self.model.config.games.is_empty() {
            return;
        }
        if self.shared.state() != WatchState::Running {
            self.shared.start_watch();
        }
        let names: Vec<String> = self.model.config.games.iter().filter(|g| !self.model.settings.per_game.get(&g.name).map(|s| s.paused).unwrap_or(false)).map(|g| g.name.clone()).collect();
        if let Ok(mut q) = self.shared.sync_requests.lock() {
            q.extend(names.iter().map(|n| Some(n.clone())));
        }
        crate::perf::log(&self.model.ctx, &format!("UI : premier lancement, premiere copie demandee pour {} element(s)", names.len()));
        self.wizard.copy = Some(FirstCopy { names, requested_at: Instant::now(), last_refresh: Instant::now() - Duration::from_secs(10), settled_at: None });
        self.refresh_statuses(ctx);
    }

    /// Etat de chaque element pendant la premiere copie.
    fn wizard_copy_states(&self) -> Vec<(String, CopyState)> {
        let Some(c) = &self.wizard.copy else { return vec![] };
        let busy = crate::perf::busy_games();
        let queue_empty = self.shared.sync_requests.lock().map(|q| q.is_empty()).unwrap_or(true);
        // Plus rien ne bouge depuis un moment (programme ouvert, coffre injoignable...) : ce qui reste attend.
        let stale = queue_empty && busy.is_empty() && crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) == 0 && c.requested_at.elapsed() > Duration::from_secs(25);
        c.names
            .iter()
            .map(|n| {
                let st = if busy.iter().any(|b| b == n) {
                    CopyState::Busy
                } else {
                    match self.model.statuses.get(n) {
                        Some(GameStatus::UpToDate) => CopyState::Safe,
                        Some(GameStatus::Running) | Some(GameStatus::Error(_)) | Some(GameStatus::NoDest) | Some(GameStatus::Conflict) => CopyState::Waiting,
                        _ if stale => CopyState::Waiting,
                        _ => CopyState::Queued,
                    }
                };
                (n.clone(), st)
            })
            .collect()
    }

    fn wizard_first_copy(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, big: bool) {
        let tk = u::tokens(ctx);
        if self.model.dest().is_none() {
            ui.label(RichText::new(self.t(K::VFirstCopyNoVault)).color(u::col(tk.weak)));
            return;
        }
        if self.model.config.games.is_empty() {
            ui.label(RichText::new(self.t(K::VFirstCopyNothing)).color(u::col(tk.weak)));
            return;
        }
        if self.wizard.copy.is_none() {
            self.wizard_start_copy(ctx);
        }
        let states = self.wizard_copy_states();
        let tally = ob::tally(&states.iter().map(|(_, s)| *s).collect::<Vec<_>>());
        let now = u::anim::now(ctx);
        // etats relus regulierement tant que la copie n'est pas posee
        if let Some(c) = self.wizard.copy.as_mut() {
            if !tally.settled() {
                c.settled_at = None;
                if c.last_refresh.elapsed() > Duration::from_millis(1500) {
                    c.last_refresh = Instant::now();
                    self.refresh_statuses(ctx);
                }
                u::anim::wake_at(ctx, now + 0.5);
            } else if c.settled_at.is_none() {
                c.settled_at = Some(now);
            }
        }
        let settled_at = self.wizard.copy.as_ref().and_then(|c| c.settled_at);
        // progression globale (ne recule jamais)
        let frac = tally.fraction();
        let shown = ctx.animate_value_with_time(egui::Id::new("wiz_copy_frac"), frac, 0.3);
        if let Some(t0) = settled_at.filter(|_| tally.all_safe()) {
            // coche finale animee + petit eclat d'accent
            ui.vertical_centered(|ui| {
                let s = if big { 84.0 } else { 68.0 };
                let (r, _) = ui.allocate_exact_size(egui::vec2(s * 2.0, s), egui::Sense::hover());
                let age = (now - t0).max(0.0);
                let c = egui::Rect::from_center_size(r.center(), egui::vec2(s, s));
                u::paint_state(ui.painter(), c, State::Safe, age, None);
                paint_burst(ui.painter(), c.center(), s * 0.75, age, u::anim::motion(ctx).reduced, u::col(tk.accent_hover));
                if age < u::tokens::DUR_BURST + 0.1 {
                    u::anim::need_transition(ctx);
                }
                ui.label(RichText::new(self.t(K::VFirstCopyDone)).strong().size(if big { u::tokens::FS_H2 * 1.2 } else { u::tokens::FS_H2 }).color(u::col(tk.text)));
            });
        } else {
            let txt = if tally.settled() {
                self.t(K::VFirstCopyPartial).replace("{0}", &tally.safe.to_string()).replace("{1}", &tally.total.to_string())
            } else {
                self.t(K::VFirstCopyRunning).replace("{0}", &(tally.safe + tally.waiting).to_string()).replace("{1}", &tally.total.to_string())
            };
            ui.horizontal(|ui| {
                if !tally.settled() {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    u::paint_state(ui.painter(), r, State::Busy, now, Some(shown));
                    u::anim::need_ambient(ctx);
                }
                ui.label(RichText::new(txt).strong().color(u::col(tk.text)));
            });
            ui.add_space(u::tokens::SP_XS);
            let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 3.0, u::col(tk.widget));
            ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * shown.clamp(0.0, 1.0), bar.height())), 3.0, u::col(if tally.settled() { tk.ok } else { tk.accent }));
            // octets de la copie en cours (surveillance)
            let v = self.shared.watch_progress.view();
            if v.phase != crate::perf::Phase::Idle && v.bytes_total > 0 {
                ui.label(RichText::new(format!("{} / {}", self.bytes(v.bytes_done), self.bytes(v.bytes_total))).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
            }
        }
        ui.add_space(u::tokens::SP_M);
        // les elements passent un a un a « A l'abri »
        ui.horizontal_wrapped(|ui| {
            let right = ui.max_rect().right();
            for (i, (name, st)) in states.iter().take(16).enumerate() {
                let shown_name = self.model.game(name).map(|g| g.shown().to_string()).unwrap_or_else(|| name.clone());
                // La pastille est dessinee dans un sous-bloc (apparition en cascade) : egui ne connait pas sa largeur
                // avant de la placer, le retour a la ligne est donc decide ici (sinon la carte deborde).
                let font = egui::FontId::proportional(u::tokens::FS_SMALL);
                let mut est = ui.painter().layout_no_wrap(shown_name.clone(), font.clone(), egui::Color32::WHITE).size().x + 18.0 + 20.0 + 2.0 * ui.spacing().item_spacing.x;
                if *st == CopyState::Waiting {
                    est += ui.painter().layout_no_wrap(self.t(K::VFirstCopyWaiting).to_string(), font, egui::Color32::WHITE).size().x + ui.spacing().item_spacing.x;
                }
                if i > 0 && ui.cursor().min.x + est > right {
                    ui.end_row();
                }
                u::cascade_in(ui, ("wiz_copy", i), i, 1, |ui| {
                    egui::Frame::new().fill(u::col(tk.card2)).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let state = match st {
                                CopyState::Safe => State::Safe,
                                CopyState::Busy => State::Busy,
                                CopyState::Queued => State::Paused,
                                CopyState::Waiting => State::Todo,
                            };
                            let age = u::anim::age_of(ctx, egui::Id::new(("wiz_copy_st", name)), state.code());
                            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            u::paint_state(ui.painter(), r, state, if state == State::Busy { now } else { age }, None);
                            ui.label(RichText::new(&shown_name).size(u::tokens::FS_SMALL).color(u::col(tk.text)));
                            if *st == CopyState::Waiting {
                                ui.label(RichText::new(self.t(K::VFirstCopyWaiting)).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
                            }
                        });
                    });
                });
            }
            if states.len() > 16 {
                ui.label(RichText::new(format!("+{}", states.len() - 16)).color(u::col(tk.weak)));
            }
        });
    }

    // ------------------------------------------------------------ diagnostic des captures

    fn wizard_demo(&mut self, ctx: &egui::Context) {
        if self.wizard.demo.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let t0 = *self.wizard.demo_t.get_or_insert(now);
        if now - t0 < 1.5 {
            ctx.request_repaint_after(Duration::from_millis(300));
            return;
        }
        let demo = self.wizard.demo.clone();
        for d in demo {
            if let Some(v) = d.strip_prefix("consent:") {
                self.wizard.demo.retain(|x| x != &d);
                self.set_usage_consent(v == "yes", "diagnostic");
            } else if let Some(i) = d.strip_prefix("vault:").and_then(|n| n.parse::<usize>().ok()) {
                // choisit le coffre existant N (ou, s'il n'y en a pas, le dossier detecte N)
                if self.vault_detect_started() && self.model.settings.onboarding.step == ob::VAULT {
                    if let Some(p) = self.vault_demo_pick(i) {
                        self.wizard.demo.retain(|x| x != &d);
                        let mut acts = super::vault_ui::VaultActs::default();
                        acts.choose = Some(p);
                        self.vault_handle(ctx, acts, false);
                    }
                }
            } else if d == "finish" && self.model.settings.onboarding.step == ob::FINISH {
                // « Voir mes protections » une fois la premiere copie posee depuis 2,5 s
                if self.wizard.copy.as_ref().and_then(|c| c.settled_at).map(|t| now - t > 2.5).unwrap_or(false) {
                    self.wizard.demo.retain(|x| x != &d);
                    self.wizard_nav(ctx, Nav::Continue);
                    return;
                }
            } else if d == "scan-add" && self.model.settings.onboarding.step == ob::SCAN {
                let (started, finished, adding, report, sel) = self.scan_wizard_status();
                if started && finished && !adding && report.is_none() && sel > 0 && now - t0 > 4.0 {
                    self.wizard.demo.retain(|x| x != &d);
                    self.wizard.advance_after_add = true;
                    self.scan_wizard_protect(ctx);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(300));
    }
}

/// Nom court d'une etape (« Quoi de neuf » : ce qui reste a regler).
fn step_name(s: u8) -> K {
    match s {
        1 => K::VWizName1,
        2 => K::VWizName2,
        3 => K::VWizName3,
        4 => K::VWizName4,
        5 => K::VWizName5,
        _ => K::VWizName6,
    }
}

/// Bordure en tirets (zone de depot).
fn dashed_rect(p: &egui::Painter, r: egui::Rect, st: egui::Stroke) {
    let (dash, gap) = (8.0, 6.0);
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        let dir = (b - a) / len.max(1.0);
        let mut t = 0.0;
        while t < len {
            let e = (t + dash).min(len);
            p.line_segment([a + dir * t, a + dir * e], st);
            t += dash + gap;
        }
    }
}

/// Petit eclat d'accent autour de la coche finale (8 points qui s'eloignent et s'effacent).
fn paint_burst(p: &egui::Painter, c: egui::Pos2, radius: f32, age: f64, reduced: bool, col: Color32) {
    if reduced {
        return;
    }
    let d = u::tokens::DUR_BURST;
    if age >= d {
        return;
    }
    let pr = (age / d) as f32;
    for i in 0..8 {
        let (dx, dy, a) = u::anim::burst(i, 8, pr, radius);
        p.circle_filled(egui::pos2(c.x + dx, c.y + dy), 3.0, col.gamma_multiply(a));
    }
}
