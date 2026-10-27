//! Fichier d'etat `<dossier de donnees>\status.json` : lu par des outils tiers (ex. une extension Playnite) qui affichent
//! l'etat des jeux. AutoGameSoftwareSync reste seul maitre de la synchro ; ce module ne fait qu'ECRIRE ce fichier.
//! Schema 1 (UTF-8 sans BOM) : voir README. Aucun chemin du dossier partage ni nom d'utilisateur dans `detail`.
use crate::vm::GameStatus;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "status.json";
pub const SCHEMA: u32 = 2;
/// Intervalle minimal entre deux ecritures.
pub const MIN_WRITE_GAP_MS: u64 = 1000;
/// Recalcul de securite, meme sans evenement.
pub const SAFETY_NET_MS: u64 = 30_000;

/// Une fiche de jeu dans status.json (lue par l'extension Playnite).
///
/// Valeurs de `state` : `running` (jeu en cours), `unreachable` (dossier partage injoignable), `no_shared_folder`
/// (aucun dossier partage configure), `paused` (surveillance en pause), `never_synced` (jamais synchronise),
/// `synced` (protege : a jour avec le cloud), `needs_backup` (modifie ici, a sauvegarder), `cloud_newer` (version
/// plus recente dans le cloud, a restaurer), `conflict` (modifie des deux cotes), `error` (voir `detail`).
/// Protege/synchronise = `state == "synced"`.
///
/// Champs additifs (2.0.1) :
/// - `last_sync_utc` : ISO 8601 UTC (`2026-10-25T14:03:09Z`) de la derniere synchro REUSSIE (sauvegarde OU restauration),
///   `null` si jamais.
/// - `origin_device` : NOM AFFICHE de l'appareil qui a produit la version actuellement en place sur ce PC (le nom de ce
///   PC si la version vient d'ici, y compris quand `state` est `needs_backup`, `conflict`, `cloud_newer`...).
/// - `origin_device_color` : couleur `#RRGGBB` de cet appareil (carte « Mes appareils »), `null` si inconnue.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GameEntry {
    pub name: String,
    pub key: String,
    pub exe: String,
    pub install_dir: String,
    pub save_path: String,
    pub state: String,
    pub last_sync_utc: Option<String>,
    pub detail: String,
    // ---- schema 2 (additif) ----
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub original_name: String,
    #[serde(default)]
    pub last_backup_utc: Option<String>,
    #[serde(default)]
    pub last_restore_utc: Option<String>,
    /// backup | restore | conflict_resolved | none
    #[serde(default)]
    pub last_action: String,
    #[serde(default)]
    pub last_action_utc: Option<String>,
    #[serde(default)]
    pub remote_head_machine: Option<String>,
    #[serde(default)]
    pub remote_head_utc: Option<String>,
    #[serde(default)]
    pub local_files: u64,
    #[serde(default)]
    pub local_size_bytes: u64,
    #[serde(default)]
    pub versions_count: u64,
    /// Couleur hexadecimale du PC distant (tete du cloud) si connue.
    #[serde(default)]
    pub machine_color: Option<String>,
    #[serde(default)]
    pub next_check_utc: Option<String>,
    /// Planification du jeu (si elle existe) : mode et prochaine echeance (schema 2, additif).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleEntry>,
    /// Emplacements du jeu (principal d abord) : etat de chacun.
    #[serde(default)]
    pub locations: Vec<LocEntry>,
    /// Nom affiche de l'appareil d'ou vient la version en place sur ce PC (voir le commentaire de la structure).
    #[serde(default)]
    pub origin_device: Option<String>,
    /// Couleur `#RRGGBB` de `origin_device`.
    #[serde(default)]
    pub origin_device_color: Option<String>,
}

/// Un appareil connu (liste `devices` de status.json) : identifiant, nom affiche, couleur `#RRGGBB`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct DeviceEntry {
    pub id: String,
    pub name: String,
    pub color: String,
}

/// Appareil d'origine de la version en place : la tete du cloud seulement si ce PC est a jour avec elle, sinon ce PC.
pub fn origin_device_name(up_to_date: bool, head_machine: Option<&str>, me: &str) -> String {
    match head_machine {
        Some(h) if up_to_date && !h.is_empty() && h != "?" => h.to_string(),
        _ => me.to_string(),
    }
}

/// Un emplacement de sauvegarde dans status.json.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct LocEntry {
    pub id: String,
    pub label: String,
    pub path: String,
    pub is_file: bool,
    pub filter: Option<String>,
    pub enabled: bool,
    pub exists: bool,
    pub files: u64,
    pub size_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StatusDoc {
    pub schema: u32,
    pub app_version: String,
    pub updated_at_utc: String,
    pub watching: bool,
    pub shared_folder_reachable: bool,
    pub games: Vec<GameEntry>,
    // ---- schema 2 (additif) ----
    #[serde(default)]
    pub machine: String,
    #[serde(default)]
    pub machine_color: String,
    #[serde(default)]
    pub last_check_utc: String,
    #[serde(default)]
    pub reactivity_mode: String,
    #[serde(default)]
    pub conflict_mode: String,
    /// Un dossier partage est-il configure ? (faux : les jeux ont l etat `no_shared_folder`)
    #[serde(default = "yes_true")]
    pub shared_folder_configured: bool,
    /// Portee de la detection automatique : all | games | apps.
    #[serde(default = "scope_all")]
    pub detection_scope: String,
    /// Appareils connus (id, nom affiche, couleur), ce PC en premier (schema 2, additif).
    #[serde(default)]
    pub devices: Vec<DeviceEntry>,
}

fn yes_true() -> bool {
    true
}

fn scope_all() -> String {
    "all".to_string()
}

/// ISO 8601 UTC (`2026-10-25T14:03:09Z`).
pub fn iso_utc(ms: u64) -> String {
    let (y, mo, d, h, mi, s, _) = crate::util::civil_from_ms(ms as i64);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, mi, s)
}

/// Etat expose. Priorite : jeu en cours > dossier partage injoignable (ou absent) > surveillance en pause > etat du jeu.
pub fn state_name(st: &GameStatus, reachable: bool, paused: bool) -> &'static str {
    if matches!(st, GameStatus::Running) {
        return "running";
    }
    if !reachable || matches!(st, GameStatus::NoDest) {
        return "unreachable";
    }
    if paused {
        return "paused";
    }
    match st {
        GameStatus::Never => "never_synced",
        GameStatus::UpToDate => "synced",
        GameStatus::LocalChanged => "needs_backup",
        GameStatus::CloudNewer => "cloud_newer",
        GameStatus::Conflict => "conflict",
        GameStatus::Error(_) => "error",
        GameStatus::Running | GameStatus::NoDest => unreachable!(),
    }
}

/// Texte technique court SANS chemin du dossier partage, ni autre chemin, ni nom d'utilisateur.
pub fn sanitize_detail(s: &str, shared: Option<&Path>, user: &str) -> String {
    let mut t = s.replace(['\r', '\n'], " ");
    if let Some(d) = shared {
        let d = d.to_string_lossy();
        if !d.is_empty() {
            t = t.replace(d.as_ref(), "<shared>");
            t = t.replace(&d.replace('\\', "/"), "<shared>");
        }
    }
    if !user.is_empty() {
        // Toutes les occurrences, sans tenir compte de la casse.
        let u = user.to_lowercase();
        loop {
            let low = t.to_lowercase();
            match low.find(&u) {
                Some(i) if low.len() == t.len() && t.is_char_boundary(i) && t.is_char_boundary(i + u.len()) && !t[i..i + u.len()].eq("<user>") => t.replace_range(i..i + u.len(), "<usr>"),
                _ => break,
            }
        }
        t = t.replace("<usr>", "<user>");
    }
    // Tout ce qui ressemble encore a un chemin disparait.
    let t: Vec<String> = t.split_whitespace().map(|w| if w.contains(":\\") || w.contains(":/") || w.starts_with("\\\\") { "<path>".to_string() } else { w.to_string() }).collect();
    let out = t.join(" ");
    out.chars().take(120).collect()
}

pub struct GameIn {
    pub name: String,
    pub key: String,
    pub exe: Option<String>,
    pub install_dir: Option<PathBuf>,
    pub save_path: PathBuf,
    pub status: GameStatus,
    pub last_sync_ms: Option<u64>,
}

pub struct Inputs<'a> {
    pub app_version: &'a str,
    pub now_ms: u64,
    pub watching: bool,
    pub paused: bool,
    pub reachable: bool,
    pub shared: Option<&'a Path>,
    pub user: &'a str,
    pub games: Vec<GameIn>,
}

pub fn build(i: &Inputs) -> StatusDoc {
    let games = i
        .games
        .iter()
        .map(|g| GameEntry {
            name: g.name.clone(),
            key: g.key.clone(),
            exe: g.exe.clone().unwrap_or_default(),
            install_dir: g.install_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            save_path: g.save_path.display().to_string(),
            state: state_name(&g.status, i.reachable, i.paused).to_string(),
            last_sync_utc: g.last_sync_ms.filter(|m| *m > 0).map(iso_utc),
            detail: match &g.status {
                GameStatus::Error(e) => sanitize_detail(e, i.shared, i.user),
                _ => String::new(),
            },
            display_name: g.name.clone(),
            original_name: g.name.clone(),
            last_backup_utc: None,
            last_restore_utc: None,
            last_action: "none".to_string(),
            last_action_utc: None,
            remote_head_machine: None,
            remote_head_utc: None,
            local_files: 0,
            local_size_bytes: 0,
            versions_count: 0,
            machine_color: None,
            next_check_utc: None,
            schedule: None,
            locations: vec![],
            origin_device: None,
            origin_device_color: None,
        })
        .collect();
    StatusDoc { schema: SCHEMA, app_version: i.app_version.to_string(), updated_at_utc: iso_utc(i.now_ms), watching: i.watching, shared_folder_reachable: i.reachable, games, machine: String::new(), machine_color: String::new(), last_check_utc: iso_utc(i.now_ms), reactivity_mode: String::new(), conflict_mode: String::new(), shared_folder_configured: true, detection_scope: "all".to_string(), devices: vec![] }
}

pub fn to_json(doc: &StatusDoc) -> String {
    serde_json::to_string_pretty(doc).unwrap_or_default()
}

/// Contenu compare pour decider d'ecrire : le document SANS `updated_at_utc`.
fn content_key(doc: &StatusDoc) -> String {
    let mut d = doc.clone();
    d.updated_at_utc.clear();
    d.last_check_utc.clear();
    for g in d.games.iter_mut() {
        g.next_check_utc = None;
    }
    to_json(&d)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Wrote {
    /// Contenu identique : rien n'est ecrit.
    Unchanged,
    /// Change, mais moins d'une seconde apres la derniere ecriture : a reessayer.
    Throttled,
    /// Ecrit ; `first` : premiere ecriture reussie (une ligne de journal).
    Written { first: bool },
    /// Echec (dossier non inscriptible...) ; `first` : premier echec (journaliser une seule fois).
    Failed { first: bool, error: String },
}

#[derive(Default)]
pub struct StatusWriter {
    last_key: Option<String>,
    last_write_ms: u64,
    ok_logged: bool,
    err_logged: bool,
}

impl StatusWriter {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    /// Ecrit si le contenu (hors `updated_at_utc`) a change, au plus une fois par seconde. `force` : ecriture finale.
    pub fn write_if_changed(&mut self, dir: &Path, doc: &StatusDoc, now_ms: u64, force: bool) -> Wrote {
        let key = content_key(doc);
        if self.last_key.as_deref() == Some(key.as_str()) {
            return Wrote::Unchanged;
        }
        if !force && self.last_write_ms > 0 && now_ms.saturating_sub(self.last_write_ms) < MIN_WRITE_GAP_MS {
            return Wrote::Throttled;
        }
        // Atomique : fichier temporaire dans le meme dossier puis renommage.
        match crate::util::write_atomic(&Self::path(dir), to_json(doc).as_bytes()) {
            Ok(()) => {
                self.last_key = Some(key);
                self.last_write_ms = now_ms;
                let first = !self.ok_logged;
                self.ok_logged = true;
                Wrote::Written { first }
            }
            Err(e) => {
                self.last_write_ms = now_ms; // ne pas reessayer a chaque tick
                let first = !self.err_logged;
                self.err_logged = true;
                Wrote::Failed { first, error: e.to_string() }
            }
        }
    }
}

/// Donnees supplementaires du schema 2 pour un jeu (calculees par l'appelant, hors `build`).
#[derive(Default, Clone, Debug)]
pub struct GameExtra {
    pub display_name: String,
    pub last_backup_ms: Option<u64>,
    pub last_restore_ms: Option<u64>,
    /// backup | restore | conflict_resolved
    pub last_action: Option<(String, u64)>,
    pub remote_machine: Option<String>,
    pub remote_ms: Option<u64>,
    pub local_files: u64,
    pub local_size: u64,
    pub versions: u64,
    pub machine_color: Option<String>,
    pub next_check_ms: Option<u64>,
    pub schedule: Option<ScheduleEntry>,
    pub locations: Vec<LocEntry>,
    pub origin_device: Option<String>,
    pub origin_device_color: Option<String>,
}

/// Donnees globales du schema 2.
#[derive(Default, Clone, Debug)]
pub struct GlobalExtra {
    pub machine: String,
    pub machine_color: String,
    pub reactivity_mode: String,
    pub conflict_mode: String,
    /// Aucun dossier partage configure.
    pub no_shared_folder: bool,
    pub detection_scope: String,
    pub devices: Vec<DeviceEntry>,
}

/// Complete un document `build` avec les champs du schema 2 (`extras[i]` = jeu i). `name` devient le nom AFFICHE ;
/// `original_name` garde le nom d'origine (et `key` ne change jamais).
pub fn enrich(doc: &mut StatusDoc, extras: &[GameExtra], g: &GlobalExtra) {
    for (e, x) in doc.games.iter_mut().zip(extras) {
        e.original_name = e.name.clone();
        if !x.display_name.is_empty() {
            e.display_name = x.display_name.clone();
            e.name = x.display_name.clone();
        }
        e.last_backup_utc = x.last_backup_ms.filter(|m| *m > 0).map(iso_utc);
        e.last_restore_utc = x.last_restore_ms.filter(|m| *m > 0).map(iso_utc);
        match &x.last_action {
            Some((k, ms)) => {
                e.last_action = k.clone();
                e.last_action_utc = Some(iso_utc(*ms));
            }
            None => e.last_action = "none".to_string(),
        }
        e.remote_head_machine = x.remote_machine.clone();
        e.remote_head_utc = x.remote_ms.filter(|m| *m > 0).map(iso_utc);
        e.local_files = x.local_files;
        e.local_size_bytes = x.local_size;
        e.versions_count = x.versions;
        e.machine_color = x.machine_color.clone();
        e.next_check_utc = x.next_check_ms.map(iso_utc);
        e.schedule = x.schedule.clone();
        e.locations = x.locations.clone();
        e.origin_device = x.origin_device.clone();
        e.origin_device_color = x.origin_device_color.clone();
    }
    doc.machine = g.machine.clone();
    doc.machine_color = g.machine_color.clone();
    doc.reactivity_mode = g.reactivity_mode.clone();
    doc.conflict_mode = g.conflict_mode.clone();
    doc.devices = g.devices.clone();
    if !g.detection_scope.is_empty() {
        doc.detection_scope = g.detection_scope.clone();
    }
    if g.no_shared_folder {
        doc.shared_folder_configured = false;
        doc.shared_folder_reachable = false;
        for e in doc.games.iter_mut().filter(|e| e.state != "running") {
            e.state = "no_shared_folder".to_string();
        }
    }
}

/// Planification d'un jeu dans status.json : `mode` = `fixed` | `interval`, prochaine echeance en UTC.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct ScheduleEntry {
    pub mode: String,
    pub next_run_utc: Option<String>,
}
