//! Aucune migration automatique depuis une version anterieure : les tables ci-dessous sont vides.
//! Les fonctions gardent leur signature pour le reste du code et ne trouvent jamais rien a migrer.
use std::path::PathBuf;

/// Anciens dossiers de donnees (aucun).
pub const DATA_DIRS: [&str; 0] = [];
/// Anciens noms du dossier partage (aucun).
pub const VAULT_DIRS: [&str; 0] = [];
/// Anciennes variables d'environnement du dossier de donnees (aucune).
pub const ENV_VARS: [&str; 0] = [];
/// Anciens dossiers de taches planifiees (aucun).
pub const TASK_FOLDERS: [&str; 0] = [];
/// Anciennes valeurs de la cle Run (aucune).
pub const RUN_VALUES: [&str; 0] = [];
/// Anciens noms d'exe, en minuscules (aucun).
pub const EXE_NAMES: [&str; 0] = [];
/// Suffixe des fichiers temporaires d'ecriture : le notre.
pub const TMP_SUFFIX: &str = ".agss-tmp";

/// Anciens dossiers de donnees : aucun.
pub fn data_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// Dossier de donnees impose par une ancienne variable d'environnement : aucun.
pub fn data_dir_from_env() -> Option<PathBuf> {
    None
}

/// Une tache est-elle une ancienne tache a nous ? Jamais.
pub fn is_task(_name: &str) -> bool {
    false
}

/// Identifiant de regle actuel pour un identifiant de tache : le meme.
pub fn task_rule_id(id: &str) -> String {
    id.to_string()
}

/// Rien a remettre a jour dans les reglages.
pub fn fix_rule_ids(_rules: &mut [crate::settings::StartupRuleCfg]) {}

/// Le nom d'exe est-il un ancien nom de notre programme ? Jamais.
pub fn is_exe_name(_lower: &str) -> bool {
    false
}
