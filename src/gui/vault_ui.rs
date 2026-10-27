//! Coffre (2.0, lot 8) : ecran de choix (tuiles des dossiers de synchronisation trouves + « Autre dossier… »), creation du
//! sous-dossier `AutoGameSoftwareSync`, conseil du mode « a la demande » de Google Drive, elements deja presents venant d'un
//! autre appareil (reprise : lier + restaurer, logique existante), et carte « Coffre » des Reglages (etat, espace libre,
//! changer avec confirmation sans jamais supprimer l'ancien, ouvrir). Logique pure et testee : `crate::vaults`.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph, TileIcon};
use crate::vaults::{self as vl, Candidate, Existing, FromDevice, Kind};
use std::collections::HashSet;
use std::path::Path;

/// Actions demandees par l'ecran de choix (dialogue ou etape 2 du premier lancement).
#[derive(Default)]
pub struct VaultActs {
    pub choose: Option<(PathBuf, Option<Candidate>)>,
    pub other: bool,
    pub resume: Option<FromDevice>,
    pub close: bool,
    /// Etape 2 : « Garder ce coffre » / « C'est bon » (passe a l'etape suivante).
    pub keep: bool,
    /// Revenir a la liste des coffres (apres un choix).
    pub repick: bool,
    /// « C'est un nouveau PC ? Tout récupérer » : migration depuis cet appareil.
    pub migrate: Option<String>,
}

/// Resultat de la preparation d'un coffre (thread).
#[derive(Clone)]
pub struct Prepared {
    pub target: PathBuf,
    pub cand: Option<Candidate>,
    pub found: Vec<FromDevice>,
    pub others: Vec<String>,
    /// Le coffre ne contient aucun element.
    pub empty: bool,
}

struct CardInfo {
    path: PathBuf,
    kind: Kind,
    streaming: Option<bool>,
    free: Option<u64>,
    at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pick,
    Busy,
    Done,
}

pub struct VaultUi {
    pub open: bool,
    phase: Phase,
    detected: Arc<Mutex<Option<Vec<Candidate>>>>,
    /// Coffres deja utilises trouves dans les dossiers detectes (proposes en premier).
    existing: Arc<Mutex<Option<Vec<Existing>>>>,
    /// Choix d'un dossier qui contient deja un coffre que `target_for` ne reprendrait pas : on le dit avant tout.
    beside: Option<(Existing, PathBuf, Option<Candidate>)>,
    prepared: Arc<Mutex<Option<Result<Prepared, String>>>>,
    resumed: Arc<Mutex<Option<(String, vl::ResumeReport)>>>,
    /// Choix en attente de confirmation (un coffre existe deja) : dossier choisi et son type.
    confirm: Option<(PathBuf, Option<Candidate>)>,
    error: Option<String>,
    done: Option<Prepared>,
    /// Appareils dont la reprise est lancee ou faite.
    answered: HashSet<String>,
    card: Arc<Mutex<Option<CardInfo>>>,
    card_asked: Option<(PathBuf, Instant)>,
    pub tip_open: bool,
    /// Diagnostic des captures (`AGSS_UI_VAULT_DEMO=pick,choose:1,found,card`).
    pub demo: Vec<String>,
    demo_t: Option<f64>,
    demo_read: bool,
}

impl Default for VaultUi {
    fn default() -> Self {
        VaultUi {
            open: false,
            phase: Phase::Pick,
            detected: Arc::new(Mutex::new(None)),
            existing: Arc::new(Mutex::new(None)),
            beside: None,
            prepared: Arc::new(Mutex::new(None)),
            resumed: Arc::new(Mutex::new(None)),
            confirm: None,
            error: None,
            done: None,
            answered: HashSet::new(),
            card: Arc::new(Mutex::new(None)),
            card_asked: None,
            tip_open: false,
            demo: vec![],
            demo_t: None,
            demo_read: false,
        }
    }
}

/// Icone et fond de la tuile d'un type de coffre.
fn kind_icon(k: Kind) -> (Glyph, [u8; 3]) {
    match k {
        Kind::GoogleDrive => (Glyph::Drive, [0x1d, 0x4f, 0x3f]),
        Kind::OneDrive => (Glyph::Cloud, [0x1c, 0x3d, 0x66]),
        Kind::Dropbox => (Glyph::Box, [0x22, 0x35, 0x6a]),
        Kind::ICloud => (Glyph::Cloud, [0x2a, 0x4a, 0x70]),
        Kind::Mega => (Glyph::Cloud, [0x6a, 0x2a, 0x2a]),
        Kind::PCloud => (Glyph::Cloud, [0x1f, 0x5a, 0x5a]),
        Kind::Nextcloud => (Glyph::Cloud, [0x1f, 0x4a, 0x6e]),
        Kind::Syncthing => (Glyph::Folder, [0x2a, 0x4d, 0x6b]),
        Kind::Network => (Glyph::Nas, [0x3a, 0x3a, 0x3a]),
        Kind::Usb => (Glyph::Usb, [0x3a, 0x3a, 0x3a]),
        Kind::Other => (Glyph::Disk, [0x3a, 0x3a, 0x3a]),
    }
}

impl App {
    /// Nom du coffre d'apres son type (nom propre du service, sinon libelle traduit).
    fn kind_title(&self, k: Kind, path: &Path) -> String {
        if let Some(b) = k.brand() {
            return b.to_string();
        }
        match k {
            Kind::Network => self.t(K::VVaultNetwork).to_string(),
            Kind::Usb => self.t(K::VVaultUsb).to_string(),
            _ => path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| path.display().to_string()),
        }
    }

    pub(super) fn vault_open(&mut self, ctx: &egui::Context) {
        self.vault_start_detect(ctx);
        self.vault.open = true;
    }

    /// Remet l'ecran de choix a zero et lance la detection (dossiers de synchronisation PUIS coffres existants dedans).
    pub(super) fn vault_start_detect(&mut self, ctx: &egui::Context) {
        let v = &mut self.vault;
        v.phase = Phase::Pick;
        v.error = None;
        v.done = None;
        v.confirm = None;
        v.beside = None;
        v.answered.clear();
        v.tip_open = false;
        if let Ok(mut g) = v.detected.lock() {
            *g = None;
        }
        if let Ok(mut g) = v.existing.lock() {
            *g = None;
        }
        let (slot, ex, c) = (v.detected.clone(), v.existing.clone(), ctx.clone());
        let current = self.model.dest();
        std::thread::spawn(move || {
            let found = vl::detect_system();
            let existing = vl::find_existing(&found, current.as_deref());
            if let Ok(mut g) = ex.lock() {
                *g = Some(existing);
            }
            if let Ok(mut g) = slot.lock() {
                *g = Some(found);
            }
            c.request_repaint();
        });
        ctx.request_repaint();
    }

    /// La detection des coffres a-t-elle ete lancee (etape 2 du premier lancement) ?
    pub(super) fn vault_detect_started(&self) -> bool {
        self.vault.detected.lock().map(|g| g.is_some()).unwrap_or(false) || self.vault.phase != Phase::Pick
    }

    /// Choix d'un dossier : s'il contient deja un coffre qui ne serait pas repris, on le dit d'abord ; confirmation si un
    /// coffre est deja configure ; sinon preparation directe.
    fn vault_choose(&mut self, ctx: &egui::Context, chosen: PathBuf, cand: Option<Candidate>) {
        let existing = self.vault.existing.lock().ok().and_then(|g| g.clone()).unwrap_or_default();
        if let Some(e) = vl::existing_beside(&chosen, &existing) {
            self.vault.beside = Some((e.clone(), chosen, cand));
            return;
        }
        self.vault_choose_direct(ctx, chosen, cand);
    }

    fn vault_choose_direct(&mut self, ctx: &egui::Context, chosen: PathBuf, cand: Option<Candidate>) {
        let same = self.model.dest().map(|d| vl::target_for(&chosen) == d).unwrap_or(false);
        if self.model.dest().is_some() && !same {
            self.vault.confirm = Some((chosen, cand));
        } else {
            self.vault_prepare(ctx, chosen, cand);
        }
    }

    /// Cree le sous-dossier du coffre et lit ce qu'il contient deja (thread) ; le resultat est repris par `vault_overlay`.
    fn vault_prepare(&mut self, ctx: &egui::Context, chosen: PathBuf, cand: Option<Candidate>) {
        self.vault.phase = Phase::Busy;
        self.vault.error = None;
        if let Ok(mut g) = self.vault.prepared.lock() {
            *g = None;
        }
        let (slot, c) = (self.vault.prepared.clone(), ctx.clone());
        let (me, keys) = (self.model.ctx.machine.clone(), self.model.config.games.iter().map(|g| cloud::dest_key(g)).chain(self.model.config.games.iter().map(|g| cloud::game_key(&g.name))).collect::<HashSet<String>>());
        std::thread::spawn(move || {
            let r = vl::prepare(&chosen).map(|target| Prepared { found: vl::found_from_others(&target, &me, &keys), others: vl::other_devices(&target, &me), empty: cloud::list_dest_games(&target).is_empty(), target, cand });
            if let Ok(mut g) = slot.lock() {
                *g = Some(r);
            }
            c.request_repaint();
        });
    }

    /// Applique le coffre prepare : nouveau dossier partage, surveillance relancee. Rien de l'ancien n'est touche.
    fn vault_apply(&mut self, ctx: &egui::Context, p: Prepared) {
        if self.model.dest().as_deref() == Some(p.target.as_path()) {
            self.toasts.push(u::anim::now(ctx), self.t(K::VVaultSame), "", u::State::Safe, None);
        } else {
            match self.model.set_dest(Some(p.target.clone())) {
                Ok(()) => {
                    self.shared.restart_watch();
                    self.dest_text = p.target.display().to_string();
                    self.dest_size = None;
                    self.dest_ok = None;
                    self.dest_probe_at = Instant::now() - Duration::from_secs(60);
                    self.vault.card_asked = None;
                    self.load_dest_games(ctx);
                    self.pad_log(&format!("[Coffre] coffre choisi ({})", p.cand.as_ref().map(|c| c.kind.code()).unwrap_or("autre")));
                }
                Err(e) => {
                    self.vault.error = Some(e.to_string());
                    self.vault.phase = Phase::Pick;
                    return;
                }
            }
        }
        self.vault.done = Some(p);
        self.vault.phase = Phase::Done;
    }

    /// Reprise des elements d'un appareil (lier + restaurer), en arriere-plan.
    fn vault_resume(&mut self, ctx: &egui::Context, vault: PathBuf, from: FromDevice) {
        if !self.vault.answered.insert(from.machine.clone()) {
            return;
        }
        let (slot, c, dd) = (self.vault.resumed.clone(), ctx.clone(), self.shared.data_dir.clone());
        std::thread::spawn(move || {
            let cx = make_ctx(&dd);
            let kf = crate::hints::KnownFolders::real();
            let prefixes = if cfg!(windows) { vec![] } else { crate::proton::wine_prefixes_cached() };
            let rep = vl::resume(&cx, &vault, &from.items, &kf, &prefixes);
            let log = Logger::new(dd.join("logs").join("watch.log"), false);
            log.log(&format!("[Coffre] reprise des elements d'un autre appareil : {} retrouves, {} remis, {} a terminer, {} erreurs", rep.added.len(), rep.restored.len(), rep.to_finish.len(), rep.failed.len()));
            for (n, e) in &rep.failed {
                log.log(&format!("[{}] ERREUR coffre : reprise impossible : {}", n, e));
            }
            if let Ok(mut g) = slot.lock() {
                *g = Some((from.machine.clone(), rep));
            }
            c.request_repaint();
        });
    }

    /// Resultats des threads (preparation, reprise) : a appeler chaque image, dialogue ouvert ou non.
    fn vault_poll(&mut self, ctx: &egui::Context) {
        let prep = self.vault.prepared.lock().ok().and_then(|mut g| g.take());
        match prep {
            Some(Ok(p)) => self.vault_apply(ctx, p),
            Some(Err(e)) => {
                self.vault.error = Some(e);
                self.vault.phase = Phase::Pick;
            }
            None => {}
        }
        let resumed = self.vault.resumed.lock().ok().and_then(|mut g| g.take());
        if let Some((machine, rep)) = resumed {
            if let Some(d) = self.vault.done.as_mut() {
                d.found.retain(|f| f.machine != machine);
            }
            let detail = self.t(K::VVaultResumeReport).replace("{0}", &rep.added.len().to_string()).replace("{1}", &(rep.to_finish.len() + rep.failed.len()).to_string());
            let detail = if rep.to_finish.is_empty() { detail } else { format!("{} {}", detail, self.t(K::VVaultResumeFinish)) };
            let st = if rep.failed.is_empty() { u::State::Safe } else { u::State::Todo };
            self.toasts.push(u::anim::now(ctx), self.t(K::VDone), &detail, st, None);
            self.after_change(ctx);
        }
    }

    /// Une image de l'ecran de choix et des confirmations (a appeler chaque image).
    pub(super) fn vault_overlay(&mut self, ctx: &egui::Context) {
        self.vault_poll(ctx);
        self.vault_demo(ctx);
        if !self.vault.open {
            self.vault_confirm_dialog(ctx);
            return;
        }
        self.focus.set_modal_scope(true);
        let mut acts = VaultActs::default();
        let closed = u::custom_dialog(ctx, "vault_choose", 640.0, |ui| self.vault_contents(ui, ctx, false, &mut acts));
        self.vault_handle(ctx, acts, closed);
        // la confirmation s'affiche PAR-DESSUS l'ecran de choix
        self.vault_confirm_dialog(ctx);
    }

    /// Applique les actions de l'ecran de choix (dialogue ou etape 2 du premier lancement).
    pub(super) fn vault_handle(&mut self, ctx: &egui::Context, acts: VaultActs, closed: bool) {
        if acts.other {
            let start = dirs::home_dir().filter(|h| h.is_dir());
            let mut d = rfd::FileDialog::new();
            if let Some(h) = start {
                d = d.set_directory(h);
            }
            if let Some(p) = d.pick_folder() {
                self.vault_choose(ctx, p, None);
            }
        }
        if let Some((p, c)) = acts.choose {
            self.vault_choose(ctx, p, c);
        }
        if let Some(f) = acts.resume {
            if let Some(d) = self.vault.done.clone() {
                self.vault_resume(ctx, d.target, f);
            }
        }
        if let Some(m) = acts.migrate {
            self.migrate_open(ctx, Some(m));
            return;
        }
        if acts.repick {
            self.vault.phase = Phase::Pick;
            self.vault.done = None;
            self.vault.error = None;
        }
        if self.vault.open && (acts.close || (closed && self.vault.phase != Phase::Busy && self.vault.confirm.is_none() && self.vault.beside.is_none())) {
            self.vault.open = false;
        }
    }

    /// Diagnostic des captures (premier lancement) : coffre existant N, sinon dossier detecte N.
    pub(super) fn vault_demo_pick(&self, i: usize) -> Option<(PathBuf, Option<Candidate>)> {
        let existing = self.vault.existing.lock().ok().and_then(|g| g.clone())?;
        let detected = self.vault.detected.lock().ok().and_then(|g| g.clone())?;
        match existing.get(i) {
            Some(e) => Some((e.path.clone(), e.cand.clone())),
            None => detected.get(i).map(|c| (c.path.clone(), Some(c.clone()))),
        }
    }

    /// Le coffre vient-il d'etre choisi et prepare (etape 2 : ecran « Ton coffre est pret ») ?
    pub(super) fn vault_ready_shown(&self) -> bool {
        self.vault.phase == Phase::Done
    }

    /// Contenu de l'ecran de choix. `inline` : dessine dans l'etape 2 du premier lancement (titre et boutons du bas fournis par
    /// l'assistant). Les coffres EXISTANTS trouves dans les dossiers detectes viennent en premier.
    pub(super) fn vault_contents(&self, ui: &mut egui::Ui, ctx: &egui::Context, inline: bool, acts: &mut VaultActs) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let detected = self.vault.detected.lock().ok().and_then(|g| g.clone());
        let existing = self.vault.existing.lock().ok().and_then(|g| g.clone()).unwrap_or_default();
        let current = self.model.dest();
        match self.vault.phase {
            Phase::Pick | Phase::Busy => {
                if !inline {
                    let g = u::layout(ui, self.t(K::VVaultPickTitle), u::tokens::FS_H2, u::col(tk.text), ui.available_width(), 2);
                    let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
                    ui.painter().galley(r.min, g, u::col(tk.text));
                    ui.add_space(u::tokens::SP_XS);
                    ui.label(RichText::new(self.t(K::VVaultPickBody)).color(u::col(tk.weak)));
                    ui.add_space(u::tokens::SP_M);
                }
                if let Some(e) = &self.vault.error {
                    egui::Frame::new().fill(u::col(crate::theme::mix(tk.card2, tk.red, 0.16))).stroke(egui::Stroke::new(1.0_f32, u::col(tk.red))).corner_radius(u::tokens::R_SM).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(self.t(K::VVaultFailed)).color(u::col(tk.text)));
                        ui.label(RichText::new(e).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                    });
                    ui.add_space(u::tokens::SP_S);
                }
                if self.vault.phase == Phase::Busy {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                    });
                    return;
                }
                let Some(list) = detected else {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(RichText::new(self.t(K::VVaultSearching)).color(u::col(tk.weak)));
                    });
                    return;
                };
                let max_h = if inline { f32::INFINITY } else { (ctx.screen_rect().height() - 260.0).max(200.0) };
                egui::ScrollArea::vertical().id_salt(("vault_cards", inline)).max_height(max_h).auto_shrink([true, true]).show(ui, |ui| {
                    // Coffres deja utilises : en tete (« On a trouve ton coffre existant »).
                    for (i, e) in existing.iter().take(3).enumerate() {
                        let is_current = current.as_deref() == Some(e.path.as_path());
                        let kind = e.cand.as_ref().map(|c| c.kind).unwrap_or(Kind::Other);
                        let title = if is_current { self.t(K::VVaultCurrentTitle) } else { self.t(K::VVaultExistingTitle) };
                        let service = self.kind_title(kind, &e.path);
                        let mut detail = self.t(K::VVaultExistingBody).replace("{0}", &e.items.to_string()).replace("{1}", &if e.devices.is_empty() { "—".to_string() } else { e.devices.join(", ") });
                        if e.legacy {
                            detail = format!("{} \u{b7} {}", detail, self.t(K::VVaultExistingOld));
                        }
                        let button = if is_current { self.t(K::VVaultKeep) } else { self.t(K::VVaultUseExisting) };
                        if existing_card(ui, ("vault_existing", i), &tk, kind, title, &service, &e.path.display().to_string(), &detail, button, big) {
                            if is_current {
                                if inline {
                                    acts.keep = true;
                                } else {
                                    acts.close = true;
                                }
                            } else {
                                acts.choose = Some((e.path.clone(), e.cand.clone()));
                            }
                        }
                        ui.add_space(u::tokens::SP_S);
                    }
                    if !existing.is_empty() {
                        ui.add_space(u::tokens::SP_XS);
                        ui.label(RichText::new(self.t(K::VVaultOtherChoices)).strong().color(u::col(tk.weak)));
                        ui.add_space(u::tokens::SP_XS);
                    }
                    if list.is_empty() {
                        ui.label(RichText::new(self.t(K::VVaultNoneFound)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                        ui.add_space(u::tokens::SP_S);
                    }
                    let gap = u::tokens::SP_S;
                    let cols = if inline && ui.available_width() > 900.0 { 3 } else { 2 };
                    let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor().max(150.0);
                    let mut cards: Vec<Option<&Candidate>> = list.iter().map(Some).collect();
                    cards.push(None);
                    for row in cards.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for c in row {
                                match c {
                                    Some(c) => {
                                        let title = self.kind_title(c.kind, &c.path);
                                        let sub = c.path.display().to_string();
                                        let (gl, bg) = kind_icon(c.kind);
                                        let mut tags: Vec<(String, [u8; 3])> = vec![(self.t(K::VVaultDetected).to_string(), tk.ok)];
                                        if vl::needs_offline_tip(c) {
                                            tags.push((self.t(K::VVaultStreamTag).to_string(), tk.orange));
                                        }
                                        if c.work {
                                            tags.push((self.t(K::VVaultWork).to_string(), tk.weak));
                                        }
                                        if choice_card(ui, ("vault_card", c.path.display().to_string()), w, TileIcon::Glyph(gl, bg), &title, &sub, &tags, self.t(K::VVaultChoose), big).clicked() {
                                            acts.choose = Some((c.path.clone(), Some((*c).clone())));
                                        }
                                    }
                                    None => {
                                        if choice_card(ui, "vault_other", w, TileIcon::Glyph(Glyph::Folder, [0x3a, 0x3a, 0x3a]), self.t(K::VVaultOther), self.t(K::VVaultOtherHint), &[], self.t(K::VVaultChoose), big).clicked() {
                                            acts.other = true;
                                        }
                                    }
                                }
                            }
                        });
                        ui.add_space(gap);
                    }
                });
                ui.add_space(u::tokens::SP_XS);
                ui.label(RichText::new(self.t(K::VVaultNote)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                if !inline {
                    ui.add_space(u::tokens::SP_M);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if u::button_with(ui, Btn::Secondary, self.t(K::BtnCancel), None, big).clicked() {
                            acts.close = true;
                        }
                    });
                }
            }
            Phase::Done => {
                let Some(p) = &self.vault.done else { return };
                let (kind, streaming) = p.cand.as_ref().map(|c| (c.kind, c.streaming)).unwrap_or((Kind::Other, None));
                ui.horizontal(|ui| {
                    let (gl, bg) = kind_icon(kind);
                    let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 10.0, u::col(bg));
                    u::paint_glyph(ui.painter(), r.shrink(9.0), gl, u::col(crate::theme::text_on(bg)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(self.t(K::VVaultReady)).size(u::tokens::FS_H2).color(u::col(tk.text)));
                        ui.label(RichText::new(p.target.display().to_string()).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                    });
                });
                ui.add_space(u::tokens::SP_M);
                if streaming == Some(true) {
                    tip_card(ui, &tk, self.t(K::VVaultStreamTitle), self.t(K::VVaultStreamBody));
                    ui.add_space(u::tokens::SP_M);
                }
                if p.found.is_empty() {
                    if p.others.is_empty() {
                        if p.empty {
                            ui.label(RichText::new(self.t(K::VVaultEmpty)).color(u::col(tk.weak)));
                        }
                    } else {
                        ui.label(RichText::new(self.t(K::VVaultUsedBy).replace("{}", &p.others.join(", "))).color(u::col(tk.weak)));
                    }
                }
                for f in &p.found {
                    let n = f.items.len();
                    let ask = if n == 1 { self.t(K::VVaultFoundOne).replace("{1}", &f.machine) } else { self.t(K::VVaultFoundMany).replace("{0}", &n.to_string()).replace("{1}", &f.machine) };
                    let ask = crate::i18n::elide(self.model.settings.lang, &ask);
                    egui::Frame::new().fill(u::col(tk.card2)).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(ask).strong().color(u::col(tk.text)));
                        ui.label(RichText::new(self.t(K::VVaultFoundBody)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                        ui.add_space(u::tokens::SP_S);
                        if self.vault.answered.contains(&f.machine) {
                            ui.add(egui::Spinner::new().size(16.0));
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                if u::button_with(ui, Btn::Primary, self.t(K::VVaultFoundYes), None, big).clicked() {
                                    acts.resume = Some(f.clone());
                                }
                                // Nouveau PC (lot 12) : tout recuperer, element par element, avec verification.
                                if u::button_with(ui, Btn::Secondary, self.t(K::VMigWizardBtn), Some(Glyph::Pc), big).clicked() {
                                    acts.migrate = Some(f.machine.clone());
                                }
                            });
                        }
                    });
                    ui.add_space(u::tokens::SP_S);
                }
                ui.add_space(u::tokens::SP_S);
                if inline {
                    if u::button_with(ui, Btn::Ghost, self.t(K::VVaultPickAnother), Some(Glyph::ChevronLeft), big).clicked() {
                        acts.repick = true;
                    }
                } else {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if p.found.is_empty() { self.t(K::VVaultDone) } else { self.t(K::VVaultLater) };
                        if u::button_with(ui, if p.found.is_empty() { Btn::Primary } else { Btn::Secondary }, label, None, big).clicked() {
                            acts.close = true;
                        }
                    });
                }
            }
        }
    }

    /// Le dossier choisi contient deja un coffre que le choix ne reprendrait pas : le dire, proposer de le reprendre ; un
    /// coffre neuf n'est cree a cote que sur demande explicite.
    fn vault_beside_dialog(&mut self, ctx: &egui::Context) {
        let Some((e, chosen, cand)) = self.vault.beside.clone() else { return };
        self.focus.set_modal_scope(true);
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let mut act = 0;
        u::custom_dialog(ctx, "vault_beside", 560.0, |ui| {
            ui.label(RichText::new(self.t(K::VVaultHasExistingTitle)).size(u::tokens::FS_H2).color(u::col(tk.text)));
            ui.add_space(u::tokens::SP_S);
            ui.label(RichText::new(self.t(K::VVaultHasExistingBody).replace("{}", &e.path.display().to_string())).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_XS);
            let detail = self.t(K::VVaultExistingBody).replace("{0}", &e.items.to_string()).replace("{1}", &if e.devices.is_empty() { "—".to_string() } else { e.devices.join(", ") });
            ui.label(RichText::new(detail).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
            ui.add_space(u::tokens::SP_M);
            ui.horizontal_wrapped(|ui| {
                if u::button_with(ui, Btn::Primary, self.t(K::VVaultUseExisting), Some(Glyph::Check), big).clicked() {
                    act = 1;
                }
                if u::button_with(ui, Btn::Secondary, self.t(K::VVaultCreateNew), None, big).clicked() {
                    act = 2;
                }
                if u::button_with(ui, Btn::Ghost, self.t(K::BtnCancel), None, big).clicked() {
                    act = 3;
                }
            });
        });
        match act {
            1 => {
                self.vault.beside = None;
                self.pad_log("[Coffre] coffre existant repris (trouve dans le dossier choisi)");
                self.vault_choose_direct(ctx, e.path.clone(), e.cand.clone().or(cand));
            }
            2 => {
                self.vault.beside = None;
                self.pad_log("[Coffre] coffre neuf cree a cote d'un coffre existant (choix explicite)");
                self.vault_choose_direct(ctx, chosen, cand);
            }
            3 => self.vault.beside = None,
            _ => {}
        }
    }

    /// Confirmation du changement de coffre (un coffre existe deja) : l'ancien n'est jamais supprime.
    fn vault_confirm_dialog(&mut self, ctx: &egui::Context) {
        self.vault_beside_dialog(ctx);
        let Some((chosen, cand)) = self.vault.confirm.clone() else { return };
        self.focus.set_modal_scope(true);
        let body = self.t(K::VVaultChangeBody).replace("{}", &chosen.display().to_string());
        let spec = u::ConfirmSpec { title: self.t(K::VVaultChangeTitle), body: &body, confirm: self.t(K::VVaultChange), cancel: self.t(K::BtnCancel), danger: false };
        match u::confirm_dialog(ctx, "vault_change", &spec) {
            Some(u::DialogAnswer::Confirm) => {
                self.vault.confirm = None;
                self.vault_prepare(ctx, chosen, cand);
            }
            Some(u::DialogAnswer::Cancel) => self.vault.confirm = None,
            None => {}
        }
    }

    // ------------------------------------------------------------ carte « Coffre » des Reglages

    fn vault_card_info(&mut self, ctx: &egui::Context) -> Option<(Kind, Option<bool>, Option<u64>)> {
        let dest = self.model.dest()?;
        let stale = match &self.vault.card_asked {
            Some((p, at)) => p != &dest || at.elapsed() > Duration::from_secs(30),
            None => true,
        };
        if stale {
            self.vault.card_asked = Some((dest.clone(), Instant::now()));
            let (slot, c) = (self.vault.card.clone(), ctx.clone());
            std::thread::spawn(move || {
                let found = vl::detect_system();
                let (kind, streaming) = vl::kind_of(&dest, &found);
                let info = CardInfo { free: vl::free_space(&dest), path: dest, kind, streaming, at: Instant::now() };
                if let Ok(mut g) = slot.lock() {
                    *g = Some(info);
                }
                c.request_repaint();
            });
        }
        let g = self.vault.card.lock().ok()?;
        let i = g.as_ref().filter(|i| Some(&i.path) == self.model.dest().as_ref())?;
        let _ = i.at;
        Some((i.kind, i.streaming, i.free))
    }

    /// Contenu de la carte « Coffre » (mode simple) ; le champ du chemin reste, en mode expert, sous cette carte.
    pub(super) fn vault_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let dest = self.model.dest();
        let Some(d) = dest else {
            ui.label(RichText::new(self.t(K::VVaultNoneSet)).color(u::col(tk.weak)));
            ui.add_space(u::tokens::SP_S);
            if u::button_with(ui, Btn::Primary, self.t(K::VChooseVault), Some(Glyph::Folder), big).clicked() {
                self.vault_open(ctx);
            }
            return;
        };
        ui.add_space(u::tokens::SP_XS);
        let info = self.vault_card_info(ctx);
        let (kind, streaming, free) = info.unwrap_or((Kind::Other, None, None));
        ui.horizontal(|ui| {
            let (gl, bg) = kind_icon(kind);
            let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 10.0, u::col(bg));
            u::paint_glyph(ui.painter(), r.shrink(9.0), gl, u::col(crate::theme::text_on(bg)));
            ui.vertical(|ui| {
                ui.label(RichText::new(self.kind_title(kind, &d)).size(u::tokens::FS_H3).strong().color(u::col(tk.text)));
                ui.horizontal_wrapped(|ui| {
                    let (txt, c) = match self.dest_ok {
                        Some(false) => (self.t(K::DestUnreachable), tk.orange),
                        _ => (self.t(K::VVaultConnected), tk.ok),
                    };
                    ui.label(RichText::new(txt).color(u::col(c)).strong());
                    if let Some(f) = free {
                        ui.label(RichText::new(format!("\u{b7} {}", self.t(K::VVaultFree).replace("{}", &self.bytes(f)))).color(u::col(tk.weak)));
                    }
                    if streaming == Some(true) {
                        ui.label(RichText::new(format!("\u{b7} {}", self.t(K::VVaultStreamTag))).color(u::col(tk.orange)));
                    }
                });
            });
        });
        if self.dest_ok == Some(false) {
            ui.label(RichText::new(self.t(K::VVaultUnreachableDetail)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
        }
        ui.label(RichText::new(d.display().to_string()).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
        ui.add_space(u::tokens::SP_S);
        ui.horizontal_wrapped(|ui| {
            let reachable = self.dest_ok != Some(false);
            if ui.add_enabled_ui(reachable, |ui| u::button_with(ui, Btn::Secondary, self.t(K::VVaultOpen), Some(Glyph::Folder), big)).inner.clicked() {
                winutil::open_folder(&d);
            }
            if u::button_with(ui, Btn::Secondary, self.t(K::VVaultChange), None, big).clicked() {
                self.vault_open(ctx);
            }
            // Nouveau PC (lot 12) : tout recuperer depuis un autre appareil du coffre.
            if u::button_with(ui, Btn::Secondary, self.t(K::VMigSettingsBtn), Some(Glyph::Pc), big).on_hover_text(self.t(K::VMigEntryHint)).clicked() {
                self.migrate_open(ctx, None);
            }
            if streaming == Some(true) && u::button_with(ui, Btn::Ghost, self.t(K::VVaultSeeTip), None, big).clicked() {
                self.vault.tip_open = !self.vault.tip_open;
            }
        });
        if self.vault.tip_open && streaming == Some(true) {
            ui.add_space(u::tokens::SP_S);
            tip_card(ui, &tk, self.t(K::VVaultStreamTitle), self.t(K::VVaultStreamBody));
        }
    }

    // ------------------------------------------------------------ diagnostic des captures

    fn vault_demo(&mut self, ctx: &egui::Context) {
        if !self.vault.demo_read {
            self.vault.demo_read = true;
            let list = |name: &str| -> Vec<String> { std::env::var(name).ok().map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default() };
            self.vault.demo = list("AGSS_UI_VAULT_DEMO");
            self.picker.demo = list("AGSS_UI_PICKER_DEMO");
            for (tok, page) in [("settings", Page::Settings), ("add", Page::Add)] {
                if self.vault.demo.iter().any(|s| s == tok) {
                    self.model.page = page;
                    self.on_page_enter(page, ctx);
                }
            }
            self.vault.tip_open = self.vault.demo.iter().any(|s| s == "tip");
        }
        if self.vault.demo.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let t0 = *self.vault.demo_t.get_or_insert(now);
        let steps = self.vault.demo.clone();
        for s in steps {
            if s == "pick" && !self.vault.open && now - t0 < 1.0 {
                self.vault_open(ctx);
            }
            // choose:N = choisit la tuile N ; ask:N = idem mais avec la confirmation (un coffre existe deja)
            let parsed = s.strip_prefix("choose:").map(|n| (false, n)).or_else(|| s.strip_prefix("ask:").map(|n| (true, n)));
            if let Some((ask, i)) = parsed.and_then(|(a, n)| n.parse::<usize>().ok().map(|i| (a, i))) {
                let list = self.vault.detected.lock().ok().and_then(|g| g.clone());
                if let (Some(list), true, Phase::Pick) = (list, now - t0 > 1.2, self.vault.phase) {
                    if let Some(c) = list.get(i).cloned() {
                        self.vault.demo.retain(|x| !x.starts_with("choose:") && !x.starts_with("ask:"));
                        if ask {
                            self.vault_choose(ctx, c.path.clone(), Some(c));
                        } else {
                            self.vault.confirm = None;
                            self.vault_prepare(ctx, c.path.clone(), Some(c));
                        }
                    }
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(300));
    }
}

/// Carte-conseil (orange doux).
fn tip_card(ui: &mut egui::Ui, tk: &u::Tokens, title: &str, body: &str) {
    egui::Frame::new().fill(u::col(crate::theme::mix(tk.card2, tk.orange, 0.14))).stroke(egui::Stroke::new(1.0_f32, u::col(tk.orange))).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong().color(u::col(tk.text)));
        ui.add_space(u::tokens::SP_XS);
        ui.label(RichText::new(body).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
    });
}

/// Tuile de choix : icone, nom, ligne de detail, etiquettes colorees, « Choisir » en accent a droite ; toute la tuile est cliquable.
#[allow(clippy::too_many_arguments)]
fn choice_card(ui: &mut egui::Ui, id: impl std::hash::Hash, w: f32, icon: TileIcon, title: &str, sub: &str, tags: &[(String, [u8; 3])], action: &str, big: bool) -> egui::Response {
    let tk = u::tokens(ui.ctx());
    let k = if big { 1.2 } else { 1.0 };
    let pad = 12.0;
    let icon_s = 40.0 * k;
    let text_w = (w - 2.0 * pad - icon_s - u::tokens::SP_M).max(60.0);
    let fs = u::tokens::FS_SMALL * k;
    let line_h = u::layout_line(ui, "Ag", fs, u::col(tk.weak)).size().y;
    let g1 = u::layout(ui, title, u::tokens::FS_H3 * k, u::col(tk.text), text_w, 1);
    let g2 = u::layout(ui, sub, fs, u::col(tk.weak), text_w, 2);
    let tag_gs: Vec<_> = tags.iter().map(|(s, c)| (u::layout_line(ui, s, fs, u::col(*c)), *c)).collect();
    let sep = u::layout_line(ui, " \u{b7} ", fs, u::col(tk.weak));
    let act = u::layout_line(ui, action, fs, u::col(tk.accent_hover));
    // hauteur identique pour toutes les tuiles : nom, 2 lignes de detail, une ligne d'etiquettes, « Choisir »
    let h = (g1.size().y + 2.0 * line_h + 4.0 + line_h + 4.0 + act.size().y).max(icon_s) + 2.0 * pad;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let resp = ui.interact(rect, ui.id().with(id), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let m = u::anim::motion(ui.ctx());
        let hov = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), m.dur(u::tokens::DUR_FAST) as f32);
        let p = ui.painter();
        p.rect(rect, u::tokens::R_LG, u::col(crate::theme::mix(tk.card2, tk.widget, hov * 0.5)), egui::Stroke::new(1.0_f32, u::col(crate::theme::mix(tk.line, tk.accent, hov))), egui::StrokeKind::Inside);
        let ir = egui::Rect::from_min_size(rect.min + egui::vec2(pad, pad), egui::vec2(icon_s, icon_s));
        if let TileIcon::Glyph(gl, bg) = icon {
            p.rect_filled(ir, 10.0, u::col(bg));
            u::paint_glyph(p, ir.shrink(icon_s * 0.22), gl, u::col(crate::theme::text_on(bg)));
        }
        let x = ir.right() + u::tokens::SP_M;
        let mut y = rect.top() + pad;
        let (g1h, g2h) = (g1.size().y, g2.size().y);
        p.galley(egui::pos2(x, y), g1, u::col(tk.text));
        y += g1h;
        p.galley(egui::pos2(x, y), g2, u::col(tk.weak));
        y += g2h + 4.0;
        let n = tag_gs.len();
        let mut tx = x;
        for (i, (g, _c)) in tag_gs.into_iter().enumerate() {
            let gw = g.size().x;
            p.galley(egui::pos2(tx, y), g, u::col(tk.weak));
            tx += gw;
            if i + 1 < n {
                p.galley(egui::pos2(tx, y), sep.clone(), u::col(tk.weak));
                tx += sep.size().x;
            }
        }
        p.galley(egui::pos2(x, rect.bottom() - pad - act.size().y), act, u::col(tk.accent_hover));
        if resp.has_focus() {
            u::focus_ring(ui, rect, u::tokens::R_LG);
        }
    }
    resp
}

impl VaultUi {
    /// Une confirmation de changement de coffre est-elle ouverte ?
    pub fn confirm_open(&self) -> bool {
        self.confirm.is_some() || self.beside.is_some()
    }
}

/// Carte d'un coffre existant (bordure d'accent) : icone du service, titre, service et chemin, detail, bouton principal.
#[allow(clippy::too_many_arguments)]
fn existing_card(ui: &mut egui::Ui, id: impl std::hash::Hash, tk: &u::Tokens, kind: Kind, title: &str, service: &str, path: &str, detail: &str, button: &str, big: bool) -> bool {
    let mut clicked = false;
    let _ = id;
    egui::Frame::new().fill(u::col(crate::theme::mix(tk.card2, tk.accent, 0.10))).stroke(egui::Stroke::new(1.5_f32, u::col(tk.accent))).corner_radius(u::tokens::R_LG).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Bouton d'abord (a droite), textes dans la place restante : un long chemin est raccourci (« … ») au lieu de
        // passer sous le bouton ; chemin complet en infobulle.
        ui.horizontal(|ui| {
            let (gl, bg) = kind_icon(kind);
            let s = if big { 48.0 } else { 40.0 };
            let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
            ui.painter().rect_filled(r, 10.0, u::col(bg));
            u::paint_glyph(ui.painter(), r.shrink(s * 0.22), gl, u::col(crate::theme::text_on(bg)));
            ui.add_space(u::tokens::SP_S);
            // place du bouton reservee a droite
            let btn = ui.painter().layout_no_wrap(button.to_string(), egui::TextStyle::Button.resolve(ui.style()), egui::Color32::WHITE).size().x;
            let text_w = (ui.available_width() - btn * if big { 1.35 } else { 1.15 } - 64.0).max(120.0);
            ui.allocate_ui_with_layout(egui::vec2(text_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_max_width(text_w);
                ui.add(egui::Label::new(RichText::new(title).size(if big { u::tokens::FS_H2 } else { u::tokens::FS_H3 }).strong().color(u::col(tk.text))).truncate());
                ui.add(egui::Label::new(RichText::new(format!("{} \u{b7} {}", service, path)).size(u::tokens::FS_SMALL).color(u::col(tk.weak))).truncate()).on_hover_text(path);
                ui.add(egui::Label::new(RichText::new(detail).size(u::tokens::FS_SMALL).color(u::col(tk.faint))).wrap());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if u::button_with(ui, Btn::Primary, button, Some(Glyph::Check), big).clicked() {
                    clicked = true;
                }
            });
        });
    });
    clicked
}
