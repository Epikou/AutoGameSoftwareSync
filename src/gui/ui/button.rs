//! Boutons : primaire (accent), secondaire, danger, discret ; normal ou grand ; icone dessinee facultative.
//! Survol 120 ms (couleur), appui : descente de 1 px + assombrissement de 6 %. Cerclage de focus au clavier / manette.
use super::{anim, col, focus_ring, layout_line, paint_glyph, tokens, tokens::*, Glyph};
use crate::theme::{mix, Rgb};
use eframe::egui::{self, Response, Sense, Ui};

/// Genre de bouton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    /// Action principale (une par ecran) : fond d'accent.
    Primary,
    /// Action secondaire : fond neutre.
    Secondary,
    /// Action irreversible ou destructrice (« Ne plus proteger ») : fond rouge.
    Danger,
    /// Discret : contour seul (« Annuler » d'un toast, « Plus tard »).
    Ghost,
}

/// Couleurs (fond normal, fond survole, texte, contour) d'un bouton : PUR.
pub fn colors(t: &Tokens, kind: Btn) -> (Rgb, Rgb, Rgb, Option<Rgb>) {
    match kind {
        // survol : on s'eloigne de la couleur du texte (le contraste ne baisse jamais, quel que soit l'accent choisi)
        Btn::Primary => (t.accent, mix(t.accent, if t.on_accent == [255, 255, 255] { [0, 0, 0] } else { [255, 255, 255] }, 0.12), t.on_accent, None),
        Btn::Secondary => (t.widget, t.widget_hover, t.text, None),
        Btn::Danger => {
            let fg = crate::theme::text_on(t.red);
            (t.red, mix(t.red, if fg == [255, 255, 255] { [0, 0, 0] } else { [255, 255, 255] }, 0.12), fg, None)
        }
        Btn::Ghost => (t.card, t.card, t.text, Some(t.line)),
    }
}

/// Bouton texte.
pub fn button(ui: &mut Ui, kind: Btn, text: &str) -> Response {
    button_with(ui, kind, text, None, false)
}

/// Bouton complet : icone facultative, grand format (`big`).
pub fn button_with(ui: &mut Ui, kind: Btn, text: &str, glyph: Option<Glyph>, big: bool) -> Response {
    button_sized(ui, kind, text, glyph, big, 0.0)
}

/// Comme `button_with`, d'une largeur d'au moins `min_w` (contenu centre) : deux choix de meme poids cote a cote.
pub fn button_sized(ui: &mut Ui, kind: Btn, text: &str, glyph: Option<Glyph>, big: bool, min_w: f32) -> Response {
    let t = tokens(ui.ctx());
    let big = big || super::theme(ui.ctx()).big;
    let (pv, ph0) = if big { BTN_PAD_BIG } else { BTN_PAD };
    let fs = if big { FS_BIG_BUTTON } else { FS_BODY };
    let (bg, bg_h, fg, line) = colors(&t, kind);
    let g = layout_line(ui, text, fs, col(fg));
    let icon_w = if glyph.is_some() { fs + SP_S - 2.0 } else { 0.0 };
    let natural = g.size().x + icon_w + 2.0 * ph0;
    let ph = ph0 + ((min_w - natural) / 2.0).max(0.0);
    let size = egui::vec2(natural.max(min_w), g.size().y + 2.0 * pv);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx().clone();
        let m = anim::motion(&ctx);
        let hover = ctx.animate_bool_with_time(resp.id.with("h"), resp.hovered() && resp.enabled(), m.dur(DUR_FAST) as f32);
        let press = ctx.animate_bool_with_time(resp.id.with("p"), resp.is_pointer_button_down_on(), m.dur(DUR_FAST) as f32 * 0.5);
        let mut fill = mix(bg, bg_h, hover);
        if press > 0.0 {
            fill = mix(fill, t.pressed(fill), press);
        }
        let r = rect.translate(egui::vec2(0.0, anim::press_offset(press)));
        let stroke = match line {
            Some(l) => egui::Stroke::new(1.0_f32, col(if hover > 0.0 { mix(l, t.accent, hover) } else { l })),
            None => egui::Stroke::NONE,
        };
        // bouton desactive : egui estompe deja tout le dessin d'une zone desactivee (aucun voile en plus)
        let p = ui.painter();
        p.rect(r, R_SM, col(fill), stroke, egui::StrokeKind::Inside);
        let mut x = r.left() + ph;
        if let Some(gl) = glyph {
            let ir = egui::Rect::from_center_size(egui::pos2(x + fs * 0.5, r.center().y), egui::vec2(fs, fs));
            paint_glyph(p, ir, gl, col(fg));
            x += icon_w;
        }
        p.galley(egui::pos2(x, r.center().y - g.size().y / 2.0), g, col(fg));
        if resp.has_focus() {
            focus_ring(ui, r, R_SM);
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast_ratio;

    #[test]
    fn texte_des_boutons_lisible_dans_les_deux_themes() {
        for dark in [true, false] {
            let t = Tokens::new(dark, ACCENT);
            for k in [Btn::Primary, Btn::Secondary, Btn::Danger, Btn::Ghost] {
                let (bg, hov, fg, _) = colors(&t, k);
                assert!(contrast_ratio(fg, bg) >= 4.5 && contrast_ratio(fg, hov) >= 4.5, "{:?} ({})", k, dark);
            }
        }
    }
}
