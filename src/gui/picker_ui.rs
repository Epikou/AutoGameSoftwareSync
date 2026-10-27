//! Selecteur de dossier guide (2.0, lot 8) : avant la boite de dialogue du systeme, nos propres suggestions (emplacements
//! trouves par la connaissance, dossiers recemment modifies par le programme, raccourcis), puis la boite du systeme
//! ouverte DIRECTEMENT dans le dossier le plus probable. Apercu et avertissements sur le dossier choisi.
//! Logique pure et testee : `crate::picker`. Un ecran qui veut un emplacement appelle `open_picker(cle, ...)` puis relit
//! le resultat avec `take_picked(cle)` a chaque image.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph};
use crate::picker::{self as pk, Shortcut, Warning, Why};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Resultat d'un choix, relu par l'ecran qui l'a demande.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    pub key: String,
    pub path: PathBuf,
    pub is_file: bool,
}

/// Ce que l'ecran appelant sait du programme et de l'emplacement.
#[derive(Clone, Default)]
pub struct PickOpts {
    /// Nom a montrer dans le titre (« Ou Celeste range-t-il ses donnees ? »).
    pub name: Option<String>,
    pub is_file: bool,
    /// Editeur, produit, nom : pour trouver `<base>/<nom>`.
    pub names: Vec<String>,
    /// Emplacements deja trouves par les couches de connaissance.
    pub found: Vec<PathBuf>,
    /// Programme a decrire : les couches locales sont interrogees en arriere-plan si `found` est vide.
    pub program: Option<crate::knowledge::Program>,
}

pub struct Picker {
    key: String,
    opts: PickOpts,
    is_file: bool,
    recent: Vec<pk::Suggestion>,
    shortcuts: Vec<pk::ShortcutItem>,
    folders: crate::knowledge::paths::Folders,
    prefixes: Vec<PathBuf>,
    vault: Option<PathBuf>,
    selected: Option<PathBuf>,
    filter: String,
    /// Emplacements trouves en arriere-plan (couches locales).
    async_found: Arc<Mutex<Vec<PathBuf>>>,
    cache: Arc<Mutex<HashMap<PathBuf, pk::Preview>>>,
    asked: HashSet<PathBuf>,
}

#[derive(Default)]
pub struct PickerUi {
    pub open: Option<Picker>,
    pub picked: Option<Picked>,
    /// Diagnostic des captures (`AGSS_UI_PICKER_DEMO=name:Celeste,names:Team Cherry|Hollow Knight,found:<dossier>,select:<dossier>,file`).
    pub demo: Vec<String>,
    demo_done: bool,
    demo_select: Option<PathBuf>,
}

impl Picker {
    fn found_paths(&self) -> Vec<PathBuf> {
        let mut v = self.opts.found.clone();
        for p in self.async_found.lock().map(|g| g.clone()).unwrap_or_default() {
            if !v.contains(&p) {
                v.push(p);
            }
        }
        v
    }

    fn hint(&self) -> pk::Hint {
        pk::Hint { names: self.opts.names.clone(), found: self.found_paths() }
    }

    /// Dossier dans lequel la boite du systeme s'ouvre.
    fn start_dir(&self) -> PathBuf {
        pk::start_dir(&self.hint(), &self.folders, &self.prefixes, cfg!(windows), &|p| p.exists())
    }
}

impl App {
    /// Ouvre le selecteur guide pour l'ecran `key` (le resultat se relit avec `take_picked(key)`).
    pub(super) fn open_picker(&mut self, ctx: &egui::Context, key: &str, opts: PickOpts) {
        let folders = pk::real_folders();
        let prefixes = if cfg!(windows) { vec![] } else { crate::proton::wine_prefixes_cached() };
        let state = crate::config::State::load(&self.model.ctx).unwrap_or_default();
        let recent = pk::recent_dirs(&self.model.config.games, &state, 3, &|p| p.is_dir());
        let async_found = Arc::new(Mutex::new(Vec::new()));
        if opts.found.is_empty() {
            if let Some(prog) = opts.program.clone() {
                let (slot, c, dd) = (async_found.clone(), ctx.clone(), self.shared.data_dir.clone());
                std::thread::spawn(move || {
                    let f = crate::knowledge::find_local(&prog, &dd, crate::util::now_ms());
                    if let Ok(mut g) = slot.lock() {
                        *g = f.into_iter().map(|x| x.path).collect();
                    }
                    c.request_repaint();
                });
            }
        }
        let shortcuts = pk::shortcuts(&folders, &prefixes, cfg!(windows), &|p| p.is_dir());
        let is_file = opts.is_file;
        self.picker.open = Some(Picker {
            key: key.to_string(),
            opts,
            is_file,
            recent,
            shortcuts,
            folders,
            prefixes,
            vault: self.model.dest(),
            selected: None,
            filter: String::new(),
            async_found,
            cache: Arc::new(Mutex::new(HashMap::new())),
            asked: HashSet::new(),
        });
        ctx.request_repaint();
    }

    /// Le choix de l'ecran `key`, une seule fois.
    pub(super) fn take_picked(&mut self, key: &str) -> Option<Picked> {
        if self.picker.picked.as_ref().map(|p| p.key == key).unwrap_or(false) {
            self.picker.picked.take()
        } else {
            None
        }
    }

    /// Programme decrit pour les couches de connaissance, d'apres un element de la liste.
    pub(super) fn pick_opts_for_game(&self, name: &str, is_file: bool) -> PickOpts {
        let g = self.model.game(name);
        let shown = g.map(|g| g.shown().to_string()).unwrap_or_else(|| name.to_string());
        let exe = g.and_then(|g| g.exe.clone()).unwrap_or_default();
        let exe_path = self.model.settings.per_game.get(name).and_then(|p| p.exe_path.clone());
        let program = if exe.is_empty() && exe_path.is_none() { None } else { Some(crate::knowledge::Program::describe(exe_path.as_deref(), &exe, &shown, false)) };
        let mut names: Vec<String> = Vec::new();
        if let Some(p) = &program {
            names.extend(p.companies());
            names.extend(p.names());
        }
        names.push(shown.clone());
        PickOpts { name: Some(shown), is_file, names, found: vec![], program }
    }

    /// Idem pour un programme deja decrit (ajout manuel, carte de detection) et ses emplacements trouves.
    pub(super) fn pick_opts_for_program(&self, prog: Option<&crate::knowledge::Program>, found: &[crate::knowledge::Found], fallback_name: &str, is_file: bool) -> PickOpts {
        let mut names: Vec<String> = Vec::new();
        if let Some(p) = prog {
            names.extend(p.companies());
            names.extend(p.names());
        }
        if !fallback_name.trim().is_empty() {
            names.push(fallback_name.trim().to_string());
        }
        let name = prog.map(|p| p.name.clone()).filter(|n| !n.trim().is_empty()).or_else(|| Some(fallback_name.trim().to_string()).filter(|n| !n.is_empty()));
        PickOpts { name, is_file, names, found: found.iter().map(|f| f.path.clone()).collect(), program: None }
    }

    /// Texte et couleur d'un avertissement.
    fn warn_text(&self, w: Warning) -> (String, [u8; 3]) {
        let (orange, red) = (self.palette_orange(), self.palette_red());
        match w {
            Warning::Huge { bytes, at_least } => {
                let size = if at_least { self.t(K::VPickAtLeast).replace("{}", &self.bytes(bytes)) } else { self.bytes(bytes) };
                (format!("{}
{}", self.t(K::VWarnHuge).replace("{}", &size), self.heavy_line(bytes, crate::weight::HEAVY_KEEP, at_least)), orange)
            }
            Warning::Heavy { bytes, at_least } => (format!("{} · {}
{}", self.t(K::VHeavyPill), self.heavy_line(bytes, crate::weight::HEAVY_KEEP, at_least), self.t(K::VHeavyBody)), orange),
            Warning::System => (self.t(K::VWarnSystem).to_string(), orange),
            Warning::TooBroad => (self.t(K::VWarnBroad).to_string(), orange),
            Warning::Sensitive => (self.t(K::VWarnSensitive).to_string(), orange),
            Warning::SensitiveInside(n) => (self.t(K::VWarnSensitiveInside).replace("{}", &n.to_string()), orange),
            Warning::IsVault => (self.t(K::VWarnVault).to_string(), red),
            Warning::Empty => (self.t(K::VWarnEmpty).to_string(), orange),
        }
    }

    fn palette_orange(&self) -> [u8; 3] {
        [0xe6, 0xaa, 0x28]
    }

    fn palette_red(&self) -> [u8; 3] {
        [0xdc, 0x50, 0x50]
    }

    /// Libelle d'un raccourci.
    pub(super) fn shortcut_label(&self, s: &pk::ShortcutItem) -> String {
        match s.kind {
            Shortcut::SavedGames => self.t(K::VPickSavedGames).to_string(),
            Shortcut::MyGames => self.t(K::VPickMyGames).to_string(),
            Shortcut::Documents => self.t(K::VPickDocuments).to_string(),
            Shortcut::AppDataRoaming => self.t(K::VPickAppData).to_string(),
            Shortcut::AppDataLocal => self.t(K::VPickLocal).to_string(),
            Shortcut::LocalLow => self.t(K::VPickLocalLow).to_string(),
            Shortcut::ConfigDir => self.t(K::VPickConfig).to_string(),
            Shortcut::ShareDir => self.t(K::VPickShare).to_string(),
            Shortcut::Proton => self.t(K::VPickProton).replace("{}", &s.tag),
            Shortcut::Home => self.t(K::VPickHome).to_string(),
        }
    }

    fn shortcut_key(s: Shortcut) -> K {
        match s {
            Shortcut::SavedGames => K::VPickSavedGames,
            Shortcut::MyGames => K::VPickMyGames,
            Shortcut::Documents => K::VPickDocuments,
            Shortcut::AppDataRoaming => K::VPickAppData,
            Shortcut::AppDataLocal => K::VPickLocal,
            Shortcut::LocalLow => K::VPickLocalLow,
            Shortcut::ConfigDir => K::VPickConfig,
            Shortcut::ShareDir => K::VPickShare,
            Shortcut::Proton => K::VPickProton,
            Shortcut::Home => K::VPickHome,
        }
    }

    /// Chemin en miettes (« Donnees locales > Team Cherry > Hollow Knight »).
    pub(super) fn crumbs(&self, path: &Path, folders: &crate::knowledge::paths::Folders) -> String {
        let (base, rest) = pk::friendly_parts(path, folders);
        match base {
            Some(b) => {
                let mut v = vec![self.t(Self::shortcut_key(b)).replace("{}", "")];
                v.extend(rest);
                v.join(" \u{203a} ")
            }
            None => path.display().to_string(),
        }
    }

    /// Ligne d'apercu : « 4 fichiers · 312 Ko · modifie il y a 2 min ».
    pub(super) fn preview_line(&self, pv: &pk::Preview, now: u64) -> String {
        if !pv.exists {
            return self.t(K::VPickNotFound).to_string();
        }
        let size = self.bytes(pv.bytes);
        let when = pv.newest_ms.map(|ms| self.ago_text(now, ms)).unwrap_or_else(|| "-".to_string());
        let files = if pv.truncated { self.t(K::VPickAtLeast).replace("{}", &pv.files.to_string()) } else { pv.files.to_string() };
        if pv.files == 1 && !pv.truncated {
            self.t(K::VPickPreviewOne).replace("{0}", &size).replace("{1}", &when)
        } else {
            self.t(K::VPickPreviewMany).replace("{0}", &files).replace("{1}", &size).replace("{2}", &when)
        }
    }

    /// Dessine le selecteur s'il est ouvert (a appeler chaque image).
    pub(super) fn picker_overlay(&mut self, ctx: &egui::Context) {
        if !self.picker.demo.is_empty() && !self.picker.demo_done {
            self.picker.demo_done = true;
            let mut opts = PickOpts::default();
            for t in self.picker.demo.clone() {
                match t.split_once(':') {
                    Some(("name", v)) => opts.name = Some(v.to_string()),
                    Some(("names", v)) => opts.names = v.split('|').map(|s| s.to_string()).collect(),
                    Some(("found", v)) => opts.found = v.split('|').map(PathBuf::from).collect(),
                    Some(("select", v)) => self.picker.demo_select = Some(PathBuf::from(v)),
                    _ if t == "file" => opts.is_file = true,
                    _ => {}
                }
            }
            self.open_picker(ctx, "demo", opts);
        }
        let Some(mut p) = self.picker.open.take() else { return };
        self.focus.set_modal_scope(true);
        if let Some(d) = self.picker.demo_select.take() {
            p.selected = Some(d);
        }
        // apercus a calculer (une seule fois par dossier, en arriere-plan)
        let mut want: Vec<PathBuf> = p.found_paths().into_iter().take(3).chain(p.recent.iter().map(|s| s.path.clone())).collect();
        if let Some(s) = &p.selected {
            want.push(s.clone());
        }
        for w in want {
            if p.asked.insert(w.clone()) {
                let (cache, c) = (p.cache.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let pv = pk::scan(&w, pk::PREVIEW_MAX_FILES);
                    if let Ok(mut g) = cache.lock() {
                        g.insert(w, pv);
                    }
                    c.request_repaint();
                });
            }
        }
        let expert = self.model.settings.expert_mode;
        let now = crate::util::now_ms();
        let title = match p.opts.name.as_deref().filter(|n| !n.trim().is_empty()) {
            Some(n) => self.t(K::VPickTitleFor).replace("{}", n),
            None => self.t(K::VPickTitle).to_string(),
        };
        let tk = u::tokens(ctx);
        let found = p.found_paths();
        let suggestions = pk::suggestions(&found, p.recent.clone(), &|x| x.exists());
        let mut action: Option<PickAct> = None;
        let max_h = (ctx.screen_rect().height() - 150.0).max(240.0);
        let big = self.big_now;
        let closed = u::custom_dialog(ctx, "picker", 640.0, |ui| {
            ui.horizontal(|ui| {
                let g = u::layout(ui, &title, u::tokens::FS_H2, u::col(tk.text), ui.available_width() - 40.0, 2);
                let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
                ui.painter().galley(r.min, g, u::col(tk.text));
            });
            ui.add_space(u::tokens::SP_S);
            // Dossier / Fichier (mode simple) ; filtre de noms seulement en mode expert.
            ui.horizontal_wrapped(|ui| {
                if u::chip(ui, self.t(K::TargetFolder), None, !p.is_file).clicked() {
                    p.is_file = false;
                    p.selected = None;
                }
                if u::chip(ui, self.t(K::TargetFile), None, p.is_file).clicked() {
                    p.is_file = true;
                    p.selected = None;
                }
                if expert && p.is_file {
                    let r = ui.add(egui::TextEdit::singleline(&mut p.filter).hint_text(self.t(K::PickFilterHint)).desired_width(260.0));
                    let _ = r;
                }
            });
            ui.add_space(u::tokens::SP_S);
            egui::ScrollArea::vertical().id_salt("picker_body").max_height(max_h).auto_shrink([true, true]).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // 1. suggestions
                let mut first_found = true;
                let mut last_head: Option<Why> = None;
                for (i, s) in suggestions.iter().enumerate() {
                    if last_head != Some(s.why) {
                        let head = if s.why == Why::Found { K::VPickFoundHead } else { K::VPickRecentHead };
                        ui.label(RichText::new(self.t(head)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                        last_head = Some(s.why);
                    }
                    let pv = p.cache.lock().ok().and_then(|g| g.get(&s.path).cloned());
                    let sub = match &pv {
                        Some(pv) => self.preview_line(pv, now),
                        None => "\u{2026}".to_string(),
                    };
                    let crumb = self.crumbs(&s.path, &p.folders);
                    let trailing = if s.why == Why::Found && first_found { self.t(K::VPickMostLikely) } else { "" };
                    if s.why == Why::Found {
                        first_found = false;
                    }
                    let sel = p.selected.as_ref() == Some(&s.path);
                    let row = u::ListRow { icon: Some(u::TileIcon::Glyph(if s.why == Why::Found { Glyph::Star } else { Glyph::Clock }, if s.why == Why::Found { [0x8a, 0x6a, 0x10] } else { [0x44, 0x55, 0x66] })), title: &crumb, subtitle: &sub, trailing, selected: sel, appear: None };
                    if u::list_row(ui, ("pick_sugg", i), &row).clicked() {
                        p.selected = Some(s.path.clone());
                    }
                }
                if suggestions.is_empty() && p.selected.is_none() {
                    ui.label(RichText::new(self.t(K::VPickNothingYet)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                }
                ui.add_space(u::tokens::SP_M);
                // 2. raccourcis
                if !p.shortcuts.is_empty() {
                    ui.label(RichText::new(self.t(K::VPickStartHere)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                    ui.horizontal_wrapped(|ui| {
                        for s in &p.shortcuts {
                            if u::chip(ui, &self.shortcut_label(s), None, false).clicked() {
                                action = Some(PickAct::Browse(s.path.clone()));
                            }
                        }
                    });
                    ui.add_space(u::tokens::SP_M);
                }
                // 3. apercu et avertissements du dossier choisi
                if let Some(sel) = p.selected.clone() {
                    let pv = p.cache.lock().ok().and_then(|g| g.get(&sel).cloned());
                    egui::Frame::new().fill(u::col(tk.card2)).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                            u::paint_glyph(ui.painter(), r, if p.is_file { Glyph::Window } else { Glyph::Folder }, u::col(tk.accent_hover));
                            ui.vertical(|ui| {
                                match &pv {
                                    Some(pv) => {
                                        ui.label(RichText::new(self.preview_line(pv, now)).color(u::col(tk.text)));
                                    }
                                    None => {
                                        ui.add(egui::Spinner::new().size(14.0));
                                    }
                                }
                                let crumb = self.crumbs(&sel, &p.folders);
                                let same = crumb == sel.display().to_string();
                                ui.label(RichText::new(crumb).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                                if expert && !same {
                                    ui.label(RichText::new(sel.display().to_string()).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
                                }
                            });
                        });
                        if let Some(pv) = &pv {
                            let wc = pk::WarnCtx::real(&p.folders, p.vault.as_deref());
                            let ws = pk::warnings(&sel, pv, &wc);
                            if ws.is_empty() {
                                ui.add_space(u::tokens::SP_XS);
                                icon_label(ui, Icon::Check, self.t(K::VPickAllGood), u::col(tk.ok));
                            }
                            for w in ws {
                                let (txt, c) = self.warn_text(w);
                                ui.add_space(u::tokens::SP_XS);
                                egui::Frame::new().fill(u::col(crate::theme::mix(tk.card, c, 0.16))).stroke(egui::Stroke::new(1.0_f32, u::col(c))).corner_radius(u::tokens::R_SM).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(txt).color(u::col(tk.text)).size(u::tokens::FS_SMALL));
                                });
                            }
                        }
                    });
                }
            });
            ui.add_space(u::tokens::SP_L);
            // pied : Parcourir a gauche ; Annuler, Choisir a droite
            let mut blocked = true;
            if let Some(sel) = &p.selected {
                let pv = p.cache.lock().ok().and_then(|g| g.get(sel).cloned());
                if let Some(pv) = pv {
                    let wc = pk::WarnCtx::real(&p.folders, p.vault.as_deref());
                    blocked = !pv.exists || pk::warnings(sel, &pv, &wc).iter().any(|w| w.blocks());
                }
            }
            ui.horizontal(|ui| {
                if u::button_with(ui, Btn::Ghost, self.t(K::BtnBrowse), Some(Glyph::Folder), big).clicked() {
                    action = Some(PickAct::Browse(p.start_dir()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = self.t(if p.is_file { K::VPickChooseFile } else { K::VPickChooseFolder });
                    let r = ui.add_enabled_ui(!blocked, |ui| u::button_with(ui, Btn::Primary, label, None, big)).inner;
                    if r.clicked() {
                        action = Some(PickAct::Choose);
                    }
                    if u::button_with(ui, Btn::Secondary, self.t(K::BtnCancel), None, big).clicked() {
                        action = Some(PickAct::Cancel);
                    }
                });
            });
        });
        match action {
            Some(PickAct::Cancel) => return,
            Some(PickAct::Choose) => {
                if let Some(sel) = p.selected.clone() {
                    self.picker.picked = Some(Picked { key: p.key.clone(), path: sel, is_file: p.is_file });
                    ctx.request_repaint();
                    return;
                }
            }
            Some(PickAct::Browse(dir)) => {
                if let Some(path) = system_dialog(&dir, p.is_file, &p.filter) {
                    p.selected = Some(path);
                }
            }
            None => {}
        }
        if closed {
            return;
        }
        self.picker.open = Some(p);
    }
}

enum PickAct {
    Browse(PathBuf),
    Choose,
    Cancel,
}

/// Boite de dialogue du systeme, ouverte directement dans `dir`.
fn system_dialog(dir: &Path, is_file: bool, filter: &str) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new();
    if dir.is_dir() {
        d = d.set_directory(dir);
    }
    if is_file {
        let exts = pk::filter_exts(filter);
        if !exts.is_empty() {
            d = d.add_filter("filter", &exts);
        }
        d.pick_file()
    } else {
        d.pick_folder()
    }
}
