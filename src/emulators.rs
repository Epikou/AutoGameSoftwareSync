//! Emulateurs partages entre le PC et Android (lot 17) : UNE table decrit, pour chaque emulateur, le profil PC embarque
//! (`data/profiles/<id>.json`) et les dossiers correspondants sur une console Android. L'element cree sur Android reutilise
//! le profil PC tel quel (meme `name`, memes libelles d'emplacements, donc meme cle dans le coffre et memes sous-espaces
//! `files/@<id>/`) : une partie commencee sur le PC continue sur la console, et inversement, sans aucune correspondance a
//! configurer a la main. Seuls les chemins changent (`<root>` = dossier de l'emulateur sur la console).
//!
//! Rien ici ne lit Internet ni ne depend d'une interface : fonctions pures + lecture de dossiers, testees sous Windows et Linux.
use crate::config::Game;
use crate::profiles::{self, ProfLocation, ProfVar, Profile, Vars};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Compatibilite des FICHIERS entre la version PC et la version Android d'un emulateur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compat {
    /// Memes fichiers des deux cotes : une partie continue de l'un a l'autre.
    Same,
    /// Sauvegardes compatibles ; les etats instantanes (save states) dependent de la version exacte de l'emulateur.
    SavesOnly,
    /// Compatible seulement sous conditions (voir la note) : a verifier sur une vraie console.
    Partial,
}

impl Compat {
    pub fn code(self) -> &'static str {
        match self {
            Compat::Same => "same",
            Compat::SavesOnly => "saves-only",
            Compat::Partial => "partial",
        }
    }
}

/// Un emplacement du profil, vu depuis la console : `rel` est le sous-dossier sous `<root>` (separateur `/`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AndroidLoc {
    /// Libelle EXACT de l'emplacement du profil PC (meme ordre que dans le JSON).
    pub label: &'static str,
    pub rel: &'static str,
}

/// Un emulateur connu des deux cotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Emu {
    /// Identifiant du profil PC embarque (`data/profiles/<id>.json`).
    pub id: &'static str,
    /// Nom montre sur la console (le nom du profil PC reste la cle du coffre).
    pub display: &'static str,
    /// Dossiers candidats de l'emulateur, relatifs a la racine d'un stockage Android (`/storage/emulated/0`, carte SD).
    /// Vide : l'emulateur range tout dans `Android/data` (inaccessible) ; il n'est pas propose.
    pub android_roots: &'static [&'static str],
    /// Emplacements, dans l'ordre du profil PC.
    pub locations: &'static [AndroidLoc],
    pub compat: Compat,
    /// Note courte (anglais, documentation et ecran d'aide) : ou l'emulateur Android range ses fichiers, ce qu'il faut regler.
    pub note: &'static str,
}

/// Nom de la variable qui porte le dossier de l'emulateur sur la console.
pub const ROOT_VAR: &str = "root";

pub const EMULATORS: &[Emu] = &[
    Emu {
        id: "retroarch",
        display: "RetroArch",
        android_roots: &["RetroArch"],
        locations: &[AndroidLoc { label: "Saves (SRAM, memory cards)", rel: "saves" }, AndroidLoc { label: "Save states", rel: "states" }],
        compat: Compat::SavesOnly,
        note: "Android: /storage/emulated/0/RetroArch/saves and /states (default since RetroArch 1.9). Same cores on PC and Android: .srm files are identical; save states need the same core version on both sides. Keep the same 'sort saves into folders' settings on both.",
    },
    Emu {
        id: "ppsspp",
        display: "PPSSPP",
        android_roots: &["PSP", "PPSSPP/PSP"],
        locations: &[AndroidLoc { label: "Save data", rel: "SAVEDATA" }, AndroidLoc { label: "Save states", rel: "PPSSPP_STATE" }],
        compat: Compat::Same,
        note: "Android: /storage/emulated/0/PSP (memory stick). Recent PPSSPP versions default to Android/data: in PPSSPP go to Settings > System > Memory Stick folder and pick a visible folder (for example /storage/emulated/0/PSP). Save data and states are the same files as on PC.",
    },
    Emu {
        id: "duckstation",
        display: "DuckStation",
        android_roots: &["duckstation"],
        locations: &[AndroidLoc { label: "Memory cards", rel: "memcards" }, AndroidLoc { label: "Save states", rel: "savestates" }],
        compat: Compat::SavesOnly,
        note: "Android: /storage/emulated/0/duckstation/memcards. Memory cards (.mcd) are identical on PC and Android; save states depend on the DuckStation version.",
    },
    Emu {
        id: "pcsx2",
        display: "AetherSX2 / NetherSX2",
        android_roots: &["AetherSX2", "NetherSX2", "aethersx2", "nethersx2"],
        locations: &[AndroidLoc { label: "Memory cards", rel: "memcards" }, AndroidLoc { label: "Save states", rel: "sstates" }],
        compat: Compat::Partial,
        note: "AetherSX2 / NetherSX2 keep their data in Android/data by default (not readable): set System > Data directory to a visible folder named AetherSX2 or NetherSX2 at the root of the storage. 8 MB memory cards (.ps2) are the same format as PCSX2 on PC; folder memory cards and save states are not interchangeable.",
    },
    Emu {
        id: "dolphin",
        display: "Dolphin",
        android_roots: &["dolphin-emu", "mmjr", "dolphin-mmjr"],
        locations: &[AndroidLoc { label: "GameCube memory cards", rel: "GC" }, AndroidLoc { label: "Wii saves", rel: "Wii/title" }, AndroidLoc { label: "Save states", rel: "StateSaves" }],
        compat: Compat::SavesOnly,
        note: "Official Dolphin on Android 11+ stores everything in Android/data (not readable). Only builds that use /storage/emulated/0/dolphin-emu (older versions, MMJR forks) can be protected. GameCube memory cards (.raw) and Wii saves are the same files as on PC; save states are not portable.",
    },
    Emu {
        id: "azahar",
        display: "Azahar / Lime3DS / Citra",
        android_roots: &["azahar", "lime3ds", "citra-emu"],
        locations: &[AndroidLoc { label: "Save data (SD card)", rel: "sdmc/Nintendo 3DS" }, AndroidLoc { label: "Save states", rel: "states" }],
        compat: Compat::Partial,
        note: "When the Android app asks for a user folder at first launch, pick a folder named azahar (or citra-emu) at the root of the storage. The SD card tree is the same layout on PC and Android, but it contains console-specific folder names: a game saved on one side is found by the other only if both use the same console id (copy config/sdmc from one side the first time). To verify on a real device.",
    },
];

/// Emulateur par identifiant de profil.
pub fn by_id(id: &str) -> Option<&'static Emu> {
    EMULATORS.iter().find(|e| e.id == id)
}

/// Profil PC embarque de cet emulateur.
pub fn pc_profile(e: &Emu) -> Option<Profile> {
    profiles::load_embedded().into_iter().find(|p| p.id == e.id)
}

/// Profil « vu de la console » : identique au profil PC (nom, libelles, filtres, cases cochees par defaut), seuls les chemins
/// deviennent `<root>/<sous-dossier>`. None si le profil PC manque ou ne correspond pas a la table (libelles differents).
pub fn android_profile(e: &Emu) -> Option<Profile> {
    let pc = pc_profile(e)?;
    if pc.locations.len() != e.locations.len() {
        return None;
    }
    let mut locs: Vec<ProfLocation> = Vec::new();
    for (l, a) in pc.locations.iter().zip(e.locations.iter()) {
        if l.label != a.label {
            return None;
        }
        locs.push(ProfLocation { path: format!("<{}>/{}", ROOT_VAR, a.rel), ..l.clone() });
    }
    Some(Profile {
        variables: vec![ProfVar { name: ROOT_VAR.into(), label: format!("{} folder", e.display), candidates: Vec::new(), scan: None }],
        locations: locs,
        detection: Default::default(),
        ..pc
    })
}

/// Variables de depliage pour un dossier d'emulateur donne (aucun dossier Windows : seulement `<root>`).
pub fn vars_for_root(root: &Path) -> Vars {
    let mut values = BTreeMap::new();
    values.insert(ROOT_VAR.to_string(), root.to_string_lossy().to_string());
    Vars { values, ..Default::default() }
}

/// Element a proteger sur la console : meme structure que `profiles::to_game` sur le PC (meme nom, memes identifiants
/// d'emplacements), chemins sous `root`, aucun programme (la console ne sait pas si l'emulateur tourne).
/// `enabled` : cases cochees par emplacement (absent = valeur par defaut du profil).
pub fn android_game(e: &Emu, root: &Path, enabled: &[bool]) -> Result<Game, String> {
    let p = android_profile(e).ok_or_else(|| format!("profil PC « {} » introuvable ou different de la table", e.id))?;
    let mut g = profiles::to_game(&p, &vars_for_root(root), enabled)?;
    g.exe = None;
    g.display_name = if e.display != p.name { Some(e.display.to_string()) } else { None };
    Ok(g)
}

/// Un emulateur trouve sur la console : son dossier et, par emplacement, si le dossier existe deja.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub emu: &'static Emu,
    pub root: PathBuf,
    pub present: Vec<bool>,
}

/// Emulateurs presents dans les stockages donnes (racine interne, cartes SD) : le dossier de l'emulateur existe ET au moins
/// un emplacement existe (un dossier vide cree par une appli desinstallee n'est pas propose). Un emulateur n'est retenu
/// qu'une fois (premier stockage, premier dossier candidat).
pub fn find_on_device(storage_roots: &[PathBuf]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for e in EMULATORS {
        'emu: for s in storage_roots {
            for cand in e.android_roots {
                let root = s.join(cand);
                if !root.is_dir() {
                    continue;
                }
                let present: Vec<bool> = e.locations.iter().map(|l| root.join(l.rel).is_dir()).collect();
                if present.iter().any(|p| *p) {
                    out.push(Found { emu: e, root, present });
                    break 'emu;
                }
            }
        }
    }
    out
}

/// Dossiers perso proposes sur la console (jamais coches par defaut) : (nom de l'element, sous-dossier du stockage).
pub const PERSONAL_FOLDERS: &[(&str, &str)] = &[("Photos (camera)", "DCIM/Camera"), ("Pictures", "Pictures"), ("Documents", "Documents"), ("Screenshots", "Pictures/Screenshots")];

/// Element « dossier perso » : un simple dossier, sans programme.
pub fn personal_game(name: &str, path: &Path) -> Game {
    Game { name: name.to_string(), path: path.to_path_buf(), ..Default::default() }
}

/// Ligne du tableau de compatibilite (documentation) : identifiant, nom, dossiers Android, compatibilite, note.
pub fn compat_table() -> Vec<(String, String, String, &'static str, &'static str)> {
    EMULATORS.iter().map(|e| (e.id.to_string(), e.display.to_string(), e.android_roots.join(", "), e.compat.code(), e.note)).collect()
}
