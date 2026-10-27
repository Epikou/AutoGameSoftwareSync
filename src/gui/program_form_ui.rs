//! Formulaire d'ajout 2.0 (lot 11), en mots simples : « Ajouter ce programme » (on cherche ses donnees : « Emplacement
//! trouve · Confiance », selecteur guide du lot 8) ou « Ajouter cet element » (dossier / fichier). Plus jamais « exe » en mode
//! simple. Les options avancees (nom, filtre, emplacements, planification, chemin du programme) sont dans
//! `form_advanced_expert` (manual_ui.rs), affichees en mode expert seulement. Utilise aussi par l'etape 5 de l'assistant.
//! Ce fichier ne contient QUE le vocabulaire du mode simple (verifie par test).
use super::*;
use crate::addform::{self, ClickResult, Validity};
use crate::addpage::{self, FormKind};
use crate::gui::ui::{self as u, Btn, Glyph, ListRow, TileIcon};
use crate::knowledge::{self, Found};

/// Cle du selecteur guide ouvert depuis le formulaire (emplacement des donnees).
const PLACE_KEY: &str = "man_path";

/// Actions demandees pendant le dessin, executees ensuite (aucun emprunt de `self` pendant le dessin).
#[derive(Default)]
struct FormAct {
    pick_place: bool,
    change_program: bool,
    use_sugg: Option<usize>,
    use_found: Option<usize>,
    ignore: bool,
    learn: bool,
    add: bool,
}

impl App {
    /// Chemin en miettes (« Donnees locales > Team Cherry > Hollow Knight »).
    fn place_crumbs(&self, p: &std::path::Path) -> String {
        match &self.add_ui.folders {
            Some(f) => self.crumbs(p, f),
            None => p.display().to_string(),
        }
    }

    fn know_level_text(&self, c: u8) -> &'static str {
        self.t(match knowledge::level(c) {
            knowledge::Level::High => K::KnowHigh,
            knowledge::Level::Medium => K::KnowMedium,
            knowledge::Level::Low => K::KnowLow,
        })
    }

    /// « Emplacement trouve · Confiance : elevee ».
    fn found_head(&self, c: u8) -> String {
        format!("{} · {}", self.t(K::KnowFound), self.t(K::KnowConfidence).replace("{}", self.know_level_text(c)))
    }

    /// Cherche l'emplacement des donnees du programme choisi (autres PC, base des jeux, connaissance intelligente) : lance
    /// les fils de recherche, applique une proposition unique. Retourne (cle du programme, propositions, vient d'un autre PC).
    fn form_lookups(&mut self, ctx: &egui::Context) -> (String, Vec<crate::hints::Suggestion>, bool) {
        let exe_key = crate::hints::exe_file_name(&self.man_exe).to_lowercase();
        // Linux : l'indice d'un programme venu d'un PC Windows est cherche dans les prefixes Proton / Wine de cette machine.
        #[cfg(not(windows))]
        let prefixes = if exe_key.ends_with(".exe") && !self.hints.is_empty() { crate::proton::wine_prefixes_cached() } else { Vec::new() };
        #[cfg(windows)]
        let prefixes: Vec<PathBuf> = Vec::new();
        let shared: Vec<crate::hints::Suggestion> = if self.hint_ignored.contains(&exe_key) { vec![] } else { crate::hints::suggestions_ext(&self.hints, &self.man_exe, &self.known, &prefixes, &|p| p.is_dir()) };
        let from_shared = !shared.is_empty();
        self.add_page.observe_exe(&self.man_exe.trim().to_lowercase());
        let full_key = self.man_exe.trim().to_lowercase();
        // Recherche dans la base : un programme Windows (ou Proton / Wine sous Linux) ; sous Linux aussi un programme natif.
        let lookup_ok = exe_key.ends_with(".exe") || (cfg!(not(windows)) && !exe_key.is_empty());
        if !from_shared && lookup_ok && full_key != self.auto_key {
            self.auto_key = full_key.clone();
            self.auto_sugg.clear();
            self.man_found.clear();
            self.man_found_key.clear();
            self.man_prog = None;
            self.man_learn_armed = None;
            let (tx, c, dd, sh) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), self.shared.clone());
            let (typed, dg, fk, nm) = (self.man_exe.trim().to_string(), self.dest_games.clone(), full_key.clone(), self.man_name_auto.clone());
            std::thread::spawn(move || {
                let db = crate::gamedb::embedded(&dd);
                let libs = StoreCtx::auto().steam_libs;
                let p = std::path::Path::new(&typed);
                let v = crate::autofill::lookup(&db, &typed, if p.is_absolute() { Some(p) } else { None }, &dg, &libs, &|p| p.is_dir());
                let known = v.iter().any(|s| s.exists && s.local_path.is_some());
                let _ = tx.send(Msg::Auto(fk.clone(), v));
                c.request_repaint();
                // Ni la base ni les autres PC : connaissance intelligente (puis recherche discrete en arriere-plan).
                if !known {
                    let prog = crate::knowledge::Program::describe(if p.is_absolute() { Some(p) } else { None }, &typed, &nm, false);
                    let found = crate::knowledge::find_local(&prog, &dd, crate::util::now_ms());
                    if found.is_empty() && p.is_absolute() {
                        sh.request_web(super::knowledge_ui::KnowTarget::Manual(fk.clone()), prog.clone());
                    }
                    let _ = tx.send(Msg::Known(super::knowledge_ui::KnowTarget::Manual(fk), prog, found));
                    c.request_repaint();
                }
            });
        }
        let all: Vec<crate::hints::Suggestion> = if from_shared { shared } else if self.auto_key == full_key { self.auto_sugg.clone() } else { vec![] };
        // Une proposition unique dont l'emplacement existe est appliquee toute seule (une fois par programme).
        if !full_key.is_empty() && self.man_filled_key != full_key {
            if let [s] = all.as_slice() {
                if let (Some(p), true) = (&s.local_path, s.exists) {
                    self.man_name = s.game_name.clone();
                    self.man_path = p.display().to_string();
                    self.man_is_file = false;
                    self.man_path_found = true;
                    self.man_filled_key = full_key.clone();
                    self.add_page.dismiss(&full_key);
                }
            }
        }
        (full_key, all, from_shared)
    }

    /// Ligne d'emplacement (coche, chemin en miettes, explication, bouton a droite) ; retourne le clic.
    fn place_row(&self, ui: &mut egui::Ui, id: impl std::hash::Hash, ok: bool, title: &str, sub: &str, trailing: &str) -> egui::Response {
        let tk = u::tokens(ui.ctx());
        let row = ListRow { icon: Some(TileIcon::Glyph(if ok { Glyph::Check } else { Glyph::Folder }, if ok { tk.ok } else { tk.card2 })), title, subtitle: sub, trailing, selected: false, appear: None };
        let r = u::list_row(ui, id, &row);
        self.deco(ui, r, None)
    }

    /// Bloc « Ou sont ses donnees ? » d'un programme : emplacement applique, propositions, ou bouton de choix.
    fn form_where(&mut self, ui: &mut egui::Ui, suggs: &[crate::hints::Suggestion], all: &[crate::hints::Suggestion], from_shared: bool, lookup_done: bool, nothing_else: bool, full_key: &str, act: &mut FormAct) {
        let tk = u::tokens(ui.ctx());
        let big = self.big_now;
        if !self.man_path.trim().is_empty() {
            // Emplacement trouve ou choisi.
            let p = PathBuf::from(self.man_path.trim());
            let crumbs = self.place_crumbs(&p);
            let same = |f: &Found| crate::gamedetect::norm_path(&f.path) == crate::gamedetect::norm_path(&p);
            let sub = if self.man_path_found {
                match self.man_found.iter().find(|f| same(f)) {
                    Some(f) => self.found_head(f.confidence),
                    None => self.t(K::KnowFound).to_string(),
                }
            } else {
                self.t(K::VAddPlaceChosen).to_string()
            };
            if self.place_row(ui, "place", true, &crumbs, &sub, self.t(K::AddChange)).clicked() {
                act.pick_place = true;
            }
            return;
        }
        if !suggs.is_empty() {
            // Trouve sur un autre appareil, ou dans la base : une ou plusieurs propositions.
            if from_shared {
                ui.label(RichText::new(self.t(K::VAddFromOther)).size(u::tokens::FS_SMALL + 1.0).strong().color(u::col(tk.ok)));
            }
            if suggs.len() > 1 {
                ui.label(RichText::new(self.t(K::VAddPlaceSeveral)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.weak)));
            }
            for (i, s) in suggs.iter().enumerate() {
                let (sub, ok) = match &s.local_path {
                    Some(p) if s.exists => (self.place_crumbs(p), true),
                    _ => (self.t(K::VAddPlaceMissing).to_string(), false),
                };
                let r = self.place_row(ui, ("sugg", i), ok, &s.game_name, &sub, self.t(K::BtnHintUse));
                if r.on_hover_text(self.t(K::VAddUseThis)).clicked() {
                    act.use_sugg = Some(i);
                }
            }
            let r = u::button(ui, Btn::Ghost, self.t(K::BtnHintIgnore));
            if self.deco(ui, r, Some(K::TipHintIgnore)).clicked() {
                act.ignore = true;
            }
        } else if lookup_done && nothing_else && !self.man_found.is_empty() {
            // Connaissance intelligente : « Emplacement trouve · Confiance : elevee ».
            let found = self.man_found.clone();
            ui.label(RichText::new(self.found_head(found[0].confidence)).size(u::tokens::FS_SMALL + 1.0).strong().color(u::col(tk.ok)));
            for (i, f) in found.iter().enumerate() {
                let kind = self.t(if f.kind == knowledge::Kind::Save { K::KnowSaves } else { K::KnowSettings });
                let src = self.know_source_text(f.layer);
                let mut sub = format!("{} · {} · {}", kind, src, self.t(K::KnowConfidence).replace("{}", self.know_level_text(f.confidence)));
                if self.model.settings.expert_mode {
                    sub = format!("{} ({}/100)", sub, f.confidence);
                }
                let title = self.place_crumbs(&f.path);
                let r = self.place_row(ui, ("found", i), true, &title, &sub, self.t(K::BtnHintUse));
                if r.on_hover_text(self.t(K::VAddUseThis)).clicked() {
                    act.use_found = Some(i);
                }
            }
        } else if !full_key.is_empty() && !lookup_done {
            // Recherche en cours.
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(self.t(K::VAddWhereHint)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.weak)));
            });
        }
        // Ni emplacement ni proposition applicable : choisir l'endroit avec le selecteur guide.
        match crate::autofill::status_of(all) {
            crate::autofill::AutoStatus::Missing if suggs.is_empty() => {
                ui.label(RichText::new(self.t(K::VAddPlaceMissing)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.orange)));
            }
            _ => {}
        }
        ui.add_space(u::tokens::SP_S);
        let r = u::button_sized(ui, Btn::Secondary, self.t(K::VAddChoosePlace), Some(Glyph::Folder), big, ui.available_width());
        if self.deco(ui, r, Some(K::TipManPath)).clicked() {
            act.pick_place = true;
        }
        // Introuvable : apprendre a la prochaine utilisation du programme (couche 3).
        if lookup_done && nothing_else && self.man_found.is_empty() && std::path::Path::new(self.man_exe.trim()).is_file() {
            let name = if self.man_name.trim().is_empty() { crate::hints::exe_file_name(&self.man_exe) } else { self.man_name.trim().to_string() };
            if self.man_learn_armed.as_deref() == Some(full_key) {
                ui.horizontal_wrapped(|ui| {
                    tiny_glyph(ui, Glyph::Check, u::col(tk.ok));
                    ui.label(RichText::new(self.t(K::KnowLearnArmed).replace("{}", &name)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.ok)));
                });
            } else {
                let r = u::button(ui, Btn::Ghost, self.t(K::BtnLearn));
                if self.deco(ui, r, Some(K::TipLearn)).clicked() {
                    act.learn = true;
                }
            }
        }
    }

    fn know_source_text(&self, l: knowledge::Layer) -> &'static str {
        // La recherche en ligne n'est jamais annoncee dans l'interface : « verifie sur ce PC ».
        self.t(match l {
            knowledge::Layer::Launcher => K::KnowSrcLauncher,
            knowledge::Layer::Deduce => K::KnowSrcDeduce,
            knowledge::Layer::Learn => K::KnowSrcLearn,
            knowledge::Layer::Web => K::KnowSrcChecked,
            knowledge::Layer::Home => K::KnowSrcHome,
        })
    }

    // ------------------------------------------------------------ formulaire

    /// Formulaire d'ajout : programme (on cherche ses donnees) ou element (dossier / fichier). Ne dessine rien s'il n'y a rien a
    /// ajouter ; affiche aussi les alertes de doublon.
    pub(super) fn add_form(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let kind = addpage::form_kind(&self.man_exe, self.add_ui.open);
        if kind == FormKind::Hidden {
            self.add_alerts(ui, ctx);
            return;
        }
        let program = kind == FormKind::Program;
        let expert = self.model.settings.expert_mode;
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let now = ctx.input(|i| i.time);
        if self.add_ui.folders.is_none() {
            self.add_ui.folders = Some(crate::picker::real_folders());
        }
        // Emplacement choisi avec le selecteur guide.
        if let Some(p) = self.take_picked(PLACE_KEY) {
            self.man_path = p.path.display().to_string();
            self.man_is_file = p.is_file;
            self.man_path_found = false;
        }
        let (full_key, all, from_shared) = if program { self.form_lookups(ctx) } else { (String::new(), Vec::new(), false) };
        let suggs: Vec<crate::hints::Suggestion> = if program && self.add_page.visible(&full_key) { all.clone() } else { vec![] };
        // Connaissance intelligente : seulement sans proposition de la base ni des autres PC.
        let lookup_done = program && !full_key.is_empty() && self.man_found_key == full_key;
        let nothing_else = !all.iter().any(|s| s.exists && s.local_path.is_some());
        if lookup_done && nothing_else && !self.man_found.is_empty() && self.man_path.trim().is_empty() && self.man_filled_key != full_key && knowledge::level(self.man_found[0].confidence) == knowledge::Level::High {
            // Confiance elevee : appliquee toute seule (une fois par programme).
            self.man_path = self.man_found[0].path.display().to_string();
            self.man_is_file = self.man_found[0].path.is_file();
            self.man_path_found = true;
            self.man_filled_key = full_key.clone();
        }
        // Sans programme, la copie est planifiee chaque jour a midi (mode simple : sans rien demander).
        if !program && !expert && self.man_sched.is_none() {
            self.man_sched = Some(crate::schedule::Schedule::fixed(vec![12 * 3600]));
        }
        let icon = if program { self.man_exe_icon(ctx) } else { None };
        let mut act = FormAct::default();
        let title = self.tm(if program { K::VAddFormProgram } else { K::VAddFormTarget });
        u::card(ui, Some(title), |ui| {
            // ---- en-tete : ce qu'on ajoute
            ui.horizontal(|ui| {
                let s = if big { 56.0 } else { 46.0 };
                match (&icon, program) {
                    (Some(t), true) => {
                        ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(s, s)));
                    }
                    _ => {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                        ui.painter().rect_filled(r, 10.0, u::col(tk.accent_soft));
                        let g = if program { Glyph::Window } else { Glyph::Folder };
                        u::paint_glyph(ui.painter(), r.shrink(s * 0.24), g, u::col(tk.accent_hover));
                    }
                }
                ui.add_space(u::tokens::SP_S);
                ui.vertical(|ui| {
                    if program {
                        let name = if self.man_name.trim().is_empty() { crate::hints::exe_file_name(&self.man_exe) } else { self.man_name.clone() };
                        ui.label(RichText::new(name).size(u::tokens::FS_H3 + 1.0).strong().color(u::col(tk.text)));
                        ui.horizontal(|ui| {
                            tiny_glyph(ui, Glyph::Check, u::col(tk.ok));
                            ui.label(RichText::new(self.t(K::AddRecognized)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.ok)));
                        });
                    } else if self.man_path.trim().is_empty() {
                        ui.label(RichText::new(self.man_name.clone()).size(u::tokens::FS_H3 + 1.0).strong().color(u::col(tk.text)));
                        ui.label(RichText::new(self.t(K::VAddWhereHint)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.weak)));
                    } else {
                        let p = PathBuf::from(self.man_path.trim());
                        ui.label(RichText::new(self.t(if self.man_is_file { K::VAddTargetFile } else { K::VAddTargetFolder })).size(u::tokens::FS_H3 + 1.0).strong().color(u::col(tk.text)));
                        ui.label(RichText::new(self.place_crumbs(&p)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.weak)));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if program {
                        let r = u::button(ui, Btn::Ghost, self.t(K::VAddChangeProgram));
                        if self.deco(ui, r, None).clicked() {
                            act.change_program = true;
                        }
                    } else if !self.man_path.trim().is_empty() {
                        let r = u::button(ui, Btn::Ghost, self.t(K::AddChange));
                        if self.deco(ui, r, None).clicked() {
                            act.pick_place = true;
                        }
                    }
                });
            });
            // ---- nom (dossier / fichier : toujours ; programme : seulement s'il manque, les options avancees l'ont sinon)
            if !program || (self.man_name.trim().is_empty() && !expert) {
                ui.add_space(u::tokens::SP_M);
                let (label, hint) = (self.t(K::VAddNameLabel), self.t(K::VAddNameHint));
                u::field(ui, "add_name", label, &mut self.man_name, hint);
            }
            // ---- ou sont ses donnees ?
            if program {
                ui.add_space(u::tokens::SP_L);
                ui.label(RichText::new(self.t(K::VAddWhereTitle)).size(u::tokens::FS_H3).strong().color(u::col(tk.text)));
                ui.add_space(u::tokens::SP_XS);
                self.form_where(ui, &suggs, &all, from_shared, lookup_done, nothing_else, &full_key, &mut act);
            } else if self.man_path.trim().is_empty() {
                ui.add_space(u::tokens::SP_M);
                let r = u::button_sized(ui, Btn::Secondary, self.t(K::VAddChoosePlace), Some(Glyph::Folder), big, ui.available_width());
                if self.deco(ui, r, Some(K::TipManPath)).clicked() {
                    act.pick_place = true;
                }
            }
            // ---- sans programme : copie quotidienne (simple) ou planification detaillee (expert)
            if !program && !expert {
                ui.add_space(u::tokens::SP_M);
                ui.label(RichText::new(self.t(K::VAddDailyNote)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.weak)));
            }
            if expert {
                self.form_advanced_expert(ui, ctx, now, program);
            }
            // ---- ajouter
            let path_ok = {
                let p = std::path::Path::new(self.man_path.trim());
                if self.man_is_file { p.is_file() } else { p.is_dir() }
            };
            let v = self.man_validity(path_ok);
            // Diagnostic (verification du parcours sans clic) : AGSS_DEMO_AUTOADD ajoute des que le formulaire est valide.
            if std::env::var_os("AGSS_DEMO_AUTOADD").is_some() && v == Validity::Ready && self.add_ui.demo_done < 1 {
                self.add_ui.demo_done += 1;
                act.add = true;
            }
            if v != Validity::NeedSchedule {
                self.man_explained = false;
            }
            ui.add_space(u::tokens::SP_L);
            let need = match v {
                Validity::NeedName => Some(self.t(K::VAddNeedName)),
                Validity::NeedPath => Some(self.t(K::VAddNeedPath)),
                Validity::NeedSchedule if addform::show_schedule_message(v, self.man_explained) => Some(self.schedule_need_expert()),
                _ => None,
            };
            if let Some(n) = need {
                ui.label(RichText::new(n).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.orange)));
                ui.add_space(u::tokens::SP_XS);
            }
            let label = if expert { self.add_label_expert() } else { self.t(K::VProtectIt) };
            let clickable = addform::button_clickable(v);
            let r = ui.add_enabled_ui(clickable, |ui| u::button_sized(ui, Btn::Primary, label, Some(Glyph::Shield), big, ui.available_width())).inner;
            let r = self.deco(ui, r, Some(K::TipManAdd));
            if r.clicked() {
                match addform::on_click(v) {
                    ClickResult::Add => act.add = true,
                    ClickResult::ExplainSchedule => {
                        self.man_explained = true;
                        self.man_flash = Some(now);
                    }
                    ClickResult::Nothing => {}
                }
            }
        });
        self.form_apply(ctx, act, &full_key, &suggs);
        // Alertes : nom ressemblant a un element deja present, emplacement deja utilise.
        self.add_alerts(ui, ctx);
    }

    /// Execute les actions demandees pendant le dessin du formulaire.
    fn form_apply(&mut self, ctx: &egui::Context, act: FormAct, full_key: &str, suggs: &[crate::hints::Suggestion]) {
        if act.change_program {
            if let Some(p) = Self::program_dialog(self.model.settings.lang).pick_file() {
                if let Some(e) = self.resolve_program(&p) {
                    self.add_start_program(e);
                }
            }
        }
        if act.pick_place {
            let opts = self.pick_opts_for_program(self.man_prog.as_ref(), &self.man_found, &self.man_name, self.man_is_file);
            self.open_picker(ctx, PLACE_KEY, opts);
        }
        if let Some(i) = act.use_sugg {
            if let Some(s) = suggs.get(i).cloned() {
                self.add_page.dismiss(full_key);
                self.man_name = s.game_name;
                if let Some(p) = s.local_path {
                    self.man_path = p.display().to_string();
                    self.man_path_found = true;
                }
            }
        }
        if act.ignore {
            self.add_page.dismiss(full_key);
            self.hint_ignored.insert(crate::hints::exe_file_name(&self.man_exe).to_lowercase());
        }
        if let Some(i) = act.use_found {
            if let Some(f) = self.man_found.get(i).cloned() {
                self.man_path = f.path.display().to_string();
                self.man_is_file = f.path.is_file();
                self.man_path_found = true;
            }
        }
        if act.learn {
            let name = if self.man_name.trim().is_empty() { crate::hints::exe_file_name(&self.man_exe) } else { self.man_name.trim().to_string() };
            let dd = self.shared.data_dir.clone();
            let mut l = crate::knowledge::store::LearnList::load(&dd);
            l.add(&self.man_exe, &name, Some(PathBuf::from(self.man_exe.trim())), crate::util::now_ms());
            let _ = l.save(&dd);
            self.man_learn_armed = Some(full_key.to_string());
            self.pad_log(&format!("[Connaissance] {} : emplacement a apprendre a la prochaine utilisation", crate::hints::exe_file_name(&self.man_exe)));
        }
        if act.add {
            self.form_submit(ctx, full_key);
        }
    }

    /// « Proteger » / « Ajouter » : cree l'element (alertes de doublon avant), puis vide le formulaire.
    fn form_submit(&mut self, ctx: &egui::Context, full_key: &str) {
        let exe_full = PathBuf::from(self.man_exe.trim());
        let exe_name = Some(crate::hints::exe_file_name(&self.man_exe)).filter(|n| !n.is_empty());
        let g = Game {
            name: self.man_name.trim().to_string(),
            path: PathBuf::from(self.man_path.trim()),
            exe: exe_name,
            save_is_file: self.man_is_file,
            save_filter: Some(self.man_filter.trim().to_string()).filter(|f| !f.is_empty() && !self.man_is_file),
            extra_locations: self.man_extras.iter().filter(|l| !l.path.as_os_str().is_empty()).cloned().collect(),
            schedule: self.man_sched.take().map(|s| s.normalized()),
            ..Default::default()
        };
        let ep = if exe_full.is_absolute() { Some(exe_full) } else { None };
        // Emplacement trouve par la connaissance intelligente et accepte : file locale (le nom n'y va que s'il n'a pas ete
        // tape a la main).
        if !full_key.is_empty() && self.man_found_key == full_key {
            let n = self.man_name.trim();
            let typed = !(n == self.man_name_auto.trim() || self.man_prog.as_ref().map(|p| p.name == n || p.names().iter().any(|x| x == n)).unwrap_or(false));
            self.accept_known(self.man_prog.as_ref(), &self.man_found, std::path::Path::new(self.man_path.trim()), typed);
        }
        self.add_src = ("manual", None);
        self.begin_add(g, ep, ctx);
        self.add_clear_form();
    }

    // ------------------------------------------------------------ alertes de doublon

    pub(super) fn add_alerts(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.similar_alert(ui, ctx);
        self.overlap_alert(ui, ctx);
    }

    /// Ajout d'un element dont le nom RESSEMBLE a un element deja present dans le coffre (sans avoir la meme cle) : rien
    /// n'est ajoute tant que l'utilisateur n'a pas choisi (utiliser l'element existant / garder son nom).
    fn similar_alert(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(p) = self.pending_add.take() else { return };
        let tk = u::tokens(ctx);
        let mut decided: Option<Game> = None;
        crate::gui::ui::card::card_frame(&tk, true).show(ui, |ui| {
            ui.label(RichText::new(self.tm(K::VAddSimilar).replace("{}", &p.similar[0].name)).size(u::tokens::FS_BODY).strong().color(u::col(tk.text)));
            ui.add_space(u::tokens::SP_S);
            ui.horizontal_wrapped(|ui| {
                for dg in &p.similar {
                    let r = u::button(ui, Btn::Primary, &self.t(K::BtnUseName).replace("{}", &dg.name));
                    if self.deco(ui, r, None).clicked() {
                        let mut g = p.game.clone();
                        g.name = dg.name.clone();
                        decided = Some(g);
                    }
                }
                let r = u::button(ui, Btn::Secondary, self.t(K::BtnKeepName));
                if self.deco(ui, r, None).clicked() {
                    decided = Some(p.game.clone());
                }
            });
        });
        match decided {
            Some(g) => self.add_games_async(vec![(g, p.exe_path.clone())], ctx),
            None => self.pending_add = Some(p),
        }
    }

    /// Un autre element utilise deja ces fichiers : avertissement, au choix continuer ou annuler (aucune fusion automatique).
    fn overlap_alert(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some((g, ep, names)) = self.pending_overlap.take() else { return };
        let tk = u::tokens(ctx);
        let mut go = false;
        let mut cancel = false;
        crate::gui::ui::card::card_frame(&tk, true).show(ui, |ui| {
            ui.label(RichText::new(self.tm(K::VAddOverlapTitle)).size(u::tokens::FS_BODY + 1.0).strong().color(u::col(tk.orange)));
            ui.label(RichText::new(self.tm(K::VAddOverlapBody).replace("{}", &names)).size(u::tokens::FS_BODY).color(u::col(tk.text)));
            ui.add_space(u::tokens::SP_S);
            ui.horizontal(|ui| {
                let r = u::button(ui, Btn::Secondary, self.t(K::BtnCancel));
                if self.deco(ui, r, None).clicked() {
                    cancel = true;
                }
                let r = u::button(ui, Btn::Primary, self.t(K::BtnOverlapContinue));
                if self.deco(ui, r, None).clicked() {
                    go = true;
                }
            });
        });
        if go {
            self.begin_add_after_overlap(g, ep, ctx);
        } else if !cancel {
            self.pending_overlap = Some((g, ep, names));
        } else {
            self.add_keep = None;
        }
    }

    /// Ajout d'un element. Si son nom RESSEMBLE a un element deja present dans le coffre (sans avoir la meme cle), rien
    /// n'est ajoute : l'utilisateur choisit (utiliser l'element existant / garder son nom).
    pub(super) fn begin_add(&mut self, g: Game, ep: Option<PathBuf>, ctx: &egui::Context) {
        // Alerte poids (2.0.1) : la taille est mesuree en fond ; un element lourd demande un choix avant d'etre ajoute.
        self.weight_gate_game(ctx, g, ep);
    }

    /// Suite de `begin_add` une fois le poids verifie (ou choisi par l'utilisateur).
    pub(super) fn begin_add_checked(&mut self, g: Game, ep: Option<PathBuf>, ctx: &egui::Context) {
        let ov = actions::overlapping_games(&self.model.config, &g);
        if !ov.is_empty() {
            self.pending_overlap = Some((g, ep, ov.join(", ")));
            return;
        }
        self.begin_add_after_overlap(g, ep, ctx);
    }

    pub(super) fn begin_add_after_overlap(&mut self, g: Game, ep: Option<PathBuf>, ctx: &egui::Context) {
        let sim: Vec<cloud::DestGame> = cloud::similar_dest_games(&g.name, &self.dest_games).into_iter().map(|x| x.0).collect();
        if sim.is_empty() {
            self.add_games_async(vec![(g, ep)], ctx);
        } else {
            self.pending_add = Some(PendingAdd { game: g, exe_path: ep, similar: sim });
        }
    }

    /// Changement d'onglet vers « Ajouter » : tout est remis a zero (formulaire, propositions, etats transitoires, selection
    /// manette / focus).
    pub(super) fn reset_add_page(&mut self, ctx: &egui::Context) {
        self.add_clear_form();
        self.hint_ignored.clear();
        self.auto_filled = Default::default();
        self.add_page.reset();
        self.pending_add = None;
        self.pending_overlap = None;
        self.add_keep = None;
        self.focus.reset();
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
}

/// Petite icone en ligne (coche) a la hauteur du texte.
fn tiny_glyph(ui: &mut egui::Ui, g: Glyph, c: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    u::paint_glyph(ui.painter(), r, g, c);
}

impl App {
    /// Mode expert : bouton « Ajouter » (vocabulaire technique d'origine).
    fn add_label_expert(&self) -> &'static str {
        self.t(K::BtnAdd)
    }

    /// Mode expert : explication de la planification obligatoire d'un element sans programme.
    fn schedule_need_expert(&self) -> &'static str {
        self.t(K::SchedNeed)
    }
}
