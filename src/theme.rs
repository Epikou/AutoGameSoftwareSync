//! Couleurs de l'interface : fonctions PURES (testees) pour le contraste, le cerclage de focus et
//! les fonds de survol. Le rendu egui (gui.rs) applique simplement la palette calculee ici.
pub type Rgb = [u8; 3];

/// Contraste minimal (WCAG, composants graphiques) du cerclage de focus contre le fond.
pub const RING_MIN_CONTRAST: f32 = 3.0;
/// Contraste minimal (WCAG AA) du texte sur ses fonds (normal, survol, focus, selection).
pub const TEXT_MIN_CONTRAST: f32 = 4.5;

fn lin(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Luminance relative WCAG 2.x.
pub fn luminance(c: Rgb) -> f32 {
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// Rapport de contraste WCAG (1.0 .. 21.0), symetrique.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Melange lineaire (t = 0 -> a, t = 1 -> b).
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// Noir ou blanc, selon le meilleur contraste sur `bg` (toujours >= 4.5 : le pire cas est ~4.58).
pub fn text_on(bg: Rgb) -> Rgb {
    const BLACK: Rgb = [0, 0, 0];
    const WHITE: Rgb = [255, 255, 255];
    if contrast_ratio(bg, BLACK) >= contrast_ratio(bg, WHITE) {
        BLACK
    } else {
        WHITE
    }
}

/// Couleur du cerclage : l'accent s'il est assez contraste contre TOUS les fonds sur lesquels le
/// cerclage peut se trouver, sinon la couleur du texte (elle-meme contrastee).
pub fn ring_color(backgrounds: &[Rgb], accent: Rgb, text: Rgb) -> Rgb {
    let min = |c: Rgb| backgrounds.iter().map(|b| contrast_ratio(c, *b)).fold(f32::INFINITY, f32::min);
    if min(accent) >= RING_MIN_CONTRAST {
        accent
    } else {
        text
    }
}

/// Fond de widget teinte par l'accent (survol / focus). Si l'accent est trop proche du fond pour que le
/// changement soit visible, on teinte vers la couleur du texte a la place.
fn tint(base: Rgb, accent: Rgb, text: Rgb, t: f32) -> Rgb {
    let m = mix(base, accent, t);
    let diff: i32 = (0..3).map(|i| (m[i] as i32 - base[i] as i32).abs()).sum();
    if diff >= 12 {
        m
    } else {
        mix(base, text, t * 0.6)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub panel_bg: Rgb,
    pub widget_bg: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
    /// Texte sur fond d'accent (onglet / element selectionne).
    pub on_accent: Rgb,
    pub ring: Rgb,
    pub hover_fill: Rgb,
    pub active_fill: Rgb,
}

/// Palette complete pour un theme (sombre / clair) et un accent.
pub fn palette(dark: bool, accent: Rgb) -> Palette {
    let (panel_bg, widget_bg, text) = if dark { ([27, 27, 27], [60, 60, 60], [235, 235, 235]) } else { ([248, 248, 248], [230, 230, 230], [20, 20, 20]) };
    Palette {
        panel_bg,
        widget_bg,
        text,
        accent,
        on_accent: text_on(accent),
        ring: ring_color(&[panel_bg, widget_bg], accent, text),
        hover_fill: tint(widget_bg, accent, text, 0.15),
        active_fill: tint(widget_bg, accent, text, 0.20),
    }
}
