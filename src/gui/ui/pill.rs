//! Pastilles : icone d'etat animee seule, pastille d'etat (icone + texte court), filtre en pastille (avec compte),
//! pastille d'appareil (nom sur sa couleur).
use super::{anim, col, focus_ring, layout_line, paint_state, tokens, tokens::*, State};
use crate::theme::{mix, Rgb};
use eframe::egui::{self, Response, Sense, Ui};
use std::hash::Hash;

/// Icone d'etat seule (`size` px). L'animation d'entree rejoue quand l'etat change ; `progress` : copie en cours (0..1).
pub fn state_icon(ui: &mut Ui, id_salt: impl Hash, st: State, size: f32, progress: Option<f32>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    if ui.is_rect_visible(rect) {
        let id = ui.id().with(("state", id_salt));
        let age = anim::age_of(ui.ctx(), id, st.code());
        paint_state(ui.painter(), rect, st, age, progress);
    }
    resp
}

/// Pastille d'etat : icone animee + texte d'etat (12 mots au plus, voir ETUDE §6). Couleur lisible de l'etat.
pub fn state_pill(ui: &mut Ui, id_salt: impl Hash, st: State, text: &str) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let fs = if big { FS_SMALL * 1.3 } else { FS_SMALL + 1.0 };
    let icon = if big { 24.0 } else { 18.0 };
    let tc = if st == State::Safe || st == State::Busy || st == State::Paused { t.weak } else { t.state_text(st) };
    let g = layout_line(ui, text, fs, col(tc));
    let size = egui::vec2(icon + SP_S - 2.0 + g.size().x, icon.max(g.size().y));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let id = ui.id().with(("pill", id_salt));
        let age = anim::age_of(ui.ctx(), id, st.code());
        let ir = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - icon / 2.0), egui::vec2(icon, icon));
        let off = paint_state(ui.painter(), ir, st, age, None);
        ui.painter().galley(egui::pos2(ir.right() + SP_S - 2.0 + off.x, rect.center().y - g.size().y / 2.0 + off.y), g, col(tc));
    }
    resp
}

/// Couleurs (fond, contour, texte, texte du compte) d'un filtre en pastille : PUR.
pub fn chip_colors(t: &Tokens, selected: bool, hover: f32) -> (Rgb, Rgb, Rgb, Rgb) {
    if selected {
        (t.accent, t.accent, t.on_accent, mix(t.on_accent, t.accent, 0.25))
    } else {
        (t.card2, mix(t.line, t.accent, hover), t.text, t.weak)
    }
}

/// Filtre en pastille (« Tout 12 », « Jeux 6 ») ; un clic le selectionne.
pub fn chip(ui: &mut Ui, text: &str, count: Option<usize>, selected: bool) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let fs = if big { FS_BODY * 1.2 } else { FS_SMALL + 1.0 };
    let g = layout_line(ui, text, fs, col(t.text));
    let n = count.map(|n| layout_line(ui, &n.to_string(), fs - 1.0, col(t.weak)));
    let pad = egui::vec2(if big { 14.0 } else { 11.0 }, if big { 7.0 } else { 4.0 });
    let w = g.size().x + n.as_ref().map(|n| SP_S - 2.0 + n.size().x).unwrap_or(0.0) + 2.0 * pad.x;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, g.size().y + 2.0 * pad.y), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let m = anim::motion(ui.ctx());
        let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), m.dur(DUR_FAST) as f32);
        let (bg, line, fg, nc) = chip_colors(&t, selected, hover);
        let p = ui.painter();
        p.rect(rect, rect.height() / 2.0, col(bg), egui::Stroke::new(1.0_f32, col(line)), egui::StrokeKind::Inside);
        let gx = rect.left() + pad.x;
        let gy = rect.center().y - g.size().y / 2.0;
        let gw = g.size().x;
        p.galley_with_override_text_color(egui::pos2(gx, gy), g, col(fg));
        if let Some(n) = n {
            p.galley_with_override_text_color(egui::pos2(gx + gw + SP_S - 2.0, rect.center().y - n.size().y / 2.0), n, col(nc));
        }
        if resp.has_focus() {
            focus_ring(ui, rect, rect.height() / 2.0);
        }
    }
    resp
}

/// Pastille d'appareil : nom de l'appareil sur sa couleur (texte noir ou blanc selon le contraste).
pub fn device_pill(ui: &mut Ui, name: &str, color: Rgb) -> Response {
    let fg = crate::theme::text_on(color);
    let g = layout_line(ui, name, FS_SMALL, col(fg));
    let pad = egui::vec2(9.0, 2.0);
    let (rect, resp) = ui.allocate_exact_size(g.size() + pad * 2.0, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, rect.height() / 2.0, col(color));
        ui.painter().galley(rect.min + pad, g, col(fg));
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast_ratio;

    #[test]
    fn filtre_lisible_selectionne_ou_non() {
        for dark in [true, false] {
            let t = Tokens::new(dark, ACCENT);
            for sel in [true, false] {
                let (bg, _, fg, n) = chip_colors(&t, sel, 0.0);
                assert!(contrast_ratio(fg, bg) >= 4.5, "texte ({}, {})", dark, sel);
                assert!(contrast_ratio(n, bg) >= 3.0, "compte ({}, {})", dark, sel);
            }
            for s in State::ALL {
                assert!(contrast_ratio(t.state_text(s), t.card) >= 4.5, "{:?} ({})", s, dark);
            }
        }
    }

    #[test]
    fn ordre_de_tri_des_etats() {
        let mut v = State::ALL.to_vec();
        v.sort_by_key(|s| s.sort_rank());
        assert_eq!(v[0], State::Problem);
        assert_eq!(v[1], State::Todo);
        assert_eq!(v[2], State::Busy);
        assert_eq!(*v.last().unwrap(), State::Safe);
        let codes: std::collections::HashSet<u64> = State::ALL.iter().map(|s| s.code()).collect();
        assert_eq!(codes.len(), 6);
    }
}
