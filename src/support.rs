//! Rappel de soutien (popup cafe) : quand l'afficher (compteur de jeux ajoutes, intervalles, situations ou c'est interdit)
//! et comportement du bouton « Skip » (esquive a la souris au premier essai). Fonctions pures, horloge injectee.

/// Jeux ajoutes avant la premiere apparition (et entre deux apparitions).
pub const GAMES_THRESHOLD: u32 = 2;
pub const INTERVAL_DAYS: u64 = 10;
/// Apres un clic sur « Offre-nous un cafe ».
pub const AFTER_COFFEE_DAYS: u64 = 60;
const DAY_MS: u64 = 86_400_000;

pub fn interval_ms(coffee_clicked: bool) -> u64 {
    (if coffee_clicked { AFTER_COFFEE_DAYS } else { INTERVAL_DAYS }) * DAY_MS
}

/// Situations ou la popup ne doit PAS apparaitre (elle est alors reportee, sans rien consommer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Blockers {
    pub tutorial: bool,
    /// Sauvegarde / restauration / synchro en cours.
    pub busy: bool,
    /// Fenetre cachee ou dans la zone de notification.
    pub window_hidden: bool,
    /// Une confirmation est deja ouverte.
    pub dialog_open: bool,
}

impl Blockers {
    pub fn any(&self) -> bool {
        self.tutorial || self.busy || self.window_hidden || self.dialog_open
    }
}

/// Doit-on afficher la popup maintenant ? `last_shown_ms` = 0 : jamais affichee.
pub fn should_show(enabled: bool, games_added: u32, last_shown_ms: u64, coffee_clicked: bool, now_ms: u64, b: &Blockers) -> bool {
    enabled && !b.any() && games_added >= GAMES_THRESHOLD && (last_shown_ms == 0 || now_ms.saturating_sub(last_shown_ms) >= interval_ms(coffee_clicked))
}

/// Nombre de jeux ajoutes depuis la derniere image (la liste ne fait que croitre par ajout ; une baisse n'est pas comptee).
pub fn newly_added(previous_total: usize, total: usize) -> u32 {
    total.saturating_sub(previous_total) as u32
}

