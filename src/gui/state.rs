//! Etat de l'interface : partage entre threads (`Shared`), messages des threads de travail, structure `App`.
use super::*;

// ================================================================== partage entre threads

#[derive(Clone, Debug, PartialEq)]
pub enum WatchState {
    Stopped,
    Running,
    /// Un autre processus (watch CLI) tient deja la surveillance.
    Other(String),
    Error(String),
}

pub(super) struct WatchHandle {
    pub(super) stop: Arc<AtomicBool>,
    pub(super) join: std::thread::JoinHandle<()>,
}

pub struct Shared {
    pub data_dir: PathBuf,
    pub quitting: AtomicBool,
    pub paused: Arc<AtomicBool>,
    pub(super) watch: Mutex<Option<WatchHandle>>,
    /// Arret / redemarrage de la surveillance demandes par l'interface : faits dans un fil de fond (attendre la fin d'une grosse
    /// copie bloquerait la fenetre). `watch_change_pending` : demandes a traiter ; `watch_change_busy` : fils en cours ;
    /// `watch_want_start` : la derniere demande veut-elle une surveillance en marche ?
    pub(super) watch_change_lock: Mutex<()>,
    pub(super) watch_change_pending: std::sync::atomic::AtomicUsize,
    pub(super) watch_change_busy: std::sync::atomic::AtomicUsize,
    pub(super) watch_want_start: AtomicBool,
    pub watch_state: Mutex<WatchState>,
    pub(super) gui_lock: Mutex<Option<InstanceLock>>,
    /// Evenements a montrer dans l'interface (ex. conflit resolu automatiquement) : (texte, succes).
    pub ui_events: Mutex<Vec<(String, bool)>>,
    /// Dossier partage joignable ? 0 = inconnu / aucun dossier, 1 = oui, 2 = non (sonde en arriere-plan).
    pub dest_reach: std::sync::atomic::AtomicU8,
    /// Des taches planifiees de la version precedente attendent une mise a jour avec autorisation administrateur.
    pub task_migration_pending: AtomicBool,
    /// Conflits en attente et erreurs recentes (icone de la zone de notification).
    pub tracker: Mutex<crate::trayicon::Tracker>,
    /// Derniere version publiee connue (alerte de mise a jour) et derniere version deja notifiee.
    pub update_latest: Mutex<Option<crate::update::ReleaseInfo>>,
    pub update_notified: Mutex<Option<String>>,
    /// Demandes « Synchroniser maintenant » (None = tous les jeux) lues par la surveillance.
    pub sync_requests: Arc<Mutex<Vec<Option<String>>>>,
    /// Un etat de jeu / de surveillance a change : recalculer `status.json`.
    pub status_dirty: AtomicBool,
    /// Un jeu surveille tourne (renseigne par la surveillance ; sert a liberer la manette).
    pub games_running: Arc<AtomicBool>,
    pub transitions: Mutex<crate::transitions::Transitions>,
    pub proposals: Mutex<crate::gamedetect::Proposals>,
    /// Tutoriel ou confirmation ouvert dans l interface (renseigne a chaque image) : pas de passage au premier plan.
    pub ui_busy: AtomicBool,
    /// settings.json modifie hors de l interface (menu de la zone de notification) : relire le reglage.
    pub settings_dirty: AtomicBool,
    /// Dernier document ecrit (ecriture finale `watching:false` a la fermeture).
    pub status_last: Mutex<Option<crate::statusfile::StatusDoc>>,
    /// Progression des synchros declenchees par la surveillance (barre de l'interface).
    pub watch_progress: Arc<crate::perf::Progress>,
    /// Un programme detecte tourne (renseigne par la detection) : aucune recherche en ligne pendant ce temps.
    pub det_running: AtomicBool,
    /// Demandes de recherche en ligne en attente (connaissance intelligente, couche 4).
    pub know_jobs: Mutex<Vec<(knowledge_ui::KnowTarget, crate::knowledge::Program)>>,
    /// Cles de cartes dont les emplacements appris viennent de changer (a re-resoudre si la carte est affichee).
    pub know_refresh: Mutex<Vec<String>>,
    /// Envoi des donnees d'usage a faire tout de suite (consentement donne, effacement demande).
    pub usage_kick: AtomicBool,
    /// Mise a jour automatique : etat partage avec le thread de controle (voir `selfupdate`).
    pub upd: Arc<crate::selfupdate::Runtime>,
    /// Images dessinees par l'interface (une mise a jour est « saine » quand l'interface tourne) et resultat de mise a jour a annoncer.
    pub ui_frames: std::sync::atomic::AtomicU32,
    pub upd_report: Mutex<Option<crate::selfupdate::Report>>,
    /// Contexte egui de la fenetre : les threads de fond reveillent l interface quand un etat visible change (aucun
    /// redessin periodique au repos).
    pub ui_ctx: Mutex<Option<egui::Context>>,
}

impl Shared {
    pub(super) fn new(data_dir: PathBuf, lock: InstanceLock) -> Arc<Shared> {
        Arc::new(Shared {
            data_dir,
            quitting: AtomicBool::new(false),
            paused: Arc::new(AtomicBool::new(false)),
            watch: Mutex::new(None),
            watch_change_lock: Mutex::new(()),
            watch_change_pending: std::sync::atomic::AtomicUsize::new(0),
            watch_change_busy: std::sync::atomic::AtomicUsize::new(0),
            watch_want_start: AtomicBool::new(false),
            watch_state: Mutex::new(WatchState::Stopped),
            gui_lock: Mutex::new(Some(lock)),
            ui_events: Mutex::new(Vec::new()),
            dest_reach: std::sync::atomic::AtomicU8::new(0),
            task_migration_pending: AtomicBool::new(false),
            tracker: Mutex::new(Default::default()),
            update_latest: Mutex::new(None),
            update_notified: Mutex::new(None),
            sync_requests: Arc::new(Mutex::new(Vec::new())),
            status_dirty: AtomicBool::new(true),
            games_running: Arc::new(AtomicBool::new(false)),
            transitions: Mutex::new(Default::default()),
            proposals: Mutex::new(Default::default()),
            ui_busy: AtomicBool::new(false),
            settings_dirty: AtomicBool::new(false),
            status_last: Mutex::new(None),
            watch_progress: crate::perf::Progress::new(),
            det_running: AtomicBool::new(false),
            know_jobs: Mutex::new(Vec::new()),
            know_refresh: Mutex::new(Vec::new()),
            usage_kick: AtomicBool::new(false),
            upd: Arc::new(Default::default()),
            ui_frames: std::sync::atomic::AtomicU32::new(0),
            upd_report: Mutex::new(None),
            ui_ctx: Mutex::new(None),
        })
    }

    fn set_state(&self, s: WatchState) {
        *self.watch_state.lock().unwrap() = s;
        self.status_dirty.store(true, Ordering::SeqCst);
    }

    pub fn state(&self) -> WatchState {
        self.watch_state.lock().unwrap().clone()
    }

    /// Demarre la surveillance (appel de l'interface). Si un arret est en cours dans le fil de fond, le demarrage est mis a la
    /// suite : jamais deux surveillances en meme temps.
    pub fn start_watch(self: &Arc<Self>) {
        if self.watch_change_busy.load(Ordering::SeqCst) > 0 {
            self.change_watch_async(true);
            return;
        }
        self.start_watch_now();
    }

    /// Demarre la surveillance dans un thread (meme code que `AutoGameSoftwareSync watch`, meme verrou).
    fn start_watch_now(self: &Arc<Self>) {
        let mut guard = self.watch.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, me) = (stop.clone(), self.clone());
        let join = std::thread::spawn(move || {
            let mut ctx = Ctx::new(me.data_dir.clone());
            let settings = crate::settings::Settings::load(&ctx).unwrap_or_default();
            settings.apply_to_ctx(&mut ctx);
            ctx.progress = Some(me.watch_progress.clone());
            let mut cfg = match Config::load(&ctx) {
                Ok(c) => c,
                Err(e) => return me.set_state(WatchState::Error(e.to_string())),
            };
            // Elements mis en pause depuis l'accueil : ni surveilles ni copies (rien n'est supprime).
            cfg.games.retain(|g| !settings.is_paused(&g.name));
            let Some(dest) = cfg.dest.clone().filter(|d| !d.as_os_str().is_empty()) else {
                return me.set_state(WatchState::Error("no destination".into()));
            };
            let _lock = match InstanceLock::acquire(&me.data_dir, &watch::pid_alive) {
                Ok(l) => l,
                Err(e) => return me.set_state(WatchState::Other(e.to_string())),
            };
            let _ = std::fs::remove_file(watch::stop_file(&ctx));
            let log = Logger::new(me.data_dir.join("logs").join("watch.log"), false);
            // Demarrage de session : le dossier partage (NAS, cloud en miroir...) peut ne pas etre encore monte.
            // On attend (sans bloquer l'interface ni boucler : un essai toutes les 3 s, 5 min au plus).
            if !dest.is_dir() {
                log.log("[Demarrage] dossier partage inaccessible : attente (5 min au plus)");
                let (d2, s3) = (dest.clone(), s2.clone());
                let ok = crate::startup::wait_until(&|| d2.is_dir(), &|| s3.load(Ordering::SeqCst), &|s| std::thread::sleep(Duration::from_secs(s)), 300, 3);
                log.log(if ok { "[Demarrage] dossier partage accessible" } else { "[Demarrage] dossier partage toujours inaccessible : la surveillance demarre quand meme" });
            }
            let wc = WatchCfg::from_params(&crate::reactive::params(settings.reactivity, &settings.reactivity_custom));
            let lang = settings.lang;
            // Evenements du dossier partage (ReadDirectoryChangesW) : une sauvegarde de l'autre PC est detectee aussitot.
            let (ev_tx, ev_rx) = std::sync::mpsc::channel();
            // Interrogation directe du dossier partage (Google Drive en miroir ne declenche aucun evenement Windows).
            let poll_ms = crate::reactive::poll_interval_ms(settings.reactivity, settings.poll_custom_s);
            let poll_stop = Arc::new(AtomicBool::new(false));
            crate::cloudpoll::spawn(dest.clone(), cfg.games.clone(), poll_ms, ev_tx.clone(), poll_stop.clone());
            {
                let rp0 = crate::reactive::params(settings.reactivity, &settings.reactivity_custom);
                let own: Vec<String> = settings.per_game.iter().filter(|(_, g)| g.resync_secs.is_some() || g.stable_secs.is_some()).map(|(n, g)| format!("{} (resynchro {} s)", n, g.resync_secs.map(|s| s.to_string()).unwrap_or_else(|| "defaut".into()))).collect();
                Logger::new(me.data_dir.join("logs").join("watch.log"), false).log(&format!("[Surveillance] reactivite {:?} : interrogation du dossier partage toutes les {} s, resynchro de secours {} s{}", settings.reactivity, poll_ms / 1000, rp0.fallback_ms / 1000, if own.is_empty() { String::new() } else { format!(" ; reglages propres : {}", own.join(", ")) }));
            }
            let lg_dw = Logger::new(me.data_dir.join("logs").join("watch.log"), false);
            let _dirwatch = crate::dirwatch::DirWatch::spawn(dest.clone(), ev_tx, move |m| lg_dw.log(m));
            let mut w = Watcher::new(&ctx, cfg.games.clone(), dest, wc, &log);
            w.set_events(ev_rx);
            w.set_triggers(me.sync_requests.clone());
            for (n, g) in &settings.per_game {
                if g.stable_secs.is_some() || g.resync_secs.is_some() {
                    w.timing.insert(n.clone(), (g.stable_secs.map(|s| s * 1000), g.resync_secs.map(|s| s * 1000)));
                }
            }
            for g in &cfg.games {
                w.auto_cloud.insert(g.name.clone(), settings.conflict_for(&g.name) == crate::settings::ConflictMode::Cloud);
            }
            let dd = me.data_dir.clone();
            let me2 = me.clone();
            let me_chg = me.clone();
            w.on_change = Some(Box::new(move || me_chg.status_dirty.store(true, Ordering::SeqCst)));
            w.running_flag = Some(me.games_running.clone());
            let dd_usage = me.data_dir.clone();
            w.on_event = Some(Box::new(move |ev: WatchEvent| {
                if let Ok(mut t) = me2.tracker.lock() {
                    t.on_event(ev.kind, &ev.game, crate::util::now_ms());
                }
                // Donnees d'usage (categories seulement : ni nom, ni chemin, ni texte d'erreur).
                match ev.kind {
                    watch::EventKind::Error => {
                        crate::usage::record(&dd_usage, "sync_error", &[("category", crate::usage::error_category(&ev.detail))]);
                    }
                    watch::EventKind::Conflict => {
                        crate::usage::record(&dd_usage, "conflict", &[("action", "shown"), ("choice", "none")]);
                    }
                    watch::EventKind::ConflictResolved => {
                        crate::usage::record(&dd_usage, "conflict", &[("action", "resolved"), ("choice", "other_device")]);
                    }
                    _ => {}
                }
                if ev.kind == watch::EventKind::ConflictResolved {
                    let (title, _) = crate::notify::notification_text(&ev, lang);
                    // Toast 2.0 : nom de l'element en titre, explication en mots simples dessous.
                    if let Ok(mut q) = me2.ui_events.lock() {
                        q.push((format!("{}\n{}", ev.game, title), true));
                    }
                }
                notify_event(&dd, lang, &ev)
            }));
            log.log("surveillance demarree depuis l'interface");
            me.set_state(WatchState::Running);
            watch::run_loop_ex(&mut w, &ctx, 5000, &s2, Some(&me.paused));
            poll_stop.store(true, Ordering::SeqCst);
            me.games_running.store(false, Ordering::SeqCst);
            me.set_state(WatchState::Stopped);
        });
        *guard = Some(WatchHandle { stop, join });
    }

    /// Arret complet et ATTENTE de la fin de la surveillance (fermeture de l'application, fil de fond). Jamais depuis le fil de
    /// l'interface : une grosse copie en cours retiendrait la fenetre jusqu'a sa fin.
    pub fn stop_watch(&self) {
        let h = self.watch.lock().unwrap().take();
        if let Some(h) = h {
            h.stop.store(true, Ordering::SeqCst);
            let _ = h.join.join();
        }
    }

    /// Redemarre la surveillance (appel de l'interface) SANS attendre : l'arret est signale tout de suite et la fin de la copie
    /// en cours est attendue dans un fil de fond.
    pub fn restart_watch(self: &Arc<Self>) {
        self.change_watch_async(true);
    }

    /// Arrete la surveillance (appel de l'interface) sans attendre.
    pub fn stop_watch_async(self: &Arc<Self>) {
        self.change_watch_async(false);
    }

    fn change_watch_async(self: &Arc<Self>, start_after: bool) {
        self.watch_want_start.store(start_after, Ordering::SeqCst);
        self.watch_change_pending.fetch_add(1, Ordering::SeqCst);
        self.watch_change_busy.fetch_add(1, Ordering::SeqCst);
        // Signal d'arret immediat : la surveillance s'arrete des que l'element en cours est termine.
        if let Ok(g) = self.watch.lock() {
            if let Some(h) = g.as_ref() {
                h.stop.store(true, Ordering::SeqCst);
            }
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new().name("agss-watch-change".into()).spawn(move || {
            {
                let _serial = me.watch_change_lock.lock().unwrap_or_else(|e| e.into_inner());
                // Une demande plus recente a deja ete traitee par un fil precedent : rien a faire.
                if me.watch_change_pending.swap(0, Ordering::SeqCst) > 0 {
                    me.stop_watch();
                    if me.watch_want_start.load(Ordering::SeqCst) && !me.quitting.load(Ordering::SeqCst) {
                        me.start_watch_now();
                    }
                }
            }
            me.watch_change_busy.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            self.watch_change_busy.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Arret complet : surveillance, verrous, puis fin du processus.
    pub fn quit(&self) -> ! {
        self.quitting.store(true, Ordering::SeqCst);
        // La fenetre disparait tout de suite : attendre la fin d une grosse copie ne la laisse pas figee a l ecran.
        winutil::hide_window(APP_TITLE);
        self.stop_watch();
        self.write_status_final();
        self.gui_lock.lock().unwrap().take();
        std::process::exit(0);
    }
}

pub(super) fn notify_event(dd: &std::path::Path, lang: Lang, ev: &WatchEvent) {
    // Notification Windows envoyee par l'application elle-meme (voir notify.rs), si les reglages l'autorisent.
    crate::notify::deliver(&crate::notify::WinNotifier, dd, lang, ev);
}

// ================================================================== messages des threads de travail


/// Ajout en attente d'une decision (nom ressemblant a un jeu deja present dans la destination).
pub(super) struct PendingAdd {
    pub(super) game: Game,
    pub(super) exe_path: Option<PathBuf>,
    pub(super) similar: Vec<cloud::DestGame>,
}


pub(super) enum Msg {
    /// Fin du travail de sauvegarde / restauration (retire la barre de progression).
    OpEnd,
    Statuses(HashMap<String, GameStatus>),
    /// Faits de l'accueil (categorie, version active, appareils), calcules avec les etats.
    Home(Vec<crate::home::Facts>),
    Done { text: String, ok: bool },
    /// Fin d un ajout (formulaire, doublon accepte) : noms ajoutes et erreurs.
    Added { names: Vec<String>, errs: Vec<String> },
    History(String, backup::History),
    DestGames(Vec<cloud::DestGame>),
    Machines(Vec<String>),
    PropPaths(String, Vec<crate::hints::Suggestion>),
    /// Emplacements trouves par la connaissance intelligente (carte de detection ou ajout manuel).
    Known(knowledge_ui::KnowTarget, crate::knowledge::Program, Vec<crate::knowledge::Found>),
    Startup { tools: Vec<crate::startup::Tool>, existing: Vec<String>, exists: std::collections::HashSet<PathBuf>, legacy: bool },
    DestSize(u64),
    DestOk(Option<bool>),
    Hints(Vec<crate::hints::Hint>),
    /// Propositions de la base embarquee pour un exe (cle = exe saisi en minuscules).
    Auto(String, Vec<crate::hints::Suggestion>),
    /// Apercu de restauration calcule en thread ; la confirmation s'ouvre ensuite.
    Preview(Confirm, Option<crate::preview::Preview>),
    SystemLight(Option<bool>),
    /// Texte de la fenetre « Ce qui est envoye ».
    UsageText(String),
    /// Copies de securite d'un element (Remonter le temps).
    Safety(String, Vec<backup::SafetyCopy>),
    /// Apercu d'un point de la frise (element, point) ; None = impossible a calculer.
    PointPreview(String, crate::timetravel::PointKey, Option<crate::preview::Preview>),
    /// « Remettre cette version » termine : libelle de la version et de quoi l'annuler, ou l'erreur.
    Restored { game: String, label: String, result: Result<backup::UndoTicket, String> },
    /// « Annuler » termine.
    Undone { game: String, result: Result<backup::UndoOutcome, String> },
    /// Action de la fiche terminee : toast (titre, detail, reussite) puis etats relus.
    FicheDone { title: String, detail: String, ok: bool },
}

pub(super) struct CachedProbe(pub(super) Option<bool>);
impl ThemeProbe for CachedProbe {
    fn system_is_light(&self) -> Option<bool> {
        self.0
    }
}

// ================================================================== application

pub struct App {
    pub(super) shared: Arc<Shared>,
    pub(super) model: AppModel,
    pub(super) tx: Sender<Msg>,
    pub(super) rx: Receiver<Msg>,
    // theme
    pub(super) sys_light: Option<bool>,
    pub(super) theme_key: Option<(bool, [u8; 3], bool)>,
    pub(super) last_theme_probe: Instant,
    pub(super) dest_ok: Option<bool>,
    pub(super) dest_probe_at: Instant,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) adv_startup: bool,
    pub(super) log_open: bool,
    /// Reglages (lot 11) : cartes expertes depliees, recherche, apparition. Ajouter : formulaire ouvert, apparition.
    pub(super) set_ui: settings_simple::SetUi,
    pub(super) add_ui: add_page::AddUi,
    /// Diagnostic (captures) : AGSS_DEMO=open_all deplie toutes les cartes.
    pub(super) demo_open_all: bool,
    /// Planification en cours d edition (ajout manuel / fiche du jeu) ; None = aucune.
    pub(super) man_sched: Option<crate::schedule::Schedule>,
    pub(super) edit_sched: Option<crate::schedule::Schedule>,
    pub(super) log_big: bool,
    pub(super) prof_big: bool,
    pub(super) log_copied: Option<Instant>,
    pub(super) max_state: uilogic::MaxState,
    pub(super) last_pad_label: Option<(uilogic::PadKind, bool)>,
    pub(super) tray_last: Option<(crate::trayicon::TrayState, u32, Lang, Option<String>)>,
    pub(super) cjk_loaded: bool,
    pub(super) cjk_tried: bool,
    pub(super) start_hidden: bool,
    pub(super) frame_no: u32,
    pub(super) last_status: Instant,
    pub(super) status_inflight: bool,
    // add page
    pub(super) man_name: String,
    pub(super) man_path: String,
    pub(super) man_exe: String,
    // log page
    pub(super) log_entries: Vec<LogEntry>,
    pub(super) log_filter: LogFilter,
    pub(super) log_level_idx: usize,
    pub(super) last_log: Instant,
    // games page
    pub(super) history: Option<backup::History>,
    pub(super) history_for: Option<String>,
    pub(super) dest_games: Vec<cloud::DestGame>,
    pub(super) pending_add: Option<PendingAdd>,
    pub(super) pending_overlap: Option<(Game, Option<PathBuf>, String)>,
    pub(super) man_is_file: bool,
    pub(super) man_filter: String,
    pub(super) link_open: bool,
    pub(super) icons: HashMap<String, Option<egui::TextureHandle>>,
    // settings page
    pub(super) dest_text: String,
    pub(super) dest_size: Option<u64>,
    // regles de demarrage (taches planifiees) : outils detectes, taches existantes, etat du disque
    pub(super) startup_tools: Vec<crate::startup::Tool>,
    pub(super) startup_existing: Vec<String>,
    pub(super) startup_exists: std::collections::HashSet<PathBuf>,
    pub(super) legacy_run: bool,
    /// Premier lancement 2.0 : assistant en 6 etapes (lot 10) ; etat persistant dans `settings.onboarding`.
    pub(super) wizard: wizard_ui::WizardUi,
    /// Alerte poids (2.0.1) : mesures et choix en attente ; versions choisies pour le prochain ajout.
    pub(super) weight: weight_ui::WeightUi,
    pub(super) add_keep: Option<usize>,
    /// Rappel des statistiques programme pendant CETTE session (il attend le prochain lancement), et fenetre « Voir ce qui est envoye ».
    pub(super) late_set_this_session: bool,
    pub(super) consent_info: bool,
    /// Carte discrete du rappel unique des statistiques affichee (en bas de l'accueil).
    pub(super) late_card_open: bool,
    pub(super) edit_keep: usize,
    pub(super) edit_stable: u64,
    pub(super) edit_resync: u64,
    pub(super) edit_notify: u8, // 0 defaut, 1 oui, 2 non
    pub(super) edit_for: Option<String>,
    pub(super) edit_exe: String,
    pub(super) edit_display: String,
    pub(super) edit_save_file: bool,
    pub(super) edit_locs: Vec<crate::config::Location>,
    pub(super) edit_primary_label: String,
    pub(super) edit_primary_enabled: bool,
    pub(super) history_unknown: Option<Vec<(String, String)>>,
    pub(super) man_extras: Vec<crate::config::Location>,
    /// Ajout manuel : nom deduit automatiquement, chemin trouve automatiquement, options avancees ouvertes, message de planification, surbrillance, exe pour lequel l emplacement a deja ete applique, icone.
    pub(super) man_name_auto: String,
    pub(super) man_path_found: bool,
    pub(super) man_adv_open: bool,
    pub(super) man_explained: bool,
    pub(super) man_flash: Option<f64>,
    pub(super) man_filled_key: String,
    pub(super) man_icon: Option<(String, Option<egui::TextureHandle>)>,
    pub(super) prop_key: Option<String>,
    pub(super) prop_path: String,
    pub(super) prop_sugg: Vec<crate::hints::Suggestion>,
    pub(super) prop_state: crate::gamedetect::CardState,
    pub(super) prop_exit: Option<detect_ui::PropExit>,
    pub(super) prop_card_rect: Option<egui::Rect>,
    pub(super) prop_warnings: Vec<String>,
    /// Connaissance intelligente : emplacements trouves pour la carte affichee, et le programme decrit.
    pub(super) prop_found: Vec<crate::knowledge::Found>,
    pub(super) prop_prog: Option<crate::knowledge::Program>,
    /// Idem pour l'ajout manuel (cle = exe saisi en minuscules) ; exe dont l'apprentissage vient d'etre demande.
    pub(super) man_found: Vec<crate::knowledge::Found>,
    pub(super) man_found_key: String,
    pub(super) man_prog: Option<crate::knowledge::Program>,
    pub(super) man_learn_armed: Option<String>,
    pub(super) profiles: Vec<crate::profiles::Profile>,
    pub(super) prof_errors: Vec<crate::profiles::LoadError>,
    pub(super) prof_loaded: bool,
    pub(super) prof_query: String,
    pub(super) prof_cat: Option<&'static str>,
    pub(super) prof_open: Option<String>,
    pub(super) prof_enabled: Vec<bool>,
    pub(super) prof_vals: std::collections::BTreeMap<String, String>,
    pub(super) prof_ack: bool,
    pub(super) prof_real_vars: crate::profiles::Vars,
    pub(super) share_state: crate::sharedfolder::SharedState,
    pub(super) reach_bad_since: Option<u64>,
    pub(super) scroll_dest: bool,
    pub(super) dest_highlight_until: Option<f64>,
    pub(super) det_mem: crate::gamedetect::Memory,
    pub(super) edit_filter: String,
    pub(super) machines: Vec<String>,
    pub(super) machine_map: std::collections::BTreeMap<String, [u8; 3]>,
    pub(super) edit_save: String,
    /// Messages a montrer en toast 2.0 a la prochaine image (`say` : l'ancienne ligne de message n'existe plus).
    pub(super) said: Vec<(String, bool)>,
    /// Progression de la sauvegarde / restauration en cours (libelle, etat partage).
    pub(super) op_progress: Option<(K, Arc<crate::perf::Progress>)>,
    pub(super) stream_warn: bool,
    pub(super) op_from_watch: bool,
    pub(super) hints: Vec<crate::hints::Hint>,
    pub(super) hint_ignored: std::collections::HashSet<String>,
    pub(super) auto_sugg: Vec<crate::hints::Suggestion>,
    pub(super) preview: Option<crate::preview::Preview>,
    pub(super) auto_key: String,
    pub(super) auto_filled: (String, String),
    pub(super) add_page: crate::autofill::AddPage,
    /// Accueil en tuiles (filtre, menu ouvert, confirmation, navigation au clavier, faits).
    pub(super) home: home_ui::HomeUi,
    /// Fiche d'un element et Remonter le temps (lot 7) : ecran, frise, apercus, confirmations, annulation.
    pub(super) fiche: fiche_ui::FicheUi,
    /// Coffre (choix, reprise, carte des Reglages) et selecteur de dossier guide (lot 8).
    pub(super) vault: vault_ui::VaultUi,
    pub(super) picker: picker_ui::PickerUi,
    /// Scan du PC (lot 9).
    pub(super) scan: scan_ui::ScanUi,
    /// Nouveau PC (lot 12) : migration en 4 ecrans, recuperations differees, inventaire des logiciels.
    pub(super) migrate: migrate_ui::MigrateUi,
    /// Toasts 2.0 (« C'est fait · Annuler »), affiches au-dessus de tout.
    pub(super) toasts: ui::Toasts,
    pub(super) support: Option<Instant>,
    pub(super) games_seen: usize,
    pub(super) known: crate::hints::KnownFolders,
    // manette
    pub(super) pad: gamepad::GamepadService,
    pub(super) page_delta: i32,
    // rendu / focus (voir `deco`)
    pub(super) palette: theme::Palette,
    pub(super) big_now: bool,
    pub(super) focus_id: Option<egui::Id>,
    pub(super) focus_since: Instant,
    pub(super) focus_from_nav: bool,
    pub(super) focus_tip_due: bool,
    /// Images restantes pendant lesquelles un gain de focus est attribue au clavier / a la manette.
    pub(super) nav_recent: u8,
    /// Selection manette maison (mode manette uniquement).
    pub(super) focus: PadFocus,
    pub(super) pad_mode: bool,
    pub(super) pad_prompter: crate::padctl::Prompter,
    pub(super) pad_prompt: Option<gamepad::PadInfo>,
    pub(super) pad_prompt_remember: bool,
    pub(super) pad_last_name: Option<String>,
    pub(super) _tray: Option<tray_sys::TrayHandle>,
    pub(super) tray_detect: Option<tray_sys::DetectHandle>,
    pub(super) tray_detect_last: Option<(bool, Lang)>,
    /// Donnees d'usage : texte de la fenetre « Ce qui est envoye », confirmation d'effacement, question forcee (captures).
    pub(super) usage_view: Option<String>,
    pub(super) usage_erase_ask: bool,
    pub(super) demo_usage_card: bool,
    /// Origine du prochain ajout (donnees d'usage) : (source, logiciel ?).
    pub(super) add_src: (&'static str, Option<bool>),
    /// Mise a jour : annonce du resultat (version, page, debut), derniere verification, installation en cours, demarrage confirme.
    pub(super) updated_banner: Option<(String, String, Instant)>,
    pub(super) exe_writable: Option<bool>,
    /// Horloge centrale des animations (redessin seulement si necessaire), ralenti de diagnostic (AGSS_UI_SLOWMO) et
    /// ecran de demonstration des composants (AGSS_UI_GALLERY=1).
    pub(super) clock: ui::anim::AnimClock,
    pub(super) slowmo: f32,
    pub(super) gallery: Option<gallery::Gallery>,
    /// Diagnostic (AGSS_UI_FPS_LOG=1) : images dessinees journalisees toutes les 10 s (debut, images, images d animation).
    pub(super) fps_mark: Option<(f64, u64, u64, u64)>,
    /// Diagnostic (AGSS_UI_SHOT=fichier.bmp) : capture de la fenetre pour les validations des lots.
    pub(super) shot: Option<shot::Shot>,
}
