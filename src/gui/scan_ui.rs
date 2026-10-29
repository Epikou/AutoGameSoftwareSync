//! Scan du PC (2.0, lot 9) : progression animee par etape (thread de fond, annulable), resultats groupes par score de
//! confiance puis par categorie, « Protéger la sélection (N) » = ajout en lot. Moteur pur et teste : `crate::scan`.
//! Ouvert depuis l'accueil vide, le bouton de l'accueil, la page Ajouter et les Reglages ; ecran de la page Ajouter.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph};
use std::path::Path;
use crate::scan::{self, AddReport, Cat, Hide, Item, Origin, Personal, Sign, Size, Skip, SysItem, Tier, What};
use crate::weight;
use std::collections::HashSet;

/// Cle du selecteur guide pour l'emplacement d'un resultat masque.
const PLACE_KEY: &str = "scan_place";

/// Etat de l'ecran entre deux images.
#[derive(Default)]
pub struct ScanUi {
    pub open: bool,
    /// Dessine dans l'etape 4 du premier lancement : ni en-tete ni retour, l'assistant porte le bouton « Protéger ».
    pub embedded: bool,
    handle: Option<scan::Handle>,
    /// Cases cochees (cle -> coche) ; valeur par defaut posee a la premiere apparition.
    checked: HashMap<String, bool>,
    touched: HashSet<String>,
    show_hidden: bool,
    show_seen: bool,
    /// Alerte poids (2.0.1) : versions choisies par element (cle) et elements dont l'utilisateur a choisi de tout garder.
    keep: HashMap<String, usize>,
    heavy_ok: HashSet<String>,
    /// Resultat masque pour lequel le selecteur guide est ouvert (cle).
    pick_for: Option<String>,
    /// Programmes dont l'emplacement sera appris a leur prochaine utilisation (cles de programme).
    learn_armed: HashSet<String>,
    epoch: u64,
    /// Ajout en lot en cours (thread) et son bilan.
    adding: Option<Arc<Mutex<Option<AddReport>>>>,
    report: Option<AddReport>,
    /// Evenement `scan_done` deja envoye pour ce scan.
    done_sent: bool,
    /// Un dernier scan existait au lancement (texte « seulement les nouveautes »).
    had_last: bool,
    /// Avancement affiche (jamais en arriere).
    shown_frac: [f32; 4],
    icons: HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// Diagnostic des captures : `AGSS_UI_SCAN_DEMO=open,hidden,seen,add,home,scroll:<px>` (jamais dans l'interface).
    demo: Vec<String>,
    demo_read: bool,
    demo_added: bool,
    demo_home: bool,
    scroll_to: Option<f32>,
    scroll_hidden: bool,
    /// Ouvert depuis la carte Playnite de l'accueil : la section Playnite est amenee a l'ecran.
    pub(super) scroll_playnite: bool,
}

impl ScanUi {
    fn running(&self) -> bool {
        self.handle.as_ref().map(|h| !h.view().finished).unwrap_or(false)
    }
}

impl Drop for ScanUi {
    fn drop(&mut self) {
        if let Some(h) = &self.handle {
            h.request_cancel();
        }
    }
}

/// Couleur de fond de l'icone d'une categorie (sans icone de programme).
fn cat_bg(c: Cat) -> [u8; 3] {
    match c {
        Cat::Games => [0x24, 0x38, 0x4f],
        Cat::Apps => [0x1f, 0x4f, 0x7a],
        Cat::Folders => [0x2f, 0x4a, 0x3b],
        Cat::System => [0x2b, 0x3d, 0x5c],
    }
}

fn cat_glyph(c: Cat) -> Glyph {
    match c {
        Cat::Games => Glyph::Pad,
        Cat::Apps => Glyph::Code,
        Cat::Folders => Glyph::Folder,
        Cat::System => Glyph::Window,
    }
}

impl App {
    /// Ouvre l'ecran du scan (page Ajouter) et lance un nouveau scan (sauf s'il y en a un en cours).
    pub(super) fn scan_open(&mut self, ctx: &egui::Context) {
        if self.model.page != Page::Add {
            self.model.page = Page::Add;
            self.on_page_enter(Page::Add, ctx);
        }
        self.scan.open = true;
        self.scan.report = None;
        if self.scan.running() {
            return;
        }
        self.scan_start(ctx);
    }

    fn scan_start(&mut self, ctx: &egui::Context) {
        self.scan_send_done(0);
        let dd = self.shared.data_dir.clone();
        let games = self.model.config.games.clone();
        let vault = self.model.dest();
        self.scan.had_last = dd.join(scan::LAST_FILE).is_file();
        let c = ctx.clone();
        let d2 = dd.clone();
        self.scan.handle = Some(scan::start_with(move || scan::sources::real_env(&d2, games, vault), Arc::new(move || c.request_repaint())));
        self.scan.checked.clear();
        self.scan.touched.clear();
        self.scan.keep.clear();
        self.scan.heavy_ok.clear();
        self.scan.pick_for = None;
        self.scan.learn_armed = crate::knowledge::store::LearnList::load(&self.shared.data_dir).items.into_iter().map(|i| i.exe).collect();
        self.scan.show_hidden = false;
        self.scan.show_seen = false;
        self.scan.shown_frac = [0.0; 4];
        self.scan.done_sent = false;
        self.scan.epoch += 1;
        crate::perf::log(&self.model.ctx, "UI : scan du PC lance");
    }

    /// Evenement d'usage `scan_done` (une fois par scan termine), avec le consentement seulement (`usage::record`).
    fn scan_send_done(&mut self, accepted: usize) {
        let Some(h) = &self.scan.handle else { return };
        let v = h.view();
        if self.scan.done_sent || !v.finished || v.cancelled {
            return;
        }
        self.scan.done_sent = true;
        let proposed = v.items.iter().filter(|i| i.selectable() && !i.seen).count();
        let props = scan::scan_done_props(v.elapsed_ms, proposed, accepted);
        crate::usage::record(&self.shared.data_dir, "scan_done", &props);
    }

    fn scan_close(&mut self) {
        if let Some(h) = &self.scan.handle {
            if !h.view().finished {
                h.request_cancel();
            }
        }
        self.scan_send_done(0);
        self.scan.open = false;
        self.scan.report = None;
    }

    // ------------------------------------------------------------ textes

    fn scan_name(&self, it: &Item) -> String {
        let k = match &it.origin {
            Origin::Personal(p) => Some(match p {
                Personal::Documents => K::VPersDocuments,
                Personal::Pictures => K::VPersPictures,
                Personal::Desktop => K::VPersDesktop,
                Personal::Music => K::VPersMusic,
                Personal::Videos => K::VPersVideos,
                Personal::Downloads => K::VPersDownloads,
            }),
            Origin::System(s) => Some(match s {
                SysItem::DesktopShortcuts => K::VSysDesktop,
                SysItem::StartMenu => K::VSysStartMenu,
                SysItem::Taskbar => K::VSysTaskbar,
                SysItem::Wallpaper => K::VSysWallpaper,
                SysItem::RegistryPrefs => K::VSysPrefs,
                SysItem::Pref(k) => super::winprefs_ui::pref_label_key(*k),
            }),
            _ => None,
        };
        k.map(|k| self.t(k).to_string()).unwrap_or_else(|| it.name.clone())
    }

    fn hide_text(&self, h: Hide) -> &'static str {
        self.t(match h {
            Hide::DevTool => K::VHideDev,
            Hide::System => K::VHideSystem,
            Hide::Launcher => K::VHideLauncher,
            Hide::Sensitive => K::VHideSensitive,
            Hide::Empty => K::VHideEmpty,
            Hide::NoData => K::VHideNoData,
            Hide::Weak => K::VHideWeak,
            Hide::InVault => K::VHideVault,
            Hide::Later => K::VHideLater,
        })
    }

    /// « Pourquoi ? » en mots simples, puis ce qui sera protege.
    fn scan_why(&self, it: &Item) -> String {
        if let Some(a) = &it.already {
            return self.t(K::VScanAlready).replace("{}", a);
        }
        if it.tier == Tier::Hidden {
            return self.hide_text(scan::hide_reason(it)).to_string();
        }
        let mut parts: Vec<String> = Vec::new();
        parts.push(match &it.origin {
            Origin::Launcher(s) => self.t(K::VWhyLauncher).replace("{}", s.label()),
            Origin::Playnite(s) if s.is_empty() => self.t(K::VWhyPlayniteManual).to_string(),
            Origin::Playnite(s) => self.t(K::VWhyPlaynite).replace("{}", s),
            Origin::Base => self.t(K::VWhyBase).to_string(),
            Origin::Profile => self.t(K::VWhyProfile).to_string(),
            Origin::Emulator => self.t(K::VWhyEmulator).to_string(),
            Origin::Installed => self.t(K::VWhyInstalled).to_string(),
            Origin::Personal(_) => self.t(K::VWhyPersonal).to_string(),
            Origin::System(_) => self.t(K::VWhySystem).to_string(),
        });
        let has = |s: Sign| it.signs.contains(&s);
        if has(Sign::UserPlace) {
            parts.push(self.t(K::VScanPlaced).into());
        } else if has(Sign::HomeBase) {
            parts.push(self.t(K::VWhyConfirmed).into());
        } else if has(Sign::LauncherCloud) {
            parts.push(self.t(K::VWhyFromLauncher).into());
        } else if has(Sign::Deduced) || has(Sign::DeducedStrong) {
            parts.push(self.t(K::VWhyByName).into());
        } else if has(Sign::BaseKnown) {
            parts.push(self.t(K::VWhyKnownPlaces).into());
        }
        if it.origin == Origin::Base && !has(Sign::BaseFolderName) {
            parts.push(self.t(K::VWhyUnsure).into());
        }
        if has(Sign::ProfileWarning) {
            parts.push(self.t(K::VWhyReview).into());
        }
        // Reglages Windows : precision (propre a l'appareil, accord administrateur, en partie, liste seulement).
        if let Origin::System(SysItem::Pref(k)) = &it.origin {
            let mut v: Vec<String> = vec![self.t(K::VPrefTitle).into()];
            if let Some(n) = super::winprefs_ui::pref_note_key(*k) {
                v.push(self.t(n).into());
            }
            return v.join(" · ");
        }
        let now = crate::util::now_ms();
        if it.newest_ms.map(|t| now.saturating_sub(t) > scan::RECENT_DAYS * 86_400_000).unwrap_or(false) {
            parts.push(self.t(K::VWhyOld).into());
        }
        let what = match it.what {
            What::Saves => self.t(K::VSavedGames),
            What::Settings => self.t(K::VAppSettings),
            What::SavesAndSettings => self.t(K::VWhatBoth),
            What::Folder => self.t(K::VWhatFolder),
            What::Shortcuts => self.t(K::VWhatShortcuts),
            What::File => self.t(K::VWhatFile),
        };
        parts.push(if it.locs.len() > 1 { format!("{} ({})", what, self.t(K::VScanPlaces).replace("{}", &it.locs.len().to_string())) } else { what.to_string() });
        parts.join(" · ")
    }

    fn size_text(&self, s: &Size) -> String {
        let b = self.bytes(s.bytes);
        if s.partial {
            self.t(K::VPickAtLeast).replace("{}", &b)
        } else {
            b
        }
    }

    // ------------------------------------------------------------ ecran

    /// Ecran du scan (dans la page Ajouter).
    pub(super) fn scan_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.scan_demo(ctx);
        // Emplacement choisi pour un resultat masque : il passe dans « Peut-etre » avec sa case.
        if let Some(p) = self.take_picked(PLACE_KEY) {
            self.scan_placed(ctx, p);
        }
        // Ajout en lot termine : etats et accueil relus, bilan affiche.
        if let Some(slot) = self.scan.adding.clone() {
            let done = slot.lock().ok().and_then(|mut g| g.take());
            if let Some(rep) = done {
                self.scan.adding = None;
                self.scan_send_done(rep.added.len());
                self.after_change(ctx);
                // Premiere copie tout de suite (sinon un element sans programme attendrait son heure planifiee). La file est
                // partagee entre les redemarrages de la surveillance : la demande survit a `after_change`.
                if let Ok(mut q) = self.shared.sync_requests.lock() {
                    q.extend(rep.added.iter().map(|n| Some(n.clone())));
                }
                self.home.epoch += 1;
                let n = rep.added.len();
                let title = if n == 1 { self.t(K::VScanDoneOne).to_string() } else { self.t(K::VScanDoneMany).replace("{}", &n.to_string()) };
                self.toasts.push(u::anim::now(ctx), &title, "", if n > 0 { u::State::Safe } else { u::State::Todo }, None);
                crate::perf::log(&self.model.ctx, &format!("UI : scan du PC, {} element(s) ajoute(s), {} non ajoute(s)", n, rep.skipped.len()));
                self.scan.report = Some(rep);
                self.scan.epoch += 1;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        let t = u::tokens(ctx);
        let big = self.big_now;
        let embedded = self.scan.embedded;
        if !embedded {
            let mut back = false;
            ui.horizontal(|ui| {
                let r = u::button_with(ui, Btn::Ghost, self.t(K::VNavAdd), Some(Glyph::ChevronLeft), big);
                if self.deco(ui, r, None).clicked() {
                    back = true;
                }
                ui.add_space(u::tokens::SP_S);
                ui.label(RichText::new(self.t(K::VScanTitle)).size(if big { u::tokens::FS_H2 * 1.25 } else { u::tokens::FS_H2 }).color(u::col(t.text)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(self.t(K::VScanLocal)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
                });
            });
            if self.model.confirm.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                back = true;
            }
            if back {
                self.scan_close();
                return;
            }
            ui.add_space(u::tokens::SP_M);
        }
        let Some(h) = self.scan.handle.clone() else {
            self.scan_start(ctx);
            return;
        };
        let v = h.view();
        self.scan_categories(ui, &v);
        ui.add_space(u::tokens::SP_S);
        if let Some(rep) = self.scan.report.clone() {
            self.scan_report(ui, ctx, &rep);
            return;
        }
        // Ligne d'etat : arreter / relancer / nouveautes.
        ui.horizontal_wrapped(|ui| {
            if !v.finished {
                ui.spinner();
                let r = u::button(ui, Btn::Secondary, self.t(K::VScanStop));
                if self.deco(ui, r, None).clicked() {
                    h.request_cancel();
                }
            } else {
                if v.cancelled {
                    ui.label(RichText::new(self.t(K::VScanCancelled)).color(u::col(t.weak)));
                } else if self.scan.had_last {
                    ui.label(RichText::new(self.t(K::VScanNewOnly)).color(u::col(t.weak)));
                }
                let r = u::button_with(ui, Btn::Ghost, self.t(K::VScanAgain), Some(Glyph::Eye), false);
                if self.deco(ui, r, None).clicked() {
                    self.scan_start(ctx);
                }
            }
        });
        if !v.finished {
            u::anim::need_ambient(ctx);
        }
        // Coches par defaut a la premiere apparition (dossier perso : une fois sa taille connue, regle des 500 Mo) ; tres gros
        // dossier decoche des que sa taille est connue.
        for it in &v.items {
            if !self.scan.checked.contains_key(&it.key) {
                if scan::check_waits_for_size(it) && !v.sizes.contains_key(&it.key) {
                    continue;
                }
                self.scan.checked.insert(it.key.clone(), it.default_checked());
            }
            if let Some(s) = v.sizes.get(&it.key) {
                if s.bytes >= scan::HUGE_BYTES && !self.scan.touched.contains(&it.key) {
                    self.scan.checked.insert(it.key.clone(), false);
                }
            }
        }
        // Resultats + barre du bas collante.
        let bar_h = if big { 70.0 } else { 58.0 };
        let avail = ui.available_height();
        let mut area = egui::ScrollArea::vertical().id_salt("scan_results").auto_shrink([false, false]).max_height((avail - bar_h).max(80.0));
        if let Some(y) = self.scan.scroll_to.take() {
            area = area.vertical_scroll_offset(y);
        }
        area.show(ui, |ui| {
            ui.set_max_width((ui.available_width() - 14.0).max(120.0));
            self.scan_results(ui, ctx, &v);
        });
        self.scan_bottom_bar(ui, ctx, &v, bar_h);
    }

    /// Les 4 etapes : nom, compteur (ou pourcentage), barre de progression animee.
    fn scan_categories(&mut self, ui: &mut egui::Ui, v: &scan::Progress) {
        let t = u::tokens(ui.ctx());
        let big = self.big_now;
        let w = ui.available_width();
        let cols = if w < 640.0 { 2 } else { 4 };
        let gap = u::tokens::SP_S + 2.0;
        let cw = ((w - gap * (cols as f32 - 1.0)) / cols as f32).max(80.0);
        let labels = [K::VScanCatGames, K::VScanCatApps, K::VScanCatFolders, K::VScanCatSystem];
        for row in 0..(4 / cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for c in 0..cols {
                    let i = row * cols + c;
                    let target = if v.finished && !v.cancelled { 1.0 } else { v.frac[i] };
                    self.scan.shown_frac[i] = self.scan.shown_frac[i].max(target);
                    let f = ui.ctx().animate_value_with_time(egui::Id::new(("scan_frac", self.scan.epoch, i)), self.scan.shown_frac[i], 0.25);
                    let h = if big { 64.0 } else { 52.0 };
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(cw, h), egui::Sense::hover());
                    let p = ui.painter();
                    p.rect(rect, u::tokens::R_MD, u::col(t.card), egui::Stroke::new(1.0_f32, u::col(t.line)), egui::StrokeKind::Inside);
                    let fs = if big { u::tokens::FS_BODY * 1.2 } else { u::tokens::FS_BODY - 1.0 };
                    let g = u::layout(ui, self.t(labels[i]), fs, u::col(t.text), cw - 70.0, 1);
                    p.galley(egui::pos2(rect.left() + 12.0, rect.top() + 10.0), g, u::col(t.text));
                    let done = v.frac[i] >= 1.0 || (v.finished && !v.cancelled);
                    let (txt, col) = if done {
                        (v.counts[i].to_string(), t.ok)
                    } else if v.step == i && !v.finished {
                        (format!("{} %", (f * 100.0).round() as u32), t.weak)
                    } else {
                        ("—".to_string(), t.faint)
                    };
                    let g2 = u::layout_line(ui, &txt, fs, u::col(col));
                    let mut x = rect.right() - 12.0;
                    if done {
                        let cr = egui::Rect::from_min_size(egui::pos2(x - 14.0, rect.top() + 10.0), egui::vec2(14.0, 14.0));
                        u::paint_glyph(p, cr, Glyph::Check, u::col(t.ok));
                        x -= 18.0;
                    }
                    p.galley(egui::pos2(x - g2.size().x, rect.top() + 9.0), g2, u::col(col));
                    let bar = egui::Rect::from_min_size(egui::pos2(rect.left() + 12.0, rect.bottom() - 14.0), egui::vec2(cw - 24.0, 5.0));
                    p.rect_filled(bar, 3.0, u::col(t.widget));
                    if f > 0.0 {
                        p.rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * f.clamp(0.0, 1.0), bar.height())), 3.0, u::col(if done { t.ok } else { t.accent }));
                    }
                }
            });
            ui.add_space(gap);
        }
    }

    fn group_header(&self, ui: &mut egui::Ui, title: &str, n: usize, hint: &str, faint: bool, open: Option<bool>) -> egui::Response {
        let t = u::tokens(ui.ctx());
        ui.add_space(u::tokens::SP_M);
        let r = ui
            .horizontal(|ui| {
                if let Some(o) = open {
                    let (gr, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    u::paint_glyph(ui.painter(), gr, if o { Glyph::ChevronDown } else { Glyph::ChevronRight }, u::col(t.weak));
                }
                ui.label(RichText::new(title).strong().size(u::tokens::FS_H3).color(u::col(if faint { t.weak } else { t.text })));
                let _ = u::chip(ui, &n.to_string(), None, false);
                ui.label(RichText::new(format!("— {}", hint)).size(u::tokens::FS_SMALL).color(u::col(if faint { t.faint } else { t.weak })));
            })
            .response;
        let r = ui.interact(r.rect, ui.id().with(("scan_grp", title)), if open.is_some() { egui::Sense::click() } else { egui::Sense::hover() });
        ui.add_space(u::tokens::SP_XS);
        if open.is_some() {
            r.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            r
        }
    }

    fn scan_results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, v: &scan::Progress) {
        let t = u::tokens(ctx);
        let fresh = |it: &&Item| !it.seen || it.already.is_some();
        // 2.0.3 : jeux de la bibliotheque Playnite pas encore proteges, dans leur propre section (en tete).
        let mut pn: Vec<&Item> = v.items.iter().filter(|i| i.is_playnite() && i.already.is_none()).collect();
        let other = |i: &&Item| !i.is_playnite();
        let mut rec: Vec<&Item> = v.items.iter().filter(other).filter(|i| i.tier == Tier::Recommended).filter(fresh).collect();
        let mut maybe: Vec<&Item> = v.items.iter().filter(other).filter(|i| i.tier == Tier::Maybe).filter(fresh).collect();
        let mut seen: Vec<&Item> = v.items.iter().filter(other).filter(|i| i.tier != Tier::Hidden && i.seen && i.already.is_none()).collect();
        let mut hidden: Vec<&Item> = v.items.iter().filter(other).filter(|i| i.tier == Tier::Hidden).collect();
        pn.sort_by(|a, b| (a.tier == Tier::Hidden, std::cmp::Reverse(a.score), a.name.to_lowercase()).cmp(&(b.tier == Tier::Hidden, std::cmp::Reverse(b.score), b.name.to_lowercase())));
        let order = |a: &&Item, b: &&Item| (a.cat, a.already.is_some(), std::cmp::Reverse(a.score), a.name.to_lowercase()).cmp(&(b.cat, b.already.is_some(), std::cmp::Reverse(b.score), b.name.to_lowercase()));
        for g in [&mut rec, &mut maybe, &mut seen, &mut hidden] {
            g.sort_by(order);
        }
        let mut idx = 0usize;
        if v.finished && pn.is_empty() && rec.iter().chain(maybe.iter()).all(|i| i.already.is_some()) && !v.cancelled {
            ui.add_space(u::tokens::SP_S);
            let nothing = v.items.iter().all(|i| i.tier == Tier::Hidden);
            ui.label(RichText::new(if nothing { self.t(K::VHideNoData) } else { self.t(K::VScanNothingNew) }).color(u::col(t.weak)));
        }
        if !pn.is_empty() {
            let r = self.group_header(ui, self.t(K::VPlayniteSection), pn.len(), self.t(K::VPlayniteHint), false, None);
            if std::mem::take(&mut self.scan.scroll_playnite) {
                r.scroll_to_me(Some(egui::Align::TOP));
            }
            self.scan_rows(ui, ctx, &pn, v, &mut idx, true);
        }
        if !rec.is_empty() {
            self.group_header(ui, self.t(K::VScanRecommended), rec.len(), self.t(K::VScanRecommendedHint), false, None);
            self.scan_rows(ui, ctx, &rec, v, &mut idx, true);
        }
        if !maybe.is_empty() {
            self.group_header(ui, self.t(K::VScanMaybe), maybe.len(), self.t(K::VScanMaybeHint), false, None);
            self.scan_rows(ui, ctx, &maybe, v, &mut idx, true);
        }
        if !v.finished {
            for k in 0..2 {
                skeleton_row(ui, k);
            }
        }
        if !seen.is_empty() {
            let open = self.scan.show_seen;
            if self.group_header(ui, self.t(K::VScanSeen), seen.len(), self.t(K::VScanSeenHint), true, Some(open)).clicked() {
                self.scan.show_seen = !open;
            }
            if open {
                self.scan_rows(ui, ctx, &seen, v, &mut idx, true);
            }
        }
        if !hidden.is_empty() {
            let open = self.scan.show_hidden;
            let r = self.group_header(ui, self.t(K::VScanHidden), hidden.len(), self.t(K::VScanHiddenHint), true, Some(open));
            if std::mem::take(&mut self.scan.scroll_hidden) {
                r.scroll_to_me(Some(egui::Align::TOP));
            }
            if r.clicked() {
                self.scan.show_hidden = !open;
            }
            if open {
                self.scan_rows(ui, ctx, &hidden, v, &mut idx, false);
            }
        }
        ui.add_space(u::tokens::SP_L);
    }

    /// Lignes d'un groupe, avec un sous-titre par categorie.
    fn scan_rows(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, items: &[&Item], v: &scan::Progress, idx: &mut usize, checkable: bool) {
        let t = u::tokens(ctx);
        let mut last: Option<Cat> = None;
        let labels = [K::VScanCatGames, K::VScanCatApps, K::VScanCatFolders, K::VScanCatSystem];
        for it in items {
            if last != Some(it.cat) {
                last = Some(it.cat);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    let (gr, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    u::paint_glyph(ui.painter(), gr, cat_glyph(it.cat), u::col(t.faint));
                    ui.label(RichText::new(self.t(labels[it.cat.index()])).size(u::tokens::FS_SMALL).color(u::col(t.faint)));
                });
            }
            let i = *idx;
            *idx += 1;
            let tex = it.exe_path.as_ref().and_then(|p| self.scan_icon(ctx, p));
            let checked = self.scan.checked.get(&it.key).copied().unwrap_or(false) && it.selectable();
            let size = v.sizes.get(&it.key).copied();
            let name = self.scan_name(it);
            let why = self.scan_why(it);
            let mut trailing: Vec<String> = Vec::new();
            match size {
                Some(s) if it.tier != Tier::Hidden => trailing.push(self.size_text(&s)),
                None if it.tier != Tier::Hidden && it.already.is_none() && !v.sizes_done => trailing.push(self.t(K::VScanMeasuring).to_string()),
                _ => {}
            }
            if let Some(ms) = it.newest_ms.filter(|_| it.tier != Tier::Hidden) {
                trailing.push(self.t(K::VScanModified).replace("{}", &self.ago_text(crate::util::now_ms(), ms)));
            }
            // Alerte poids (2.0.1) : au-dessus du seuil, taille + impact dans le coffre (versions choisies ou par defaut).
            let keep_now = self.scan.keep.get(&it.key).copied();
            let heavy = size.filter(|s| weight::is_heavy(s.bytes) && it.tier != Tier::Hidden && it.already.is_none());
            let warn = heavy.map(|s| {
                let keep = keep_now.unwrap_or_else(|| scan::keep_for(it, s.bytes));
                let mut line = format!("{} · {}", self.t(K::VHeavyPill), self.heavy_line(s.bytes, keep, s.partial));
                if s.bytes >= scan::HUGE_BYTES {
                    line = format!("{} · {}", line, self.t(K::VScanUnticked));
                }
                line
            });
            let spec = RowSpec { name: &name, why: &why, warn: warn.as_deref(), trailing: &trailing.join(" · "), checked, checkable: checkable && it.selectable(), score: it.score, tier: it.tier, cat: it.cat, tex: tex.as_ref(), dim: it.already.is_some() || !checkable };
            let r = u::cascade_in(ui, ("scan_row", &it.key), i.min(12), self.scan.epoch, |ui| scan_row(ui, ("scan_row", &it.key), &spec, self.big_now));
            // Infobulle : emplacements en clair ; mode expert : score et indices.
            let mut tip: Vec<String> = vec![name.clone(), why.clone()];
            for l in &it.locs {
                tip.push(format!("{}{}", l.path.display(), l.filter.as_deref().map(|f| format!("  [{}]", f)).unwrap_or_default()));
            }
            if self.model.settings.expert_mode {
                tip.push(format!("{} / 100 · {:?}", it.score, it.signs));
            }
            let r = self.deco(ui, r, None).on_hover_text(tip.join("\n"));
            if r.clicked() && spec.checkable {
                self.scan.checked.insert(it.key.clone(), !checked);
                self.scan.touched.insert(it.key.clone());
            }
            if heavy.is_some() && it.selectable() {
                self.scan_heavy_choices(ui, it);
            }
            if it.tier == Tier::Hidden && matches!(it.hide, Some(Hide::NoData)) && matches!(it.cat, Cat::Games | Cat::Apps) {
                self.scan_hidden_actions(ui, ctx, it);
            }
        }
    }

    /// Trois choix sous un resultat lourd : Garder / Garder moins de versions / Ne pas proteger.
    fn scan_heavy_choices(&mut self, ui: &mut egui::Ui, it: &Item) {
        let t = u::tokens(ui.ctx());
        let kept = self.scan.heavy_ok.contains(&it.key);
        let fewer = self.scan.keep.get(&it.key).copied();
        let off = self.scan.touched.contains(&it.key) && !self.scan.checked.get(&it.key).copied().unwrap_or(false);
        let mut pick: Option<weight::Choice> = None;
        ui.horizontal(|ui| {
            ui.add_space(52.0);
            ui.spacing_mut().item_spacing.x = u::tokens::SP_S;
            let r = u::chip(ui, self.t(K::VHeavyKeep), None, kept && fewer.is_none() && !off);
            if self.deco(ui, r, None).clicked() {
                pick = Some(weight::Choice::Keep);
            }
            let r = u::chip(ui, &self.t(K::VHeavyFewer).replace("{}", &weight::fewer_keep().to_string()), None, fewer.is_some() && !off);
            if self.deco(ui, r, None).on_hover_text(self.t(K::VHeavyFewerTip)).clicked() {
                pick = Some(weight::Choice::Fewer);
            }
            let r = u::chip(ui, self.t(K::VHeavyDontProtect), None, off);
            if self.deco(ui, r, None).clicked() {
                pick = Some(weight::Choice::Cancel);
            }
        });
        ui.add_space(u::tokens::SP_XS);
        let _ = t;
        match pick {
            Some(weight::Choice::Keep) => {
                self.scan.heavy_ok.insert(it.key.clone());
                self.scan.keep.remove(&it.key);
                self.scan.checked.insert(it.key.clone(), true);
                self.scan.touched.insert(it.key.clone());
            }
            Some(weight::Choice::Fewer) => {
                self.scan.keep.insert(it.key.clone(), weight::fewer_keep());
                self.scan.checked.insert(it.key.clone(), true);
                self.scan.touched.insert(it.key.clone());
            }
            Some(weight::Choice::Cancel) => {
                self.scan.checked.insert(it.key.clone(), false);
                self.scan.touched.insert(it.key.clone());
            }
            None => {}
        }
    }

    /// Resultat masque « rien a proteger trouve » : deux actions. Choisir l'emplacement (selecteur guide) ou l'apprendre a la
    /// prochaine utilisation (mode apprentissage). Les outils de developpement et programmes systeme n'ont aucune action.
    fn scan_hidden_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, it: &Item) {
        let t = u::tokens(ctx);
        let armed = it.exe.as_deref().map(|e| self.scan.learn_armed.contains(&crate::knowledge::Program::key_of(e))).unwrap_or(false);
        let mut place = false;
        let mut learn = false;
        ui.horizontal_wrapped(|ui| {
            ui.add_space(52.0);
            ui.spacing_mut().item_spacing.x = u::tokens::SP_S;
            let r = u::button_with(ui, Btn::Secondary, self.t(K::VScanPlace), Some(Glyph::Folder), false);
            if self.deco(ui, r, None).clicked() {
                place = true;
            }
            {
                if armed {
                    ui.label(RichText::new(self.t(K::VScanLearnArmed).replace("{}", &it.name)).size(u::tokens::FS_SMALL).color(u::col(t.ok)));
                } else {
                    let known = it.exe.is_some();
                    let r = ui.add_enabled_ui(known, |ui| u::button_with(ui, Btn::Ghost, self.t(K::VScanLearn), Some(Glyph::Eye), false)).inner;
                    let r = if known { self.deco(ui, r, Some(K::TipLearn)) } else { r.on_disabled_hover_text(self.t(K::VScanLearnNoExe)) };
                    if r.clicked() && known {
                        learn = true;
                    }
                }
            }
        });
        ui.add_space(u::tokens::SP_XS);
        if place {
            self.scan.pick_for = Some(it.key.clone());
            let prog = crate::knowledge::Program::describe(it.exe_path.as_deref(), it.exe.as_deref().unwrap_or(""), &it.name, it.app);
            let opts = crate::gui::picker_ui::PickOpts { name: Some(it.name.clone()), is_file: false, names: vec![it.name.clone()], found: vec![], program: Some(prog) };
            self.open_picker(ctx, PLACE_KEY, opts);
        }
        if learn {
            if let Some(exe) = it.exe.clone() {
                let dd = self.shared.data_dir.clone();
                let mut l = crate::knowledge::store::LearnList::load(&dd);
                l.add(&exe, &it.name, it.exe_path.clone(), crate::util::now_ms());
                let _ = l.save(&dd);
                self.scan.learn_armed.insert(crate::knowledge::Program::key_of(&exe));
                self.pad_log(&format!("[Connaissance] {} : emplacement a apprendre a la prochaine utilisation (depuis le scan)", exe));
            }
        }
    }

    /// Emplacement choisi avec le selecteur guide pour un resultat masque : la ligne passe dans « Peut-etre », cochee.
    fn scan_placed(&mut self, ctx: &egui::Context, p: crate::gui::picker_ui::Picked) {
        let Some(key) = self.scan.pick_for.take() else { return };
        let Some(h) = self.scan.handle.clone() else { return };
        let loc = scan::Loc { path: p.path.clone(), is_file: p.is_file, label: String::new(), filter: None, exclude: None };
        if scan::place_item(&h, &key, loc) {
            self.scan.checked.insert(key.clone(), true);
            self.scan.touched.insert(key.clone());
            let c = ctx.clone();
            let k2 = key.clone();
            std::thread::spawn(move || {
                let c2 = c.clone();
                scan::measure_one(&h, &k2, &move || c2.request_repaint());
                c.request_repaint();
            });
            self.pad_log(&format!("[Scan] emplacement choisi pour {} : {}", key, p.path.display()));
        } else {
            self.toasts.push(u::anim::now(ctx), self.t(K::VWarnSensitive), "", u::State::Todo, None);
        }
    }

    /// Icone d'un programme (lue une fois, au plus 3 nouvelles par image).
    fn scan_icon(&mut self, ctx: &egui::Context, p: &Path) -> Option<egui::TextureHandle> {
        if let Some(t) = self.scan.icons.get(p) {
            return t.clone();
        }
        // (jamais d'appel au contexte pendant `data_mut` : verrou repris = interface bloquee)
        let f = ctx.cumulative_pass_nr();
        let loaded_now = ctx.data_mut(|d| {
            let n = d.get_temp_mut_or_default::<(u64, u32)>(egui::Id::new("scan_icons_frame"));
            if n.0 != f {
                *n = (f, 0);
            }
            n.1 += 1;
            n.1
        });
        if loaded_now > 3 {
            ctx.request_repaint_after(Duration::from_millis(30));
            return None;
        }
        let tex = winutil::exe_icon_rgba(p).map(|(rgba, w, h)| ctx.load_texture(format!("scan-icon-{}", p.display()), egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), crate::gui::ui::ICON_TEX));
        self.scan.icons.insert(p.to_path_buf(), tex.clone());
        tex
    }

    /// Bas collant : resume de la selection et « Protéger la sélection (N) ».
    fn scan_bottom_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, v: &scan::Progress, h: f32) {
        let t = u::tokens(ctx);
        let sel: Vec<&Item> = v.items.iter().filter(|i| i.selectable() && self.scan.checked.get(&i.key).copied().unwrap_or(false)).collect();
        let bytes: u64 = sel.iter().filter_map(|i| v.sizes.get(&i.key)).map(|s| s.bytes).sum();
        let n = sel.len();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
        ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0_f32, u::col(t.line)));
        let mut go = false;
        let embedded = self.scan.embedded;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(4.0, 8.0))).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            let adding = self.scan.adding.is_some();
            let label = if adding { self.t(K::VScanAdding).to_string() } else { self.t(K::VScanProtect).replace("{}", &n.to_string()) };
            let enabled = n > 0 && v.finished && !adding;
            // Premier lancement : le bouton « Protéger » est celui de l'assistant (bas de page).
            if !embedded {
                let r = ui.add_enabled_ui(enabled, |ui| u::button_with(ui, Btn::Primary, &label, Some(Glyph::Shield), true)).inner;
                if self.deco(ui, r, None).clicked() && enabled {
                    go = true;
                }
                ui.add_space(u::tokens::SP_M);
            }
            let total = if v.sizes_done { self.bytes(bytes) } else { self.t(K::VScanMeasuring).to_string() };
            let s = if n == 1 { self.t(K::VScanSelectedOne).replace("{}", &total) } else { self.t(K::VScanSelectedMany).replace("{0}", &n.to_string()).replace("{1}", &total) };
            ui.label(RichText::new(s).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
        });
        if go {
            self.scan_protect(ctx, v);
        }
    }

    /// Ajout en lot (thread) des elements coches, dans l'ordre affiche.
    fn scan_protect(&mut self, ctx: &egui::Context, v: &scan::Progress) {
        let mut items: Vec<Item> = v.items.iter().filter(|i| i.selectable() && self.scan.checked.get(&i.key).copied().unwrap_or(false)).cloned().collect();
        items.sort_by_key(|i| (i.tier != Tier::Recommended, i.cat));
        if items.is_empty() {
            return;
        }
        let shown: HashMap<String, String> = items.iter().map(|i| (i.key.clone(), self.scan_name(i))).collect();
        let sizes = v.sizes.clone();
        let keep = self.scan.keep.clone();
        let slot: Arc<Mutex<Option<AddReport>>> = Arc::new(Mutex::new(None));
        self.scan.adding = Some(slot.clone());
        let (dd, c) = (self.shared.data_dir.clone(), ctx.clone());
        std::thread::spawn(move || {
            let cx = make_ctx(&dd);
            let rep = scan::add_items_with_keep(&cx, &items, &sizes, &|it| shown.get(&it.key).cloned(), "scan", &keep);
            if let Ok(mut s) = slot.lock() {
                *s = Some(rep);
            }
            c.request_repaint();
        });
    }

    /// Bilan de l'ajout : nombre d'elements proteges, non ajoutes et pourquoi, suite.
    fn scan_report(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, rep: &AddReport) {
        let t = u::tokens(ctx);
        let big = self.big_now;
        let n = rep.added.len();
        let mut act = 0;
        egui::ScrollArea::vertical().id_salt("scan_report").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width((ui.available_width() - 14.0).max(120.0));
            u::cascade_in(ui, "scan_report", 0, self.scan.epoch, |ui| {
                u::card(ui, None, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(u::tokens::SP_M);
                        let s = if big { 72.0 } else { 60.0 };
                        let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                        let age = u::anim::age_of(ctx, egui::Id::new("scan_done_shield"), self.scan.epoch);
                        u::status_banner::paint_shield(ui.painter(), r, u::State::Safe, (age / 0.6).min(1.0) as f32);
                        if age < 0.7 {
                            u::anim::need_transition(ctx);
                        }
                        ui.add_space(u::tokens::SP_S);
                        let title = if n == 1 { self.t(K::VScanDoneOne).to_string() } else { self.t(K::VScanDoneMany).replace("{}", &n.to_string()) };
                        ui.label(RichText::new(title).size(if big { u::tokens::FS_H2 * 1.25 } else { u::tokens::FS_H2 }).color(u::col(t.text)));
                        let body = if self.model.dest().is_some() { self.t(K::VScanDoneBody) } else { self.t(K::VScanDoneNoVault) };
                        ui.label(RichText::new(body).color(u::col(t.weak)));
                        ui.add_space(u::tokens::SP_M);
                    });
                    if !rep.added.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for a in &rep.added {
                                let shown = self.model.game(a).map(|g| g.shown().to_string()).unwrap_or_else(|| a.clone());
                                let _ = u::chip(ui, &shown, None, true);
                            }
                        });
                    }
                    if !rep.skipped.is_empty() {
                        ui.add_space(u::tokens::SP_M);
                        ui.label(RichText::new(self.t(K::VScanNotAdded)).strong().color(u::col(t.text)));
                        for (name, why) in &rep.skipped {
                            let s = match why {
                                Skip::Already(o) => self.t(K::VScanSkipAlready).replace("{0}", name).replace("{1}", o),
                                Skip::Overlap(o) => self.t(K::VScanSkipOverlap).replace("{0}", name).replace("{1}", o),
                                Skip::Missing => self.t(K::VScanSkipMissing).replace("{}", name),
                                Skip::Error(_) => self.t(K::VScanSkipError).replace("{}", name),
                            };
                            ui.label(RichText::new(format!("• {}", s)).size(u::tokens::FS_SMALL).color(u::col(t.weak)));
                        }
                    }
                    ui.add_space(u::tokens::SP_L);
                    // Premier lancement : la suite est le bouton « Continuer » de l'assistant.
                    if !self.scan.embedded { ui.horizontal(|ui| {
                        let r = u::button_with(ui, Btn::Primary, self.t(K::VScanSeeHome), Some(Glyph::Home), true);
                        if self.deco(ui, r, None).clicked() {
                            act = 1;
                        }
                        let r = u::button(ui, Btn::Secondary, self.t(K::VScanBackResults));
                        if self.deco(ui, r, None).clicked() {
                            act = 2;
                        }
                    }); }
                });
            });
        });
        match act {
            1 => {
                self.scan.open = false;
                self.scan.report = None;
                self.model.page = Page::Games;
                self.on_page_enter(Page::Games, ctx);
                self.home.epoch += 1;
            }
            2 => {
                self.scan.report = None;
                // Le scan suivant marque deja ces elements « deja proteges » : on relance pour un etat juste.
                self.scan_start(ctx);
            }
            _ => {}
        }
    }

    /// Diagnostic des captures (`AGSS_UI_SCAN_DEMO`).
    fn scan_demo(&mut self, ctx: &egui::Context) {
        if self.scan.demo.is_empty() {
            return;
        }
        let Some(h) = self.scan.handle.clone() else { return };
        let v = h.view();
        let demo = self.scan.demo.clone();
        let has = |k: &str| demo.iter().any(|d| d == k);
        if v.finished && v.sizes_done {
            if has("hidden") && !self.scan.show_hidden {
                self.scan.show_hidden = true;
                self.scan.scroll_hidden = true;
            }
            if has("seen") {
                self.scan.show_seen = true;
            }
            if let Some(px) = self.scan.demo.iter().find_map(|d| d.strip_prefix("scroll:").and_then(|x| x.parse::<f32>().ok())) {
                if !self.scan.demo_home {
                    self.scan.scroll_to = Some(px);
                    self.scan.demo_home = true;
                }
            }
            if has("add") && !self.scan.demo_added && ctx.input(|i| i.time) > 2.0 {
                self.scan.demo_added = true;
                // `only-games` : seuls les jeux restent coches (pas de gros dossier perso copie pendant les captures).
                if has("only-games") {
                    for it in v.items.iter().filter(|i| i.cat != Cat::Games) {
                        self.scan.checked.insert(it.key.clone(), false);
                    }
                }
                // `only-windows` : seuls les Reglages Windows (decoches par defaut) sont coches puis proteges.
                if has("only-windows") {
                    for it in v.items.iter() {
                        self.scan.checked.insert(it.key.clone(), it.cat == Cat::System && it.selectable());
                    }
                }
                self.scan_protect(ctx, &v);
            }
        }
        if has("home") && self.scan.report.is_some() && ctx.input(|i| i.time) > 6.0 {
            self.scan.demo.clear();
            self.scan.open = false;
            self.scan.report = None;
            self.model.page = Page::Games;
            self.on_page_enter(Page::Games, ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    // ------------------------------------------------------------ premier lancement (etapes 4 et 5)

    /// Etat du scan pour l'assistant : (lance, termine, ajout en cours, bilan affiche, elements coches).
    pub(super) fn scan_wizard_status(&self) -> (bool, bool, bool, Option<usize>, usize) {
        let Some(h) = &self.scan.handle else { return (false, false, false, self.scan.report.as_ref().map(|r| r.added.len()), 0) };
        let v = h.view();
        let sel = v.items.iter().filter(|i| i.selectable() && self.scan.checked.get(&i.key).copied().unwrap_or(false)).count();
        (true, v.finished, self.scan.adding.is_some(), self.scan.report.as_ref().map(|r| r.added.len()), sel)
    }

    /// Etape 4 : lance le scan s'il n'a jamais tourne dans cette session.
    pub(super) fn scan_wizard_ensure(&mut self, ctx: &egui::Context) {
        if self.scan.handle.is_none() {
            self.scan_start(ctx);
        }
    }

    /// Etape 4 : « Protéger la sélection (N) » (bouton de l'assistant).
    pub(super) fn scan_wizard_protect(&mut self, ctx: &egui::Context) {
        let Some(h) = self.scan.handle.clone() else { return };
        let v = h.view();
        if v.finished && self.scan.adding.is_none() {
            self.scan_protect(ctx, &v);
        }
    }

    /// Etape 5 : ajouts rapides proposes par le dernier scan (Documents, Bureau, raccourcis du Bureau, fond d'ecran) encore
    /// proposables et pas encore proteges.
    pub(super) fn scan_quick_items(&self) -> Vec<(Item, String)> {
        let Some(h) = &self.scan.handle else { return vec![] };
        let v = h.view();
        let wanted = |it: &Item| matches!(it.origin, Origin::Personal(Personal::Documents) | Origin::Personal(Personal::Desktop) | Origin::System(SysItem::DesktopShortcuts) | Origin::System(SysItem::Wallpaper));
        let added: HashSet<String> = self.scan.report.as_ref().map(|r| r.added.iter().map(|n| n.to_lowercase()).collect()).unwrap_or_default();
        v.items
            .iter()
            .filter(|i| wanted(i) && i.selectable() && !added.contains(&i.name.to_lowercase()) && self.model.game(&i.name).is_none())
            .map(|i| (i.clone(), self.scan_name(i)))
            .collect()
    }

    /// Taille mesuree d'un resultat du dernier scan.
    pub(super) fn scan_size_of(&self, key: &str) -> Option<Size> {
        self.scan.handle.as_ref().and_then(|h| h.view().sizes.get(key).copied())
    }

    /// A appeler a chaque image (avant le dessin des pages) : lit le diagnostic des captures et ouvre le scan.
    pub(super) fn scan_demo_start(&mut self, ctx: &egui::Context) {
        if self.scan.demo_read {
            return;
        }
        if ctx.input(|i| i.time) < 1.0 {
            ui::anim::wake_at(ctx, 1.05);
            return;
        }
        self.scan.demo_read = true;
        self.scan.demo = std::env::var("AGSS_UI_SCAN_DEMO").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        if self.scan.demo.iter().any(|d| d == "open") {
            self.scan_open(ctx);
        }
    }
}

/// Contenu d'une ligne de resultat.
struct RowSpec<'a> {
    name: &'a str,
    why: &'a str,
    warn: Option<&'a str>,
    trailing: &'a str,
    checked: bool,
    checkable: bool,
    score: u8,
    tier: Tier,
    cat: Cat,
    tex: Option<&'a egui::TextureHandle>,
    /// Grise (deja protege, masque).
    dim: bool,
}

/// Ligne : case, icone, nom, pourquoi (+ avertissement), taille et date a droite, barre de confiance.
fn scan_row(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, s: &RowSpec, big: bool) -> egui::Response {
    let t = u::tokens(ui.ctx());
    let k = if big { 1.25 } else { 1.0 };
    let w = ui.available_width();
    let icon = 34.0 * k;
    let right_w = 260.0 * k;
    let text_w = (w - 28.0 * k - icon - 2.0 * u::tokens::SP_M - right_w - 20.0).max(80.0);
    let (tc, wc) = if s.dim { (t.faint, t.faint) } else { (t.text, t.weak) };
    let g1 = u::layout(ui, s.name, u::tokens::FS_BODY * k, u::col(tc), text_w, 1);
    let g2 = u::layout(ui, s.why, u::tokens::FS_SMALL * k, u::col(wc), text_w, 1);
    let g3 = s.warn.map(|x| u::layout(ui, x, u::tokens::FS_SMALL * k, u::col(t.orange), text_w, 1));
    let h = (g1.size().y + g2.size().y + g3.as_ref().map(|g| g.size().y + 2.0).unwrap_or(0.0) + 2.0).max(icon) + 16.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let resp = ui.interact(rect, ui.id().with(id_salt), if s.checkable { egui::Sense::click() } else { egui::Sense::hover() });
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let ctx = ui.ctx().clone();
    let hov = ctx.animate_bool_with_time(resp.id.with("h"), resp.hovered() && s.checkable, 0.12);
    let p = ui.painter();
    if hov > 0.0 {
        p.rect_filled(rect, u::tokens::R_MD, u::col(crate::theme::mix(t.panel, t.row_hover, hov)));
    }
    if resp.has_focus() {
        u::focus_ring(ui, rect, u::tokens::R_MD);
    }
    // case a cocher
    let mut x = rect.left() + 8.0;
    let cb = egui::Rect::from_min_size(egui::pos2(x, rect.center().y - 10.0 * k), egui::vec2(20.0 * k, 20.0 * k));
    if s.checkable {
        let on = ctx.animate_bool_with_time(resp.id.with("c"), s.checked, 0.12);
        p.rect(cb, 5.0, u::col(crate::theme::mix(t.card2, t.accent, on)), egui::Stroke::new(1.5_f32, u::col(if s.checked { t.accent } else { t.weak })), egui::StrokeKind::Inside);
        if on > 0.0 {
            u::paint_glyph(p, cb.shrink(3.0), Glyph::Check, u::col_a(t.on_accent, on));
        }
    } else if s.tier != Tier::Hidden {
        // deja protege : coche grise
        p.rect(cb, 5.0, u::col(t.card2), egui::Stroke::new(1.0_f32, u::col(t.line)), egui::StrokeKind::Inside);
        u::paint_glyph(p, cb.shrink(3.0), Glyph::Check, u::col(t.faint));
    }
    x = cb.right() + u::tokens::SP_M;
    // icone
    let ir = egui::Rect::from_min_size(egui::pos2(x, rect.center().y - icon / 2.0), egui::vec2(icon, icon));
    let op = if s.dim { 0.55 } else { 1.0 };
    match s.tex {
        Some(tex) => {
            p.image(tex.id(), ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE.gamma_multiply(op));
        }
        None => {
            let bg = cat_bg(s.cat);
            p.rect_filled(ir, 8.0, u::col_a(bg, op));
            u::paint_glyph(p, ir.shrink(icon * 0.24), cat_glyph(s.cat), u::col_a(crate::theme::text_on(bg), op));
        }
    }
    x = ir.right() + u::tokens::SP_M;
    // textes
    let th = g1.size().y + g2.size().y + g3.as_ref().map(|g| g.size().y + 2.0).unwrap_or(0.0) + 2.0;
    let mut y = rect.center().y - th / 2.0;
    let (h1, h2) = (g1.size().y, g2.size().y);
    p.galley(egui::pos2(x, y), g1, u::col(tc));
    y += h1 + 2.0;
    p.galley(egui::pos2(x, y), g2, u::col(wc));
    y += h2 + 2.0;
    if let Some(g3) = g3 {
        p.galley(egui::pos2(x, y), g3, u::col(t.orange));
    }
    // a droite : barre de confiance puis taille / date
    let mut rx = rect.right() - 10.0;
    if s.tier != Tier::Hidden {
        let bar = egui::Rect::from_min_size(egui::pos2(rx - 56.0, rect.center().y - 2.5), egui::vec2(56.0, 5.0));
        p.rect_filled(bar, 3.0, u::col(t.widget));
        let fc = if s.dim { t.faint } else if s.tier == Tier::Recommended { t.ok } else { t.orange };
        p.rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * (s.score as f32 / 100.0).clamp(0.05, 1.0), bar.height())), 3.0, u::col(fc));
        rx = bar.left() - u::tokens::SP_M;
    }
    if !s.trailing.is_empty() {
        let g = u::layout(ui, s.trailing, u::tokens::FS_SMALL * k, u::col(wc), right_w - 76.0 * k, 1);
        p.galley(egui::pos2(rx - g.size().x, rect.center().y - g.size().y / 2.0), g, u::col(wc));
    }
    if s.checkable {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Ligne-squelette pendant le scan : bande claire qui traverse (seul degrade de l'appli : maillage a couleurs de sommets).
fn skeleton_row(ui: &mut egui::Ui, k: usize) {
    let t = u::tokens(ui.ctx());
    let w = ui.available_width() * if k == 0 { 1.0 } else { 0.7 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
    let r = rect.shrink2(egui::vec2(10.0, 3.0));
    let p = ui.painter();
    p.rect_filled(r, 8.0, u::col(t.row_hover));
    let m = u::anim::motion(ui.ctx());
    if m.reduced {
        return;
    }
    let now = u::anim::now(ui.ctx());
    let ph = ((now / 1.4 + k as f64 * 0.35) % 1.0) as f32;
    let bw = r.width() * 0.35;
    let x0 = r.left() - bw + (r.width() + bw) * ph;
    let band = egui::Rect::from_min_max(egui::pos2(x0.max(r.left()), r.top()), egui::pos2((x0 + bw).min(r.right()), r.bottom()));
    if band.width() <= 1.0 {
        return;
    }
    let clear = egui::Color32::TRANSPARENT;
    let light = u::col_a(if t.dark { [255, 255, 255] } else { [0, 0, 0] }, 0.06);
    let mut mesh = egui::Mesh::default();
    let mid = x0 + bw / 2.0;
    let cols = [(x0, clear), (mid, light), (x0 + bw, clear)];
    for (i, (x, c)) in cols.iter().enumerate() {
        let xc = x.clamp(band.left(), band.right());
        mesh.colored_vertex(egui::pos2(xc, band.top()), *c);
        mesh.colored_vertex(egui::pos2(xc, band.bottom()), *c);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b, b + 1);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}
