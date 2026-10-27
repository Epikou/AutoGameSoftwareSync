//! Icone de la zone de notification DYNAMIQUE : etat calcule par une fonction pure a partir des entrees
//! (surveillance, dossier partage, activite, conflits, erreurs) puis icone de base + pastille generee au runtime.
use crate::watch::EventKind;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrayState {
    /// Rouge : erreur de sauvegarde / restauration recente.
    Error,
    /// Jaune : conflit a regler (mode « Me demander »).
    Conflict,
    /// Orange : dossier partage injoignable.
    Unreachable,
    /// Orange : dossier partage « a la demande » (streaming) detecte.
    OnDemand,
    /// Bleu anime : synchronisation en cours.
    Syncing,
    /// Gris + pause : surveillance en pause (ou arretee).
    Paused,
    /// Vert : surveillance active, dossier partage joignable.
    Ok,
}

/// Entrees de l'etat (toutes calculees ailleurs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrayInputs {
    pub error: bool,
    pub conflict: bool,
    pub unreachable: bool,
    pub on_demand: bool,
    pub syncing: bool,
    pub paused: bool,
}

/// Priorite d'affichage : rouge > jaune > orange > bleu > gris > vert.
pub fn state_of(i: &TrayInputs) -> TrayState {
    if i.error {
        TrayState::Error
    } else if i.conflict {
        TrayState::Conflict
    } else if i.unreachable {
        TrayState::Unreachable
    } else if i.on_demand {
        TrayState::OnDemand
    } else if i.syncing {
        TrayState::Syncing
    } else if i.paused {
        TrayState::Paused
    } else {
        TrayState::Ok
    }
}

/// Une erreur reste « recente » 10 minutes ; un mode « a la demande » detecte reste signale 30 minutes.
pub const ERROR_RECENT_MS: u64 = 10 * 60 * 1000;
pub const ON_DEMAND_RECENT_MS: u64 = 30 * 60 * 1000;

/// Memoire des evenements de la surveillance (conflits en attente, erreurs recentes par jeu).
#[derive(Default, Debug)]
pub struct Tracker {
    conflicts: BTreeSet<String>,
    errors: BTreeMap<String, u64>,
}

impl Tracker {
    pub fn on_event(&mut self, kind: EventKind, game: &str, now_ms: u64) {
        match kind {
            EventKind::Error => {
                self.errors.insert(game.to_string(), now_ms);
            }
            EventKind::Conflict => {
                self.conflicts.insert(game.to_string());
            }
            EventKind::BackedUp | EventKind::Restored | EventKind::ConflictResolved => {
                self.conflicts.remove(game);
                self.errors.remove(game);
            }
        }
    }

    pub fn conflict(&self) -> bool {
        !self.conflicts.is_empty()
    }

    pub fn error_recent(&self, now_ms: u64) -> bool {
        self.errors.values().any(|t| now_ms.saturating_sub(*t) < ERROR_RECENT_MS)
    }
}

// ------------------------------------------------------------------ rendu de l'icone

fn badge_color(s: TrayState) -> [u8; 3] {
    match s {
        TrayState::Error => [220, 50, 47],
        TrayState::Conflict => [242, 201, 0],
        TrayState::Unreachable | TrayState::OnDemand => [245, 140, 25],
        TrayState::Syncing => [50, 120, 240],
        TrayState::Paused => [128, 128, 128],
        TrayState::Ok => [46, 184, 92],
    }
}

fn dist_seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.0 + abx * t, a.1 + aby * t);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// Couverture (0..1) d'un disque de rayon `r` au pixel `p`.
fn cov_disc(p: (f32, f32), c: (f32, f32), r: f32) -> f32 {
    let d = ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

/// Couverture d'un segment epais.
fn cov_seg(p: (f32, f32), a: (f32, f32), b: (f32, f32), w: f32) -> f32 {
    (w / 2.0 - dist_seg(p, a, b) + 0.5).clamp(0.0, 1.0)
}

fn blend(px: &mut [u8], col: [u8; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    for i in 0..3 {
        px[i] = (px[i] as f32 * (1.0 - a) + col[i] as f32 * a).round() as u8;
    }
    px[3] = px[3].max((a * 255.0).round() as u8);
}

/// Icone `size` x `size` (RGBA) : icone de base + pastille d'etat en bas a droite (lisible aussi a 16 px).
/// `frame` (0 / 1) anime l'etat « synchronisation » ; ignore pour les autres etats.
pub fn render(base: &[u8], size: u32, state: TrayState, frame: u32) -> Vec<u8> {
    let mut out = base.to_vec();
    let s = size as f32;
    let c = (s * 0.69, s * 0.69);
    let r = s * 0.30;
    let col = badge_color(state);
    let glyph: [u8; 3] = if state == TrayState::Conflict { [40, 30, 0] } else { [255, 255, 255] };
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let i = ((y * size + x) * 4) as usize;
            if i + 3 >= out.len() {
                continue;
            }
            // contour sombre puis disque de couleur
            blend(&mut out[i..i + 4], [0, 0, 0], cov_disc(p, c, r + 1.3) * 0.65);
            blend(&mut out[i..i + 4], col, cov_disc(p, c, r));
            let u = |dx: f32, dy: f32| (c.0 + dx * r, c.1 + dy * r);
            let mut g = 0.0_f32;
            match state {
                TrayState::Ok => {
                    g = g.max(cov_seg(p, u(-0.5, 0.05), u(-0.12, 0.45), r * 0.26));
                    g = g.max(cov_seg(p, u(-0.12, 0.45), u(0.52, -0.38), r * 0.26));
                }
                TrayState::Paused => {
                    g = g.max(cov_seg(p, u(-0.28, -0.5), u(-0.28, 0.5), r * 0.3));
                    g = g.max(cov_seg(p, u(0.28, -0.5), u(0.28, 0.5), r * 0.3));
                }
                TrayState::Unreachable | TrayState::OnDemand | TrayState::Conflict => {
                    g = g.max(cov_seg(p, u(0.0, -0.58), u(0.0, 0.12), r * 0.3));
                    g = g.max(cov_disc(p, u(0.0, 0.52), r * 0.17));
                }
                TrayState::Error => {
                    g = g.max(cov_seg(p, u(-0.45, -0.45), u(0.45, 0.45), r * 0.28));
                    g = g.max(cov_seg(p, u(0.45, -0.45), u(-0.45, 0.45), r * 0.28));
                }
                TrayState::Syncing => {
                    let lit_ns = frame % 2 == 0;
                    for (dx, dy, ns) in [(0.0, -0.52, true), (0.0, 0.52, true), (0.52, 0.0, false), (-0.52, 0.0, false)] {
                        let k = if ns == lit_ns { 1.0 } else { 0.35 };
                        g = g.max(cov_disc(p, u(dx, dy), r * 0.2) * k);
                    }
                }
            }
            blend(&mut out[i..i + 4], glyph, g * cov_disc(p, c, r));
        }
    }
    out
}

/// Image de l'animation affichee a l'instant `ms` : alternance toutes les ~500 ms, seulement pendant la synchronisation.
pub fn anim_frame(state: TrayState, ms: u64) -> u32 {
    if state == TrayState::Syncing {
        ((ms / 500) % 2) as u32
    } else {
        0
    }
}
