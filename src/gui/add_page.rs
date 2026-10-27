//! Page « Ajouter » 2.0 (lot 11) : trois grandes tuiles (« Chercher sur mon PC », « Un programme ou un jeu », « Un dossier ou
//! un fichier »), le formulaire d'ajout en mots simples (`program_form_ui.rs`), « Deja dans ton coffre » en liste propre.
//! L'ancienne carte « Jeux detectes » a disparu : elle faisait doublon avec le scan du PC. Logique pure : `crate::addpage`.
//! Ce fichier ne contient QUE le vocabulaire du mode simple (verifie par test) ; les options techniques sont dans des
//! fonctions `*_expert`.
use super::*;
use crate::addpage::{self, Dropped, Tile};
use crate::gui::ui::{self as u, ChoiceSpec, Glyph, ListRow, TileIcon};

/// Cle du selecteur guide ouvert par la tuile « Un dossier ou un fichier ».
const TARGET_KEY: &str = "add_target";

/// Etat de la page Ajouter entre deux images.
#[derive(Default)]
pub struct AddUi {
    /// Formulaire « element » ouvert (dossier / fichier choisi, ou nom repris de « Deja dans ton coffre »).
    pub open: bool,
    /// Cle d'apparition en cascade des tuiles et des cartes.
    pub epoch: u64,
    seen_frame: u64,
    /// Carte « Connus par l'appli » depliee (mode expert).
    pub profiles_open: bool,
    /// Faire defiler jusqu'au formulaire a sa prochaine image.
    pub scroll_form: bool,
    /// Diagnostic : nombre d ajouts automatiques deja faits (AGSS_DEMO_AUTOADD).
    pub demo_done: u8,
    /// Dossiers connus du systeme, lus une fois (affichage des chemins en miettes).
    pub folders: Option<crate::knowledge::paths::Folders>,
}

impl App {
    /// Programme choisi ou depose : le formulaire « programme » s'ouvre (tout reste de l'element precedent est oublie).
    pub(super) fn add_start_program(&mut self, exe: PathBuf) {
        if self.man_exe.trim().is_empty() {
            self.add_clear_form();
        }
        self.add_ui.open = false;
        self.set_man_exe(exe);
        self.add_ui.scroll_form = true;
    }

    /// Dossier ou fichier choisi, depose ou issu du selecteur guide : le formulaire « element » s'ouvre, prerempli. Sans
    /// programme a surveiller, la copie est planifiee chaque jour a midi.
    pub(super) fn add_start_target(&mut self, path: PathBuf, is_file: bool) {
        self.add_clear_form();
        self.man_path = path.display().to_string();
        self.man_is_file = is_file;
        self.man_name = addpage::target_name(&path, is_file);
        self.man_name_auto = self.man_name.clone();
        self.man_sched = Some(crate::schedule::Schedule::fixed(vec![12 * 3600]));
        self.add_ui.open = true;
        self.add_ui.scroll_form = true;
    }

    /// Remet le formulaire a zero (programme, nom, emplacement, planification, propositions).
    pub(super) fn add_clear_form(&mut self) {
        self.man_exe.clear();
        self.man_name.clear();
        self.man_name_auto.clear();
        self.man_path.clear();
        self.man_path_found = false;
        self.man_is_file = false;
        self.man_filter.clear();
        self.man_extras.clear();
        self.man_filled_key.clear();
        self.man_found.clear();
        self.man_found_key.clear();
        self.man_prog = None;
        self.man_learn_armed = None;
        self.man_sched = None;
        self.man_icon = None;
        self.auto_key.clear();
        self.auto_sugg.clear();
        self.add_ui.open = false;
    }

    /// Fichiers deposes sur la fenetre et resultat du selecteur guide.
    fn add_take_inputs(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(d) = dropped.first() {
            match addpage::classify_drop(d, &|p| self.resolve_program(p)) {
                Dropped::Folder(p) => self.add_start_target(p, false),
                Dropped::File(p) => self.add_start_target(p, true),
                Dropped::Program(e) => self.add_start_program(e),
            }
        }
        if let Some(p) = self.take_picked(TARGET_KEY) {
            self.add_start_target(p.path, p.is_file);
        }
    }

    // ------------------------------------------------------------ page

    pub(super) fn page_add(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Scan du PC (lot 9) : ecran de la page Ajouter.
        if self.scan.open {
            ui::page_transition(ui, "add_screen", 1, |ui| self.scan_screen(ui, ctx));
            return;
        }
        // Changement d'onglet : la page n'a pas ete dessinee a l'image precedente -> apparition rejouee, cartes repliees.
        let frame = ctx.cumulative_pass_nr();
        if self.add_ui.seen_frame == 0 || frame > self.add_ui.seen_frame + 2 {
            self.add_ui.epoch += 1;
            self.add_ui.profiles_open = self.demo_open_all;
        }
        self.add_ui.seen_frame = frame;
        if self.add_ui.folders.is_none() {
            self.add_ui.folders = Some(crate::picker::real_folders());
        }
        self.add_take_inputs(ctx);
        let expert = self.model.settings.expert_mode;
        let mut area = egui::ScrollArea::vertical().id_salt("add").auto_shrink([false, false]);
        if let Some(px) = super::shot::scroll_offset() {
            area = area.vertical_scroll_offset(px);
        }
        area.show(ui, |ui| {
            // Reserve la largeur de la barre de defilement.
            ui.set_max_width((ui.available_width() - 18.0).max(160.0));
            let tk = u::tokens(ctx);
            ui.add_space(u::tokens::SP_S);
            ui.label(RichText::new(self.tm(K::VProtectSomething)).size(u::tokens::FS_TITLE - 4.0).strong().color(u::col(tk.text)));
            ui.label(RichText::new(self.t(K::VAddIntro)).size(u::tokens::FS_BODY).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_M);
            if let Some(t) = self.add_tiles(ui, ctx) {
                match t {
                    Tile::Scan => self.scan_open(ctx),
                    Tile::Program => {
                        if let Some(p) = Self::program_dialog(self.model.settings.lang).pick_file() {
                            if let Some(e) = self.resolve_program(&p) {
                                self.add_start_program(e);
                            }
                        }
                    }
                    Tile::Target => self.open_picker(ctx, TARGET_KEY, crate::gui::picker_ui::PickOpts::default()),
                }
            }
            ui.add_space(u::tokens::SP_L);
            // Formulaire d'ajout (programme ou element), quand il y a quelque chose a ajouter.
            let form_top = ui.cursor().top();
            if addpage::form_kind(&self.man_exe, self.add_ui.open) != addpage::FormKind::Hidden || self.pending_add.is_some() || self.pending_overlap.is_some() {
                if std::mem::take(&mut self.add_ui.scroll_form) {
                    ui.scroll_to_rect(egui::Rect::from_min_size(egui::pos2(ui.min_rect().left(), form_top), egui::vec2(10.0, 60.0)), Some(egui::Align::TOP));
                }
                self.add_form(ui, ctx);
                ui.add_space(u::tokens::SP_L);
            }
            self.add_existing_list(ui);
            if expert {
                self.add_profiles_expert(ui, ctx);
            }
        });
    }

    /// Les trois grandes tuiles ; retourne celle qui vient d'etre cliquee.
    fn add_tiles(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Tile> {
        let w = ui.available_width();
        let cols = addpage::tile_columns(w);
        let gap = u::tokens::TILE_GAP;
        let tw = addpage::tile_width(w, cols, gap);
        let hot = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let epoch = self.add_ui.epoch;
        let horizontal = cols == 1;
        let mut clicked = None;
        let mut draw = |ui: &mut egui::Ui, app: &App, i: usize, tile: Tile| {
            let (glyph, title, hint, drop) = match tile {
                Tile::Scan => (Glyph::Eye, app.t(K::VFindOnPc), app.t(K::VScanPcHint), false),
                Tile::Program => (Glyph::Window, app.t(K::VAddTileProgram), app.t(K::VAddTileProgramHint), true),
                Tile::Target => (Glyph::Folder, app.t(K::VAddTileTarget), app.t(K::VAddTileTargetHint), true),
            };
            let on = hot && drop;
            let spec = ChoiceSpec { glyph, title, hint: if on { app.t(K::VAddDropHot) } else { hint }, hot: on, index: i, epoch, width: tw, horizontal };
            let r = u::choice_tile(ui, ("addtile", i), &spec);
            if app.deco(ui, r, None).clicked() {
                clicked = Some(tile);
            }
        };
        if horizontal {
            for (i, t) in addpage::TILES.iter().enumerate() {
                draw(ui, self, i, *t);
                ui.add_space(gap * 0.6);
            }
        } else {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (i, t) in addpage::TILES.iter().enumerate() {
                    draw(ui, self, i, *t);
                }
            });
        }
        clicked
    }

    /// « Deja dans ton coffre » : les elements protegés depuis d'autres appareils et pas encore ici, en liste propre.
    fn add_existing_list(&mut self, ui: &mut egui::Ui) {
        let todo: Vec<crate::cloud::DestGame> = self.dest_games.iter().filter(|dg| !actions::config_has_key(&self.model.config, &dg.name)).cloned().collect();
        if todo.is_empty() {
            return;
        }
        let tk = u::tokens(ui.ctx());
        let expert = self.model.settings.expert_mode;
        let epoch = self.add_ui.epoch;
        let mut join: Option<String> = None;
        u::card(ui, Some(self.tm(K::VAddExistingTitle)), |ui| {
            ui.label(RichText::new(self.tm(K::VAddExistingHint)).size(u::tokens::FS_SMALL + 1.0).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_S);
            for (i, dg) in todo.iter().enumerate() {
                let sub = if expert { dg.key.clone() } else { String::new() };
                let row = ListRow { icon: Some(TileIcon::Glyph(Glyph::Folder, tk.card2)), title: &dg.name, subtitle: &sub, trailing: self.tm(K::VAddRejoin), selected: false, appear: Some((i, epoch)) };
                let r = u::list_row(ui, ("exist", i), &row);
                if self.deco(ui, r, Some(K::TipJoin)).clicked() {
                    join = Some(dg.name.clone());
                }
            }
        });
        if let Some(n) = join {
            self.add_clear_form();
            self.man_name = n;
            self.add_ui.open = true;
            self.add_ui.scroll_form = true;
            self.man_sched = Some(crate::schedule::Schedule::fixed(vec![12 * 3600]));
        }
    }
}
