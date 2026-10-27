//! Fiche d'un element (2.0, lot 7) et ecran « Remonter le temps ».
//! Fiche : en-tete (icone, nom, etat, synchro par appareil), actions principales (Synchroniser maintenant, Remonter le
//! temps, Pause), conflit en mots simples, « Ce qui est protege » (ouvrir, ajouter, retirer avec confirmation), Options
//! repliees, Reglages avances (mode expert : toutes les fonctions de l'ancienne fiche), « Ne plus proteger » isole.
//! Remonter le temps : frise par jour (un point par version, couleur de l'appareil, heure), branches de conflit et
//! copies de securite expliquees, apercu en mots simples, « Remettre cette version » (jamais programme ouvert, copie de
//! securite avant) puis toast « Annuler » qui remet l'etat d'avant. Logique pure et testee : `crate::timetravel`.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph, State, TileIcon};
use crate::home::Kind;
use crate::timetravel::{self as tt, Block, DayLabel, DevState, Explain, Place, PointKey, PointKind};

/// Ecran ouvert pour l'element choisi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Fiche,
    TimeTravel,
}

/// Confirmation ouverte depuis la fiche (dialogue 2.0 avec « Annuler »).
#[derive(Clone, Debug)]
pub enum Ask {
    Restore { key: PointKey, when: String },
    AddPlace { path: PathBuf, is_file: bool, overlap: Option<String> },
    RemovePlace { id: String, label: String },
    Stop,
}

#[derive(Default)]
pub struct FicheUi {
    pub for_name: Option<String>,
    pub screen: Screen,
    pub options_open: bool,
    pub list_view: bool,
    /// Point choisi sur la frise (garde quand la liste est relue).
    pub sel_key: Option<PointKey>,
    sel_prev: usize,
    /// Apercus calcules (None = impossible) et calcul en cours.
    pub previews: HashMap<PointKey, Option<crate::preview::Preview>>,
    preview_inflight: Option<PointKey>,
    pub safety: Vec<backup::SafetyCopy>,
    pub safety_for: Option<String>,
    pub ask: Option<Ask>,
    /// Restauration ou annulation en cours (une seule a la fois).
    pub busy: bool,
    /// Toast « Annuler » affiche : (identifiant du toast, de quoi annuler).
    pub undo: Option<(u64, backup::UndoTicket)>,
    /// Le premier element de l'ecran prend le focus clavier a l'ouverture.
    pub focus_pending: bool,
    /// Diagnostic des captures : `AGSS_UI_FICHE_DEMO=tt,sel:3,ask,restore,undo,stop,options,list,add`.
    demo: Vec<String>,
    demo_read: bool,
    demo_at: f64,
    demo_scroll: Option<f32>,
}

impl FicheUi {
    /// Nouvelle fiche : tout est replie, la selection repart de « ce que tu as ».
    pub fn reset(&mut self, name: &str) {
        let busy = self.busy;
        let undo = self.undo.take();
        let demo = std::mem::take(&mut self.demo);
        let (demo_read, demo_at) = (self.demo_read, self.demo_at);
        *self = FicheUi { for_name: Some(name.to_string()), busy, undo, demo, demo_read, demo_at, ..Default::default() };
    }
}

/// L'ecran a ete ouvert au clavier (Entree, Espace, Echap) : son premier element prend le focus (cerclage visible) ;
/// a la souris, rien n'est cercle.
pub(super) fn opened_by_keyboard(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Escape))
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

impl App {
    // ------------------------------------------------------------ textes

    fn day_text(&self, day: i64, today: i64) -> String {
        match tt::day_label(day, today) {
            DayLabel::Today => self.t(K::VToday).to_string(),
            DayLabel::Yesterday => self.t(K::VYesterday).to_string(),
            DayLabel::Weekday { weekday, dom } => {
                let names: Vec<&str> = self.t(K::VWeekdays).split(',').collect();
                self.t(K::VDayWeekday).replace("{0}", names.get(weekday as usize).copied().unwrap_or("")).replace("{1}", &dom.to_string())
            }
            DayLabel::Date { dom, month, year: None } => self.t(K::VDayDate).replace("{0}", &format!("{:02}", dom)).replace("{1}", &format!("{:02}", month)),
            DayLabel::Date { dom, month, year: Some(y) } => self.t(K::VDayDateYear).replace("{0}", &format!("{:02}", dom)).replace("{1}", &format!("{:02}", month)).replace("{2}", &y.to_string()),
        }
    }

    /// « Hier à 21:14 ».
    fn when_text(&self, p: &tt::Point, today: i64) -> String {
        let s = self.t(K::VAtTime).replace("{0}", &self.day_text(p.day, today)).replace("{1}", &format!("{:02}:{:02}", p.hour, p.minute));
        // en debut de titre : « Vendredi 23 à 19:05 »
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => s,
        }
    }

    fn today(&self) -> i64 {
        let now = crate::util::now_ms();
        tt::local_day(now, crate::timefmt::TzProvider::utc_offset_secs(&crate::timefmt::WinTz, now))
    }

    fn dev_text(&self, s: DevState) -> &'static str {
        self.t(match s {
            DevState::UpToDate => K::VDevUpToDate,
            DevState::Behind => K::VDevBehind,
            DevState::Newer => K::VDevNewer,
            DevState::NotCopied => K::VDevNotCopied,
            DevState::Conflict => K::VDevConflict,
            DevState::Unknown => K::VDevUnknown,
        })
    }

    pub(super) fn place_text(&self, p: Place) -> &'static str {
        self.t(match p {
            Place::AppData => K::VPlaceAppData,
            Place::Documents => K::VPlaceDocuments,
            Place::SavedGames => K::VPlaceSavedGames,
            Place::Elsewhere => K::VPlaceElsewhere,
        })
    }

    /// Toast de fin d'une action de la fiche (ancienne fiche : ligne de message) ; detail technique en mode expert.
    pub(super) fn fiche_toast_done(&mut self, ctx: &egui::Context, text: &str, ok: bool) {
        let expert = self.model.settings.expert_mode;
        let (title, st) = if ok { (self.t(K::VDone), State::Safe) } else { (self.t(K::VProblem), State::Problem) };
        let detail = if expert || !ok { text.to_string() } else { String::new() };
        self.toasts.push(u::anim::now(ctx), title, &detail, st, None);
    }

    // ------------------------------------------------------------ page

    /// Fiche ou Remonter le temps de l'element `name`.
    pub(super) fn fiche_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        if self.fiche.for_name.as_deref() != Some(name) {
            self.fiche.reset(name);
        }
        if self.model.game(name).is_none() {
            // element retire entre-temps
            self.fiche_close(name);
            return;
        }
        self.fiche_demo(ctx, name);
        let dialog_open = self.model.confirm.is_some() || self.fiche.ask.is_some() || self.home.remove_ask.is_some() || self.home.menu.is_some();
        let esc = !dialog_open && ui.input(|i| i.key_pressed(egui::Key::Escape));
        match self.fiche.screen {
            Screen::Fiche => {
                if esc {
                    self.fiche_close(name);
                    return;
                }
                self.fiche_screen(ui, ctx, name);
            }
            Screen::TimeTravel => {
                if esc {
                    self.fiche.screen = Screen::Fiche;
                    self.fiche.focus_pending = opened_by_keyboard(ctx);
                    return;
                }
                self.time_travel_screen(ui, ctx, name);
            }
        }
    }

    /// Diagnostic des captures (jamais dans l'interface) : actions appliquees une fois, quand l'historique est charge.
    fn fiche_demo(&mut self, ctx: &egui::Context, name: &str) {
        if !self.fiche.demo_read {
            self.fiche.demo_read = true;
            self.fiche.demo = std::env::var("AGSS_UI_FICHE_DEMO").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        }
        if self.fiche.demo.is_empty() || self.history.is_none() {
            return;
        }
        let now = u::anim::now(ctx);
        if now < self.fiche.demo_at {
            u::anim::wake_at(ctx, self.fiche.demo_at + 0.01);
            return;
        }
        let step = self.fiche.demo.remove(0);
        // une etape toutes les 1,5 s (le resultat d'une restauration a le temps d'arriver)
        self.fiche.demo_at = now + 1.5;
        u::anim::wake_at(ctx, self.fiche.demo_at + 0.01);
        match step.split_once(':') {
            Some(("down", n)) => self.fiche.demo_scroll = n.parse::<f32>().ok(),
            Some(("sel", n)) => {
                if let Ok(i) = n.parse::<usize>() {
                    let pts = self.fiche_points(name);
                    if let Some(p) = pts.get(i) {
                        self.fiche.sel_key = Some(p.key.clone());
                    }
                }
            }
            _ => match step.as_str() {
                "tt" => self.fiche.screen = Screen::TimeTravel,
                "list" => self.fiche.list_view = true,
                "options" => self.fiche.options_open = true,
                "stop" => self.fiche.ask = Some(Ask::Stop),
                "ask" | "restore" => {
                    let pts = self.fiche_points(name);
                    let i = self.fiche_sel_index(&pts);
                    if let Some(p) = pts.get(i) {
                        let when = self.when_text(p, self.today());
                        if step == "ask" {
                            self.fiche.ask = Some(Ask::Restore { key: p.key.clone(), when });
                        } else {
                            self.fiche_start_restore(ctx, name, p.key.clone(), when);
                        }
                    }
                }
                "undo" => match self.fiche.undo.clone() {
                    Some((id, _)) => self.fiche_undo(ctx, id),
                    // restauration pas encore terminee : on reessaie un peu plus tard
                    None if self.fiche.busy => {
                        self.fiche.demo.insert(0, step.clone());
                        self.fiche.demo_at = now + 0.3;
                    }
                    None => {}
                },
                _ => {}
            },
        }
    }

    // ------------------------------------------------------------ fiche

    fn fiche_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        let Some(g) = self.model.game(name).cloned() else { return };
        let t = u::tokens(ctx);
        let big = self.big_now;
        let expert = self.model.settings.expert_mode;
        let busy_list = crate::perf::busy_games();
        let item = self.home_items(ctx, &busy_list).into_iter().find(|i| i.name == name);
        let status = self.model.statuses.get(name).cloned().unwrap_or(GameStatus::Never);
        let me = self.model.ctx.machine.clone();
        let reachable = self.dest_ok != Some(false) && self.model.dest().is_some();
        let facts = self.home.facts.iter().find(|f| f.name == name).cloned().unwrap_or_else(|| crate::home::Facts::bare(&g, if g.exe.as_deref().map(str::trim).unwrap_or("").is_empty() { Kind::Folder } else { Kind::Game }, status.clone()));
        let kind = item.as_ref().map(|i| i.kind).unwrap_or(facts.kind);
        let conflict_heads = self.history.as_ref().map(|h| h.conflict_heads.clone()).unwrap_or_default();
        let conflict_devices: Vec<String> = conflict_heads.iter().filter_map(|d| crate::machinecolor::machine_of_dirname(d).map(|s| s.to_string())).collect();
        let mut back = false;
        let mut go_tt = false;
        let focus_first = std::mem::take(&mut self.fiche.focus_pending);

        // ---- retour
        ui.horizontal(|ui| {
            let r = u::button_with(ui, Btn::Ghost, self.t(K::VNavHome), Some(Glyph::ChevronLeft), big);
            if self.deco(ui, r, None).clicked() {
                back = true;
            }
        });
        ui.add_space(u::tokens::SP_S);
        let mut area = egui::ScrollArea::vertical().id_salt(("fiche", name)).auto_shrink([false, false]);
        if let Some(y) = self.fiche.demo_scroll.take() {
            area = area.vertical_scroll_offset(y);
        }
        area.show(ui, |ui| {
            ui.set_max_width((ui.available_width() - 14.0).max(160.0).min(1100.0));
            let mut row = 0usize;
            let epoch = self.home.epoch;
            // ---- en-tete
            u::cascade_in(ui, ("fiche_head", name), row, epoch, |ui| {
                u::card(ui, None, |ui| {
                    ui.horizontal(|ui| {
                        let s = if big { 64.0 } else { 56.0 };
                        let (ir, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                        let tex = if kind == Kind::Folder { None } else { self.icon_for(ctx, name) };
                        let shown = g.shown().to_string();
                        match tex {
                            Some(tex) => {
                                ui.painter().image(tex.id(), ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            None if kind == Kind::Folder => {
                                ui.painter().rect_filled(ir, 10.0, u::col([0x2f, 0x4a, 0x3b]));
                                u::paint_glyph(ui.painter(), ir.shrink(s * 0.22), Glyph::Folder, u::col([0xe9, 0xee, 0xf5]));
                            }
                            None => {
                                let c = crate::home::initial_color(&shown);
                                ui.painter().rect_filled(ir, 10.0, u::col(c));
                                let fg = crate::theme::text_on(c);
                                let gl = ui.painter().layout_no_wrap(u::tile::initials(&shown), u::font(s * 0.4), u::col(fg));
                                ui.painter().galley(ir.center() - gl.size() / 2.0, gl, u::col(fg));
                            }
                        }
                        ui.add_space(u::tokens::SP_M);
                        ui.vertical(|ui| {
                            let w = (ui.available_width() * 0.55).max(160.0);
                            let gname = u::layout(ui, &shown, if big { u::tokens::FS_TITLE * 1.1 } else { u::tokens::FS_TITLE }, u::col(t.text), w, 2);
                            let (r, _) = ui.allocate_exact_size(gname.size(), egui::Sense::hover());
                            ui.painter().galley(r.min, gname, u::col(t.text));
                            ui.add_space(u::tokens::SP_XS);
                            if let Some(it) = &item {
                                let p = u::state_pill(ui, ("fiche_state", name), it.state, &it.status);
                                self.tip(p, K::TipStatus, false);
                            }
                            let now = crate::util::now_ms();
                            let last = match facts.last_sync_ms {
                                Some(ms) => self.t(K::VLastCopyAgo).replace("{}", &self.ago_text(now, ms)),
                                None => self.t(K::VNotCopiedYet).to_string(),
                            };
                            ui.label(RichText::new(last).color(u::col(t.weak)));
                        });
                        // synchro par appareil (a droite)
                        let dev_w = if big { 380.0 } else { 320.0 };
                        ui.add_space((ui.available_width() - dev_w).max(0.0));
                        {
                            ui.vertical(|ui| {
                                ui.set_width(dev_w.min(ui.available_width()));
                                let g2 = u::layout_line(ui, self.t(K::VOnMyDevices), u::tokens::FS_SMALL + 1.0, u::col(t.weak));
                                let (r, _) = ui.allocate_exact_size(g2.size(), egui::Sense::hover());
                                ui.painter().galley(r.min, g2, u::col(t.weak));
                                let now = crate::util::now_ms();
                                for d in tt::device_rows(&facts, &me, reachable, &conflict_devices) {
                                    ui.horizontal(|ui| {
                                        u::device_pill(ui, &d.name, self.machine_rgb(&d.name));
                                        let st_col = match d.state {
                                            DevState::UpToDate => t.state_text(State::Safe),
                                            DevState::Behind | DevState::NotCopied => t.state_text(State::Todo),
                                            DevState::Newer => t.state_text(State::Incoming),
                                            DevState::Conflict => t.state_text(State::Problem),
                                            DevState::Unknown => t.weak,
                                        };
                                        let mut txt = self.dev_text(d.state).to_string();
                                        if let Some(ms) = d.last_ms {
                                            txt = format!("{} · {}", txt, self.ago_text(now, ms));
                                        }
                                        ui.label(RichText::new(txt).color(u::col(st_col)));
                                        if d.here {
                                            ui.label(RichText::new(format!("({})", self.t(K::VThisDevice))).color(u::col(t.faint)).small());
                                        }
                                    });
                                }
                            });
                        }
                    });
                });
            });
            row += 1;
            ui.add_space(u::tokens::SP_M);

            // ---- actions principales
            let paused = self.model.settings.is_paused(name);
            let can_sync = !paused && self.model.dest().is_some();
            u::cascade_in(ui, ("fiche_actions", name), row, epoch, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = u::tokens::SP_S;
                    let r = ui.add_enabled_ui(can_sync, |ui| u::button_with(ui, Btn::Primary, self.t(K::VSyncNow), None, big)).inner;
                    if focus_first {
                        r.request_focus();
                    }
                    let tip = if expert { Some(K::TipSyncNowGame) } else { None };
                    if self.deco(ui, r, tip).clicked() {
                        self.home_sync(ctx, Some(name.to_string()));
                    }
                    let r = u::button_with(ui, Btn::Secondary, self.t(K::VTimeTravel), Some(Glyph::Clock), big);
                    if self.deco(ui, r, None).clicked() {
                        go_tt = true;
                    }
                    let r = u::button_with(ui, Btn::Secondary, self.t(if paused { K::VResume } else { K::VPause }), None, big);
                    if self.deco(ui, r, None).clicked() {
                        self.toggle_pause(ctx, name);
                    }
                });
            });
            row += 1;
            ui.add_space(u::tokens::SP_M);

            // ---- alerte poids (2.0.1) : taille, impact dans le coffre, trois choix
            self.fiche_heavy_block(ui, ctx, name);
            // ---- Reglages Windows (2.0.2) : ce qui est garde, quand ca prend effet, bilan, fichier hosts en attente
            self.fiche_prefs_block(ui, ctx, name);

            // ---- conflit en mots simples (les deux versions restent gardees)
            if !conflict_heads.is_empty() {
                let free = !status.blocks_actions() && self.model.dest().is_some();
                let mut act: Option<Confirm> = None;
                u::cascade_in(ui, ("fiche_conflict", name), row, epoch, |ui| {
                    u::card::card_frame(&t, false).stroke(egui::Stroke::new(1.5_f32, u::col(t.orange))).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            u::state_icon(ui, ("fiche_conf", name), State::Problem, 24.0, None);
                            ui.label(RichText::new(self.tm(K::VConflict)).strong().color(u::col(t.state_text(State::Todo))));
                        });
                        ui.label(RichText::new(self.t(K::VConflictExplain)).color(u::col(t.weak)));
                        let other = conflict_devices.iter().find(|m| !m.eq_ignore_ascii_case(&me)).cloned();
                        ui.add_space(u::tokens::SP_S);
                        ui.horizontal_wrapped(|ui| {
                            let r = ui.add_enabled_ui(free, |ui| u::button_with(ui, Btn::Secondary, self.t(K::VKeepThisDevice), None, big)).inner;
                            if self.deco(ui, r, if expert { Some(K::TipResolveMine) } else { None }).clicked() {
                                act = Some(Confirm::Resolve { game: name.to_string(), keep_mine: true });
                            }
                            let label = match &other {
                                Some(m) => self.tf(K::VTakeOther, m),
                                None => self.t(K::VTakeOtherVersion).to_string(),
                            };
                            let r = ui.add_enabled_ui(free, |ui| u::button_with(ui, Btn::Secondary, &label, None, big)).inner;
                            if self.deco(ui, r, if expert { Some(K::TipResolveCloud) } else { None }).clicked() {
                                act = Some(Confirm::Resolve { game: name.to_string(), keep_mine: false });
                            }
                        });
                    });
                });
                if let Some(a) = act {
                    self.request_with_preview(a, ctx);
                }
                row += 1;
                ui.add_space(u::tokens::SP_M);
            }

            // ---- ce qui est protege
            u::cascade_in(ui, ("fiche_places", name), row, epoch, |ui| {
                u::card(ui, Some(self.t(K::VWhatsProtected)), |ui| {
                    self.places_list(ui, ctx, &g, kind);
                });
            });
            row += 1;
            ui.add_space(u::tokens::SP_M);

            // ---- options (repliees)
            u::cascade_in(ui, ("fiche_options", name), row, epoch, |ui| {
                u::card(ui, None, |ui| {
                    if self.fold_header(ui, ("fold_options", name), self.t(K::VOptions), self.t(K::VOptionsHint), self.fiche.options_open) {
                        self.fiche.options_open = !self.fiche.options_open;
                    }
                    if self.fiche.options_open {
                        ui.add_space(u::tokens::SP_S);
                        self.options_body(ui, ctx, name);
                    }
                });
            });
            row += 1;

            // ---- mode expert : reglages avances (toutes les fonctions de l'ancienne fiche)
            if expert {
                ui.add_space(u::tokens::SP_M);
                u::cascade_in(ui, ("fiche_expert", name), row, epoch, |ui| {
                    u::card(ui, Some(self.t(K::VExpertSection)), |ui| {
                        ui.label(RichText::new(self.t(K::VExpertHint)).color(u::col(t.weak)).small());
                        ui.add_space(u::tokens::SP_S);
                        self.expert_block(ui, ctx, name);
                        ui.add_space(u::tokens::SP_S);
                        let r = u::button(ui, Btn::Secondary, self.t(K::VSaveOptions));
                        if self.deco(ui, r, Some(K::TipApplyGame)).clicked() {
                            self.save_item_options(ctx, name);
                        }
                    });
                });
            }

            // ---- ne plus proteger (isole, en bas)
            ui.add_space(u::tokens::SP_XL * 1.5);
            ui.separator();
            ui.add_space(u::tokens::SP_M);
            let r = u::button_with(ui, Btn::Danger, &format!("{}…", self.tm(K::VStopProtecting)), None, false);
            if self.deco(ui, r, if expert { Some(K::TipRemove) } else { None }).clicked() {
                self.fiche.ask = Some(Ask::Stop);
            }
            ui.add_space(u::tokens::SP_XL);
        });
        if go_tt {
            self.fiche.screen = Screen::TimeTravel;
            self.fiche.focus_pending = opened_by_keyboard(ctx);
            crate::usage::record(&self.shared.data_dir, "time_travel", &[("action", "opened")]);
        }
        if back {
            self.fiche_close(name);
        }
    }

    /// En-tete cliquable d'une section repliee (titre, resume, chevron) ; renvoie true au clic.
    fn fold_header(&self, ui: &mut egui::Ui, id_salt: impl std::hash::Hash, title: &str, hint: &str, open: bool) -> bool {
        let t = u::tokens(ui.ctx());
        let w = ui.available_width();
        let big = self.big_now;
        let h = if big { 52.0 } else { 44.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let resp = ui.interact(rect, ui.id().with(id_salt), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let resp = self.deco(ui, resp, None);
        let p = ui.painter();
        let k = if big { 1.2 } else { 1.0 };
        let g1 = u::layout_line(ui, title, u::tokens::FS_H3 * k, u::col(t.text));
        let g2 = u::layout(ui, hint, u::tokens::FS_SMALL * k, u::col(t.weak), w - 40.0, 1);
        let top = rect.center().y - (g1.size().y + g2.size().y + 2.0) / 2.0;
        let g1h = g1.size().y;
        p.galley(egui::pos2(rect.left(), top), g1, u::col(t.text));
        p.galley(egui::pos2(rect.left(), top + g1h + 2.0), g2, u::col(t.weak));
        let cr = egui::Rect::from_center_size(egui::pos2(rect.right() - 12.0, rect.center().y), egui::vec2(18.0, 18.0));
        u::paint_glyph(p, cr, if open { Glyph::ChevronDown } else { Glyph::ChevronRight }, u::col(t.weak));
        if resp.has_focus() {
            u::focus_ring(ui, rect, u::tokens::R_SM);
        }
        resp.clicked()
    }

    /// Emplacements lisibles : nom, lieu en mots simples (chemin en infobulle, et en clair en mode expert), Ouvrir le
    /// dossier, Retirer (emplacements ajoutes), Ajouter un emplacement.
    fn places_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, g: &Game, kind: Kind) {
        let t = u::tokens(ui.ctx());
        let expert = self.model.settings.expert_mode;
        let big = self.big_now;
        let kf = self.known.clone();
        for l in g.all_locations() {
            let main = l.id == crate::config::MAIN_LOCATION;
            // Reglages Windows : l'export local n'est montre qu'en mode expert (le bloc « Reglages Windows » le decrit).
            if main && !expert && crate::winprefs::kind_of_path(&self.shared.data_dir, &l.path).is_some() {
                continue;
            }
            let title = if !l.label.trim().is_empty() {
                l.label.clone()
            } else if main {
                self.t(match kind {
                    Kind::Game => K::VSavedGames,
                    Kind::App => K::VAppSettings,
                    Kind::Folder => K::VFolderWord,
                })
                .to_string()
            } else {
                self.t(K::VFolderWord).to_string()
            };
            let exists = l.exists();
            let sub = if !l.enabled {
                self.t(K::VPlaceOff).to_string()
            } else if !exists {
                self.t(K::VPlaceMissing).to_string()
            } else {
                format!("{} · {}", tt::short_name(&l.path), self.place_text(tt::place_of(&l.path, &kf)))
            };
            let full = l.path.display().to_string();
            let mut open = false;
            let mut remove = false;
            ui.horizontal(|ui| {
                let s = if big { 34.0 } else { 28.0 };
                let (ir, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                u::paint_glyph(ui.painter(), ir.shrink(3.0), Glyph::Folder, u::col(if l.enabled && exists { t.accent_hover } else { t.faint }));
                ui.add_space(u::tokens::SP_S);
                let text_w = (ui.available_width() - if big { 330.0 } else { 260.0 }).max(120.0);
                let r = ui.vertical(|ui| {
                    ui.set_width(text_w);
                    ui.label(RichText::new(&title).strong().color(u::col(t.text)));
                    ui.label(RichText::new(&sub).color(u::col(t.weak)));
                    if expert {
                        ui.label(RichText::new(&full).small().color(u::col(t.faint)));
                    }
                });
                r.response.on_hover_text(&full);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !main {
                        let r = u::button_with(ui, Btn::Ghost, self.t(K::VRemovePlace), None, big);
                        if self.deco(ui, r, if expert { Some(K::TipLocRemove) } else { None }).clicked() {
                            remove = true;
                        }
                    }
                    let r = ui.add_enabled_ui(exists, |ui| u::button_with(ui, Btn::Ghost, self.t(K::VOpenFolder), Some(Glyph::Folder), big)).inner;
                    if self.deco(ui, r, None).on_hover_text(&full).clicked() {
                        open = true;
                    }
                });
            });
            if open {
                let dir = if l.is_file { l.path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| l.path.clone()) } else { l.path.clone() };
                winutil::open_folder(&dir);
            }
            if remove {
                self.fiche.ask = Some(Ask::RemovePlace { id: l.id.clone(), label: title.clone() });
            }
            ui.add_space(u::tokens::SP_XS);
        }
        ui.add_space(u::tokens::SP_S);
        let r = u::button_with(ui, Btn::Secondary, self.t(K::VAddPlace), Some(Glyph::Plus), big);
        if self.deco(ui, r, if expert { Some(K::TipLocAdd) } else { None }).clicked() {
            let opts = self.pick_opts_for_game(&g.name, false);
            self.open_picker(ctx, "fiche_add", opts);
        }
        if let Some(pk) = self.take_picked("fiche_add") {
            let (p, is_file) = (pk.path, pk.is_file);
            let mut cand = g.clone();
            cand.extra_locations.push(crate::config::Location { id: "new".into(), label: String::new(), path: p.clone(), is_file, filter: None, exclude: None, enabled: true });
            let mut o = actions::overlapping_games(&self.model.config, &cand);
            if actions::own_locations_overlap(&cand) {
                o.push(cand.shown().to_string());
            }
            self.fiche.ask = Some(Ask::AddPlace { path: p, is_file, overlap: if o.is_empty() { None } else { Some(o.join(", ")) } });
        }
    }

    /// Options : programme associe, nom affiche, planification, versions a garder, notifications, Enregistrer.
    fn options_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        let t = u::tokens(ctx);
        let expert = self.model.settings.expert_mode;
        let Some(cur) = self.model.game(name).cloned() else { return };
        let tip = |k: K| if expert { Some(k) } else { None };
        // programme associe
        ui.label(RichText::new(self.t(K::VProgramLinked)).strong());
        ui.label(RichText::new(self.t(K::VProgramLinkedHint)).color(u::col(t.weak)).small());
        let mut exe_txt = std::mem::take(&mut self.edit_exe);
        if self.wide_field(ui, &mut exe_txt, K::TipLocExe, true) {
            #[cfg(windows)]
            if let Some(p) = rfd::FileDialog::new().add_filter("exe", &["exe"]).pick_file() {
                exe_txt = p.display().to_string();
            }
            #[cfg(not(windows))]
            if let Some(p) = Self::program_dialog(self.model.settings.lang).pick_file() {
                if let Some(e) = self.resolve_program(&p) {
                    exe_txt = e.display().to_string();
                }
            }
        }
        self.edit_exe = exe_txt;
        let exe_new = self.edit_exe.trim().to_string();
        if exe_new.is_empty() {
            ui.label(RichText::new(self.t(K::LocUnset)).small().color(u::col(t.weak)));
        } else if std::path::Path::new(&exe_new).is_file() {
            icon_label(ui, Icon::Check, self.t(K::LocOk), u::col(t.state_text(State::Safe)));
        } else {
            icon_label(ui, Icon::Cross, self.t(K::LocMissing), u::col(t.state_text(State::Problem)));
        }
        ui.add_space(u::tokens::SP_M);
        // nom affiche
        ui.label(RichText::new(self.t(K::VShownName)).strong());
        ui.horizontal(|ui| {
            let mut txt = std::mem::take(&mut self.edit_display);
            let w = (ui.available_width() - 200.0).max(160.0);
            let r = ui.add(egui::TextEdit::singleline(&mut txt).desired_width(w));
            self.deco_kind(ui, r, tip(K::TipRenameApply), ItemKind::Text);
            self.edit_display = txt;
            if cur.display_name.is_some() {
                let b = u::button(ui, Btn::Ghost, self.t(K::BtnRenameReset));
                if self.deco(ui, b, tip(K::TipRenameReset)).clicked() {
                    self.edit_display = cur.name.clone();
                    self.run_confirmed(Confirm::Rename { game: name.to_string(), display: None }, ctx);
                }
            }
        });
        ui.add_space(u::tokens::SP_M);
        // planification
        {
            let no_exe = cur.exe.as_deref().map(|e| e.trim().is_empty()).unwrap_or(true) && self.edit_exe.trim().is_empty();
            let mut on = self.edit_sched.is_some();
            if self.toggle(ui, &mut on, self.t(K::VSchedule), K::TipSched).changed() {
                self.edit_sched = if on { Some(crate::schedule::Schedule::fixed(vec![12 * 3600])) } else { None };
            }
            if let Some(mut s) = self.edit_sched.take() {
                self.schedule_editor(ui, &mut s, "edit");
                self.edit_sched = Some(s);
            }
            if no_exe && !self.edit_sched.as_ref().map(|s| s.is_valid()).unwrap_or(false) {
                ui.colored_label(u::col(t.state_text(State::Todo)), self.t(K::SchedNeed));
            }
        }
        ui.add_space(u::tokens::SP_M);
        // versions a garder
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(self.t(K::VKeepVersions)).strong());
            let r = ui.add(egui::DragValue::new(&mut self.edit_keep).range(backup::MIN_KEEP..=200));
            self.deco_kind(ui, r, tip(K::TipKeep), ItemKind::Drag);
        });
        ui.label(RichText::new(self.t(K::VKeepHint)).small().color(u::col(t.weak)));
        ui.add_space(u::tokens::SP_M);
        // notifications
        let (o_def, o_yes, o_no) = (self.t(K::VNotifyDefault), self.t(K::OptYes), self.t(K::OptNo));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(self.t(K::VNotify)).strong());
            let cb = egui::ComboBox::from_id_salt("notif_game")
                .selected_text(match self.edit_notify {
                    1 => o_yes,
                    2 => o_no,
                    _ => o_def,
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.edit_notify, 0, o_def);
                    ui.selectable_value(&mut self.edit_notify, 1, o_yes);
                    ui.selectable_value(&mut self.edit_notify, 2, o_no);
                });
            self.deco(ui, cb.response, tip(K::TipNotifGame));
        });
        ui.add_space(u::tokens::SP_M);
        let r = u::button(ui, Btn::Primary, self.t(K::VSaveOptions));
        if self.deco(ui, r, tip(K::TipApplyGame)).clicked() {
            self.save_item_options(ctx, name);
        }
    }

    // ------------------------------------------------------------ remonter le temps

    /// Points de la frise de l'element (historique charge + copies de securite).
    fn fiche_points(&self, name: &str) -> Vec<tt::Point> {
        let Some(h) = &self.history else { return vec![] };
        if self.history_for.as_deref() != Some(name) {
            return vec![];
        }
        let safety: &[backup::SafetyCopy] = if self.fiche.safety_for.as_deref() == Some(name) { &self.fiche.safety } else { &[] };
        tt::points(h, safety, &self.model.ctx.machine, &crate::timefmt::WinTz)
    }

    fn fiche_sel_index(&self, pts: &[tt::Point]) -> usize {
        self.fiche.sel_key.as_ref().and_then(|k| tt::index_of(pts, k)).unwrap_or_else(|| tt::default_selected(pts))
    }

    fn time_travel_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        let Some(g) = self.model.game(name).cloned() else { return };
        let t = u::tokens(ctx);
        let big = self.big_now;
        let expert = self.model.settings.expert_mode;
        let shown = g.shown().to_string();
        let focus_first = std::mem::take(&mut self.fiche.focus_pending);
        let mut back = false;
        ui.horizontal(|ui| {
            let r = u::button_with(ui, Btn::Ghost, &shown, Some(Glyph::ChevronLeft), big);
            if self.deco(ui, r, None).clicked() {
                back = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.fiche.list_view { self.t(K::VSeeTimeline) } else { self.t(K::VSeeList) };
                let r = u::button(ui, Btn::Ghost, label);
                if self.deco(ui, r, None).clicked() {
                    self.fiche.list_view = !self.fiche.list_view;
                }
            });
        });
        ui.add_space(u::tokens::SP_S);
        ui.horizontal(|ui| {
            let s = if big { 26.0 } else { 22.0 };
            let (ir, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
            u::paint_glyph(ui.painter(), ir, Glyph::Clock, u::col(t.accent_hover));
            let gl = u::layout_line(ui, &self.t(K::VTimeTravelOf).replace("{}", &shown), if big { u::tokens::FS_H2 * 1.2 } else { u::tokens::FS_H2 }, u::col(t.text));
            let (r, _) = ui.allocate_exact_size(gl.size(), egui::Sense::hover());
            ui.painter().galley(r.min, gl, u::col(t.text));
        });
        ui.label(RichText::new(self.t(K::VTimelineHint)).color(u::col(t.weak)));
        ui.add_space(u::tokens::SP_M);

        let pts = self.fiche_points(name);
        egui::ScrollArea::vertical().id_salt(("tt", name)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width((ui.available_width() - 14.0).max(160.0));
            if self.history.is_none() {
                ui.spinner();
                return;
            }
            if pts.is_empty() {
                u::card(ui, None, |ui| {
                    ui.label(RichText::new(self.t(K::VNoVersions)).color(u::col(t.weak)));
                });
                return;
            }
            let today = self.today();
            let mut sel = self.fiche_sel_index(&pts);
            let before = sel;
            let colors = tt::colors(&pts, &|d| self.machine_rgb(d));
            if self.fiche.list_view {
                u::card(ui, None, |ui| {
                    for (i, p) in pts.iter().enumerate().rev() {
                        let title = self.when_text(p, today);
                        let mut sub = self.t(K::VOnDevice).replace("{}", &p.device);
                        match p.kind {
                            PointKind::Safety => sub = format!("{} · {}", self.t(K::VSafetyCopy), self.t(K::VSafetyHere)),
                            PointKind::ConflictOpen | PointKind::BranchKept => sub = format!("{} · {}", sub, self.t(K::VDevConflict)),
                            PointKind::Version if p.current => sub = format!("{} · {}", sub, self.t(K::VLegendNow)),
                            _ => {}
                        }
                        let trailing = self.bytes(p.bytes);
                        let icon = TileIcon::Glyph(if p.kind == PointKind::Safety { Glyph::Shield } else { Glyph::Clock }, colors[i]);
                        let r = u::list_row(ui, ("tt_row", i), &u::ListRow { icon: Some(icon), title: &title, subtitle: &sub, trailing: &trailing, selected: i == sel, appear: Some((pts.len() - 1 - i, self.home.epoch)) });
                        let r = self.deco(ui, r, None);
                        if focus_first && i == sel {
                            r.request_focus();
                        }
                        if r.clicked() {
                            sel = i;
                        }
                    }
                });
            } else {
                let marks: Vec<u::Mark> = pts
                    .iter()
                    .zip(&colors)
                    .map(|(p, c)| u::Mark { conflict: matches!(p.kind, PointKind::ConflictOpen | PointKind::BranchKept), current: p.current, safety: p.kind == PointKind::Safety, label: format!("{:02}:{:02}", p.hour, p.minute), ..u::Mark::new(p.day, *c) })
                    .collect();
                u::card(ui, None, |ui| {
                    let label = |d: i64| self.day_text(d, today);
                    let r = u::timeline(ui, ("frise", name), &marks, &label, &mut sel);
                    let r = self.deco(ui, r, None);
                    if focus_first {
                        r.request_focus();
                    }
                    ui.add_space(u::tokens::SP_S);
                    self.timeline_legend(ui, &pts, &colors);
                });
            }
            if sel != before || self.fiche.sel_key.is_none() {
                self.fiche.sel_key = pts.get(sel).map(|p| p.key.clone());
            }
            let dir = if sel >= self.fiche.sel_prev { 1.0 } else { -1.0 };
            if sel != self.fiche.sel_prev {
                self.fiche.sel_prev = sel;
            }
            ui.add_space(u::tokens::SP_M);
            if let Some(p) = pts.get(sel).cloned() {
                self.fiche_request_preview(ctx, name, &p.key);
                let color = colors[sel];
                u::step_transition(ui, ("vcard", name), sel as u64, dir, |ui| {
                    self.version_card(ui, ctx, name, &shown, &p, color, today, expert);
                });
            }
        });
        if back {
            self.fiche.screen = Screen::Fiche;
            self.fiche.focus_pending = opened_by_keyboard(ctx);
        }
    }

    /// Legende : appareils (couleur), ce que tu as (cercle), modifie en meme temps (losange), copie de securite (carre).
    fn timeline_legend(&self, ui: &mut egui::Ui, pts: &[tt::Point], colors: &[crate::theme::Rgb]) {
        let t = u::tokens(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = u::tokens::SP_S;
            let mut seen: Vec<String> = Vec::new();
            for (p, c) in pts.iter().zip(colors) {
                if !seen.iter().any(|s| s.eq_ignore_ascii_case(&p.device)) {
                    seen.push(p.device.clone());
                    u::device_pill(ui, &p.device, *c);
                }
            }
            ui.add_space(u::tokens::SP_M);
            let sample = |ui: &mut egui::Ui, shape: u8, text: &str| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                let c = r.center();
                let p = ui.painter();
                match shape {
                    0 => {
                        p.circle_filled(c, 4.5, u::col(t.weak));
                        p.circle_stroke(c, 7.0, egui::Stroke::new(2.0_f32, u::col(t.text)));
                    }
                    1 => {
                        let pts = vec![egui::pos2(c.x, c.y - 6.5), egui::pos2(c.x + 6.5, c.y), egui::pos2(c.x, c.y + 6.5), egui::pos2(c.x - 6.5, c.y)];
                        p.add(egui::Shape::convex_polygon(pts, u::col(t.orange), egui::Stroke::NONE));
                    }
                    _ => {
                        p.rect(egui::Rect::from_center_size(c, egui::vec2(11.0, 11.0)), 2.0, u::col(t.panel), egui::Stroke::new(2.0_f32, u::col(t.weak)), egui::StrokeKind::Inside);
                    }
                }
                ui.label(RichText::new(text).small().color(u::col(t.weak)));
            };
            if pts.iter().any(|p| p.current) {
                sample(ui, 0, self.t(K::VLegendNow));
            }
            if pts.iter().any(|p| matches!(p.kind, PointKind::ConflictOpen | PointKind::BranchKept)) {
                sample(ui, 1, self.t(K::VDevConflict));
            }
            if pts.iter().any(|p| p.kind == PointKind::Safety) {
                sample(ui, 2, self.t(K::VSafetyCopy));
            }
        });
    }

    /// Carte du point choisi : quand, quel appareil, explication, taille, difference avec maintenant, « Remettre ».
    #[allow(clippy::too_many_arguments)]
    fn version_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str, shown: &str, p: &tt::Point, color: crate::theme::Rgb, today: i64, expert: bool) {
        let t = u::tokens(ctx);
        let big = self.big_now;
        let when = self.when_text(p, today);
        let status = self.model.statuses.get(name).cloned().unwrap_or(GameStatus::Never);
        let busy = self.fiche.busy || self.op_progress.is_some() || crate::perf::busy_games().iter().any(|b| b == name);
        let block = tt::restore_block(p, &status, busy, self.model.dest().is_some() && self.dest_ok != Some(false));
        let mut ask = false;
        u::card(ui, None, |ui| {
            ui.horizontal_wrapped(|ui| {
                let gl = u::layout_line(ui, &when, if big { u::tokens::FS_H3 * 1.25 } else { u::tokens::FS_H3 + 1.0 }, u::col(t.text));
                let (r, _) = ui.allocate_exact_size(gl.size(), egui::Sense::hover());
                ui.painter().galley(r.min, gl, u::col(t.text));
                ui.add_space(u::tokens::SP_S);
                u::device_pill(ui, &p.device, color);
                if p.device.eq_ignore_ascii_case(&self.model.ctx.machine) {
                    ui.label(RichText::new(self.t(K::VThisDevice)).small().color(u::col(t.faint)));
                }
                if p.kind == PointKind::Safety {
                    ui.label(RichText::new(self.t(K::VSafetyCopy)).strong().color(u::col(t.state_text(State::Incoming))));
                }
            });
            ui.add_space(u::tokens::SP_XS);
            let (ek, ecol) = match tt::explain(p) {
                Explain::Current => (self.t(K::VExplainCurrent).to_string(), t.weak),
                Explain::NewestNotHere => (self.t(K::VExplainNewest).to_string(), t.state_text(State::Incoming)),
                Explain::ConflictOpen => (self.tf(K::VExplainConflict, &p.device),t.state_text(State::Todo)),
                Explain::BranchKept => (self.t(K::VExplainBranch).to_string(), t.weak),
                Explain::Safety => (self.t(K::VExplainSafety).to_string(), t.weak),
                Explain::Older => (self.t(K::VExplainOlder).to_string(), t.weak),
            };
            ui.label(RichText::new(ek).color(u::col(ecol)));
            ui.label(RichText::new(self.tn(p.files as u64, K::VVersionFilesOne, K::VVersionFiles).replace("{0}", &p.files.to_string()).replace("{1}", &self.bytes(p.bytes))).color(u::col(t.weak)));
            ui.add_space(u::tokens::SP_S);
            // difference avec l'etat actuel (preview.rs)
            let pv = self.fiche.previews.get(&p.key).cloned();
            if p.current {
                ui.label(RichText::new(self.t(K::VSameAsNow)).strong().color(u::col(t.state_text(State::Safe))));
            } else {
                match pv {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(self.t(K::VComputingDiff)).color(u::col(t.weak)));
                        });
                    }
                    Some(None) => {}
                    Some(Some(pr)) => {
                        if pr.added == 0 && pr.replaced == 0 && pr.local_only.is_empty() {
                            ui.label(RichText::new(self.t(K::VSameAsNow)).strong().color(u::col(t.state_text(State::Safe))));
                        } else {
                            ui.label(RichText::new(self.diff_now_text(pr.replaced as u64, pr.added as u64)).strong().color(u::col(t.text)));
                        }
                        ui.label(RichText::new(format!("{} · {}", self.tn(pr.identical as u64, K::VDiffIdenticalOne, K::VDiffIdentical).replace("{}", &pr.identical.to_string()), self.t(K::VNothingDeleted))).color(u::col(t.weak)));
                        if pr.local_newer > 0 {
                            ui.label(RichText::new(self.tn(pr.local_newer as u64, K::VLocalNewerWarnOne, K::VLocalNewerWarn).replace("{}", &pr.local_newer.to_string())).color(u::col(t.state_text(State::Todo))));
                        }
                        if expert {
                            ui.add_space(u::tokens::SP_XS);
                            egui::CollapsingHeader::new(self.t(K::PrevTitle)).id_salt(("tt_files", name)).show(ui, |ui| {
                                self.preview_files(ui, &pr, self.model.settings.lang, 180.0);
                            });
                        }
                    }
                }
            }
            if expert {
                let tech = match &p.key {
                    PointKey::Version(n) => n.clone(),
                    PointKey::Safety(d) => d.display().to_string(),
                };
                ui.label(RichText::new(format!("{}{}", tech, p.note.as_ref().map(|n| format!(" · {}", n)).unwrap_or_default())).small().color(u::col(t.faint)));
            }
            ui.add_space(u::tokens::SP_M);
            ui.horizontal_wrapped(|ui| {
                let label = if p.kind == PointKind::Safety { self.t(K::VRestoreSafety) } else { self.tm(K::VRestoreThis) };
                let enabled = block.is_none();
                let r = ui.add_enabled_ui(enabled, |ui| u::button_with(ui, Btn::Primary, label, None, big)).inner;
                if self.deco(ui, r, if expert { Some(K::TipVersionRestore) } else { None }).clicked() {
                    ask = true;
                }
                let why = match block {
                    Some(Block::ProgramOpen) => Some(self.t(K::VBlockProgramOpen).replace("{}", shown)),
                    Some(Block::Busy) => Some(if self.fiche.busy { self.t(K::VRestoring).to_string() } else { self.t(K::VBlockBusy).to_string() }),
                    Some(Block::NoVault) => Some(self.t(K::VBlockNoVault).to_string()),
                    Some(Block::SameAsNow) | None => None,
                };
                if let Some(w) = why {
                    ui.label(RichText::new(w).color(u::col(t.state_text(State::Todo))));
                }
            });
        });
        if ask {
            self.fiche.ask = Some(Ask::Restore { key: p.key.clone(), when });
        }
    }

    /// Apercu d'un point (une fois par point ; recalcule quand l'historique est relu).
    fn fiche_request_preview(&mut self, ctx: &egui::Context, name: &str, key: &PointKey) {
        if self.fiche.previews.contains_key(key) || self.fiche.preview_inflight.as_ref() == Some(key) {
            return;
        }
        let Some(g) = self.model.game(name).cloned() else { return };
        let dest = self.model.dest();
        self.fiche.preview_inflight = Some(key.clone());
        let (tx, c, dd, k) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), key.clone());
        std::thread::spawn(move || {
            let cx = make_ctx(&dd);
            let p = match &k {
                PointKey::Version(n) => dest.as_ref().and_then(|d| crate::preview::for_game(&cx, &g, d, Some(n))),
                PointKey::Safety(d) => crate::preview::for_safety(&cx, &g, d),
            };
            let _ = tx.send(Msg::PointPreview(g.name.clone(), k, p));
            c.request_repaint();
        });
    }

    pub(super) fn fiche_on_preview(&mut self, game: String, key: PointKey, p: Option<crate::preview::Preview>) {
        if self.fiche.preview_inflight.as_ref() == Some(&key) {
            self.fiche.preview_inflight = None;
        }
        if self.fiche.for_name.as_deref() == Some(&game) {
            self.fiche.previews.insert(key, p);
        }
    }

    // ------------------------------------------------------------ remettre / annuler

    /// « Remettre cette version » confirme : restauration dans un thread (memes protections que le moteur), puis toast
    /// « Version remise · Annuler ».
    fn fiche_start_restore(&mut self, ctx: &egui::Context, name: &str, key: PointKey, when: String) {
        let Some(g) = self.model.game(name).cloned() else { return };
        if self.fiche.busy {
            return;
        }
        self.fiche.busy = true;
        let dest = self.model.dest();
        let prog = crate::perf::Progress::new();
        self.op_progress = Some((K::OpRestore, prog.clone()));
        let (tx, c, dd, sh) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), self.shared.clone());
        std::thread::spawn(move || {
            let log = Logger::new(dd.join("logs").join("watch.log"), false);
            let r: Result<backup::UndoTicket, String> = match AppModel::new(make_ctx(&dd)) {
                Ok(mut m) => {
                    m.ctx.progress = Some(prog);
                    match &key {
                        PointKey::Version(v) => match &dest {
                            Some(d) => backup::restore_version_undoable(&m.ctx, &g, d, v).map(|x| x.1).map_err(|e| e.to_string()),
                            None => Err("aucun coffre".into()),
                        },
                        PointKey::Safety(d) => backup::restore_safety_undoable(&m.ctx, &g, d).map(|x| x.1).map_err(|e| e.to_string()),
                    }
                }
                Err(e) => Err(e.to_string()),
            };
            let what = match &key {
                PointKey::Version(v) => format!("version {}", v),
                PointKey::Safety(d) => format!("copie de securite {}", d.display()),
            };
            match &r {
                Ok(t) => {
                    log.log(&format!("[{}] UI : Remonter le temps -> {} remise (copie de securite : {:?})", g.name, what, t.safety_dir));
                    if let PointKey::Version(v) = &key {
                        let age = crate::util::parse_timestamp(v).map(|t| crate::usage::bucket_age(crate::util::now_ms().saturating_sub(t)));
                        crate::usage::record(&dd, "time_travel", &[("action", "restored"), ("age", age.unwrap_or("30d+"))]);
                    }
                }
                Err(e) => log.log(&format!("[{}] ERREUR UI : Remonter le temps -> {} : {}", g.name, what, e)),
            }
            if let Ok(mut t) = sh.tracker.lock() {
                t.on_event(if r.is_ok() { watch::EventKind::BackedUp } else { watch::EventKind::Error }, &g.name, crate::util::now_ms());
            }
            sh.status_dirty.store(true, Ordering::SeqCst);
            let _ = tx.send(Msg::OpEnd);
            let _ = tx.send(Msg::Restored { game: g.name.clone(), label: when, result: r });
            c.request_repaint();
        });
    }

    pub(super) fn fiche_on_restored(&mut self, ctx: &egui::Context, game: String, label: String, result: Result<backup::UndoTicket, String>) {
        self.fiche.busy = false;
        let now = u::anim::now(ctx);
        match result {
            Ok(ticket) => {
                let mut detail = self.tf(K::VRestoredDetail, &lower_first(&label));
                if let Some(p) = self.pref_restored_detail(&game) {
                    detail = format!("{}\n{}", detail, p);
                }
                let id = self.toasts.push(now, self.t(K::VDone), &detail, State::Safe, Some(self.t(K::VUndo)));
                self.fiche.undo = Some((id, ticket));
            }
            Err(e) => {
                let expert = self.model.settings.expert_mode;
                let detail = if e.contains("REFUS") {
                    let shown = self.model.game(&game).map(|g| g.shown().to_string()).unwrap_or_else(|| game.clone());
                    self.t(K::VBlockProgramOpen).replace("{}", &shown)
                } else if expert {
                    e
                } else {
                    String::new()
                };
                self.toasts.push(now, self.t(K::VRestoreFailed), &detail, State::Problem, None);
            }
        }
        self.after_change(ctx);
    }

    /// « Annuler » du toast : l'etat d'avant revient (fichiers, coffre, etat de synchro), dans un thread.
    pub(super) fn fiche_undo(&mut self, ctx: &egui::Context, toast_id: u64) {
        let Some((id, ticket)) = self.fiche.undo.clone() else { return };
        if id != toast_id || self.fiche.busy {
            return;
        }
        self.fiche.undo = None;
        let Some(g) = self.model.game(&ticket.game).cloned() else { return };
        self.fiche.busy = true;
        let dest = self.model.dest();
        let prog = crate::perf::Progress::new();
        self.op_progress = Some((K::OpRestore, prog.clone()));
        let (tx, c, dd, sh) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), self.shared.clone());
        std::thread::spawn(move || {
            let log = Logger::new(dd.join("logs").join("watch.log"), false);
            let r = match AppModel::new(make_ctx(&dd)) {
                Ok(mut m) => {
                    m.ctx.progress = Some(prog);
                    backup::undo_restore(&m.ctx, &g, dest.as_deref(), &ticket).map_err(|e| e.to_string())
                }
                Err(e) => Err(e.to_string()),
            };
            match &r {
                Ok(o) => log.log(&format!(
                    "[{}] UI : restauration annulee ({} fichiers remis, {} retires, {} gardes car modifies ; coffre {} ; copie de securite : {:?})",
                    g.name,
                    o.files_put_back,
                    o.files_removed,
                    o.kept_changed,
                    if o.vault_restored { "remis" } else { o.vault_error.as_deref().unwrap_or("inchange") },
                    o.safety_dir
                )),
                Err(e) => log.log(&format!("[{}] ERREUR UI : annulation impossible : {}", g.name, e)),
            }
            // liste blanche du lot 3 inchangee : une restauration annulee = « cancelled »
            if r.is_ok() {
                crate::usage::record(&dd, "time_travel", &[("action", "cancelled")]);
            }
            sh.status_dirty.store(true, Ordering::SeqCst);
            let _ = tx.send(Msg::OpEnd);
            let _ = tx.send(Msg::Undone { game: g.name.clone(), result: r });
            c.request_repaint();
        });
    }

    pub(super) fn fiche_on_undone(&mut self, ctx: &egui::Context, _game: String, result: Result<backup::UndoOutcome, String>) {
        self.fiche.busy = false;
        let now = u::anim::now(ctx);
        let expert = self.model.settings.expert_mode;
        match result {
            Ok(o) => {
                let mut detail = self.t(K::VUndoneDetail).to_string();
                if expert {
                    if let Some(e) = &o.vault_error {
                        detail = format!("{}\n{}", detail, e);
                    }
                }
                self.toasts.push(now, self.t(K::VUndoneTitle), &detail, State::Safe, None);
            }
            Err(e) => {
                self.toasts.push(now, self.t(K::VUndoFailed), if expert { &e } else { "" }, State::Problem, None);
            }
        }
        self.after_change(ctx);
    }

    // ------------------------------------------------------------ superpositions

    /// Toasts (toutes pages) et confirmations de la fiche ; « Annuler » d'un toast lance l'annulation.
    pub(super) fn fiche_overlays(&mut self, ctx: &egui::Context) {
        // l'annulation n'est possible que tant que son toast est affiche
        if let Some((id, _)) = &self.fiche.undo {
            if self.toasts.current().map(|t| t.id) != Some(*id) {
                self.fiche.undo = None;
            }
        }
        if let Some(u::ToastAction::Undo(id)) = self.toasts.show(ctx) {
            self.fiche_undo(ctx, id);
        }
        let Some(ask) = self.fiche.ask.clone() else { return };
        let Some(name) = self.model.selected.clone().filter(|_| self.model.page == Page::Games) else {
            self.fiche.ask = None;
            return;
        };
        let shown = self.model.game(&name).map(|g| g.shown().to_string()).unwrap_or_else(|| name.clone());
        self.focus.set_modal_scope(true);
        match ask {
            Ask::Restore { key, when } => {
                let safety = matches!(key, PointKey::Safety(_));
                let title = self.tf(if safety { K::VRestoreAskSafety } else { K::VRestoreAskWhen }, &lower_first(&when));
                let mut body = self.t(K::VRestoreAskBody).to_string();
                if let Some(Some(pr)) = self.fiche.previews.get(&key) {
                    body = format!("{}\n\n{}\n{}", body, self.diff_now_text(pr.replaced as u64, pr.added as u64), self.t(K::VNothingDeleted));
                }
                if let Some(n) = self.pref_restore_note(&name) {
                    body = format!("{}\n\n{}", body, n);
                }
                let confirm = if safety { self.t(K::VRestoreSafety) } else { self.tm(K::VRestoreThis) };
                let spec = u::ConfirmSpec { title: &title, body: &body, confirm, cancel: self.t(K::BtnCancel), danger: false };
                match u::confirm_dialog(ctx, ("fiche_restore", &name), &spec) {
                    Some(u::DialogAnswer::Confirm) => {
                        self.fiche.ask = None;
                        self.fiche_start_restore(ctx, &name, key, when);
                    }
                    Some(u::DialogAnswer::Cancel) => self.fiche.ask = None,
                    None => {}
                }
            }
            Ask::AddPlace { path, is_file, overlap } => {
                let title = self.t(K::VAddPlaceTitle).replace("{}", &tt::short_name(&path));
                let mut body = format!("{}\n\n{}", path.display(), self.t(K::VAddPlaceBody));
                if let Some(o) = &overlap {
                    body = format!("{}\n\n{}", body, self.t(K::VAddPlaceOverlap).replace("{}", o));
                }
                let spec = u::ConfirmSpec { title: &title, body: &body, confirm: self.t(K::VAddPlace), cancel: self.t(K::BtnCancel), danger: false };
                match u::confirm_dialog(ctx, ("fiche_add", &name), &spec) {
                    Some(u::DialogAnswer::Confirm) => {
                        self.fiche.ask = None;
                        let label = tt::short_name(&path);
                        let (tx, c, dd, lang, n) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), self.model.settings.lang, name.clone());
                        std::thread::spawn(move || {
                            let log = Logger::new(dd.join("logs").join("watch.log"), false);
                            let r = AppModel::new(make_ctx(&dd)).map_err(|e| e.to_string()).and_then(|m| actions::add_location(&m.ctx, &n, &path, is_file, &label).map_err(|e| e.to_string()));
                            let (title, detail, ok) = match &r {
                                Ok(l) => {
                                    log.log(&format!("[{}] UI : emplacement ajoute ({}, {})", n, l.label, l.path.display()));
                                    (tr(lang, K::VDone).to_string(), String::new(), true)
                                }
                                Err(e) => {
                                    log.log(&format!("[{}] ERREUR UI : ajout d'emplacement impossible : {}", n, e));
                                    (tr(lang, K::VProblem).to_string(), e.clone(), false)
                                }
                            };
                            let _ = tx.send(Msg::FicheDone { title, detail, ok });
                            c.request_repaint();
                        });
                    }
                    Some(u::DialogAnswer::Cancel) => self.fiche.ask = None,
                    None => {}
                }
            }
            Ask::RemovePlace { id, label } => {
                let title = self.t(K::VRemovePlaceTitle).replace("{}", &label);
                let spec = u::ConfirmSpec { title: &title, body: self.t(K::VRemovePlaceBody), confirm: self.t(K::VRemovePlace), cancel: self.t(K::BtnCancel), danger: true };
                match u::confirm_dialog(ctx, ("fiche_remove_place", &name, &id), &spec) {
                    Some(u::DialogAnswer::Confirm) => {
                        self.fiche.ask = None;
                        self.run_confirmed(Confirm::RemoveLocation { game: name.clone(), id, label }, ctx);
                    }
                    Some(u::DialogAnswer::Cancel) => self.fiche.ask = None,
                    None => {}
                }
            }
            Ask::Stop => {
                let title = self.tm(K::VStopTitle).replace("{}", &shown);
                let body = format!("{}\n{}", self.t(K::VStopBody), self.t(K::VStopBodyMore));
                let spec = u::ConfirmSpec { title: &title, body: &body, confirm: self.tm(K::VStopProtecting), cancel: self.t(K::BtnCancel), danger: true };
                match u::confirm_dialog(ctx, ("fiche_stop", &name), &spec) {
                    Some(u::DialogAnswer::Confirm) => {
                        self.fiche.ask = None;
                        self.fiche_close(&name);
                        self.home.focus_name = None;
                        self.run_confirmed(Confirm::Remove { game: name.clone() }, ctx);
                    }
                    Some(u::DialogAnswer::Cancel) => self.fiche.ask = None,
                    None => {}
                }
            }
        }
        self.focus.set_modal_scope(false);
    }
}
