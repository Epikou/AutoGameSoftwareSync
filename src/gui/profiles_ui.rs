//! Section « Profils » de « Ajouter un jeu » : jeux exotiques, applications, emulateurs, configurations.
use super::*;
use crate::profiles::{self, Profile};

const CATS: [(&str, K); 4] = [("game", K::ProfCatGame), ("app", K::ProfCatApp), ("emulator", K::ProfCatEmu), ("config", K::ProfCatConfig)];

impl App {
    /// Charge les profils embarques + personnalises (une fois ; rechargeables en rouvrant la page).
    pub(super) fn load_profiles(&mut self) {
        let emb = profiles::load_embedded();
        let (custom, errs) = profiles::load_custom(&self.shared.data_dir, &emb);
        self.profiles = emb.into_iter().chain(custom).collect();
        self.prof_errors = errs;
        self.prof_loaded = true;
        self.prof_real_vars = profiles::Vars::real();
    }

    fn cat_label(&self, c: &str) -> &'static str {
        CATS.iter().find(|(k, _)| *k == c).map(|(_, k)| self.t(*k)).unwrap_or("?")
    }

    /// Vars du profil ouvert : valeurs choisies par l'utilisateur ajoutees aux dossiers connus.
    fn prof_vars_now(&self) -> profiles::Vars {
        let mut v = self.prof_real_vars.clone();
        v.values = self.prof_vals.clone();
        v
    }

    pub(super) fn profiles_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !self.prof_loaded {
            self.load_profiles();
        }
        ui.label(RichText::new(self.t(K::ProfHint)).small().weak());
        // Erreurs des profils personnalises (clairs, jamais silencieux).
        for e in self.prof_errors.clone() {
            ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::ProfCustomErr).replace("{0}", &e.file).replace("{1}", &e.message));
        }
        ui.horizontal_wrapped(|ui| {
            let hint = self.t(K::ProfSearch);
            let mut q = std::mem::take(&mut self.prof_query);
            let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text(hint).desired_width(220.0));
            self.prof_query = q;
            self.deco_kind(ui, r, Some(K::TipProfSearch), ItemKind::Text);
            let r = ui.selectable_label(self.prof_cat.is_none(), self.t(K::ProfCatAll));
            if self.deco(ui, r, Some(K::TipProfCat)).clicked() {
                self.prof_cat = None;
            }
            for (c, k) in CATS {
                let r = ui.selectable_label(self.prof_cat == Some(c), self.t(k));
                if self.deco(ui, r, Some(K::TipProfCat)).clicked() {
                    self.prof_cat = Some(c);
                }
            }
            let r = ui.button(self.t(if self.prof_big { K::BtnLogShrink } else { K::BtnLogEnlarge }));
            if self.deco(ui, r, Some(K::TipProfEnlarge)).clicked() {
                self.prof_big = !self.prof_big;
            }
        });
        let vars = self.prof_real_vars.clone();
        let det: Vec<bool> = self.profiles.iter().map(|p| profiles::detected(p, &vars)).collect();
        let idx = {
            let profs = self.profiles.clone();
            profiles::filter_rows(&profs, &self.prof_query, self.prof_cat, &|p| profs.iter().position(|q| q.id == p.id).map(|i| det[i]).unwrap_or(false))
        };
        if idx.is_empty() {
            ui.label(RichText::new(self.t(K::ProfNone)).weak());
        }
        let area_h = if self.prof_big { (ui.clip_rect().bottom() - ui.cursor().top() - 28.0).max(crate::settings::LOG_HEIGHT_MIN) } else { self.model.settings.prof_height };
        egui::ScrollArea::vertical().id_salt("profiles_list").max_height(area_h).min_scrolled_height(area_h.min(300.0)).auto_shrink([false, true]).show(ui, |ui| {
            for i in idx {
                let p = self.profiles[i].clone();
                let open = self.prof_open.as_deref() == Some(p.id.as_str());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&p.name).strong());
                    ui.label(RichText::new(self.cat_label(&p.category)).small().weak());
                    if det[i] {
                        icon_label(ui, Icon::Check, self.t(K::ProfDetected), Color32::from_rgb(60, 180, 100));
                    }
                    let r = ui.small_button(if open { self.t(K::ProfHide) } else { self.t(K::ProfDetails) });
                    if self.deco(ui, r, Some(K::TipProfDetails)).clicked() {
                        if open {
                            self.prof_open = None;
                        } else {
                            self.open_profile(&p);
                        }
                    }
                });
                if open {
                    self.profile_details(ui, ctx, &p);
                }
            }
        });
        if !self.prof_big {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::drag());
            let resp = resp.on_hover_text(self.t(K::TipProfResize)).on_hover_cursor(egui::CursorIcon::ResizeVertical);
            let col = if resp.hovered() || resp.dragged() { ui.visuals().widgets.hovered.fg_stroke.color } else { ui.visuals().weak_text_color() };
            let c = rect.center();
            for dx in [-14.0, 0.0, 14.0] {
                ui.painter().circle_filled(egui::pos2(c.x + dx, c.y), 2.0, col);
            }
            if resp.dragged() {
                let h = (self.model.settings.prof_height + resp.drag_delta().y).clamp(crate::settings::LOG_HEIGHT_MIN, crate::settings::LOG_HEIGHT_MAX);
                self.model.settings.prof_height = h;
            }
            if resp.drag_stopped() {
                let _ = self.model.save_settings();
            }
        }
    }

    /// Ouvre un profil : emplacements coches par defaut, variables preremplies par la detection.
    pub(super) fn open_profile(&mut self, p: &Profile) {
        self.prof_open = Some(p.id.clone());
        self.prof_enabled = p.locations.iter().map(|l| l.default_enabled).collect();
        self.prof_ack = false;
        self.prof_vals.clear();
        for v in &p.variables {
            if let Some(first) = profiles::var_candidates(v, &self.prof_real_vars).into_iter().next() {
                self.prof_vals.insert(v.name.clone(), first);
            }
        }
    }

    fn profile_details(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, p: &Profile) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            for w in &p.warnings {
                egui::Frame::new().fill(Color32::from_rgb(230, 170, 40).gamma_multiply(0.15)).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
                    ui.label(RichText::new(w).color(Color32::from_rgb(230, 170, 40)).small());
                });
            }
            if let Some(n) = &p.notes {
                ui.label(RichText::new(n).small().weak());
            }
            if let (Some(c), Some(d)) = (&p.confidence, &p.verified) {
                ui.label(RichText::new(self.t(K::ProfConfidence).replace("{0}", c).replace("{1}", d)).small().weak());
            }
            // Variables a choisir (dossier d'installation, version...).
            let mut missing: Vec<String> = Vec::new();
            for v in &p.variables {
                let cands = profiles::var_candidates(v, &self.prof_real_vars);
                ui.horizontal_wrapped(|ui| {
                    ui.label(&v.label);
                    let mut cur = self.prof_vals.get(&v.name).cloned().unwrap_or_default();
                    if !cands.is_empty() && v.scan.is_some() {
                        let cb = egui::ComboBox::from_id_salt(format!("pv_{}", v.name)).selected_text(cur.clone()).show_ui(ui, |ui| {
                            for c in &cands {
                                ui.selectable_value(&mut cur, c.clone(), c);
                            }
                        });
                        self.deco(ui, cb.response, Some(K::TipProfVar));
                    } else {
                        let r = ui.add(egui::TextEdit::singleline(&mut cur).desired_width(300.0));
                        self.deco_kind(ui, r, Some(K::TipProfVar), ItemKind::Text);
                        let pkey = format!("pv:{}", v.name);
                        if let Some(f) = self.take_picked(&pkey) {
                            cur = f.path.display().to_string();
                        }
                        let b = ui.button(self.t(K::BtnBrowse));
                        if self.deco(ui, b, Some(K::TipBrowse)).clicked() {
                            let opts = super::picker_ui::PickOpts { name: Some(p.name.clone()), is_file: false, names: vec![p.name.clone()], found: vec![], program: None };
                            self.open_picker(ctx, &pkey, opts);
                        }
                    }
                    self.prof_vals.insert(v.name.clone(), cur);
                });
            }
            ui.label(RichText::new(self.t(K::ProfLocations)).strong());
            let vars = self.prof_vars_now();
            for (i, l) in p.locations.iter().enumerate() {
                let mut on = self.prof_enabled.get(i).copied().unwrap_or(l.default_enabled);
                let exp = profiles::expand(&l.path, &vars);
                ui.horizontal_wrapped(|ui| {
                    if self.toggle(ui, &mut on, &l.label, K::TipProfLoc).changed() {
                        if let Some(e) = self.prof_enabled.get_mut(i) {
                            *e = on;
                        }
                    }
                    match &exp {
                        Some(path) => {
                            ui.label(RichText::new(path.display().to_string()).small().monospace());
                            let exists = if l.is_file { path.is_file() } else { path.is_dir() };
                            if exists {
                                icon_label(ui, Icon::Check, self.t(K::LocOk), Color32::from_rgb(60, 180, 100));
                            } else {
                                icon_label(ui, Icon::Cross, self.t(K::LocMissing), Color32::from_rgb(220, 80, 80));
                            }
                        }
                        None => {
                            ui.label(RichText::new(&l.path).small().monospace().weak());
                            if on {
                                let names: Vec<String> = p.variables.iter().filter(|v| l.path.contains(&format!("<{}>", v.name))).map(|v| v.label.clone()).collect();
                                missing.push(names.join(", "));
                            }
                        }
                    }
                });
                if let Some(n) = &l.note {
                    ui.label(RichText::new(n).small().weak());
                }
            }
            if p.needs_ack() {
                let mut a = self.prof_ack;
                if self.toggle(ui, &mut a, self.t(K::ProfAck), K::TipProfAck).changed() {
                    self.prof_ack = a;
                }
            }
            let any_on = self.prof_enabled.iter().any(|e| *e);
            let ack_ok = !p.needs_ack() || self.prof_ack;
            if !missing.is_empty() {
                ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::ProfNeedVar).replace("{}", &missing.join(" ; ")));
            }
            let r = ui.add_enabled(any_on && ack_ok && missing.is_empty(), egui::Button::new(RichText::new(self.t(K::BtnProfAdd)).strong()));
            if self.deco(ui, r, Some(K::TipProfAdd)).clicked() {
                match profiles::to_game(p, &vars, &self.prof_enabled) {
                    Ok(g) => {
                        self.prof_open = None;
                        self.add_src = ("profile", Some(matches!(p.category.as_str(), "app" | "config")));
                        self.begin_add(g, None, ctx);
                    }
                    Err(e) => self.say(e, false),
                }
            }
        });
    }
}
