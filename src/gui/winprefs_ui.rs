//! Reglages Windows (2.0.2), partie ecran : noms affiches, bloc de la fiche (ce qui est garde, quand ca prend effet,
//! bilan de la derniere remise : reglages ignores entre Windows 10 et 11, choix proteges par Windows a refaire dans les
//! Parametres), accord explicite pour le fichier hosts (droits administrateur), liste « A refaire » du Nouveau PC.
//! Logique pure et testee : `crate::winprefs`.
use super::*;
use crate::gui::ui::{self as u, Btn, State};
use crate::winprefs::{self, Effect, Kind, Report};
use std::collections::HashMap;
use std::sync::Mutex;

/// Ce que la fiche montre d'un element (fichiers locaux relus au plus toutes les 2 s).
#[derive(Clone, Default)]
pub(super) struct PrefInfo {
    pub count: usize,
    pub report: Option<Report>,
    pub hosts_pending: bool,
}

static INFO: Mutex<Option<HashMap<String, (std::time::Instant, PrefInfo)>>> = Mutex::new(None);

/// Texte du nom affiche d'un element des Reglages Windows.
pub(super) fn pref_label_key(k: Kind) -> K {
    match k {
        Kind::Theme => K::VPrefTheme,
        Kind::Mouse => K::VPrefMouse,
        Kind::Keyboard => K::VPrefKeyboard,
        Kind::Explorer => K::VPrefExplorer,
        Kind::Taskbar => K::VPrefTaskbar,
        Kind::Region => K::VPrefRegion,
        Kind::Sound => K::VPrefSound,
        Kind::Power => K::VPrefPower,
        Kind::EnvVars => K::VPrefEnv,
        Kind::FileAssoc => K::VPrefAssoc,
        Kind::Fonts => K::VPrefFonts,
        Kind::Office => K::VPrefOffice,
        Kind::Dictionary => K::VPrefDict,
        Kind::Terminal => K::VPrefTerminal,
        Kind::Hosts => K::VPrefHosts,
        Kind::QuickAccess => K::VPrefQuick,
        Kind::TodoList => K::VPrefTodo,
    }
}

/// Precision sous le nom (propre a l'appareil, accord administrateur, en partie, liste seulement).
pub(super) fn pref_note_key(k: Kind) -> Option<K> {
    match k {
        Kind::Power => Some(K::VPrefDevice),
        Kind::TodoList => Some(K::VPrefListOnly),
        Kind::Hosts => Some(K::VPrefAdmin),
        Kind::FileAssoc => Some(K::VPrefPartial),
        _ => None,
    }
}

pub(super) fn effect_key(e: Effect) -> K {
    match e {
        Effect::Now => K::VPrefEffectNow,
        Effect::NextOpen => K::VPrefEffectOpen,
        Effect::NextSignIn => K::VPrefEffectSignIn,
        Effect::Never => K::VPrefEffectNever,
    }
}

impl App {
    /// Element des Reglages Windows (emplacement principal = export local) ?
    pub(super) fn pref_kind(&self, name: &str) -> Option<Kind> {
        self.model.game(name).and_then(|g| winprefs::kind_of_game(&self.shared.data_dir, g))
    }

    pub(super) fn pref_info(&self, k: Kind) -> PrefInfo {
        let dd = &self.shared.data_dir;
        let key = k.code().to_string();
        let mut g = INFO.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.get_or_insert_with(HashMap::new);
        if let Some((at, i)) = m.get(&key) {
            if at.elapsed() < std::time::Duration::from_secs(2) {
                return i.clone();
            }
        }
        let dir = winprefs::staging(dd, k);
        let count = match winprefs::PrefFile::read(&dir.join(winprefs::FILE)) {
            Some(p) => p.entries.len() + p.todo.map(|t| t.printers.len() + t.drives.len() + t.wifi.len()).unwrap_or(0),
            None => usize::from(dir.join(winprefs::HOSTS_FILE).is_file()),
        };
        let l = winprefs::load_local(dd, k);
        let i = PrefInfo { count, report: l.report, hosts_pending: l.hosts_pending };
        m.insert(key, (std::time::Instant::now(), i.clone()));
        i
    }

    fn pref_info_forget(&self) {
        if let Ok(mut g) = INFO.lock() {
            *g = None;
        }
    }

    pub(super) fn pref_count_text(&self, n: usize) -> String {
        let l = self.model.settings.lang;
        self.t(crate::i18n::pick_n(l, n as u64, K::VPrefCountOne, K::VPrefCountMany)).replace("{}", &n.to_string())
    }

    /// Bilan d'une remise en phrases simples (fiche, toast, Nouveau PC).
    pub(super) fn pref_report_lines(&self, r: &Report) -> Vec<String> {
        let mut v = Vec::new();
        if r.item != Kind::TodoList.code() {
            v.push(self.t(K::VPrefLast).replace("{0}", &r.applied.to_string()).replace("{1}", &r.same.to_string()));
        }
        if r.other_os > 0 {
            v.push(self.t(K::VPrefOtherOs).replace("{}", &r.other_os.to_string()));
        }
        if r.failed > 0 {
            v.push(self.t(K::VPrefFailed).replace("{}", &r.failed.to_string()));
        }
        let redo: Vec<&String> = r.todo.iter().filter(|t| t.starts_with('.')).collect();
        if !redo.is_empty() {
            let mut list: Vec<String> = redo.iter().take(8).map(|s| s.to_string()).collect();
            if redo.len() > 8 {
                list.push("…".into());
            }
            v.push(self.t(K::VPrefRedo).replace("{}", &list.join(", ")));
        }
        if r.dry_run {
            v.push(self.t(K::VPrefDryRun).to_string());
        }
        v
    }

    /// Bloc « Reglages Windows » de la fiche.
    pub(super) fn fiche_prefs_block(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        let Some(k) = self.pref_kind(name) else { return };
        let info = self.pref_info(k);
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let mut open_settings = false;
        let mut hosts: Option<bool> = None;
        u::cascade_in(ui, ("fiche_prefs", name), 1, self.home.epoch, |ui| {
            u::card(ui, Some(self.t(K::VPrefTitle)), |ui| {
                ui.label(RichText::new(self.t(pref_label_key(k))).strong().color(u::col(tk.text)));
                if info.count > 0 && k.reads_registry() {
                    ui.label(RichText::new(self.pref_count_text(info.count)).color(u::col(tk.weak)));
                }
                if let Some(n) = pref_note_key(k) {
                    ui.label(RichText::new(self.t(n)).color(u::col(tk.weak)));
                }
                ui.label(RichText::new(self.t(effect_key(k.effect()))).color(u::col(tk.weak)));
                if let Some(r) = &info.report {
                    ui.add_space(u::tokens::SP_S);
                    for line in self.pref_report_lines(r) {
                        ui.label(RichText::new(line).color(u::col(tk.text)));
                    }
                    if !r.todo.is_empty() && k != Kind::Hosts {
                        let b = u::button_with(ui, Btn::Secondary, self.t(K::VPrefOpenSettings), None, big);
                        if self.deco(ui, b, None).clicked() {
                            open_settings = true;
                        }
                    }
                }
                if k == Kind::Hosts && info.hosts_pending {
                    ui.add_space(u::tokens::SP_S);
                    ui.label(RichText::new(self.t(K::VPrefHostsPending)).strong().color(u::col(tk.state_text(State::Todo))));
                    ui.label(RichText::new(self.t(K::VPrefHostsBody)).color(u::col(tk.weak)));
                    ui.horizontal_wrapped(|ui| {
                        let b = u::button_with(ui, Btn::Primary, self.t(K::VPrefHostsApply), None, big);
                        if self.deco(ui, b, None).clicked() {
                            hosts = Some(true);
                        }
                        let b = u::button_with(ui, Btn::Secondary, self.t(K::VPrefHostsKeep), None, big);
                        if self.deco(ui, b, None).clicked() {
                            hosts = Some(false);
                        }
                    });
                }
            });
        });
        ui.add_space(u::tokens::SP_M);
        if open_settings {
            winutil::open_url(k.settings_uri());
        }
        match hosts {
            Some(false) => {
                winprefs::hosts_keep_mine(&self.shared.data_dir);
                self.pref_info_forget();
                self.shared.restart_watch();
            }
            Some(true) => {
                let (tx, c, dd, lang) = (self.tx.clone(), ctx.clone(), self.shared.data_dir.clone(), self.model.settings.lang);
                std::thread::spawn(move || {
                    let log = Logger::new(dd.join("logs").join("watch.log"), false);
                    let r = winprefs::approve_hosts(&dd);
                    log.log(&format!("[Reglages Windows] fichier hosts recu : accord donne, resultat {:?}", r));
                    let (title, detail, ok) = match r {
                        winprefs::HostsApply::Done => (tr(lang, K::VPrefHostsDone).to_string(), String::new(), true),
                        winprefs::HostsApply::Cancelled => (tr(lang, K::VPrefHostsCancelled).to_string(), String::new(), false),
                        winprefs::HostsApply::Failed(e) => (tr(lang, K::VPrefHostsFailed).to_string(), e, false),
                    };
                    let _ = tx.send(Msg::FicheDone { title, detail, ok });
                    c.request_repaint();
                });
                self.pref_info_forget();
            }
            None => {}
        }
    }

    /// Carte de l'accueil : le fichier hosts recu attend l'accord (jamais applique en silence).
    pub(super) fn prefs_home_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(name) = self.model.config.games.iter().find(|g| winprefs::kind_of_game(&self.shared.data_dir, g) == Some(Kind::Hosts)).map(|g| g.name.clone()) else { return };
        if !self.pref_info(Kind::Hosts).hosts_pending {
            return;
        }
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let mut open = false;
        egui::Frame::new().fill(u::col(crate::theme::mix(tk.card2, tk.orange, 0.12))).stroke(egui::Stroke::new(1.0_f32, u::col(tk.orange))).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                u::state_icon(ui, "prefs_hosts_card", State::Todo, 24.0, None);
                ui.vertical(|ui| {
                    ui.label(RichText::new(self.t(K::VPrefHostsPending)).strong().color(u::col(tk.text)));
                    ui.label(RichText::new(self.t(K::VPrefAdmin)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = u::button_with(ui, Btn::Primary, self.t(K::VDetails), None, big);
                    if self.deco(ui, r, None).clicked() {
                        open = true;
                    }
                });
            });
        });
        ui.add_space(u::tokens::SP_S);
        if open {
            self.select_game(&name, ctx);
        }
    }

    /// Phrase ajoutee a la confirmation « Remettre cette version » d'un element des Reglages Windows.
    pub(super) fn pref_restore_note(&self, name: &str) -> Option<String> {
        let k = self.pref_kind(name)?;
        Some(format!("{}\n{}", self.t(K::VPrefRestoreNote), self.t(effect_key(k.effect()))))
    }

    /// Bilan de la derniere remise (toast apres « Remettre cette version »).
    pub(super) fn pref_restored_detail(&self, name: &str) -> Option<String> {
        let k = self.pref_kind(name)?;
        self.pref_info_forget();
        let i = self.pref_info(k);
        let mut v = i.report.as_ref().map(|r| self.pref_report_lines(r)).unwrap_or_default();
        if i.hosts_pending {
            v.push(self.t(K::VPrefHostsPending).to_string());
        }
        v.push(self.t(effect_key(k.effect())).to_string());
        Some(v.join("\n"))
    }

    /// Liste « A refaire » du Nouveau PC (imprimantes, lecteurs reseau, Wi-Fi notes par l'ancien appareil).
    pub(super) fn todo_list_block(&mut self, ui: &mut egui::Ui, device: &str, list: &winprefs::TodoList) {
        let tk = u::tokens(ui.ctx());
        let big = self.big_now;
        u::card(ui, Some(self.t(K::VPrefTodoTitle)), |ui| {
            ui.label(RichText::new(self.t(K::VPrefTodoHint).replace("{}", device)).color(u::col(tk.weak)));
            let sections: [(K, Vec<String>, K, &str); 3] = [
                (
                    K::VPrefPrinters,
                    list.printers.iter().map(|p| if p.driver.is_empty() { p.name.clone() } else { format!("{} · {}", p.name, self.t(K::VPrefDriver).replace("{}", &p.driver)) }).collect(),
                    K::VPrefOpenPrinters,
                    "ms-settings:printers",
                ),
                (K::VPrefDrives, list.drives.iter().map(|d| format!("{}  {}", d.letter, d.path)).collect(), K::VPrefOpenDrives, "shell:MyComputerFolder"),
                (K::VPrefWifi, list.wifi.clone(), K::VPrefOpenWifi, "ms-settings:network-wifi"),
            ];
            for (title, lines, btn, uri) in sections {
                if lines.is_empty() {
                    continue;
                }
                ui.add_space(u::tokens::SP_S);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.t(title)).strong().color(u::col(tk.text)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let b = u::button_with(ui, Btn::Secondary, self.t(btn), None, big);
                        if self.deco(ui, b, None).clicked() {
                            if uri.starts_with("shell:") {
                                let _ = winutil::open_exe(std::path::Path::new("explorer.exe"), uri);
                            } else {
                                winutil::open_url(uri);
                            }
                        }
                    });
                });
                for l in lines.iter().take(30) {
                    ui.label(RichText::new(format!("•  {}", l)).color(u::col(tk.weak)));
                }
            }
        });
    }
}
