//! Journal des CHANGEMENTS d'etat d'un jeu (une ligne par transition, pas de repetition) et declenchement immediat de la
//! verification quand l'etat devient « cloud plus recent » (l'interface ne doit jamais l'afficher sans qu'une action soit lancee).
use crate::vm::GameStatus;
use std::collections::HashMap;

/// Tant que l'etat « cloud plus recent » dure, la verification est relancee au plus toutes les 60 s (pas de boucle serree).
pub const RETRIGGER_MS: u64 = 60_000;

/// Code stable (ASCII) d'un etat, et son libelle de journal.
pub fn code(s: &GameStatus) -> &'static str {
    match s {
        GameStatus::Never => "never",
        GameStatus::NoDest => "no_dest",
        GameStatus::UpToDate => "up_to_date",
        GameStatus::LocalChanged => "local_changed",
        GameStatus::CloudNewer => "cloud_newer",
        GameStatus::Running => "running",
        GameStatus::Conflict => "conflict",
        GameStatus::Error(_) => "error",
    }
}

pub fn label(code: &str) -> &'static str {
    match code {
        "never" => "jamais synchronise",
        "no_dest" => "pas de dossier partage",
        "up_to_date" => "a jour",
        "local_changed" => "modifie localement",
        "cloud_newer" => "cloud plus recent",
        "running" => "en cours d'utilisation",
        "conflict" => "conflit",
        "error" => "erreur",
        _ => "inconnu",
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Obs {
    /// Ligne de journal (None = aucun changement).
    pub line: Option<String>,
    /// Lancer maintenant la verification / synchro de ce jeu.
    pub trigger: bool,
}

#[derive(Default)]
pub struct Transitions {
    last: HashMap<String, &'static str>,
    last_trigger: HashMap<String, u64>,
}

impl Transitions {
    /// `remote` : (PC de la sauvegarde distante, heure de constat) ; appele seulement si l'etat devient « cloud plus recent ».
    pub fn observe(&mut self, game: &str, st: &GameStatus, now_ms: u64, remote: &mut dyn FnMut() -> (String, String)) -> Obs {
        let c = code(st);
        let prev = self.last.insert(game.to_string(), c);
        let changed = prev != Some(c);
        let mut o = Obs::default();
        if changed {
            let suffix = if c == "cloud_newer" {
                let (m, t) = remote();
                format!(" (sauvegarde de {} vue a {})", m, t)
            } else {
                String::new()
            };
            o.line = match prev {
                Some(p) => Some(format!("etat : {} -> {}{}", label(p), label(c), suffix)),
                None if c == "cloud_newer" => Some(format!("etat initial : {}{}", label(c), suffix)),
                None => None,
            };
        }
        if c == "cloud_newer" {
            let due = changed || now_ms.saturating_sub(self.last_trigger.get(game).copied().unwrap_or(0)) >= RETRIGGER_MS;
            if due {
                self.last_trigger.insert(game.to_string(), now_ms);
                o.trigger = true;
            }
        } else {
            self.last_trigger.remove(game);
        }
        o
    }

    /// Jeu retire de la liste : on l'oublie.
    pub fn retain(&mut self, names: &[String]) {
        self.last.retain(|k, _| names.contains(k));
        self.last_trigger.retain(|k, _| names.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_ligne_par_transition_et_declenchement() {
        let mut t = Transitions::default();
        let mut rem = || ("ALLY".to_string(), "02:47:36".to_string());
        assert_eq!(t.observe("J", &GameStatus::UpToDate, 0, &mut rem), Obs::default(), "premier constat a jour : silencieux");
        assert_eq!(t.observe("J", &GameStatus::UpToDate, 1000, &mut rem), Obs::default(), "pas de repetition");
        let o = t.observe("J", &GameStatus::CloudNewer, 2000, &mut rem);
        assert_eq!(o.line.as_deref(), Some("etat : a jour -> cloud plus recent (sauvegarde de ALLY vue a 02:47:36)"));
        assert!(o.trigger);
        // Meme etat : pas de ligne, pas de relance avant 60 s ; puis relance silencieuse.
        let o = t.observe("J", &GameStatus::CloudNewer, 10_000, &mut rem);
        assert!(o.line.is_none() && !o.trigger);
        let o = t.observe("J", &GameStatus::CloudNewer, 2000 + RETRIGGER_MS, &mut rem);
        assert!(o.line.is_none() && o.trigger);
        let o = t.observe("J", &GameStatus::UpToDate, 70_000, &mut rem);
        assert_eq!(o.line.as_deref(), Some("etat : cloud plus recent -> a jour"));
        assert!(!o.trigger);
        // Au demarrage deja en retard : constat journalise et verification lancee.
        let mut t2 = Transitions::default();
        let o = t2.observe("K", &GameStatus::CloudNewer, 0, &mut rem);
        assert!(o.trigger && o.line.unwrap().starts_with("etat initial : cloud plus recent"));
    }
}
