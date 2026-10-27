//! Systeme d'animations (ETUDE §7). Partie PURE (testee) : courbes, interpolations, ressort, formes des animations
//! (fonctions du temps), preferences de mouvement et HORLOGE CENTRALE qui decide s'il faut redessiner :
//!  - une transition est en cours  -> redessin a la frequence de l'ecran ;
//!  - seule une ambiance tourne (anneau de copie) -> 30 images par seconde ;
//!  - rien ne bouge -> AUCUN redessin (sauf un reveil programme, ex. expiration d'un toast).
//! Partie egui (en bas) : les composants signalent leurs besoins a l'horloge via la memoire du contexte, sans passer
//! d'etat a la main ; l'application lit et applique la decision une fois par image.
use super::tokens::*;
use std::f32::consts::{PI, TAU};
use std::time::Duration;

// ------------------------------------------------------------------ courbes

pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Constante du « back out » donnant un depassement maximal de 6 % (4 s^3 / 27 (s+1)^2 = 0,06).
pub const BACK_S: f32 = 1.2825;

/// Arrivee avec leger depassement (6 %) puis retour : apparition de tuile, coche, toast.
pub fn back_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let u = t - 1.0;
    1.0 + (BACK_S + 1.0) * u * u * u + BACK_S * u * u
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ease {
    Linear,
    Out,
    OutCubic,
    InOut,
    BackOut,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Ease::Linear => t.clamp(0.0, 1.0),
            Ease::Out => ease_out(t),
            Ease::OutCubic => ease_out_cubic(t),
            Ease::InOut => ease_in_out(t),
            Ease::BackOut => back_out(t),
        }
    }
}

/// Progression lineaire 0..1 d'une animation commencee a `start` (secondes), de duree `dur`.
pub fn progress(start: f64, now: f64, dur: f64) -> f32 {
    if dur <= 0.0 {
        return 1.0;
    }
    ((now - start) / dur).clamp(0.0, 1.0) as f32
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Interpolation d'une valeur de `from` a `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub start: f64,
    pub dur: f64,
    pub ease: Ease,
}

impl Tween {
    pub fn new(from: f32, to: f32, start: f64, dur: f64, ease: Ease) -> Tween {
        Tween { from, to, start, dur, ease }
    }

    /// Valeur fixe (aucune animation).
    pub fn fixed(v: f32) -> Tween {
        Tween { from: v, to: v, start: 0.0, dur: 0.0, ease: Ease::Linear }
    }

    pub fn value(&self, now: f64) -> f32 {
        lerp(self.from, self.to, self.ease.apply(progress(self.start, now, self.dur)))
    }

    pub fn done(&self, now: f64) -> bool {
        self.dur <= 0.0 || now >= self.start + self.dur
    }

    /// Nouvelle cible : repart de la valeur courante (aucun saut).
    pub fn retarget(&mut self, to: f32, now: f64, dur: f64) {
        if (to - self.to).abs() < f32::EPSILON {
            return;
        }
        self.from = self.value(now);
        self.to = to;
        self.start = now;
        self.dur = dur;
    }
}

/// Ressort amorti (curseur aimante de la frise) : integration semi-implicite par pas de 1/240 s au plus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub pos: f32,
    pub vel: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    /// Ressort legerement sous-amorti : un petit depassement, puis immobile en ~300 ms.
    pub fn new(pos: f32) -> Spring {
        Spring { pos, vel: 0.0, stiffness: 320.0, damping: 26.0 }
    }

    pub fn step(&mut self, target: f32, dt: f32) {
        let mut left = dt.clamp(0.0, 0.25);
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            let acc = self.stiffness * (target - self.pos) - self.damping * self.vel;
            self.vel += acc * h;
            self.pos += self.vel * h;
            left -= h;
        }
        if self.settled(target) {
            self.pos = target;
            self.vel = 0.0;
        }
    }

    pub fn settled(&self, target: f32) -> bool {
        (self.pos - target).abs() < 0.05 && self.vel.abs() < 0.5
    }
}

// ------------------------------------------------------------------ preferences de mouvement

/// Preferences de mouvement : reglage « Animations » de l'appli + reglage du systeme (Windows : effets d'animation ;
/// Linux : GNOME / KDE Plasma), et facteur de ralenti (diagnostic : `AGSS_UI_SLOWMO`, captures d'animations).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub reduced: bool,
    pub slow: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Motion { reduced: false, slow: 1.0 }
    }
}

impl Motion {
    /// `mode` : reglage de l'appli ; `system_on` : les animations du systeme sont activees.
    pub fn from_settings(mode: crate::gamedetect::CardAnim, system_on: bool, slow: f32) -> Motion {
        Motion { reduced: crate::gamedetect::reduce_decision(mode, system_on).0, slow: if slow.is_finite() && slow >= 1.0 { slow.min(50.0) } else { 1.0 } }
    }

    /// Duree effective d'une transition : mouvement reduit = fondu court ; ralenti applique.
    pub fn dur(&self, d: f64) -> f64 {
        let d = if self.reduced { d.min(DUR_REDUCED_FADE) } else { d };
        d * self.slow as f64
    }

    /// Une animation d'ambiance (anneau qui tourne, pulsation) peut-elle jouer ?
    pub fn ambient_on(&self) -> bool {
        !self.reduced
    }

    /// Temps « ralenti » (pour les formes cycliques).
    pub fn time(&self, now: f64) -> f64 {
        now / self.slow as f64
    }
}

// ------------------------------------------------------------------ formes des animations (p = progression 0..1)

/// Transition de page : (opacite, decalage vertical en px). Fondu + glissement de 8 px ; reduit = fondu seul.
pub fn page_in(p: f32, reduced: bool) -> (f32, f32) {
    if reduced {
        return (p.clamp(0.0, 1.0), 0.0);
    }
    let e = ease_out_cubic(p);
    (e, 8.0 * (1.0 - e))
}

/// Etape suivante / precedente (premier lancement) : (opacite, decalage horizontal en px), 24 px dans le sens `dir`.
pub fn step_in(p: f32, dir: f32, reduced: bool) -> (f32, f32) {
    if reduced {
        return (p.clamp(0.0, 1.0), 0.0);
    }
    let e = ease_out_cubic(p);
    (e, 24.0 * dir.signum() * (1.0 - e))
}

/// Apparition d'une tuile (ou d'une ligne de resultat) : (opacite, decalage vertical en px, 6 px avec depassement).
pub fn tile_in(p: f32, reduced: bool) -> (f32, f32) {
    if reduced {
        return (p.clamp(0.0, 1.0), 0.0);
    }
    (ease_out(p), 6.0 * (1.0 - back_out(p)))
}

/// Retard de la cascade pour l'element `i` (+30 ms par element ; au-dela de 10, tous partent avec le 10e).
pub fn cascade_delay(i: usize) -> f64 {
    i.min(CASCADE_MAX) as f64 * DUR_CASCADE_STEP
}

/// Decalage vertical d'une tuile : survol = montee de 2 px ; appui = descente de 1 px (l'appui l'emporte).
pub fn tile_lift(hover: f32, press: f32) -> f32 {
    let (h, p) = (hover.clamp(0.0, 1.0), press.clamp(0.0, 1.0));
    lerp(-2.0 * h, 1.0, p)
}

/// Bouton : descente de 1 px a l'appui.
pub fn press_offset(press: f32) -> f32 {
    press.clamp(0.0, 1.0)
}

/// Anneau de copie : angle de depart de l'arc (radians) a l'instant `t` (un tour par `DUR_RING`).
pub fn ring_angle(t: f64) -> f32 {
    (((t / DUR_RING).fract()) as f32) * TAU - PI / 2.0
}

/// Coche de succes : fraction du trace dessinee (0..1), 320 ms.
pub fn check_draw(p: f32) -> f32 {
    ease_out(p)
}

/// Pulsation (« a faire », pause) a l'instant `t` : (echelle du point 0,85..1, echelle du halo 0,6..1,6, opacite du halo 0,45..0).
pub fn pulse(t: f64) -> (f32, f32, f32) {
    let ph = (t / DUR_PULSE).fract() as f32;
    let dot = 0.925 - 0.075 * (ph * TAU).cos();
    let halo = lerp(0.6, 1.6, ease_out(ph));
    let alpha = 0.45 * (1.0 - ease_out(ph));
    (dot, halo, alpha)
}

/// Nombre de pulsations jouees apres un changement d'etat, puis l'etat reste immobile (le calme par defaut).
pub const PULSE_COUNT: f64 = 3.0;

/// Secousse d'erreur (une fois) : decalage horizontal en px, amplitude 3 px amortie ; 0 au debut et a la fin.
pub fn shake_dx(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    3.0 * (1.0 - p) * (p * 3.0 * TAU).sin()
}

/// Petit rebond (« a verifier », une fois) : decalage vertical en px (vers le haut = negatif), 4 px.
pub fn hop_dy(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    -4.0 * (p * PI).sin() * (1.0 - p)
}

/// Respiration du bouclier du bandeau pendant une copie : echelle 1..1,04, periode 2,4 s.
pub fn breathe(t: f64) -> f32 {
    let ph = (t / 2.4).fract() as f32;
    1.0 + 0.02 * (1.0 - (ph * TAU).cos())
}

/// Eclat de reussite : point `i` sur `n` a la progression `p` : (dx, dy, opacite), distance maximale `radius`.
pub fn burst(i: usize, n: usize, p: f32, radius: f32) -> (f32, f32, f32) {
    let a = i as f32 / n.max(1) as f32 * TAU;
    let d = radius * ease_out_cubic(p);
    (a.cos() * d, a.sin() * d, 1.0 - p.clamp(0.0, 1.0))
}

/// Toast : (opacite, decalage vertical en px) : arrive du bas (16 px) avec un leger depassement.
pub fn toast_in(p: f32, reduced: bool) -> (f32, f32) {
    if reduced {
        return (p.clamp(0.0, 1.0), 0.0);
    }
    (ease_out(p), 16.0 * (1.0 - back_out(p)))
}

/// Dialogue : (opacite, decalage vertical en px, 10 px).
pub fn dialog_in(p: f32, reduced: bool) -> (f32, f32) {
    if reduced {
        return (p.clamp(0.0, 1.0), 0.0);
    }
    (ease_out(p), 10.0 * (1.0 - back_out(p)))
}

/// Compteur qui defile (600 ms, ease_out_cubic).
pub fn counter(from: i64, to: i64, p: f32) -> i64 {
    (from as f64 + (to - from) as f64 * ease_out_cubic(p) as f64).round() as i64
}

// ------------------------------------------------------------------ horloge centrale

/// Besoin de redessin signale par une animation pendant l'image courante (ordre = priorite).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Need {
    #[default]
    Idle,
    /// Ambiance (anneau de copie) : 30 images par seconde.
    Ambient,
    /// Transition : frequence de l'ecran.
    Transition,
}

/// Decision de l'horloge pour l'image suivante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repaint {
    /// Aucun redessin (rien ne bouge).
    Idle,
    /// Redessin immediat (synchronise a l'ecran).
    Now,
    /// Redessin apres ce delai.
    After(Duration),
}

impl Repaint {
    /// Combine avec un besoin de sondage periodique de l'application (`None` = aucun) : le plus proche l'emporte.
    pub fn with_poll(self, poll: Option<Duration>) -> Repaint {
        match (self, poll) {
            (r, None) => r,
            (Repaint::Now, _) => Repaint::Now,
            (Repaint::Idle, Some(p)) => Repaint::After(p),
            (Repaint::After(a), Some(p)) => Repaint::After(a.min(p)),
        }
    }
}

/// Besoins accumules pendant une image.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameNeeds {
    pub need: Need,
    /// Reveil programme le plus proche (temps absolu en secondes), ex. debut differe d'une cascade, fin d'un toast.
    pub wake_at: Option<f64>,
}

impl FrameNeeds {
    pub fn transition(&mut self) {
        self.need = self.need.max(Need::Transition);
    }

    pub fn ambient(&mut self) {
        self.need = self.need.max(Need::Ambient);
    }

    pub fn wake_at(&mut self, t: f64) {
        self.wake_at = Some(self.wake_at.map(|w| w.min(t)).unwrap_or(t));
    }

    pub fn merge(&mut self, o: FrameNeeds) {
        self.need = self.need.max(o.need);
        if let Some(t) = o.wake_at {
            self.wake_at(t);
        }
    }
}

/// Horloge centrale : decide a la fin de chaque image s'il faut redessiner, et compte les images (mesures).
#[derive(Clone, Debug, Default)]
pub struct AnimClock {
    needs: FrameNeeds,
    /// Fenetre cachee ou reduite : aucun redessin d'animation.
    pub hidden: bool,
    pub motion: Motion,
    /// Images dessinees, dont images d'animation (transition ou ambiance) : diagnostic.
    pub frames: u64,
    pub anim_frames: u64,
}

impl AnimClock {
    pub fn new(motion: Motion) -> AnimClock {
        AnimClock { motion, ..Default::default() }
    }

    /// Debut d'image : les besoins de l'image precedente sont oublies.
    pub fn begin_frame(&mut self) {
        self.needs = FrameNeeds::default();
        self.frames += 1;
    }

    pub fn transition(&mut self) {
        self.needs.transition();
    }

    pub fn ambient(&mut self) {
        self.needs.ambient();
    }

    pub fn wake_at(&mut self, t: f64) {
        self.needs.wake_at(t);
    }

    pub fn merge(&mut self, n: FrameNeeds) {
        self.needs.merge(n);
    }

    pub fn needs(&self) -> FrameNeeds {
        self.needs
    }

    /// Decision pour l'image suivante, a l'instant `now` (secondes).
    pub fn decide(&mut self, now: f64) -> Repaint {
        if self.hidden {
            return Repaint::Idle;
        }
        let wake = self.needs.wake_at.map(|t| Duration::from_secs_f64((t - now).max(0.0)));
        let r = match self.needs.need {
            Need::Transition => Repaint::Now,
            Need::Ambient if self.motion.ambient_on() => Repaint::After(Duration::from_secs_f64(AMBIENT_FRAME)),
            _ => Repaint::Idle,
        };
        if r != Repaint::Idle {
            self.anim_frames += 1;
        }
        match (r, wake) {
            (Repaint::Now, _) => Repaint::Now,
            (Repaint::After(a), Some(w)) => Repaint::After(a.min(w)),
            (Repaint::After(a), None) => Repaint::After(a),
            (_, Some(w)) if w.is_zero() => Repaint::Now,
            (_, Some(w)) => Repaint::After(w),
            (_, None) => Repaint::Idle,
        }
    }
}

// ------------------------------------------------------------------ partie egui : besoins signales par les composants

use eframe::egui;

fn needs_id() -> egui::Id {
    egui::Id::new("agss_ui_anim_needs")
}

fn motion_id() -> egui::Id {
    egui::Id::new("agss_ui_motion")
}

/// Temps courant du contexte (secondes).
pub fn now(ctx: &egui::Context) -> f64 {
    ctx.input(|i| i.time)
}

/// Une transition joue pendant cette image : redessin a la frequence de l'ecran.
pub fn need_transition(ctx: &egui::Context) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<FrameNeeds>(needs_id()).transition());
}

/// Une ambiance joue (anneau de copie) : 30 images par seconde, si le mouvement n'est pas reduit.
pub fn need_ambient(ctx: &egui::Context) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<FrameNeeds>(needs_id()).ambient());
}

/// Reveil programme a l'instant absolu `t` (secondes).
pub fn wake_at(ctx: &egui::Context, t: f64) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<FrameNeeds>(needs_id()).wake_at(t));
}

/// Besoins accumules depuis le dernier appel (remis a zero).
pub fn take_needs(ctx: &egui::Context) -> FrameNeeds {
    ctx.data_mut(|d| d.remove_temp::<FrameNeeds>(needs_id())).unwrap_or_default()
}

pub fn set_motion(ctx: &egui::Context, m: Motion) {
    ctx.data_mut(|d| d.insert_temp(motion_id(), m));
}

pub fn motion(ctx: &egui::Context) -> Motion {
    ctx.data(|d| d.get_temp::<Motion>(motion_id())).unwrap_or_default()
}

/// Age (secondes) de la valeur `key` pour l'element `id` : remis a zero quand la valeur change (etat d'une pastille,
/// page affichee...). Premier appel = age 0 (l'animation d'entree joue).
pub fn age_of(ctx: &egui::Context, id: egui::Id, key: u64) -> f64 {
    let t = now(ctx);
    let id = id.with("agss_age");
    ctx.data_mut(|d| {
        let e = d.get_temp_mut_or_insert_with::<(u64, f64)>(id, || (key, t));
        if e.0 != key {
            *e = (key, t);
        }
        t - e.1
    })
}

/// Rejoue l'animation d'entree de `id` (bouton « Rejouer » de la galerie, retour sur un ecran).
pub fn restart(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.remove_temp::<(u64, f64)>(id.with("agss_age")));
}

/// Progression d'une animation de duree `dur` (deja ajustee par `Motion::dur`) commencee il y a `age` secondes ;
/// signale une transition tant qu'elle n'est pas finie.
pub fn run(ctx: &egui::Context, age: f64, dur: f64) -> f32 {
    let p = if dur <= 0.0 { 1.0 } else { (age / dur).clamp(0.0, 1.0) as f32 };
    if p < 1.0 {
        need_transition(ctx);
    }
    p
}

/// Progression d'une animation qui commence apres `delay` secondes (cascade) : 0 avant, reveil programme au depart.
pub fn run_delayed(ctx: &egui::Context, age: f64, delay: f64, dur: f64) -> f32 {
    if age < delay {
        wake_at(ctx, now(ctx) + (delay - age));
        return 0.0;
    }
    run(ctx, age - delay, dur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn courbes_bornees_et_monotones() {
        for e in [Ease::Linear, Ease::Out, Ease::OutCubic, Ease::InOut] {
            assert_eq!(e.apply(0.0), 0.0);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6);
            let mut prev = 0.0;
            for i in 0..=100 {
                let v = e.apply(i as f32 / 100.0);
                assert!(v >= prev - 1e-6, "{:?} non monotone", e);
                prev = v;
            }
        }
        // back_out : depassement de 6 % au plus, puis retour exact a 1.
        let max = (0..=1000).map(|i| back_out(i as f32 / 1000.0)).fold(0.0f32, f32::max);
        assert!(max > 1.055 && max < 1.065, "depassement {}", max);
        assert!((back_out(1.0) - 1.0).abs() < 1e-6 && back_out(0.0).abs() < 1e-6);
        assert_eq!(ease_out(-1.0), 0.0);
        assert_eq!(ease_out(2.0), 1.0);
    }

    #[test]
    fn tween_et_ressort() {
        let mut t = Tween::new(0.0, 10.0, 1.0, 0.2, Ease::Linear);
        assert_eq!(t.value(0.5), 0.0);
        assert!((t.value(1.1) - 5.0).abs() < 1e-4);
        assert!(!t.done(1.1) && t.done(1.2));
        t.retarget(0.0, 1.1, 0.2);
        assert!((t.value(1.1) - 5.0).abs() < 1e-4, "aucun saut au changement de cible");
        assert_eq!(t.value(2.0), 0.0);
        let mut s = Spring::new(0.0);
        let mut over = 0.0f32;
        for _ in 0..120 {
            s.step(100.0, 1.0 / 60.0);
            over = over.max(s.pos);
        }
        assert_eq!(s.pos, 100.0, "immobile a la cible en 2 s");
        assert!(over > 100.0 && over < 110.0, "leger depassement : {}", over);
    }

    #[test]
    fn formes_commencent_et_finissent_au_repos() {
        for r in [false, true] {
            assert_eq!(page_in(1.0, r), (1.0, 0.0));
            assert_eq!(tile_in(1.0, r).0, 1.0);
            assert!(tile_in(1.0, r).1.abs() < 1e-5);
            assert_eq!(toast_in(0.0, r).0, 0.0);
        }
        assert_eq!(page_in(0.0, false), (0.0, 8.0));
        assert_eq!(step_in(0.0, -1.0, false).1, -24.0);
        assert_eq!(page_in(0.5, true).1, 0.0, "mouvement reduit : fondu seul");
        assert!(shake_dx(0.0).abs() < 1e-5 && shake_dx(1.0).abs() < 1e-5);
        assert!((0..100).all(|i| shake_dx(i as f32 / 100.0).abs() <= 3.0));
        assert!(hop_dy(0.0).abs() < 1e-5 && hop_dy(1.0).abs() < 1e-5 && hop_dy(0.3) < 0.0);
        assert_eq!(cascade_delay(0), 0.0);
        assert!((cascade_delay(3) - 0.09).abs() < 1e-9);
        assert_eq!(cascade_delay(50), cascade_delay(10));
        assert_eq!(tile_lift(1.0, 0.0), -2.0);
        assert_eq!(tile_lift(1.0, 1.0), 1.0);
        let (d, h, a) = pulse(0.0);
        assert!((d - 0.85).abs() < 1e-5 && (h - 0.6).abs() < 1e-5 && (a - 0.45).abs() < 1e-5);
        assert!((breathe(0.0) - 1.0).abs() < 1e-6 && (breathe(1.2) - 1.04).abs() < 1e-4);
        assert_eq!(counter(0, 21, 1.0), 21);
        assert_eq!(burst(0, 8, 1.0, 20.0), (20.0, 0.0, 0.0));
        let a0 = ring_angle(0.0);
        assert!((ring_angle(DUR_RING) - a0).abs() < 1e-4, "un tour par periode");
    }

    #[test]
    fn mouvement_reduit_et_ralenti() {
        use crate::gamedetect::CardAnim;
        let m = Motion::from_settings(CardAnim::Always, false, 1.0);
        assert!(!m.reduced && m.ambient_on());
        assert_eq!(m.dur(DUR_SLOW), DUR_SLOW);
        let r = Motion::from_settings(CardAnim::Auto, false, 1.0);
        assert!(r.reduced && !r.ambient_on(), "Auto suit le systeme (animations coupees)");
        assert_eq!(r.dur(DUR_SLOW), DUR_REDUCED_FADE, "tout devient un fondu de 120 ms");
        assert!(!Motion::from_settings(CardAnim::Auto, true, 1.0).reduced);
        assert!(Motion::from_settings(CardAnim::Reduced, true, 1.0).reduced);
        let s = Motion::from_settings(CardAnim::Always, true, 10.0);
        assert!((s.dur(DUR_NORMAL) - 2.0).abs() < 1e-9);
        assert_eq!(Motion::from_settings(CardAnim::Always, true, f32::NAN).slow, 1.0);
        assert_eq!(Motion::from_settings(CardAnim::Always, true, 0.2).slow, 1.0);
    }

    #[test]
    fn horloge_aucun_redessin_au_repos() {
        let mut c = AnimClock::new(Motion::default());
        // Repos : rien n'est signale -> aucun redessin, image apres image.
        for _ in 0..5 {
            c.begin_frame();
            assert_eq!(c.decide(10.0), Repaint::Idle);
        }
        assert_eq!(c.anim_frames, 0);
        // Transition : frequence de l'ecran ; l'ambiance seule : 30 i/s.
        c.begin_frame();
        c.ambient();
        c.transition();
        assert_eq!(c.decide(10.0), Repaint::Now);
        c.begin_frame();
        c.ambient();
        assert_eq!(c.decide(10.0), Repaint::After(Duration::from_secs_f64(1.0 / 30.0)));
        // La transition terminee n'est plus signalee : retour au repos immediat.
        c.begin_frame();
        assert_eq!(c.decide(10.5), Repaint::Idle);
        assert_eq!(c.anim_frames, 2);
        // Reveil programme (debut differe, fin de toast) : un seul redessin au bon moment.
        c.begin_frame();
        c.wake_at(12.0);
        c.wake_at(11.0);
        assert_eq!(c.decide(10.0), Repaint::After(Duration::from_secs(1)));
        c.begin_frame();
        c.wake_at(9.0);
        assert_eq!(c.decide(10.0), Repaint::Now, "reveil depasse : tout de suite");
        // Ambiance + reveil plus proche.
        c.begin_frame();
        c.ambient();
        c.wake_at(10.01);
        assert_eq!(c.decide(10.0), Repaint::After(Duration::from_secs_f64(0.01)));
        // Fenetre cachee : jamais de redessin d'animation.
        c.hidden = true;
        c.begin_frame();
        c.transition();
        assert_eq!(c.decide(10.0), Repaint::Idle);
        // Mouvement reduit : l'ambiance ne tourne pas.
        let mut r = AnimClock::new(Motion { reduced: true, slow: 1.0 });
        r.begin_frame();
        r.ambient();
        assert_eq!(r.decide(0.0), Repaint::Idle);
        // Sondage periodique de l'application : le plus proche l'emporte.
        assert_eq!(Repaint::Idle.with_poll(None), Repaint::Idle);
        assert_eq!(Repaint::Idle.with_poll(Some(Duration::from_secs(8))), Repaint::After(Duration::from_secs(8)));
        assert_eq!(Repaint::After(Duration::from_millis(33)).with_poll(Some(Duration::from_secs(8))), Repaint::After(Duration::from_millis(33)));
        assert_eq!(Repaint::Now.with_poll(Some(Duration::ZERO)), Repaint::Now);
    }
}
