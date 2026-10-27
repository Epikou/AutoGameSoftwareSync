//! Alerte poids (2.0.1), partie ecran : avant d'ajouter un element lourd (formulaire, selecteur guide, glisser-deposer, carte
//! de detection, assistant), sa taille est mesuree dans un fil de fond puis, au-dessus du seuil, une fenetre montre la taille
//! et l'impact dans le coffre avec trois choix : Garder / Garder moins de versions / Annuler. Dans la fiche, un bloc propose
//! les memes choix ; sur l'accueil, une pastille « Lourd » et un toast (une seule fois) signalent un element qui grossit.
//! Logique pure et testee : `crate::weight`.
use super::*;
use crate::gui::ui::{self as u, Btn, State};
use crate::scan;
use crate::weight::{self, Choice};

/// Ce qui attend la mesure ou le choix de l'utilisateur.
pub(super) enum Job {
    Game(Game, Option<PathBuf>),
    /// Un element de l'assistant (dossier depose ou choisi, ajout rapide) et l'origine de l'ajout pour les donnees d'usage.
    Items(Vec<scan::Item>, &'static str),
}

impl Job {
    fn name(&self) -> String {
        match self {
            Job::Game(g, _) => g.shown().to_string(),
            Job::Items(v, _) => v.first().map(|i| i.name.clone()).unwrap_or_default(),
        }
    }
}

struct Probe {
    job: Job,
    slot: Arc<Mutex<Option<scan::Size>>>,
}

pub(super) struct Pending {
    job: Job,
    size: scan::Size,
}

/// Etat de l'alerte poids entre deux images.
#[derive(Default)]
pub struct WeightUi {
    probes: Vec<Probe>,
    pending: Vec<Pending>,
}

impl WeightUi {
    /// Une mesure ou un choix est-il en cours ?
    pub fn busy(&self) -> bool {
        !self.probes.is_empty() || !self.pending.is_empty()
    }
}

/// Emplacements d'un element a ajouter (principal + supplementaires actifs), pour mesurer sa taille.
pub fn game_locs(g: &Game) -> Vec<scan::Loc> {
    let mut v = vec![scan::Loc { path: g.path.clone(), is_file: g.save_is_file, label: String::new(), filter: g.save_filter.clone(), exclude: g.save_exclude.clone() }];
    v.extend(g.extra_locations.iter().filter(|l| l.enabled).map(|l| scan::Loc { path: l.path.clone(), is_file: l.is_file, label: l.label.clone(), filter: l.filter.clone(), exclude: l.exclude.clone() }));
    v
}

impl App {
    /// « 4,2 Go · environ 12,6 Go dans ton coffre avec 3 versions ».
    pub(super) fn heavy_line(&self, bytes: u64, keep: usize, partial: bool) -> String {
        let size = if partial { self.t(K::VPickAtLeast).replace("{}", &self.bytes(bytes)) } else { self.bytes(bytes) };
        self.t(K::VHeavyLine).replace("{0}", &size).replace("{1}", &self.bytes(weight::impact(bytes, keep))).replace("{2}", &keep.to_string())
    }

    /// Pastille orange « Lourd » (meme dessin que sur la tuile).
    pub(super) fn heavy_pill(&self, ui: &mut egui::Ui) {
        let t = u::tokens(ui.ctx());
        let g = u::layout_line(ui, self.t(K::VHeavyPill), u::tokens::FS_SMALL + 1.0, u::col(t.orange));
        let (r, _) = ui.allocate_exact_size(g.size() + egui::vec2(16.0, 6.0), egui::Sense::hover());
        ui.painter().rect(r, r.height() / 2.0, u::col(crate::theme::mix(t.card, t.orange, 0.22)), egui::Stroke::new(1.0_f32, u::col(crate::theme::mix(t.card, t.orange, 0.6))), egui::StrokeKind::Inside);
        ui.painter().galley(r.center() - g.size() / 2.0, g, u::col(t.orange));
    }

    /// Premiere etape de tout ajout d'un element (`begin_add`) : mesure en fond puis, si lourd, fenetre de choix.
    pub(super) fn weight_gate_game(&mut self, ctx: &egui::Context, g: Game, ep: Option<PathBuf>) {
        let locs = game_locs(&g);
        let slot = self.weight_spawn(ctx, locs);
        self.weight.probes.push(Probe { job: Job::Game(g, ep), slot });
    }

    /// Meme chose pour l'assistant (un element a la fois : dossier depose ou choisi, ajout rapide).
    pub(super) fn weight_gate_items(&mut self, ctx: &egui::Context, items: Vec<scan::Item>, source: &'static str) {
        let known = items.first().and_then(|i| self.scan_size_of(&i.key));
        let slot: Arc<Mutex<Option<scan::Size>>> = match known {
            Some(s) => Arc::new(Mutex::new(Some(s))),
            None => self.weight_spawn(ctx, items.iter().flat_map(|i| i.locs.clone()).collect()),
        };
        self.weight.probes.push(Probe { job: Job::Items(items, source), slot });
    }

    fn weight_spawn(&self, ctx: &egui::Context, locs: Vec<scan::Loc>) -> Arc<Mutex<Option<scan::Size>>> {
        let slot: Arc<Mutex<Option<scan::Size>>> = Arc::new(Mutex::new(None));
        let (s2, c) = (slot.clone(), ctx.clone());
        let spawned = std::thread::Builder::new().name("agss-weight".into()).spawn(move || {
            let size = scan::measure(&locs, &std::sync::atomic::AtomicBool::new(false));
            if let Ok(mut g) = s2.lock() {
                *g = Some(size);
            }
            c.request_repaint();
        });
        if spawned.is_err() {
            // pas de fil : on n'empeche jamais un ajout
            if let Ok(mut g) = slot.lock() {
                *g = Some(scan::Size::default());
            }
        }
        slot
    }

    /// A appeler a chaque image : une mesure terminee continue l'ajout (leger) ou ouvre la fenetre de choix (lourd).
    pub(super) fn weight_poll(&mut self, ctx: &egui::Context) {
        if self.weight.probes.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.weight.probes.len() {
            let ready = self.weight.probes[i].slot.lock().ok().and_then(|g| *g);
            match ready {
                Some(size) => {
                    let p = self.weight.probes.remove(i);
                    if weight::is_heavy(size.bytes) {
                        self.pad_log(&format!("[Poids] {} : {} octets, au-dessus du seuil ({} Mo) : choix demande", p.job.name(), size.bytes, weight::limit_mb()));
                        self.weight.pending.push(Pending { job: p.job, size });
                    } else {
                        self.weight_continue(ctx, p.job, None);
                    }
                }
                None => i += 1,
            }
        }
        if !self.weight.probes.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    /// Ajout reellement lance, avec le nombre de versions choisi (None = reglage par defaut).
    fn weight_continue(&mut self, ctx: &egui::Context, job: Job, keep: Option<usize>) {
        match job {
            Job::Game(g, ep) => {
                self.add_keep = keep;
                self.begin_add_checked(g, ep, ctx);
            }
            Job::Items(items, source) => {
                let mut map = HashMap::new();
                if let Some(k) = keep {
                    for it in &items {
                        map.insert(it.key.clone(), k);
                    }
                }
                self.wizard_add_items_checked(ctx, items, source, map);
            }
        }
    }

    /// Fenetre de choix d'un element lourd (au premier plan de n'importe quelle page).
    pub(super) fn weight_overlay(&mut self, ctx: &egui::Context) {
        let Some(p) = self.weight.pending.first() else { return };
        let tk = u::tokens(ctx);
        let name = p.job.name();
        let line = self.heavy_line(p.size.bytes, weight::HEAVY_KEEP, p.size.partial);
        let mut choice: Option<Choice> = None;
        self.focus.set_modal_scope(true);
        egui::Modal::new(egui::Id::new("weight_ask")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.horizontal(|ui| {
                self.heavy_pill(ui);
                ui.heading(self.t(K::VHeavyTitle));
            });
            ui.add_space(u::tokens::SP_XS);
            ui.label(RichText::new(name).strong().color(u::col(tk.text)));
            ui.add_space(u::tokens::SP_S);
            ui.label(RichText::new(line).size(u::tokens::FS_BODY + 1.0).strong().color(u::col(tk.orange)));
            ui.add_space(u::tokens::SP_XS);
            ui.label(RichText::new(self.t(K::VHeavyBody)).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_M);
            ui.horizontal_wrapped(|ui| {
                let r = u::button(ui, Btn::Primary, self.t(K::VHeavyKeep));
                if self.deco(ui, r, None).clicked() {
                    choice = Some(Choice::Keep);
                }
                let r = u::button(ui, Btn::Secondary, &self.t(K::VHeavyFewer).replace("{}", &weight::fewer_keep().to_string()));
                if self.deco(ui, r, None).on_hover_text(self.t(K::VHeavyFewerTip)).clicked() {
                    choice = Some(Choice::Fewer);
                }
                let r = u::button(ui, Btn::Ghost, self.t(K::VHeavyCancelAdd));
                if self.deco(ui, r, None).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        self.focus.set_modal_scope(false);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(Choice::Cancel);
        }
        if let Some(c) = choice {
            let p = self.weight.pending.remove(0);
            match weight::keep_for_choice(c) {
                Some(k) => self.weight_continue(ctx, p.job, Some(k)),
                None => {
                    self.pad_log(&format!("[Poids] {} : ajout annule", p.job.name()));
                    self.toasts.push(u::anim::now(ctx), self.t(K::VHeavyCancelled), "", State::Paused, None);
                }
            }
        }
    }

    // ------------------------------------------------------------ element deja protege : pastille, toast, fiche

    /// Tailles actuelles des elements (version active du coffre), lues dans les faits de l'accueil.
    fn heavy_sizes(&self) -> Vec<(String, u64)> {
        self.home.facts.iter().map(|f| (f.name.clone(), f.bytes)).collect()
    }

    /// Faits de l'accueil recus : elements devenus lourds, signales UNE fois (un seul toast groupe s'il y en a plusieurs).
    pub(super) fn heavy_check(&mut self, ctx: &egui::Context) {
        if self.home.facts.is_empty() || self.wizard.active {
            return;
        }
        let notified: std::collections::BTreeSet<String> = self.model.settings.heavy_notified.iter().cloned().collect();
        let fresh = weight::newly_heavy(&self.heavy_sizes(), &notified);
        if fresh.is_empty() {
            return;
        }
        for n in &fresh {
            self.model.settings.heavy_notified.push(n.clone());
        }
        let _ = self.model.save_settings();
        let now = u::anim::now(ctx);
        if fresh.len() == 1 {
            let f = self.home.facts.iter().find(|f| f.name == fresh[0]).cloned();
            let shown = self.model.game(&fresh[0]).map(|g| g.shown().to_string()).unwrap_or_else(|| fresh[0].clone());
            let keep = backup::keep_for(&self.model.ctx, &fresh[0]);
            let detail = f.map(|f| self.heavy_line(f.bytes, keep, false)).unwrap_or_default();
            self.toasts.push(now, &self.t(K::VHeavyGrownOne).replace("{}", &shown), &detail, State::Todo, None);
        } else {
            self.toasts.push(now, &self.t(K::VHeavyGrownMany).replace("{}", &fresh.len().to_string()), self.t(K::VHeavyGrownManyHint), State::Todo, None);
        }
        self.pad_log(&format!("[Poids] elements devenus lourds (signales une fois) : {}", fresh.join(", ")));
    }

    /// Bloc de la fiche : taille, impact et trois choix (Garder / Garder moins de versions / Ne plus proteger).
    pub(super) fn fiche_heavy_block(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        let Some(bytes) = self.home.facts.iter().find(|f| f.name == name).map(|f| f.bytes) else { return };
        if !weight::is_heavy(bytes) {
            return;
        }
        let tk = u::tokens(ctx);
        let keep = backup::keep_for(&self.model.ctx, name);
        let line = self.heavy_line(bytes, keep, false);
        let can_fewer = keep > weight::fewer_keep();
        let mut act: Option<Choice> = None;
        u::cascade_in(ui, ("fiche_heavy", name), 1, self.home.epoch, |ui| {
            u::card::card_frame(&tk, false).stroke(egui::Stroke::new(1.5_f32, u::col(tk.orange))).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    self.heavy_pill(ui);
                    ui.label(RichText::new(self.t(K::VHeavyTitle)).strong().color(u::col(tk.state_text(State::Todo))));
                });
                ui.label(RichText::new(line).strong().color(u::col(tk.text)));
                ui.label(RichText::new(self.t(K::VHeavyBody)).color(u::col(tk.weak)));
                ui.add_space(u::tokens::SP_S);
                ui.horizontal_wrapped(|ui| {
                    let r = u::button(ui, Btn::Secondary, self.t(K::VHeavyKeep));
                    if self.deco(ui, r, None).clicked() {
                        act = Some(Choice::Keep);
                    }
                    let r = ui.add_enabled_ui(can_fewer, |ui| u::button(ui, Btn::Secondary, &self.t(K::VHeavyFewer).replace("{}", &weight::fewer_keep().to_string()))).inner;
                    if self.deco(ui, r, None).on_hover_text(self.t(K::VHeavyFewerTip)).clicked() {
                        act = Some(Choice::Fewer);
                    }
                    let r = u::button(ui, Btn::Ghost, self.tm(K::VStopProtecting));
                    if self.deco(ui, r, None).clicked() {
                        act = Some(Choice::Cancel);
                    }
                });
            });
        });
        ui.add_space(u::tokens::SP_M);
        match act {
            Some(Choice::Keep) => {
                if !self.model.settings.heavy_notified.iter().any(|n| n == name) {
                    self.model.settings.heavy_notified.push(name.to_string());
                    let _ = self.model.save_settings();
                }
                self.toasts.push(u::anim::now(ctx), self.t(K::VHeavyKeptToast), "", State::Safe, None);
            }
            Some(Choice::Fewer) => {
                let k = self.model.settings.set_keep(name, weight::fewer_keep());
                self.edit_keep = k;
                let _ = self.model.save_settings();
                self.shared.restart_watch();
                self.toasts.push(u::anim::now(ctx), &self.t(K::VHeavyFewerToast).replace("{}", &k.to_string()), self.t(K::VHeavyFewerToastHint), State::Safe, None);
            }
            Some(Choice::Cancel) => self.fiche.ask = Some(super::fiche_ui::Ask::Stop),
            None => {}
        }
    }
}
