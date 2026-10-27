//! Logique testable de l'interface (sans egui) : navigation manette, pictogrammes, mode grand
//! format automatique, delai des infobulles. Le rendu est dans `gui`.
use crate::gamepad::{Nav, PadStatus};
use crate::settings::BigChoice;
use crate::vm::Page;

/// Delai (ms) avant l'apparition d'une infobulle : survol souris OU element selectionne au
/// clavier / a la manette. Constante unique, a modifier ici.
pub const TOOLTIP_DELAY_MS: u64 = 1000;

/// L'infobulle d'un element selectionne depuis `held_ms` doit-elle etre affichee ?
pub fn tooltip_due(held_ms: u64) -> bool {
    held_ms >= TOOLTIP_DELAY_MS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavAction {
    /// Touche envoyee a egui : le curseur de focus reste DANS l'onglet courant (la barre d'onglets
    /// n'est pas focalisable).
    Key(NavKey),
    /// Changement d'onglet (LB/RB, L1/R1), seulement par ce biais.
    Page(i32),
    Ignore,
}

/// Action d'interface pour un evenement manette. Pendant un dialogue de confirmation, le
/// changement d'onglet est ignore (il agirait derriere le dialogue).
pub fn nav_action(n: Nav, modal_open: bool) -> NavAction {
    match n {
        Nav::Up => NavAction::Key(NavKey::Up),
        Nav::Down => NavAction::Key(NavKey::Down),
        Nav::Left => NavAction::Key(NavKey::Left),
        Nav::Right => NavAction::Key(NavKey::Right),
        Nav::Confirm => NavAction::Key(NavKey::Enter),
        Nav::Back => NavAction::Key(NavKey::Escape),
        Nav::PagePrev if !modal_open => NavAction::Page(-1),
        Nav::PageNext if !modal_open => NavAction::Page(1),
        Nav::PagePrev | Nav::PageNext => NavAction::Ignore,
    }
}

/// Onglet atteint apres `delta` pas (circulaire).
pub fn page_after(mut p: Page, delta: i32) -> Page {
    for _ in 0..delta.unsigned_abs() {
        p = if delta > 0 { p.next() } else { p.prev() };
    }
    p
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyphs {
    /// LB / RB (Xbox, XInput, manette generique)
    Xbox,
    /// L1 / R1 (DualSense, DualShock)
    PlayStation,
}

/// Famille de pictogrammes selon les manettes UTILISABLES detectees (Sony prioritaire quand une
/// DualSense est lue). None = aucune manette utilisable : pas de pictogrammes.
pub fn glyph_set(pads: &[PadStatus]) -> Option<Glyphs> {
    let usable: Vec<&PadStatus> = pads.iter().filter(|p| p.readable).collect();
    if usable.is_empty() {
        None
    } else if usable.iter().any(|p| p.info.name.starts_with("Dual")) {
        Some(Glyphs::PlayStation)
    } else {
        Some(Glyphs::Xbox)
    }
}

pub fn shoulder_labels(g: Glyphs) -> (&'static str, &'static str) {
    match g {
        Glyphs::Xbox => ("LB", "RB"),
        Glyphs::PlayStation => ("L1", "R1"),
    }
}

/// Une manette utilisable est-elle presente ?
pub fn pad_present(pads: &[PadStatus]) -> bool {
    pads.iter().any(|p| p.readable)
}

/// Linux : session de console portable ou du « mode jeu » de SteamOS (gamescope), ou appareil portable connu (Steam Deck,
/// Legion Go, ROG Ally...). `env` lit une variable d'environnement ; `vendor` / `product` : `/sys/class/dmi/id`.
/// Sert au mode grand format « Auto » : sous Linux la manette passe par Steam Input, aucune manette n'est donc « vue ».
pub fn handheld_from(env: &dyn Fn(&str) -> Option<String>, vendor: &str, product: &str) -> bool {
    let on = |k: &str| env(k).map(|v| !v.is_empty() && v != "0").unwrap_or(false);
    if on("SteamDeck") || on("SteamGamepadUI") || on("SteamTenfoot") || on("GAMESCOPE_WAYLAND_DISPLAY") {
        return true;
    }
    if env("XDG_CURRENT_DESKTOP").map(|d| d.to_lowercase().contains("gamescope")).unwrap_or(false) {
        return true;
    }
    let (v, p) = (vendor.trim().to_lowercase(), product.trim().to_lowercase());
    v.contains("valve") || p == "jupiter" || p == "galileo" || p.starts_with("83e1") || p.starts_with("83l3") || p.starts_with("83n6") || p.starts_with("83q2") || p.starts_with("83q3") || p.starts_with("rc71l") || p.starts_with("rc72l")
}

/// Linux : raccourcis clavier de changement d'onglet (Ctrl+Tab, Ctrl+Maj+Tab, Ctrl+Page suivante / precedente) : -1, +1
/// ou 0. Steam Input peut y associer les boutons d'epaule de la manette.
/// `key` : nom de la touche (`"Tab"`, `"PageDown"`, `"PageUp"`).
pub fn tab_shortcut(key: &str, ctrl: bool, shift: bool) -> i32 {
    if !ctrl {
        return 0;
    }
    match key {
        "Tab" if shift => -1,
        "Tab" | "PageDown" => 1,
        "PageUp" => -1,
        _ => 0,
    }
}

/// Mode grand format effectif : "Auto" suit la presence d'une manette (et revient au choix
/// precedent a la deconnexion), "Toujours" / "Jamais" l'imposent.
pub fn effective_big(choice: BigChoice, pad_present: bool) -> bool {
    match choice {
        BigChoice::Auto => pad_present,
        BigChoice::Always => true,
        BigChoice::Never => false,
    }
}

/// Positions horizontales (relatives au bord gauche de la barre) des elements de la barre d'onglets.
#[derive(Clone, Debug, PartialEq)]
pub struct TabLayout {
    pub left_glyph_x: f32,
    pub tab_x: Vec<f32>,
    pub right_glyph_x: f32,
}

/// Disposition de la barre d'onglets. `glyph_w` : largeur des pictogrammes d'epaule (None = aucun).
/// Hors mode manette (`centered == false`) : pictogramme, puis onglets a gauche, pictogramme droit au
/// bord droit. En mode manette (`centered == true`) : onglets GROUPES AU CENTRE de la barre, pictogrammes
/// aux deux extremites (jamais recouverts par les onglets si la barre est trop etroite).
pub fn tab_bar_layout(avail_w: f32, glyph_w: Option<f32>, tab_ws: &[f32], gap: f32, centered: bool) -> TabLayout {
    let gw = glyph_w.unwrap_or(0.0);
    let total: f32 = tab_ws.iter().sum::<f32>() + gap * tab_ws.len().saturating_sub(1) as f32;
    let left_min = if glyph_w.is_some() { gw + gap } else { 0.0 };
    let start = if centered { ((avail_w - total) / 2.0).max(left_min) } else { left_min };
    let mut x = start;
    let mut tab_x = Vec::with_capacity(tab_ws.len());
    for w in tab_ws {
        tab_x.push(x);
        x += w + gap;
    }
    TabLayout { left_glyph_x: 0.0, tab_x, right_glyph_x: (avail_w - gw).max(0.0) }
}

// ------------------------------------------------------------------ demarrage masque (--tray)

/// Etape du masquage au demarrage. eframe 0.31 affiche la fenetre de force apres la 1re image :
/// on la prepare (hors ecran) pendant l'image 0, puis on la masque reellement aux images suivantes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HideStep {
    None,
    Prepare,
    Finish,
}

/// Dernier numero d'image ou l'on re-masque (l'operation est idempotente).
pub const HIDE_LAST_FRAME: u32 = 4;

/// Que faire a l'image `frame` (0 = premiere) quand `start_hidden` ?
pub fn hidden_start_step(start_hidden: bool, frame: u32) -> HideStep {
    match (start_hidden, frame) {
        (false, _) => HideStep::None,
        (true, 0) => HideStep::Prepare,
        (true, f) if f <= HIDE_LAST_FRAME => HideStep::Finish,
        _ => HideStep::None,
    }
}

// ------------------------------------------------------------------ barre du bas : etat de la manette

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadKind {
    DualSense,
    /// Manette Xbox / XInput (dont celle de la Legion Go en mode XInput).
    Xbox,
    Other,
    None,
}

/// Famille de la manette a afficher : seules les manettes lisibles comptent ; DualSense > Xbox > autre.
pub fn pad_kind(pads: &[PadStatus]) -> PadKind {
    let usable: Vec<&PadStatus> = pads.iter().filter(|p| p.readable).collect();
    if usable.iter().any(|p| p.info.name.starts_with("DualSense")) {
        PadKind::DualSense
    } else if usable.iter().any(|p| p.info.name.contains("XInput") || p.info.name.contains("Xbox")) {
        PadKind::Xbox
    } else if !usable.is_empty() {
        PadKind::Other
    } else {
        PadKind::None
    }
}

// ------------------------------------------------------------------ mise en page adaptative

/// Sous cette largeur (px) la mise en page passe en mode « etroit » : lignes sur deux niveaux, champs et boutons empiles.
pub const NARROW_W: f32 = 600.0;
/// Sous cette largeur : barre du bas reduite a des icones.
pub const TINY_W: f32 = 460.0;

pub fn is_narrow(width: f32) -> bool {
    width < NARROW_W
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabMode {
    Full,
    Short,
    Icons,
}

/// Libelles complets si ils tiennent, sinon courts, sinon icones. Largeurs = largeur de chaque onglet (marges comprises).
pub fn tab_mode(avail: f32, full: &[f32], short: &[f32], gap: f32) -> TabMode {
    let sum = |v: &[f32]| v.iter().sum::<f32>() + gap * v.len().saturating_sub(1) as f32;
    if sum(full) <= avail {
        TabMode::Full
    } else if sum(short) <= avail {
        TabMode::Short
    } else {
        TabMode::Icons
    }
}

// ------------------------------------------------------------------ mode manette : fenetre maximisee

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaxAction {
    None,
    Maximize,
    Restore,
}

/// Memoire du mode manette : `prev_want` = derniere decision appliquee ; `we_maximized` = c'est NOUS qui avons maximise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaxState {
    pub prev_want: bool,
    pub we_maximized: bool,
}

/// Faut-il maximiser la fenetre ? Auto = manette utilisable ; Toujours ; Jamais.
pub fn want_maximized(choice: BigChoice, pad_present: bool) -> bool {
    effective_big(choice, pad_present)
}

/// Decision a chaque image. La fenetre n'est maximisee qu'a la TRANSITION vers le mode manette (le
/// redimensionnement de l'utilisateur est ensuite respecte) ; a la sortie on ne restaure que ce que nous
/// avons maximise et seulement si la fenetre l'est toujours (l'utilisateur avait deja maximise : on ne touche a rien).
/// Fenetre invisible (reduite dans la zone de notification) : rien n'est fait, la decision est reprise a l'ouverture.
pub fn max_step(st: &mut MaxState, want: bool, is_maximized: bool, visible: bool) -> MaxAction {
    if !visible {
        return MaxAction::None;
    }
    if want && !st.prev_want {
        st.prev_want = true;
        if is_maximized {
            st.we_maximized = false;
            return MaxAction::None;
        }
        st.we_maximized = true;
        return MaxAction::Maximize;
    }
    if !want && st.prev_want {
        st.prev_want = false;
        let had = std::mem::take(&mut st.we_maximized);
        if had && is_maximized {
            return MaxAction::Restore;
        }
    }
    MaxAction::None
}

// ------------------------------------------------------------------ polices (chinois)

/// Polices systeme candidates pour le chinois simplifie, par ordre de preference. Rien n'est embarque dans l'exe.
pub fn cjk_font_candidates(windir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let fonts = windir.join("Fonts");
    ["msyh.ttc", "msyhbd.ttc", "msyhl.ttc", "simsun.ttc", "NotoSansSC-Regular.otf", "NotoSansSC-Regular.ttf", "NotoSansCJKsc-Regular.otf", "Deng.ttf", "simhei.ttf"].iter().map(|f| fonts.join(f)).collect()
}

/// Premiere police candidate existante (par le test `exists`).
pub fn pick_font(cands: &[std::path::PathBuf], exists: &dyn Fn(&std::path::Path) -> bool) -> Option<std::path::PathBuf> {
    cands.iter().find(|p| exists(p)).cloned()
}

/// La police CJK doit-elle etre chargee maintenant ? Langue chinoise, ou un selecteur de langue (qui affiche « 中文 ») est visible.
pub fn needs_cjk_font(zh_selected: bool, on_settings_page: bool, first_run_language_screen: bool) -> bool {
    zh_selected || on_settings_page || first_run_language_screen
}

// ------------------------------------------------------------------ bouton « Actualiser » (animation)

pub const REFRESH_MIN_MS: u64 = 600;
pub const REFRESH_DONE_MS: u64 = 1500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshPhase {
    Idle,
    /// Icone qui tourne : au moins 600 ms, et tant que le rafraichissement est en cours.
    Spinning,
    /// Confirmation breve « A jour ».
    Done,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RefreshAnim {
    start_ms: Option<u64>,
    done_ms: Option<u64>,
}

impl RefreshAnim {
    pub fn click(&mut self, now_ms: u64) {
        self.start_ms = Some(now_ms);
        self.done_ms = None;
    }

    /// Phase a l'instant `now_ms` ; `inflight` : un rafraichissement est en cours.
    pub fn phase(&mut self, now_ms: u64, inflight: bool) -> RefreshPhase {
        let Some(s) = self.start_ms else { return RefreshPhase::Idle };
        if now_ms.saturating_sub(s) < REFRESH_MIN_MS || inflight {
            return RefreshPhase::Spinning;
        }
        let d = *self.done_ms.get_or_insert(now_ms);
        if now_ms.saturating_sub(d) < REFRESH_DONE_MS {
            RefreshPhase::Done
        } else {
            self.start_ms = None;
            self.done_ms = None;
            RefreshPhase::Idle
        }
    }
}

// ------------------------------------------------------------------ polices CJK sous Linux

/// Noms de fichiers de polices chinoises courantes sous Linux (Noto Sans CJK, WenQuanYi, Source Han).
const LINUX_CJK_FILES: [&str; 9] = [
    "NotoSansCJK-Regular.ttc",
    "NotoSansCJKsc-Regular.otf",
    "NotoSansSC-Regular.otf",
    "NotoSansSC-Regular.ttf",
    "SourceHanSansSC-Regular.otf",
    "SourceHanSansCN-Regular.otf",
    "wqy-microhei.ttc",
    "wqy-zenhei.ttc",
    "DroidSansFallbackFull.ttf",
];

/// Polices candidates sous `roots` (dossiers de polices) : emplacements usuels puis parcours limite en profondeur.
/// `list_dirs(dir)` renvoie les sous-dossiers de `dir` (injecte pour les tests).
pub fn linux_cjk_font_candidates(roots: &[std::path::PathBuf], list_dirs: &dyn Fn(&std::path::Path) -> Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for r in roots {
        dirs.push(r.clone());
        for d1 in list_dirs(r) {
            dirs.push(d1.clone());
            dirs.extend(list_dirs(&d1));
        }
    }
    let mut out = Vec::new();
    for f in LINUX_CJK_FILES {
        for d in &dirs {
            out.push(d.join(f));
        }
    }
    out
}

#[cfg(windows)]
pub fn system_cjk_candidates() -> Vec<std::path::PathBuf> {
    let windir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(r"C:/Windows"));
    cjk_font_candidates(&windir)
}

#[cfg(not(windows))]
pub fn system_cjk_candidates() -> Vec<std::path::PathBuf> {
    let mut roots = vec![std::path::PathBuf::from("/usr/share/fonts"), std::path::PathBuf::from("/usr/local/share/fonts")];
    if let Some(h) = dirs::home_dir() {
        roots.push(h.join(".local").join("share").join("fonts"));
        roots.push(h.join(".fonts"));
    }
    let list = |d: &std::path::Path| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(d).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
    };
    let mut v = linux_cjk_font_candidates(&roots, &list);
    // Dernier recours : fontconfig.
    if let Some(p) = crate::winutil::fontconfig_cjk_font() {
        v.push(p);
    }
    v
}

#[cfg(test)]
mod linux_font_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn noto_cjk_is_found_in_usual_places() {
        let list = |d: &Path| -> Vec<PathBuf> {
            match d.to_string_lossy().replace('\\', "/").as_str() {
                "/usr/share/fonts" => vec![PathBuf::from("/usr/share/fonts/opentype")],
                "/usr/share/fonts/opentype" => vec![PathBuf::from("/usr/share/fonts/opentype/noto")],
                _ => vec![],
            }
        };
        let c = linux_cjk_font_candidates(&[PathBuf::from("/usr/share/fonts")], &list);
        let wanted = PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc");
        let exists = |p: &Path| p == wanted;
        assert_eq!(pick_font(&c, &exists), Some(wanted));
    }
}
