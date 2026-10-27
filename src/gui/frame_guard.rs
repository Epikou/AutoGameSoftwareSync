//! Garde-fou « interface fluide » (2.0.1) : aucune image ne doit durer longtemps, sinon Windows marque la fenetre
//! « Ne repond pas ». Mesure la duree de chaque image (`FrameTimer`), journalise toute image lente (>= 200 ms) et un
//! veilleur separe signale une image qui n'est pas finie apres 1,5 s, avec l'etape ou elle est bloquee. Aucun travail
//! lourd ne doit tourner dans l'interface : copie, hachage, parcours de dossiers, tailles, lecture du coffre, apercu.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Image lente : journalisee.
pub const SLOW_MS: u64 = 200;
/// Image bloquee : signalee par le veilleur pendant qu'elle dure.
pub const STALL_MS: u64 = 1500;

/// Debut de l'image en cours (ms depuis l'horloge du processus), 0 = aucune image en cours.
static FRAME_START: AtomicU64 = AtomicU64::new(0);
/// Image la plus longue depuis le lancement (ms) et nombre d'images lentes.
pub static MAX_MS: AtomicU64 = AtomicU64::new(0);
pub static SLOW_COUNT: AtomicU64 = AtomicU64::new(0);
static STAGE: Mutex<&'static str> = Mutex::new("");
/// Derniere ligne de journal d'image lente (ms depuis l'origine) : au plus une ligne toutes les 2 s.
static LAST_LOG: AtomicU64 = AtomicU64::new(0);

fn origin() -> &'static Instant {
    static O: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    O.get_or_init(Instant::now)
}

fn since_origin_ms() -> u64 {
    origin().elapsed().as_millis() as u64 + 1
}

/// Etape de l'image en cours (pour le diagnostic d'une image bloquee).
pub fn stage(name: &'static str) {
    if let Ok(mut s) = STAGE.lock() {
        *s = name;
    }
}

fn current_stage() -> &'static str {
    STAGE.lock().map(|s| *s).unwrap_or("")
}

/// Mesure d'une image ; fin = duree enregistree (journal si lente).
pub struct FrameTimer {
    t0: Instant,
    log: Option<std::path::PathBuf>,
}

pub fn begin(data_dir: &std::path::Path) -> FrameTimer {
    let _ = origin();
    FRAME_START.store(since_origin_ms(), Ordering::SeqCst);
    stage("debut");
    FrameTimer { t0: Instant::now(), log: Some(data_dir.join("logs").join("watch.log")) }
}

/// Une duree d'image est-elle « lente » ? PUR.
pub fn is_slow(ms: u64) -> bool {
    ms >= SLOW_MS
}

/// Une image commencee il y a `age_ms` est-elle bloquee ? PUR.
pub fn is_stalled(age_ms: u64) -> bool {
    age_ms >= STALL_MS
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        FRAME_START.store(0, Ordering::SeqCst);
        let ms = self.t0.elapsed().as_millis() as u64;
        MAX_MS.fetch_max(ms, Ordering::SeqCst);
        if is_slow(ms) {
            SLOW_COUNT.fetch_add(1, Ordering::SeqCst);
            let now = since_origin_ms();
            let last = LAST_LOG.load(Ordering::SeqCst);
            if now.saturating_sub(last) < 2000 && last != 0 {
                return;
            }
            LAST_LOG.store(now, Ordering::SeqCst);
            if let Some(p) = self.log.take() {
                crate::watch::Logger::new(p, false).log(&format!("[UI] image lente : {} ms (derniere etape : {})", ms, current_stage()));
            }
        }
    }
}

/// Veilleur : journalise une image qui dure plus de 1,5 s, une seule fois par image, avec son etape.
pub fn spawn_watchdog(shared: Arc<super::Shared>) {
    let _ = std::thread::Builder::new().name("agss-ui-watchdog".into()).spawn(move || {
        let lg = crate::watch::Logger::new(shared.data_dir.join("logs").join("watch.log"), false);
        let mut reported: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if shared.quitting.load(Ordering::SeqCst) {
                return;
            }
            let start = FRAME_START.load(Ordering::SeqCst);
            if start == 0 || start == reported {
                continue;
            }
            let age = since_origin_ms().saturating_sub(start);
            if is_stalled(age) {
                reported = start;
                lg.log(&format!("[UI] image bloquee depuis {} ms (etape : {}) : un travail lourd tourne dans l'interface", age, current_stage()));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seuils() {
        assert!(!is_slow(199) && is_slow(200));
        assert!(!is_stalled(1499) && is_stalled(1500));
    }
}
