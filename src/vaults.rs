//! Coffre (2.0, lot 8) : detection des dossiers de synchronisation presents sur le PC (Google Drive, OneDrive, Dropbox,
//! iCloud Drive, MEGA, pCloud, Nextcloud, Syncthing, lecteurs reseau et USB ; Linux : points de montage et dossiers
//! usuels), sous-dossier `AutoGameSoftwareSync`, contenu deja present venant d'un autre appareil et reprise.
//! Toute la detection passe par le trait `Env` (variables, fichiers, registre, lecteurs, montages) : les tests la font
//! tourner sur un PC simule (`SimEnv`), Windows comme Linux, sans toucher au vrai disque ni au vrai registre.
use crate::cloud;
use crate::config::Game;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Sous-dossier cree dans le coffre choisi.
pub const VAULT_DIR: &str = "AutoGameSoftwareSync";
/// Petit fichier ecrit UNE fois dans le coffre (jamais ecrase, jamais supprime) : prouve l'acces en ecriture.
pub const MARKER: &str = ".agss-vault";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    GoogleDrive,
    OneDrive,
    Dropbox,
    ICloud,
    Mega,
    PCloud,
    Nextcloud,
    Syncthing,
    Network,
    Usb,
    Other,
}

impl Kind {
    /// Ordre d'affichage des tuiles.
    pub const ORDER: [Kind; 11] = [Kind::GoogleDrive, Kind::OneDrive, Kind::Dropbox, Kind::ICloud, Kind::Mega, Kind::PCloud, Kind::Nextcloud, Kind::Syncthing, Kind::Network, Kind::Usb, Kind::Other];

    /// Nom propre du service (jamais traduit) ; None pour les types sans nom propre (reseau, USB, autre).
    pub fn brand(self) -> Option<&'static str> {
        Some(match self {
            Kind::GoogleDrive => "Google Drive",
            Kind::OneDrive => "OneDrive",
            Kind::Dropbox => "Dropbox",
            Kind::ICloud => "iCloud Drive",
            Kind::Mega => "MEGA",
            Kind::PCloud => "pCloud",
            Kind::Nextcloud => "Nextcloud",
            Kind::Syncthing => "Syncthing",
            Kind::Network | Kind::Usb | Kind::Other => return None,
        })
    }

    /// Code stable (journal, tests).
    pub fn code(self) -> &'static str {
        match self {
            Kind::GoogleDrive => "gdrive",
            Kind::OneDrive => "onedrive",
            Kind::Dropbox => "dropbox",
            Kind::ICloud => "icloud",
            Kind::Mega => "mega",
            Kind::PCloud => "pcloud",
            Kind::Nextcloud => "nextcloud",
            Kind::Syncthing => "syncthing",
            Kind::Network => "network",
            Kind::Usb => "usb",
            Kind::Other => "other",
        }
    }

    fn rank(self) -> usize {
        Kind::ORDER.iter().position(|k| *k == self).unwrap_or(99)
    }
}

/// Un dossier de synchronisation trouve sur ce PC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub kind: Kind,
    pub path: PathBuf,
    /// Google Drive : Some(true) = mode streaming (fichiers a la demande), Some(false) = mode miroir ; None = inconnu.
    #[serde(default)]
    pub streaming: Option<bool>,
    /// Compte professionnel (OneDrive Commercial, Dropbox Business).
    #[serde(default)]
    pub work: bool,
}

// ------------------------------------------------------------------ environnement

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Other,
}

#[derive(Clone, Debug)]
pub struct Drive {
    pub root: PathBuf,
    pub kind: DriveKind,
    pub label: String,
    pub fs: String,
}

#[derive(Clone, Debug)]
pub struct Mount {
    pub point: PathBuf,
    pub fstype: String,
}

/// Tout ce que la detection lit du PC.
pub trait Env {
    /// Famille de regles a appliquer (Windows / Linux).
    fn windows(&self) -> bool;
    fn var(&self, name: &str) -> Option<String>;
    fn home(&self) -> Option<PathBuf>;
    fn is_dir(&self, p: &Path) -> bool;
    fn read_text(&self, p: &Path) -> Option<String>;
    /// Valeur texte du registre (`HKCU\...`), Windows seulement.
    fn reg_string(&self, key: &str, name: &str) -> Option<String>;
    fn reg_subkeys(&self, key: &str) -> Vec<String>;
    fn drives(&self) -> Vec<Drive>;
    fn mounts(&self) -> Vec<Mount>;
    /// Dossiers « Mon Drive » declares dans la configuration de Google Drive pour ordinateur (base `root_preference_sqlite.db`).
    fn drivefs_my_drive(&self) -> Vec<PathBuf> {
        vec![]
    }
}

/// Le vrai PC.
pub struct RealEnv;

impl Env for RealEnv {
    fn windows(&self) -> bool {
        cfg!(windows)
    }
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.trim().is_empty())
    }
    fn home(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
    fn is_dir(&self, p: &Path) -> bool {
        p.is_dir()
    }
    fn read_text(&self, p: &Path) -> Option<String> {
        let b = std::fs::read(p).ok()?;
        if b.len() > 2_000_000 {
            return None;
        }
        // Les fichiers de configuration peuvent etre en UTF-8 avec ou sans BOM.
        Some(String::from_utf8_lossy(b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b)).to_string())
    }
    #[cfg(windows)]
    fn reg_string(&self, key: &str, name: &str) -> Option<String> {
        crate::registry::read_string(crate::registry::Hive::CurrentUser, key.strip_prefix("HKCU\\")?, name)
    }
    #[cfg(not(windows))]
    fn reg_string(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    #[cfg(windows)]
    fn reg_subkeys(&self, key: &str) -> Vec<String> {
        key.strip_prefix("HKCU\\").map(|k| crate::registry::subkeys(crate::registry::Hive::CurrentUser, k)).unwrap_or_default()
    }
    #[cfg(not(windows))]
    fn reg_subkeys(&self, _: &str) -> Vec<String> {
        vec![]
    }
    #[cfg(windows)]
    fn drives(&self) -> Vec<Drive> {
        windows_drives()
    }
    #[cfg(not(windows))]
    fn drives(&self) -> Vec<Drive> {
        vec![]
    }
    #[cfg(windows)]
    fn mounts(&self) -> Vec<Mount> {
        vec![]
    }
    #[cfg(not(windows))]
    fn mounts(&self) -> Vec<Mount> {
        parse_mounts(&std::fs::read_to_string("/proc/mounts").unwrap_or_default())
    }
    #[cfg(windows)]
    fn drivefs_my_drive(&self) -> Vec<PathBuf> {
        let Some(base) = self.var("LOCALAPPDATA") else { return vec![] };
        let db = PathBuf::from(base).join("Google").join("DriveFS").join("root_preference_sqlite.db");
        drivefs_roots_from_db(&db)
    }
}

/// « Mon Drive » declare dans la base de preferences de Google Drive (table `roots`, ligne `is_my_drive`) : chemin local vu en dernier.
pub fn drivefs_roots_from_db(db: &Path) -> Vec<PathBuf> {
    let Ok(d) = crate::knowledge::sqlite::Db::open(db) else { return vec![] };
    let mut out = Vec::new();
    for r in d.table("roots").unwrap_or_default() {
        if r.get("is_my_drive").and_then(|v| v.as_i64()).unwrap_or(0) == 0 {
            continue;
        }
        for col in ["last_seen_absolute_path", "root_path"] {
            if let Some(p) = r.get(col).and_then(|v| v.as_text()).map(str::trim).filter(|p| !p.is_empty()) {
                out.push(clean_path(p));
                break;
            }
        }
    }
    out
}

#[cfg(windows)]
fn windows_drives() -> Vec<Drive> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{}:\\", letter);
        let w: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = match unsafe { GetDriveTypeW(w.as_ptr()) } {
            2 => DriveKind::Removable,
            3 => DriveKind::Fixed,
            4 => DriveKind::Network,
            5 => DriveKind::Optical,
            _ => DriveKind::Other,
        };
        let (mut name, mut fs) = ([0u16; 261], [0u16; 261]);
        let ok = unsafe { GetVolumeInformationW(w.as_ptr(), name.as_mut_ptr(), 261, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), 261) };
        let cut = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|c| *c == 0).unwrap_or(b.len())]);
        let (label, fs) = if ok != 0 { (cut(&name), cut(&fs)) } else { (String::new(), String::new()) };
        out.push(Drive { root: PathBuf::from(root), kind, label, fs });
    }
    out
}

/// `/proc/mounts` -> points de montage (les espaces sont codes `\040`).
pub fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let _src = it.next()?;
            let point = it.next()?.replace("\\040", " ").replace("\\011", "\t");
            let fstype = it.next()?.to_string();
            Some(Mount { point: PathBuf::from(point), fstype })
        })
        .collect()
}

/// PC simule (tests et verifications manuelles) : systeme de fichiers virtuel, registre, lecteurs, montages.
#[derive(Default, Clone)]
pub struct SimEnv {
    pub windows: bool,
    pub vars: BTreeMap<String, String>,
    pub home: Option<PathBuf>,
    pub dirs: HashSet<String>,
    pub files: BTreeMap<String, String>,
    pub reg: BTreeMap<(String, String), String>,
    pub reg_keys: Vec<String>,
    pub drives: Vec<Drive>,
    pub mounts: Vec<Mount>,
    pub drivefs_roots: Vec<PathBuf>,
}

/// Forme comparable d'un chemin simule : minuscules, `/`, sans doublons ni `/` final.
fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/").to_lowercase();
    let mut out = String::new();
    let mut last = ' ';
    for c in s.chars() {
        if c == '/' && last == '/' {
            continue;
        }
        out.push(c);
        last = c;
    }
    if out.len() > 3 {
        out = out.trim_end_matches('/').to_string();
    }
    out
}

impl SimEnv {
    pub fn windows_pc(home: &str) -> SimEnv {
        let mut s = SimEnv { windows: true, home: Some(PathBuf::from(home)), ..Default::default() };
        s.add_dir(home);
        s
    }
    pub fn linux_pc(home: &str) -> SimEnv {
        let mut s = SimEnv { windows: false, home: Some(PathBuf::from(home)), ..Default::default() };
        s.add_dir(home);
        s
    }
    /// Ajoute un dossier (et ses parents).
    pub fn add_dir(&mut self, p: &str) -> &mut Self {
        let n = norm(Path::new(p));
        let mut cur = String::new();
        for part in n.split('/') {
            if part.is_empty() {
                cur.push('/');
                continue;
            }
            if !cur.is_empty() && !cur.ends_with('/') {
                cur.push('/');
            }
            cur.push_str(part);
            self.dirs.insert(cur.clone());
            if part.ends_with(':') {
                // racine d'un lecteur : « g: » et « g:/ » designent le meme dossier
                self.dirs.insert(format!("{}/", cur));
            }
        }
        self
    }
    pub fn add_file(&mut self, p: &str, text: &str) -> &mut Self {
        let parent = Path::new(p).parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        if !parent.is_empty() {
            self.add_dir(&parent);
        }
        self.files.insert(norm(Path::new(p)), text.to_string());
        self
    }
    pub fn add_drive(&mut self, root: &str, kind: DriveKind, label: &str, fs: &str) -> &mut Self {
        self.add_dir(root);
        self.drives.push(Drive { root: PathBuf::from(root), kind, label: label.to_string(), fs: fs.to_string() });
        self
    }
    pub fn set_var(&mut self, k: &str, v: &str) -> &mut Self {
        self.vars.insert(k.to_string(), v.to_string());
        self
    }
    pub fn set_reg(&mut self, key: &str, name: &str, v: &str) -> &mut Self {
        self.reg.insert((key.to_lowercase(), name.to_lowercase()), v.to_string());
        let k = key.to_string();
        if !self.reg_keys.contains(&k) {
            self.reg_keys.push(k);
        }
        self
    }
    pub fn add_mount(&mut self, point: &str, fstype: &str) -> &mut Self {
        self.add_dir(point);
        self.mounts.push(Mount { point: PathBuf::from(point), fstype: fstype.to_string() });
        self
    }
}

impl Env for SimEnv {
    fn windows(&self) -> bool {
        self.windows
    }
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned().filter(|v| !v.trim().is_empty())
    }
    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }
    fn is_dir(&self, p: &Path) -> bool {
        self.dirs.contains(&norm(p))
    }
    fn read_text(&self, p: &Path) -> Option<String> {
        self.files.get(&norm(p)).cloned()
    }
    fn reg_string(&self, key: &str, name: &str) -> Option<String> {
        self.reg.get(&(key.to_lowercase(), name.to_lowercase())).cloned()
    }
    fn reg_subkeys(&self, key: &str) -> Vec<String> {
        let prefix = format!("{}\\", key.to_lowercase());
        let mut v: Vec<String> = self.reg_keys.iter().filter(|k| k.to_lowercase().starts_with(&prefix)).filter_map(|k| k[prefix.len()..].split('\\').next().map(|s| s.to_string())).collect();
        v.sort();
        v.dedup();
        v
    }
    fn drives(&self) -> Vec<Drive> {
        self.drives.clone()
    }
    fn mounts(&self) -> Vec<Mount> {
        self.mounts.clone()
    }
    fn drivefs_my_drive(&self) -> Vec<PathBuf> {
        self.drivefs_roots.clone()
    }
}

// ------------------------------------------------------------------ detection

/// Noms du « Mon Drive » de Google Drive selon la langue de l'interface de Drive.
pub const GDRIVE_FOLDERS: &[&str] = &["My Drive", "Mon Drive", "Mi unidad", "Meine Ablage", "Мой диск", "我的云端硬盘", "Il mio Drive", "Meu Drive", "マイドライブ"];

/// Nettoie un chemin lu dans un fichier de configuration (guillemets, espaces, `/` final sauf racine).
pub fn clean_path(s: &str) -> PathBuf {
    let t = s.trim().trim_matches('"').trim();
    let t = if t.len() > 3 { t.trim_end_matches(['/', '\\']) } else { t };
    PathBuf::from(t)
}

/// `info.json` de Dropbox : chemins des comptes personnel et professionnel.
pub fn parse_dropbox_info(json: &str) -> Vec<(PathBuf, bool)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let mut out = Vec::new();
    for (key, work) in [("personal", false), ("business", true)] {
        if let Some(p) = v.get(key).and_then(|a| a.get("path")).and_then(|p| p.as_str()).filter(|p| !p.trim().is_empty()) {
            out.push((clean_path(p), work));
        }
    }
    out
}

/// `nextcloud.cfg` : dossiers locaux synchronises (`0\Folders\1\localPath=...`).
pub fn parse_nextcloud_cfg(text: &str) -> Vec<PathBuf> {
    text.lines().filter_map(|l| l.trim().rsplit_once("localPath=").map(|(_, p)| clean_path(p))).filter(|p| !p.as_os_str().is_empty()).collect()
}

/// `config.xml` de Syncthing : chemin de chaque dossier partage (`<folder ... path="...">`), `~` developpe.
pub fn parse_syncthing_config(xml: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<folder ") {
        let tag_start = &rest[i..];
        let end = tag_start.find('>').unwrap_or(tag_start.len());
        let tag = &tag_start[..end];
        if let Some(j) = tag.find(" path=\"") {
            let v = &tag[j + 7..];
            if let Some(k) = v.find('"') {
                let raw = v[..k].replace("&amp;", "&");
                let p = if let (Some(h), Some(r)) = (home, raw.strip_prefix("~")) {
                    h.join(r.trim_start_matches(['/', '\\']))
                } else {
                    clean_path(&raw)
                };
                if !p.as_os_str().is_empty() {
                    out.push(p);
                }
            }
        }
        rest = &tag_start[end.min(tag_start.len())..];
        if rest.is_empty() {
            break;
        }
    }
    out
}

struct Collector<'a> {
    env: &'a dyn Env,
    out: Vec<Candidate>,
    seen: HashSet<String>,
}

impl<'a> Collector<'a> {
    fn add(&mut self, kind: Kind, path: PathBuf, streaming: Option<bool>, work: bool) {
        if path.as_os_str().is_empty() || !self.env.is_dir(&path) {
            return;
        }
        if self.seen.insert(norm(&path)) {
            self.out.push(Candidate { kind, path, streaming, work });
        }
    }
}

/// Dossiers de synchronisation presents sur ce PC, dans l'ordre d'affichage (Google Drive en tete).
pub fn detect(env: &dyn Env) -> Vec<Candidate> {
    let mut c = Collector { env, out: vec![], seen: HashSet::new() };
    let home = env.home();
    if env.windows() {
        detect_windows(&mut c, home.as_deref());
    } else {
        detect_linux(&mut c, home.as_deref());
    }
    let mut v = c.out;
    v.sort_by_key(|x| x.kind.rank());
    v
}

fn detect_windows(c: &mut Collector, home: Option<&Path>) {
    let env = c.env;
    let drives = env.drives();
    // --- Google Drive : lecteur virtuel (streaming) ou dossier local (miroir).
    let mount = env.reg_string("HKCU\\Software\\Google\\DriveFS\\Share", "MountPoint").and_then(|s| s.trim().chars().next()).map(|ch| ch.to_ascii_uppercase());
    let mut roots: Vec<PathBuf> = drives.iter().map(|d| d.root.clone()).collect();
    for letter in ['G'].into_iter().chain(mount) {
        let r = PathBuf::from(format!("{}:\\", letter));
        if !roots.iter().any(|x| norm(x) == norm(&r)) {
            roots.push(r);
        }
    }
    for root in &roots {
        let is_drive_label = drives.iter().find(|d| norm(&d.root) == norm(root)).map(|d| d.label.to_lowercase().starts_with("google drive")).unwrap_or(false);
        let is_mount = mount.map(|m| norm(root).starts_with(&format!("{}:", m.to_ascii_lowercase()))).unwrap_or(false);
        for name in GDRIVE_FOLDERS {
            c.add(Kind::GoogleDrive, root.join(name), Some(true), false);
        }
        // Lecteur virtuel de Drive sans « Mon Drive » reconnu (autre langue) : la racine du lecteur reste proposee.
        if (is_drive_label || is_mount) && !GDRIVE_FOLDERS.iter().any(|n| env.is_dir(&root.join(n))) {
            c.add(Kind::GoogleDrive, root.clone(), Some(true), false);
        }
    }
    if let Some(h) = home {
        c.add(Kind::GoogleDrive, h.join("Google Drive"), Some(false), false);
    }
    // configuration de Google Drive pour ordinateur : « Mon Drive » (miroir, ou lecteur virtuel s'il est sous un lecteur de Drive)
    for p in env.drivefs_my_drive() {
        let virt = drives.iter().any(|d| d.label.to_lowercase().starts_with("google drive") && norm(&p).starts_with(&norm(&d.root)));
        c.add(Kind::GoogleDrive, p, Some(virt), false);
    }
    // --- OneDrive : variables d'environnement puis registre.
    for (var, work) in [("OneDrive", false), ("OneDriveConsumer", false), ("OneDriveCommercial", true)] {
        if let Some(p) = env.var(var) {
            c.add(Kind::OneDrive, clean_path(&p), None, work);
        }
    }
    let acc = "HKCU\\Software\\Microsoft\\OneDrive\\Accounts";
    for sub in env.reg_subkeys(acc) {
        if let Some(p) = env.reg_string(&format!("{}\\{}", acc, sub), "UserFolder") {
            c.add(Kind::OneDrive, clean_path(&p), None, sub.to_lowercase().starts_with("business"));
        }
    }
    // --- Dropbox.
    for var in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = env.var(var) {
            if let Some(t) = env.read_text(&PathBuf::from(base).join("Dropbox").join("info.json")) {
                for (p, work) in parse_dropbox_info(&t) {
                    c.add(Kind::Dropbox, p, None, work);
                }
            }
        }
    }
    if let Some(h) = home {
        c.add(Kind::Dropbox, h.join("Dropbox"), None, false);
        c.add(Kind::ICloud, h.join("iCloudDrive"), None, false);
        c.add(Kind::Mega, h.join("MEGA"), None, false);
        c.add(Kind::Mega, h.join("MEGAsync"), None, false);
        c.add(Kind::PCloud, h.join("pCloud Sync"), None, false);
        c.add(Kind::Nextcloud, h.join("Nextcloud"), None, false);
    }
    if let Some(appdata) = env.var("APPDATA") {
        if let Some(t) = env.read_text(&PathBuf::from(appdata).join("Nextcloud").join("nextcloud.cfg")) {
            for p in parse_nextcloud_cfg(&t) {
                c.add(Kind::Nextcloud, p, None, false);
            }
        }
    }
    if let Some(local) = env.var("LOCALAPPDATA") {
        if let Some(t) = env.read_text(&PathBuf::from(local).join("Syncthing").join("config.xml")) {
            for p in parse_syncthing_config(&t, home).into_iter().take(3) {
                c.add(Kind::Syncthing, p, None, false);
            }
        }
    }
    // --- Lecteurs : pCloud Drive (virtuel), reseau / NAS, USB.
    for d in &drives {
        match d.kind {
            _ if d.label.to_lowercase().starts_with("pcloud") => c.add(Kind::PCloud, d.root.clone(), None, false),
            DriveKind::Network => c.add(Kind::Network, d.root.clone(), None, false),
            DriveKind::Removable => c.add(Kind::Usb, d.root.clone(), None, false),
            _ => {}
        }
    }
}

fn detect_linux(c: &mut Collector, home: Option<&Path>) {
    let env = c.env;
    if let Some(h) = home {
        c.add(Kind::GoogleDrive, h.join("Google Drive"), None, false);
        c.add(Kind::GoogleDrive, h.join("GoogleDrive"), None, false);
        c.add(Kind::OneDrive, h.join("OneDrive"), None, false);
        if let Some(t) = env.read_text(&h.join(".dropbox").join("info.json")) {
            for (p, work) in parse_dropbox_info(&t) {
                c.add(Kind::Dropbox, p, None, work);
            }
        }
        c.add(Kind::Dropbox, h.join("Dropbox"), None, false);
        c.add(Kind::ICloud, h.join("iCloudDrive"), None, false);
        c.add(Kind::Mega, h.join("MEGA"), None, false);
        c.add(Kind::Mega, h.join("MEGAsync"), None, false);
        c.add(Kind::PCloud, h.join("pCloudDrive"), None, false);
        c.add(Kind::PCloud, h.join("pCloud Sync"), None, false);
        if let Some(t) = env.read_text(&h.join(".config").join("Nextcloud").join("nextcloud.cfg")) {
            for p in parse_nextcloud_cfg(&t) {
                c.add(Kind::Nextcloud, p, None, false);
            }
        }
        c.add(Kind::Nextcloud, h.join("Nextcloud"), None, false);
        for cfg in [h.join(".local/state/syncthing/config.xml"), h.join(".config/syncthing/config.xml")] {
            if let Some(t) = env.read_text(&cfg) {
                for p in parse_syncthing_config(&t, home).into_iter().take(3) {
                    c.add(Kind::Syncthing, p, None, false);
                }
            }
        }
        c.add(Kind::Syncthing, h.join("Sync"), None, false);
    }
    for m in env.mounts() {
        let p = m.point.to_string_lossy().to_string();
        let net = matches!(m.fstype.as_str(), "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "davfs" | "fuse.sshfs" | "fuse.rclone" | "afs");
        let removable = (p.starts_with("/media/") || p.starts_with("/run/media/")) && !net;
        if net && p != "/" {
            c.add(Kind::Network, m.point.clone(), None, false);
        } else if removable {
            c.add(Kind::Usb, m.point.clone(), None, false);
        }
    }
}

/// Type de coffre d'un chemin deja choisi (le coffre actuel) : celui du dossier detecte qui le contient, sinon `Other`.
pub fn kind_of(path: &Path, found: &[Candidate]) -> (Kind, Option<bool>) {
    let p = norm(path);
    let best = found.iter().filter(|c| {
        let q = norm(&c.path);
        p == q || p.starts_with(&format!("{}/", q.trim_end_matches('/')))
    });
    match best.max_by_key(|c| norm(&c.path).len()) {
        Some(c) => (c.kind, c.streaming),
        None => (Kind::Other, None),
    }
}

/// Detection du vrai PC (ou du PC simule de `AGSS_VAULT_SIM`, un fichier JSON de candidats, pour les verifications manuelles).
pub fn detect_system() -> Vec<Candidate> {
    if let Some(f) = std::env::var_os("AGSS_VAULT_SIM") {
        if let Some(v) = std::fs::read_to_string(f).ok().and_then(|t| serde_json::from_str::<Vec<Candidate>>(t.trim_start_matches('\u{feff}')).ok()) {
            return v;
        }
    }
    detect(&RealEnv)
}

// ------------------------------------------------------------------ choix du dossier du coffre

/// Dossier du coffre pour le dossier choisi par l'utilisateur :
/// - le dossier choisi s'appelle deja `AutoGameSoftwareSync` : il sert tel quel ;
/// - il contient deja un sous-dossier `AutoGameSoftwareSync` : on le reprend ;
/// - il contient deja des elements (ancien dossier partage) : il sert tel quel, rien n'est imbrique ni deplace ;
/// - sinon le sous-dossier `AutoGameSoftwareSync` sera cree dedans.
pub fn target_for(chosen: &Path) -> PathBuf {
    if chosen.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(VAULT_DIR)).unwrap_or(false) {
        return chosen.to_path_buf();
    }
    // Deja un coffre (marqueur ecrit par l'appli, quel que soit le nom du dossier) : repris tel quel, rien d'imbrique.
    if chosen.join(MARKER).is_file() {
        return chosen.to_path_buf();
    }
    let sub = chosen.join(VAULT_DIR);
    if sub.is_dir() {
        return sub;
    }
    if !cloud::list_dest_games(chosen).is_empty() {
        return chosen.to_path_buf();
    }
    sub
}

/// Cree le coffre (sous-dossier + petit marqueur ecrit une fois) et verifie qu'on peut y ecrire. Ne supprime ni ne
/// renomme jamais rien. Retourne le dossier du coffre.
pub fn prepare(chosen: &Path) -> Result<PathBuf, String> {
    if !chosen.is_dir() {
        return Err(format!("dossier introuvable : {}", chosen.display()));
    }
    let t = target_for(chosen);
    std::fs::create_dir_all(&t).map_err(|e| format!("{} : {}", t.display(), e))?;
    let marker = t.join(MARKER);
    if !marker.exists() {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&marker).map_err(|e| format!("{} : {}", t.display(), e))?;
        f.write_all(b"AutoGameSoftwareSync vault v1\n").map_err(|e| format!("{} : {}", t.display(), e))?;
    }
    Ok(t)
}

/// Espace libre (octets) du volume qui contient `p` ; None si inconnu.
#[cfg(windows)]
pub fn free_space(p: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(w.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok != 0 {
        Some(free)
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn free_space(p: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0 {
        Some((s.f_bavail as u64).saturating_mul(s.f_frsize as u64))
    } else {
        None
    }
}

// ------------------------------------------------------------------ coffres existants (premier lancement, lot 10)

/// Coffre DEJA utilise, trouve dans un dossier de synchronisation detecte (ou le coffre actuel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Existing {
    pub path: PathBuf,
    /// Dossier de synchronisation detecte qui le contient (type du service, mode streaming).
    pub cand: Option<Candidate>,
    /// Elements deja dans le coffre.
    pub items: usize,
    /// Appareils qui y ont ecrit (noms de dossiers seulement).
    pub devices: Vec<String>,
    /// Ancien nom de dossier (version precedente de l'appli).
    pub legacy: bool,
    /// Marqueur `.agss-vault` present.
    pub marker: bool,
}

/// Au moins un element au format du coffre : `<cle>/saves/<version>/meta.json` (3 versions regardees au plus par element ;
/// seulement des noms et l'existence d'un fichier : rien n'est lu ni telecharge sous Google Drive en streaming).
pub fn has_vault_items(dir: &Path) -> bool {
    for dg in cloud::list_dest_games(dir) {
        for root in &dg.dirs {
            let Ok(rd) = std::fs::read_dir(root.join(cloud::SAVES_DIR)) else { continue };
            for e in rd.filter_map(|e| e.ok()).take(3) {
                if e.path().join("meta.json").is_file() {
                    return true;
                }
            }
        }
    }
    false
}

/// Le dossier est-il un coffre ? Marqueur `.agss-vault` ou elements au format du coffre.
pub fn is_vault(dir: &Path) -> bool {
    dir.is_dir() && (dir.join(MARKER).is_file() || has_vault_items(dir))
}

/// Nombre maximal de sous-dossiers regardes (marqueur) a la racine d'un dossier de synchronisation.
const EXISTING_SCAN_MAX: usize = 400;

/// Coffres existants dans les dossiers detectes, A PROPOSER EN PREMIER : le coffre actuel, puis pour chaque dossier de
/// synchronisation le dossier lui-meme (ancien dossier partage a la racine, cle USB), ses sous-dossiers `AutoGameSoftwareSync`
/// et aux anciens noms (`legacy::VAULT_DIRS`), puis tout sous-dossier qui porte le marqueur (nom choisi a la main).
/// Le plus fourni d'abord. Lecture seule.
pub fn find_existing(cands: &[Candidate], current: Option<&Path>) -> Vec<Existing> {
    let mut out: Vec<Existing> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let container = |p: &Path| -> Option<Candidate> {
        let q = norm(p);
        cands.iter().filter(|c| {
            let r = norm(&c.path);
            q == r || q.starts_with(&format!("{}/", r.trim_end_matches('/')))
        }).max_by_key(|c| norm(&c.path).len()).cloned()
    };
    let mut push = |p: PathBuf, legacy: bool, out: &mut Vec<Existing>| {
        if !seen.insert(norm(&p)) || !is_vault(&p) {
            return;
        }
        let items = cloud::list_dest_games(&p).len();
        let mut devices = cloud::machines_seen(&p);
        devices.sort_by_key(|d| d.to_lowercase());
        let marker = p.join(MARKER).is_file();
        let legacy = legacy && !marker;
        out.push(Existing { cand: container(&p), items, devices, legacy, marker, path: p });
    };
    if let Some(c) = current {
        let legacy = c.file_name().map(|n| crate::legacy::VAULT_DIRS.iter().any(|l| n.to_string_lossy().eq_ignore_ascii_case(l))).unwrap_or(false);
        push(c.to_path_buf(), legacy, &mut out);
    }
    for c in cands {
        push(c.path.clone(), false, &mut out);
        push(c.path.join(VAULT_DIR), false, &mut out);
        for l in crate::legacy::VAULT_DIRS {
            push(c.path.join(l), true, &mut out);
        }
        if let Ok(rd) = std::fs::read_dir(&c.path) {
            for e in rd.filter_map(|e| e.ok()).take(EXISTING_SCAN_MAX) {
                let p = e.path();
                let hidden = e.file_name().to_string_lossy().starts_with('.');
                if !hidden && p.join(MARKER).is_file() {
                    push(p, false, &mut out);
                }
            }
        }
    }
    // Le coffre actuel reste en tete ; les autres : le plus fourni d'abord.
    let cur = current.map(norm);
    out.sort_by(|a, b| {
        let ca = cur.as_deref() == Some(norm(&a.path).as_str());
        let cb = cur.as_deref() == Some(norm(&b.path).as_str());
        cb.cmp(&ca).then(b.items.cmp(&a.items)).then(b.marker.cmp(&a.marker)).then(norm(&a.path).cmp(&norm(&b.path)))
    });
    out
}

/// Coffre existant SOUS le dossier choisi (ou egal) qui ne serait PAS repris par `target_for` : choisir ce dossier
/// creerait un coffre neuf vide a cote. L'interface le dit et propose de reprendre l'existant.
pub fn existing_beside<'a>(chosen: &Path, existing: &'a [Existing]) -> Option<&'a Existing> {
    let target = norm(&target_for(chosen));
    let base = norm(chosen);
    existing.iter().find(|e| {
        let p = norm(&e.path);
        p != target && (p == base || p.starts_with(&format!("{}/", base.trim_end_matches('/')))) && e.items > 0
    })
}

// ------------------------------------------------------------------ contenu deja present

/// Un element du coffre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub name: String,
}

/// Elements du coffre qui viennent d'un appareil et ne sont pas encore sur ce PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FromDevice {
    pub machine: String,
    pub items: Vec<Item>,
}

/// Elements du coffre ecrits par un AUTRE appareil et absents de la liste de ce PC (`local_keys` : cles normalisees),
/// regroupes par appareil, le plus fourni en premier. Aucun fichier n'est lu (noms de dossiers seulement).
pub fn found_from_others(vault: &Path, own_machine: &str, local_keys: &HashSet<String>) -> Vec<FromDevice> {
    let mut by: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for (dg, machines) in cloud::machines_by_game(vault) {
        if local_keys.contains(&dg.key) {
            continue;
        }
        for m in machines.iter().filter(|m| !m.eq_ignore_ascii_case(own_machine)) {
            by.entry(m.clone()).or_default().push(Item { key: dg.key.clone(), name: dg.name.clone() });
        }
    }
    let mut v: Vec<FromDevice> = by.into_iter().map(|(machine, mut items)| {
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        FromDevice { machine, items }
    }).collect();
    v.sort_by(|a, b| b.items.len().cmp(&a.items.len()).then(a.machine.cmp(&b.machine)));
    v
}

/// Nombre d'appareils differents vus dans le coffre (hors celui-ci).
pub fn other_devices(vault: &Path, own_machine: &str) -> Vec<String> {
    let mut v: Vec<String> = cloud::machines_seen(vault).into_iter().filter(|m| !m.eq_ignore_ascii_case(own_machine)).collect();
    v.sort();
    v
}

/// Resultat d'une reprise.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResumeReport {
    /// Ajoutes sur ce PC (l'emplacement a ete retrouve).
    pub added: Vec<String>,
    /// Parmi eux, ceux dont la derniere version a ete remise (dossier vide ou absent ici ; copie de securite faite avant par le moteur).
    pub restored: Vec<String>,
    /// Emplacement a choisir par l'utilisateur (page Ajouter > deja dans ton coffre).
    pub to_finish: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Dossier de ce PC qui correspond au dossier de sauvegarde annonce par l'autre appareil (gabarit universel du meta.json).
pub fn local_path_for(tpl: &str, exe: Option<&str>, kf: &crate::hints::KnownFolders, prefixes: &[PathBuf]) -> Option<PathBuf> {
    let windows_exe = exe.map(|e| e.to_lowercase().ends_with(".exe")).unwrap_or(false);
    if windows_exe && !prefixes.is_empty() {
        crate::proton::find_in_prefixes(tpl, prefixes).into_iter().next()
    } else {
        crate::hints::expand(tpl, kf)
    }
}

/// Reprise (logique existante : lier + restaurer) : pour chaque element choisi, retrouve son dossier sur ce PC grace aux
/// indices ecrits par l'autre appareil, l'ajoute lie au dossier du coffre et, si le dossier local est vide ou vient d'etre
/// cree, remet sa derniere version (le moteur fait la copie de securite avant). Rien n'est jamais supprime ; ce qui ne peut
/// pas etre retrouve tout seul est liste dans `to_finish`.
pub fn resume(ctx: &crate::Ctx, vault: &Path, items: &[Item], kf: &crate::hints::KnownFolders, prefixes: &[PathBuf]) -> ResumeReport {
    let mut rep = ResumeReport::default();
    let all = cloud::list_dest_games(vault);
    for it in items {
        let Some(dg) = all.iter().find(|d| d.key == it.key) else { continue };
        let Some(meta) = cloud::latest_complete_meta(dg) else {
            rep.to_finish.push(dg.name.clone());
            continue;
        };
        // Plusieurs emplacements : a terminer a la main (page Ajouter).
        let path = if meta.locations.is_empty() { meta.save_path_tpl.as_deref().and_then(|t| local_path_for(t, meta.exe.as_deref(), kf, prefixes)) } else { None };
        let Some(path) = path else {
            rep.to_finish.push(dg.name.clone());
            continue;
        };
        if path.is_file() {
            rep.to_finish.push(dg.name.clone());
            continue;
        }
        let was_empty = !path.is_dir() || std::fs::read_dir(&path).map(|mut r| r.next().is_none()).unwrap_or(false);
        if !path.is_dir() {
            if let Err(e) = std::fs::create_dir_all(&path) {
                rep.failed.push((dg.name.clone(), e.to_string()));
                continue;
            }
        }
        let game = Game { name: dg.name.clone(), path, exe: meta.exe.clone().filter(|e| !e.is_empty()), dest_key: if cloud::game_key(&dg.name) == dg.key { None } else { Some(dg.key.clone()) }, ..Default::default() };
        match crate::actions::add_game(ctx, game.clone()) {
            Ok(()) => rep.added.push(dg.name.clone()),
            Err(e) => {
                rep.failed.push((dg.name.clone(), e.to_string()));
                continue;
            }
        }
        if was_empty {
            match crate::actions::do_restore(ctx, &game, vault, None) {
                Ok(_) => rep.restored.push(dg.name.clone()),
                Err(e) => rep.failed.push((dg.name.clone(), e.to_string())),
            }
        }
    }
    rep
}

// ------------------------------------------------------------------ conseils

/// Texte du conseil a afficher pour un coffre en mode streaming (Google Drive) : cle de l'interface.
pub fn needs_offline_tip(c: &Candidate) -> bool {
    c.kind == Kind::GoogleDrive && c.streaming == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropbox_info() {
        let j = r#"{"personal":{"path":"D:\\Dropbox","host":123},"business":{"path":"C:\\Users\\a\\Dropbox (Acme)\\"}}"#;
        let v = parse_dropbox_info(j);
        assert_eq!(v, vec![(PathBuf::from("D:\\Dropbox"), false), (PathBuf::from("C:\\Users\\a\\Dropbox (Acme)"), true)]);
        assert!(parse_dropbox_info("pas du json").is_empty());
        assert!(parse_dropbox_info("{}").is_empty());
    }

    #[test]
    fn nextcloud_et_syncthing() {
        let cfg = "[Accounts]\n0\\Folders\\1\\localPath=C:/Users/a/Nextcloud/\n0\\Folders\\1\\targetPath=/\n0\\Folders\\2\\localPath=D:/Docs\n";
        assert_eq!(parse_nextcloud_cfg(cfg), vec![PathBuf::from("C:/Users/a/Nextcloud"), PathBuf::from("D:/Docs")]);
        let xml = "<configuration><folder id=\"abcd\" label=\"Jeux\" path=\"/home/a/Sync Jeux\" type=\"sendreceive\"><device id=\"x\"></device></folder><folder id=\"b\" label=\"R\" path=\"~/Docs\"></folder></configuration>";
        let v = parse_syncthing_config(xml, Some(Path::new("/home/a")));
        assert_eq!(v, vec![PathBuf::from("/home/a/Sync Jeux"), PathBuf::from("/home/a/Docs")]);
    }

    #[test]
    fn montages_linux() {
        let t = "/dev/sda1 / ext4 rw 0 0\n//nas/jeux /mnt/nas\\040jeux cifs rw 0 0\n/dev/sdb1 /run/media/a/CLE vfat rw 0 0\n";
        let m = parse_mounts(t);
        assert_eq!(m.len(), 3);
        assert_eq!(m[1].point, PathBuf::from("/mnt/nas jeux"));
        assert_eq!(m[1].fstype, "cifs");
    }

    #[test]
    fn chemin_simule_normalise() {
        assert_eq!(norm(Path::new("G:\\Mon Drive\\")), "g:/mon drive");
        assert_eq!(norm(Path::new("G:\\")), "g:/");
        assert_eq!(norm(&PathBuf::from("G:\\").join("Mon Drive")), "g:/mon drive");
    }
}
