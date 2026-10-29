//! COMPATIBILITE avec les noms d'une version anterieure. Ce module est le SEUL endroit du depot qui cite ces anciens noms
//! (dossier de donnees, variables d'environnement, taches planifiees, valeur de demarrage, nom d'exe, adresse de mise a jour).
//! Il peut etre supprime d'un bloc : la migration automatique disparait alors (migration manuelle unique), rien d'autre ne change.
//! Aucun texte visible par l'utilisateur ni message de journal ne cite ces noms.
use std::path::PathBuf;

/// Anciens dossiers de donnees (sous `%LOCALAPPDATA%`), par priorite decroissante.
pub const DATA_DIRS: [&str; 3] = ["AutoGameSaveSync", "GameSaveSync", "SaveSync"];
/// Anciens noms du dossier partage (coffre) crees par une version anterieure dans Google Drive, OneDrive... : un coffre
/// existant sous l'un de ces noms est propose EN PREMIER au premier lancement (jamais de coffre neuf vide a cote).
pub const VAULT_DIRS: [&str; 3] = ["AutoGameSaveSync", "GameSaveSync", "SaveSync"];
/// Anciennes variables d'environnement du dossier de donnees, par priorite decroissante.
pub const ENV_VARS: [&str; 3] = ["AUTOGAMESAVESYNC_DATA_DIR", "GAMESAVESYNC_DATA_DIR", "SAVESYNC_DATA_DIR"];
/// Anciens dossiers de taches planifiees.
pub const TASK_FOLDERS: [&str; 2] = ["GameSaveSync", "AutoGameSaveSync"];
/// Anciennes valeurs de la cle Run.
pub const RUN_VALUES: [&str; 3] = ["AutoGameSaveSync", "GameSaveSync", "SaveSync"];
/// Anciens noms d'exe (minuscules).
pub const EXE_NAMES: [&str; 3] = ["autogamesavesync.exe", "gamesavesync.exe", "savesync.exe"];
/// Ancienne adresse de l'API des releases (repli tant que le depot n'est pas renomme).
pub const UPDATE_URL: &str = "https://api.github.com/repos/Epikou/AutoGameSaveSync/releases/latest";
/// Ancien identifiant de la regle de demarrage de l'application (nom des taches et des reglages).
pub const APP_RULE_ID: &str = "gamesavesync";

/// Anciens dossiers de donnees existants ou non, dans l'ordre de priorite.
pub fn data_dirs() -> Vec<PathBuf> {
    match dirs::data_local_dir() {
        Some(base) => DATA_DIRS.iter().map(|d| base.join(d)).collect(),
        None => vec![],
    }
}

/// Dossier de donnees impose par une ancienne variable d'environnement.
pub fn data_dir_from_env() -> Option<PathBuf> {
    ENV_VARS.iter().find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from))
}

fn id_ok(id: &str) -> bool {
    matches!(id, APP_RULE_ID | "gdrive" | "onedrive" | "dropbox") || id.strip_prefix("custom-").map(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit())).unwrap_or(false)
}

/// Une tache est-elle une ANCIENNE tache a nous ? (ancien dossier + identifiant que nous avons cree ; toute autre tache est laissee)
pub fn is_task(name: &str) -> bool {
    let n = name.trim_start_matches('\\');
    TASK_FOLDERS.iter().any(|f| n.strip_prefix(&format!("{}\\", f)).map(id_ok).unwrap_or(false))
}

/// Identifiant de regle actuel correspondant a un ancien identifiant.
pub fn task_rule_id(id: &str) -> String {
    if id == APP_RULE_ID { crate::startup::ID_APP.to_string() } else { id.to_string() }
}

/// Reglages enregistres avec l'ancien identifiant de la regle de l'application : remis a jour.
pub fn fix_rule_ids(rules: &mut [crate::settings::StartupRuleCfg]) {
    let has_new = rules.iter().any(|r| r.id == crate::startup::ID_APP);
    for r in rules.iter_mut().filter(|r| r.id == APP_RULE_ID) {
        if !has_new {
            r.id = crate::startup::ID_APP.to_string();
        }
    }
}

/// Le nom d'exe (minuscules) est-il un ancien nom de notre programme ?
pub fn is_exe_name(lower: &str) -> bool {
    EXE_NAMES.contains(&lower)
}

/// Suffixe des fichiers temporaires d'ecriture de l'ancienne version (nettoyes comme les notres).
pub const TMP_SUFFIX: &str = ".savesync-tmp";
