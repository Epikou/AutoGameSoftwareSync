//! Detection automatique des jeux : thread de detection (aucun sous-processus), carte de proposition et reglages.
use super::*;
use crate::gamedetect::{self, Blockers, DbHit, Memory};
use crate::gui::ui::{self as u, Btn, Glyph};

impl Shared {
    /// Thread leger : toutes les ~5 s, enumere les processus (une capture Toolhelp), classe ceux demarres apres le lancement
    /// de l'appli et pousse les propositions. Ne touche jamais a la fenetre (aucun focus vole).
    pub fn spawn_detect_thread(self: &Arc<Self>) {
        let me = self.clone();
        let _ = std::thread::Builder::new().name("agss-detect".into()).spawn(move || {
            let mut det = gamedetect::Detector::default();
            let mut was_enabled = false;
            let mut roots: Vec<String> = Vec::new();
            let mut roots_at = Instant::now() - Duration::from_secs(3600);
            let lg = Logger::new(me.data_dir.join("logs").join("watch.log"), false);
            let mut show = gamedetect::ShowOnClose::default();
            let mut last_scan = Instant::now() - Duration::from_secs(60);
            let mut facts = crate::procfacts::FactsCache::default();
            let mut prof_exes: std::collections::HashSet<String> = std::collections::HashSet::new();
            // Connaissance intelligente, couche 3 : apprentissage pendant l'utilisation (cle de carte -> programme, observation).
            let mut learners: super::knowledge_ui::Learners = HashMap::new();
            let mut learn_list = crate::knowledge::store::LearnList::load(&me.data_dir);
            let mut learn_at = Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if me.quitting.load(Ordering::SeqCst) {
                    return;
                }
                for (_, l) in learners.values_mut() {
                    l.pump();
                }
                let s = crate::settings::Settings::load_from(&me.data_dir).unwrap_or_default();
                if !s.detect_enabled {
                    was_enabled = false;
                    me.det_running.store(false, Ordering::SeqCst);
                    learners.clear();
                    continue;
                }
                // Fermeture d un jeu detecte : afficher l appli (une fois, jamais pendant un jeu).
                {
                    let ctx = gamedetect::ShowCtx {
                        enabled: true, // toujours actif quand la detection est activee (interrupteur principal)
                        cards_pending: me.proposals.lock().map(|p| !p.queue.is_empty()).unwrap_or(false),
                        other_game_running: me.games_running.load(Ordering::SeqCst) || det.running_count() > 0,
                        fullscreen_active: winutil::foreground_is_fullscreen(),
                        tutorial: me.ui_busy.load(Ordering::SeqCst),
                        confirm_open: false,
                        already_foreground: winutil::own_window_is_foreground(),
                    };
                    match show.tick(&ctx, crate::util::now_ms(), &mut || winutil::bring_to_front(APP_TITLE)) {
                        gamedetect::ShowAction::BringToFront => lg.log("[Detection] fin d'utilisation : appli passee au premier plan (cartes en attente)"),
                        gamedetect::ShowAction::FlashTaskbar => {
                            lg.log(if cfg!(windows) {
                                "[Detection] fin d'utilisation : premier plan refuse par Windows, bouton de la barre des taches clignotant"
                            } else {
                                "[Detection] fin d'utilisation : premier plan refuse par le gestionnaire de fenetres, demande d'attention envoyee"
                            });
                            winutil::flash_taskbar(APP_TITLE);
                        }
                        gamedetect::ShowAction::None => {}
                    }
                }
                if last_scan.elapsed() < Duration::from_secs(5) {
                    continue;
                }
                last_scan = Instant::now();
                if !was_enabled {
                    det = gamedetect::Detector::default(); // nouvelle reference : les jeux deja ouverts ne sont jamais proposes
                    was_enabled = true;
                    roots_at = Instant::now() - Duration::from_secs(3600); // recharge aussi les profils (extra)
                }
                det.scope = s.detect_scope;
                if let Ok(mut q) = me.proposals.lock() {
                    if q.retain_scope(s.detect_scope) > 0 {
                        let _ = q.save(&me.data_dir);
                    }
                }
                if roots_at.elapsed() > Duration::from_secs(300) {
                    roots_at = Instant::now();
                    // Profils communautaires (applications, configurations) : recharges avec les racines de bibliotheques.
                    let emb_list = crate::profiles::load_embedded();
                    let (custom_list, _) = crate::profiles::load_custom(&me.data_dir, &emb_list);
                    // Seules les applications AVEC un profil sont proposees : embarques (suggest par defaut), personnalises (suggest explicite).
                    // Le profil seul ne suffit pas : programme installe ou dossier correspondant (score de confiance).
                    let vars = crate::profiles::Vars::real();
                    let mut idx = crate::profiles::detection_hints(&emb_list, true, &vars);
                    idx.extend(crate::profiles::detection_hints(&custom_list, false, &vars));
                    prof_exes = idx.keys().cloned().collect();
                    det.extra = Some(Box::new(move |p: &crate::process::ProcInfo, f: &gamedetect::Facts| gamedetect::assess_profile(p, f, idx.get(&p.exe)?)));
                    facts.manifests = crate::procfacts::launcher_manifests();
                    roots = detect::steam_root().map(|r| detect::steam_libraries(&r)).unwrap_or_default().iter().map(|l| gamedetect::norm_path(&l.join("steamapps").join("common"))).collect();
                    // Linux : dossier de jeux par defaut de Lutris et de Heroic (~/Games), et dossiers d'installation de Heroic.
                    #[cfg(not(windows))]
                    if let Some(h) = dirs::home_dir() {
                        roots.push(gamedetect::norm_path(&h.join("Games")));
                        for g in detect::heroic_games(&h) {
                            // Jamais le dossier personnel lui-meme ni un de ses parents (tout y deviendrait « jeu »).
                            if let Some(p) = g.install_dir.parent().filter(|p| *p != h.as_path() && !h.starts_with(p) && p.components().count() > 3) {
                                let r = gamedetect::norm_path(p);
                                if !roots.contains(&r) {
                                    roots.push(r);
                                }
                            }
                        }
                    }
                }
                let Ok(ext) = crate::process::list_processes_ext() else { continue };
                let procs: Vec<crate::process::ProcInfo> = ext.iter().map(|e| e.info.clone()).collect();
                let table: std::collections::HashMap<u32, crate::process::ProcExt> = ext.into_iter().map(|e| (e.info.pid, e)).collect();
                facts.retain_alive(&table);
                let known: Vec<String> = Config::load(&make_ctx(&me.data_dir)).map(|c| c.games.iter().filter_map(|g| g.exe.clone()).map(|e| crate::process::normalize_exe_name(&e)).collect()).unwrap_or_default();
                let db = crate::gamedb::embedded(&me.data_dir);
                let dbf = |exe: &str| -> Vec<DbHit> { db.find_by_exe(exe).into_iter().map(|g| DbHit { name: g.name.clone(), install_dirs: g.install_dirs.clone() }).collect() };
                let env = gamedetect::Env { db: &dbf, launcher_roots: &roots, known_exes: &known };
                let mem = Memory::load(&me.data_dir);
                let now = crate::util::now_ms();
                // Indices d'execution (fichier, parent, age, bibliotheques) : seulement pour les processus qui ressemblent a quelque chose.
                let wanted: Vec<&crate::process::ProcInfo> = procs.iter().filter(|p| gamedetect::assess(p, &env, &gamedetect::Facts::default()).is_some() || prof_exes.contains(&p.exe) || facts.under_manifest(p)).collect();
                det.facts = wanted.into_iter().map(|p| ((p.pid, p.start), facts.facts(p, &table, now))).collect();
                let events = det.scan(&procs, &env, &mem, s.notify_global, now);
                for c in det.refused.drain(..) {
                    let why = c.confidence.as_ref().map(|s| s.summary()).unwrap_or_default();
                    lg.log(&format!("[Detection] non propose : {} ({}) score {} [{}]", c.exe, c.name, why, c.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()));
                }
                // Programmes ajoutes sans emplacement : apprentissage a chaque utilisation, carte a la fermeture.
                if learn_at.elapsed() > Duration::from_secs(20) {
                    learn_at = Instant::now();
                    learn_list = crate::knowledge::store::LearnList::load(&me.data_dir);
                }
                me.learn_list_tick(&mut learners, &mut learn_list, &procs, &lg, now);
                for ev in events {
                    match ev {
                        gamedetect::Event::Ended(c) => {
                            show.on_closed(now);
                            if let Some((p, l)) = learners.remove(&c.key) {
                                me.learn_done(&c.key, &p, l, &lg, now);
                            }
                        }
                        gamedetect::Event::Launched(c) => {
                            let why = c.confidence.as_ref().map(|s| s.summary()).unwrap_or_default();
                            lg.log(&format!("[Detection] candidat : {} ({}) score {} -> carte ajoutee a la file", c.exe, c.name, why));
                            if c.profile_id.is_none() && !learners.contains_key(&c.key) && !learn_list.contains(&c.exe) {
                                if let Some(p) = me.needs_learning(&c, &db) {
                                    lg.log(&format!("[Connaissance] {} : emplacement inconnu, apprentissage pendant l'utilisation", c.exe));
                                    let l = crate::knowledge::learn::Learner::start(&p, p.folders(), now);
                                    learners.insert(c.key.clone(), (p, l));
                                }
                            }
                            me.queue_card(c, now);
                        }
                        gamedetect::Event::Notify(c) => {
                            let body = tr(s.lang, K::DetNotifLaunch).replace("{}", &c.name);
                            crate::notify::Notifier::notify(&crate::notify::WinNotifier, "AutoGameSoftwareSync", &body);
                        }
                    }
                }
                // Aucune recherche en ligne pendant l'utilisation d'un programme detecte (ou en apprentissage).
                me.det_running.store(det.running_count() > 0 || !learners.is_empty(), Ordering::SeqCst);
            }
        });
    }

    /// Ajoute une carte a la file (sans doublon) et la persiste (expiration 7 jours).
    pub fn queue_card(&self, c: gamedetect::Candidate, now: u64) {
        if let Ok(mut p) = self.proposals.lock() {
            p.push(c, now);
            let _ = p.save(&self.data_dir);
        }
    }
}

impl App {
    /// Texte de la fiabilite d'une detection : titre puis raisons (une par ligne) ; `expert` : sur une ligne, avec les points.
    fn confidence_text(&self, c: &gamedetect::Confidence, expert: bool) -> String {
        let head = self.t(K::DetConfidence).replace("{}", &c.score.to_string());
        let rs: Vec<String> = c.reasons.iter().map(|r| if expert { format!("{} ({:+})", self.t(reason_key(*r)), r.points()) } else { self.t(reason_key(*r)).to_string() }).collect();
        if rs.is_empty() {
            head
        } else if expert {
            format!("{} · {}", head, rs.join(", "))
        } else {
            format!("{}\n• {}", head, rs.join("\n• "))
        }
    }

    /// Texture de l'icone de l'exe d'une carte (extraction existante) ; None = icone generique dessinee.
    fn prop_icon(&mut self, ctx: &egui::Context, cand: &gamedetect::Candidate) -> Option<egui::TextureHandle> {
        let key = format!("prop:{}", cand.key);
        if !self.icons.contains_key(&key) {
            let tex = cand
                .path
                .as_ref()
                .and_then(|p| winutil::exe_icon_rgba(p))
                .map(|(rgba, w, h)| ctx.load_texture(key.clone(), egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR));
            self.icons.insert(key.clone(), tex);
        }
        self.icons.get(&key).cloned().flatten()
    }

    /// Dessine une carte « fantome » (non interactive) : fond arrondi pivote, icone et nom. Sert aux couches de la pile et aux animations.
    #[allow(clippy::too_many_arguments)]
    fn draw_ghost(&self, p: &egui::Painter, center: egui::Pos2, size: egui::Vec2, g: gamedetect::LayerGeom, off: egui::Vec2, extra_rot: f32, alpha: f32, name: &str, tex: Option<&egui::TextureHandle>) {
        let ang = (g.rot_deg + extra_rot).to_radians();
        let (sn, cs) = ang.sin_cos();
        let rot = |v: egui::Vec2| egui::vec2(v.x * cs - v.y * sn, v.x * sn + v.y * cs);
        let c = center + egui::vec2(g.dx, g.dy) + off;
        let half = size * 0.5 * g.scale;
        // Rectangle arrondi : 4 coins de 6 points, puis rotation.
        let r = 18.0 * g.scale;
        let mut pts: Vec<egui::Pos2> = Vec::new();
        for (cx, cy, a0) in [(half.x - r, -half.y + r, -90.0f32), (half.x - r, half.y - r, 0.0), (-half.x + r, half.y - r, 90.0), (-half.x + r, -half.y + r, 180.0)] {
            for i in 0..=6 {
                let a = (a0 + 15.0 * i as f32).to_radians();
                pts.push(c + rot(egui::vec2(cx + r * a.cos(), cy + r * a.sin())));
            }
        }
        let bg = rgb(self.palette.panel_bg);
        let dim = g.dim;
        let mix = |v: u8| (v as f32 * (1.0 - dim)) as u8;
        let fill = Color32::from_rgba_unmultiplied(mix(bg.r()), mix(bg.g()), mix(bg.b()), (255.0 * alpha) as u8);
        let acc = rgb(self.palette.accent);
        let stroke = egui::Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(mix(acc.r()), mix(acc.g()), mix(acc.b()), (255.0 * alpha) as u8));
        p.add(egui::Shape::convex_polygon(pts, fill, stroke));
        // Icone (quad texture pivote) ou icone generique.
        let isz = 84.0 * g.scale;
        let icon_c = c + rot(egui::vec2(0.0, -half.y + 28.0 * g.scale + isz * 0.5));
        let tint = Color32::from_rgba_unmultiplied(mix(255), mix(255), mix(255), (255.0 * alpha) as u8);
        if let Some(t) = tex {
            let mut mesh = egui::epaint::Mesh::with_texture(t.id());
            let corners = [(-1.0f32, -1.0f32, 0.0f32, 0.0f32), (1.0, -1.0, 1.0, 0.0), (1.0, 1.0, 1.0, 1.0), (-1.0, 1.0, 0.0, 1.0)];
            for (x, y, u, v) in corners {
                mesh.vertices.push(egui::epaint::Vertex { pos: icon_c + rot(egui::vec2(x * isz * 0.5, y * isz * 0.5)), uv: egui::pos2(u, v), color: tint });
            }
            mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
            p.add(egui::Shape::mesh(mesh));
        } else {
            p.circle_filled(icon_c, isz * 0.5, Color32::from_rgba_unmultiplied(mix(acc.r()), mix(acc.g()), mix(acc.b()), (255.0 * alpha) as u8));
            super::widgets::paint_pad(p, egui::Rect::from_center_size(icon_c, egui::vec2(isz * 0.62, isz * 0.62)), Color32::WHITE.gamma_multiply(alpha), Color32::TRANSPARENT);
        }
        // Nom (pivote).
        let font = egui::FontId::proportional(20.0 * g.scale);
        let col = rgb(self.palette.text);
        let galley = p.layout_no_wrap(name.to_string(), font, Color32::from_rgba_unmultiplied(mix(col.r()), mix(col.g()), mix(col.b()), (255.0 * alpha) as u8));
        let top_left = c + rot(egui::vec2(0.0, -half.y + 28.0 * g.scale + isz + 14.0 * g.scale)) - rot(egui::vec2(galley.size().x * 0.5, 0.0));
        p.add(egui::epaint::TextShape::new(top_left, galley, Color32::WHITE).with_angle(ang));
    }

    /// Overlay de proposition : fond assombri, carte centree (icone de l'exe, nom, question, chemins verifies, avertissements,
    /// Oui / Non / Plus tard), pile de cartes derriere, animations legeres (repaint seulement pendant leur duree).
    pub(super) fn proposal_overlay(&mut self, ctx: &egui::Context) {
        use gamedetect::{Answer, CardState};
        let t_ms = (ctx.input(|i| i.time) * 1000.0) as u64;
        let now = crate::util::now_ms();
        let b = Blockers {
            tutorial: self.wizard.active,
            confirm_open: self.model.confirm.is_some() || self.pending_add.is_some() || self.pending_overlap.is_some(),
            restoring: self.op_progress.is_some(),
            window_hidden: !winutil::own_window_visible(APP_TITLE) || ctx.input(|i| i.viewport().minimized.unwrap_or(false)),
            detection_off: !self.model.settings.detect_enabled,
        };
        // Pas de carte (jamais) pendant qu'un jeu surveille tourne et que sa fenetre a le focus : l'appli n'est alors pas au premier plan.
        let snapshot = {
            let g = self.shared.proposals.lock().ok();
            g.and_then(|q| q.current(b, now).map(|(p, rank, total)| (p.clone(), rank, total, q.queue.iter().filter(|o| o.cand.key != p.cand.key).map(|o| o.cand.clone()).collect::<Vec<_>>())))
        };
        let screen = ctx.screen_rect();
        let center = screen.center();
        let card_size = self.prop_card_rect.map(|r| r.size()).unwrap_or(egui::vec2(440.0, 340.0));
        let acc = rgb(self.palette.accent);
        // Animation de sortie en cours : carte du dessus qui s'envole, suivante qui monte.
        if let Some(ex) = self.prop_exit.clone() {
            let prog = gamedetect::exit_progress(ex.answer, ex.reduce, ex.start_ms, t_ms);
            if prog < 1.0 {
                egui::Area::new(egui::Id::new("prop_dim")).order(egui::Order::Foreground).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
                    let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
                    ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(150));
                });
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("prop_ghost_layer")));
                let frame = gamedetect::exit_frame(ex.answer, prog, ex.reduce, ex.layers);
                let mut g_exit = gamedetect::layer_geom(frame.to_depth.unwrap_or(0.0));
                g_exit.scale *= frame.scale;
                let exit_off = egui::vec2(frame.dx, frame.dy);
                // « Plus tard » : la carte glisse DERRIERE le paquet (dessinee d'abord).
                if frame.to_depth.is_some() {
                    self.draw_ghost(&painter, center, card_size, g_exit, exit_off, frame.rot_deg, frame.alpha, &ex.name, ex.tex.as_ref());
                }
                // Couches de la pile : la carte suivante monte au premier plan en se redressant.
                let mut layers: Vec<(f32, gamedetect::Candidate)> = Vec::new();
                if let Some(c) = snapshot.as_ref().map(|s| s.0.cand.clone()) {
                    layers.push((gamedetect::rise_depth(1, prog), c));
                }
                let behind_list: Vec<gamedetect::Candidate> = snapshot.as_ref().map(|s| s.3.iter().filter(|c| c.key != ex.key).cloned().collect()).unwrap_or_default();
                for (i, c) in behind_list.iter().take(gamedetect::MAX_LAYERS).enumerate() {
                    layers.push((gamedetect::rise_depth(i + 2, prog), c.clone()));
                }
                for (depth, c) in layers.into_iter().rev() {
                    let tex = self.prop_icon(ctx, &c);
                    self.draw_ghost(&painter, center, card_size, gamedetect::layer_geom(depth), egui::Vec2::ZERO, 0.0, 1.0, &c.name, tex.as_ref());
                }
                // Eclat de couleur d'accent (Oui) : anneau qui s'elargit.
                if frame.burst > 0.01 {
                    let acc = rgb(self.palette.accent);
                    let rr = 90.0 + 160.0 * frame.burst;
                    painter.circle_stroke(center + exit_off, rr, egui::Stroke::new(5.0_f32 * frame.burst.max(0.2), acc.gamma_multiply(frame.burst.clamp(0.0, 1.0))));
                    for i in 0..10 {
                        let a = i as f32 * std::f32::consts::TAU / 10.0 + 0.3;
                        let (c, s) = (a.cos(), a.sin());
                        painter.line_segment([center + exit_off + egui::vec2(c, s) * (rr * 0.8), center + exit_off + egui::vec2(c, s) * (rr * 0.8 + 18.0 * frame.burst)], egui::Stroke::new(3.0_f32, acc.gamma_multiply(frame.burst.clamp(0.0, 1.0))));
                    }
                }
                if frame.to_depth.is_none() {
                    self.draw_ghost(&painter, center, card_size, g_exit, exit_off, frame.rot_deg, frame.alpha, &ex.name, ex.tex.as_ref());
                }
                ctx.request_repaint();
                return;
            }
            self.prop_exit = None;
            // La carte suivante est deja a sa place : pas de nouveau fondu.
            if let Some((p, ..)) = &snapshot {
                self.prop_state = CardState::Showing { key: p.cand.key.clone(), since_ms: 0 };
            }
        }
        // Confirmation breve apres « Oui ».
        if let CardState::Confirmed { name, until_ms } = self.prop_state.clone() {
            if t_ms < until_ms {
                egui::Area::new(egui::Id::new("prop_dim")).order(egui::Order::Foreground).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
                    let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
                    ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(120));
                });
                egui::Area::new(egui::Id::new("prop_confirm")).order(egui::Order::Tooltip).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
                    egui::Frame::new().fill(rgb(self.palette.panel_bg)).corner_radius(16.0).stroke(egui::Stroke::new(2.0_f32, acc)).inner_margin(20.0).show(ui, |ui| {
                        icon_label(ui, Icon::Check, &self.t(K::PropAdded).replace("{}", &name), Color32::from_rgb(60, 180, 100));
                    });
                });
                ctx.request_repaint_after(Duration::from_millis(120));
                return;
            }
            self.prop_state = CardState::Idle;
        }
        let Some((pending, rank, total, behind)) = snapshot else {
            self.prop_state.sync(None, t_ms);
            self.prop_key = None;
            return;
        };
        let cand = pending.cand.clone();
        self.prop_state.sync(Some(&cand.key), t_ms);
        let since = if let CardState::Showing { since_ms, .. } = &self.prop_state { *since_ms } else { t_ms };
        let alpha = gamedetect::fade(since, t_ms);
        if alpha < 1.0 {
            ctx.request_repaint();
        }
        // Emplacements appris a la fermeture du programme : la carte affichee est re-resolue.
        let refresh = self.shared.know_refresh.lock().map(|mut v| {
            let had = v.contains(&cand.key);
            v.retain(|k| *k != cand.key);
            had
        });
        if refresh.unwrap_or(false) && self.prop_path.trim().is_empty() {
            self.prop_key = None;
        }
        // Resolution des chemins (une fois par carte).
        if self.prop_key.as_deref() != Some(cand.key.as_str()) {
            self.prop_key = Some(cand.key.clone());
            self.prop_path.clear();
            self.prop_sugg.clear();
            self.prop_warnings.clear();
            self.prop_found.clear();
            self.prop_prog = None;
            if let Some(pid) = cand.profile_id.clone() {
                if !self.prof_loaded {
                    self.load_profiles();
                }
                if let Some(p) = self.profiles.iter().find(|p| p.id == pid).cloned() {
                    self.prop_warnings = p.warnings.clone();
                    let mut vars = self.prof_real_vars.clone();
                    for v in &p.variables {
                        if let Some(first) = crate::profiles::var_candidates(v, &vars).into_iter().next() {
                            vars.values.insert(v.name.clone(), first);
                        }
                    }
                    self.prop_sugg = p.locations.iter().filter(|l| l.default_enabled).filter_map(|l| crate::profiles::expand(&l.path, &vars).map(|path| crate::hints::Suggestion { game_name: p.name.clone(), exists: path.exists(), local_path: Some(path) })).collect();
                }
            }
            let (tx, c2, dd, cc, sh) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), cand.clone(), self.shared.clone());
            let dest_games = self.dest_games.clone();
            std::thread::spawn(move || {
                if cc.profile_id.is_some() {
                    return;
                }
                let db = crate::gamedb::embedded(&dd);
                let libs: Vec<PathBuf> = detect::steam_root().map(|r| detect::steam_libraries(&r)).unwrap_or_default();
                let mut sugg = crate::autofill::lookup(&db, &cc.exe, cc.path.as_deref(), &dest_games, &libs, &|p| p.exists());
                // Hook de verification manuelle (comme AGSS_FAKE_PAD) : emplacement de test fourni par l environnement.
                if let Some(p) = std::env::var_os("AGSS_TEST_SAVEPATH").map(PathBuf::from).filter(|p| p.exists()) {
                    sugg.insert(0, crate::hints::Suggestion { game_name: cc.name.clone(), local_path: Some(p), exists: true });
                }
                let known = sugg.iter().any(|s| s.exists && s.local_path.is_some());
                let _ = tx.send(Msg::PropPaths(cc.key.clone(), sugg));
                c2.request_repaint();
                // Ni la base ni les profils : connaissance intelligente (lanceurs, deduction, apprentissage, puis recherche).
                if !known {
                    let mut p = crate::knowledge::Program::describe(cc.path.as_deref(), &cc.exe, &cc.name, cc.app);
                    if p.install_dir.is_none() {
                        p.install_dir = cc.install_dir.clone();
                    }
                    let found = crate::knowledge::find_local(&p, &dd, crate::util::now_ms());
                    if found.is_empty() {
                        sh.request_web(super::knowledge_ui::KnowTarget::Card(cc.key.clone()), p.clone());
                    }
                    let _ = tx.send(Msg::Known(super::knowledge_ui::KnowTarget::Card(cc.key.clone()), p, found));
                    c2.request_repaint();
                }
            });
        }
        // Fond assombri (clics absorbes).
        egui::Area::new(egui::Id::new("prop_dim")).order(egui::Order::Foreground).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha((150.0 * alpha) as u8));
        });
        // Pile : couches derriere la carte (la plus eloignee d'abord).
        let (nlayers, badge) = gamedetect::visible_layers(behind.len());
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("prop_ghost_layer")));
        ctx.move_to_top(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("prop_ghost_layer")));
        for (i, c) in behind.iter().take(nlayers).enumerate().rev() {
            let tex = self.prop_icon(ctx, c);
            self.draw_ghost(&painter, center, card_size, gamedetect::layer_geom(i as f32 + 1.0), egui::vec2(0.0, (1.0 - alpha) * 18.0), 0.0, alpha, &c.name, tex.as_ref());
        }
        if badge > 0 {
            let pos = center + egui::vec2(card_size.x * 0.5 - 6.0, card_size.y * 0.5 + 40.0);
            painter.circle_filled(pos, 15.0, acc);
            painter.text(pos, egui::Align2::CENTER_CENTER, format!("+{}", badge), egui::FontId::proportional(13.0), rgb(self.palette.on_accent));
        }
        let icon = self.prop_icon(ctx, &cand);
        let mut answer: Option<Answer> = None;
        let mut go_share = false;
        self.focus.set_modal_scope(true);
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let tk = u::tokens(ctx);
        let expert = self.model.settings.expert_mode;
        let area = egui::Area::new(egui::Id::new("prop_card")).order(egui::Order::Tooltip).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, (1.0 - alpha) * 18.0)).show(ctx, |ui| {
            ui.set_opacity(alpha);
            egui::Frame::new().fill(u::col(tk.card)).corner_radius(18.0).stroke(egui::Stroke::new(2.0_f32, acc)).inner_margin(22.0).shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 2, color: Color32::from_black_alpha(120) }).show(ui, |ui| {
                ui.set_width(440.0);
                if total > 1 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(RichText::new(format!("{}/{}", rank, total)).small().weak());
                    });
                }
                let name = self.prop_sugg.first().map(|s| s.game_name.clone()).unwrap_or_else(|| cand.name.clone());
                // Fiabilite de la detection : visible en mode expert, sinon en infobulle (icone et nom).
                let conf_tip = cand.confidence.as_ref().map(|c| self.confidence_text(c, false));
                let conf_expert = cand.confidence.as_ref().filter(|_| self.model.settings.expert_mode).map(|c| self.confidence_text(c, true));
                ui.vertical_centered(|ui| {
                    // Grande icone de l'exe (ou icone generique stylee).
                    let (r, icon_resp) = ui.allocate_exact_size(egui::vec2(96.0, 96.0), egui::Sense::hover());
                    if let (Some(t), None) = (&conf_tip, &conf_expert) {
                        icon_resp.on_hover_text(t);
                    }
                    ui.painter().circle_filled(r.center(), 54.0, acc.gamma_multiply(0.22));
                    if let Some(t) = &icon {
                        ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    } else {
                        ui.painter().circle_filled(r.center(), 42.0, acc);
                        super::widgets::paint_pad(ui.painter(), r.shrink(26.0), Color32::WHITE, Color32::TRANSPARENT);
                    }
                    ui.add_space(8.0);
                    let name_resp = ui.label(RichText::new(&name).strong().color(acc));
                    match (&conf_tip, &conf_expert) {
                        (_, Some(x)) => {
                            ui.label(RichText::new(x).small().weak());
                        }
                        (Some(t), None) => {
                            name_resp.on_hover_text(t);
                        }
                        _ => {}
                    }
                    ui.add_space(2.0);
                    ui.label(RichText::new(self.t(K::PropHey).replace("{}", &name)).heading().strong());
                    ui.label(self.t(if cand.app { K::PropAskApp } else { K::PropAsk }));
                });
                ui.add_space(8.0);
                // Chemins detectes, avec indicateur de verification dessine.
                let found: Vec<std::path::PathBuf> = self.prop_sugg.iter().filter(|s| s.exists).filter_map(|s| s.local_path.clone()).collect();
                if found.is_empty() && !self.prop_found.is_empty() && cand.profile_id.is_none() {
                    // Connaissance intelligente : « Emplacement trouve » et sa confiance.
                    let kf = self.prop_found.clone();
                    if let Some(i) = self.known_block(ui, &kf, kf.len() > 1) {
                        self.prop_path = kf[i].path.display().to_string();
                    }
                } else if found.is_empty() && self.prop_path.trim().is_empty() {
                    icon_label(ui, Icon::Cross, self.t(K::PropNoPath), u::col(tk.state_text(u::State::Todo)));
                } else {
                    // Mode simple : lieu en mots simples (« dans tes données d'applis »), chemin complet en infobulle ;
                    // mode expert : chemin complet.
                    ui.label(RichText::new(self.t(K::PropWhere)).small().color(u::col(tk.weak)));
                    for p in &found {
                        let full = p.display().to_string();
                        let shown = if expert { full.clone() } else { format!("{} · {}", crate::timetravel::short_name(p), self.place_text(crate::timetravel::place_of(p, &self.known))) };
                        icon_label(ui, Icon::Check, &shown, u::col(tk.state_text(u::State::Safe))).on_hover_text(full);
                    }
                }
                if self.share_not_configured() {
                    // Pas encore de coffre : l'element sera ajoute, la copie commencera une fois le coffre choisi.
                    egui::Frame::new().fill(u::col(tk.orange).gamma_multiply(0.15)).corner_radius(u::tokens::R_MD).inner_margin(u::tokens::SP_S).show(ui, |ui| {
                        ui.label(RichText::new(self.t(K::PropNoShare)).color(u::col(tk.state_text(u::State::Todo))).small());
                        let r = u::button(ui, Btn::Secondary, self.t(K::VChooseVault));
                        if self.deco(ui, r, Some(K::TipChooseShare)).clicked() {
                            go_share = true;
                        }
                    });
                }
                // Avertissements des profils sensibles (applications / configurations).
                for w in self.prop_warnings.clone() {
                    egui::Frame::new().fill(u::col(tk.orange).gamma_multiply(0.15)).corner_radius(u::tokens::R_MD).inner_margin(u::tokens::SP_S).show(ui, |ui| {
                        ui.label(RichText::new(w).color(u::col(tk.state_text(u::State::Todo))).small());
                    });
                }
                let mut path = std::mem::take(&mut self.prop_path);
                if let Some(p) = self.take_picked("prop_path") {
                    path = p.path.display().to_string();
                }
                // Choix de l'emplacement : sélecteur guidé ; le champ du chemin n'existe qu'en mode expert.
                let open_pick = cand.profile_id.is_none()
                    && if expert {
                        self.wide_field(ui, &mut path, K::TipLocSave, true)
                    } else {
                        let k = if path.trim().is_empty() && found.is_empty() { K::VAddChoosePlace } else { K::AddChange };
                        let r = u::button_with(ui, Btn::Ghost, self.t(k), Some(Glyph::Folder), false);
                        self.deco(ui, r, Some(K::TipLocSave)).clicked()
                    };
                if !expert && !path.trim().is_empty() && found.is_empty() {
                    let p = std::path::PathBuf::from(path.trim());
                    let shown = format!("{} · {}", crate::timetravel::short_name(&p), self.place_text(crate::timetravel::place_of(&p, &self.known)));
                    icon_label(ui, Icon::Check, &shown, u::col(tk.state_text(u::State::Safe))).on_hover_text(path.trim());
                }
                self.prop_path = path;
                if open_pick {
                    let opts = self.pick_opts_for_program(self.prop_prog.as_ref(), &self.prop_found, &name, false);
                    self.open_picker(ctx, "prop_path", opts);
                }
                let chosen = self.prop_path.trim().to_string();
                let ok = cand.profile_id.is_some() || (!chosen.is_empty() && std::path::Path::new(&chosen).exists());
                ui.add_space(u::tokens::SP_S);
                // Disposition : « Oui » (principal, le plus grand) a gauche, « Non » juste apres, « Plus tard » aligne a droite.
                ui.horizontal(|ui| {
                    let r = ui.add_enabled_ui(ok, |ui| u::button_with(ui, Btn::Primary, self.t(K::BtnPropYes), None, true)).inner;
                    self.focus.set_default(r.id);
                    if self.deco(ui, r, Some(K::TipPropYes)).clicked() {
                        answer = Some(Answer::Yes);
                    }
                    ui.add_space(u::tokens::SP_S);
                    let r = u::button(ui, Btn::Secondary, self.t(K::BtnPropNo));
                    if self.deco(ui, r, Some(K::TipPropNo)).clicked() {
                        answer = Some(Answer::No);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = u::button(ui, Btn::Ghost, self.t(K::BtnPropLater));
                        if self.deco(ui, r, Some(K::TipPropLater)).clicked() {
                            answer = Some(Answer::Later);
                        }
                    });
                });
            });
        });
        if go_share {
            self.go_choose_share(ctx);
        }
        ctx.move_to_top(area.response.layer_id); // la carte active passe toujours devant la pile
        // ... mais derriere ses propres infobulles (fiabilite, boutons), sinon elles seraient cachees sous la carte.
        let card_layer = area.response.layer_id;
        let ghost = egui::Id::new("prop_ghost_layer");
        let tips: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().filter(|l| l.order == egui::Order::Tooltip && *l != card_layer && l.id != ghost && m.areas().is_visible(l)).collect());
        for l in tips {
            ctx.move_to_top(l);
        }
        self.focus.set_modal_scope(false);
        self.prop_card_rect = Some(area.response.rect);
        if esc && answer.is_none() && alpha >= 1.0 {
            answer = Some(Answer::Later); // Echap / B = Plus tard
        }
        if let Some(a) = answer {
            let dd = self.shared.data_dir.clone();
            let name = self.prop_sugg.first().map(|s| s.game_name.clone()).unwrap_or_else(|| cand.name.clone());
            let mut mem = Memory::load(&dd);
            if let Ok(mut q) = self.shared.proposals.lock() {
                gamedetect::apply_answer(&mut mem, &mut q, &cand, a, now);
                let _ = q.save(&dd);
            }
            let _ = mem.save(&dd);
            self.det_mem = mem;
            let (reduce, why) = gamedetect::reduce_decision(self.model.settings.card_anim, winutil::client_animations_enabled());
            if let Some(w) = why {
                self.pad_log(&format!("[Detection] animation de la carte remplacee par un fondu : {}", w));
            }
            self.prop_exit = Some(PropExit { answer: a, start_ms: t_ms, name: name.clone(), tex: icon.clone(), key: cand.key.clone(), reduce, layers: gamedetect::visible_layers(behind.len()).0 });
            self.prop_state.answer(a, &name, t_ms + gamedetect::confirm_start_ms(reduce));
            {
                let reason = match cand.why {
                    gamedetect::Why::LauncherFolder => "launcher",
                    gamedetect::Why::InDb => "base",
                    gamedetect::Why::Profile => "profile",
                    gamedetect::Why::Emulator => "emulator",
                    gamedetect::Why::Learned => "other",
                };
                let answer = match a {
                    Answer::Yes => "yes",
                    Answer::No => "no",
                    Answer::Later => "later",
                };
                let score = crate::usage::bucket_score(cand.confidence.as_ref().map(|c| c.score).unwrap_or(100));
                crate::usage::record(&self.shared.data_dir, "detection_answer", &[("answer", answer), ("reason", reason), ("score", score)]);
            }
            if a == Answer::Yes {
                self.add_src = ("detection", Some(cand.app));
                if let Some(pid) = cand.profile_id.clone() {
                    // Profil (application / configuration) : ajout direct si tout se resout, sinon ouverture du profil dans « Ajouter un jeu ».
                    let p = self.profiles.iter().find(|p| p.id == pid).cloned();
                    if let Some(p) = p {
                        let mut vars = self.prof_real_vars.clone();
                        for v in &p.variables {
                            if let Some(first) = crate::profiles::var_candidates(v, &vars).into_iter().next() {
                                vars.values.insert(v.name.clone(), first);
                            }
                        }
                        match crate::profiles::to_game(&p, &vars, &[]) {
                            Ok(g) => self.begin_add(g, cand.path.clone(), ctx),
                            Err(_) => {
                                self.model.page = Page::Add;
                                self.open_profile(&p);
                            }
                        }
                    }
                    self.prop_key = None;
                    ctx.request_repaint();
                    return;
                }
                let chosen = PathBuf::from(self.prop_path.trim());
                // Emplacement trouve par la connaissance intelligente et accepte : file locale (anonymisee).
                self.accept_known(self.prop_prog.as_ref(), &self.prop_found, &chosen, false);
                let g = Game { name, path: chosen.clone(), exe: Some(cand.exe.clone()), save_is_file: chosen.is_file(), ..Default::default() };
                self.begin_add(g, cand.path.clone(), ctx);
            }
            self.prop_key = None;
            ctx.request_repaint();
        }
    }

    /// Reglages > Protection automatique, mode expert : portee de la detection et liste des programmes ignores. (L'interrupteur
    /// de detection, les animations et « Chercher sur mon PC » sont dans la carte du mode simple ; le « Mode expert » est le
    /// reglage global du bas de la page.)
    pub(super) fn detect_expert(&mut self, ui: &mut egui::Ui) {
        // Detection desactivee : tous les sous-reglages sont grises (valeurs conservees, rien n est reinitialise).
        let enabled = gamedetect::sub_options_interactive(self.model.settings.detect_enabled);
        ui.add_enabled_ui(enabled, |ui| {
            ui.label(RichText::new(self.t(K::DetScopeTitle)).strong());
            ui.horizontal_wrapped(|ui| {
                for (sc, k) in [(gamedetect::DetectScope::All, K::DetScopeAll), (gamedetect::DetectScope::Games, K::DetScopeGames), (gamedetect::DetectScope::Apps, K::DetScopeApps)] {
                    let r = ui.selectable_label(self.model.settings.detect_scope == sc, self.t(k));
                    if self.deco(ui, r, Some(K::TipDetScope)).clicked() && self.model.settings.detect_scope != sc {
                        self.model.settings.detect_scope = sc;
                        let _ = self.model.save_settings();
                        if let Ok(mut q) = self.shared.proposals.lock() {
                            q.retain_scope(sc);
                            let _ = q.save(&self.shared.data_dir);
                        }
                    }
                }
            });
            ui.label(RichText::new(self.t(K::DetIgnoredTitle)).strong());
            let ignored: Vec<(String, gamedetect::Decision)> = self.det_mem.ignored().into_iter().map(|(k, d)| (k.clone(), d.clone())).collect();
            if ignored.is_empty() {
                ui.label(RichText::new(self.t(K::DetIgnoredNone)).small().weak());
            }
            let dd = self.shared.data_dir.clone();
            for (key, d) in ignored {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} ({})", d.name, d.exe));
                    let r = ui.small_button(self.t(K::BtnDetAsk));
                    if self.deco(ui, r, Some(K::TipDetAsk)).clicked() {
                        let mut m = Memory::load(&dd);
                        m.decisions.remove(&key);
                        let _ = m.save(&dd);
                        self.det_mem = m;
                    }
                    let r = ui.small_button(self.t(K::BtnDetForget));
                    if self.deco(ui, r, Some(K::TipDetForget)).clicked() {
                        let mut m = Memory::load(&dd);
                        m.decisions.remove(&key);
                        let _ = m.save(&dd);
                        self.det_mem = m;
                    }
                });
            }
        });
    }
}

/// Libelle traduit d'un indice du score de detection.
fn reason_key(r: gamedetect::Reason) -> K {
    use gamedetect::Reason as R;
    match r {
        R::Library => K::RsLibrary,
        R::Manifest => K::RsManifest,
        R::Profile => K::RsProfile,
        R::ProfileInstalled => K::RsProfileInstalled,
        R::ProfileFolder => K::RsProfileFolder,
        R::Emulator => K::RsEmulator,
        R::EmulatorFolder => K::RsEmulatorFolder,
        R::DbDir => K::RsDbDir,
        R::DbDirPartial => K::RsDbDirPartial,
        R::DbUnique => K::RsDbUnique,
        R::DbFew => K::RsDbFew,
        R::Graphics => K::RsGraphics,
        R::LongLived => K::RsLongLived,
        R::Publisher => K::RsPublisher,
        R::ShortName => K::RsShortName,
        R::NoGraphics => K::RsNoGraphics,
    }
}

/// Carte qui s'envole (animation de sortie) : reponse donnee, instant de depart, nom et icone figes.
#[derive(Clone)]
pub(super) struct PropExit {
    pub answer: gamedetect::Answer,
    pub start_ms: u64,
    pub name: String,
    pub tex: Option<egui::TextureHandle>,
    pub key: String,
    pub reduce: bool,
    pub layers: usize,
}

impl App {
    /// Menu de la zone de notification <-> reglage : une seule source de verite (settings.json). Relit le reglage s'il a ete
    /// change hors de l'interface, puis met a jour la case et le libelle (langue comprise).
    pub(super) fn sync_tray_detect(&mut self) {
        if self.shared.settings_dirty.swap(false, Ordering::SeqCst) {
            if let Ok(s) = crate::settings::Settings::load_from(&self.shared.data_dir) {
                self.model.settings.detect_enabled = s.detect_enabled;
            }
        }
        let key = (self.model.settings.detect_enabled, self.model.settings.lang);
        if self.tray_detect_last == Some(key) {
            return;
        }
        // Toujours reimpose l'etat voulu (la case native a pu basculer toute seule au clic).
        if let Some(item) = &self.tray_detect {
            use super::tray_sys::DetectLike;
            item.set_detect(key.0, self.t(if key.0 { K::TrayDetectOn } else { K::TrayDetectOff }));
        }
        self.tray_detect_last = Some(key);
    }
}
