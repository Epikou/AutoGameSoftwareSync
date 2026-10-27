//! Document d'etat `status.json` (lecture seule, pour les integrations) : calcul, thread d'ecriture, ecriture finale.
use super::*;

/// Empreinte des etats visibles modifies par les threads de fond (voir `wake_signature`).
type WakeSig = (usize, u8, usize, u8, usize, usize, bool, bool, bool);

impl Shared {
    /// Construit le document d'etat (lecture seule) a partir de la config, des reglages et des etats calcules.
    /// Journalise chaque changement d etat (une ligne) et lance immediatement la verification d un jeu ferme « cloud plus recent ».
    pub(super) fn observe_states(&self, m: &AppModel) {
        let lg = Logger::new(self.data_dir.join("logs").join("watch.log"), false);
        let dest = m.dest();
        let names: Vec<String> = m.config.games.iter().map(|g| g.name.clone()).collect();
        let Ok(mut tr) = self.transitions.lock() else { return };
        tr.retain(&names);
        let now = crate::util::now_ms();
        for g in &m.config.games {
            let st = m.statuses.get(&g.name).cloned().unwrap_or(GameStatus::Never);
            let mut remote = || {
                let machine = dest.as_deref().and_then(|d| backup::active_meta(d, g)).map(|x| x.machine).unwrap_or_else(|| "?".to_string());
                let t = crate::timefmt::format_now_tz(now, Lang::Fr);
                (machine, t.split_whitespace().rev().find(|w| w.contains(':')).unwrap_or("").to_string())
            };
            let o = tr.observe(&g.name, &st, now, &mut remote);
            if let Some(l) = o.line {
                lg.log(&format!("[{}] {}", g.name, l));
            }
            // Verification immediate : seulement pour un element AVEC programme (sans programme : planification ou demande
            // explicite de l'utilisateur uniquement, voir watch::tick_verifiers).
            let has_exe = g.exe.as_deref().map(|e| !e.trim().is_empty()).unwrap_or(false);
            if o.trigger && has_exe && matches!(self.state(), WatchState::Running) {
                if let Ok(mut q) = self.sync_requests.lock() {
                    q.push(Some(g.name.clone()));
                }
            }
        }
    }

    pub(super) fn build_status(&self, reachable: bool) -> Option<crate::statusfile::StatusDoc> {
        use crate::statusfile::*;
        let mut m = AppModel::new(make_ctx(&self.data_dir)).ok()?;
        m.refresh_statuses();
        self.observe_states(&m);
        let st = crate::config::State::load(&m.ctx).unwrap_or_default();
        let dest = m.dest();
        let games: Vec<GameIn> = m
            .config
            .games
            .iter()
            .map(|g| GameIn {
                name: g.name.clone(),
                key: cloud::dest_key(g),
                exe: g.exe.clone(),
                install_dir: m.settings.per_game.get(&g.name).and_then(|p| p.exe_path.as_ref()).and_then(|p| p.parent().map(|d| d.to_path_buf())),
                save_path: g.path.clone(),
                status: m.statuses.get(&g.name).cloned().unwrap_or(GameStatus::Never),
                last_sync_ms: st.last_sync_ms.get(&g.name).copied(),
            })
            .collect();
        let watching = matches!(self.state(), WatchState::Running);
        let user = std::env::var("USERNAME").unwrap_or_default();
        let mut mach_names: Vec<String> = vec![m.ctx.machine.clone()];
        let mut extras: Vec<GameExtra> = Vec::new();
        let poll_ms = crate::reactive::poll_interval_ms(m.settings.reactivity, m.settings.poll_custom_s);
        let now = crate::util::now_ms();
        let lg = Logger::new(self.data_dir.join("logs").join("watch.log"), false);
        for g in &m.config.games {
            let head = dest.as_deref().and_then(|d| backup::active_meta(d, g));
            if let Some(h) = &head {
                mach_names.push(h.machine.clone());
                // Accueil : « a jour sur N appareils ». Contenu local = version active -> marque de position de cet appareil
                // (ecrite une seule fois, jamais pendant une copie).
                let calm = crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) == 0;
                if let (Some(d), true, true) = (dest.as_deref(), reachable && calm, matches!(m.statuses.get(&g.name), Some(GameStatus::UpToDate))) {
                    if crate::home::note_position(d, &m.ctx.machine, &cloud::dest_key(g), &h.version_id(), now) {
                        lg.log(&format!("[{}] position de cet appareil notee dans le coffre ({})", g.name, h.version_id()));
                    }
                }
            }
            let (lf, ls) = backup::local_summary(&m.ctx, g);
            extras.push(GameExtra {
                display_name: g.shown().to_string(),
                last_backup_ms: st.last_backup_ms.get(&g.name).copied(),
                last_restore_ms: st.last_restore_ms.get(&g.name).copied(),
                last_action: st.last_action.get(&g.name).cloned(),
                remote_machine: head.as_ref().map(|h| h.machine.clone()),
                remote_ms: head.as_ref().map(|h| h.created_ms),
                local_files: lf,
                local_size: ls,
                versions: dest.as_deref().map(|d| backup::list_versions(d, g).len() as u64).unwrap_or(0),
                machine_color: None,
                next_check_ms: Some(now + poll_ms),
                schedule: g.schedule.as_ref().filter(|s| s.is_valid()).map(|s| {
                    let next = crate::schedule::next_run_utc(s, s.last_run_utc.unwrap_or(now as i64 / 1000), now as i64 / 1000, &crate::timefmt::WinTz);
                    crate::statusfile::ScheduleEntry { mode: if s.mode == crate::schedule::SchedMode::Fixed { "fixed" } else { "interval" }.to_string(), next_run_utc: next.map(|n| crate::statusfile::iso_utc(n.max(0) as u64 * 1000)) }
                }),
                locations: g
                    .all_locations()
                    .iter()
                    .map(|l| {
                        let t = crate::store::Target::of(&g.pseudo_for(l));
                        let (n, sz) = if t.exists() { t.scan(&m.ctx).map(|(f, _)| (f.len() as u64, f.iter().map(|x| x.size).sum())).unwrap_or((0, 0)) } else { (0, 0) };
                        LocEntry { id: l.id.clone(), label: l.label.clone(), path: l.path.display().to_string(), is_file: l.is_file, filter: l.filter.clone(), enabled: l.enabled, exists: l.exists(), files: n, size_bytes: sz }
                    })
                    .collect(),
                origin_device: Some(origin_device_name(matches!(m.statuses.get(&g.name), Some(GameStatus::UpToDate)), head.as_ref().map(|h| h.machine.as_str()), &m.ctx.machine)),
                origin_device_color: None,
            });
        }
        let cmap = crate::machinecolor::assign(&mach_names, &m.settings.machine_colors);
        for (x, g) in extras.iter_mut().zip(&m.config.games) {
            if let Some(h) = dest.as_deref().and_then(|d| backup::active_meta(d, g)) {
                x.machine_color = cmap.get(&h.machine).map(|c| crate::machinecolor::to_hex(*c));
            }
            x.origin_device_color = x.origin_device.as_ref().and_then(|n| cmap.get(n)).map(|c| crate::machinecolor::to_hex(*c));
        }
        let gx = GlobalExtra {
            machine: m.ctx.machine.clone(),
            machine_color: cmap.get(&m.ctx.machine).map(|c| crate::machinecolor::to_hex(*c)).unwrap_or_default(),
            reactivity_mode: format!("{:?}", m.settings.reactivity).to_lowercase(),
            conflict_mode: format!("{:?}", m.settings.conflict_mode).to_lowercase(),
            no_shared_folder: dest.is_none(),
            detection_scope: m.settings.detect_scope.code().to_string(),
            devices: {
                let mut seen: Vec<String> = Vec::new();
                for n in &mach_names {
                    if !seen.contains(n) {
                        seen.push(n.clone());
                    }
                }
                seen.into_iter().map(|n| DeviceEntry { color: cmap.get(&n).map(|c| crate::machinecolor::to_hex(*c)).unwrap_or_default(), id: n.clone(), name: n }).collect()
            },
        };
        let mut doc = build(&Inputs {
            app_version: crate::update::current_version(),
            now_ms: crate::util::now_ms(),
            watching,
            paused: self.paused.load(Ordering::SeqCst),
            reachable: reachable && dest.is_some(),
            shared: dest.as_deref(),
            user: &user,
            games,
        });
        enrich(&mut doc, &extras, &gx);
        Some(doc)
    }

    /// Empreinte des etats visibles dans l'interface que les threads de fond modifient (operations en cours, progression
    /// de la surveillance, evenements a montrer, dossier partage joignable, cartes de detection, etat de la surveillance).
    fn wake_signature(&self, reachable: bool) -> WakeSig {
        let ops = crate::perf::ACTIVE_OPS.load(Ordering::SeqCst);
        let phase = self.watch_progress.view().phase as u8;
        let events = self.ui_events.lock().map(|q| q.len()).unwrap_or(0);
        let reach = self.dest_reach.load(Ordering::SeqCst);
        let cards = self.proposals.lock().map(|p| p.queue.len()).unwrap_or(0);
        let refresh = self.know_refresh.lock().map(|q| q.len()).unwrap_or(0);
        let watching = matches!(self.state(), WatchState::Running);
        let paused = self.paused.load(Ordering::SeqCst);
        (ops, phase, events, reach, cards, refresh, watching, paused, reachable)
    }

    /// Thread de l'etat : fonctionne aussi fenetre cachee. Recalcul sur changement (surveillance, pause, dossier partage,
    /// evenements des jeux) et toutes les 30 s ; n'ecrit que si le contenu change (min. 1 s entre deux ecritures).
    pub fn spawn_status_thread(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::spawn(move || {
            use crate::statusfile::*;
            let lg = Logger::new(me.data_dir.join("logs").join("watch.log"), false);
            let mut w = StatusWriter::default();
            let mut sig: Option<(bool, bool)> = None;
            let mut reachable = true;
            let mut last_reach_check = Instant::now() - Duration::from_secs(60);
            let mut last_calc = Instant::now() - Duration::from_secs(3600);
            let mut pending: Option<StatusDoc> = None;
            let mut wake_sig: Option<WakeSig> = None;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if me.quitting.load(Ordering::SeqCst) {
                    return;
                }
                // Reveil de l'interface (2.0 : aucun redessin periodique au repos) quand un etat visible change.
                let s = me.wake_signature(reachable);
                if wake_sig.is_some() && wake_sig != Some(s) {
                    if let Some(c) = me.ui_ctx.lock().ok().and_then(|c| c.clone()) {
                        c.request_repaint();
                    }
                }
                wake_sig = Some(s);
                // Accessibilite du dossier partage : sondee toutes les 5 s (dans ce thread, jamais dans l'UI).
                if last_reach_check.elapsed() >= Duration::from_secs(5) {
                    last_reach_check = Instant::now();
                    let dest = Config::load(&make_ctx(&me.data_dir)).ok().and_then(|c| c.dest);
                    let r = dest.map(|d| d.is_dir()).unwrap_or(false);
                    if r != reachable {
                        reachable = r;
                        me.status_dirty.store(true, Ordering::SeqCst);
                    }
                }
                let s = (matches!(me.state(), WatchState::Running), me.paused.load(Ordering::SeqCst));
                if sig != Some(s) {
                    sig = Some(s);
                    me.status_dirty.store(true, Ordering::SeqCst);
                }
                if last_calc.elapsed() >= Duration::from_millis(SAFETY_NET_MS) {
                    me.status_dirty.store(true, Ordering::SeqCst);
                }
                if me.status_dirty.swap(false, Ordering::SeqCst) {
                    last_calc = Instant::now();
                    pending = me.build_status(reachable);
                }
                if let Some(doc) = pending.clone() {
                    match w.write_if_changed(&me.data_dir, &doc, crate::util::now_ms(), false) {
                        Wrote::Throttled => {}
                        Wrote::Written { first } => {
                            if first {
                                lg.log(&format!("[Etat] fichier d'etat ecrit : {}", StatusWriter::path(&me.data_dir).display()));
                            }
                            if let Ok(mut l) = me.status_last.lock() {
                                *l = Some(doc);
                            }
                            pending = None;
                        }
                        Wrote::Unchanged => pending = None,
                        Wrote::Failed { first, error } => {
                            if first {
                                lg.log(&format!("[Etat] fichier d'etat non inscriptible : {}", error));
                            }
                            pending = None;
                        }
                    }
                }
            }
        });
    }

    /// Fermeture : dernier etat avec `watching: false` (ecriture immediate).
    pub fn write_status_final(&self) {
        let doc = self.status_last.lock().ok().and_then(|l| l.clone()).or_else(|| self.build_status(true));
        if let Some(mut d) = doc {
            d.watching = false;
            d.updated_at_utc = crate::statusfile::iso_utc(crate::util::now_ms());
            let mut w = crate::statusfile::StatusWriter::default();
            let _ = w.write_if_changed(&self.data_dir, &d, crate::util::now_ms(), true);
        }
    }
}
