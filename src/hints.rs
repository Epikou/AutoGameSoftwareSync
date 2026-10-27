//! Indices ecrits dans le meta.json des NOUVELLES sauvegardes (exe, nom du jeu, dossier de sauvegarde en forme
//! universelle) et leur reconnaissance sur un autre PC. Aucun nom d'utilisateur ni lettre de lecteur dans un indice :
//! le chemin n'est publie que s'il est sous un dossier connu de Windows (voir `KnownFolders`).
use std::path::{Component, Path, PathBuf};

/// Dossiers connus de CETTE machine (resolus par SHGetKnownFolderPath ; Documents peut etre redirige, ex. OneDrive).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnownFolders {
    pub local_app_data: Option<PathBuf>,
    pub roaming_app_data: Option<PathBuf>,
    pub local_low: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub saved_games: Option<PathBuf>,
}

impl KnownFolders {
    /// Linux : dossiers XDG (jeux natifs). Les jeux Windows sous Proton sont reconnus par `proton::template_from_prefix_path`.
    /// Hook de verification manuelle (`AGSS_KNOWLEDGE_ROOT`, voir `knowledge::paths::Folders::test_root`) : dossiers perso
    /// simules, pour verifier un nouveau PC (autre nom d'utilisateur) sans toucher aux vrais dossiers.
    fn simulated() -> Option<KnownFolders> {
        let root = crate::knowledge::paths::Folders::test_root()?;
        let f = crate::knowledge::paths::Folders::under_user(&root, &crate::knowledge::paths::Folders::test_user());
        Some(if cfg!(windows) {
            KnownFolders { local_app_data: f.local, roaming_app_data: f.roaming, local_low: f.locallow, documents: f.documents, saved_games: f.saved_games }
        } else {
            KnownFolders { local_app_data: f.xdg_data, roaming_app_data: f.xdg_config, local_low: None, documents: f.home.map(|h| h.join("Documents")), saved_games: None }
        })
    }

    #[cfg(not(windows))]
    pub fn real() -> KnownFolders {
        if let Some(k) = Self::simulated() {
            return k;
        }
        let home = dirs::home_dir().unwrap_or_default();
        let x = crate::proton::XdgDirs::from_env(&home, std::env::var("XDG_DATA_HOME").ok().as_deref(), std::env::var("XDG_CONFIG_HOME").ok().as_deref(), dirs::document_dir().as_deref());
        KnownFolders { local_app_data: Some(x.data), roaming_app_data: Some(x.config), local_low: None, documents: Some(x.documents), saved_games: None }
    }

    #[cfg(windows)]
    pub fn real() -> KnownFolders {
        use windows_sys::Win32::UI::Shell::*;
        if let Some(k) = Self::simulated() {
            return k;
        }
        KnownFolders {
            local_app_data: crate::winutil::known_folder(&FOLDERID_LocalAppData),
            roaming_app_data: crate::winutil::known_folder(&FOLDERID_RoamingAppData),
            local_low: crate::winutil::known_folder(&FOLDERID_LocalAppDataLow),
            documents: crate::winutil::known_folder(&FOLDERID_Documents),
            saved_games: crate::winutil::known_folder(&FOLDERID_SavedGames),
        }
    }

    fn bases(&self) -> Vec<(&'static str, &PathBuf)> {
        let mut v = Vec::new();
        for (ph, p) in [("%LOCALAPPDATA%", &self.local_app_data), ("%APPDATA%", &self.roaming_app_data), ("%LOCALLOW%", &self.local_low), ("%DOCUMENTS%", &self.documents), ("%SAVEDGAMES%", &self.saved_games)] {
            if let Some(p) = p {
                v.push((ph, p));
            }
        }
        v
    }
}

fn comps(p: &Path) -> Option<Vec<String>> {
    let mut v = Vec::new();
    for c in p.components() {
        match c {
            Component::Prefix(x) => v.push(x.as_os_str().to_string_lossy().to_lowercase()),
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => return None,
            Component::Normal(s) => v.push(s.to_string_lossy().to_string()),
        }
    }
    Some(v)
}

/// Chemin -> gabarit universel (`%LOCALAPPDATA%/Jeu/Saves`), seulement s'il est SOUS une base connue (la plus specifique
/// gagne) ; sinon None (D:\..., dossier d'installation...). Refuse `..` et le dossier de base lui-meme.
pub fn to_template(path: &Path, kf: &KnownFolders) -> Option<String> {
    #[cfg(not(windows))]
    {
        if let Some(t) = crate::proton::template_from_prefix_path(path) {
            return Some(t);
        }
    }
    let pc = comps(path)?;
    let mut best: Option<(usize, &'static str)> = None;
    for (ph, base) in kf.bases() {
        let bc = comps(base)?;
        if bc.is_empty() || pc.len() <= bc.len() {
            continue;
        }
        let same = bc.iter().zip(pc.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b));
        if same && best.map(|(n, _)| bc.len() > n).unwrap_or(true) {
            best = Some((bc.len(), ph));
        }
    }
    let (n, ph) = best?;
    let rest = &pc[n..];
    if rest.iter().any(|c| c.is_empty() || c.contains(':') || c.contains('%')) {
        return None;
    }
    Some(format!("{}/{}", ph, rest.join("/")))
}

/// Gabarit -> chemin de CETTE machine. Refuse tout ce qui n'est pas `%BASE%/composants` sans `..`, sans lecteur,
/// sans separateur en tete : le resultat reste forcement sous la base.
pub fn expand(tpl: &str, kf: &KnownFolders) -> Option<PathBuf> {
    let (ph, rest) = tpl.split_once('/')?;
    let base = kf.bases().into_iter().find(|(p, _)| *p == ph)?.1.clone();
    let parts: Vec<&str> = rest.split(['/', '\\']).collect();
    if parts.is_empty() || parts.iter().any(|c| c.is_empty() || *c == "." || *c == ".." || c.contains(':') || c.contains('%')) {
        return None;
    }
    let mut out = base.clone();
    for c in parts {
        out.push(c);
    }
    // Garde-fou final : le resultat reste sous la base.
    let (oc, bc) = (comps(&out)?, comps(&base)?);
    if oc.len() <= bc.len() || !bc.iter().zip(oc.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b)) {
        return None;
    }
    Some(out)
}

/// Nom de fichier seul d'un exe saisi (« C:\x\Jeu.exe » ou « Jeu.exe »).
pub fn exe_file_name(s: &str) -> String {
    s.trim().trim_matches('"').rsplit(['/', '\\']).next().unwrap_or("").trim().to_string()
}

/// Indice lu dans le meta.json de la derniere sauvegarde valide d'un jeu du dossier partage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub game_name: String,
    pub exe: String,
    pub save_path_tpl: Option<String>,
}

/// Jeux dont l'exe correspond (insensible a la casse, nom de fichier seul), sans doublon de nom de jeu.
pub fn matches<'a>(hints: &'a [Hint], exe_typed: &str) -> Vec<&'a Hint> {
    let want = exe_file_name(exe_typed);
    let mut out: Vec<&Hint> = Vec::new();
    if want.is_empty() {
        return out;
    }
    for h in hints {
        if h.exe.eq_ignore_ascii_case(&want) && !out.iter().any(|o| o.game_name == h.game_name) {
            out.push(h);
        }
    }
    out
}

/// Ligne de l'encart « Trouve sur un autre PC ».
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub game_name: String,
    /// Dossier de sauvegarde converti pour CE PC (None : pas d'indice de chemin ou chemin refuse).
    pub local_path: Option<PathBuf>,
    /// Le dossier existe-t-il deja sur ce PC ?
    pub exists: bool,
}

pub fn suggestions(hints: &[Hint], exe_typed: &str, kf: &KnownFolders, exists: &dyn Fn(&Path) -> bool) -> Vec<Suggestion> {
    suggestions_ext(hints, exe_typed, kf, &[], exists)
}

/// Comme `suggestions`, avec les prefixes Wine / Proton de la machine (Linux) : l'indice d'un programme Windows (`.exe`)
/// venu d'un autre PC (`%APPDATA%/Jeu`) designe le dossier correspondant DANS le prefixe ou il existe ; jamais le dossier
/// XDG natif (un .exe lance par Proton n'y ecrit pas). Sans prefixe (Windows), comportement de `suggestions`.
pub fn suggestions_ext(hints: &[Hint], exe_typed: &str, kf: &KnownFolders, prefixes: &[PathBuf], exists: &dyn Fn(&Path) -> bool) -> Vec<Suggestion> {
    let windows_exe = exe_file_name(exe_typed).to_lowercase().ends_with(".exe");
    matches(hints, exe_typed)
        .into_iter()
        .map(|h| {
            let local_path = if windows_exe && !prefixes.is_empty() {
                h.save_path_tpl.as_deref().and_then(|t| crate::proton::find_in_prefixes(t, prefixes).into_iter().next())
            } else {
                h.save_path_tpl.as_deref().and_then(|t| expand(t, kf))
            };
            let exists = local_path.as_deref().map(exists).unwrap_or(false);
            Suggestion { game_name: h.game_name.clone(), local_path, exists }
        })
        .collect()
}
