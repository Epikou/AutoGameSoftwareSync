//! Travaux de fond lances par l'interface (etats, versions, detection, catalogue, ajouts) et reception de leurs messages ; theme et police CJK.
use super::*;

impl App {
    pub(super) fn t(&self, k: K) -> &'static str {
        tr(self.model.settings.lang, k)
    }

    /// Taille dans la langue de l'interface (unites et separateur decimal).
    pub(super) fn bytes(&self, b: u64) -> String {
        crate::i18n::format_bytes(self.model.settings.lang, b)
    }

    /// Texte avec un nom ou une date a la place de « {} », elision du francais (« d'aujourd'hui », « d'ALLY »).
    pub(super) fn tf(&self, k: K, v: &str) -> String {
        crate::i18n::elide(self.model.settings.lang, &self.t(k).replace("{}", v))
    }

    /// Texte singulier / pluriel selon `n` dans la langue de l'interface.
    pub(super) fn tn(&self, n: u64, one: K, many: K) -> &'static str {
        self.t(crate::i18n::pick_n(self.model.settings.lang, n, one, many))
    }

    /// « Par rapport à maintenant : 1 fichier modifié, 0 ajouté » (accords selon la langue).
    pub(super) fn diff_now_text(&self, replaced: u64, added: u64) -> String {
        crate::i18n::diff_now(self.model.settings.lang, replaced, added)
    }

    /// Une icone de zone de notification existe-t-elle ? (Linux : selon la compilation et le bureau.)
    #[cfg(not(windows))]
    pub(crate) fn has_tray(&self) -> bool {
        self._tray.is_some()
    }

    /// Message court (resultat d'une action, evenement de la surveillance) : toast 2.0 a la prochaine image.
    /// « titre\ndetail » pour un message en deux parties ; un echec s'affiche « Probleme » avec le texte dessous.
    pub(super) fn say(&mut self, text: String, ok: bool) {
        self.said.push((text, ok));
    }

    /// Transforme les messages en attente en toasts (appele a chaque image).
    pub(super) fn flush_said(&mut self, ctx: &egui::Context) {
        for (text, ok) in std::mem::take(&mut self.said) {
            let now = ui::anim::now(ctx);
            if ok {
                let (title, detail) = text.split_once('\n').unwrap_or((text.as_str(), ""));
                self.toasts.push(now, title, detail, ui::State::Safe, None);
            } else {
                self.toasts.push(now, self.t(K::VProblem), &text, ui::State::Problem, None);
            }
        }
    }

    // ------------------------------------------------------------ workers

    pub(super) fn thread_probe_theme(&self, ectx: &egui::Context) {
        let (tx, c) = (self.tx.clone(), ectx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::SystemLight(winutil::RegThemeProbe.system_is_light()));
            c.request_repaint();
        });
    }

    pub(super) fn refresh_statuses(&mut self, ectx: &egui::Context) {
        if self.status_inflight {
            return;
        }
        self.status_inflight = true;
        self.last_status = Instant::now();
        let (tx, c, dd) = (self.tx.clone(), ectx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let ctx = make_ctx(&dd);
            let m = AppModel::new(ctx);
            let (map, facts) = match m {
                Ok(mut m) => {
                    m.refresh_statuses();
                    // Accueil : categorie, derniere copie, version active, appareils (lecture seule du coffre).
                    let facts = crate::home::gather(&m.ctx, &m.config, &m.settings, &m.statuses);
                    (m.statuses, facts)
                }
                Err(_) => (HashMap::new(), Vec::new()),
            };
            let _ = tx.send(Msg::Home(facts));
            let _ = tx.send(Msg::Statuses(map));
            c.request_repaint();
        });
    }

    pub(super) fn load_versions(&mut self, game: &str, ectx: &egui::Context) {
        let Some(dest) = self.model.dest() else { return };
        let Some(g) = self.model.game(game).cloned() else { return };
        self.history_for = Some(game.to_string());
        let (tx, c, dd) = (self.tx.clone(), ectx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let ctx = make_ctx(&dd);
            let h = backup::history(&ctx, &g, &dest);
            let _ = tx.send(Msg::History(g.name.clone(), h));
            c.request_repaint();
        });
    }

    /// Charge la liste des jeux deja presents dans la destination (lecture seule).
    pub(super) fn load_dest_games(&mut self, ectx: &egui::Context) {
        let Some(dest) = self.model.dest() else { return };
        let (tx, c) = (self.tx.clone(), ectx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::DestGames(cloud::list_dest_games(&dest)));
            let _ = tx.send(Msg::Machines(cloud::machines_seen(&dest)));
            let _ = tx.send(Msg::Hints(cloud::latest_hints(&dest)));
            c.request_repaint();
        });
    }

    /// Execute une action confirmee dans un thread (memes fonctions que le CLI).
    pub(super) fn run_confirmed(&mut self, c: Confirm, ectx: &egui::Context) {
        let (tx, ectx2, dd) = (self.tx.clone(), ectx.clone(), self.shared.data_dir.clone());
        let sh_done = self.shared.clone();
        let lang = self.model.settings.lang;
        let label = c.message(lang);
        let op_kind = match &c {
            Confirm::Backup { .. } => Some(K::OpBackup),
            Confirm::Restore { .. } => Some(K::OpRestore),
            Confirm::Resolve { .. } => Some(K::OpResolve),
            _ => None,
        };
        let prog = op_kind.map(|_| crate::perf::Progress::new());
        if let (Some(k), Some(p)) = (op_kind, &prog) {
            self.op_progress = Some((k, p.clone()));
        }
        std::thread::spawn(move || {
            let log = Logger::new(dd.join("logs").join("watch.log"), false);
            let gname = match &c {
                Confirm::Backup { game } | Confirm::Restore { game, .. } | Confirm::Prune { game, .. } | Confirm::Remove { game } | Confirm::Resolve { game, .. } | Confirm::Link { game, .. } | Confirm::Relocate { game, .. } | Confirm::Rename { game, .. } | Confirm::RemoveLocation { game, .. } => format!("[{}] ", game),
                Confirm::Import { .. } | Confirm::RestoreAppearance | Confirm::ApplyStartup | Confirm::MigrateRun | Confirm::MigrateTasks => String::new(),
            };
            let r = match AppModel::new(make_ctx(&dd)) {
                Ok(mut m) => {
                    m.ctx.progress = prog;
                    m.execute(&c).map_err(|e| e.to_string())
                }
                Err(e) => Err(e.to_string()),
            };
            let (text, ok) = match r {
                Ok(d) => {
                    if matches!(c, Confirm::MigrateTasks) {
                        sh_done.task_migration_pending.store(false, Ordering::SeqCst);
                    }
                    log.log(&format!("{}UI : {} -> OK ({})", gname, label.lines().next().unwrap_or(""), d));
                    match &c {
                        Confirm::Resolve { keep_mine, .. } => {
                            crate::usage::record(&dd, "conflict", &[("action", "resolved"), ("choice", if *keep_mine { "this_pc" } else { "other_device" })]);
                        }
                        Confirm::Restore { version: Some(v), .. } => {
                            let age = crate::util::parse_timestamp(v).map(|t| crate::usage::bucket_age(crate::util::now_ms().saturating_sub(t)));
                            crate::usage::record(&dd, "time_travel", &[("action", "restored"), ("age", age.unwrap_or("30d+"))]);
                        }
                        _ => {}
                    }
                    (format!("{} {}", tr(lang, K::MsgDone), d), true)
                }
                Err(e) => {
                    log.log(&format!("{}ERREUR UI : {} -> {}", gname, label.lines().next().unwrap_or(""), e));
                    (format!("{} {}", tr(lang, K::MsgFailed), e), false)
                }
            };
            if let Confirm::Backup { game } | Confirm::Restore { game, .. } | Confirm::Resolve { game, .. } = &c {
                if let Ok(mut t) = sh_done.tracker.lock() {
                    t.on_event(if ok { watch::EventKind::BackedUp } else { watch::EventKind::Error }, game, crate::util::now_ms());
                }
            }
            sh_done.status_dirty.store(true, Ordering::SeqCst);
            let _ = tx.send(Msg::OpEnd);
            let _ = tx.send(Msg::Done { text, ok });
            ectx2.request_repaint();
        });
    }


    /// Ajoute des jeux a la config (dans un thread) puis redemarre la surveillance.
    pub(super) fn add_games_async(&mut self, games: Vec<(Game, Option<PathBuf>)>, ectx: &egui::Context) {
        let (tx, c, dd, sh) = (self.tx.clone(), ectx.clone(), self.shared.data_dir.clone(), self.shared.clone());
        let (src, app) = std::mem::replace(&mut self.add_src, ("manual", None));
        let keep = self.add_keep.take();
        std::thread::spawn(move || {
            let ctx = make_ctx(&dd);
            let log = Logger::new(dd.join("logs").join("watch.log"), false);
            let mut names: Vec<String> = Vec::new();
            let mut copy_now: Vec<String> = Vec::new();
            let mut errs = Vec::new();
            for (g, exe_path) in games {
                let name = g.name.clone();
                let no_program = g.exe.is_none();
                // Donnees d'usage : origine et categorie seulement.
                let category = match (g.exe.is_some(), app) {
                    (false, _) => "folder",
                    (true, Some(a)) => if a { "app" } else { "game" },
                    (true, None) => {
                        let known = crate::usage::known_title(&g, &crate::gamedb::embedded(&dd), &crate::profiles::load_embedded(), &crate::knowledge::home::Base::load(&dd));
                        if known.map(|k| k.1).unwrap_or(true) { "app" } else { "game" }
                    }
                };
                let multi = if g.extra_locations.is_empty() { "no" } else { "yes" };
                match actions::add_game(&ctx, g) {
                    Ok(()) => {
                        names.push(name.clone());
                        if no_program {
                            copy_now.push(name.clone());
                        }
                        crate::usage::record(&dd, "item_added", &[("source", src), ("category", category), ("multi", multi)]);
                        if let Some(p) = exe_path {
                            let mut s = crate::settings::Settings::load(&ctx).unwrap_or_default();
                            s.game_mut(&name).exe_path = Some(p);
                            let _ = s.save(&ctx);
                        }
                        // Alerte poids : versions choisies par l'utilisateur ; l'element est deja signale (pas de toast plus tard).
                        if let Some(k) = keep {
                            let mut s = crate::settings::Settings::load(&ctx).unwrap_or_default();
                            s.set_keep(&name, k);
                            if !s.heavy_notified.contains(&name) {
                                s.heavy_notified.push(name.clone());
                            }
                            let _ = s.save(&ctx);
                        }
                        log.log(&format!("[{}] UI : synchro ajoutee", name));
                    }
                    Err(e) => {
                        log.log(&format!("[{}] ERREUR UI : ajout impossible : {}", name, e));
                        errs.push(format!("{} : {}", name, e));
                    }
                }
            }
            // Sans programme a surveiller, la premiere copie est demandee aussitot (puis chaque jour a l'heure planifiee) ;
            // si la surveillance est arretee, la demande attend son prochain demarrage (elle n'est jamais relancee d'office).
            if !copy_now.is_empty() && sh.state() == WatchState::Running {
                if let Ok(mut q) = sh.sync_requests.lock() {
                    q.extend(copy_now.into_iter().map(Some));
                }
            }
            let _ = tx.send(Msg::Added { names, errs });
            c.request_repaint();
        });
    }

    pub(super) fn after_change(&mut self, ectx: &egui::Context) {
        self.load_dest_games(ectx);
        self.load_startup(ectx);
        let _ = self.model.reload();
        self.dest_text = self.model.dest().map(|d| d.display().to_string()).unwrap_or_default();
        self.shared.restart_watch();
        self.last_status = Instant::now() - Duration::from_secs(60);
        if let Some(g) = self.model.selected.clone() {
            self.load_versions(&g, ectx);
            self.load_safety(&g, ectx);
            self.fill_location_fields(&g);
        }
    }

    /// Copies de securite d'un element (lecture seule du dossier de donnees local).
    pub(super) fn load_safety(&mut self, game: &str, ectx: &egui::Context) {
        let Some(g) = self.model.game(game).cloned() else { return };
        self.fiche.safety_for = Some(game.to_string());
        let (tx, c, dd) = (self.tx.clone(), ectx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let v = backup::safety_copies(&make_ctx(&dd), &g);
            let _ = tx.send(Msg::Safety(g.name.clone(), v));
            c.request_repaint();
        });
    }

    pub(super) fn drain(&mut self, ectx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Statuses(s) => {
                    self.model.statuses = s;
                    self.status_inflight = false;
                }
                Msg::Home(f) => {
                    self.home.facts = f;
                    self.heavy_check(ectx);
                }
                Msg::OpEnd => {
                    if let Some((_, p)) = self.op_progress.take() {
                        self.stream_warn = p.streaming();
                    }
                }
                Msg::Done { text, ok } => {
                    let (text, ok) = if std::mem::take(&mut self.stream_warn) { (format!("{} {}", text, self.t(K::StreamingWarn)), false) } else { (text, ok) };
                    if self.model.page == Page::Games && self.model.selected.is_some() {
                        // Fiche 2.0 : toast (detail technique en mode expert, ou en cas d'echec).
                        self.fiche_toast_done(ectx, &text, ok);
                    } else {
                        self.say(text, ok);
                    }
                    self.after_change(ectx);
                }
                Msg::Added { names, errs } => {
                    let expert = self.model.settings.expert_mode;
                    if !names.is_empty() {
                        let detail = if expert { format!("{} {}", self.t(K::AddDone), names.len()) } else { self.t(K::VAddedToast).replace("{}", &names.join(", ")) };
                        self.toasts.push(ui::anim::now(ectx), self.tm(K::VDone), &detail, ui::State::Safe, None);
                    }
                    if !errs.is_empty() {
                        let all = errs.join(" | ");
                        self.say(self.t(K::VAddFailedToast).replace("{}", &all), false);
                    }
                    self.after_change(ectx);
                }
                Msg::Safety(g, v) => {
                    if self.fiche.safety_for.as_deref() == Some(&g) {
                        self.fiche.safety = v;
                    }
                }
                Msg::PointPreview(g, k, p) => self.fiche_on_preview(g, k, p),
                Msg::Restored { game, label, result } => self.fiche_on_restored(ectx, game, label, result),
                Msg::Undone { game, result } => self.fiche_on_undone(ectx, game, result),
                Msg::FicheDone { title, detail, ok } => {
                    let st = if ok { ui::State::Safe } else { ui::State::Problem };
                    self.toasts.push(ui::anim::now(ectx), &title, &detail, st, None);
                    self.after_change(ectx);
                }
                Msg::History(g, h) => {
                    if self.history_for.as_deref() == Some(&g) {
                        self.history = Some(h);
                        // l'etat local a pu changer : apercus a recalculer
                        self.fiche.previews.clear();
                    }
                }
                Msg::DestGames(v) => self.dest_games = v,
                Msg::Machines(v) => self.machines = v,
                Msg::PropPaths(k, v) => {
                    if self.prop_key.as_deref() == Some(k.as_str()) {
                        if let Some(p) = v.iter().find(|s| s.exists && s.local_path.is_some()).and_then(|s| s.local_path.clone()) {
                            if self.prop_path.trim().is_empty() {
                                self.prop_path = p.display().to_string();
                            }
                        }
                        self.prop_sugg = v;
                    }
                }
                Msg::Known(target, prog, found) => self.on_known(target, prog, found),
                Msg::Startup { tools, existing, exists, legacy } => {
                    self.startup_tools = tools;
                    self.startup_existing = existing;
                    self.startup_exists = exists;
                    self.legacy_run = legacy;
                }
                Msg::DestSize(s) => self.dest_size = Some(s),
                Msg::DestOk(v) => self.dest_ok = v,
                Msg::Hints(v) => self.hints = v,
                Msg::Preview(c, p) => {
                    self.preview = p;
                    self.model.request(c);
                }
                Msg::Auto(k, v) => {
                    if k == self.auto_key {
                        // Remplissage automatique : seulement une proposition unique, et jamais par-dessus ce que l'utilisateur a tape.
                        if let Some(s) = crate::autofill::autofill_candidate(&v) {
                            if self.man_name.trim().is_empty() || self.man_name == self.auto_filled.0 {
                                self.man_name = s.game_name.clone();
                                self.auto_filled.0 = s.game_name.clone();
                            }
                            let p = s.local_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                            if self.man_path.trim().is_empty() || self.man_path == self.auto_filled.1 {
                                self.man_path = p.clone();
                                self.auto_filled.1 = p;
                            }
                        }
                        self.auto_sugg = v;
                    }
                }
                Msg::SystemLight(l) => self.sys_light = l,
                Msg::UsageText(t) => {
                    if self.usage_view.is_some() {
                        self.usage_view = Some(t);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ theme / manette

    pub(super) fn apply_theme(&mut self, ectx: &egui::Context) {
        let dark = self.gallery_dark().unwrap_or_else(|| vm::resolve_dark(self.model.settings.theme, &CachedProbe(self.sys_light)));
        // Mode grand format : "Auto" = actif des qu'une manette utilisable est detectee, retour au
        // choix de l'utilisateur a la deconnexion (voir uilogic::effective_big).
        // Linux : la manette passe par Steam Input (jamais « vue ») ; « Auto » suit alors le Steam Deck / le mode jeu.
        #[cfg(not(windows))]
        let pad_like = uilogic::pad_present(&self.pad.status()) || winutil::handheld_session();
        #[cfg(windows)]
        let pad_like = uilogic::pad_present(&self.pad.status());
        let big = uilogic::effective_big(self.model.settings.big_choice(), pad_like);
        self.big_now = big;
        let key = (dark, self.model.settings.accent, big);
        if self.theme_key == Some(key) {
            return;
        }
        self.theme_key = Some(key);
        let p = theme::palette(dark, key.1);
        self.palette = p;
        let c = |x: theme::Rgb| Color32::from_rgb(x[0], x[1], x[2]);
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        // Selection : fond d'accent, texte noir ou blanc selon le contraste calcule.
        v.selection.bg_fill = c(p.accent);
        v.selection.stroke.color = c(p.on_accent);
        v.hyperlink_color = c(p.ring);
        // Fonds : leger changement au survol et au focus (le cerclage de 2 px est dessine par `deco`).
        v.widgets.inactive.weak_bg_fill = c(p.widget_bg);
        v.widgets.inactive.bg_fill = c(p.widget_bg);
        v.widgets.inactive.fg_stroke.color = c(p.text);
        // Libelles de champs (texte non interactif) : couleur de texte principale du theme, jamais un gris peu lisible.
        v.widgets.noninteractive.fg_stroke.color = c(p.text);
        for (w, fill) in [(&mut v.widgets.hovered, p.hover_fill), (&mut v.widgets.active, p.active_fill), (&mut v.widgets.open, p.hover_fill)] {
            w.weak_bg_fill = c(fill);
            w.bg_fill = c(fill);
            w.fg_stroke.color = c(p.text);
            w.bg_stroke = egui::Stroke::new(1.0_f32, c(p.ring));
        }
        let mut style = egui::Style { visuals: v, ..Default::default() };
        if big {
            for (_, f) in style.text_styles.iter_mut() {
                f.size *= 1.3;
            }
        }
        style.spacing.interact_size.y = if big { 44.0 } else { 24.0 };
        style.spacing.button_padding = if big { egui::vec2(14.0, 8.0) } else { egui::vec2(8.0, 4.0) };
        style.spacing.item_spacing = if big { egui::vec2(10.0, 10.0) } else { egui::vec2(8.0, 6.0) };
        // Infobulles : delai unique, reglable dans uilogic::TOOLTIP_DELAY_MS.
        style.interaction.tooltip_delay = uilogic::TOOLTIP_DELAY_MS as f32 / 1000.0;
        ectx.set_style(style);
    }
}

impl App {
    /// Charge UNE FOIS une police systeme couvrant le chinois (police de repli d'egui) ; sans police trouvee, le chinois se
    /// replie sur l'anglais lisible (journalise). Appele seulement quand le chinois ou un selecteur de langue est visible.
    pub(super) fn ensure_cjk(&mut self, ctx: &egui::Context) {
        if self.cjk_tried {
            return;
        }
        self.cjk_tried = true;
        let lg = Logger::new(self.shared.data_dir.join("logs").join("watch.log"), false);
        let cands = uilogic::system_cjk_candidates();
        let loaded = uilogic::pick_font(&cands, &|p| p.is_file()).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b)));
        match loaded {
            Some((path, bytes)) => {
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert("cjk".to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    if let Some(v) = fonts.families.get_mut(&fam) {
                        v.push("cjk".to_string());
                    }
                }
                ctx.set_fonts(fonts);
                self.cjk_loaded = true;
                crate::i18n::set_cjk_font_available(true);
                lg.log(&format!("[Polices] police chinoise chargee : {}", path.display()));
            }
            None => {
                crate::i18n::set_cjk_font_available(false);
                lg.log("[Polices] aucune police chinoise systeme trouvee (msyh.ttc, simsun.ttc, NotoSansSC...) : le chinois se replie sur l'anglais");
            }
        }
    }
}
