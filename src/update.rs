//! Alerte de nouvelle version : interroge l'API publique des Releases GitHub (anonyme, ETag), compare en semver avec
//! l'exe courant. L'installation automatique (manifeste signe, SHA-256, retour arriere) est dans `selfupdate`.
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const LATEST_URL: &str = "https://api.github.com/repos/Epikou/AutoGameSoftwareSync/releases/latest";
/// Intervalle entre deux controles.
pub const CHECK_EVERY_SECS: u64 = 24 * 3600;
const CACHE_FILE: &str = "update-cache.json";

/// Version courante de l'exe (Cargo.toml).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ------------------------------------------------------------------ semver

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Semver {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Identifiants de pre-version (`beta.2` -> ["beta", "2"]) ; vide = version finale.
    pub pre: Vec<String>,
}

/// `v1.2.3`, `1.2`, `1.2.3-beta.1`, `1.2.3+build` (les metadonnees `+...` sont ignorees).
pub fn parse_version(s: &str) -> Option<Semver> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split('+').next()?;
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(|x| x.to_string()).collect::<Vec<_>>()),
        None => (s, vec![]),
    };
    if pre.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
    let patch = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
    if it.next().is_some() {
        return None;
    }
    Some(Semver { major, minor, patch, pre })
}

impl Ord for Semver {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(o.major, o.minor, o.patch)).then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater, // 1.0.0 > 1.0.0-beta
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(o.pre.iter()) {
                    let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less, // numerique < alphanumerique
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                self.pre.len().cmp(&o.pre.len())
            }
        })
    }
}

impl PartialOrd for Semver {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// `latest` est-elle strictement plus recente que `current` ? (version illisible : faux)
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

// ------------------------------------------------------------------ reponse de l'API

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    /// Page de la Release (ouverte dans le navigateur).
    pub url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Lit la reponse JSON de `releases/latest`. Les brouillons et pre-versions sont ignores ; l'URL doit etre en https.
pub fn parse_release(json: &str) -> Option<ReleaseInfo> {
    let r: ApiRelease = serde_json::from_str(json).ok()?;
    if r.draft || r.prerelease || parse_version(&r.tag_name).is_none() || !r.html_url.starts_with("https://") {
        return None;
    }
    Some(ReleaseInfo { tag: r.tag_name, url: r.html_url })
}

// ------------------------------------------------------------------ bandeau

/// Release a annoncer : plus recente que `current` et differente de la version « ignoree ». Une version PLUS RECENTE que
/// celle qu'on a ignoree est annoncee de nouveau.
pub fn banner<'a>(latest: Option<&'a ReleaseInfo>, current: &str, ignored: Option<&str>, enabled: bool) -> Option<&'a ReleaseInfo> {
    if !enabled {
        return None;
    }
    let r = latest?;
    if !is_newer(&r.tag, current) {
        return None;
    }
    match ignored {
        Some(i) if !is_newer(&r.tag, i) => None,
        _ => Some(r),
    }
}

// ------------------------------------------------------------------ reseau (hors ligne tolere)

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// 304 : rien de nouveau depuis le dernier controle.
    NotModified,
    New { info: ReleaseInfo, etag: Option<String> },
    /// Pas de reseau, API indisponible, limite atteinte, reponse illisible : silencieux, reessai au prochain controle.
    Offline(String),
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Cache {
    pub etag: Option<String>,
    pub latest: Option<ReleaseInfo>,
}

pub fn load_cache(dir: &std::path::Path) -> Cache {
    std::fs::read_to_string(dir.join(CACHE_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_cache(dir: &std::path::Path, c: &Cache) {
    if let Ok(s) = serde_json::to_string(c) {
        let _ = crate::util::write_atomic(&dir.join(CACHE_FILE), s.as_bytes());
    }
}

/// Un controle : GET anonyme (aucune donnee personnelle ; seulement `User-Agent: AutoGameSoftwareSync/<version>`),
/// `If-None-Match` si un ETag est connu. Jamais bloquant plus de 10 s.
pub fn fetch(url: &str, etag: Option<&str>) -> Fetched {
    let mut req = ureq::AgentBuilder::new().redirects(5).timeout(std::time::Duration::from_secs(10)).build().get(url).set("User-Agent", &format!("AutoGameSoftwareSync/{}", current_version())).set("Accept", "application/vnd.github+json");
    if let Some(e) = etag {
        req = req.set("If-None-Match", e);
    }
    match req.call() {
        Ok(resp) => {
            let et = resp.header("ETag").map(|s| s.to_string());
            match resp.into_string().ok().and_then(|b| parse_release(&b)) {
                Some(info) => Fetched::New { info, etag: et },
                None => Fetched::Offline("reponse inattendue".into()),
            }
        }
        Err(ureq::Error::Status(304, _)) => Fetched::NotModified,
        Err(e) => Fetched::Offline(e.to_string()),
    }
}

/// Applique un resultat au cache : en cas d'echec reseau, le dernier resultat connu est conserve.
pub fn apply(cache: &mut Cache, f: Fetched) {
    if let Fetched::New { info, etag } = f {
        cache.latest = Some(info);
        cache.etag = etag;
    }
}

/// Adresses a interroger, dans l'ordre.
pub fn candidate_urls() -> [&'static str; 1] {
    [LATEST_URL]
}

/// Premier resultat qui n'est pas un echec reseau / adresse inexistante ; sinon le dernier echec.
pub fn first_answer(attempts: &mut [&mut dyn FnMut() -> Fetched]) -> Fetched {
    let mut last = Fetched::Offline("aucune adresse".into());
    for a in attempts.iter_mut() {
        let r = a();
        if !matches!(r, Fetched::Offline(_)) {
            return r;
        }
        last = r;
    }
    last
}

/// Controle complet (redirections suivies).
pub fn fetch_latest(etag: Option<&str>) -> Fetched {
    let [a] = candidate_urls();
    let mut f1 = || fetch(a, etag);
    first_answer(&mut [&mut f1])
}
