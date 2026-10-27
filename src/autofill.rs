//! Ajout dynamique par exe : retrouve le jeu dans la base EMBARQUEE (par exe, par dossier d'installation, par AppID Steam),
//! resout ses emplacements de sauvegarde pour CE PC et verifie qu'ils existent. Pur et testable (dossiers temporaires).
//! Ordre : overrides locales (deja fusionnees dans la base) > indices du dossier partage (`hints`) > base embarquee.
use crate::cloud::DestGame;
use crate::gamedb::{DbGame, GameDb};
use crate::hints::Suggestion;
use crate::catalog::{self, StoreCtx};
use std::path::{Path, PathBuf};

/// Etat affiche sous les champs « Nom » / « Dossier de sauvegarde ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoStatus {
    None,
    /// Un seul jeu, un dossier de sauvegarde qui existe.
    Verified,
    /// Dossier introuvable sur ce PC.
    Missing,
    /// Plusieurs jeux ou plusieurs dossiers possibles : jamais de choix silencieux.
    Many,
}

pub fn status_of(suggs: &[Suggestion]) -> AutoStatus {
    match suggs {
        [] => AutoStatus::None,
        [s] if s.local_path.is_some() && s.exists => AutoStatus::Verified,
        [_] => AutoStatus::Missing,
        _ => AutoStatus::Many,
    }
}

/// Une proposition unique et utilisable (chemin connu) peut remplir les champs automatiquement.
pub fn autofill_candidate(suggs: &[Suggestion]) -> Option<&Suggestion> {
    match suggs {
        [s] if s.local_path.is_some() => Some(s),
        _ => None,
    }
}

/// `steamapps\common\<dir>` : (bibliotheque, nom du dossier) si `p` est un dossier d'installation Steam.
fn steam_install(p: &Path, libs: &[PathBuf]) -> Option<(PathBuf, String)> {
    let name = p.file_name()?.to_string_lossy().to_string();
    let common = p.parent()?;
    if !common.file_name()?.to_string_lossy().eq_ignore_ascii_case("common") {
        return None;
    }
    let steamapps = common.parent()?;
    if !steamapps.file_name()?.to_string_lossy().eq_ignore_ascii_case("steamapps") {
        return None;
    }
    let lib = steamapps.parent()?.to_path_buf();
    let _ = libs;
    Some((lib, name))
}

/// AppID Steam d'un dossier d'installation (appmanifest_*.acf dont `installdir` correspond).
pub fn steam_app_id(steamapps: &Path, install_dir_name: &str) -> Option<u64> {
    for e in std::fs::read_dir(steamapps).ok()?.filter_map(|e| e.ok()) {
        let n = e.file_name().to_string_lossy().to_string();
        if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
            continue;
        }
        if let Ok((id, _, dir)) = crate::detect::parse_acf(&std::fs::read_to_string(e.path()).unwrap_or_default()) {
            if dir.eq_ignore_ascii_case(install_dir_name) {
                return id.parse().ok();
            }
        }
    }
    None
}

struct Match<'a> {
    game: &'a DbGame,
    install_dir: Option<PathBuf>,
    app_id: Option<u64>,
}

fn push_unique<'a>(out: &mut Vec<Match<'a>>, game: &'a DbGame, install_dir: Option<PathBuf>, app_id: Option<u64>) {
    if !out.iter().any(|m| m.game.name == game.name) {
        out.push(Match { game, install_dir, app_id });
    }
}

/// Recherche en cascade dans la base : (1) dossier d'installation / AppID (specifiques), sinon (2) nom d'exe.
/// `exe_full` : chemin complet de l'exe quand il vient de « Parcourir ».
fn find<'a>(db: &'a GameDb, exe_typed: &str, exe_full: Option<&Path>, libs: &[PathBuf]) -> Vec<Match<'a>> {
    let mut specific: Vec<Match> = Vec::new();
    if let Some(full) = exe_full {
        let mut cur = full.parent();
        for _ in 0..4 {
            let Some(dir) = cur else { break };
            if let Some(name) = dir.file_name().map(|n| n.to_string_lossy().to_string()) {
                for g in db.find_by_install_dir(&name) {
                    push_unique(&mut specific, g, Some(dir.to_path_buf()), None);
                }
                if let Some((lib, dname)) = steam_install(dir, libs) {
                    if let Some(id) = steam_app_id(&lib.join("steamapps"), &dname) {
                        for g in db.find_by_steam(id) {
                            push_unique(&mut specific, g, Some(dir.to_path_buf()), Some(id));
                        }
                    }
                }
            }
            cur = dir.parent();
        }
    }
    // Les correspondances specifiques doivent aussi citer l'exe quand la base le connait (evite un dossier parent trop generique).
    let want = crate::hints::exe_file_name(exe_typed).to_lowercase();
    // Linux : programme natif (sans .exe) ; la base ne connait que les .exe Windows : on compare les noms sans extension,
    // et un AppID Steam suffit.
    let native = cfg!(not(windows)) && !want.ends_with(".exe");
    let stem_of = |s: &str| -> String { crate::detect::norm_name(s.split('.').next().unwrap_or(s)) };
    let want_stem = stem_of(&want);
    let confirmed: Vec<Match> = specific
        .into_iter()
        .filter(|m| m.game.exes.is_empty() || m.game.exes.iter().any(|x| x.eq_ignore_ascii_case(&want)) || (native && (m.app_id.is_some() || m.game.exes.iter().any(|x| stem_of(x) == want_stem))))
        .collect();
    if !confirmed.is_empty() {
        return confirmed;
    }
    let mut out = Vec::new();
    // Un nom d'exe ne suffit jamais s'il est generique (runtime, outil, navigateur...) ou trop ambigu dans la base.
    if crate::gamedetect::is_generic_name(&want) {
        return out;
    }
    let by_name = if native && want_stem.len() >= 4 { db.find_by_exe(&format!("{}.exe", want.split('.').next().unwrap_or(&want))) } else { db.find_by_exe(exe_typed) };
    if by_name.len() > crate::gamedetect::MAX_AMBIGUOUS_HITS {
        return out;
    }
    for g in by_name {
        push_unique(&mut out, g, None, None);
    }
    out
}

/// Propositions pour un exe : une ligne par (jeu, dossier de sauvegarde EXISTANT) ; sans dossier existant, le meilleur
/// chemin avec `exists = false`. Le nom reprend celui d'un jeu deja present dans le dossier partage (meme cle normalisee).
pub fn lookup(db: &GameDb, exe_typed: &str, exe_full: Option<&Path>, dest_games: &[DestGame], libs: &[PathBuf], exists: &dyn Fn(&Path) -> bool) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for m in find(db, exe_typed, exe_full, libs) {
        let name = dest_games.iter().find(|d| d.key == crate::cloud::game_key(&m.game.name)).map(|d| d.name.clone()).unwrap_or_else(|| m.game.name.clone());
        let entry = m.game.to_entry();
        let sc = StoreCtx { steam_libs: libs.to_vec(), install_dir: m.install_dir.clone(), game_id: m.app_id.map(|i| i.to_string()) };
        let (dirs, _) = crate::actions::existing_save_dirs(&entry, &m.game.name, &sc);
        let found: Vec<PathBuf> = dirs.into_iter().map(|(p, _)| p).filter(|p| exists(p)).take(5).collect();
        let mut rows: Vec<Suggestion> = Vec::new();
        if found.is_empty() {
            let r = catalog::resolve_with(&entry, &m.game.name, &sc);
            let best = r.paths.iter().find(|p| !p.contains('*') && !p.contains('?')).map(PathBuf::from);
            rows.push(Suggestion { game_name: name.clone(), local_path: best, exists: false });
        } else {
            for p in found {
                rows.push(Suggestion { game_name: name.clone(), local_path: Some(p), exists: true });
            }
        }
        for r in rows {
            if !out.iter().any(|o| o.game_name == r.game_name && o.local_path == r.local_path) {
                out.push(r);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ bouton « Ajouter » (machine a etats pure)

/// Etat visuel du bouton « Ajouter ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddBtn {
    /// Nom ou dossier invalide : bouton normal.
    Invalid,
    /// Nom + dossier existant valides : halo d'accent qui pulse.
    Pulsing,
    /// Valide mais deja clique : l'animation s'arrete.
    Stopped,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AddBtnState {
    clicked: bool,
}

impl AddBtnState {
    /// A appeler a chaque image ou le bouton est dessine. L'animation s'arrete des le clic, des qu'un champ redevient
    /// invalide (et repart seulement apres une nouvelle validite), et `reset()` a la sortie de la page.
    pub fn update(&mut self, valid: bool, clicked_now: bool) -> AddBtn {
        if !valid {
            self.clicked = false;
            return AddBtn::Invalid;
        }
        if clicked_now {
            self.clicked = true;
        }
        if self.clicked {
            AddBtn::Stopped
        } else {
            AddBtn::Pulsing
        }
    }

    pub fn reset(&mut self) {
        self.clicked = false;
    }
}

/// Intensite (0..1) du halo a l'instant `t` secondes : pulsation douce (periode ~2,4 s).
pub fn pulse(t: f64) -> f32 {
    (0.5 + 0.5 * (t * std::f64::consts::TAU / 2.4).sin()) as f32
}

// ------------------------------------------------------------------ etat de la page « Ajouter un jeu » (machine a etats pure)

/// Etat transitoire de l'encadre « Jeu reconnu » : masque apres « Utiliser » / « Ignorer », jusqu'au changement d'exe ;
/// `reset()` a chaque changement d'onglet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddPage {
    dismissed: Option<String>,
}

impl AddPage {
    /// A appeler a chaque image avec l'exe saisi (en minuscules) : un exe different reaffiche l'encadre.
    pub fn observe_exe(&mut self, exe_key: &str) {
        if self.dismissed.as_deref().map(|d| d != exe_key).unwrap_or(false) {
            self.dismissed = None;
        }
    }

    /// L'encadre est-il visible pour cet exe ?
    pub fn visible(&self, exe_key: &str) -> bool {
        self.dismissed.as_deref() != Some(exe_key)
    }

    /// « Utiliser » ou « Ignorer » : l'encadre disparait pour cet exe.
    pub fn dismiss(&mut self, exe_key: &str) {
        self.dismissed = Some(exe_key.to_string());
    }

    pub fn reset(&mut self) {
        self.dismissed = None;
    }
}
