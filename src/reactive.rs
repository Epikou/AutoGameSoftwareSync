//! Reactivite de la surveillance : modes (Instantane / Normal / Connexion lente / Personnalise), tri des evenements du
//! dossier partage par jeu, et machine a etats PURE de verification d'une sauvegarde distante (delai de securite,
//! reessais, sauvegarde incomplete puis complete, jeu en cours reporte). L'horloge est injectee (tests).
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Reactivity {
    #[default]
    Instant,
    Normal,
    Slow,
    Custom,
}

impl Reactivity {
    pub const ALL: [Reactivity; 4] = [Reactivity::Instant, Reactivity::Normal, Reactivity::Slow, Reactivity::Custom];
}

/// Valeurs du mode « Personnalise » (en secondes).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct ReactCustom {
    /// Delai de securite apres le dernier evenement avant de verifier / restaurer.
    pub safety_s: u32,
    /// Intervalle entre deux verifications quand la sauvegarde distante est incomplete.
    pub retry_s: u32,
    /// Temps maximum de reessais avant d'abandonner (le filet de secours reprend).
    pub retry_max_s: u32,
    /// Intervalle du filet de secours (resynchronisation periodique).
    pub fallback_s: u32,
    /// Stabilite des fichiers exigee apres la fermeture du jeu avant la sauvegarde.
    pub stable_s: u32,
}

impl Default for ReactCustom {
    fn default() -> Self {
        ReactCustom { safety_s: 2, retry_s: 1, retry_max_s: 30, fallback_s: 60, stable_s: 3 }
    }
}

/// Parametres effectifs (millisecondes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReactParams {
    pub safety_ms: u64,
    pub retry_ms: u64,
    pub retry_max_ms: u64,
    pub fallback_ms: u64,
    pub stable_ms: u64,
}

/// Parametres d'un mode. La verification d'empreintes reste OBLIGATOIRE dans tous les modes.
pub fn params(mode: Reactivity, custom: &ReactCustom) -> ReactParams {
    let s = |v: u64| v * 1000;
    match mode {
        Reactivity::Instant => ReactParams { safety_ms: s(2), retry_ms: s(1), retry_max_ms: s(30), fallback_ms: s(60), stable_ms: s(3) },
        Reactivity::Normal => ReactParams { safety_ms: s(10), retry_ms: s(2), retry_max_ms: s(60), fallback_ms: s(60), stable_ms: s(10) },
        Reactivity::Slow => ReactParams { safety_ms: s(30), retry_ms: s(5), retry_max_ms: s(180), fallback_ms: s(120), stable_ms: s(20) },
        Reactivity::Custom => {
            let retry = custom.retry_s.clamp(1, 600) as u64;
            ReactParams {
                safety_ms: s(custom.safety_s.min(600) as u64),
                retry_ms: s(retry),
                retry_max_ms: s((custom.retry_max_s.clamp(1, 3600) as u64).max(retry)),
                fallback_ms: s(custom.fallback_s.clamp(10, 86_400) as u64),
                stable_ms: s(custom.stable_s.clamp(1, 600) as u64),
            }
        }
    }
}

// ------------------------------------------------------------------ evenements du dossier partage

/// Jeu concerne par un evenement du dossier partage : chemin relatif `<cle_jeu>/saves/...` (separateurs `/` ou `\`).
/// `keys[i]` = cles normalisees (`cloud::game_key`) admises pour le jeu i ; le dossier de l'evenement est normalise de la meme facon. Tout autre evenement (autre dossier, game.json, racine) est ignore.
pub fn game_of_event(rel: &str, keys: &[Vec<String>]) -> Option<usize> {
    let mut parts = rel.split(['/', '\\']).filter(|p| !p.is_empty());
    let first = parts.next()?;
    if parts.next()? != "saves" {
        return None;
    }
    let norm = crate::cloud::game_key(first);
    keys.iter().position(|ks| ks.iter().any(|k| *k == norm))
}

// ------------------------------------------------------------------ verification d'une sauvegarde distante

/// Resultat d'un controle du dossier partage pour un jeu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    /// Aucun dossier incomplet : tout ce qui est annonce est la.
    Complete,
    /// Des dossiers incomplets (meta.json present mais fichiers manquants / taille differente...) ; la valeur change
    /// quand des octets arrivent (progression).
    Incomplete(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VAction {
    /// Rien a faire pour l'instant.
    Wait,
    /// La sauvegarde distante est complete (ou n'avance plus) : lancer la synchro de ce jeu (mêmes protections).
    Proceed,
    /// Le jeu tourne : abandonner cette verification, la fermeture du jeu declenche la synchro.
    Postpone,
    /// Temps maximum de reessais depasse : abandon propre, le filet de secours reprend.
    Abandon,
}

#[derive(Clone, Debug)]
pub struct Verifier {
    pub detected_ms: u64,
    last_change_ms: u64,
    next_check_ms: u64,
    last_sig: Option<u64>,
    last_progress_ms: u64,
    skip_safety: bool,
    /// La mise en attente (sauvegarde incomplete) a deja ete journalisee pour cette verification.
    pub waiting_logged: bool,
}

impl Verifier {
    /// `skip_safety` : demande explicite (« Synchroniser maintenant ») : pas de delai de securite.
    pub fn new(now_ms: u64, skip_safety: bool) -> Self {
        Verifier { detected_ms: now_ms, last_change_ms: now_ms, next_check_ms: 0, last_sig: None, last_progress_ms: now_ms, skip_safety, waiting_logged: false }
    }

    /// Demande explicite de l'utilisateur (« Synchroniser maintenant ») ?
    pub fn manual(&self) -> bool {
        self.skip_safety
    }

    /// Une sauvegarde incomplete a ete vue par la derniere verification.
    pub fn incomplete_seen(&self) -> bool {
        self.last_sig.is_some()
    }

    /// Nouvel evenement pour ce jeu : le delai de securite repart (debounce).
    pub fn note_activity(&mut self, now_ms: u64) {
        self.last_change_ms = now_ms;
        self.next_check_ms = 0;
        self.last_progress_ms = now_ms;
        self.skip_safety = false;
    }

    /// Fait avancer la verification. `check` n'est appele que quand une verification est due.
    pub fn poll(&mut self, now_ms: u64, p: &ReactParams, running: bool, check: &mut dyn FnMut() -> Check) -> VAction {
        if running {
            return VAction::Postpone;
        }
        if !self.skip_safety && now_ms < self.last_change_ms + p.safety_ms {
            return VAction::Wait;
        }
        if now_ms < self.next_check_ms {
            return VAction::Wait;
        }
        match check() {
            Check::Complete => VAction::Proceed,
            Check::Incomplete(sig) => {
                if self.last_sig != Some(sig) {
                    self.last_sig = Some(sig);
                    self.last_progress_ms = now_ms;
                }
                if now_ms.saturating_sub(self.last_progress_ms) >= 3 * p.retry_ms {
                    // N'avance plus (dossier perime ou bloque) : on traite ce qui est complet ; la restauration
                    // n'utilise de toute facon que des sauvegardes completes et verifiees par empreinte.
                    VAction::Proceed
                } else if now_ms.saturating_sub(self.detected_ms) >= p.retry_max_ms {
                    VAction::Abandon
                } else {
                    self.next_check_ms = now_ms + p.retry_ms;
                    VAction::Wait
                }
            }
        }
    }
}

/// Intervalle (ms) d'interrogation directe du dossier partage (noms des dossiers de `saves/`, presence de meta.json),
/// independante des evenements Windows : Instantane 2 s, Normal 5 s, Connexion lente 10 s, Personnalise `custom_s`.
pub fn poll_interval_ms(mode: Reactivity, custom_s: u32) -> u64 {
    match mode {
        Reactivity::Instant => 2_000,
        Reactivity::Normal => 5_000,
        Reactivity::Slow => 10_000,
        Reactivity::Custom => custom_s.clamp(1, 600) as u64 * 1000,
    }
}
