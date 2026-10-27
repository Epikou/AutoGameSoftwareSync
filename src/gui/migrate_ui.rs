//! Nouveau PC (2.0, lot 12) : 4 ecrans pour tout retrouver apres un formatage ou sur une nouvelle machine. 1 ancien PC
//! (appareils du coffre), 2 verification element par element (etat, case, taille totale, emplacement a choisir au
//! selecteur guide), 3 recuperation en lot (progression globale et par element, arret entre deux elements, jamais
//! pendant qu'un programme concerne tourne), 4 bilan (recupere, ce qui reste a faire, pages des boutiques, winget,
//! annulation complete). Ouvert depuis l'accueil (« Nouveau PC ? »), la carte « Ton coffre » des Reglages et l'etape
//! coffre du premier lancement. Aussi : cartes « programme installe : recuperer » (recuperation differee) et ecriture
//! quotidienne de l'inventaire des logiciels de ce PC dans le coffre. Logique pure et testee : `crate::migrate`.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph, State};
use crate::home::Kind as HKind;
use crate::migrate::{self as mg, inventory as inv, Device, Outcome, Plan, Status};
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;

/// Cle du selecteur guide (emplacement a choisir).
const PICK_KEY: &str = "migrate_place";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Device,
    Check,
    Run,
    Done,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Step::Device => 0,
            Step::Check => 1,
            Step::Run => 2,
            Step::Done => 3,
        }
    }
}

/// Verification en cours (thread) : avancement et resultat.
#[derive(Default)]
struct CheckJob {
    done: AtomicUsize,
    total: AtomicUsize,
    result: Mutex<Option<(Vec<Plan>, Option<inv::Inventory>)>>,
}

pub struct MigrateUi {
    pub open: bool,
    /// Ouvert depuis le premier lancement : on y revient a la fin.
    pub from_wizard: bool,
    /// Une recuperation a eu lieu pendant cette session (donnees d'usage du premier lancement).
    pub did_migrate: bool,
    step: Step,
    dir: f32,
    epoch: u64,
    devices: Arc<Mutex<Option<Vec<Device>>>>,
    device: Option<Device>,
    preselect: Option<String>,
    check: Option<Arc<CheckJob>>,
    plans: Vec<Plan>,
    inventory: Option<inv::Inventory>,
    run: Option<Arc<mg::Run>>,
    started: Instant,
    running: Vec<String>,
    running_at: Option<Instant>,
    picking: Option<String>,
    undo_ask: bool,
    undo: Option<Arc<Mutex<Option<mg::UndoReport>>>>,
    undone: Option<mg::UndoReport>,
    winget_open: bool,
    winget_ask: bool,
    /// Liste « A refaire » de l'ancien appareil (imprimantes, lecteurs reseau, Wi-Fi), lue pendant la verification.
    todo: Arc<Mutex<Option<crate::winprefs::TodoList>>>,
    /// Recuperation differee : elements dont le programme vient d'apparaitre (cartes de l'accueil).
    ready: Arc<Mutex<Vec<(mg::Pending, Plan)>>>,
    ready_at: Option<Instant>,
    ready_hidden: HashSet<String>,
    ready_busy: Option<(String, Arc<Mutex<Option<mg::ItemResult>>>)>,
    /// Inventaire des logiciels de ce PC : jour deja traite, ecriture en cours, dernier essai.
    inv_day: Option<String>,
    inv_busy: Arc<AtomicBool>,
    inv_at: Option<Instant>,
    /// Diagnostic des captures (`AGSS_UI_MIGRATE_DEMO=open,device:0,choose:<nom>=<dossier>,check:<nom>,go,stop:1,winget,undo-ask,scroll:<px>`).
    demo: Vec<String>,
    demo_read: bool,
    demo_t: Option<f64>,
    scroll_to: Option<f32>,
}

impl Default for MigrateUi {
    fn default() -> Self {
        MigrateUi {
            open: false,
            from_wizard: false,
            did_migrate: false,
            step: Step::Device,
            dir: 1.0,
            epoch: 0,
            devices: Arc::new(Mutex::new(None)),
            device: None,
            preselect: None,
            check: None,
            plans: vec![],
            inventory: None,
            run: None,
            started: Instant::now(),
            running: vec![],
            running_at: None,
            picking: None,
            undo_ask: false,
            undo: None,
            undone: None,
            winget_open: false,
            winget_ask: false,
            ready: Arc::new(Mutex::new(vec![])),
            ready_at: None,
            ready_hidden: HashSet::new(),
            ready_busy: None,
            todo: Arc::new(Mutex::new(None)),
            inv_day: None,
            inv_busy: Arc::new(AtomicBool::new(false)),
            inv_at: None,
            demo: vec![],
            demo_read: false,
            demo_t: None,
            scroll_to: None,
        }
    }
}

impl MigrateUi {
    /// Une recuperation (ou son annulation) est-elle en cours ?
    pub fn busy(&self) -> bool {
        self.undo.is_some() || self.run.as_ref().map(|r| !r.state.lock().map(|s| s.finished).unwrap_or(true)).unwrap_or(false)
    }

    /// Ecran courant (0..3) : selection manette par ecran.
    pub fn step_index(&self) -> u8 {
        self.step.index() as u8
    }

    /// Un dialogue de « Nouveau PC » est-il ouvert ?
    pub fn modal(&self) -> bool {
        self.open && (self.undo_ask || self.winget_ask)
    }
}

/// Texte d'une cle pour un nom stable de dossier perso / element systeme.
fn personal_key(stable: &str) -> Option<K> {
    Some(match stable {
        "Documents" => K::VPersDocuments,
        "Pictures" => K::VPersPictures,
        "Desktop" => K::VPersDesktop,
        "Music" => K::VPersMusic,
        "Videos" => K::VPersVideos,
        "Downloads" => K::VPersDownloads,
        "Desktop shortcuts" => K::VSysDesktop,
        "Start menu" => K::VSysStartMenu,
        "Taskbar pins" => K::VSysTaskbar,
        "Wallpaper" => K::VSysWallpaper,
        _ => return None,
    })
}

fn kind_icon(p: &Plan) -> (Glyph, [u8; 3]) {
    match (p.personal.is_some(), p.kind) {
        (true, _) if p.main.as_ref().map(|m| m.filter.is_some() || m.is_file).unwrap_or(false) => (Glyph::Window, [0x2b, 0x3d, 0x5c]),
        (true, _) | (_, HKind::Folder) => (Glyph::Folder, [0x2f, 0x4a, 0x3b]),
        (_, HKind::Game) => (Glyph::Pad, [0x24, 0x38, 0x4f]),
        (_, HKind::App) => (Glyph::Code, [0x1f, 0x4f, 0x7a]),
    }
}

impl App {
    // ------------------------------------------------------------ ouverture, fermeture

    /// Ouvre « Nouveau PC » (appareil `machine` preselectionne s'il est donne).
    pub(super) fn migrate_open(&mut self, ctx: &egui::Context, machine: Option<String>) {
        let from_wizard = self.wizard.active;
        let keep = (self.migrate.ready.clone(), self.migrate.ready_hidden.clone(), self.migrate.inv_day.clone(), self.migrate.inv_busy.clone(), self.migrate.did_migrate, std::mem::take(&mut self.migrate.demo), self.migrate.demo_read);
        self.migrate = MigrateUi { open: true, from_wizard, ..Default::default() };
        (self.migrate.ready, self.migrate.ready_hidden, self.migrate.inv_day, self.migrate.inv_busy, self.migrate.did_migrate, self.migrate.demo, self.migrate.demo_read) = keep;
        self.migrate.preselect = machine;
        self.migrate.epoch += 1;
        self.migrate_load_devices(ctx);
        self.vault.open = false;
        self.pad_log("[Nouveau PC] ouvert");
    }

    fn migrate_load_devices(&mut self, ctx: &egui::Context) {
        let Some(vault) = self.model.dest() else { return };
        let (slot, c, me, local) = (self.migrate.devices.clone(), ctx.clone(), self.model.ctx.machine.clone(), mg::local_keys(&self.model.config));
        std::thread::spawn(move || {
            let d = mg::devices(&vault, &me, &local);
            if let Ok(mut g) = slot.lock() {
                *g = Some(d);
            }
            c.request_repaint();
        });
    }

    fn migrate_close(&mut self, ctx: &egui::Context) {
        let running = self.migrate.run.as_ref().map(|r| !r.state.lock().map(|s| s.finished).unwrap_or(true)).unwrap_or(false);
        if running {
            return; // jamais pendant une recuperation : on attend la fin de l'element en cours
        }
        self.migrate.open = false;
        self.migrate.winget_open = false;
        if self.migrate.step == Step::Done || self.migrate.did_migrate {
            self.after_change(ctx);
            self.home.epoch += 1;
            self.last_status = Instant::now() - Duration::from_secs(60);
        }
        if !self.migrate.from_wizard {
            self.model.page = Page::Games;
            self.on_page_enter(Page::Games, ctx);
        }
    }

    fn migrate_goto(&mut self, s: Step) {
        self.migrate.dir = if s.index() >= self.migrate.step.index() { 1.0 } else { -1.0 };
        self.migrate.step = s;
        self.migrate.epoch += 1;
    }

    /// Nom affiche d'un element (nom stable d'un dossier perso traduit).
    fn migrate_shown(&self, p: &Plan) -> String {
        if let Some(k) = crate::winprefs::Kind::from_stable(&p.name) {
            return self.t(super::winprefs_ui::pref_label_key(k)).to_string();
        }
        p.personal.as_deref().and_then(personal_key).map(|k| self.t(k).to_string()).unwrap_or_else(|| p.name.clone())
    }

    // ------------------------------------------------------------ verification

    fn migrate_check(&mut self, ctx: &egui::Context, d: Device) {
        let Some(vault) = self.model.dest() else { return };
        self.migrate.device = Some(d.clone());
        self.migrate.plans.clear();
        self.migrate.running.clear();
        let job = Arc::new(CheckJob::default());
        self.migrate.check = Some(job.clone());
        self.migrate_goto(Step::Check);
        let (c, dd, local) = (ctx.clone(), self.shared.data_dir.clone(), mg::local_keys(&self.model.config));
        crate::usage::record(&self.shared.data_dir, "migration", &mg::usage_props("start", d.todo, 0, 0));
        let todo_slot = self.migrate.todo.clone();
        if let Ok(mut g) = todo_slot.lock() {
            *g = None;
        }
        std::thread::spawn(move || {
            let here = mg::Here::real(&dd);
            let invt = inv::latest(&vault, &d.name);
            if here.windows {
                if let Ok(mut g) = todo_slot.lock() {
                    *g = mg::todo_list_of(&vault, &d);
                }
            }
            let j2 = job.clone();
            let c2 = c.clone();
            let plans = mg::check(&vault, &d, &local, &here, invt.as_ref(), &move |i, n| {
                j2.done.store(i, Ordering::SeqCst);
                j2.total.store(n, Ordering::SeqCst);
                c2.request_repaint();
            });
            if let Ok(mut g) = job.result.lock() {
                *g = Some((plans, invt));
            }
            c.request_repaint();
        });
    }

    fn migrate_poll(&mut self, ctx: &egui::Context) {
        if let Some(job) = self.migrate.check.clone() {
            if let Some((mut plans, invt)) = job.result.lock().ok().and_then(|mut g| g.take()) {
                for p in &mut plans {
                    p.display = Some(self.migrate_shown(p)).filter(|s| *s != p.name);
                }
                self.migrate.plans = plans;
                self.migrate.inventory = invt;
                self.migrate.check = None;
                self.migrate.epoch += 1;
                self.migrate.running_at = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
        }
        // Emplacement choisi au selecteur guide.
        if let Some(key) = self.migrate.picking.clone() {
            if let Some(p) = self.take_picked(PICK_KEY) {
                if let Some(plan) = self.migrate.plans.iter_mut().find(|x| x.key == key) {
                    mg::choose_place(plan, p.path, p.is_file);
                }
                self.migrate.picking = None;
            } else if self.picker.open.is_none() {
                self.migrate.picking = None;
            }
        }
        // Programmes concernes ouverts : verifie toutes les 2 s sur l'ecran de verification.
        if self.migrate.step == Step::Check && self.migrate.check.is_none() && self.migrate.running_at.map(|t| t.elapsed() > Duration::from_secs(2)).unwrap_or(true) {
            self.migrate.running_at = Some(Instant::now());
            let procs = &self.model.ctx.procs;
            self.migrate.running = mg::running_programs(&self.migrate.plans, &|e| !matches!(procs.is_running(e), Ok(false)));
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        // Recuperation terminee : bilan.
        if let Some(run) = self.migrate.run.clone() {
            let (finished, n_ok, n_res) = run.state.lock().map(|s| (s.finished, mg::summary(&s.results).0, s.results.len())).unwrap_or((true, 0, 0));
            if finished && self.migrate.step == Step::Run {
                let total = self.migrate.plans.iter().filter(|p| p.checked).count();
                crate::usage::record(&self.shared.data_dir, "migration", &mg::usage_props("end", n_ok, total.saturating_sub(n_ok), self.migrate.started.elapsed().as_secs()));
                self.migrate.did_migrate = n_res > 0;
                self.after_change(ctx);
                // Elements sans programme : premiere synchro tout de suite (comme le scan).
                if let Ok(s) = run.state.lock() {
                    if let Ok(mut q) = self.shared.sync_requests.lock() {
                        q.extend(s.results.iter().filter(|r| matches!(r.outcome, Outcome::Restored | Outcome::Linked)).filter_map(|r| r.game.as_ref().map(|g| Some(g.name.clone()))));
                    }
                }
                self.migrate_goto(Step::Done);
                self.pad_log(&format!("[Nouveau PC] recuperation terminee : {} sur {}", n_ok, n_res));
            } else if !finished {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if let Some(slot) = self.migrate.undo.clone() {
            if let Some(rep) = slot.lock().ok().and_then(|mut g| g.take()) {
                self.migrate.undo = None;
                let detail = if rep.failed.is_empty() { String::new() } else { rep.failed.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>().join(", ") };
                self.toasts.push(u::anim::now(ctx), self.t(K::VMigUndone), &detail, if rep.failed.is_empty() { State::Safe } else { State::Todo }, None);
                self.migrate.undone = Some(rep);
                self.after_change(ctx);
            } else {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
        }
    }

    fn migrate_start_run(&mut self, ctx: &egui::Context) {
        let Some(vault) = self.model.dest() else { return };
        let run = mg::Run::new();
        self.migrate.run = Some(run.clone());
        self.migrate.started = Instant::now();
        self.migrate_goto(Step::Run);
        let plans = self.migrate.plans.clone();
        let source = self.migrate.device.as_ref().map(|d| d.name.clone()).unwrap_or_default();
        let (c, dd) = (ctx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let mut cx = make_ctx(&dd);
            let s = crate::settings::Settings::load(&cx).unwrap_or_default();
            s.apply_to_ctx(&mut cx);
            let c2 = c.clone();
            mg::run(&mut cx, &vault, &plans, &source, &run, &move |_| c2.request_repaint());
            c.request_repaint();
        });
    }

    fn migrate_undo(&mut self, ctx: &egui::Context) {
        let (Some(vault), Some(run)) = (self.model.dest(), self.migrate.run.clone()) else { return };
        let results = run.state.lock().map(|s| s.results.clone()).unwrap_or_default();
        let slot = Arc::new(Mutex::new(None));
        self.migrate.undo = Some(slot.clone());
        let (c, dd) = (ctx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let cx = make_ctx(&dd);
            let rep = mg::undo(&cx, &vault, &results);
            Logger::new(dd.join("logs").join("watch.log"), false).log(&format!("[Nouveau PC] recuperation annulee : {} element(s), {} erreur(s)", rep.undone.len(), rep.failed.len()));
            if let Ok(mut g) = slot.lock() {
                *g = Some(rep);
            }
            c.request_repaint();
        });
    }

    // ------------------------------------------------------------ ecran

    /// Une image de « Nouveau PC » (a la place de la page courante, ou de l'assistant).
    pub(super) fn migrate_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.migrate_poll(ctx);
        self.migrate_demo(ctx);
        let t = u::tokens(ctx);
        let big = self.big_now;
        // en-tete : retour, titre, etapes
        let mut back = false;
        ui.horizontal(|ui| {
            let busy = self.migrate.step == Step::Run;
            let r = ui.add_enabled_ui(!busy, |ui| u::button_with(ui, Btn::Ghost, self.t(K::VWizBack), Some(Glyph::ChevronLeft), big)).inner;
            if self.deco(ui, r, None).clicked() {
                back = true;
            }
            ui.add_space(u::tokens::SP_S);
            ui.label(RichText::new(self.t(K::VMigTitle)).size(if big { u::tokens::FS_H2 * 1.25 } else { u::tokens::FS_H2 }).color(u::col(t.text)));
        });
        ui.add_space(u::tokens::SP_XS);
        ui.horizontal_wrapped(|ui| {
            let cur = self.migrate.step.index();
            for (i, k) in [K::VMigStep1, K::VMigStep2, K::VMigStep3, K::VMigStep4].into_iter().enumerate() {
                let label = format!("{} \u{b7} {}", i + 1, self.t(k));
                let _ = u::chip(ui, &label, None, i <= cur);
                if i < 3 {
                    // separateur typographique (pas un texte)
                    let dash = '\u{2014}'.to_string();
                    ui.label(RichText::new(dash).color(u::col(t.faint)));
                }
            }
        });
        ui.add_space(u::tokens::SP_M);
        if self.model.confirm.is_none() && !self.migrate.undo_ask && !self.migrate.winget_ask && self.picker.open.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            back = true;
        }
        if back {
            match self.migrate.step {
                Step::Device | Step::Done => self.migrate_close(ctx),
                Step::Check => {
                    self.migrate.check = None;
                    self.migrate_goto(Step::Device);
                    self.migrate_load_devices(ctx);
                }
                Step::Run => {}
            }
            return;
        }
        let step = self.migrate.step;
        let dir = self.migrate.dir;
        u::step_transition(ui, "migrate", step.index() as u64, dir, |ui| match step {
            Step::Device => self.migrate_devices(ui, ctx),
            Step::Check => self.migrate_check_screen(ui, ctx),
            Step::Run => self.migrate_run_screen(ui, ctx),
            Step::Done => self.migrate_done(ui, ctx),
        });
        self.migrate_dialogs(ctx);
    }

    // ---- 1. ancien PC

    fn migrate_devices(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = u::tokens(ctx);
        let big = self.big_now;
        ui.label(RichText::new(self.t(K::VMigPickTitle)).size(u::tokens::FS_H3).strong().color(u::col(t.text)));
        ui.label(RichText::new(self.t(K::VMigPickBody)).color(u::col(t.weak)));
        ui.add_space(u::tokens::SP_M);
        if self.model.dest().is_none() {
            ui.label(RichText::new(self.t(K::VMigNoVault)).color(u::col(t.weak)));
            ui.add_space(u::tokens::SP_S);
            if u::button_with(ui, Btn::Primary, self.t(K::VChooseVault), Some(Glyph::Folder), big).clicked() {
                self.vault_open(ctx);
            }
            return;
        }
        let Some(list) = self.migrate.devices.lock().ok().and_then(|g| g.clone()) else {
            ui.horizontal(|ui| {
                ui.spinner();
            });
            return;
        };
        if let Some(name) = self.migrate.preselect.take() {
            if let Some(d) = list.iter().find(|d| d.name.eq_ignore_ascii_case(&name)).cloned() {
                self.migrate_check(ctx, d);
                return;
            }
        }
        if list.is_empty() {
            ui.label(RichText::new(self.t(K::VMigNoDevice)).color(u::col(t.weak)));
            return;
        }
        let now = crate::util::now_ms();
        let w = ui.available_width();
        let cols = if w > 900.0 { 3 } else if w > 560.0 { 2 } else { 1 };
        let gap = u::tokens::SP_S;
        let cw = ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let mut pick: Option<Device> = None;
        egui::ScrollArea::vertical().id_salt("mig_devices").auto_shrink([false, false]).show(ui, |ui| {
            for (row, chunk) in list.chunks(cols).enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (j, d) in chunk.iter().enumerate() {
                        let items = if d.todo == 1 { self.t(K::VMigOneItem).to_string() } else { self.t(K::VMigItems).replace("{}", &d.todo.to_string()) };
                        let items = if d.todo == 0 { self.t(K::VMigAllHere).to_string() } else { items };
                        let last = d.last_ms.map(|ms| self.t(K::VMigLastCopy).replace("{}", &self.ago_text(now, ms))).unwrap_or_default();
                        let color = self.machine_rgb(&d.name);
                        let tag = d.same_as_here.then(|| self.t(K::VMigSameName));
                        let r = u::cascade_in(ui, ("mig_dev", &d.name), row * cols + j, self.migrate.epoch, |ui| device_card(ui, ("mig_dev", &d.name), cw, &d.name, color, &items, &last, tag, d.todo > 0, big));
                        if self.deco(ui, r, None).clicked() && d.todo > 0 {
                            pick = Some(d.clone());
                        }
                    }
                });
                ui.add_space(gap);
            }
        });
        if let Some(d) = pick {
            self.migrate_check(ctx, d);
        }
    }

    // ---- 2. verification

    fn status_text(&self, p: &Plan, t: &u::Tokens) -> (String, [u8; 3], String) {
        let now = crate::util::now_ms();
        match p.status {
            Status::Ready => {
                let place = if p.personal.as_deref() == Some("Documents") {
                    crate::timetravel::Place::Documents
                } else {
                    p.main.as_ref().map(|m| crate::timetravel::place_of(&m.path, &self.known)).unwrap_or(crate::timetravel::Place::Elsewhere)
                };
                let where_ = match place {
                    crate::timetravel::Place::AppData => self.t(K::VPlaceAppData),
                    crate::timetravel::Place::Documents => self.t(K::VPlaceDocuments),
                    crate::timetravel::Place::SavedGames => self.t(K::VPlaceSavedGames),
                    crate::timetravel::Place::Elsewhere => self.t(K::VPlaceElsewhere),
                };
                (self.t(K::VMigReady).to_string(), t.ok, self.t(K::VMigWhyReady).replace("{}", where_))
            }
            Status::NotInstalled => (self.t(K::VMigNotInstalled).to_string(), t.orange, self.t(K::VMigWhyNotInstalled).to_string()),
            Status::Choose => (self.t(K::VMigChoose).to_string(), t.blue, self.t(K::VMigWhyChoose).to_string()),
            Status::Conflict => (self.t(K::VMigConflict).to_string(), t.red, self.t(K::VMigWhyConflict).replace("{}", &p.local_newest_ms.map(|ms| self.ago_text(now, ms)).unwrap_or_default())),
            Status::AlreadyHere => (self.t(K::VMigAlready).to_string(), t.blue, self.t(K::VMigWhyAlready).to_string()),
        }
    }

    fn migrate_check_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = u::tokens(ctx);
        let big = self.big_now;
        if let Some(d) = self.migrate.device.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.t(K::VMigFrom)).color(u::col(t.weak)));
                let _ = u::device_pill(ui, &d.name, self.machine_rgb(&d.name));
            });
            ui.add_space(u::tokens::SP_S);
        }
        if let Some(job) = &self.migrate.check {
            let (i, n) = (job.done.load(Ordering::SeqCst), job.total.load(Ordering::SeqCst));
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(self.t(K::VMigChecking).replace("{0}", &i.to_string()).replace("{1}", &n.max(i).to_string())).color(u::col(t.weak)));
            });
            u::anim::need_ambient(ctx);
            return;
        }
        if self.migrate.plans.is_empty() {
            ui.label(RichText::new(self.t(K::VMigNothing)).color(u::col(t.weak)));
            return;
        }
        let bar_h = if big { 96.0 } else { 80.0 };
        let avail = ui.available_height();
        let mut toggle: Option<usize> = None;
        let mut choose: Option<usize> = None;
        let rows: Vec<(String, String, (String, [u8; 3], String), bool, bool, String, (Glyph, [u8; 3]))> = self
            .migrate
            .plans
            .iter()
            .map(|p| (p.key.clone(), self.migrate_shown(p), self.status_text(p, &t), p.checked, p.status == Status::Choose, self.bytes(p.bytes), kind_icon(p)))
            .collect();
        let mut area = egui::ScrollArea::vertical().id_salt("mig_check").auto_shrink([false, false]).max_height((avail - bar_h).max(80.0));
        if let Some(y) = self.migrate.scroll_to.take() {
            area = area.vertical_scroll_offset(y);
        }
        area.show(ui, |ui| {
            ui.set_max_width((ui.available_width() - 14.0).max(120.0));
            for (i, (key, name, (pill, pc, why), checked, can_choose, size, icon)) in rows.iter().enumerate() {
                let spec = RowSpec { name, why, pill, pill_col: *pc, trailing: size, checked: *checked, action: if *can_choose { Some(self.t(K::VMigChoosePlace)) } else { None }, icon: *icon };
                let (r, a) = u::cascade_in(ui, ("mig_row", key), i.min(12), self.migrate.epoch, |ui| mig_row(ui, ("mig_row", key), &spec, big));
                let r = self.deco(ui, r, None);
                if a.map(|a| self.deco(ui, a, None).clicked()).unwrap_or(false) {
                    choose = Some(i);
                } else if r.clicked() {
                    toggle = Some(i);
                }
            }
            ui.add_space(u::tokens::SP_L);
        });
        if let Some(i) = toggle {
            if let Some(p) = self.migrate.plans.get_mut(i) {
                p.checked = !p.checked;
            }
        }
        if let Some(i) = choose {
            self.migrate_pick_place(ctx, i);
        }
        // bas collant : copie de securite, selection, taille totale, programmes ouverts, « Tout recuperer (N) »
        let n = self.migrate.plans.iter().filter(|p| p.checked).count();
        let total = self.bytes(mg::checked_bytes(&self.migrate.plans));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), bar_h), egui::Sense::hover());
        ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0_f32, u::col(t.line)));
        let mut go = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(4.0, 8.0))), |ui| {
            if !self.migrate.running.is_empty() {
                ui.label(RichText::new(self.t(K::VMigCloseFirst).replace("{}", &self.migrate.running.join(", "))).color(u::col(t.orange)).strong());
            } else {
                ui.label(RichText::new(self.t(K::VMigSafetyNote)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let enabled = n > 0 && self.migrate.running.is_empty();
                let r = ui.add_enabled_ui(enabled, |ui| u::button_with(ui, Btn::Primary, &self.t(K::VMigGo).replace("{}", &n.to_string()), Some(Glyph::ArrowDown), true)).inner;
                let r = self.deco(ui, r, None);
                if !self.focus_from_nav && r.enabled() {
                    self.focus.set_default(r.id);
                }
                if r.clicked() && enabled {
                    go = true;
                }
                ui.add_space(u::tokens::SP_M);
                let s = if n == 1 { self.t(K::VScanSelectedOne).replace("{}", &total) } else { self.t(K::VScanSelectedMany).replace("{0}", &n.to_string()).replace("{1}", &total) };
                ui.label(RichText::new(s).color(u::col(t.weak)));
            });
        });
        if go {
            self.migrate_start_run(ctx);
        }
    }

    fn migrate_pick_place(&mut self, ctx: &egui::Context, i: usize) {
        let Some(p) = self.migrate.plans.get(i).cloned() else { return };
        let shown = self.migrate_shown(&p);
        let is_file = p.main.as_ref().map(|m| m.is_file).unwrap_or(false);
        let program = p.exe.as_deref().map(|e| crate::knowledge::Program::describe(None, e, &p.name, p.kind == HKind::App));
        let mut names: Vec<String> = Vec::new();
        if let Some(pr) = &program {
            names.extend(pr.companies());
            names.extend(pr.names());
        }
        names.push(p.name.clone());
        let opts = picker_ui::PickOpts { name: Some(shown), is_file, names, found: p.suggestions.clone(), program };
        self.migrate.picking = Some(p.key.clone());
        self.open_picker(ctx, PICK_KEY, opts);
    }

    // ---- 3. recuperation

    fn migrate_run_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = u::tokens(ctx);
        let big = self.big_now;
        let Some(run) = self.migrate.run.clone() else { return };
        let (total, index, current, results, bytes_total, bytes_done, prog) = match run.state.lock() {
            Ok(s) => (s.total, s.index, s.current.clone(), s.results.clone(), s.bytes_total, s.bytes_done, s.progress.as_ref().map(|p| p.view())),
            Err(_) => return,
        };
        let cur_plan_bytes = current.as_ref().and_then(|_| self.migrate.plans.iter().filter(|p| p.checked).nth(index)).filter(|p| p.status == Status::Ready).map(|p| p.bytes).unwrap_or(0);
        // element en cours : lecture verifiee du coffre (moitie), copie de securite, ecriture (moitie)
        let item_frac = prog.map(|v| {
            let b = if v.bytes_total > 0 { v.bytes_done as f32 / v.bytes_total as f32 } else if v.files_total > 0 { v.files_done as f32 / v.files_total as f32 } else { 0.0 };
            match v.phase {
                crate::perf::Phase::Read => 0.5 * b,
                crate::perf::Phase::Safety => 0.5,
                crate::perf::Phase::Write => 0.5 + 0.5 * b,
                _ => 0.0,
            }
            .clamp(0.0, 1.0)
        });
        let global = if bytes_total > 0 { (bytes_done as f32 + cur_plan_bytes as f32 * item_frac.unwrap_or(0.0)) / bytes_total as f32 } else if total > 0 { (results.len() as f32 + item_frac.unwrap_or(0.0)) / total as f32 } else { 1.0 };
        let shown = ui.ctx().animate_value_with_time(egui::Id::new(("mig_global", self.migrate.epoch)), global.clamp(0.0, 1.0), 0.25);
        u::card(ui, None, |ui| {
            ui.horizontal(|ui| {
                let _ = u::state_icon(ui, "mig_ring", State::Busy, if big { 40.0 } else { 32.0 }, Some(shown));
                ui.vertical(|ui| {
                    ui.label(RichText::new(self.t(K::VMigRunning).replace("{0}", &(results.len() + current.is_some() as usize).min(total).to_string()).replace("{1}", &total.to_string())).size(u::tokens::FS_H3).strong().color(u::col(t.text)));
                    if bytes_total > 0 {
                        let done = bytes_done + (cur_plan_bytes as f32 * item_frac.unwrap_or(0.0)) as u64;
                        ui.label(RichText::new(self.t(K::VMigRunBytes).replace("{0}", &self.bytes(done.min(bytes_total))).replace("{1}", &self.bytes(bytes_total))).color(u::col(t.weak)));
                    }
                });
            });
            ui.add_space(u::tokens::SP_S);
            progress_bar(ui, shown, t.accent, 8.0);
            if let Some(c) = &current {
                ui.add_space(u::tokens::SP_S);
                ui.label(RichText::new(self.t(K::VMigNow).replace("{}", c)).color(u::col(t.text)));
                let f = ui.ctx().animate_value_with_time(egui::Id::new(("mig_item", index)), item_frac.unwrap_or(0.0), 0.2);
                progress_bar(ui, f, t.ok, 5.0);
            }
            ui.add_space(u::tokens::SP_S);
            let stopping = run.cancel.load(Ordering::SeqCst);
            let label = if stopping { self.t(K::VMigStopping) } else { self.t(K::VMigStop) };
            let r = ui.add_enabled_ui(!stopping, |ui| u::button(ui, Btn::Secondary, label)).inner;
            if self.deco(ui, r, None).clicked() {
                run.request_cancel();
            }
        });
        u::anim::need_ambient(ctx);
        ui.add_space(u::tokens::SP_M);
        egui::ScrollArea::vertical().id_salt("mig_run").auto_shrink([false, false]).show(ui, |ui| {
            for (i, r) in results.iter().enumerate().rev() {
                u::cascade_in(ui, ("mig_res", &r.key), 0, i as u64 + 1, |ui| self.result_line(ui, r));
            }
        });
    }

    fn outcome_state(o: &Outcome) -> State {
        match o {
            Outcome::Restored | Outcome::Linked => State::Safe,
            Outcome::Deferred => State::Paused,
            Outcome::NoPlace | Outcome::Busy | Outcome::Already(_) => State::Todo,
            Outcome::Failed(_) => State::Problem,
        }
    }

    fn result_line(&self, ui: &mut egui::Ui, r: &mg::ItemResult) {
        let t = u::tokens(ui.ctx());
        ui.horizontal(|ui| {
            let _ = u::state_icon(ui, ("mig_res_ico", &r.key), Self::outcome_state(&r.outcome), 18.0, None);
            ui.label(RichText::new(&r.shown).color(u::col(t.text)));
            let s = match &r.outcome {
                Outcome::Restored => self.t(K::VMigReady).to_string(),
                Outcome::Linked => self.t(K::VMigAlready).to_string(),
                Outcome::Deferred => self.t(K::VMigNotInstalled).to_string(),
                Outcome::NoPlace => self.t(K::VMigChoose).to_string(),
                Outcome::Busy => self.t(K::VMigTodoBusy).replace("{}", &r.shown),
                Outcome::Already(o) => self.t(K::VScanSkipAlready).replace("{0}", &r.shown).replace("{1}", o),
                Outcome::Failed(_) => self.t(K::VMigTodoFailed).replace("{}", &r.shown),
            };
            let resp = ui.label(RichText::new(format!("\u{b7} {}", s)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
            if let (Outcome::Failed(e), true) = (&r.outcome, self.model.settings.expert_mode) {
                resp.on_hover_text(e);
            }
        });
    }

    // ---- 4. bilan

    fn migrate_done(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = u::tokens(ctx);
        let big = self.big_now;
        let Some(run) = self.migrate.run.clone() else { return };
        let (results, cancelled, total) = run.state.lock().map(|s| (s.results.clone(), s.cancelled, s.total)).unwrap_or_default();
        let (ok, _) = mg::summary(&results);
        let mut act = 0;
        let mut open_url: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("mig_done").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width((ui.available_width() - 14.0).max(120.0));
            u::cascade_in(ui, "mig_done_head", 0, self.migrate.epoch, |ui| {
                u::card(ui, None, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(u::tokens::SP_S);
                        let s = if big { 72.0 } else { 60.0 };
                        let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                        let age = u::anim::age_of(ctx, egui::Id::new("mig_done_shield"), self.migrate.epoch);
                        u::status_banner::paint_shield(ui.painter(), r, if self.migrate.undone.is_some() { State::Paused } else { State::Safe }, (age / 0.6).min(1.0) as f32);
                        if age < 0.7 {
                            u::anim::need_transition(ctx);
                        }
                        let title = if self.migrate.undone.is_some() {
                            self.t(K::VMigUndone).to_string()
                        } else if cancelled {
                            self.t(K::VMigDoneStopped).to_string()
                        } else if ok == 1 {
                            self.t(K::VMigDoneOne).to_string()
                        } else {
                            self.t(K::VMigDoneMany).replace("{}", &ok.to_string())
                        };
                        ui.label(RichText::new(title).size(if big { u::tokens::FS_H2 * 1.25 } else { u::tokens::FS_H2 }).color(u::col(t.text)));
                        if self.migrate.undone.is_none() {
                            let body = if cancelled { self.t(K::VMigDoneMany).replace("{}", &ok.to_string()) } else { self.t(K::VMigDoneBody).to_string() };
                            ui.label(RichText::new(body).color(u::col(t.weak)));
                        } else {
                            ui.label(RichText::new(self.t(K::VMigUndoBody)).color(u::col(t.weak)));
                        }
                        ui.add_space(u::tokens::SP_S);
                    });
                    let got: Vec<&mg::ItemResult> = results.iter().filter(|r| matches!(r.outcome, Outcome::Restored | Outcome::Linked)).collect();
                    if !got.is_empty() && self.migrate.undone.is_none() {
                        ui.horizontal_wrapped(|ui| {
                            for r in got {
                                let _ = u::chip(ui, &r.shown, None, true);
                            }
                        });
                    }
                });
            });
            ui.add_space(u::tokens::SP_M);
            // Ce qui reste a faire
            let deferred: Vec<&mg::ItemResult> = results.iter().filter(|r| r.outcome == Outcome::Deferred).collect();
            let places: Vec<&mg::ItemResult> = results.iter().filter(|r| r.outcome == Outcome::NoPlace).collect();
            let others: Vec<&mg::ItemResult> = results.iter().filter(|r| matches!(r.outcome, Outcome::Busy | Outcome::Already(_) | Outcome::Failed(_))).collect();
            let kept: Vec<String> = self.migrate.plans.iter().filter(|p| p.status == Status::Conflict && !p.checked).map(|p| self.migrate_shown(p)).collect();
            let not_started: Vec<String> = if cancelled { self.migrate.plans.iter().filter(|p| p.checked).skip(results.len()).map(|p| self.migrate_shown(p)).collect() } else { vec![] };
            let _ = total;
            u::cascade_in(ui, "mig_done_todo", 1, self.migrate.epoch, |ui| {
                u::card(ui, Some(self.t(K::VMigTodo)), |ui| {
                    if !deferred.is_empty() && self.migrate.undone.is_none() {
                        ui.label(RichText::new(self.t(K::VMigTodoInstall)).strong().color(u::col(t.text)));
                        ui.label(RichText::new(self.t(K::VMigTodoInstallHint)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
                        ui.add_space(u::tokens::SP_XS);
                        for r in &deferred {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("\u{2022} {}", r.shown)).color(u::col(t.text)));
                                ui.add_space(u::tokens::SP_S);
                                let label = match r.link.store() {
                                    Some(s) => self.t(K::VMigOpenStore).replace("{}", s),
                                    None => self.t(K::VMigSearchWeb).to_string(),
                                };
                                let b = u::button(ui, Btn::Secondary, &label);
                                if self.deco(ui, b, None).on_hover_text(r.link.url(&r.name)).clicked() {
                                    open_url = Some(r.link.url(&r.name));
                                }
                            });
                        }
                        ui.add_space(u::tokens::SP_M);
                    }
                    if !places.is_empty() {
                        ui.label(RichText::new(self.t(K::VMigTodoPlaces)).strong().color(u::col(t.text)));
                        ui.label(RichText::new(places.iter().map(|r| r.shown.clone()).collect::<Vec<_>>().join(", ")).color(u::col(t.weak)));
                        ui.label(RichText::new(self.t(K::VMigTodoPlacesHint)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
                        ui.add_space(u::tokens::SP_M);
                    }
                    for r in &others {
                        let s = match &r.outcome {
                            Outcome::Busy => self.t(K::VMigTodoBusy).replace("{}", &r.shown),
                            Outcome::Already(o) => self.t(K::VScanSkipAlready).replace("{0}", &r.shown).replace("{1}", o),
                            _ => self.t(K::VMigTodoFailed).replace("{}", &r.shown),
                        };
                        let resp = ui.label(RichText::new(format!("\u{2022} {}", s)).color(u::col(t.text)));
                        if let (Outcome::Failed(e), true) = (&r.outcome, self.model.settings.expert_mode) {
                            resp.on_hover_text(e);
                        }
                    }
                    for k in &kept {
                        ui.label(RichText::new(format!("\u{2022} {}", self.t(K::VMigTodoKept).replace("{}", k))).color(u::col(t.text)));
                    }
                    if !not_started.is_empty() {
                        ui.label(RichText::new(format!("\u{2022} {}", self.t(K::VMigTodoNotStarted).replace("{}", &not_started.join(", ")))).color(u::col(t.text)));
                    }
                    if !others.is_empty() || !kept.is_empty() || !not_started.is_empty() {
                        ui.add_space(u::tokens::SP_M);
                    }
                    ui.label(RichText::new(self.t(K::VMigTodoAccounts)).strong().color(u::col(t.text)));
                    ui.label(RichText::new(self.t(K::VMigTodoAccountsBody)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
                    // winget (Windows) : seulement si l'ancien PC a note des logiciels reconnus
                    let ids = self.migrate.inventory.as_ref().map(|i| i.winget.clone()).unwrap_or_default();
                    if cfg!(windows) && !ids.is_empty() {
                        ui.add_space(u::tokens::SP_M);
                        let r = u::button_with(ui, Btn::Secondary, self.t(K::VMigWinget), Some(Glyph::ArrowDown), big);
                        if self.deco(ui, r, None).clicked() {
                            self.migrate.winget_open = !self.migrate.winget_open;
                        }
                        if self.migrate.winget_open {
                            let src = self.migrate.device.as_ref().map(|d| d.name.clone()).unwrap_or_default();
                            ui.label(RichText::new(self.tf(K::VMigWingetBody, &src)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
                            let mut text = inv::winget_lines(&ids).join("\n");
                            egui::ScrollArea::vertical().id_salt("mig_winget").max_height(160.0).show(ui, |ui| {
                                ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).interactive(false));
                            });
                            ui.horizontal(|ui| {
                                let r = u::button(ui, Btn::Secondary, self.t(K::VMigWingetCopy));
                                if self.deco(ui, r, None).clicked() {
                                    ctx.copy_text(inv::winget_lines(&ids).join("\r\n"));
                                }
                                let r = u::button(ui, Btn::Ghost, self.t(K::VMigWingetRun));
                                if self.deco(ui, r, None).clicked() {
                                    self.migrate.winget_ask = true;
                                }
                            });
                        }
                    }
                    // Reglages Windows remis : reglages ignores (autre version de Windows), choix a refaire.
                    let pref_lines: Vec<String> = results
                        .iter()
                        .filter(|r| matches!(r.outcome, Outcome::Restored))
                        .filter_map(|r| {
                            let k = crate::winprefs::Kind::from_stable(&r.name)?;
                            let rep = crate::winprefs::load_local(&self.shared.data_dir, k).report?;
                            let lines: Vec<String> = self.pref_report_lines(&rep).into_iter().skip(1).collect();
                            (!lines.is_empty()).then(|| format!("{} : {}", r.shown, lines.join(" · ")))
                        })
                        .collect();
                    if !pref_lines.is_empty() {
                        ui.add_space(u::tokens::SP_M);
                        ui.label(RichText::new(self.t(K::VPrefTitle)).strong().color(u::col(t.text)));
                        for l in pref_lines {
                            ui.label(RichText::new(format!("\u{2022} {}", l)).color(u::col(t.text)));
                        }
                    }
                });
            });
            // Liste « A refaire » de l'ancien appareil (lecture seule).
            let todo = self.migrate.todo.lock().ok().and_then(|g| g.clone());
            if let Some(list) = todo {
                ui.add_space(u::tokens::SP_M);
                let src = self.migrate.device.as_ref().map(|d| d.name.clone()).unwrap_or_default();
                u::cascade_in(ui, "mig_done_redo", 2, self.migrate.epoch, |ui| {
                    self.todo_list_block(ui, &src, &list);
                });
            }
            ui.add_space(u::tokens::SP_L);
            ui.horizontal(|ui| {
                let r = u::button_with(ui, Btn::Primary, self.t(K::VScanSeeHome), Some(Glyph::Home), true);
                if self.deco(ui, r, None).clicked() {
                    act = 1;
                }
                let can_undo = self.migrate.undone.is_none() && self.migrate.undo.is_none() && results.iter().any(|r| matches!(r.outcome, Outcome::Restored | Outcome::Linked | Outcome::Deferred));
                if can_undo {
                    let r = u::button(ui, Btn::Ghost, self.t(K::VMigUndo));
                    if self.deco(ui, r, None).clicked() {
                        self.migrate.undo_ask = true;
                    }
                }
                if self.migrate.undo.is_some() {
                    ui.spinner();
                }
            });
            ui.add_space(u::tokens::SP_L);
        });
        if let Some(url) = open_url {
            winutil::open_url(&url);
        }
        if act == 1 {
            self.migrate_close(ctx);
        }
    }

    fn migrate_dialogs(&mut self, ctx: &egui::Context) {
        if self.migrate.undo_ask {
            let spec = u::ConfirmSpec { title: self.t(K::VMigUndoTitle), body: self.t(K::VMigUndoBody), confirm: self.t(K::VMigUndo), cancel: self.t(K::BtnCancel), danger: true };
            match u::confirm_dialog(ctx, "mig_undo", &spec) {
                Some(u::DialogAnswer::Confirm) => {
                    self.migrate.undo_ask = false;
                    self.migrate_undo(ctx);
                }
                Some(u::DialogAnswer::Cancel) => self.migrate.undo_ask = false,
                None => {}
            }
        }
        if self.migrate.winget_ask {
            let n = self.migrate.inventory.as_ref().map(|i| i.winget.len()).unwrap_or(0);
            let body = self.tn(n as u64, K::VMigWingetAskBodyOne, K::VMigWingetAskBody).replace("{}", &n.to_string());
            let spec = u::ConfirmSpec { title: self.t(K::VMigWingetAskTitle), body: &body, confirm: self.t(K::VMigWingetLaunch), cancel: self.t(K::BtnCancel), danger: false };
            match u::confirm_dialog(ctx, "mig_winget", &spec) {
                Some(u::DialogAnswer::Confirm) => {
                    self.migrate.winget_ask = false;
                    self.migrate_run_winget();
                }
                Some(u::DialogAnswer::Cancel) => self.migrate.winget_ask = false,
                None => {}
            }
        }
    }

    /// Lance winget dans une fenetre visible (apres confirmation explicite) : script ecrit dans le dossier de donnees.
    fn migrate_run_winget(&mut self) {
        let ids = self.migrate.inventory.as_ref().map(|i| i.winget.clone()).unwrap_or_default();
        let lines = inv::winget_lines(&ids);
        if lines.is_empty() {
            return;
        }
        #[cfg(windows)]
        {
            let script = self.shared.data_dir.join("migration").join("reinstall-winget.cmd");
            let body = format!("@echo off\r\n{}\r\necho.\r\npause\r\n", lines.join("\r\n"));
            if crate::util::write_atomic(&script, body.as_bytes()).is_ok() {
                let _ = std::process::Command::new("cmd").args(["/c", "start", "", "cmd", "/k"]).arg(&script).spawn();
                self.pad_log(&format!("[Nouveau PC] reinstallation winget lancee ({} logiciels)", lines.len()));
            }
        }
    }

    // ------------------------------------------------------------ recuperation differee (cartes de l'accueil)

    /// Revoit les elements differes (toutes les 2 min, coffre joignable, pas pendant un programme suivi).
    pub(super) fn migrate_pending_tick(&mut self, ctx: &egui::Context) {
        if self.migrate.ready_at.map(|t| t.elapsed() < Duration::from_secs(120)).unwrap_or(false) {
            return;
        }
        self.migrate.ready_at = Some(Instant::now());
        let Some(vault) = self.model.dest() else { return };
        // elements du coffre (bouton « Nouveau PC ? » de l'accueil)
        self.load_dest_games(ctx);
        if self.dest_ok == Some(false) || self.shared.games_running.load(Ordering::SeqCst) || mg::load_pending(&self.shared.data_dir).is_empty() {
            if let Ok(mut g) = self.migrate.ready.lock() {
                g.clear();
            }
            return;
        }
        let (slot, c, dd, local) = (self.migrate.ready.clone(), ctx.clone(), self.shared.data_dir.clone(), mg::local_keys(&self.model.config));
        std::thread::spawn(move || {
            let here = mg::Here::real(&dd);
            let r = mg::pending_ready(&vault, &dd, &local, &here);
            if let Ok(mut g) = slot.lock() {
                *g = r;
            }
            c.request_repaint();
        });
    }

    /// Cartes « « X » est installe : recuperer ? » en tete de l'accueil.
    pub(super) fn migrate_cards(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.migrate_pending_tick(ctx);
        if let Some((key, slot)) = self.migrate.ready_busy.clone() {
            if let Some(r) = slot.lock().ok().and_then(|mut g| g.take()) {
                self.migrate.ready_busy = None;
                if let Ok(mut g) = self.migrate.ready.lock() {
                    g.retain(|(p, _)| p.key != key);
                }
                let ok = matches!(r.outcome, Outcome::Restored | Outcome::Linked);
                let title = if ok { self.t(K::VMigCardDone).replace("{}", &r.shown) } else { self.t(K::VMigTodoFailed).replace("{}", &r.shown) };
                let detail = if r.outcome == Outcome::Busy { self.t(K::VMigTodoBusy).replace("{}", &r.shown) } else { String::new() };
                self.toasts.push(u::anim::now(ctx), &title, &detail, if ok { State::Safe } else { State::Todo }, None);
                self.after_change(ctx);
                if let (true, Ok(mut q)) = (ok, self.shared.sync_requests.lock()) {
                    q.push(r.game.as_ref().map(|g| g.name.clone()));
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
        }
        let list: Vec<(mg::Pending, Plan)> = self.migrate.ready.lock().map(|g| g.clone()).unwrap_or_default();
        let t = u::tokens(ctx);
        let big = self.big_now;
        let mut go: Option<(mg::Pending, Plan)> = None;
        for (i, (p, plan)) in list.iter().enumerate() {
            if self.migrate.ready_hidden.contains(&p.key) {
                continue;
            }
            let shown = p.display.clone().unwrap_or_else(|| p.name.clone());
            u::cascade_in(ui, ("mig_card", &p.key), i, self.home.epoch, |ui| {
                egui::Frame::new().fill(u::col(crate::theme::mix(t.card2, t.accent, 0.12))).stroke(egui::Stroke::new(1.0_f32, u::col(t.accent))).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                        u::paint_glyph(ui.painter(), r, Glyph::ArrowDown, u::col(t.accent_hover));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(self.t(K::VMigCardTitle).replace("{}", &shown)).strong().color(u::col(t.text)));
                            ui.label(RichText::new(self.tf(K::VMigCardBody, &p.source)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let busy = self.migrate.ready_busy.as_ref().map(|(k, _)| *k == p.key).unwrap_or(false);
                            if busy {
                                ui.spinner();
                            } else {
                                let r = u::button(ui, Btn::Ghost, self.t(K::VWizLater));
                                if self.deco(ui, r, None).clicked() {
                                    self.migrate.ready_hidden.insert(p.key.clone());
                                }
                                let r = u::button_with(ui, Btn::Primary, self.t(K::VMigCardGo), None, big);
                                if self.deco(ui, r, None).clicked() && self.migrate.ready_busy.is_none() {
                                    go = Some((p.clone(), plan.clone()));
                                }
                            }
                        });
                    });
                });
            });
            ui.add_space(u::tokens::SP_S);
        }
        // Diagnostic des captures : `card` recupere le premier element propose.
        if go.is_none() && self.migrate.ready_busy.is_none() && self.migrate.demo.iter().any(|d| d == "card") && ctx.input(|i| i.time) > 6.0 {
            if let Some(first) = list.iter().find(|(p, _)| !self.migrate.ready_hidden.contains(&p.key)) {
                go = Some(first.clone());
                self.migrate.demo.retain(|d| d != "card");
            }
        }
        if let Some((p, plan)) = go {
            let Some(vault) = self.model.dest() else { return };
            let slot = Arc::new(Mutex::new(None));
            self.migrate.ready_busy = Some((p.key.clone(), slot.clone()));
            let (c, dd) = (ctx.clone(), self.shared.data_dir.clone());
            std::thread::spawn(move || {
                let cx = make_ctx(&dd);
                let r = mg::apply(&cx, &vault, &plan, &p.source);
                if let Ok(mut g) = slot.lock() {
                    *g = Some(r);
                }
                c.request_repaint();
            });
        }
    }

    // ------------------------------------------------------------ inventaire de ce PC (une fois par jour)

    /// Ecrit l'inventaire des logiciels de ce PC dans le coffre (une fois par jour, en arriere-plan, jamais pendant un
    /// programme suivi ou detecte).
    pub(super) fn migrate_inventory_tick(&mut self) {
        if self.migrate.inv_at.map(|t| t.elapsed() < Duration::from_secs(600)).unwrap_or(false) || self.migrate.inv_busy.load(Ordering::SeqCst) {
            return;
        }
        // premier essai une minute apres le lancement (rien au demarrage)
        if self.migrate.inv_at.is_none() {
            self.migrate.inv_at = Some(Instant::now() - Duration::from_secs(540));
            return;
        }
        self.migrate.inv_at = Some(Instant::now());
        let Some(vault) = self.model.dest() else { return };
        let day = inv::day_of(crate::util::now_ms());
        if self.migrate.inv_day.as_deref() == Some(day.as_str()) || self.dest_ok == Some(false) || self.shared.games_running.load(Ordering::SeqCst) || self.shared.det_running.load(Ordering::SeqCst) {
            return;
        }
        let me = self.model.ctx.machine.clone();
        self.migrate.inv_day = Some(day.clone());
        let (busy, dd) = (self.migrate.inv_busy.clone(), self.shared.data_dir.clone());
        busy.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            if inv::needs_today(&vault, &me, &day) {
                let i = inv::collect(&me, &dd);
                let r = inv::write(&vault, &i);
                Logger::new(dd.join("logs").join("watch.log"), false).log(&format!("[Nouveau PC] inventaire des logiciels du jour : {} logiciels, {} identifiants winget, {} programmes de lanceurs ({})", i.apps.len(), i.winget.len(), i.games.len(), match r {
                    Ok(true) => "ecrit".to_string(),
                    Ok(false) => "deja present".to_string(),
                    Err(e) => format!("non ecrit : {}", e),
                }));
            }
            busy.store(false, Ordering::SeqCst);
        });
    }

    // ------------------------------------------------------------ diagnostic des captures

    fn migrate_demo(&mut self, ctx: &egui::Context) {
        if self.migrate.demo.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let t0 = *self.migrate.demo_t.get_or_insert(now);
        if now - t0 < 1.5 {
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }
        let demo = self.migrate.demo.clone();
        let mut consumed: Vec<String> = Vec::new();
        for full in &demo {
            // `etape@12` : pas avant 12 s (horloge de l'interface) ; l'ordre des etapes est garde.
            let (s, at) = match full.rsplit_once('@').and_then(|(a, b)| b.parse::<f64>().ok().map(|t| (a.to_string(), t))) {
                Some((a, t)) => (a, t),
                None => (full.clone(), 0.0),
            };
            if now < at {
                break;
            }
            let s = &s;
            if let Some(i) = s.strip_prefix("device:").and_then(|n| n.parse::<usize>().ok()) {
                if self.migrate.step == Step::Device {
                    if let Some(d) = self.migrate.devices.lock().ok().and_then(|g| g.clone()).and_then(|l| l.get(i).cloned()) {
                        self.migrate_check(ctx, d);
                        consumed.push(s.clone());
                    }
                }
                break;
            }
            if let Some((name, dir)) = s.strip_prefix("choose:").and_then(|x| x.split_once('=')) {
                if self.migrate.step == Step::Check && self.migrate.check.is_none() {
                    if let Some(p) = self.migrate.plans.iter_mut().find(|p| p.name == name) {
                        mg::choose_place(p, PathBuf::from(dir), false);
                    }
                    consumed.push(s.clone());
                }
                continue;
            }
            if let Some(name) = s.strip_prefix("check:") {
                if self.migrate.step == Step::Check && self.migrate.check.is_none() {
                    if let Some(p) = self.migrate.plans.iter_mut().find(|p| p.name == name) {
                        p.checked = !p.checked;
                    }
                    consumed.push(s.clone());
                }
                continue;
            }
            if let Some(px) = s.strip_prefix("scroll:").and_then(|x| x.parse::<f32>().ok()) {
                if self.migrate.step == Step::Check && self.migrate.check.is_none() {
                    self.migrate.scroll_to = Some(px);
                    consumed.push(s.clone());
                }
                continue;
            }
            if s == "go" {
                if self.migrate.step == Step::Check && self.migrate.check.is_none() && !self.migrate.plans.is_empty() && self.migrate.running.is_empty() && self.migrate.running_at.is_some() {
                    self.migrate_start_run(ctx);
                    consumed.push(s.clone());
                }
                break;
            }
            if let Some(i) = s.strip_prefix("stop:").and_then(|n| n.parse::<usize>().ok()) {
                if let Some(run) = &self.migrate.run {
                    if run.state.lock().map(|st| st.results.len() >= i).unwrap_or(false) {
                        run.request_cancel();
                        consumed.push(s.clone());
                    }
                }
                break;
            }
            if s == "winget" && self.migrate.step == Step::Done {
                self.migrate.winget_open = true;
                consumed.push(s.clone());
                continue;
            }
            if s == "undo-ask" && self.migrate.step == Step::Done {
                self.migrate.undo_ask = true;
                consumed.push(s.clone());
            }
            if s == "open" {
                consumed.push(s.clone());
            }
        }
        let strip = |d: &String| d.rsplit_once('@').filter(|(_, b)| b.parse::<f64>().is_ok()).map(|(a, _)| a.to_string()).unwrap_or_else(|| d.clone());
        self.migrate.demo.retain(|d| !consumed.contains(&strip(d)));
        ctx.request_repaint_after(Duration::from_millis(300));
    }

    /// A appeler a chaque image : lit le diagnostic des captures (`open` ouvre l'ecran).
    pub(super) fn migrate_demo_start(&mut self, ctx: &egui::Context) {
        if self.migrate.demo_read || ctx.input(|i| i.time) < 1.0 {
            return;
        }
        self.migrate.demo_read = true;
        self.migrate.demo = std::env::var("AGSS_UI_MIGRATE_DEMO").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        if self.migrate.demo.iter().any(|d| d == "open") {
            let demo = std::mem::take(&mut self.migrate.demo);
            self.migrate_open(ctx, None);
            self.migrate.demo = demo;
            self.migrate.demo_read = true;
        }
    }
}

// ------------------------------------------------------------ composants de l'ecran

/// Tuile d'un appareil : pastille coloree, nombre d'elements, derniere copie, etiquette eventuelle.
#[allow(clippy::too_many_arguments)]
fn device_card(ui: &mut egui::Ui, id: impl std::hash::Hash, w: f32, name: &str, color: [u8; 3], items: &str, last: &str, tag: Option<&str>, enabled: bool, big: bool) -> egui::Response {
    let t = u::tokens(ui.ctx());
    let k = if big { 1.2 } else { 1.0 };
    let pad = 14.0;
    let h = 96.0 * k;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let resp = ui.interact(rect, ui.id().with(id), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    if ui.is_rect_visible(rect) {
        let m = u::anim::motion(ui.ctx());
        let hov = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered() && enabled, m.dur(u::tokens::DUR_FAST) as f32);
        let p = ui.painter();
        p.rect(rect, u::tokens::R_LG, u::col(crate::theme::mix(t.card2, t.widget, hov * 0.5)), egui::Stroke::new(1.0_f32, u::col(crate::theme::mix(t.line, t.accent, hov))), egui::StrokeKind::Inside);
        // icone d'ordinateur sur la couleur de l'appareil
        let ir = egui::Rect::from_min_size(rect.min + egui::vec2(pad, pad), egui::vec2(40.0 * k, 40.0 * k));
        p.rect_filled(ir, 10.0, u::col(color));
        u::paint_glyph(p, ir.shrink(9.0 * k), Glyph::Pc, u::col(crate::theme::text_on(color)));
        let x = ir.right() + u::tokens::SP_M;
        let tw = (rect.right() - pad - x).max(40.0);
        let g1 = u::layout(ui, name, u::tokens::FS_H3 * k, u::col(t.text), tw, 1);
        let g2 = u::layout(ui, items, u::tokens::FS_BODY * k, u::col(if enabled { t.text } else { t.weak }), tw, 1);
        let g3 = u::layout(ui, last, u::tokens::FS_SMALL * k, u::col(t.weak), tw, 1);
        let mut y = rect.top() + pad - 2.0;
        let (h1, h2) = (g1.size().y, g2.size().y);
        p.galley(egui::pos2(x, y), g1, u::col(t.text));
        y += h1 + 2.0;
        p.galley(egui::pos2(x, y), g2, u::col(t.text));
        y += h2 + 2.0;
        p.galley(egui::pos2(x, y), g3, u::col(t.weak));
        if let Some(tag) = tag {
            let g = u::layout_line(ui, tag, u::tokens::FS_SMALL * k, u::col(t.orange));
            p.galley(egui::pos2(rect.right() - pad - g.size().x, rect.bottom() - pad - g.size().y), g, u::col(t.orange));
        }
        if resp.has_focus() {
            u::focus_ring(ui, rect, u::tokens::R_LG);
        }
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

struct RowSpec<'a> {
    name: &'a str,
    why: &'a str,
    pill: &'a str,
    pill_col: [u8; 3],
    trailing: &'a str,
    checked: bool,
    action: Option<&'a str>,
    icon: (Glyph, [u8; 3]),
}

/// Ligne de la verification : case, icone, nom, explication, etat colore, taille, bouton eventuel.
fn mig_row(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, s: &RowSpec, big: bool) -> (egui::Response, Option<egui::Response>) {
    let t = u::tokens(ui.ctx());
    let k = if big { 1.25 } else { 1.0 };
    let w = ui.available_width();
    let icon = 34.0 * k;
    // texte de l'etat eclairci vers le texte (contraste sur le fond colore doux)
    let pill_fg = crate::theme::mix(s.pill_col, t.text, 0.45);
    let pill_g = u::layout_line(ui, s.pill, u::tokens::FS_SMALL * k, u::col(pill_fg));
    let size_g = u::layout_line(ui, s.trailing, u::tokens::FS_SMALL * k, u::col(t.weak));
    let act_w = s.action.map(|a| u::layout_line(ui, a, u::tokens::FS_BODY * k, u::col(t.text)).size().x + 34.0).unwrap_or(0.0);
    let right_w = pill_g.size().x + 20.0 + size_g.size().x + u::tokens::SP_M + act_w + 16.0;
    let text_w = (w - 28.0 * k - icon - 2.0 * u::tokens::SP_M - right_w).max(80.0);
    let g1 = u::layout(ui, s.name, u::tokens::FS_BODY * k, u::col(t.text), text_w, 1);
    let g2 = u::layout(ui, s.why, u::tokens::FS_SMALL * k, u::col(t.weak), text_w, 2);
    let h = (g1.size().y + g2.size().y + 2.0).max(icon) + 16.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let resp = ui.interact(rect, ui.id().with(&id_salt), egui::Sense::click());
    let mut act = None;
    if ui.is_rect_visible(rect) {
        let ctx = ui.ctx().clone();
        let hov = ctx.animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
        let p = ui.painter().clone();
        if hov > 0.0 {
            p.rect_filled(rect, u::tokens::R_MD, u::col(crate::theme::mix(t.panel, t.row_hover, hov)));
        }
        if resp.has_focus() {
            u::focus_ring(ui, rect, u::tokens::R_MD);
        }
        let cb = egui::Rect::from_min_size(egui::pos2(rect.left() + 8.0, rect.center().y - 10.0 * k), egui::vec2(20.0 * k, 20.0 * k));
        let on = ctx.animate_bool_with_time(resp.id.with("c"), s.checked, 0.12);
        p.rect(cb, 5.0, u::col(crate::theme::mix(t.card2, t.accent, on)), egui::Stroke::new(1.5_f32, u::col(if s.checked { t.accent } else { t.weak })), egui::StrokeKind::Inside);
        if on > 0.0 {
            u::paint_glyph(&p, cb.shrink(3.0), Glyph::Check, u::col_a(t.on_accent, on));
        }
        let ir = egui::Rect::from_min_size(egui::pos2(cb.right() + u::tokens::SP_M, rect.center().y - icon / 2.0), egui::vec2(icon, icon));
        p.rect_filled(ir, 8.0, u::col(s.icon.1));
        u::paint_glyph(&p, ir.shrink(icon * 0.24), s.icon.0, u::col(crate::theme::text_on(s.icon.1)));
        let x = ir.right() + u::tokens::SP_M;
        let th = g1.size().y + g2.size().y + 2.0;
        let y = rect.center().y - th / 2.0;
        let h1 = g1.size().y;
        p.galley(egui::pos2(x, y), g1, u::col(t.text));
        p.galley(egui::pos2(x, y + h1 + 2.0), g2, u::col(t.weak));
        // a droite : bouton eventuel, taille, etat colore
        let mut rx = rect.right() - 8.0;
        if let Some(a) = s.action {
            let br = egui::Rect::from_min_max(egui::pos2(rx - act_w, rect.top() + 4.0), egui::pos2(rx, rect.bottom() - 4.0));
            let r = ui.scope_builder(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| u::button(ui, Btn::Secondary, a)).inner;
            act = Some(r);
            rx = br.left() - u::tokens::SP_S;
        }
        let sw = size_g.size().x;
        p.galley(egui::pos2(rx - sw, rect.center().y - size_g.size().y / 2.0), size_g, u::col(t.weak));
        rx -= sw + u::tokens::SP_M;
        let pw = pill_g.size().x + 16.0;
        let pr = egui::Rect::from_min_size(egui::pos2(rx - pw, rect.center().y - 11.0 * k), egui::vec2(pw, 22.0 * k));
        p.rect_filled(pr, 11.0 * k, u::col_a(s.pill_col, 0.18));
        p.galley(egui::pos2(pr.left() + 8.0, pr.center().y - pill_g.size().y / 2.0), pill_g, u::col(pill_fg));
    }
    (resp.on_hover_cursor(egui::CursorIcon::PointingHand), act)
}

/// Barre de progression (ne recule jamais : la valeur donnee est deja lissee).
fn progress_bar(ui: &mut egui::Ui, f: f32, color: [u8; 3], h: f32) {
    let t = u::tokens(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    ui.painter().rect_filled(rect, h / 2.0, u::col(t.widget));
    if f > 0.0 {
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * f.clamp(0.0, 1.0), h)), h / 2.0, u::col(color));
    }
}
