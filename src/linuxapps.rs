//! Programmes sous Linux : raccourcis `.desktop`, applications Flatpak, AppImage, scripts, raccourcis Steam, et icones du
//! theme du bureau. Les fonctions prennent leurs racines (dossiers, `PATH`) en parametre : elles sont PURES vis-a-vis de
//! l'environnement et testees sur toutes les plateformes, mais ne servent en pratique que sous Linux.
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ fichiers .desktop

/// Entree `[Desktop Entry]` d'un fichier `.desktop` (seulement ce qui sert ici).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Fichier d'origine.
    pub file: PathBuf,
    pub name: String,
    pub exec: String,
    pub try_exec: Option<String>,
    pub icon: Option<String>,
    pub wm_class: Option<String>,
    /// Dossier de travail (`Path=`).
    pub workdir: Option<String>,
}

/// Valeur d'une chaine `.desktop` : sequences `\s`, `\n`, `\t`, `\r`, `\\`.
fn unescape_value(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Lit le groupe `[Desktop Entry]` d'une application (Type=Application). None : autre type, entree cachee, ou sans Exec.
pub fn parse_desktop(text: &str) -> Option<DesktopEntry> {
    let mut in_group = false;
    let mut e = DesktopEntry::default();
    let mut typ = String::new();
    let mut hidden = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_group = l == "[Desktop Entry]";
            continue;
        }
        if !in_group || l.starts_with('#') {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        let (k, v) = (k.trim(), unescape_value(v.trim()));
        match k {
            "Type" => typ = v,
            "Name" => e.name = v,
            "Exec" => e.exec = v,
            "TryExec" => e.try_exec = Some(v).filter(|s| !s.is_empty()),
            "Icon" => e.icon = Some(v).filter(|s| !s.is_empty()),
            "StartupWMClass" => e.wm_class = Some(v).filter(|s| !s.is_empty()),
            "Path" => e.workdir = Some(v).filter(|s| !s.is_empty()),
            "Hidden" => hidden = v.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    if hidden || !typ.eq_ignore_ascii_case("Application") || e.exec.trim().is_empty() {
        return None;
    }
    Some(e)
}

/// Decoupe la cle `Exec` (specification Desktop Entry : guillemets doubles avec `\"`, `` \` ``, `\$`, `\\`) et retire les
/// codes de champ (`%f`, `%U`...). `%%` devient `%`.
pub fn exec_args(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut any = false;
    let mut quoted = false;
    let mut it = exec.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            // Hors guillemets, « \ » suivi d'un espace (raccourcis ecrits par Wine) : espace litteral.
            '\\' if it.peek().map(|n| *n == ' ').unwrap_or(false) => {
                cur.push(' ');
                it.next();
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .filter_map(|a| {
            if a.len() == 2 && a.starts_with('%') && a != "%%" {
                None // code de champ seul (%f, %U, %i, %c, %k...)
            } else {
                Some(a.replace("%%", "%"))
            }
        })
        .collect()
}

/// Ce qu'un raccourci lance vraiment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Programme (chemin absolu ou nom cherche dans `PATH` ; un `.exe` Windows pour Wine).
    Program(String),
    /// Application Flatpak (`flatpak run [--command=X] id`).
    Flatpak { id: String, command: Option<String> },
    /// Jeu Steam (`steam steam://rungameid/<appid>`).
    Steam(u64),
}

fn base(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

/// Enveloppes courantes qui lancent le vrai programme en argument.
const WRAPPERS: &[&str] = &["env", "gamemoderun", "mangohud", "prime-run", "nice", "ionice", "taskset", "obs-gamecapture", "dbus-run-session", "pw-jack", "strangle", "nohup", "setsid"];

fn steam_appid(args: &[String]) -> Option<u64> {
    for (i, a) in args.iter().enumerate() {
        for p in ["steam://rungameid/", "steam://run/", "steam://launch/"] {
            if let Some(r) = a.strip_prefix(p) {
                return r.split(['/', '?']).next().and_then(|x| x.parse().ok());
            }
        }
        if a == "-applaunch" {
            return args.get(i + 1).and_then(|x| x.parse().ok());
        }
    }
    None
}

/// Cible d'une ligne `Exec`. None : rien d'exploitable (lanceur tiers par URL, commande vide...).
pub fn exec_target(exec: &str) -> Option<Target> {
    let args = exec_args(exec);
    target_of(&args, 0)
}

fn target_of(args: &[String], depth: u32) -> Option<Target> {
    if depth > 3 {
        return None;
    }
    if let Some(id) = steam_appid(args) {
        return Some(Target::Steam(id));
    }
    let mut i = 0;
    // VAR=valeur, enveloppes et leurs options.
    while let Some(a) = args.get(i) {
        let b = base(a);
        let is_var = a.split_once('=').map(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(false);
        if is_var {
            i += 1;
        } else if WRAPPERS.contains(&b) {
            i += 1;
            while args.get(i).map(|x| x.starts_with('-')).unwrap_or(false) {
                i += 1;
            }
        } else {
            break;
        }
    }
    let prog = args.get(i)?;
    let b = base(prog).to_lowercase();
    let rest = &args[i + 1..];
    if b == "flatpak" && rest.first().map(|r| r == "run").unwrap_or(false) {
        let mut command = None;
        let mut j = 1;
        while let Some(a) = rest.get(j) {
            if let Some(c) = a.strip_prefix("--command=") {
                command = Some(c.to_string());
            } else if a == "--command" {
                command = rest.get(j + 1).cloned();
                j += 1;
            } else if !a.starts_with('-') {
                return Some(Target::Flatpak { id: a.clone(), command });
            }
            j += 1;
        }
        return None;
    }
    if (b == "sh" || b == "bash" || b == "dash") && rest.first().map(|r| r == "-c").unwrap_or(false) {
        // « cd /dossier && ./programme » : derniere commande, relative au dernier « cd ».
        let inner = rest.get(1)?;
        let mut cd: Option<String> = None;
        let mut last: Vec<String> = Vec::new();
        for seg in inner.split("&&").flat_map(|s| s.split(';')) {
            let a = exec_args(seg.trim());
            if a.first().map(|x| x == "cd").unwrap_or(false) {
                cd = a.get(1).cloned();
            } else if !a.is_empty() {
                last = a;
            }
        }
        return match target_of(&last, depth + 1)? {
            Target::Program(p) if !p.starts_with('/') && cd.is_some() => {
                let d = cd.unwrap_or_default();
                // Chemin Linux (separateur '/' quelle que soit la plateforme de compilation).
                Some(Target::Program(format!("{}/{}", d.trim_end_matches('/'), p.trim_start_matches("./"))))
            }
            t => Some(t),
        };
    }
    if b.starts_with("wine") || b == "proton" {
        // Raccourci du menu ecrit par Wine : « start.exe /Unix <chemin du .lnk> ».
        if let Some(i) = rest.iter().position(|a| a.eq_ignore_ascii_case("/unix")) {
            if let Some(l) = rest.get(i + 1) {
                return Some(Target::Program(l.clone()));
            }
        }
        if let Some(x) = rest.iter().find(|a| a.to_lowercase().ends_with(".exe")) {
            return Some(Target::Program(x.clone()));
        }
        return None;
    }
    if b == "xdg-open" || b == "lutris" || b == "heroic" || b == "steam" {
        return None; // lanceur par URL : cible inconnue ici
    }
    Some(Target::Program(prog.clone()))
}

/// Cherche un nom de programme dans `PATH` (premier fichier existant).
pub fn which(name: &str, path_env: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return if p.is_file() { Some(p) } else { None };
    }
    path_env.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

// ------------------------------------------------------------------ Flatpak

/// Installations Flatpak (utilisateur puis systeme).
pub fn flatpak_roots(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".local/share/flatpak"), PathBuf::from("/var/lib/flatpak")]
}

/// Dossier `files` de l'application (contenu de `/app` dans le bac a sable).
pub fn flatpak_files(id: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if id.is_empty() || id.contains('/') || id.contains("..") {
        return None;
    }
    roots.iter().map(|r| r.join("app").join(id).join("current/active/files")).find(|p| p.is_dir())
}

/// Commande par defaut de l'application (fichier `metadata`, groupe `[Application]`, cle `command`).
pub fn flatpak_command(id: &str, roots: &[PathBuf]) -> Option<String> {
    let files = flatpak_files(id, roots)?;
    let meta = std::fs::read_to_string(files.parent()?.join("metadata")).ok()?;
    let mut in_app = false;
    for l in meta.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_app = l == "[Application]";
        } else if in_app {
            if let Some(v) = l.strip_prefix("command=") {
                return Some(v.trim().to_string()).filter(|s| !s.is_empty());
            }
        }
    }
    None
}

/// Identifiant Flatpak d'un chemin d'installation (`.../flatpak/app/<id>/...`).
pub fn flatpak_id_from_path(p: &Path) -> Option<String> {
    let c: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let i = c.windows(2).position(|w| w[0] == "flatpak" && w[1] == "app")?;
    c.get(i + 2).cloned().filter(|s| s.contains('.'))
}

/// Identifiant Flatpak d'un processus d'apres son `/proc/<pid>/cgroup` (`app-flatpak-<id>-<nombre>.scope`).
pub fn flatpak_id_from_cgroup(text: &str) -> Option<String> {
    let i = text.find("app-flatpak-")?;
    let rest = &text[i + "app-flatpak-".len()..];
    let scope = rest.split(['/', '\n']).next()?;
    let scope = scope.strip_suffix(".scope").unwrap_or(scope);
    // l'identifiant contient des points ; le dernier « -nombre » est l'instance.
    let (id, n) = scope.rsplit_once('-')?;
    if n.chars().all(|c| c.is_ascii_digit()) && id.contains('.') {
        Some(id.to_string())
    } else {
        None
    }
}

/// Chemin hote du programme d'un processus Flatpak (`/app/bin/x` dans le bac a sable).
pub fn flatpak_host_path(id: &str, inside: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let files = flatpak_files(id, roots)?;
    let rel = inside.strip_prefix("/app").ok()?;
    let p = files.join(rel);
    if p.exists() { Some(p) } else { None }
}

// ------------------------------------------------------------------ Steam

/// Dossier d'installation d'un AppID dans les bibliotheques Steam (`appmanifest_<id>.acf`).
pub fn steam_install_dir(appid: u64, libs: &[PathBuf]) -> Option<PathBuf> {
    for l in libs {
        let apps = l.join("steamapps");
        if let Ok(t) = std::fs::read_to_string(apps.join(format!("appmanifest_{}.acf", appid))) {
            if let Ok((_, _, dir)) = crate::detect::parse_acf(&t) {
                let d = apps.join("common").join(dir);
                if d.is_dir() {
                    return Some(d);
                }
            }
        }
    }
    None
}

/// AppID Steam d'un programme installe sous `<bibliotheque>/steamapps/common/<dossier>/...`.
pub fn steam_appid_of_path(p: &Path) -> Option<u64> {
    let mut cur = p.parent();
    while let Some(d) = cur {
        if d.parent().and_then(|c| c.file_name()).map(|n| n.eq_ignore_ascii_case("common")).unwrap_or(false) {
            let apps = d.parent()?.parent()?;
            let name = d.file_name()?.to_string_lossy().to_string();
            return crate::autofill::steam_app_id(apps, &name);
        }
        cur = d.parent();
    }
    None
}

// ------------------------------------------------------------------ fichiers executables

/// Le fichier est-il un programme Linux (ELF) ?
pub fn is_elf(p: &Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 4];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).map(|_| &b == b"\x7fELF").unwrap_or(false)
}

/// Le fichier commence-t-il par `#!` (script) ?
pub fn is_script(p: &Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 2];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).map(|_| &b == b"#!").unwrap_or(false)
}

#[cfg(unix)]
fn exec_bit(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn exec_bit(_p: &Path) -> bool {
    false
}

/// Peut-on surveiller ce fichier comme programme ? `.exe` (Proton / Wine), AppImage, ELF, script, ou fichier executable.
pub fn is_program_file(p: &Path) -> bool {
    if !p.is_file() {
        return false;
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "exe" | "appimage" | "sh" | "x86_64" | "x86") || exec_bit(p) || is_elf(p) || is_script(p)
}

const JUNK: &[&str] = &["unins", "crash", "redist", "setup", "vc_", "dxsetup", "dotnet", "uninstall", "install", "update", "helper", "launcherhelper", "report", "cefprocess", "webhelper"];

/// Programme principal le plus probable d'un dossier d'installation (jeu Steam lance par raccourci) : un `.exe` connu,
/// sinon le plus gros `.exe` ou ELF executable jusqu'a 3 niveaux, en preferant un nom proche du dossier.
pub fn main_executable(dir: &Path, known_exes: &[String]) -> Option<PathBuf> {
    let dir_norm = crate::detect::norm_name(&dir.file_name()?.to_string_lossy());
    let mut best: Option<(u64, bool, PathBuf)> = None;
    for e in walkdir::WalkDir::new(dir).max_depth(3).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        let n = e.file_name().to_string_lossy().to_lowercase();
        if known_exes.iter().any(|k| k.eq_ignore_ascii_case(&n)) {
            return Some(e.path().to_path_buf());
        }
        if JUNK.iter().any(|j| n.contains(j)) || n.ends_with(".so") || n.contains(".so.") || n.ends_with(".dll") {
            continue;
        }
        let p = e.path();
        if !(n.ends_with(".exe") || (exec_bit(p) && is_elf(p))) {
            continue;
        }
        let sz = e.metadata().map(|m| m.len()).unwrap_or(0);
        let stem = n.split('.').next().unwrap_or("");
        let near = !stem.is_empty() && (dir_norm.contains(&crate::detect::norm_name(stem)) || crate::detect::norm_name(stem).contains(&dir_norm));
        let better = match &best {
            None => true,
            Some((bs, bn, _)) => (near && !bn) || (near == *bn && sz > *bs),
        };
        if better {
            best = Some((sz, near, p.to_path_buf()));
        }
    }
    best.map(|b| b.2)
}

// ------------------------------------------------------------------ resolution d'un fichier choisi

/// Racines du systeme utiles a la resolution (passees en parametre pour les tests).
#[derive(Clone, Debug, Default)]
pub struct SysRoots {
    pub home: PathBuf,
    /// Valeur de `PATH`.
    pub path_env: String,
    pub flatpak: Vec<PathBuf>,
    pub steam_libs: Vec<PathBuf>,
}

impl SysRoots {
    /// Valeurs reelles de cette machine.
    pub fn real() -> SysRoots {
        let home = dirs::home_dir().unwrap_or_default();
        let steam_libs = crate::detect::steam_root().map(|r| crate::detect::steam_libraries(&r)).unwrap_or_default();
        SysRoots { flatpak: flatpak_roots(&home), path_env: std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()), home, steam_libs }
    }
}

/// Programme lance par un raccourci `.desktop` (chemin hote si possible, sinon nom du programme).
pub fn program_from_desktop(e: &DesktopEntry, r: &SysRoots, known_exes_for_appid: &dyn Fn(u64) -> Vec<String>) -> Option<PathBuf> {
    match exec_target(&e.exec)? {
        Target::Program(p) => {
            if p.to_lowercase().ends_with(".lnk") {
                // Raccourci Windows dans un prefixe Wine : son .exe cible (le nom suffit a reconnaitre le processus).
                let t = crate::addform::parse_lnk(&std::fs::read(&p).ok()?)?;
                return Some(PathBuf::from(base(&t.to_string_lossy())));
            }
            if p.to_lowercase().ends_with(".exe") {
                // Raccourci cree par Wine : « C:\...\jeu.exe » ou « Z:\... » ; le nom suffit a reconnaitre le processus.
                if let Some(x) = crate::proton::translate_windows_path(&p, Path::new("/nonexistent-prefix")).filter(|x| !x.starts_with("/nonexistent-prefix")) {
                    return Some(x);
                }
                return Some(PathBuf::from(base(&p)));
            }
            let found = which(&p, &r.path_env).or_else(|| e.workdir.as_ref().map(|w| Path::new(w).join(&p)).filter(|x| x.is_file()));
            Some(found.unwrap_or_else(|| PathBuf::from(p)))
        }
        Target::Flatpak { id, command } => {
            let cmd = command.or_else(|| flatpak_command(&id, &r.flatpak))?;
            let inside = if cmd.starts_with('/') { PathBuf::from(&cmd) } else { Path::new("/app/bin").join(&cmd) };
            Some(flatpak_host_path(&id, &inside, &r.flatpak).unwrap_or_else(|| PathBuf::from(base(&cmd))))
        }
        Target::Steam(id) => {
            let dir = steam_install_dir(id, &r.steam_libs)?;
            main_executable(&dir, &known_exes_for_appid(id))
        }
    }
}

/// Fichier depose ou choisi -> programme a surveiller. `.desktop` : programme lance ; `.exe`, AppImage, binaire, script :
/// le fichier lui-meme. None : dossier, document, raccourci sans cible exploitable.
pub fn resolve_input(p: &Path, r: &SysRoots, known_exes_for_appid: &dyn Fn(u64) -> Vec<String>) -> Option<PathBuf> {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "desktop" {
        let e = parse_desktop(&std::fs::read_to_string(p).ok()?)?;
        return program_from_desktop(&DesktopEntry { file: p.to_path_buf(), ..e }, r, known_exes_for_appid);
    }
    if ext == "exe" {
        return Some(p.to_path_buf());
    }
    if is_program_file(p) {
        return Some(p.to_path_buf());
    }
    None
}

// ------------------------------------------------------------------ index des raccourcis (nom, icone)

/// Dossiers `applications` (utilisateur, Flatpak, systeme, `$XDG_DATA_DIRS`).
pub fn data_dirs(home: &Path, xdg_data_home: Option<&str>, xdg_data_dirs: Option<&str>) -> Vec<PathBuf> {
    let mut v = vec![xdg_data_home.filter(|s| s.starts_with('/')).map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"))];
    v.push(home.join(".local/share/flatpak/exports/share"));
    v.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    let sys = xdg_data_dirs.filter(|s| !s.trim().is_empty()).unwrap_or("/usr/local/share:/usr/share");
    for d in sys.split(':').filter(|d| d.starts_with('/')) {
        let p = PathBuf::from(d);
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

/// Lit tous les raccourcis d'application (`<data>/applications/**.desktop`, 4 niveaux).
pub fn load_entries(data: &[PathBuf]) -> Vec<DesktopEntry> {
    let mut out = Vec::new();
    for d in data {
        let apps = d.join("applications");
        for e in walkdir::WalkDir::new(&apps).max_depth(4).into_iter().filter_map(|e| e.ok()) {
            if !e.file_type().is_file() || !e.file_name().to_string_lossy().ends_with(".desktop") {
                continue;
            }
            if let Some(x) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_desktop(&t)) {
                out.push(DesktopEntry { file: e.path().to_path_buf(), ..x });
            }
        }
    }
    out
}

/// Raccourci correspondant a un programme : meme Flatpak, meme jeu Steam, meme programme (chemin ou nom), ou meme
/// `StartupWMClass`.
pub fn entry_for_program<'a>(entries: &'a [DesktopEntry], program: &Path, appid: Option<u64>) -> Option<&'a DesktopEntry> {
    let pb = base(&program.to_string_lossy()).to_lowercase();
    let stem = pb.split('.').next().unwrap_or("").to_string();
    let fid = flatpak_id_from_path(program);
    let mut by_name: Option<&DesktopEntry> = None;
    for e in entries {
        match exec_target(&e.exec) {
            Some(Target::Flatpak { id, .. }) if fid.as_deref() == Some(id.as_str()) => return Some(e),
            Some(Target::Steam(id)) if appid == Some(id) => return Some(e),
            Some(Target::Program(p)) => {
                if Path::new(&p) == program {
                    return Some(e);
                }
                if by_name.is_none() && !pb.is_empty() && base(&p).to_lowercase() == pb {
                    by_name = Some(e);
                }
            }
            _ => {}
        }
        if by_name.is_none() {
            let te = e.try_exec.as_deref().map(|t| base(t).to_lowercase());
            let wm = e.wm_class.as_deref().map(|w| w.to_lowercase());
            if te.as_deref() == Some(pb.as_str()) || (!stem.is_empty() && wm.as_deref() == Some(stem.as_str())) {
                by_name = Some(e);
            }
        }
    }
    by_name
}

/// Fichier PNG d'une icone : chemin absolu, ou nom cherche dans le theme `hicolor` (plus grande taille d'abord) puis
/// dans `pixmaps`. Les icones SVG seules ne sont pas utilisees (pas de moteur de rendu SVG embarque).
pub fn find_icon(icon: &str, data: &[PathBuf]) -> Option<PathBuf> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    if icon.starts_with('/') {
        let p = PathBuf::from(icon);
        return if p.extension().map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false) && p.is_file() { Some(p) } else { None };
    }
    let name = icon.strip_suffix(".png").unwrap_or(icon);
    for size in ["256x256", "128x128", "96x96", "64x64", "48x48", "32x32"] {
        for d in data {
            for theme in ["hicolor", "breeze", "Adwaita"] {
                let p = d.join("icons").join(theme).join(size).join("apps").join(format!("{}.png", name));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    data.iter().map(|d| d.join("pixmaps").join(format!("{}.png", name))).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_is_parsed_and_unescaped() {
        let t = "# c\n[Desktop Entry]\nType=Application\nName=Mon Jeu\nExec=\"/home/deck/Jeux/Mon Jeu/start.sh\" %U\nIcon=mon-jeu\nStartupWMClass=MonJeu\n[Desktop Action x]\nExec=autre\n";
        let e = parse_desktop(t).unwrap();
        assert_eq!(e.name, "Mon Jeu");
        assert_eq!(e.icon.as_deref(), Some("mon-jeu"));
        assert_eq!(e.wm_class.as_deref(), Some("MonJeu"));
        assert_eq!(exec_args(&e.exec), vec!["/home/deck/Jeux/Mon Jeu/start.sh"]);
        assert!(parse_desktop("[Desktop Entry]\nType=Link\nExec=x\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nType=Application\nHidden=true\nExec=x\n").is_none());
    }

    #[test]
    fn exec_targets_cover_common_launchers() {
        assert_eq!(exec_target("flatpak run --branch=stable --arch=x86_64 --command=retroarch org.libretro.RetroArch"), Some(Target::Flatpak { id: "org.libretro.RetroArch".into(), command: Some("retroarch".into()) }));
        assert_eq!(exec_target("/usr/bin/flatpak run org.DolphinEmu.dolphin-emu %f"), Some(Target::Flatpak { id: "org.DolphinEmu.dolphin-emu".into(), command: None }));
        assert_eq!(exec_target("steam steam://rungameid/1245620"), Some(Target::Steam(1245620)));
        assert_eq!(exec_target("env SteamAppId=1 /usr/bin/steam -applaunch 413150"), Some(Target::Steam(413150)));
        assert_eq!(exec_target("env WINEPREFIX=\"/home/deck/.wine\" wine C:\\\\Games\\\\Jeu\\\\jeu.exe"), Some(Target::Program("C:\\\\Games\\\\Jeu\\\\jeu.exe".into())));
        assert_eq!(
            exec_target("env WINEPREFIX=\"/home/u/.wine\" wine C:\\\\windows\\\\command\\\\start.exe /Unix /home/u/.wine/dosdevices/c:/users/u/Start\\ Menu/Programs/Jeu.lnk"),
            Some(Target::Program("/home/u/.wine/dosdevices/c:/users/u/Start Menu/Programs/Jeu.lnk".into()))
        );
        assert_eq!(exec_target("env DXVK_HUD=1 gamemoderun /opt/jeu/jeu --fs"), Some(Target::Program("/opt/jeu/jeu".into())));
        assert_eq!(exec_target("sh -c \"cd /opt/x && ./x.x86_64\""), Some(Target::Program("/opt/x/x.x86_64".into())));
        assert_eq!(exec_target("xdg-open heroic://launch/legendary/abc"), None);
        assert_eq!(exec_target(""), None);
    }

    #[test]
    fn flatpak_ids_from_paths_and_cgroups() {
        assert_eq!(flatpak_id_from_path(Path::new("/var/lib/flatpak/app/org.libretro.RetroArch/x86_64/stable/abc/files/bin/retroarch")).as_deref(), Some("org.libretro.RetroArch"));
        assert_eq!(flatpak_id_from_path(Path::new("/usr/bin/retroarch")), None);
        let cg = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-flatpak-org.DolphinEmu.dolphin\\x2demu-4242.scope\n";
        assert_eq!(flatpak_id_from_cgroup(cg).as_deref(), Some("org.DolphinEmu.dolphin\\x2demu"));
        assert_eq!(flatpak_id_from_cgroup("0::/user.slice/app-flatpak-com.valvesoftware.Steam-17.scope").as_deref(), Some("com.valvesoftware.Steam"));
        assert_eq!(flatpak_id_from_cgroup("0::/init.scope"), None);
    }

    #[test]
    fn resolve_desktop_flatpak_and_files() {
        let t = std::env::temp_dir().join(format!("agss-linuxapps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let fp = t.join("flatpak");
        let files = fp.join("app/org.ppsspp.PPSSPP/current/active/files");
        std::fs::create_dir_all(files.join("bin")).unwrap();
        std::fs::write(files.join("bin/PPSSPPSDL"), b"\x7fELFxxxx").unwrap();
        std::fs::write(fp.join("app/org.ppsspp.PPSSPP/current/active/metadata"), "[Application]\nname=org.ppsspp.PPSSPP\ncommand=PPSSPPSDL\n").unwrap();
        let r = SysRoots { home: t.clone(), path_env: String::new(), flatpak: vec![fp.clone()], steam_libs: vec![] };
        let none = |_: u64| -> Vec<String> { vec![] };
        let d = t.join("ppsspp.desktop");
        std::fs::write(&d, "[Desktop Entry]\nType=Application\nName=PPSSPP\nExec=/usr/bin/flatpak run --branch=stable org.ppsspp.PPSSPP @@u %U @@\n").unwrap();
        assert_eq!(resolve_input(&d, &r, &none), Some(files.join("bin/PPSSPPSDL")));
        // AppImage et script : le fichier lui-meme ; un document : rien.
        let ai = t.join("Cemu.AppImage");
        std::fs::write(&ai, b"\x7fELF").unwrap();
        assert_eq!(resolve_input(&ai, &r, &none), Some(ai.clone()));
        let sh = t.join("lancer jeu");
        std::fs::write(&sh, "#!/bin/sh\necho\n").unwrap();
        assert_eq!(resolve_input(&sh, &r, &none), Some(sh.clone()));
        let doc = t.join("notes.txt");
        std::fs::write(&doc, "texte").unwrap();
        assert_eq!(resolve_input(&doc, &r, &none), None);
        assert_eq!(resolve_input(&t, &r, &none), None);
        // Wine : chemin Z: traduit, chemin C: reduit au nom de l'exe.
        let w = t.join("w.desktop");
        std::fs::write(&w, "[Desktop Entry]\nType=Application\nName=W\nExec=env WINEPREFIX=/x wine C:\\\\\\\\Jeux\\\\\\\\W\\\\\\\\w.exe\n").unwrap();
        assert_eq!(resolve_input(&w, &r, &none), Some(PathBuf::from("w.exe")));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn steam_shortcut_resolves_to_main_executable() {
        let t = std::env::temp_dir().join(format!("agss-linuxapps-steam-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let lib = t.join("Steam");
        let game = lib.join("steamapps/common/Hollow Knight");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(lib.join("steamapps/appmanifest_367520.acf"), "\"AppState\"\n{\n\"appid\" \"367520\"\n\"name\" \"Hollow Knight\"\n\"installdir\" \"Hollow Knight\"\n}\n").unwrap();
        std::fs::write(game.join("hollow_knight.exe"), vec![0u8; 100]).unwrap();
        std::fs::write(game.join("UnityCrashHandler64.exe"), vec![0u8; 1000]).unwrap();
        let r = SysRoots { home: t.clone(), path_env: String::new(), flatpak: vec![], steam_libs: vec![lib.clone()] };
        let d = t.join("hk.desktop");
        std::fs::write(&d, "[Desktop Entry]\nType=Application\nName=Hollow Knight\nExec=steam steam://rungameid/367520\nIcon=steam_icon_367520\n").unwrap();
        assert_eq!(resolve_input(&d, &r, &|_| vec![]), Some(game.join("hollow_knight.exe")));
        assert_eq!(steam_appid_of_path(&game.join("hollow_knight.exe")), Some(367520));
        let e = parse_desktop(&std::fs::read_to_string(&d).unwrap()).unwrap();
        assert!(entry_for_program(std::slice::from_ref(&e), &game.join("hollow_knight.exe"), Some(367520)).is_some());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn icons_are_found_in_the_theme() {
        let t = std::env::temp_dir().join(format!("agss-linuxapps-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let d = t.join("share");
        std::fs::create_dir_all(d.join("icons/hicolor/48x48/apps")).unwrap();
        std::fs::create_dir_all(d.join("icons/hicolor/256x256/apps")).unwrap();
        std::fs::write(d.join("icons/hicolor/48x48/apps/jeu.png"), b"png").unwrap();
        std::fs::write(d.join("icons/hicolor/256x256/apps/jeu.png"), b"png").unwrap();
        assert_eq!(find_icon("jeu", &[d.clone()]), Some(d.join("icons/hicolor/256x256/apps/jeu.png")));
        assert_eq!(find_icon("absent", &[d.clone()]), None);
        let entries = vec![DesktopEntry { name: "Jeu".into(), exec: "/opt/jeu/jeu.x86_64 --x".into(), icon: Some("jeu".into()), ..Default::default() }];
        assert_eq!(entry_for_program(&entries, Path::new("/opt/jeu/jeu.x86_64"), None).map(|e| e.name.as_str()), Some("Jeu"));
        assert_eq!(entry_for_program(&entries, Path::new("/ailleurs/jeu.x86_64"), None).map(|e| e.name.as_str()), Some("Jeu"));
        assert!(entry_for_program(&entries, Path::new("/opt/autre"), None).is_none());
        let _ = std::fs::remove_dir_all(&t);
    }
}
