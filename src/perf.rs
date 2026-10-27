//! Mesures de duree par etape (journal), progression partagee avec l'interface (avec annulation avant
//! l'ecriture finale) et detection d'un dossier partage « a la demande » (mode flux).
use crate::Ctx;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Message d'erreur d'une operation annulee par l'utilisateur.
pub const CANCELLED: &str = "annule par l'utilisateur (rien n'a ete modifie)";

/// Avertissement journal quand le dossier partage semble telecharger les fichiers a la demande.
pub const STREAMING_WARNING: &str = "ATTENTION : le dossier partage telecharge les fichiers a la demande : activez le mode miroir / fichiers hors ligne pour ce dossier (sinon chaque lecture attend le telechargement)";

/// Nombre d'operations (sauvegarde / restauration / resolution) en cours dans ce processus : icone de la zone de notification.
pub static ACTIVE_OPS: AtomicUsize = AtomicUsize::new(0);
/// Elements (noms d'origine) dont une operation comptee est en cours : tuiles « copie en cours » de l'accueil.
static BUSY_GAMES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Elements en cours de copie (sauvegarde, restauration, resolution) dans ce processus.
pub fn busy_games() -> Vec<String> {
    BUSY_GAMES.lock().map(|v| v.clone()).unwrap_or_default()
}

fn busy_remove(game: &str) {
    if let Ok(mut v) = BUSY_GAMES.lock() {
        if let Some(i) = v.iter().position(|g| g == game) {
            v.remove(i);
        }
    }
}

/// Dernier moment (ms) ou un dossier partage « a la demande » a ete detecte (0 = jamais).
pub static STREAMING_SEEN_MS: AtomicU64 = AtomicU64::new(0);

pub fn mark_streaming() {
    STREAMING_SEEN_MS.store(crate::util::now_ms(), Ordering::SeqCst);
}

// ------------------------------------------------------------------ progression

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle = 0,
    /// Lecture + verification de la source (dossier partage).
    Read = 1,
    /// Copie de securite de l'etat local.
    Safety = 2,
    /// Ecriture dans le dossier de sauvegardes (restauration) ou dans le dossier partage (sauvegarde).
    Write = 3,
    /// Preparation d'une sauvegarde (copie locale verifiee).
    Prepare = 4,
}

impl Phase {
    fn from_u8(v: u8) -> Phase {
        match v {
            1 => Phase::Read,
            2 => Phase::Safety,
            3 => Phase::Write,
            4 => Phase::Prepare,
            _ => Phase::Idle,
        }
    }
}

/// Etat partage entre le travail (thread) et l'interface.
#[derive(Default)]
pub struct Progress {
    phase: AtomicU8,
    files_done: AtomicU64,
    files_total: AtomicU64,
    bytes_done: AtomicU64,
    bytes_total: AtomicU64,
    cancel: AtomicBool,
    locked: AtomicBool,
    streaming: AtomicBool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressView {
    pub phase: Phase,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// L'annulation est encore possible (rien n'a ete ecrit dans le dossier de sauvegardes).
    pub cancellable: bool,
    pub cancel_requested: bool,
    pub streaming: bool,
}

impl Progress {
    pub fn new() -> Arc<Progress> {
        Arc::new(Progress::default())
    }

    /// Nouvelle phase : remet les compteurs a zero avec les totaux attendus.
    pub fn begin(&self, phase: Phase, files: u64, bytes: u64) {
        self.phase.store(phase as u8, Ordering::SeqCst);
        self.files_done.store(0, Ordering::SeqCst);
        self.files_total.store(files, Ordering::SeqCst);
        self.bytes_done.store(0, Ordering::SeqCst);
        self.bytes_total.store(bytes, Ordering::SeqCst);
    }

    pub fn add_bytes(&self, n: u64) {
        self.bytes_done.fetch_add(n, Ordering::Relaxed);
    }

    pub fn file_done(&self) {
        self.files_done.fetch_add(1, Ordering::Relaxed);
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Point de non-retour : des fichiers vont etre ecrits dans le dossier de sauvegardes, l'annulation n'est plus prise en compte.
    pub fn lock(&self) {
        self.locked.store(true, Ordering::SeqCst);
    }

    pub fn flag_streaming(&self) {
        mark_streaming();
        self.streaming.store(true, Ordering::SeqCst);
    }

    pub fn streaming(&self) -> bool {
        self.streaming.load(Ordering::SeqCst)
    }

    /// Erreur `CANCELLED` si l'utilisateur a annule ET qu'il est encore temps.
    pub fn check_cancel(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) && !self.locked.load(Ordering::SeqCst) {
            Err(CANCELLED.to_string())
        } else {
            Ok(())
        }
    }

    pub fn view(&self) -> ProgressView {
        ProgressView {
            phase: Phase::from_u8(self.phase.load(Ordering::SeqCst)),
            files_done: self.files_done.load(Ordering::Relaxed),
            files_total: self.files_total.load(Ordering::Relaxed),
            bytes_done: self.bytes_done.load(Ordering::Relaxed),
            bytes_total: self.bytes_total.load(Ordering::Relaxed),
            cancellable: !self.locked.load(Ordering::SeqCst),
            cancel_requested: self.cancel.load(Ordering::SeqCst),
            streaming: self.streaming.load(Ordering::SeqCst),
        }
    }
}

// ------------------------------------------------------------------ formats

pub fn fmt_secs(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64())
}

pub fn fmt_mb(bytes: u64) -> String {
    format!("{:.1} Mo", bytes as f64 / (1024.0 * 1024.0))
}

/// Ligne de journal d'une operation : une duree par etape + total + volume.
pub fn format_summary(label: &str, game: &str, steps: &[(String, Duration)], total: Duration, files: usize, bytes: u64, interrupted: bool) -> String {
    let parts: Vec<String> = steps.iter().map(|(n, d)| format!("{} {}", n, fmt_secs(*d))).collect();
    format!(
        "[Perf] {} {}: {}{}total {} ({} fichiers, {}){}",
        label,
        if game.is_empty() { String::new() } else { format!("{} ", game) },
        parts.join(", "),
        if parts.is_empty() { "" } else { ", " },
        fmt_secs(total),
        files,
        fmt_mb(bytes),
        if interrupted { " [interrompu]" } else { "" }
    )
}

/// Une analyse (plan) n'est journalisee que si elle depasse ce seuil : elle est lancee tres souvent.
pub const PLAN_LOG_MIN: Duration = Duration::from_millis(500);

// ------------------------------------------------------------------ journal + minuteur

/// Ecrit une ligne dans le Journal (logs/watch.log).
pub fn log(ctx: &Ctx, msg: &str) {
    let lg = crate::watch::Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
    lg.log(msg);
}

/// Minuteur d'une operation : `step("nom")` note la duree depuis l'etape precedente ; le resume est
/// ecrit dans le Journal a `finish` (ou, si l'operation echoue / est interrompue, quand l'objet disparait).
pub struct Op<'a> {
    ctx: &'a Ctx,
    label: String,
    game: String,
    t0: Instant,
    last: Instant,
    steps: Vec<(String, Duration)>,
    files: usize,
    bytes: u64,
    done: bool,
    min_total: Duration,
    counted: bool,
}

impl<'a> Op<'a> {
    pub fn start(ctx: &'a Ctx, label: &str, game: &str) -> Op<'a> {
        ACTIVE_OPS.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut v) = BUSY_GAMES.lock() {
            v.push(game.to_string());
        }
        let now = Instant::now();
        Op { ctx, label: label.to_string(), game: game.to_string(), t0: now, last: now, steps: vec![], files: 0, bytes: 0, done: false, min_total: Duration::ZERO, counted: true }
    }

    /// N'ecrit au Journal que si l'operation dure au moins `d` (analyses frequentes).
    pub fn with_min(mut self, d: Duration) -> Self {
        if self.counted {
            // Analyse frequente : ne compte pas comme « synchronisation en cours ».
            self.counted = false;
            ACTIVE_OPS.fetch_sub(1, Ordering::SeqCst);
            busy_remove(&self.game);
        }
        self.min_total = d;
        self
    }

    pub fn step(&mut self, name: &str) {
        let now = Instant::now();
        self.steps.push((name.to_string(), now - self.last));
        self.last = now;
    }

    pub fn set_counts(&mut self, files: usize, bytes: u64) {
        self.files = files;
        self.bytes = bytes;
    }

    pub fn finish(mut self) -> String {
        self.done = true;
        let total = self.t0.elapsed();
        let s = format_summary(&self.label, &self.game, &self.steps, total, self.files, self.bytes, false);
        if total >= self.min_total {
            log(self.ctx, &s);
        }
        s
    }
}

impl Drop for Op<'_> {
    fn drop(&mut self) {
        if self.counted {
            ACTIVE_OPS.fetch_sub(1, Ordering::SeqCst);
            busy_remove(&self.game);
        }
        if !self.done {
            if self.t0.elapsed() < self.min_total {
                return;
            }
            let s = format_summary(&self.label, &self.game, &self.steps, self.t0.elapsed(), self.files, self.bytes, true);
            log(self.ctx, &s);
        }
    }
}

// ------------------------------------------------------------------ mode flux (fichiers a la demande)

const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

/// Attributs Windows d'un fichier « pas encore telecharge » (OFFLINE / RECALL_*) : sa lecture declenche un telechargement.
pub fn attrs_say_on_demand(attrs: u32) -> bool {
    attrs & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0
}

/// Lecture anormalement lente d'un fichier de taille normale (mesure reelle, pas une supposition) :
/// moins de 1 Mo/s sur au moins 512 Ko, ou moins de 2 Mo/s apres 10 s.
pub fn read_is_slow(bytes: u64, elapsed: Duration) -> bool {
    let secs = elapsed.as_secs_f64().max(0.000_001);
    let rate = bytes as f64 / secs;
    (bytes >= 512 * 1024 && rate < 1_048_576.0) || (secs >= 10.0 && rate < 2_097_152.0)
}

#[cfg(windows)]
pub fn file_attrs(md: &std::fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes()
}

#[cfg(not(windows))]
pub fn file_attrs(_md: &std::fs::Metadata) -> u32 {
    0
}

/// Compte comme « activite » (icone de la zone de notification : synchronisation en cours) tant qu'il existe.
pub struct ActivityGuard;

impl ActivityGuard {
    pub fn new() -> ActivityGuard {
        ACTIVE_OPS.fetch_add(1, Ordering::SeqCst);
        ActivityGuard
    }
}

impl Default for ActivityGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        ACTIVE_OPS.fetch_sub(1, Ordering::SeqCst);
    }
}
