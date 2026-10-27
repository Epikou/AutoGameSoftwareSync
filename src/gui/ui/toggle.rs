//! Interrupteur anime (38 x 22, pastille de 16 qui glisse en 120 ms, fond gris -> accent) avec son libelle cliquable.
use super::{anim, col, focus_ring, layout, tokens, tokens::*};
use crate::theme::mix;
use eframe::egui::{self, Response, Sense, Ui};

/// Position de la pastille (0 = eteint, 1 = allume) a la progression `p` : PUR.
pub fn knob_x(left: f32, p: f32) -> f32 {
    left + 3.0 + (TOGGLE_W - TOGGLE_KNOB - 6.0) * anim::ease_out(p)
}

/// Interrupteur ; le clic (sur l'interrupteur ou le libelle) inverse `on`. `label` vide = interrupteur seul.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let k = if big { 1.3 } else { 1.0 };
    let fs = if big { FS_BODY * 1.3 } else { FS_BODY };
    let max_w = (ui.available_width() - TOGGLE_W * k - SP_S).max(60.0);
    let g = (!label.is_empty()).then(|| layout(ui, label, fs, col(t.text), max_w, 3));
    let h = (TOGGLE_H * k).max(g.as_ref().map(|g| g.size().y).unwrap_or(0.0));
    let w = TOGGLE_W * k + g.as_ref().map(|g| SP_S + g.size().x).unwrap_or(0.0);
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx().clone();
        let m = anim::motion(&ctx);
        let p = ctx.animate_bool_with_time(resp.id.with("knob"), *on, m.dur(DUR_FAST) as f32);
        let hover = ctx.animate_bool_with_time(resp.id.with("h"), resp.hovered(), m.dur(DUR_FAST) as f32);
        let track = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - TOGGLE_H * k / 2.0), egui::vec2(TOGGLE_W * k, TOGGLE_H * k));
        let off = if t.dark { [0x55, 0x55, 0x55] } else { [0xb5, 0xba, 0xb6] };
        let mut fill = mix(off, t.accent, p);
        if hover > 0.0 {
            fill = mix(fill, if t.dark { [255, 255, 255] } else { [0, 0, 0] }, 0.08 * hover);
        }
        let painter = ui.painter();
        painter.rect_filled(track, track.height() / 2.0, col(fill));
        let x = track.left() + (knob_x(0.0, p)) * k;
        let kr = TOGGLE_KNOB * k / 2.0;
        painter.circle_filled(egui::pos2(x + kr, track.center().y), kr, egui::Color32::WHITE);
        if let Some(g) = g {
            painter.galley(egui::pos2(track.right() + SP_S, rect.center().y - g.size().y / 2.0), g, col(t.text));
        }
        if resp.has_focus() {
            focus_ring(ui, track, track.height() / 2.0);
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pastille_aux_extremites() {
        assert_eq!(knob_x(0.0, 0.0), 3.0);
        assert_eq!(knob_x(0.0, 1.0), TOGGLE_W - TOGGLE_KNOB - 3.0);
        assert!(knob_x(0.0, 0.5) > 3.0 && knob_x(0.0, 0.5) < TOGGLE_W - TOGGLE_KNOB - 3.0);
    }
}
