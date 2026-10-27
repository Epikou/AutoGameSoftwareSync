//! Surveillance des jeux : backup a la fermeture (apres stabilite des fichiers),
//! synchronisation des jeux FERMES (jamais un jeu en cours), instance unique, logs avec rotation.
//!
//! LIMITE ASSUMEE : on ne peut pas intercepter le lancement d'un jeu pour restaurer AVANT
//! qu'il lise ses sauvegardes (il faudrait un hook de lancement). Compensation : au demarrage
//! de `watch` puis periodiquement (`resync`), tout jeu FERME est resynchronise (restauration si
//! le cloud est plus recent et que le local n'a pas change). Un jeu lance entre deux
//! resynchronisations peut donc demarrer sur une sauvegarde un peu ancienne.
use crate::backup::{self, Plan};
use crate::config::Game;
use crate::{util, Ctx, Res};
use crate::dirwatch::WatchMsg;
use crate::reactive::{self, Check, ReactParams, VAction, Verifier};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ Logs

pub struct Logger {
    pub path: PathBuf,
    pub max_bytes: u64,
    /// Nombre de fichiers de rotation conserves (.1 ... .keep).
    pub keep: usize,
    pub echo: bool,
}

impl Logger {
    pub fn new(path: PathBuf, echo: bool) -> Self {
        Logger { path, max_bytes: 1024 * 1024, keep: 3, echo }
    }

    fn rotate_if_needed(&self) {
        let big = std::fs::metadata(&self.path).map(|m| m.len() >= self.max_bytes).unwrap_or(false);
        if !big {
            return;
        }
        let num = |i: usize| PathBuf::from(format!("{}.{}", self.path.display(), i));
        for i in (1..self.keep).rev() {
            if num(i).exists() {
                let _ = std::fs::rename(num(i), num(i + 1)); // remplace le plus ancien (nos propres logs)
            }
        }
        let _ = std::fs::rename(&self.path, num(1));
    }

    pub fn log(&self, msg: &str) {
        let line = format!("{} {}\n", util::timestamp(util::now_ms()), msg);
        if self.echo {
            print!("{}", line);
        }
        if let Some(p) = self.path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        self.rotate_if_needed();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

// ------------------------------------------------------------------ Instance unique

pub struct InstanceLock {
    path: PathBuf,
    pid: u32,
}

/// Le processus `pid` existe-t-il ? (API Windows directe, aucun sous-processus ; doute => vivant)
pub fn pid_alive(pid: u32) -> bool {
    crate::process::pid_alive(pid)
}

impl InstanceLock {
    pub fn acquire(dir: &Path, alive: &dyn Fn(u32) -> bool) -> Res<InstanceLock> {
        Self::acquire_named(dir, "watch.lock", alive)
    }

    pub fn acquire_named(dir: &Path, name: &str, alive: &dyn Fn(u32) -> bool) -> Res<InstanceLock> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(name);
        let pid = std::process::id();
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    write!(f, "{}", pid)?;
                    return Ok(InstanceLock { path, pid });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let txt = std::fs::read_to_string(&path).unwrap_or_default();
                    match txt.trim().parse::<u32>() {
                        Ok(other) if !alive(other) => {
                            // Verrou perime d'un watch mort : on le reprend.
                            let _ = std::fs::remove_file(&path);
                        }
                        Ok(other) => return Err(format!("un watch tourne deja (pid {})", other).into()),
                        Err(_) => return Err("un watch est en cours de demarrage (verrou present)".into()),
                    }
                }
                Err(e) => return Err(format!("verrou impossible : {}", e).into()),
            }
        }
        Err("impossible d'obtenir le verrou d'instance".into())
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).map(|t| t.trim() == self.pid.to_string()).unwrap_or(false) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn stop_file(ctx: &Ctx) -> PathBuf {
    ctx.data_dir.join("watch.stop")
}

// ------------------------------------------------------------------ Stabilite

/// Signature legere (chemin, taille, mtime) pour detecter qu'un jeu a fini d'ecrire.
/// Signature de tous les emplacements actifs d un jeu (union).
pub fn game_signature(g: &Game) -> Vec<(String, u64, u64)> {
    let mut v: Vec<(String, u64, u64)> = g.enabled_locations().iter().flat_map(|l| signature(&l.path)).collect();
    v.sort();
    v
}

pub fn signature(dir: &Path) -> Vec<(String, u64, u64)> {
    let mut v = Vec::new();
    for e in walkdir::WalkDir::new(dir).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        if let Ok(md) = e.metadata() {
            let mt = md.modified().map(util::mtime_ms).unwrap_or(0);
            v.push((e.path().to_string_lossy().into_owned(), md.len(), mt));
        }
    }
    v.sort();
    v
}

// ------------------------------------------------------------------ Watcher

#[derive(Clone, Copy)]
pub struct WatchCfg {
    /// Stabilite exigee apres fermeture (ms).
    pub stable_ms: u64,
    /// Intervalle de resynchronisation des jeux fermes (ms).
    pub resync_ms: u64,
    /// Tentatives max de backup apres fermeture.
    pub max_tries: u32,
    /// Delai de securite apres le dernier evenement du dossier partage avant de verifier / restaurer (ms).
    pub safety_ms: u64,
    /// Intervalle de reessai quand la sauvegarde distante est incomplete (ms).
    pub retry_ms: u64,
    /// Temps maximum de reessais (ms).
    pub retry_max_ms: u64,
}

impl WatchCfg {
    pub fn from_params(p: &ReactParams) -> WatchCfg {
        WatchCfg { stable_ms: p.stable_ms, resync_ms: p.fallback_ms, max_tries: 5, safety_ms: p.safety_ms, retry_ms: p.retry_ms, retry_max_ms: p.retry_max_ms }
    }

    pub fn react(&self) -> ReactParams {
        ReactParams { safety_ms: self.safety_ms, retry_ms: self.retry_ms, retry_max_ms: self.retry_max_ms, fallback_ms: self.resync_ms, stable_ms: self.stable_ms }
    }
}

impl Default for WatchCfg {
    fn default() -> Self {
        WatchCfg::from_params(&reactive::params(reactive::Reactivity::Instant, &Default::default()))
    }
}

enum St {
    Running,
    Idle { last_sync_ms: u64 },
    Pending { sig: Vec<(String, u64, u64)>, since_ms: u64, tries: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    BackedUp,
    Restored,
    Conflict,
    /// Conflit resolu automatiquement (version du cloud appliquee, version locale conservee).
    ConflictResolved,
    Error,
}

#[derive(Clone, Debug)]
pub struct WatchEvent {
    pub game: String,
    pub kind: EventKind,
    pub detail: String,
}

pub struct Watcher<'a> {
    /// Delais par jeu : nom -> (stabilite ms, resync ms).
    pub timing: std::collections::HashMap<String, (Option<u64>, Option<u64>)>,
    /// Jeu -> resoudre automatiquement les vrais conflits en prenant la version du cloud (defaut : non,
    /// l'appelant applique les reglages `conflict_mode`).
    pub auto_cloud: std::collections::HashMap<String, bool>,
    /// Rappel appele a chaque evenement notable (notifications, interface).
    pub on_event: Option<Box<dyn Fn(WatchEvent) + 'a>>,
    /// Appele quand l'etat d'un jeu a pu changer (lancement / fermeture, fin d'une synchro) : fichier d'etat.
    pub on_change: Option<Box<dyn Fn() + 'a>>,
    /// Drapeau partage : un jeu surveille tourne (utilise pour liberer la manette).
    pub running_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ctx: &'a Ctx,
    games: Vec<Game>,
    dest: PathBuf,
    cfg: WatchCfg,
    log: &'a Logger,
    st: Vec<St>,
    /// Evenements du dossier partage (voir `dirwatch`) et demandes « Synchroniser maintenant » (None = tous les jeux).
    rx: Option<Receiver<WatchMsg>>,
    triggers: Option<Arc<Mutex<Vec<Option<String>>>>>,
    verifiers: HashMap<usize, Verifier>,
    keys: Vec<Vec<String>>,
    /// Derniere empreinte rapide (distant + local) apres une synchro reussie : si rien n'a change, la resynchro periodique ne calcule rien.
    sigs: Vec<Option<u64>>,
    activity: Option<crate::perf::ActivityGuard>,
    /// Planification : derniere execution (ou depart) par jeu, derniere echeance journalisee, derniere seconde evaluee.
    sched_anchor: Vec<Option<i64>>,
    sched_logged: Vec<i64>,
    sched_last_s: i64,
}

impl<'a> Watcher<'a> {
    pub fn new(ctx: &'a Ctx, games: Vec<Game>, dest: PathBuf, cfg: WatchCfg, log: &'a Logger) -> Self {
        let st = games.iter().map(|_| St::Idle { last_sync_ms: 0 }).collect();
        let keys = games
            .iter()
            .map(|g| {
                let mut k = vec![crate::cloud::dest_key(g), crate::cloud::game_key(&g.name)];
                k.dedup();
                k
            })
            .collect();
        let sigs = vec![None; games.len()];
        let now_s = crate::schedule::now_utc_s();
        let sched_anchor = games.iter().map(|g| g.schedule.as_ref().filter(|s| s.is_valid()).map(|s| s.last_run_utc.unwrap_or(now_s))).collect();
        let sched_logged = vec![0; games.len()];
        Watcher { timing: Default::default(), auto_cloud: Default::default(), on_event: None, on_change: None, running_flag: None, ctx, games, dest, cfg, log, st, rx: None, triggers: None, verifiers: HashMap::new(), keys, sigs, activity: None, sched_anchor, sched_logged, sched_last_s: 0 }
    }

    /// Force le point de depart de la planification d'un jeu (tests, rattrapage).
    pub fn set_schedule_anchor(&mut self, i: usize, utc: i64) {
        self.sched_anchor[i] = Some(utc);
    }

    /// Planification avec l'horloge et le fuseau reels ; evaluee au plus une fois par seconde.
    pub fn tick_schedule_now(&mut self) {
        let now = crate::schedule::now_utc_s();
        if now == self.sched_last_s {
            return;
        }
        self.sched_last_s = now;
        self.tick_schedule(now, &crate::timefmt::WinTz);
    }

    /// Synchro planifiee des jeux dont l'echeance est arrivee (horloge injectee pour les tests). Retourne les jeux synchronises.
    pub fn tick_schedule(&mut self, now: i64, tz: &dyn crate::timefmt::TzProvider) -> Vec<String> {
        let mut done = Vec::new();
        for i in 0..self.games.len() {
            let g = self.games[i].clone();
            let (Some(sch), Some(anchor)) = (g.schedule.clone(), self.sched_anchor[i]) else { continue };
            let Some(ev) = crate::schedule::evaluate(&sch, anchor, now, tz) else { continue };
            let Some(due) = ev.due else {
                if self.sched_logged[i] != ev.next_utc {
                    self.sched_logged[i] = ev.next_utc;
                    let when = crate::timefmt::iso_local((ev.next_utc.max(0) as u64) * 1000, tz);
                    self.log.log(&format!("[{}] prochaine synchro planifiee : {}", g.name, when));
                }
                continue;
            };
            self.sched_anchor[i] = Some(now);
            self.sched_logged[i] = 0;
            if self.running(&g) == Some(true) || !matches!(self.st[i], St::Idle { .. }) {
                self.log.log(&format!("[{}] synchro planifiee ignoree : en cours d'utilisation (synchro a la fermeture)", g.name));
                continue;
            }
            self.log.log(&format!("[{}] synchro planifiee ({}){}", g.name, due.label, if due.catch_up { " : rattrapage d'une echeance manquee" } else { "" }));
            crate::schedule::record_run(self.ctx, &g.name, now);
            match self.sync_scheduled(&g) {
                Ok(true) => {
                    self.sigs[i] = Some(self.fast_sig(&g));
                    done.push(g.name.clone());
                }
                Ok(false) => {}
                Err(e) => {
                    self.log.log(&format!("[{}] ERREUR synchro planifiee : {}", g.name, e));
                    self.emit(&g.name, EventKind::Error, e.to_string());
                }
            }
        }
        done
    }

    /// Synchro planifiee : sauvegarde toujours si le local a change ; restauration seulement si aucun fichier n'est verrouille.
    /// Retourne faux si la restauration a ete reportee.
    /// Le coffre doit exister : jamais de coffre recree en silence par une copie automatique (dossier du service de partage
    /// absent, disque debranche, dossier renomme) ; la copie attend son retour.
    fn vault_present(&self) -> Res<()> {
        if self.dest.is_dir() {
            Ok(())
        } else {
            Err(format!("dossier partage injoignable : {} (rien n'est cree, nouvel essai plus tard)", self.dest.display()).into())
        }
    }

    fn sync_scheduled(&self, g: &Game) -> Res<bool> {
        self.vault_present()?;
        let plan = backup::plan(self.ctx, g, &self.dest)?;
        let would_restore = plan == Plan::Restore || (plan == Plan::Conflict && self.auto_cloud.get(&g.name).copied().unwrap_or(false));
        if crate::schedule::guard(would_restore, would_restore && crate::schedule::any_locked(g)) == crate::schedule::Guard::PostponeRestore {
            self.log.log(&format!("[{}] restauration reportee : fichiers en cours d'utilisation (nouvel essai a l'echeance suivante)", g.name));
            return Ok(false);
        }
        self.sync_closed(g)?;
        Ok(true)
    }

    fn emit(&self, game: &str, kind: EventKind, detail: String) {
        self.changed();
        // Coffre absent (disque debranche, service ferme) : l'interface l'affiche deja (« Ton coffre est injoignable ») ;
        // pas de notification d'erreur a chaque nouvel essai.
        if kind == EventKind::Error && !self.dest.is_dir() {
            return;
        }
        if let Some(f) = &self.on_event {
            f(WatchEvent { game: game.to_string(), kind, detail });
        }
    }

    /// Indique a l interface (manette) si au moins un jeu surveille tourne.
    fn publish_running(&self) {
        if let Some(f) = &self.running_flag {
            f.store(self.st.iter().any(|s| matches!(s, St::Running)), std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn changed(&self) {
        if let Some(f) = &self.on_change {
            f();
        }
    }

    fn stable_ms(&self, g: &str) -> u64 {
        self.timing.get(g).and_then(|t| t.0).unwrap_or(self.cfg.stable_ms)
    }

    fn resync_ms(&self, g: &str) -> u64 {
        self.timing.get(g).and_then(|t| t.1).unwrap_or(self.cfg.resync_ms)
    }

    /// Sans exe configure on ne peut pas savoir si le jeu tourne : jamais de sync automatique.
    fn running(&self, g: &Game) -> Option<bool> {
        let exe = g.exe.as_deref().filter(|e| !e.trim().is_empty())?;
        Some(match self.ctx.procs.is_running(exe) {
            Ok(b) => b,
            Err(e) => {
                self.log.log(&format!("[{}] etat du processus indeterminable ({}) : considere en cours", g.name, e));
                true
            }
        })
    }

    /// Synchronise un jeu FERME selon le plan (jamais appele pour un jeu en cours).
    fn sync_closed(&self, g: &Game) -> Res<()> {
        self.sync_closed_at(g, None)
    }

    /// Idem ; `detected` = instant de la detection (evenement du dossier partage) pour journaliser les durees.
    fn sync_closed_at(&self, g: &Game, detected: Option<Instant>) -> Res<()> {
        let r = self.sync_closed_inner(g, detected);
        self.changed();
        if let Some(p) = &self.ctx.progress {
            p.begin(crate::perf::Phase::Idle, 0, 0); // la barre de progression de l'interface disparait
        }
        r
    }

    fn sync_closed_inner(&self, g: &Game, detected: Option<Instant>) -> Res<()> {
        self.vault_present()?;
        match backup::plan(self.ctx, g, &self.dest)? {
            Plan::Nothing => {}
            Plan::InSync => {
                // Point de synchro seulement : jamais de nouvelle version si le cloud vient d'avancer (course entre deux PC).
                backup::backup_in_sync(self.ctx, g, &self.dest)?;
            }
            Plan::Backup => {
                match backup::backup(self.ctx, g, &self.dest)? {
                    backup::BackupOutcome::UpToDate => self.log.log(&format!("[{}] sauvegarde ignoree : contenu identique", g.name)),
                    _ => {
                        self.log.log(&format!("[{}] sauvegarde effectuee", g.name));
                        self.emit(&g.name, EventKind::BackedUp, String::new());
                    }
                }
            }
            Plan::Restore => {
                let wait = detected.map(|d| d.elapsed());
                let machine = backup::active_meta(&self.dest, g).map(|m| m.machine).unwrap_or_else(|| "?".to_string());
                self.log.log(&format!("[{}] cloud plus recent detecte (sauvegarde de {}), verification...", g.name, machine));
                if let Some(pv) = crate::preview::for_game(self.ctx, g, &self.dest, None) {
                    self.log.log(&format!("[{}] restauration automatique : {}", g.name, crate::preview::journal_summary(&pv)));
                }
                let t0 = Instant::now();
                let r = backup::restore(self.ctx, g, &self.dest)?;
                let dt = t0.elapsed();
                for u in &r.unknown_locations {
                    self.log.log(&format!("[{}] emplacement inconnu : {} (fichiers non restaures ; a configurer dans More)", g.name, u));
                }
                self.log.log(&format!("[{}] cloud plus recent : restaure ({} fichiers), snapshot: {:?}", g.name, r.files_restored, r.safety_dir));
                if r.local_only_kept > 0 {
                    self.log.log(&format!("[{}] restauration terminee, {} fichiers locaux supplementaires conserves", g.name, r.local_only_kept));
                }
                self.log.log(&format!(
                    "[{}] restauration terminee en {:.1} s (detection -> debut {:.1} s, verification + restauration {:.1} s)",
                    g.name,
                    (wait.unwrap_or_default() + dt).as_secs_f64(),
                    wait.unwrap_or_default().as_secs_f64(),
                    dt.as_secs_f64()
                ));
                self.emit(&g.name, EventKind::Restored, format!("{}", r.files_restored));
            }
            Plan::Conflict if self.auto_cloud.get(&g.name).copied().unwrap_or(false) => {
                self.log_conflict_start(g, true);
                // Mode « cloud automatique » (jeu FERME uniquement : sync_closed n'est jamais appele pour un jeu en cours).
                // Ordre : (1) la version locale non synchronisee est sauvegardee comme branche, (2) snapshot safety verifie,
                // (3) restauration de la tete du cloud, (4) sauvegarde de resolution reunissant les branches.
                match backup::resolve_conflict(self.ctx, g, &self.dest, false) {
                    Ok(r) => {
                        self.log.log(&format!(
                            "[{}] CONFLIT resolu automatiquement : version du cloud appliquee ; votre version locale reste dans l'historique (snapshot de securite : {:?}, sauvegarde de resolution : {})",
                            g.name,
                            r.restored.as_ref().and_then(|x| x.safety_dir.clone()),
                            r.merged_dir.display()
                        ));
                        self.emit(&g.name, EventKind::ConflictResolved, r.merged_dir.display().to_string());
                    }
                    Err(e) => {
                        let m = e.to_string();
                        if m.contains("REFUS") {
                            self.log.log(&format!("[{}] conflit : resolution automatique reportee ({})", g.name, m));
                        } else {
                            // Rien n'a ete supprime : la version locale est deja dans le cloud (branche) ou dans le dossier de securite.
                            self.log.log(&format!("[{}] ERREUR resolution automatique du conflit : {} (rien n'a ete supprime)", g.name, m));
                            self.emit(&g.name, EventKind::Error, m);
                        }
                    }
                }
            }
            Plan::Conflict => {
                self.log_conflict_start(g, false);
                // Vrai conflit : la version locale est conservee comme branche ; aucune restauration automatique.
                let o = backup::backup(self.ctx, g, &self.dest)?;
                if let backup::BackupOutcome::Conflict { dir } = &o {
                    self.log.log(&format!(
                        "[{}] CONFLIT : les deux versions sont conservees ({}). Aucune restauration automatique ; a resoudre dans la fiche de suivi ou avec `AutoGameSoftwareSync resolve`.",
                        g.name,
                        dir.display()
                    ));
                    self.emit(&g.name, EventKind::Conflict, format!("{:?}", o));
                }
            }
        }
        Ok(())
    }

    /// Journal d un conflit detecte : premiere synchro (versions differentes) ou divergence, puis mode applique.
    fn log_conflict_start(&self, g: &Game, auto: bool) {
        if let Some(ci) = backup::conflict_info(self.ctx, g, &self.dest) {
            if ci.first_sync {
                self.log.log(&format!(
                    "[{}] premiere synchro : versions differentes (local {} fichiers / cloud {} fichiers de {}) = conflit",
                    g.name, ci.local_files, ci.cloud_files, ci.cloud_machine
                ));
            } else {
                self.log.log(&format!(
                    "[{}] conflit : les deux PC ont modifie la meme entree (local {} fichiers / cloud {} fichiers de {})",
                    g.name, ci.local_files, ci.cloud_files, ci.cloud_machine
                ));
            }
        }
        if auto {
            self.log.log(&format!("[{}] conflit : version locale gardee en branche, version du cloud appliquee (mode cloud automatique)", g.name));
        }
    }

    /// Reprise apres une pause : tout ce qui a pu arriver pendant la pause est verifie tout de suite (sans attendre le controle de secours).
    pub fn resume(&mut self, now_ms: u64) {
        for i in 0..self.games.len() {
            self.sigs[i] = None;
            self.note_remote_activity(i, now_ms, true);
        }
    }

    /// Branche les evenements du dossier partage (voir `dirwatch`).
    pub fn set_events(&mut self, rx: Receiver<WatchMsg>) {
        self.rx = Some(rx);
    }

    /// Branche la file des demandes « Synchroniser maintenant » (None = tous les jeux, Some(nom) = ce jeu).
    pub fn set_triggers(&mut self, t: Arc<Mutex<Vec<Option<String>>>>) {
        self.triggers = Some(t);
    }

    /// Des verifications de sauvegardes distantes sont-elles en cours ? (la boucle accelere alors son rythme)
    pub fn has_pending(&self) -> bool {
        !self.verifiers.is_empty()
    }

    /// Empreinte rapide d'un jeu : distant (dossiers + meta.json) et local (chemin, taille, date).
    fn fast_sig(&self, g: &Game) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        crate::cloud::quick_signature(&self.dest, g).hash(&mut h);
        game_signature(g).hash(&mut h);
        h.finish()
    }

    fn note_remote_activity(&mut self, i: usize, now_ms: u64, skip_safety: bool) {
        if !matches!(self.st[i], St::Idle { .. }) {
            return; // jeu en cours / en attente de stabilite : la fermeture declenche la synchro
        }
        if skip_safety {
            self.verifiers.insert(i, Verifier::new(now_ms, true));
        } else {
            self.verifiers.entry(i).and_modify(|v| v.note_activity(now_ms)).or_insert_with(|| Verifier::new(now_ms, false));
        }
    }

    /// Vide les evenements du dossier partage et les demandes manuelles. Retourne vrai s'il y avait du nouveau.
    pub fn pump(&mut self, now_ms: u64) -> bool {
        let mut msgs = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
        }
        let mut any = false;
        for m in msgs {
            match m {
                WatchMsg::Changed(rel) => {
                    // Evenement hors de <cle_jeu>/saves/ : ignore.
                    if let Some(i) = reactive::game_of_event(&rel, &self.keys) {
                        self.note_remote_activity(i, now_ms, false);
                        any = true;
                    }
                }
                WatchMsg::Rescan => {
                    for i in 0..self.games.len() {
                        self.note_remote_activity(i, now_ms, false);
                    }
                    any = true;
                }
            }
        }
        let reqs: Vec<Option<String>> = self.triggers.as_ref().and_then(|t| t.lock().ok().map(|mut q| std::mem::take(&mut *q))).unwrap_or_default();
        for r in reqs {
            any = true;
            for i in 0..self.games.len() {
                if r.as_deref().map(|n| n == self.games[i].name).unwrap_or(true) {
                    if matches!(self.st[i], St::Idle { .. }) {
                        self.log.log(&format!("[{}] synchronisation demandee", self.games[i].name));
                    } else {
                        self.log.log(&format!("[{}] synchronisation demandee : en cours d'utilisation, traitee a la fermeture", self.games[i].name));
                    }
                    self.note_remote_activity(i, now_ms, true);
                }
            }
        }
        self.update_activity();
        any
    }

    /// L'icone de la zone de notification passe au bleu tant qu'une verification est en cours.
    fn update_activity(&mut self) {
        if self.verifiers.is_empty() {
            self.activity = None;
        } else if self.activity.is_none() {
            self.activity = Some(crate::perf::ActivityGuard::new());
        }
    }

    /// Fait avancer les verifications de sauvegardes distantes (machine a etats pure : `reactive::Verifier`).
    fn tick_verifiers(&mut self, now_ms: u64) {
        let idxs: Vec<usize> = self.verifiers.keys().copied().collect();
        let p = self.cfg.react();
        for i in idxs {
            let g = self.games[i].clone();
            let Some(mut v) = self.verifiers.remove(&i) else { continue };
            let Some(running) = self.running(&g) else {
                // Sans exe : jamais de synchro automatique. Une demande explicite (« Synchroniser maintenant », accueil 2.0)
                // suit les regles d'une synchro planifiee : sauvegarde si besoin, restauration seulement sans fichier verrouille.
                if v.manual() {
                    match self.sync_scheduled(&g) {
                        Ok(_) => self.sigs[i] = Some(self.fast_sig(&g)),
                        Err(e) => {
                            self.log.log(&format!("[{}] ERREUR synchro demandee : {}", g.name, e));
                            self.emit(&g.name, EventKind::Error, e.to_string());
                        }
                    }
                }
                continue;
            };
            let dest = self.dest.clone();
            let act = v.poll(now_ms, &p, running, &mut || -> Check { crate::cloud::remote_check(&dest, &g) });
            let waited = Duration::from_millis(now_ms.saturating_sub(v.detected_ms));
            match act {
                VAction::Wait => {
                    if v.incomplete_seen() && !v.waiting_logged {
                        v.waiting_logged = true;
                        let reason = crate::cloud::scan_dest(&self.dest, &g).incomplete.first().map(|x| x.reason.clone()).unwrap_or_default();
                        self.log.log(&format!("[{}] verification en attente : sauvegarde incomplete ({})", g.name, reason));
                    }
                    self.verifiers.insert(i, v);
                }
                VAction::Postpone => {
                    self.log.log(&format!("[{}] sauvegarde distante detectee mais le programme est en cours d'utilisation : traitee a la fermeture", g.name));
                }
                VAction::Abandon => {
                    self.log.log(&format!("[{}] sauvegarde distante encore incomplete apres {:.0} s : abandon, reprise par la resynchro periodique", g.name, waited.as_secs_f64()));
                }
                VAction::Proceed => {
                    self.log.log(&format!("[{}] verification terminee : sauvegarde distante complete, synchronisation lancee ({:.1} s apres la detection)", g.name, waited.as_secs_f64()));
                    let detected = Instant::now().checked_sub(waited).unwrap_or_else(Instant::now);
                    match self.sync_closed_at(&g, Some(detected)) {
                        Ok(()) => {
                            self.st[i] = St::Idle { last_sync_ms: now_ms };
                            self.sigs[i] = Some(self.fast_sig(&g));
                        }
                        Err(e) => {
                            self.log.log(&format!("[{}] ERREUR synchro apres evenement : {} (reprise par la resynchro periodique)", g.name, e));
                            self.emit(&g.name, EventKind::Error, e.to_string());
                        }
                    }
                }
            }
        }
        self.update_activity();
    }

    /// A appeler une fois au lancement de watch.
    pub fn startup(&mut self, now_ms: u64) {
        // Nos propres dossiers de travail locaux orphelins (crash) : nettoyes ici, et SEULEMENT ici.
        let n = backup::cleanup_local_tmp(self.ctx, std::time::Duration::from_secs(600));
        if n > 0 {
            self.log.log(&format!("{} dossier(s) de travail local orphelin(s) supprime(s)", n));
        }
        // Anomalies de la destination (signalees une seule fois, jamais supprimees).
        for g in self.games.clone() {
            let snap = crate::cloud::scan_dest(&self.dest, &g);
            for inc in &snap.incomplete {
                self.log.log(&format!("[{}] dossier incomplet ignore (synchronisation Drive en cours ?) : {} ({})", g.name, inc.dir.display(), inc.reason));
            }
            for t in &snap.tmp_dirs {
                self.log.log(&format!("[{}] dossier temporaire ignore dans la destination : {}", g.name, t.display()));
            }
        }
        for i in 0..self.games.len() {
            let g = self.games[i].clone();
            match self.running(&g) {
                None => self.log.log(&format!("[{}] pas d'exe configure : ignore par watch", g.name)),
                Some(true) => {
                    self.log.log(&format!("[{}] deja en cours d'utilisation : aucune synchro (sauvegarde a la fermeture)", g.name));
                    self.st[i] = St::Running;
                }
                Some(false) => {
                    match self.sync_closed(&g) {
                        Ok(()) => self.sigs[i] = Some(self.fast_sig(&g)),
                        Err(e) => self.log.log(&format!("[{}] ERREUR synchro au demarrage : {}", g.name, e)),
                    }
                    self.st[i] = St::Idle { last_sync_ms: now_ms };
                }
            }
        }
        self.publish_running();
    }

    /// Un cycle de surveillance. `now_ms` est injecte pour les tests.
    pub fn tick(&mut self, now_ms: u64) {
        self.pump(now_ms);
        self.tick_verifiers(now_ms);
        for i in 0..self.games.len() {
            let g = self.games[i].clone();
            let Some(running) = self.running(&g) else { continue };
            if running {
                if !matches!(self.st[i], St::Running) {
                    self.log.log(&format!("[{}] lancement detecte", g.name));
                    self.changed();
                }
                self.st[i] = St::Running;
                continue;
            }
            let next = match std::mem::replace(&mut self.st[i], St::Idle { last_sync_ms: now_ms }) {
                St::Running => {
                    self.log.log(&format!("[{}] fermeture detectee : attente de stabilite des fichiers", g.name));
                    self.changed();
                    St::Pending { sig: game_signature(&g), since_ms: now_ms, tries: 0 }
                }
                St::Pending { sig, since_ms, tries } => {
                    let cur = game_signature(&g);
                    if cur != sig {
                        St::Pending { sig: cur, since_ms: now_ms, tries }
                    } else if now_ms.saturating_sub(since_ms) >= self.stable_ms(&g.name) {
                        match self.sync_closed(&g) {
                            Ok(()) => St::Idle { last_sync_ms: now_ms },
                            Err(e) => {
                                self.log.log(&format!("[{}] ERREUR sauvegarde (essai {}) : {}", g.name, tries + 1, e));
                                self.emit(&g.name, EventKind::Error, e.to_string());
                                if tries + 1 >= self.cfg.max_tries {
                                    self.log.log(&format!("[{}] abandon apres {} essais ; reessai a la prochaine resynchro", g.name, tries + 1));
                                    St::Idle { last_sync_ms: now_ms }
                                } else {
                                    St::Pending { sig: cur, since_ms: now_ms, tries: tries + 1 }
                                }
                            }
                        }
                    } else {
                        St::Pending { sig, since_ms, tries }
                    }
                }
                St::Idle { last_sync_ms } => {
                    if now_ms.saturating_sub(last_sync_ms) >= self.resync_ms(&g.name) {
                        // Comparaison RAPIDE d'abord (liste des dossiers + meta.json distants, signature locale) : si rien n'a
                        // change depuis la derniere synchro reussie, aucun plan ni empreinte n'est calcule.
                        if self.sigs[i].is_none() || self.sigs[i] != Some(self.fast_sig(&g)) {
                            match self.sync_closed(&g) {
                                Ok(()) => self.sigs[i] = Some(self.fast_sig(&g)),
                                Err(e) => self.log.log(&format!("[{}] ERREUR resynchro : {}", g.name, e)),
                            }
                        }
                        St::Idle { last_sync_ms: now_ms }
                    } else {
                        St::Idle { last_sync_ms }
                    }
                }
            };
            self.st[i] = next;
        }
        self.publish_running();
    }
}

/// Boucle principale (bloquante). S'arrete sur `stop` (Ctrl+C) ou sur le fichier watch.stop.
pub fn run_loop(w: &mut Watcher, ctx: &Ctx, poll_ms: u64, stop: &std::sync::atomic::AtomicBool) {
    run_loop_ex(w, ctx, poll_ms, stop, None)
}

/// Comme `run_loop`, avec une pause optionnelle (la surveillance ne fait rien tant que `paused` est vrai).
pub fn run_loop_ex(w: &mut Watcher, ctx: &Ctx, poll_ms: u64, stop: &std::sync::atomic::AtomicBool, paused: Option<&std::sync::atomic::AtomicBool>) {
    use std::sync::atomic::Ordering;
    let t0 = std::time::Instant::now();
    w.startup(0);
    let is_paused = || paused.map(|p| p.load(Ordering::SeqCst)).unwrap_or(false);
    let mut was_paused = false;
    while !stop.load(Ordering::SeqCst) {
        if stop_file(ctx).exists() {
            let _ = std::fs::remove_file(stop_file(ctx));
            w.log.log("fichier d'arret detecte");
            break;
        }
        // Sommeil fractionne pour reagir vite a l'arret et aux evenements du dossier partage ; rythme de 0,5 s pendant les verifications.
        let eff = if w.has_pending() { 500.min(poll_ms) } else { poll_ms };
        let mut slept = 0;
        while slept < eff && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100.min(eff)));
            slept += 100;
            if w.pump(t0.elapsed().as_millis() as u64) {
                break;
            }
            if is_paused() != was_paused {
                break; // pause / reprise : prise en compte sans attendre la fin du cycle
            }
            if !is_paused() {
                w.tick_schedule_now(); // planification : evaluee au plus une fois par seconde
            }
        }
        let now_paused = is_paused();
        if now_paused != was_paused {
            was_paused = now_paused;
            if now_paused {
                w.log.log("[Surveillance] mise en pause");
            } else {
                w.log.log("[Surveillance] reprise : verification immediate de tous les elements");
                w.resume(t0.elapsed().as_millis() as u64);
            }
        }
        if now_paused {
            continue;
        }
        w.tick(t0.elapsed().as_millis() as u64);
    }
    w.log.log("arret propre de watch");
}
