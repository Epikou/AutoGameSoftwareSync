//! Accueil en tuiles (2.0, lot 6) : bandeau d'etat global, filtres, grille de tuiles triees par priorite, menu « ... »,
//! accueil vide, fiche ouverte par un clic (fiche actuelle, refaite au lot 7). Logique pure et testee : `crate::home`.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph, State, TileIcon};
use crate::home::{self, DeviceSync, Filter, Headline, Kind, MainAction, Why};
use crate::i18n::Mode;

/// Etat de l'accueil entre deux images.
#[derive(Default)]
pub struct HomeUi {
    /// Faits calcules en arriere-plan (avec les etats).
    pub facts: Vec<home::Facts>,
    pub filter: Filter,
    /// Recherche de l'accueil (2.0.1) : texte tape ; le champ n'apparait qu'a partir de 12 elements.
    pub query: String,
    /// Cle d'apparition (cascade rejouee a chaque entree sur l'ecran).
    pub epoch: u64,
    seen_frame: u64,
    /// Menu « ... » ouvert : element, position, ouvert au clavier (focus sur la premiere ligne).
    pub menu: Option<(String, egui::Pos2, bool)>,
    menu_frame: u64,
    menu_rect: Option<egui::Rect>,
    /// « Ne plus proteger » en attente de confirmation.
    pub remove_ask: Option<String>,
    /// Navigation au clavier : tuiles de l'image precedente (ordre de la grille), colonnes, tuile a selectionner.
    tile_ids: Vec<egui::Id>,
    cols: usize,
    focus_to: Option<usize>,
    /// Tuile a reselectionner au retour de la fiche.
    pub(super) focus_name: Option<String>,
    busy_prev: Vec<String>,
    /// Diagnostic (captures des lots) : `AGSS_UI_HOME_DEMO=menu:0|remove:0|detail:0|detail:<nom>|focus:2|scan:0|sync:<nom>`, applique une
    /// fois quand les faits sont charges.
    demo: Vec<(String, String, f64)>,
    demo_read: bool,
    /// 2.0.3 : ordre des tuiles fige tant que l'accueil est affiche, et hysteresis du titre.
    pub(super) frozen: home::FrozenOrder,
    pub(super) hold: home::HeadlineHold,
    /// Diagnostic des captures (`AGSS_UI_ORDER_LOG=fichier`) : ordre des tuiles et titre a chaque changement.
    order_log: Option<(PathBuf, String)>,
    demo_sortmenu: bool,
    /// Debut du calme (plus aucune copie) : apres `REORDER_CALM_MS`, l'ordre des tuiles est recalcule s'il a change.
    calm_since: Option<Instant>,
}

/// Une tuile prete a dessiner.
pub(super) struct Item {
    pub(super) name: String,
    pub(super) shown: String,
    pub(super) kind: Kind,
    pub(super) state: State,
    pub(super) status: String,
    pub(super) sync: Option<(String, Option<[u8; 3]>)>,
    pub(super) progress: Option<f32>,
    tip: String,
    recent: u64,
    /// Alerte poids : l'element depasse le seuil (pastille « Lourd »).
    pub(super) heavy: bool,
    /// Tris choisis (2.0.3) : derniere synchro, derniere version, taille, vrai probleme.
    last_sync: u64,
    modified: u64,
    bytes: u64,
    real_problem: bool,
    /// Copie a faire qui partira toute seule dans quelques secondes (pas de copie planifiee) : rien a verifier.
    soon: bool,
}

/// Action demandee pendant le dessin, executee ensuite (aucun emprunt de `self` pendant le dessin).
enum Act {
    Open(String),
    Menu(String, egui::Pos2, bool),
    SyncAll,
    SyncOne(String),
    TimeTravel(String),
    TogglePause(String),
    Remove(String),
    ChooseVault,
    Resume,
    Restart,
    Add,
    Scan,
    NewPc,
}

impl App {
    /// Texte du mode courant (simple par defaut, vocabulaire technique en mode expert).
    pub(super) fn tm(&self, k: K) -> &'static str {
        crate::i18n::tr_mode(self.model.settings.lang, Mode::from_expert(self.model.settings.expert_mode), k)
    }

    pub(super) fn ago_text(&self, now: u64, then: u64) -> String {
        match home::ago(now, then) {
            home::Ago::JustNow => self.t(K::VJustNow).to_string(),
            home::Ago::Min(n) => self.t(K::VAgoMin).replace("{}", &n.to_string()),
            home::Ago::Hour(n) => self.t(K::VAgoHour).replace("{}", &n.to_string()),
            home::Ago::Yesterday => self.t(K::VYesterday).to_lowercase(),
            home::Ago::Day(n) => self.t(K::VAgoDay).replace("{}", &n.to_string()),
        }
    }

    fn why_text(&self, why: &Why, kind: Kind, now: u64) -> String {
        match why {
            Why::Safe(Some(ms)) => self.tm(K::VSafeAgo).replace("{}", &self.ago_text(now, *ms)),
            Why::Safe(None) => self.tm(K::VSafe).to_string(),
            Why::Copying(Some(p)) => self.t(K::VCopyingPct).replace("{}", &p.to_string()),
            Why::Copying(None) => self.t(K::VCopying).to_string(),
            Why::Pending => self.tm(K::VPending).to_string(),
            Why::InUse if kind == Kind::Game => self.t(K::VPausedPlaying).to_string(),
            Why::InUse => self.t(K::VPausedInUse).to_string(),
            Why::PausedByUser => self.t(K::VPausedByYou).to_string(),
            Why::WaitingVault => self.t(K::VWaitingVault).to_string(),
            Why::Locked => self.t(K::VFileLocked).to_string(),
            Why::Problem(_) => self.t(K::VProblem).to_string(),
            Why::Conflict => self.tm(K::VConflict).to_string(),
            Why::NewerFrom(m) if m.is_empty() => self.tm(K::VNewerArrived).to_string(),
            Why::NewerFrom(m) => self.tf(K::VNewerFrom, m),
        }
    }

    fn sync_text(&self, ctx: &egui::Context, ds: &DeviceSync) -> Option<(String, Option<[u8; 3]>)> {
        let me = &self.model.ctx.machine;
        match ds {
            DeviceSync::UpToDateOn(n) => Some((self.t(K::VUpToDateOn).replace("{}", &n.to_string()), Some(u::tokens(ctx).accent_hover))),
            DeviceSync::Waiting(v) => Some((self.t(K::VWaitingOn).replace("{}", &v.join(", ")), v.first().map(|m| self.machine_rgb(m)))),
            DeviceSync::NewerOn(m) => Some((self.t(K::VNewerOn).replace("{}", m), Some(self.machine_rgb(m)))),
            DeviceSync::NewerInVault => Some((self.t(K::VNewerInVault).to_string(), None)),
            DeviceSync::NewerHere => Some((self.t(K::VNewerHere).to_string(), Some(self.machine_rgb(me)))),
            DeviceSync::OnlyHere => Some((self.t(K::VOnlyHere).to_string(), Some(self.machine_rgb(me)))),
            DeviceSync::Conflict | DeviceSync::Unknown => None,
        }
    }

    /// Progression de la copie en cours (0..1), si elle est connue.
    fn copy_fraction(&self) -> Option<f32> {
        let v = self.op_progress.as_ref().map(|(_, p)| p.view()).unwrap_or_else(|| self.shared.watch_progress.view());
        if v.phase == crate::perf::Phase::Idle {
            return None;
        }
        if v.bytes_total > 0 {
            Some(v.bytes_done as f32 / v.bytes_total as f32)
        } else if v.files_total > 0 {
            Some(v.files_done as f32 / v.files_total as f32)
        } else {
            None
        }
    }

    fn sort_label(&self, k: home::SortKey) -> &'static str {
        self.t(match k {
            home::SortKey::Name => K::VSortName,
            home::SortKey::LastSync => K::VSortLastSync,
            home::SortKey::Modified => K::VSortModified,
            home::SortKey::Size => K::VSortSize,
            home::SortKey::State => K::VSortState,
        })
    }

    /// Diagnostic (jamais dans l'interface) : `AGSS_UI_ORDER_LOG=<fichier>` ajoute une ligne « ms | titre | ordre » a chaque
    /// changement de l'ordre des tuiles ou du titre (mesure du clignotement pendant une rafale de synchros).
    fn home_order_log(&mut self, items: &[Item], h: Headline) {
        if self.home.order_log.is_none() {
            match std::env::var_os("AGSS_UI_ORDER_LOG") {
                Some(p) => self.home.order_log = Some((PathBuf::from(p), String::new())),
                None => return,
            }
        }
        let Some((path, last)) = self.home.order_log.as_mut() else { return };
        let states: Vec<String> = items.iter().map(|i| format!("{}={:?}", i.name, i.state)).collect();
        let line = format!("{:?} | {} | epoque {}", h, items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(", "), self.home.epoch);
        if *last != line {
            *last = line.clone();
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path.as_path()) {
                let _ = writeln!(f, "{} | {} | {}", crate::util::now_ms(), line, states.join(" "));
            }
        }
    }

    /// Tuiles de l'accueil (toutes categories), triees par priorite.
    pub(super) fn home_items(&self, ctx: &egui::Context, busy: &[String]) -> Vec<Item> {
        let now = crate::util::now_ms();
        let me = self.model.ctx.machine.clone();
        let lang = self.model.settings.lang;
        let expert = self.model.settings.expert_mode;
        let reachable = self.dest_ok != Some(false) && self.model.dest().is_some();
        let frac = self.copy_fraction();
        let mut items: Vec<Item> = Vec::new();
        for g in &self.model.config.games {
            let status = self.model.statuses.get(&g.name).cloned();
            let mut f = self.home.facts.iter().find(|f| f.name == g.name).cloned().unwrap_or_else(|| {
                let k = if g.exe.as_deref().map(str::trim).unwrap_or("").is_empty() { Kind::Folder } else { Kind::Game };
                home::Facts::bare(g, k, status.clone().unwrap_or(GameStatus::Never))
            });
            if let Some(s) = status {
                f.status = s;
            }
            f.user_paused = self.model.settings.is_paused(&g.name);
            f.shown = g.shown().to_string();
            f.path = g.path.clone();
            let b = busy.iter().any(|x| *x == g.name).then_some(frac);
            let (state, why) = home::item_state(&f, b, reachable, &me);
            let status_txt = self.why_text(&why, f.kind, now);
            let ds = home::device_sync(&f, &me, reachable);
            let sync = self.sync_text(ctx, &ds);
            // Infobulle : texte complet (rien n'est jamais tronque sans recours), details techniques en mode expert.
            let mut tip = vec![f.shown.clone(), status_txt.clone()];
            if let Some((s, _)) = &sync {
                tip.push(s.clone());
            }
            let heavy = crate::weight::is_heavy(f.bytes);
            if heavy {
                let keep = backup::keep_for(&self.model.ctx, &g.name);
                tip.push(self.t(K::VHeavyTip).replace("{}", &self.bytes(crate::weight::limit_bytes())));
                tip.push(self.heavy_line(f.bytes, keep, false));
            }
            if expert {
                tip.push(String::new());
                tip.push(self.t(K::VPathLabel).replace("{}", &f.path.display().to_string()));
                if let Some(h) = &f.head {
                    tip.push(self.t(K::VLastVersion).replace("{0}", &crate::timefmt::format_now_tz(h.saved_ms, lang)).replace("{1}", &h.machine));
                }
                let known = home::devices_known(&f, &me);
                tip.push(self.t(K::VDevicesLabel).replace("{}", &known.join(", ")));
                tip.push(self.t(K::VVersionsCount).replace("{}", &f.versions.to_string()));
                if let Why::Problem(e) = &why {
                    tip.push(e.clone());
                }
                tip.push(self.t(K::TipGameName).to_string());
            }
            let real_problem = state == State::Problem && !matches!(why, Why::Locked);
            items.push(Item {
                name: g.name.clone(),
                shown: f.shown.clone(),
                kind: f.kind,
                state,
                status: status_txt,
                sync,
                progress: if state == State::Busy { frac.or(Some(0.0)) } else { None },
                tip: tip.join("\n"),
                recent: home::recent_ms(&f),
                heavy,
                last_sync: f.last_sync_ms.unwrap_or(0),
                modified: f.head.as_ref().map(|h| h.saved_ms).unwrap_or(0),
                bytes: f.bytes,
                real_problem,
                soon: matches!(why, Why::Pending) && g.schedule.is_none(),
            });
        }
        // Ordre du tri choisi (sans le gel de l'accueil : fiche, captures).
        let order = home::sorted_names(&Self::sort_rows_of(&items), self.model.settings.home_sort);
        Self::reorder(items, &order)
    }

    fn sort_rows_of(items: &[Item]) -> Vec<home::SortRow> {
        items
            .iter()
            .map(|i| home::SortRow { name: i.name.clone(), shown: i.shown.clone(), state: i.state, recent_ms: i.recent, last_sync_ms: i.last_sync, modified_ms: i.modified, bytes: i.bytes, real_problem: i.real_problem })
            .collect()
    }

    fn reorder(mut items: Vec<Item>, order: &[String]) -> Vec<Item> {
        let mut sorted = Vec::with_capacity(items.len());
        for n in order {
            if let Some(p) = items.iter().position(|i| i.name == *n) {
                sorted.push(items.remove(p));
            }
        }
        sorted.extend(items);
        sorted
    }

    /// Selection manette + defilement + infobulle au clavier, sans le cerclage de survol (la tuile a le sien).
    fn tile_deco(&self, ui: &egui::Ui, resp: egui::Response, tip: &str) -> egui::Response {
        self.focus.register(&resp, ItemKind::Button, true);
        let selected = if self.pad_mode { self.focus.is_selected(resp.id) } else { resp.has_focus() };
        if self.pad_mode && selected {
            u::focus_ring(ui, resp.rect, u::tokens::R_LG);
        }
        let scroll = if self.pad_mode { self.focus.take_scroll(resp.id, true) } else { resp.gained_focus() && self.nav_recent > 0 };
        if scroll {
            resp.scroll_to_me(Some(egui::Align::Center));
        }
        let due = if self.pad_mode { uilogic::tooltip_due(self.focus.selected_for_ms()) } else { self.focus_tip_due };
        let resp = resp.on_hover_text(tip);
        if selected && due && !resp.hovered() {
            resp.show_tooltip_text(tip);
        }
        resp
    }

    // ------------------------------------------------------------ page

    /// Accueil, fiche ou Remonter le temps, avec la transition d'ecran (fondu + 8 px) a chaque passage de l'un a l'autre.
    pub(super) fn page_home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let key = match (&self.model.selected, self.fiche.screen) {
            (None, _) => 0,
            (Some(_), fiche_ui::Screen::Fiche) => 1,
            (Some(_), fiche_ui::Screen::TimeTravel) => 2,
        };
        u::page_transition(ui, "home_screen", key, |ui| self.page_home_inner(ui, ctx));
    }

    fn page_home_inner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let frame = ctx.cumulative_pass_nr();
        if self.home.seen_frame == 0 || frame > self.home.seen_frame + 2 {
            self.home.epoch += 1;
        }
        self.home.seen_frame = frame;
        // Une copie vient de finir : etats relus tout de suite (pas d'etat perime affiche).
        let busy = crate::perf::busy_games();
        if busy != self.home.busy_prev {
            if busy.len() < self.home.busy_prev.len() {
                self.last_status = Instant::now() - Duration::from_secs(60);
            }
            self.home.busy_prev = busy.clone();
        }
        if let Some(sel) = self.model.selected.clone() {
            self.home_detail(ui, ctx, &sel);
            return;
        }
        let items = self.home_items(ctx, &busy);
        // Ordre FIGE tant que l'accueil est affiche (2.0.3) : recalcule au retour sur l'accueil, au changement de tri ou de
        // filtre (epoque), quand un element passe en vrai probleme, ou AU CALME (30 s sans aucune copie) si l'ordre a change
        // (jamais pendant une synchro). Un seul redessin est programme a la fin du calme : aucun redessin periodique.
        let rows = Self::sort_rows_of(&items);
        if busy.is_empty() {
            let since = *self.home.calm_since.get_or_insert_with(Instant::now);
            if self.home.frozen.stale(&rows, self.model.settings.home_sort) {
                let left = Duration::from_millis(home::REORDER_CALM_MS).saturating_sub(since.elapsed());
                if left.is_zero() {
                    self.home.epoch += 1;
                    self.home.calm_since = Some(Instant::now());
                } else {
                    ctx.request_repaint_after(left + Duration::from_millis(50));
                }
            }
        } else {
            self.home.calm_since = None;
        }
        let order = self.home.frozen.apply(&rows, self.model.settings.home_sort, self.home.epoch);
        let items = Self::reorder(items, &order);
        let mut act: Option<Act> = None;
        // Diagnostic des captures (AGSS_UI_HOME_DEMO) : menu, confirmation, fiche, selection au clavier, copie.
        if !self.home.demo_read {
            self.home.demo_read = true;
            // `commande:valeur[@secondes]`, plusieurs separees par `;` (la 1re agit des que possible, les autres apres leur
            // delai : mesures de fluidite pendant une grosse copie).
            self.home.demo = std::env::var("AGSS_UI_HOME_DEMO")
                .ok()
                .map(|v| {
                    v.split(';')
                        .filter_map(|e| {
                            let (k, rest) = e.split_once(':')?;
                            let (val, d) = rest.split_once('@').map(|(a, b)| (a, b.parse::<f64>().unwrap_or(2.0))).unwrap_or((rest, 2.0));
                            Some((k.to_string(), val.to_string(), d))
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        let mut demo_menu: Option<usize> = None;
        let now_t = ctx.input(|i| i.time);
        if let Some(pos) = self.home.demo.iter().position(|(_, _, d)| now_t > d.max(2.0)).filter(|_| !self.home.facts.is_empty() || self.model.config.games.is_empty()) {
            let (k, v, _) = self.home.demo[pos].clone();
            // index de la tuile, ou nom de l'element (captures reproductibles quel que soit le tri)
            let idx = v.parse::<usize>().ok().or_else(|| items.iter().position(|i| i.name == v)).unwrap_or(0);
            let name = items.get(idx).map(|i| i.name.clone());
            let mut done = true;
            match k.as_str() {
                "menu" => demo_menu = Some(idx),
                "remove" => act = name.map(Act::Remove),
                "detail" => act = name.map(Act::Open),
                "focus" => {
                    self.home.focus_to = Some(idx);
                    self.focus_from_nav = true;
                    self.nav_recent = 3;
                }
                "scan" => act = Some(Act::Scan),
                "search" => self.home.query = v.clone(),
                "sortmenu" => self.home.demo_sortmenu = true,
                "pause" => act = name.map(Act::TogglePause),
                "restart" => act = Some(Act::Restart),
                "sync" if self.shared.state() == WatchState::Running => act = Some(Act::SyncOne(v.clone())),
                "sync" => done = false,
                _ => {}
            }
            if done {
                self.home.demo.remove(pos);
            }
            ctx.request_repaint_after(Duration::from_millis(300));
        } else if !self.home.demo.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        let big = self.big_now;
        let expert = self.model.settings.expert_mode;

        // ---- bandeau d'etat global (un seul message)
        let watch = match self.shared.state() {
            WatchState::Running if self.shared.paused.load(Ordering::SeqCst) => home::Watch::Paused,
            WatchState::Running | WatchState::Other(_) => home::Watch::Running,
            WatchState::Error(_) => home::Watch::Failed,
            WatchState::Stopped => home::Watch::Starting,
        };
        // 2.0.3 : une modification qui sera copiee toute seule dans quelques secondes n'est pas « a verifier » (la tuile dit
        // « Copie a faire ») ; seule une vraie copie de plus de 2 s change le titre.
        let states: Vec<State> = items.iter().map(|i| if i.state == State::Todo && i.soon && watch == home::Watch::Running { State::Safe } else { i.state }).collect();
        let last = self.home.facts.iter().filter_map(|f| f.last_sync_ms).max();
        let configured = self.model.dest().is_some();
        let raw = home::summary(&states, configured, self.dest_ok != Some(false), watch, last);
        let now = crate::util::now_ms();
        // Hysteresis (2.0.3) : « Synchronisation… » seulement au-dela de 2 s d'activite, et au moins 2 s a l'ecran.
        let s = self.home.hold.apply(raw, now);
        if self.home.hold.pending(now) {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.home_order_log(&items, s.headline);
        let protected = if s.total == 1 { self.t(K::VOneItemProtected).to_string() } else { self.t(K::VItemsProtected).replace("{}", &s.total.to_string()) };
        let (title, detail): (String, String) = match s.headline {
            Headline::NoVault => (self.t(K::VNoVault).into(), self.t(K::VNoVaultDetail).into()),
            Headline::VaultUnreachable => (self.t(K::VVaultUnreachable).into(), self.t(K::VVaultUnreachableDetail).into()),
            Headline::WatchFailed => (self.t(K::VProtectionStopped).into(), self.t(K::VProtectionStoppedDetail).into()),
            Headline::WatchPaused => (self.t(K::VProtectionPaused).into(), self.t(K::VProtectionPausedDetail).into()),
            Headline::ToCheck(n) => (self.t(K::VToCheck).replace("{}", &n.to_string()), self.t(K::VToCheckDetail).into()),
            Headline::Copying(_) => (self.t(K::VSyncingTitle).into(), protected.clone()),
            Headline::Incoming(_) => (self.tm(K::VNewerArrived).into(), protected.clone()),
            Headline::AllSafe => {
                let d = match s.last_copy_ms {
                    Some(ms) if s.total == 1 => self.t(K::VOneItemSafeDetail).replace("{}", &self.ago_text(now, ms)),
                    Some(ms) => self.t(K::VAllSafeDetail).replace("{0}", &s.total.to_string()).replace("{1}", &self.ago_text(now, ms)),
                    None => protected.clone(),
                };
                (self.tm(K::VAllSafe).into(), d)
            }
            Headline::Empty => (String::new(), String::new()),
        };
        if s.headline != Headline::Empty {
            let label = s.action.map(|a| match a {
                MainAction::SyncAll => self.tm(K::VSyncAllNow),
                MainAction::ChooseVault => self.t(K::VChooseVault),
                MainAction::Resume => self.t(K::VResume),
                MainAction::Restart => self.t(K::VRestart),
            });
            let spec = u::BannerSpec { state: s.state, title: &title, detail: &detail, action: label };
            if let Some(b) = u::status_banner(ui, &spec) {
                let tip = if expert { Some(K::TipSyncNow) } else { None };
                let b = if s.action == Some(MainAction::SyncAll) { self.deco(ui, b, tip) } else { self.deco(ui, b, None) };
                if b.clicked() {
                    act = Some(match s.action {
                        Some(MainAction::ChooseVault) => Act::ChooseVault,
                        Some(MainAction::Resume) => Act::Resume,
                        Some(MainAction::Restart) => Act::Restart,
                        _ => Act::SyncAll,
                    });
                }
            }
            ui.add_space(u::tokens::SP_M);
        }

        // ---- Nouveau PC : programmes installes depuis, dont les donnees peuvent etre remises (recuperation differee)
        self.migrate_cards(ui, ctx);
        self.prefs_home_card(ui, ctx);
        // 2.0.3 : jeux installes dans Playnite pas encore proteges (carte discrete).
        if !items.is_empty() {
            self.playnite_home_card(ui, ctx);
        }

        // ---- accueil vide
        if items.is_empty() {
            self.home_empty(ui, &mut act);
            self.home.tile_ids.clear();
            self.home_apply(act, ctx);
            return;
        }

        // ---- recherche (2.0.1) : a partir de 12 elements, filtrage instantane combine aux filtres de categorie
        let searchable = home::search_visible(items.len());
        if !searchable {
            self.home.query.clear();
        } else {
            self.home_search_row(ui, ctx);
        }
        let rows: Vec<(Kind, Vec<&str>)> = items.iter().map(|i| (i.kind, vec![i.shown.as_str(), i.name.as_str()])).collect();
        let counts = home::filter_counts_for(&rows, &self.home.query);

        // ---- filtres (compteurs)
        ui.horizontal_wrapped(|ui| {
            // Menu de tri (2.0.3), juste a gauche des filtres ; choix memorise dans les reglages.
            let cur = self.model.settings.home_sort;
            let mut pick: Option<home::SortKey> = None;
            if std::mem::take(&mut self.home.demo_sortmenu) {
                let id = ui.make_persistent_id("home_sort").with("popup");
                ui.memory_mut(|m| m.open_popup(id));
            }
            let cb = egui::ComboBox::from_id_salt("home_sort").selected_text(self.sort_label(cur)).show_ui(ui, |ui| {
                for k in home::SortKey::ALL {
                    if ui.selectable_label(k == cur, self.sort_label(k)).clicked() {
                        pick = Some(k);
                    }
                }
            });
            let r = self.deco(ui, cb.response, None).on_hover_text(self.t(K::VSortTip));
            if self.pad_mode && r.clicked() && pick.is_none() {
                // Manette : A fait defiler les tris (le menu deroulant reste utilisable a la souris).
                let i = home::SortKey::ALL.iter().position(|k| *k == cur).unwrap_or(0);
                pick = Some(home::SortKey::ALL[(i + 1) % home::SortKey::ALL.len()]);
                ui.memory_mut(|m| m.close_popup());
            }
            if let Some(k) = pick.filter(|k| *k != cur) {
                self.model.settings.home_sort = k;
                let _ = self.model.save_settings();
                self.home.epoch += 1;
            }
            for (i, (f, k)) in Filter::ALL.iter().zip([K::VFilterAll, K::VFilterGames, K::VFilterApps, K::VFilterFolders]).enumerate() {
                let r = u::chip(ui, self.t(k), Some(counts[i]), self.home.filter == *f);
                if self.deco(ui, r, None).clicked() && self.home.filter != *f {
                    self.home.filter = *f;
                    self.home.epoch += 1;
                }
            }
            // Scan du PC (lot 9) : propose ce qui n'est pas encore protege.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = u::button_with(ui, Btn::Ghost, self.t(K::VFindOnPc), Some(Glyph::Eye), false);
                if self.deco(ui, r, None).on_hover_text(self.t(K::VScanPcHint)).clicked() {
                    act = Some(Act::Scan);
                }
                // Nouveau PC (lot 12) : seulement si le coffre contient des elements d'un autre appareil absents d'ici.
                if self.home_new_pc_hint() {
                    let r = u::button_with(ui, Btn::Ghost, self.t(K::VMigEntry), Some(Glyph::Pc), false);
                    if self.deco(ui, r, None).on_hover_text(self.t(K::VMigEntryHint)).clicked() {
                        act = Some(Act::NewPc);
                    }
                }
            });
        });
        ui.add_space(u::tokens::SP_M);

        // ---- grille
        let filter = self.home.filter;
        let vis = home::visible_indices(&rows, filter, &self.home.query);
        let shown: Vec<&Item> = vis.iter().map(|i| &items[*i]).collect();
        let mut textures: Vec<Option<egui::TextureHandle>> = Vec::with_capacity(shown.len());
        for it in &shown {
            textures.push(if it.kind == Kind::Folder { None } else { self.icon_for(ctx, &it.name) });
        }
        let epoch = self.home.epoch;
        let focus_to = self.home.focus_to.take();
        let focus_name = if self.focus_from_nav { self.home.focus_name.take() } else { self.home.focus_name.take().and(None) };
        let mut ids: Vec<egui::Id> = Vec::new();
        let mut cols_used = 1;
        egui::ScrollArea::vertical().id_salt("home").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width((ui.available_width() - 14.0).max(120.0));
            if shown.is_empty() {
                let t = u::tokens(ui.ctx());
                let msg = if self.home.query.trim().is_empty() { self.t(K::VFilterEmpty).to_string() } else { self.t(K::VSearchEmpty).replace("{}", self.home.query.trim()) };
                ui.label(RichText::new(msg).color(u::col(t.weak)));
                ui.add_space(u::tokens::SP_M);
            }
            let avail = ui.available_width();
            let cols = u::tokens::grid_columns(avail, big);
            let (bw, _) = u::tokens::tile_size(big);
            let w = home::tile_width(avail, cols, bw, u::tokens::TILE_GAP);
            cols_used = cols;
            let total = shown.len() + 1;
            let mut i = 0;
            while i < total {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = u::tokens::TILE_GAP;
                    for _ in 0..cols {
                        if i < shown.len() {
                            let it = shown[i];
                            // 2.0.3 : vraie icone du programme ; sinon icone de sa categorie sur un fond teinte (plus de lettre).
                            let icon = match (&textures[i], it.kind) {
                                (Some(tex), _) => TileIcon::Texture(tex),
                                (None, Kind::Folder) => TileIcon::Glyph(Glyph::Folder, [0x2f, 0x6a, 0x55]),
                                (None, Kind::App) => TileIcon::Glyph(Glyph::Window, home::initial_color(&it.shown)),
                                (None, Kind::Game) => TileIcon::Glyph(Glyph::Pad, home::initial_color(&it.shown)),
                            };
                            let spec = u::TileSpec {
                                name: &it.shown,
                                icon,
                                state: it.state,
                                status: &it.status,
                                progress: it.progress,
                                index: i,
                                epoch,
                                sync: it.sync.as_ref().map(|(s, c)| (s.as_str(), *c)),
                                width: Some(w),
                                heavy: it.heavy.then(|| self.t(K::VHeavyPill)),
                            };
                            let r = u::tile(ui, ("home", &it.name), &spec);
                            let resp = self.tile_deco(ui, r.response, &it.tip);
                            if focus_to == Some(i) || focus_name.as_deref() == Some(it.name.as_str()) {
                                resp.request_focus();
                                resp.scroll_to_me(None);
                            }
                            ids.push(resp.id);
                            let key_menu = resp.has_focus() && ui.input(|x| x.modifiers.shift && x.key_pressed(egui::Key::F10));
                            let key_del = resp.has_focus() && ui.input(|x| x.key_pressed(egui::Key::Delete));
                            let menu_tip = if expert { self.t(K::TipGameName) } else { self.t(K::VMoreActions) };
                            let menu = r.menu.on_hover_text(menu_tip);
                            if demo_menu == Some(i) {
                                act = Some(Act::Menu(it.name.clone(), menu.rect.left_bottom() + egui::vec2(-150.0, 4.0), true));
                            } else if menu.clicked() {
                                act = Some(Act::Menu(it.name.clone(), menu.rect.left_bottom() + egui::vec2(-150.0, 4.0), false));
                            } else if resp.secondary_clicked() {
                                let p = ui.input(|x| x.pointer.interact_pos()).unwrap_or(resp.rect.center());
                                act = Some(Act::Menu(it.name.clone(), p, false));
                            } else if key_menu {
                                act = Some(Act::Menu(it.name.clone(), resp.rect.center(), true));
                            } else if key_del {
                                act = Some(Act::Remove(it.name.clone()));
                            } else if resp.clicked() {
                                act = Some(Act::Open(it.name.clone()));
                            }
                        } else if i == shown.len() {
                            let r = u::add_tile_sized(ui, "home_add", self.t(K::VProtectElse), i, epoch, Some(w));
                            let r = self.tile_deco(ui, r, self.t(K::VTipAdd));
                            if focus_to == Some(i) {
                                r.request_focus();
                                r.scroll_to_me(None);
                            }
                            ids.push(r.id);
                            if r.clicked() {
                                act = Some(Act::Add);
                            }
                        }
                        i += 1;
                    }
                });
                ui.add_space(u::tokens::TILE_GAP);
            }
        });
        self.home.tile_ids = ids;
        self.home.cols = cols_used;
        self.home_apply(act, ctx);
    }

    /// Champ de recherche au-dessus des tuiles (2.0.1). Ctrl+F ou « / » pour y aller, Echap pour effacer.
    fn home_search_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = u::tokens(ctx);
        let id = egui::Id::new("home_search");
        let modal = self.home.menu.is_some() || self.home.remove_ask.is_some() || self.model.confirm.is_some() || self.weight.busy();
        let focused = ctx.memory(|m| m.has_focus(id));
        if !modal {
            let ctrl_f = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F));
            let slash = !focused && !ctx.wants_keyboard_input() && ctx.input(|i| !i.modifiers.command && !i.modifiers.alt && i.key_pressed(egui::Key::Slash));
            if ctrl_f || slash {
                ctx.memory_mut(|m| m.request_focus(id));
                if slash {
                    // le « / » tape ne doit pas entrer dans le champ
                    ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == "/")));
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && (focused || !self.home.query.is_empty()) {
                self.home.query.clear();
                ctx.memory_mut(|m| m.surrender_focus(id));
                self.home.epoch += 1;
            }
        }
        let before = self.home.query.clone();
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 40.0).clamp(160.0, 360.0);
            let hint = self.t(K::VSearchHint);
            let mut q = std::mem::take(&mut self.home.query);
            let stroke = egui::Stroke::new(1.0_f32, u::col(if focused { t.accent } else { t.line }));
            let r = egui::Frame::new()
                .fill(u::col(t.card2))
                .stroke(stroke)
                .corner_radius(u::tokens::R_MD)
                .inner_margin(egui::Margin::symmetric(10, 5))
                .show(ui, |ui| ui.add(egui::TextEdit::singleline(&mut q).id(id).frame(false).hint_text(hint).desired_width(w - 20.0)))
                .inner;
            self.home.query = q;
            self.deco_kind(ui, r, None, ItemKind::Text).on_hover_text(self.t(K::VSearchTip));
            if !self.home.query.is_empty() {
                let r = u::button(ui, Btn::Ghost, self.t(K::VSearchClear));
                if self.deco(ui, r, None).clicked() {
                    self.home.query.clear();
                }
            }
        });
        if self.home.query != before {
            // les tuiles qui restent se replacent avec la cascade
            self.home.epoch += 1;
        }
        ui.add_space(u::tokens::SP_S);
    }

    /// Accueil vide : invitation claire, recherche sur le PC (detection et connaissance existantes) ou ajout a la main.
    fn home_empty(&mut self, ui: &mut egui::Ui, act: &mut Option<Act>) {
        let t = u::tokens(ui.ctx());
        let big = self.big_now;
        let epoch = self.home.epoch;
        u::cascade_in(ui, "home_empty", 0, epoch, |ui| {
            u::card(ui, None, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(u::tokens::SP_L);
                    let s = if big { 84.0 } else { 68.0 };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                    u::status_banner::paint_shield(ui.painter(), r, State::Safe, 1.0);
                    ui.add_space(u::tokens::SP_M);
                    let w = (ui.available_width() - 40.0).clamp(200.0, 560.0);
                    let g = u::layout(ui, self.t(K::VEmptyTitle), if big { u::tokens::FS_H2 * 1.3 } else { u::tokens::FS_H2 + 2.0 }, u::col(t.text), w, 2);
                    let (r1, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
                    ui.painter().galley(r1.min, g, u::col(t.text));
                    ui.add_space(u::tokens::SP_S);
                    let g2 = u::layout(ui, self.t(K::VEmptyBody), if big { u::tokens::FS_BODY * 1.2 } else { u::tokens::FS_BODY }, u::col(t.weak), w, 4);
                    let (r2, _) = ui.allocate_exact_size(g2.size(), egui::Sense::hover());
                    ui.painter().galley(r2.min, g2, u::col(t.weak));
                    ui.add_space(u::tokens::SP_L);
                    ui.horizontal(|ui| {
                        // centre les deux boutons
                        let a = u::layout_line(ui, self.t(K::VFindOnPc), u::tokens::FS_BIG_BUTTON, u::col(t.text)).size().x;
                        let b = u::layout_line(ui, self.t(K::VAddMyself), u::tokens::FS_BIG_BUTTON, u::col(t.text)).size().x;
                        let total = a + b + 4.0 * u::tokens::BTN_PAD_BIG.1 + 60.0;
                        ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                        let r = u::button_with(ui, Btn::Primary, self.t(K::VFindOnPc), Some(Glyph::Eye), true);
                        if self.deco(ui, r, None).clicked() {
                            *act = Some(Act::Scan);
                        }
                        let r = u::button_with(ui, Btn::Secondary, self.t(K::VAddMyself), Some(Glyph::Plus), true);
                        if self.deco(ui, r, None).clicked() {
                            *act = Some(Act::Add);
                        }
                    });
                    // Nouveau PC (lot 12) : le coffre contient deja les elements d'un autre appareil.
                    if self.home_new_pc_hint() {
                        ui.add_space(u::tokens::SP_M);
                        let r = u::button_with(ui, Btn::Ghost, self.t(K::VMigSettingsBtn), Some(Glyph::Pc), true);
                        if self.deco(ui, r, None).on_hover_text(self.t(K::VMigEntryHint)).clicked() {
                            *act = Some(Act::NewPc);
                        }
                    }
                    ui.add_space(u::tokens::SP_L);
                });
            });
        });
    }

    /// Fiche d'un element (lot 7, `fiche_ui`).
    fn home_detail(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        self.fiche_page(ui, ctx, name);
    }

    /// Retour a l'accueil depuis la fiche : la tuile de l'element est reselectionnee.
    pub(super) fn fiche_close(&mut self, name: &str) {
        self.model.selected = None;
        self.history = None;
        self.link_open = false;
        self.fiche.ask = None;
        self.fiche.screen = fiche_ui::Screen::Fiche;
        self.home.focus_name = Some(name.to_string());
        self.home.epoch += 1;
    }

    // ------------------------------------------------------------ actions

    pub(super) fn home_sync(&mut self, ctx: &egui::Context, game: Option<String>) {
        let now = ui::anim::now(ctx);
        if self.shared.state() != WatchState::Running {
            self.toasts.push(now, self.t(K::VProtectionStopped), self.t(K::VProtectionStoppedDetail), State::Problem, None);
            return;
        }
        if let Ok(mut q) = self.shared.sync_requests.lock() {
            q.push(game);
        }
        self.toasts.push(now, self.t(K::VSyncStarted), "", State::Busy, None);
    }

    /// Pause / reprise d'un element (accueil et fiche) : rien n'est supprime ; toast de confirmation.
    pub(super) fn toggle_pause(&mut self, ctx: &egui::Context, n: &str) {
        let p = !self.model.settings.is_paused(n);
        self.model.settings.game_mut(n).paused = p;
        let _ = self.model.save_settings();
        self.shared.restart_watch();
        self.last_status = Instant::now() - Duration::from_secs(60);
        crate::perf::log(&self.model.ctx, &format!("[{}] UI : {}", n, if p { "mis en pause" } else { "repris" }));
        let shown = self.model.game(n).map(|g| g.shown().to_string()).unwrap_or_else(|| n.to_string());
        let (k, st) = if p { (K::VPausedToast, State::Paused) } else { (K::VResumedToast, State::Safe) };
        self.toasts.push(ui::anim::now(ctx), &self.t(k).replace("{}", &shown), "", st, None);
    }

    fn home_open(&mut self, name: &str, ctx: &egui::Context) {
        crate::usage::record(&self.shared.data_dir, "time_travel", &[("action", "opened")]);
        self.select_game(name, ctx);
        self.fiche.focus_pending = fiche_ui::opened_by_keyboard(ctx);
    }

    fn home_apply(&mut self, act: Option<Act>, ctx: &egui::Context) {
        let Some(a) = act else { return };
        match a {
            Act::Open(n) => self.home_open(&n, ctx),
            Act::TimeTravel(n) => {
                self.home_open(&n, ctx);
                self.fiche.screen = fiche_ui::Screen::TimeTravel;
            }
            Act::Menu(n, p, kb) => {
                self.home.menu = Some((n, p, kb));
                self.home.menu_frame = ctx.cumulative_pass_nr();
                self.home.menu_rect = None;
            }
            Act::SyncAll => self.home_sync(ctx, None),
            Act::SyncOne(n) => self.home_sync(ctx, Some(n)),
            Act::TogglePause(n) => self.toggle_pause(ctx, &n),
            Act::Remove(n) => self.home.remove_ask = Some(n),
            Act::ChooseVault => self.go_choose_share(ctx),
            Act::Resume => self.shared.paused.store(false, Ordering::SeqCst),
            Act::Restart => self.shared.restart_watch(),
            Act::Add => {
                self.model.page = Page::Add;
                self.on_page_enter(Page::Add, ctx);
            }
            Act::Scan => self.scan_open(ctx),
            Act::NewPc => self.migrate_open(ctx, None),
        }
    }

    /// Le coffre contient-il des elements d'un autre appareil absents de ce PC ? (bouton « Nouveau PC ? »)
    fn home_new_pc_hint(&self) -> bool {
        if self.model.dest().is_none() || self.dest_games.is_empty() {
            return false;
        }
        let local = crate::migrate::local_keys(&self.model.config);
        self.dest_games.iter().any(|d| !local.contains(&d.key))
    }

    /// Menu « ... » et confirmation « Ne plus proteger » (au-dessus de tout, accueil seulement).
    pub(super) fn home_overlays(&mut self, ctx: &egui::Context) {
        if self.model.page != Page::Games {
            self.home.menu = None;
            self.home.remove_ask = None;
            return;
        }
        let mut act: Option<Act> = None;
        if let Some((name, pos, kb)) = self.home.menu.clone() {
            let t = u::tokens(ctx);
            let big = self.big_now;
            let paused = self.model.settings.is_paused(&name);
            let expert = self.model.settings.expert_mode;
            let can_sync = !paused && self.model.dest().is_some();
            let first_frame = ctx.cumulative_pass_nr() == self.home.menu_frame + 1 || ctx.cumulative_pass_nr() == self.home.menu_frame;
            self.focus.set_modal_scope(true);
            let mut close = false;
            let area = egui::Area::new(egui::Id::new("home_menu")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ctx, |ui| {
                egui::Frame::new()
                    .fill(u::col(t.card2))
                    .stroke(egui::Stroke::new(1.0_f32, u::col(t.line)))
                    .corner_radius(u::tokens::R_MD)
                    .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 16, spread: 0, color: u::col_a([0, 0, 0], t.shadow_alpha as f32 / 255.0) })
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        let mw = if big { 280.0 } else { 230.0 };
                        ui.set_min_width(mw);
                        ui.set_max_width(mw);
                        let rows: [(K, Option<K>, bool, bool); 4] = [
                            (K::VSyncNow, Some(K::TipSyncNowGame), can_sync, false),
                            (K::VTimeTravel, Some(K::TipVersions), true, false),
                            (if paused { K::VResume } else { K::VPause }, None, true, false),
                            (K::VStopProtecting, Some(K::TipRemove), true, true),
                        ];
                        for (i, (k, tip, enabled, danger)) in rows.iter().enumerate() {
                            let r = menu_row(ui, ("home_menu_row", i), self.tm(*k), *danger, *enabled, big);
                            self.focus.register(&r, ItemKind::Button, false);
                            if kb && first_frame && i == 0 {
                                r.request_focus();
                            }
                            let r = match (expert, tip) {
                                (true, Some(tk)) => r.on_hover_text(self.t(*tk)),
                                _ => r,
                            };
                            if r.clicked() {
                                close = true;
                                act = Some(match i {
                                    0 => Act::SyncOne(name.clone()),
                                    1 => Act::TimeTravel(name.clone()),
                                    2 => Act::TogglePause(name.clone()),
                                    _ => Act::Remove(name.clone()),
                                });
                            }
                        }
                    });
            });
            self.focus.set_modal_scope(false);
            let rect = area.response.rect;
            self.home.menu_rect = Some(rect);
            let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().map(|p| !rect.contains(p)).unwrap_or(false));
            if (outside && ctx.cumulative_pass_nr() > self.home.menu_frame + 1) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
                if kb {
                    self.home.focus_name = Some(name.clone());
                }
            }
            if close {
                self.home.menu = None;
            }
        }
        if let Some(n) = self.home.remove_ask.clone() {
            let shown = self.model.game(&n).map(|g| g.shown().to_string()).unwrap_or_else(|| n.clone());
            let title = self.tm(K::VStopTitle).replace("{}", &shown);
            let spec = u::ConfirmSpec { title: &title, body: self.t(K::VStopBody), confirm: self.tm(K::VStopProtecting), cancel: self.t(K::BtnCancel), danger: true };
            match u::confirm_dialog(ctx, ("home_remove", &n), &spec) {
                Some(u::DialogAnswer::Confirm) => {
                    self.home.remove_ask = None;
                    self.run_confirmed(Confirm::Remove { game: n }, ctx);
                }
                Some(u::DialogAnswer::Cancel) => {
                    self.home.remove_ask = None;
                    self.home.focus_name = Some(n);
                }
                None => {}
            }
        }
        self.home_apply(act, ctx);
    }

    /// Clavier : fleches, Debut, Fin entre les tuiles (grille), avant qu'egui ne deplace lui-meme le focus.
    pub(super) fn home_key_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.model.page != Page::Games || self.model.selected.is_some() || self.home.menu.is_some() || self.home.remove_ask.is_some() || self.model.confirm.is_some() || self.pad_mode {
            return;
        }
        let Some(f) = ctx.memory(|m| m.focused()) else { return };
        let Some(mut cur) = self.home.tile_ids.iter().position(|id| *id == f) else { return };
        let (n, cols) = (self.home.tile_ids.len(), self.home.cols);
        let mut target = None;
        raw.events.retain(|e| match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } if modifiers.is_none() => {
                let d = match key {
                    egui::Key::ArrowLeft => home::Dir::Left,
                    egui::Key::ArrowRight => home::Dir::Right,
                    egui::Key::ArrowUp => home::Dir::Up,
                    egui::Key::ArrowDown => home::Dir::Down,
                    egui::Key::Home => home::Dir::Home,
                    egui::Key::End => home::Dir::End,
                    _ => return true,
                };
                match home::grid_neighbor(cur, cols, n, d) {
                    Some(j) => {
                        cur = j;
                        target = Some(j);
                        false
                    }
                    None => true, // au-dessus de la premiere ligne : egui remonte vers les filtres
                }
            }
            _ => true,
        });
        if target.is_some() {
            self.home.focus_to = target;
            self.focus_from_nav = true;
            self.nav_recent = 3;
        }
    }
}

/// Ligne du menu « ... » (texte seul ; action destructrice en rouge).
fn menu_row(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, text: &str, danger: bool, enabled: bool, big: bool) -> egui::Response {
    let t = u::tokens(ui.ctx());
    let h = if big { 40.0 } else { 32.0 };
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let resp = ui.interact(rect, ui.id().with(id_salt), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let hov = enabled && (resp.hovered() || resp.has_focus());
    if hov {
        ui.painter().rect_filled(rect, u::tokens::R_SM, u::col(t.widget));
    }
    if resp.has_focus() {
        u::focus_ring(ui, rect, u::tokens::R_SM);
    }
    let c = if !enabled {
        t.faint
    } else if danger {
        t.state_text(State::Problem)
    } else {
        t.text
    };
    let g = u::layout(ui, text, if big { u::tokens::FS_BIG_BUTTON } else { u::tokens::FS_BODY }, u::col(c), w - 20.0, 1);
    ui.painter().galley(egui::pos2(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, u::col(c));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}
