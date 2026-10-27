//! Etat du dossier partage vu de l'interface (logique pure, testee) : non configure, injoignable, invalide depuis longtemps.
//! Sert au bandeau de chaque onglet, a la barre du bas, a la carte de detection, a l'icone de la zone de notification et
//! a `status.json`.
use std::path::Path;

/// Apres ce delai d'injoignabilite continue, le dossier est considere comme invalide (le bandeau le dit).
pub const INVALID_AFTER_MS: u64 = 120_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedState {
    /// Dossier configure et joignable (ou verification pas encore faite).
    Ok,
    /// Aucun dossier partage choisi.
    NotConfigured,
    /// Configure mais injoignable depuis peu (reseau, lecteur en cours de montage...).
    Unreachable,
    /// Configure mais injoignable depuis longtemps : a verifier ou a remplacer.
    Invalid,
}

/// `dest` : dossier configure (None ou chemin vide = non configure) ; `reachable` : dernier resultat de la sonde (None = inconnu) ;
/// `bad_since_ms` : depuis quand la sonde dit « injoignable » sans interruption.
pub fn state(dest: Option<&Path>, reachable: Option<bool>, bad_since_ms: Option<u64>, now_ms: u64) -> SharedState {
    let Some(d) = dest.filter(|d| !d.as_os_str().is_empty()) else { return SharedState::NotConfigured };
    let _ = d;
    match reachable {
        Some(false) => match bad_since_ms {
            Some(t) if now_ms.saturating_sub(t) >= INVALID_AFTER_MS => SharedState::Invalid,
            _ => SharedState::Unreachable,
        },
        _ => SharedState::Ok,
    }
}

/// Bandeau a afficher en haut de chaque onglet : jamais pendant le tutoriel (qui explique deja l'etape), disparait des
/// qu'un dossier valide est defini. Court pour un dossier injoignable depuis peu : pas de bandeau (c'est souvent passager).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    NotConfigured,
    Invalid,
}

pub fn banner(s: SharedState, tutorial: bool) -> Option<Banner> {
    if tutorial {
        return None;
    }
    match s {
        SharedState::NotConfigured => Some(Banner::NotConfigured),
        SharedState::Invalid => Some(Banner::Invalid),
        _ => None,
    }
}

/// L'icone de la zone de notification passe a l'etat orange « attention » pour ces etats.
pub fn tray_attention(s: SharedState) -> bool {
    !matches!(s, SharedState::Ok)
}

/// La synchronisation effective est-elle impossible ? (le jeu peut quand meme etre ajoute a la liste)
pub fn sync_blocked(s: SharedState) -> bool {
    matches!(s, SharedState::NotConfigured)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn etats_et_bandeau() {
        let p = PathBuf::from("G:\\Mon Drive\\Saves");
        assert_eq!(state(None, None, None, 0), SharedState::NotConfigured);
        assert_eq!(state(Some(Path::new("")), Some(true), None, 0), SharedState::NotConfigured, "chemin vide = non configure");
        assert_eq!(state(Some(&p), Some(true), None, 0), SharedState::Ok);
        assert_eq!(state(Some(&p), None, None, 0), SharedState::Ok, "sonde pas encore faite");
        assert_eq!(state(Some(&p), Some(false), Some(1000), 1000 + 5000), SharedState::Unreachable);
        assert_eq!(state(Some(&p), Some(false), Some(1000), 1000 + INVALID_AFTER_MS), SharedState::Invalid);
        assert_eq!(banner(SharedState::NotConfigured, false), Some(Banner::NotConfigured));
        assert_eq!(banner(SharedState::Invalid, false), Some(Banner::Invalid));
        assert_eq!(banner(SharedState::Unreachable, false), None);
        assert_eq!(banner(SharedState::Ok, false), None, "disparait des qu'un dossier valide est defini");
        assert_eq!(banner(SharedState::NotConfigured, true), None, "pas pendant le tutoriel");
        assert!(tray_attention(SharedState::NotConfigured) && tray_attention(SharedState::Unreachable) && !tray_attention(SharedState::Ok));
        assert!(sync_blocked(SharedState::NotConfigured) && !sync_blocked(SharedState::Invalid));
    }
}
