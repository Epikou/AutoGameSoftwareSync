//! Pictogrammes dessines au painter (independants de la police, aucun emoji) : `Icon` (existants : coche, croix,
//! rafraichir), `Glyph` (jeu d'icones 2.0 sur une grille de 24, voir `glyphs.rs`) et icones d'etat animees (`paint_state`).
use super::super::*;

// ================================================================== pictogrammes dessines (independants de la police)

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Icon {
    Check,
    Cross,
    Refresh,
}

/// Dessine un petit pictogramme dans `r` (aucun glyphe de police : coche, croix, rafraichir, triangles).
pub(crate) fn paint_icon(p: &egui::Painter, r: egui::Rect, kind: Icon, col: Color32) {
    let c = r.center();
    let s = r.width().min(r.height()) * 0.5;
    let stroke = egui::Stroke::new((s * 0.28).max(1.6), col);
    match kind {
        Icon::Check => {
            let a = egui::pos2(c.x - s * 0.85, c.y + s * 0.05);
            let b = egui::pos2(c.x - s * 0.25, c.y + s * 0.65);
            let d = egui::pos2(c.x + s * 0.9, c.y - s * 0.65);
            p.line_segment([a, b], stroke);
            p.line_segment([b, d], stroke);
        }
        Icon::Cross => {
            let k = s * 0.7;
            p.line_segment([egui::pos2(c.x - k, c.y - k), egui::pos2(c.x + k, c.y + k)], stroke);
            p.line_segment([egui::pos2(c.x - k, c.y + k), egui::pos2(c.x + k, c.y - k)], stroke);
        }
        Icon::Refresh => {
            // Arc de cercle (300 degres) + pointe de fleche.
            let rad = s * 0.8;
            let (a0, a1) = (-0.5f32, -0.5f32 + std::f32::consts::TAU * 0.80);
            let n = 18;
            let pts: Vec<egui::Pos2> = (0..=n).map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                egui::pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts.clone(), stroke));
            let tip = *pts.last().unwrap();
            let dir = egui::vec2(-(a1.sin()), a1.cos());
            let side = egui::vec2(a1.cos(), a1.sin());
            let hs = s * 0.55;
            p.add(egui::Shape::convex_polygon(vec![tip + dir * hs, tip + side * hs * 0.8, tip - side * hs * 0.8], col, egui::Stroke::NONE));
        }
    }
}

/// Pictogramme + texte colore sur une ligne.
pub(crate) fn icon_label(ui: &mut egui::Ui, kind: Icon, text: &str, col: Color32) -> egui::Response {
    ui.horizontal(|ui| {
        let h = ui.text_style_height(&egui::TextStyle::Body);
        let (r, _) = ui.allocate_exact_size(egui::vec2(h, h), egui::Sense::hover());
        paint_icon(ui.painter(), r.shrink(1.0), kind, col);
        ui.add(egui::Label::new(RichText::new(text).color(col)).wrap())
    })
    .response
}

/// Bouton avec un pictogramme a gauche du texte (place reservee par des espaces).
pub(crate) fn icon_button(ui: &mut egui::Ui, kind: Icon, text: &str) -> egui::Response {
    let h = ui.text_style_height(&egui::TextStyle::Button);
    let sp = ui.painter().layout_no_wrap(" ".to_string(), egui::TextStyle::Button.resolve(ui.style()), Color32::WHITE).size().x.max(1.0);
    let pad = " ".repeat(((h * 1.3) / sp).ceil() as usize);
    let r = ui.button(format!("{}{}", pad, text));
    let rect = egui::Rect::from_min_size(egui::pos2(r.rect.left() + ui.spacing().button_padding.x, r.rect.center().y - h * 0.5), egui::vec2(h, h));
    let col = ui.style().interact(&r).fg_stroke.color;
    paint_icon(ui.painter(), rect, kind, col);
    r
}
