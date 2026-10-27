//! Navigation : manette, focus clavier / manette, cerclage et infobulles, barre d'onglets, changement d'onglet, question « mode manette ? ».
use super::*;

impl App {
    pub(super) fn handle_gamepad(&mut self, raw: &mut egui::RawInput, ectx: &egui::Context) {
        use uilogic::NavAction;
        // Touches decidees a la fin de la frame precedente (Entree / Echap / haut-bas d'un DragValue).
        for k in self.focus.take_injected() {
            for pressed in [true, false] {
                raw.events.push(crate::padfocus::key_event(k, pressed));
            }
        }
        let navs = self.pad.drain();
        // XInput / HID sont lus meme sans le focus : hors premier plan on ignore tout, pour qu'un joueur
        // en pleine partie n'agisse jamais sur AutoGameSoftwareSync (validation, restauration...) a son insu.
        if navs.is_empty() || !winutil::own_window_is_foreground() {
            return;
        }
        let modal = self.model.confirm.is_some();
        // Mode manette : selection maison (PadFocus). Un menu deroulant ouvert garde la navigation native d'egui.
        let popup_open = ectx.memory(|m| m.any_popup_open());
        let custom = uilogic::pad_present(&self.pad.status()) && !popup_open;
        let mut keys: Vec<egui::Key> = Vec::new();
        for n in navs {
            match uilogic::nav_action(n, modal) {
                NavAction::Key(k) if custom => self.focus.push(crate::padfocus::action_from_key(k)),
                NavAction::Key(k) => keys.push(crate::padfocus::egui_key(k)),
                NavAction::Page(d) => self.page_delta += d,
                NavAction::Ignore => {}
            }
        }
        if custom {
            ectx.request_repaint();
        }
        if keys.is_empty() {
            return;
        }
        let nothing_focused = ectx.memory(|m| m.focused().is_none());
        if nothing_focused {
            for pressed in [true, false] {
                raw.events.push(crate::padfocus::key_event(egui::Key::Tab, pressed));
            }
        }
        for k in keys {
            for pressed in [true, false] {
                raw.events.push(crate::padfocus::key_event(k, pressed));
            }
        }
    }
}

impl App {
    // ------------------------------------------------------------ focus, cerclage, infobulles

    /// Suivi du focus clavier / manette (une fois par image, avant le dessin).
    pub(super) fn track_focus(&mut self, ctx: &egui::Context) {
        use egui::{Event, Key};
        let (nav_key, pressed) = ctx.input(|i| {
            let nav = i.events.iter().any(|e| matches!(e, Event::Key { pressed: true, key: Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::Tab, .. }));
            (nav, i.pointer.any_pressed())
        });
        // Un gain de focus dans les 3 images suivant une touche de deplacement vient du clavier / de la manette.
        self.nav_recent = if nav_key { 3 } else { self.nav_recent.saturating_sub(1) };
        if nav_key {
            self.focus_from_nav = true;
        } else if pressed {
            self.focus_from_nav = false;
        }
        let cur = ctx.memory(|m| m.focused());
        if cur != self.focus_id {
            self.focus_id = cur;
            self.focus_since = Instant::now();
        }
        let held = self.focus_since.elapsed().as_millis() as u64;
        self.focus_tip_due = cur.is_some() && self.focus_from_nav && uilogic::tooltip_due(held);
    }

    /// Infobulle (survol souris, delai `TOOLTIP_DELAY_MS`) + apparition apres le meme delai quand
    /// l'element est selectionne au clavier / a la manette. Texte issu de la table i18n.
    pub(super) fn tip(&self, resp: egui::Response, tip: K, selected: bool) -> egui::Response {
        let txt = self.t(tip);
        let resp = resp.on_hover_text(txt).on_disabled_hover_text(txt);
        let due = if self.pad_mode { uilogic::tooltip_due(self.focus.selected_for_ms()) } else { self.focus_tip_due };
        if selected && due && !resp.hovered() {
            resp.show_tooltip_text(txt);
        }
        resp
    }

    /// Decoration commune de tout widget interactif : cerclage net de 2 px au survol et a la
    /// selection (couleur calculee pour le contraste, voir `theme::ring_color`), enregistrement
    /// dans la selection manette, defilement centre, et infobulle.
    pub(super) fn deco(&self, ui: &egui::Ui, resp: egui::Response, tip: Option<K>) -> egui::Response {
        self.deco_kind(ui, resp, tip, ItemKind::Button)
    }

    pub(super) fn deco_kind(&self, ui: &egui::Ui, resp: egui::Response, tip: Option<K>, kind: ItemKind) -> egui::Response {
        let in_scroll = ui.stack().iter().any(|n| n.kind() == Some(egui::UiKind::ScrollArea));
        if resp.sense.contains(egui::Sense::FOCUSABLE) {
            self.focus.register(&resp, kind, in_scroll);
        }
        // En mode manette la selection est celle de `PadFocus` ; sinon le focus natif d'egui.
        let selected = if self.pad_mode { self.focus.is_selected(resp.id) } else { resp.has_focus() };
        if resp.hovered() || selected {
            let painter = ui.painter().with_clip_rect(ui.clip_rect().expand(4.0));
            painter.rect_stroke(resp.rect.expand(1.0), 4.0, egui::Stroke::new(2.0_f32, rgb(self.palette.ring)), egui::StrokeKind::Outside);
        }
        // Defilement centre (animation d'egui : pas de saccade ; uniquement a un changement de selection).
        let scroll = if self.pad_mode { self.focus.take_scroll(resp.id, in_scroll) } else { resp.gained_focus() && self.nav_recent > 0 && in_scroll };
        if scroll {
            resp.scroll_to_me(Some(egui::Align::Center));
        }
        match tip {
            Some(k) => self.tip(resp, k, selected),
            None => resp,
        }
    }

    // ------------------------------------------------------------ barre d'onglets

    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let pads = self.pad.status();
        let glyphs = uilogic::glyph_set(&pads);
        let ctx = ui.ctx().clone();
        // Mode manette (manette detectee ou grand format actif) : onglets centres, pictogrammes aux extremites.
        let centered = glyphs.is_some() || self.big_now;
        ui.add_space(4.0);
        let font = egui::TextStyle::Button.resolve(ui.style());
        let pad = if self.big_now { egui::vec2(22.0, 10.0) } else { egui::vec2(16.0, 6.0) };
        let gsize = if self.big_now { egui::vec2(60.0, 34.0) } else { egui::vec2(46.0, 26.0) };
        let gap = ui.spacing().item_spacing.x;
        let gw = glyphs.map(|_| gsize.x);
        let usable = ui.available_width() - gw.map(|g| 2.0 * (g + gap)).unwrap_or(0.0);
        let measure = |app: &App, ui: &egui::Ui, keys: [K; 3]| -> Vec<egui::Vec2> {
            keys.iter().map(|k| ui.painter().layout_no_wrap(app.t(*k).to_string(), font.clone(), Color32::WHITE).size() + pad * 2.0).collect()
        };
        // 2.0 : Accueil / Ajouter / Reglages (vocabulaire simple, dans les deux modes).
        let full_k = [K::VNavHome, K::VNavAdd, K::VNavSettings];
        let short_k = full_k;
        let full_sz = measure(self, ui, full_k);
        let short_sz = measure(self, ui, short_k);
        let icon_h = full_sz.iter().map(|s| s.y).fold(gsize.y, f32::max);
        let icon_sz: Vec<egui::Vec2> = vec![egui::vec2(icon_h + 10.0, icon_h); 3];
        let fw: Vec<f32> = full_sz.iter().map(|s| s.x).collect();
        let sw: Vec<f32> = short_sz.iter().map(|s| s.x).collect();
        let mode = uilogic::tab_mode(usable, &fw, &sw, gap);
        let sizes = match mode {
            uilogic::TabMode::Full => full_sz,
            uilogic::TabMode::Short => short_sz,
            uilogic::TabMode::Icons => icon_sz,
        };
        let widths: Vec<f32> = sizes.iter().map(|s| s.x).collect();
        let height = sizes.iter().map(|s| s.y).fold(gsize.y, f32::max);
        let lay = uilogic::tab_bar_layout(ui.available_width(), gw, &widths, gap, centered);
        let (bar_id, bar) = ui.allocate_space(egui::vec2(ui.available_width(), height));
        let at = |x: f32, size: egui::Vec2| egui::Rect::from_min_size(egui::pos2(bar.min.x + x, bar.min.y + (height - size.y) / 2.0), size);
        if let Some(g) = glyphs {
            self.shoulder_glyph(ui, g, true, at(lay.left_glyph_x, gsize), bar_id.with("lb"));
            self.shoulder_glyph(ui, g, false, at(lay.right_glyph_x, gsize), bar_id.with("rb"));
        }
        for (i, p) in Page::ALL.into_iter().enumerate() {
            let selected = self.model.page == p;
            let rect = at(lay.tab_x[i], sizes[i]);
            // Sense::CLICK sans FOCUSABLE : les onglets ne sont jamais atteints par le D-pad / le stick /
            // la touche Tab (le curseur reste dans l'onglet courant) ; on change d'onglet par LB/RB ou a la souris.
            let resp = ui.interact(rect, bar_id.with(("tab", i)), tab_sense());
            let expert = self.model.settings.expert_mode;
            let tip = match (p, expert) {
                (Page::Games, false) => K::VTipHome,
                (Page::Add, false) => K::VTipAdd,
                (Page::Settings, false) => K::VTipSettings,
                (Page::Games, true) => K::TipTabGames,
                (Page::Add, true) => K::TipTabAdd,
                (Page::Settings, true) => K::TipTabSettings,
            };
            let resp = self.deco(ui, resp, Some(tip));
            let pal = self.palette;
            let fill = if selected {
                rgb(pal.accent)
            } else if resp.hovered() {
                rgb(pal.hover_fill)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 6.0, fill);
            let fg = if selected { rgb(pal.on_accent) } else { rgb(pal.text) };
            match mode {
                uilogic::TabMode::Icons => {
                    let ir = egui::Rect::from_center_size(rect.center(), egui::vec2(height * 0.8, height * 0.8));
                    match p {
                        Page::Games => ui::paint_glyph(ui.painter(), ir.shrink(ir.width() * 0.1), ui::Glyph::Home, fg),
                        Page::Add => paint_plus(ui.painter(), ir, fg),
                        Page::Settings => paint_gear(ui.painter(), ir, fg, fill),
                    }
                }
                m => {
                    let key = if m == uilogic::TabMode::Full { full_k[i] } else { short_k[i] };
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, self.t(key), font.clone(), fg);
                }
            }
            if resp.clicked() && !selected {
                self.model.page = p;
                self.on_page_enter(p, &ctx);
            }
        }
        ui.add_space(4.0);
    }

    /// Pictogramme de bouton d'epaule (LB/RB ou L1/R1), trace au painter (aucune image embarquee).
    /// Cliquable a la souris (meme effet que le bouton de la manette).
    pub(super) fn shoulder_glyph(&mut self, ui: &mut egui::Ui, g: uilogic::Glyphs, left: bool, rect: egui::Rect, id: egui::Id) {
        let (l, r) = uilogic::shoulder_labels(g);
        let label = if left { l } else { r };
        let size = rect.size();
        let resp = ui.interact(rect, id, egui::Sense::CLICK);
        let resp = self.deco(ui, resp, Some(if left { K::TipPadPrev } else { K::TipPadNext }));
        let pal = self.palette;
        // Silhouette de gachette : grand arrondi cote exterieur haut, petits arrondis ailleurs.
        let big_r = (size.y * 0.8) as u8;
        let cr = if left { egui::CornerRadius { nw: big_r, ne: 4, sw: 4, se: 4 } } else { egui::CornerRadius { nw: 4, ne: big_r, sw: 4, se: 4 } };
        let fill = if resp.hovered() { rgb(pal.hover_fill) } else { rgb(pal.widget_bg) };
        ui.painter().rect(rect, cr, fill, egui::Stroke::new(1.5_f32, rgb(pal.text)), egui::StrokeKind::Inside);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(size.y * 0.5), rgb(pal.text));
        if resp.clicked() && self.model.confirm.is_none() {
            self.page_delta += if left { -1 } else { 1 };
        }
    }


    pub(super) fn on_page_enter(&mut self, p: Page, ctx: &egui::Context) {
        // Changement d'onglet : toutes les fiches de versions ouvertes se referment.
        self.model.selected = None;
        self.history = None;
        self.link_open = false;
        // « Nouveau PC » se referme en changeant d'onglet (jamais pendant une recuperation).
        if self.migrate.open && !self.migrate.busy() {
            self.migrate.open = false;
        }
        match p {
            Page::Add => {
                self.reset_add_page(ctx);
                self.load_dest_games(ctx);
            }
            Page::Games => self.last_status = Instant::now() - Duration::from_secs(60),
            Page::Settings => {
                self.load_startup(ctx);
                self.det_mem = crate::gamedetect::Memory::load(&self.shared.data_dir);
                self.load_dest_games(ctx);
            }
        }
    }


    // ------------------------------------------------------------ question « mode manette ? »

    /// Mode « Me demander » : si une manette est detectee (passivement), pose la question UNE fois par connexion,
    /// sauf report (fenetre cachee, tutoriel, jeu en cours, autre dialogue).
    pub(super) fn pad_prompt_tick(&mut self) {
        let sh = self.pad.shared.clone();
        sh.set_mode(self.model.settings.pad_mode);
        if self.model.settings.pad_mode != crate::settings::PadMode::Ask || sh.session_yes() {
            self.pad_prompter.reset();
            self.pad_prompt = None;
            return;
        }
        if self.pad_prompt.is_some() {
            return;
        }
        let c = crate::padctl::PromptCtx {
            hidden: !winutil::own_window_visible(APP_TITLE) || !winutil::own_window_is_foreground(),
            tutorial: self.wizard.active,
            game_running: self.shared.games_running.load(Ordering::SeqCst),
            other_dialog: self.model.confirm.is_some() || self.support.is_some() || self.pending_add.is_some() || self.pending_overlap.is_some(),
        };
        if let Some(info) = self.pad_prompter.on_presence(&self.pad.presence(), &c) {
            self.pad_log(&format!("[Manette] detectee (mode Me demander) : {} ; question posee", info.display()));
            self.pad_prompt = Some(info);
            self.pad_prompt_remember = false;
        }
    }

    pub(super) fn pad_log(&self, line: &str) {
        Logger::new(self.shared.data_dir.join("logs").join("watch.log"), false).log(line);
    }

}
