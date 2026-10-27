//! Ecran INTERNE de demonstration des composants 2.0 (captures, lots suivants) : `AGSS_UI_GALLERY=1`.
//! Options : `AGSS_UI_GALLERY_THEME=light`, `AGSS_UI_GALLERY_PAGE=0|1|2` (composants, accueil, animations),
//! `AGSS_UI_SLOWMO=8` (animations ralenties, pour capturer des images intermediaires), `AGSS_UI_GALLERY_AMBIENT=0` (aucun
//! etat « en cours »), `AGSS_UI_GALLERY_LOOP=1` (cascade rejouee toutes les 2 s : mesure du CPU pendant les transitions). Les titres de section sont les
//! noms techniques des fonctions (ecran de developpement, non traduit) ; les textes d'exemple sont les cles du mode simple.
use super::*;
use crate::gui::ui::{self as u, anim, Btn, Glyph, State, TileIcon};

pub struct Gallery {
    pub light: bool,
    pub reduced: bool,
    pub ambient: bool,
    pub looping: bool,
    page: usize,
    epoch: u64,
    loop_at: f64,
    step: u64,
    step_dir: f32,
    toggles: [bool; 3],
    chip: usize,
    field: String,
    list_sel: usize,
    tl_sel: usize,
    toasts: u::Toasts,
    dialog: Option<bool>,
    answer: Option<String>,
    flip: bool,
    demo_toast: bool,
}

impl Gallery {
    /// Galerie demandee par l'environnement (`AGSS_UI_GALLERY=1`).
    pub fn from_env() -> Option<Gallery> {
        if std::env::var("AGSS_UI_GALLERY").as_deref() != Ok("1") {
            return None;
        }
        let page = std::env::var("AGSS_UI_GALLERY_PAGE").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(2);
        let demo = std::env::var("AGSS_UI_GALLERY_DEMO").unwrap_or_default();
        let mut g = Gallery {
            light: std::env::var("AGSS_UI_GALLERY_THEME").as_deref() == Ok("light"),
            reduced: false,
            ambient: std::env::var("AGSS_UI_GALLERY_AMBIENT").as_deref() != Ok("0"),
            looping: std::env::var("AGSS_UI_GALLERY_LOOP").as_deref() == Ok("1"),
            page,
            epoch: 1,
            loop_at: 0.0,
            step: 1,
            step_dir: 1.0,
            toggles: [true, true, false],
            chip: 0,
            field: String::new(),
            list_sel: 0,
            tl_sel: 9,
            toasts: u::Toasts::default(),
            dialog: None,
            answer: None,
            flip: false,
            demo_toast: false,
        };
        // Captures : toast ou dialogue ouverts des le demarrage (AGSS_UI_GALLERY_DEMO=toast,dialog,danger).
        g.dialog = if demo.contains("danger") { Some(false) } else if demo.contains("dialog") { Some(true) } else { None };
        g.demo_toast = demo.contains("toast");
        Some(g)
    }
}

/// Titre de section : nom technique en police a chasse fixe.
fn heading(ui: &mut egui::Ui, name: &str) {
    let t = u::tokens(ui.ctx());
    let g = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::monospace(12.0), u::col(t.accent_hover));
    let (r, _) = ui.allocate_exact_size(g.size() + egui::vec2(0.0, 2.0), egui::Sense::hover());
    ui.painter().galley(r.min, g, u::col(t.accent_hover));
    ui.add_space(2.0);
}

/// Petit texte technique (noms des etats, des icones, des jetons).
fn mono(ui: &mut egui::Ui, text: &str, color: Color32) {
    let g = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::monospace(11.0), color);
    let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
    ui.painter().galley(r.min, g, color);
}

impl App {
    /// Theme force par la galerie (None = reglage de l'utilisateur).
    pub(super) fn gallery_dark(&self) -> Option<bool> {
        self.gallery.as_ref().map(|g| !g.light)
    }

    pub(super) fn gallery_motion_reduced(&self) -> bool {
        self.gallery.as_ref().map(|g| g.reduced).unwrap_or(false)
    }

    pub(super) fn gallery_screen(&mut self, ctx: &egui::Context) {
        let Some(mut g) = self.gallery.take() else { return };
        let now = anim::now(ctx);
        if g.demo_toast && now > 1.0 {
            g.demo_toast = false;
            let d = self.t(K::VRestoredDetail).replace("{}", &self.t(K::VYesterdayAt).replace("{}", "21:14"));
            g.toasts.push(now, self.t(K::VDone), &d, State::Safe, Some(self.t(K::VUndo)));
        }
        if g.demo_toast {
            anim::wake_at(ctx, 1.0);
        }
        if g.looping && now - g.loop_at > 2.0 {
            g.loop_at = now;
            g.epoch += 1;
            g.flip = !g.flip;
        }
        if g.looping {
            anim::wake_at(ctx, g.loop_at + 2.0);
        }
        let t = u::tokens(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(u::col(t.panel)).inner_margin(egui::Margin::symmetric(18, 14))).show(ctx, |ui| {
            // barre de controle
            ui.horizontal_wrapped(|ui| {
                heading(ui, "AGSS ui gallery");
                ui.add_space(12.0);
                for (i, name) in ["composants", "accueil", "animations"].iter().enumerate() {
                    if u::chip(ui, name, None, g.page == i).clicked() {
                        g.page = i;
                        g.epoch += 1;
                    }
                }
                ui.add_space(12.0);
                u::toggle(ui, &mut g.light, "light");
                u::toggle(ui, &mut g.reduced, "reduced");
                u::toggle(ui, &mut g.ambient, "ambient");
                u::toggle(ui, &mut g.looping, "loop");
                if u::button(ui, Btn::Secondary, "replay").clicked() {
                    g.epoch += 1;
                    g.flip = !g.flip;
                }
            });
            ui.add_space(10.0);
            let page = g.page;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                u::page_transition(ui, "gallery", (page as u64) << 32 | g.epoch, |ui| match page {
                    0 => self.gallery_components(ui, &mut g),
                    1 => self.gallery_home(ui, &mut g),
                    _ => self.gallery_anims(ui, &mut g),
                });
            });
        });
        if let Some(open) = g.dialog {
            let (title, body, confirm) = if open {
                (self.t(K::VRestoreAsk).to_string(), self.t(K::VRestoreAskBody).to_string(), self.t(K::VRestoreThis))
            } else {
                (self.t(K::VStopTitle).replace("{}", "Celeste"), self.t(K::VStopBody).to_string(), self.t(K::VStopProtecting))
            };
            let spec = u::ConfirmSpec { title: &title, body: &body, confirm, cancel: self.t(K::BtnCancel), danger: !open };
            if let Some(a) = u::confirm_dialog(ctx, "gallery", &spec) {
                g.answer = Some(format!("{:?}", a));
                g.dialog = None;
            }
        }
        if let Some(a) = g.toasts.show(ctx) {
            g.answer = Some(format!("{:?}", a));
        }
        self.gallery = Some(g);
    }

    fn sample_texts(&self) -> [(State, String); 6] {
        let ago = self.t(K::VAgoMin).replace("{}", "5");
        [
            (State::Safe, self.t(K::VSafeAgo).replace("{}", &ago)),
            (State::Busy, self.t(K::VCopyingPct).replace("{}", "64")),
            (State::Todo, self.t(K::VToDo).to_string()),
            (State::Problem, self.t(K::VFileLocked).to_string()),
            (State::Incoming, self.t(K::VNewerFrom).replace("{}", "ALLY")),
            (State::Paused, self.t(K::VPausedInUse).to_string()),
        ]
    }

    fn gallery_components(&self, ui: &mut egui::Ui, g: &mut Gallery) {
        let t = u::tokens(ui.ctx());
        let ep = g.epoch;
        ui.columns(2, |cols| {
            let (a, b) = cols.split_at_mut(1);
            let ui = &mut a[0];
            u::card(ui, None, |ui| {
                heading(ui, "ui::button");
                ui.horizontal_wrapped(|ui| {
                    u::button(ui, Btn::Primary, self.t(K::VCopyAllNow));
                    u::button(ui, Btn::Secondary, self.t(K::VCopyNow));
                    u::button(ui, Btn::Danger, self.t(K::VStopProtecting));
                    u::button(ui, Btn::Ghost, self.t(K::VUndo));
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    u::button_with(ui, Btn::Primary, self.t(K::VProtectSomething), Some(Glyph::Plus), true);
                    ui.add_enabled_ui(false, |ui| u::button(ui, Btn::Secondary, self.t(K::VScanPc)));
                });
            });
            ui.add_space(10.0);
            u::card(ui, None, |ui| {
                heading(ui, "ui::toggle");
                u::toggle(ui, &mut g.toggles[0], self.t(K::VStartWithPc));
                u::toggle(ui, &mut g.toggles[1], self.t(K::VSuggestNew));
                u::toggle(ui, &mut g.toggles[2], self.t(K::VAutoUpdates));
            });
            ui.add_space(10.0);
            u::card(ui, None, |ui| {
                heading(ui, "ui::state_pill");
                for (i, (st, txt)) in self.sample_texts().iter().enumerate() {
                    if *st == State::Busy && !g.ambient {
                        continue;
                    }
                    u::state_pill(ui, ("pill", i, ep), *st, txt);
                    ui.add_space(4.0);
                }
                ui.add_space(4.0);
                heading(ui, "ui::chip  ui::device_pill");
                ui.horizontal_wrapped(|ui| {
                    for (i, (k, n)) in [(K::VFilterAll, 12), (K::VFilterGames, 6), (K::VFilterApps, 3), (K::VFilterFolders, 2)].iter().enumerate() {
                        if u::chip(ui, self.t(*k), Some(*n), g.chip == i).clicked() {
                            g.chip = i;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    u::device_pill(ui, "PC-DESKTOP", t.teal);
                    u::device_pill(ui, "ALLY", t.purple);
                });
            });
            let ui = &mut b[0];
            u::card(ui, Some(self.t(K::VWhatsProtected)), |ui| {
                heading(ui, "ui::card  ui::list_row");
                let rows = [
                    ("Hollow Knight", K::VSavedGames, "2,1 Mo", TileIcon::Glyph(Glyph::Pad, [0x24, 0x38, 0x4f])),
                    ("Visual Studio Code", K::VAppSettings, "340 Ko", TileIcon::Glyph(Glyph::Code, [0x1f, 0x4f, 0x7a])),
                    ("Documents", K::VFilterFolders, "1,4 Go", TileIcon::Glyph(Glyph::Folder, [0x2f, 0x4a, 0x3b])),
                ];
                for (i, (name, k, size, icon)) in rows.iter().enumerate() {
                    let row = u::ListRow { icon: Some(*icon), title: name, subtitle: self.t(*k), trailing: size, selected: g.list_sel == i, appear: Some((i, ep)) };
                    if u::list_row(ui, i, &row).clicked() {
                        g.list_sel = i;
                    }
                }
            });
            ui.add_space(10.0);
            u::card(ui, None, |ui| {
                heading(ui, "ui::field");
                u::field(ui, "device", self.t(K::VDeviceName), &mut g.field, self.t(K::VDeviceNameHint));
            });
            ui.add_space(10.0);
            u::card(ui, None, |ui| {
                heading(ui, "ui::Toasts  ui::confirm_dialog");
                ui.horizontal_wrapped(|ui| {
                    if u::button(ui, Btn::Secondary, self.t(K::VDone)).clicked() {
                        let d = self.t(K::VRestoredDetail).replace("{}", &self.t(K::VYesterdayAt).replace("{}", "21:14"));
                        g.toasts.push(anim::now(ui.ctx()), self.t(K::VDone), &d, State::Safe, Some(self.t(K::VUndo)));
                    }
                    if u::button(ui, Btn::Secondary, self.t(K::VRestoreAsk)).clicked() {
                        g.dialog = Some(true);
                    }
                    if u::button(ui, Btn::Danger, self.t(K::VStopProtecting)).clicked() {
                        g.dialog = Some(false);
                    }
                });
                if let Some(a) = &g.answer {
                    mono(ui, a, u::col(t.weak));
                }
            });
        });
    }

    fn gallery_home(&self, ui: &mut egui::Ui, g: &mut Gallery) {
        let ep = g.epoch;
        let busy = g.ambient;
        let detail = self.t(K::VAllSafeDetail).replace("{0}", "12").replace("{1}", &self.t(K::VAgoMin).replace("{}", "2"));
        heading(ui, "ui::status_banner");
        let spec = u::BannerSpec { state: if busy { State::Busy } else { State::Safe }, title: self.t(K::VAllSafe), detail: &detail, action: Some(self.t(K::VCopyAllNow)) };
        u::status_banner(ui, &spec);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let todo = self.t(K::VToCheck).replace("{}", "1");
            for (st, title) in [(State::Todo, todo.as_str()), (State::Problem, self.t(K::VVaultUnreachable))] {
                ui.allocate_ui(egui::vec2((ui.available_width() - 12.0) / 2.0, 90.0), |ui| {
                    u::status_banner(ui, &u::BannerSpec { state: st, title, detail: "", action: None });
                });
            }
        });
        ui.add_space(10.0);
        heading(ui, "ui::chip  ui::tile  ui::add_tile");
        ui.horizontal_wrapped(|ui| {
            for (i, (k, n)) in [(K::VFilterAll, 10), (K::VFilterGames, 6), (K::VFilterApps, 2), (K::VFilterFolders, 2)].iter().enumerate() {
                if u::chip(ui, self.t(*k), Some(*n), g.chip == i).clicked() {
                    g.chip = i;
                }
            }
        });
        ui.add_space(10.0);
        let texts = self.sample_texts();
        let ago = |n: &str, k: K| self.t(K::VSafeAgo).replace("{}", &self.t(k).replace("{}", n));
        let tiles: Vec<(&str, TileIcon, State, String)> = vec![
            ("Elden Ring", TileIcon::Initial("ER", [0x5b, 0x4a, 0x2a]), State::Todo, texts[2].1.clone()),
            ("Hollow Knight", TileIcon::Glyph(Glyph::Pad, [0x24, 0x38, 0x4f]), if busy { State::Busy } else { State::Safe }, if busy { texts[1].1.clone() } else { texts[0].1.clone() }),
            ("Celeste", TileIcon::Initial("C", [0x6a, 0x2d, 0x4a]), State::Paused, texts[5].1.clone()),
            ("Factorio", TileIcon::Initial("F", [0x5a, 0x40, 0x22]), State::Incoming, texts[4].1.clone()),
            ("Visual Studio Code", TileIcon::Glyph(Glyph::Code, [0x1f, 0x4f, 0x7a]), State::Safe, texts[0].1.clone()),
            ("OBS Studio", TileIcon::Initial("O", [0x3a, 0x3a, 0x3a]), State::Safe, ago("3", K::VAgoHour)),
            ("Documents", TileIcon::Glyph(Glyph::Folder, [0x2f, 0x4a, 0x3b]), State::Safe, ago("1", K::VAgoHour)),
            ("Minecraft", TileIcon::Initial("M", [0x3d, 0x5a, 0x2a]), State::Safe, ago("2", K::VAgoDay)),
            ("Dolphin", TileIcon::Initial("D", [0x4a, 0x2a, 0x2a]), State::Problem, texts[3].1.clone()),
        ];
        let big = u::theme(ui.ctx()).big;
        let cols = u::tokens::grid_columns(ui.available_width(), big);
        let total = tiles.len() + 1;
        let mut i = 0;
        while i < total {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = u::tokens::TILE_GAP;
                for _ in 0..cols {
                    if i < tiles.len() {
                        let (name, icon, st, status) = &tiles[i];
                        let spec = u::TileSpec { name, icon: *icon, state: *st, status, progress: (*st == State::Busy).then_some(0.64), index: i, epoch: ep, sync: None, width: None, heavy: None };
                        u::tile(ui, i, &spec);
                    } else if i == tiles.len() {
                        u::add_tile(ui, "add", self.t(K::VProtectElse), i, ep);
                    }
                    i += 1;
                }
            });
            ui.add_space(u::tokens::TILE_GAP);
        }
    }

    fn gallery_anims(&self, ui: &mut egui::Ui, g: &mut Gallery) {
        let t = u::tokens(ui.ctx());
        let ep = g.epoch;
        u::card(ui, Some(self.t(K::VTimeTravel)), |ui| {
            heading(ui, "ui::timeline");
            let (a, b) = (t.teal, t.purple);
            let marks: Vec<u::Mark> = [(19, b, false), (19, b, false), (20, a, false), (20, a, false), (21, a, true), (21, b, false), (22, b, false), (22, a, false), (23, a, false), (23, a, false)]
                .iter()
                .enumerate()
                .map(|(i, (d, c, conf))| u::Mark { conflict: *conf, current: i == 9, label: format!("{}:{:02}", 10 + i, (i * 7) % 60), ..u::Mark::new(*d, *c) })
                .collect();
            let today = self.t(K::VToday).to_string();
            let yday = self.t(K::VYesterday).to_string();
            let label = move |d: i64| match d {
                23 => today.clone(),
                22 => yday.clone(),
                other => other.to_string(),
            };
            u::timeline(ui, "tl", &marks, &label, &mut g.tl_sel);
            let card = self.t(K::VVersionCard).replace("{0}", &self.t(K::VYesterdayAt).replace("{}", "21:14")).replace("{1}", "ALLY").replace("{2}", "3");
            u::cascade_in(ui, "vcard", 0, g.tl_sel as u64 + 1000 * ep, |ui| {
                let words = self.t(K::VPreviewWords).replace("{0}", "2").replace("{1}", "1");
                egui::Frame::new().fill(u::col(t.card2)).stroke(egui::Stroke::new(1.0_f32, u::col(t.line))).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    mono(ui, &format!("#{}", g.tl_sel), u::col(t.faint));
                    let g1 = u::layout(ui, &card, 14.0, u::col(t.text), ui.available_width(), 2);
                    let (r, _) = ui.allocate_exact_size(g1.size(), egui::Sense::hover());
                    ui.painter().galley(r.min, g1, u::col(t.text));
                    let g2 = u::layout(ui, &words, 13.0, u::col(t.weak), ui.available_width(), 2);
                    let (r2, _) = ui.allocate_exact_size(g2.size(), egui::Sense::hover());
                    ui.painter().galley(r2.min, g2, u::col(t.weak));
                });
            });
        });
        ui.add_space(10.0);
        ui.columns(2, |cols| {
            let (a, b) = cols.split_at_mut(1);
            let ui = &mut a[0];
            u::card(ui, None, |ui| {
                heading(ui, "ui::state_icon (Safe Busy Todo Problem Incoming Paused)");
                ui.horizontal(|ui| {
                    for (i, st) in State::ALL.iter().enumerate() {
                        if *st == State::Busy && !g.ambient {
                            continue;
                        }
                        ui.vertical(|ui| {
                            u::state_icon(ui, ("big", i, ep), *st, 34.0, (*st == State::Busy).then_some(0.4));
                            mono(ui, &format!("{:?}", st), u::col(t.faint));
                        });
                        ui.add_space(6.0);
                    }
                });
            });
            ui.add_space(10.0);
            u::card(ui, None, |ui| {
                heading(ui, "ui::page_transition  ui::step_transition");
                ui.horizontal(|ui| {
                    if u::button(ui, Btn::Secondary, self.t(K::VWizBack)).clicked() {
                        g.step = g.step.saturating_sub(1).max(1);
                        g.step_dir = -1.0;
                    }
                    if u::button(ui, Btn::Primary, self.t(K::VWizContinue)).clicked() {
                        g.step += 1;
                        g.step_dir = 1.0;
                    }
                });
                ui.add_space(6.0);
                let step = g.step;
                u::step_transition(ui, "step", step + 1000 * ep, g.step_dir, |ui| {
                    egui::Frame::new().fill(u::col(t.card2)).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let text = self.t(K::VWizStepOf).replace("{0}", &step.to_string()).replace("{1}", "6");
                        let g1 = u::layout(ui, &text, 14.0, u::col(t.text), ui.available_width(), 1);
                        let (r, _) = ui.allocate_exact_size(g1.size(), egui::Sense::hover());
                        ui.painter().galley(r.min, g1, u::col(t.text));
                        let g2 = u::layout(ui, self.t(K::VWelcomeBody), 13.0, u::col(t.weak), ui.available_width(), 4);
                        let (r2, _) = ui.allocate_exact_size(g2.size(), egui::Sense::hover());
                        ui.painter().galley(r2.min, g2, u::col(t.weak));
                    });
                });
            });
            let ui = &mut b[0];
            u::card(ui, None, |ui| {
                heading(ui, "anim::burst  anim::counter");
                let ctx = ui.ctx().clone();
                let m = anim::motion(&ctx);
                let age = anim::age_of(&ctx, egui::Id::new(("burst", ep)), 1);
                let p = anim::run(&ctx, age, m.dur(u::tokens::DUR_BURST));
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(70.0, 70.0), egui::Sense::hover());
                    if !m.reduced && p < 1.0 {
                        for i in 0..8 {
                            let (dx, dy, a) = anim::burst(i, 8, p, 28.0);
                            ui.painter().circle_filled(r.center() + egui::vec2(dx, dy), 3.0, u::col_a(t.accent_hover, a));
                        }
                    }
                    u::paint_state(ui.painter(), egui::Rect::from_center_size(r.center(), egui::vec2(28.0, 28.0)), State::Safe, age, None);
                    let pc = anim::run(&ctx, age, m.dur(0.6));
                    let n = anim::counter(0, 21, pc);
                    let g1 = ui.painter().layout_no_wrap(n.to_string(), egui::FontId::monospace(28.0), u::col(t.text));
                    let (r2, _) = ui.allocate_exact_size(g1.size(), egui::Sense::hover());
                    ui.painter().galley(r2.min, g1, u::col(t.text));
                });
            });
            ui.add_space(10.0);
            u::card(ui, None, |ui| {
                heading(ui, "ui::Glyph");
                ui.horizontal_wrapped(|ui| {
                    for gl in Glyph::ALL {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 6.0, u::col(t.card2));
                        u::paint_glyph(ui.painter(), r.shrink(6.0), gl, u::col(t.text));
                        resp.on_hover_ui(|ui| mono(ui, &format!("{:?}", gl), u::col(t.text)));
                    }
                });
                ui.add_space(6.0);
                heading(ui, "ui::tokens");
                ui.horizontal_wrapped(|ui| {
                    let sw = [t.bg, t.panel, t.card, t.card2, t.widget, t.line, t.text, t.weak, t.faint, t.accent, t.accent_hover, t.ok, t.blue, t.orange, t.red, t.purple, t.teal];
                    for c in sw {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                        ui.painter().rect(r, 4.0, u::col(c), egui::Stroke::new(1.0_f32, u::col(t.line)), egui::StrokeKind::Inside);
                        resp.on_hover_ui(|ui| mono(ui, &format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]), u::col(t.text)));
                    }
                });
            });
        });
    }
}
