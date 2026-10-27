//! Carte : fond de carte, bordure fine, rayon 12, marge 14 x 16, titre facultatif (15 px). Pleine largeur disponible.
use super::{col, layout, tokens, tokens::*};
use eframe::egui::{self, InnerResponse, Ui};

/// Cadre egui d'une carte (reutilisable pour un contenu libre).
pub fn card_frame(t: &Tokens, highlight: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(col(t.card))
        .stroke(egui::Stroke::new(1.0_f32, col(if highlight { t.accent } else { t.line })))
        .corner_radius(R_LG)
        .inner_margin(egui::Margin::symmetric(CARD_PAD.1 as i8, CARD_PAD.0 as i8))
}

/// Carte avec un titre facultatif ; `add` dessine le contenu.
pub fn card<R>(ui: &mut Ui, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = tokens(ui.ctx());
    card_frame(&t, false).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(title) = title {
            let g = layout(ui, title, FS_H3, col(t.text), ui.available_width(), 2);
            let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
            ui.painter().galley(r.min, g, col(t.text));
            ui.add_space(SP_S - 2.0);
        }
        add(ui)
    })
}
