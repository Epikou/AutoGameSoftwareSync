//! Formulaire « Ajout manuel » en trois gestes : logique PURE (validite, surbrillance du bouton, message conditionnel,
//! nom deduit, resolution d'un raccourci .lnk). Le dessin est dans `gui.rs`.
use crate::schedule::Schedule;
use std::path::{Path, PathBuf};

/// Etat de validite du formulaire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    /// Tout est valide : le bouton « Ajouter » est en surbrillance animee.
    Ready,
    NeedName,
    NeedPath,
    /// Sans exe, une planification valide est obligatoire.
    NeedSchedule,
}

pub struct FormIn<'a> {
    pub exe: &'a str,
    pub name: &'a str,
    pub path: &'a str,
    /// L'emplacement choisi existe (dossier ou fichier selon le choix).
    pub path_ok: bool,
    pub schedule: Option<&'a Schedule>,
}

/// Sans exe (aucune surveillance de processus), la planification est obligatoire.
pub fn schedule_required(exe: &str) -> bool {
    exe.trim().is_empty()
}

pub fn validity(f: &FormIn) -> Validity {
    if f.name.trim().is_empty() {
        Validity::NeedName
    } else if f.path.trim().is_empty() || !f.path_ok {
        Validity::NeedPath
    } else if schedule_required(f.exe) && !f.schedule.map(|s| s.is_valid()).unwrap_or(false) {
        Validity::NeedSchedule
    } else {
        Validity::Ready
    }
}

/// Le bouton « Ajouter » est cliquable : tout est valide, OU seule la planification manque (le clic explique alors pourquoi).
pub fn button_clickable(v: Validity) -> bool {
    matches!(v, Validity::Ready | Validity::NeedSchedule)
}

/// Surbrillance animee du bouton : uniquement quand tout est valide, quelle que soit la methode (avec exe, ou sans exe + planification).
pub fn button_highlighted(v: Validity) -> bool {
    v == Validity::Ready
}

/// Resultat d'un clic sur « Ajouter ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickResult {
    Add,
    /// Message « active une planification valide » + planification mise en evidence.
    ExplainSchedule,
    Nothing,
}

pub fn on_click(v: Validity) -> ClickResult {
    match v {
        Validity::Ready => ClickResult::Add,
        Validity::NeedSchedule => ClickResult::ExplainSchedule,
        _ => ClickResult::Nothing,
    }
}

/// Le message de planification n'est JAMAIS affiche en permanence : seulement apres un clic invalide, tant que le probleme persiste.
pub fn show_schedule_message(v: Validity, explained: bool) -> bool {
    explained && v == Validity::NeedSchedule
}

/// Nom du dossier partage deduit des informations de version de l'exe (ProductName, puis FileDescription), sinon du nom du fichier.
pub fn derive_name(product: Option<&str>, description: Option<&str>, exe_file_name: &str) -> String {
    // Un ProductName de systeme (« Microsoft Windows ») ne decrit pas le programme : la description passe en premier.
    let product = product.filter(|p| !(p.to_lowercase().contains("windows") && description.map(|d| !d.trim().is_empty()).unwrap_or(false)));
    for c in [product, description].into_iter().flatten() {
        let c = c.trim();
        if !c.is_empty() {
            return c.to_string();
        }
    }
    let stem = Path::new(exe_file_name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    stem.trim().to_string()
}

/// Extension acceptee par la zone de depot (.exe ou raccourci .lnk).
pub fn accepts_extension(p: &Path) -> bool {
    matches!(p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("exe") | Some("lnk"))
}

/// Cible d'un raccourci Windows (.lnk) : parcours du format binaire (LinkInfo, chemin local de base + suffixe).
pub fn parse_lnk(b: &[u8]) -> Option<PathBuf> {
    let u16le = |o: usize| b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]));
    let u32le = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    if u32le(0)? != 0x4C {
        return None;
    }
    let flags = u32le(0x14)?;
    let mut off = 0x4C_usize;
    if flags & 1 != 0 {
        off += 2 + u16le(off)? as usize;
    }
    if flags & 2 == 0 {
        return None;
    }
    let info = off;
    let hdr = u32le(info + 4)? as usize;
    if u32le(info + 8)? & 1 == 0 {
        return None; // pas de chemin local (cible reseau / Store)
    }
    let ansi = |o: usize| -> Option<String> {
        let s = b.get(o..)?;
        let end = s.iter().position(|c| *c == 0)?;
        Some(String::from_utf8_lossy(&s[..end]).to_string())
    };
    let wide = |o: usize| -> Option<String> {
        let s = b.get(o..)?;
        let mut v = Vec::new();
        for c in s.chunks_exact(2) {
            let u = u16::from_le_bytes([c[0], c[1]]);
            if u == 0 {
                return Some(String::from_utf16_lossy(&v));
            }
            v.push(u);
        }
        None
    };
    let (base, suffix) = if hdr >= 0x24 {
        (wide(info + u32le(info + 28)? as usize)?, wide(info + u32le(info + 32)? as usize).unwrap_or_default())
    } else {
        (ansi(info + u32le(info + 16)? as usize)?, ansi(info + u32le(info + 24)? as usize).unwrap_or_default())
    };
    let full = format!("{}{}", base, suffix);
    if full.trim().is_empty() { None } else { Some(PathBuf::from(full)) }
}

/// Fichier depose / choisi -> exe : un .exe tel quel, un .lnk resolu vers son .exe ; sinon None.
pub fn resolve_input(p: &Path) -> Option<PathBuf> {
    match p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("exe") => Some(p.to_path_buf()),
        Some("lnk") => {
            let t = parse_lnk(&std::fs::read(p).ok()?)?;
            if t.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false) { Some(t) } else { None }
        }
        _ => None,
    }
}
