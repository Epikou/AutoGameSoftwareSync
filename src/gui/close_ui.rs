//! 2.0.3 : grand ecran de proposition a la FERMETURE d'un programme pas encore protege (programme inconnu appris pendant son
//! utilisation, ou programme connu detecte), SEULEMENT si quelque chose a ete trouve a proteger. Nom + icone, emplacement
//! trouve + taille, « Protéger », « Choisir un autre emplacement », « Jamais pour celui-ci », « Plus tard » (reproposera a
//! la prochaine fermeture, deux fois au plus, ensuite carte de l'accueil seulement) et un bouton « Fermer » toujours visible.
//! Souris ET manette (boutons inscrits dans la selection, A = choisir, B / Echap = Plus tard). Plein ecran si l'option des
//! Reglages est active ; il existe TOUJOURS une sortie : Echap / B / Fermer, et sortie automatique si la carte disparait
//! (voir `proposal_overlay`).
//!
//! Premier plan : le thread de detection montre et restaure la fenetre (`winutil::bring_to_front` : ShowWindow SW_SHOW +
//! SW_RESTORE puis SetForegroundWindow, rien de plus). Si Windows refuse le premier plan (verrou de focus : l'appli n'a pas
//! recu la derniere saisie), on n'insiste pas : notification cliquable (le clic montre la fenetre, l'ecran est deja pret) et
//! bouton de la barre des taches qui clignote.
use super::*;
use crate::gamedetect::{self, Answer};
use crate::gui::ui::{self as u, Btn, Glyph};
use std::path::Path;

/// Etat de l'ecran de fermeture.
#[derive(Default)]
pub struct CloseUi {
    /// Carte (cle) a montrer en grand.
    pub key: Option<String>,
    /// Plein ecran demande (option des Reglages) ; deja passe en plein ecran.
    full: bool,
    pub(super) entered: bool,
    /// Instant de la demande (ms) : au-dela de `CLOSE_WAIT_MS` sans avoir pu s'afficher, l'ecran est abandonne.
    pub(super) at: u64,
    /// Taille de l'emplacement choisi (mesuree en arriere-plan) : chemin -> octets.
    sizes: Arc<Mutex<HashMap<PathBuf, u64>>>,
    asked: Vec<PathBuf>,
    /// Existence des chemins deja verifiee (aucun acces disque a chaque image : un chemin reseau peut bloquer).
    exists: HashMap<PathBuf, bool>,
    demo_done: bool,
}

impl CloseUi {
    /// `path.exists()` memorise par chemin (invalide par `close_done` et a chaque nouvelle carte).
    pub(super) fn path_exists(&mut self, p: &Path) -> bool {
        if let Some(b) = self.exists.get(p) {
            return *b;
        }
        let b = p.exists();
        if self.exists.len() > 64 {
            self.exists.clear();
        }
        self.exists.insert(p.to_path_buf(), b);
        b
    }

    pub(super) fn forget_paths(&mut self) {
        self.exists.clear();
    }
}

impl App {
    /// Reglage « Ouvrir l'appli en plein ecran a la fermeture d'un programme » (carte Protection automatique).
    pub(super) fn close_setting(&mut self, ui: &mut egui::Ui) {
        ui.add_space(u::tokens::SP_S);
        let mut on = self.model.settings.close_fullscreen;
        let enabled = self.model.settings.detect_enabled;
        let r = ui.add_enabled_ui(enabled, |ui| u::toggle(ui, &mut on, self.t(K::VSetCloseFull))).inner;
        if self.deco(ui, r, None).on_hover_text(self.t(K::VSetCloseFullTip)).changed() {
            self.model.settings.close_fullscreen = on;
            let _ = self.model.save_settings();
            self.pad_log(&format!("[Detection] plein ecran a la fermeture d'un programme {}", if on { "active" } else { "desactive (notification seulement)" }));        }
        let tk = u::tokens(ui.ctx());
        ui.label(RichText::new(self.t(K::VSetCloseFullTip)).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
    }

    /// Diagnostic des captures (jamais dans l'interface) : `AGSS_UI_CLOSE_DEMO=<nom>[:full]` met une carte de demonstration en
    /// tete de file et ouvre l'ecran de fermeture (emplacement trouve si `AGSS_TEST_SAVEPATH` designe un dossier existant).
    pub(super) fn close_demo(&mut self) {
        if self.close.demo_done {
            return;
        }
        self.close.demo_done = true;
        let Ok(v) = std::env::var("AGSS_UI_CLOSE_DEMO") else { return };
        let (name, full) = v.split_once(':').map(|(a, b)| (a.to_string(), b == "full")).unwrap_or((v.clone(), false));
        let exe = format!("{}.exe", gamedetect::alnum(&name));
        let c = gamedetect::Candidate { key: format!("{}|demo", exe), exe, path: None, why: gamedetect::Why::Learned, name, install_dir: None, profile_id: None, app: false, confidence: None };
        if let Ok(mut q) = self.shared.proposals.lock() {
            q.promote(&c, crate::util::now_ms());
        }
        self.close_open(c.key, full, crate::util::now_ms());
    }

    /// Ouvre l'ecran de fermeture pour cette carte (`at` : instant de la demande, ms).
    pub(super) fn close_open(&mut self, key: String, full: bool, at: u64) {
        self.close.key = Some(key);
        self.close.full = full;
        self.close.entered = false;
        self.close.at = at;
        self.close.forget_paths();
        self.prop_key = None; // emplacements re-resolus (appris a la fermeture)
    }

    /// Fin de l'ecran : sortie du plein ecran si on y etait entre.
    pub(super) fn close_done(&mut self, ctx: &egui::Context) {
        if self.close.entered {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        self.close = CloseUi { sizes: self.close.sizes.clone(), demo_done: self.close.demo_done, ..Default::default() };
    }

    fn close_size(&mut self, ctx: &egui::Context, p: &Path) -> Option<u64> {
        if let Some(b) = self.close.sizes.lock().ok().and_then(|m| m.get(p).copied()) {
            return Some(b);
        }
        if !self.close.asked.iter().any(|x| x == p) {
            self.close.asked.push(p.to_path_buf());
            let (sizes, p2, c) = (self.close.sizes.clone(), p.to_path_buf(), ctx.clone());
            std::thread::spawn(move || {
                let b = if p2.is_file() { std::fs::metadata(&p2).map(|m| m.len()).unwrap_or(0) } else { crate::actions::dir_size(&p2) };
                if let Ok(mut m) = sizes.lock() {
                    m.insert(p2, b);
                }
                c.request_repaint();
            });
        }
        None
    }

    /// Dessine l'ecran (au-dessus de toute l'interface, sous les dialogues du selecteur) ; retourne la reponse choisie.
    pub(super) fn close_screen(&mut self, ctx: &egui::Context, cand: &gamedetect::Candidate, icon: Option<&egui::TextureHandle>) -> Option<Answer> {
        let visible = winutil::own_window_visible(APP_TITLE) && !ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if self.close.full && !self.close.entered && visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.close.entered = true;
        }
        let t = u::tokens(ctx);
        let big = self.big_now || self.close.full;
        let expert = self.model.settings.expert_mode;
        let screen = ctx.screen_rect();
        // Emplacements : base / profils (verifies sur ce PC), sinon connaissance locale (appris a la fermeture...).
        let found: Vec<PathBuf> = self.prop_sugg.iter().filter(|s| s.exists).filter_map(|s| s.local_path.clone()).collect();
        if self.prop_path.trim().is_empty() {
            if let Some(f) = found.first().cloned().or_else(|| self.prop_found.first().map(|f| f.path.clone())) {
                self.prop_path = f.display().to_string();
            }
        }
        if let Some(p) = self.take_picked("prop_path") {
            self.prop_path = p.path.display().to_string();
        }
        let chosen = PathBuf::from(self.prop_path.trim());
        let has_place = cand.profile_id.is_some() || (!self.prop_path.trim().is_empty() && self.close.path_exists(&chosen));
        let size = if has_place && cand.profile_id.is_none() { self.close_size(ctx, &chosen) } else { None };
        let name = self.prop_sugg.first().map(|s| s.game_name.clone()).unwrap_or_else(|| cand.name.clone());
        let mut answer: Option<Answer> = None;
        let mut pick = false;
        self.focus.set_modal_scope(true);
        egui::Area::new(egui::Id::new("close_screen")).order(egui::Order::Middle).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            ui.painter().rect_filled(r, 0.0, u::col(t.bg));
            // « Fermer » toujours visible en haut a droite (= Plus tard) : il existe toujours une sortie, meme en plein ecran.
            let close_rect = egui::Rect::from_min_size(egui::pos2(screen.max.x - 150.0, screen.min.y + 14.0), egui::vec2(136.0, 40.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(close_rect).layout(egui::Layout::right_to_left(egui::Align::Min)), |ui| {
                let r = u::button_with(ui, Btn::Ghost, self.t(K::BtnClose), Some(Glyph::Cross), false);
                if self.deco(ui, r, None).on_hover_text(self.t(K::VCloseLaterTip)).clicked() {
                    answer = Some(Answer::Later);
                }
            });
            let w = (screen.width() - 48.0).clamp(280.0, 760.0);
            let col = egui::Rect::from_center_size(screen.center(), egui::vec2(w, screen.height() - 48.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(col).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
                ui.add_space(((screen.height() - 520.0) / 2.0).max(8.0));
                // Grande icone du programme (ou icone generique).
                let (ir, _) = ui.allocate_exact_size(egui::vec2(128.0, 128.0), egui::Sense::hover());
                ui.painter().circle_filled(ir.center(), 72.0, u::col(t.accent).gamma_multiply(0.22));
                if let Some(tx) = icon {
                    ui.painter().image(tx.id(), ir.shrink(8.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                } else {
                    ui.painter().circle_filled(ir.center(), 56.0, u::col(t.accent));
                    super::widgets::paint_pad(ui.painter(), ir.shrink(34.0), Color32::WHITE, Color32::TRANSPARENT);
                }
                ui.add_space(u::tokens::SP_M);
                ui.label(RichText::new(&name).size(u::tokens::FS_TITLE + if big { 6.0 } else { 0.0 }).strong().color(u::col(t.text)));
                ui.add_space(u::tokens::SP_S);
                let title = if has_place { self.t(K::VCloseFoundTitle) } else { self.t(K::VCloseNoneTitle) };
                ui.label(RichText::new(title.replace("{}", &name)).size(u::tokens::FS_H2).color(u::col(t.text)));
                ui.add_space(u::tokens::SP_M);
                // Emplacement trouve (ou choisi) + taille ; sinon explication et selecteur guide.
                egui::Frame::new().fill(u::col(t.card)).corner_radius(u::tokens::R_LG).inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
                    ui.set_width(w - 80.0);
                    if has_place && cand.profile_id.is_none() {
                        let full = chosen.display().to_string();
                        let shown = if expert { full.clone() } else { format!("{} · {}", crate::timetravel::short_name(&chosen), self.place_text(crate::timetravel::place_of(&chosen, &self.known))) };
                        ui.horizontal_wrapped(|ui| {
                            let (gr, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            u::paint_glyph(ui.painter(), gr, Glyph::Check, u::col(t.ok));
                            ui.label(RichText::new(shown).size(u::tokens::FS_BODY + 2.0).color(u::col(t.text))).on_hover_text(&full);
                        });
                        let sz = match size {
                            Some(b) => self.t(K::VCloseSize).replace("{}", &self.bytes(b)),
                            None => self.t(K::VScanMeasuring).to_string(),
                        };
                        ui.label(RichText::new(sz).size(u::tokens::FS_SMALL + 1.0).color(u::col(t.weak)));
                        ui.label(RichText::new(self.t(K::VCloseAsk)).size(u::tokens::FS_SMALL + 1.0).color(u::col(t.weak)));
                    } else if cand.profile_id.is_some() {
                        ui.label(RichText::new(self.t(K::VCloseAsk)).color(u::col(t.weak)));
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            let (gr, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            u::paint_glyph(ui.painter(), gr, Glyph::Folder, u::col(t.orange));
                            ui.label(RichText::new(self.t(K::VCloseNoneBody)).size(u::tokens::FS_BODY + 1.0).color(u::col(t.text)));
                        });
                    }
                });
                ui.add_space(u::tokens::SP_L);
                ui.horizontal_wrapped(|ui| {
                    if has_place {
                        let r = u::button_with(ui, Btn::Primary, self.t(K::VCloseProtect), Some(Glyph::Check), true);
                        self.focus.set_default(r.id);
                        if self.deco(ui, r, None).clicked() {
                            answer = Some(Answer::Yes);
                        }
                        if cand.profile_id.is_none() {
                            let r = u::button_with(ui, Btn::Secondary, self.t(K::VCloseOther), Some(Glyph::Folder), true);
                            if self.deco(ui, r, None).clicked() {
                                pick = true;
                            }
                        }
                    } else {
                        let r = u::button_with(ui, Btn::Primary, self.t(K::VAddChoosePlace), Some(Glyph::Folder), true);
                        self.focus.set_default(r.id);
                        if self.deco(ui, r, None).clicked() {
                            pick = true;
                        }
                    }
                    let r = u::button_with(ui, Btn::Secondary, self.t(K::VCloseNever), None, true);
                    if self.deco(ui, r, None).clicked() {
                        answer = Some(Answer::No);
                    }
                    let r = u::button_with(ui, Btn::Ghost, self.t(K::VCloseLater), None, true);
                    if self.deco(ui, r, None).on_hover_text(self.t(K::VCloseLaterTip)).clicked() {
                        answer = Some(Answer::Later);
                    }
                });
                ui.add_space(u::tokens::SP_M);
                // Indice manette seulement quand une manette est connectee.
                if self.pad_mode {
                    ui.label(RichText::new(self.t(K::VCloseHint)).size(u::tokens::FS_SMALL).color(u::col(t.faint)));
                }
            });
        });
        self.focus.set_modal_scope(false);
        if pick {
            let opts = self.pick_opts_for_program(self.prop_prog.as_ref(), &self.prop_found, &name, false);
            self.open_picker(ctx, "prop_path", opts);
        }
        // Echap / B = Plus tard (sauf si le selecteur guide est ouvert : il se ferme d'abord).
        if answer.is_none() && !pick && self.picker.open.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            answer = Some(Answer::Later);
        }
        if let Some(a) = answer {
            let what = match a {
                Answer::Yes => "proteger",
                Answer::No => "jamais",
                Answer::Later => "plus tard",
            };
            self.pad_log(&format!("[Detection] ecran de fermeture de {} : {}", cand.exe, what));
        }
        answer
    }
}
