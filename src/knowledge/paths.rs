//! Chemins de la connaissance intelligente : dossiers de base de CE PC, gabarits generiques (`%APPDATA%/Studio/Jeu`,
//! `$XDG_DATA_HOME/jeu`), conversion chemin <-> gabarit, extraction de chemins dans un texte (pages web), normalisation
//! et verification sur le disque (existe et contient des fichiers recents). Tout est pur sauf la lecture du disque.
use std::path::{Component, Path, PathBuf};

/// Gabarits Windows (aussi utilises sous Linux dans un prefixe Proton / Wine).
pub const WIN_BASES: [&str; 6] = ["%LOCALAPPDATA%", "%APPDATA%", "%LOCALLOW%", "%DOCUMENTS%", "%SAVEDGAMES%", "%USERPROFILE%"];
/// Gabarits Linux natifs.
pub const LX_BASES: [&str; 3] = ["$XDG_DATA_HOME", "$XDG_CONFIG_HOME", "$HOME"];
/// Dossier d'installation (gabarit generique, resolu avec le dossier du programme).
pub const INSTALL: &str = "%INSTALLDIR%";

/// Dossiers de base d'un utilisateur (chaque champ absent = base inconnue sur ce PC).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folders {
    pub local: Option<PathBuf>,
    pub roaming: Option<PathBuf>,
    pub locallow: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub saved_games: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub xdg_data: Option<PathBuf>,
    pub xdg_config: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl Folders {
    /// Hook de verification manuelle (comme AGSS_TEST_SAVEPATH) : dossiers perso simules sous un dossier de test.
    pub fn test_root() -> Option<PathBuf> {
        std::env::var_os("AGSS_KNOWLEDGE_ROOT").map(PathBuf::from).filter(|p| p.is_dir())
    }

    /// Nom d'utilisateur des dossiers simules (`AGSS_KNOWLEDGE_USER`, defaut « Joueur ») : verification d'un nouveau PC
    /// dont le nom d'utilisateur differe de l'ancien.
    pub fn test_user() -> String {
        std::env::var("AGSS_KNOWLEDGE_USER").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && !s.contains(['/', '\\', '.'])).unwrap_or_else(|| "Joueur".to_string())
    }

    pub fn real() -> Folders {
        match Self::test_root() {
            Some(r) => Folders::under_user(&r, &Self::test_user()),
            None => Self::system(),
        }
    }

    #[cfg(windows)]
    fn system() -> Folders {
        let k = crate::hints::KnownFolders::real();
        Folders { local: k.local_app_data, roaming: k.roaming_app_data, locallow: k.local_low, documents: k.documents, saved_games: k.saved_games, profile: dirs::home_dir(), ..Default::default() }
    }

    #[cfg(not(windows))]
    fn system() -> Folders {
        let home = dirs::home_dir().unwrap_or_default();
        let x = crate::proton::XdgDirs::from_env(&home, std::env::var("XDG_DATA_HOME").ok().as_deref(), std::env::var("XDG_CONFIG_HOME").ok().as_deref(), dirs::document_dir().as_deref());
        Folders { xdg_data: Some(x.data), xdg_config: Some(x.config), home: Some(home), ..Default::default() }
    }

    /// Bases Windows d'un prefixe Proton / Wine (`.../pfx/drive_c/users/steamuser/...`), en plus des bases Linux.
    pub fn with_prefix(mut self, pfx: &Path) -> Folders {
        let u = crate::proton::user_dir(pfx);
        self.local = Some(u.join("AppData").join("Local"));
        self.roaming = Some(u.join("AppData").join("Roaming"));
        self.locallow = Some(u.join("AppData").join("LocalLow"));
        self.documents = Some(u.join("Documents"));
        self.saved_games = Some(u.join("Saved Games"));
        self.profile = Some(u);
        self
    }

    /// Toutes les bases sous un dossier (tests) : arborescence Windows et Linux.
    pub fn under(root: &Path) -> Folders {
        Folders::under_user(root, "Joueur")
    }

    /// Comme `under`, avec un nom d'utilisateur choisi (`<racine>/Users/<nom>/...`).
    pub fn under_user(root: &Path, user: &str) -> Folders {
        let p = root.join("Users").join(user);
        Folders {
            local: Some(p.join("AppData").join("Local")),
            roaming: Some(p.join("AppData").join("Roaming")),
            locallow: Some(p.join("AppData").join("LocalLow")),
            documents: Some(p.join("Documents")),
            saved_games: Some(p.join("Saved Games")),
            profile: Some(p),
            xdg_data: Some(root.join("home").join(".local").join("share")),
            xdg_config: Some(root.join("home").join(".config")),
            home: Some(root.join("home")),
        }
    }

    /// (gabarit, dossier) connus, du plus specifique au plus general.
    pub fn bases(&self) -> Vec<(&'static str, &PathBuf)> {
        let all = [
            ("%LOCALLOW%", &self.locallow),
            ("%LOCALAPPDATA%", &self.local),
            ("%APPDATA%", &self.roaming),
            ("%SAVEDGAMES%", &self.saved_games),
            ("%DOCUMENTS%", &self.documents),
            ("%USERPROFILE%", &self.profile),
            ("$XDG_DATA_HOME", &self.xdg_data),
            ("$XDG_CONFIG_HOME", &self.xdg_config),
            ("$HOME", &self.home),
        ];
        all.into_iter().filter_map(|(k, v)| v.as_ref().map(|p| (k, p))).collect()
    }

    pub fn base(&self, ph: &str) -> Option<&PathBuf> {
        self.bases().into_iter().find(|(k, _)| k.eq_ignore_ascii_case(ph)).map(|(_, p)| p)
    }

    /// Noms de dossiers propres a cet utilisateur (nom du profil Windows, dossier personnel Linux) : jamais envoyes.
    pub fn user_names(&self) -> Vec<String> {
        let mut v: Vec<String> = [&self.profile, &self.home].iter().filter_map(|p| p.as_ref()).filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).filter(|n| n.len() >= 2).collect();
        for var in ["USERNAME", "USER", "LOGNAME"] {
            if let Ok(u) = std::env::var(var) {
                if u.len() >= 2 && !v.iter().any(|x| x.eq_ignore_ascii_case(&u)) {
                    v.push(u);
                }
            }
        }
        v
    }
}

fn comps(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            Component::Prefix(x) => Some(x.as_os_str().to_string_lossy().to_lowercase()),
            Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

fn same(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Composant acceptable dans un gabarit (ni vide, ni lecteur, ni remontee, ni variable).
fn clean_comp(c: &str) -> bool {
    !c.is_empty() && c != "." && c != ".." && !c.contains(':') && !c.contains('%') && !c.contains('$')
}

/// Chemin de CE PC -> gabarit generique (la base la plus specifique gagne) ; None hors des dossiers de base
/// (autre lecteur, dossier d'installation, dossier perso quelconque...).
pub fn to_generic(path: &Path, f: &Folders) -> Option<String> {
    #[cfg(not(windows))]
    if let Some(t) = crate::proton::template_from_prefix_path(path) {
        return Some(t);
    }
    let pc = comps(path);
    let mut best: Option<(usize, &'static str)> = None;
    for (ph, base) in f.bases() {
        let bc = comps(base);
        if bc.is_empty() || pc.len() <= bc.len() {
            continue;
        }
        if bc.iter().zip(pc.iter()).all(|(a, b)| same(a, b)) && best.map(|(n, _)| bc.len() > n).unwrap_or(true) {
            best = Some((bc.len(), ph));
        }
    }
    let (n, ph) = best?;
    let rest = &pc[n..];
    if !rest.iter().all(|c| clean_comp(c)) {
        return None;
    }
    Some(format!("{}/{}", ph, rest.join("/")))
}

/// Gabarit -> (base, composants). Refuse un gabarit qui sortirait de sa base.
pub fn split_template(tpl: &str) -> Option<(String, Vec<String>)> {
    let t = tpl.trim();
    let i = t.find(['/', '\\']).unwrap_or(t.len());
    let ph = t[..i].to_string();
    let rest: Vec<String> = t.get(i + 1..).unwrap_or("").split(['/', '\\']).filter(|c| !c.is_empty()).map(|c| c.to_string()).collect();
    if rest.iter().any(|c| c == ".." || c.contains(':')) {
        return None;
    }
    Some((ph, rest))
}

/// Sous-dossiers (ou fichiers) d'un dossier dont le nom correspond a `want` (`*` : tous ; sans casse sous Linux aussi).
fn children(dir: &Path, want: &str) -> Vec<PathBuf> {
    if want != "*" && !want.contains('*') {
        let direct = dir.join(want);
        if direct.exists() {
            return vec![direct];
        }
        if cfg!(windows) {
            return vec![];
        }
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let w = want.to_lowercase();
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if w == "*" {
                e.file_type().map(|t| t.is_dir()).unwrap_or(false)
            } else {
                crate::store::glob_match(&w, &n)
            }
        })
        .map(|e| e.path())
        .collect();
    v.sort();
    v.truncate(16);
    v
}

/// Gabarit -> chemins EXISTANTS de ce PC (`*` = un niveau quelconque). `install` : dossier d'installation du programme.
pub fn expand(tpl: &str, f: &Folders, install: Option<&Path>) -> Vec<PathBuf> {
    let Some((ph, rest)) = split_template(tpl) else { return vec![] };
    let base = if ph.eq_ignore_ascii_case(INSTALL) { install.map(Path::to_path_buf) } else { f.base(&ph).cloned() };
    let Some(base) = base else { return vec![] };
    if rest.is_empty() {
        return vec![];
    }
    let mut cur = vec![base];
    for c in &rest {
        let mut next = Vec::new();
        for d in &cur {
            next.extend(children(d, c));
        }
        next.truncate(16);
        cur = next;
        if cur.is_empty() {
            break;
        }
    }
    cur
}

// ------------------------------------------------------------------ fraicheur des fichiers

/// Date (ms) du fichier le plus recent sous `p` (fichier ou dossier ; profondeur et nombre d'entrees limites).
pub fn newest_file_ms(p: &Path) -> Option<u64> {
    let ms = |m: std::fs::Metadata| m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
    if p.is_file() {
        return std::fs::metadata(p).ok().and_then(ms);
    }
    let mut best: Option<u64> = None;
    for e in walkdir::WalkDir::new(p).max_depth(5).follow_links(false).into_iter().filter_map(|e| e.ok()).take(4000) {
        if e.file_type().is_file() {
            if let Some(t) = e.metadata().ok().and_then(ms) {
                best = Some(best.map_or(t, |b: u64| b.max(t)));
            }
        }
    }
    best
}

/// Le dossier contient-il au moins un fichier modifie dans les `days` derniers jours ?
pub fn has_recent_files(p: &Path, now_ms: u64, days: u64) -> bool {
    newest_file_ms(p).map(|t| now_ms.saturating_sub(t) <= days * 86_400_000).unwrap_or(false)
}

// ------------------------------------------------------------------ texte des pages

/// HTML -> texte : scripts et styles retires, balises de bloc = fin de ligne, balises en ligne = rien (un chemin coupe
/// par `<wbr>` ou `<span>` reste entier), entites decodees.
pub fn html_to_text(html: &str) -> String {
    const INLINE: &[&str] = &["span", "code", "b", "i", "em", "strong", "a", "wbr", "kbd", "mark", "sub", "sup", "font", "u", "small", "tt", "var", "samp"];
    let mut out = String::with_capacity(html.len() / 2);
    let low = html.to_ascii_lowercase();
    let b = html.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let Some(end) = html[i..].find('>').map(|e| i + e) else { break };
            let tag = &low[i + 1..end];
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if (name == "script" || name == "style" || name == "noscript") && !tag.starts_with('/') {
                let close = format!("</{}", name);
                i = low[end..].find(&close).map(|c| end + c).unwrap_or(b.len());
                continue;
            }
            if !INLINE.contains(&name.as_str()) {
                out.push('\n');
            }
            i = end + 1;
            continue;
        }
        let ch = html[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    decode_entities(&out)
}

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.char_indices().take_while(|(i, _)| *i < 12).find(|(_, c)| *c == ';').map(|(i, _)| i);
        let Some(e) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..e];
        let rep: Option<String> = match ent {
            "amp" => Some("&".into()),
            "lt" => Some("<".into()),
            "gt" => Some(">".into()),
            "quot" => Some("\"".into()),
            "apos" => Some("'".into()),
            "nbsp" => Some(" ".into()),
            "bsol" => Some("\\".into()),
            "sol" => Some("/".into()),
            "percnt" => Some("%".into()),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32).map(|c| c.to_string()),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32).map(|c| c.to_string()),
            _ => None,
        };
        match rep {
            Some(r) => {
                out.push_str(&r);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace(['\u{a0}', '\u{2009}', '\u{202f}'], " ").replace(['\u{200b}', '\u{200c}', '\u{feff}', '\u{ad}'], "")
}

// ------------------------------------------------------------------ extraction de chemins

/// Debuts de chemins reconnus (minuscules).
const STARTS: &[&str] = &[
    "%", "c:\\users", "c:/users", "~/", "$home/", "${home}/", "$xdg_", "${xdg_", "appdata\\", "appdata/", "documents\\", "documents/", "my documents\\", "saved games\\", "saved games/", "/home/", "users\\",
];

/// Chemins bruts (Windows et Linux) cites dans un texte. Les espaces sont gardes (« My Games », « Saved Games ») : la fin
/// exacte est tranchee plus tard par la verification sur le disque.
pub fn extract_paths(text: &str) -> Vec<String> {
    // Minuscules ASCII seulement : memes positions d'octets que le texte d'origine (casse conservee dans le resultat).
    let low = text.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let lb = low.as_bytes();
    let mut i = 0;
    while i < lb.len() {
        let at = &low[i..];
        let prev_ok = i == 0 || !(lb[i - 1].is_ascii_alphanumeric() || lb[i - 1] == b'_' || lb[i - 1] == b'%');
        let starts = prev_ok && STARTS.iter().any(|s| at.starts_with(s)) && (!at.starts_with('%') || looks_like_var(at));
        if !starts {
            i += at.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            continue;
        }
        let raw = take_path(&text[i..]);
        let n = raw.len();
        let r = clean_tail(&raw.replace("\\\\", "\\"));
        if r.chars().filter(|c| *c == '\\' || *c == '/').count() >= 1 && r.len() >= 6 && !out.contains(&r) {
            out.push(r);
        }
        i += n.max(1);
        while i < lb.len() && !text.is_char_boundary(i) {
            i += 1;
        }
    }
    out
}

/// `%NOM%` suivi d'un separateur (ou d'un autre `%NOM%`).
fn looks_like_var(at: &str) -> bool {
    let b = at.as_bytes();
    let mut j = 1;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'(' || b[j] == b')') {
        j += 1;
    }
    j > 2 && j < b.len() && b[j] == b'%' && (b.get(j + 1).map(|c| *c == b'\\' || *c == b'/' || *c == b'%' || c.is_ascii_alphabetic()).unwrap_or(false))
}

/// Prend la suite de caracteres d'un chemin (s'arrete aux caracteres interdits, a une fin de phrase, a une virgule suivie
/// d'un espace, a une parenthese fermante sans ouvrante).
fn take_path(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let chars: Vec<char> = s.chars().take(300).collect();
    let mut k = 0;
    while k < chars.len() {
        let c = chars[k];
        let next = chars.get(k + 1).copied();
        // `<nom d'utilisateur>` ou `"Your user"` au milieu d'un chemin : garde tel quel (remplace plus tard par un joker).
        let closer = match c {
            '<' => Some('>'),
            '"' | '“' if out.ends_with('\\') || out.ends_with('/') => Some(if c == '“' { '”' } else { '"' }),
            _ => None,
        };
        if let Some(cl) = closer {
            let close = chars[k + 1..].iter().take(40).position(|&x| x == cl || x == '\n').map(|j| j + 1);
            if let Some(j) = close.filter(|&j| j > 1 && chars[k + j] == cl && matches!(chars.get(k + j + 1), Some('\\') | Some('/'))) {
                out.extend(&chars[k..=k + j]);
                k += j + 1;
                continue;
            }
            break;
        }
        match c {
            '\n' | '\r' | '\t' | '"' | '>' | '|' | '*' | '?' | ';' | '“' | '”' | '«' | '»' | '`' => break,
            // « ...\Jeu (Windows) » : une parenthese apres un espace est une remarque, pas un nom de dossier.
            '(' if out.ends_with(' ') || out.ends_with('\\') || out.ends_with('/') => break,
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            '.' | ',' | ':' if next.map(|n| n.is_whitespace()).unwrap_or(true) && !(c == ':' && out.len() == 1) => break,
            ' ' if next == Some(' ') => break,
            _ => {}
        }
        out.push(c);
        k += 1;
    }
    out
}

fn clean_tail(s: &str) -> String {
    s.trim().trim_end_matches(['.', ',', ':', '\'', ')', ']', '\\', '/', ' ']).trim().to_string()
}

/// Composant qui designe une valeur propre a l'utilisateur (nom, identifiant) : remplace par `*`.
fn is_placeholder(c: &str) -> bool {
    let l = c.trim().to_lowercase();
    let n: String = l.chars().filter(|c| c.is_alphanumeric()).collect();
    (l.starts_with('<') && l.ends_with('>'))
        || (l.starts_with('[') && l.ends_with(']'))
        || (l.starts_with('{') && l.ends_with('}'))
        || (l.starts_with('%') && l.ends_with('%'))
        || (l.starts_with('$') && l.len() > 1)
        || ["username", "yourusername", "yourname", "yourusernamehere", "username1", "yourprofile", "youraccount", "userid", "steamid", "steamid64", "yoursteamid", "profileid", "accountid", "numbers", "randomnumbers", "uniqueid", "playerid", "epicid", "ubisoftid", "steamuserid", "useridnumber"].contains(&n.as_str())
        || (n.len() >= 5 && n.chars().all(|c| c.is_ascii_digit()))
        || (n.len() >= 4 && n.chars().all(|c| c == 'x'))
        || (n.len() >= 24 && n.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Variable Windows -> base de gabarit.
fn var_base(v: &str) -> Option<&'static str> {
    let n: String = v.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    Some(match n.as_str() {
        "localappdata" => "%LOCALAPPDATA%",
        "appdata" | "roamingappdata" => "%APPDATA%",
        "userprofile" | "homepath" | "homedrivehomepath" | "home" => "%USERPROFILE%",
        "documents" | "mydocuments" | "personal" | "userdocuments" => "%DOCUMENTS%",
        "savedgames" => "%SAVEDGAMES%",
        "locallow" | "localappdatalow" | "localappdatalocallow" | "userprofileappdatalocallow" => "%LOCALLOW%",
        _ => return None,
    })
}

/// Chemin brut (copie d'une page) -> gabarit generique, ou None s'il est trop vague / hors des dossiers de base.
pub fn normalize(raw: &str) -> Option<String> {
    let mut parts: Vec<String> = raw.split(['\\', '/']).map(|c| c.trim().trim_matches(['"', '\'', '`']).to_string()).filter(|c| !c.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let first = parts[0].to_lowercase();
    let (mut base, mut rest): (String, Vec<String>);
    if first.starts_with('%') {
        // %APPDATA% ; %HOMEDRIVE%%HOMEPATH% ; %LOCALAPPDATA%Low ; %LocalAppData%Low
        let inner = first.trim_end_matches("low");
        let low_suffix = first.ends_with("%low");
        let b = var_base(inner.trim_matches('%')).or_else(|| var_base(&inner.replace('%', "")))?;
        base = if low_suffix && b == "%LOCALAPPDATA%" { "%LOCALLOW%".into() } else { b.into() };
        rest = parts.split_off(1);
    } else if first == "c:" || first.len() == 2 && first.ends_with(':') {
        if parts.len() < 3 || !parts[1].eq_ignore_ascii_case("users") {
            return None;
        }
        base = "%USERPROFILE%".into();
        rest = parts.split_off(3);
    } else if first == "users" {
        if parts.len() < 2 {
            return None;
        }
        base = "%USERPROFILE%".into();
        rest = parts.split_off(2);
    } else if first == "~" || first == "$home" || first == "${home}" {
        base = "$HOME".into();
        rest = parts.split_off(1);
    } else if first == "home" && parts.len() >= 2 {
        base = "$HOME".into();
        rest = parts.split_off(2);
    } else if first.starts_with("$xdg_data_home") || first.starts_with("${xdg_data_home") {
        base = "$XDG_DATA_HOME".into();
        rest = parts.split_off(1);
    } else if first.starts_with("$xdg_config_home") || first.starts_with("${xdg_config_home") {
        base = "$XDG_CONFIG_HOME".into();
        rest = parts.split_off(1);
    } else if first == "appdata" {
        base = "%USERPROFILE%".into();
        rest = parts;
    } else if first == "documents" || first == "my documents" {
        base = "%DOCUMENTS%".into();
        rest = parts.split_off(1);
    } else if first == "saved games" {
        base = "%SAVEDGAMES%".into();
        rest = parts.split_off(1);
    } else {
        return None;
    }
    // Remontees connues : %APPDATA%\..\LocalLow -> %LOCALLOW%.
    if rest.first().map(|c| c == "..").unwrap_or(false) && (base == "%APPDATA%" || base == "%LOCALAPPDATA%") {
        if rest.get(1).map(|c| c.eq_ignore_ascii_case("locallow")).unwrap_or(false) {
            base = "%LOCALLOW%".into();
            rest.drain(..2);
        } else if rest.get(1).map(|c| c.eq_ignore_ascii_case("local")).unwrap_or(false) {
            base = "%LOCALAPPDATA%".into();
            rest.drain(..2);
        } else if rest.get(1).map(|c| c.eq_ignore_ascii_case("roaming")).unwrap_or(false) {
            base = "%APPDATA%".into();
            rest.drain(..2);
        }
    }
    // Prefixe Proton : .../compatdata/<appid>/pfx/drive_c/users/<nom>/... -> gabarit Windows.
    if base == "$HOME" || base == "$XDG_DATA_HOME" {
        if let Some(i) = rest.iter().position(|c| c.eq_ignore_ascii_case("drive_c")) {
            if rest.get(i + 1).map(|c| c.eq_ignore_ascii_case("users")).unwrap_or(false) && rest.len() > i + 3 {
                base = "%USERPROFILE%".into();
                rest = rest.split_off(i + 3);
            }
        }
    }
    // Reductions vers la base la plus specifique.
    loop {
        let r0 = rest.first().map(|c| c.to_lowercase()).unwrap_or_default();
        let r1 = rest.get(1).map(|c| c.to_lowercase()).unwrap_or_default();
        let (nb, n): (&str, usize) = match (base.as_str(), r0.as_str(), r1.as_str()) {
            ("%USERPROFILE%", "appdata", "local") => ("%LOCALAPPDATA%", 2),
            ("%USERPROFILE%", "appdata", "locallow") => ("%LOCALLOW%", 2),
            ("%USERPROFILE%", "appdata", "roaming") => ("%APPDATA%", 2),
            ("%USERPROFILE%", "documents", _) | ("%USERPROFILE%", "my documents", _) => ("%DOCUMENTS%", 1),
            ("%USERPROFILE%", "saved games", _) => ("%SAVEDGAMES%", 1),
            ("%USERPROFILE%", "onedrive", "documents") => ("%DOCUMENTS%", 2),
            ("$HOME", ".local", "share") => ("$XDG_DATA_HOME", 2),
            ("$HOME", ".config", _) => ("$XDG_CONFIG_HOME", 1),
            _ => break,
        };
        base = nb.into();
        rest.drain(..n);
    }
    let rest: Vec<String> = rest.into_iter().map(|c| if is_placeholder(&c) { "*".to_string() } else { c }).collect();
    if rest.is_empty() || rest[0] == "*" || rest.len() > 10 {
        return None;
    }
    if rest.iter().any(|c| c == "." || c == ".." || c.contains(':') || c.contains('%') || c.len() > 120) {
        return None;
    }
    // Un composant generique (« AppData », « Users »...) en tete signe un chemin mal compris.
    if ["appdata", "users", "local", "roaming", "locallow"].contains(&rest[0].to_lowercase().as_str()) {
        return None;
    }
    Some(format!("{}/{}", base, rest.join("/")))
}

/// Variantes d'un gabarit issu d'un texte, de la plus longue a la plus courte : le dernier composant peut contenir la
/// suite de la phrase (« Saves and ... ») ; on retire des mots, puis (variante marquee `true`) le dernier composant
/// entier, une seule fois et jamais jusqu'a la base seule.
pub fn shorter_variants(tpl: &str) -> Vec<(String, bool)> {
    let Some((ph, rest)) = split_template(tpl) else { return vec![] };
    let mut out: Vec<(String, bool)> = Vec::new();
    let Some(last) = rest.last() else { return out };
    let head = &rest[..rest.len() - 1];
    let words: Vec<&str> = last.split(' ').filter(|w| !w.is_empty()).collect();
    // Un nom de dossier a rarement plus de 8 mots : au-dela, c'est la suite de la phrase.
    for w in (1..=words.len().min(8)).rev() {
        let l = words[..w].join(" ");
        let l = l.trim_end_matches(['.', ',', ':', '-']).to_string();
        if l.is_empty() {
            continue;
        }
        let mut v: Vec<String> = head.to_vec();
        v.push(l);
        let t = format!("{}/{}", ph, v.join("/"));
        if !out.iter().any(|(x, _)| *x == t) {
            out.push((t, false));
        }
    }
    if !head.is_empty() {
        out.push((format!("{}/{}", ph, head.join("/")), true));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_des_formes_courantes() {
        let cases = [
            ("%APPDATA%\\StardewValley\\Saves", Some("%APPDATA%/StardewValley/Saves")),
            ("%USERPROFILE%\\AppData\\LocalLow\\Team Cherry\\Hollow Knight", Some("%LOCALLOW%/Team Cherry/Hollow Knight")),
            ("C:\\Users\\<username>\\AppData\\Local\\Pal\\Saved\\SaveGames", Some("%LOCALAPPDATA%/Pal/Saved/SaveGames")),
            ("C:\\Users\\Bob\\Documents\\My Games\\Terraria", Some("%DOCUMENTS%/My Games/Terraria")),
            ("C:/Users/[YourName]/Saved Games/Frontier", Some("%SAVEDGAMES%/Frontier")),
            ("%LocalAppData%Low\\Studio\\Game", Some("%LOCALLOW%/Studio/Game")),
            ("%APPDATA%\\..\\LocalLow\\Studio\\Game", Some("%LOCALLOW%/Studio/Game")),
            ("%HOMEDRIVE%%HOMEPATH%\\Documents\\X", Some("%DOCUMENTS%/X")),
            ("Documents\\My Games\\Skyrim Special Edition\\Saves", Some("%DOCUMENTS%/My Games/Skyrim Special Edition/Saves")),
            ("%LOCALAPPDATA%\\Game\\Saved\\SaveGames\\76561198012345678", Some("%LOCALAPPDATA%/Game/Saved/SaveGames/*")),
            ("%APPDATA%\\Game\\{SteamID}\\saves", Some("%APPDATA%/Game/*/saves")),
            ("~/.local/share/Terraria", Some("$XDG_DATA_HOME/Terraria")),
            ("~/.config/unity3d/Team Cherry/Hollow Knight", Some("$XDG_CONFIG_HOME/unity3d/Team Cherry/Hollow Knight")),
            ("$HOME/.steam/steam/steamapps/compatdata/1091500/pfx/drive_c/users/steamuser/Saved Games/CD Projekt Red/Cyberpunk 2077", Some("%SAVEDGAMES%/CD Projekt Red/Cyberpunk 2077")),
            ("/home/deck/.local/share/Steam/steamapps/compatdata/1/pfx/drive_c/users/steamuser/AppData/Roaming/X", Some("%APPDATA%/X")),
            ("%APPDATA%", None),
            ("%APPDATA%\\<user id>", None),
            ("C:\\Program Files\\Game\\save", None),
            ("D:\\Games\\X\\Saves", None),
            ("%TEMP%\\x", None),
            ("%USERPROFILE%\\AppData\\x", None),
        ];
        for (raw, want) in cases {
            assert_eq!(normalize(raw).as_deref(), want, "{}", raw);
        }
    }

    #[test]
    fn extraction_dans_un_texte() {
        let t = "Saves are in %APPDATA%\\StardewValley\\Saves. Config: C:\\Users\\<user>\\AppData\\LocalLow\\Team Cherry\\Hollow Knight\\ (Windows), or ~/.config/unity3d/Team Cherry/Hollow Knight on Linux, \"%USERPROFILE%\\Documents\\My Games\\Terraria\"";
        let v = extract_paths(t);
        assert!(v.contains(&"%APPDATA%\\StardewValley\\Saves".to_string()), "{:?}", v);
        assert!(v.contains(&"C:\\Users\\<user>\\AppData\\LocalLow\\Team Cherry\\Hollow Knight".to_string()), "{:?}", v);
        assert_eq!(normalize("C:\\Users\\<user>\\AppData\\LocalLow\\Team Cherry\\Hollow Knight").as_deref(), Some("%LOCALLOW%/Team Cherry/Hollow Knight"));
        assert_eq!(normalize("%APPDATA%\\Code\\User").as_deref(), Some("%APPDATA%/Code/User"), "« User » est un vrai dossier");
        assert!(v.contains(&"~/.config/unity3d/Team Cherry/Hollow Knight on Linux".to_string()), "{:?}", v);
        assert!(v.contains(&"%USERPROFILE%\\Documents\\My Games\\Terraria".to_string()), "{:?}", v);
        // 100% de reussite ou 50 % ne sont pas des variables.
        assert!(extract_paths("100% sure, 50% done / maybe").is_empty());
    }

    #[test]
    fn html_vers_texte() {
        let h = "<p>Path: <code>C:\\Users\\<wbr>name\\AppData\\<span>Local</span>\\X</code></p><script>var a='%APPDATA%\\\\bad';</script>&lt;b&gt; &amp; &#92;&#x5C;";
        let t = html_to_text(h);
        assert!(t.contains("C:\\Users\\name\\AppData\\Local\\X"), "{}", t);
        assert!(!t.contains("bad"));
        assert!(t.contains("<b> & \\\\"));
    }

    #[test]
    fn variantes_plus_courtes() {
        let v = shorter_variants("%APPDATA%/Studio/Saves and more");
        assert_eq!(v[0], ("%APPDATA%/Studio/Saves and more".to_string(), false));
        assert!(v.contains(&("%APPDATA%/Studio/Saves".to_string(), false)));
        assert_eq!(v.last(), Some(&("%APPDATA%/Studio".to_string(), true)));
        assert!(!v.iter().any(|(x, _)| x == "%APPDATA%/" || x == "%APPDATA%"));
        assert_eq!(shorter_variants("%APPDATA%/Jeu"), vec![("%APPDATA%/Jeu".to_string(), false)], "jamais la base seule");
    }

    #[test]
    fn gabarit_aller_retour() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        let save = f.locallow.as_ref().unwrap().join("Team Cherry").join("Hollow Knight");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::write(save.join("user1.dat"), b"x").unwrap();
        let tpl = to_generic(&save, &f).unwrap();
        assert_eq!(tpl, "%LOCALLOW%/Team Cherry/Hollow Knight");
        assert_eq!(expand(&tpl, &f, None), vec![save.clone()]);
        // Joker : identifiant de compte.
        let id = f.local.as_ref().unwrap().join("Pal").join("Saved").join("SaveGames").join("76561198000000001");
        std::fs::create_dir_all(&id).unwrap();
        assert_eq!(expand("%LOCALAPPDATA%/Pal/Saved/SaveGames/*", &f, None), vec![id]);
        // Hors base : aucun gabarit.
        assert_eq!(to_generic(&t.path().join("Jeux").join("X"), &f), None);
        assert!(has_recent_files(&save, crate::util::now_ms(), 1));
        assert!(!has_recent_files(&save, crate::util::now_ms() + 10 * 86_400_000, 2));
        // Dossier d'installation.
        let inst = t.path().join("Jeu");
        std::fs::create_dir_all(inst.join("Saves")).unwrap();
        assert_eq!(expand("%INSTALLDIR%/Saves", &f, Some(&inst)), vec![inst.join("Saves")]);
        assert!(expand("%INSTALLDIR%/Saves", &f, None).is_empty());
        assert!(expand("%APPDATA%/../x", &f, None).is_empty());
    }
}
