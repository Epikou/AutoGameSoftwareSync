//! Preferences de Windows (2.0.2, lot 15) : theme, souris, clavier, Explorateur, barre des taches, region, son, mode
//! d'alimentation, variables d'environnement (expert), applications par defaut, polices, fichiers Office, dictionnaire,
//! PowerShell / Terminal, fichier hosts, acces rapide, liste « A refaire » (imprimantes, lecteurs reseau, Wi-Fi).
//!
//! Principe : chaque element est un element ordinaire du moteur (versions immuables du coffre, copie de securite avant
//! toute restauration, « Annuler », Remonter le temps). Son emplacement PRINCIPAL est un dossier de travail local
//! `<donnees>/winprefs/<code>/` qui contient l'export LISIBLE du registre (`prefs.json`) ; les fichiers (polices,
//! signatures, profils...) sont des emplacements supplementaires ordinaires.
//! - Avant toute lecture par le moteur (analyse, copie, restauration, annulation) : `refresh` exporte l'etat actuel des
//!   SEULES cles listees vers `prefs.json` (fichier reecrit seulement s'il change). La copie de securite d'avant
//!   restauration contient donc l'etat actuel de Windows.
//! - Apres l'ecriture des fichiers par le moteur : `apply` ecrit dans le registre les seules valeurs listees et
//!   AUTORISEES par le catalogue (jamais une cle hors liste, meme si le coffre en contient), puis previent Windows
//!   (WM_SETTINGCHANGE / SystemParametersInfo) ; sinon « prend effet a la prochaine connexion ».
//! - Windows 10 / 11 : une valeur propre a l'autre systeme, ou dont la cle n'existe pas ici, est ignoree et comptee
//!   (bilan affiche) ; elle reste dans le fichier (aller-retour sans perte, aucun echange sans fin entre appareils).
//! - Fichier hosts : jamais ecrit en silence (droits administrateur) : la version recue attend l'accord explicite.
//! - JAMAIS : mots de passe Wi-Fi, gestionnaire d'identifiants, certificats et cles privees, mots de passe et cookies
//!   des navigateurs, comptes Microsoft, licences et cles de produit, pilotes (`FORBIDDEN`, verifie par les tests).
//!
//! Tout ce qui touche au systeme passe par le trait `Sys` (registre, commandes, notifications) : les tests utilisent
//! `MemSys` (memoire) ou `TestKeySys` (cle de test `HKCU\Software\AGSS-Test\...`), jamais le vrai registre.
use crate::knowledge::paths::Folders;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Dossier des exports locaux (dans le dossier de donnees).
pub const DIR: &str = "winprefs";
/// Export du registre (emplacement principal).
pub const FILE: &str = "prefs.json";
/// Copie du fichier hosts (emplacement principal de l'element « fichier hosts »).
pub const HOSTS_FILE: &str = "hosts";
/// Fiche des elements faits uniquement de fichiers (contenu FIXE : identique sur tous les appareils).
pub const ABOUT_FILE: &str = "about.json";
pub const FORMAT: u32 = 1;
/// Resultat de l'ecriture du fichier hosts avec droits eleves (dossier `winprefs`, hors du dossier de travail).
pub const HOSTS_RESULT: &str = "hosts-apply.result";
/// Premier numero de build de Windows 11.
pub const WIN11_BUILD: u32 = 22000;

// ================================================================== catalogue

/// Un element « Reglages Windows ».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Theme,
    Mouse,
    Keyboard,
    Explorer,
    Taskbar,
    Region,
    Sound,
    Power,
    EnvVars,
    FileAssoc,
    Fonts,
    Office,
    Dictionary,
    Terminal,
    Hosts,
    QuickAccess,
    TodoList,
}

/// Quand un reglage remis prend effet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Tout de suite.
    #[default]
    Now,
    /// A la prochaine ouverture (fenetre de l'Explorateur, programme).
    NextOpen,
    /// A la prochaine connexion a la session.
    NextSignIn,
    /// Jamais : liste a consulter seulement.
    Never,
}

/// Systeme cible d'une valeur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    #[default]
    Any,
    Win10,
    Win11,
}

impl Os {
    pub fn is_any(&self) -> bool {
        *self == Os::Any
    }
    pub fn of_build(build: u32) -> Os {
        if build >= WIN11_BUILD {
            Os::Win11
        } else {
            Os::Win10
        }
    }
    /// La valeur concerne-t-elle ce systeme ?
    pub fn fits(self, here: Os) -> bool {
        self == Os::Any || self == here
    }
}

impl Kind {
    pub const ALL: [Kind; 17] = [
        Kind::Theme,
        Kind::Mouse,
        Kind::Keyboard,
        Kind::Explorer,
        Kind::Taskbar,
        Kind::Region,
        Kind::Sound,
        Kind::Power,
        Kind::EnvVars,
        Kind::FileAssoc,
        Kind::Fonts,
        Kind::Office,
        Kind::Dictionary,
        Kind::Terminal,
        Kind::Hosts,
        Kind::QuickAccess,
        Kind::TodoList,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Kind::Theme => "theme",
            Kind::Mouse => "mouse",
            Kind::Keyboard => "keyboard",
            Kind::Explorer => "explorer",
            Kind::Taskbar => "taskbar",
            Kind::Region => "region",
            Kind::Sound => "sound",
            Kind::Power => "power",
            Kind::EnvVars => "envvars",
            Kind::FileAssoc => "defaultapps",
            Kind::Fonts => "fonts",
            Kind::Office => "office",
            Kind::Dictionary => "dictionary",
            Kind::Terminal => "terminal",
            Kind::Hosts => "hosts",
            Kind::QuickAccess => "quickaccess",
            Kind::TodoList => "todolist",
        }
    }

    pub fn from_code(c: &str) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.code().eq_ignore_ascii_case(c))
    }

    /// Nom STABLE (cle du coffre, identique sur tous les appareils) ; le nom affiche vient des textes de l'interface.
    pub fn stable(self) -> &'static str {
        match self {
            Kind::Theme => "Windows theme and colors",
            Kind::Mouse => "Windows mouse",
            Kind::Keyboard => "Windows keyboard and input languages",
            Kind::Explorer => "Windows File Explorer options",
            Kind::Taskbar => "Windows taskbar settings",
            Kind::Region => "Windows region and formats",
            Kind::Sound => "Windows sound scheme",
            Kind::Power => "AGSS Windows power plan",
            Kind::EnvVars => "Windows user environment variables",
            Kind::FileAssoc => "Windows default apps",
            Kind::Fonts => "Windows user fonts",
            Kind::Office => "Office signatures and templates",
            Kind::Dictionary => "Office custom dictionary",
            Kind::Terminal => "PowerShell and Terminal",
            Kind::Hosts => "Windows hosts file",
            Kind::QuickAccess => "Windows quick access",
            Kind::TodoList => "AGSS Windows to-do list",
        }
    }

    pub fn from_stable(name: &str) -> Option<Kind> {
        let k = crate::cloud::game_key(name);
        Kind::ALL.iter().copied().find(|x| crate::cloud::game_key(x.stable()) == k)
    }

    /// Propre a cet appareil : copie gardee dans un dossier du coffre a part pour chaque appareil, jamais remise sur un
    /// autre (un PC fixe n'est pas une console portable).
    pub fn device_specific(self) -> bool {
        matches!(self, Kind::Power | Kind::TodoList)
    }

    /// Visible seulement en mode expert.
    pub fn expert_only(self) -> bool {
        matches!(self, Kind::EnvVars)
    }

    /// Element qui lit le registre (export `prefs.json`).
    pub fn reads_registry(self) -> bool {
        matches!(self, Kind::Theme | Kind::Mouse | Kind::Keyboard | Kind::Explorer | Kind::Taskbar | Kind::Region | Kind::Sound | Kind::Power | Kind::EnvVars | Kind::FileAssoc | Kind::Fonts | Kind::TodoList)
    }

    /// Restaurer peut creer une cle absente (dispositions de clavier ajoutees, schema de pointeurs, sons).
    pub fn creates_keys(self) -> bool {
        matches!(self, Kind::Keyboard | Kind::Mouse | Kind::Sound)
    }

    /// Quand le reglage remis prend effet.
    pub fn effect(self) -> Effect {
        match self {
            Kind::Theme | Kind::Mouse | Kind::Region | Kind::Sound | Kind::Power | Kind::FileAssoc | Kind::Fonts | Kind::Hosts => Effect::Now,
            Kind::Explorer | Kind::EnvVars | Kind::Office | Kind::Dictionary | Kind::Terminal => Effect::NextOpen,
            Kind::Keyboard | Kind::Taskbar | Kind::QuickAccess => Effect::NextSignIn,
            Kind::TodoList => Effect::Never,
        }
    }

    /// Ecran des Parametres de Windows a ouvrir pour cet element (« A refaire »).
    pub fn settings_uri(self) -> &'static str {
        match self {
            Kind::Theme => "ms-settings:colors",
            Kind::Mouse => "ms-settings:mousetouchpad",
            Kind::Keyboard => "ms-settings:regionlanguage",
            Kind::Explorer => "ms-settings:developers",
            Kind::Taskbar => "ms-settings:taskbar",
            Kind::Region => "ms-settings:regionformatting",
            Kind::Sound => "ms-settings:sound",
            Kind::Power => "ms-settings:powersleep",
            Kind::EnvVars => "ms-settings:about",
            Kind::FileAssoc => "ms-settings:defaultapps",
            Kind::Fonts => "ms-settings:fonts",
            _ => "ms-settings:",
        }
    }
}

/// Element systeme du scan pour un element de ce catalogue (code `windows-<code>`).
pub fn scan_code(k: Kind) -> String {
    format!("windows-{}", k.code())
}

/// Cle du coffre d'un element propre a un appareil (`dest_key`).
pub fn device_dest(k: Kind, machine: &str) -> String {
    format!("{} {}", k.stable(), machine.trim())
}

/// Debut de la cle du coffre des elements propres a un appareil (`device_dest`).
pub fn device_prefix(k: Kind) -> String {
    crate::cloud::game_key(k.stable())
}

/// Cle du coffre : element des Reglages Windows (nom stable, ou cle propre a un appareil) ?
pub fn kind_of_key(key: &str) -> Option<Kind> {
    let k = crate::cloud::game_key(key);
    if let Some(x) = Kind::ALL.iter().copied().find(|x| crate::cloud::game_key(x.stable()) == k) {
        return Some(x);
    }
    Kind::ALL.iter().copied().filter(|x| x.device_specific()).find(|x| k.starts_with(&device_prefix(*x)))
}

// ================================================================== cles du registre (liste blanche)

/// Racine du registre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Root {
    Hkcu,
    Hklm,
}

impl Root {
    pub fn prefix(self) -> &'static str {
        match self {
            Root::Hkcu => "HKCU",
            Root::Hklm => "HKLM",
        }
    }
}

/// Valeurs retenues d'une cle.
#[derive(Clone, Copy, Debug)]
pub enum Sel {
    All,
    Only(&'static [&'static str]),
}

/// Une cle de la liste blanche.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub path: &'static str,
    pub sel: Sel,
    pub os: Os,
    /// Sous-cles comprises (profondeur limitee).
    pub recursive: bool,
    /// Seulement les sous-cles de ce nom (ex. `.Current` des sons).
    pub leaf: Option<&'static str>,
}

const fn spec(path: &'static str, sel: Sel, os: Os) -> Spec {
    Spec { path, sel, os, recursive: false, leaf: None }
}

const ADV: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

const THEME: &[Spec] = &[
    spec(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", Sel::All, Os::Any),
    spec(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent", Sel::All, Os::Any),
    spec(
        r"Software\Microsoft\Windows\DWM",
        Sel::Only(&["AccentColor", "AccentColorInactive", "ColorizationColor", "ColorizationAfterglow", "ColorizationColorBalance", "ColorizationAfterglowBalance", "ColorizationBlurBalance", "EnableWindowColorization", "ColorPrevalence"]),
        Os::Any,
    ),
    spec(r"Control Panel\Desktop", Sel::Only(&["WallpaperStyle", "TileWallpaper", "AutoColorization"]), Os::Any),
];

const MOUSE: &[Spec] = &[
    spec(r"Control Panel\Mouse", Sel::All, Os::Any),
    Spec { path: r"Control Panel\Cursors", sel: Sel::All, os: Os::Any, recursive: true, leaf: None },
    spec(r"Control Panel\Desktop", Sel::Only(&["WheelScrollLines", "WheelScrollChars"]), Os::Any),
];

const KEYBOARD: &[Spec] = &[
    spec(r"Control Panel\Keyboard", Sel::All, Os::Any),
    spec(r"Keyboard Layout\Preload", Sel::All, Os::Any),
    spec(r"Keyboard Layout\Substitutes", Sel::All, Os::Any),
    spec(r"Keyboard Layout\Toggle", Sel::All, Os::Any),
    Spec { path: r"Control Panel\International\User Profile", sel: Sel::All, os: Os::Any, recursive: true, leaf: None },
    spec(r"Control Panel\Desktop", Sel::Only(&["CursorBlinkRate"]), Os::Any),
];

const EXPLORER: &[Spec] = &[
    spec(
        ADV,
        Sel::Only(&[
            "HideFileExt", "Hidden", "ShowSuperHidden", "LaunchTo", "NavPaneExpandToCurrentFolder", "NavPaneShowAllFolders", "SeparateProcess", "ShowStatusBar", "ShowInfoTip",
            "HideDrivesWithNoMedia", "HideMergeConflicts", "AutoCheckSelect", "ShowCompColor", "ShowEncryptCompressedColor", "ShowSyncProviderNotifications", "Start_TrackDocs",
            "ShowPreviewHandlers", "FolderContentsInfoTip", "PersistBrowsers", "SharingWizardOn", "ShowTypeOverlay", "DontPrettyPath", "IconsOnly",
        ]),
        Os::Any,
    ),
    spec(r"Software\Microsoft\Windows\CurrentVersion\Explorer", Sel::Only(&["ShowFrequent", "ShowRecent"]), Os::Any),
    spec(r"Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState", Sel::Only(&["FullPath"]), Os::Any),
];

const TASKBAR: &[Spec] = &[
    spec(ADV, Sel::Only(&["ShowTaskViewButton", "TaskbarGlomLevel", "MMTaskbarEnabled", "MMTaskbarMode", "MMTaskbarGlomLevel", "TaskbarBadges", "TaskbarAutoHideInTabletMode", "TaskbarSd"]), Os::Any),
    spec(ADV, Sel::Only(&["TaskbarAl", "TaskbarMn", "TaskbarDa", "TaskbarSi"]), Os::Win11),
    spec(ADV, Sel::Only(&["TaskbarSmallIcons", "ShowCortanaButton"]), Os::Win10),
    spec(r"Software\Microsoft\Windows\CurrentVersion\Search", Sel::Only(&["SearchboxTaskbarMode"]), Os::Any),
    spec(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StuckRects3", Sel::Only(&["Settings"]), Os::Any),
    spec(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\People", Sel::Only(&["PeopleBand"]), Os::Win10),
    spec(r"Software\Microsoft\Windows\CurrentVersion\Feeds", Sel::Only(&["ShellFeedsTaskbarViewMode"]), Os::Win10),
];

const REGION: &[Spec] = &[spec(r"Control Panel\International", Sel::All, Os::Any), spec(r"Control Panel\International\Geo", Sel::All, Os::Any)];

const SOUND: &[Spec] = &[spec(r"AppEvents\Schemes", Sel::Only(&[""]), Os::Any), Spec { path: r"AppEvents\Schemes\Apps", sel: Sel::All, os: Os::Any, recursive: true, leaf: Some(".Current") }];

const ENV: &[Spec] = &[spec("Environment", Sel::All, Os::Any)];

const FONTS: &[Spec] = &[spec(r"Software\Microsoft\Windows NT\CurrentVersion\Fonts", Sel::All, Os::Any)];

/// Choix des applications par defaut (lecture seule : Windows protege `UserChoice` par une empreinte).
pub const FILE_EXTS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";
/// Ou une application par defaut PEUT etre ecrite (seulement sans choix protege sur ce PC).
pub const USER_CLASSES: &str = r"Software\Classes";

/// Cles lues / ecrites pour un element (HKCU).
pub fn specs(k: Kind) -> &'static [Spec] {
    match k {
        Kind::Theme => THEME,
        Kind::Mouse => MOUSE,
        Kind::Keyboard => KEYBOARD,
        Kind::Explorer => EXPLORER,
        Kind::Taskbar => TASKBAR,
        Kind::Region => REGION,
        Kind::Sound => SOUND,
        Kind::EnvVars => ENV,
        Kind::Fonts => FONTS,
        _ => &[],
    }
}

/// LISTE NOIRE (jamais lu, jamais ecrit, quelle que soit la liste) : identifiants, certificats et cles, navigateurs,
/// comptes Microsoft, licences et cles de produit, pilotes, Wi-Fi (profils et cles), demarrage automatique.
pub const FORBIDDEN: &[&str] = &[
    r"software\microsoft\windows nt\currentversion\softwareprotectionplatform",
    r"software\microsoft\windows nt\currentversion\digitalproductid",
    r"software\microsoft\systemcertificates",
    r"software\policies\microsoft\systemcertificates",
    r"software\microsoft\cryptography",
    r"software\microsoft\protect",
    r"software\microsoft\credentials",
    r"software\microsoft\vault",
    r"software\microsoft\identitycrl",
    r"software\microsoft\windows\currentversion\authentication",
    r"software\microsoft\windows live",
    r"software\microsoft\onedrive\accounts",
    r"software\microsoft\office\16.0\common\identity",
    r"software\microsoft\office\16.0\registration",
    r"software\microsoft\office\registration",
    r"software\microsoft\windows\currentversion\internet settings",
    r"software\google\chrome",
    r"software\microsoft\edge",
    r"software\mozilla",
    r"software\microsoft\wlansvc",
    r"software\microsoft\wcmsvc",
    r"system\currentcontrolset\services",
    r"system\currentcontrolset\control\class",
    r"system\driverdatabase",
    r"software\microsoft\windows\currentversion\run",
    r"software\microsoft\windows\currentversion\runonce",
    r"software\microsoft\windows nt\currentversion\winlogon",
];

/// Cle interdite (liste noire) ?
pub fn forbidden(key: &str) -> bool {
    let k = key.to_ascii_lowercase().replace('/', "\\");
    let k = k.trim_start_matches("hkcu\\").trim_start_matches("hklm\\");
    FORBIDDEN.iter().any(|f| k == *f || k.starts_with(&format!("{}\\", f)) || (f.ends_with("digitalproductid") && k.contains("digitalproductid")))
}

/// Nom de variable d'environnement qui peut contenir un secret (jeton, cle, mot de passe) : jamais copie.
pub fn sensitive_var(name: &str, value: &str) -> bool {
    let n = name.to_ascii_uppercase();
    for w in ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "APIKEY", "API_KEY", "CREDENTIAL", "PRIVATE", "COOKIE", "SESSION", "CONNECTIONSTRING", "CONNECTION_STRING"] {
        if n.contains(w) {
            return true;
        }
    }
    if n.split(|c: char| c == '_' || c == '-' || c == '.').any(|s| matches!(s, "KEY" | "PASS" | "PWD" | "AUTH" | "PAT" | "SAS" | "DSN")) {
        return true;
    }
    // Valeur qui ressemble a un jeton : longue suite sans espace ni separateur de chemin.
    let v = value.trim();
    v.len() >= 24 && !v.contains(['\\', '/', ';', ' ', '%', ':']) && v.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+=".contains(c))
}

// ================================================================== format du fichier exporte

/// Donnee d'une valeur du registre (forme lisible).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Data {
    Sz(String),
    Expand(String),
    Dword(u32),
    Qword(u64),
    Multi(Vec<String>),
    /// Octets en hexadecimal.
    Binary(String),
    /// Autre type : (numero du type, octets en hexadecimal).
    Raw(u32, String),
}

pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;
pub const REG_BINARY: u32 = 3;
pub const REG_DWORD: u32 = 4;
pub const REG_MULTI_SZ: u32 = 7;
pub const REG_QWORD: u32 = 11;

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect()
}

fn from_utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

impl Data {
    /// Octets bruts du registre -> donnee lisible.
    pub fn from_raw(ty: u32, b: &[u8]) -> Data {
        match ty {
            REG_SZ => Data::Sz(from_utf16(b)),
            REG_EXPAND_SZ => Data::Expand(from_utf16(b)),
            REG_DWORD if b.len() >= 4 => Data::Dword(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            REG_QWORD if b.len() >= 8 => Data::Qword(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])),
            REG_MULTI_SZ => {
                let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                let v: Vec<String> = units.split(|&c| c == 0).map(String::from_utf16_lossy).filter(|s| !s.is_empty()).collect();
                Data::Multi(v)
            }
            REG_BINARY => Data::Binary(hex(b)),
            t => Data::Raw(t, hex(b)),
        }
    }
    /// Donnee lisible -> (type, octets bruts).
    pub fn to_raw(&self) -> Option<(u32, Vec<u8>)> {
        Some(match self {
            Data::Sz(s) => (REG_SZ, utf16z(s)),
            Data::Expand(s) => (REG_EXPAND_SZ, utf16z(s)),
            Data::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
            Data::Qword(v) => (REG_QWORD, v.to_le_bytes().to_vec()),
            Data::Multi(v) => {
                let mut b: Vec<u8> = Vec::new();
                for s in v {
                    b.extend(utf16z(s));
                }
                b.extend([0u8, 0u8]);
                (REG_MULTI_SZ, b)
            }
            Data::Binary(h) => (REG_BINARY, unhex(h)?),
            Data::Raw(t, h) => (*t, unhex(h)?),
        })
    }
    fn text(&self) -> Option<&str> {
        match self {
            Data::Sz(s) | Data::Expand(s) => Some(s),
            _ => None,
        }
    }
}

/// Une valeur exportee.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// `HKCU\Control Panel\Mouse`.
    pub key: String,
    /// Nom de la valeur (vide = valeur par defaut de la cle).
    pub name: String,
    pub value: Data,
    /// Le texte contient des dossiers de l'utilisateur ramenes a `%USERPROFILE%`, `%APPDATA%`, `%LOCALAPPDATA%`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub tpl: bool,
    /// Propre a Windows 10 ou 11.
    #[serde(default, skip_serializing_if = "Os::is_any")]
    pub os: Os,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Entry {
    pub fn id(&self) -> String {
        format!("{}|{}", self.key.to_lowercase(), self.name.to_lowercase())
    }
    /// (racine, chemin sous la racine).
    pub fn split(&self) -> Option<(Root, String)> {
        split_key(&self.key)
    }
}

pub fn split_key(key: &str) -> Option<(Root, String)> {
    let (r, rest) = key.split_once('\\')?;
    let root = match r.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => Root::Hkcu,
        "HKLM" | "HKEY_LOCAL_MACHINE" => Root::Hklm,
        _ => return None,
    };
    Some((root, rest.to_string()))
}

/// Imprimante (inventaire).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Printer {
    pub name: String,
    #[serde(default)]
    pub port: String,
    #[serde(default)]
    pub driver: String,
}

/// Lecteur reseau (lettre + chemin, JAMAIS d'identifiant).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Drive {
    pub letter: String,
    pub path: String,
}

/// Liste « A refaire » d'un appareil (lecture seule : rien n'est remis automatiquement).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TodoList {
    #[serde(default)]
    pub printers: Vec<Printer>,
    #[serde(default)]
    pub drives: Vec<Drive>,
    /// Noms des reseaux Wi-Fi connus (JAMAIS de mot de passe).
    #[serde(default)]
    pub wifi: Vec<String>,
}

impl TodoList {
    pub fn is_empty(&self) -> bool {
        self.printers.is_empty() && self.drives.is_empty() && self.wifi.is_empty()
    }
}

/// Fichier `prefs.json` : stable (aucune date, aucun nom d'appareil) pour qu'un etat identique donne le meme fichier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PrefFile {
    pub format: u32,
    pub item: String,
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo: Option<TodoList>,
}

impl PrefFile {
    pub fn new(k: Kind) -> PrefFile {
        PrefFile { format: FORMAT, item: k.code().to_string(), entries: vec![], todo: None }
    }
    pub fn sort(&mut self) {
        self.entries.sort_by(|a, b| (a.key.to_lowercase(), a.name.to_lowercase()).cmp(&(b.key.to_lowercase(), b.name.to_lowercase())));
        self.entries.dedup_by(|a, b| a.id() == b.id());
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s.into_bytes()
    }
    pub fn read(p: &Path) -> Option<PrefFile> {
        serde_json::from_slice(&std::fs::read(p).ok()?).ok()
    }
}

/// Fiche fixe d'un element fait de fichiers (meme contenu partout).
pub fn about_bytes(k: Kind) -> Vec<u8> {
    format!("{{\n  \"format\": {},\n  \"item\": \"{}\"\n}}\n", FORMAT, k.code()).into_bytes()
}

// ================================================================== dossiers de l'utilisateur dans les valeurs

fn user_dirs(f: &Folders) -> Vec<(&'static str, String)> {
    let mut v = Vec::new();
    for (tok, p) in [("%LOCALAPPDATA%", &f.local), ("%APPDATA%", &f.roaming), ("%USERPROFILE%", &f.profile)] {
        if let Some(p) = p {
            let s = p.display().to_string().trim_end_matches(['\\', '/']).to_string();
            if s.len() >= 3 {
                v.push((tok, s));
            }
        }
    }
    v
}

fn replace_ci(s: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return s.to_string();
    }
    let low = s.to_lowercase();
    let f = from.to_lowercase();
    // Remplacement seulement si les longueurs en minuscules correspondent (pas de decalage d'indices).
    if low.len() != s.len() {
        return s.replace(from, to);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(j) = low[i..].find(&f) {
        let a = i + j;
        let b = a + f.len();
        // Fin de mot de chemin : la suite doit etre une fin, un separateur ou `;`.
        let next = s[b..].chars().next();
        if next.map(|c| c == '\\' || c == '/' || c == ';' || c == '"').unwrap_or(true) {
            out.push_str(&s[i..a]);
            out.push_str(to);
        } else {
            out.push_str(&s[i..b]);
        }
        i = b;
    }
    out.push_str(&s[i..]);
    out
}

/// Ramene les dossiers de l'utilisateur a des gabarits (`%LOCALAPPDATA%` avant `%USERPROFILE%`). None = inchange.
pub fn template(s: &str, f: &Folders) -> Option<String> {
    let mut out = s.to_string();
    for (tok, p) in user_dirs(f) {
        out = replace_ci(&out, &p, tok);
    }
    (out != s).then_some(out)
}

/// Developpe les gabarits avec les dossiers de CET utilisateur.
pub fn expand(s: &str, f: &Folders) -> String {
    let mut out = s.to_string();
    for (tok, p) in user_dirs(f) {
        out = out.replace(tok, &p);
    }
    out
}

fn templated(d: Data, f: &Folders) -> (Data, bool) {
    match d {
        Data::Sz(s) => match template(&s, f) {
            Some(t) => (Data::Sz(t), true),
            None => (Data::Sz(s), false),
        },
        Data::Expand(s) => match template(&s, f) {
            Some(t) => (Data::Expand(t), true),
            None => (Data::Expand(s), false),
        },
        Data::Multi(v) => {
            let t: Vec<String> = v.iter().map(|s| template(s, f).unwrap_or_else(|| s.clone())).collect();
            let changed = t != v;
            (Data::Multi(t), changed)
        }
        other => (other, false),
    }
}

fn expanded(e: &Entry, f: &Folders) -> Data {
    if !e.tpl {
        return e.value.clone();
    }
    match &e.value {
        Data::Sz(s) => Data::Sz(expand(s, f)),
        Data::Expand(s) => Data::Expand(expand(s, f)),
        Data::Multi(v) => Data::Multi(v.iter().map(|s| expand(s, f)).collect()),
        other => other.clone(),
    }
}

// ================================================================== acces au systeme (injectable)

/// Notification envoyee a Windows apres une restauration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// WM_SETTINGCHANGE avec ce texte (`ImmersiveColorSet`, `intl`, `Environment`, `TraySettings`).
    Setting(&'static str),
    /// Rechargement des pointeurs, vitesse et double-clic de la souris, molette.
    Mouse,
    /// Delai et vitesse de repetition du clavier.
    Keyboard,
    /// Polices ajoutees (chemins des fichiers).
    Fonts(Vec<PathBuf>),
    /// Applications par defaut changees.
    Assoc,
}

/// Tout ce qui touche au systeme.
pub trait Sys: Send + Sync {
    /// Numero de build de Windows.
    fn build(&self) -> u32;
    /// Valeurs d'une cle (None = cle absente).
    fn values(&self, root: Root, key: &str) -> Option<Vec<(String, Data)>>;
    fn subkeys(&self, root: Root, key: &str) -> Vec<String>;
    fn write(&self, root: Root, key: &str, name: &str, d: &Data) -> bool;
    /// Sortie texte d'une commande (fenetre cachee). None = echec.
    fn run(&self, prog: &str, args: &[&str]) -> Option<String>;
    fn notify(&self, n: &Notice);
    fn folders(&self) -> Folders;
    /// Fichier hosts de ce PC.
    fn hosts_path(&self) -> PathBuf;
    /// Ecritures permises ? (diagnostic des captures : jamais d'ecriture dans le vrai registre).
    fn can_write(&self) -> bool {
        true
    }
}

/// Systeme a utiliser : celui du contexte (tests), sinon le vrai Windows ; aucun sous Linux.
pub fn sys_of(ctx: &crate::Ctx) -> Option<Arc<dyn Sys>> {
    if let Some(s) = &ctx.sys {
        return Some(s.clone());
    }
    real()
}

#[cfg(windows)]
pub fn real() -> Option<Arc<dyn Sys>> {
    if let Some(d) = demo_from_env() {
        return Some(d);
    }
    Some(Arc::new(WinSys::from_env()))
}

#[cfg(not(windows))]
pub fn real() -> Option<Arc<dyn Sys>> {
    None
}

/// Reglages de DEMONSTRATION (captures, jamais dans l'interface) : `AGSS_WINPREFS_DEMO=<fichier.json>` remplace le
/// registre par ces valeurs, en LECTURE SEULE (rien n'est jamais ecrit, ni dans le registre ni ailleurs).
#[derive(Serialize, Deserialize, Default)]
pub struct DemoSys {
    pub build: u32,
    /// (cle complete `HKCU\...`, nom, valeur).
    pub values: Vec<(String, String, Data)>,
    /// (« prog args », sortie).
    #[serde(default)]
    pub commands: Vec<(String, String)>,
    #[serde(default)]
    pub hosts: Option<PathBuf>,
}

impl DemoSys {
    pub fn into_sys(self, folders: Folders) -> MemSys {
        let s = MemSys::new(self.build, folders, self.hosts.unwrap_or_default());
        for (k, n, d) in self.values {
            if let Some((r, key)) = split_key(&k) {
                s.set(r, &key, &n, d);
            }
        }
        for (c, o) in self.commands {
            s.command(&c, &o);
        }
        MemSys { read_only: true, ..s }
    }
}

pub fn demo_from_env() -> Option<Arc<dyn Sys>> {
    let p = std::env::var_os("AGSS_WINPREFS_DEMO")?;
    let d: DemoSys = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
    Some(Arc::new(d.into_sys(Folders::real())))
}

/// Systeme EN MEMOIRE (tests, toutes plateformes).
#[derive(Default)]
pub struct MemSys {
    pub build: u32,
    pub keys: Mutex<BTreeMap<(String, String), (String, Vec<(String, Data)>)>>,
    /// Valeurs qui refusent l'ecriture (`cle|nom` en minuscules, cle sans racine).
    pub deny: Mutex<BTreeSet<String>>,
    pub writes: Mutex<Vec<(String, String, Data)>>,
    pub notices: Mutex<Vec<Notice>>,
    /// Sorties des commandes : « prog args » -> texte.
    pub commands: Mutex<HashMap<String, String>>,
    pub runs: Mutex<Vec<String>>,
    pub folders: Folders,
    pub hosts: PathBuf,
    pub read_only: bool,
}

impl MemSys {
    pub fn new(build: u32, folders: Folders, hosts: PathBuf) -> MemSys {
        MemSys { build, folders, hosts, ..Default::default() }
    }
    fn k(root: Root, key: &str) -> (String, String) {
        (root.prefix().to_string(), key.to_lowercase())
    }
    pub fn add_key(&self, root: Root, key: &str) {
        let mut m = self.keys.lock().unwrap();
        m.entry(Self::k(root, key)).or_insert_with(|| (key.to_string(), vec![]));
    }
    pub fn set(&self, root: Root, key: &str, name: &str, d: Data) {
        let mut m = self.keys.lock().unwrap();
        let e = m.entry(Self::k(root, key)).or_insert_with(|| (key.to_string(), vec![]));
        match e.1.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some(x) => x.1 = d,
            None => e.1.push((name.to_string(), d)),
        }
    }
    pub fn get(&self, root: Root, key: &str, name: &str) -> Option<Data> {
        self.keys.lock().unwrap().get(&Self::k(root, key)).and_then(|e| e.1.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|x| x.1.clone()))
    }
    pub fn command(&self, line: &str, out: &str) {
        self.commands.lock().unwrap().insert(line.to_string(), out.to_string());
    }
    /// Tout le contenu (comparaison des etats).
    pub fn dump(&self) -> Vec<(String, String, Data)> {
        let m = self.keys.lock().unwrap();
        let mut v: Vec<(String, String, Data)> = m.iter().flat_map(|((r, _), (k, vals))| vals.iter().map(move |(n, d)| (format!("{}\\{}", r, k), n.clone(), d.clone()))).collect();
        v.sort_by(|a, b| (a.0.to_lowercase(), a.1.to_lowercase()).cmp(&(b.0.to_lowercase(), b.1.to_lowercase())));
        v
    }
}

impl Sys for MemSys {
    fn build(&self) -> u32 {
        self.build
    }
    fn values(&self, root: Root, key: &str) -> Option<Vec<(String, Data)>> {
        self.keys.lock().unwrap().get(&Self::k(root, key)).map(|e| e.1.clone())
    }
    fn subkeys(&self, root: Root, key: &str) -> Vec<String> {
        let pre = format!("{}\\", key.to_lowercase());
        let m = self.keys.lock().unwrap();
        let mut out: Vec<String> = Vec::new();
        for ((r, k), (orig, _)) in m.iter() {
            if r != root.prefix() || !k.starts_with(&pre) {
                continue;
            }
            let rest = &orig[pre.len()..];
            let first = rest.split('\\').next().unwrap_or("").to_string();
            if !first.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(&first)) {
                out.push(first);
            }
        }
        out
    }
    fn write(&self, root: Root, key: &str, name: &str, d: &Data) -> bool {
        if self.read_only || self.deny.lock().unwrap().contains(&format!("{}|{}", key.to_lowercase(), name.to_lowercase())) {
            return false;
        }
        self.set(root, key, name, d.clone());
        self.writes.lock().unwrap().push((format!("{}\\{}", root.prefix(), key), name.to_string(), d.clone()));
        true
    }
    fn run(&self, prog: &str, args: &[&str]) -> Option<String> {
        let line = format!("{} {}", prog, args.join(" "));
        self.runs.lock().unwrap().push(line.clone());
        self.commands.lock().unwrap().get(&line).cloned()
    }
    fn notify(&self, n: &Notice) {
        self.notices.lock().unwrap().push(n.clone());
    }
    fn folders(&self) -> Folders {
        self.folders.clone()
    }
    fn hosts_path(&self) -> PathBuf {
        self.hosts.clone()
    }
    fn can_write(&self) -> bool {
        !self.read_only
    }
}

// ================================================================== export

/// Valeurs d'une liste de cles (sous-cles comprises si demande), au plus 4000.
fn read_spec(sys: &dyn Sys, sp: &Spec, out: &mut Vec<(String, String, Data)>) {
    fn walk(sys: &dyn Sys, sp: &Spec, key: &str, depth: usize, out: &mut Vec<(String, String, Data)>) {
        if out.len() > 4000 || forbidden(key) {
            return;
        }
        let leaf_ok = sp.leaf.map(|l| key.rsplit('\\').next().map(|x| x.eq_ignore_ascii_case(l)).unwrap_or(false)).unwrap_or(true);
        if leaf_ok || depth == 0 {
            if let Some(vals) = sys.values(Root::Hkcu, key) {
                for (n, d) in vals {
                    let keep = match sp.sel {
                        Sel::All => sp.leaf.is_none() || leaf_ok,
                        Sel::Only(list) => list.iter().any(|x| x.eq_ignore_ascii_case(&n)),
                    };
                    if keep {
                        out.push((key.to_string(), n, d));
                    }
                }
            }
        }
        if sp.recursive && depth < 6 {
            for s in sys.subkeys(Root::Hkcu, key) {
                walk(sys, sp, &format!("{}\\{}", key, s), depth + 1, out);
            }
        }
    }
    walk(sys, sp, sp.path, 0, out);
}

/// L'etat actuel des valeurs listees (ordre stable). None : rien a lire pour cet element.
pub fn export(sys: &dyn Sys, k: Kind) -> Option<PrefFile> {
    let f = sys.folders();
    let mut pf = PrefFile::new(k);
    let here = Os::of_build(sys.build());
    match k {
        Kind::Power => {
            let out = sys.run("powercfg", &["/getactivescheme"])?;
            let (guid, name) = parse_active_scheme(&out)?;
            pf.entries.push(Entry { key: "HKCU\\powercfg".into(), name: "active".into(), value: Data::Sz(guid), tpl: false, os: Os::Any });
            pf.entries.push(Entry { key: "HKCU\\powercfg".into(), name: "name".into(), value: Data::Sz(name), tpl: false, os: Os::Any });
        }
        Kind::FileAssoc => {
            for ext in sys.subkeys(Root::Hkcu, FILE_EXTS) {
                if !valid_ext(&ext) {
                    continue;
                }
                let key = format!("{}\\{}\\UserChoice", FILE_EXTS, ext);
                if let Some(vals) = sys.values(Root::Hkcu, &key) {
                    if let Some((_, Data::Sz(p))) = vals.iter().find(|(n, _)| n.eq_ignore_ascii_case("ProgId")) {
                        if valid_progid(p) {
                            pf.entries.push(Entry { key: format!("HKCU\\{}", key), name: "ProgId".into(), value: Data::Sz(p.clone()), tpl: false, os: Os::Any });
                        }
                    }
                }
            }
        }
        Kind::TodoList => pf.todo = Some(todo_list(sys)),
        _ => {
            for sp in specs(k) {
                if !sp.os.fits(here) {
                    continue;
                }
                let mut raw = Vec::new();
                read_spec(sys, sp, &mut raw);
                for (key, name, d) in raw {
                    if k == Kind::EnvVars && sensitive_var(&name, d.text().unwrap_or("")) {
                        continue;
                    }
                    let (value, tpl) = templated(d, &f);
                    pf.entries.push(Entry { key: format!("HKCU\\{}", key), name, value, tpl, os: sp.os });
                }
            }
            if !specs(k).is_empty() && pf.entries.is_empty() {
                return None;
            }
        }
    }
    pf.sort();
    Some(pf)
}

/// Fusion SANS PERTE avec l'export precedent : les valeurs d'ici remplacent les anciennes ; une ancienne valeur absente
/// ici est gardee si elle est propre a l'autre systeme, si sa cle n'existe pas ici, ou si ce PC n'a pas pu l'ecrire
/// (sinon deux appareils differents s'echangeraient des versions sans fin) ; sinon elle a ete supprimee ici.
pub fn merge(old: Option<&PrefFile>, live: PrefFile, sys: &dyn Sys, unwritable: &BTreeSet<String>) -> PrefFile {
    let Some(old) = old else { return live };
    if old.item != live.item || live.todo.is_some() {
        return live;
    }
    let here = Os::of_build(sys.build());
    let mut out = live.clone();
    let ids: BTreeSet<String> = live.entries.iter().map(|e| e.id()).collect();
    for e in &old.entries {
        let id = e.id();
        if unwritable.contains(&id) {
            out.entries.retain(|x| x.id() != id);
            out.entries.push(e.clone());
            continue;
        }
        if ids.contains(&id) {
            continue;
        }
        let key_missing = e.split().map(|(r, k)| sys.values(r, &k).is_none()).unwrap_or(true);
        if !e.os.fits(here) || key_missing {
            out.entries.push(e.clone());
        }
    }
    out.sort();
    out
}

fn valid_ext(e: &str) -> bool {
    e.len() >= 2 && e.len() <= 24 && e.starts_with('.') && e[1..].chars().all(|c| c.is_ascii_alphanumeric() || "_+-".contains(c))
}

fn valid_progid(p: &str) -> bool {
    !p.is_empty() && p.len() <= 160 && p.chars().all(|c| c.is_alphanumeric() || "._-+ !{}".contains(c))
}

fn valid_guid(g: &str) -> bool {
    g.len() == 36 && g.chars().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// `powercfg /getactivescheme` -> (GUID, nom). Texte de toutes les langues : GUID repere par sa forme, nom entre parentheses.
pub fn parse_active_scheme(out: &str) -> Option<(String, String)> {
    for line in out.lines() {
        for w in line.split(|c: char| c.is_whitespace()) {
            let w = w.trim();
            if valid_guid(w) {
                let name = line.split_once('(').and_then(|(_, r)| r.rsplit_once(')')).map(|(n, _)| n.trim().to_string()).unwrap_or_default();
                return Some((w.to_lowercase(), name));
            }
        }
    }
    None
}

/// Noms des reseaux Wi-Fi de `netsh wlan show profiles` (toutes langues : lignes « libelle : nom » sous les profils).
/// Seuls les NOMS sont lus ; aucune autre commande ne touche aux profils Wi-Fi.
pub fn parse_wifi(out: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for line in out.lines() {
        if !line.starts_with("    ") {
            continue;
        }
        let Some((label, name)) = line.split_once(" : ") else { continue };
        let label = label.trim().to_lowercase();
        let name = name.trim();
        // Jamais une ligne de cle ou de mot de passe (sortie d'une autre commande collee par erreur).
        if label.contains("key") || label.contains("clé") || label.contains("cle") || label.contains("ключ") || label.contains("密钥") || label.contains("clave") || label.contains("password") || label.contains("mot de passe") {
            continue;
        }
        if name.is_empty() || name.starts_with('<') || name.len() > 64 {
            continue;
        }
        if !v.iter().any(|x| x == name) {
            v.push(name.to_string());
        }
    }
    v
}

/// Inventaire de ce PC : imprimantes (nom, port, pilote requis), lecteurs reseau (lettre, chemin ; JAMAIS d'identifiant),
/// reseaux Wi-Fi connus (NOMS seulement).
pub fn todo_list(sys: &dyn Sys) -> TodoList {
    let mut t = TodoList::default();
    const PRINTERS: &str = r"SYSTEM\CurrentControlSet\Control\Print\Printers";
    for p in sys.subkeys(Root::Hklm, PRINTERS) {
        let vals = sys.values(Root::Hklm, &format!("{}\\{}", PRINTERS, p)).unwrap_or_default();
        let get = |n: &str| vals.iter().find(|(x, _)| x.eq_ignore_ascii_case(n)).and_then(|(_, d)| d.text().map(|s| s.to_string())).unwrap_or_default();
        let name = { let n = get("Name"); if n.is_empty() { p.clone() } else { n } };
        t.printers.push(Printer { name, port: get("Port"), driver: get("Printer Driver") });
    }
    for c in sys.subkeys(Root::Hkcu, r"Printers\Connections") {
        // `,,serveur,imprimante` -> `\\serveur\imprimante`
        let name = format!("\\\\{}", c.trim_start_matches(',').replace(',', "\\"));
        if !t.printers.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
            t.printers.push(Printer { name, port: String::new(), driver: String::new() });
        }
    }
    for l in sys.subkeys(Root::Hkcu, "Network") {
        if l.len() != 1 || !l.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let vals = sys.values(Root::Hkcu, &format!("Network\\{}", l)).unwrap_or_default();
        // Seule la valeur RemotePath est lue (jamais UserName ni rien d'autre).
        if let Some((_, d)) = vals.iter().find(|(n, _)| n.eq_ignore_ascii_case("RemotePath")) {
            if let Some(p) = d.text() {
                t.drives.push(Drive { letter: format!("{}:", l.to_ascii_uppercase()), path: p.to_string() });
            }
        }
    }
    if let Some(out) = sys.run("netsh", &["wlan", "show", "profiles"]) {
        t.wifi = parse_wifi(&out);
    }
    t.printers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    t.drives.sort_by(|a, b| a.letter.cmp(&b.letter));
    t.wifi.sort_by_key(|a| a.to_lowercase());
    t
}

// ================================================================== restauration (ecriture ciblee)

/// Valeur AUTORISEE par la liste blanche de l'element (chemin et nom), hors liste noire. Une valeur venue du coffre qui
/// sort de la liste n'est jamais ecrite.
pub fn allowed(k: Kind, e: &Entry) -> bool {
    let Some((root, key)) = e.split() else { return false };
    if root != Root::Hkcu || forbidden(&key) || key.contains("..") {
        return false;
    }
    let low = key.to_lowercase();
    match k {
        Kind::FileAssoc => {
            let pre = format!("{}\\", FILE_EXTS.to_lowercase());
            let Some(rest) = low.strip_prefix(&pre) else { return false };
            let parts: Vec<&str> = rest.split('\\').collect();
            parts.len() == 2 && parts[1] == "userchoice" && valid_ext(parts[0]) && e.name.eq_ignore_ascii_case("ProgId") && matches!(&e.value, Data::Sz(p) if valid_progid(p))
        }
        Kind::Power => low == "powercfg" && (e.name == "active" || e.name == "name"),
        _ => specs(k).iter().any(|sp| {
            let p = sp.path.to_lowercase();
            let path_ok = low == p || (sp.recursive && low.starts_with(&format!("{}\\", p)) && low.matches('\\').count() <= p.matches('\\').count() + 6);
            let leaf_ok = sp.leaf.map(|l| low == p || low.rsplit('\\').next() == Some(&l.to_lowercase())).unwrap_or(true);
            let name_ok = match sp.sel {
                Sel::All => true,
                Sel::Only(list) => list.iter().any(|x| x.eq_ignore_ascii_case(&e.name)),
            };
            path_ok && leaf_ok && name_ok && !(k == Kind::EnvVars && sensitive_var(&e.name, e.value.text().unwrap_or("")))
        }),
    }
}

/// Bilan d'une restauration de reglages (garde localement, affiche dans la fiche).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Report {
    pub at_ms: u64,
    pub item: String,
    /// Valeurs ecrites.
    pub applied: usize,
    /// Deja identiques.
    pub same: usize,
    /// Propres a l'autre version de Windows, ou absentes de cet appareil : ignorees.
    pub other_os: usize,
    /// Refusees (hors liste blanche ou liste noire).
    pub refused: usize,
    /// Ecriture impossible (protegee par Windows).
    pub failed: usize,
    /// A refaire a la main (extensions des applications par defaut, fichier hosts...).
    #[serde(default)]
    pub todo: Vec<String>,
    pub effect: Effect,
    /// La version recue du fichier hosts attend l'accord (droits administrateur).
    #[serde(default)]
    pub pending_admin: bool,
    /// Rien n'a ete ecrit (ecritures desactivees).
    #[serde(default)]
    pub dry_run: bool,
    /// Le systeme cible (Windows 10 / 11).
    #[serde(default)]
    pub here: Os,
}

/// Ecrit dans le registre les valeurs du fichier (seulement celles qui different, seulement celles qui sont permises).
/// `unwritable` recoit les valeurs que Windows refuse (gardees telles quelles dans les exports suivants).
pub fn apply(sys: &dyn Sys, k: Kind, file: &PrefFile, unwritable: &mut BTreeSet<String>) -> Report {
    let f = sys.folders();
    let here = Os::of_build(sys.build());
    let mut r = Report { at_ms: util::now_ms(), item: k.code().to_string(), effect: k.effect(), here, ..Default::default() };
    if file.item != k.code() {
        r.refused = file.entries.len();
        return r;
    }
    match k {
        Kind::TodoList => {
            r.effect = Effect::Never;
            return r;
        }
        Kind::Power => return apply_power(sys, file, r),
        Kind::FileAssoc => return apply_assoc(sys, file, r),
        _ => {}
    }
    let mut touched: Vec<Entry> = Vec::new();
    let mut cache: HashMap<String, Option<Vec<(String, Data)>>> = HashMap::new();
    for e in &file.entries {
        if !allowed(k, e) {
            r.refused += 1;
            continue;
        }
        if !e.os.fits(here) {
            r.other_os += 1;
            continue;
        }
        let Some((root, key)) = e.split() else { continue };
        let live = cache.entry(key.to_lowercase()).or_insert_with(|| sys.values(root, &key)).clone();
        if live.is_none() && !k.creates_keys() {
            r.other_os += 1;
            continue;
        }
        let want = expanded(e, &f);
        if live.as_ref().map(|v| v.iter().any(|(n, d)| n.eq_ignore_ascii_case(&e.name) && *d == want)).unwrap_or(false) {
            r.same += 1;
            continue;
        }
        if !sys.can_write() {
            r.dry_run = true;
            r.applied += 1;
            continue;
        }
        if sys.write(root, &key, &e.name, &want) {
            r.applied += 1;
            unwritable.remove(&e.id());
            cache.remove(&key.to_lowercase());
            touched.push(Entry { value: want, ..e.clone() });
        } else {
            r.failed += 1;
            unwritable.insert(e.id());
        }
    }
    if !touched.is_empty() {
        for n in notices(k, &touched, &f) {
            sys.notify(&n);
        }
    }
    r
}

fn notices(k: Kind, touched: &[Entry], f: &Folders) -> Vec<Notice> {
    match k {
        Kind::Theme => vec![Notice::Setting("ImmersiveColorSet")],
        Kind::Mouse => vec![Notice::Mouse],
        Kind::Keyboard => vec![Notice::Keyboard, Notice::Setting("intl")],
        Kind::Explorer => vec![Notice::Setting("ShellState"), Notice::Assoc],
        Kind::Taskbar => vec![Notice::Setting("TraySettings")],
        Kind::Region => vec![Notice::Setting("intl")],
        Kind::EnvVars => vec![Notice::Setting("Environment")],
        Kind::Fonts => {
            let dir = f.local.as_ref().map(|l| l.join("Microsoft").join("Windows").join("Fonts"));
            let files: Vec<PathBuf> = touched
                .iter()
                .filter_map(|e| e.value.text().map(PathBuf::from))
                .filter(|p| dir.as_ref().map(|d| crate::scan::under(d, p)).unwrap_or(false))
                .collect();
            vec![Notice::Fonts(files)]
        }
        _ => vec![],
    }
}

fn apply_power(sys: &dyn Sys, file: &PrefFile, mut r: Report) -> Report {
    let Some(guid) = file.entries.iter().find(|e| e.name == "active").and_then(|e| e.value.text().map(|s| s.to_lowercase())) else { return r };
    if !valid_guid(&guid) {
        r.refused += 1;
        return r;
    }
    if sys.run("powercfg", &["/getactivescheme"]).and_then(|o| parse_active_scheme(&o)).map(|(g, _)| g == guid).unwrap_or(false) {
        r.same += 1;
        return r;
    }
    let exists = sys.run("powercfg", &["/list"]).map(|o| o.to_lowercase().contains(&guid)).unwrap_or(false);
    if !exists {
        r.other_os += 1;
        r.todo.push("power".into());
        return r;
    }
    if !sys.can_write() {
        r.dry_run = true;
        r.applied += 1;
        return r;
    }
    if sys.run("powercfg", &["/setactive", &guid]).is_some() {
        r.applied += 1;
    } else {
        r.failed += 1;
    }
    r
}

/// Applications par defaut : Windows protege le choix (`UserChoice`, empreinte) : il n'est JAMAIS ecrit. Si ce PC n'a
/// aucun choix pour l'extension et que l'application y est installee, le choix est ecrit dans `HKCU\Software\Classes` ;
/// sinon l'extension est « a refaire » dans les Parametres.
fn apply_assoc(sys: &dyn Sys, file: &PrefFile, mut r: Report) -> Report {
    let mut changed = false;
    for e in &file.entries {
        if !allowed(Kind::FileAssoc, e) {
            r.refused += 1;
            continue;
        }
        let Some((_, key)) = e.split() else { continue };
        let ext = key.rsplit('\\').nth(1).unwrap_or("").to_string();
        let Data::Sz(progid) = &e.value else { continue };
        let live = sys.values(Root::Hkcu, &key).and_then(|v| v.into_iter().find(|(n, _)| n.eq_ignore_ascii_case("ProgId")).and_then(|(_, d)| d.text().map(|s| s.to_string())));
        match live {
            Some(p) if p.eq_ignore_ascii_case(progid) => r.same += 1,
            Some(_) => r.todo.push(ext),
            None => {
                let installed = sys.values(Root::Hkcu, &format!("{}\\{}", USER_CLASSES, progid)).is_some() || sys.values(Root::Hklm, &format!("SOFTWARE\\Classes\\{}", progid)).is_some();
                if !installed {
                    r.todo.push(ext);
                    continue;
                }
                let ck = format!("{}\\{}", USER_CLASSES, ext);
                let cur = sys.values(Root::Hkcu, &ck).and_then(|v| v.into_iter().find(|(n, _)| n.is_empty()).and_then(|(_, d)| d.text().map(|s| s.to_string())));
                if cur.as_deref().map(|c| c.eq_ignore_ascii_case(progid)).unwrap_or(false) {
                    r.same += 1;
                } else if !sys.can_write() {
                    r.dry_run = true;
                    r.applied += 1;
                } else if sys.write(Root::Hkcu, &ck, "", &Data::Sz(progid.clone())) {
                    r.applied += 1;
                    changed = true;
                } else {
                    r.failed += 1;
                    r.todo.push(ext);
                }
            }
        }
    }
    r.todo.sort();
    r.todo.dedup();
    if changed {
        sys.notify(&Notice::Assoc);
    }
    r
}

// ================================================================== etat local (jamais dans le coffre)

/// Etat local d'un element : valeurs refusees par ce PC, dernier bilan, fichier hosts en attente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Local {
    #[serde(default)]
    pub unwritable: BTreeSet<String>,
    #[serde(default)]
    pub report: Option<Report>,
    #[serde(default)]
    pub hosts_pending: bool,
}

pub fn root(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR)
}

/// Dossier de travail (emplacement principal) d'un element.
pub fn staging(data_dir: &Path, k: Kind) -> PathBuf {
    root(data_dir).join(k.code())
}

fn local_file(data_dir: &Path, k: Kind) -> PathBuf {
    root(data_dir).join(format!("{}.local.json", k.code()))
}

pub fn load_local(data_dir: &Path, k: Kind) -> Local {
    std::fs::read(local_file(data_dir, k)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_local(data_dir: &Path, k: Kind, l: &Local) {
    let _ = std::fs::create_dir_all(root(data_dir));
    let _ = util::write_atomic(&local_file(data_dir, k), serde_json::to_string_pretty(l).unwrap_or_default().as_bytes());
}

/// L'emplacement est-il le dossier de travail d'un element ?
pub fn kind_of_path(data_dir: &Path, p: &Path) -> Option<Kind> {
    if p.parent()? != root(data_dir).as_path() {
        return None;
    }
    Kind::from_code(&p.file_name()?.to_string_lossy())
}

/// Elements des Reglages Windows d'un element configure (emplacements actives).
pub fn staged_in(data_dir: &Path, g: &crate::config::Game) -> Vec<(Kind, PathBuf)> {
    g.enabled_locations().into_iter().filter(|l| !l.is_file).filter_map(|l| kind_of_path(data_dir, &l.path).map(|k| (k, l.path))).collect()
}

/// Element des Reglages Windows (principal) de cet element configure.
pub fn kind_of_game(data_dir: &Path, g: &crate::config::Game) -> Option<Kind> {
    kind_of_path(data_dir, &g.path)
}

fn write_if_changed(p: &Path, bytes: &[u8]) -> bool {
    if std::fs::read(p).ok().as_deref() == Some(bytes) {
        return false;
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    util::write_atomic(p, bytes).is_ok()
}

/// Dernier export des elements qui lancent une commande (mode d'alimentation, liste A refaire) : au plus toutes les 10 min.
static THROTTLE: Mutex<Option<HashMap<PathBuf, u64>>> = Mutex::new(None);
const THROTTLE_MS: u64 = 10 * 60 * 1000;

/// Met a jour le dossier de travail avec l'etat actuel de Windows (avant toute lecture par le moteur). Rien n'est
/// ecrit si rien n'a change. Vrai si le fichier a change.
pub fn refresh_kind(data_dir: &Path, sys: &dyn Sys, k: Kind, force: bool) -> bool {
    let dir = staging(data_dir, k);
    match k {
        Kind::Hosts => {
            let local = load_local(data_dir, k);
            if local.hosts_pending {
                return false; // la version recue attend l'accord : on ne l'ecrase pas
            }
            match std::fs::read(sys.hosts_path()) {
                Ok(b) if b.len() <= 4 * 1024 * 1024 => write_if_changed(&dir.join(HOSTS_FILE), &b),
                _ => false,
            }
        }
        _ if !k.reads_registry() => write_if_changed(&dir.join(ABOUT_FILE), &about_bytes(k)),
        _ => {
            let p = dir.join(FILE);
            if matches!(k, Kind::Power | Kind::TodoList) && !force && p.is_file() {
                let now = util::now_ms();
                let mut g = THROTTLE.lock().unwrap_or_else(|e| e.into_inner());
                let m = g.get_or_insert_with(HashMap::new);
                if m.get(&dir).map(|t| now.saturating_sub(*t) < THROTTLE_MS).unwrap_or(false) {
                    return false;
                }
                m.insert(dir.clone(), now);
            }
            let Some(live) = export(sys, k) else { return false };
            let local = load_local(data_dir, k);
            let old = PrefFile::read(&p);
            let merged = merge(old.as_ref(), live, sys, &local.unwritable);
            write_if_changed(&p, &merged.to_bytes())
        }
    }
}

/// Avant toute lecture par le moteur : exporte l'etat actuel des elements Windows de cet element.
pub fn refresh(ctx: &crate::Ctx, g: &crate::config::Game) {
    let staged = staged_in(&ctx.data_dir, g);
    if staged.is_empty() {
        return;
    }
    let Some(sys) = sys_of(ctx) else { return };
    for (k, _) in staged {
        refresh_kind(&ctx.data_dir, &*sys, k, false);
    }
}

/// Apres l'ecriture des fichiers par le moteur : applique les reglages remis (ecriture ciblee + notification). Le bilan
/// est garde pour la fiche. Ne fait jamais echouer la restauration des fichiers.
pub fn after_restore(ctx: &crate::Ctx, g: &crate::config::Game) -> Vec<Report> {
    let staged = staged_in(&ctx.data_dir, g);
    if staged.is_empty() {
        return vec![];
    }
    let Some(sys) = sys_of(ctx) else { return vec![] };
    let mut out = Vec::new();
    for (k, dir) in staged {
        let mut local = load_local(&ctx.data_dir, k);
        let r = if k == Kind::Hosts {
            hosts_after_restore(&*sys, &dir, &mut local)
        } else if k.reads_registry() {
            match PrefFile::read(&dir.join(FILE)) {
                Some(pf) => apply(&*sys, k, &pf, &mut local.unwritable),
                None => continue,
            }
        } else {
            Report { at_ms: util::now_ms(), item: k.code().into(), effect: k.effect(), here: Os::of_build(sys.build()), ..Default::default() }
        };
        crate::perf::log(ctx, &format!("[Reglages Windows] {} : {} remis, {} identiques, {} ignores (autre systeme ou absents ici), {} refuses, {} impossibles, a refaire : {}", k.code(), r.applied, r.same, r.other_os, r.refused, r.failed, r.todo.len()));
        local.report = Some(r.clone());
        save_local(&ctx.data_dir, k, &local);
        out.push(r);
    }
    out
}

fn hosts_after_restore(sys: &dyn Sys, dir: &Path, local: &mut Local) -> Report {
    let mut r = Report { at_ms: util::now_ms(), item: Kind::Hosts.code().into(), effect: Effect::Now, here: Os::of_build(sys.build()), ..Default::default() };
    let got = std::fs::read(dir.join(HOSTS_FILE)).ok();
    let cur = std::fs::read(sys.hosts_path()).ok();
    if got.is_none() || got == cur {
        local.hosts_pending = false;
        r.same = 1;
        return r;
    }
    // Jamais en silence : droits administrateur, accord explicite demande dans la fiche.
    local.hosts_pending = true;
    r.pending_admin = true;
    r.todo.push("hosts".into());
    r
}

/// Fichier hosts recu valable ? (texte, taille raisonnable)
pub fn hosts_ok(b: &[u8]) -> bool {
    b.len() <= 4 * 1024 * 1024 && !b.contains(&0)
}

/// « Garder celui de cet appareil » : la version recue n'est pas appliquee ; l'export suivant reprend le fichier d'ici.
pub fn hosts_keep_mine(data_dir: &Path) {
    let mut l = load_local(data_dir, Kind::Hosts);
    l.hosts_pending = false;
    if let Some(r) = &mut l.report {
        r.pending_admin = false;
        r.todo.retain(|t| t != "hosts");
    }
    save_local(data_dir, Kind::Hosts, &l);
}

/// Apres l'accord et l'ecriture avec droits eleves : plus rien en attente.
pub fn hosts_done(data_dir: &Path) {
    hosts_keep_mine(data_dir);
}

/// Copie de securite locale du fichier hosts actuel (avant l'ecriture avec droits eleves).
pub fn hosts_backup(data_dir: &Path, sys: &dyn Sys) -> Option<PathBuf> {
    let b = std::fs::read(sys.hosts_path()).ok()?;
    let p = root(data_dir).join(format!("hosts-before-{}.txt", util::timestamp(util::now_ms())));
    util::write_atomic(&p, &b).ok()?;
    Some(p)
}

/// Etape AVEC DROITS ELEVES (`--apply-hosts <fichier>`) : le fichier doit etre la copie recue (`winprefs\hosts\hosts`),
/// du texte ; l'ancien fichier est garde a cote (`hosts.agss-before`).
pub fn write_hosts_elevated(src: &Path, hosts: &Path) -> crate::Res<()> {
    let ok_name = src.file_name().map(|n| n == HOSTS_FILE).unwrap_or(false)
        && src.parent().and_then(|p| p.file_name()).map(|n| n == Kind::Hosts.code()).unwrap_or(false)
        && src.parent().and_then(|p| p.parent()).and_then(|p| p.file_name()).map(|n| n == DIR).unwrap_or(false);
    if !ok_name {
        return Err("fichier refuse (copie recue du fichier hosts attendue)".into());
    }
    let b = std::fs::read(src)?;
    if !hosts_ok(&b) {
        return Err("fichier hosts recu invalide".into());
    }
    if let Ok(cur) = std::fs::read(hosts) {
        std::fs::write(hosts.with_file_name("hosts.agss-before"), cur)?;
    }
    std::fs::write(hosts, &b)?;
    Ok(())
}

/// Resultat de « Appliquer le fichier hosts recu ».
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostsApply {
    Done,
    /// L'utilisateur a refuse la fenetre de droits administrateur.
    Cancelled,
    Failed(String),
}

/// Applique le fichier hosts recu APRES l'accord explicite de l'utilisateur : copie de securite locale de l'actuel, puis
/// ecriture par une etape avec droits eleves (fenetre de Windows). Jamais appele automatiquement.
#[cfg(windows)]
pub fn approve_hosts(data_dir: &Path) -> HostsApply {
    let sys = WinSys::from_env();
    if !sys.can_write() {
        return HostsApply::Failed("lecture seule".into());
    }
    let src = staging(data_dir, Kind::Hosts).join(HOSTS_FILE);
    match std::fs::read(&src) {
        Ok(b) if hosts_ok(&b) => {}
        _ => return HostsApply::Failed("fichier recu invalide".into()),
    }
    if hosts_backup(data_dir, &sys).is_none() {
        return HostsApply::Failed("copie de securite impossible".into());
    }
    let res = root(data_dir).join(HOSTS_RESULT);
    let _ = std::fs::remove_file(&res);
    match crate::winutil::run_elevated_and_wait(&format!("--apply-hosts \"{}\"", src.display()), 60_000) {
        crate::winutil::ElevateResult::Finished(0) => {
            hosts_done(data_dir);
            HostsApply::Done
        }
        crate::winutil::ElevateResult::Cancelled => HostsApply::Cancelled,
        other => HostsApply::Failed(std::fs::read_to_string(&res).unwrap_or_else(|_| format!("{:?}", other))),
    }
}

#[cfg(not(windows))]
pub fn approve_hosts(_data_dir: &Path) -> HostsApply {
    HostsApply::Failed("Windows seulement".into())
}

// ================================================================== propositions du scan

/// Un element propose par le scan (emplacement principal = dossier de travail, puis fichiers).
#[derive(Clone, Debug)]
pub struct Offer {
    pub kind: Kind,
    pub locs: Vec<crate::scan::Loc>,
    /// Reglages lus (registre) ; 0 pour un element fait de fichiers.
    pub count: usize,
}

/// Emplacements des fichiers d'un element (ceux qui existent sur ce PC).
pub fn file_locs(k: Kind, f: &Folders) -> Vec<crate::scan::Loc> {
    use crate::scan::Loc;
    let dir = |p: PathBuf, filter: Option<&str>, label: &str| Loc { path: p, is_file: false, label: label.into(), filter: filter.map(|s| s.to_string()), exclude: Some("~$*".into()) };
    let file = |p: PathBuf, label: &str| Loc { path: p, is_file: true, label: label.into(), filter: None, exclude: None };
    let mut v: Vec<Loc> = Vec::new();
    let roaming = f.roaming.clone();
    let local = f.local.clone();
    match k {
        Kind::Fonts => {
            if let Some(l) = &local {
                v.push(dir(l.join("Microsoft").join("Windows").join("Fonts"), None, "Fonts"));
            }
        }
        Kind::Office => {
            if let Some(r) = &roaming {
                v.push(dir(r.join("Microsoft").join("Signatures"), None, "Signatures"));
                v.push(dir(r.join("Microsoft").join("Templates"), None, "Templates"));
            }
        }
        Kind::Dictionary => {
            if let Some(r) = &roaming {
                v.push(dir(r.join("Microsoft").join("UProof"), Some("*.dic"), "UProof"));
                v.push(dir(r.join("Microsoft").join("Spelling"), Some("*.dic;*.exc;*.acl"), "Spelling"));
            }
        }
        Kind::Terminal => {
            if let Some(d) = &f.documents {
                v.push(dir(d.join("PowerShell"), Some("*profile.ps1"), "PowerShell"));
                v.push(dir(d.join("WindowsPowerShell"), Some("*profile.ps1"), "WindowsPowerShell"));
            }
            if let Some(l) = &local {
                v.push(file(l.join("Packages").join("Microsoft.WindowsTerminal_8wekyb3d8bbwe").join("LocalState").join("settings.json"), "Terminal"));
                v.push(file(l.join("Packages").join("Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe").join("LocalState").join("settings.json"), "Terminal Preview"));
                v.push(file(l.join("Microsoft").join("Windows Terminal").join("settings.json"), "Terminal (portable)"));
            }
        }
        Kind::QuickAccess => {
            if let Some(r) = &roaming {
                v.push(file(r.join("Microsoft").join("Windows").join("Recent").join("AutomaticDestinations").join("f01b4d95cf55d32a.automaticDestinations-ms"), "Quick access"));
            }
        }
        _ => {}
    }
    v.retain(|l| if l.is_file { l.path.is_file() } else { crate::scan::has_matching_file(&l.path, l.filter.as_deref()) });
    v
}

/// Propositions du scan : exporte l'etat actuel (dossiers de travail locaux) et retient les elements qui ont quelque
/// chose a proteger. Windows seulement.
pub fn offers(data_dir: &Path, sys: &dyn Sys) -> Vec<Offer> {
    let f = sys.folders();
    let mut out = Vec::new();
    for k in Kind::ALL {
        let main = crate::scan::Loc { path: staging(data_dir, k), is_file: false, label: String::new(), filter: None, exclude: None };
        let files = file_locs(k, &f);
        let count = if k.reads_registry() {
            refresh_kind(data_dir, sys, k, true);
            PrefFile::read(&main.path.join(FILE)).map(|p| p.entries.len() + p.todo.map(|t| t.printers.len() + t.drives.len() + t.wifi.len()).unwrap_or(0)).unwrap_or(0)
        } else if k == Kind::Hosts {
            refresh_kind(data_dir, sys, k, true);
            usize::from(main.path.join(HOSTS_FILE).is_file())
        } else {
            0
        };
        let has = match k {
            Kind::Fonts => count > 0 || !files.is_empty(),
            Kind::TodoList => count > 0,
            _ if k.reads_registry() || k == Kind::Hosts => count > 0,
            _ => !files.is_empty(),
        };
        if !has {
            continue;
        }
        if !k.reads_registry() && k != Kind::Hosts {
            refresh_kind(data_dir, sys, k, true);
        }
        let mut locs = vec![main];
        locs.extend(files);
        out.push(Offer { kind: k, locs, count });
    }
    out
}

/// Prepare le dossier de travail d'un element avant son ajout (l'emplacement principal doit exister).
pub fn prepare(data_dir: &Path, k: Kind, sys: Option<&dyn Sys>) {
    match sys {
        Some(s) => {
            refresh_kind(data_dir, s, k, true);
        }
        None => {
            let _ = write_if_changed(&staging(data_dir, k).join(ABOUT_FILE), &about_bytes(k));
        }
    }
}

// ================================================================== vrai Windows

#[cfg(windows)]
pub struct WinSys {
    no_write: bool,
}

#[cfg(windows)]
impl WinSys {
    /// `AGSS_WINPREFS_NO_WRITE=1` (diagnostic des captures) : lecture seule, aucune ecriture dans le registre.
    pub fn from_env() -> WinSys {
        WinSys { no_write: std::env::var("AGSS_WINPREFS_NO_WRITE").map(|v| v == "1").unwrap_or(false) }
    }
}

#[cfg(windows)]
fn hive(r: Root) -> crate::registry::Hive {
    match r {
        Root::Hkcu => crate::registry::Hive::CurrentUser,
        Root::Hklm => crate::registry::Hive::LocalMachine,
    }
}

#[cfg(windows)]
pub fn run_hidden(prog: &str, args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    // Sortie en UTF-8 (noms accentues) : page de code 65001 pour cette seule commande.
    let mut line = String::from("chcp 65001 >nul & ");
    line.push_str(prog);
    for a in args {
        line.push(' ');
        line.push_str(a);
    }
    let o = std::process::Command::new("cmd").args(["/d", "/c", &line]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).creation_flags(0x0800_0000).output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).to_string())
}

#[cfg(windows)]
fn broadcast(what: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
    let w: Vec<u16> = what.encode_utf16().chain(std::iter::once(0)).collect();
    let mut res: usize = 0;
    unsafe {
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, w.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, &mut res);
    }
}

#[cfg(windows)]
fn spi(action: u32, ui: u32, pv: *mut core::ffi::c_void) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE};
    unsafe {
        SystemParametersInfoW(action, ui, pv, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE);
    }
}

#[cfg(windows)]
fn reg_num(path: &str, name: &str) -> Option<u32> {
    crate::registry::read_string(crate::registry::Hive::CurrentUser, path, name).and_then(|s| s.trim().parse().ok()).or_else(|| crate::registry::read_dword(crate::registry::Hive::CurrentUser, path, name))
}

#[cfg(windows)]
impl Sys for WinSys {
    fn build(&self) -> u32 {
        crate::registry::read_string(crate::registry::Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber").and_then(|s| s.trim().parse().ok()).unwrap_or(19045)
    }
    fn values(&self, root: Root, key: &str) -> Option<Vec<(String, Data)>> {
        if forbidden(key) {
            return None;
        }
        crate::registry::values(hive(root), key).map(|v| v.into_iter().map(|(n, t, b)| (n, Data::from_raw(t, &b))).collect())
    }
    fn subkeys(&self, root: Root, key: &str) -> Vec<String> {
        if forbidden(key) {
            return vec![];
        }
        crate::registry::subkeys(hive(root), key)
    }
    fn write(&self, root: Root, key: &str, name: &str, d: &Data) -> bool {
        if self.no_write || root != Root::Hkcu || forbidden(key) {
            return false;
        }
        match d.to_raw() {
            Some((t, b)) => crate::registry::write_raw(hive(root), key, name, t, &b),
            None => false,
        }
    }
    fn run(&self, prog: &str, args: &[&str]) -> Option<String> {
        // Seules commandes utilisees : mode d'alimentation et NOMS des reseaux Wi-Fi.
        let ok = match prog {
            "powercfg" => matches!(args.first().copied(), Some("/getactivescheme") | Some("/list")) || (args.first() == Some(&"/setactive") && !self.no_write && args.get(1).map(|g| valid_guid(g)).unwrap_or(false)),
            "netsh" => args == ["wlan", "show", "profiles"],
            _ => false,
        };
        if !ok {
            return None;
        }
        run_hidden(prog, args)
    }
    fn notify(&self, n: &Notice) {
        if self.no_write {
            return;
        }
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        match n {
            Notice::Setting(s) => broadcast(s),
            Notice::Mouse => {
                let m = r"Control Panel\Mouse";
                let mut v = [reg_num(m, "MouseThreshold1").unwrap_or(6), reg_num(m, "MouseThreshold2").unwrap_or(10), reg_num(m, "MouseSpeed").unwrap_or(1)];
                spi(SPI_SETMOUSE, 0, v.as_mut_ptr() as *mut _);
                if let Some(s) = reg_num(m, "MouseSensitivity") {
                    spi(SPI_SETMOUSESPEED, 0, s as usize as *mut _);
                }
                if let Some(d) = reg_num(m, "DoubleClickSpeed") {
                    spi(SPI_SETDOUBLECLICKTIME, d, std::ptr::null_mut());
                }
                if let Some(b) = reg_num(m, "SwapMouseButtons") {
                    spi(SPI_SETMOUSEBUTTONSWAP, b, std::ptr::null_mut());
                }
                if let Some(l) = reg_num(r"Control Panel\Desktop", "WheelScrollLines") {
                    spi(SPI_SETWHEELSCROLLLINES, l, std::ptr::null_mut());
                }
                spi(SPI_SETCURSORS, 0, std::ptr::null_mut());
            }
            Notice::Keyboard => {
                let k = r"Control Panel\Keyboard";
                if let Some(d) = reg_num(k, "KeyboardDelay") {
                    spi(SPI_SETKEYBOARDDELAY, d, std::ptr::null_mut());
                }
                if let Some(s) = reg_num(k, "KeyboardSpeed") {
                    spi(SPI_SETKEYBOARDSPEED, s, std::ptr::null_mut());
                }
            }
            Notice::Fonts(files) => {
                use windows_sys::Win32::Graphics::Gdi::AddFontResourceW;
                for f in files {
                    let w: Vec<u16> = f.display().to_string().encode_utf16().chain(std::iter::once(0)).collect();
                    unsafe {
                        AddFontResourceW(w.as_ptr());
                    }
                }
                let mut res: usize = 0;
                unsafe {
                    SendMessageTimeoutW(HWND_BROADCAST, WM_FONTCHANGE, 0, 0, SMTO_ABORTIFHUNG, 2000, &mut res);
                }
            }
            Notice::Assoc => unsafe {
                use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
                SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());
            },
        }
    }
    fn folders(&self) -> Folders {
        Folders::real()
    }
    fn hosts_path(&self) -> PathBuf {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        root.join("System32").join("drivers").join("etc").join("hosts")
    }
    fn can_write(&self) -> bool {
        !self.no_write
    }
}

/// Vrai registre, mais TOUT est redirige sous `HKCU\Software\AGSS-Test\<nom>\<HKCU|HKLM>\...` (tests d'aller-retour sur
/// une cle de test ; jamais une vraie cle de l'utilisateur).
#[cfg(windows)]
pub struct TestKeySys {
    pub base: String,
    pub build: u32,
    pub folders: Folders,
    pub hosts: PathBuf,
    pub notices: Mutex<Vec<Notice>>,
}

#[cfg(windows)]
impl TestKeySys {
    pub fn new(name: &str, build: u32, folders: Folders, hosts: PathBuf) -> TestKeySys {
        let n: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        TestKeySys { base: format!(r"Software\AGSS-Test\{}", n), build, folders, hosts, notices: Mutex::new(vec![]) }
    }
    fn map(&self, root: Root, key: &str) -> String {
        format!(r"{}\{}\{}", self.base, root.prefix(), key)
    }
    /// Ecrit une valeur de depart (preparation du test).
    pub fn seed(&self, root: Root, key: &str, name: &str, d: &Data) -> bool {
        let (t, b) = d.to_raw().unwrap();
        crate::registry::write_raw(crate::registry::Hive::CurrentUser, &self.map(root, key), name, t, &b)
    }
    pub fn read(&self, root: Root, key: &str, name: &str) -> Option<Data> {
        self.values(root, key)?.into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|x| x.1)
    }
    /// Supprime toute la cle de test.
    pub fn cleanup(&self) -> bool {
        crate::registry::delete_test_tree(&self.base) || !crate::registry::key_exists(crate::registry::Hive::CurrentUser, &self.base)
    }
}

#[cfg(windows)]
impl Sys for TestKeySys {
    fn build(&self) -> u32 {
        self.build
    }
    fn values(&self, root: Root, key: &str) -> Option<Vec<(String, Data)>> {
        crate::registry::values(crate::registry::Hive::CurrentUser, &self.map(root, key)).map(|v| v.into_iter().map(|(n, t, b)| (n, Data::from_raw(t, &b))).collect())
    }
    fn subkeys(&self, root: Root, key: &str) -> Vec<String> {
        crate::registry::subkeys(crate::registry::Hive::CurrentUser, &self.map(root, key))
    }
    fn write(&self, root: Root, key: &str, name: &str, d: &Data) -> bool {
        if root != Root::Hkcu || forbidden(key) {
            return false;
        }
        match d.to_raw() {
            Some((t, b)) => crate::registry::write_raw(crate::registry::Hive::CurrentUser, &self.map(root, key), name, t, &b),
            None => false,
        }
    }
    fn run(&self, _prog: &str, _args: &[&str]) -> Option<String> {
        None
    }
    fn notify(&self, n: &Notice) {
        self.notices.lock().unwrap().push(n.clone());
    }
    fn folders(&self) -> Folders {
        self.folders.clone()
    }
    fn hosts_path(&self) -> PathBuf {
        self.hosts.clone()
    }
}
