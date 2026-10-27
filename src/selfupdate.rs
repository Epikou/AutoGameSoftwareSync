//! Mise a jour automatique et SURE (lot 4).
//!
//! Chaine de confiance : le manifeste `agss-manifest.json` de la Release est signe Ed25519 (cle publique integree au
//! binaire, cle privee gardee hors ligne par l'auteur) ; il donne, pour chaque plateforme, l'URL de l'archive, sa taille
//! et son SHA-256. Le telechargement est verifie (taille, SHA-256), le programme extrait est verifie a son tour, puis
//! remplace l'exe en cours d'execution (Windows : renommage en `.old` ; Linux : `rename(2)` atomique). Un veilleur (ce meme
//! programme, lance depuis l'ancienne version) relance la nouvelle version et, si elle n'ecrit pas le marqueur
//! « demarrage OK » dans les 60 s (ou s'arrete), remet l'ancienne en place et bloque la version fautive.
//!
//! Tout ce qui peut l'etre est PUR ou injectable (tests dans un dossier temporaire, avec de faux exe).
use crate::update::{self, ReleaseInfo};
use crate::util;
use crate::watch::Logger;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Cle publique Ed25519 de signature des manifestes (32 octets, hexadecimal).
pub const PUBLIC_KEY_HEX: &str = "24f51371620e78c09909772a9526883703cf60cc3db85537f90a60b33ab1573d";
/// Manifeste de la derniere Release (GitHub redirige vers l'asset de la Release « latest »).
pub const MANIFEST_URL: &str = "https://github.com/Epikou/AutoGameSoftwareSync/releases/latest/download/agss-manifest.json";
pub const MANIFEST_NAME: &str = "agss-manifest.json";
/// Variable de debogage : adresse d'un manifeste (ex. serveur local). La signature reste exigee.
pub const ENV_URL: &str = "AGSS_UPDATE_URL";
/// Variable de debogage : duree (s) de la surveillance du demarrage (defaut 60).
pub const ENV_WATCH: &str = "AGSS_UPDATE_WATCH_SECS";
/// Duree de la surveillance du demarrage de la nouvelle version.
pub const WATCH_SECS: u64 = 60;
/// Versions du schema de manifeste comprises.
pub const SCHEMA: u32 = 1;
/// Taille maximale d'une archive et d'un exe (garde-fou contre un manifeste absurde).
pub const MAX_ARCHIVE_BYTES: u64 = 150 * 1024 * 1024;
pub const MAX_EXE_BYTES: u64 = 300 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const STATE_FILE: &str = "state.json";
const MARKER_FILE: &str = "started-ok";
const DAY_MS: u64 = 24 * 3600 * 1000;
const RETRY_MS: u64 = 3600 * 1000;

// ------------------------------------------------------------------ erreurs

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// Manifeste illisible ou incoherent.
    Format(String),
    BadSignature,
    BadHash,
    BadSize,
    Download(String),
    Install(String),
    /// Dossier de l'exe non inscriptible.
    NotWritable,
}

impl UpdateError {
    /// Valeur de l'evenement d'usage `update.result`.
    pub fn code(&self) -> &'static str {
        match self {
            UpdateError::Format(_) | UpdateError::BadSignature => "signature_ko",
            UpdateError::BadHash | UpdateError::BadSize => "hash_ko",
            _ => "download_failed",
        }
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::Format(m) => write!(f, "manifeste invalide : {}", m),
            UpdateError::BadSignature => write!(f, "signature du manifeste invalide"),
            UpdateError::BadHash => write!(f, "SHA-256 incorrect"),
            UpdateError::BadSize => write!(f, "taille incorrecte"),
            UpdateError::Download(m) => write!(f, "telechargement : {}", m),
            UpdateError::Install(m) => write!(f, "installation : {}", m),
            UpdateError::NotWritable => write!(f, "dossier de l'exe non inscriptible"),
        }
    }
}

impl std::error::Error for UpdateError {}

// ------------------------------------------------------------------ manifeste signe

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// Adresse de l'archive (zip ou tar.gz) dans la Release.
    pub url: String,
    pub size: u64,
    pub sha256: String,
    /// Nom du fichier a extraire de l'archive.
    pub member: String,
    /// SHA-256 du fichier extrait (facultatif : verifie s'il est present).
    #[serde(default)]
    pub exe_sha256: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    #[serde(default)]
    pub date: String,
    /// Page de la Release (« Quoi de neuf ? »).
    pub notes_url: String,
    /// Version minimale depuis laquelle la mise a jour AUTOMATIQUE est permise (sinon : etape manuelle, page de la Release).
    #[serde(default)]
    pub min_version: Option<String>,
    /// Plateformes : `win-x64`, `linux-x86_64`, `linux-x86_64-tray`.
    pub platforms: BTreeMap<String, Asset>,
}

#[derive(Serialize, Deserialize)]
struct SignedFile {
    /// Texte JSON du manifeste, signe TEL QUEL (aucune canonicalisation).
    payload: String,
    /// Signature Ed25519 du texte `payload`, en hexadecimal (128 caracteres).
    signature: String,
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Cle publique integree au binaire.
pub fn public_key() -> [u8; 32] {
    let v = unhex(PUBLIC_KEY_HEX).unwrap_or_default();
    let mut k = [0u8; 32];
    if v.len() == 32 {
        k.copy_from_slice(&v);
    }
    k
}

/// Signe le texte JSON d'un manifeste (outil de publication) : renvoie le contenu du fichier `agss-manifest.json`.
pub fn sign_manifest_file(payload: &str, seed: &[u8; 32]) -> String {
    let sk = SigningKey::from_bytes(seed);
    let sig = sk.sign(payload.as_bytes());
    let f = SignedFile { payload: payload.to_string(), signature: util::hex_digest(&sig.to_bytes()) };
    serde_json::to_string_pretty(&f).unwrap_or_default() + "\n"
}

/// Cle publique correspondant a une graine privee (outil de publication).
pub fn public_from_seed(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// Verifie la signature puis lit le manifeste. Le contenu n'est interprete QU'APRES la verification.
pub fn verify_signed(bytes: &[u8], key: &[u8; 32]) -> Result<Manifest, UpdateError> {
    let f: SignedFile = serde_json::from_slice(bytes).map_err(|e| UpdateError::Format(e.to_string()))?;
    let vk = VerifyingKey::from_bytes(key).map_err(|_| UpdateError::BadSignature)?;
    let raw = unhex(&f.signature).filter(|v| v.len() == 64).ok_or(UpdateError::BadSignature)?;
    let sig = Signature::from_slice(&raw).map_err(|_| UpdateError::BadSignature)?;
    vk.verify_strict(f.payload.as_bytes(), &sig).map_err(|_| UpdateError::BadSignature)?;
    serde_json::from_str(&f.payload).map_err(|e| UpdateError::Format(e.to_string()))
}

fn is_loopback_url(url: &str) -> bool {
    ["http://127.0.0.1", "http://localhost", "http://[::1]"].iter().any(|p| url.strip_prefix(p).map(|r| r.is_empty() || r.starts_with(':') || r.starts_with('/')).unwrap_or(false))
}

/// Une adresse est acceptee si elle est en https (ou en http vers cette machine quand le debogage local est actif).
pub fn url_allowed(url: &str, allow_local: bool) -> bool {
    url.starts_with("https://") || (allow_local && is_loopback_url(url))
}

fn plain_file_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', ':', '\0'])
}

impl Manifest {
    /// Controles de coherence (apres la verification de signature).
    pub fn check(&self, allow_local: bool) -> Result<(), UpdateError> {
        let bad = |m: &str| Err(UpdateError::Format(m.to_string()));
        if self.schema == 0 || self.schema > SCHEMA {
            return bad("schema inconnu");
        }
        if update::parse_version(&self.version).is_none() {
            return bad("version illisible");
        }
        if let Some(m) = &self.min_version {
            if update::parse_version(m).is_none() {
                return bad("version minimale illisible");
            }
        }
        if !url_allowed(&self.notes_url, allow_local) {
            return bad("adresse des notes non https");
        }
        if self.platforms.is_empty() {
            return bad("aucune plateforme");
        }
        for (name, a) in &self.platforms {
            if name.is_empty() || !url_allowed(&a.url, allow_local) {
                return bad("adresse d'archive non https");
            }
            if a.size == 0 || a.size > MAX_ARCHIVE_BYTES {
                return bad("taille d'archive hors limites");
            }
            if !is_hex64(&a.sha256) || a.exe_sha256.as_deref().map(|h| !is_hex64(h)).unwrap_or(false) {
                return bad("SHA-256 illisible");
            }
            if !plain_file_name(&a.member) {
                return bad("nom de fichier a extraire invalide");
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ plateforme et decision

/// Cle de plateforme du manifeste. Pure (testable).
pub fn platform_key(os: &str, arch: &str, tray: bool) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("win-x64"),
        ("linux", "x86_64") => Some(if tray { "linux-x86_64-tray" } else { "linux-x86_64" }),
        _ => None,
    }
}

/// Plateforme de CE binaire (le binaire Linux avec icone de zone de notification est une plateforme a part).
pub fn current_platform() -> Option<&'static str> {
    platform_key(std::env::consts::OS, std::env::consts::ARCH, cfg!(feature = "tray"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    UpToDate,
    /// Version mise de cote (elle ne demarrait pas, ou son fichier etait corrompu).
    Blocked,
    /// La version courante est plus ancienne que `min_version` : etape manuelle.
    ManualStep(String),
    NoPlatform,
    Available,
}

pub fn decide(m: &Manifest, current: &str, blocked: &[String], platform: Option<&str>) -> Decision {
    if !update::is_newer(&m.version, current) {
        return Decision::UpToDate;
    }
    let same = |a: &str, b: &str| match (update::parse_version(a), update::parse_version(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    };
    if blocked.iter().any(|b| same(b, &m.version)) {
        return Decision::Blocked;
    }
    if let Some(min) = &m.min_version {
        if update::is_newer(min, current) {
            return Decision::ManualStep(min.clone());
        }
    }
    match platform {
        Some(p) if m.platforms.contains_key(p) => Decision::Available,
        _ => Decision::NoPlatform,
    }
}

// ------------------------------------------------------------------ etat persistant

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub version: String,
    pub notes_url: String,
    /// Programme extrait, pret a etre installe.
    pub staged: PathBuf,
    pub exe_size: u64,
    pub exe_sha256: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Trial {
    pub from: String,
    pub to: String,
    pub notes_url: String,
    pub started_ms: u64,
}

/// Resultat a annoncer au prochain demarrage (notification + evenement d'usage).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// `ok` ou `rollback`.
    pub kind: String,
    pub from: String,
    pub to: String,
    pub notes_url: String,
    #[serde(default)]
    pub reason: String,
    pub at_ms: u64,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct State {
    #[serde(default)]
    pub last_check_ms: u64,
    #[serde(default)]
    pub last_attempt_ms: u64,
    /// Versions mises de cote : jamais reinstallees.
    #[serde(default)]
    pub blocked: Vec<String>,
    #[serde(default)]
    pub pending: Option<Pending>,
    #[serde(default)]
    pub trial: Option<Trial>,
    #[serde(default)]
    pub report: Option<Report>,
    /// Echecs deja signales (« resultat:version »), pour ne pas inonder la file d'usage.
    #[serde(default)]
    pub reported: Vec<String>,
}

pub fn update_dir(dd: &Path) -> PathBuf {
    dd.join("update")
}

pub fn load_state(dd: &Path) -> State {
    std::fs::read_to_string(update_dir(dd).join(STATE_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_state(dd: &Path, st: &State) {
    if let Ok(s) = serde_json::to_string_pretty(st) {
        let _ = util::write_atomic(&update_dir(dd).join(STATE_FILE), s.as_bytes());
    }
}

/// Lit-modifie-ecrit l'etat.
pub fn with_state<R>(dd: &Path, f: impl FnOnce(&mut State) -> R) -> R {
    let mut st = load_state(dd);
    let r = f(&mut st);
    save_state(dd, &st);
    r
}

fn marker_path(dd: &Path) -> PathBuf {
    update_dir(dd).join(MARKER_FILE)
}

/// Le marqueur « demarrage OK » de la version `version` existe-t-il ?
pub fn marker_present(dd: &Path, version: &str) -> bool {
    std::fs::read_to_string(marker_path(dd)).map(|t| t.trim() == version).unwrap_or(false)
}

fn write_marker(dd: &Path, version: &str) {
    let _ = util::write_atomic(&marker_path(dd), version.as_bytes());
}

/// Controle quotidien du jour : au plus un par jour ; apres un echec reseau, nouvel essai au bout d'une heure.
pub fn due(st: &State, now_ms: u64) -> bool {
    let checked = st.last_check_ms != 0 && st.last_check_ms <= now_ms && now_ms - st.last_check_ms < DAY_MS;
    let tried = st.last_attempt_ms != 0 && st.last_attempt_ms <= now_ms && now_ms - st.last_attempt_ms < RETRY_MS;
    !checked && !tried
}

// ------------------------------------------------------------------ telechargement et verification

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(5)
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent(&format!("AutoGameSoftwareSync/{}", update::current_version()))
        .build()
}

/// Telecharge un petit fichier (manifeste) en memoire, taille limitee.
pub fn fetch_bytes(url: &str, max: u64) -> Result<Vec<u8>, UpdateError> {
    let resp = agent().get(url).call().map_err(|e| UpdateError::Download(e.to_string()))?;
    let mut out = Vec::new();
    resp.into_reader().take(max + 1).read_to_end(&mut out).map_err(|e| UpdateError::Download(e.to_string()))?;
    if out.len() as u64 > max {
        return Err(UpdateError::Format("fichier trop gros".into()));
    }
    Ok(out)
}

/// Telecharge l'archive vers `dest` (via `dest.part`) : taille annoncee exacte, jamais plus.
pub fn download_to(url: &str, dest: &Path, expected: u64, progress: &mut dyn FnMut(u64, u64)) -> Result<(), UpdateError> {
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| UpdateError::Download(e.to_string()))?;
    }
    let resp = agent().get(url).call().map_err(|e| UpdateError::Download(e.to_string()))?;
    if let Some(len) = resp.header("Content-Length").and_then(|v| v.trim().parse::<u64>().ok()) {
        if len != expected {
            return Err(UpdateError::BadSize);
        }
    }
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let res = (|| -> Result<(), UpdateError> {
        let mut f = std::fs::File::create(&part).map_err(|e| UpdateError::Download(e.to_string()))?;
        let mut r = resp.into_reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = r.read(&mut buf).map_err(|e| UpdateError::Download(e.to_string()))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > expected {
                return Err(UpdateError::BadSize);
            }
            f.write_all(&buf[..n]).map_err(|e| UpdateError::Download(e.to_string()))?;
            progress(total, expected);
        }
        f.sync_all().map_err(|e| UpdateError::Download(e.to_string()))?;
        if total != expected {
            return Err(UpdateError::BadSize);
        }
        Ok(())
    })();
    match res {
        Ok(()) => std::fs::rename(&part, dest).map_err(|e| UpdateError::Download(e.to_string())),
        Err(e) => {
            let _ = std::fs::remove_file(&part); // notre propre fichier partiel
            Err(e)
        }
    }
}

/// Taille puis SHA-256 d'un fichier.
pub fn verify_file(path: &Path, size: u64, sha256: &str) -> Result<(), UpdateError> {
    let len = std::fs::metadata(path).map_err(|e| UpdateError::Install(e.to_string()))?.len();
    if len != size {
        return Err(UpdateError::BadSize);
    }
    let h = util::hash_file(path).map_err(|e| UpdateError::Install(e.to_string()))?;
    if !h.eq_ignore_ascii_case(sha256) {
        return Err(UpdateError::BadHash);
    }
    Ok(())
}

fn copy_limited(r: &mut dyn Read, out: &Path, max: u64) -> Result<u64, UpdateError> {
    let mut f = std::fs::File::create(out).map_err(|e| UpdateError::Install(e.to_string()))?;
    let n = std::io::copy(&mut r.take(max + 1), &mut f).map_err(|e| UpdateError::Install(e.to_string()))?;
    if n > max {
        drop(f);
        let _ = std::fs::remove_file(out);
        return Err(UpdateError::BadSize);
    }
    f.sync_all().map_err(|e| UpdateError::Install(e.to_string()))?;
    Ok(n)
}

/// Extrait UN fichier (par son nom, a n'importe quelle profondeur) d'une archive zip ou tar.gz vers `out`.
/// Aucun autre fichier n'est ecrit : pas de traversee de dossiers possible.
pub fn extract_member(archive: &Path, member: &str, out: &Path) -> Result<u64, UpdateError> {
    if !plain_file_name(member) {
        return Err(UpdateError::Format("nom de fichier a extraire invalide".into()));
    }
    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let f = std::fs::File::open(archive).map_err(|e| UpdateError::Install(e.to_string()))?;
    if name.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(f).map_err(|e| UpdateError::Install(e.to_string()))?;
        for i in 0..z.len() {
            let mut e = z.by_index(i).map_err(|e| UpdateError::Install(e.to_string()))?;
            if e.is_dir() {
                continue;
            }
            let hit = e.enclosed_name().and_then(|p| p.file_name().map(|n| n.to_string_lossy() == member)).unwrap_or(false);
            if hit {
                return copy_limited(&mut e, out, MAX_EXE_BYTES);
            }
        }
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(f));
        for e in t.entries().map_err(|e| UpdateError::Install(e.to_string()))? {
            let mut e = e.map_err(|e| UpdateError::Install(e.to_string()))?;
            if !e.header().entry_type().is_file() {
                continue;
            }
            let hit = e.path().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy() == member)).unwrap_or(false);
            if hit {
                return copy_limited(&mut e, out, MAX_EXE_BYTES);
            }
        }
    } else {
        return Err(UpdateError::Format("archive ni zip ni tar.gz".into()));
    }
    Err(UpdateError::Install(format!("{} introuvable dans l'archive", member)))
}

#[cfg(unix)]
fn make_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
}
#[cfg(not(unix))]
fn make_executable(_p: &Path) {}

/// Telecharge, verifie et extrait la version annoncee vers `<donnees>/update/<version>/`.
pub fn stage(dd: &Path, m: &Manifest, platform: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<Pending, UpdateError> {
    let a = m.platforms.get(platform).ok_or_else(|| UpdateError::Format("plateforme absente".into()))?;
    let dir = update_dir(dd).join(&m.version);
    let _ = std::fs::remove_dir_all(&dir); // dossier de travail de CETTE version (chemin construit a partir de la version verifiee)
    std::fs::create_dir_all(&dir).map_err(|e| UpdateError::Download(e.to_string()))?;
    let file = a.url.rsplit('/').next().unwrap_or("archive").split(['?', '#']).next().unwrap_or("archive");
    let file = if plain_file_name(file) { file.to_string() } else { "archive.bin".to_string() };
    let archive = dir.join(&file);
    download_to(&a.url, &archive, a.size, progress)?;
    verify_file(&archive, a.size, &a.sha256)?;
    let staged = dir.join(&a.member);
    extract_member(&archive, &a.member, &staged)?;
    let _ = std::fs::remove_file(&archive);
    let exe_size = std::fs::metadata(&staged).map_err(|e| UpdateError::Install(e.to_string()))?.len();
    let exe_sha256 = util::hash_file(&staged).map_err(|e| UpdateError::Install(e.to_string()))?;
    if let Some(want) = &a.exe_sha256 {
        if !exe_sha256.eq_ignore_ascii_case(want) {
            return Err(UpdateError::BadHash);
        }
    }
    make_executable(&staged);
    Ok(Pending { version: m.version.clone(), notes_url: m.notes_url.clone(), staged, exe_size, exe_sha256 })
}

/// Le programme extrait annonce-t-il bien la version attendue ? (`--version-file`) Un programme impossible a lancer
/// n'est pas un echec (dossier monte `noexec`...) : le SHA-256 et la signature l'ont deja garanti.
pub fn smoke_test(staged: &Path, version: &str, dd: &Path) -> Result<(), UpdateError> {
    let out = update_dir(dd).join("smoke.txt");
    let _ = std::fs::remove_file(&out);
    let mut cmd = std::process::Command::new(staged);
    cmd.arg("--version-file").arg(&out).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(mut child) = cmd.spawn() else { return Ok(()) };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(15) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(UpdateError::Install("le nouveau programme ne repond pas".into()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
    let txt = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    if txt.split_whitespace().nth(1) == Some(version) {
        Ok(())
    } else {
        Err(UpdateError::Install(format!("version annoncee incorrecte ({})", txt.trim())))
    }
}

// ------------------------------------------------------------------ remplacement de l'exe

pub fn old_path(exe: &Path) -> PathBuf {
    sibling(exe, ".old")
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut n = exe.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    n.push(suffix);
    exe.with_file_name(n)
}

/// Le dossier peut-il recevoir de nouveaux fichiers ? (Program Files sans droits : non)
pub fn dir_writable(dir: &Path) -> bool {
    let p = dir.join(format!(".agss-write-test-{}", std::process::id()));
    match std::fs::File::create(&p) {
        Ok(_) => {
            let _ = std::fs::remove_file(&p);
            true
        }
        Err(_) => false,
    }
}

/// Peut-on remplacer cet exe ? (dossier inscriptible)
pub fn can_replace(exe: &Path) -> bool {
    exe.parent().map(dir_writable).unwrap_or(false)
}

fn retry_io<T>(mut f: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut last = None;
    for i in 0..6 {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(150 * (i + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("echec")))
}

#[cfg(windows)]
fn swap_in(current: &Path, new: &Path, old: &Path) -> std::io::Result<()> {
    // Un exe en cours d'execution peut etre RENOMME (pas supprime ni ecrase) : l'ancien devient `.old`, le nouveau prend sa place.
    retry_io(|| std::fs::rename(current, old))?;
    if let Err(e) = retry_io(|| std::fs::rename(new, current)) {
        let _ = std::fs::rename(old, current);
        return Err(e);
    }
    Ok(())
}

#[cfg(not(windows))]
fn swap_in(current: &Path, new: &Path, old: &Path) -> std::io::Result<()> {
    // L'ancien reste disponible sous `.old` (lien dur : meme contenu, aucune copie), puis rename(2) ATOMIQUE.
    if std::fs::hard_link(current, old).is_err() {
        std::fs::copy(current, old)?;
    }
    std::fs::rename(new, current)
}

/// Remplace `current` par le programme `staged` : copie voisine `.new` (meme volume), l'ancien est garde sous `.old`.
/// Le chemin de l'exe ne change pas (taches planifiees, demarrage automatique, raccourcis).
pub fn replace_exe(current: &Path, staged: &Path) -> Result<PathBuf, UpdateError> {
    let dir = current.parent().ok_or_else(|| UpdateError::Install("dossier de l'exe introuvable".into()))?;
    if !dir_writable(dir) {
        return Err(UpdateError::NotWritable);
    }
    let new = sibling(current, ".new");
    let old = old_path(current);
    let _ = std::fs::remove_file(&new); // reste d'une tentative precedente (notre fichier)
    std::fs::copy(staged, &new).map_err(|e| UpdateError::Install(format!("copie : {}", e)))?;
    make_executable(&new);
    if let Ok(f) = std::fs::File::open(&new) {
        let _ = f.sync_all();
    }
    if old.exists() {
        if let Err(e) = retry_io(|| std::fs::remove_file(&old)) {
            let _ = std::fs::remove_file(&new);
            return Err(UpdateError::Install(format!("ancienne sauvegarde .old occupee : {}", e)));
        }
    }
    if let Err(e) = swap_in(current, &new, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(UpdateError::Install(format!("remplacement : {}", e)));
    }
    Ok(old)
}

/// Retour arriere : remet `.old` a la place de l'exe.
pub fn rollback_exe(current: &Path) -> Result<(), UpdateError> {
    let old = old_path(current);
    if !old.is_file() {
        return Err(UpdateError::Install("ancienne version (.old) introuvable".into()));
    }
    #[cfg(windows)]
    {
        let failed = sibling(current, ".failed");
        let _ = std::fs::remove_file(&failed);
        if current.exists() {
            retry_io(|| std::fs::rename(current, &failed)).map_err(|e| UpdateError::Install(format!("retrait de la version fautive : {}", e)))?;
        }
        if let Err(e) = retry_io(|| std::fs::rename(&old, current)) {
            let _ = std::fs::rename(&failed, current);
            return Err(UpdateError::Install(format!("remise en place : {}", e)));
        }
        let _ = std::fs::remove_file(&failed);
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&old, current).map_err(|e| UpdateError::Install(format!("remise en place : {}", e)))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ installation (cote ancien programme)

/// Arguments du veilleur : `--update-watchdog <donnees> <exe> <pid de l'ancien> <secondes> <tray|normal>`.
pub fn watchdog_args(dd: &Path, exe: &Path, old_pid: u32, secs: u64, hidden: bool) -> Vec<String> {
    vec!["--update-watchdog".into(), dd.display().to_string(), exe.display().to_string(), old_pid.to_string(), secs.to_string(), if hidden { "tray".into() } else { "normal".into() }]
}

/// Duree de surveillance (debogage : AGSS_UPDATE_WATCH_SECS).
pub fn watch_secs() -> u64 {
    std::env::var(ENV_WATCH).ok().and_then(|v| v.parse().ok()).filter(|n| (3..=600).contains(n)).unwrap_or(WATCH_SECS)
}

/// Installe la version telechargee : verification finale du fichier, essai de lancement, remplacement, veilleur.
/// `spawn` lance le veilleur (injectable). En cas d'echec apres le remplacement, l'ancien exe est remis en place.
/// Succes : l'appelant doit QUITTER aussitot (le veilleur relance la nouvelle version).
pub fn apply_pending(dd: &Path, exe: &Path, p: &Pending, hidden: bool, now_ms: u64, spawn: &dyn Fn(&Path, &[String]) -> std::io::Result<()>) -> Result<(), UpdateError> {
    verify_file(&p.staged, p.exe_size, &p.exe_sha256)?;
    smoke_test(&p.staged, &p.version, dd)?;
    let current = update::current_version().to_string();
    with_state(dd, |st| {
        st.trial = Some(Trial { from: current.clone(), to: p.version.clone(), notes_url: p.notes_url.clone(), started_ms: now_ms });
        st.report = None;
    });
    let _ = std::fs::remove_file(marker_path(dd));
    let old = match replace_exe(exe, &p.staged) {
        Ok(o) => o,
        Err(e) => {
            with_state(dd, |st| st.trial = None);
            return Err(e);
        }
    };
    let args = watchdog_args(dd, exe, std::process::id(), watch_secs(), hidden);
    if let Err(e) = spawn(&old, &args) {
        let rb = rollback_exe(exe);
        with_state(dd, |st| st.trial = None);
        return Err(UpdateError::Install(format!("veilleur : {}{}", e, if rb.is_err() { " (retour arriere impossible !)" } else { "" })));
    }
    Ok(())
}

/// Linux : le veilleur devient chef de sa propre session (independant du processus appelant).
fn detach_unix(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let _ = cmd;
}

pub fn spawn_watchdog(old_exe: &Path, args: &[String]) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(old_exe);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP : sans console, independant
    }
    detach_unix(&mut cmd);
    cmd.spawn().map(|_| ())
}

// ------------------------------------------------------------------ veilleur

pub trait ChildProc {
    fn has_exited(&mut self) -> bool;
    fn kill(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Healthy,
    /// Le nouveau programme s'est arrete sans signaler « demarrage OK ».
    Crashed,
    Timeout,
}

/// Surveille le nouveau programme : `max_polls` verifications espacees par `sleep`.
pub fn supervise(child: &mut dyn ChildProc, marker_ok: &mut dyn FnMut() -> bool, max_polls: u32, sleep: &mut dyn FnMut()) -> Verdict {
    for _ in 0..max_polls {
        if marker_ok() {
            return Verdict::Healthy;
        }
        if child.has_exited() {
            return if marker_ok() { Verdict::Healthy } else { Verdict::Crashed };
        }
        sleep();
    }
    if marker_ok() {
        Verdict::Healthy
    } else {
        Verdict::Timeout
    }
}

/// Retour arriere complet : ancien exe remis en place, version bloquee, rapport pour le prochain demarrage.
pub fn perform_rollback(dd: &Path, exe: &Path, reason: &str, now_ms: u64) -> Result<(), UpdateError> {
    let r = rollback_exe(exe);
    with_state(dd, |st| {
        if let Some(t) = st.trial.take() {
            if !st.blocked.contains(&t.to) {
                st.blocked.push(t.to.clone());
            }
            st.report = Some(Report { kind: "rollback".into(), from: t.from, to: t.to.clone(), notes_url: t.notes_url, reason: reason.to_string(), at_ms: now_ms });
            st.pending = None;
            let _ = std::fs::remove_dir_all(update_dir(dd).join(&t.to)); // dossier de travail de la version fautive
        }
    });
    r
}

struct RealChild(std::process::Child);

impl ChildProc for RealChild {
    fn has_exited(&mut self) -> bool {
        !matches!(self.0.try_wait(), Ok(None))
    }
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Lance l'application (`tray` : en arriere-plan ; `after` : marque un demarrage apres mise a jour).
pub fn spawn_app(exe: &Path, tray: bool, after: bool) -> std::io::Result<std::process::Child> {
    let mut cmd = std::process::Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP (application graphique : aucune console)
    }
    if tray {
        cmd.arg("--tray");
    } else if after {
        cmd.arg("gui");
    }
    if after {
        cmd.arg("--after-update");
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn()
}

/// Un processus a-t-il disparu ? (Linux : un zombie compte comme disparu)
pub fn pid_gone(pid: u32) -> bool {
    #[cfg(not(windows))]
    {
        if let Ok(s) = std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
            if let Some(after) = s.rsplit(')').next() {
                if after.trim_start().starts_with('Z') {
                    return true;
                }
            }
        }
    }
    !crate::process::pid_alive(pid)
}

/// Mode `--update-watchdog` : attend la fin de l'ancien programme, lance la nouvelle version et la surveille ; au moindre
/// doute (arret, pas de marqueur avant `secs`), retour arriere et relance de l'ancienne version.
pub fn run_watchdog(dd: &Path, exe: &Path, old_pid: u32, secs: u64, hidden: bool) -> i32 {
    let log = Logger::new(dd.join("logs").join("watch.log"), false);
    let to = load_state(dd).trial.as_ref().map(|t| t.to.clone()).unwrap_or_default();
    log.log(&format!("[MiseAJour] veilleur demarre : attente de la fin de l'ancien programme (pid {}), puis surveillance de la version {} ({} s)", old_pid, to, secs));
    let mut waited = 0;
    while !pid_gone(old_pid) && waited < 180 {
        std::thread::sleep(Duration::from_millis(500));
        waited += 1;
    }
    if !pid_gone(old_pid) {
        log.log("[MiseAJour] l'ancien programme ne se ferme pas : retour arriere des fichiers, rien n'est relance");
        let _ = perform_rollback(dd, exe, "old_stuck", util::now_ms());
        with_state(dd, |st| {
            // Ce n'est pas la faute de la nouvelle version : elle n'est pas bloquee.
            if let Some(r) = st.report.take() {
                st.blocked.retain(|b| *b != r.to);
            }
        });
        return 1;
    }
    let child = match spawn_app(exe, hidden, true) {
        Ok(c) => c,
        Err(e) => {
            log.log(&format!("[MiseAJour] la nouvelle version ne se lance pas ({}) : retour arriere", e));
            return finish_rollback(dd, exe, hidden, &log, "spawn_failed", None);
        }
    };
    let mut child = RealChild(child);
    let to2 = to.clone();
    let dd2 = dd.to_path_buf();
    let verdict = supervise(&mut child, &mut || marker_present(&dd2, &to2), (secs * 4) as u32, &mut || std::thread::sleep(Duration::from_millis(250)));
    match verdict {
        Verdict::Healthy => {
            log.log(&format!("[MiseAJour] la version {} a demarre correctement", to));
            0
        }
        v => {
            log.log(&format!("[MiseAJour] la version {} ne demarre pas correctement ({:?}) : retour a l'ancienne version", to, v));
            finish_rollback(dd, exe, hidden, &log, if v == Verdict::Crashed { "crashed" } else { "timeout" }, Some(&mut child))
        }
    }
}

fn finish_rollback(dd: &Path, exe: &Path, hidden: bool, log: &Logger, reason: &str, child: Option<&mut RealChild>) -> i32 {
    if let Some(c) = child {
        c.kill();
    }
    match perform_rollback(dd, exe, reason, util::now_ms()) {
        Ok(()) => {
            log.log("[MiseAJour] ancienne version remise en place ; la version fautive est bloquee");
            match spawn_app(exe, hidden, false) {
                Ok(_) => 0,
                Err(e) => {
                    log.log(&format!("[MiseAJour] relance de l'ancienne version impossible : {}", e));
                    1
                }
            }
        }
        Err(e) => {
            log.log(&format!("[MiseAJour] RETOUR ARRIERE IMPOSSIBLE : {}", e));
            1
        }
    }
}

// ------------------------------------------------------------------ cote nouvelle version

/// Appele par la nouvelle version quand elle est en bonne sante (interface lancee) : ecrit le marqueur « demarrage OK »,
/// clot l'essai et renvoie ce qu'il faut annoncer (mise a jour reussie, ou retour arriere decide par le veilleur).
pub fn confirm_started(dd: &Path, current: &str, now_ms: u64) -> Option<Report> {
    let mut st = load_state(dd);
    let mut out = None;
    if let Some(t) = st.trial.take() {
        if t.to == current {
            write_marker(dd, current);
            out = Some(Report { kind: "ok".into(), from: t.from, to: t.to.clone(), notes_url: t.notes_url, reason: String::new(), at_ms: now_ms });
            st.pending = None;
            let _ = std::fs::remove_dir_all(update_dir(dd).join(&t.to));
        }
        // sinon : essai obsolete (version remplacee a la main), simplement clos
    }
    if out.is_none() {
        out = st.report.take();
    }
    save_state(dd, &st);
    out
}

/// Demarrage `--after-update` dont l'instance est deja prise : l'application tourne, le veilleur peut etre rassure.
pub fn confirm_marker_only(dd: &Path, current: &str) {
    if load_state(dd).trial.map(|t| t.to == current).unwrap_or(false) {
        write_marker(dd, current);
    }
}

/// Une version en attente (telechargee) toujours valable : plus recente, non bloquee, fichier intact.
pub fn valid_pending(dd: &Path, current: &str) -> Option<Pending> {
    let st = load_state(dd);
    let p = st.pending?;
    if !update::is_newer(&p.version, current) || st.blocked.contains(&p.version) || verify_file(&p.staged, p.exe_size, &p.exe_sha256).is_err() {
        return None;
    }
    Some(p)
}

// ------------------------------------------------------------------ etat partage avec l'interface

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// Version annoncee (`installable` faux : dossier non inscriptible, plateforme absente, etape manuelle).
    Available { version: String, url: String, installable: bool },
    Downloading { version: String, done: u64, total: u64 },
    /// Telechargee et verifiee ; `now` : l'utilisateur l'a demandee (installation sans attendre un moment calme).
    Ready { version: String, now: bool },
    Failed { version: String },
}

pub struct Runtime {
    pub phase: Mutex<Phase>,
    offered: Mutex<Option<(Manifest, String)>>,
    /// Clic sur « Mettre a jour » (mode « Me demander »).
    pub install_requested: AtomicBool,
    /// Installation demandee en cours : `Ready.now`.
    user_wanted: AtomicBool,
}

impl Default for Runtime {
    fn default() -> Self {
        Runtime { phase: Mutex::new(Phase::Idle), offered: Mutex::new(None), install_requested: AtomicBool::new(false), user_wanted: AtomicBool::new(false) }
    }
}

/// Ce que montre le bandeau de l'interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BannerView {
    None,
    /// Nouvelle version : bouton « Mettre a jour » si `installable`, sinon « Ouvrir la page de telechargement ».
    Offer { version: String, url: String, installable: bool },
    Progress { version: String, percent: Option<u32> },
    Installing { version: String },
    Failed { version: String, url: String },
}

/// Choix du bandeau : PUR (testable). En mode Automatique tout est silencieux sauf quand une etape manuelle est necessaire.
pub fn banner_view(mode: crate::settings::UpdateMode, phase: &Phase, announced: Option<&ReleaseInfo>) -> BannerView {
    use crate::settings::UpdateMode::*;
    if mode == Never {
        return BannerView::None;
    }
    let url_of = |v: &str| announced.filter(|r| r.tag.trim_start_matches(['v', 'V']) == v).map(|r| r.url.clone()).or_else(|| announced.map(|r| r.url.clone())).unwrap_or_default();
    match phase {
        Phase::Idle => match (mode, announced) {
            (Ask, Some(r)) => BannerView::Offer { version: r.tag.clone(), url: r.url.clone(), installable: false },
            _ => BannerView::None,
        },
        Phase::Available { version, url, installable } => BannerView::Offer { version: version.clone(), url: url.clone(), installable: *installable },
        Phase::Downloading { version, done, total } => {
            if mode == Ask {
                BannerView::Progress { version: version.clone(), percent: if *total > 0 { Some((done * 100 / total) as u32) } else { None } }
            } else {
                BannerView::None
            }
        }
        Phase::Ready { version, now } => {
            if *now {
                BannerView::Installing { version: version.clone() }
            } else if mode == Ask {
                BannerView::Offer { version: version.clone(), url: url_of(version), installable: true }
            } else {
                BannerView::None
            }
        }
        Phase::Failed { version } => BannerView::Failed { version: version.clone(), url: url_of(version) },
    }
}

impl Runtime {
    /// Clic sur « Mettre a jour » : telechargement lance, ou installation sans attendre si la version est deja prete.
    pub fn request_install(&self) {
        let mut g = match self.phase.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match &*g {
            Phase::Ready { version, .. } => *g = Phase::Ready { version: version.clone(), now: true },
            Phase::Available { version, .. } => {
                self.install_requested.store(true, Ordering::SeqCst);
                *g = Phase::Downloading { version: version.clone(), done: 0, total: 0 };
            }
            _ => {}
        }
    }

    /// Remet l'etat au repos (la version prete n'est plus valable).
    pub fn reset(&self) {
        self.user_wanted.store(false, Ordering::SeqCst);
        self.set(Phase::Idle);
    }

    /// Echec de l'installation d'une version.
    pub fn fail(&self, version: &str) {
        self.user_wanted.store(false, Ordering::SeqCst);
        self.set(Phase::Failed { version: version.to_string() });
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().map(|p| p.clone()).unwrap_or(Phase::Idle)
    }
    fn set(&self, p: Phase) {
        if let Ok(mut g) = self.phase.lock() {
            *g = p;
        }
    }
}

/// Fonctions fournies par l'interface au thread de mise a jour.
pub struct Env {
    /// Un programme detecte tourne, plein ecran, ou une synchro est en cours : aucun controle.
    pub busy: Box<dyn Fn() -> bool + Send + Sync>,
    pub quitting: Box<dyn Fn() -> bool + Send + Sync>,
    /// Derniere version publiee connue (annonce, info-bulle de l'icone).
    pub on_latest: Box<dyn Fn(Option<ReleaseInfo>) + Send + Sync>,
    pub repaint: Box<dyn Fn() + Send + Sync>,
    /// Moment calme pour installer seul (aucun programme suivi, synchro, tutoriel ou fenetre de confirmation ouverts).
    pub calm: Box<dyn Fn() -> bool + Send + Sync>,
    /// Une synchro est en cours.
    pub syncing: Box<dyn Fn() -> bool + Send + Sync>,
    /// L'application tourne en arriere-plan (fenetre cachee) : elle sera relancee avec --tray.
    pub hidden: Box<dyn Fn() -> bool + Send + Sync>,
    /// Fermeture de l'application (ne revient pas) une fois le remplacement fait.
    pub quit: Box<dyn Fn() + Send + Sync>,
    /// Echec de l'installation : message pour l'interface.
    pub failed: Box<dyn Fn(&str) + Send + Sync>,
}

fn allow_local() -> bool {
    std::env::var_os(ENV_URL).is_some()
}

fn debug_url() -> Option<String> {
    std::env::var(ENV_URL).ok().filter(|u| !u.trim().is_empty())
}

/// Evenement d'usage `update` (le consentement est verifie par `usage::push`).
pub fn record_usage(dd: &Path, from: &str, to: &str, result: &str) {
    crate::usage::record(dd, "update", &[("from", from), ("to", to), ("result", result)]);
}

fn report_once(dd: &Path, from: &str, to: &str, result: &str) {
    let key = format!("{}:{}", result, to);
    let first = with_state(dd, |st| {
        if st.reported.contains(&key) {
            false
        } else {
            st.reported.push(key);
            if st.reported.len() > 40 {
                st.reported.remove(0);
            }
            true
        }
    });
    if first {
        record_usage(dd, from, to, result);
    }
}

/// Echec d'installation d'une version prete : evenement d'usage (une fois), version prete oubliee, nouvel essai demain.
pub fn record_failure(dd: &Path, from: &str, to: &str, e: &UpdateError) {
    report_once(dd, from, to, e.code());
    with_state(dd, |st| {
        st.pending = None;
        st.last_attempt_ms = util::now_ms();
        if matches!(e, UpdateError::BadHash | UpdateError::BadSize) && !st.blocked.iter().any(|b| b == to) {
            st.blocked.push(to.to_string());
        }
    });
}

pub fn spawn_worker(dd: PathBuf, rt: Arc<Runtime>, env: Env) {
    let _ = std::thread::Builder::new().name("agss-update".into()).spawn(move || worker(dd, rt, env));
}

fn sleep_secs(env: &Env, n: u64) -> bool {
    for _ in 0..n {
        std::thread::sleep(Duration::from_secs(1));
        if (env.quitting)() {
            return false;
        }
    }
    true
}

fn worker(dd: PathBuf, rt: Arc<Runtime>, env: Env) {
    let lg = Logger::new(dd.join("logs").join("watch.log"), false);
    if !sleep_secs(&env, 15) {
        return;
    }
    loop {
        if (env.quitting)() {
            return;
        }
        let s = crate::settings::Settings::load_from(&dd).unwrap_or_default();
        let mode = s.update_mode();
        if mode == crate::settings::UpdateMode::Never {
            rt.set(Phase::Idle);
            if !sleep_secs(&env, 5) {
                return;
            }
            continue;
        }
        let current = update::current_version();
        // Version deja telechargee lors d'une session precedente.
        if matches!(rt.phase(), Phase::Idle) {
            if let Some(p) = valid_pending(&dd, current) {
                lg.log(&format!("[MiseAJour] version {} deja telechargee et verifiee : installation au prochain moment calme", p.version));
                rt.set(Phase::Ready { version: p.version, now: false });
                (env.repaint)();
            }
        }
        if rt.install_requested.swap(false, Ordering::SeqCst) {
            let offered = rt.offered.lock().ok().and_then(|o| o.clone());
            if let Some((m, plat)) = offered {
                rt.user_wanted.store(true, Ordering::SeqCst);
                stage_and_publish(&dd, &rt, &env, &lg, &m, &plat, current);
            }
        } else if !(env.busy)() {
            let st = load_state(&dd);
            if due(&st, util::now_ms()) && !matches!(rt.phase(), Phase::Ready { .. } | Phase::Downloading { .. }) {
                check_cycle(&dd, &rt, &env, &lg, mode, current);
            }
        }
        // Installation : au moment calme (Automatique) ou tout de suite apres un clic (Me demander). Faite ici et non dans
        // l'interface : une fenetre cachee (zone de notification) ne se redessine plus.
        if let Phase::Ready { now, .. } = rt.phase() {
            let go = match mode {
                crate::settings::UpdateMode::Auto => (env.calm)(),
                crate::settings::UpdateMode::Ask => now && !(env.syncing)(),
                crate::settings::UpdateMode::Never => false,
            };
            if go {
                apply_now(&dd, &rt, &env, &lg, current);
            }
        }
        if !sleep_secs(&env, 3) {
            return;
        }
    }
}

fn apply_now(dd: &Path, rt: &Runtime, env: &Env, lg: &Logger, current: &str) {
    let Some(p) = valid_pending(dd, current) else {
        rt.reset();
        return;
    };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            lg.log(&format!("[MiseAJour] exe introuvable : {}", e));
            rt.fail(&p.version);
            return;
        }
    };
    lg.log(&format!("[MiseAJour] installation de la version {} (remplacement de {})", p.version, exe.display()));
    match apply_pending(dd, &exe, &p, (env.hidden)(), util::now_ms(), &spawn_watchdog) {
        Ok(()) => {
            lg.log("[MiseAJour] exe remplace, veilleur lance : fermeture pour redemarrer sur la nouvelle version");
            (env.quit)();
        }
        Err(e) => {
            lg.log(&format!("[MiseAJour] installation impossible : {}", e));
            record_failure(dd, current, &p.version, &e);
            rt.fail(&p.version);
            (env.failed)(&p.version);
        }
    }
    (env.repaint)();
}

fn stage_and_publish(dd: &Path, rt: &Runtime, env: &Env, lg: &Logger, m: &Manifest, plat: &str, current: &str) {
    rt.set(Phase::Downloading { version: m.version.clone(), done: 0, total: 0 });
    (env.repaint)();
    lg.log(&format!("[MiseAJour] telechargement de la version {} ({})", m.version, plat));
    let mut last = std::time::Instant::now() - Duration::from_secs(1);
    let ver = m.version.clone();
    let res = stage(dd, m, plat, &mut |done, total| {
        if last.elapsed() > Duration::from_millis(250) {
            last = std::time::Instant::now();
            rt.set(Phase::Downloading { version: ver.clone(), done, total });
            (env.repaint)();
        }
    });
    match res {
        Ok(p) => {
            lg.log(&format!("[MiseAJour] version {} telechargee et verifiee (SHA-256 {}...)", p.version, &p.exe_sha256[..12.min(p.exe_sha256.len())]));
            with_state(dd, |st| {
                st.pending = Some(p.clone());
            });
            rt.set(Phase::Ready { version: p.version, now: rt.user_wanted.load(Ordering::SeqCst) });
        }
        Err(e) => {
            lg.log(&format!("[MiseAJour] echec pour la version {} : {}", m.version, e));
            report_once(dd, current, &m.version, e.code());
            let fatal = matches!(e, UpdateError::BadHash | UpdateError::BadSize | UpdateError::BadSignature);
            with_state(dd, |st| {
                st.last_attempt_ms = util::now_ms();
                st.last_check_ms = 0;
                if fatal && !st.blocked.contains(&m.version) {
                    st.blocked.push(m.version.clone());
                }
            });
            rt.user_wanted.store(false, Ordering::SeqCst);
            rt.set(Phase::Failed { version: m.version.clone() });
        }
    }
    (env.repaint)();
}

fn fetch_manifest(dbg: &Option<String>) -> Result<(Vec<u8>, Option<ReleaseInfo>), String> {
    match dbg {
        Some(u) => fetch_bytes(u, MAX_MANIFEST_BYTES).map(|b| (b, None)).map_err(|e| e.to_string()),
        None => fetch_bytes(MANIFEST_URL, MAX_MANIFEST_BYTES).map(|b| (b, None)).map_err(|e| e.to_string()),
    }
}

fn check_cycle(dd: &Path, rt: &Runtime, env: &Env, lg: &Logger, mode: crate::settings::UpdateMode, current: &str) {
    use crate::settings::UpdateMode;
    let now = util::now_ms();
    let dbg = debug_url();
    let mut api: Option<ReleaseInfo> = None;
    if dbg.is_none() {
        // Annonce (API publique des Releases, ETag) : sert aussi quand la Release n'a pas de manifeste.
        let mut cache = update::load_cache(dd);
        let f = update::fetch_latest(cache.etag.as_deref());
        let offline = matches!(f, update::Fetched::Offline(_));
        update::apply(&mut cache, f);
        update::save_cache(dd, &cache);
        (env.on_latest)(cache.latest.clone());
        if offline {
            with_state(dd, |st| st.last_attempt_ms = now);
            return;
        }
        match cache.latest {
            Some(r) if update::is_newer(&r.tag, current) => api = Some(r),
            _ => {
                with_state(dd, |st| st.last_check_ms = now);
                rt.set(Phase::Idle);
                return;
            }
        }
    }
    let bytes = match fetch_manifest(&dbg) {
        Ok((b, _)) => b,
        Err(e) => {
            lg.log(&format!("[MiseAJour] manifeste indisponible : {}", e));
            with_state(dd, |st| {
                st.last_attempt_ms = now;
                // Release sans manifeste (ancienne version) : l'annonce seule suffit, un controle par jour.
                if api.is_some() {
                    st.last_check_ms = now;
                }
            });
            if let Some(r) = api {
                rt.set(Phase::Available { version: r.tag, url: r.url, installable: false });
                (env.repaint)();
            }
            return;
        }
    };
    with_state(dd, |st| st.last_check_ms = now);
    let manifest = match verify_signed(&bytes, &public_key()).and_then(|m| m.check(allow_local()).map(|_| m)) {
        Ok(m) => m,
        Err(e) => {
            lg.log(&format!("[MiseAJour] manifeste refuse : {}", e));
            let to = api.as_ref().map(|r| r.tag.trim_start_matches(['v', 'V']).to_string());
            if let Some(to) = to.filter(|t| crate::usage::is_version(t)) {
                report_once(dd, current, &to, e.code());
            }
            if let Some(r) = api {
                rt.set(Phase::Available { version: r.tag, url: r.url, installable: false });
                (env.repaint)();
            }
            return;
        }
    };
    let blocked = load_state(dd).blocked;
    let plat = current_platform();
    let page = ReleaseInfo { tag: manifest.version.clone(), url: manifest.notes_url.clone() };
    match decide(&manifest, current, &blocked, plat) {
        Decision::UpToDate => {
            rt.set(Phase::Idle);
            if dbg.is_some() {
                (env.on_latest)(Some(page));
            }
        }
        Decision::Blocked => {
            lg.log(&format!("[MiseAJour] version {} mise de cote (elle ne demarrait pas ou son fichier etait invalide) : ignoree", manifest.version));
            rt.set(Phase::Idle);
        }
        d @ (Decision::ManualStep(_) | Decision::NoPlatform) => {
            lg.log(&format!("[MiseAJour] version {} disponible, installation manuelle ({:?})", manifest.version, d));
            (env.on_latest)(Some(page.clone()));
            rt.set(Phase::Available { version: page.tag, url: page.url, installable: false });
        }
        Decision::Available => {
            let plat = plat.unwrap_or_default().to_string();
            (env.on_latest)(Some(page.clone()));
            let writable = std::env::current_exe().map(|e| can_replace(&e)).unwrap_or(false);
            if !writable {
                lg.log(&format!("[MiseAJour] version {} disponible mais le dossier du programme n'est pas modifiable : mise a jour manuelle (page de la Release)", manifest.version));
                rt.set(Phase::Available { version: page.tag, url: page.url, installable: false });
            } else if mode == UpdateMode::Auto {
                if let Ok(mut o) = rt.offered.lock() {
                    *o = Some((manifest.clone(), plat.clone()));
                }
                rt.user_wanted.store(false, Ordering::SeqCst);
                stage_and_publish(dd, rt, env, lg, &manifest, &plat, current);
            } else {
                if let Ok(mut o) = rt.offered.lock() {
                    *o = Some((manifest.clone(), plat));
                }
                rt.set(Phase::Available { version: page.tag, url: page.url, installable: true });
            }
        }
    }
    (env.repaint)();
}

#[cfg(test)]
mod tests;
