//! Profils communautaires : fichiers JSON versionnes (schema 1) decrivant des emplacements a synchroniser pour des jeux
//! exotiques, des applications, des emulateurs ou des configurations. Embarques dans l'exe (`data/profiles/`) et/ou lus
//! depuis `<dossier de donnees>\profiles\*.json` (valides ; erreurs claires). Aucun profil n'est actif par defaut.
//! La liste noire dure (`sensitive`) interdit tout identifiant, cle, jeton, cookie, mot de passe et profil de navigateur.
use crate::config::{Game, Location};
use crate::hints::KnownFolders;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;
pub const CATEGORIES: &[&str] = &["game", "app", "emulator", "config"];
/// Taille maximale d'un fichier de profil personnalise.
pub const MAX_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_LOCATIONS: usize = 8;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Detection {
    /// Noms d'exe du programme (surveillance de sa fermeture).
    #[serde(default)]
    pub exes: Vec<String>,
    /// Gabarits de dossiers dont l'existence indique que le programme est installe.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Variable a choisir ou a detecter (`<install>`, `<version>`, `<project>`...).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ProfVar {
    pub name: String,
    pub label: String,
    /// Gabarits de dossiers candidats (la valeur est le dossier lui-meme).
    #[serde(default)]
    pub candidates: Vec<String>,
    /// Gabarit `...\*` : les sous-dossiers existants sont proposes (la valeur est le NOM du sous-dossier).
    #[serde(default)]
    pub scan: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ProfLocation {
    pub label: String,
    /// Gabarit : `%LOCALAPPDATA%`, `%APPDATA%`, `%USERPROFILE%`, `%DOCUMENTS%`, `%SAVEDGAMES%`, `%LOCALLOW%`, `%PROGRAMFILES%`,
    /// `%PROGRAMFILES(X86)%` et `<variable>`.
    pub path: String,
    #[serde(default)]
    pub is_file: bool,
    /// Motifs inclus (vide = tout le dossier).
    #[serde(default)]
    pub include: Vec<String>,
    /// Motifs exclus (nom de fichier ou de dossier a n'importe quel niveau, ou chemin relatif).
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Coche par defaut dans l'interface (decocher les emplacements sensibles ou lourds).
    #[serde(default = "yes")]
    pub default_enabled: bool,
    #[serde(default)]
    pub note: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Profile {
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// game | app | emulator | config
    pub category: String,
    pub version: String,
    pub author: String,
    pub license: String,
    #[serde(default)]
    pub detection: Detection,
    #[serde(default)]
    pub variables: Vec<ProfVar>,
    pub locations: Vec<ProfLocation>,
    /// Avertissements lisibles, a valider avant activation d'un profil app / config.
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Niveau de confiance des chemins : high | medium | low.
    #[serde(default)]
    pub confidence: Option<String>,
    /// Date de verification des chemins (AAAA-MM-JJ).
    #[serde(default)]
    pub verified: Option<String>,
    /// Faut-il PROPOSER ce programme quand il se lance (detection automatique) ? Defaut : oui pour les profils embarques, non pour
    /// les profils personnalises (a activer explicitement avec `"suggest": true`). Jamais une application sans profil.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggest: Option<bool>,
}

impl Profile {
    /// Une application ou une configuration demande une validation explicite de l'avertissement avant ajout.
    pub fn needs_ack(&self) -> bool {
        matches!(self.category.as_str(), "app" | "config")
    }
}

// ------------------------------------------------------------------ validation

const TOKENS: &[&str] = &["%localappdata%", "%appdata%", "%userprofile%", "%documents%", "%savedgames%", "%locallow%", "%programfiles%", "%programfiles(x86)%"];

fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() >= 2 && b.len() <= 64 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Tous les problemes d'un profil (liste vide = valide). Liste noire dure incluse.
pub fn validate(p: &Profile) -> Vec<String> {
    let mut e: Vec<String> = Vec::new();
    if p.schema != SCHEMA {
        e.push(format!("schema {} non pris en charge (attendu {})", p.schema, SCHEMA));
    }
    if !valid_id(&p.id) {
        e.push("id invalide (minuscules, chiffres et tirets, 2 a 64 caracteres)".into());
    }
    for (n, v) in [("name", &p.name), ("version", &p.version), ("author", &p.author), ("license", &p.license)] {
        if v.trim().is_empty() {
            e.push(format!("champ `{}` vide", n));
        }
    }
    if !CATEGORIES.contains(&p.category.as_str()) {
        e.push(format!("category `{}` inconnue (game, app, emulator, config)", p.category));
    }
    if p.needs_ack() && p.warnings.iter().all(|w| w.trim().is_empty()) {
        e.push("un profil app / config doit avoir au moins un avertissement lisible (`warnings`)".into());
    }
    if let Some(c) = &p.confidence {
        if !["high", "medium", "low"].contains(&c.as_str()) {
            e.push("confidence doit valoir high, medium ou low".into());
        }
    }
    if let Some(d) = &p.verified {
        let ok = d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-' && d.bytes().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
        if !ok {
            e.push("verified doit etre une date AAAA-MM-JJ".into());
        }
    }
    if p.locations.is_empty() || p.locations.len() > MAX_LOCATIONS {
        e.push(format!("1 a {} emplacements attendus", MAX_LOCATIONS));
    }
    let var_names: Vec<String> = p.variables.iter().map(|v| v.name.to_lowercase()).collect();
    for v in &p.variables {
        if v.name.is_empty() || !v.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            e.push(format!("variable `{}` : nom invalide", v.name));
        }
        for t in v.candidates.iter().chain(v.scan.iter()) {
            if let Some(r) = crate::sensitive::text_is_sensitive(t) {
                e.push(format!("variable `{}` : {}", v.name, r));
            }
        }
    }
    for t in p.detection.paths.iter() {
        if let Some(r) = crate::sensitive::text_is_sensitive(t) {
            e.push(format!("detection : {}", r));
        }
    }
    for (i, l) in p.locations.iter().enumerate() {
        let who = format!("emplacement {} (« {} »)", i + 1, l.label);
        if l.label.trim().is_empty() {
            e.push(format!("emplacement {} : `label` vide", i + 1));
        }
        let t = l.path.trim();
        if t.is_empty() {
            e.push(format!("{} : `path` vide", who));
            continue;
        }
        let low = t.to_lowercase().replace('\\', "/");
        let starts_ok = TOKENS.iter().any(|k| low.starts_with(k)) || low.starts_with('<');
        if !starts_ok {
            e.push(format!("{} : le chemin doit commencer par un gabarit (%LOCALAPPDATA%, %APPDATA%, %USERPROFILE%, %DOCUMENTS%, %SAVEDGAMES%, %LOCALLOW%, %PROGRAMFILES%, <variable>) : jamais de chemin absolu ni de nom d'utilisateur", who));
        }
        if low.split('/').any(|c| c == "..") {
            e.push(format!("{} : `..` interdit", who));
        }
        // Variables inconnues.
        let mut rest = low.as_str();
        while let Some(i) = rest.find('<') {
            if let Some(j) = rest[i..].find('>') {
                let name = &rest[i + 1..i + j];
                if !var_names.contains(&name.to_string()) {
                    e.push(format!("{} : variable `<{}>` non declaree dans `variables`", who, name));
                }
                rest = &rest[i + j + 1..];
            } else {
                e.push(format!("{} : `<` sans `>`", who));
                break;
            }
        }
        // Trop large : le gabarit seul, ou la racine d'une variable seule sans sous-dossier ni filtre.
        let comps: Vec<&str> = low.split('/').filter(|c| !c.is_empty()).collect();
        if comps.len() < 2 && l.include.is_empty() && !var_names.iter().any(|v| low == format!("<{}>", v)) {
            e.push(format!("{} : emplacement trop large (ajoutez un sous-dossier ou des motifs `include`)", who));
        }
        if comps.len() < 2 && low.starts_with('%') {
            e.push(format!("{} : emplacement trop large (un dossier de base entier n'est jamais accepte)", who));
        }
        if let Some(r) = crate::sensitive::text_is_sensitive(&l.path) {
            e.push(format!("{} : chemin refuse : {}", who, r));
        }
        for inc in &l.include {
            if let Some(r) = crate::sensitive::text_is_sensitive(inc) {
                e.push(format!("{} : motif inclus `{}` refuse : {}", who, inc, r));
            }
        }
    }
    e
}

// ------------------------------------------------------------------ gabarits

/// Valeurs servant a deplier les gabarits.
#[derive(Clone, Debug, Default)]
pub struct Vars {
    pub kf: KnownFolders,
    pub user_profile: Option<PathBuf>,
    pub program_files: Option<PathBuf>,
    pub program_files_x86: Option<PathBuf>,
    /// Valeurs des variables du profil (`install`, `version`...).
    pub values: BTreeMap<String, String>,
    /// Linux : prefixes Wine / Proton ou chercher un emplacement Windows absent en natif (vide sous Windows).
    pub wine_prefixes: Vec<PathBuf>,
}

impl Vars {
    #[cfg(windows)]
    pub fn real() -> Vars {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Vars { kf: KnownFolders::real(), user_profile: env("USERPROFILE"), program_files: env("ProgramFiles"), program_files_x86: env("ProgramFiles(x86)"), values: BTreeMap::new(), wine_prefixes: Vec::new() }
    }

    #[cfg(not(windows))]
    pub fn real() -> Vars {
        let env = |k: &str| if k == "USERPROFILE" { dirs::home_dir() } else { std::env::var_os(k).map(PathBuf::from) };
        Vars { kf: KnownFolders::real(), user_profile: env("USERPROFILE"), program_files: env("ProgramFiles"), program_files_x86: env("ProgramFiles(x86)"), values: BTreeMap::new(), wine_prefixes: crate::proton::wine_prefixes_cached() }
    }
}

fn replace_ci(s: &str, token: &str, with: &str) -> String {
    let low = s.to_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while let Some(p) = low[i..].find(token) {
        out.push_str(&s[i..i + p]);
        out.push_str(with);
        i += p + token.len();
    }
    out.push_str(&s[i..]);
    out
}

/// Deplie un gabarit ; None si une base ou une variable n'est pas disponible.
pub fn expand(tpl: &str, v: &Vars) -> Option<PathBuf> {
    let bases: [(&str, Option<&PathBuf>); 8] = [
        ("%localappdata%", v.kf.local_app_data.as_ref()),
        ("%appdata%", v.kf.roaming_app_data.as_ref()),
        ("%userprofile%", v.user_profile.as_ref()),
        ("%documents%", v.kf.documents.as_ref()),
        ("%savedgames%", v.kf.saved_games.as_ref()),
        ("%locallow%", v.kf.local_low.as_ref()),
        ("%programfiles(x86)%", v.program_files_x86.as_ref()),
        ("%programfiles%", v.program_files.as_ref()),
    ];
    let mut s = tpl.to_string();
    for (tok, val) in bases {
        if s.to_lowercase().contains(tok) {
            s = replace_ci(&s, tok, &val?.to_string_lossy());
        }
    }
    for (k, val) in &v.values {
        s = replace_ci(&s, &format!("<{}>", k.to_lowercase()), val);
    }
    if s.contains('%') || s.contains('<') {
        return None;
    }
    #[cfg(windows)]
    return Some(PathBuf::from(s.replace('/', "\\")));
    #[cfg(not(windows))]
    {
        let native = PathBuf::from(s.replace('\\', "/"));
        // Linux : un emplacement Windows (%APPDATA%...) absent en natif est cherche dans les prefixes Proton / Wine.
        if !native.exists() && !v.wine_prefixes.is_empty() {
            let low = tpl.trim().to_lowercase();
            if ["%localappdata%", "%appdata%", "%locallow%", "%documents%", "%savedgames%", "%userprofile%"].iter().any(|p| low.starts_with(p)) {
                let mut t = tpl.replace('\\', "/");
                for (k, val) in &v.values {
                    t = replace_ci(&t, &format!("<{}>", k.to_lowercase()), val);
                }
                if let Some(x) = crate::proton::find_in_prefixes(&t, &v.wine_prefixes).into_iter().next() {
                    return Some(x);
                }
            }
        }
        Some(native)
    }
}

/// Valeurs candidates d'une variable : dossiers existants (`candidates`) et sous-dossiers (`scan`).
pub fn var_candidates(var: &ProfVar, v: &Vars) -> Vec<String> {
    let mut out = Vec::new();
    for c in &var.candidates {
        if let Some(p) = expand(c, v) {
            if p.is_dir() {
                out.push(p.to_string_lossy().to_string());
            }
        }
    }
    if let Some(s) = &var.scan {
        let base = s.trim_end_matches(['*', '\\', '/']);
        if let Some(dir) = expand(base, v) {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                let mut names: Vec<String> = rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
                names.sort();
                names.reverse();
                out.extend(names);
            }
        }
    }
    out
}

/// Le programme est-il present sur ce PC ? (un dossier de detection existe, ou un emplacement sans variable existe)
pub fn detected(p: &Profile, v: &Vars) -> bool {
    if p.detection.paths.iter().any(|t| expand(t, v).map(|x| x.exists()).unwrap_or(false)) {
        return true;
    }
    p.locations.iter().filter(|l| !l.path.contains('<')).any(|l| expand(&l.path, v).map(|x| x.exists()).unwrap_or(false))
}

/// Identifiant d'emplacement STABLE et identique sur tous les PC (profil + nom de l'emplacement).
pub fn location_id(profile_id: &str, label: &str) -> String {
    crate::util::hash_bytes(format!("{}|{}", profile_id, label.to_lowercase()).as_bytes()).chars().take(6).collect()
}

/// Cree le jeu correspondant : premier emplacement coche = principal, les autres = emplacements supplementaires.
pub fn to_game(p: &Profile, v: &Vars, enabled: &[bool]) -> Result<Game, String> {
    let mut chosen: Vec<(&ProfLocation, PathBuf)> = Vec::new();
    for (i, l) in p.locations.iter().enumerate() {
        if enabled.get(i).copied().unwrap_or(l.default_enabled) {
            let path = expand(&l.path, v).ok_or_else(|| format!("emplacement « {} » : variable ou dossier de base indisponible", l.label))?;
            chosen.push((l, path));
        }
    }
    let Some((first, fpath)) = chosen.first().cloned() else { return Err("aucun emplacement coche".into()) };
    let join = |v: &[String]| Some(v.join(";")).filter(|s| !s.is_empty());
    let mut g = Game {
        name: p.name.clone(),
        path: fpath,
        exe: p.detection.exes.first().cloned(),
        save_is_file: first.is_file,
        save_filter: if first.is_file { None } else { join(&first.include) },
        save_exclude: if first.is_file { None } else { join(&first.exclude) },
        primary_label: Some(first.label.clone()),
        ..Default::default()
    };
    for (l, path) in chosen.iter().skip(1) {
        g.extra_locations.push(Location {
            id: location_id(&p.id, &l.label),
            label: l.label.clone(),
            path: path.clone(),
            is_file: l.is_file,
            filter: if l.is_file { None } else { join(&l.include) },
            exclude: if l.is_file { None } else { join(&l.exclude) },
            enabled: true,
        });
    }
    Ok(g)
}

// ------------------------------------------------------------------ chargement

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError {
    pub file: String,
    pub message: String,
}

/// Analyse + validation d'un texte JSON de profil. Erreurs lisibles (ligne / colonne, champs).
pub fn parse(text: &str) -> Result<Profile, String> {
    let p: Profile = serde_json::from_str(text).map_err(|e| format!("JSON invalide (ligne {}, colonne {}) : {}", e.line(), e.column(), e))?;
    let errs = validate(&p);
    if errs.is_empty() {
        Ok(p)
    } else {
        Err(errs.join(" ; "))
    }
}

/// Profils embarques dans l'exe (ceux qui sont invalides sont ignores : un test garantit qu'il n'y en a aucun).
/// Lus et valides une seule fois par processus (l'accueil les relit a chaque rafraichissement des etats).
pub fn load_embedded() -> Vec<Profile> {
    static CACHE: std::sync::OnceLock<Vec<Profile>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| crate::profiles_data::EMBEDDED.iter().filter_map(|(_, t)| parse(t).ok()).collect()).clone()
}

/// Profils personnalises : `<dossier de donnees>\profiles\*.json`. Un profil invalide est rejete avec un message clair ;
/// un profil personnalise ne peut pas remplacer un profil embarque (meme `id`).
pub fn load_custom(data_dir: &Path, embedded: &[Profile]) -> (Vec<Profile>, Vec<LoadError>) {
    let dir = data_dir.join("profiles");
    let mut ok: Vec<Profile> = Vec::new();
    let mut errs = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return (ok, errs) };
    let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("json")).unwrap_or(false)).collect();
    files.sort();
    for f in files {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let md = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        if md > MAX_FILE_BYTES {
            errs.push(LoadError { file: name, message: format!("fichier trop gros ({} Ko, maximum {} Ko)", md / 1024, MAX_FILE_BYTES / 1024) });
            continue;
        }
        let text = match std::fs::read_to_string(&f) {
            Ok(t) => t,
            Err(e) => {
                errs.push(LoadError { file: name, message: format!("lecture impossible : {}", e) });
                continue;
            }
        };
        match parse(&text) {
            Ok(p) => {
                if embedded.iter().any(|e| e.id == p.id) {
                    errs.push(LoadError { file: name, message: format!("l'id `{}` est deja utilise par un profil embarque (non remplacable)", p.id) });
                } else if ok.iter().any(|o| o.id == p.id) {
                    errs.push(LoadError { file: name, message: format!("l'id `{}` est deja utilise par un autre fichier", p.id) });
                } else {
                    ok.push(p);
                }
            }
            Err(m) => errs.push(LoadError { file: name, message: m }),
        }
    }
    (ok, errs)
}

// ------------------------------------------------------------------ view-model de la section « Profils »

/// Indices des profils a afficher : filtre par categorie et recherche (nom, id, categorie, notes), detectes d'abord puis par nom.
pub fn filter_rows(profiles: &[Profile], query: &str, category: Option<&str>, is_detected: &dyn Fn(&Profile) -> bool) -> Vec<usize> {
    let q = crate::gamedb::norm(query);
    let mut rows: Vec<(bool, String, usize)> = profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| category.map(|c| p.category == c).unwrap_or(true))
        .filter(|(_, p)| q.is_empty() || crate::gamedb::norm(&format!("{} {} {} {}", p.name, p.id, p.category, p.notes.clone().unwrap_or_default())).contains(&q))
        .map(|(i, p)| (!is_detected(p), p.name.to_lowercase(), i))
        .collect();
    rows.sort();
    rows.into_iter().map(|r| r.2).collect()
}

/// Index exe (minuscules) -> indices du profil pour le score de detection (`gamedetect::assess_profile`) : identifiant, nom,
/// application ?, programme installe sur ce PC (`detected`).
pub fn detection_hints(profiles: &[Profile], default_suggest: bool, v: &Vars) -> BTreeMap<String, crate::gamedetect::ProfileHint> {
    detection_index(profiles, default_suggest)
        .into_iter()
        .map(|(exe, (id, name, app))| {
            let installed = profiles.iter().find(|p| p.id == id).map(|p| detected(p, v)).unwrap_or(false);
            (exe, crate::gamedetect::ProfileHint { id, name, app, installed })
        })
        .collect()
}

/// Index exe (minuscules) -> (id, nom, application ou configuration ?) des profils reconnaissables par leur exe ; les noms trop generiques (`javaw.exe`...) sont ignores.
pub fn detection_index(profiles: &[Profile], default_suggest: bool) -> BTreeMap<String, (String, String, bool)> {
    let mut m = BTreeMap::new();
    for p in profiles.iter().filter(|p| p.suggest.unwrap_or(default_suggest)) {
        for e in &p.detection.exes {
            let k = e.to_lowercase();
            // Un profil curé peut cibler un exe « générique » explicitement autorisé (éditeur, terminal) ; jamais un runtime.
            if !crate::gamedetect::GENERIC_EXES.contains(&k.as_str()) || crate::gamedetect::PROFILE_OK_EXES.contains(&k.as_str()) {
                m.entry(k).or_insert_with(|| (p.id.clone(), p.name.clone(), p.needs_ack()));
            }
        }
    }
    m
}
