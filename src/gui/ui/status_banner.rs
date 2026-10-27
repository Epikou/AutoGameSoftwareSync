//! Bandeau d'etat global (accueil) : bouclier + phrase d'etat + detail + action principale a droite.
//! Un seul message a la fois. Le bouclier « respire » (echelle 1 -> 1,04) seulement pendant une copie.
use super::{anim, button_with, col, layout, tokens, tokens::*, Btn, State};
use crate::theme::mix;
use eframe::egui::{self, pos2, Pos2, Response, Sense, Stroke, Ui};

/// Contenu du bandeau.
pub struct BannerSpec<'a> {
    pub state: State,
    /// Phrase d'etat (« Tout est a l'abri »).
    pub title: &'a str,
    /// Detail (« 12 elements proteges · derniere copie il y a 2 min »).
    pub detail: &'a str,
    /// Action principale facultative (« Tout copier maintenant »).
    pub action: Option<&'a str>,
}

/// Points du bouclier (grille 48) autour du centre `c`, a l'echelle `k`.
fn shield_points(c: Pos2, k: f32) -> Vec<Pos2> {
    [(24.0, 4.0), (40.0, 10.0), (40.0, 22.0), (39.0, 29.0), (35.5, 35.2), (30.4, 40.4), (24.0, 44.0), (17.6, 40.4), (12.5, 35.2), (9.0, 29.0), (8.0, 22.0), (8.0, 10.0)]
        .iter()
        .map(|&(x, y)| pos2(c.x + (x - 24.0) * k, c.y + (y - 24.0) * k))
        .collect()
}

/// Bouclier colore selon l'etat (coche, point d'exclamation, croix).
pub fn paint_shield(p: &egui::Painter, rect: egui::Rect, st: State, scale: f32) {
    let t = tokens(p.ctx());
    let k = rect.width().min(rect.height()) / 48.0 * scale;
    let c = rect.center();
    let base = match st {
        State::Safe | State::Busy | State::Paused => t.accent_hover,
        other => t.state(other),
    };
    let fill = mix(if t.dark { [0x0b, 0x1a, 0x16] } else { [0xff, 0xff, 0xff] }, base, if t.dark { 0.35 } else { 0.18 });
    let pts = shield_points(c, k);
    p.add(egui::Shape::convex_polygon(pts.clone(), col(fill), Stroke::NONE));
    p.add(egui::Shape::closed_line(pts, Stroke::new(2.5 * k, col(base))));
    let mark = col(if t.dark { [0xe9, 0xff, 0xf7] } else { base });
    let w = Stroke::new(3.2 * k, mark);
    let q = |x: f32, y: f32| pos2(c.x + (x - 24.0) * k, c.y + (y - 24.0) * k);
    match st {
        State::Todo => {
            p.line_segment([q(24.0, 15.0), q(24.0, 27.0)], w);
            p.circle_filled(q(24.0, 33.0), 2.0 * k, mark);
        }
        State::Problem => {
            p.line_segment([q(18.0, 18.0), q(30.0, 30.0)], w);
            p.line_segment([q(30.0, 18.0), q(18.0, 30.0)], w);
        }
        State::Incoming => {
            p.line_segment([q(24.0, 14.0), q(24.0, 31.0)], w);
            p.add(egui::Shape::line(vec![q(17.5, 25.0), q(24.0, 31.5), q(30.5, 25.0)], w));
        }
        _ => {
            p.add(egui::Shape::line(vec![q(16.0, 24.0), q(22.0, 30.0), q(33.0, 18.0)], w));
        }
    }
}

/// Bandeau ; renvoie la reponse du bouton d'action s'il y en a un.
pub fn status_banner(ui: &mut Ui, spec: &BannerSpec) -> Option<Response> {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let mut action = None;
    let border = match spec.state {
        State::Problem => mix(t.line, t.red, 0.6),
        State::Todo => mix(t.line, t.orange, 0.55),
        _ => t.line,
    };
    egui::Frame::new()
        .fill(col(t.card))
        .stroke(Stroke::new(1.0_f32, col(border)))
        .corner_radius(R_LG)
        .inner_margin(egui::Margin::symmetric(CARD_PAD.1 as i8, CARD_PAD.0 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let s = if big { 56.0 } else { 46.0 };
                let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), Sense::hover());
                let m = anim::motion(ui.ctx());
                let scale = if spec.state == State::Busy && m.ambient_on() {
                    anim::need_ambient(ui.ctx());
                    anim::breathe(m.time(anim::now(ui.ctx())))
                } else {
                    1.0
                };
                paint_shield(ui.painter(), r, spec.state, scale);
                ui.add_space(SP_M - ui.spacing().item_spacing.x);
                // place reservee au bouton a droite
                let btn_w = spec.action.map(|a| super::layout_line(ui, a, if big { FS_BIG_BUTTON } else { FS_BODY }, col(t.text)).size().x + 2.0 * BTN_PAD.1 + SP_M).unwrap_or(0.0);
                let text_w = (ui.available_width() - btn_w).max(80.0);
                ui.vertical(|ui| {
                    ui.set_width(text_w);
                    let g = layout(ui, spec.title, if big { FS_H2 * 1.25 } else { FS_H2 }, col(t.text), text_w, 2);
                    let (r1, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(r1.min, g, col(t.text));
                    if !spec.detail.is_empty() {
                        let g2 = layout(ui, spec.detail, if big { FS_BODY * 1.2 } else { FS_BODY - 1.0 }, col(t.weak), text_w, 3);
                        let (r2, _) = ui.allocate_exact_size(g2.size(), Sense::hover());
                        ui.painter().galley(r2.min, g2, col(t.weak));
                    }
                });
                if let Some(a) = spec.action {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        action = Some(button_with(ui, Btn::Primary, a, None, big));
                    });
                }
            });
        });
    action
}
