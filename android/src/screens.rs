//! Ecrans : premier lancement (5 etapes), accueil en tuiles, fiche + Remonter le temps, reglages, appareil, emulateurs,
//! a propos, journal, navigateur de dossiers, barre de toast et dialogue de confirmation. Aucune lecture profonde du
//! stockage ici (fil de travail) ; les cartes ont des lignes de hauteur fixe (`App::row`).
use crate::app::{App, BrowseFor, BrowseState, Modal, ModalAct, Screen, GITHUB_URL, ROW_H, VERSION};
use crate::i18n::{self, fill, Lang, T};
use crate::model::{self, CandKind, ItemState, Kind, TtKey};
use crate::sys;
use crate::worker::Job;
use agss::emulators::{self, Compat};
use agss::{machinecolor, timefmt, util, vaults};
use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as ph;

const SETUP_STEPS: usize = 5;
/// Hauteur d'une tuile d'accueil (icone + titre, etat sur deux lignes, derniere copie + appareil).
const TILE_H: f32 = 156.0;
/// Hauteur d'une ligne de « Remonter le temps » (date + appareil + fichiers, puis etiquettes).
const TT_H: f32 = 92.0;

fn muted(app: &App) -> Color32 {
    App::col(agss::theme::mix(app.palette.text, app.palette.panel_bg, 0.35))
}

fn date(app: &App, ms: u64) -> String {
    timefmt::format_local(ms, app.lang, &timefmt::WinTz)
}

fn wait(app: &App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(22.0));
        ui.label(RichText::new(app.t(T::Wait)).color(muted(app)).size(18.0));
    });
}

// ================================================================== premier lancement

pub fn setup(app: &mut App, ui: &mut egui::Ui) {
    let step = app.setup.step;
    if !app.setup.add_mode {
        ui.label(RichText::new(fill(&fill(app.t(T::Step), "n", &(step + 1).to_string()), "total", &SETUP_STEPS.to_string())).color(muted(app)));
    }
    match step {
        0 => setup_lang(app, ui),
        1 => setup_perm(app, ui),
        2 => setup_device(app, ui, false),
        3 => setup_vault(app, ui),
        _ => setup_items(app, ui),
    }
}

fn nav_row(app: &mut App, ui: &mut egui::Ui, can_next: bool, next_label: T) -> (bool, bool) {
    ui.add_space(14.0);
    let mut back = false;
    let mut next = false;
    ui.horizontal(|ui| {
        if app.setup.step > 0 && !app.setup.add_mode {
            back = app.btn(ui, app.t(T::Back), false, 160.0).clicked();
        }
        next = app.btn_enabled(ui, app.t(next_label), true, 220.0, can_next).clicked();
    });
    (back, next)
}

fn setup_lang(app: &mut App, ui: &mut egui::Ui) {
    ui.heading(app.t(T::LangTitle));
    ui.add_space(10.0);
    let mut chosen: Option<Lang> = None;
    ui.horizontal_wrapped(|ui| {
        for l in Lang::ALL {
            if app.btn(ui, l.native_name(), app.lang == l, 150.0).clicked() {
                chosen = Some(l);
            }
        }
    });
    if let Some(l) = chosen {
        app.lang = l;
        app.store.settings.lang = l;
        app.store.prefs.lang_chosen = true;
        let _ = app.store.save_settings();
        let _ = app.store.save_prefs();
    }
    let (_, next) = nav_row(app, ui, true, T::Next);
    if next {
        app.setup.step = 1;
        app.setup.perm_checked_ms = 0;
        app.focus.reset();
    }
}

fn setup_perm(app: &mut App, ui: &mut egui::Ui) {
    ui.heading(app.t(T::PermTitle));
    ui.label(app.t(T::PermBody));
    ui.add_space(8.0);
    let now = util::now_ms();
    if now.saturating_sub(app.setup.perm_checked_ms) > 1000 {
        app.setup.perm_checked_ms = now;
        app.setup.perm_ok = sys::storage_readable();
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
    let ok = app.setup.perm_ok;
    ui.horizontal(|ui| {
        let (icon, txt, col) = if ok { (ph::CHECK_CIRCLE, app.t(T::PermOk), App::col(app.palette.accent)) } else { (ph::WARNING_CIRCLE, app.t(T::PermMissing), App::warn_color()) };
        ui.label(RichText::new(format!("{}  {}", icon, txt)).color(col).size(20.0));
    });
    ui.add_space(6.0);
    if !ok && app.btn(ui, app.t(T::PermButton), true, 320.0).clicked() {
        sys::request_all_files_access();
    }
    let (back, next) = nav_row(app, ui, ok, T::Next);
    if back {
        app.setup.step = 0;
        app.focus.reset();
    }
    if next {
        app.setup.step = 2;
        app.focus.reset();
    }
}

/// Clavier a l'ecran, pilotable a la manette (les claviers Android ne tapent pas toujours dans une NativeActivity).
const KB_ROWS: [&str; 4] = ["ABCDEFGHIJ", "KLMNOPQRST", "UVWXYZ0123", "456789-_()"];

fn keyboard(app: &mut App, ui: &mut egui::Ui, name: &mut String) {
    egui::Frame::new().fill(App::col(app.palette.widget_bg)).corner_radius(12.0).inner_margin(10.0).show(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for row in KB_ROWS {
            ui.horizontal(|ui| {
                for ch in row.chars() {
                    if app.btn(ui, &ch.to_string(), false, 50.0).clicked() && name.chars().count() < 32 {
                        name.push(ch);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if app.btn(ui, app.t(T::KbSpace), false, 170.0).clicked() && name.chars().count() < 32 {
                name.push(' ');
            }
            if app.btn(ui, ph::BACKSPACE, false, 110.0).clicked() {
                name.pop();
            }
            if app.btn(ui, &format!("{}  {}", ph::CHECK, app.t(T::KbDone)), true, 170.0).clicked() {
                app.setup.kb_open = false;
            }
        });
    });
}

/// Etape « appareil » (premier lancement) ou ecran Reglages > Cet appareil (`standalone`).
fn setup_device(app: &mut App, ui: &mut egui::Ui, standalone: bool) {
    ui.heading(app.t(T::DeviceTitle));
    ui.label(app.t(T::DeviceBody));
    ui.add_space(8.0);
    ui.label(RichText::new(app.t(T::DeviceName)).strong());
    let mut name = app.setup.name.clone();
    // Choix rapides sans clavier (avant le champ : la selection manette commence sur un bouton, pas sur le clavier).
    let model = model::default_device_name(&sys::device_model());
    let mut quick: Vec<String> = vec![model.clone(), "Console".into(), "Retroid".into(), "Odin".into(), "Ayaneo".into()];
    quick.dedup();
    ui.horizontal_wrapped(|ui| {
        for q in quick {
            if app.btn(ui, &q, false, 120.0).clicked() {
                name = q;
            }
        }
    });
    ui.horizontal(|ui| {
        app.text(ui, &mut name, 420.0);
        let kb_label = if app.setup.kb_open { app.t(T::KbHide) } else { app.t(T::KbShow) };
        if app.btn(ui, &format!("{}  {}", ph::KEYBOARD, kb_label), false, 230.0).clicked() {
            app.setup.kb_open = !app.setup.kb_open;
        }
    });
    if app.setup.kb_open {
        keyboard(app, ui, &mut name);
    }
    // Le clavier et le champ travaillent sur le nom BRUT (espaces autorises en cours de frappe) ; nettoye a la fin.
    if name != app.setup.name {
        app.setup.name = name.clone();
        let clean = model::clean_device_name(&name);
        if !app.store.settings.machine_colors.contains_key(&clean) {
            app.setup.color = machinecolor::default_color(&clean);
        }
    }
    let clean = model::clean_device_name(&app.setup.name);
    ui.add_space(8.0);
    ui.label(RichText::new(app.t(T::DeviceColor)).strong());
    ui.horizontal_wrapped(|ui| {
        for c in machinecolor::PALETTE {
            let sel = app.setup.color == c;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::click());
            let id = ui.make_persistent_id(("color", c));
            let r = ui.interact(rect, id, egui::Sense::click());
            ui.painter().rect_filled(rect, 10.0, App::col(c));
            if sel {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, ph::CHECK, egui::FontId::proportional(24.0), App::col(machinecolor::text_on(c)));
            }
            app.focus.register(&r, agss::padfocus::ItemKind::Button, true);
            if app.focus.is_selected(r.id) {
                ui.painter().rect_stroke(rect.expand(3.0), 12.0, egui::Stroke::new(3.0_f32, App::col(app.palette.ring)), egui::StrokeKind::Outside);
                if app.focus.take_scroll(r.id, true) {
                    r.scroll_to_me(Some(egui::Align::Center));
                }
            }
            if r.clicked() {
                app.setup.color = c;
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(app.t(T::SetDevice)).color(muted(app)));
        let tmp = app.setup.color;
        let txt = RichText::new(&clean).color(App::col(machinecolor::text_on(tmp))).strong();
        egui::Frame::new().fill(App::col(tmp)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
            ui.label(txt);
        });
    });
    let can = !clean.is_empty();
    if !can {
        ui.label(RichText::new(app.t(T::DeviceNameEmpty)).color(App::warn_color()));
    }
    let (back, next) = if standalone {
        ui.add_space(14.0);
        let ok = app.btn_enabled(ui, app.t(T::Ok), true, 220.0, can).clicked();
        (false, ok)
    } else {
        nav_row(app, ui, can, T::Next)
    };
    if back {
        app.setup.step = 1;
        app.focus.reset();
    }
    if next {
        app.setup.name = clean.clone();
        app.store.prefs.device_name = clean.clone();
        app.store.settings.machine_colors.insert(clean, app.setup.color);
        let _ = app.store.save_settings();
        let _ = app.store.save_prefs();
        if standalone {
            app.go(Screen::Settings);
        } else {
            app.setup.step = 3;
            app.setup.candidates.clear();
            app.scan(Job::ScanVaults { roots: sys::storage_roots() });
            app.focus.reset();
        }
    }
}

fn setup_vault(app: &mut App, ui: &mut egui::Ui) {
    ui.heading(app.t(T::VaultTitle));
    ui.label(app.t(T::VaultBody));
    ui.add_space(8.0);
    if let Some(v) = app.setup.chosen_vault.clone() {
        ui.label(RichText::new(fill(app.t(T::VaultChosen), "path", &v.to_string_lossy())).color(App::col(app.palette.accent)).strong());
        if !vaults::is_vault(&v) && !v.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(vaults::VAULT_DIR)).unwrap_or(false) {
            ui.label(RichText::new(app.t(T::VaultWillCreate)).color(muted(app)));
        }
    }
    if let Some(e) = app.setup.vault_error.clone() {
        ui.label(RichText::new(fill(app.t(T::VaultError), "msg", &e)).color(App::warn_color()));
    }
    ui.add_space(6.0);
    if app.waiting {
        wait(app, ui);
    }
    let cands = app.setup.candidates.clone();
    if cands.is_empty() {
        if !app.waiting {
            ui.label(RichText::new(app.t(T::VaultNone)).color(muted(app)));
        }
    } else {
        ui.label(RichText::new(app.t(T::VaultFound)).strong());
        for c in &cands {
            let sub = match &c.kind {
                CandKind::Syncthing => app.t(T::VaultSyncthing).to_string(),
                CandKind::Existing(d) => fill(app.t(T::VaultExisting), "devices", &if d.is_empty() { "?".to_string() } else { d.join(", ") }),
                CandKind::Named => app.t(T::VaultNamed).to_string(),
            };
            let sel = app.setup.chosen_vault.as_ref() == Some(&c.path);
            let p = c.path.to_string_lossy().to_string();
            let r = app.card(ui, &format!("cand-{}", p), egui::vec2(ui.available_width().min(760.0), 72.0), sel, |ui, _| {
                App::row(ui, ROW_H, |ui| {
                    ui.label(RichText::new(&p).strong());
                });
                App::row(ui, ROW_H - 6.0, |ui| {
                    ui.label(RichText::new(&sub).small());
                });
            });
            if r.clicked() {
                app.setup.chosen_vault = Some(c.path.clone());
                app.setup.vault_error = None;
            }
        }
    }
    ui.add_space(6.0);
    if app.btn(ui, &format!("{}  {}", ph::FOLDER_OPEN, app.t(T::VaultBrowse)), false, 240.0).clicked() {
        app.back_to = Screen::Setup;
        app.browse = Some(BrowseState::new(BrowseFor::Vault));
        app.go(Screen::Browse);
    }
    let (back, next) = nav_row(app, ui, app.setup.chosen_vault.is_some() && !app.waiting, T::Next);
    if back {
        app.setup.step = 2;
        app.focus.reset();
    }
    if next {
        if let Some(v) = app.setup.chosen_vault.clone() {
            app.setup.vault_error = None;
            app.scan(Job::PrepareVault { path: v, from_setup: true });
        }
    }
}

fn setup_items(app: &mut App, ui: &mut egui::Ui) {
    ui.heading(app.t(T::ItemsTitle));
    ui.label(app.t(T::ItemsBody));
    ui.add_space(8.0);
    if app.waiting {
        wait(app, ui);
    }
    let props = app.setup.proposals.clone();
    let emus: Vec<usize> = props.iter().enumerate().filter(|(_, p)| !p.locations.is_empty()).map(|(i, _)| i).collect();
    let folders: Vec<usize> = props.iter().enumerate().filter(|(_, p)| p.locations.is_empty()).map(|(i, _)| i).collect();
    if emus.is_empty() && !app.waiting {
        ui.label(RichText::new(app.t(T::ItemsNone)).color(muted(app)));
    }
    for i in emus {
        let p = &props[i];
        let already = p.already;
        egui::Frame::new().fill(App::col(app.palette.widget_bg)).corner_radius(12.0).inner_margin(12.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width().min(1000.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}  {}", ph::GAME_CONTROLLER, p.title)).size(20.0).strong());
                if already {
                    ui.label(RichText::new(format!("· {}", app.t(T::ItemsAlready))).color(muted(app)));
                }
            });
            ui.label(RichText::new(&p.subtitle).small().color(muted(app)));
            if !already {
                for (j, (label, _)) in p.locations.iter().enumerate() {
                    let mut v = app.setup.ticks[i][j];
                    app.check(ui, &mut v, &i18n::loc_label(app.lang, label));
                    app.setup.ticks[i][j] = v;
                }
            }
        });
    }
    if !folders.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(app.t(T::ItemsPersonal)).strong());
        for i in folders {
            let p = &props[i];
            let title = i18n::item_display(app.lang, &p.title);
            if p.already {
                ui.label(RichText::new(format!("{}  {} · {}", ph::FOLDER, title, app.t(T::ItemsAlready))).color(muted(app)));
                continue;
            }
            let mut v = app.setup.ticks[i][0];
            app.check(ui, &mut v, &format!("{}  {}   ({})", ph::FOLDER, title, p.subtitle));
            app.setup.ticks[i][0] = v;
        }
    }
    ui.add_space(6.0);
    if app.btn(ui, &format!("{}  {}", ph::FOLDER_OPEN, app.t(T::VaultBrowse)), false, 240.0).clicked() {
        app.back_to = Screen::Setup;
        app.browse = Some(BrowseState::new(BrowseFor::PersonalFolder));
        app.go(Screen::Browse);
    }
    let any = app.setup.ticks.iter().any(|t| t.iter().any(|x| *x));
    ui.add_space(14.0);
    let mut finish = false;
    let mut skip = false;
    ui.horizontal(|ui| {
        if !app.setup.add_mode && app.btn(ui, app.t(T::Back), false, 160.0).clicked() {
            app.setup.step = 3;
            app.focus.reset();
        }
        finish = app.btn_enabled(ui, app.t(T::ItemsFinish), true, 220.0, any && !app.waiting).clicked();
        skip = app.btn(ui, app.t(if app.setup.add_mode { T::Cancel } else { T::Skip }), false, 160.0).clicked();
    });
    if !any && !app.waiting {
        ui.label(RichText::new(app.t(T::ItemsNothingTicked)).color(muted(app)));
    }
    if finish || skip {
        let mut games = Vec::new();
        if finish {
            for (i, p) in props.iter().enumerate() {
                if p.already {
                    continue;
                }
                if let Some(g) = model::build_game(p, &app.setup.ticks[i]) {
                    games.push(g);
                }
            }
        }
        let n = app.store.add_games(games);
        let _ = app.store.save_config();
        app.store.prefs.setup_done = true;
        let _ = app.store.save_prefs();
        if n > 0 {
            app.toast(i18n::added(app.lang, n));
        }
        let add_mode = app.setup.add_mode;
        app.setup.add_mode = false;
        app.go(if add_mode { Screen::Settings } else { Screen::Home });
        app.refresh();
        app.sync_all();
    }
}

// ================================================================== accueil

fn banner(app: &App, ui: &mut egui::Ui, title: &str, body: &str, col: Color32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(App::col(app.palette.widget_bg)).stroke(egui::Stroke::new(2.0_f32, col)).corner_radius(12.0).inner_margin(12.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width().min(1000.0));
        ui.label(RichText::new(format!("{}  {}", ph::WARNING_CIRCLE, title)).size(20.0).strong().color(col));
        ui.label(body);
        add(ui);
    });
    ui.add_space(6.0);
}

pub fn home(app: &mut App, ui: &mut egui::Ui) {
    let busy = app.worker.busy();
    ui.horizontal(|ui| {
        ui.heading(RichText::new(app.t(T::HomeTitle)).size(26.0));
        ui.add_space(8.0);
        let dev = app.store.prefs.device_name.clone();
        app.chip(ui, &dev);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if app.btn(ui, &format!("{}  {}", ph::GEAR, app.t(T::Settings)), false, 150.0).clicked() {
                app.go(Screen::Settings);
            }
            if app.btn(ui, &format!("{}  {}", ph::PLUS, app.t(T::Add)), false, 130.0).clicked() {
                start_add(app);
            }
            // Pendant une copie : « Arreter » prend la place de « Synchroniser » tant que rien n'est ecrit dans le coffre.
            let v = app.worker.progress.view();
            if busy && v.cancellable && !v.cancel_requested {
                if app.btn(ui, &format!("{}  {}", ph::X_CIRCLE, app.t(T::StopCopy)), false, 240.0).clicked() {
                    app.worker.progress.request_cancel();
                }
            } else {
                let label = if busy { format!("{}  {}", ph::ARROWS_CLOCKWISE, app.t(T::Syncing)) } else { format!("{}  {}", ph::ARROWS_CLOCKWISE, app.t(T::SyncNow)) };
                if app.btn_enabled(ui, &label, true, 240.0, !busy && !app.store.config.games.is_empty() && !app.perm_missing).clicked() {
                    app.sync_all();
                }
            }
        });
    });
    // Autorisation retiree : bandeau bloquant, aucune synchro.
    if app.perm_missing {
        let (t, b) = (app.t(T::PermLostTitle).to_string(), app.t(T::PermLostBody).to_string());
        let mut open = false;
        banner(app, ui, &t, &b, App::warn_color(), |ui| {
            ui.add_space(4.0);
            open = app.btn(ui, app.t(T::PermButton), true, 320.0).clicked();
        });
        if open {
            sys::request_all_files_access();
        }
    }
    if app.clock_wrong {
        let (t, b) = (app.t(T::StError).to_string(), fill(app.t(T::ClockWrong), "date", &date(app, util::now_ms())));
        banner(app, ui, &t, &b, App::warn_color(), |_| {});
    }
    // Titre d'etat.
    let attention = app.items.iter().filter(|i| app.state_of(i).needs_attention(i.versions)).count();
    let (headline, col) = if app.perm_missing {
        (app.t(T::PermLostTitle).to_string(), App::warn_color())
    } else if busy {
        let cur = app.worker.current().and_then(|n| app.store.game(&n).map(|g| i18n::item_display(app.lang, g.shown()))).unwrap_or_default();
        (format!("{} {}", app.t(T::Syncing), cur), Color32::from_rgb(86, 180, 233))
    } else if !app.vault_reachable() {
        (app.t(T::VaultUnreachable).to_string(), App::warn_color())
    } else if app.store.config.games.is_empty() {
        (app.t(T::NothingYet).to_string(), muted(app))
    } else if attention > 0 {
        (fill(app.t(T::NeedAttention), "n", &attention.to_string()), App::warn_color())
    } else {
        (app.t(T::AllSafe).to_string(), App::col(app.palette.accent))
    };
    ui.label(RichText::new(headline).color(col).size(18.0));
    if busy {
        let v = app.worker.progress.view();
        if v.bytes_total > 0 {
            ui.add(egui::ProgressBar::new((v.bytes_done as f32 / v.bytes_total as f32).min(1.0)).desired_width(400.0));
        }
    }
    ui.add_space(8.0);
    // Tuiles.
    let items = app.items.clone();
    let games = app.store.config.games.clone();
    let avail = ui.available_width();
    let cols = ((avail + 12.0) / 272.0).floor().max(1.0) as usize;
    let w = ((avail - 12.0 * (cols as f32 - 1.0)) / cols as f32).floor();
    let now = util::now_ms();
    let mut open: Option<String> = None;
    for row in games.chunks(cols) {
        ui.horizontal(|ui| {
            for g in row {
                let info = items.iter().find(|i| i.name == g.name).cloned().unwrap_or_else(|| model::ItemInfo { name: g.name.clone(), display: g.shown().to_string(), kind: model::kind_of(g), path: g.path.clone(), exists: true, state: ItemState::Pending, last_ms: None, last_device: None, versions: 0, conflict: false });
                let st = app.state_of(&info);
                let icon = if info.kind == Kind::Emulator { ph::GAME_CONTROLLER } else { ph::FOLDER };
                let st_txt = app.state_text(&st, info.versions);
                let st_col = app.state_color(&st);
                let last = info.last_ms.map(|ms| fill(app.t(T::LastCopy), "ago", &i18n::ago(app.lang, now, ms)));
                let dev = info.last_device.clone();
                let display = i18n::item_display(app.lang, &info.display);
                let r = app.card(ui, &format!("tile-{}", g.name), egui::vec2(w, TILE_H), false, |ui, _| {
                    App::row(ui, ROW_H + 4.0, |ui| {
                        ui.label(RichText::new(icon).size(26.0));
                        ui.add(egui::Label::new(RichText::new(&display).size(19.0).strong()).truncate());
                    });
                    // Etat sur deux lignes au plus.
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 2.0 * ROW_H - 4.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.add(egui::Label::new(RichText::new(st_txt).color(st_col)).wrap());
                    });
                    App::row(ui, ROW_H - 4.0, |ui| {
                        if let Some(l) = last {
                            ui.label(RichText::new(l).small());
                        }
                        if let Some(d) = dev {
                            app.mini_chip(ui, &d);
                        }
                    });
                });
                if r.clicked() {
                    open = Some(g.name.clone());
                }
            }
        });
    }
    if let Some(n) = open {
        app.reload_item(&n);
        app.go(Screen::Item);
    }
    ui.add_space(10.0);
    ui.label(RichText::new(app.t(T::PadHint)).small().color(muted(app)));
}

fn start_add(app: &mut App) {
    app.setup.add_mode = true;
    app.setup.step = 4;
    app.setup.proposals.clear();
    app.setup.ticks.clear();
    app.scan(Job::Proposals { roots: sys::storage_roots(), existing: app.store.config.games.clone() });
    app.go(Screen::Setup);
}

// ================================================================== fiche + remonter le temps

pub fn item(app: &mut App, ui: &mut egui::Ui) {
    let Some(name) = app.item.as_ref().map(|i| i.name.clone()) else {
        app.go(Screen::Home);
        return;
    };
    let Some(g) = app.store.game(&name).cloned() else {
        app.go(Screen::Home);
        return;
    };
    let info = app.items.iter().find(|i| i.name == name).cloned();
    let title = i18n::item_display(app.lang, g.shown());
    app.header(ui, &title, Some(Screen::Home));
    let st = info.as_ref().map(|i| app.state_of(i)).unwrap_or(ItemState::Pending);
    let versions = info.as_ref().map(|i| i.versions).unwrap_or(0);
    ui.label(RichText::new(app.state_text(&st, versions)).color(app.state_color(&st)).size(18.0));
    ui.label(RichText::new(format!("{} : {}", app.t(T::Folder), g.path.to_string_lossy())).small().color(muted(app)));
    if !g.extra_locations.is_empty() {
        for l in &g.extra_locations {
            ui.label(RichText::new(format!("{} : {}", i18n::loc_label(app.lang, &l.label), l.path.to_string_lossy())).small().color(muted(app)));
        }
    }
    ui.add_space(8.0);
    let busy = app.worker.busy();
    let can_act = !busy && app.vault_reachable() && !app.perm_missing;
    ui.horizontal(|ui| {
        if app.btn_enabled(ui, &format!("{}  {}", ph::SHIELD_CHECK, app.t(T::ProtectNow)), true, 260.0, can_act).clicked() {
            app.sync_one(&g);
        }
        if app.btn(ui, app.t(T::Unprotect), false, 200.0).clicked() {
            app.modal = Some(Modal { title: app.t(T::Unprotect).into(), body: app.t(T::UnprotectBody).into(), yes: app.t(T::Unprotect).into(), act: ModalAct::Unprotect(name.clone()) });
        }
    });
    // Version plus recente d'un autre appareil : jamais remise toute seule, on demande.
    if let ItemState::Available(from) = &st {
        ui.add_space(6.0);
        let from = from.clone();
        egui::Frame::new().fill(App::col(app.palette.widget_bg)).corner_radius(12.0).inner_margin(12.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(fill(app.t(T::AvailableTitle), "device", &from)).strong().color(Color32::from_rgb(86, 180, 233)));
                app.mini_chip(ui, &from);
            });
            ui.label(app.t(T::AvailableBody));
            let label = fill(app.t(T::TakeVersion), "device", &from);
            if app.btn_enabled(ui, &format!("{}  {}", ph::DOWNLOAD_SIMPLE, label), true, 360.0, can_act).clicked() {
                app.modal = Some(Modal { title: label.clone(), body: app.t(T::AvailableBody).into(), yes: label, act: ModalAct::TakeVersion(name.clone()) });
            }
        });
    }
    if st == ItemState::Conflict {
        ui.add_space(6.0);
        egui::Frame::new().fill(App::col(app.palette.widget_bg)).corner_radius(12.0).inner_margin(12.0).show(ui, |ui| {
            ui.label(RichText::new(app.t(T::StConflict)).strong().color(App::warn_color()));
            ui.label(app.t(T::ConflictBody));
            ui.horizontal(|ui| {
                if app.btn_enabled(ui, app.t(T::KeepMine), false, 220.0, can_act).clicked() {
                    app.modal = Some(Modal { title: app.t(T::KeepMine).into(), body: app.t(T::ConflictBody).into(), yes: app.t(T::KeepMine).into(), act: ModalAct::Resolve { name: name.clone(), keep_mine: true } });
                }
                if app.btn_enabled(ui, app.t(T::TakeOther), false, 300.0, can_act).clicked() {
                    app.modal = Some(Modal { title: app.t(T::TakeOther).into(), body: app.t(T::AvailableBody).into(), yes: app.t(T::TakeOther).into(), act: ModalAct::Resolve { name: name.clone(), keep_mine: false } });
                }
            });
        });
    }
    ui.add_space(10.0);
    let (rows, loaded) = app.item.as_ref().map(|i| (i.rows.clone(), i.loaded)).unwrap_or_default();
    let nv = rows.iter().filter(|r| matches!(r.key, TtKey::Version(_))).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{}  {}", ph::CLOCK_COUNTER_CLOCKWISE, app.t(T::TimeTravel))).size(20.0).strong());
        if loaded {
            ui.label(RichText::new(i18n::versions(app.lang, nv)).color(muted(app)));
        }
    });
    if !loaded {
        wait(app, ui);
    } else if rows.is_empty() {
        ui.label(RichText::new(app.t(T::NoVersions)).color(muted(app)));
    }
    let mut restore: Option<(TtKey, String)> = None;
    for (i, r) in rows.iter().enumerate() {
        let is_safety = matches!(r.key, TtKey::Safety(_));
        let mut tags: Vec<(String, Color32)> = Vec::new();
        if r.current {
            tags.push((app.t(T::Current).into(), App::col(app.palette.accent)));
        }
        if r.active {
            tags.push((app.t(T::Active).into(), Color32::from_rgb(86, 180, 233)));
        }
        if r.conflict_branch {
            tags.push((app.t(T::ConflictBranch).into(), App::warn_color()));
        }
        if is_safety {
            tags.push((app.t(T::SafetyCopy).into(), muted(app)));
        }
        let when = date(app, r.ms);
        let dev = r.device.clone();
        let detail = format!("{} · {}", i18n::files(app.lang, r.files), agss::i18n::format_bytes(app.lang, r.bytes));
        let resp = app.card(ui, &format!("tt-{}-{}", name, i), egui::vec2(ui.available_width().min(820.0), TT_H), r.current, |ui, _| {
            App::row(ui, ROW_H + 4.0, |ui| {
                ui.label(RichText::new(if is_safety { ph::LIFEBUOY } else { ph::CLOCK }).size(22.0));
                ui.label(RichText::new(&when).strong());
                app.mini_chip(ui, &dev);
                ui.label(RichText::new(&detail).small());
            });
            App::row(ui, ROW_H, |ui| {
                for (t, c) in &tags {
                    ui.label(RichText::new(format!("{} {}", ph::TAG, t)).small().color(*c));
                }
            });
        });
        if resp.clicked() && !r.current && can_act {
            restore = Some((r.key.clone(), when.clone()));
        }
    }
    if let Some((key, when)) = restore {
        app.modal = Some(Modal { title: format!("{} · {}", app.t(T::Restore), when), body: app.t(T::RestoreBody).into(), yes: app.t(T::Restore).into(), act: ModalAct::Restore { name: name.clone(), key } });
    }
}

// ================================================================== reglages

pub fn settings(app: &mut App, ui: &mut egui::Ui) {
    let title = app.t(T::Settings).to_string();
    app.header(ui, &title, Some(Screen::Home));
    // Langue.
    ui.label(RichText::new(app.t(T::SetLang)).strong());
    let mut chosen: Option<Lang> = None;
    ui.horizontal_wrapped(|ui| {
        for l in Lang::ALL {
            if app.btn(ui, l.native_name(), app.lang == l, 140.0).clicked() {
                chosen = Some(l);
            }
        }
    });
    if let Some(l) = chosen {
        app.lang = l;
        app.store.settings.lang = l;
        app.store.prefs.lang_chosen = true;
        let _ = app.store.save_settings();
        let _ = app.store.save_prefs();
    }
    ui.add_space(8.0);
    // Appareil.
    ui.horizontal(|ui| {
        ui.label(RichText::new(app.t(T::SetDevice)).strong());
        let d = app.store.prefs.device_name.clone();
        app.chip(ui, &d);
        if app.btn(ui, app.t(T::SetChange), false, 140.0).clicked() {
            app.setup.name = d.clone();
            app.setup.color = app.device_color(&d);
            app.setup.kb_open = false;
            app.go(Screen::Device);
        }
    });
    // Coffre.
    ui.horizontal(|ui| {
        ui.label(RichText::new(app.t(T::SetVault)).strong());
        let v = app.store.prefs.vault.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "—".into());
        let col = if app.vault_reachable() { App::col(app.palette.text) } else { App::warn_color() };
        ui.label(RichText::new(v).color(col));
        if app.btn(ui, app.t(T::SetChange), false, 140.0).clicked() {
            app.back_to = Screen::Settings;
            app.browse = Some(BrowseState::new(BrowseFor::Vault));
            app.go(Screen::Browse);
        }
    });
    ui.add_space(8.0);
    // Conflits : toujours demandes sur la console (horloge incertaine, emulateur peut-etre encore ouvert).
    ui.label(RichText::new(app.t(T::SetConflict)).strong());
    ui.label(app.t(T::ConflictPolicy));
    ui.add_space(8.0);
    ui.label(RichText::new(app.t(T::AutoTitle)).strong());
    ui.label(app.t(T::AutoBody));
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if app.btn(ui, &format!("{}  {}", ph::PLUS, app.t(T::Add)), false, 160.0).clicked() {
            start_add(app);
        }
        if app.btn(ui, &format!("{}  {}", ph::GAME_CONTROLLER, app.t(T::SetEmulators)), false, 200.0).clicked() {
            app.emu_found = None;
            app.emu_roots = sys::storage_roots();
            app.scan(Job::FindEmus { roots: app.emu_roots.clone() });
            app.go(Screen::Emulators);
        }
        if app.btn(ui, &format!("{}  {}", ph::SCROLL, app.t(T::Journal)), false, 160.0).clicked() {
            app.journal = read_journal(&app.store.data_dir);
            app.go(Screen::Journal);
        }
        if app.btn(ui, &format!("{}  {}", ph::INFO, app.t(T::SetAbout)), false, 170.0).clicked() {
            app.go(Screen::About);
        }
        if app.btn(ui, &format!("{}  {}", ph::EYE_SLASH, app.t(T::Minimize)), false, 190.0).clicked() {
            sys::move_task_to_back();
        }
    });
}

pub fn device(app: &mut App, ui: &mut egui::Ui) {
    let title = app.t(T::SetDevice).to_string();
    app.header(ui, &title, Some(Screen::Settings));
    setup_device(app, ui, true);
}

pub fn emulators(app: &mut App, ui: &mut egui::Ui) {
    let title = app.t(T::SetEmulators).to_string();
    app.header(ui, &title, Some(Screen::Settings));
    let Some(found) = app.emu_found.clone() else {
        wait(app, ui);
        return;
    };
    // Racines reelles (stockage interne, cartes SD), lues une fois a l'entree de l'ecran.
    let roots: Vec<String> = app.emu_roots.iter().map(|r| r.to_string_lossy().to_string()).collect();
    for e in emulators::EMULATORS {
        let f = found.iter().find(|f| f.emu.id == e.id);
        egui::Frame::new().fill(App::col(app.palette.widget_bg)).corner_radius(12.0).inner_margin(12.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}  {}", ph::GAME_CONTROLLER, e.display)).size(20.0).strong());
                match f {
                    Some(f) => ui.label(RichText::new(format!("{} {} · {}", ph::CHECK_CIRCLE, app.t(T::Found), f.root.to_string_lossy())).color(App::col(app.palette.accent))),
                    None => ui.label(RichText::new(format!("{} {}", ph::CIRCLE, app.t(T::NotFound))).color(muted(app))),
                };
            });
            let compat = match e.compat {
                Compat::Same => app.t(T::CompatSame),
                Compat::SavesOnly => app.t(T::CompatSaves),
                Compat::Partial => app.t(T::CompatPartial),
            };
            ui.label(compat);
            let expected: Vec<String> = roots.iter().flat_map(|r| e.android_roots.iter().map(move |x| format!("{}/{}", r, x))).collect();
            ui.label(RichText::new(format!("{} : {}", app.t(T::ExpectedIn), expected.join("  ·  "))).small().color(muted(app)));
            ui.label(RichText::new(i18n::emu_note(app.lang, e.id).unwrap_or(e.note)).small().color(muted(app)));
        });
    }
}

pub fn about(app: &mut App, ui: &mut egui::Ui) {
    let title = app.t(T::SetAbout).to_string();
    app.header(ui, &title, Some(Screen::Settings));
    ui.label(RichText::new(app.t(T::AppName)).size(24.0).strong());
    ui.label(fill(app.t(T::AboutBody), "v", VERSION));
    ui.add_space(8.0);
    if app.btn(ui, &format!("{}  {}", ph::ARROW_SQUARE_OUT, app.t(T::OpenGitHub)), true, 300.0).clicked() {
        sys::open_url(GITHUB_URL);
    }
    ui.label(RichText::new(GITHUB_URL).small().color(muted(app)));
    ui.add_space(8.0);
    ui.label(RichText::new(app.t(T::AutoTitle)).strong());
    ui.label(app.t(T::AutoBody));
    ui.add_space(8.0);
    ui.label(RichText::new(app.t(T::AboutLicenses)).small().color(muted(app)));
}

pub fn journal(app: &mut App, ui: &mut egui::Ui) {
    let title = app.t(T::Journal).to_string();
    app.header(ui, &title, Some(Screen::Settings));
    if app.journal.is_empty() {
        ui.label(RichText::new("—").color(muted(app)));
    }
    for l in &app.journal {
        ui.label(RichText::new(l).monospace().small());
    }
}

fn read_journal(data_dir: &std::path::Path) -> Vec<String> {
    let t = std::fs::read_to_string(data_dir.join("logs").join("android.log")).unwrap_or_default();
    let lines: Vec<&str> = t.lines().collect();
    lines.iter().rev().take(80).rev().map(|s| s.to_string()).collect()
}

// ================================================================== navigateur de dossiers

pub fn browse(app: &mut App, ui: &mut egui::Ui) {
    let Some(b) = app.browse.as_mut() else {
        app.go(Screen::Home);
        return;
    };
    // Sous-dossiers lus une fois par dossier (un seul read_dir, jamais a chaque image).
    if b.dir.is_some() && b.entries_for != b.dir {
        b.entries = b.dir.as_ref().map(|d| model::list_subdirs(d)).unwrap_or_default();
        b.entries_for = b.dir.clone();
    }
    let roots = b.roots.clone();
    let dir = b.dir.clone();
    let subs = b.entries.clone();
    let purpose = b.purpose;
    let back = app.back_to;
    let title = app.t(T::BrowseTitle).to_string();
    ui.horizontal(|ui| {
        if app.btn(ui, &format!("{}  {}", ph::ARROW_LEFT, app.t(T::Back)), false, 120.0).clicked() {
            app.browse = None;
            app.go(back);
        }
        ui.heading(RichText::new(title).size(26.0));
    });
    ui.add_space(6.0);
    match dir {
        None => {
            for r in &roots {
                let label = match sys::root_label(r) {
                    None => app.t(T::BrowseInternal).to_string(),
                    Some(n) => fill(app.t(T::BrowseSd), "name", &n),
                };
                if app.btn(ui, &format!("{}  {}", ph::HARD_DRIVES, label), false, 420.0).clicked() {
                    if let Some(b) = app.browse.as_mut() {
                        b.dir = Some(r.clone());
                    }
                    app.focus.reset();
                }
            }
        }
        Some(d) => {
            ui.label(RichText::new(d.to_string_lossy()).color(muted(app)));
            ui.horizontal(|ui| {
                if app.btn_enabled(ui, &format!("{}  {}", ph::CHECK, app.t(T::BrowseChoose)), true, 300.0, !app.waiting).clicked() {
                    choose_folder(app, &d, purpose);
                }
                if app.btn(ui, &format!("{}  {}", ph::ARROW_UP, app.t(T::BrowseUp)), false, 220.0).clicked() {
                    if let Some(b) = app.browse.as_mut() {
                        b.dir = if roots.contains(&d) { None } else { d.parent().map(|p| p.to_path_buf()) };
                    }
                    app.focus.reset();
                }
            });
            if subs.is_empty() {
                ui.label(RichText::new(app.t(T::BrowseEmpty)).color(muted(app)));
            }
            for s in subs {
                let n = s.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if app.btn(ui, &format!("{}  {}", ph::FOLDER, n), false, ui.available_width().min(760.0)).clicked() {
                    if let Some(b) = app.browse.as_mut() {
                        b.dir = Some(s.clone());
                    }
                    app.focus.reset();
                }
            }
        }
    }
}

fn choose_folder(app: &mut App, d: &std::path::Path, purpose: BrowseFor) {
    let back = app.back_to;
    match purpose {
        BrowseFor::Vault => {
            if back == Screen::Setup {
                app.setup.chosen_vault = Some(d.to_path_buf());
                app.setup.vault_error = None;
            } else {
                app.scan(Job::PrepareVault { path: d.to_path_buf(), from_setup: false });
            }
        }
        BrowseFor::PersonalFolder => {
            let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Folder".into());
            let g = emulators::personal_game(&name, d);
            let n = app.store.add_games(vec![g]);
            let _ = app.store.save_config();
            if n > 0 {
                app.toast(i18n::added(app.lang, 1));
                app.scan(Job::Proposals { roots: sys::storage_roots(), existing: app.store.config.games.clone() });
            }
        }
    }
    app.browse = None;
    app.go(back);
}

// ================================================================== toast et dialogue

pub fn toast_bar(app: &mut App, ctx: &egui::Context) {
    let Some(t) = app.toast.as_ref() else { return };
    let text = t.text.clone();
    let undo = t.undo.clone();
    let screen = ctx.screen_rect();
    let mut do_undo = false;
    egui::Area::new(egui::Id::new("toast")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0)).show(ctx, |ui| {
        egui::Frame::new().fill(App::col(app.palette.widget_bg)).corner_radius(12.0).stroke(egui::Stroke::new(2.0_f32, App::col(app.palette.accent))).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
            ui.set_max_width(screen.width() - 40.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&text).size(17.0));
                if undo.is_some() && app.btn(ui, &format!("{}  {}", ph::ARROW_COUNTER_CLOCKWISE, app.t(T::Undo)), true, 150.0).clicked() {
                    do_undo = true;
                }
            });
        });
    });
    if do_undo {
        if let Some((name, ticket)) = undo {
            if let Some(g) = app.store.game(&name).cloned() {
                app.worker.send(Job::Undo { env: app.env(), game: g, ticket });
            }
        }
        app.toast = None;
    }
}

pub fn modal(app: &mut App, ctx: &egui::Context) {
    let Some(m) = app.modal.as_ref() else { return };
    let (title, body, yes, act) = (m.title.clone(), m.body.clone(), m.yes.clone(), m.act.clone());
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("modal_dim")).order(egui::Order::Foreground).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
        ui.allocate_rect(screen, egui::Sense::click());
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(160));
    });
    let mut decision: Option<bool> = None;
    egui::Area::new(egui::Id::new("modal")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        egui::Frame::new().fill(App::col(app.palette.panel_bg)).corner_radius(16.0).stroke(egui::Stroke::new(2.0_f32, App::col(app.palette.accent))).inner_margin(22.0).show(ui, |ui| {
            ui.set_max_width((screen.width() - 80.0).min(640.0));
            ui.label(RichText::new(&title).size(22.0).strong());
            ui.add_space(6.0);
            ui.label(&body);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let no = app.btn(ui, app.t(T::Cancel), false, 160.0);
                app.focus.set_default(no.id);
                if no.clicked() {
                    decision = Some(false);
                }
                if app.btn(ui, &yes, true, 240.0).clicked() {
                    decision = Some(true);
                }
            });
        });
    });
    match decision {
        Some(false) => app.modal = None,
        Some(true) => {
            app.modal = None;
            match act {
                ModalAct::Restore { name, key } => {
                    if let Some(g) = app.store.game(&name).cloned() {
                        let env = app.env_restore();
                        match key {
                            TtKey::Version(v) => app.worker.send(Job::RestoreVersion { env, game: g, version: v }),
                            TtKey::Safety(dir) => app.worker.send(Job::RestoreSafety { env, game: g, dir }),
                        }
                    }
                }
                ModalAct::Unprotect(name) => {
                    if app.store.remove_game(&name) {
                        let _ = app.store.save_config();
                        app.worker.forget(&name);
                        app.items.retain(|i| i.name != name);
                        app.toast(app.t(T::Removed).into());
                    }
                    app.item = None;
                    app.go(Screen::Home);
                }
                ModalAct::Resolve { name, keep_mine } => {
                    if let Some(g) = app.store.game(&name).cloned() {
                        app.worker.send(Job::Resolve { env: app.env_restore(), game: g, keep_mine });
                    }
                }
                ModalAct::TakeVersion(name) => {
                    if let Some(g) = app.store.game(&name).cloned() {
                        app.worker.send(Job::SyncOne { env: app.env_restore(), game: g });
                    }
                }
            }
        }
        None => {}
    }
}
