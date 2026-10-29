//! Creation de l'application : etat initial, threads de fond (surveillance, manette, detection, mises a jour).
use super::*;

impl App {
    pub(super) fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>, opts: LaunchOpts) -> App {
        // 2.0.3 : icones de l'interface (police Phosphor embarquee) installees avant la premiere image.
        cc.egui_ctx.set_fonts(ui::glyphs::fonts_with_icons());
        let (tx, rx) = channel();
        let ctx = make_ctx(&shared.data_dir);
        let model = AppModel::new(ctx).unwrap_or_else(|_| AppModel::new(make_ctx(&shared.data_dir)).expect("init"));
        let mut model = model;
        // Aide au diagnostic : AGSS_START_PAGE=games|add|log|settings ouvre directement cette page.
        model.page = match std::env::var("AGSS_START_PAGE").as_deref() {
            Ok("add") => Page::Add,
            Ok("settings") => Page::Settings,
            _ => Page::Games,
        };
        let lang = model.settings.lang;

        // --- tray : fermer la fenetre = reduire ici, la surveillance continue
        let tray_pair = build_tray(&shared, lang, model.settings.accent, &cc.egui_ctx, model.settings.detect_enabled);
        let (tray, tray_det) = match tray_pair {
            Some((t, d)) => (Some(t), Some(d)),
            None => (None, None),
        };

        // --- threads utilitaires (afficher / quitter a la demande, arret automatique de test)
        {
            let (sh, ectx) = (shared.clone(), cc.egui_ctx.clone());
            let auto = opts.auto_quit_secs;
            std::thread::spawn(move || {
                let t0 = Instant::now();
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    let show = sh.data_dir.join("gui.show");
                    let quit = sh.data_dir.join("gui.quit");
                    if show.exists() {
                        let _ = std::fs::remove_file(&show);
                        winutil::show_window(APP_TITLE);
                        ectx.request_repaint();
                    }
                    if quit.exists() {
                        let _ = std::fs::remove_file(&quit);
                        sh.quit();
                    }
                    if auto.map(|a| t0.elapsed().as_secs() >= a).unwrap_or(false) {
                        sh.quit();
                    }
                }
            });
        }

        if model.settings.auto_watch && model.dest().is_some() {
            shared.start_watch();
        }

        // Premier lancement apres le renommage : entree Run et taches planifiees de l'ancienne version mises a jour
        // automatiquement (jamais en dossier de test : seulement avec le dossier de donnees par defaut).
        if crate::appdirs::data_dir_from_env().is_none() {
            let (sh, dd) = (shared.clone(), shared.data_dir.clone());
            std::thread::spawn(move || {
                let lg = Logger::new(dd.join("logs").join("watch.log"), false);
                if let Ok(exe) = std::env::current_exe() {
                    for l in winutil::migrate_run_entries(&exe) {
                        lg.log(&l);
                    }
                }
                match actions::auto_migrate_tasks(&make_ctx(&dd)) {
                    Ok(actions::TaskMigration::Done(n)) => lg.log(&format!("[Migration] {} tache(s) planifiee(s) migree(s) vers AutoGameSoftwareSync", n)),
                    Ok(actions::TaskMigration::NeedsElevation(n)) => {
                        lg.log(&format!("[Migration] {} ancienne(s) tache(s) a mettre a jour : autorisation administrateur necessaire (Reglages > Demarrage)", n));
                        sh.task_migration_pending.store(true, Ordering::SeqCst);
                    }
                    Ok(actions::TaskMigration::NothingToDo) => {}
                    Ok(actions::TaskMigration::Failed(e)) => lg.log(&format!("[Migration] taches planifiees non migrees : {}", e)),
                    Err(e) => lg.log(&format!("[Migration] taches planifiees non migrees : {}", e)),
                }
            });
        }

        // --- manettes (XInput + HID Sony) dans un thread d'arriere-plan ; connexions journalisees
        let pad = {
            let c = cc.egui_ctx.clone();
            let lg = Logger::new(shared.data_dir.join("logs").join("watch.log"), false);
            let sh = gamepad::PadShared::new(model.settings.pad_mode, shared.games_running.clone());
            gamepad::spawn(sh, move || c.request_repaint(), winutil::own_window_is_foreground, move |m| {
                // Linux : aucune manette lue directement (Steam Input) ; « liberee / reprise » ne voudrait rien dire.
                if cfg!(not(windows)) && (m.contains("liberee") || m.ends_with("reprise")) {
                    return;
                }
                lg.log(m)
            })
        };

        let dest_text = model.dest().map(|d| d.display().to_string()).unwrap_or_default();
        crate::gamedb::preload(&shared.data_dir); // decompression de la base embarquee en arriere-plan
        shared.spawn_status_thread();
        {
            let mem0 = crate::gamedetect::Memory::load(&shared.data_dir);
            if let Ok(mut q) = shared.proposals.lock() {
                *q = crate::gamedetect::Proposals::load(&shared.data_dir, &mem0, crate::util::now_ms());
            }
        }
        shared.spawn_detect_thread();
        shared.spawn_knowledge_thread(tx.clone(), cc.egui_ctx.clone());
        // Donnees d'usage : rien sans consentement (la question s'affiche apres le choix de la langue).
        crate::usage::set_consent(model.settings.usage_consent);
        crate::weight::set_limit_mb(model.settings.heavy_limit_mb);
        if model.settings.usage_consent == Some(true) {
            crate::usage::install_id(&shared.data_dir, crate::util::now_ms()); // cree ou renouvele (13 mois)
        }
        {
            let s = &model.settings;
            crate::usage::push(&shared.data_dir, crate::usage::app_start_event(s.lang, s.expert_mode, opts.start_hidden, s.big_mode, crate::util::now_ms()), s.usage_consent);
        }
        shared.spawn_usage_thread();
        #[cfg(not(windows))]
        {
            winutil::set_context(&cc.egui_ctx);
            // Demarrage masque demande sans icone de zone de notification : on affiche la fenetre (sinon l'appli serait invisible).
            if opts.start_hidden && tray.is_none() {
                winutil::show_window(APP_TITLE);
            }
        }
        {
            let c = cc.egui_ctx.clone();
            crate::notify::set_click_handler(move || {
                // Notification de mise a jour : le clic ouvre la page de la Release ; sinon, la fenetre.
                match crate::notify::take_click_url() {
                    Some(u) => winutil::open_url(&u),
                    None => winutil::show_window(APP_TITLE),
                }
                c.request_repaint();
            });
        }
        #[cfg(not(windows))]
        {
            let lg = Logger::new(shared.data_dir.join("logs").join("watch.log"), false);
            crate::notify::set_failure_handler(move |m| lg.log(m));
            crate::notify::set_open_label(tr(model.settings.lang, K::TrayOpen));
        }
        // Mise a jour : controle quotidien (au plus une fois par jour, jamais pendant un programme suivi), telechargement et
        // installation automatiques selon le reglage ; rien dans un dossier de test sauf AGSS_UPDATE_URL (voir `selfupdate`).
        frame_guard::spawn_watchdog(shared.clone());
        shared.spawn_update_thread(cc.egui_ctx.clone());
        shared.spawn_update_confirm_thread(opts.start_hidden);
        // Premier lancement 2.0 : parcours calcule une seule fois (installation neuve : 6 etapes ; mise a jour : « Quoi de
        // neuf » + etapes non faites), repris ensuite a l'etape memorisee.
        let wizard_active = {
            let facts = crate::onboarding::Facts {
                first_install: !model.settings.tutorial_seen,
                has_vault: model.dest().is_some(),
                has_items: !model.config.games.is_empty(),
                consent_needed: crate::usage::needs_question(model.settings.usage_asked.as_deref(), crate::update::current_version()),
            };
            let before = model.settings.onboarding.clone();
            crate::onboarding::begin(&mut model.settings.onboarding, &facts);
            // Diagnostic (captures) : AGSS_WIZARD_STEP=0..6 ouvre directement un ecran de l'assistant.
            if let Some(n) = std::env::var("AGSS_WIZARD_STEP").ok().and_then(|v| v.parse::<u8>().ok()).filter(|n| *n <= crate::onboarding::STEPS) {
                let o = &mut model.settings.onboarding;
                o.finished = false;
                o.started = true;
                if o.plan == 0 || (n > 0 && o.plan & crate::onboarding::bit(n) == 0) {
                    o.plan = crate::onboarding::ALL;
                }
                o.whats_new = o.whats_new || n == crate::onboarding::WHATS_NEW;
                o.step = n;
            }
            if model.settings.onboarding != before {
                let _ = model.save_settings();
            }
            crate::onboarding::pending(&model.settings.onboarding)
        };
        // Rappel unique de la question des statistiques (2.0.1) : une question restee sans reponse alors que l'assistant n'est
        // plus a faire revient une seule fois, en carte discrete en bas de l'accueil.
        {
            let needs = crate::usage::needs_question(model.settings.usage_asked.as_deref(), crate::update::current_version());
            let next = crate::usage::late_at_start(needs, model.settings.usage_late, wizard_active);
            if next != model.settings.usage_late {
                model.settings.usage_late = next;
                model.settings.usage_late_ms = 0;
                let _ = model.save_settings();
            }
        }
        if let Ok(mut c) = shared.ui_ctx.lock() {
            *c = Some(cc.egui_ctx.clone());
        }
        let mut app = App {
            shared,
            model,
            tx,
            rx,
            sys_light: None,
            theme_key: None,
            last_theme_probe: Instant::now() - Duration::from_secs(60),
            dest_ok: None,
            dest_probe_at: Instant::now() - Duration::from_secs(60),
            adv_startup: false,
            log_open: false,
            set_ui: Default::default(),
            add_ui: Default::default(),
            demo_open_all: false,
            man_sched: None,
            edit_sched: None,
            log_big: false,
            prof_big: false,
            log_copied: None,
            max_state: Default::default(),
            last_pad_label: None,
            tray_last: None,
            cjk_loaded: false,
            cjk_tried: false,
            start_hidden: opts.start_hidden,
            frame_no: 0,
            last_status: Instant::now() - Duration::from_secs(60),
            status_inflight: false,
            man_name: String::new(),
            man_path: String::new(),
            man_exe: String::new(),
            log_entries: vec![],
            log_filter: LogFilter::default(),
            log_level_idx: 0,
            last_log: Instant::now() - Duration::from_secs(60),
            history: None,
            history_for: None,
            dest_games: vec![],
            pending_add: None,
            pending_overlap: None,
            man_is_file: false,
            man_filter: String::new(),
            link_open: false,
            icons: HashMap::new(),
            dest_text,
            dest_size: None,
            startup_tools: vec![],
            startup_existing: vec![],
            startup_exists: Default::default(),
            legacy_run: false,
            wizard: wizard_ui::WizardUi::new(wizard_active),
            weight: Default::default(),
            late_set_this_session: false,
            consent_info: false,
            late_card_open: false,
            add_keep: None,
            edit_keep: 10,
            edit_stable: 10,
            edit_resync: 300,
            edit_notify: 0,
            edit_for: None,
            edit_exe: String::new(),
            edit_display: String::new(),
            edit_save_file: false,
            edit_locs: Vec::new(),
            edit_primary_label: String::new(),
            edit_primary_enabled: true,
            history_unknown: None,
            man_extras: Vec::new(),
            man_name_auto: String::new(),
            man_path_found: false,
            man_adv_open: false,
            man_explained: false,
            man_flash: None,
            man_filled_key: String::new(),
            man_icon: None,
            prop_key: None,
            prop_path: String::new(),
            prop_sugg: Vec::new(),
            prop_state: Default::default(),
            prop_exit: None,
            prop_card_rect: None,
            prop_warnings: Vec::new(),
            prop_found: Vec::new(),
            prop_prog: None,
            man_found: Vec::new(),
            man_found_key: String::new(),
            man_prog: None,
            man_learn_armed: None,
            profiles: Vec::new(),
            prof_errors: Vec::new(),
            prof_loaded: false,
            prof_query: String::new(),
            prof_cat: None,
            prof_open: None,
            prof_enabled: Vec::new(),
            prof_vals: Default::default(),
            prof_ack: false,
            prof_real_vars: Default::default(),
            share_state: crate::sharedfolder::SharedState::Ok,
            reach_bad_since: None,
            scroll_dest: false,
            dest_highlight_until: None,
            det_mem: Default::default(),
            edit_filter: String::new(),
            machines: Vec::new(),
            machine_map: Default::default(),
            edit_save: String::new(),
            said: Vec::new(),
            op_progress: None,
            stream_warn: false,
            op_from_watch: false,
            hints: Vec::new(),
            hint_ignored: Default::default(),
            auto_sugg: Vec::new(),
            preview: None,
            auto_key: String::new(),
            auto_filled: Default::default(),
            add_page: Default::default(),
            home: Default::default(),
            fiche: Default::default(),
            vault: Default::default(),
            picker: Default::default(),
            scan: Default::default(),
            close: Default::default(),
            safety_ui: Default::default(),
            playnite: Default::default(),
            migrate: Default::default(),
            toasts: Default::default(),
            support: None,
            games_seen: 0,
            known: crate::hints::KnownFolders::real(),
            pad,
            page_delta: 0,
            palette: theme::palette(true, [0x4c, 0x8d, 0xf6]),
            big_now: false,
            focus_id: None,
            focus_since: Instant::now(),
            focus_from_nav: false,
            focus_tip_due: false,
            nav_recent: 0,
            focus: PadFocus::default(),
            pad_mode: false,
            pad_prompter: Default::default(),
            pad_prompt: None,
            pad_prompt_remember: false,
            pad_last_name: None,
            _tray: tray,
            tray_detect: tray_det,
            tray_detect_last: None,
            usage_view: None,
            usage_erase_ask: false,
            demo_usage_card: false,
            add_src: ("manual", None),
            updated_banner: None,
            exe_writable: None,
            clock: ui::anim::AnimClock::default(),
            slowmo: std::env::var("AGSS_UI_SLOWMO").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0),
            gallery: gallery::Gallery::from_env(),
            fps_mark: (std::env::var("AGSS_UI_FPS_LOG").as_deref() == Ok("1")).then_some((0.0, 0, 0, 0)),
            shot: shot::Shot::from_env(),
        };
        app.games_seen = app.model.config.games.len();
        app.thread_probe_theme(&cc.egui_ctx);
        let p = app.model.page;
        app.on_page_enter(p, &cc.egui_ctx);
        app.apply_demo_env();
        app
    }
}
