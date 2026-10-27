//! Tuile de l'accueil (168 x 148, 192 en grand format) : icone 48, nom (2 lignes au plus), etat anime + texte court,
//! menu « ... ». Survol : bordure d'accent, ombre, montee de 2 px ; appui : descente de 1 px, fond assombri de 6 % ;
//! apparition en cascade (fondu + 6 px, +30 ms par tuile). Tuile « + Ajouter » a bordure pointillee.
use super::{anim, col, col_a, focus_ring, layout, layout_line, paint_glyph, paint_state, tokens, tokens::*, Glyph, State};
use crate::theme::{mix, Rgb};
use eframe::egui::{self, pos2, Rect, Response, Sense, Stroke, Ui};
use std::hash::Hash;

/// Icone d'une tuile.
#[derive(Clone, Copy)]
pub enum TileIcon<'a> {
    /// Icone extraite du programme.
    Texture(&'a egui::TextureHandle),
    /// Initiale(s) sur un fond de couleur (aucune icone connue).
    Initial(&'a str, Rgb),
    /// Icone dessinee sur un fond de couleur (dossier, Windows, manette...).
    Glyph(Glyph, Rgb),
}

/// Contenu d'une tuile.
pub struct TileSpec<'a> {
    pub name: &'a str,
    pub icon: TileIcon<'a>,
    pub state: State,
    /// Texte d'etat court (« A l'abri · 5 min »).
    pub status: &'a str,
    /// Progression reelle d'une copie (etat `Busy`).
    pub progress: Option<f32>,
    /// Rang dans la grille (cascade d'apparition).
    pub index: usize,
    /// Cle d'apparition : la cascade rejoue quand elle change (ex. numero d'entree sur l'ecran).
    pub epoch: u64,
    /// Deuxieme ligne : etat de la synchro entre appareils (« A jour sur 2 appareils »), avec la couleur de l'appareil
    /// concerne (point) si elle est connue.
    pub sync: Option<(&'a str, Option<Rgb>)>,
    /// Largeur imposee (grille qui remplit la ligne) ; None = largeur nominale.
    pub width: Option<f32>,
    /// Pastille « Lourd » (alerte poids, 2.0.1) : texte de la pastille ; None = pas de pastille.
    pub heavy: Option<&'a str>,
}

impl<'a> TileSpec<'a> {
    /// Tuile simple (sans ligne d'appareils, largeur nominale).
    pub fn new(name: &'a str, icon: TileIcon<'a>, state: State, status: &'a str, progress: Option<f32>, index: usize, epoch: u64) -> TileSpec<'a> {
        TileSpec { name, icon, state, status, progress, index, epoch, sync: None, width: None, heavy: None }
    }
}


/// Reponse d'une tuile : clic sur la tuile, clic sur le menu « ... » (jamais atteint au clavier : la tuile s'y substitue),
/// texte raccourci (« … ») quelque part dans la tuile.
pub struct TileResponse {
    pub response: Response,
    pub menu: Response,
    pub elided: bool,
}

/// Initiales affichees quand l'icone manque (1 ou 2 lettres) : PUR.
pub fn initials(name: &str) -> String {
    // Precision entre parentheses ignoree : « Dolphin (emulateur) » -> « D ».
    let mut base = String::new();
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => base.push(c),
            _ => {}
        }
    }
    let base = if base.trim().is_empty() { name.to_string() } else { base };
    let words: Vec<&str> = base.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    match words.as_slice() {
        [] => "?".to_string(),
        [one] => one.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default(),
        [a, b, ..] => [a, b].iter().filter_map(|w| w.chars().next()).flat_map(|c| c.to_uppercase()).collect(),
    }
}

/// Bordure d'une tuile selon l'etat et le survol : PUR.
pub fn border_color(t: &Tokens, st: State, hover: f32) -> Rgb {
    let base = match st {
        State::Todo => mix(t.line, t.orange, 0.55),
        State::Problem => mix(t.line, t.red, 0.55),
        _ => t.line,
    };
    mix(base, t.accent, hover)
}

fn paint_icon(p: &egui::Painter, r: Rect, icon: TileIcon) {
    match icon {
        TileIcon::Texture(tex) => {
            p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        TileIcon::Initial(s, bg) => {
            p.rect_filled(r, 10.0, col(bg));
            let fg = crate::theme::text_on(bg);
            let g = p.layout_no_wrap(s.to_string(), super::font(r.height() * 0.42), col(fg));
            p.galley(r.center() - g.size() / 2.0, g, col(fg));
        }
        TileIcon::Glyph(gl, bg) => {
            p.rect_filled(r, 10.0, col(bg));
            let fg = mix(crate::theme::text_on(bg), bg, 0.1);
            paint_glyph(p, r.shrink(r.width() * 0.21), gl, col(fg));
        }
    }
}

/// Progression d'apparition (cascade) de l'element `index` pour la cle `epoch` : (opacite, decalage vertical).
pub fn appear(ui: &Ui, id: egui::Id, index: usize, epoch: u64) -> (f32, f32) {
    let ctx = ui.ctx();
    let m = anim::motion(ctx);
    let age = anim::age_of(ctx, id.with("appear"), epoch);
    let p = anim::run_delayed(ctx, age, m.dur(anim::cascade_delay(index)), m.dur(DUR_SLOW));
    anim::tile_in(p, m.reduced)
}

/// Tuile ; le clic sur le menu « ... » n'active pas la tuile.
pub fn tile(ui: &mut Ui, id_salt: impl Hash, spec: &TileSpec) -> TileResponse {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let (bw, h) = tile_size(big);
    let k = bw / TILE_W;
    let w = spec.width.unwrap_or(bw).max(bw);
    let id = ui.id().with(("tile", id_salt));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    let response = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let menu_rect = Rect::from_min_size(pos2(rect.right() - 12.0 * k - 22.0 * k, rect.top() + 8.0 * k), egui::vec2(26.0 * k, 22.0 * k));
    // Menu : souris seulement (au clavier / a la manette, la touche Menu ou le clic droit sur la tuile l'ouvrent).
    let menu = ui.interact(menu_rect, id.with("menu"), Sense::CLICK);
    let mut elided = false;
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx().clone();
        let m = anim::motion(&ctx);
        let (op, dy) = appear(ui, id, spec.index, spec.epoch);
        let hov = ctx.animate_bool_with_time(id.with("h"), response.hovered() || menu.hovered(), m.dur(DUR_FAST) as f32);
        let press = ctx.animate_bool_with_time(id.with("p"), response.is_pointer_button_down_on() && !menu.hovered(), m.dur(DUR_FAST) as f32 * 0.5);
        let lift = if m.reduced { 0.0 } else { anim::tile_lift(hov, press) };
        let r = rect.translate(egui::vec2(0.0, dy + lift));
        let mut p = ui.painter().clone();
        p.multiply_opacity(op);
        if hov > 0.0 && press < 1.0 {
            let sh = egui::epaint::Shadow { offset: [0, 6], blur: 14, spread: 0, color: col_a([0, 0, 0], t.shadow_alpha as f32 / 255.0 * hov * (1.0 - press)) };
            p.add(sh.as_shape(r, R_LG));
        }
        let bg = mix(t.card, t.pressed(t.card), press);
        p.rect(r, R_LG, col(bg), Stroke::new(1.0_f32, col(border_color(&t, spec.state, hov))), egui::StrokeKind::Inside);
        let pad = 12.0 * k;
        let ir = Rect::from_min_size(r.min + egui::vec2(pad, pad), egui::vec2(TILE_ICON * k, TILE_ICON * k));
        paint_icon(&p, ir, spec.icon);
        // menu « ... »
        let mc = if menu.hovered() { t.text } else { t.faint };
        paint_glyph(&p, Rect::from_center_size(menu_rect.center() + egui::vec2(0.0, dy + lift), egui::vec2(18.0 * k, 18.0 * k)), Glyph::More, col(mc));
        // pastille « Lourd » a gauche du menu
        if let Some(txt) = spec.heavy {
            let g = layout_line(ui, txt, FS_SMALL * k - 1.0, col(t.orange));
            let pw = g.size().x + 12.0 * k;
            let ph = g.size().y + 4.0 * k;
            let pr = Rect::from_min_size(pos2(menu_rect.left() - 4.0 * k - pw, menu_rect.center().y + dy + lift - ph / 2.0), egui::vec2(pw, ph));
            p.rect(pr, ph / 2.0, col(mix(t.card, t.orange, 0.22)), Stroke::new(1.0_f32, col(mix(t.card, t.orange, 0.6))), egui::StrokeKind::Inside);
            p.galley(pos2(pr.left() + 6.0 * k, pr.center().y - g.size().y / 2.0), g, col(t.orange));
        }
        // nom (2 lignes au plus)
        let g = layout(ui, spec.name, FS_BODY * k + 0.5, col(t.text), r.width() - 2.0 * pad, 2);
        elided |= g.elided;
        let name_top = ir.bottom() + 8.0 * k;
        let name_bottom = name_top + g.size().y;
        p.galley(pos2(r.left() + pad, name_top), g, col(t.text));
        // Bas de la tuile : etat (icone + texte) puis synchro entre appareils. Deux lignes chacun si besoin et si la place
        // le permet sous le nom (sinon une ligne terminee par « … », le texte complet etant en infobulle).
        let sz = 18.0 * k;
        let tc = match spec.state {
            State::Todo | State::Problem | State::Incoming => t.state_text(spec.state),
            _ => t.weak,
        };
        let dr = 3.5 * k;
        let sync_x = r.left() + pad + spec.sync.and_then(|s| s.1).map(|_| 2.0 * dr + 7.0).unwrap_or(0.0);
        let lay_sync = |rows: usize| spec.sync.map(|(txt, _)| layout(ui, txt, FS_SMALL * k - 0.5, col(t.weak), r.right() - pad - sync_x, rows));
        let lay_status = |rows: usize| layout(ui, spec.status, FS_SMALL * k, col(tc), r.width() - 2.0 * pad - sz - 6.0, rows);
        let gap = 4.0 * k;
        let fits = |gs: &Option<std::sync::Arc<egui::Galley>>, sg: &std::sync::Arc<egui::Galley>| {
            let hs = gs.as_ref().map(|g| g.size().y.max(15.0 * k) + gap).unwrap_or(0.0);
            r.bottom() - pad - hs - sz.max(sg.size().y) >= name_bottom + 4.0 * k
        };
        let (mut gs, mut sg) = (lay_sync(2), lay_status(3));
        for rows in [2, 1] {
            if !fits(&gs, &sg) {
                sg = lay_status(rows);
            }
        }
        if !fits(&gs, &sg) {
            gs = lay_sync(1);
        }
        let mut base = r.bottom() - pad;
        if let (Some(gs), Some((_, dot))) = (gs, spec.sync) {
            elided |= gs.elided;
            let h = gs.size().y.max(15.0 * k);
            let top = base - h;
            if let Some(c) = dot {
                let row = gs.rows.first().map(|x| x.rect.height()).unwrap_or(h);
                p.circle_filled(pos2(r.left() + pad + dr + 1.0, top + (h - gs.size().y) / 2.0 + row / 2.0), dr, col(c));
            }
            p.galley(pos2(sync_x, top + (h - gs.size().y) / 2.0), gs, col(t.weak));
            base = top - gap;
        }
        elided |= sg.elided;
        let hb = sz.max(sg.size().y);
        let sr = Rect::from_min_size(pos2(r.left() + pad, base - hb + (hb - sz) / 2.0), egui::vec2(sz, sz));
        let age = anim::age_of(&ctx, id.with("state"), spec.state.code());
        let off = paint_state(&p, sr, spec.state, age, spec.progress);
        p.galley(pos2(sr.right() + 6.0 + off.x, sr.center().y - sg.size().y / 2.0 + off.y), sg, col(tc));
        if response.has_focus() {
            focus_ring(ui, r, R_LG);
        }
    }
    TileResponse { response, menu, elided }
}

/// Points d'un rectangle arrondi (contour ferme), pour le pointille.
fn rounded_path(r: Rect, rad: f32) -> Vec<egui::Pos2> {
    let mut v = Vec::new();
    let corners = [(r.right() - rad, r.top() + rad, -90.0f32), (r.right() - rad, r.bottom() - rad, 0.0), (r.left() + rad, r.bottom() - rad, 90.0), (r.left() + rad, r.top() + rad, 180.0)];
    for (cx, cy, a0) in corners {
        for i in 0..=6 {
            let a = (a0 + 15.0 * i as f32).to_radians();
            v.push(pos2(cx + rad * a.cos(), cy + rad * a.sin()));
        }
    }
    v.push(v[0]);
    v
}

/// Tuile « + Ajouter » (bordure pointillee, icone plus et libelle).
pub fn add_tile(ui: &mut Ui, id_salt: impl Hash, label: &str, index: usize, epoch: u64) -> Response {
    add_tile_sized(ui, id_salt, label, index, epoch, None)
}

/// Tuile « + Ajouter » de largeur imposee (grille qui remplit la ligne).
pub fn add_tile_sized(ui: &mut Ui, id_salt: impl Hash, label: &str, index: usize, epoch: u64, width: Option<f32>) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let (bw, h) = tile_size(big);
    let w = width.unwrap_or(bw).max(bw);
    let id = ui.id().with(("addtile", id_salt));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx().clone();
        let m = anim::motion(&ctx);
        let (op, dy) = appear(ui, id, index, epoch);
        let hov = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), m.dur(DUR_FAST) as f32);
        let r = rect.translate(egui::vec2(0.0, dy));
        let mut p = ui.painter().clone();
        p.multiply_opacity(op);
        let line = mix(if t.dark { [0x4a, 0x4a, 0x4a] } else { [0xb8, 0xbd, 0xb9] }, t.accent, hov);
        p.extend(egui::Shape::dashed_line(&rounded_path(r.shrink(1.0), R_LG), Stroke::new(2.0_f32, col(line)), 6.0, 4.0));
        let fg = mix(t.weak, t.text, hov);
        let g = layout(ui, label, FS_BODY, col(fg), r.width() - 24.0, 2);
        let gs = 26.0;
        let total = gs + 6.0 + g.size().y;
        let top = r.center().y - total / 2.0;
        paint_glyph(&p, Rect::from_center_size(pos2(r.center().x, top + gs / 2.0), egui::vec2(gs, gs)), Glyph::Plus, col(fg));
        p.galley(pos2(r.center().x - g.size().x / 2.0, top + gs + 6.0), g, col(fg));
        if resp.has_focus() {
            focus_ring(ui, r, R_LG);
        }
    }
    resp
}

/// Grande tuile de choix (page Ajouter, lot 11) : disque d'icone, titre, explication. Survol et depot en cours : bordure
/// d'accent, fond teinte, ombre ; apparition en cascade. `horizontal` : icone a gauche (une tuile par ligne, fenetre etroite).
pub struct ChoiceSpec<'a> {
    pub glyph: Glyph,
    pub title: &'a str,
    pub hint: &'a str,
    /// Un element est en train d'etre depose sur la fenetre : la tuile se met en evidence.
    pub hot: bool,
    pub index: usize,
    pub epoch: u64,
    pub width: f32,
    pub horizontal: bool,
}

/// Hauteur d'une grande tuile de choix.
pub fn choice_height(horizontal: bool, big: bool) -> f32 {
    let k = if big { 1.2 } else { 1.0 };
    if horizontal {
        88.0 * k
    } else {
        152.0 * k
    }
}

pub fn choice_tile(ui: &mut Ui, id_salt: impl Hash, spec: &ChoiceSpec) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let k = if big { 1.2 } else { 1.0 };
    let h = choice_height(spec.horizontal, big);
    let id = ui.id().with(("choice", id_salt));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(spec.width, h), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx().clone();
        let m = anim::motion(&ctx);
        let (op, dy) = appear(ui, id, spec.index, spec.epoch);
        let hov = ctx.animate_bool_with_time(id.with("h"), resp.hovered() || spec.hot, m.dur(DUR_FAST) as f32);
        let press = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), m.dur(DUR_FAST) as f32 * 0.5);
        let lift = if m.reduced { 0.0 } else { anim::tile_lift(hov, press) };
        let r = rect.translate(egui::vec2(0.0, dy + lift));
        let mut p = ui.painter().clone();
        p.multiply_opacity(op);
        if hov > 0.0 && press < 1.0 {
            let sh = egui::epaint::Shadow { offset: [0, 6], blur: 14, spread: 0, color: col_a([0, 0, 0], t.shadow_alpha as f32 / 255.0 * hov * (1.0 - press)) };
            p.add(sh.as_shape(r, R_LG));
        }
        let hot = if spec.hot { 1.0 } else { 0.0 };
        let bg = mix(mix(t.card, t.pressed(t.card), press), t.accent_soft, hot * 0.7);
        let line = mix(t.line, t.accent, hov);
        p.rect(r, R_LG, col(bg), Stroke::new(if spec.hot { 2.0_f32 } else { 1.0_f32 }, col(line)), egui::StrokeKind::Inside);
        let pad = 16.0 * k;
        let disc = 44.0 * k;
        let (icon_c, text_x, text_top, text_w) = if spec.horizontal {
            (pos2(r.left() + pad + disc / 2.0, r.center().y), r.left() + pad + disc + SP_M, r.top() + 12.0 * k, r.width() - 2.0 * pad - disc - SP_M)
        } else {
            (pos2(r.left() + pad + disc / 2.0, r.top() + pad + disc / 2.0), r.left() + pad, r.top() + pad + disc + 10.0 * k, r.width() - 2.0 * pad)
        };
        p.circle_filled(icon_c, disc / 2.0, col(mix(t.accent_soft, t.accent, hov * 0.18)));
        paint_glyph(&p, Rect::from_center_size(icon_c, egui::vec2(disc * 0.5, disc * 0.5)), spec.glyph, col(t.accent_hover));
        let gt = layout(ui, spec.title, (FS_H3 + 1.0) * k, col(t.text), text_w, 2);
        let title_h = gt.size().y;
        let gh = layout(ui, spec.hint, FS_SMALL * k + 0.5, col(t.weak), text_w, if spec.horizontal { 2 } else { 3 });
        let total = title_h + 4.0 + gh.size().y;
        let top = if spec.horizontal { (r.center().y - total / 2.0).max(r.top() + 6.0) } else { text_top };
        p.galley(pos2(text_x, top), gt, col(t.text));
        p.galley(pos2(text_x, top + title_h + 4.0), gh, col(t.weak));
        if resp.has_focus() {
            focus_ring(ui, r, R_LG);
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initiales_et_bordures() {
        assert_eq!(initials("Celeste"), "C");
        assert_eq!(initials("Elden Ring"), "ER");
        assert_eq!(initials("hollow-knight silksong"), "HK");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("éclair"), "É");
        assert_eq!(initials("Dolphin (émulateur)"), "D");
        assert_eq!(initials("(Beta)"), "B");
        let t = Tokens::default_dark();
        assert_eq!(border_color(&t, State::Safe, 0.0), t.line);
        assert_eq!(border_color(&t, State::Safe, 1.0), t.accent);
        assert_ne!(border_color(&t, State::Todo, 0.0), t.line, "a faire : bordure orangee");
    }
}
