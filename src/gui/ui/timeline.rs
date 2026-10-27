//! Frise « Remonter le temps » (base, completee au lot 7) : un point par version, groupes par jour, couleur = appareil,
//! version actuelle cerclee, conflit = losange orange. Curseur aimante par un ressort (leger depassement) ; clic,
//! glisser, fleches du clavier. Mise en page PURE (testee).
use super::{anim, col, focus_ring, layout_line, tokens, tokens::*};
use crate::theme::Rgb;
use eframe::egui::{self, pos2, Response, Sense, Stroke, Ui};
use std::hash::Hash;

/// Une version sur la frise.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// Jour (numero croissant fourni par l'appelant, ex. jours depuis 1970 en heure locale).
    pub day: i64,
    /// Couleur de l'appareil qui a fait la copie.
    pub color: Rgb,
    pub conflict: bool,
    pub current: bool,
    /// Copie de securite (carre creux) plutot qu'une version du coffre.
    pub safety: bool,
    /// Heure affichee au-dessus du point (« 21:14 ») quand la place le permet (toujours pour le point choisi).
    pub label: String,
}

impl Mark {
    /// Point simple (sans heure).
    pub fn new(day: i64, color: Rgb) -> Mark {
        Mark { day, color, conflict: false, current: false, safety: false, label: String::new() }
    }
}

/// Abscisses (0..1) des points : chaque jour occupe une part proportionnelle a son nombre de versions (au moins 1),
/// points regulierement espaces dans le jour, ecart entre les jours. `marks` triees par date croissante.
pub fn positions(marks: &[Mark]) -> Vec<f32> {
    if marks.is_empty() {
        return vec![];
    }
    let mut days: Vec<(i64, usize)> = Vec::new();
    for m in marks {
        match days.last_mut() {
            Some((d, n)) if *d == m.day => *n += 1,
            _ => days.push((m.day, 1)),
        }
    }
    let gap = 0.6f32;
    let total: f32 = days.iter().map(|(_, n)| *n as f32).sum::<f32>() + gap * (days.len() as f32 - 1.0).max(0.0);
    let (lo, hi) = (0.04f32, 0.96f32);
    let mut out = Vec::with_capacity(marks.len());
    let mut acc = 0.0f32;
    for (i, (_, n)) in days.iter().enumerate() {
        if i > 0 {
            acc += gap;
        }
        for j in 0..*n {
            let x = (acc + j as f32 + 0.5) / total.max(1.0);
            out.push(lo + (hi - lo) * x);
        }
        acc += *n as f32;
    }
    out
}

/// Centre (0..1) de chaque jour, pour les libelles : (jour, abscisse).
pub fn day_centers(marks: &[Mark]) -> Vec<(i64, f32)> {
    let xs = positions(marks);
    let mut out: Vec<(i64, f32, f32)> = Vec::new();
    for (m, x) in marks.iter().zip(xs) {
        match out.last_mut() {
            Some((d, a, b)) if *d == m.day => {
                *a = a.min(x);
                *b = b.max(x);
            }
            _ => out.push((m.day, x, x)),
        }
    }
    out.into_iter().map(|(d, a, b)| (d, (a + b) / 2.0)).collect()
}

/// Version la plus proche de l'abscisse `x` (aimantation).
pub fn nearest(xs: &[f32], x: f32) -> Option<usize> {
    xs.iter().enumerate().min_by(|a, b| (a.1 - x).abs().partial_cmp(&(b.1 - x).abs()).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i)
}

/// Heures affichees au-dessus des points : toutes si les points sont assez espaces (`min_gap` px), sinon seulement le
/// point choisi. PUR.
pub fn show_labels(xs: &[f32], min_gap: f32) -> bool {
    xs.windows(2).all(|w| w[1] - w[0] >= min_gap)
}

/// Point d'arrivee d'une touche (fleches, Debut, Fin) depuis `cur` sur `n` points. PUR.
pub fn key_step(cur: usize, n: usize, left: bool, right: bool, home: bool, end: bool) -> usize {
    if n == 0 {
        return 0;
    }
    let mut i = cur.min(n - 1);
    if left && i > 0 {
        i -= 1;
    }
    if right && i + 1 < n {
        i += 1;
    }
    if home {
        i = 0;
    }
    if end {
        i = n - 1;
    }
    i
}

/// Frise ; `selected` = version choisie (modifiee par clic, glisser, fleches, Debut, Fin). `day_label` donne le libelle
/// d'un jour (« Aujourd'hui », « Hier », « mardi 21 »).
pub fn timeline(ui: &mut Ui, id_salt: impl Hash, marks: &[Mark], day_label: &dyn Fn(i64) -> String, selected: &mut usize) -> Response {
    let t = tokens(ui.ctx());
    let big = super::theme(ui.ctx()).big;
    let w = ui.available_width().max(120.0);
    let h = if big { 118.0 } else { 104.0 };
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click_and_drag());
    let id = ui.id().with(("timeline", id_salt));
    let xs: Vec<f32> = positions(marks).iter().map(|x| rect.left() + x * rect.width()).collect();
    if marks.is_empty() {
        return resp;
    }
    *selected = (*selected).min(marks.len() - 1);
    // interactions : clic / glisser = point le plus proche ; fleches / Debut / Fin = voisin, premier, dernier
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.clicked() || resp.dragged() {
            if let Some(i) = nearest(&xs, pos.x) {
                if i != *selected {
                    *selected = i;
                    resp.mark_changed();
                }
            }
        }
    }
    if resp.has_focus() {
        // les fleches gauche / droite restent sur la frise (egui ne deplace pas le focus)
        ui.memory_mut(|mem| mem.set_focus_lock_filter(resp.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() }));
        let (l, r, hm, en) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Home), i.key_pressed(egui::Key::End)));
        let j = key_step(*selected, marks.len(), l, r, hm, en);
        if j != *selected {
            *selected = j;
            resp.mark_changed();
        }
    }
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let ctx = ui.ctx().clone();
    let m = anim::motion(&ctx);
    let target = xs[*selected];
    // curseur : ressort aimante (memoire : position, vitesse, temps de la derniere image)
    let now = anim::now(&ctx);
    // (l'horloge est prevenue APRES : `need_transition` reprend le verrou des donnees, jamais pendant `data_mut`)
    let (cur_x, moving) = ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_insert_with::<(anim::Spring, f64)>(id.with("spring"), || (anim::Spring::new(target), now));
        let dt = ((now - s.1) as f32 / m.slow).clamp(0.0, 0.05);
        s.1 = now;
        if m.reduced {
            s.0 = anim::Spring::new(target);
        } else {
            s.0.step(target, dt);
        }
        (s.0.pos, !s.0.settled(target))
    });
    if moving {
        anim::need_transition(&ctx);
    }
    let p = ui.painter();
    let k = if big { 1.15 } else { 1.0 };
    let label_y = rect.top() + 2.0;
    let head_y = rect.top() + 26.0 * k;
    let axis_y = rect.top() + 54.0 * k;
    let axis_col = if t.dark { [0x44, 0x44, 0x44] } else { [0xc4, 0xc9, 0xc5] };
    p.line_segment([pos2(rect.left(), axis_y), pos2(rect.right(), axis_y)], Stroke::new(2.0_f32, col(axis_col)));
    // libelles des jours, et un petit trait entre deux jours
    let mut prev_last: Option<f32> = None;
    let mut i0 = 0;
    for (d, cx) in day_centers(marks) {
        let x = rect.left() + cx * rect.width();
        let g = layout_line(ui, &day_label(d), FS_SMALL * k, col(t.weak));
        p.galley(pos2(x - g.size().x / 2.0, axis_y + 16.0 * k), g, col(t.weak));
        let n = marks[i0..].iter().take_while(|m| m.day == d).count();
        if let Some(pl) = prev_last {
            let mid = (pl + xs[i0]) / 2.0;
            p.line_segment([pos2(mid, axis_y - 7.0), pos2(mid, axis_y + 7.0)], Stroke::new(1.0_f32, col(if t.dark { [0x5a, 0x5a, 0x5a] } else { [0xa8, 0xae, 0xaa] })));
        }
        prev_last = Some(xs[i0 + n.max(1) - 1]);
        i0 += n.max(1);
    }
    let hover_x = resp.hover_pos().map(|p| p.x);
    let hovered = hover_x.and_then(|hx| nearest(&xs, hx).filter(|i| (xs[*i] - hx).abs() < 10.0));
    let all_labels = show_labels(&xs, 44.0 * k);
    for (i, (mk, x)) in marks.iter().zip(&xs).enumerate() {
        let grow = ctx.animate_bool_with_time(id.with(("pt", i)), hovered == Some(i) || i == *selected, m.dur(DUR_FAST) as f32);
        let r = (6.0 + 2.0 * grow) * k;
        let c = pos2(*x, axis_y + 1.0);
        if mk.conflict {
            let pts = vec![pos2(c.x, c.y - r - 1.0), pos2(c.x + r + 1.0, c.y), pos2(c.x, c.y + r + 1.0), pos2(c.x - r - 1.0, c.y)];
            p.add(egui::Shape::convex_polygon(pts, col(t.orange), Stroke::new(2.0_f32, col(t.panel))));
        } else if mk.safety {
            let sq = egui::Rect::from_center_size(c, egui::vec2(2.0 * r - 1.0, 2.0 * r - 1.0));
            p.rect(sq, 2.0, col(t.panel), Stroke::new(2.5_f32, col(mk.color)), egui::StrokeKind::Inside);
        } else {
            p.circle(c, r, col(mk.color), Stroke::new(2.0_f32, col(t.panel)));
        }
        if mk.current {
            p.circle_stroke(c, r + 3.0, Stroke::new(2.5_f32, col(t.text)));
        }
        if !mk.label.is_empty() && (all_labels || i == *selected || hovered == Some(i)) {
            let strong = i == *selected;
            let g = layout_line(ui, &mk.label, FS_SMALL * k, col(if strong { t.text } else { t.weak }));
            let gx = (*x - g.size().x / 2.0).clamp(rect.left(), rect.right() - g.size().x);
            p.galley(pos2(gx, label_y), g, col(if strong { t.text } else { t.weak }));
        }
    }
    // curseur
    let cx = cur_x;
    p.line_segment([pos2(cx, head_y), pos2(cx, axis_y + 12.0)], Stroke::new(2.0_f32, col(t.text)));
    p.circle_filled(pos2(cx, head_y), 6.0 * k, col(t.text));
    if resp.has_focus() {
        focus_ring(ui, rect, R_MD);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(day: i64) -> Mark {
        Mark::new(day, [1, 2, 3])
    }

    #[test]
    fn heures_et_clavier() {
        assert!(show_labels(&[0.0, 50.0, 100.0], 44.0));
        assert!(!show_labels(&[0.0, 30.0], 44.0));
        assert!(show_labels(&[10.0], 44.0), "un seul point : son heure");
        assert_eq!(key_step(2, 5, true, false, false, false), 1);
        assert_eq!(key_step(0, 5, true, false, false, false), 0, "pas avant le premier");
        assert_eq!(key_step(4, 5, false, true, false, false), 4, "pas apres le dernier");
        assert_eq!(key_step(2, 5, false, false, true, false), 0);
        assert_eq!(key_step(2, 5, false, false, false, true), 4);
        assert_eq!(key_step(9, 3, false, false, false, false), 2, "selection hors limites ramenee");
        assert_eq!(key_step(0, 0, false, true, false, false), 0);
    }

    #[test]
    fn mise_en_page_par_jour() {
        assert!(positions(&[]).is_empty());
        let one = positions(&[mk(1)]);
        assert!((one[0] - 0.5).abs() < 1e-6, "une seule version : au milieu");
        let v = vec![mk(1), mk(1), mk(2), mk(4), mk(4), mk(4)];
        let xs = positions(&v);
        assert_eq!(xs.len(), 6);
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "ordre conserve");
        assert!(xs.iter().all(|x| *x >= 0.04 && *x <= 0.96));
        // l'ecart entre deux jours est plus grand qu'entre deux versions du meme jour
        assert!(xs[2] - xs[1] > xs[1] - xs[0]);
        let days = day_centers(&v);
        assert_eq!(days.iter().map(|d| d.0).collect::<Vec<_>>(), vec![1, 2, 4]);
        assert!((days[1].1 - xs[2]).abs() < 1e-6);
        assert_eq!(nearest(&xs, 0.0), Some(0));
        assert_eq!(nearest(&xs, 1.0), Some(5));
        assert_eq!(nearest(&xs, xs[3] + 0.001), Some(3));
        assert_eq!(nearest(&[], 0.5), None);
    }
}
