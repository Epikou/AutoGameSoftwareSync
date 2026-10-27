//! Mise a jour automatique dans l'interface (lot 4) : thread de controle quotidien, bandeau, installation au moment calme,
//! annonce du resultat (« Mis a jour en X · Quoi de neuf ? », retour arriere). Toute la logique est dans `selfupdate`.
use super::*;
use crate::selfupdate::{self, BannerView};
use crate::settings::UpdateMode;

impl Shared {
    /// Thread de controle. Dossier de donnees de test : rien, sauf adresse de debogage explicite (AGSS_UPDATE_URL).
    pub(super) fn spawn_update_thread(self: &Arc<Self>, ctx: egui::Context) {
        if crate::appdirs::data_dir_from_env().is_some() && std::env::var_os(selfupdate::ENV_URL).is_none() {
            return;
        }
        let (a, b, c, d, e, f) = (self.clone(), self.clone(), self.clone(), self.clone(), self.clone(), self.clone());
        let env = selfupdate::Env {
            busy: Box::new(move || a.knowledge_busy() || crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) > 0),
            quitting: Box::new(move || b.quitting.load(Ordering::SeqCst)),
            on_latest: Box::new(move |latest| {
                if let Ok(mut l) = c.update_latest.lock() {
                    *l = latest.clone();
                }
                let s = crate::settings::Settings::load_from(&c.data_dir).unwrap_or_default();
                // Mode Automatique : silence (la nouvelle version s'installe seule) ; sinon une annonce par version.
                if let Some(r) = crate::update::banner(latest.as_ref(), crate::update::current_version(), s.ignored_update.as_deref(), s.update_mode() != UpdateMode::Never) {
                    let first = c.update_notified.lock().map(|mut n| if n.as_deref() == Some(r.tag.as_str()) { false } else { *n = Some(r.tag.clone()); true }).unwrap_or(false);
                    if first && s.notify_global && s.update_mode() == UpdateMode::Ask {
                        use crate::notify::Notifier;
                        crate::notify::WinNotifier.notify(tr(s.lang, K::NotifUpdate), &tr(s.lang, K::UpdAvailable).replace("{}", &r.tag));
                    }
                }
            }),
            repaint: Box::new(move || ctx.request_repaint()),
            calm: Box::new(move || {
                // (La question des statistiques n'est plus bloquante : ligne discrete, carte unique.)
                !d.knowledge_busy() && crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) == 0 && !d.ui_busy.load(Ordering::SeqCst)
            }),
            syncing: Box::new(move || crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) > 0),
            hidden: Box::new(|| !winutil::own_window_visible(APP_TITLE)),
            quit: Box::new(move || e.quit()),
            failed: Box::new(move |v| {
                let lang = crate::settings::Settings::load_from(&f.data_dir).unwrap_or_default().lang;
                if let Ok(mut q) = f.ui_events.lock() {
                    q.push((tr(lang, K::UpdFailed).replace("{}", v), false));
                }
            }),
        };
        selfupdate::spawn_worker(self.data_dir.clone(), self.upd.clone(), env);
    }
}

impl App {
    /// Bandeau de mise a jour (sous la barre, au-dessus des pages).
    pub(super) fn update_banner(&mut self, ui: &mut egui::Ui) {
        let mode = self.model.settings.update_mode();
        let upd = self.shared.update_latest.lock().ok().and_then(|l| l.clone());
        let phase = self.shared.upd.phase();
        let announced = crate::update::banner(upd.as_ref(), crate::update::current_version(), self.model.settings.ignored_update.as_deref(), mode != UpdateMode::Never).cloned();
        let view = selfupdate::banner_view(mode, &phase, announced.as_ref().or(upd.as_ref()));
        let ignored = self.model.settings.ignored_update.clone();
        let cur = crate::update::current_version();
        let visible = |tag: &str| crate::update::is_newer(tag, cur) && ignored.as_deref().map(|i| crate::update::is_newer(tag, i)).unwrap_or(true);
        let accent = rgb(self.palette.accent);
        let frame = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.5_f32, accent)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| add(ui));
            });
        };
        match view {
            BannerView::None => {}
            BannerView::Offer { version, url, installable } if visible(&version) => {
                let mut act = 0; // 1 = installer, 2 = ouvrir la page, 3 = ignorer
                frame(ui, &mut |ui| {
                    ui.label(RichText::new(self.t(K::UpdAvailable).replace("{}", &version)).strong());
                    if installable {
                        let b = ui.button(self.t(K::BtnUpdInstall));
                        if self.deco(ui, b, Some(K::TipUpdInstall)).clicked() {
                            act = 1;
                        }
                    }
                    let b = ui.button(self.t(K::BtnUpdOpen));
                    if self.deco(ui, b, Some(K::TipUpdOpen)).clicked() {
                        act = 2;
                    }
                    let b = ui.button(self.t(K::BtnUpdIgnore));
                    if self.deco(ui, b, Some(K::TipUpdIgnore)).clicked() {
                        act = 3;
                    }
                    if !installable && mode == UpdateMode::Auto {
                        ui.label(RichText::new(self.t(K::UpdManualHint)).small().weak());
                    }
                });
                match act {
                    1 => self.shared.upd.request_install(),
                    2 => winutil::open_url(&url),
                    3 => {
                        self.model.settings.ignored_update = Some(version);
                        let _ = self.model.save_settings();
                    }
                    _ => {}
                }
            }
            BannerView::Progress { version, percent } => {
                frame(ui, &mut |ui| {
                    let mut t = self.t(K::UpdDownloading).replace("{}", &version);
                    if let Some(p) = percent {
                        t.push_str(&format!(" {} %", p));
                    }
                    ui.label(RichText::new(t).strong());
                    ui.spinner();
                });
            }
            BannerView::Installing { version } => {
                frame(ui, &mut |ui| {
                    ui.label(RichText::new(self.t(K::UpdInstalling).replace("{}", &version)).strong());
                    ui.spinner();
                });
            }
            BannerView::Failed { version, url } if visible(&version) => {
                let mut open = false;
                frame(ui, &mut |ui| {
                    ui.colored_label(Color32::from_rgb(230, 170, 40), self.t(K::UpdFailed).replace("{}", &version));
                    if !url.is_empty() {
                        let b = ui.button(self.t(K::BtnUpdOpen));
                        if self.deco(ui, b, Some(K::TipUpdOpen)).clicked() {
                            open = true;
                        }
                    }
                });
                if open {
                    winutil::open_url(&url);
                }
            }
            _ => {}
        }
        // Resultat de la derniere mise a jour (30 s) : « Mis a jour en X · Quoi de neuf ? ».
        if let Some((ver, url, at)) = self.updated_banner.clone() {
            if at.elapsed() < Duration::from_secs(60) {
                let mut act = 0;
                frame(ui, &mut |ui| {
                    ui.label(RichText::new(format!("{} · ", self.t(K::NotifUpdated).replace("{}", &ver))).strong());
                    let b = ui.button(self.t(K::UpdWhatsNew));
                    if self.deco(ui, b, Some(K::TipUpdOpen)).clicked() {
                        act = 1;
                    }
                    if ui.small_button(self.t(K::BtnClose)).clicked() {
                        act = 2;
                    }
                });
                match act {
                    1 => winutil::open_url(&url),
                    2 => self.updated_banner = None,
                    _ => {}
                }
            } else {
                self.updated_banner = None;
            }
        }
    }

    /// Resultat a afficher dans la fenetre (banniere « Mis a jour en X · Quoi de neuf ? », message de retour arriere).
    pub(super) fn update_report_tick(&mut self) {
        self.shared.ui_frames.fetch_add(1, Ordering::Relaxed);
        let r = self.shared.upd_report.lock().ok().and_then(|mut g| g.take());
        let Some(r) = r else { return };
        if r.kind == "ok" {
            self.updated_banner = Some((r.to.clone(), r.notes_url.clone(), Instant::now()));
        } else {
            let lang = self.model.settings.lang;
            self.say(tr(lang, K::UpdRolledBack).replace("{0}", &r.to).replace("{1}", &r.from), false);
        }
    }
}

impl Shared {
    /// Confirme le demarrage de la version installee par une mise a jour (fichier marqueur lu par le veilleur) puis annonce le
    /// resultat. Dans un fil a part : une fenetre cachee (zone de notification) ne se redessine pas. « Sain » = l'interface a
    /// dessine quelques images, ou (fenetre cachee, reduite ou sans image a dessiner) que le processus tient 10 s.
    pub(super) fn spawn_update_confirm_thread(self: &Arc<Self>, start_hidden: bool) {
        let me = self.clone();
        let _ = std::thread::Builder::new().name("agss-update-confirm".into()).spawn(move || {
            let t0 = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(300));
                if me.quitting.load(Ordering::SeqCst) {
                    return;
                }
                let up = t0.elapsed();
                let healthy = up > Duration::from_secs(10) || (!start_hidden && me.ui_frames.load(Ordering::Relaxed) >= 5 && up > Duration::from_secs(3));
                if healthy {
                    break;
                }
            }
            let dd = me.data_dir.clone();
            let Some(r) = selfupdate::confirm_started(&dd, crate::update::current_version(), crate::util::now_ms()) else { return };
            let lg = Logger::new(dd.join("logs").join("watch.log"), false);
            let s = crate::settings::Settings::load_from(&dd).unwrap_or_default();
            use crate::notify::Notifier;
            if r.kind == "ok" {
                lg.log(&format!("[MiseAJour] mise a jour reussie : {} -> {}", r.from, r.to));
                selfupdate::record_usage(&dd, &r.from, &r.to, "ok");
                if s.notify_global {
                    crate::notify::notify_with_url(&crate::notify::WinNotifier, &tr(s.lang, K::NotifUpdated).replace("{}", &r.to), tr(s.lang, K::UpdWhatsNew), &r.notes_url);
                }
            } else {
                lg.log(&format!("[MiseAJour] retour arriere : la version {} ne demarrait pas ({}), retour a {} ; version bloquee", r.to, r.reason, r.from));
                selfupdate::record_usage(&dd, &r.from, &r.to, "rollback");
                if s.notify_global {
                    crate::notify::WinNotifier.notify(tr(s.lang, K::NotifUpdateRolledBack), &tr(s.lang, K::UpdRolledBack).replace("{0}", &r.to).replace("{1}", &r.from));
                }
            }
            if let Ok(mut g) = me.upd_report.lock() {
                *g = Some(r);
            }
        });
    }
}
