//! Page « Reglages » : partie TECHNIQUE (mode expert). Les six cartes du mode simple et la page elle-meme sont dans
//! `settings_simple.rs` ; ici vivent les options expertes qui s'ajoutent a ces cartes et les cartes reservees au mode
//! expert (conflits et notifications, surveillance, configuration, journal). Aucune option 1.x n'est perdue.
use super::*;
use crate::gui::ui::{self as u};
use crate::settings::ConflictMode;
use crate::setmode::Card;
use crate::usage;

impl App {
    // ------------------------------------------------------------ champ + boutons (adaptatif)

    /// Champ de texte pleine largeur + boutons ; en fenetre etroite les boutons passent SOUS le champ.
    /// Retourne l'indice du bouton clique.
    pub(super) fn field_row(&self, ui: &mut egui::Ui, text: &mut String, tip: K, btns: &[(K, K)]) -> Option<usize> {
        let mut clicked = None;
        let h = ui.spacing().interact_size.y;
        let narrow = ui.available_width() < 470.0;
        if narrow {
            let r = ui.add_sized([ui.available_width().max(60.0), h], egui::TextEdit::singleline(text));
            self.deco_kind(ui, r, Some(tip), ItemKind::Text);
            ui.horizontal_wrapped(|ui| {
                for (i, (label, btip)) in btns.iter().enumerate() {
                    let b = ui.button(self.t(*label));
                    if self.deco(ui, b, Some(*btip)).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (i, (label, btip)) in btns.iter().enumerate().rev() {
                        let b = ui.button(self.t(*label));
                        if self.deco(ui, b, Some(*btip)).clicked() {
                            clicked = Some(i);
                        }
                    }
                    let r = ui.add_sized([ui.available_width().max(60.0), h], egui::TextEdit::singleline(text));
                    self.deco_kind(ui, r, Some(tip), ItemKind::Text);
                });
            });
        }
        clicked
    }

    // ------------------------------------------------------------ mode expert : recherche, cartes, options techniques

    /// Petit titre qui ouvre les options expertes d'une carte du mode simple.
    fn expert_block_expert(&self, ui: &mut egui::Ui) {
        let tk = u::tokens(ui.ctx());
        ui.add_space(u::tokens::SP_S);
        ui.separator();
        ui.label(RichText::new(self.t(K::SetExpertInside)).size(u::tokens::FS_SMALL + 1.0).strong().color(u::col(tk.weak)));
        ui.add_space(u::tokens::SP_XS);
    }

    /// Champ de recherche des Reglages (mode expert) ; retourne la requete.
    pub(super) fn search_expert(&mut self, ui: &mut egui::Ui) -> String {
        let mut q = std::mem::take(&mut self.set_ui.search);
        let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text(self.t(K::SetSearch)).desired_width(f32::INFINITY));
        self.deco_kind(ui, r, None, ItemKind::Text);
        ui.add_space(u::tokens::SP_M);
        self.set_ui.search = q.clone();
        q
    }

    /// Titre du groupe des cartes reservees au mode expert.
    pub(super) fn expert_group_header_expert(&self, ui: &mut egui::Ui) {
        let tk = u::tokens(ui.ctx());
        ui.label(RichText::new(self.t(K::SetExpertGroup)).size(u::tokens::FS_H3).strong().color(u::col(tk.weak)));
        ui.add_space(u::tokens::SP_S);
    }

    /// Aucune carte ne correspond a la recherche.
    pub(super) fn no_match_expert(&self, ui: &mut egui::Ui) {
        let tk = u::tokens(ui.ctx());
        ui.label(RichText::new(self.t(K::SetNoMatch)).color(u::col(tk.weak)));
    }

    /// Carte reservee au mode expert : repliable (repliee par defaut), depliee d'office pendant une recherche.
    pub(super) fn expert_card_expert(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, card: Card, force_open: bool) {
        let idx = card.expert_index().unwrap_or(0);
        let title = self.t(card.title());
        u::card(ui, None, |ui| {
            let open = self.set_ui.expert_open[idx] || force_open;
            if self.chevron_header_sized(ui, open, title, if self.big_now { 19.0 } else { 15.0 }) {
                self.set_ui.expert_open[idx] = !self.set_ui.expert_open[idx];
            }
            if self.set_ui.expert_open[idx] || force_open {
                match card {
                    Card::Conflicts => self.conflict_section(ui),
                    Card::Watch => self.watch_card(ui),
                    Card::Config => self.config_card(ui),
                    Card::Log => self.log_section(ui),
                    _ => {}
                }
            }
        });
        let _ = ctx;
    }

    /// Langue et apparence, options techniques : couleur d'accent libre et retour a l'apparence d'origine.
    pub(super) fn look_expert(&mut self, ui: &mut egui::Ui) {
        self.expert_block_expert(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(self.t(K::SetAccent));
            let mut c = self.model.settings.accent;
            let r = ui.color_edit_button_srgb(&mut c);
            if self.deco(ui, r, Some(K::TipAccentCustom)).changed() {
                self.model.settings.accent = c;
                let _ = self.model.save_settings();
            }
        });
        let r = ui.button(self.t(K::BtnRestoreAppearance));
        if self.deco(ui, r, Some(K::TipRestoreAppearance)).clicked() {
            self.model.request(Confirm::RestoreAppearance);
        }
    }

    /// Coffre, options techniques : chemin saisi a la main, Parcourir, Appliquer, taille, etat.
    pub(super) fn dest_card_expert(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.expert_block_expert(ui);
        let mut s = std::mem::take(&mut self.dest_text);
        let clicked = self.field_row(ui, &mut s, K::TipDest, &[(K::BtnBrowse, K::TipBrowse), (K::BtnApply, K::TipApplyDest)]);
        self.dest_text = s;
        match clicked {
            Some(0) => {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.dest_text = p.display().to_string();
                }
            }
            Some(_) => {
                let d = if self.dest_text.trim().is_empty() { None } else { Some(PathBuf::from(self.dest_text.trim())) };
                match self.model.set_dest(d) {
                    Ok(()) => {
                        self.shared.restart_watch();
                        self.dest_size = None;
                        self.dest_ok = None;
                        self.dest_probe_at = Instant::now() - Duration::from_secs(60);
                        self.say(self.t(K::MsgSaved).to_string(), true)
                    }
                    Err(e) => self.say(e.to_string(), false),
                }
            }
            None => {}
        }
        ui.horizontal_wrapped(|ui| {
            let r = ui.button(self.t(K::SetDestSize));
            if self.deco(ui, r, Some(K::TipDestSize)).clicked() {
                if let Some(d) = self.model.dest() {
                    let (tx, c) = (self.tx.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(Msg::DestSize(actions::dir_size(&d)));
                        c.request_repaint();
                    });
                }
            }
            if let Some(s) = self.dest_size {
                ui.label(self.bytes(s));
            }
            let (txt, col) = match (self.model.dest().is_some(), self.dest_ok) {
                (false, _) => (self.t(K::DestNone), Color32::GRAY),
                (true, Some(true)) => (self.t(K::DestConnected), Color32::from_rgb(60, 180, 100)),
                (true, Some(false)) => (self.t(K::DestUnreachable), Color32::from_rgb(230, 170, 40)),
                (true, None) => (self.t(K::DestConnected), Color32::GRAY),
            };
            let r = ui.label(RichText::new(txt).color(col).strong());
            self.tip(r, K::TipDestState, false);
        });
        // Alerte poids (2.0.1) : seuil unique, en Mo (500 par defaut).
        ui.horizontal(|ui| {
            let r = ui.label(self.t(K::VHeavyLimit));
            self.deco(ui, r, Some(K::VHeavyLimitTip));
            let mut mb = self.model.settings.heavy_limit_mb;
            let r = ui.add(egui::DragValue::new(&mut mb).range(crate::weight::MIN_LIMIT_MB..=crate::weight::MAX_LIMIT_MB).speed(10.0));
            let r = self.deco_kind(ui, r, Some(K::VHeavyLimitTip), ItemKind::Text);
            if r.changed() {
                self.model.settings.heavy_limit_mb = crate::weight::clamp_limit(mb);
                crate::weight::set_limit_mb(self.model.settings.heavy_limit_mb);
                let _ = self.model.save_settings();
                self.last_status = Instant::now() - Duration::from_secs(60);
                self.home.epoch += 1;
            }
        });
    }

    /// Protection automatique, options techniques : portee de la detection, elements ignores, demarrage en arriere-plan et
    /// regles de demarrage detaillees (Windows).
    pub(super) fn protect_expert(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.expert_block_expert(ui);
        self.detect_expert(ui);
        self.startup_expert(ui, ctx);
    }

    /// Donnees d'usage, options techniques : identifiant d'installation.
    pub(super) fn usage_expert(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = usage::current_id(&self.shared.data_dir) {
            self.expert_block_expert(ui);
            ui.horizontal_wrapped(|ui| {
                let r = ui.label(RichText::new(self.t(K::UsageId)).small());
                self.deco(ui, r, Some(K::TipUsageId));
                let mut t = id.as_str();
                let r = ui.add(egui::TextEdit::singleline(&mut t).font(egui::TextStyle::Monospace).desired_width(300.0));
                self.deco_kind(ui, r, Some(K::TipUsageId), ItemKind::Text);
            });
        }
    }


    fn watch_card(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let running = self.shared.state() == WatchState::Running;
            let r = ui.add_enabled(!running, egui::Button::new(self.t(K::WatchStart)));
            if self.deco(ui, r, Some(K::TipWatchStart)).clicked() {
                self.shared.start_watch();
            }
            let r = ui.add_enabled(running, egui::Button::new(self.t(K::WatchStop)));
            if self.deco(ui, r, Some(K::TipWatchStop)).clicked() {
                self.shared.stop_watch_async();
            }
            let paused = self.shared.paused.load(Ordering::SeqCst);
            let r = ui.add_enabled(running, egui::Button::new(if paused { self.t(K::WatchResume) } else { self.t(K::WatchPause) }));
            if self.deco(ui, r, Some(K::TipWatchPause)).clicked() {
                self.shared.paused.store(!paused, Ordering::SeqCst);
            }
            let r = ui.add_enabled(running, egui::Button::new(self.t(K::SyncNow)));
            if self.deco(ui, r, Some(K::TipSyncNow)).clicked() {
                self.request_sync(None);
            }
        });
        let mut aw = self.model.settings.auto_watch;
        if self.toggle(ui, &mut aw, self.t(K::SetAutoWatch), K::TipAutoWatch).changed() {
            self.model.settings.auto_watch = aw;
            let _ = self.model.save_settings();
        }
        self.react_section(ui);
        // mode grand format : Auto (manette branchee) / Toujours / Jamais
        ui.label(self.t(K::SetBig));
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (ch, k) in [(BigChoice::Auto, K::SetBigAuto), (BigChoice::Always, K::SetBigAlways), (BigChoice::Never, K::SetBigNever)] {
                let r = ui.selectable_label(self.model.settings.big_choice() == ch, self.t(k));
                if self.deco(ui, r, Some(K::TipBig)).clicked() {
                    self.model.settings.set_big_choice(ch);
                    changed = true;
                }
            }
        });
        if changed {
            let _ = self.model.save_settings();
        }
        // Linux / SteamOS : la manette passe par Steam Input (clavier + souris) ; aucune lecture directe, donc rien a regler.
        if cfg!(windows) {
            self.pad_mode_row(ui);
        }
    }

    /// Mode manette : Me demander / Toujours activee / Desactivee (Windows : lecture directe XInput / HID).
    fn pad_mode_row(&mut self, ui: &mut egui::Ui) {
        ui.label(self.t(K::SetPadMode));
        let mut pad_changed = false;
        ui.horizontal_wrapped(|ui| {
            for (m, k) in [(crate::settings::PadMode::Ask, K::PadModeAsk), (crate::settings::PadMode::Always, K::PadModeAlways), (crate::settings::PadMode::Disabled, K::PadModeOff)] {
                let r = ui.selectable_label(self.model.settings.pad_mode == m, self.t(k));
                if self.deco(ui, r, Some(K::TipPadMode)).clicked() && self.model.settings.pad_mode != m {
                    self.model.settings.pad_mode = m;
                    pad_changed = true;
                }
            }
        });
        if pad_changed {
            self.pad.shared.set_session_yes(false);
            self.pad.shared.set_mode(self.model.settings.pad_mode);
            let _ = self.model.save_settings();
        }
    }

    /// « Synchroniser maintenant » : meme moteur que la surveillance (file de demandes lue par le Watcher).
    pub(super) fn request_sync(&mut self, game: Option<String>) {
        if self.shared.state() != WatchState::Running {
            self.say(self.t(K::SyncNeedsWatch).to_string(), false);
            return;
        }
        if let Ok(mut q) = self.shared.sync_requests.lock() {
            q.push(game);
        }
        self.say(self.t(K::SyncRequested).to_string(), true);
    }

    /// Reglage « Reactivite » : Instantane / Normal / Connexion lente / Personnalise (+ champs).
    fn react_section(&mut self, ui: &mut egui::Ui) {
        use crate::reactive::Reactivity;
        ui.label(self.t(K::SetReact));
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (m, k) in [(Reactivity::Instant, K::ReactInstant), (Reactivity::Normal, K::ReactNormal), (Reactivity::Slow, K::ReactSlow), (Reactivity::Custom, K::ReactCustom)] {
                let r = ui.selectable_label(self.model.settings.reactivity == m, self.t(k));
                if self.deco(ui, r, Some(K::TipReact)).clicked() && self.model.settings.reactivity != m {
                    self.model.settings.reactivity = m;
                    let v = match m {
                        Reactivity::Instant => "instant",
                        Reactivity::Normal => "normal",
                        Reactivity::Slow => "slow",
                        Reactivity::Custom => "custom",
                    };
                    crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "reactivity"), ("value", v)]);
                    changed = true;
                }
            }
        });
        if self.model.settings.reactivity == Reactivity::Custom {
            let mut c = self.model.settings.reactivity_custom;
            let before = c;
            for (label, tip, val, lo, hi) in [
                (K::ReactSafety, K::TipReactSafety, &mut c.safety_s, 0u32, 600u32),
                (K::ReactRetry, K::TipReactRetry, &mut c.retry_s, 1, 600),
                (K::ReactRetryMax, K::TipReactRetryMax, &mut c.retry_max_s, 1, 3600),
                (K::ReactFallback, K::TipReactFallback, &mut c.fallback_s, 10, 86_400),
                (K::ReactStable, K::TipReactStable, &mut c.stable_s, 1, 600),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(self.t(label));
                    let r = ui.add(egui::DragValue::new(val).range(lo..=hi));
                    self.deco_kind(ui, r, Some(tip), ItemKind::Drag);
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(self.t(K::ReactPoll));
                let mut p = self.model.settings.poll_custom_s;
                let r = ui.add(egui::DragValue::new(&mut p).range(1..=600));
                self.deco_kind(ui, r, Some(K::TipReactPoll), ItemKind::Drag);
                self.model.settings.poll_custom_s = p;
            });
            self.model.settings.reactivity_custom = c;
            let r = ui.button(self.t(K::BtnApply));
            if self.deco(ui, r, Some(K::TipApplyReact)).clicked() {
                changed = true;
            }
            let _ = before;
        }
        if changed {
            let _ = self.model.save_settings();
            self.shared.restart_watch(); // applique sans redemarrer l'application
        }
    }

    fn config_card(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let r = ui.button(self.t(K::BtnExport));
            if self.deco(ui, r, Some(K::TipExport)).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_file_name("autogamesoftwaresync-config.json").add_filter("json", &["json"]).save_file() {
                    match actions::export_config(&self.model.ctx, &p) {
                        Ok(()) => self.say(self.t(K::MsgSaved).to_string(), true),
                        Err(e) => self.say(e.to_string(), false),
                    }
                }
            }
            let r = ui.button(self.t(K::BtnImport));
            if self.deco(ui, r, Some(K::TipImport)).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("json", &["json"]).pick_file() {
                    match actions::read_bundle(&p) {
                        Ok(_) => self.model.request(Confirm::Import { path: p }),
                        Err(e) => self.say(e.to_string(), false),
                    }
                }
            }
        });
        let r = self.icon_button(ui, self.t(K::BtnOpenDataDir), |p, r, c| paint_folder(p, r.shrink(4.0), c));
        if self.deco(ui, r, Some(K::TipOpenDataDir)).clicked() {
            winutil::open_folder(&self.shared.data_dir);
        }
        ui.add_space(4.0);
        let r = ui.button(self.t(K::TrayQuit));
        if self.deco(ui, r, Some(K::TipQuit)).clicked() {
            self.shared.quit();
        }
        // Linux sans icone de zone de notification : fermer la fenetre quitte (le dire, au lieu de promettre une reduction).
        #[cfg(not(windows))]
        let close_hint = if self.has_tray() { K::CloseToTrayHint } else { K::CloseQuitsHint };
        #[cfg(windows)]
        let close_hint = K::CloseToTrayHint;
        ui.label(RichText::new(self.t(close_hint)).small().weak());
        let mut sp = self.model.settings.support_reminders;
        if self.toggle(ui, &mut sp, self.t(K::SetSupport), K::TipSupport).changed() {
            self.model.settings.support_reminders = sp;
            let _ = self.model.save_settings();
        }
    }

    // ------------------------------------------------------------ conflits + notifications

    pub(super) fn conflict_section(&mut self, ui: &mut egui::Ui) {
        ui.label(self.t(K::SetConflict));
        ui.horizontal_wrapped(|ui| {
            for (m, k) in [(ConflictMode::Cloud, K::ConflictCloud), (ConflictMode::Ask, K::ConflictAsk)] {
                let r = ui.selectable_label(self.model.settings.conflict_mode == m, self.t(k));
                if self.deco(ui, r, Some(K::TipConflict)).clicked() && self.model.settings.conflict_mode != m {
                    self.model.settings.conflict_mode = m;
                    let _ = self.model.save_settings();
                    self.shared.restart_watch();
                }
            }
        });
        ui.label(RichText::new(self.t(K::SetConflictHint)).small().weak());
        let mut nc = self.model.settings.notify_conflict_resolved;
        if self.toggle(ui, &mut nc, self.t(K::SetNotifConflict), K::TipNotifConflict).changed() {
            self.model.settings.notify_conflict_resolved = nc;
            let _ = self.model.save_settings();
        }
        let mut nt = self.model.settings.notify_global;
        if self.toggle(ui, &mut nt, self.t(K::SetNotif), K::TipNotif).changed() {
            self.model.settings.notify_global = nt;
            let _ = self.model.save_settings();
        }
    }

    /// Reglage « conflit » d'un jeu (Herite / Cloud auto / Demander), dans sa fiche.
    pub(super) fn game_conflict_row(&mut self, ui: &mut egui::Ui, name: &str) {
        // Choix effectif de ce jeu : le sien s'il en a un, sinon le reglage global (« cloud automatique » par defaut).
        let cur = self.model.settings.conflict_for(name);
        ui.horizontal_wrapped(|ui| {
            ui.label(self.t(K::SetConflictGame));
            for (m, k) in [(ConflictMode::Cloud, K::ConflictCloud), (ConflictMode::Ask, K::ConflictAsk)] {
                let r = ui.selectable_label(cur == m, self.t(k));
                if self.deco(ui, r, Some(K::TipConflictGame)).clicked() && cur != m {
                    self.model.settings.game_mut(name).conflict = Some(m);
                    let _ = self.model.save_settings();
                    self.shared.restart_watch();
                }
            }
        });
    }

    // ------------------------------------------------------------ demarrage : interrupteurs simples + regles avancees

    pub(super) fn load_startup(&mut self, ectx: &egui::Context) {
        let (tx, c, dd) = (self.tx.clone(), ectx.clone(), self.shared.data_dir.clone());
        let custom: Vec<PathBuf> = self.model.settings.startup_rules.iter().filter_map(|r| r.exe.clone()).collect();
        std::thread::spawn(move || {
            let tools = crate::startup::detect_tools(&crate::startup::RealProbe);
            let sched = crate::startup::SchtasksScheduler { tmp_dir: dd.join("tmp") };
            let existing = crate::startup::Scheduler::list(&sched).unwrap_or_default();
            let mut exists: std::collections::HashSet<PathBuf> = tools.iter().map(|t| t.exe.clone()).filter(|p| p.is_file()).collect();
            if let Ok(e) = std::env::current_exe() {
                exists.insert(e);
            }
            exists.extend(custom.into_iter().filter(|p| p.is_file()));
            let legacy = crate::legacy::RUN_VALUES.iter().copied().chain([winutil::RUN_VALUE]).any(|n| winutil::run_entry(n).is_some());
            let _ = tx.send(Msg::Startup { tools, existing, exists, legacy });
            c.request_repaint();
        });
    }

    #[cfg(windows)]
    pub(super) fn set_rule_cfg(&mut self, id: &str, f: impl FnOnce(&mut crate::settings::StartupRuleCfg)) {
        let kind = crate::startup::ToolKind::from_id(id);
        let list = &mut self.model.settings.startup_rules;
        if !list.iter().any(|c| c.id == id) {
            list.push(crate::settings::StartupRuleCfg { id: id.to_string(), name: String::new(), exe: None, delay_secs: crate::startup::default_delay(kind), enabled: false, minimized: true });
        }
        if let Some(c) = list.iter_mut().find(|c| c.id == id) {
            f(c);
        }
        let _ = self.model.save_settings();
    }

    /// Linux : un seul interrupteur (autostart XDG), dans la carte « Protection automatique » : rien de plus en mode expert.
    #[cfg(not(windows))]
    fn startup_expert(&mut self, _ui: &mut egui::Ui, _ctx: &egui::Context) {}

    /// Windows, mode expert : « Demarrer en arriere-plan », etat de la tache, mise a jour des anciennes taches et regles de
    /// demarrage detaillees (replies par defaut). L'interrupteur « Demarrer avec l'ordinateur » est dans la carte simple.
    #[cfg(windows)]
    fn startup_expert(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::startup::{self, RuleStatus};
        let app = self.model.settings.startup_rules.iter().find(|c| c.id == startup::ID_APP).cloned();
        let open = app.as_ref().map(|c| c.enabled).unwrap_or(false);
        let mut bg = app.as_ref().map(|c| c.minimized).unwrap_or(true);
        let changed_bg = ui.add_enabled_ui(open, |ui| self.toggle(ui, &mut bg, self.t(K::StartupBackground), K::TipStartupBackground).changed()).inner;
        if changed_bg {
            self.set_rule_cfg(startup::ID_APP, |c| c.minimized = bg);
            self.model.request(Confirm::ApplyStartup);
        }
        let exe = std::env::current_exe().unwrap_or_default();
        let exists = self.startup_exists.clone();
        let rules = startup::build_rules(&self.startup_tools, &self.model.settings.startup_rules, &exe, &|p: &std::path::Path| exists.contains(p));
        if let Some(r) = rules.iter().find(|r| r.id == startup::ID_APP) {
            let (txt, col) = match startup::rule_status(r, &self.startup_existing) {
                RuleStatus::Active => (self.t(K::StartupActive), Color32::from_rgb(60, 180, 100)),
                RuleStatus::Inactive => (self.t(K::StartupInactive), Color32::GRAY),
                RuleStatus::NotFound => (self.t(K::StartupNotFound), Color32::from_rgb(220, 80, 80)),
            };
            ui.label(RichText::new(txt).color(col).small());
        }
        if self.shared.task_migration_pending.load(std::sync::atomic::Ordering::SeqCst) {
            ui.label(RichText::new(self.t(K::StartupMigratePending)).color(Color32::from_rgb(230, 170, 40)));
            let b = ui.button(self.t(K::BtnStartupMigrate));
            if self.deco(ui, b, Some(K::TipStartupMigrate)).clicked() {
                self.model.request(Confirm::MigrateTasks);
            }
        }
        let r = ui.add(egui::Button::new(self.t(K::BtnAdvancedStartup)).selected(self.adv_startup));
        if self.deco(ui, r, Some(K::TipAdvancedStartup)).clicked() {
            self.adv_startup = !self.adv_startup;
        }
        if self.adv_startup {
            ui.add_space(6.0);
            self.startup_advanced(ui, ctx, &rules);
        }
    }

    /// Regles detaillees (delais, « Demarrer reduit », outils cloud, test, ajout de programme, application).
    #[cfg(windows)]
    fn startup_advanced(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, rules: &[crate::startup::Rule]) {
        use crate::startup::{self, RuleStatus};
        let mut test: Option<String> = None;
        let mut remove: Option<String> = None;
        for r in rules {
            let status = startup::rule_status(r, &self.startup_existing);
            ui.separator();
            let is_app = r.kind == startup::ToolKind::App;
            let mut en = r.enabled;
            if self.toggle(ui, &mut en, r.name.as_str(), K::TipStartupToggle).changed() {
                let id = r.id.clone();
                self.set_rule_cfg(&id, |c| c.enabled = en);
            }
            ui.horizontal_wrapped(|ui| {
                let (txt, col) = match status {
                    RuleStatus::Active => (self.t(K::StartupActive), Color32::from_rgb(60, 180, 100)),
                    RuleStatus::Inactive => (self.t(K::StartupInactive), Color32::GRAY),
                    RuleStatus::NotFound => (self.t(K::StartupNotFound), Color32::from_rgb(220, 80, 80)),
                };
                ui.label(RichText::new(txt).color(col));
                ui.label(self.t(K::StartupDelay));
                let mut d = r.delay_secs;
                let dv = ui.add(egui::DragValue::new(&mut d).range(0..=600));
                if self.deco_kind(ui, dv, Some(K::TipStartupDelay), ItemKind::Drag).changed() {
                    let id = r.id.clone();
                    self.set_rule_cfg(&id, |c| c.delay_secs = d);
                }
                let b = ui.add_enabled(status == RuleStatus::Active, egui::Button::new(self.t(K::BtnTestNow)));
                if self.deco(ui, b, Some(K::TipTestNow)).clicked() {
                    test = Some(r.id.clone());
                }
                if r.kind == startup::ToolKind::Custom {
                    let b = ui.button(self.t(K::BtnRemoveRule));
                    if self.deco(ui, b, Some(K::TipRemoveRule)).clicked() {
                        remove = Some(r.id.clone());
                    }
                }
            });
            // « Demarrer reduit » (outils) / « Demarrer en arriere-plan » (application) : meme reglage que les interrupteurs simples.
            let (mut mn, label, tip) = if is_app {
                let v = self.model.settings.startup_rules.iter().find(|c| c.id == r.id).map(|c| c.minimized).unwrap_or(true);
                (v, K::StartupBackground, K::TipStartupBackground)
            } else {
                (r.minimized, K::StartupMinimized, K::TipStartupMinimized)
            };
            if self.toggle(ui, &mut mn, self.t(label), tip).changed() {
                let id = r.id.clone();
                self.set_rule_cfg(&id, |c| c.minimized = mn);
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let b = ui.button(self.t(K::BtnAddProgram));
            if self.deco(ui, b, Some(K::TipAddProgram)).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("exe", &["exe"]).pick_file() {
                    let id = startup::next_custom_id(&self.model.settings.startup_rules);
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    self.model.settings.startup_rules.push(crate::settings::StartupRuleCfg { id, name, exe: Some(p), delay_secs: startup::DELAY_CLOUD_SECS, enabled: false, minimized: true });
                    let _ = self.model.save_settings();
                    self.load_startup(ctx);
                }
            }
            let b = ui.button(self.t(K::BtnApplyStartup));
            if self.deco(ui, b, Some(K::TipApplyStartup)).clicked() {
                self.model.request(Confirm::ApplyStartup);
            }
        });
        if self.legacy_run {
            ui.label(RichText::new(self.t(K::LegacyRunFound)).color(Color32::from_rgb(230, 170, 40)));
            let b = ui.button(self.t(K::BtnMigrateRun));
            if self.deco(ui, b, Some(K::TipMigrateRun)).clicked() {
                self.model.request(Confirm::MigrateRun);
            }
        }
        if let Some(id) = remove {
            self.model.settings.startup_rules.retain(|c| c.id != id);
            let _ = self.model.save_settings();
            self.load_startup(ctx);
        }
        if let Some(id) = test {
            let (tx, c, dd, lang) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), self.model.settings.lang);
            std::thread::spawn(move || {
                let sched = startup::SchtasksScheduler { tmp_dir: dd.join("tmp") };
                let r = startup::Scheduler::run(&sched, &startup::task_name(&id));
                let (text, ok) = match r {
                    Ok(()) => (format!("{} {}", tr(lang, K::MsgDone), startup::task_name(&id)), true),
                    Err(e) => (format!("{} {}", tr(lang, K::MsgFailed), e), false),
                };
                let _ = tx.send(Msg::Done { text, ok });
                c.request_repaint();
            });
        }
    }

    // ------------------------------------------------------------ journal (section repliable)

    fn log_section(&mut self, ui: &mut egui::Ui) {
        if !self.log_open {
            self.log_open = true; // la carte est depliee : le journal est charge
            self.last_log = Instant::now() - Duration::from_secs(60);
        }
        if self.last_log.elapsed() > Duration::from_secs(3) {
            self.log_entries = vm::read_log(&self.shared.data_dir, 2000);
            // Noms AFFICHES (le fichier journal garde les noms d origine).
            let map: std::collections::HashMap<String, String> = self.model.config.games.iter().filter(|g| g.shown() != g.name).map(|g| (g.name.clone(), g.shown().to_string())).collect();
            for e in self.log_entries.iter_mut() {
                if let Some(s) = e.game.as_ref().and_then(|g| map.get(g)) {
                    e.game = Some(s.clone());
                }
            }
            self.last_log = Instant::now();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(self.t(K::LogFilterLevel));
            let names = [self.t(K::LevelAll), self.t(K::LevelWarn), self.t(K::LevelError)];
            let cb = egui::ComboBox::from_id_salt("lvl").selected_text(names[self.log_level_idx]).show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut self.log_level_idx, i, *n);
                }
            });
            self.deco(ui, cb.response, Some(K::TipLogLevel));
            self.log_filter.min_level = match self.log_level_idx {
                1 => Some(Level::Warn),
                2 => Some(Level::Error),
                _ => None,
            };
            ui.label(self.t(K::LogFilterGame));
            let all_txt = self.t(K::LevelAll);
            let cur = self.log_filter.game.clone().unwrap_or_else(|| all_txt.to_string());
            let cb = egui::ComboBox::from_id_salt("lg").selected_text(cur).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.log_filter.game, None, all_txt);
                for g in &self.model.config.games {
                    ui.selectable_value(&mut self.log_filter.game, Some(g.shown().to_string()), g.shown());
                }
            });
            self.deco(ui, cb.response, Some(K::TipLogGame));
            ui.label(self.t(K::LogFilterText));
            let r = ui.add(egui::TextEdit::singleline(&mut self.log_filter.text).desired_width(140.0));
            self.deco_kind(ui, r, Some(K::TipLogText), ItemKind::Text);
            let r = crate::gui::icon_button(ui, crate::gui::Icon::Refresh, self.t(K::BtnRefresh));
            if self.deco(ui, r, Some(K::TipRefreshLog)).clicked() {
                self.last_log = Instant::now() - Duration::from_secs(60);
            }
            let r = ui.button(self.t(if self.log_big { K::BtnLogShrink } else { K::BtnLogEnlarge }));
            if self.deco(ui, r, Some(K::TipLogEnlarge)).clicked() {
                self.log_big = !self.log_big;
            }
            let copied = self.log_copied.map(|t| t.elapsed() < Duration::from_millis(1800)).unwrap_or(false);
            let r = ui.button(self.t(K::BtnLogCopy));
            if self.deco(ui, r, Some(K::TipLogCopy)).clicked() {
                let text = vm::log_copy_text(&self.log_entries, &self.log_filter, self.model.settings.lang, &crate::timefmt::WinTz);
                ui.ctx().copy_text(text);
                self.log_copied = Some(Instant::now());
            }
            if copied {
                crate::gui::icon_label(ui, crate::gui::Icon::Check, self.t(K::LogCopied), Color32::from_rgb(60, 180, 100));
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
        });
        ui.separator();
        let shown = vm::filter_log(&self.log_entries, &self.log_filter);
        let area_h = if self.log_big {
            (ui.clip_rect().bottom() - ui.cursor().top() - 28.0).max(crate::settings::LOG_HEIGHT_MIN)
        } else {
            self.model.settings.log_height
        };
        egui::ScrollArea::vertical().id_salt("logs").max_height(area_h).min_scrolled_height(area_h).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for e in shown {
                let col = match e.level {
                    Level::Info => ui.visuals().text_color(),
                    Level::Warn => Color32::from_rgb(230, 170, 40),
                    Level::Error => Color32::from_rgb(220, 80, 80),
                };
                let line = format!("{}  {}{}", crate::timefmt::log_ts_label(&e.ts, self.model.settings.lang, &crate::timefmt::WinTz), e.game.as_ref().map(|g| format!("[{}] ", g)).unwrap_or_default(), e.text);
                ui.label(self.log_job(&line, col));
            }
        });
        if !self.log_big {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::drag());
            let resp = resp.on_hover_text(self.t(K::TipLogResize)).on_hover_cursor(egui::CursorIcon::ResizeVertical);
            let col = if resp.hovered() || resp.dragged() { ui.visuals().widgets.hovered.fg_stroke.color } else { ui.visuals().weak_text_color() };
            let c = rect.center();
            for dx in [-14.0, 0.0, 14.0] {
                ui.painter().circle_filled(egui::pos2(c.x + dx, c.y), 2.0, col);
            }
            if resp.dragged() {
                let h = (self.model.settings.log_height + resp.drag_delta().y).clamp(crate::settings::LOG_HEIGHT_MIN, crate::settings::LOG_HEIGHT_MAX);
                self.model.settings.log_height = h;
            }
            if resp.drag_stopped() {
                let _ = self.model.save_settings();
            }
        }
    }
}

impl App {
    /// Ligne du journal avec le nom de chaque PC connu colore (fond = couleur de la machine).
    fn log_job(&self, line: &str, base: Color32) -> egui::text::LayoutJob {
        use egui::text::{LayoutJob, TextFormat};
        let font = egui::TextStyle::Monospace.resolve(&egui::Style::default());
        let plain = TextFormat { font_id: font.clone(), color: base, ..Default::default() };
        let mut job = LayoutJob::default();
        let mut rest = line;
        while !rest.is_empty() {
            // Plus proche occurrence d'un nom de machine connu.
            let hit = self
                .machine_map
                .keys()
                .filter(|n| n.chars().count() >= 3)
                .filter_map(|n| rest.find(n.as_str()).map(|i| (i, n)))
                .filter(|(i, n)| {
                    let before_ok = rest[..*i].chars().last().map(|c| !c.is_alphanumeric()).unwrap_or(true);
                    let after_ok = rest[*i + n.len()..].chars().next().map(|c| !c.is_alphanumeric()).unwrap_or(true);
                    before_ok && after_ok
                })
                .min_by_key(|(i, _)| *i);
            match hit {
                Some((i, n)) => {
                    job.append(&rest[..i], 0.0, plain.clone());
                    let c = self.machine_rgb(n);
                    let t = crate::machinecolor::text_on(c);
                    job.append(n, 0.0, TextFormat { font_id: font.clone(), color: Color32::from_rgb(t[0], t[1], t[2]), background: Color32::from_rgb(c[0], c[1], c[2]), ..Default::default() });
                    rest = &rest[i + n.len()..];
                }
                None => {
                    job.append(rest, 0.0, plain.clone());
                    break;
                }
            }
        }
        job
    }
}
