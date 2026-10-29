//! Jeu d'icones 2.0 dessinees au painter (grille 24 x 24, traits de 2, memes formes que les maquettes) et icones d'etat
//! animees (grille 28 x 28). Aucun glyphe de police, aucun emoji.
use super::anim;
use super::{col, col_a, State};
use eframe::egui::{self, pos2, Color32, Pos2, Rect, Stroke};

/// Icones dessinees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Home,
    Plus,
    Pc,
    Gear,
    Folder,
    Pad,
    Window,
    Code,
    Star,
    Shield,
    Drive,
    Cloud,
    Box,
    Nas,
    Usb,
    Disk,
    Eye,
    Clock,
    Check,
    Cross,
    Alert,
    ArrowDown,
    More,
    ChevronRight,
    ChevronDown,
    ChevronLeft,
}

impl Glyph {
    pub const ALL: [Glyph; 26] = [
        Glyph::Home, Glyph::Plus, Glyph::Pc, Glyph::Gear, Glyph::Folder, Glyph::Pad, Glyph::Window, Glyph::Code, Glyph::Star, Glyph::Shield, Glyph::Drive,
        Glyph::Cloud, Glyph::Box, Glyph::Nas, Glyph::Usb, Glyph::Disk, Glyph::Eye, Glyph::Clock, Glyph::Check, Glyph::Cross, Glyph::Alert, Glyph::ArrowDown,
        Glyph::More, Glyph::ChevronRight, Glyph::ChevronDown, Glyph::ChevronLeft,
    ];
}

/// Repere : coordonnees de la grille 24 x 24 -> ecran (carre centre dans `r`).
struct Grid {
    o: Pos2,
    k: f32,
}

impl Grid {
    fn new(r: Rect) -> Grid {
        let s = r.width().min(r.height());
        Grid { o: pos2(r.center().x - s / 2.0, r.center().y - s / 2.0), k: s / 24.0 }
    }
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.o.x + x * self.k, self.o.y + y * self.k)
    }
    fn pts(&self, v: &[(f32, f32)]) -> Vec<Pos2> {
        v.iter().map(|&(x, y)| self.p(x, y)).collect()
    }
    /// Arc (angles en degres, sens horaire a l'ecran).
    fn arc(&self, cx: f32, cy: f32, rad: f32, a0: f32, a1: f32) -> Vec<Pos2> {
        let n = ((a1 - a0).abs() / 12.0).ceil().max(2.0) as usize;
        (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                self.p(cx + rad * a.cos(), cy + rad * a.sin())
            })
            .collect()
    }
}

fn open(p: &egui::Painter, pts: Vec<Pos2>, s: Stroke) {
    p.add(egui::Shape::line(pts, s));
}

fn closed(p: &egui::Painter, pts: Vec<Pos2>, s: Stroke) {
    p.add(egui::Shape::closed_line(pts, s));
}

// ------------------------------------------------------------------ 2.0.3 : jeu d'icones Phosphor (police embarquee, MIT)

/// Famille de police des icones de l'interface (Phosphor « Regular » : une seule famille, un seul trait).
pub const ICON_FAMILY: &str = "agss-icons";

static ICON_FONT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ajoute la police des icones aux polices de l'interface (a appeler pour TOUTE definition de polices passee a egui).
pub fn install_icon_font(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert(ICON_FAMILY.into(), std::sync::Arc::new(egui_phosphor::Variant::Regular.font_data()));
    fonts.families.insert(egui::FontFamily::Name(ICON_FAMILY.into()), vec![ICON_FAMILY.into()]);
    ICON_FONT.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Polices de l'interface : celles d'egui + les icones.
pub fn fonts_with_icons() -> egui::FontDefinitions {
    let mut f = egui::FontDefinitions::default();
    install_icon_font(&mut f);
    f
}

/// Caractere Phosphor d'une icone.
pub fn glyph_char(g: Glyph) -> &'static str {
    use egui_phosphor::regular as ph;
    match g {
        Glyph::Home => ph::HOUSE,
        Glyph::Plus => ph::PLUS,
        Glyph::Pc => ph::DESKTOP,
        Glyph::Gear => ph::GEAR_SIX,
        Glyph::Folder => ph::FOLDER,
        Glyph::Pad => ph::GAME_CONTROLLER,
        Glyph::Window => ph::APP_WINDOW,
        Glyph::Code => ph::CODE,
        Glyph::Star => ph::STAR,
        Glyph::Shield => ph::SHIELD_CHECK,
        Glyph::Drive => ph::HARD_DRIVE,
        Glyph::Cloud => ph::CLOUD,
        Glyph::Box => ph::PACKAGE,
        Glyph::Nas => ph::HARD_DRIVES,
        Glyph::Usb => ph::USB,
        Glyph::Disk => ph::FLOPPY_DISK,
        Glyph::Eye => ph::MAGNIFYING_GLASS,
        Glyph::Clock => ph::CLOCK_COUNTER_CLOCKWISE,
        Glyph::Check => ph::CHECK,
        Glyph::Cross => ph::X,
        Glyph::Alert => ph::WARNING,
        Glyph::ArrowDown => ph::ARROW_DOWN,
        Glyph::More => ph::DOTS_THREE,
        Glyph::ChevronRight => ph::CARET_RIGHT,
        Glyph::ChevronDown => ph::CARET_DOWN,
        Glyph::ChevronLeft => ph::CARET_LEFT,
    }
}

/// Dessine un caractere Phosphor centre dans `r` (taille = cote), tourne de `angle` radians autour du centre. Faux si la
/// police n'est pas installee (l'appelant dessine alors au pinceau).
pub fn paint_char(p: &egui::Painter, r: Rect, ch: &str, c: Color32, angle: f32) -> bool {
    if !icon_font_ready() {
        return false;
    }
    let s = r.width().min(r.height());
    let gal = p.layout_no_wrap(ch.to_string(), egui::FontId::new(s, egui::FontFamily::Name(ICON_FAMILY.into())), c);
    let half = gal.size() / 2.0;
    let rot = egui::emath::Rot2::from_angle(angle);
    let pos = r.center() - rot * half;
    p.add(egui::epaint::TextShape::new(pos, gal, c).with_angle(angle));
    true
}

/// La police des icones est-elle installee (sinon : dessin au pinceau, ex. contextes egui des tests) ?
pub fn icon_font_ready() -> bool {
    ICON_FONT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Dessine l'icone `g` dans `r` avec la couleur `c`.
pub fn paint_glyph(p: &egui::Painter, r: Rect, g: Glyph, c: Color32) {
    // 2.0.3 : la manette (jugee mal dessinee) vient du jeu d'icones Phosphor ; les autres icones gardent leur dessin.
    if g == Glyph::Pad && paint_char(p, r.expand(r.width() * 0.08), glyph_char(g), c, 0.0) {
        return;
    }
    paint_glyph_drawn(p, r, g, c);
}

/// Ancien dessin au pinceau (repli).
fn paint_glyph_drawn(p: &egui::Painter, r: Rect, g: Glyph, c: Color32) {
    let gr = Grid::new(r);
    let s = Stroke::new((2.0 * gr.k).max(1.2), c);
    match g {
        Glyph::Home => closed(p, gr.pts(&[(3.0, 11.0), (12.0, 4.0), (21.0, 11.0), (21.0, 20.0), (15.0, 20.0), (15.0, 14.0), (9.0, 14.0), (9.0, 20.0), (3.0, 20.0)]), s),
        Glyph::Plus => {
            p.line_segment([gr.p(12.0, 5.0), gr.p(12.0, 19.0)], s);
            p.line_segment([gr.p(5.0, 12.0), gr.p(19.0, 12.0)], s);
        }
        Glyph::Pc => {
            p.rect_stroke(Rect::from_min_max(gr.p(3.0, 4.0), gr.p(21.0, 16.0)), 2.0 * gr.k, s, egui::StrokeKind::Middle);
            p.line_segment([gr.p(8.0, 20.0), gr.p(16.0, 20.0)], s);
            p.line_segment([gr.p(12.0, 16.0), gr.p(12.0, 20.0)], s);
        }
        Glyph::Gear => {
            p.circle_stroke(gr.p(12.0, 12.0), 3.2 * gr.k, s);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                p.line_segment([gr.p(12.0 + 6.0 * a.cos(), 12.0 + 6.0 * a.sin()), gr.p(12.0 + 9.5 * a.cos(), 12.0 + 9.5 * a.sin())], s);
            }
        }
        Glyph::Folder => closed(p, gr.pts(&[(3.0, 19.0), (3.0, 5.0), (9.0, 5.0), (11.0, 7.0), (21.0, 7.0), (21.0, 19.0)]), s),
        Glyph::Pad => {
            p.rect_stroke(Rect::from_min_max(gr.p(2.0, 8.0), gr.p(22.0, 17.5)), 4.5 * gr.k, s, egui::StrokeKind::Middle);
            p.line_segment([gr.p(7.0, 11.0), gr.p(7.0, 14.5)], s);
            p.line_segment([gr.p(5.25, 12.75), gr.p(8.75, 12.75)], s);
            p.circle_filled(gr.p(16.5, 11.5), 1.1 * gr.k, c);
            p.circle_filled(gr.p(18.5, 13.8), 1.1 * gr.k, c);
        }
        Glyph::Window => {
            p.rect_stroke(Rect::from_min_max(gr.p(3.0, 4.0), gr.p(21.0, 20.0)), 2.0 * gr.k, s, egui::StrokeKind::Middle);
            p.line_segment([gr.p(3.0, 8.0), gr.p(21.0, 8.0)], s);
            p.circle_filled(gr.p(6.0, 6.0), 0.8 * gr.k, c);
            p.circle_filled(gr.p(8.5, 6.0), 0.8 * gr.k, c);
        }
        Glyph::Code => {
            open(p, gr.pts(&[(8.0, 7.0), (3.0, 12.0), (8.0, 17.0)]), s);
            open(p, gr.pts(&[(16.0, 7.0), (21.0, 12.0), (16.0, 17.0)]), s);
        }
        Glyph::Star => {
            // etoile concave : pentagone central + 5 pointes (polygones convexes)
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = (-90.0 + i as f32 * 36.0f32).to_radians();
                    let rad = if i % 2 == 0 { 9.5 } else { 4.0 };
                    gr.p(12.0 + rad * a.cos(), 12.5 + rad * a.sin())
                })
                .collect();
            let inner: Vec<Pos2> = pts.iter().skip(1).step_by(2).copied().collect();
            p.add(egui::Shape::convex_polygon(inner, c, Stroke::NONE));
            for i in 0..5 {
                let tri = vec![pts[(2 * i + 9) % 10], pts[2 * i], pts[2 * i + 1]];
                p.add(egui::Shape::convex_polygon(tri, c, Stroke::NONE));
            }
        }
        Glyph::Shield => {
            let pts = gr.pts(&[(12.0, 2.0), (20.0, 5.0), (20.0, 11.0), (19.6, 14.5), (17.8, 17.6), (15.2, 20.2), (12.0, 22.0), (8.8, 20.2), (6.2, 17.6), (4.4, 14.5), (4.0, 11.0), (4.0, 5.0)]);
            let a = c.a() as f32 / 255.0;
            p.add(egui::Shape::convex_polygon(pts.clone(), col_a([0x0b, 0x3a, 0x2e], a), Stroke::NONE));
            closed(p, pts, Stroke::new((1.25 * gr.k).max(1.0), c));
            open(p, gr.pts(&[(8.0, 12.0), (11.0, 15.0), (16.5, 9.0)]), Stroke::new((1.6 * gr.k).max(1.2), col_a([0xe9, 0xff, 0xf7], a)));
        }
        Glyph::Drive => {
            closed(p, gr.pts(&[(8.0, 3.0), (16.0, 3.0), (22.0, 13.0), (18.0, 20.0), (6.0, 20.0), (2.0, 13.0)]), s);
            open(p, gr.pts(&[(8.0, 3.0), (14.0, 13.0), (22.0, 13.0)]), Stroke::new(s.width * 0.8, c));
            open(p, gr.pts(&[(2.0, 13.0), (14.0, 13.0), (10.0, 20.0)]), Stroke::new(s.width * 0.8, c));
        }
        Glyph::Cloud => {
            let mut pts = gr.arc(7.5, 13.5, 4.5, 90.0, 270.0);
            pts.extend(gr.arc(12.5, 10.0, 5.5, 200.0, 340.0));
            pts.extend(gr.arc(17.0, 14.0, 4.0, 270.0, 450.0));
            closed(p, pts, s);
        }
        Glyph::Box => {
            closed(p, gr.pts(&[(12.0, 3.0), (21.0, 8.0), (12.0, 13.0), (3.0, 8.0)]), s);
            open(p, gr.pts(&[(3.0, 8.0), (3.0, 16.0), (12.0, 21.0), (21.0, 16.0), (21.0, 8.0)]), s);
            p.line_segment([gr.p(12.0, 13.0), gr.p(12.0, 21.0)], s);
        }
        Glyph::Nas => {
            for y in [3.0, 13.0] {
                p.rect_stroke(Rect::from_min_max(gr.p(4.0, y), gr.p(20.0, y + 7.0)), 1.5 * gr.k, s, egui::StrokeKind::Middle);
                p.circle_filled(gr.p(8.0, y + 3.5), 1.0 * gr.k, c);
            }
        }
        Glyph::Usb => {
            p.rect_stroke(Rect::from_min_max(gr.p(8.0, 10.0), gr.p(16.0, 21.0)), 1.5 * gr.k, s, egui::StrokeKind::Middle);
            open(p, gr.pts(&[(9.5, 10.0), (9.5, 4.0), (14.5, 4.0), (14.5, 10.0)]), s);
        }
        Glyph::Disk => {
            p.rect_stroke(Rect::from_min_max(gr.p(3.0, 6.0), gr.p(21.0, 18.0)), 2.0 * gr.k, s, egui::StrokeKind::Middle);
            p.circle_filled(gr.p(17.0, 12.0), 1.2 * gr.k, c);
        }
        Glyph::Eye => {
            // deux courbes quadratiques (2,12) -> (12,cy) -> (22,12)
            let q = |t: f32, cy: f32| -> (f32, f32) {
                let x = (1.0 - t) * (1.0 - t) * 2.0 + 2.0 * (1.0 - t) * t * 12.0 + t * t * 22.0;
                let y = (1.0 - t) * (1.0 - t) * 12.0 + 2.0 * (1.0 - t) * t * cy + t * t * 12.0;
                (x, y)
            };
            let mut pts: Vec<(f32, f32)> = (0..=12).map(|i| q(i as f32 / 12.0, 3.0)).collect();
            pts.extend((1..12).rev().map(|i| q(i as f32 / 12.0, 21.0)));
            closed(p, gr.pts(&pts), s);
            p.circle_filled(gr.p(12.0, 12.0), 3.0 * gr.k, c);
        }
        Glyph::Clock => {
            p.circle_stroke(gr.p(12.0, 12.0), 9.0 * gr.k, s);
            open(p, gr.pts(&[(12.0, 7.0), (12.0, 12.0), (9.0, 14.0)]), s);
        }
        Glyph::Check => open(p, gr.pts(&[(4.5, 12.5), (9.5, 17.5), (19.5, 6.5)]), Stroke::new(s.width * 1.2, c)),
        Glyph::Cross => {
            p.line_segment([gr.p(6.0, 6.0), gr.p(18.0, 18.0)], Stroke::new(s.width * 1.1, c));
            p.line_segment([gr.p(18.0, 6.0), gr.p(6.0, 18.0)], Stroke::new(s.width * 1.1, c));
        }
        Glyph::Alert => {
            p.circle_stroke(gr.p(12.0, 12.0), 9.5 * gr.k, s);
            p.line_segment([gr.p(12.0, 7.0), gr.p(12.0, 13.0)], Stroke::new(s.width * 1.1, c));
            p.circle_filled(gr.p(12.0, 16.8), 1.4 * gr.k, c);
        }
        Glyph::ArrowDown => {
            p.line_segment([gr.p(12.0, 5.0), gr.p(12.0, 18.0)], s);
            open(p, gr.pts(&[(7.0, 13.0), (12.0, 18.0), (17.0, 13.0)]), s);
        }
        Glyph::More => {
            for x in [6.0, 12.0, 18.0] {
                p.circle_filled(gr.p(x, 12.0), 1.7 * gr.k, c);
            }
        }
        Glyph::ChevronRight => open(p, gr.pts(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]), s),
        Glyph::ChevronDown => open(p, gr.pts(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]), s),
        Glyph::ChevronLeft => open(p, gr.pts(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]), s),
    }
}

/// Trace partiel d'une ligne brisee (fraction `f` de sa longueur) : coche qui se dessine.
pub fn partial_line(pts: &[Pos2], f: f32) -> Vec<Pos2> {
    let f = f.clamp(0.0, 1.0);
    let total: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut left = total * f;
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let d = w[0].distance(w[1]);
        if left >= d {
            out.push(w[1]);
            left -= d;
        } else {
            if left > 0.0 {
                out.push(w[0] + (w[1] - w[0]) * (left / d));
            }
            break;
        }
    }
    out
}

/// Arc de cercle (angles en radians, sens horaire a l'ecran).
pub fn arc_points(c: Pos2, rad: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = (((a1 - a0).abs() * rad / 3.0).ceil() as usize).clamp(6, 64);
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
        })
        .collect()
}

/// Icone d'etat animee (grille 28 x 28 des maquettes) dans `r`. `age` : secondes depuis le changement d'etat ;
/// `progress` : progression reelle d'une copie (arc plein). Signale ses besoins a l'horloge centrale.
/// Retourne le decalage (secousse / rebond) a appliquer aussi au texte voisin.
pub fn paint_state(p: &egui::Painter, r: Rect, st: State, age: f64, progress: Option<f32>) -> egui::Vec2 {
    use super::tokens::{DUR_PULSE, DUR_SHAKE, DUR_SLOW};
    let ctx = p.ctx().clone();
    let m = anim::motion(&ctx);
    let t = super::tokens(&ctx);
    let k = r.width().min(r.height()) / 28.0;
    let mut off = egui::vec2(0.0, 0.0);
    let c0 = r.center();
    let ring = Stroke::new(2.5 * k, col(t.state(st)));
    let mark = Stroke::new(2.6 * k, col(t.state(st)));
    match st {
        State::Safe => {
            let f = anim::run(&ctx, age, m.dur(DUR_SLOW));
            p.circle_stroke(c0, 11.0 * k, ring);
            let pts: Vec<Pos2> = [(9.0, 14.5), (12.5, 18.0), (19.5, 10.5)].iter().map(|&(x, y)| pos2(c0.x + (x - 14.0) * k, c0.y + (y - 14.0) * k)).collect();
            let draw = if m.reduced { 1.0 } else { anim::check_draw(f) };
            if draw > 0.0 {
                open(p, partial_line(&pts, draw), mark);
            }
        }
        State::Busy => {
            p.circle_stroke(c0, 11.0 * k, Stroke::new(2.5 * k, col(t.line)));
            if let Some(f) = progress.filter(|f| *f > 0.0) {
                let a0 = -std::f32::consts::FRAC_PI_2;
                open(p, arc_points(c0, 11.0 * k, a0, a0 + std::f32::consts::TAU * f.clamp(0.0, 1.0)), Stroke::new(2.5 * k, col(t.accent)));
            }
            if m.ambient_on() {
                let a = anim::ring_angle(m.time(anim::now(&ctx)));
                open(p, arc_points(c0, 11.0 * k, a, a + 1.8), ring);
                anim::need_ambient(&ctx);
            } else {
                // mouvement reduit : arc fixe, rien ne tourne
                open(p, arc_points(c0, 11.0 * k, -1.57, 0.23), ring);
            }
        }
        State::Todo | State::Paused => {
            let dur = DUR_PULSE * anim::PULSE_COUNT * m.slow as f64;
            let pulsing = m.ambient_on() && age < dur;
            let (dot, halo, ha) = if pulsing { anim::pulse(age / m.slow as f64) } else { (1.0, 0.0, 0.0) };
            if pulsing {
                anim::need_ambient(&ctx);
                // dernier redessin a la fin des pulsations (image finale immobile)
                anim::wake_at(&ctx, anim::now(&ctx) + (dur - age));
            }
            if st == State::Todo {
                // anneau orange + point d'exclamation ; petit rebond a l'apparition
                let hp = anim::run(&ctx, age, m.dur(0.5));
                if !m.reduced {
                    off.y = anim::hop_dy(hp) * k;
                }
                let c = c0 + off;
                if ha > 0.0 {
                    p.circle_filled(c, 11.0 * k * (0.8 + 0.4 * (halo - 0.6)), col_a(t.orange, ha * 0.55));
                }
                p.circle_stroke(c, 11.0 * k, ring);
                p.line_segment([c + egui::vec2(0.0, -6.0 * k), c + egui::vec2(0.0, 1.0 * k)], mark);
                p.circle_filled(c + egui::vec2(0.0, 5.5 * k), 1.6 * k, col(t.state(st)));
            } else {
                if ha > 0.0 {
                    p.circle_filled(c0, 7.0 * k * halo, col_a(t.accent_hover, ha));
                }
                p.circle_filled(c0, 6.0 * k * dot, col(t.accent_hover));
            }
        }
        State::Problem => {
            let f = anim::run(&ctx, age, m.dur(DUR_SHAKE));
            if !m.reduced {
                off.x = anim::shake_dx(f) * k;
            }
            let c = c0 + off;
            p.circle_stroke(c, 11.0 * k, ring);
            let d = 4.0 * k;
            p.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], mark);
            p.line_segment([c + egui::vec2(d, -d), c + egui::vec2(-d, d)], mark);
        }
        State::Incoming => {
            let f = anim::run(&ctx, age, m.dur(0.8));
            let e = anim::ease_out_cubic(f);
            if !m.reduced && f < 1.0 {
                p.circle_filled(c0, 10.0 * k * (0.6 + 0.9 * e), col_a(t.blue, 0.35 * (1.0 - e)));
            }
            p.circle_stroke(c0, 11.0 * k, ring);
            let dy = if m.reduced { 0.0 } else { -6.0 * k * (1.0 - e) };
            let ac = Stroke::new(2.5 * k, col_a(t.blue, if m.reduced { 1.0 } else { e }));
            let top = c0 + egui::vec2(0.0, -6.0 * k + dy);
            let bot = c0 + egui::vec2(0.0, 4.0 * k + dy);
            p.line_segment([top, bot], ac);
            open(p, vec![bot + egui::vec2(-4.5 * k, -4.5 * k), bot, bot + egui::vec2(4.5 * k, -4.5 * k)], ac);
        }
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_partiel_de_la_coche() {
        let pts = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0)];
        assert_eq!(partial_line(&pts, 0.0), vec![pos2(0.0, 0.0)]);
        assert_eq!(partial_line(&pts, 0.25), vec![pos2(0.0, 0.0), pos2(5.0, 0.0)]);
        assert_eq!(partial_line(&pts, 0.75), vec![pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 5.0)]);
        assert_eq!(partial_line(&pts, 1.0), pts.to_vec());
        let a = arc_points(pos2(0.0, 0.0), 10.0, 0.0, std::f32::consts::PI);
        assert!((a[0].x - 10.0).abs() < 1e-4 && (a.last().unwrap().x + 10.0).abs() < 1e-4);
    }
}
