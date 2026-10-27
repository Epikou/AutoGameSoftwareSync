//! Selecteur de dossier guide (2.0, lot 8). Logique pure et testee ; l'ecran est dans `gui/picker_ui.rs`.
//! Avant la boite de dialogue du systeme, on montre nos propres suggestions (emplacements trouves par les couches de
//! connaissance, dossiers recemment modifies par ce programme, raccourcis vers les dossiers usuels), puis on ouvre la boite
//! du systeme DIRECTEMENT dans le dossier le plus probable. Sur un dossier choisi : apercu (fichiers, taille, date) et
//! avertissements (dossier enorme, dossier systeme, liste noire de `sensitive`, coffre lui-meme).
use crate::config::{Game, State};
use crate::knowledge::paths::Folders;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Au-dela de cette taille, le dossier est « enorme » (copie complete a chaque version : voir ETUDE §5).
pub const HUGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// L'apercu s'arrete apres ce nombre de fichiers (le dossier est alors dit « au moins »).
pub const PREVIEW_MAX_FILES: usize = 20_000;
const PREVIEW_MAX_TIME: Duration = Duration::from_millis(1500);

// ------------------------------------------------------------------ raccourcis

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    SavedGames,
    MyGames,
    Documents,
    AppDataRoaming,
    AppDataLocal,
    LocalLow,
    /// Linux : `~/.config`.
    ConfigDir,
    /// Linux : `~/.local/share`.
    ShareDir,
    /// Linux : dossier utilisateur d'un prefixe Wine / Proton (`AppData`).
    Proton,
    Home,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutItem {
    pub kind: Shortcut,
    pub path: PathBuf,
    /// Nom du prefixe Proton (identifiant du jeu) pour `Shortcut::Proton`.
    pub tag: String,
}

/// Dossiers de base de CE PC pour le selecteur (Documents rempli aussi sous Linux).
pub fn real_folders() -> Folders {
    let mut f = Folders::real();
    if f.documents.is_none() {
        f.documents = dirs::document_dir();
    }
    f
}

/// Raccourcis a montrer : seulement ceux dont le dossier existe. Windows : Parties enregistrees, Mes jeux, Documents, Donnees
/// d'applis (Roaming, Local, LocalLow). Linux : `~/.config`, `~/.local/share`, Documents, dossier perso, et le dossier
/// AppData de chaque prefixe Proton / Wine (4 au plus).
pub fn shortcuts(f: &Folders, prefixes: &[PathBuf], windows: bool, exists: &dyn Fn(&Path) -> bool) -> Vec<ShortcutItem> {
    let mut v: Vec<ShortcutItem> = Vec::new();
    let mut add = |kind: Shortcut, p: Option<PathBuf>, tag: &str| {
        if let Some(p) = p.filter(|p| exists(p)) {
            if !v.iter().any(|x| x.path == p) {
                v.push(ShortcutItem { kind, path: p, tag: tag.to_string() });
            }
        }
    };
    if windows {
        add(Shortcut::SavedGames, f.saved_games.clone(), "");
        add(Shortcut::MyGames, f.documents.as_ref().map(|d| d.join("My Games")), "");
        add(Shortcut::Documents, f.documents.clone(), "");
        add(Shortcut::AppDataRoaming, f.roaming.clone(), "");
        add(Shortcut::AppDataLocal, f.local.clone(), "");
        add(Shortcut::LocalLow, f.locallow.clone(), "");
    } else {
        add(Shortcut::ConfigDir, f.xdg_config.clone(), "");
        add(Shortcut::ShareDir, f.xdg_data.clone(), "");
        add(Shortcut::Documents, f.documents.clone(), "");
        for p in prefixes.iter().take(4) {
            let tag = p.parent().and_then(|x| x.file_name()).filter(|_| p.file_name().map(|n| n == "pfx").unwrap_or(false)).or_else(|| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            add(Shortcut::Proton, Some(crate::proton::user_dir(p).join("AppData")), &tag);
        }
        add(Shortcut::Home, f.home.clone(), "");
    }
    v
}

// ------------------------------------------------------------------ dossier de depart

/// Ce qu'on sait du programme pour lequel on choisit un emplacement.
#[derive(Clone, Debug, Default)]
pub struct Hint {
    /// Editeur, produit, nom : dans l'ordre de preference.
    pub names: Vec<String>,
    /// Emplacements trouves par les couches de connaissance (verifies, du plus fiable au moins fiable).
    pub found: Vec<PathBuf>,
}

/// Variantes d'un nom pour retrouver son dossier (« Team Cherry, Inc. » -> « Team Cherry » -> « TeamCherry »).
pub fn name_variants(name: &str) -> Vec<String> {
    fn add(out: &mut Vec<String>, s: &str) {
        let s = s.trim().trim_end_matches([',', '.', ' ']).to_string();
        if s.len() >= 2 && !out.iter().any(|o| o.eq_ignore_ascii_case(&s)) {
            out.push(s);
        }
    }
    const SUFFIXES: [&str; 18] = [", inc", " inc", ", ltd", " ltd", " llc", " gmbh", " s.a", " sas", " corporation", " corp", " studios", " studio", " games", " entertainment", " interactive", " software", " co", " ab"];
    let mut out: Vec<String> = Vec::new();
    add(&mut out, name.trim().trim_matches('"'));
    let mut i = 0;
    while i < out.len() {
        let cur = out[i].clone();
        let low = cur.to_lowercase();
        for s in SUFFIXES {
            if low.ends_with(s) && cur.len() > s.len() + 1 {
                add(&mut out, &cur[..cur.len() - s.len()]);
            }
        }
        i += 1;
    }
    let snapshot = out.clone();
    for s in snapshot {
        if s.contains(' ') && !s.contains(',') {
            add(&mut out, &s.replace(' ', ""));
        }
    }
    out
}

/// Dossier dans lequel la boite du systeme s'ouvre : 1. un emplacement trouve qui existe ; 2. `<base>/<editeur|produit|nom>`
/// qui existe, dans les bases Local, Roaming, LocalLow, Parties enregistrees, Mes jeux, Documents (Linux : `~/.local/share`,
/// `~/.config`, prefixes Proton) ; 3. sinon AppData (Local) ; sinon Roaming, Documents, dossier perso.
pub fn start_dir(hint: &Hint, f: &Folders, prefixes: &[PathBuf], windows: bool, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    for p in &hint.found {
        if exists(p) {
            return p.clone();
        }
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    if windows {
        let docs = f.documents.clone();
        bases.extend([f.local.clone(), f.roaming.clone(), f.locallow.clone(), f.saved_games.clone(), docs.as_ref().map(|d| d.join("My Games")), docs].into_iter().flatten());
    } else {
        bases.extend([f.xdg_data.clone(), f.xdg_config.clone(), f.documents.clone()].into_iter().flatten());
        for p in prefixes.iter().take(8) {
            let u = crate::proton::user_dir(p).join("AppData");
            bases.extend([u.join("Local"), u.join("Roaming"), u.join("LocalLow")]);
        }
    }
    let variants: Vec<String> = hint.names.iter().flat_map(|n| name_variants(n)).collect();
    for name in &variants {
        for b in &bases {
            let p = b.join(name);
            if exists(&p) {
                return p;
            }
        }
    }
    let fallbacks: Vec<Option<PathBuf>> = if windows { vec![f.local.clone(), f.roaming.clone(), f.documents.clone(), f.profile.clone()] } else { vec![f.xdg_data.clone(), f.xdg_config.clone(), f.home.clone()] };
    fallbacks.into_iter().flatten().find(|p| exists(p)).unwrap_or_default()
}

// ------------------------------------------------------------------ suggestions

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Trouve par les couches de connaissance pour ce programme.
    Found,
    /// Dossier recemment modifie par ce programme (copie, restauration).
    Recent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub path: PathBuf,
    pub why: Why,
    /// Date de derniere activite (ms) pour `Recent`.
    pub at_ms: u64,
}

/// Dossiers recemment modifies par ce programme : emplacements des elements dont la derniere copie ou synchro est la plus
/// recente (un fichier unique est remplace par son dossier), les plus recents d'abord, sans doublon.
pub fn recent_dirs(games: &[Game], state: &State, limit: usize, exists: &dyn Fn(&Path) -> bool) -> Vec<Suggestion> {
    let mut v: Vec<Suggestion> = Vec::new();
    for g in games {
        let at = [&state.last_backup_ms, &state.last_sync_ms, &state.last_restore_ms].iter().map(|m| m.get(&g.name).copied().unwrap_or(0)).max().unwrap_or(0);
        if at == 0 {
            continue;
        }
        let mut places: Vec<(PathBuf, bool)> = vec![(g.path.clone(), g.save_is_file)];
        places.extend(g.extra_locations.iter().map(|l| (l.path.clone(), l.is_file)));
        for (p, is_file) in places {
            let dir = if is_file { p.parent().map(|x| x.to_path_buf()).unwrap_or(p) } else { p };
            if !dir.as_os_str().is_empty() && exists(&dir) && !v.iter().any(|s| s.path == dir) {
                v.push(Suggestion { path: dir, why: Why::Recent, at_ms: at });
            }
        }
    }
    v.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(a.path.cmp(&b.path)));
    v.truncate(limit);
    v
}

/// Liste finale des suggestions : emplacements trouves (3 au plus), puis dossiers recents (3 au plus), sans doublon.
pub fn suggestions(found: &[PathBuf], recent: Vec<Suggestion>, exists: &dyn Fn(&Path) -> bool) -> Vec<Suggestion> {
    let mut v: Vec<Suggestion> = Vec::new();
    for p in found.iter().filter(|p| exists(p)).take(3) {
        if !v.iter().any(|s| &s.path == p) {
            v.push(Suggestion { path: p.clone(), why: Why::Found, at_ms: 0 });
        }
    }
    for s in recent.into_iter().take(3) {
        if !v.iter().any(|x| x.path == s.path) {
            v.push(s);
        }
    }
    v
}

/// Chemin presente en miettes : la base connue (Parties enregistrees, Donnees locales...) puis les sous-dossiers.
/// `None` = aucune base connue, le chemin est montre en entier.
pub fn friendly_parts(path: &Path, f: &Folders) -> (Option<Shortcut>, Vec<String>) {
    let p = comps_keep(path);
    let mut bases: Vec<(Shortcut, PathBuf)> = Vec::new();
    for (k, b) in [
        (Shortcut::LocalLow, f.locallow.clone()),
        (Shortcut::AppDataLocal, f.local.clone()),
        (Shortcut::AppDataRoaming, f.roaming.clone()),
        (Shortcut::SavedGames, f.saved_games.clone()),
        (Shortcut::MyGames, f.documents.as_ref().map(|d| d.join("My Games"))),
        (Shortcut::Documents, f.documents.clone()),
        (Shortcut::ShareDir, f.xdg_data.clone()),
        (Shortcut::ConfigDir, f.xdg_config.clone()),
    ] {
        if let Some(b) = b {
            bases.push((k, b));
        }
    }
    let mut best: Option<(usize, Shortcut)> = None;
    for (k, b) in &bases {
        let bc = comps_keep(b);
        if !bc.is_empty() && p.len() > bc.len() && bc.iter().zip(p.iter()).all(|(a, c)| a.eq_ignore_ascii_case(c)) && best.map(|(n, _)| bc.len() > n).unwrap_or(true) {
            best = Some((bc.len(), *k));
        }
    }
    match best {
        Some((n, k)) => (Some(k), p[n..].to_vec()),
        None => (None, p),
    }
}

fn comps_keep(p: &Path) -> Vec<String> {
    p.to_string_lossy().replace('\\', "/").split('/').filter(|c| !c.is_empty()).map(|c| c.to_string()).collect()
}

/// Extensions d'un filtre tape en mode expert (`*.sav ; Profil*.dat`) pour la boite du systeme (extensions seulement).
pub fn filter_exts(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pat in text.split([';', ',']) {
        let pat = pat.trim();
        if let Some((_, ext)) = pat.rsplit_once('.') {
            let ext = ext.trim().to_lowercase();
            if !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) && !out.contains(&ext) {
                out.push(ext);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ apercu

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub exists: bool,
    pub is_file: bool,
    pub files: u64,
    pub bytes: u64,
    /// Derniere modification (ms depuis 1970) du fichier le plus recent.
    pub newest_ms: Option<u64>,
    /// L'apercu s'est arrete avant la fin (trop de fichiers ou trop long) : les nombres sont des minimums.
    pub truncated: bool,
    /// Fichiers ou dossiers de la liste noire (`sensitive`) rencontres : ils ne seront jamais copies.
    pub sensitive: u32,
}

fn mtime_ms(m: &std::fs::Metadata) -> Option<u64> {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64)
}

/// Apercu d'un dossier ou d'un fichier (lecture seule). `max_files` : arret apres ce nombre de fichiers.
pub fn scan(path: &Path, max_files: usize) -> Preview {
    let Ok(md) = std::fs::metadata(path) else { return Preview::default() };
    if md.is_file() {
        return Preview { exists: true, is_file: true, files: 1, bytes: md.len(), newest_ms: mtime_ms(&md), truncated: false, sensitive: u32::from(crate::sensitive::rel_is_sensitive(&path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default())) };
    }
    let mut pv = Preview { exists: true, ..Default::default() };
    let t0 = Instant::now();
    let mut it = walkdir::WalkDir::new(path).follow_links(false).into_iter();
    while let Some(e) = it.next() {
        let Ok(e) = e else { continue };
        if e.depth() == 0 {
            continue;
        }
        let rel = e.path().strip_prefix(path).map(|r| r.to_string_lossy().replace('\\', "/").to_lowercase()).unwrap_or_default();
        if crate::sensitive::rel_is_sensitive(&rel) {
            pv.sensitive += 1;
            if e.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        }
        if !e.file_type().is_file() {
            continue;
        }
        pv.files += 1;
        if let Ok(m) = e.metadata() {
            pv.bytes += m.len();
            if let Some(t) = mtime_ms(&m) {
                pv.newest_ms = Some(pv.newest_ms.map_or(t, |o| o.max(t)));
            }
        }
        if pv.files as usize >= max_files || (pv.files % 256 == 0 && t0.elapsed() > PREVIEW_MAX_TIME) {
            pv.truncated = true;
            break;
        }
    }
    pv
}

// ------------------------------------------------------------------ avertissements

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// Plus de 2 Go (au moins, si l'apercu s'est arrete avant la fin).
    Huge { bytes: u64, at_least: bool },
    /// Au-dessus du seuil de l'alerte poids (reglable, 500 Mo par defaut) mais sous 2 Go : taille et impact dans le coffre.
    Heavy { bytes: u64, at_least: bool },
    /// Dossier du systeme (Windows, Program Files, `/usr`...) ou disque entier.
    System,
    /// Dossier beaucoup trop large (dossier perso, AppData entier).
    TooBroad,
    /// Le dossier lui-meme est un emplacement de la liste noire (navigateur, cles, mots de passe).
    Sensitive,
    /// Il contient des fichiers de la liste noire : ils seront ignores.
    SensitiveInside(u32),
    /// C'est le coffre lui-meme (ou un dossier a l'interieur) : refuse.
    IsVault,
    /// Le dossier est vide.
    Empty,
}

impl Warning {
    /// Un avertissement qui empeche de valider.
    pub fn blocks(self) -> bool {
        matches!(self, Warning::IsVault)
    }
}

/// Contexte des avertissements.
pub struct WarnCtx<'a> {
    pub folders: &'a Folders,
    pub vault: Option<&'a Path>,
    pub windows: bool,
    /// Dossiers du systeme (Windows : `%SystemRoot%`, `%ProgramFiles%`, `%ProgramFiles(x86)%`).
    pub system_dirs: Vec<PathBuf>,
}

impl<'a> WarnCtx<'a> {
    pub fn real(folders: &'a Folders, vault: Option<&'a Path>) -> WarnCtx<'a> {
        let mut system_dirs: Vec<PathBuf> = Vec::new();
        if cfg!(windows) {
            for v in ["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
                if let Some(p) = std::env::var_os(v).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()) {
                    if !system_dirs.contains(&p) {
                        system_dirs.push(p);
                    }
                }
            }
        }
        WarnCtx { folders, vault, windows: cfg!(windows), system_dirs }
    }
}

fn comps(p: &Path) -> Vec<String> {
    p.to_string_lossy().replace('\\', "/").split('/').filter(|c| !c.is_empty()).map(|c| c.to_lowercase()).collect()
}

fn is_under(p: &[String], base: &[String]) -> bool {
    !base.is_empty() && p.len() >= base.len() && p.iter().zip(base.iter()).all(|(a, b)| a == b)
}

fn is_drive_root(p: &Path) -> bool {
    let c = comps(p);
    c.len() == 1 && c[0].ends_with(':') || (c.is_empty() && !p.as_os_str().is_empty())
}

const LINUX_SYSTEM: &[&str] = &["usr", "etc", "bin", "sbin", "lib", "lib64", "boot", "dev", "proc", "sys", "var", "run", "root", "srv", "snap"];

/// Le dossier est-il un dossier du systeme ?
pub fn is_system_dir(path: &Path, c: &WarnCtx) -> bool {
    let p = comps(path);
    if c.windows {
        if is_drive_root(path) {
            return true;
        }
        if c.system_dirs.iter().any(|s| is_under(&p, &comps(s))) {
            return true;
        }
        return p.iter().skip(1).any(|x| x == "$recycle.bin" || x == "system volume information" || x == "system32" || x == "winsxs");
    }
    if p.is_empty() {
        return true;
    }
    LINUX_SYSTEM.contains(&p[0].as_str()) && !is_under(&p, &["run".to_string(), "media".to_string()]) && !is_under(&p, &["var".to_string(), "lib".to_string(), "flatpak".to_string()])
}

/// Le dossier est-il beaucoup trop large pour etre un emplacement de sauvegarde ?
pub fn is_too_broad(path: &Path, c: &WarnCtx) -> bool {
    let p = comps(path);
    let f = c.folders;
    [&f.profile, &f.home, &f.local, &f.roaming, &f.locallow, &f.xdg_data, &f.xdg_config].into_iter().flatten().any(|b| comps(b) == p)
}

/// Tous les avertissements d'un dossier choisi, dans l'ordre d'importance.
pub fn warnings(path: &Path, pv: &Preview, c: &WarnCtx) -> Vec<Warning> {
    let mut v = Vec::new();
    if let Some(vault) = c.vault {
        if is_under(&comps(path), &comps(vault)) {
            v.push(Warning::IsVault);
        }
    }
    if is_system_dir(path, c) {
        v.push(Warning::System);
    } else if is_too_broad(path, c) {
        v.push(Warning::TooBroad);
    }
    if crate::sensitive::text_is_sensitive(&path.to_string_lossy()).is_some() {
        v.push(Warning::Sensitive);
    } else if pv.sensitive > 0 {
        v.push(Warning::SensitiveInside(pv.sensitive));
    }
    if pv.bytes >= HUGE_BYTES || (pv.truncated && pv.files as usize >= PREVIEW_MAX_FILES) {
        v.push(Warning::Huge { bytes: pv.bytes, at_least: pv.truncated });
    } else if crate::weight::is_heavy(pv.bytes) {
        v.push(Warning::Heavy { bytes: pv.bytes, at_least: pv.truncated });
    }
    if pv.exists && !pv.is_file && pv.files == 0 && !pv.truncated {
        v.push(Warning::Empty);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variantes_de_noms() {
        assert_eq!(name_variants("Team Cherry"), vec!["Team Cherry", "TeamCherry"]);
        assert_eq!(name_variants("Supergiant Games, Inc."), vec!["Supergiant Games, Inc", "Supergiant Games", "Supergiant", "SupergiantGames"]);
        assert!(name_variants("a").is_empty());
    }

    #[test]
    fn lecteur_racine() {
        assert!(is_drive_root(Path::new("C:\\")));
        assert!(is_drive_root(Path::new("D:")));
        assert!(!is_drive_root(Path::new("C:\\Users")));
    }
}
