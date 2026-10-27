//! Couche 4 : recherche web discrete (aucune fenetre, aucun reglage, rien dans l'interface). Requetes
//! « "<produit>" save game location » et « "<produit>" config file location ». Moteurs de repli, dans l'ordre :
//! DuckDuckGo HTML, DuckDuckGo lite, Brave Search, Mojeek, Startpage ; puis sites cibles (magasin Steam pour le nom exact
//! et l'editeur, Wikipedia). Lit 3 a 5 resultats, en extrait les chemins Windows et Linux, les convertit en gabarits,
//! les ramene au PC courant et VERIFIE sur le disque (le dossier existe et contient des fichiers recents) : rien n'est
//! propose sans cette verification.
//!
//! Garde-fous : un moteur qui bloque ou change de format est mis en pause automatiquement ; quelques recherches par jour
//! au plus ; jamais pendant un jeu (verifie par l'appelant) ; delai reseau court ; User-Agent d'un navigateur courant
//! (aucun cookie, aucune autre identite) ; cache local par programme, nouvel essai apres quelques jours.
use super::paths::{self, Folders};
use super::{best_match, kind_for, Found, Layer, Program, RECENT_DAYS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Programmes cherches par jour au plus.
pub const MAX_PER_DAY: u32 = 4;
/// Delai reseau (secondes).
pub const TIMEOUT_S: u64 = 6;
/// Pages de resultats lues par requete.
pub const MAX_PAGES: usize = 4;
/// Taille maximale lue par page.
pub const MAX_BODY: u64 = 1_500_000;
/// User-Agent d'un navigateur courant.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Reponse HTTP (code et corps texte).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resp {
    pub status: u16,
    pub body: String,
}

/// Acces reseau (simule en test). `Err` : aucune reponse (hors ligne, delai depasse...).
pub trait Fetch {
    fn get(&self, url: &str) -> Result<Resp, String>;
}

pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Http {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(TIMEOUT_S))
            .timeout(std::time::Duration::from_secs(TIMEOUT_S + 2))
            .user_agent(USER_AGENT)
            .redirects(4)
            .build();
        Http { agent }
    }
}

fn read_body(r: ureq::Response) -> String {
    use std::io::Read;
    let mut buf = Vec::new();
    let _ = r.into_reader().take(MAX_BODY).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

impl Fetch for Http {
    fn get(&self, url: &str) -> Result<Resp, String> {
        let req = self.agent.get(url).set("Accept", "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8").set("Accept-Language", "en-US,en;q=0.8");
        match req.call() {
            Ok(r) => Ok(Resp { status: r.status(), body: read_body(r) }),
            Err(ureq::Error::Status(code, r)) => Ok(Resp { status: code, body: read_body(r) }),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Moteurs de recherche, dans l'ordre de repli.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Engine {
    DdgHtml,
    DdgLite,
    Brave,
    Mojeek,
    Startpage,
}

impl Engine {
    pub const ALL: [Engine; 5] = [Engine::DdgHtml, Engine::DdgLite, Engine::Brave, Engine::Mojeek, Engine::Startpage];

    pub fn code(self) -> &'static str {
        match self {
            Engine::DdgHtml => "ddg-html",
            Engine::DdgLite => "ddg-lite",
            Engine::Brave => "brave",
            Engine::Mojeek => "mojeek",
            Engine::Startpage => "startpage",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Engine::DdgHtml => "DuckDuckGo HTML",
            Engine::DdgLite => "DuckDuckGo lite",
            Engine::Brave => "Brave Search",
            Engine::Mojeek => "Mojeek",
            Engine::Startpage => "Startpage",
        }
    }

    pub fn url(self, q: &str) -> String {
        let q = urlencode(q);
        match self {
            Engine::DdgHtml => format!("https://html.duckduckgo.com/html/?q={}", q),
            Engine::DdgLite => format!("https://lite.duckduckgo.com/lite/?q={}", q),
            Engine::Brave => format!("https://search.brave.com/search?q={}&source=web", q),
            Engine::Mojeek => format!("https://www.mojeek.com/search?q={}", q),
            Engine::Startpage => format!("https://www.startpage.com/sp/search?query={}", q),
        }
    }

    /// Domaines du moteur (liens internes ignores).
    fn hosts(self) -> &'static [&'static str] {
        match self {
            Engine::DdgHtml | Engine::DdgLite => &["duckduckgo.com"],
            Engine::Brave => &["brave.com", "bravesoftware.com"],
            Engine::Mojeek => &["mojeek.com", "mojeek.co.uk"],
            Engine::Startpage => &["startpage.com", "startmail.com"],
        }
    }

    /// (presence de resultats, page sans resultat, page de blocage) : marqueurs du format attendu.
    fn markers(self) -> (&'static [&'static str], &'static [&'static str], &'static [&'static str]) {
        const DDG_BLOCK: &[&str] = &["anomaly-modal", "bots use duckduckgo", "challenge-form", "anomaly.js", "captcha"];
        // Pages de verification « navigateur » (preuve de travail en JavaScript) : equivalent d'un blocage.
        const BLOCK: &[&str] = &["captcha", "pow-challenge", "verifying your request", "anubis", "are you a robot", "unusual traffic", "blocked"];
        match self {
            Engine::DdgHtml => (&["result__a"], &["no results.", "no-results"], DDG_BLOCK),
            Engine::DdgLite => (&["result-link"], &["no results."], DDG_BLOCK),
            Engine::Brave => (&["data-type=\"web\"", "snippet"], &["not many great matches", "no results found"], BLOCK),
            Engine::Mojeek => (&["class=\"ob\"", "results-standard"], &["no pages found"], BLOCK),
            Engine::Startpage => (&["result-link", "w-gl__result"], &["did not match any"], BLOCK),
        }
    }
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn host_of(url: &str) -> String {
    let r = url.split_once("://").map(|x| x.1).unwrap_or(url);
    r.split(['/', '?', '#']).next().unwrap_or("").trim_start_matches("www.").to_lowercase()
}

/// Domaines dont les pages n'apportent jamais de chemin lisible (videos, reseaux, boutiques).
const SKIP_HOSTS: &[&str] = &["youtube.com", "youtu.be", "facebook.com", "twitter.com", "x.com", "tiktok.com", "instagram.com", "amazon.", "ebay.", "pinterest.", "twitch.tv", "store.steampowered.com", "apps.apple.com", "play.google.com"];

/// Liens de resultats d'une page de moteur (absolus, hors moteur, redirections DuckDuckGo decodees, sans doublon).
pub fn result_links(engine: Engine, html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=") {
        rest = &rest[i + 5..];
        let q = rest.chars().next().unwrap_or(' ');
        let val = if q == '"' || q == '\'' { rest[1..].split(q).next().unwrap_or("") } else { rest.split([' ', '>']).next().unwrap_or("") };
        let mut url = paths::decode_entities(val);
        if url.contains("duckduckgo.com/l/?") || url.starts_with("/l/?") {
            if let Some(u) = url.split(['?', '&']).find_map(|kv| kv.strip_prefix("uddg=")) {
                url = urldecode(u);
            }
        }
        if url.starts_with("//") {
            url = format!("https:{}", url);
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            continue;
        }
        let h = host_of(&url);
        if h.is_empty() || engine.hosts().iter().any(|e| h == *e || h.ends_with(&format!(".{}", e))) || SKIP_HOSTS.iter().any(|s| h.contains(s)) {
            continue;
        }
        if url.to_lowercase().ends_with(".pdf") || url.contains("/y.js") || url.contains("ad_domain=") {
            continue;
        }
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Lecture d'une page de resultats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    Results(Vec<String>),
    Empty,
    Blocked,
    Changed,
}

pub fn parse(engine: Engine, r: &Resp) -> Parsed {
    if [202, 403, 418, 429, 503].contains(&r.status) {
        return Parsed::Blocked;
    }
    if r.status != 200 {
        return Parsed::Changed;
    }
    let low = r.body.to_lowercase();
    let (res, empty, block) = engine.markers();
    let links = result_links(engine, &r.body);
    if res.iter().any(|m| low.contains(m)) && !links.is_empty() {
        return Parsed::Results(links);
    }
    if block.iter().any(|m| low.contains(m)) {
        return Parsed::Blocked;
    }
    if empty.iter().any(|m| low.contains(m)) {
        return Parsed::Empty;
    }
    Parsed::Changed
}

// ------------------------------------------------------------------ etat persistant (quota, pauses)

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Jour courant (jours depuis 1970) et nombre de programmes cherches ce jour-la.
    #[serde(default)]
    pub day: u64,
    #[serde(default)]
    pub count: u32,
    /// Moteur -> fin de pause (ms).
    #[serde(default)]
    pub paused: BTreeMap<String, u64>,
    /// Moteur -> echecs consecutifs (la pause s'allonge).
    #[serde(default)]
    pub strikes: BTreeMap<String, u32>,
}

impl State {
    pub fn path(data_dir: &Path) -> std::path::PathBuf {
        super::store::dir(data_dir).join("web.json")
    }
    pub fn load(data_dir: &Path) -> State {
        std::fs::read_to_string(Self::path(data_dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self, data_dir: &Path) -> crate::Res<()> {
        let p = Self::path(data_dir);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        crate::util::write_atomic(&p, serde_json::to_string_pretty(self)?.as_bytes())
    }
    fn roll(&mut self, now_ms: u64) {
        let d = now_ms / 86_400_000;
        if d != self.day {
            self.day = d;
            self.count = 0;
        }
    }
    /// Reste-t-il une recherche pour aujourd'hui ?
    pub fn quota_left(&mut self, now_ms: u64) -> bool {
        self.roll(now_ms);
        self.count < MAX_PER_DAY
    }
    pub fn count_one(&mut self, now_ms: u64) {
        self.roll(now_ms);
        self.count += 1;
    }
    pub fn is_paused(&self, e: Engine, now_ms: u64) -> bool {
        self.paused.get(e.code()).map(|u| *u > now_ms).unwrap_or(false)
    }
    /// Pause : 6 h au premier echec, puis 24 h, puis 3 jours.
    pub fn pause(&mut self, e: Engine, now_ms: u64) {
        let n = self.strikes.entry(e.code().to_string()).or_insert(0);
        *n += 1;
        let hours = match *n {
            1 => 6,
            2 => 24,
            _ => 72,
        };
        self.paused.insert(e.code().to_string(), now_ms + hours * 3_600_000);
    }
    pub fn ok(&mut self, e: Engine) {
        self.strikes.remove(e.code());
        self.paused.remove(e.code());
    }
}

// ------------------------------------------------------------------ recherche

/// Resultat brut d'une recherche (avant verification sur le disque).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// (gabarit, nombre de sources distinctes qui le citent).
    pub templates: Vec<(String, u32)>,
    /// Moteur (ou site cible) qui a repondu.
    pub engine: Option<String>,
    /// Nom exact et editeurs donnes par les sites cibles.
    pub names: Vec<String>,
    pub companies: Vec<String>,
    /// Aucun acces reseau : ne pas compter la recherche, reessayer plus tard.
    pub offline: bool,
    /// Lignes de journal (sans donnee personnelle).
    pub log: Vec<String>,
}

fn queries(name: &str, app: bool) -> Vec<String> {
    let save = format!("\"{}\" save game location", name);
    let conf = format!("\"{}\" config file location", name);
    if app {
        vec![conf, save]
    } else {
        vec![save, conf]
    }
}

fn ranked(sources: &HashMap<String, HashSet<String>>) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = sources.iter().map(|(t, s)| (t.clone(), s.len() as u32)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(40);
    v
}

/// Recherche (reseau) puis extraction des chemins. Met a jour les pauses des moteurs dans `st`. `enough` : vrai quand
/// les gabarits deja lus suffisent (la deuxieme requete n'est alors pas envoyee : moins de trafic).
pub fn search(p: &Program, fetch: &dyn Fetch, st: &mut State, now_ms: u64, pause_ms: u64, enough: &dyn Fn(&[(String, u32)]) -> bool) -> Outcome {
    let mut out = Outcome::default();
    let Some(name) = p.search_name() else {
        out.log.push("aucun nom exploitable".into());
        return out;
    };
    let mut sources: HashMap<String, HashSet<String>> = HashMap::new();
    let add = |sources: &mut HashMap<String, HashSet<String>>, text: &str, src: &str| {
        for raw in paths::extract_paths(text) {
            if let Some(t) = paths::normalize(&raw) {
                sources.entry(t).or_default().insert(src.to_string());
            }
        }
    };
    let mut answered = false;
    for q in queries(&name, p.app) {
        let mut links: Option<Vec<String>> = None;
        for e in Engine::ALL {
            if st.is_paused(e, now_ms) {
                continue;
            }
            let r = match fetch.get(&e.url(&q)) {
                Ok(r) => r,
                Err(why) => {
                    // Hors ligne : on s'arrete sans mettre les moteurs en pause.
                    out.offline = true;
                    out.log.push(format!("{} : pas de reponse ({})", e.label(), why.chars().take(80).collect::<String>()));
                    return out;
                }
            };
            match parse(e, &r) {
                Parsed::Results(l) => {
                    st.ok(e);
                    answered = true;
                    out.engine.get_or_insert_with(|| e.label().to_string());
                    out.log.push(format!("{} : {} resultat(s)", e.label(), l.len()));
                    add(&mut sources, &paths::html_to_text(&r.body), e.code());
                    links = Some(l);
                    break;
                }
                Parsed::Empty => {
                    st.ok(e);
                    answered = true;
                    out.engine.get_or_insert_with(|| e.label().to_string());
                    out.log.push(format!("{} : aucun resultat", e.label()));
                    links = Some(Vec::new());
                    break;
                }
                Parsed::Blocked => {
                    st.pause(e, now_ms);
                    out.log.push(format!("{} : bloque (code {}), mis en pause", e.label(), r.status));
                }
                Parsed::Changed => {
                    st.pause(e, now_ms);
                    out.log.push(format!("{} : format inattendu (code {}), mis en pause", e.label(), r.status));
                }
            }
            sleep_ms(pause_ms);
        }
        let Some(links) = links else { break };
        // 3 a 5 resultats, un seul par site.
        let mut hosts: HashSet<String> = HashSet::new();
        for u in links.iter().filter(|u| hosts.insert(host_of(u))).take(MAX_PAGES) {
            sleep_ms(pause_ms);
            match fetch.get(u) {
                Ok(r) if r.status == 200 && r.body.contains('<') => add(&mut sources, &paths::html_to_text(&r.body), &host_of(u)),
                Ok(r) if r.status == 200 => add(&mut sources, &r.body, &host_of(u)),
                _ => {}
            }
        }
        if !sources.is_empty() && enough(&ranked(&sources)) {
            break;
        }
    }
    // Sites cibles : nom exact et editeur (quand les moteurs n'ont rien donne d'utilisable).
    if sources.is_empty() {
        let (names, companies, which) = targeted(&name, fetch, pause_ms);
        if !names.is_empty() || !companies.is_empty() {
            answered = true;
            out.engine.get_or_insert_with(|| which.to_string());
            out.log.push(format!("{} : nom exact et editeur", which));
        }
        out.names = names;
        out.companies = companies;
    }
    if !answered {
        out.log.push("aucun moteur disponible".into());
    }
    out.templates = ranked(&sources);
    out
}

fn sleep_ms(ms: u64) {
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// Magasin Steam puis Wikipedia : (noms, editeurs, site qui a repondu).
fn targeted(name: &str, fetch: &dyn Fetch, pause_ms: u64) -> (Vec<String>, Vec<String>, &'static str) {
    let json = |url: &str| -> Option<serde_json::Value> {
        let r = fetch.get(url).ok().filter(|r| r.status == 200)?;
        serde_json::from_str(&r.body).ok()
    };
    let wanted = vec![name.to_string()];
    if let Some(v) = json(&format!("https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US", urlencode(name))) {
        let item = v.get("items").and_then(|i| i.as_array()).and_then(|a| a.iter().find(|i| i.get("name").and_then(|n| n.as_str()).and_then(|n| best_match(n, &wanted)).map(|s| s >= 85).unwrap_or(false)).cloned());
        if let Some(it) = item {
            let id = it.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let exact = it.get("name").and_then(|n| n.as_str()).unwrap_or(name).to_string();
            sleep_ms(pause_ms);
            let mut comps = Vec::new();
            if let Some(d) = json(&format!("https://store.steampowered.com/api/appdetails?appids={}&l=english", id)) {
                let data = d.get(id.to_string()).and_then(|x| x.get("data"));
                for k in ["developers", "publishers"] {
                    for c in data.and_then(|x| x.get(k)).and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                        if let Some(s) = c.as_str() {
                            if !comps.contains(&s.to_string()) {
                                comps.push(s.to_string());
                            }
                        }
                    }
                }
            }
            return (vec![exact], comps, "magasin Steam");
        }
    }
    sleep_ms(pause_ms);
    if let Some(v) = json(&format!("https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&srlimit=3&format=json", urlencode(name))) {
        let title = v.pointer("/query/search").and_then(|s| s.as_array()).and_then(|a| {
            a.iter().filter_map(|i| i.get("title").and_then(|t| t.as_str())).find(|t| best_match(t.split(" (").next().unwrap_or(t), &wanted).map(|s| s >= 85).unwrap_or(false)).map(|t| t.to_string())
        });
        if let Some(t) = title {
            sleep_ms(pause_ms);
            let mut comps = Vec::new();
            if let Some(d) = json(&format!("https://en.wikipedia.org/w/api.php?action=parse&page={}&prop=wikitext&format=json&formatversion=2", urlencode(&t))) {
                comps = infobox_companies(d.pointer("/parse/wikitext").and_then(|x| x.as_str()).unwrap_or(""));
            }
            return (vec![t.split(" (").next().unwrap_or(&t).to_string()], comps, "Wikipedia");
        }
    }
    (vec![], vec![], "")
}

/// Developpeur et editeur d'une infobox Wikipedia (`| developer = [[Team Cherry]]`).
pub fn infobox_companies(wikitext: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in wikitext.lines() {
        let l = line.trim().trim_start_matches('|').trim();
        let Some((k, v)) = l.split_once('=') else { continue };
        let k = k.trim().to_lowercase();
        if k != "developer" && k != "publisher" && k != "developers" && k != "publishers" {
            continue;
        }
        let v = v.split("<ref").next().unwrap_or("");
        let v = v.replace("<br>", "|").replace("<br/>", "|").replace("<br />", "|");
        for part in v.split(['|', ',']) {
            let s: String = part.replace("[[", "").replace("]]", "").replace("{{", "").replace("}}", "").replace("Plainlist", "").replace("plainlist", "").replace('*', "").trim().to_string();
            let s = s.rsplit('|').next().unwrap_or(&s).trim().to_string();
            if s.len() >= 2 && s.len() <= 60 && !s.contains('=') && !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ verification sur ce PC

/// Composants trop generiques pour etre l'emplacement d'un programme.
const GENERIC_LAST: &[&str] = &["my games", "saved games", "documents", "appdata", "local", "roaming", "locallow", "programs", "games", "steam", "users", "public", "*", "saves", "save", "config", "settings"];

/// Gabarits -> emplacements existants de CE PC, avec des fichiers recents. Un emplacement sans lien avec le nom du
/// programme ou de son editeur exige au moins trois sources (et reste en confiance faible).
pub fn verify(p: &Program, out: &Outcome, f: &Folders, now_ms: u64) -> Vec<Found> {
    let mut names = p.names();
    names.extend(out.names.iter().cloned());
    let mut related = names.clone();
    related.extend(p.companies());
    related.extend(out.companies.iter().cloned());
    let mut found: Vec<Found> = Vec::new();
    for (tpl, n) in &out.templates {
        for (v, dropped) in paths::shorter_variants(tpl) {
            let Some((base, comps)) = paths::split_template(&v) else { continue };
            if comps.is_empty() || super::learn::is_noise(&base, &comps) {
                continue;
            }
            let last = comps.last().map(|c| c.to_lowercase()).unwrap_or_default();
            // Seul : « Saves », « Config »... ; sous un dossier de programme, ils restent valables.
            if comps.len() == 1 && GENERIC_LAST.contains(&last.as_str()) || last == "my games" || last == "*" {
                continue;
            }
            let rel = comps.iter().filter_map(|c| best_match(c, &related)).max();
            if dropped && comps.last().and_then(|c| best_match(c, &names)).is_none() {
                continue; // le parent n'est accepte que s'il porte le nom du programme
            }
            if rel.is_none() && *n < 3 {
                continue;
            }
            let mut hit = false;
            for d in paths::expand(&v, f, p.install_dir.as_deref()) {
                if !paths::has_recent_files(&d, now_ms, RECENT_DAYS) || found.iter().any(|x| x.path == d) {
                    continue;
                }
                let conf = if rel.is_some() { 72 } else { 56 } + 4 * (n.saturating_sub(1)).min(3) as u8;
                let template = paths::to_generic(&d, f).map(|t| if v.contains('*') { v.clone() } else { t });
                found.push(Found { kind: kind_for(&v, p.app), template, path: d, layer: Layer::Web, confidence: conf.min(85), source: format!("recherche : {} source(s)", n) });
                hit = true;
            }
            if hit {
                break;
            }
        }
    }
    found.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    found
}

/// Resultat d'un passage complet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunResult {
    pub found: Vec<Found>,
    pub engine: Option<String>,
    /// Raison d'une recherche non faite (quota, deja faite, hors ligne).
    pub skipped: Option<&'static str>,
    pub log: Vec<String>,
}

/// Recherche complete pour un programme : quota, cache, recherche, verification, repli « nom exact / editeur » vers la
/// deduction locale, mise en cache. `pause_ms` : temps entre deux requetes (politesse ; 0 en test).
pub fn run(p: &Program, data_dir: &Path, fetch: &dyn Fetch, f: &Folders, now_ms: u64, pause_ms: u64) -> RunResult {
    let mut cache = super::store::Cache::load(data_dir);
    if !cache.web_due(&p.key, now_ms) {
        return RunResult { found: cache.known(&p.key), skipped: Some("deja cherche"), ..Default::default() };
    }
    let mut st = State::load(data_dir);
    if !st.quota_left(now_ms) {
        return RunResult { skipped: Some("quota du jour atteint"), ..Default::default() };
    }
    let enough = |t: &[(String, u32)]| !verify(p, &Outcome { templates: t.to_vec(), ..Default::default() }, f, now_ms).is_empty();
    let out = search(p, fetch, &mut st, now_ms, pause_ms, &enough);
    if out.offline {
        let _ = st.save(data_dir);
        return RunResult { skipped: Some("hors ligne"), log: out.log, ..Default::default() };
    }
    st.count_one(now_ms);
    let _ = st.save(data_dir);
    let mut found = verify(p, &out, f, now_ms);
    if found.is_empty() && (!out.names.is_empty() || !out.companies.is_empty()) {
        // Nom exact et editeur trouves en ligne : nouvelle deduction locale avec ces informations.
        let mut q = p.clone();
        q.title = out.names.first().cloned().or(q.title);
        if q.developer.is_none() {
            q.developer = out.companies.first().cloned();
        }
        if q.publisher.is_none() {
            q.publisher = out.companies.get(1).cloned();
        }
        found = super::deduce::find(&q, f, now_ms, &super::deduce::RealRegistry)
            .into_iter()
            .map(|mut x| {
                x.layer = Layer::Web;
                x.source = format!("{} ; nom exact et editeur en ligne", x.source);
                x
            })
            .collect();
    }
    let found = super::dedupe(found);
    cache.set_web(&p.key, super::store::WebEntry { at: now_ms, found: found.clone(), templates: out.templates.iter().map(|t| t.0.clone()).collect(), engine: out.engine.clone(), tries: 0 });
    let _ = cache.save(data_dir);
    RunResult { found, engine: out.engine, skipped: None, log: out.log }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Reseau simule : (debut d'URL -> reponse) ; les URL demandees sont gardees.
    struct Fake {
        routes: Vec<(String, Result<Resp, String>)>,
        asked: RefCell<Vec<String>>,
    }
    impl Fetch for Fake {
        fn get(&self, url: &str) -> Result<Resp, String> {
            self.asked.borrow_mut().push(url.to_string());
            self.routes.iter().find(|(p, _)| url.starts_with(p)).map(|(_, r)| r.clone()).unwrap_or(Ok(Resp { status: 404, body: String::new() }))
        }
    }
    fn ok(b: &str) -> Result<Resp, String> {
        Ok(Resp { status: 200, body: b.to_string() })
    }

    #[test]
    fn liens_de_resultats() {
        let h = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fwiki%2FSaves&amp;rut=abc">x</a><a href="https://duckduckgo.com/y.js?ad_domain=x">ad</a><a href="https://www.youtube.com/watch?v=1">v</a><a href='https://forum.example.net/t/1'>f</a>"#;
        assert_eq!(result_links(Engine::DdgHtml, h), vec!["https://example.org/wiki/Saves".to_string(), "https://forum.example.net/t/1".to_string()]);
        assert_eq!(urldecode("a%2Fb+c%3"), "a/b c%3");
        assert_eq!(urlencode("\"Hollow Knight\" save"), "%22Hollow+Knight%22+save");
    }

    #[test]
    fn analyse_des_pages_de_moteur() {
        let r = |s: u16, b: &str| Resp { status: s, body: b.to_string() };
        assert_eq!(parse(Engine::DdgHtml, &r(202, "")), Parsed::Blocked);
        assert_eq!(parse(Engine::DdgHtml, &r(200, "<div class=\"anomaly-modal\">")), Parsed::Blocked);
        assert_eq!(parse(Engine::DdgHtml, &r(200, "<div class=\"no-results\">No results.</div>")), Parsed::Empty);
        assert_eq!(parse(Engine::DdgHtml, &r(200, "<html>nouvelle page</html>")), Parsed::Changed);
        assert_eq!(parse(Engine::Mojeek, &r(200, "<a class=\"ob\" href=\"https://a.example/x\">")), Parsed::Results(vec!["https://a.example/x".into()]));
        assert_eq!(parse(Engine::Brave, &r(429, "")), Parsed::Blocked);
        assert_eq!(parse(Engine::Startpage, &r(404, "")), Parsed::Changed);
    }

    #[test]
    fn infobox() {
        let w = "{{Infobox video game\n| developer = [[Team Cherry]]\n| publisher = {{Plainlist|\n* [[Team Cherry]]}}\n| engine = [[Unity (game engine)|Unity]]\n";
        assert_eq!(infobox_companies(w), vec!["Team Cherry".to_string()]);
    }

    #[test]
    fn quota_et_pauses() {
        let mut s = State::default();
        let day = 20_000 * 86_400_000u64;
        for _ in 0..MAX_PER_DAY {
            assert!(s.quota_left(day));
            s.count_one(day);
        }
        assert!(!s.quota_left(day + 1000));
        assert!(s.quota_left(day + 86_400_000), "nouveau jour");
        s.pause(Engine::Brave, 0);
        assert!(s.is_paused(Engine::Brave, 3_600_000));
        assert!(!s.is_paused(Engine::Brave, 7 * 3_600_000));
        s.pause(Engine::Brave, 0);
        assert!(s.is_paused(Engine::Brave, 20 * 3_600_000), "pause plus longue au deuxieme echec");
        s.ok(Engine::Brave);
        assert!(!s.is_paused(Engine::Brave, 1));
    }

    #[test]
    fn repli_moteur_bloque_puis_suivant_et_verification() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        let save = f.roaming.clone().unwrap().join("Zorblax Studio").join("Zorblax");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::write(save.join("slot1.sav"), b"x").unwrap();
        let fake = Fake {
            routes: vec![
                ("https://html.duckduckgo.com/".into(), Ok(Resp { status: 202, body: "<div class=\"anomaly-modal\"></div>".into() })),
                ("https://lite.duckduckgo.com/".into(), ok("<html>format totalement nouveau</html>")),
                ("https://search.brave.com/search?q=%22Zorblax%22".into(), ok("<div class=\"snippet\" data-type=\"web\"><a href=\"https://wiki.example.org/Zorblax\">Zorblax</a> saves in %APPDATA%\\Zorblax Studio\\Zorblax.</div>")),
                ("https://search.brave.com/".into(), ok("<div>No results found</div>")),
                ("https://wiki.example.org/".into(), ok("<p>Save files: <code>C:\\Users\\&lt;user&gt;\\AppData\\Roaming\\Zorblax Studio\\Zorblax\\</code></p><p>Old: %APPDATA%\\Microsoft\\Zorblax</p>")),
            ],
            asked: RefCell::new(vec![]),
        };
        let p = Program { key: "zorblax.exe".into(), exe: "Zorblax.exe".into(), name: "Zorblax".into(), ..Default::default() };
        let now = crate::util::now_ms();
        let r = run(&p, t.path(), &fake, &f, now, 0);
        assert_eq!(r.engine.as_deref(), Some("Brave Search"), "{:?}", r.log);
        assert_eq!(r.found.iter().map(|x| x.path.clone()).collect::<Vec<_>>(), vec![save.clone()], "{:?}", r.log);
        assert_eq!(r.found[0].layer, Layer::Web);
        assert_eq!(r.found[0].template.as_deref(), Some("%APPDATA%/Zorblax Studio/Zorblax"));
        let st = State::load(t.path());
        assert!(st.is_paused(Engine::DdgHtml, now + 1000), "moteur bloque mis en pause");
        assert!(st.is_paused(Engine::DdgLite, now + 1000), "format change : mis en pause");
        assert!(!st.is_paused(Engine::Brave, now + 1000));
        assert_eq!(st.count, 1);
        assert!(!fake.asked.borrow().iter().any(|u| u.contains("config+file")), "premiere requete suffisante : pas de deuxieme");
        // Deuxieme programme : les moteurs en pause ne sont plus interroges.
        fake.asked.borrow_mut().clear();
        let q = Program { key: "autre.exe".into(), exe: "Autre.exe".into(), name: "Autre Programme".into(), ..Default::default() };
        let r2 = run(&q, t.path(), &fake, &f, now + 1000, 0);
        assert!(r2.found.is_empty());
        assert!(!fake.asked.borrow().iter().any(|u| u.contains("duckduckgo.com")), "{:?}", fake.asked.borrow());
        // Jamais deux fois le meme programme quand un resultat existe.
        let r3 = run(&p, t.path(), &fake, &f, now + 2000, 0);
        assert_eq!(r3.skipped, Some("deja cherche"));
        assert_eq!(r3.found.len(), 1);
    }

    #[test]
    fn hors_ligne_ni_pause_ni_quota() {
        let t = tempfile::tempdir().unwrap();
        let fake = Fake { routes: vec![("https://".into(), Err("dns".into()))], asked: RefCell::new(vec![]) };
        let p = Program { key: "z.exe".into(), exe: "Z.exe".into(), name: "Zorblax".into(), ..Default::default() };
        let r = run(&p, t.path(), &fake, &Folders::under(t.path()), crate::util::now_ms(), 0);
        assert_eq!(r.skipped, Some("hors ligne"));
        let st = State::load(t.path());
        assert!(st.paused.is_empty());
        assert_eq!(st.count, 0);
        assert!(super::super::store::Cache::load(t.path()).web_due("z.exe", crate::util::now_ms()));
    }

    #[test]
    fn rien_de_verifie_rien_de_propose() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        // Chemin cite mais absent du disque ; chemin present mais sans fichier recent ; chemin sans lien et une seule source.
        let old = f.local.clone().unwrap().join("Zorblax");
        std::fs::create_dir_all(&old).unwrap();
        let unrelated = f.local.clone().unwrap().join("Outil");
        std::fs::create_dir_all(&unrelated).unwrap();
        std::fs::write(unrelated.join("a.txt"), b"x").unwrap();
        let p = Program { exe: "Zorblax.exe".into(), name: "Zorblax".into(), ..Default::default() };
        let out = Outcome { templates: vec![("%APPDATA%/Zorblax".into(), 3), ("%LOCALAPPDATA%/Zorblax".into(), 1), ("%LOCALAPPDATA%/Outil".into(), 1), ("%LOCALAPPDATA%/Microsoft/Zorblax".into(), 5)], ..Default::default() };
        assert!(verify(&p, &out, &f, crate::util::now_ms()).is_empty());
        // Trois sources : accepte meme sans lien de nom, confiance plus basse.
        let out2 = Outcome { templates: vec![("%LOCALAPPDATA%/Outil".into(), 3)], ..Default::default() };
        let v = verify(&p, &out2, &f, crate::util::now_ms());
        assert_eq!(v.len(), 1);
        assert!(v[0].confidence < 70);
    }
}
