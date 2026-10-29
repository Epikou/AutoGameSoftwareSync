//! Interface graphique (egui/eframe). Le rendu est ici ; TOUTE la logique testable est dans `vm`,
//! `actions` et `settings`. Ce fichier n'est pas teste automatiquement (voir README).
use crate::config::{Config, Game};
use crate::cloud;
use crate::detect;
use crate::gamepad;
use crate::padfocus::{ItemKind, PadFocus};
use crate::{theme, uilogic};
use crate::i18n::{tr, Lang, K};
use crate::catalog::StoreCtx;
use crate::settings::{BigChoice, ThemeMode};
use crate::vm::{self, AppModel, Confirm, GameStatus, Level, LogEntry, LogFilter, Page, ThemeProbe};
use crate::watch::{self, InstanceLock, Logger, WatchCfg, WatchEvent, Watcher};
use crate::{actions, backup, winutil, Ctx};
// Android : aucune boite de dialogue systeme (voir rfd_shim.rs) ; les modules de l'interface importent `super::*`.
#[cfg(target_os = "android")]
use crate::rfd_shim as rfd;
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Titre de la fenetre (sert aussi a la retrouver pour l'afficher/masquer depuis le tray).
pub const APP_TITLE: &str = "AutoGameSoftwareSync";
/// Abreviation pour les endroits courts (zone de notification).
pub const APP_SHORT: &str = "AGSS";

// Organisation (voir docs/refonte/AVANCEMENT.md, lot 5) :
//  - etat       : state (Shared, Msg, App), status_doc (status.json), init (creation), jobs (travaux de fond)
//  - navigation : nav (manette, focus, onglets), chrome (cadre, barre d'etat), tray_ui / tray_sys (zone de notification)
//  - ecrans     : games_ui, detail_ui, add_page, program_form_ui, manual_ui, detect_ui, profiles_ui, knowledge_ui, settings, settings_simple, usage_ui,
//                 update_ui, share_ui, wizard_ui (premier lancement), banner, modals, gallery (demonstration interne des composants)
//  - composants : ui (bibliotheque 2.0 : jetons, animations, tuiles, boutons...), widgets et common (existants)
mod add_page;
mod banner;
mod chrome;
mod close_ui;
mod common;
mod detail_ui;
mod detect_ui;
mod fiche_ui;
mod frame_clock;
mod frame_guard;
mod gallery;
mod games_ui;
mod home_ui;
mod init;
mod jobs;
mod knowledge_ui;
mod manual_ui;
mod migrate_ui;
mod program_form_ui;
mod modals;
mod nav;
mod picker_ui;
mod playnite_ui;
mod profiles_ui;
mod safety_ui;
mod scan_ui;
mod settings;
mod settings_simple;
mod share_ui;
mod shot;
mod state;
mod status_doc;
mod tray_sys;
mod tray_ui;
mod weight_ui;
mod winprefs_ui;
mod wizard_ui;
pub mod ui;
mod update_ui;
mod usage_ui;
mod vault_ui;
mod widgets;
pub use state::{Shared, WatchState};
use state::*;
pub(crate) use ui::icons::{icon_button, icon_label, Icon};
use tray_ui::build_tray;
use widgets::*;

pub struct LaunchOpts {
    pub start_hidden: bool,
    /// Pour les tests manuels : quitte proprement apres N secondes.
    pub auto_quit_secs: Option<u64>,
}

/// Icone de l'application (mediaicon.svg rendu par examples/make_icon.rs), integree a l'exe : fenetre 64x64.
fn window_icon() -> (Vec<u8>, u32, u32) {
    (include_bytes!("../media/icon_64.rgba").to_vec(), 64, 64)
}

/// Icone de la zone de notification 32x32.
fn tray_icon_rgba() -> (Vec<u8>, u32, u32) {
    (include_bytes!("../media/icon_32.rgba").to_vec(), 32, 32)
}

fn make_ctx(dd: &std::path::Path) -> Ctx {
    Ctx::new(dd.to_path_buf())
}

pub fn run(data_dir: PathBuf, lock: InstanceLock, opts: LaunchOpts) -> Result<(), String> {
    let shared = Shared::new(data_dir.clone(), lock);
    let settings0 = crate::settings::Settings::load_from(&data_dir).unwrap_or_default();
    let _ = &settings0;
    let (icon, iw, ih) = window_icon();
    // Diagnostic (captures des lots) : AGSS_UI_SIZE=1280x800 impose la taille de la fenetre, coin en haut a gauche.
    let forced = std::env::var("AGSS_UI_SIZE").ok().and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))));
    let viewport = egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_inner_size(forced.map(|(w, h)| [w, h]).unwrap_or([1040.0, 700.0]))
        .with_min_inner_size([380.0, 260.0])
        .with_visible(!opts.start_hidden)
        .with_icon(egui::IconData { rgba: icon, width: iw, height: ih });
    // Linux : identifiant de fenetre = nom du raccourci AutoGameSoftwareSync.desktop (icone et regroupement sous Wayland,
    // classe de fenetre X11). Sur un ecran de console portable (1280x800, mode jeu), la fenetre tient dans l'ecran.
    #[cfg(not(windows))]
    let viewport = viewport.with_app_id("AutoGameSoftwareSync");
    // Sans cadre : la fenetre peut depasser un petit ecran (capture d'une taille de Steam Deck ou de plein ecran).
    let viewport = if forced.is_some() { viewport.with_position([0.0, 0.0]).with_decorations(false) } else { viewport };
    let native = eframe::NativeOptions { viewport, ..Default::default() };
    let sh = shared.clone();
    eframe::run_native(
        APP_TITLE,
        native,
        Box::new(move |cc| Ok(Box::new(App::new(cc, sh, opts)))),
    )
    .map_err(|e| e.to_string())
}

// ================================================================== rendu

/// Sense des onglets : cliquables mais NON focalisables, donc jamais atteints par D-pad / stick / Tab
/// (le curseur de selection reste dans l'onglet courant ; on change d'onglet par LB/RB ou a la souris).
pub fn tab_sense() -> egui::Sense {
    egui::Sense::CLICK
}

fn rgb(x: theme::Rgb) -> Color32 {
    Color32::from_rgb(x[0], x[1], x[2])
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.handle_gamepad(raw, ctx);
        // Accueil : fleches / Debut / Fin entre les tuiles de la grille.
        self.home_key_hook(ctx, raw);
        // Linux : Ctrl+Tab / Ctrl+Maj+Tab / Ctrl+Page suivante-precedente changent d'onglet (Steam Input peut y associer
        // les boutons d'epaule) ; la touche est retiree pour ne pas deplacer aussi le focus.
        #[cfg(not(windows))]
        {
            let mut d = 0;
            raw.events.retain(|e| match e {
                egui::Event::Key { key, pressed, modifiers, .. } => {
                    let name = match key {
                        egui::Key::Tab => "Tab",
                        egui::Key::PageDown => "PageDown",
                        egui::Key::PageUp => "PageUp",
                        _ => return true,
                    };
                    let s = uilogic::tab_shortcut(name, modifiers.ctrl || modifiers.command, modifiers.shift);
                    if s == 0 {
                        return true;
                    }
                    if *pressed {
                        d += s;
                    }
                    false
                }
                _ => true,
            });
            if d != 0 && self.model.confirm.is_none() {
                self.page_delta += d;
                ctx.request_repaint();
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Garde-fou : duree de l'image mesuree, image lente / bloquee journalisee (voir frame_guard.rs).
        let _frame_timer = frame_guard::begin(&self.shared.data_dir);
        // Demarrage masque (--tray) : eframe force l'affichage apres la 1re image, on le contre.
        match uilogic::hidden_start_step(self.start_hidden, self.frame_no) {
            uilogic::HideStep::Prepare => {
                winutil::hidden_start_prepare(APP_TITLE);
                ctx.request_repaint();
            }
            uilogic::HideStep::Finish => {
                winutil::hidden_start_finish(APP_TITLE);
                ctx.request_repaint();
            }
            uilogic::HideStep::None => {}
        }
        // Linux sans icone de zone de notification : le demarrage « en arriere-plan » (--tray, autostart) ouvre la fenetre
        // REDUITE (visible dans la barre des taches) au lieu de la cacher, sinon l'appli serait introuvable.
        #[cfg(not(windows))]
        if self.start_hidden && self._tray.is_none() && self.frame_no == 3 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        self.frame_no = self.frame_no.saturating_add(1);
        // Fermer la fenetre = reduire dans la zone de notification (la surveillance continue).
        #[cfg(not(windows))]
        {
            winutil::note_frame(ctx);
            crate::notify::set_open_label(self.t(K::TrayOpen));
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.shared.quitting.load(Ordering::SeqCst) {
            // Sans icone de zone de notification (Linux sans la fonction `tray`, ou bureau sans zone de notification),
            // masquer la fenetre la rendrait introuvable : fermer la fenetre quitte alors l'application.
            #[cfg(not(windows))]
            if self._tray.is_none() {
                self.shared.quit();
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            winutil::hide_window(APP_TITLE);
        }
        self.track_focus(ctx);
        frame_guard::stage("messages des threads");
        self.drain(ctx);
        frame_guard::stage("etat");
        self.update_report_tick();
        // Evenements de la surveillance a montrer (conflit resolu automatiquement) : message + rafraichissement.
        let evs: Vec<(String, bool)> = self.shared.ui_events.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for (t, ok) in evs {
            self.say(t, ok);
            self.last_status = Instant::now() - Duration::from_secs(60);
            if let Some(g) = self.model.selected.clone() {
                self.load_versions(&g, ctx);
            }
        }
        if self.last_theme_probe.elapsed() > Duration::from_secs(15) && self.model.settings.theme == ThemeMode::System {
            self.last_theme_probe = Instant::now();
            self.thread_probe_theme(ctx);
        }
        self.apply_theme(ctx);
        self.clock_begin(ctx);
        if self.gallery.is_some() {
            // Ecran interne de demonstration des composants (AGSS_UI_GALLERY=1) : rien d autre n est dessine.
            self.gallery_screen(ctx);
            self.clock_end(ctx);
            return;
        }
        self.pad_mode = uilogic::pad_present(&self.pad.status());
        self.pad_prompt_tick();
        self.weight_poll(ctx);
        self.update_share_state();
        self.shared.ui_busy.store(self.wizard.active || self.model.confirm.is_some() || self.pending_add.is_some() || self.pending_overlap.is_some() || self.fiche.ask.is_some() || self.vault.open || self.picker.open.is_some() || self.migrate.busy() || self.migrate.modal(), Ordering::SeqCst);
        {
            let mut names = self.machines.clone();
            names.push(self.model.ctx.machine.clone());
            self.machine_map = crate::machinecolor::assign(&names, &self.model.settings.machine_colors);
        }
        if self.page_delta != 0 {
            let p = uilogic::page_after(self.model.page, self.page_delta);
            self.page_delta = 0;
            if p != self.model.page {
                self.model.page = p;
                self.on_page_enter(p, ctx);
            }
        }
        if self.model.page == Page::Games && self.last_status.elapsed() > Duration::from_secs(10) {
            self.refresh_statuses(ctx);
        }

        frame_guard::stage("page");
        self.ensure_cjk(ctx);
        // Nouveau PC : diagnostic des captures, inventaire quotidien des logiciels de ce PC dans le coffre.
        self.migrate_demo_start(ctx);
        self.migrate_inventory_tick();
        // Premier lancement 2.0 : assistant plein ecran (jamais par-dessus un jeu en cours : l'accueil normal s'affiche
        // tant qu'un programme suivi tourne ou qu'une autre appli est en plein ecran).
        if self.wizard.active && !self.wizard_blocked() {
            self.probe_dest(ctx);
            self.update_window_mode(ctx);
            self.update_tray();
            self.wizard_frame(ctx);
            self.clock_end(ctx);
            return;
        }
        self.probe_dest(ctx);
        self.update_window_mode(ctx);
        self.update_tray();
        self.update_support_counter();
        self.scan_demo_start(ctx);

        // Barre d'onglets horizontale (taille fixe, aucun separateur redimensionnable).
        egui::TopBottomPanel::top("tabs").resizable(false).show(ctx, |ui| {
            self.tab_bar(ui);
        });

        egui::TopBottomPanel::bottom("status").resizable(false).show(ctx, |ui| {
            self.status_bar(ui);
        });

        // Rappel unique de la question des statistiques : carte discrete en bas de l'accueil (jamais une fenetre bloquante).
        self.usage_late_tick();
        if self.usage_late_visible() {
            egui::TopBottomPanel::bottom("usage_late").resizable(false).show(ctx, |ui| self.usage_late_card(ui));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.op_progress.is_none() && crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) > 0 && self.shared.watch_progress.view().phase != crate::perf::Phase::Idle {
                self.op_progress = Some((K::OpSync, self.shared.watch_progress.clone()));
                self.op_from_watch = true;
            }
            if self.op_from_watch && self.shared.watch_progress.view().phase == crate::perf::Phase::Idle {
                self.op_progress = None;
                self.op_from_watch = false;
            }
            // Mode simple : une copie automatique se voit sur sa tuile (anneau + pourcentage) et dans le bandeau de l'accueil,
            // pas de ligne technique en plus, sur aucune page (mode expert : ligne detaillee).
            let on_tiles = self.op_from_watch && !self.model.settings.expert_mode;
            if let Some((k, p)) = self.op_progress.clone().filter(|_| !on_tiles) {
                use crate::perf::Phase;
                let v = p.view();
                let ph = match v.phase {
                    Phase::Read => K::PhRead,
                    Phase::Safety => K::PhSafety,
                    Phase::Write => K::PhWrite,
                    Phase::Prepare | Phase::Idle => K::PhPrepare,
                };
                ui.horizontal_wrapped(|ui| {
                    ui.spinner();
                    let line = format!(
                        "{} {}/{} {}, {} / {} ({})",
                        self.t(k),
                        v.files_done.min(v.files_total),
                        v.files_total,
                        self.t(K::ProgFiles),
                        self.bytes(v.bytes_done),
                        self.bytes(v.bytes_total),
                        self.t(ph)
                    );
                    ui.label(line);
                    let b = ui.add_enabled(v.cancellable && !v.cancel_requested && !self.op_from_watch, egui::Button::new(self.t(K::BtnCancel)));
                    if self.deco(ui, b, Some(K::TipCancelOp)).clicked() {
                        p.request_cancel();
                    }
                });
                if v.streaming {
                    ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::StreamingWarn));
                }
                ui.separator();
            }
            self.update_banner(ui);
            if self.model.page != Page::Games {
                // Accueil : son bandeau d'etat global dit deja tout (un seul message a la fois).
                self.share_banner(ui, ctx);
            }
            // Messages courts : toasts 2.0 (plus d'ancienne ligne de message en haut de page).
            self.flush_said(ctx);
            // Transition d'ecran (fondu + glissement de 8 px ; fondu seul en mouvement reduit) au changement d'onglet.
            let page = self.model.page;
            if self.migrate.open {
                // Nouveau PC : ecran a part (4 etapes), a la place de la page courante.
                ui::page_transition(ui, "pages", 100, |ui| self.migrate_screen(ui, ctx));
            } else {
                ui::page_transition(ui, "pages", page as u64, |ui| match page {
                    Page::Games => self.page_home(ui, ctx),
                    Page::Add => self.page_add(ui, ctx),
                    Page::Settings => self.page_settings(ui, ctx),
                });
            }
        });

        // La question des statistiques n'est plus une fenetre : ligne discrete en bas de la derniere etape de l'assistant, puis
        // une seule fois en carte en bas de l'accueil (voir usage_ui.rs).
        self.support_popup(ctx);
        self.proposal_overlay(ctx);
        self.pad_prompt_modal(ctx);
        self.weight_overlay(ctx);
        self.home_overlays(ctx);
        self.fiche_overlays(ctx);
        self.vault_overlay(ctx);
        self.picker_overlay(ctx);
        self.confirm_modal(ctx);
        self.usage_windows(ctx);
        // Selection manette : mise a jour une fois tout dessine (voir padfocus.rs).
        self.focus.set_modal_scope(false);
        let page_key = Page::ALL.iter().position(|p| *p == self.model.page).unwrap_or(0) as u8;
        let modal = self.model.confirm.is_some() || self.support.is_some() || self.usage_erase_ask || self.home.menu.is_some() || self.home.remove_ask.is_some() || self.fiche.ask.is_some() || self.vault.open || self.vault.confirm_open() || self.picker.open.is_some() || self.migrate.modal();
        let page_key = if self.migrate.open { 90 + self.migrate.step_index() } else { page_key };
        if self.focus.end_frame(ctx, page_key, modal, self.pad_mode) {
            ctx.request_repaint();
        }
        // Redessin seulement si necessaire (animations, activite, echeances) : rien au repos.
        self.clock_end(ctx);
    }
}


// ================================================================== conflits, regles de demarrage, tutoriel

/// Drapeau britannique dessine au painter (pas d'emoji : Windows n'affiche pas les emojis de drapeau).
fn paint_flag_en(p: &egui::Painter, r: egui::Rect) {
    let p = p.with_clip_rect(r);
    let (w, h) = (r.width(), r.height());
    let blue = Color32::from_rgb(1, 33, 105);
    let white = Color32::WHITE;
    let red = Color32::from_rgb(200, 16, 46);
    p.rect_filled(r, 0.0, blue);
    // diagonales : blanches larges, rouges fines
    for (a, b) in [(r.left_top(), r.right_bottom()), (r.right_top(), r.left_bottom())] {
        p.line_segment([a, b], egui::Stroke::new(h * 0.2, white));
        p.line_segment([a, b], egui::Stroke::new(h * 0.08, red));
    }
    // croix blanche puis rouge
    p.rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(w, h / 3.0)), 0.0, white);
    p.rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(h / 3.0, h)), 0.0, white);
    p.rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(w, h / 5.0)), 0.0, red);
    p.rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(h / 5.0, h)), 0.0, red);
}

/// Drapeau francais (bleu, blanc, rouge) dessine au painter.
fn paint_flag_fr(p: &egui::Painter, r: egui::Rect) {
    let third = r.width() / 3.0;
    for (i, c) in [Color32::from_rgb(0, 85, 164), Color32::WHITE, Color32::from_rgb(239, 65, 53)].into_iter().enumerate() {
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(r.min.x + third * i as f32, r.min.y), egui::vec2(third + 0.5, r.height())), 0.0, c);
    }
}

