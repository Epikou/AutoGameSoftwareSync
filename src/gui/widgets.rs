//! Composants maison : interrupteur (toggle), titres de section, cartes, bouton cafe et pictogrammes
//! traces au painter (aucune image embarquee, aucun logo officiel).
use super::*;

pub const COFFEE_PAGE: &str = "https://ko-fi.com/autogamesoftwaresave";

// ------------------------------------------------------------------ pictogrammes (painter)

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Tasse a cafe : corps, anse, soucoupe et deux volutes de vapeur.
pub fn paint_cup(p: &egui::Painter, r: egui::Rect, body: Color32, line: Color32) {
    let s = r.height().min(r.width());
    let c = r.center();
    let u = s / 24.0;
    let cup = egui::Rect::from_min_max(egui::pos2(c.x - 7.5 * u, c.y - 3.0 * u), egui::pos2(c.x + 5.5 * u, c.y + 7.0 * u));
    p.rect_filled(cup, egui::CornerRadius { nw: 1, ne: 1, sw: (5.0 * u) as u8, se: (5.0 * u) as u8 }, body);
    // anse
    p.circle_stroke(egui::pos2(c.x + 6.0 * u, c.y + 1.5 * u), 3.2 * u, egui::Stroke::new((1.8 * u).max(1.2), body));
    // soucoupe
    p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x - 1.0 * u, c.y + 8.4 * u), egui::vec2(18.0 * u, 2.0 * u)), 1.0, body);
    // vapeur
    for dx in [-4.0_f32, 0.0] {
        let x = c.x + dx * u;
        let pts = [egui::pos2(x, c.y - 4.5 * u), egui::pos2(x + 2.5 * u, c.y - 7.0 * u), egui::pos2(x - 2.5 * u, c.y - 9.0 * u), egui::pos2(x, c.y - 11.5 * u)];
        p.add(egui::epaint::CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, egui::Stroke::new((1.5 * u).max(1.0), line)));
    }
}

/// Dossier (ouvrir le dossier de configuration).
pub fn paint_folder(p: &egui::Painter, r: egui::Rect, col: Color32) {
    let c = r.center();
    let s = r.height().min(r.width()) / 16.0;
    p.rect_filled(egui::Rect::from_min_size(egui::pos2(c.x - 7.0 * s, c.y - 5.0 * s), egui::vec2(6.0 * s, 3.0 * s)), 1.5, col);
    p.rect_filled(egui::Rect::from_min_max(egui::pos2(c.x - 7.0 * s, c.y - 3.0 * s), egui::pos2(c.x + 7.0 * s, c.y + 5.0 * s)), 2.0, col);
}

/// Manette : corps, croix directionnelle, deux boutons.
pub fn paint_pad(p: &egui::Painter, r: egui::Rect, col: Color32, bg: Color32) {
    if super::ui::glyphs::paint_char(p, r.expand(r.width() * 0.1), egui_phosphor::regular::GAME_CONTROLLER, col, 0.0) {
        return;
    }
    let c = r.center();
    let s = r.height().min(r.width()) / 16.0;
    let body = egui::Rect::from_center_size(c, egui::vec2(15.0 * s, 8.0 * s));
    p.rect_filled(body, 4.0 * s, col);
    p.circle_filled(egui::pos2(c.x - 5.5 * s, c.y + 4.0 * s), 3.0 * s, col);
    p.circle_filled(egui::pos2(c.x + 5.5 * s, c.y + 4.0 * s), 3.0 * s, col);
    let d = egui::pos2(c.x - 4.5 * s, c.y - 0.5 * s);
    p.rect_filled(egui::Rect::from_center_size(d, egui::vec2(4.0 * s, 1.3 * s)), 0.0, bg);
    p.rect_filled(egui::Rect::from_center_size(d, egui::vec2(1.3 * s, 4.0 * s)), 0.0, bg);
    p.circle_filled(egui::pos2(c.x + 4.0 * s, c.y - 1.5 * s), 1.1 * s, bg);
    p.circle_filled(egui::pos2(c.x + 6.0 * s, c.y + 0.5 * s), 1.1 * s, bg);
}

pub fn paint_plus(p: &egui::Painter, r: egui::Rect, col: Color32) {
    let c = r.center();
    let s = r.height().min(r.width()) / 2.0 * 0.6;
    let st = egui::Stroke::new(2.2_f32, col);
    p.line_segment([egui::pos2(c.x - s, c.y), egui::pos2(c.x + s, c.y)], st);
    p.line_segment([egui::pos2(c.x, c.y - s), egui::pos2(c.x, c.y + s)], st);
}

pub fn paint_gear(p: &egui::Painter, r: egui::Rect, col: Color32, bg: Color32) {
    let c = r.center();
    let rad = r.height().min(r.width()) / 2.0 * 0.8;
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let d = egui::vec2(a.cos(), a.sin());
        p.line_segment([c + d * rad * 0.6, c + d * rad], egui::Stroke::new(rad * 0.32, col));
    }
    p.circle_filled(c, rad * 0.72, col);
    p.circle_filled(c, rad * 0.28, bg);
}

/// Pastille de couleur (etat de la surveillance en version compacte).
pub fn paint_dot(p: &egui::Painter, r: egui::Rect, col: Color32) {
    p.circle_filled(r.center(), r.height().min(r.width()) * 0.32, col);
}

// ------------------------------------------------------------------ composants

/// Titre de section : un peu plus grand et en gras, partout.
pub fn section_title(big: bool, ui: &mut egui::Ui, text: &str) -> egui::Response {
    // egui n'embarque pas de police grasse : « gras » = texte trace deux fois avec un leger decalage.
    let sz = if big { 24.0 } else { 18.0 };
    let col = ui.visuals().strong_text_color();
    let galley = ui.painter().layout(text.to_string(), egui::FontId::proportional(sz), col, ui.available_width().max(40.0));
    let (rect, resp) = ui.allocate_exact_size(galley.size() + egui::vec2(1.0, 0.0), egui::Sense::hover());
    ui.painter().galley(rect.min, galley.clone(), col);
    ui.painter().galley(rect.min + egui::vec2(0.8, 0.0), galley, col);
    resp
}


impl App {
    pub(super) fn section_title(&self, ui: &mut egui::Ui, text: &str) -> egui::Response {
        section_title(self.big_now, ui, text)
    }

    /// Interrupteur arrondi (couleur d'accent quand actif, gris sinon, animation courte). Meme usage qu'une
    /// case a cocher : `deco(...)` ajoute cerclage, selection manette et infobulle ; `changed()` signale un basculement.
    pub(super) fn toggle(&self, ui: &mut egui::Ui, on: &mut bool, label: &str, tip: K) -> egui::Response {
        let k = if self.big_now { 1.3 } else { 1.0 };
        let size = egui::vec2(38.0 * k, 20.0 * k);
        let mut out = None;
        ui.horizontal_wrapped(|ui| {
            let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
            let lr = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
            if lr.clicked() {
                resp.mark_changed();
                *on = !*on;
            } else if resp.clicked() {
                resp.mark_changed();
                *on = !*on;
            }
            let t = ui.ctx().animate_bool_with_time(resp.id.with("anim"), *on, 0.12);
            let off = lerp_color(rgb(self.palette.widget_bg), rgb(self.palette.text), 0.35);
            let track = lerp_color(off, rgb(self.palette.accent), t);
            let rad = rect.height() / 2.0;
            let enabled = ui.is_enabled();
            let p = ui.painter();
            p.rect_filled(rect, rad, if enabled { track } else { track.gamma_multiply(0.5) });
            let kx = egui::lerp((rect.min.x + rad)..=(rect.max.x - rad), t);
            p.circle_filled(egui::pos2(kx, rect.center().y), rad - 3.0, if t > 0.5 { rgb(self.palette.on_accent) } else { Color32::WHITE });
            out = Some(self.deco(ui, resp, Some(tip)));
        });
        out.unwrap()
    }

    /// Bouton « Offre-nous un cafe » avec une tasse dessinee ; ouvre la page Ko-fi dans le navigateur.
    pub(super) fn coffee_button(&self, ui: &mut egui::Ui, compact: bool) -> bool {
        self.coffee_button_sized(ui, compact, 1.0)
    }

    /// `scale` < 1 : version plus petite (barre du bas).
    pub(super) fn coffee_button_sized(&self, ui: &mut egui::Ui, compact: bool, scale: f32) -> bool {
        let mut font = egui::TextStyle::Button.resolve(ui.style());
        font.size *= scale;
        let text = self.t(K::CoffeeBtn);
        let galley = ui.painter().layout_no_wrap(text.to_string(), font, Color32::BLACK);
        let h = ui.spacing().interact_size.y.max(26.0) * scale;
        let w = if compact { h + 8.0 } else { galley.size().x + h + 22.0 };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
        let fill = if resp.hovered() { Color32::from_rgb(255, 214, 90) } else { Color32::from_rgb(250, 200, 60) };
        ui.painter().rect_filled(rect, 8.0, fill);
        let icon = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 0.0), egui::vec2(h, h));
        paint_cup(ui.painter(), icon.shrink(3.0), Color32::from_rgb(90, 52, 28), Color32::from_rgb(120, 80, 50));
        if !compact {
            ui.painter().galley(egui::pos2(rect.min.x + h + 6.0, rect.center().y - galley.size().y / 2.0), galley, Color32::BLACK);
        }
        let resp = self.deco(ui, resp, Some(K::TipCoffee));
        if resp.clicked() {
            winutil::open_url(COFFEE_PAGE);
            let place = if scale < 1.0 { "bar" } else if self.support.is_some() { "popup" } else if self.wizard.active { "tutorial" } else { "settings" };
            crate::usage::record(&self.shared.data_dir, "support_click", &[("place", place)]);
        }
        resp.clicked()
    }

    /// Bouton avec pictogramme (icone + libelle) : l'icone est tracee par `icon`.
    pub(super) fn icon_button(&self, ui: &mut egui::Ui, label: &str, icon: impl FnOnce(&egui::Painter, egui::Rect, Color32)) -> egui::Response {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(label.to_string(), font, rgb(self.palette.text));
        let h = ui.spacing().interact_size.y.max(26.0);
        let pad = ui.spacing().button_padding.x;
        let w = galley.size().x + h + pad * 2.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w.min(ui.available_width().max(h + 10.0)), h), egui::Sense::click());
        let v = ui.style().interact(&resp);
        ui.painter().rect(rect, 6.0, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
        icon(ui.painter(), egui::Rect::from_min_size(rect.min + egui::vec2(pad * 0.5, 0.0), egui::vec2(h, h)), rgb(self.palette.text));
        ui.painter().with_clip_rect(rect).galley(egui::pos2(rect.min.x + h + pad * 0.5, rect.center().y - galley.size().y / 2.0), galley, rgb(self.palette.text));
        resp
    }
}

// ------------------------------------------------------------------ drapeaux (painter, jamais d'emoji)

fn star(p: &egui::Painter, c: egui::Pos2, r: f32, rot: f32, col: Color32) {
    use std::f32::consts::PI;
    let pt = |rad: f32, a: f32| egui::pos2(c.x + rad * a.cos(), c.y + rad * a.sin());
    let inner = r * 0.382;
    let tips: Vec<egui::Pos2> = (0..5).map(|i| pt(r, rot + i as f32 * 2.0 * PI / 5.0)).collect();
    let ins: Vec<egui::Pos2> = (0..5).map(|i| pt(inner, rot + PI / 5.0 + i as f32 * 2.0 * PI / 5.0)).collect();
    // pentagone central + 5 pointes (tout est convexe)
    p.add(egui::Shape::convex_polygon(ins.clone(), col, egui::Stroke::NONE));
    for i in 0..5 {
        p.add(egui::Shape::convex_polygon(vec![ins[(i + 4) % 5], tips[i], ins[i]], col, egui::Stroke::NONE));
    }
}

fn hstripes(p: &egui::Painter, r: egui::Rect, parts: &[(f32, Color32)]) {
    let total: f32 = parts.iter().map(|x| x.0).sum();
    let mut y = r.min.y;
    for (w, c) in parts {
        let h = r.height() * w / total;
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(r.min.x, y), egui::vec2(r.width(), h + 0.5)), 0.0, *c);
        y += h;
    }
}

pub fn paint_flag_ru(p: &egui::Painter, r: egui::Rect) {
    hstripes(p, r, &[(1.0, Color32::WHITE), (1.0, Color32::from_rgb(0, 57, 166)), (1.0, Color32::from_rgb(213, 43, 30))]);
}

pub fn paint_flag_es(p: &egui::Painter, r: egui::Rect) {
    hstripes(p, r, &[(1.0, Color32::from_rgb(170, 21, 27)), (2.0, Color32::from_rgb(241, 191, 0)), (1.0, Color32::from_rgb(170, 21, 27))]);
}

pub fn paint_flag_zh(p: &egui::Painter, r: egui::Rect) {
    let red = Color32::from_rgb(222, 41, 16);
    let yellow = Color32::from_rgb(255, 222, 0);
    p.rect_filled(r, 0.0, red);
    let (w, h) = (r.width(), r.height());
    let at = |fx: f32, fy: f32| egui::pos2(r.min.x + w * fx, r.min.y + h * fy);
    star(p, at(0.17, 0.27), h * 0.2, -std::f32::consts::FRAC_PI_2, yellow);
    // quatre petites etoiles, une pointe vers la grande
    for (fx, fy) in [(0.34, 0.1), (0.41, 0.2), (0.41, 0.35), (0.34, 0.45)] {
        let c = at(fx, fy);
        let big = at(0.17, 0.27);
        let a = (big.y - c.y).atan2(big.x - c.x);
        star(p, c, h * 0.07, a, yellow);
    }
}

/// Drapeau d'une langue dans `r` (cadre fin autour).
pub fn paint_flag(l: Lang, p: &egui::Painter, r: egui::Rect) {
    let pc = p.with_clip_rect(r);
    match l {
        Lang::En => paint_flag_en(&pc, r),
        Lang::Fr => paint_flag_fr(&pc, r),
        Lang::Ru => paint_flag_ru(&pc, r),
        Lang::Es => paint_flag_es(&pc, r),
        Lang::Zh => paint_flag_zh(&pc, r),
    }
    p.rect_stroke(r, 1.5, egui::Stroke::new(1.0_f32, Color32::from_black_alpha(90)), egui::StrokeKind::Outside);
}

impl App {
    /// Nom de langue affiche : endonyme ; le chinois s'affiche « Chinese » tant qu'aucune police CJK n'est chargee.
    pub(super) fn lang_label(&self, l: Lang) -> &'static str {
        if l == Lang::Zh && !self.cjk_loaded {
            "Chinese"
        } else {
            l.native_name()
        }
    }

    /// Bouton de langue : drapeau dessine + nom dans sa propre langue.
    pub(super) fn lang_button(&self, ui: &mut egui::Ui, l: Lang, selected: bool) -> egui::Response {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(self.lang_label(l).to_string(), font, rgb(self.palette.text));
        let h = ui.spacing().interact_size.y.max(28.0);
        let fw = h * 1.1;
        let size = egui::vec2(fw + galley.size().x + 26.0, h);
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        let pal = self.palette;
        let fill = if selected { rgb(pal.accent) } else if resp.hovered() { rgb(pal.hover_fill) } else { rgb(pal.widget_bg) };
        ui.painter().rect_filled(rect, 6.0, fill);
        let flag = egui::Rect::from_center_size(egui::pos2(rect.min.x + 10.0 + fw / 2.0, rect.center().y), egui::vec2(fw, fw * 0.66));
        paint_flag(l, ui.painter(), flag);
        let fg = if selected { rgb(pal.on_accent) } else { rgb(pal.text) };
        ui.painter().galley(egui::pos2(rect.min.x + 16.0 + fw, rect.center().y - galley.size().y / 2.0), galley, fg);
        self.deco(ui, resp, Some(K::TipLang))
    }
}

/// Chevron (v ouvert, > ferme) trace au painter.
pub fn paint_chevron(p: &egui::Painter, c: egui::Pos2, open: bool, col: Color32) {
    let s = 5.0;
    let st = egui::Stroke::new(2.0_f32, col);
    if open {
        p.line_segment([egui::pos2(c.x - s, c.y - s * 0.5), egui::pos2(c.x, c.y + s * 0.5)], st);
        p.line_segment([egui::pos2(c.x, c.y + s * 0.5), egui::pos2(c.x + s, c.y - s * 0.5)], st);
    } else {
        p.line_segment([egui::pos2(c.x - s * 0.5, c.y - s), egui::pos2(c.x + s * 0.5, c.y)], st);
        p.line_segment([egui::pos2(c.x + s * 0.5, c.y), egui::pos2(c.x - s * 0.5, c.y + s)], st);
    }
}

impl App {
    /// En-tete a chevron dessine (style unique des cartes repliables). Retourne vrai si on vient de cliquer / valider.
    pub(super) fn chevron_header(&mut self, ui: &mut egui::Ui, open: bool, title: &str) -> bool {
        let sz = if self.big_now { 24.0 } else { 18.0 };
        self.chevron_header_sized(ui, open, title, sz)
    }

    /// Meme en-tete avec une taille de police donnee (sous-sections).
    pub(super) fn chevron_header_sized(&mut self, ui: &mut egui::Ui, open: bool, title: &str, sz: f32) -> bool {
        let h = (sz + 8.0_f32).max(ui.spacing().interact_size.y);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
        let col = ui.visuals().strong_text_color();
        paint_chevron(ui.painter(), egui::pos2(rect.left() + 12.0, rect.center().y), open, col);
        ui.painter().text(egui::pos2(rect.left() + 30.0, rect.center().y), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(sz), col);
        let resp = self.deco(ui, resp, None);
        if open {
            ui.add_space(4.0);
        }
        resp.clicked()
    }

    /// Editeur de planification (HH:MM:SS modifiables). Retourne vrai si la planification est modifiee.
    pub(super) fn schedule_editor(&self, ui: &mut egui::Ui, s: &mut crate::schedule::Schedule, id: &str) -> bool {
        use crate::schedule::{hms, SchedError, SchedMode};
        let before = s.clone();
        ui.horizontal_wrapped(|ui| {
            for (m, k) in [(SchedMode::Fixed, K::SchedFixed), (SchedMode::Interval, K::SchedInterval)] {
                let r = ui.selectable_label(s.mode == m, self.t(k));
                if self.deco(ui, r, Some(K::TipSched)).clicked() {
                    s.mode = m;
                    if m == SchedMode::Fixed && s.times_secs.is_empty() {
                        s.times_secs.push(12 * 3600);
                    }
                    if m == SchedMode::Interval && s.interval_secs == 0 {
                        s.interval_secs = 900;
                    }
                }
            }
        });
        // Trois champs numeriques heures / minutes / secondes (clavier, molette, fleches ; navigation manette via deco_kind).
        let hms_row = |ui: &mut egui::Ui, v: &mut u32, max_h: u32, tag: &str| {
            let (mut h, mut m, mut sec) = (*v / 3600, (*v / 60) % 60, *v % 60);
            ui.horizontal(|ui| {
                for (val, hi, suffix, sub) in [(&mut h, max_h, ":", "h"), (&mut m, 59u32, ":", "m"), (&mut sec, 59u32, "", "s")] {
                    ui.push_id(format!("{}_{}_{}", id, tag, sub), |ui| {
                        let r = ui.add(egui::DragValue::new(val).range(0..=hi).speed(0.2).custom_formatter(|n, _| format!("{:02}", n as u32)));
                        self.deco_kind(ui, r, Some(K::TipSched), ItemKind::Drag);
                    });
                    if !suffix.is_empty() {
                        ui.label(suffix);
                    }
                }
            });
            *v = (h * 3600 + m * 60 + sec).min(max_h * 3600 + 59 * 60 + 59);
        };
        match s.mode {
            SchedMode::Fixed => {
                let mut rm: Option<usize> = None;
                for i in 0..s.times_secs.len() {
                    ui.horizontal_wrapped(|ui| {
                        hms_row(ui, &mut s.times_secs[i], 23, &format!("t{}", i));
                        let b = ui.button(self.t(K::SchedRemove));
                        if self.deco(ui, b, Some(K::TipSched)).clicked() {
                            rm = Some(i);
                        }
                    });
                }
                if let Some(i) = rm {
                    s.times_secs.remove(i);
                }
                let b = ui.button(self.t(K::SchedAddTime));
                if self.deco(ui, b, Some(K::TipSched)).clicked() {
                    let last = s.times_secs.last().copied().unwrap_or(11 * 3600);
                    s.times_secs.push((last + 3600) % 86_400);
                }
            }
            SchedMode::Interval => {
                hms_row(ui, &mut s.interval_secs, 24, "iv");
            }
        }
        match s.validate() {
            Err(SchedError::NoTime) | Err(SchedError::BadTime) => {
                ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::SchedNoTime));
            }
            Err(SchedError::BadInterval) => {
                ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::SchedBadInterval));
            }
            Ok(()) => {
                if s.is_short() {
                    ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::SchedShort));
                }
                let now = crate::schedule::now_utc_s();
                if let Some(n) = crate::schedule::next_run_utc(s, s.last_run_utc.unwrap_or(now), now, &crate::timefmt::WinTz) {
                    let when = crate::timefmt::iso_local(n.max(0) as u64 * 1000, &crate::timefmt::WinTz);
                    ui.label(RichText::new(self.t(K::SchedNext).replace("{}", &when)).small().weak());
                }
            }
        }
        let _ = hms;
        ui.label(RichText::new(self.t(K::SchedHelp)).small().weak());
        *s != before
    }
}
