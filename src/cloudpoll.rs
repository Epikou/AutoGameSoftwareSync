//! Interrogation directe, legere, du dossier partage (independante des evenements Windows, que Google Drive
//! monte en miroir ne declenche pas pour les fichiers qu'il depose lui-meme).
//!
//! Partie pure (`CloudPoller`, testee avec une horloge simulee) + thread qui envoie, a chaque changement de
//! signature d'un jeu, un message `WatchMsg::Changed` au moteur reactif existant (debounce, verification de completude,
//! reessais, restauration avec toutes les protections anti-perte).
use crate::cloud;
use crate::config::Game;
use crate::dirwatch::WatchMsg;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// Compare, a intervalle regulier, la signature de chaque jeu a la precedente.
pub struct CloudPoller {
    interval_ms: u64,
    next_ms: u64,
    sigs: Vec<Option<u64>>,
}

impl CloudPoller {
    pub fn new(games: usize, interval_ms: u64) -> Self {
        CloudPoller { interval_ms: interval_ms.max(200), next_ms: 0, sigs: vec![None; games] }
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    /// Prochaine echeance (ms, meme horloge que `tick`).
    pub fn next_due_ms(&self) -> u64 {
        self.next_ms
    }

    /// Un passage. `sig(i)` = signature du jeu i (None = dossier injoignable : on garde la derniere valeur connue).
    /// La premiere observation sert de reference (le demarrage fait sa propre synchro). Retourne les jeux dont la
    /// signature a change.
    pub fn tick(&mut self, now_ms: u64, sig: &mut dyn FnMut(usize) -> Option<u64>) -> Vec<usize> {
        if now_ms < self.next_ms {
            return Vec::new();
        }
        self.next_ms = now_ms + self.interval_ms;
        let mut changed = Vec::new();
        for i in 0..self.sigs.len() {
            let Some(s) = sig(i) else { continue };
            match self.sigs[i] {
                Some(prev) if prev != s => changed.push(i),
                _ => {}
            }
            self.sigs[i] = Some(s);
        }
        changed
    }
}

/// Thread d'interrogation. Un changement pour un element = un message `Changed("<cle>/saves")` (traite comme un evenement).
pub fn spawn(dest: PathBuf, games: Vec<Game>, interval_ms: u64, tx: Sender<WatchMsg>, stop: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new().name("agss-cloudpoll".into()).spawn(move || {
        let t0 = std::time::Instant::now();
        let mut p = CloudPoller::new(games.len(), interval_ms);
        while !stop.load(Ordering::SeqCst) {
            let now = t0.elapsed().as_millis() as u64;
            let changed = p.tick(now, &mut |i| cloud::poll_signature(&dest, &games[i]));
            for i in changed {
                if tx.send(WatchMsg::Changed(format!("{}/saves", cloud::dest_key(&games[i])))).is_err() {
                    return;
                }
            }
            // Sommeil fractionne : arret rapide, pas de boucle serree.
            let mut slept = 0;
            while slept < 200 && !stop.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(50));
                slept += 50;
            }
        }
    });
}
