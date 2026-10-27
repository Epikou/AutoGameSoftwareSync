//! Page « Reglages » 2.0 (lot 11) : MODE SIMPLE par defaut (six cartes claires), mode expert (interrupteur discret en bas)
//! qui garde TOUTES les options 1.x. Un seul « Mode expert » pour toute l'application (`settings.expert_mode`).
//! Ce fichier ne contient QUE le vocabulaire du mode simple (verifie par test : aucun « exe », aucun « cloud ») ; tout ce
//! qui est technique vit dans des fonctions `*_expert` de `settings.rs`, `detect_ui.rs` et `usage_ui.rs`.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph};
use crate::selfupdate;
use crate::setmode::{self, Card};
use crate::usage;

/// Etat de la page Reglages entre deux images.
#[derive(Default)]
pub struct SetUi {
    /// Cartes expertes depliees (Conflits, Surveillance, Configuration, Journal).
    pub expert_open: [bool; 4],
    /// Recherche (mode expert).
    pub search: String,
    /// Cle d'apparition des cartes expertes (rejouee quand le mode expert s'allume).
    pub epoch: u64,
    seen_frame: u64,
    /// Carte a montrer (defilement), pour les captures et « Choisir un dossier partage ».
    pub scroll_to: Option<Card>,
}

impl App {
    /// Texte secondaire sous un titre de bloc.
    fn set_label(&self, ui: &mut egui::Ui, text: &str) {
        let tk = u::tokens(ui.ctx());
        ui.add_space(u::tokens::SP_XS);
        ui.label(RichText::new(text).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.weak)));
    }

    /// Petit texte discret (explication d'un interrupteur).
    fn set_hint(&self, ui: &mut egui::Ui, text: &str) {
        let tk = u::tokens(ui.ctx());
        ui.label(RichText::new(text).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
    }

    /// Rangee de choix en pastilles ; retourne le choix clique.
    fn set_chips<T: Copy + PartialEq>(&self, ui: &mut egui::Ui, items: &[(T, K)], cur: T, tip: K) -> Option<T> {
        let mut out = None;
        ui.horizontal_wrapped(|ui| {
            for (v, k) in items {
                let r = u::chip(ui, self.t(*k), None, cur == *v);
                if self.deco(ui, r, Some(tip)).clicked() {
                    out = Some(*v);
                }
            }
        });
        out
    }

    /// Interrupteur du mode simple, enregistre dans la selection manette.
    fn set_toggle(&self, ui: &mut egui::Ui, on: &mut bool, label: &str, tip: Option<K>) -> bool {
        let r = u::toggle(ui, on, label);
        self.deco(ui, r, tip).changed()
    }

    // ------------------------------------------------------------ page

    pub(super) fn page_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Changement d'onglet : cartes expertes repliees, recherche vide, apparition rejouee.
        let frame = ctx.cumulative_pass_nr();
        if self.set_ui.seen_frame == 0 || frame > self.set_ui.seen_frame + 2 {
            self.set_ui.expert_open = if self.demo_open_all { [true; 4] } else { [false; 4] };
            self.set_ui.search.clear();
            self.set_ui.epoch += 1;
        }
        self.set_ui.seen_frame = frame;
        // Diagnostics (captures) et « Choisir un dossier partage » : carte a montrer.
        if self.demo_usage_card {
            self.set_ui.scroll_to = Some(Card::Usage);
        }
        if self.vault.demo.iter().any(|d| d == "card") || self.scroll_dest {
            self.set_ui.scroll_to = Some(Card::Vault);
        }
        let expert = self.model.settings.expert_mode;
        let mut area = egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]);
        if let Some(px) = super::shot::scroll_offset() {
            area = area.vertical_scroll_offset(px);
        }
        area.show(ui, |ui| {
            // Reserve la place de la barre de defilement.
            ui.set_max_width((ui.available_width() - 14.0).max(160.0));
            let tk = u::tokens(ctx);
            // En-tete : version et accroche, discretes ; puis « Refaire la configuration ».
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("AutoGameSoftwareSync v{}", crate::update::current_version())).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
                ui.label(RichText::new(format!("{} {}", self.t(K::VTagline), self.t(K::VTaglineSub))).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
                ui.add_space(u::tokens::SP_S);
                let r = u::button_with(ui, Btn::Secondary, self.t(K::VRedoSetup), Some(Glyph::Star), false);
                if self.deco(ui, r, Some(K::TipRedoSetup)).clicked() {
                    self.wizard_restart(ctx);
                }
            });
            ui.add_space(u::tokens::SP_M);
            // Recherche (mode expert seulement).
            let query = if expert { self.search_expert(ui) } else { String::new() };
            let shown = setmode::matching_cards(self.model.settings.lang, expert, &query);
            let mut n_expert = 0;
            for (i, card) in shown.iter().enumerate() {
                if card.expert_only() {
                    if n_expert == 0 {
                        self.expert_group_header_expert(ui);
                    }
                    let idx = n_expert;
                    n_expert += 1;
                    let force = !query.trim().is_empty();
                    let epoch = self.set_ui.epoch;
                    u::cascade_in(ui, ("setexp", idx), idx, epoch, |ui| self.expert_card_expert(ui, ctx, *card, force));
                } else {
                    self.simple_card(ui, ctx, *card, i);
                }
                ui.add_space(u::tokens::SP_M);
            }
            if shown.is_empty() {
                self.no_match_expert(ui);
            }
            // Mode expert : interrupteur discret en bas, le meme partout dans l'application.
            ui.add_space(u::tokens::SP_S);
            let mut ex = expert;
            if self.set_toggle(ui, &mut ex, self.t(K::VExpertMode), None) {
                self.model.settings.expert_mode = ex;
                let _ = self.model.save_settings();
                crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "expert_mode"), ("value", if ex { "on" } else { "off" })]);
                self.pad_log(&format!("[Reglages] mode expert {}", if ex { "active" } else { "desactive" }));
                self.set_ui.epoch += 1;
                self.set_ui.search.clear();
                ctx.request_repaint();
            }
            self.set_hint(ui, self.t(K::VSetExpertHint));
            ui.add_space(u::tokens::SP_L);
        });
        self.set_ui.scroll_to = None;
    }

    /// Une des six cartes du mode simple (en mode expert, elles gardent leur contenu et gagnent leurs options techniques).
    fn simple_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, card: Card, _index: usize) {
        let title = self.tm(card.title());
        let r = u::card(ui, Some(title), |ui| match card {
            Card::Look => self.look_section(ui),
            Card::Vault => self.vault_section(ui, ctx),
            Card::Devices => self.devices_section(ui),
            Card::Protect => self.protect_section(ui, ctx),
            Card::Updates => self.updates_section(ui),
            Card::Usage => self.usage_section(ui),
            _ => {}
        });
        if self.set_ui.scroll_to == Some(card) {
            ui.scroll_to_rect(r.response.rect, Some(egui::Align::Center));
        }
    }

    // ------------------------------------------------------------ 1. langue et apparence

    fn look_section(&mut self, ui: &mut egui::Ui) {
        self.set_label(ui, self.t(K::SetLang));
        let mut chosen: Option<Lang> = None;
        ui.horizontal_wrapped(|ui| {
            for l in Lang::ALL {
                if self.lang_button(ui, l, self.model.settings.lang == l).clicked() {
                    chosen = Some(l);
                }
            }
        });
        if let Some(l) = chosen {
            if self.model.settings.lang != l {
                self.model.settings.lang = l;
                let _ = self.model.save_settings();
            }
        }
        ui.add_space(u::tokens::SP_S);
        self.set_label(ui, self.t(K::SetTheme));
        let cur = self.model.settings.theme;
        if let Some(m) = self.set_chips(ui, &[(ThemeMode::System, K::ThemeSystem), (ThemeMode::Dark, K::ThemeDark), (ThemeMode::Light, K::ThemeLight)], cur, K::TipTheme) {
            if m != cur {
                let v = match m {
                    ThemeMode::System => "system",
                    ThemeMode::Dark => "dark",
                    ThemeMode::Light => "light",
                };
                crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "theme"), ("value", v)]);
                self.model.settings.theme = m;
                let _ = self.model.save_settings();
            }
        }
        ui.add_space(u::tokens::SP_S);
        self.set_label(ui, self.t(K::SetAccent));
        let tk = u::tokens(ui.ctx());
        let mut accent: Option<[u8; 3]> = None;
        ui.horizontal_wrapped(|ui| {
            let presets: Vec<[u8; 3]> = std::iter::once(crate::settings::DEFAULT_ACCENT).chain(vm::ACCENT_PRESETS).collect();
            for a in presets {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
                ui.painter().circle_filled(r.center(), 14.0, Color32::from_rgb(a[0], a[1], a[2]));
                if self.model.settings.accent == a {
                    ui.painter().circle_stroke(r.center(), 16.5, egui::Stroke::new(2.0_f32, u::col(tk.text)));
                    u::paint_glyph(ui.painter(), r.shrink(8.0), Glyph::Check, u::col(crate::theme::text_on(a)));
                }
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if self.deco(ui, resp, Some(K::TipAccentPreset)).clicked() {
                    accent = Some(a);
                }
            }
        });
        if let Some(a) = accent {
            self.model.settings.accent = a;
            let _ = self.model.save_settings();
        }
        if self.model.settings.expert_mode {
            self.look_expert(ui);
        }
    }

    // ------------------------------------------------------------ 2. coffre

    fn vault_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.vault_card(ui, ctx);
        if self.model.settings.expert_mode {
            ui.add_space(8.0);
            self.dest_card_expert(ui, ctx);
        }
        let rect = ui.min_rect();
        if self.scroll_dest {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
            self.scroll_dest = false;
        }
        if let Some(until) = self.dest_highlight_until {
            let t = ui.input(|i| i.time);
            if t < until {
                let a = (0.5 + 0.5 * ((t * 6.0).sin() as f32)).clamp(0.0, 1.0);
                ui.painter().rect_stroke(rect.expand(8.0), 12.0, egui::Stroke::new(3.0_f32, Color32::from_rgb(230, 170, 40).gamma_multiply(0.4 + 0.6 * a)), egui::StrokeKind::Outside);
                ui.ctx().request_repaint();
            } else {
                self.dest_highlight_until = None;
            }
        }
    }

    // ------------------------------------------------------------ 3. appareils

    fn devices_section(&mut self, ui: &mut egui::Ui) {
        self.set_hint(ui, self.t(K::VSetDevicesHint));
        ui.add_space(u::tokens::SP_S);
        let me = self.model.ctx.machine.clone();
        let mut names = self.machines.clone();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&me)) {
            names.push(me.clone());
        }
        names.sort();
        let all = self.model.settings.expert_mode;
        let tk = u::tokens(ui.ctx());
        let mut changed = false;
        for n in names {
            ui.horizontal_wrapped(|ui| {
                let rgb_now = self.machine_rgb(&n);
                // Nom de l'appareil dans une colonne de largeur fixe : les pastilles de couleur restent alignees.
                ui.allocate_ui_with_layout(egui::vec2(190.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    u::device_pill(ui, &n, rgb_now);
                    if n.eq_ignore_ascii_case(&me) {
                        ui.label(RichText::new(self.t(K::VThisDevice)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                    }
                });
                // Huit couleurs en mode simple, seize en mode expert (la couleur actuelle reste toujours visible).
                let mut palette: Vec<[u8; 3]> = crate::machinecolor::PALETTE.iter().copied().take(if all { 16 } else { 8 }).collect();
                if !palette.contains(&rgb_now) {
                    palette.push(rgb_now);
                }
                for c in palette {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                    ui.painter().circle_filled(r.center(), 9.5, Color32::from_rgb(c[0], c[1], c[2]));
                    if c == rgb_now {
                        ui.painter().circle_stroke(r.center(), 11.0, egui::Stroke::new(2.0_f32, u::col(tk.text)));
                        let t = crate::machinecolor::text_on(c);
                        u::paint_glyph(ui.painter(), r.shrink(6.0), Glyph::Check, Color32::from_rgb(t[0], t[1], t[2]));
                    }
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if self.deco(ui, resp, Some(K::TipDeviceColor)).clicked() && c != rgb_now {
                        self.model.settings.machine_colors.insert(n.clone(), c);
                        changed = true;
                    }
                }
                if self.model.settings.machine_colors.contains_key(&n) {
                    let r = u::button(ui, Btn::Ghost, self.t(K::BtnDeviceReset));
                    if self.deco(ui, r, Some(K::TipDeviceReset)).clicked() {
                        self.model.settings.machine_colors.remove(&n);
                        changed = true;
                    }
                }
            });
            ui.add_space(u::tokens::SP_XS);
        }
        if changed {
            let _ = self.model.save_settings();
        }
        if self.machines.is_empty() {
            self.set_hint(ui, self.t(K::VSetDevicesNone));
        }
    }

    // ------------------------------------------------------------ 4. protection automatique

    fn protect_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut det = self.model.settings.detect_enabled;
        if self.set_toggle(ui, &mut det, self.t(K::VSuggestNew), Some(K::TipDetEnable)) {
            self.model.settings.detect_enabled = det;
            let _ = self.model.save_settings();
            self.pad_log(&format!("[Detection] detection {} (reglages)", if det { "activee" } else { "desactivee" }));
            crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "detection"), ("value", if det { "on" } else { "off" })]);
        }
        self.set_hint(ui, self.t(K::VSuggestHint));
        ui.add_space(u::tokens::SP_S);
        let mut start = self.wizard_startup_enabled();
        if self.set_toggle(ui, &mut start, self.t(K::VStartWithPc), Some(K::TipStartupOpenApp)) {
            self.settings_set_startup(start);
        }
        self.set_hint(ui, self.t(K::VStartHint));
        ui.add_space(u::tokens::SP_S);
        self.set_label(ui, self.t(K::CardAnimTitle));
        let cur = self.model.settings.card_anim;
        let items = [(crate::gamedetect::CardAnim::Always, K::CardAnimAlways), (crate::gamedetect::CardAnim::Auto, K::CardAnimAuto), (crate::gamedetect::CardAnim::Reduced, K::CardAnimReduced)];
        if let Some(m) = self.set_chips(ui, &items, cur, K::TipCardAnim) {
            if m != cur {
                self.model.settings.card_anim = m;
                let _ = self.model.save_settings();
            }
        }
        ui.add_space(u::tokens::SP_M);
        ui.horizontal_wrapped(|ui| {
            let r = u::button_with(ui, Btn::Secondary, self.t(K::VFindOnPc), Some(Glyph::Eye), false);
            if self.deco(ui, r, None).clicked() {
                let c = ui.ctx().clone();
                self.scan_open(&c);
            }
            ui.label(RichText::new(self.t(K::VScanPcHint)).size(u::tokens::FS_SMALL).color(u::col(u::tokens(ui.ctx()).faint)));
        });
        if self.model.settings.expert_mode {
            self.protect_expert(ui, ctx);
        }
    }

    /// « Demarrer avec l'ordinateur » : Windows = regle de demarrage de l'appli (confirmation existante) ; Linux = autostart.
    fn settings_set_startup(&mut self, on: bool) {
        #[cfg(windows)]
        {
            self.set_rule_cfg(crate::startup::ID_APP, |c| c.enabled = on);
            self.model.request(Confirm::ApplyStartup);
        }
        #[cfg(not(windows))]
        {
            let exe = std::env::current_exe().unwrap_or_default();
            winutil::set_autostart(on, &exe);
        }
        crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "startup"), ("value", if on { "on" } else { "off" })]);
    }

    // ------------------------------------------------------------ 5. mises a jour

    fn updates_section(&mut self, ui: &mut egui::Ui) {
        use crate::settings::UpdateMode::*;
        let cur = self.model.settings.update_mode();
        if let Some(m) = self.set_chips(ui, &[(Auto, K::UpdModeAuto), (Ask, K::UpdModeAsk), (Never, K::UpdModeNever)], cur, K::TipCheckUpdates) {
            if m != cur {
                self.model.settings.set_update_mode(m);
                let _ = self.model.save_settings();
                let v = match m {
                    Auto => "auto",
                    Ask => "notify",
                    Never => "off",
                };
                crate::usage::record(&self.shared.data_dir, "setting_changed", &[("key", "update_mode"), ("value", v)]);
            }
        }
        let mode = self.model.settings.update_mode();
        let hint = match mode {
            Auto => K::VUpdHintAuto,
            Ask => K::VUpdHintAsk,
            Never => K::VUpdHintNever,
        };
        ui.add_space(u::tokens::SP_XS);
        self.set_hint(ui, self.t(hint));
        // Dossier du programme non modifiable : la mise a jour automatique est impossible.
        let writable = *self.exe_writable.get_or_insert_with(|| std::env::current_exe().map(|e| selfupdate::can_replace(&e)).unwrap_or(false));
        if !writable && mode != Never {
            self.set_hint(ui, self.t(K::UpdManualHint));
        }
        self.set_label(ui, &self.t(K::VUpdVersion).replace("{}", &crate::update::current_version().to_string()));
    }

    // ------------------------------------------------------------ 6. donnees d'usage

    fn usage_section(&mut self, ui: &mut egui::Ui) {
        let mut on = self.model.settings.usage_consent == Some(true);
        if self.set_toggle(ui, &mut on, self.t(K::SetUsageToggle), Some(K::TipUsageToggle)) {
            self.set_usage_consent(on, "reglages");
        }
        self.set_hint(ui, self.t(if on { K::UsageWhat } else { K::UsageOff }));
        ui.add_space(u::tokens::SP_S);
        ui.horizontal_wrapped(|ui| {
            let r = u::button(ui, Btn::Secondary, self.t(K::BtnUsageView));
            if self.deco(ui, r, Some(K::TipUsageView)).clicked() {
                let c = ui.ctx().clone();
                self.load_usage_view(&c);
            }
            let r = u::button(ui, Btn::Secondary, self.t(K::BtnUsageErase));
            if self.deco(ui, r, Some(K::TipUsageErase)).clicked() {
                self.usage_erase_ask = true;
            }
            let r = u::button(ui, Btn::Ghost, self.t(K::UsagePrivacyLink));
            if self.deco(ui, r, Some(K::TipUsagePrivacy)).clicked() {
                winutil::open_url(usage::PRIVACY_URL);
            }
        });
        if self.model.settings.expert_mode {
            self.usage_expert(ui);
        }
    }
}
