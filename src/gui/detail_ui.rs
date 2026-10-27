//! Reglages techniques d'un element (mode expert de la fiche 2.0) : TOUTES les fonctions de l'ancienne fiche « Plus »
//! y restent (sauvegarder / restaurer, emplacements bruts, filtre, emplacements supplementaires, delais, conflits,
//! nettoyage, fusion avec un element du coffre), plus l'apercu de restauration et l'enregistrement des options.
use super::*;

impl App {
    /// Bloc « Reglages avances » (mode expert) de la fiche ; les actions demandees passent par une confirmation.
    pub(super) fn expert_block(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        let mut action: Option<Confirm> = None;
        let status = self.model.statuses.get(name).cloned().unwrap_or(GameStatus::Never);
        let running = status.blocks_actions();
        let free = !running && self.model.dest().is_some();
        // Sauvegarder / Restaurer (vocabulaire technique d'origine).
        ui.horizontal_wrapped(|ui| {
            let r = ui.add_enabled(free, egui::Button::new(self.t(K::BtnBackup)));
            if self.deco(ui, r, Some(K::TipBackup)).clicked() {
                action = Some(Confirm::Backup { game: name.to_string() });
            }
            let r = ui.add_enabled(free, egui::Button::new(self.t(K::BtnRestore)));
            if self.deco(ui, r, Some(K::TipRestore)).clicked() {
                action = Some(Confirm::Restore { game: name.to_string(), version: None });
            }
        });
        ui.add_space(6.0);
        // Delais et conflits.
        ui.horizontal_wrapped(|ui| {
            ui.label(self.t(K::SetStable));
            let r = ui.add(egui::DragValue::new(&mut self.edit_stable).range(1..=600));
            self.deco_kind(ui, r, Some(K::TipStable), ItemKind::Drag);
            ui.label(self.t(K::SetResync));
            let r = ui.add(egui::DragValue::new(&mut self.edit_resync).range(10..=86400));
            self.deco_kind(ui, r, Some(K::TipResync), ItemKind::Drag);
        });
        self.game_conflict_row(ui, name);
        ui.separator();
        self.locations_editor(ui, name, running, &mut action);
        ui.separator();
        // Nettoyage et fusion avec un element du coffre.
        ui.horizontal_wrapped(|ui| {
            let r = ui.add_enabled(self.model.dest().is_some(), egui::Button::new(self.t(K::BtnPrune)));
            if self.deco(ui, r, Some(K::TipPrune)).clicked() {
                if let (Some(d), Some(g)) = (self.model.dest(), self.model.game(name).cloned()) {
                    let n = actions::prune_preview(&self.model.ctx, &g, &d).len();
                    if n == 0 {
                        self.toasts.push(ui::anim::now(ctx), self.t(K::PruneNothing), "", ui::State::Safe, None);
                    } else {
                        action = Some(Confirm::Prune { game: name.to_string(), count: n });
                    }
                }
            }
            let r = ui.add_enabled(self.model.dest().is_some(), egui::Button::new(self.t(K::BtnLink)).selected(self.link_open));
            if self.deco(ui, r, Some(K::TipLink)).clicked() {
                self.link_open = !self.link_open;
                if self.link_open {
                    self.load_dest_games(ctx);
                }
            }
        });
        if self.link_open {
            ui.label(self.t(K::LinkPick));
            let own = cloud::game_key(name);
            for dg in self.dest_games.clone() {
                if dg.key == own {
                    continue;
                }
                let b = ui.button(format!("{}  ({})", dg.name, dg.key));
                if self.deco(ui, b, Some(K::TipLinkPick)).clicked() {
                    action = Some(Confirm::Link { game: name.to_string(), key: dg.key.clone(), name: dg.name.clone() });
                }
            }
            if self.dest_games.iter().all(|d| d.key == own) {
                ui.label(RichText::new(self.t(K::AddExistingNone)).weak());
            }
        }
        if let Some(a) = action {
            self.request_with_preview(a, ctx);
        }
    }

    /// Emplacements bruts (ancienne fiche) : principal (chemin, dossier / fichier, filtre, nom, actif), emplacements de
    /// la sauvegarde inconnus sur ce PC, emplacements supplementaires ; « Enregistrer les emplacements » avec confirmation.
    fn locations_editor(&mut self, ui: &mut egui::Ui, name: &str, running: bool, action: &mut Option<Confirm>) {
        let Some(cur) = self.model.game(name).cloned() else { return };
        let ok_col = Color32::from_rgb(60, 180, 100);
        let bad_col = Color32::from_rgb(220, 80, 80);
        ui.label(RichText::new(self.t(K::LocSave)).strong());
        ui.horizontal_wrapped(|ui| {
            for (is_file, k) in [(false, K::TargetFolder), (true, K::TargetFile)] {
                let r = ui.selectable_label(self.edit_save_file == is_file, self.t(k));
                if self.deco(ui, r, Some(K::TipTargetKind)).clicked() {
                    self.edit_save_file = is_file;
                }
            }
        });
        let mut save_txt = std::mem::take(&mut self.edit_save);
        let pkey = format!("detail_save:{}", name);
        if let Some(p) = self.take_picked(&pkey) {
            save_txt = p.path.display().to_string();
            self.edit_save_file = p.is_file;
        }
        if self.wide_field(ui, &mut save_txt, K::TipLocSave, true) {
            let opts = self.pick_opts_for_game(name, self.edit_save_file);
            let c = ui.ctx().clone();
            self.open_picker(&c, &pkey, opts);
        }
        self.edit_save = save_txt;
        let save_new = self.edit_save.trim().to_string();
        let target_new = crate::actions::SaveTarget { path: PathBuf::from(&save_new), is_file: self.edit_save_file, filter: if self.edit_save_file || self.edit_filter.trim().is_empty() { None } else { Some(self.edit_filter.trim().to_string()) } };
        let save_ok = target_new.exists();
        if save_ok {
            icon_label(ui, Icon::Check, self.t(K::LocOk), ok_col);
        } else {
            icon_label(ui, Icon::Cross, self.t(K::LocMissing), bad_col);
        }
        if !self.edit_save_file {
            egui::CollapsingHeader::new(self.t(K::FilterLabel)).id_salt("loc_filter").default_open(!self.edit_filter.trim().is_empty()).show(ui, |ui| {
                let mut f = std::mem::take(&mut self.edit_filter);
                let r = ui.add(egui::TextEdit::singleline(&mut f).hint_text(self.t(K::FilterHint)).desired_width(f32::INFINITY));
                self.deco_kind(ui, r, Some(K::TipFilter), ItemKind::Text);
                self.edit_filter = f;
            });
        }
        ui.add_space(4.0);
        ui.label(RichText::new(self.t(K::LocListTitle)).strong());
        {
            let mut en = self.edit_primary_enabled;
            ui.horizontal_wrapped(|ui| {
                if self.toggle(ui, &mut en, self.t(K::LocPrimary), K::TipLocEnabled).changed() {
                    self.edit_primary_enabled = en;
                }
                let mut pl = std::mem::take(&mut self.edit_primary_label);
                let r = ui.add(egui::TextEdit::singleline(&mut pl).hint_text(self.t(K::LocLabel)).desired_width(150.0));
                self.deco_kind(ui, r, Some(K::TipLocLabel), ItemKind::Text);
                self.edit_primary_label = pl;
            });
        }
        if let Some(unknown) = self.history_unknown.clone() {
            for (uid, ulabel) in unknown {
                if self.edit_locs.iter().any(|l| l.id == uid) {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::LocUnknownWarn).replace("{}", &ulabel));
                    let b = ui.button(self.t(K::BtnLocConfigure));
                    if self.deco(ui, b, Some(K::TipLocConfigure)).clicked() {
                        self.edit_locs.push(crate::config::Location { id: uid.clone(), label: ulabel.clone(), path: PathBuf::new(), is_file: false, filter: None, exclude: None, enabled: true });
                    }
                });
            }
        }
        let mut locs = std::mem::take(&mut self.edit_locs);
        let mut to_remove: Option<usize> = None;
        for (i, l) in locs.iter_mut().enumerate() {
            if self.loc_editor(ui, l, &format!("{}_{}", name, l.id)) {
                to_remove = Some(i);
            }
        }
        self.edit_locs = locs;
        if let Some(i) = to_remove {
            let l = self.edit_locs[i].clone();
            if cur.extra_locations.iter().any(|x| x.id == l.id) {
                *action = Some(Confirm::RemoveLocation { game: name.to_string(), id: l.id.clone(), label: l.label.clone() });
            } else {
                self.edit_locs.remove(i); // jamais enregistre : retrait immediat, rien a confirmer
            }
        }
        let b = ui.button(self.t(K::LocAdd));
        if self.deco(ui, b, Some(K::TipLocAdd)).clicked() {
            let n = self.edit_locs.len() + 2;
            let l = self.new_location(n);
            self.edit_locs.push(l);
        }
        let cur_filter = cur.save_filter.clone().unwrap_or_default();
        let save_changed = save_ok && (std::path::Path::new(&save_new) != cur.path.as_path() || self.edit_save_file != cur.save_is_file || (!self.edit_save_file && self.edit_filter.trim() != cur_filter.trim()));
        let locs_changed = self.edit_locs != cur.extra_locations || self.edit_primary_enabled == cur.primary_disabled || self.edit_primary_label.trim() != cur.primary_label.clone().unwrap_or_default().trim();
        let locs_valid = self.edit_locs.iter().all(|l| !l.path.as_os_str().is_empty());
        let can_save = !running && locs_valid && (save_changed || locs_changed);
        let r = ui.add_enabled(can_save, egui::Button::new(self.t(K::BtnLocSave)));
        if self.deco(ui, r, Some(K::TipLocSaveBtn)).clicked() {
            let mut cand = cur.clone();
            if save_changed {
                cand.path = target_new.path.clone();
                cand.save_is_file = target_new.is_file;
                cand.save_filter = target_new.filter.clone();
            }
            cand.extra_locations = self.edit_locs.clone();
            cand.primary_disabled = !self.edit_primary_enabled;
            let mut o = actions::overlapping_games(&self.model.config, &cand);
            if actions::own_locations_overlap(&cand) {
                o.push(cand.shown().to_string());
            }
            let overlap = if o.is_empty() { None } else { Some(o.join(", ")) };
            *action = Some(Confirm::Relocate {
                game: name.to_string(),
                exe: None,
                save: if save_changed { Some(target_new.clone()) } else { None },
                save_changed,
                overlap,
                locs: if locs_changed { Some(actions::LocEdit { primary_label: Some(self.edit_primary_label.clone()), primary_enabled: self.edit_primary_enabled, extras: self.edit_locs.clone() }) } else { None },
            });
        }
    }

    /// « Enregistrer » des Options (et des delais du mode expert) : programme associe, nom affiche, planification,
    /// versions a garder, notifications, delai de stabilite, reverification. Aucun fichier n'est touche.
    pub(super) fn save_item_options(&mut self, ctx: &egui::Context, name: &str) {
        let Some(cur) = self.model.game(name).cloned() else { return };
        let now = ui::anim::now(ctx);
        let mut problems: Vec<String> = Vec::new();
        // Programme associe (chemin verifie ; vide = inchange).
        let exe_new = self.edit_exe.trim().to_string();
        let cur_exe = self.model.settings.per_game.get(name).and_then(|p| p.exe_path.clone()).map(|p| p.display().to_string());
        if !exe_new.is_empty() && cur_exe.as_deref() != Some(exe_new.as_str()) {
            if std::path::Path::new(&exe_new).is_file() {
                if let Err(e) = actions::relocate_game(&self.model.ctx, name, Some(std::path::Path::new(&exe_new)), None) {
                    problems.push(e.to_string());
                }
                self.icons.remove(name);
            } else {
                problems.push(self.t(K::LocMissing).to_string());
            }
        }
        // Nom affiche.
        let disp = self.edit_display.trim().to_string();
        if !disp.is_empty() && disp != cur.shown() {
            if let Err(e) = actions::set_display_name(&self.model.ctx, name, Some(&disp)) {
                problems.push(e.to_string());
            }
        }
        let _ = self.model.reload();
        let kept = self.model.settings.set_keep(name, self.edit_keep);
        self.edit_keep = kept;
        // Une valeur egale au defaut de la reactivite n'est PAS figee : un changement de reactivite s'applique alors a cet element.
        let rp = crate::reactive::params(self.model.settings.reactivity, &self.model.settings.reactivity_custom);
        let g = self.model.settings.game_mut(name);
        g.stable_secs = if self.edit_stable == rp.stable_ms / 1000 { None } else { Some(self.edit_stable) };
        g.resync_secs = if self.edit_resync == rp.fallback_ms / 1000 { None } else { Some(self.edit_resync) };
        g.notify = match self.edit_notify {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        };
        // Planification : sans programme elle doit etre valide ; sinon le reste est enregistre mais pas la planification.
        let no_exe = self.model.game(name).map(|g| g.exe.as_deref().map(|e| e.trim().is_empty()).unwrap_or(true)).unwrap_or(false) && self.edit_exe.trim().is_empty();
        let sched_new = self.edit_sched.clone().map(|s| s.normalized());
        let sched_ok = sched_new.as_ref().map(|s| s.is_valid()).unwrap_or(!no_exe);
        if !sched_ok {
            problems.push(self.t(K::SchedNeed).to_string());
        } else if sched_new != self.model.game(name).and_then(|g| g.schedule.clone()) {
            if let Err(e) = actions::set_schedule(&self.model.ctx, name, sched_new) {
                problems.push(e.to_string());
            }
            let _ = self.model.reload();
        }
        if let Err(e) = self.model.save_settings() {
            problems.push(e.to_string());
        }
        self.shared.restart_watch();
        self.last_status = Instant::now() - Duration::from_secs(60);
        crate::perf::log(&self.model.ctx, &format!("[{}] UI : options de la fiche enregistrees{}", name, if problems.is_empty() { String::new() } else { format!(" ({})", problems.join(" ; ")) }));
        if problems.is_empty() {
            self.toasts.push(now, self.t(K::VDone), "", ui::State::Safe, None);
        } else {
            self.toasts.push(now, self.t(K::VProblem), &problems.join("\n"), ui::State::Problem, None);
        }
    }

    /// Toute restauration MANUELLE (Restaurer, Restaurer cette version, resolution « prendre le cloud ») calcule d'abord,
    /// dans un thread, un apercu des fichiers ajoutes / remplaces / identiques avant d'ouvrir la confirmation.
    pub(super) fn request_with_preview(&mut self, a: Confirm, ctx: &egui::Context) {
        let (game, version) = match &a {
            Confirm::Restore { game, version } => (game.clone(), version.clone()),
            Confirm::Resolve { game, keep_mine: false } => (game.clone(), None),
            _ => {
                self.preview = None;
                self.model.request(a);
                return;
            }
        };
        let (Some(g), Some(dest)) = (self.model.game(&game).cloned(), self.model.dest()) else {
            self.preview = None;
            self.model.request(a);
            return;
        };
        let (tx, c, dd) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let p = crate::preview::for_game(&make_ctx(&dd), &g, &dest, version.as_deref());
            let _ = tx.send(Msg::Preview(a, p));
            c.request_repaint();
        });
    }
}

impl App {
    /// Apercu de restauration : totaux, mise en avant des fichiers locaux plus recents, liste defilante navigable a la manette.
    pub(super) fn preview_ui(&self, ui: &mut egui::Ui, p: &crate::preview::Preview) {
        let lang = self.model.settings.lang;
        ui.add_space(8.0);
        self.section_title(ui, self.t(K::PrevTitle));
        let sum = self
            .t(K::PrevSummary)
            .replace("{0}", &p.added.to_string())
            .replace("{1}", &self.bytes(p.added_bytes))
            .replace("{2}", &p.replaced.to_string())
            .replace("{3}", &self.bytes(p.replaced_bytes))
            .replace("{4}", &p.identical.to_string());
        if !p.cloud_machine.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(self.t(K::PrevFrom));
                self.machine_pill(ui, &p.cloud_machine);
            });
        }
        ui.label(sum);
        if p.local_newer > 0 {
            ui.colored_label(Color32::from_rgb(230, 170, 40), format!("[!] {}", self.t(K::PrevLocalNewer).replace("{}", &p.local_newer.to_string())));
        }
        ui.label(RichText::new(self.t(K::PrevLocalOnly).replace("{}", &p.local_only.len().to_string())).small().weak());
        self.preview_files(ui, p, lang, 220.0);
        ui.add_space(4.0);
    }

    /// Liste des fichiers d'un apercu ([+] ajoute, [~] remplace, [!] plus recent en local, [=] identique).
    pub(super) fn preview_files(&self, ui: &mut egui::Ui, p: &crate::preview::Preview, lang: Lang, max_h: f32) {
        use crate::preview::DiffKind;
        let cloud_date = crate::timefmt::format_now_tz(p.cloud_time_ms, lang);
        egui::ScrollArea::vertical().id_salt("preview").max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            for f in p.files.iter().take(200) {
                let (tag, col) = match f.kind {
                    DiffKind::Added => ("[+]", Color32::from_rgb(60, 180, 100)),
                    DiffKind::Replaced => ("[~]", rgb(self.palette.accent)),
                    DiffKind::LocalNewer => ("[!]", Color32::from_rgb(230, 170, 40)),
                    DiffKind::Identical => ("[=]", Color32::GRAY),
                };
                let local = match (f.local_size, f.local_mtime_ms) {
                    (Some(s), Some(m)) => format!("{} {} ({})", self.t(K::PrevLocal), self.bytes(s), crate::timefmt::format_now_tz(m, lang)),
                    _ => String::new(),
                };
                let line = format!("{} {}  |  {}  |  {} {} ({})", tag, f.path, local, self.t(K::PrevCloud), self.bytes(f.cloud_size), cloud_date);
                let r = ui.add(egui::Label::new(RichText::new(line).small().color(col)).sense(egui::Sense::click()).wrap());
                self.deco_kind(ui, r, None, ItemKind::Button);
            }
            if p.files.len() > 200 {
                ui.label(RichText::new(self.t(K::PrevMore).replace("{}", &(p.files.len() - 200).to_string())).small().weak());
            }
        });
    }
}
