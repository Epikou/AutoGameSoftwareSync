//! Animation de l'etape 1 du premier lancement (programme + donnees -> surveillance -> synchronisation -> a l'abri).
//! Icones tracees au painter, aucune police ni emoji. La machine a etats est dans `crate::banner`.
use super::*;
use crate::banner as bn;
use egui::{pos2, vec2, Pos2, Rect, Shape, Stroke};

fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    (0..=18).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / 18.0;
        pos2(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect()
}

fn arrow_head(p: &egui::Painter, tip: Pos2, dir: f32, size: f32, col: Color32) {
    let (a, b) = (dir + 2.5, dir - 2.5);
    p.add(Shape::convex_polygon(vec![tip, pos2(tip.x + size * a.cos(), tip.y + size * a.sin()), pos2(tip.x + size * b.cos(), tip.y + size * b.sin())], col, Stroke::NONE));
}

/// Fenetre d'application : cadre, barre de titre et trois points.
fn icon_window(p: &egui::Painter, r: Rect, col: Color32) {
    let st = Stroke::new((r.width() * 0.07).max(1.5), col);
    p.rect_stroke(r, 3.0, st, egui::StrokeKind::Inside);
    p.line_segment([pos2(r.min.x, r.min.y + r.height() * 0.28), pos2(r.max.x, r.min.y + r.height() * 0.28)], st);
    for i in 0..3 {
        p.circle_filled(pos2(r.min.x + r.width() * (0.18 + 0.14 * i as f32), r.min.y + r.height() * 0.14), r.width() * 0.04, col);
    }
}

/// Dossier : corps et onglet.
fn icon_folder(p: &egui::Painter, r: Rect, col: Color32) {
    let st = Stroke::new((r.width() * 0.07).max(1.5), col);
    let tab = Rect::from_min_max(r.min, pos2(r.min.x + r.width() * 0.45, r.min.y + r.height() * 0.22));
    p.rect_stroke(tab, 2.0, st, egui::StrokeKind::Inside);
    let body = Rect::from_min_max(pos2(r.min.x, r.min.y + r.height() * 0.2), r.max);
    p.rect_stroke(body, 3.0, st, egui::StrokeKind::Inside);
}

/// Oeil : amande et pupille.
fn icon_eye(p: &egui::Painter, r: Rect, col: Color32) {
    let st = Stroke::new((r.width() * 0.07).max(1.6), col);
    let (c, w, h) = (r.center(), r.width(), r.height() * 0.55);
    let mut pts: Vec<Pos2> = (0..=20).map(|i| { let u = i as f32 / 20.0; pos2(c.x - w / 2.0 + w * u, c.y - h / 2.0 * (std::f32::consts::PI * u).sin()) }).collect();
    pts.extend((0..=20).rev().map(|i| { let u = i as f32 / 20.0; pos2(c.x - w / 2.0 + w * u, c.y + h / 2.0 * (std::f32::consts::PI * u).sin()) }));
    p.add(Shape::closed_line(pts, st));
    p.circle_stroke(c, r.width() * 0.17, st);
    p.circle_filled(c, r.width() * 0.07, col);
}

/// Deux fleches circulaires (tournent de `angle`).
fn icon_sync(p: &egui::Painter, r: Rect, col: Color32, angle: f32) {
    let st = Stroke::new((r.width() * 0.08).max(1.8), col);
    let (c, rad) = (r.center(), r.width() * 0.36);
    for k in 0..2 {
        let a0 = angle + k as f32 * std::f32::consts::PI + 0.35;
        let a1 = a0 + 2.3;
        p.add(Shape::line(arc(c, rad, a0, a1), st));
        let tip = pos2(c.x + rad * a1.cos(), c.y + rad * a1.sin());
        arrow_head(p, tip, a1 + std::f32::consts::FRAC_PI_2 + 0.2, r.width() * 0.17, col);
    }
}

/// Coche dans un disque plein.
fn icon_check(p: &egui::Painter, r: Rect, col: Color32, on: Color32) {
    let c = r.center();
    p.circle_filled(c, r.width() * 0.46, col);
    let st = Stroke::new((r.width() * 0.09).max(2.0), on);
    p.add(Shape::line(vec![pos2(c.x - r.width() * 0.22, c.y + r.height() * 0.02), pos2(c.x - r.width() * 0.06, c.y + r.height() * 0.18), pos2(c.x + r.width() * 0.24, c.y - r.height() * 0.16)], st));
}

/// Petit chevron entre deux elements.
/// Legende centree dont la taille de police est reduite pour tenir dans `max_w`.
fn fit_label(p: &egui::Painter, c: Pos2, text: &str, font: egui::FontId, max_w: f32, col: Color32) {
    let w = p.layout_no_wrap(text.to_string(), font.clone(), col).size().x;
    let font = if w > max_w && w > 0.0 { egui::FontId::proportional((font.size * max_w / w).max(7.0)) } else { font };
    p.text(c, egui::Align2::CENTER_TOP, text, font, col);
}

fn small_chevron(p: &egui::Painter, c: Pos2, col: Color32) {
    let st = Stroke::new(2.0_f32, col);
    p.line_segment([pos2(c.x - 3.0, c.y - 5.0), pos2(c.x + 3.0, c.y)], st);
    p.line_segment([pos2(c.x + 3.0, c.y), pos2(c.x - 3.0, c.y + 5.0)], st);
}

impl App {
    /// Animation de l'etape 1 du premier lancement, mise a l'echelle pour tenir dans `max_w` x `max_h` (version compacte).
    /// Redessin par l'horloge centrale : 30 i/s (ambiance) ; mouvement reduit = une image par changement de phase.
    pub(super) fn welcome_animation(&self, ui: &mut egui::Ui, max_w: f32, max_h: f32) {
        let t = ui.input(|i| i.time);
        let reduced = ui::anim::motion(ui.ctx()).reduced;
        let w = if reduced { bn::weights_reduced(t) } else { bn::weights(t) };
        // un peu plus grand que l ancien bandeau (jusqu a x1,45) ; bloc programme + donnees elargi pour ses deux libelles
        let k = (max_w / bn::natural_width()).min(max_h / bn::natural_height()).clamp(0.5, 1.45);
        const BF: f32 = 3.3;
        let sizes: Vec<f32> = bn::SIZES.iter().map(|s| s * k).collect();
        let (gap, chev) = (8.0 * k, 14.0 * k);
        let total = sizes[0] * BF + sizes[1] + sizes[2] + sizes[3] + 3.0 * (gap * 2.0 + chev);
        let label_h = 18.0;
        let height = sizes[3] * 1.12 + label_h + 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(max_w.max(total), height), egui::Sense::hover());
        let pal = self.palette;
        let (accent, text, ok) = (rgb(pal.accent), rgb(pal.text), Color32::from_rgb(60, 180, 100));
        let p = ui.painter();
        let base_y = rect.min.y + sizes[3] * 1.12;
        let mut x = rect.center().x - total / 2.0;
        let fade = |c: Color32, a: f32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * a).round() as u8);
        let label_font = egui::FontId::proportional(if self.big_now { 15.0 } else { 13.0 });
        for item in 0..4 {
            let sc = bn::scale_of(w[item], reduced);
            let op = bn::opacity_of(w[item]);
            let on = w[item];
            let ink = |c: Color32| fade(c, op);
            let col_active = if item == 3 { ok } else { accent };
            let col = Color32::from_rgb(
                (text.r() as f32 + (col_active.r() as f32 - text.r() as f32) * on) as u8,
                (text.g() as f32 + (col_active.g() as f32 - text.g() as f32) * on) as u8,
                (text.b() as f32 + (col_active.b() as f32 - text.b() as f32) * on) as u8,
            );
            let s = sizes[item];
            match item {
                0 => {
                    // Deux tuiles SOUDEES en un seul bloc, sans signe « + ».
                    let (bw, bh) = (s * BF * sc, s * sc);
                    let block = Rect::from_min_size(pos2(x - (bw - s * BF) / 2.0, base_y - bh), vec2(bw, bh));
                    p.rect_filled(block, 8.0, ink(rgb(pal.widget_bg)));
                    p.rect_stroke(block, 8.0, Stroke::new(1.5_f32, ink(col)), egui::StrokeKind::Inside);
                    p.line_segment([pos2(block.center().x, block.min.y + 5.0), pos2(block.center().x, block.max.y - 5.0)], Stroke::new(1.0_f32, ink(col).gamma_multiply(0.6)));
                    let half = Rect::from_min_size(block.min, vec2(bw / 2.0, bh));
                    icon_window(p, Rect::from_center_size(half.center(), vec2(bh * 0.5, bh * 0.5)), ink(col));
                    icon_folder(p, Rect::from_center_size(pos2(half.center().x + bw / 2.0, half.center().y), vec2(bh * 0.5, bh * 0.45)), ink(col));
                    fit_label(p, pos2(half.center().x, base_y + 4.0), self.t(K::VAnimProgram), label_font.clone(), bw / 2.0 - 3.0, ink(text));
                    fit_label(p, pos2(half.center().x + bw / 2.0, base_y + 4.0), self.t(K::VAnimData), label_font.clone(), bw / 2.0 - 3.0, ink(text));
                    x += s * BF;
                }
                _ => {
                    let d = s * sc;
                    let r = Rect::from_min_size(pos2(x - (d - s) / 2.0, base_y - d), vec2(d, d));
                    p.rect_filled(r, 12.0, ink(rgb(pal.widget_bg)));
                    p.rect_stroke(r, 12.0, Stroke::new(1.5_f32, ink(col)), egui::StrokeKind::Inside);
                    let inner = r.shrink(d * 0.18);
                    match item {
                        1 => icon_eye(p, inner, ink(col)),
                        2 => icon_sync(p, inner.expand(d * 0.04), ink(col), bn::spin_angle(t, reduced)),
                        _ => icon_check(p, inner.expand(d * 0.06), ink(col), ink(rgb(pal.panel_bg))),
                    }
                    let name = match item { 1 => K::VAnimWatch, 2 => K::VAnimSync, _ => K::VAnimSafe };
                    fit_label(p, pos2(r.center().x, base_y + 4.0), self.t(name), label_font.clone(), s + gap * 2.0 + chev * 0.6, ink(text));
                    x += s;
                }
            }
            if item < 3 {
                small_chevron(p, pos2(x + gap + chev / 2.0, base_y - sizes[item.min(2) + 1] / 2.0), fade(text, 0.45));
                x += gap * 2.0 + chev;
            }
        }
        match bn::repaint_after(t, reduced) {
            None => ui::anim::need_ambient(ui.ctx()),
            Some(s) => ui::anim::wake_at(ui.ctx(), t + s),
        }
    }
}
