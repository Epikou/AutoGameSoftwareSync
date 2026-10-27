//! Dossier de donnees (`%LOCALAPPDATA%/AutoGameSoftwareSync`) et migration SURE depuis, par priorite, `AutoGameSoftwareSync`,
//! `AutoGameSoftwareSync` puis `AutoGameSoftwareSync` : COPIE verifiee (jamais deplacement, jamais suppression de l ancien),
//! sans rien ecraser si le nouveau dossier contient deja quelque chose.
use crate::{util, Res};
use std::path::{Path, PathBuf};

pub const APP_DIR: &str = "AutoGameSoftwareSync";
/// Marqueur ecrit dans le NOUVEAU dossier une fois la migration faite.
pub const MIGRATION_MARKER: &str = "migration.json";

/// Fichiers et dossiers migres (le reste, ex. verrous, est ignore).
const FILES: [&str; 5] = ["config.json", "settings.json", "state.json", "clock.json", "catalog.yaml"];
const DIRS: [&str; 2] = ["safety", "logs"];
/// Dossier impose par l environnement : `AUTOGAMESOFTWARESYNC_DATA_DIR` (prioritaire), sinon une ancienne variable (voir `legacy`).
/// Dossier impose par l'environnement : `AUTOGAMESOFTWARESYNC_DATA_DIR`, sinon `AUTOGAMESOFTWARESYNC_DATA_DIR`.
pub fn data_dir_from_env() -> Option<PathBuf> {
    std::env::var_os("AUTOGAMESOFTWARESYNC_DATA_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(crate::legacy::data_dir_from_env)
}

/// Linux : `$XDG_DATA_HOME/AutoGameSoftwareSync` (defaut `~/.local/share/AutoGameSoftwareSync`). Pure : variables passees en parametres.
pub fn xdg_data_dir(home: &Path, xdg_data_home: Option<&str>) -> PathBuf {
    crate::proton::XdgDirs::from_env(home, xdg_data_home, None, None).data.join(APP_DIR)
}

#[cfg(windows)]
pub fn default_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(APP_DIR))
}

#[cfg(not(windows))]
pub fn default_data_dir() -> Option<PathBuf> {
    Some(xdg_data_dir(&dirs::home_dir()?, std::env::var("XDG_DATA_HOME").ok().as_deref()))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Nombre de fichiers copies et verifies.
    pub files: usize,
    /// Raison pour laquelle rien n'a ete fait (None = migration effectuee).
    pub skipped: Option<String>,
}

fn dir_is_nonempty(p: &Path) -> bool {
    std::fs::read_dir(p).map(|mut r| r.next().is_some()).unwrap_or(false)
}

fn copy_verified_file(src: &Path, dst: &Path) -> Res<()> {
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::copy(src, dst).map_err(|e| format!("copie {} : {}", src.display(), e))?;
    if std::fs::metadata(src)?.len() != std::fs::metadata(dst)?.len() || util::hash_file(src)? != util::hash_file(dst)? {
        return Err(format!("verification echouee pour {}", dst.display()).into());
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Res<usize> {
    let mut n = 0;
    for e in walkdir::WalkDir::new(src).follow_links(false) {
        let e = e.map_err(|e| e.to_string())?;
        let rel = e.path().strip_prefix(src)?;
        if e.file_type().is_dir() {
            std::fs::create_dir_all(dst.join(rel))?;
        } else if e.file_type().is_file() {
            copy_verified_file(e.path(), &dst.join(rel))?;
            n += 1;
        }
    }
    Ok(n)
}

/// Copie `old` vers `new`. N'ecrase JAMAIS : si `new` existe et n'est pas vide, ou porte deja le marqueur,
/// ne fait rien. Construit dans un dossier voisin puis le renomme (en cas d'echec, seul ce dossier de
/// travail est supprime). L'ancien dossier n'est jamais modifie.
pub fn migrate(old: &Path, new: &Path) -> Res<MigrationReport> {
    let skip = |why: &str| Ok(MigrationReport { files: 0, skipped: Some(why.to_string()) });
    if new.join(MIGRATION_MARKER).exists() {
        return skip("deja migre");
    }
    if !old.is_dir() {
        return skip("ancien dossier absent");
    }
    if dir_is_nonempty(new) {
        return skip("le nouveau dossier n'est pas vide : rien n'est ecrase");
    }
    let has_any = FILES.iter().any(|f| old.join(f).is_file()) || DIRS.iter().any(|d| old.join(d).is_dir());
    if !has_any {
        return skip("rien a migrer");
    }
    let parent = new.parent().ok_or("dossier parent introuvable")?;
    std::fs::create_dir_all(parent)?;
    let work = parent.join(format!("{}.migrating", new.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| APP_DIR.into())));
    let _ = std::fs::remove_dir_all(&work); // reste d'une tentative precedente (notre dossier de travail)
    let run = (|| -> Res<usize> {
        std::fs::create_dir_all(&work)?;
        let mut n = 0;
        for f in FILES {
            if old.join(f).is_file() {
                copy_verified_file(&old.join(f), &work.join(f))?;
                n += 1;
            }
        }
        for d in DIRS {
            if old.join(d).is_dir() {
                n += copy_tree(&old.join(d), &work.join(d))?;
            }
        }
        let marker = serde_json::json!({ "files": n, "at": util::timestamp(util::now_ms()), "note": "copie verifiee ; l'ancien dossier n'a pas ete modifie et peut etre supprime manuellement apres verification" });
        std::fs::write(work.join(MIGRATION_MARKER), serde_json::to_string_pretty(&marker)?)?;
        Ok(n)
    })();
    match run {
        Ok(n) => {
            if new.exists() {
                let _ = std::fs::remove_dir(new); // dossier vide uniquement (remove_dir echoue sinon)
            }
            if let Err(e) = std::fs::rename(&work, new) {
                let _ = std::fs::remove_dir_all(&work);
                return Err(format!("installation du nouveau dossier impossible : {}", e).into());
            }
            Ok(MigrationReport { files: n, skipped: None })
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&work);
            Err(e)
        }
    }
}

/// Ligne de journal pour une migration.
pub fn report_line(_old: &Path, r: &MigrationReport) -> String {
    match &r.skipped {
        None => format!("[Migration] {} fichier(s) copies et verifies depuis un ancien dossier (l ancien dossier est intact : vous pouvez le supprimer a la main apres verification)", r.files),
        Some(why) => format!("[Migration] non effectuee : {}", why),
    }
}


/// Anciens dossiers de donnees (voir `legacy`), par priorite.
pub fn old_data_dirs() -> Vec<PathBuf> {
    crate::legacy::data_dirs()
}

/// Premier dossier existant de la liste (ordre = priorite).
pub fn pick_first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_dir()).cloned()
}

#[cfg(test)]
mod xdg_tests {
    use super::*;

    #[test]
    fn linux_data_dir_follows_xdg() {
        let home = Path::new("/home/deck");
        assert_eq!(xdg_data_dir(home, None), PathBuf::from("/home/deck/.local/share/AutoGameSoftwareSync"));
        assert_eq!(xdg_data_dir(home, Some("/data")), PathBuf::from("/data/AutoGameSoftwareSync"));
        // une valeur relative ou vide est ignoree (regle XDG)
        assert_eq!(xdg_data_dir(home, Some("")), PathBuf::from("/home/deck/.local/share/AutoGameSoftwareSync"));
        assert_eq!(xdg_data_dir(home, Some("relatif/x")), PathBuf::from("/home/deck/.local/share/AutoGameSoftwareSync"));
    }
}
