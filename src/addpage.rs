//! Page « Ajouter » 2.0 (lot 11) : trois grandes tuiles, formulaire d'ajout en mots simples. Calcul PUR et teste : tuiles,
//! classement d'un element depose, nature du formulaire, nom propose, disposition des tuiles.
use std::path::{Path, PathBuf};

/// Les trois facons d'ajouter, dans l'ordre d'affichage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tile {
    /// « Chercher sur mon PC » : le scan (lot 9).
    Scan,
    /// « Un programme ou un jeu » : glisser-deposer ou choisir un programme, un raccourci, un lanceur.
    Program,
    /// « Un dossier ou un fichier » : selecteur guide (lot 8).
    Target,
}

pub const TILES: [Tile; 3] = [Tile::Scan, Tile::Program, Tile::Target];

/// Ce qu'on vient de deposer sur la fenetre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// Un dossier : formulaire « element » prerempli.
    Folder(PathBuf),
    /// Un programme (ou son raccourci / lanceur, deja resolu vers le programme).
    Program(PathBuf),
    /// Un autre fichier (une sauvegarde, un reglage) : formulaire « element » en mode fichier.
    File(PathBuf),
}

/// Classe un element depose. `resolve` transforme un raccourci / lanceur / programme en programme a surveiller (None si le
/// fichier n'en est pas un). Un dossier reste un dossier.
pub fn classify_drop(p: &Path, resolve: &dyn Fn(&Path) -> Option<PathBuf>) -> Dropped {
    if p.is_dir() {
        return Dropped::Folder(p.to_path_buf());
    }
    match resolve(p) {
        Some(e) => Dropped::Program(e),
        None => Dropped::File(p.to_path_buf()),
    }
}

/// Formulaire d'ajout affiche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    Hidden,
    /// Un programme est choisi : on cherche ses donnees.
    Program,
    /// Un dossier / fichier sans programme : copie planifiee.
    Target,
}

/// Nature du formulaire : un programme choisi => Program ; sinon, formulaire ouvert (dossier / fichier choisi, ou nom repris
/// de « Deja dans ton coffre ») => Target ; sinon rien.
pub fn form_kind(program: &str, open: bool) -> FormKind {
    if !program.trim().is_empty() {
        FormKind::Program
    } else if open {
        FormKind::Target
    } else {
        FormKind::Hidden
    }
}

/// Nom propose pour un dossier ou un fichier choisi : son dernier element (sans extension pour un fichier), l'initiale en
/// majuscule ; vide si le chemin n'a pas de nom (racine d'un disque).
pub fn target_name(p: &Path, is_file: bool) -> String {
    let n = if is_file { p.file_stem() } else { p.file_name() };
    let s = n.map(|x| x.to_string_lossy().trim().to_string()).unwrap_or_default();
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Nombre de colonnes des grandes tuiles selon la largeur disponible : trois cote a cote, sinon une par ligne.
pub fn tile_columns(width: f32) -> usize {
    if width >= 720.0 {
        3
    } else {
        1
    }
}

/// Largeur d'une tuile pour `cols` colonnes et un espace de `gap` (la ligne est entierement remplie).
pub fn tile_width(width: f32, cols: usize, gap: f32) -> f32 {
    let n = cols.max(1) as f32;
    ((width - gap * (n - 1.0)) / n).max(120.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trois_tuiles_dans_l_ordre() {
        assert_eq!(TILES, [Tile::Scan, Tile::Program, Tile::Target]);
    }

    #[test]
    fn nature_du_formulaire() {
        assert_eq!(form_kind("", false), FormKind::Hidden);
        assert_eq!(form_kind("  ", false), FormKind::Hidden);
        assert_eq!(form_kind("C:/x/jeu.exe", false), FormKind::Program);
        assert_eq!(form_kind("C:/x/jeu.exe", true), FormKind::Program);
        assert_eq!(form_kind("", true), FormKind::Target);
    }

    #[test]
    fn nom_propose() {
        assert_eq!(target_name(Path::new("C:/Users/a/Documents/mes notes"), false), "Mes notes");
        assert_eq!(target_name(Path::new("C:/Users/a/Documents/profil.dat"), true), "Profil");
        assert_eq!(target_name(Path::new("/"), false), "");
    }

    #[test]
    fn colonnes_et_largeur() {
        assert_eq!(tile_columns(900.0), 3);
        assert_eq!(tile_columns(559.0), 1);
        let w = tile_width(900.0, 3, 12.0);
        assert!((w * 3.0 + 24.0 - 900.0).abs() < 0.01);
        assert!(tile_width(100.0, 3, 12.0) >= 120.0);
    }
}
