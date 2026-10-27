//! Animation de l'etape 1 du premier lancement (« Ton programme + Ses donnees » -> « On surveille » -> « On synchronise »
//! -> « A l'abri ! », ancien bandeau du tutoriel). Machine a etats PURE : quatre phases de 2,6 s en boucle, un element actif
//! mis en evidence a la fois. Le dessin (icones tracees au painter) est dans `gui::banner`.

/// Duree d'une phase (secondes).
pub const PHASE_SECS: f64 = 2.6;
/// Nombre de phases / d'elements mis en evidence.
pub const PHASES: usize = 4;
/// Tailles (px) des quatre elements, en escalier croissant : bloc exe+save, oeil, fleches, coche.
pub const SIZES: [f32; PHASES] = [46.0, 58.0, 76.0, 96.0];
/// Part de la phase (fin) pendant laquelle l'element actif s'estompe et le suivant apparait.
const FADE_FRAC: f64 = 0.15;

/// Phase courante (0..4) et avancement (0..1) a l'instant `t` (secondes).
pub fn phase_at(t: f64) -> (usize, f64) {
    let t = if t.is_finite() && t >= 0.0 { t } else { 0.0 };
    let cycle = t % (PHASE_SECS * PHASES as f64);
    let p = (cycle / PHASE_SECS).floor() as usize;
    (p.min(PHASES - 1), (cycle - p as f64 * PHASE_SECS) / PHASE_SECS)
}

fn smooth(x: f64) -> f32 {
    let x = x.clamp(0.0, 1.0);
    (x * x * (3.0 - 2.0 * x)) as f32
}

/// Poids de mise en evidence (0 = estompe, 1 = actif) des quatre elements. `reduced` : bascule nette sans interpolation
/// (l'interface remplace alors l'animation par un fondu).
pub fn weights(t: f64) -> [f32; PHASES] {
    let (p, q) = phase_at(t);
    let k = smooth((q - (1.0 - FADE_FRAC)) / FADE_FRAC);
    let mut w = [0.0; PHASES];
    w[p] = 1.0 - k;
    w[(p + 1) % PHASES] += k;
    w
}

/// Poids sans interpolation : seul l'element de la phase est actif.
pub fn weights_reduced(t: f64) -> [f32; PHASES] {
    let mut w = [0.0; PHASES];
    w[phase_at(t).0] = 1.0;
    w
}

/// Facteur d'agrandissement d'un element de poids `w` (1.0 sans animation).
pub fn scale_of(w: f32, reduced: bool) -> f32 {
    if reduced { 1.0 } else { 1.0 + 0.12 * w }
}

/// Opacite d'un element de poids `w` : 50 % estompe, 100 % actif.
pub fn opacity_of(w: f32) -> f32 {
    0.5 + 0.5 * w
}

/// Angle (radians) des fleches circulaires : tournent seulement pendant leur phase et hors mode reduit.
pub fn spin_angle(t: f64, reduced: bool) -> f32 {
    if reduced || phase_at(t).0 != 2 {
        return 0.0;
    }
    ((t * std::f64::consts::TAU / 1.4) % std::f64::consts::TAU) as f32
}

/// Indice de la legende (= phase).
pub fn caption_index(t: f64) -> usize {
    phase_at(t).0
}

/// Delai avant le prochain changement visible : `None` = repeindre a chaque image (animation fluide), `Some(s)` = une seule
/// image au changement de phase (mode reduit).
pub fn repaint_after(t: f64, reduced: bool) -> Option<f64> {
    if !reduced {
        return None;
    }
    let (p, q) = phase_at(t);
    let _ = p;
    Some(((1.0 - q) * PHASE_SECS).max(0.05))
}

/// Largeur totale du bandeau (sans mise a l'echelle) : bloc exe+save (deux tuiles soudees) + trois autres elements + chevrons.
pub fn natural_width() -> f32 {
    let (gap, chev) = (8.0, 14.0);
    SIZES[0] * 2.0 + SIZES[1] + SIZES[2] + SIZES[3] + 3.0 * (gap * 2.0 + chev)
}

// ------------------------------------------------------------------ mise a l'echelle (etape 1 du premier lancement)

const LABEL_H: f32 = 18.0;

/// Hauteur naturelle de l'animation (icones en escalier + libelles).
pub fn natural_height() -> f32 {
    SIZES[3] * 1.12 + LABEL_H + 6.0
}

/// Echelle (0.5..1) pour tenir dans `avail_w` x `avail_h` : la version compacte remplace tout deplacement.
pub fn fit_scale(avail_w: f32, avail_h: f32) -> f32 {
    (avail_w / natural_width()).min(avail_h / natural_height()).clamp(0.5, 1.0)
}
