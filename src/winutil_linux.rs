//! Equivalent Linux de `winutil` (meme interface) : theme du bureau, demarrage automatique (fichier `.desktop`), ouverture
//! de liens et de dossiers via `xdg-open`, lancement de programmes, masquage / affichage de la fenetre par les commandes
//! d'egui (aucun appel specifique a X11 ou a Wayland). Tout est « au mieux » : un echec ne gene jamais le reste.
use crate::vm::ThemeProbe;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Nom historique de la valeur de demarrage (Windows) : conserve pour l'interface commune.
pub const RUN_VALUE: &str = "AutoGameSoftwareSync";

// ------------------------------------------------------------------ fenetre (commandes egui)

static CTX: OnceLock<eframe::egui::Context> = OnceLock::new();
static VISIBLE: AtomicBool = AtomicBool::new(true);
static FOCUSED: AtomicBool = AtomicBool::new(false);
static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Memorise le contexte egui (appele une fois au demarrage de l'interface).
pub fn set_context(ctx: &eframe::egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// Appele a chaque image : memorise si la fenetre a le focus.
pub fn note_frame(ctx: &eframe::egui::Context) {
    FOCUSED.store(ctx.input(|i| i.viewport().focused.unwrap_or(false)), Ordering::SeqCst);
}

fn cmd(c: eframe::egui::ViewportCommand) {
    if let Some(ctx) = CTX.get() {
        ctx.send_viewport_cmd(c);
        ctx.request_repaint();
    }
}

pub fn show_window(_title: &str) {
    use eframe::egui::ViewportCommand as V;
    SHOW_REQUESTED.store(true, Ordering::SeqCst);
    VISIBLE.store(true, Ordering::SeqCst);
    cmd(V::Visible(true));
    cmd(V::Minimized(false));
    cmd(V::Focus);
}

pub fn hide_window(_title: &str) {
    VISIBLE.store(false, Ordering::SeqCst);
    cmd(eframe::egui::ViewportCommand::Visible(false));
}

/// Demarrage masque : la fenetre est cachee des la premiere image (aucun decalage hors ecran necessaire).
pub fn hidden_start_prepare(_title: &str) {
    if !SHOW_REQUESTED.load(Ordering::SeqCst) {
        VISIBLE.store(false, Ordering::SeqCst);
        cmd(eframe::egui::ViewportCommand::Visible(false));
    }
}

pub fn hidden_start_finish(_title: &str) {
    if !SHOW_REQUESTED.load(Ordering::SeqCst) {
        VISIBLE.store(false, Ordering::SeqCst);
        cmd(eframe::egui::ViewportCommand::Visible(false));
    }
}

pub fn own_window_visible(_title: &str) -> bool {
    VISIBLE.load(Ordering::SeqCst)
}

pub fn own_window_is_foreground() -> bool {
    FOCUSED.load(Ordering::SeqCst)
}

pub fn bring_to_front(title: &str) -> bool {
    show_window(title);
    std::thread::sleep(std::time::Duration::from_millis(150));
    FOCUSED.load(Ordering::SeqCst)
}

/// Pas d'equivalent portable : demande d'attention au gestionnaire de fenetres.
pub fn flash_taskbar(_title: &str) {
    cmd(eframe::egui::ViewportCommand::RequestUserAttention(eframe::egui::UserAttentionType::Informational));
}

/// Non detecte sous Linux (aucune API commune a X11 et Wayland) : la carte s'affiche normalement.
pub fn foreground_is_fullscreen() -> bool {
    false
}

static ANIM_ON: AtomicBool = AtomicBool::new(true);
static ANIM_PROBE: std::sync::Once = std::sync::Once::new();

/// `~/.config/kdeglobals` : `AnimationDurationFactor=0` dans `[KDE]` = animations coupees. None : reglage absent.
pub fn parse_kde_animations(kdeglobals: &str) -> Option<bool> {
    let mut in_kde = false;
    for l in kdeglobals.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_kde = l == "[KDE]";
        } else if in_kde {
            if let Some(v) = l.strip_prefix("AnimationDurationFactor=") {
                return v.trim().parse::<f32>().ok().map(|f| f > 0.0);
            }
        }
    }
    None
}

/// Sortie de `gsettings get org.gnome.desktop.interface enable-animations` (`true` / `false`).
pub fn parse_gsettings_bool(out: &str) -> Option<bool> {
    match out.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn probe_animations() -> bool {
    if let Some(cfg) = dirs::config_dir() {
        if let Some(on) = std::fs::read_to_string(cfg.join("kdeglobals")).ok().and_then(|t| parse_kde_animations(&t)) {
            return on;
        }
    }
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_gsettings_bool(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(true)
}

/// Animations du bureau activees ? (KDE Plasma : `AnimationDurationFactor` ; GNOME : `enable-animations`.) Lu dans un
/// thread toutes les 30 s : l'appel est instantane (utilise a chaque image).
pub fn client_animations_enabled() -> bool {
    ANIM_PROBE.call_once(|| {
        let _ = std::thread::Builder::new().name("agss-anim-probe".into()).spawn(|| loop {
            ANIM_ON.store(probe_animations(), Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_secs(30));
        });
    });
    ANIM_ON.load(Ordering::SeqCst)
}

static ALLY: OnceLock<bool> = OnceLock::new();

/// Steam Deck, console portable connue ou mode jeu de SteamOS (gamescope) ? Evalue une fois.
pub fn handheld_session() -> bool {
    *ALLY.get_or_init(|| {
        let rd = |f: &str| std::fs::read_to_string(format!("/sys/class/dmi/id/{}", f)).unwrap_or_default();
        crate::uilogic::handheld_from(&|k| std::env::var(k).ok(), &rd("board_vendor"), &rd("product_name"))
    })
}

// ------------------------------------------------------------------ console / theme

/// Rien a faire : un programme Linux est toujours rattache a son terminal.
pub fn attach_parent_console() {}

/// Theme du bureau (GNOME / GTK via `gsettings`, variable `GTK_THEME`). Inconnu => theme sombre par defaut.
pub struct RegThemeProbe;

/// Sortie de `gsettings get org.gnome.desktop.interface color-scheme` -> Some(true) si clair.
pub fn parse_color_scheme(output: &str) -> Option<bool> {
    let o = output.trim().trim_matches('\'').to_lowercase();
    match o.as_str() {
        "prefer-dark" => Some(false),
        "prefer-light" | "default" => Some(true),
        _ => None,
    }
}

impl ThemeProbe for RegThemeProbe {
    fn system_is_light(&self) -> Option<bool> {
        if let Ok(t) = std::env::var("GTK_THEME") {
            return Some(!t.to_lowercase().contains("dark"));
        }
        let out = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "color-scheme"]).stderr(std::process::Stdio::null()).output().ok()?;
        if !out.status.success() {
            return None;
        }
        parse_color_scheme(&String::from_utf8_lossy(&out.stdout))
    }
}

// ------------------------------------------------------------------ demarrage automatique

fn autostart_file() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    Some(crate::autostart::autostart_path(&home, std::env::var("XDG_CONFIG_HOME").ok().as_deref()))
}

pub fn autostart_enabled() -> bool {
    autostart_file().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| crate::autostart::is_enabled(&t)).unwrap_or(false)
}

pub fn set_autostart(enable: bool, exe: &Path) -> bool {
    let Some(p) = autostart_file() else { return false };
    if enable {
        let text = crate::autostart::desktop_entry(exe, "AutoGameSoftwareSync", "Synchronisation des sauvegardes et reglages entre vos appareils");
        if let Some(d) = p.parent() {
            if std::fs::create_dir_all(d).is_err() {
                return false;
            }
        }
        crate::util::write_atomic(&p, text.as_bytes()).is_ok()
    } else {
        match std::fs::remove_file(&p) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }
}

/// Aucune entree de demarrage historique sous Linux.
pub fn run_entry(_name: &str) -> Option<String> {
    None
}

pub fn remove_run_entry(_name: &str) -> bool {
    false
}

pub fn migrate_run_entries(_new_exe: &Path) -> Vec<String> {
    Vec::new()
}

// ------------------------------------------------------------------ ouvrir / lancer

fn spawn_detached(program: &str, args: &[&str]) -> bool {
    match std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        Ok(mut child) => {
            // Recolte le processus fils pour ne pas laisser de zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Ouvre une URL avec le navigateur par defaut (`xdg-open`).
pub fn open_url(url: &str) {
    if url.starts_with("http://") || url.starts_with("https://") {
        spawn_detached("xdg-open", &[url]);
    }
}

/// Ouvre un dossier dans le gestionnaire de fichiers (`xdg-open`).
pub fn open_folder(dir: &Path) {
    spawn_detached("xdg-open", &[&dir.to_string_lossy()]);
}

/// Decoupe une chaine d'arguments en respectant les guillemets simples et doubles.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Lance un programme natif avec ses arguments.
pub fn open_exe(path: &Path, args: &str) -> bool {
    let a = split_args(args);
    let refs: Vec<&str> = a.iter().map(String::as_str).collect();
    let mut c = std::process::Command::new(path);
    c.args(&refs).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Some(d) = path.parent().filter(|d| d.is_dir()) {
        c.current_dir(d);
    }
    match c.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Ligne de commande : programme entre guillemets puis arguments.
pub fn build_cmdline(path: &Path, args: &str) -> String {
    if args.trim().is_empty() {
        format!("\"{}\"", path.display())
    } else {
        format!("\"{}\" {}", path.display(), args)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchHow {
    Minimized,
    Normal,
    Failed,
}

/// Lancement ordinaire : la demande « reduit » n'existe pas sous Linux.
pub fn launch_program(path: &Path, args: &str, _minimized: bool) -> LaunchHow {
    if open_exe(path, args) {
        LaunchHow::Normal
    } else {
        LaunchHow::Failed
    }
}

// ------------------------------------------------------------------ droits eleves, icones d'exe (sans objet)

#[derive(Debug, PartialEq, Eq)]
pub enum ElevateResult {
    Finished(u32),
    Cancelled,
    Failed(u32),
}

/// Pas d'elevation sous Linux : les regles de demarrage (planificateur Windows) ne sont pas disponibles.
pub fn run_elevated_and_wait(_args: &str, _timeout_ms: u32) -> ElevateResult {
    ElevateResult::Failed(1)
}

// ------------------------------------------------------------------ icones et noms des programmes (raccourcis .desktop)

struct AppIndex {
    at: std::time::Instant,
    data: Vec<std::path::PathBuf>,
    entries: Vec<crate::linuxapps::DesktopEntry>,
}

static APP_INDEX: std::sync::Mutex<Option<AppIndex>> = std::sync::Mutex::new(None);

/// Raccourcis d'application de la machine (relus au plus toutes les 60 s).
fn with_index<R>(f: impl FnOnce(&[crate::linuxapps::DesktopEntry], &[std::path::PathBuf]) -> R) -> R {
    let mut g = APP_INDEX.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().map(|i| i.at.elapsed() > std::time::Duration::from_secs(60)).unwrap_or(true) {
        let home = dirs::home_dir().unwrap_or_default();
        let data = crate::linuxapps::data_dirs(&home, std::env::var("XDG_DATA_HOME").ok().as_deref(), std::env::var("XDG_DATA_DIRS").ok().as_deref());
        let entries = crate::linuxapps::load_entries(&data);
        *g = Some(AppIndex { at: std::time::Instant::now(), data, entries });
    }
    let i = g.as_ref().expect("index");
    f(&i.entries, &i.data)
}

/// Raccourci `.desktop` d'un programme (le fichier lui-meme s'il s'agit d'un raccourci).
fn entry_for(exe: &Path) -> (Option<crate::linuxapps::DesktopEntry>, Option<u64>) {
    if exe.extension().map(|e| e == "desktop").unwrap_or(false) {
        let e = std::fs::read_to_string(exe).ok().and_then(|t| crate::linuxapps::parse_desktop(&t));
        return (e, None);
    }
    let appid = crate::linuxapps::steam_appid_of_path(exe);
    (with_index(|entries, _| crate::linuxapps::entry_for_program(entries, exe, appid).cloned()), appid)
}

/// PNG -> RGBA 8 bits (reduit par moities au-dela de 128 px : l'icone s'affiche en 40 px).
pub fn decode_png(bytes: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let px = w.checked_mul(h)?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3].chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2].chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(shrink(rgba, w, h))
}

/// JPEG -> RGBA 8 bits (vignettes de la bibliotheque Steam).
pub fn decode_jpeg(bytes: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(zune_core::bytestream::ZCursor::new(bytes), opts);
    let px = d.decode().ok()?;
    let info = d.info()?;
    let (w, h) = (info.width as usize, info.height as usize);
    if px.len() != w * h * 4 {
        return None;
    }
    Some(shrink(px, w, h))
}

fn shrink(mut rgba: Vec<u8>, mut w: usize, mut h: usize) -> (Vec<u8>, usize, usize) {
    while w >= 256 && h >= 256 && w % 2 == 0 && h % 2 == 0 {
        let (nw, nh) = (w / 2, h / 2);
        let mut out = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let s = |dx: usize, dy: usize| rgba[((2 * y + dy) * w + 2 * x + dx) * 4 + c] as u32;
                    out[(y * nw + x) * 4 + c] = ((s(0, 0) + s(1, 0) + s(0, 1) + s(1, 1)) / 4) as u8;
                }
            }
        }
        rgba = out;
        w = nw;
        h = nh;
    }
    (rgba, w, h)
}

/// Icone d'un jeu Steam dans le cache de la bibliotheque (`appcache/librarycache`).
fn steam_library_icon(appid: u64) -> Option<(Vec<u8>, usize, usize)> {
    let root = crate::detect::steam_root()?;
    let lc = root.join("appcache").join("librarycache");
    let old = lc.join(format!("{}_icon.jpg", appid));
    if let Some(x) = std::fs::read(&old).ok().and_then(|b| decode_jpeg(&b)) {
        return Some(x);
    }
    // Disposition recente : <appid>/<empreinte de 40 caracteres>.jpg
    for e in std::fs::read_dir(lc.join(appid.to_string())).ok()?.filter_map(|e| e.ok()) {
        let n = e.file_name().to_string_lossy().to_string();
        if let Some(stem) = n.strip_suffix(".jpg") {
            if stem.len() == 40 && stem.chars().all(|c| c.is_ascii_hexdigit()) {
                if let Some(x) = std::fs::read(e.path()).ok().and_then(|b| decode_jpeg(&b)) {
                    return Some(x);
                }
            }
        }
    }
    None
}

/// Icone d'un programme : celle de son raccourci `.desktop` (theme du bureau), sinon celle de la bibliotheque Steam,
/// sinon une icone du theme portant le nom du programme. None : icone generique dessinee par l'interface.
pub fn exe_icon_rgba(exe: &Path) -> Option<(Vec<u8>, usize, usize)> {
    let (entry, appid) = entry_for(exe);
    let stem = exe.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut names: Vec<String> = entry.as_ref().and_then(|e| e.icon.clone()).into_iter().collect();
    if let Some(id) = appid {
        names.push(format!("steam_icon_{}", id));
    }
    if let Some(id) = crate::linuxapps::flatpak_id_from_path(exe) {
        names.push(id);
    }
    if !stem.is_empty() {
        names.push(stem);
    }
    let file = with_index(|_, data| names.iter().find_map(|n| crate::linuxapps::find_icon(n, data)));
    if let Some(x) = file.and_then(|f| std::fs::read(f).ok()).and_then(|b| decode_png(&b)) {
        return Some(x);
    }
    appid.and_then(steam_library_icon)
}

/// (nom du produit, description) : nom du raccourci `.desktop` du programme s'il en a un.
pub fn exe_version_strings(exe: &Path) -> (Option<String>, Option<String>) {
    let (entry, _) = entry_for(exe);
    (entry.map(|e| e.name).filter(|n| !n.trim().is_empty()), None)
}

/// (editeur, produit) : aucun editeur sous Linux ; produit = nom du raccourci `.desktop` du programme.
pub fn exe_company_product(exe: &Path) -> (Option<String>, Option<String>) {
    (None, exe_version_strings(exe).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_parsing() {
        assert_eq!(parse_color_scheme("'prefer-dark'\n"), Some(false));
        assert_eq!(parse_color_scheme("'default'"), Some(true));
        assert_eq!(parse_color_scheme("n'importe quoi"), None);
    }

    #[test]
    fn args_split_respects_quotes() {
        assert_eq!(split_args(r#"--a "b c" 'd e' f"#), vec!["--a", "b c", "d e", "f"]);
        assert_eq!(split_args("   "), Vec::<String>::new());
        assert_eq!(split_args(r#""" x"#), vec!["", "x"]);
    }
}

/// Police chinoise choisie par fontconfig (`fc-match`), dernier recours de la recherche de polices.
pub fn fontconfig_cjk_font() -> Option<std::path::PathBuf> {
    let o = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans:lang=zh-cn"]).stderr(std::process::Stdio::null()).output().ok()?;
    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if p.is_empty() { None } else { Some(std::path::PathBuf::from(p)) }
}
