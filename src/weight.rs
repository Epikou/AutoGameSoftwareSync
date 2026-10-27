//! Alerte poids (2.0.1) : un element a proteger qui depasse un seuil reglable (500 Mo par defaut, mode expert des Reglages)
//! est signale clairement avec sa taille et son IMPACT dans le coffre (« 4,2 Go · environ 12,6 Go dans ton coffre avec 3
//! versions »). Tant que le stockage ne deduplique pas, chaque version est une copie entiere : impact = taille x versions.
//! Un seul seuil pour toute l'appli : alerte a l'ajout (formulaire, selecteur guide, glisser-deposer), au scan, dans la
//! fiche, pastille « Lourd » sur la tuile quand un element deja protege grossit, et regle des dossiers perso du scan (jamais
//! coches d'office au-dela). Logique PURE et testee ; l'ecran est dans `gui::weight_ui`.
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering};

pub const MB: u64 = 1024 * 1024;
/// Seuil par defaut (Mo).
pub const DEFAULT_LIMIT_MB: u32 = 500;
/// Bornes du reglage (Mo).
pub const MIN_LIMIT_MB: u32 = 10;
pub const MAX_LIMIT_MB: u32 = 1_000_000;
/// Versions annoncees pour un element lourd a l'ajout (« Garder »).
pub const HEAVY_KEEP: usize = 3;

static LIMIT_MB: AtomicU32 = AtomicU32::new(DEFAULT_LIMIT_MB);

/// Seuil ramene dans les bornes.
pub fn clamp_limit(mb: u32) -> u32 {
    mb.clamp(MIN_LIMIT_MB, MAX_LIMIT_MB)
}

/// Seuil courant du processus (renseigne par l'interface a partir de `settings.json`).
pub fn set_limit_mb(mb: u32) {
    LIMIT_MB.store(clamp_limit(mb), Ordering::SeqCst);
}

pub fn limit_mb() -> u32 {
    LIMIT_MB.load(Ordering::SeqCst)
}

pub fn limit_bytes() -> u64 {
    limit_mb() as u64 * MB
}

/// Un element de cette taille est-il « lourd » ? (strictement au-dessus du seuil)
pub fn is_heavy(bytes: u64) -> bool {
    bytes > limit_bytes()
}

/// Meme test avec un seuil donne (Mo). Pur.
pub fn is_heavy_at(bytes: u64, limit_mb: u32) -> bool {
    bytes > clamp_limit(limit_mb) as u64 * MB
}

/// Versions a garder le plus bas proposees par « Garder moins de versions » : le plancher de securite du moteur (jamais
/// moins : voir `backup::MIN_KEEP`).
pub fn fewer_keep() -> usize {
    crate::backup::MIN_KEEP
}

/// Place estimee dans le coffre : taille x versions gardees.
pub fn impact(bytes: u64, keep: usize) -> u64 {
    bytes.saturating_mul(keep as u64)
}

/// Versions gardees a l'ajout d'un element de cette taille : 3 si lourd, sinon le reglage par defaut.
pub fn default_keep(bytes: u64) -> usize {
    if is_heavy(bytes) {
        HEAVY_KEEP
    } else {
        crate::backup::DEFAULT_KEEP
    }
}

/// Ce que l'alerte affiche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alert {
    pub bytes: u64,
    pub keep: usize,
    /// Place estimee dans le coffre.
    pub impact: u64,
}

/// Alerte d'un element (None s'il n'est pas lourd). `keep` : versions qui seront gardees.
pub fn alert(bytes: u64, keep: usize) -> Option<Alert> {
    is_heavy(bytes).then(|| Alert { bytes, keep, impact: impact(bytes, keep) })
}

/// Choix proposes devant une alerte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Garder tel quel (3 versions).
    Keep,
    /// Garder moins de versions (le plancher de securite).
    Fewer,
    /// Annuler l'ajout / retirer l'element.
    Cancel,
}

/// Versions a enregistrer pour un choix (None = rien n'est ajoute).
pub fn keep_for_choice(c: Choice) -> Option<usize> {
    match c {
        Choice::Keep => Some(HEAVY_KEEP),
        Choice::Fewer => Some(fewer_keep()),
        Choice::Cancel => None,
    }
}

/// Elements devenus lourds qui n'ont pas encore ete signales (un toast, une seule fois par element). Pur.
pub fn newly_heavy(sizes: &[(String, u64)], notified: &BTreeSet<String>) -> Vec<String> {
    sizes.iter().filter(|(n, b)| is_heavy(*b) && !notified.contains(n)).map(|(n, _)| n.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seuil_par_defaut_et_bornes() {
        assert_eq!(DEFAULT_LIMIT_MB, 500);
        assert!(!is_heavy_at(500 * MB, 500) && is_heavy_at(500 * MB + 1, 500));
        assert_eq!(clamp_limit(0), MIN_LIMIT_MB);
        assert_eq!(clamp_limit(u32::MAX), MAX_LIMIT_MB);
        assert_eq!(HEAVY_KEEP, crate::scan::BIG_KEEP, "meme nombre de versions que le scan");
    }

    #[test]
    fn impact_est_taille_fois_versions() {
        let b = 4_400_000_000;
        assert_eq!(impact(b, 3), b * 3);
        assert_eq!(impact(u64::MAX, 3), u64::MAX, "pas de debordement");
        assert_eq!(keep_for_choice(Choice::Keep), Some(3));
        assert_eq!(keep_for_choice(Choice::Fewer), Some(crate::backup::MIN_KEEP));
        assert_eq!(keep_for_choice(Choice::Cancel), None);
    }
}
