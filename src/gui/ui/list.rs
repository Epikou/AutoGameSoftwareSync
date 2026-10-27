//! Liste : lignes cliquables (icone 34, titre, sous-titre, texte a droite), fond au survol, selection en accent doux,
//! apparition en cascade facultative (resultats du scan).
use super::{anim, col, focus_ring, layout, tile::TileIcon, tokens, tokens::*};
use crate::theme::mix;
use eframe::egui::{self, pos2, Rect, Response, Sense, Stroke, Ui};
use std::hash::Hash;

/// Contenu d'une ligne.
pub struct ListRow<'a> {
    pub icon: Option<TileIcon<'a>>,
    pub title: &'a str,
    pub subtitle: &'a str,
    /// Texte a droite (taille, « modifie il y a 2 j »).
    pub trailing: &'a str,
    pub selected: bool,
    /// Cascade d'apparition : (rang, cle) ; None = aucune animation.
    pub appear: Option<(usize, u64)>,
}

/// Ligne de liste ; pleine largeur.
pub fn list_row(ui: &mut Ui, id_salt: impl Hash, row: &ListRow) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let k = if big { 1.25 } else { 1.0 };
    let w = ui.available_width();
    let icon_s = 34.0 * k;
    let text_w = (w - 20.0 - if row.icon.is_some() { icon_s + SP_M } else { 0.0 } - 90.0).max(60.0);
    let g1 = layout(ui, row.title, FS_BODY * k, col(t.text), text_w, 1);
    let g2 = (!row.subtitle.is_empty()).then(|| layout(ui, row.subtitle, FS_SMALL * k, col(t.weak), text_w, 2));
    let h = (g1.size().y + g2.as_ref().map(|g| g.size().y + 2.0).unwrap_or(0.0)).max(icon_s) + 18.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    let resp = ui.interact(rect, ui.id().with(("row", id_salt)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx().clone();
        let m = anim::motion(&ctx);
        let (op, dy) = match row.appear {
            Some((i, epoch)) => super::tile::appear(ui, resp.id, i, epoch),
            None => (1.0, 0.0),
        };
        let hov = ctx.animate_bool_with_time(resp.id.with("h"), resp.hovered(), m.dur(DUR_FAST) as f32);
        let r = rect.translate(egui::vec2(0.0, dy));
        let mut p = ui.painter().clone();
        p.multiply_opacity(op);
        let bg = if row.selected { t.accent_soft } else { mix(t.panel, t.row_hover, hov) };
        if row.selected || hov > 0.0 {
            p.rect(r, R_MD, col(bg), if row.selected { Stroke::new(1.0_f32, col(t.accent)) } else { Stroke::NONE }, egui::StrokeKind::Inside);
        }
        let mut x = r.left() + 10.0;
        if let Some(icon) = row.icon {
            let ir = Rect::from_min_size(pos2(x, r.center().y - icon_s / 2.0), egui::vec2(icon_s, icon_s));
            match icon {
                TileIcon::Texture(tex) => {
                    p.image(tex.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
                }
                TileIcon::Initial(s, bgc) => {
                    p.rect_filled(ir, 8.0, col(bgc));
                    let fg = crate::theme::text_on(bgc);
                    let g = p.layout_no_wrap(s.to_string(), super::font(icon_s * 0.42), col(fg));
                    p.galley(ir.center() - g.size() / 2.0, g, col(fg));
                }
                TileIcon::Glyph(gl, bgc) => {
                    p.rect_filled(ir, 8.0, col(bgc));
                    super::paint_glyph(&p, ir.shrink(icon_s * 0.22), gl, col(crate::theme::text_on(bgc)));
                }
            }
            x = ir.right() + SP_M;
        }
        let th = g1.size().y + g2.as_ref().map(|g| g.size().y + 2.0).unwrap_or(0.0);
        let top = r.center().y - th / 2.0;
        let g1h = g1.size().y;
        p.galley(pos2(x, top), g1, col(t.text));
        if let Some(g2) = g2 {
            p.galley(pos2(x, top + g1h + 2.0), g2, col(t.weak));
        }
        if !row.trailing.is_empty() {
            let g3 = super::layout_line(ui, row.trailing, FS_SMALL * k, col(t.weak));
            p.galley(pos2(r.right() - 10.0 - g3.size().x, r.center().y - g3.size().y / 2.0), g3, col(t.weak));
        }
        if resp.has_focus() {
            focus_ring(ui, r, R_MD);
        }
    }
    resp
}
