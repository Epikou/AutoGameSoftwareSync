//! Champ de saisie : libelle au-dessus (12 px, secondaire), zone sur fond de carte secondaire, bordure fine qui passe
//! a l'accent au focus (120 ms), texte d'aide en gris quand le champ est vide. Pleine largeur disponible.
use super::{anim, col, layout_line, tokens, tokens::*};
use crate::theme::mix;
use eframe::egui::{self, Response, Ui};
use std::hash::Hash;

/// Champ d'une ligne ; `label` vide = pas de libelle. Renvoie la reponse du texte (`changed()`, `lost_focus()`...).
pub fn field(ui: &mut Ui, id_salt: impl Hash, label: &str, text: &mut String, hint: &str) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let fs = if big { FS_BODY * 1.3 } else { FS_BODY };
    let id = ui.id().with(("field", id_salt));
    if !label.is_empty() {
        let g = layout_line(ui, label, FS_SMALL, col(t.weak));
        let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
        ui.painter().galley(r.min, g, col(t.weak));
        ui.add_space(SP_XS);
    }
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let m = anim::motion(ui.ctx());
    let f = ui.ctx().animate_bool_with_time(id.with("focus"), focused, m.dur(DUR_FAST) as f32);
    let border = mix(t.line, t.accent, f);
    egui::Frame::new()
        .fill(col(t.card2))
        .stroke(egui::Stroke::new(1.0 + f, col(border)))
        .corner_radius(R_MD)
        .inner_margin(egui::Margin::symmetric(10, if big { 9 } else { 6 }))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .id(id)
                    .frame(false)
                    .font(super::font(fs))
                    .text_color(col(t.text))
                    .hint_text(egui::RichText::new(hint).color(col(t.faint)).size(fs))
                    .desired_width(f32::INFINITY),
            )
        })
        .inner
}
