//! Donnees de l'application Android : preferences propres a la console (`android.json`), reglages et liste des elements
//! au MEME format que le PC (`settings.json`, `config.json` de la bibliotheque `agss`), contexte du moteur, faits d'un
//! element pour les tuiles, dossiers candidats pour le coffre. Aucune interface ici : tout est testable sur PC.
use agss::config::{Config, Game};
use agss::process::ProcessChecker;
use agss::settings::Settings;
use agss::{backup, cloud, emulators, util, vaults, Ctx, Res};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Preferences propres a la console.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Prefs {
    #[serde(default)]
    pub setup_done: bool,
    /// Nom de cet appareil (ecrit dans les versions du coffre et les marques de position).
    #[serde(default)]
    pub device_name: String,
    /// Dossier du coffre (deja prepare : marqueur ecrit).
    #[serde(default)]
    pub vault: Option<PathBuf>,
    /// La langue a ete choisie (sinon : langue du systeme a chaque demarrage).
    #[serde(default)]
    pub lang_chosen: bool,
    /// Derniere synchro complete (ms UTC).
    #[serde(default)]
    pub last_sync_ms: u64,
}

const PREFS_FILE: &str = "android.json";

impl Prefs {
    pub fn load(dir: &Path) -> Prefs {
        std::fs::read_to_string(dir.join(PREFS_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self, dir: &Path) -> Res<()> {
        util::write_atomic(&dir.join(PREFS_FILE), serde_json::to_string_pretty(self)?.as_bytes())
    }
}

/// La console ne sait pas si un emulateur tourne : aucun programme n'est jamais considere « en cours ».
pub struct NeverRunning;
impl ProcessChecker for NeverRunning {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(false)
    }
}

/// Nom d'appareil propre pour le coffre (noms de dossiers) : caracteres interdits remplaces, longueur bornee.
pub fn clean_device_name(s: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_control()).collect();
    let t = util::sanitize(t.trim());
    let t: String = t.chars().take(32).collect();
    t.trim().to_string()
}

/// Nom par defaut : modele de l'appareil (« Retroid Pocket 4 Pro »), sinon « Console ».
pub fn default_device_name(model: &str) -> String {
    let m = clean_device_name(model);
    if m.is_empty() || m == "_" {
        "Console".into()
    } else {
        m
    }
}

/// Tout ce que l'application garde en memoire et sur disque.
pub struct Store {
    pub data_dir: PathBuf,
    pub prefs: Prefs,
    pub settings: Settings,
    pub config: Config,
}

impl Store {
    pub fn load(data_dir: PathBuf) -> Store {
        let _ = std::fs::create_dir_all(&data_dir);
        let prefs = Prefs::load(&data_dir);
        let settings = Settings::load_from(&data_dir).unwrap_or_default();
        let ctx = Ctx::new(data_dir.clone());
        let config = Config::load(&ctx).unwrap_or_default();
        Store { data_dir, prefs, settings, config }
    }

    /// Contexte du moteur pour cet appareil (nom choisi, aucun programme surveille).
    pub fn ctx(&self) -> Ctx {
        make_ctx(&self.data_dir, &self.prefs.device_name)
    }

    pub fn save_prefs(&self) -> Res<()> {
        self.prefs.save(&self.data_dir)
    }
    pub fn save_settings(&self) -> Res<()> {
        self.settings.save(&self.ctx())
    }
    pub fn save_config(&mut self) -> Res<()> {
        self.config.dest = self.prefs.vault.clone();
        self.config.save(&self.ctx())
    }

    /// Ajoute des elements (noms deja presents ignores). Retourne le nombre ajoute.
    pub fn add_games(&mut self, games: Vec<Game>) -> usize {
        let mut n = 0;
        for g in games {
            if self.config.games.iter().any(|x| x.name.eq_ignore_ascii_case(&g.name)) {
                continue;
            }
            self.config.games.push(g);
            n += 1;
        }
        n
    }

    pub fn remove_game(&mut self, name: &str) -> bool {
        let before = self.config.games.len();
        self.config.games.retain(|g| !g.name.eq_ignore_ascii_case(name));
        self.config.games.len() != before
    }

    pub fn game(&self, name: &str) -> Option<&Game> {
        self.config.games.iter().find(|g| g.name.eq_ignore_ascii_case(name))
    }
}

pub fn make_ctx(data_dir: &Path, device: &str) -> Ctx {
    let mut ctx = Ctx::new(data_dir.to_path_buf());
    if !device.trim().is_empty() {
        ctx.machine = device.to_string();
    }
    ctx.procs = Box::new(NeverRunning);
    ctx
}

// ------------------------------------------------------------------ etat d'un element

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemState {
    Never,
    Safe,
    Copied,
    Received(String),
    /// Version plus recente d'un autre appareil dans le coffre, PAS remise (jamais automatique sur la console) : a valider.
    Available(String),
    Conflict,
    Error(String),
    Missing,
    Empty,
    /// L'autorisation « Acces a tous les fichiers » manque : rien n'est lu ni ecrit.
    NoAccess,
    Syncing,
    Pending,
}

impl ItemState {
    /// Demande un regard de l'utilisateur (compteur de l'accueil). Un dossier devenu vide alors que le coffre a deja des
    /// versions (carte SD retiree, emulateur reinstalle) compte aussi.
    pub fn needs_attention(&self, versions: usize) -> bool {
        match self {
            ItemState::Conflict | ItemState::Error(_) | ItemState::Missing | ItemState::Available(_) | ItemState::NoAccess => true,
            ItemState::Empty => versions > 0,
            _ => false,
        }
    }
}

/// Ce que l'application a le droit de faire au demarrage / au retour au premier plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Premier lancement pas termine : rien.
    NotSetUp,
    /// Autorisation retiree : aucune synchro, aucune remise, etat « Acces aux fichiers manquant ».
    NoPermission,
    Ok,
}

/// Decision pure (testable) : synchro autorisee ? `storage_readable` = `sys::storage_readable()`.
pub fn sync_gate(setup_done: bool, storage_readable: bool) -> Gate {
    if !setup_done {
        Gate::NotSetUp
    } else if !storage_readable {
        Gate::NoPermission
    } else {
        Gate::Ok
    }
}

/// Nettoyage au demarrage (un seul processus sur Android, aucun autre ecrivain) : dossiers de travail locaux orphelins et
/// version de CET appareil restee sans `meta.json` dans le coffre apres un arret brutal. Retourne (dossiers tmp retires,
/// dossier de version retire).
pub fn startup_cleanup(data_dir: &Path, device: &str) -> (usize, Option<PathBuf>) {
    let ctx = make_ctx(data_dir, device);
    let n = backup::cleanup_local_tmp(&ctx, std::time::Duration::ZERO);
    let claimed = backup::cleanup_claimed(&ctx, std::time::Duration::ZERO);
    (n, claimed)
}

// ------------------------------------------------------------------ remonter le temps

/// Ligne de « Remonter le temps » (version du coffre ou copie de securite locale).
#[derive(Clone, Debug, PartialEq)]
pub struct TtRow {
    pub key: TtKey,
    pub ms: u64,
    pub device: String,
    pub files: usize,
    pub bytes: u64,
    pub current: bool,
    pub active: bool,
    pub conflict_branch: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TtKey {
    Version(String),
    Safety(PathBuf),
}

/// Lignes de « Remonter le temps » d'un element (lecture du coffre et des copies de securite : fil de travail).
pub fn history_rows(ctx: &Ctx, g: &Game, vault: Option<&Path>) -> Vec<TtRow> {
    let mut rows: Vec<TtRow> = Vec::new();
    if let Some(v) = vault.filter(|v| v.is_dir()) {
        let h = backup::history(ctx, g, v);
        for r in &h.rows {
            let conflict_branch = r.note.as_deref().map(|n| n.contains("conflit")).unwrap_or(false) && !r.active;
            rows.push(TtRow { key: TtKey::Version(r.name.clone()), ms: r.saved_at_ms, device: r.machine.clone(), files: r.files, bytes: r.bytes, current: r.is_local, active: r.active, conflict_branch });
        }
    }
    for s in backup::safety_copies(ctx, g) {
        rows.push(TtRow { key: TtKey::Safety(s.dir.clone()), ms: s.ms, device: ctx.machine.clone(), files: s.files, bytes: s.bytes, current: false, active: false, conflict_branch: false });
    }
    rows.sort_by(|a, b| b.ms.cmp(&a.ms));
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Emulator,
    Folder,
}

/// Faits d'un element pour l'accueil et la fiche.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemInfo {
    pub name: String,
    pub display: String,
    pub kind: Kind,
    pub path: PathBuf,
    pub exists: bool,
    pub state: ItemState,
    /// Derniere version du coffre (ms UTC) et appareil qui l'a faite.
    pub last_ms: Option<u64>,
    pub last_device: Option<String>,
    pub versions: usize,
    pub conflict: bool,
}

/// Categorie d'un element : profil d'emulateur connu (nom du profil PC) ou dossier.
pub fn kind_of(g: &Game) -> Kind {
    let known = emulators::EMULATORS.iter().filter_map(emulators::pc_profile).any(|p| p.name.eq_ignore_ascii_case(&g.name));
    if known {
        Kind::Emulator
    } else {
        Kind::Folder
    }
}

/// Faits d'un element d'apres le coffre (lecture seule ; `prev` = etat issu de la derniere synchro, garde s'il dit plus).
pub fn info_of(ctx: &Ctx, g: &Game, vault: Option<&Path>, prev: Option<&ItemState>) -> ItemInfo {
    let exists = agss::store::GameTarget::of(g).exists();
    let mut info = ItemInfo { name: g.name.clone(), display: g.shown().to_string(), kind: kind_of(g), path: g.path.clone(), exists, state: ItemState::Never, last_ms: None, last_device: None, versions: 0, conflict: false };
    if let Some(v) = vault.filter(|v| v.is_dir()) {
        let h = backup::history(ctx, g, v);
        info.versions = h.rows.len();
        info.conflict = !h.conflict_heads.is_empty();
        if let Some(r) = h.rows.first() {
            info.last_ms = Some(r.saved_at_ms);
            info.last_device = Some(r.machine.clone());
        }
        let is_local_head = h.rows.iter().any(|r| r.active && r.is_local);
        info.state = match prev {
            Some(ItemState::Error(e)) => ItemState::Error(e.clone()),
            Some(ItemState::Received(d)) => ItemState::Received(d.clone()),
            Some(ItemState::Available(d)) => ItemState::Available(d.clone()),
            Some(ItemState::Copied) => ItemState::Copied,
            Some(ItemState::Empty) => ItemState::Empty,
            _ if !exists => ItemState::Missing,
            _ if info.conflict => ItemState::Conflict,
            _ if is_local_head => ItemState::Safe,
            _ if info.versions == 0 => ItemState::Never,
            _ => ItemState::Pending,
        };
    } else if !exists {
        info.state = ItemState::Missing;
    }
    info
}

// ------------------------------------------------------------------ synchro d'un element (meme logique que le PC)

/// Resultat d'une synchro d'un element.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncResult {
    UpToDate,
    Backed,
    Restored { from: String, files: usize },
    /// Version plus recente d'un autre appareil, non remise (demande une confirmation).
    Available { from: String },
    Conflict,
    ConflictResolved,
    Missing,
    Empty,
    Error(String),
}

impl SyncResult {
    pub fn state(&self) -> ItemState {
        match self {
            SyncResult::UpToDate => ItemState::Safe,
            SyncResult::Backed => ItemState::Copied,
            SyncResult::Restored { from, .. } => ItemState::Received(from.clone()),
            SyncResult::Available { from } => ItemState::Available(from.clone()),
            SyncResult::Conflict => ItemState::Conflict,
            SyncResult::ConflictResolved => ItemState::Safe,
            SyncResult::Missing => ItemState::Missing,
            SyncResult::Empty => ItemState::Empty,
            SyncResult::Error(e) => ItemState::Error(e.clone()),
        }
    }
}

/// Synchronise un element ferme, comme le PC : point de synchro, copie, restauration (copie de securite avant), conflit
/// (les deux versions gardees). Le coffre doit exister.
///
/// `auto_restore` : la console ne sait pas si un emulateur tourne encore en arriere-plan et son horloge peut etre fausse ;
/// quand il est faux (toute synchro automatique), une version plus recente du coffre n'est PAS remise (`Available`) et un
/// conflit n'est jamais resolu tout seul (meme avec `auto_cloud`) : l'utilisateur valide dans la fiche. Il n'est vrai que
/// pour une remise explicitement confirmee.
pub fn sync_item(ctx: &Ctx, g: &Game, vault: &Path, auto_cloud: bool, auto_restore: bool, log: &dyn Fn(&str)) -> SyncResult {
    if !vault.is_dir() {
        return SyncResult::Error("vault unreachable".into());
    }
    if !agss::store::GameTarget::of(g).exists() {
        return SyncResult::Missing;
    }
    let plan = match backup::plan(ctx, g, vault) {
        Ok(p) => p,
        Err(e) => return SyncResult::Error(e.to_string()),
    };
    log(&format!("[{}] plan : {:?}", g.name, plan));
    let r = match plan {
        backup::Plan::Nothing => return SyncResult::Empty,
        backup::Plan::InSync => backup::backup_in_sync(ctx, g, vault).map(|_| SyncResult::UpToDate),
        backup::Plan::Backup => backup::backup(ctx, g, vault).map(|o| match o {
            backup::BackupOutcome::UpToDate => SyncResult::UpToDate,
            _ => SyncResult::Backed,
        }),
        backup::Plan::Restore => {
            let from = backup::active_meta(vault, g).map(|m| m.machine).unwrap_or_else(|| "?".into());
            if !auto_restore {
                log(&format!("[{}] version plus recente de {} disponible : non remise (a valider dans la fiche)", g.name, from));
                return SyncResult::Available { from };
            }
            backup::restore(ctx, g, vault).map(|o| {
                log(&format!("[{}] version plus recente de {} remise ({} fichiers, copie de securite : {:?})", g.name, from, o.files_restored, o.safety_dir));
                SyncResult::Restored { from, files: o.files_restored }
            })
        }
        backup::Plan::Conflict if auto_cloud && auto_restore => match backup::resolve_conflict(ctx, g, vault, false) {
            Ok(_) => Ok(SyncResult::ConflictResolved),
            Err(e) if e.to_string().contains("REFUS") => Ok(SyncResult::Conflict),
            Err(e) => Err(e),
        },
        backup::Plan::Conflict => backup::backup(ctx, g, vault).map(|_| SyncResult::Conflict),
    };
    match r {
        Ok(s) => {
            // Marque de position : cet appareil a maintenant la version active (si son contenu y correspond).
            if let (Some(m), Some(local_hash)) = (backup::active_meta(vault, g), local_hash(ctx, g)) {
                if m.global_hash == local_hash {
                    let key = cloud::dest_key(g);
                    if let Some(v) = backup::history(ctx, g, vault).rows.iter().find(|r| r.active) {
                        agss::home::note_position(vault, &ctx.machine, &key, &v.id, util::now_ms());
                    }
                }
            }
            s
        }
        Err(e) => {
            log(&format!("[{}] ERREUR : {}", g.name, e));
            SyncResult::Error(e.to_string())
        }
    }
}

fn local_hash(ctx: &Ctx, g: &Game) -> Option<String> {
    let (files, _) = agss::store::GameTarget::of(g).scan(ctx).ok()?;
    if files.is_empty() {
        return None;
    }
    Some(agss::store::global_hash(&files))
}

// ------------------------------------------------------------------ dossiers candidats pour le coffre

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandKind {
    /// Dossier synchronise par Syncthing (marqueur `.stfolder`).
    Syncthing,
    /// Coffre deja utilise (marqueur `.agss-vault` ou elements), avec les appareils vus.
    Existing(Vec<String>),
    /// Dossier qui porte le nom du coffre.
    Named,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub kind: CandKind,
}

const CAND_MAX_DEPTH: usize = 3;
pub const CAND_MAX_DIRS: usize = 600;
/// Dossiers jamais parcourus (donnees d'applis, medias, systeme).
const CAND_SKIP: &[&str] = &["Android", "DCIM", "Pictures", "Movies", "Music", "Podcasts", "Ringtones", "Alarms", "Notifications", "Audiobooks", "Recordings", "Fonts", "LOST.DIR", "MIUI", "Huawei", "Samsung", "tencent", "WhatsApp", "obb", "data", "RetroArch", "PSP", "ROMs", "roms", "Roms", "Emulation"];
/// Dossiers souvent enormes : parcourus, mais pas au-dela du niveau 2.
const CAND_SHALLOW: &[&str] = &["Download", "Downloads", "Documents"];

/// Coffres possibles dans les stockages : coffres existants d'abord, puis dossiers Syncthing, puis dossiers nommes.
/// Parcours en LARGEUR d'abord (tous les dossiers de niveau 1 de toutes les racines, puis niveau 2…) avec un budget
/// de dossiers lus : un gros `Download` ne peut pas cacher un `Sync` voisin.
pub fn vault_candidates(roots: &[PathBuf]) -> Vec<Candidate> {
    vault_candidates_budget(roots, CAND_MAX_DIRS)
}

pub fn vault_candidates_budget(roots: &[PathBuf], mut budget: usize) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    // (dossier, profondeur, ne pas descendre plus bas : enfant direct de Download / Documents)
    let mut queue: std::collections::VecDeque<(PathBuf, usize, bool)> = roots.iter().map(|r| (r.clone(), 0, false)).collect();
    while let Some((dir, depth, leaf)) = queue.pop_front() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let subs = visit(&dir, depth, &mut out);
        if leaf {
            continue;
        }
        let shallow = depth == 1 && CAND_SHALLOW.iter().any(|k| dir.file_name().map(|n| k.eq_ignore_ascii_case(&n.to_string_lossy())).unwrap_or(false));
        for s in subs {
            queue.push_back((s, depth + 1, shallow));
        }
    }
    out.sort_by_key(|c| match &c.kind {
        CandKind::Existing(_) => 0,
        CandKind::Syncthing => 1,
        CandKind::Named => 2,
    });
    out.dedup_by(|a, b| a.path == b.path);
    out
}

/// Lit un dossier, note s'il est candidat, retourne les sous-dossiers a parcourir ensuite.
fn visit(dir: &Path, depth: usize, out: &mut Vec<Candidate>) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut subs: Vec<PathBuf> = Vec::new();
    let mut is_st = false;
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path();
        if name == ".stfolder" {
            is_st = true;
            continue;
        }
        if name.starts_with('.') || !p.is_dir() {
            continue;
        }
        subs.push(p);
    }
    if depth > 0 {
        if vaults::is_vault(dir) {
            let mut devices = cloud::machines_seen(dir);
            devices.sort();
            out.push(Candidate { path: dir.to_path_buf(), kind: CandKind::Existing(devices) });
            return Vec::new(); // on ne descend pas dans un coffre
        }
        if is_st {
            out.push(Candidate { path: dir.to_path_buf(), kind: CandKind::Syncthing });
        } else if dir.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(vaults::VAULT_DIR)).unwrap_or(false) {
            out.push(Candidate { path: dir.to_path_buf(), kind: CandKind::Named });
        }
    }
    if depth >= CAND_MAX_DEPTH {
        return Vec::new();
    }
    let name_of = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    subs.sort();
    subs.retain(|s| !(depth == 0 && CAND_SKIP.iter().any(|k| k.eq_ignore_ascii_case(&name_of(s)))));
    subs
}

/// Sous-dossiers lisibles d'un dossier (navigateur de dossiers), tries, sans les caches.
pub fn list_subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true)).collect(),
        Err(_) => Vec::new(),
    };
    v.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    v
}

// ------------------------------------------------------------------ detection pour le premier lancement

/// Proposition d'element au premier lancement / a l'ajout.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub title: String,
    pub subtitle: String,
    /// Emplacements (libelle, coche) ; vide pour un dossier perso.
    pub locations: Vec<(String, bool)>,
    pub already: bool,
    pub game_builder: ProposalSource,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProposalSource {
    Emu { id: &'static str, root: PathBuf },
    Folder { name: String, path: PathBuf },
}

/// Emulateurs trouves + dossiers perso existants ; ce qui est deja protege est signale (et non recoche).
pub fn proposals(roots: &[PathBuf], existing: &[Game]) -> Vec<Proposal> {
    let has = |name: &str| existing.iter().any(|g| g.name.eq_ignore_ascii_case(name));
    let mut out: Vec<Proposal> = Vec::new();
    for f in emulators::find_on_device(roots) {
        let Some(p) = emulators::android_profile(f.emu) else { continue };
        let locations: Vec<(String, bool)> = p.locations.iter().zip(f.present.iter()).map(|(l, present)| (l.label.clone(), l.default_enabled && *present)).collect();
        out.push(Proposal { title: f.emu.display.to_string(), subtitle: f.root.to_string_lossy().to_string(), locations, already: has(&p.name), game_builder: ProposalSource::Emu { id: f.emu.id, root: f.root.clone() } });
    }
    for (name, rel) in emulators::PERSONAL_FOLDERS {
        for r in roots {
            let p = r.join(rel);
            if p.is_dir() {
                out.push(Proposal { title: name.to_string(), subtitle: p.to_string_lossy().to_string(), locations: Vec::new(), already: has(name), game_builder: ProposalSource::Folder { name: name.to_string(), path: p } });
                break;
            }
        }
    }
    out
}

/// Element correspondant a une proposition et aux cases cochees (None : rien de coche ou profil manquant).
pub fn build_game(p: &Proposal, ticked: &[bool]) -> Option<Game> {
    match &p.game_builder {
        ProposalSource::Emu { id, root } => {
            if !ticked.iter().any(|t| *t) {
                return None;
            }
            emulators::android_game(emulators::by_id(id)?, root, ticked).ok()
        }
        ProposalSource::Folder { name, path } => {
            if !ticked.first().copied().unwrap_or(false) {
                return None;
            }
            Some(emulators::personal_game(name, path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_et_nom_d_appareil() {
        let t = tempfile::TempDir::new().unwrap();
        let mut p = Prefs::load(t.path());
        assert!(!p.setup_done && p.device_name.is_empty());
        p.device_name = "Retroid".into();
        p.vault = Some(t.path().join("v"));
        p.save(t.path()).unwrap();
        assert_eq!(Prefs::load(t.path()), p);
        assert_eq!(default_device_name(""), "Console");
        assert_eq!(default_device_name("Retroid Pocket 4 Pro"), "Retroid Pocket 4 Pro");
        assert_eq!(clean_device_name("  a/b:c*  "), "a_b_c_");
        assert!(clean_device_name(&"x".repeat(100)).len() <= 32);
    }

    #[test]
    fn store_memes_fichiers_que_le_pc_et_ajout_sans_doublon() {
        let t = tempfile::TempDir::new().unwrap();
        let mut s = Store::load(t.path().join("data"));
        s.prefs.device_name = "Odin".into();
        s.prefs.vault = Some(t.path().join("vault"));
        let g = emulators::personal_game("Documents", &t.path().join("Documents"));
        assert_eq!(s.add_games(vec![g.clone(), g.clone()]), 1);
        assert_eq!(s.add_games(vec![g.clone()]), 0);
        s.save_config().unwrap();
        s.save_prefs().unwrap();
        let s2 = Store::load(t.path().join("data"));
        assert_eq!(s2.config.games.len(), 1);
        assert_eq!(s2.config.dest, Some(t.path().join("vault")));
        assert_eq!(s2.ctx().machine, "Odin");
        assert!(t.path().join("data/config.json").is_file(), "config.json : meme format que le PC");
        let mut s3 = s2;
        assert!(s3.remove_game("documents"));
        assert!(s3.config.games.is_empty());
    }

    #[test]
    fn candidats_de_coffre_syncthing_existant_nomme() {
        let t = tempfile::TempDir::new().unwrap();
        let root = t.path().join("emulated/0");
        std::fs::create_dir_all(root.join("Sync/.stfolder")).unwrap();
        std::fs::create_dir_all(root.join("Download/AutoGameSoftwareSync")).unwrap();
        std::fs::create_dir_all(root.join("Android/data/x/.stfolder")).unwrap(); // jamais parcouru
        std::fs::create_dir_all(root.join("Old/vault")).unwrap();
        std::fs::write(root.join("Old/vault/.agss-vault"), "x").unwrap();
        let c = vault_candidates(&[root.clone()]);
        let kinds: Vec<(PathBuf, CandKind)> = c.iter().map(|c| (c.path.clone(), c.kind.clone())).collect();
        assert_eq!(kinds[0], (root.join("Old/vault"), CandKind::Existing(vec![])));
        assert!(kinds.contains(&(root.join("Sync"), CandKind::Syncthing)));
        assert!(kinds.contains(&(root.join("Download/AutoGameSoftwareSync"), CandKind::Named)));
        assert!(!kinds.iter().any(|(p, _)| p.starts_with(root.join("Android"))));
        assert_eq!(list_subdirs(&root).len(), 4);
    }

    #[test]
    fn propositions_emulateurs_et_dossiers_perso() {
        let t = tempfile::TempDir::new().unwrap();
        let root = t.path().join("emulated/0");
        std::fs::create_dir_all(root.join("RetroArch/saves")).unwrap();
        std::fs::create_dir_all(root.join("DCIM/Camera")).unwrap();
        let p = proposals(&[root.clone()], &[]);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].title, "RetroArch");
        assert_eq!(p[0].locations, vec![("Saves (SRAM, memory cards)".to_string(), true), ("Save states".to_string(), false)]);
        let g = build_game(&p[0], &[true, false]).unwrap();
        assert_eq!(g.name, "RetroArch saves");
        assert_eq!(g.path, root.join("RetroArch/saves"));
        assert!(build_game(&p[0], &[false, false]).is_none());
        assert_eq!(kind_of(&g), Kind::Emulator);
        assert!(build_game(&p[1], &[false]).is_none(), "dossier perso non coche : rien");
        let f = build_game(&p[1], &[true]).unwrap();
        assert_eq!(kind_of(&f), Kind::Folder);
        let again = proposals(&[root], &[g]);
        assert!(again[0].already && !again[1].already);
    }

    #[test]
    fn synchro_d_un_element_comme_le_pc() {
        let t = tempfile::TempDir::new().unwrap();
        let vault = t.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let ctx = make_ctx(&t.path().join("data"), "Console");
        let g = emulators::personal_game("Documents", &t.path().join("Documents"));
        let nolog = |_: &str| {};
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::Missing);
        std::fs::create_dir_all(&g.path).unwrap();
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::Empty);
        std::fs::write(g.path.join("a.txt"), "1").unwrap();
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::Backed);
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::UpToDate);
        let info = info_of(&ctx, &g, Some(&vault), None);
        assert_eq!(info.state, ItemState::Safe);
        assert_eq!(info.versions, 1);
        assert_eq!(info.last_device.as_deref(), Some("Console"));
        // Un autre appareil ecrit une version plus recente : SIGNALEE (jamais remise toute seule sur la console)…
        let other = make_ctx(&t.path().join("data-pc"), "PC");
        let g2 = emulators::personal_game("Documents", &t.path().join("Documents-pc"));
        std::fs::create_dir_all(&g2.path).unwrap();
        backup::restore(&other, &g2, &vault).unwrap();
        std::fs::write(g2.path.join("a.txt"), "2").unwrap();
        backup::backup(&other, &g2, &vault).unwrap();
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::Available { from: "PC".into() });
        assert_eq!(std::fs::read_to_string(g.path.join("a.txt")).unwrap(), "1", "rien n'a ete ecrit en local");
        let info = info_of(&ctx, &g, Some(&vault), Some(&ItemState::Available("PC".into())));
        assert_eq!(info.state, ItemState::Available("PC".into()));
        assert!(info.state.needs_attention(info.versions));
        // … puis remise avec l'appareil d'origine quand l'utilisateur confirme.
        assert_eq!(sync_item(&ctx, &g, &vault, true, true, &nolog), SyncResult::Restored { from: "PC".into(), files: 1 });
        assert_eq!(std::fs::read_to_string(g.path.join("a.txt")).unwrap(), "2");
        assert_eq!(agss::home::positions(&vault, &cloud::dest_key(&g)).len(), 1, "marque de position ecrite");
        assert!(matches!(sync_item(&ctx, &g, &t.path().join("absent"), true, false, &nolog), SyncResult::Error(_)));
    }

    /// Conflit a deux appareils : jamais resolu tout seul sur la console (les deux versions gardees), resolution
    /// automatique « la plus recente » seulement sur demande explicite (auto_cloud ET auto_restore).
    #[test]
    fn synchro_en_conflit_jamais_resolue_toute_seule() {
        let t = tempfile::TempDir::new().unwrap();
        let vault = t.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let nolog = |_: &str| {};
        let ctx = make_ctx(&t.path().join("data"), "Console");
        let g = emulators::personal_game("Documents", &t.path().join("Documents"));
        std::fs::create_dir_all(&g.path).unwrap();
        std::fs::write(g.path.join("a.txt"), "base").unwrap();
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::Backed);
        let pc = make_ctx(&t.path().join("data-pc"), "PC");
        let g2 = emulators::personal_game("Documents", &t.path().join("Documents-pc"));
        std::fs::create_dir_all(&g2.path).unwrap();
        backup::restore(&pc, &g2, &vault).unwrap();
        // Les deux modifient depuis la meme base.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(g2.path.join("a.txt"), "pc").unwrap();
        backup::backup(&pc, &g2, &vault).unwrap();
        std::fs::write(g.path.join("a.txt"), "console").unwrap();
        assert_eq!(backup::plan(&ctx, &g, &vault).unwrap(), backup::Plan::Conflict);
        // 1. Synchro automatique avec « la plus recente automatiquement » : conflit signale, version locale gardee en branche.
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::Conflict);
        assert_eq!(std::fs::read_to_string(g.path.join("a.txt")).unwrap(), "console", "rien d'ecrase");
        let h = backup::history(&ctx, &g, &vault);
        assert_eq!(h.conflict_heads.len(), 2, "les deux versions sont dans le coffre");
        let info = info_of(&ctx, &g, Some(&vault), Some(&ItemState::Conflict));
        assert_eq!(info.state, ItemState::Conflict);
        assert!(info.conflict && info.state.needs_attention(info.versions));
        // 2. Mode « me demander » : pareil.
        assert_eq!(sync_item(&ctx, &g, &vault, false, false, &nolog), SyncResult::Conflict);
        assert_eq!(sync_item(&ctx, &g, &vault, false, true, &nolog), SyncResult::Conflict);
        // 3. Demande explicite avec « la plus recente » : resolu, le local prend la version du PC (plus recente).
        assert_eq!(sync_item(&ctx, &g, &vault, true, true, &nolog), SyncResult::ConflictResolved);
        assert_eq!(std::fs::read_to_string(g.path.join("a.txt")).unwrap(), "pc");
        assert_eq!(sync_item(&ctx, &g, &vault, true, false, &nolog), SyncResult::UpToDate);
        assert!(backup::history(&ctx, &g, &vault).conflict_heads.is_empty());
    }

    #[test]
    fn precedence_des_etats_et_etats_a_regarder() {
        let t = tempfile::TempDir::new().unwrap();
        let vault = t.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let ctx = make_ctx(&t.path().join("data"), "Console");
        let g = emulators::personal_game("Documents", &t.path().join("Documents"));
        // Dossier absent : Missing, quoi qu'en dise l'etat precedent (sauf erreur / recu / copie / vide, qui priment).
        assert_eq!(info_of(&ctx, &g, Some(&vault), None).state, ItemState::Missing);
        assert_eq!(info_of(&ctx, &g, Some(&vault), Some(&ItemState::Safe)).state, ItemState::Missing);
        assert_eq!(info_of(&ctx, &g, Some(&vault), Some(&ItemState::Error("x".into()))).state, ItemState::Error("x".into()));
        assert_eq!(info_of(&ctx, &g, None, None).state, ItemState::Missing);
        std::fs::create_dir_all(&g.path).unwrap();
        assert_eq!(info_of(&ctx, &g, Some(&vault), None).state, ItemState::Never, "dossier present, coffre vide");
        assert_eq!(info_of(&ctx, &g, Some(&vault), Some(&ItemState::Empty)).state, ItemState::Empty);
        assert_eq!(info_of(&ctx, &g, Some(&vault), Some(&ItemState::Copied)).state, ItemState::Copied);
        assert_eq!(info_of(&ctx, &g, Some(&vault), Some(&ItemState::Received("PC".into()))).state, ItemState::Received("PC".into()));
        assert_eq!(info_of(&ctx, &g, Some(&vault), Some(&ItemState::Syncing)).state, ItemState::Never, "etat transitoire ignore");
        // A regarder : conflit, probleme, introuvable, version disponible, acces manquant ; vide seulement s'il y a eu des versions.
        assert!(!ItemState::Empty.needs_attention(0));
        assert!(ItemState::Empty.needs_attention(2));
        assert!(ItemState::NoAccess.needs_attention(0));
        assert!(ItemState::Available("PC".into()).needs_attention(0));
        assert!(!ItemState::Safe.needs_attention(5) && !ItemState::Copied.needs_attention(5) && !ItemState::Received("PC".into()).needs_attention(5));
        assert_eq!(sync_gate(false, true), Gate::NotSetUp);
        assert_eq!(sync_gate(true, false), Gate::NoPermission, "autorisation retiree : aucune synchro");
        assert_eq!(sync_gate(true, true), Gate::Ok);
    }

    #[test]
    fn candidats_de_coffre_en_largeur_un_gros_download_ne_cache_pas_sync() {
        let t = tempfile::TempDir::new().unwrap();
        let root = t.path().join("emulated/0");
        for i in 0..40 {
            std::fs::create_dir_all(root.join(format!("Download/d{:02}/x/y", i))).unwrap();
        }
        std::fs::create_dir_all(root.join("Sync/.stfolder")).unwrap();
        std::fs::create_dir_all(root.join("Download/d05/AutoGameSoftwareSync")).unwrap();
        // Budget serre : la racine, ses 2 enfants, puis le niveau 2 ; Sync (niveau 1) est vu avant les 40 sous-dossiers de Download.
        let c = vault_candidates_budget(&[root.clone()], 10);
        assert!(c.iter().any(|c| c.path == root.join("Sync") && c.kind == CandKind::Syncthing), "{:?}", c);
        // Avec le budget normal : Download n'est pas parcouru au-dela du niveau 2 (d05/AutoGameSoftwareSync est au niveau 3).
        let c = vault_candidates(&[root.clone()]);
        assert!(c.iter().any(|c| c.path == root.join("Sync")));
        assert!(!c.iter().any(|c| c.path.starts_with(root.join("Download/d05"))), "{:?}", c);
        // Un dossier nomme directement dans Download reste trouve (niveau 2).
        std::fs::create_dir_all(root.join("Download/AutoGameSoftwareSync")).unwrap();
        let c = vault_candidates(&[root.clone()]);
        assert!(c.iter().any(|c| c.path == root.join("Download/AutoGameSoftwareSync") && c.kind == CandKind::Named));
    }

    /// Arret brutal pendant une copie : dossier de travail local et version de CET appareil sans meta.json retires au
    /// demarrage ; une version complete ou celle d'un autre appareil ne sont jamais touchees.
    #[test]
    fn orphelins_nettoyes_au_demarrage() {
        let t = tempfile::TempDir::new().unwrap();
        let data = t.path().join("data");
        let vault = t.path().join("vault");
        let tmp = data.join("tmp");
        std::fs::create_dir_all(tmp.join("20261029-060330-715-Retroid-d8841d39/files")).unwrap();
        std::fs::write(tmp.join("20261029-060330-715-Retroid-d8841d39/files/BigGame.srm.agss-tmp"), "partiel").unwrap();
        let claimed = vault.join("retroarchsaves/saves/20261029-060402-809-Retroid");
        std::fs::create_dir_all(claimed.join("files")).unwrap();
        std::fs::write(claimed.join("files/BigGame.srm"), "partiel").unwrap();
        std::fs::write(tmp.join("claimed.txt"), claimed.to_string_lossy().as_bytes()).unwrap();
        let full = vault.join("retroarchsaves/saves/20261029-050000-000-Retroid");
        std::fs::create_dir_all(full.join("files")).unwrap();
        std::fs::write(full.join("meta.json"), "{}").unwrap();
        let other = vault.join("retroarchsaves/saves/20261029-060500-000-PC");
        std::fs::create_dir_all(other.join("files")).unwrap();
        let (n, removed) = startup_cleanup(&data, "Retroid");
        assert_eq!(n, 1, "dossier de travail local retire");
        assert_eq!(removed, Some(claimed.clone()));
        assert!(!claimed.exists() && !tmp.join("claimed.txt").exists());
        assert!(std::fs::read_dir(&tmp).unwrap().count() == 0, "tmp vide");
        assert!(full.join("meta.json").exists(), "version complete intacte");
        assert!(other.exists(), "version incomplete d'un autre appareil intacte (synchro en cours ?)");
        assert_eq!(startup_cleanup(&data, "Retroid"), (0, None));
    }
}
