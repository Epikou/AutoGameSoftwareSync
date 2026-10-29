//! Android : la bibliotheque `rfd` (boites de dialogue du systeme) n'a pas de version Android. L'interface PC n'est jamais
//! affichee sur Android (l'application Android, dans `android/`, a ses propres ecrans) : ces boites repondent simplement
//! « rien choisi », ce qui laisse le code de l'interface PC compiler tel quel.
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct FileDialog;

impl FileDialog {
    pub fn new() -> FileDialog {
        FileDialog
    }
    pub fn add_filter<S: AsRef<str>>(self, _name: &str, _ext: &[S]) -> FileDialog {
        self
    }
    pub fn set_directory<P: AsRef<Path>>(self, _dir: P) -> FileDialog {
        self
    }
    pub fn set_file_name(self, _name: &str) -> FileDialog {
        self
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        None
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        None
    }
}
