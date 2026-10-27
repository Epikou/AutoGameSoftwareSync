//! Planification d'un jeu : heures fixes chaque jour (HH:MM:SS) ou intervalle (HH:MM:SS). Calcul PUR (horloge et fuseau injectes) :
//! prochaine echeance, rattrapage apres une extinction, changement d'heure (jamais de doublon ni de saut absurde).
use crate::timefmt::TzProvider;
use serde::{Deserialize, Serialize};

pub const MIN_INTERVAL_SECS: u32 = 10;
/// Sous ce seuil, l'interface avertit d'une charge elevee.
pub const WARN_INTERVAL_SECS: u32 = 60;
pub const MAX_INTERVAL_SECS: u32 = 86_400;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SchedMode {
    /// Une ou plusieurs heures fixes chaque jour.
    #[default]
    Fixed,
    /// Chronometre interne.
    Interval,
}

/// Planification d'un jeu (`schedule` optionnel dans config.json).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Schedule {
    #[serde(default)]
    pub mode: SchedMode,
    /// Heures fixes, en secondes depuis minuit (heure locale).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub times_secs: Vec<u32>,
    #[serde(default)]
    pub interval_secs: u32,
    /// Derniere execution planifiee (secondes UTC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_utc: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedError {
    NoTime,
    BadTime,
    BadInterval,
}

impl Schedule {
    pub fn fixed(times: Vec<u32>) -> Schedule {
        Schedule { mode: SchedMode::Fixed, times_secs: times, ..Default::default() }
    }
    pub fn interval(secs: u32) -> Schedule {
        Schedule { mode: SchedMode::Interval, interval_secs: secs, ..Default::default() }
    }
    pub fn validate(&self) -> Result<(), SchedError> {
        match self.mode {
            SchedMode::Fixed => {
                if self.times_secs.is_empty() {
                    return Err(SchedError::NoTime);
                }
                if self.times_secs.iter().any(|t| *t >= 86_400) {
                    return Err(SchedError::BadTime);
                }
                Ok(())
            }
            SchedMode::Interval => {
                if (MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&self.interval_secs) {
                    Ok(())
                } else {
                    Err(SchedError::BadInterval)
                }
            }
        }
    }
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }
    /// Intervalle court : avertir (charge).
    pub fn is_short(&self) -> bool {
        self.mode == SchedMode::Interval && self.interval_secs < WARN_INTERVAL_SECS
    }
    /// Heures fixes triees et sans doublon.
    pub fn normalized(&self) -> Schedule {
        let mut s = self.clone();
        s.times_secs.sort_unstable();
        s.times_secs.dedup();
        s
    }
}

/// Le formulaire d'ajout peut-il etre valide ? Sans exe (aucune surveillance de processus), une planification valide est obligatoire.
pub fn form_ok(exe: &str, schedule: Option<&Schedule>) -> bool {
    !exe.trim().is_empty() || schedule.map(|s| s.is_valid()).unwrap_or(false)
}

/// `HH:MM:SS` (secondes depuis minuit ou duree).
pub fn hms(secs: u32) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}

/// Lit `HH:MM:SS` ou `HH:MM` ; None si invalide.
pub fn parse_hms(s: &str) -> Option<u32> {
    let p: Vec<&str> = s.trim().split(':').collect();
    if !(2..=3).contains(&p.len()) {
        return None;
    }
    let n: Vec<u32> = p.iter().map(|x| x.trim().parse::<u32>().ok()).collect::<Option<Vec<_>>>()?;
    let (h, m, sec) = (n[0], n[1], n.get(2).copied().unwrap_or(0));
    if m > 59 || sec > 59 {
        return None;
    }
    Some(h * 3600 + m * 60 + sec)
}

fn offset(tz: &dyn TzProvider, utc_s: i64) -> i64 {
    tz.utc_offset_secs((utc_s.max(0) as u64) * 1000) as i64
}

/// Instant UTC d'une heure LOCALE (secondes locales depuis l'epoque). Heure inexistante (saut d'ete) : decalee d'au plus 1 h ;
/// heure doublee (retour d'hiver) : une seule occurrence est retenue.
fn local_to_utc(tz: &dyn TzProvider, local_s: i64) -> i64 {
    let o1 = offset(tz, local_s);
    let u = local_s - o1;
    let o2 = offset(tz, u);
    if o2 == o1 { u } else { local_s - o2 }
}

/// Instants UTC de toutes les heures fixes dans l'intervalle ouvert (from, to], tries, sans doublon.
pub fn fixed_slots(times: &[u32], from: i64, to: i64, tz: &dyn TzProvider) -> Vec<i64> {
    let day = |utc: i64| (utc + offset(tz, utc)).div_euclid(86_400);
    let mut out = Vec::new();
    for d in (day(from) - 1)..=(day(to) + 1) {
        for t in times {
            let u = local_to_utc(tz, d * 86_400 + *t as i64);
            if u > from && u <= to {
                out.push(u);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Resultat d'evaluation a un instant donne.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Eval {
    /// Une echeance est arrivee (a executer maintenant) : description pour le journal.
    pub due: Option<Due>,
    /// Prochaine echeance (UTC, secondes) apres l'execution eventuelle.
    pub next_utc: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    /// `heure fixe HH:MM:SS` ou `intervalle HH:MM:SS`.
    pub label: String,
    /// Echeance manquee (PC eteint, application fermee) rattrapee maintenant.
    pub catch_up: bool,
}

/// Evalue la planification. `anchor` = derniere execution (ou instant de depart), `now` = maintenant (secondes UTC).
/// Une horloge qui recule est ramenee a `now` (aucune execution fantome). Au plus UNE execution par appel (rattrapage unique).
pub fn evaluate(s: &Schedule, anchor: i64, now: i64, tz: &dyn TzProvider) -> Option<Eval> {
    s.validate().ok()?;
    let anchor = anchor.min(now);
    match s.mode {
        SchedMode::Interval => {
            let iv = s.interval_secs as i64;
            let due_at = anchor + iv;
            if due_at <= now {
                Some(Eval { due: Some(Due { label: format!("intervalle {}", hms(s.interval_secs)), catch_up: now - due_at > iv.max(60) }), next_utc: now + iv })
            } else {
                Some(Eval { due: None, next_utc: due_at })
            }
        }
        SchedMode::Fixed => {
            let times = s.normalized().times_secs;
            let lo = anchor.max(now - 2 * 86_400);
            let missed = fixed_slots(&times, lo, now, tz);
            let next = fixed_slots(&times, now, now + 3 * 86_400, tz).first().copied().unwrap_or(now + 86_400);
            let due = missed.last().map(|slot| {
                let tod = ((slot + offset(tz, *slot)).rem_euclid(86_400)) as u32;
                Due { label: format!("heure fixe {}", hms(tod)), catch_up: now - slot > 90 }
            });
            Some(Eval { due, next_utc: next })
        }
    }
}

/// Prochaine echeance seule (pour le journal, le statut et l'interface).
pub fn next_run_utc(s: &Schedule, anchor: i64, now: i64, tz: &dyn TzProvider) -> Option<i64> {
    let e = evaluate(s, anchor, now, tz)?;
    Some(if e.due.is_some() { now } else { e.next_utc })
}

/// Decision pour une synchro planifiee : la RESTAURATION est reportee si un fichier de l'emplacement est verrouille ;
/// la sauvegarde (le local a change) est toujours faite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    Run,
    PostponeRestore,
}

pub fn guard(would_restore: bool, locked: bool) -> Guard {
    if would_restore && locked { Guard::PostponeRestore } else { Guard::Run }
}

/// Un fichier est-il verrouille (impossible de l'ouvrir en acces exclusif) ?
#[cfg(not(windows))]
pub fn file_locked(p: &std::path::Path) -> bool {
    use std::os::unix::io::AsRawFd;
    // Verrou consultatif (flock) : pris par Wine / Proton pour les fichiers ouverts en exclusif. Lecture seule : aucun risque d'ecriture.
    match std::fs::File::open(p) {
        Ok(f) => {
            let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if r == 0 {
                unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_UN) };
                false
            } else {
                std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK)
            }
        }
        Err(e) => e.kind() != std::io::ErrorKind::NotFound && e.kind() != std::io::ErrorKind::PermissionDenied,
    }
}

#[cfg(windows)]
pub fn file_locked(p: &std::path::Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(p) {
        Ok(_) => false,
        // Fichier absent : rien a verrouiller. Acces refuse / violation de partage : verrouille (prudence).
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

/// Au moins un fichier de l'un des emplacements ACTIVES du jeu est-il verrouille ? (parcours borne)
pub fn any_locked(g: &crate::config::Game) -> bool {
    let mut n = 0usize;
    for l in g.enabled_locations() {
        if l.is_file {
            if file_locked(&l.path) {
                return true;
            }
            continue;
        }
        for e in walkdir::WalkDir::new(&l.path).follow_links(false).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
            n += 1;
            if n > 20_000 {
                return false;
            }
            if file_locked(e.path()) {
                return true;
            }
        }
    }
    false
}

/// Secondes UTC actuelles.
pub fn now_utc_s() -> i64 {
    (crate::util::now_ms() / 1000) as i64
}

/// Enregistre `last_run_utc` dans config.json (lecture-modification-ecriture atomique, une seule valeur).
pub fn record_run(ctx: &crate::Ctx, game: &str, utc: i64) {
    if let Ok(mut cfg) = crate::config::Config::load(ctx) {
        if let Some(g) = cfg.games.iter_mut().find(|g| g.name == game) {
            if let Some(s) = g.schedule.as_mut() {
                s.last_run_utc = Some(utc);
                let _ = cfg.save(ctx);
            }
        }
    }
}

/// Texte compact d'une planification (liste des jeux, journal).
pub fn summary(s: &Schedule) -> String {
    match s.mode {
        SchedMode::Fixed => s.normalized().times_secs.iter().map(|t| hms(*t)).collect::<Vec<_>>().join(", "),
        SchedMode::Interval => hms(s.interval_secs),
    }
}

/// Planification issue de la ligne de commande : `--schedule-interval HH:MM:SS` ou `--schedule-at HH:MM:SS` (repetable).
pub fn from_cli(interval: Option<&str>, at: &[String]) -> Result<Option<Schedule>, String> {
    match (interval, at.is_empty()) {
        (None, true) => Ok(None),
        (Some(_), false) => Err("--schedule-interval et --schedule-at s'excluent".to_string()),
        (Some(i), true) => {
            let secs = parse_hms(i).ok_or_else(|| format!("intervalle invalide : {} (attendu HH:MM:SS)", i))?;
            let s = Schedule::interval(secs);
            s.validate().map_err(|_| format!("intervalle hors limites : {} (de 00:00:10 a 24:00:00)", i))?;
            Ok(Some(s))
        }
        (None, false) => {
            let mut v = Vec::new();
            for a in at {
                v.push(parse_hms(a).filter(|t| *t < 86_400).ok_or_else(|| format!("heure invalide : {} (attendu HH:MM:SS)", a))?);
            }
            Ok(Some(Schedule::fixed(v).normalized()))
        }
    }
}
