//! Application (etat, boucle d'images, entree manette / tactile, theme, polices, messages du fil de travail, toasts,
//! dialogues, garde-fous : autorisation retiree, horloge fausse). Les ecrans sont dans `screens.rs`. Aucune lecture
//! profonde du stockage ici : tout passe par le fil de travail (`worker.rs`).
use crate::i18n::{self, fill, tr, Lang, T};
use crate::model::{self, ItemInfo, ItemState, Store, TtKey, TtRow};
use crate::worker::{Done, Env, Job, Worker};
use crate::{screens, sys};
use agss::backup::UndoTicket;
use agss::config::Game;
use agss::emulators::Found;
use agss::padfocus::{Dir, ItemKind, PadAction, PadFocus};
use agss::{theme, util};
use eframe::egui::{self, Color32, Stroke};
use std::path::PathBuf;
use std::sync::Arc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Heure de construction (ms UTC, posee par build.rs) : une horloge en dessous est forcement fausse.
pub const BUILD_MS: &str = env!("AGSS_BUILD_MS");
pub const GITHUB_URL: &str = "https://github.com/Epikou/AutoGameSoftwareSync/releases";
/// Apres cette duree en arriere-plan, le retour au premier plan relance une synchro (copie vers le coffre uniquement).
pub const RESUME_SYNC_MS: u64 = 20_000;
pub const TOAST_MS: u64 = 8_000;
pub const UNDO_MS: u64 = 60_000;
/// Hauteur d'une ligne de carte (tuile, version) : fixe, independante de `interact_size`.
pub const ROW_H: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Setup,
    Home,
    Item,
    Settings,
    Device,
    Emulators,
    About,
    Journal,
    Browse,
}

impl Screen {
    pub fn key(self) -> u8 {
        match self {
            Screen::Setup => 10,
            Screen::Home => 1,
            Screen::Item => 2,
            Screen::Settings => 3,
            Screen::Device => 4,
            Screen::Emulators => 5,
            Screen::About => 6,
            Screen::Journal => 7,
            Screen::Browse => 8,
        }
    }
}

/// Pourquoi le navigateur de dossiers est ouvert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseFor {
    Vault,
    PersonalFolder,
}

pub struct SetupState {
    pub step: usize,
    /// Mode « Ajouter » depuis les reglages : seulement l'etape des elements, retour aux reglages.
    pub add_mode: bool,
    pub name: String,
    pub color: [u8; 3],
    /// Clavier a l'ecran (manette) ouvert sous le champ du nom.
    pub kb_open: bool,
    pub candidates: Vec<model::Candidate>,
    pub chosen_vault: Option<PathBuf>,
    pub vault_error: Option<String>,
    pub proposals: Vec<model::Proposal>,
    pub ticks: Vec<Vec<bool>>,
    pub perm_ok: bool,
    pub perm_checked_ms: u64,
    pub error: Option<String>,
}

pub struct BrowseState {
    pub purpose: BrowseFor,
    /// None : liste des stockages.
    pub dir: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
    /// Sous-dossiers du dossier courant, lus une fois par changement de dossier (jamais a chaque image).
    pub entries: Vec<PathBuf>,
    pub entries_for: Option<PathBuf>,
}

impl BrowseState {
    pub fn new(purpose: BrowseFor) -> BrowseState {
        BrowseState { purpose, dir: None, roots: sys::storage_roots(), entries: Vec::new(), entries_for: None }
    }
}

pub struct ItemView {
    pub name: String,
    pub rows: Vec<TtRow>,
    /// L'historique est arrive du fil de travail (sinon « Un instant… »).
    pub loaded: bool,
}

pub struct Toast {
    pub text: String,
    pub undo: Option<(String, UndoTicket)>,
    pub until_ms: u64,
}

#[derive(Clone, Debug)]
pub enum ModalAct {
    Restore { name: String, key: TtKey },
    Unprotect(String),
    Resolve { name: String, keep_mine: bool },
    /// Remise confirmee de la version plus recente du coffre (jamais automatique sur la console).
    TakeVersion(String),
}

pub struct Modal {
    pub title: String,
    pub body: String,
    pub yes: String,
    pub act: ModalAct,
}

pub struct App {
    pub store: Store,
    pub worker: Worker,
    pub lang: Lang,
    pub screen: Screen,
    pub back_to: Screen,
    pub focus: PadFocus,
    pub palette: theme::Palette,
    theme_key: Option<[u8; 3]>,
    pub items: Vec<ItemInfo>,
    pub toast: Option<Toast>,
    pub modal: Option<Modal>,
    pub setup: SetupState,
    pub browse: Option<BrowseState>,
    pub item: Option<ItemView>,
    pub journal: Vec<String>,
    /// Emulateurs trouves (ecran Emulateurs), calcules a l'entree de l'ecran par le fil de travail, et racines lues alors.
    pub emu_found: Option<Vec<Found>>,
    pub emu_roots: Vec<PathBuf>,
    /// Une lecture du stockage est en cours dans le fil de travail (« Un instant… »).
    pub waiting: bool,
    /// Autorisation « Acces a tous les fichiers » retiree apres la configuration : aucune synchro ni remise.
    pub perm_missing: bool,
    perm_checked_ms: u64,
    /// Horloge de la console anterieure a la construction de l'appli : avertissement.
    pub clock_wrong: bool,
    last_frame_ms: u64,
    was_focused: bool,
    unfocused_since_ms: u64,
    started: bool,
    cleaned: (usize, Option<PathBuf>),
    cjk_tried: bool,
    fonts_set: bool,
    /// Retour (B / Echap) recu dans cette image : a traiter par l'ecran courant.
    pending_back: bool,
    /// Raccourcis manette recus (X = F1, Start = F3, L1 / R1 = pages).
    shortcuts: Vec<egui::Key>,
    scroll_delta: f32,
    /// Un champ de texte avait le focus a la derniere image (B en sort au lieu de quitter l'ecran ; fleches au champ).
    pub text_focus: std::cell::Cell<bool>,
}

impl App {
    /// `cleaned` : resultat du nettoyage au demarrage (dossiers de travail, version incomplete retiree).
    pub fn new(data_dir: PathBuf, ectx: &egui::Context, cleaned: (usize, Option<PathBuf>)) -> App {
        let store = Store::load(data_dir);
        let lang = if store.prefs.lang_chosen { store.settings.lang } else { i18n::from_system(&sys::system_language()) };
        let wake_ctx = ectx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || wake_ctx.request_repaint());
        let worker = Worker::start(wake);
        let device_default = if store.prefs.device_name.is_empty() { model::default_device_name(&sys::device_model()) } else { store.prefs.device_name.clone() };
        let color = store.settings.machine_colors.get(&device_default).copied().unwrap_or_else(|| agss::machinecolor::default_color(&device_default));
        let setup = SetupState { step: 0, add_mode: false, name: device_default, color, kb_open: true, candidates: Vec::new(), chosen_vault: store.prefs.vault.clone(), vault_error: None, proposals: Vec::new(), ticks: Vec::new(), perm_ok: false, perm_checked_ms: 0, error: None };
        let screen = if store.prefs.setup_done { Screen::Home } else { Screen::Setup };
        let palette = theme::palette(true, store.settings.accent);
        let clock_wrong = BUILD_MS.parse::<u64>().map(|b| util::now_ms() < b).unwrap_or(false);
        App { store, worker, lang, screen, back_to: Screen::Home, focus: PadFocus::default(), palette, theme_key: None, items: Vec::new(), toast: None, modal: None, setup, browse: None, item: None, journal: Vec::new(), emu_found: None, emu_roots: Vec::new(), waiting: false, perm_missing: false, perm_checked_ms: 0, clock_wrong, last_frame_ms: 0, was_focused: true, unfocused_since_ms: 0, started: false, cleaned, cjk_tried: false, fonts_set: false, pending_back: false, shortcuts: Vec::new(), scroll_delta: 0.0, text_focus: std::cell::Cell::new(false) }
    }

    // ------------------------------------------------------------------ outils

    pub fn t(&self, k: T) -> &'static str {
        tr(self.lang, k)
    }

    pub fn col(c: theme::Rgb) -> Color32 {
        Color32::from_rgb(c[0], c[1], c[2])
    }

    /// Environnement d'une synchro AUTOMATIQUE : jamais de remise ni de resolution de conflit toute seule.
    pub fn env(&self) -> Env {
        Env { data_dir: self.store.data_dir.clone(), device: self.store.prefs.device_name.clone(), vault: self.store.prefs.vault.clone(), auto_cloud: self.store.settings.conflict_mode == agss::settings::ConflictMode::Cloud, auto_restore: false }
    }

    /// Environnement d'une remise CONFIRMEE par l'utilisateur.
    pub fn env_restore(&self) -> Env {
        Env { auto_restore: true, ..self.env() }
    }

    pub fn vault_reachable(&self) -> bool {
        self.store.prefs.vault.as_ref().map(|v| v.is_dir()).unwrap_or(false)
    }

    pub fn go(&mut self, s: Screen) {
        if self.screen != s {
            self.focus.reset();
        }
        self.screen = s;
    }

    pub fn toast(&mut self, text: String) {
        self.toast = Some(Toast { text, undo: None, until_ms: util::now_ms() + TOAST_MS });
    }

    /// Synchro automatique de tout (copie vers le coffre ; une version plus recente ailleurs est seulement signalee).
    pub fn sync_all(&mut self) {
        if self.worker.busy() || self.store.config.games.is_empty() || self.perm_missing {
            return;
        }
        self.worker.send(Job::SyncAll { env: self.env(), games: self.store.config.games.clone() });
    }

    pub fn refresh(&mut self) {
        self.worker.send(Job::Refresh { env: self.env(), games: self.store.config.games.clone() });
    }

    pub fn sync_one(&mut self, g: &Game) {
        if self.worker.busy() || self.perm_missing {
            return;
        }
        self.worker.send(Job::SyncOne { env: self.env(), game: g.clone() });
    }

    /// Lance une lecture du stockage dans le fil de travail (ecran « Un instant… » en attendant).
    pub fn scan(&mut self, job: Job) {
        self.waiting = true;
        self.worker.send(job);
    }

    pub fn device_color(&self, name: &str) -> theme::Rgb {
        agss::machinecolor::color_of(name, &self.store.settings.machine_colors)
    }

    /// Etat affiche d'un element : acces manquant, sinon celui du fil (en cours), sinon le dernier connu.
    pub fn state_of(&self, info: &ItemInfo) -> ItemState {
        if self.perm_missing {
            return ItemState::NoAccess;
        }
        match self.worker.state_of(&info.name) {
            Some(ItemState::Syncing) => ItemState::Syncing,
            _ => info.state.clone(),
        }
    }

    pub fn state_text(&self, s: &ItemState, versions: usize) -> String {
        match s {
            ItemState::Never => self.t(T::StNever).into(),
            ItemState::Safe => self.t(T::StSafe).into(),
            ItemState::Copied => self.t(T::StCopied).into(),
            ItemState::Received(d) => fill(self.t(T::StReceived), "device", d),
            ItemState::Available(d) => fill(self.t(T::StAvailable), "device", d),
            ItemState::Conflict => self.t(T::StConflict).into(),
            ItemState::Error(e) => fill(self.t(T::Problem), "msg", &i18n::error_text(self.lang, e)),
            ItemState::Missing => self.t(T::StMissing).into(),
            ItemState::Empty if versions > 0 => self.t(T::StEmptyWas).into(),
            ItemState::Empty => self.t(T::StEmpty).into(),
            ItemState::NoAccess => self.t(T::StNoAccess).into(),
            ItemState::Syncing => self.t(T::Syncing).into(),
            ItemState::Pending => self.t(T::StPending).into(),
        }
    }

    pub fn state_color(&self, s: &ItemState) -> Color32 {
        match s {
            ItemState::Safe | ItemState::Copied | ItemState::Received(_) => Self::col(self.palette.accent),
            ItemState::Conflict | ItemState::Error(_) | ItemState::Missing | ItemState::NoAccess => Color32::from_rgb(230, 120, 60),
            ItemState::Syncing | ItemState::Available(_) => Color32::from_rgb(86, 180, 233),
            _ => Self::col(theme::mix(self.palette.text, self.palette.panel_bg, 0.35)),
        }
    }

    pub fn warn_color() -> Color32 {
        Color32::from_rgb(230, 120, 60)
    }

    // ------------------------------------------------------------------ widgets avec selection manette

    fn deco(&self, ui: &mut egui::Ui, r: &egui::Response) {
        if self.focus.is_selected(r.id) {
            ui.painter().rect_stroke(r.rect.expand(3.0), 10.0, Stroke::new(3.0_f32, Self::col(self.palette.ring)), egui::StrokeKind::Outside);
            if self.focus.take_scroll(r.id, true) {
                r.scroll_to_me(Some(egui::Align::Center));
            }
        }
    }

    /// Gros bouton (manette + tactile). `primary` : fond d'accent.
    pub fn btn(&self, ui: &mut egui::Ui, text: &str, primary: bool, w: f32) -> egui::Response {
        let rich = if primary { egui::RichText::new(text).color(Self::col(self.palette.on_accent)).strong() } else { egui::RichText::new(text) };
        let mut b = egui::Button::new(rich).corner_radius(10.0).min_size(egui::vec2(w, 48.0));
        if primary {
            b = b.fill(Self::col(self.palette.accent));
        }
        let r = ui.add(b);
        self.focus.register(&r, ItemKind::Button, true);
        self.deco(ui, &r);
        r
    }

    pub fn btn_enabled(&self, ui: &mut egui::Ui, text: &str, primary: bool, w: f32, enabled: bool) -> egui::Response {
        ui.add_enabled_ui(enabled, |ui| self.btn(ui, text, primary, w)).inner
    }

    /// Case a cocher grande.
    pub fn check(&self, ui: &mut egui::Ui, v: &mut bool, text: &str) -> egui::Response {
        let r = ui.allocate_ui_with_layout(egui::vec2(ui.available_width().min(560.0), 40.0), egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(egui::Checkbox::new(v, text))).inner;
        self.focus.register(&r, ItemKind::Button, true);
        self.deco(ui, &r);
        r
    }

    /// Champ de texte (A : editer, B : sortir ; en edition les fleches deplacent le curseur).
    pub fn text(&self, ui: &mut egui::Ui, v: &mut String, w: f32) -> egui::Response {
        let r = ui.add_sized([w, 44.0], egui::TextEdit::singleline(v).font(egui::TextStyle::Heading));
        if r.has_focus() {
            self.text_focus.set(true);
        }
        self.focus.register(&r, ItemKind::Text, true);
        self.deco(ui, &r);
        r
    }

    /// Ligne de hauteur FIXE dans une carte (une ligne `horizontal` ferait sinon 44 px, la hauteur des boutons).
    pub fn row(ui: &mut egui::Ui, h: f32, add: impl FnOnce(&mut egui::Ui)) {
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), h), egui::Layout::left_to_right(egui::Align::Center), add);
    }

    /// Carte cliquable (tuile, ligne de version) : rectangle de `size`, contenu dessine par `paint`.
    pub fn card(&self, ui: &mut egui::Ui, id_src: &str, size: egui::Vec2, selected_tint: bool, paint: impl FnOnce(&mut egui::Ui, egui::Rect)) -> egui::Response {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::click());
        let id = ui.make_persistent_id(id_src);
        let r = ui.interact(rect, id, egui::Sense::click());
        let bg = if r.hovered() || selected_tint { self.palette.hover_fill } else { self.palette.widget_bg };
        ui.painter().rect_filled(rect, 12.0, Self::col(bg));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(12.0)).layout(egui::Layout::top_down(egui::Align::Min)));
        child.spacing_mut().item_spacing.y = 6.0;
        paint(&mut child, rect);
        self.focus.register(&r, ItemKind::Button, true);
        self.deco(ui, &r);
        r
    }

    /// Petite pastille d'appareil (peinte : hauteur fixe, jamais plus haute que la ligne).
    pub fn mini_chip(&self, ui: &mut egui::Ui, name: &str) {
        let c = self.device_color(name);
        let galley = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(14.0), Self::col(agss::machinecolor::text_on(c)));
        let size = galley.size() + egui::vec2(12.0, 6.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().rect_filled(rect, 6.0, Self::col(c));
        ui.painter().galley(rect.min + egui::vec2(6.0, 3.0), galley, Self::col(agss::machinecolor::text_on(c)));
    }

    pub fn chip(&self, ui: &mut egui::Ui, name: &str) {
        let c = self.device_color(name);
        let txt = egui::RichText::new(name).color(Self::col(agss::machinecolor::text_on(c))).small();
        egui::Frame::new().fill(Self::col(c)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.label(txt);
        });
    }

    /// Titre d'ecran avec bouton Retour (B).
    pub fn header(&mut self, ui: &mut egui::Ui, title: &str, back: Option<Screen>) {
        ui.horizontal(|ui| {
            if let Some(b) = back {
                if self.btn(ui, &format!("{}  {}", egui_phosphor::regular::ARROW_LEFT, self.t(T::Back)), false, 120.0).clicked() {
                    self.go(b);
                }
                ui.add_space(8.0);
            }
            ui.heading(egui::RichText::new(title).size(26.0));
        });
        ui.add_space(6.0);
    }

    // ------------------------------------------------------------------ theme et polices

    fn apply_theme(&mut self, ectx: &egui::Context) {
        let accent = self.store.settings.accent;
        if self.theme_key == Some(accent) {
            return;
        }
        self.theme_key = Some(accent);
        let p = theme::palette(true, accent);
        self.palette = p;
        let c = Self::col;
        let mut v = egui::Visuals::dark();
        v.panel_fill = c(p.panel_bg);
        v.window_fill = c(p.panel_bg);
        v.selection.bg_fill = c(p.accent);
        v.selection.stroke.color = c(p.on_accent);
        v.hyperlink_color = c(p.ring);
        v.widgets.inactive.weak_bg_fill = c(p.widget_bg);
        v.widgets.inactive.bg_fill = c(p.widget_bg);
        v.widgets.inactive.fg_stroke.color = c(p.text);
        v.widgets.noninteractive.fg_stroke.color = c(p.text);
        for (w, fill) in [(&mut v.widgets.hovered, p.hover_fill), (&mut v.widgets.active, p.active_fill), (&mut v.widgets.open, p.hover_fill)] {
            w.weak_bg_fill = c(fill);
            w.bg_fill = c(fill);
            w.fg_stroke.color = c(p.text);
            w.bg_stroke = Stroke::new(1.0_f32, c(p.ring));
        }
        let mut style = egui::Style { visuals: v, ..Default::default() };
        for (_, f) in style.text_styles.iter_mut() {
            f.size *= 1.25;
        }
        style.spacing.interact_size.y = 44.0;
        style.spacing.button_padding = egui::vec2(16.0, 10.0);
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.interaction.tooltip_delay = 0.4;
        ectx.set_style(style);
    }

    fn ensure_fonts(&mut self, ectx: &egui::Context) {
        if !self.fonts_set {
            self.fonts_set = true;
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            ectx.set_fonts(fonts);
        }
        // Police chinoise : chargee des qu'un selecteur de langue est visible ou que le chinois est choisi.
        let lang_visible = self.screen == Screen::Settings || (self.screen == Screen::Setup && self.setup.step == 0);
        if (self.lang == Lang::Zh || lang_visible) && !self.cjk_tried {
            self.cjk_tried = true;
            // (chemin, index de la face dans le fichier : NotoSansCJK-Regular.ttc = JP, KR, SC, TC -> 2 = chinois simplifie)
            let cands: [(&str, u32); 4] = [("/system/fonts/NotoSansCJK-Regular.ttc", 2), ("/system/fonts/NotoSansSC-Regular.otf", 0), ("/system/fonts/NotoSansCJKsc-Regular.otf", 0), ("/system/fonts/DroidSansFallback.ttf", 0)];
            let found = cands.iter().find(|(p, _)| std::path::Path::new(p).is_file()).and_then(|(p, i)| std::fs::read(p).ok().map(|b| (b, *i)));
            match found {
                Some((bytes, index)) => {
                    let mut fonts = egui::FontDefinitions::default();
                    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
                    let mut fd = egui::FontData::from_owned(bytes);
                    fd.index = index;
                    fonts.font_data.insert("cjk".into(), Arc::new(fd));
                    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                        if let Some(v) = fonts.families.get_mut(&fam) {
                            v.push("cjk".into());
                        }
                    }
                    ectx.set_fonts(fonts);
                    agss::i18n::set_cjk_font_available(true);
                }
                None => agss::i18n::set_cjk_font_available(false),
            }
        }
    }

    // ------------------------------------------------------------------ garde-fous

    /// Autorisation « Acces a tous les fichiers » encore la ? (appele au demarrage, au retour au premier plan, et toutes
    /// les secondes tant qu'elle manque). Retourne vrai si elle vient de revenir.
    fn check_permission(&mut self, now: u64) -> bool {
        self.perm_checked_ms = now;
        let gate = model::sync_gate(self.store.prefs.setup_done, sys::storage_readable());
        let missing = gate == model::Gate::NoPermission;
        let came_back = self.perm_missing && !missing;
        if missing && !self.perm_missing {
            log::warn!("autorisation « Acces a tous les fichiers » retiree : aucune synchro");
        }
        self.perm_missing = missing;
        came_back
    }

    // ------------------------------------------------------------------ messages du fil

    fn handle_done(&mut self) {
        for d in self.worker.poll() {
            match d {
                Done::Synced { name, result } => {
                    if let model::SyncResult::Restored { from, .. } = &result {
                        let g = self.store.game(&name).map(|g| g.shown().to_string()).unwrap_or(name.clone());
                        self.toast(format!("{} : {}", i18n::item_display(self.lang, &g), fill(self.t(T::StReceived), "device", from)));
                    }
                    if let Some(it) = self.items.iter_mut().find(|i| i.name == name) {
                        it.state = result.state();
                    }
                    // Fiche ouverte sur cet element : son historique a pu changer (nouvelle version, remise).
                    if self.item.as_ref().map(|i| i.name == name).unwrap_or(false) {
                        self.reload_item(&name);
                    }
                }
                Done::Restored { name, ticket, files: _ } => {
                    self.toast = Some(Toast { text: self.t(T::Restored).into(), undo: Some((name.clone(), ticket)), until_ms: util::now_ms() + UNDO_MS });
                }
                Done::Undone { .. } => {
                    self.toast(self.t(T::Undone).into());
                }
                Done::Resolved { .. } => {
                    self.toast(self.t(T::Resolved).into());
                }
                Done::Error { name: _, msg } => {
                    self.toast(fill(self.t(T::Problem), "msg", &i18n::error_text(self.lang, &msg)));
                }
                Done::Facts(v) => {
                    for f in v {
                        match self.items.iter_mut().find(|i| i.name == f.name) {
                            Some(it) => *it = f,
                            None => self.items.push(f),
                        }
                    }
                    let names: Vec<String> = self.store.config.games.iter().map(|g| g.name.clone()).collect();
                    self.items.retain(|i| names.contains(&i.name));
                }
                Done::Vaults(c) => {
                    self.setup.candidates = c;
                    self.waiting = false;
                }
                Done::Proposals(p) => {
                    self.setup.ticks = p.iter().map(|p| if p.locations.is_empty() { vec![false] } else { p.locations.iter().map(|l| l.1 && !p.already).collect() }).collect();
                    self.setup.proposals = p;
                    self.waiting = false;
                }
                Done::VaultReady { from_setup, result } => {
                    self.waiting = false;
                    match result {
                        Ok(t) => {
                            self.store.prefs.vault = Some(t);
                            let _ = self.store.save_prefs();
                            let _ = self.store.save_config();
                            if from_setup {
                                self.setup.step = 4;
                                self.focus.reset();
                                self.scan(Job::Proposals { roots: sys::storage_roots(), existing: self.store.config.games.clone() });
                            } else {
                                self.refresh();
                            }
                        }
                        Err(e) => {
                            if from_setup {
                                self.setup.vault_error = Some(e);
                            } else {
                                self.toast(fill(self.t(T::VaultError), "msg", &e));
                            }
                        }
                    }
                }
                Done::History { name, rows } => {
                    if let Some(it) = self.item.as_mut().filter(|i| i.name == name) {
                        it.rows = rows;
                        it.loaded = true;
                    }
                }
                Done::Emus(f) => {
                    self.emu_found = Some(f);
                    self.waiting = false;
                }
                Done::Finished { synced } => {
                    if synced {
                        self.store.prefs.last_sync_ms = util::now_ms();
                        let _ = self.store.save_prefs();
                    }
                }
            }
        }
    }

    /// Ouvre la fiche d'un element : l'historique (coffre + copies de securite) arrive du fil de travail.
    pub fn reload_item(&mut self, name: &str) {
        let Some(g) = self.store.game(name).cloned() else { return };
        self.item = Some(ItemView { name: name.to_string(), rows: Vec::new(), loaded: false });
        self.worker.send(Job::LoadHistory { env: self.env(), game: g });
    }

    // ------------------------------------------------------------------ entree manette

    fn input_hook(&mut self, ectx: &egui::Context, raw: &mut egui::RawInput) {
        let popup = ectx.memory(|m| m.any_popup_open());
        let editing = self.text_focus.get();
        let mut actions: Vec<PadAction> = Vec::new();
        let mut extra: Vec<egui::Key> = Vec::new();
        if !popup {
            raw.events.retain(|e| match e {
                egui::Event::Key { key, pressed, .. } => {
                    let a = match key {
                        // En edition d'un champ, les fleches restent au champ (curseur) ; B (Echap) en sort.
                        egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight if editing => return true,
                        egui::Key::ArrowUp => Some(PadAction::Dir(Dir::Up)),
                        egui::Key::ArrowDown => Some(PadAction::Dir(Dir::Down)),
                        egui::Key::ArrowLeft => Some(PadAction::Dir(Dir::Left)),
                        egui::Key::ArrowRight => Some(PadAction::Dir(Dir::Right)),
                        egui::Key::Enter => Some(PadAction::Confirm),
                        egui::Key::Escape => Some(PadAction::Back),
                        egui::Key::F1 | egui::Key::F2 | egui::Key::F3 | egui::Key::F4 | egui::Key::PageUp | egui::Key::PageDown => {
                            if *pressed {
                                extra.push(*key);
                            }
                            return false;
                        }
                        _ => None,
                    };
                    match a {
                        Some(a) => {
                            if *pressed {
                                actions.push(a);
                            }
                            false
                        }
                        None => true,
                    }
                }
                _ => true,
            });
        }
        for a in actions {
            // Retour (B) : ferme le dialogue, ou quitte l'ecran ; sur l'accueil : masque l'appli.
            if a == PadAction::Back && !editing {
                self.pending_back = true;
            }
            self.focus.push(a);
            ectx.request_repaint();
        }
        self.shortcuts.extend(extra);
        // Touches decidees a la fin de l'image precedente (Entree / Echap injectes par la selection).
        for k in self.focus.take_injected() {
            for pressed in [true, false] {
                raw.events.push(agss::padfocus::key_event(k, pressed));
            }
        }
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.input_hook(ctx, raw);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = util::now_ms();
        self.text_focus.set(false);
        self.ensure_fonts(ctx);
        self.apply_theme(ctx);
        let focused = ctx.input(|i| i.focused);
        if !self.started {
            self.started = true;
            if self.cleaned.0 > 0 || self.cleaned.1.is_some() {
                self.toast(self.t(T::CleanedUp).into());
            }
            self.check_permission(now);
            if self.store.prefs.setup_done && !self.perm_missing {
                log::info!("demarrage : relecture et synchro de {} element(s)", self.store.config.games.len());
                self.refresh();
                self.sync_all();
            } else {
                log::info!("demarrage sans synchro (configure : {}, autorisation manquante : {})", self.store.prefs.setup_done, self.perm_missing);
            }
        } else {
            // Retour au premier plan apres une partie (focus repris, ou longue absence d'images) : on copie ce qui a change.
            let away = if !self.was_focused && focused { now.saturating_sub(self.unfocused_since_ms) } else { now.saturating_sub(self.last_frame_ms) };
            let resumed = (!self.was_focused && focused) || self.last_frame_ms > 0 && away > RESUME_SYNC_MS;
            if resumed && away > RESUME_SYNC_MS && self.store.prefs.setup_done && self.modal.is_none() {
                log::info!("retour au premier plan apres {} s", away / 1000);
                let came_back = self.check_permission(now);
                if came_back {
                    self.refresh();
                }
                self.sync_all();
            } else if self.perm_missing && now.saturating_sub(self.perm_checked_ms) > 1000 && self.check_permission(now) {
                self.refresh();
                self.sync_all();
            }
        }
        if self.was_focused && !focused {
            self.unfocused_since_ms = now;
            log::info!("arriere-plan");
        }
        self.was_focused = focused;
        self.last_frame_ms = now;
        if self.perm_missing {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        }
        self.handle_done();
        if let Some(t) = &self.toast {
            if now > t.until_ms {
                self.toast = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        if self.worker.busy() || self.waiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        // Raccourcis manette : X (F1) = synchroniser, Start (F3) = reglages, L1 / R1 = defilement de la page.
        let shortcuts = std::mem::take(&mut self.shortcuts);
        for k in shortcuts {
            match k {
                egui::Key::F1 if self.modal.is_none() && self.store.prefs.setup_done => self.sync_all(),
                egui::Key::F3 if self.modal.is_none() && self.store.prefs.setup_done && self.screen == Screen::Home => self.go(Screen::Settings),
                egui::Key::PageUp => self.scroll_delta = 260.0,
                egui::Key::PageDown => self.scroll_delta = -260.0,
                _ => {}
            }
        }
        let pending_back = std::mem::take(&mut self.pending_back);
        if pending_back {
            if self.modal.is_some() {
                self.modal = None;
            } else {
                match self.screen {
                    Screen::Home => sys::move_task_to_back(),
                    Screen::Setup => {
                        if self.setup.add_mode {
                            self.go(Screen::Settings);
                        } else if self.setup.step > 0 {
                            self.setup.step -= 1;
                            self.focus.reset();
                        }
                    }
                    Screen::Browse => {
                        let up = self.browse.as_mut().map(|b| {
                            match b.dir.clone() {
                                Some(d) if !b.roots.contains(&d) => {
                                    b.dir = d.parent().map(|p| p.to_path_buf());
                                    true
                                }
                                Some(_) => {
                                    b.dir = None;
                                    true
                                }
                                None => false,
                            }
                        });
                        if up != Some(true) {
                            let back = self.back_to;
                            self.browse = None;
                            self.go(back);
                        }
                    }
                    Screen::Item | Screen::Settings => self.go(Screen::Home),
                    Screen::Device | Screen::Emulators | Screen::About | Screen::Journal => self.go(Screen::Settings),
                }
            }
        }

        let modal_open = self.modal.is_some();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Self::col(self.palette.panel_bg)).inner_margin(egui::Margin::symmetric(18, 12))).show(ctx, |ui| {
            ui.add_enabled_ui(!modal_open, |ui| {
                let delta = std::mem::take(&mut self.scroll_delta);
                egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(self.screen.key()).show(ui, |ui| {
                    if delta != 0.0 {
                        ui.scroll_with_delta(egui::vec2(0.0, delta));
                    }
                    match self.screen {
                        Screen::Setup => screens::setup(self, ui),
                        Screen::Home => screens::home(self, ui),
                        Screen::Item => screens::item(self, ui),
                        Screen::Settings => screens::settings(self, ui),
                        Screen::Device => screens::device(self, ui),
                        Screen::Emulators => screens::emulators(self, ui),
                        Screen::About => screens::about(self, ui),
                        Screen::Journal => screens::journal(self, ui),
                        Screen::Browse => screens::browse(self, ui),
                    }
                });
            });
        });
        screens::toast_bar(self, ctx);
        if modal_open {
            self.focus.set_modal_scope(true);
            screens::modal(self, ctx);
            self.focus.set_modal_scope(false);
        }
        let key = self.screen.key() + if self.screen == Screen::Setup { self.setup.step as u8 } else { 0 };
        if self.focus.end_frame(ctx, key, self.modal.is_some(), true) {
            ctx.request_repaint();
        }
    }
}
