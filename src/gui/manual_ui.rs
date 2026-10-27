//! Ajout d'un programme ou d'un element : aides du formulaire (resolution d'un programme depose ou choisi, nom deduit, icone,
//! validite) et OPTIONS AVANCEES du mode expert. Le formulaire lui-meme est dans `program_form_ui.rs` (mots simples) ; la
//! logique pure dans `crate::addform` / `crate::addpage`.
use super::*;
use crate::addform::FormIn;
use crate::addform::{self, Validity};
use crate::gui::ui::{self as u};

impl App {
    /// Petit choix discret (gris comme « Ajouter un emplacement ») ; l'option choisie a un fond un peu plus clair (pas l'accent).
    fn quiet_choice(&self, ui: &mut egui::Ui, selected: bool, label: &str, tip: K) -> egui::Response {
        let pal = self.palette;
        let mut b = egui::Button::new(RichText::new(label).small());
        if selected {
            let lighter = crate::theme::mix(pal.widget_bg, pal.text, 0.22);
            b = b.fill(rgb(lighter)).stroke(egui::Stroke::new(1.0_f32, rgb(pal.text).gamma_multiply(0.45)));
        }
        let r = ui.add(b);
        self.deco(ui, r, Some(tip))
    }

    /// Applique un exe choisi (depose, Parcourir) : memorise le chemin et deduit le nom si l'utilisateur ne l'a pas saisi.
    pub(super) fn set_man_exe(&mut self, exe: PathBuf) {
        self.man_exe = exe.display().to_string();
        self.man_path_found = false;
        self.man_filled_key.clear();
        let (product, desc) = winutil::exe_version_strings(&exe);
        let name = addform::derive_name(product.as_deref(), desc.as_deref(), &crate::hints::exe_file_name(&self.man_exe));
        if self.man_name.trim().is_empty() || self.man_name == self.man_name_auto {
            self.man_name = name.clone();
        }
        self.man_name_auto = name;
        self.man_icon = None;
    }

    /// Fichier depose ou choisi -> programme a surveiller. Windows : .exe ou raccourci .lnk. Linux : raccourci .desktop
    /// (Flatpak, Steam, Wine...), AppImage, binaire, script ou .exe lance par Proton / Wine.
    pub(super) fn resolve_program(&self, p: &std::path::Path) -> Option<PathBuf> {
        #[cfg(not(windows))]
        {
            if p.extension().map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                return addform::resolve_input(p);
            }
            let db = crate::gamedb::embedded(&self.shared.data_dir);
            let exes = |id: u64| -> Vec<String> { db.find_by_steam(id).into_iter().flat_map(|g| g.exes.clone()).collect() };
            crate::linuxapps::resolve_input(p, &crate::linuxapps::SysRoots::real(), &exes)
        }
        #[cfg(windows)]
        addform::resolve_input(p)
    }

    /// Boite « Choisir un programme » : Windows filtre les programmes et raccourcis ; Linux accepte tout fichier (les
    /// programmes Linux n'ont pas d'extension).
    pub(super) fn program_dialog(lang: Lang) -> rfd::FileDialog {
        #[cfg(windows)]
        return rfd::FileDialog::new().add_filter(tr(lang, K::VProgramFilter), &["exe", "lnk"]);
        #[cfg(not(windows))]
        {
            let _ = lang;
            rfd::FileDialog::new()
        }
    }

    pub(super) fn man_exe_icon(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        if self.man_icon.as_ref().map(|(k, _)| k != &self.man_exe).unwrap_or(true) {
            let tex = winutil::exe_icon_rgba(std::path::Path::new(&self.man_exe)).map(|(rgba, w, h)| ctx.load_texture("man-exe-icon", egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR));
            self.man_icon = Some((self.man_exe.clone(), tex));
        }
        self.man_icon.as_ref().and_then(|(_, t)| t.clone())
    }

    pub(super) fn man_validity(&self, path_ok: bool) -> Validity {
        addform::validity(&FormIn { exe: &self.man_exe, name: &self.man_name, path: &self.man_path, path_ok, schedule: self.man_sched.as_ref() })
    }

    /// Options avancees du formulaire (MODE EXPERT) : chemin du programme, Dossier / Fichier, nom, filtre, emplacements
    /// supplementaires, planification detaillee. Rien de tout cela n'apparait en mode simple.
    pub(super) fn form_advanced_expert(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, now: f64, program: bool) {
        let big = self.big_now;
        ui.add_space(8.0);
        ui.separator();
        if program {
            ui.label(RichText::new(format!("{} : {}", self.t(K::AddExe), self.man_exe)).small().weak());
        }
        // Choix Dossier / Fichier : petit et discret.
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(self.t(K::AddSavePath)).small().weak());
            for (is_file, k) in [(false, K::TargetFolder), (true, K::TargetFile)] {
                if self.quiet_choice(ui, self.man_is_file == is_file, self.t(k), K::TipTargetKind).clicked() {
                    self.man_is_file = is_file;
                }
            }
        });
        // Sans programme : la planification est obligatoire, donc toujours visible.
        if addform::schedule_required(&self.man_exe) {
            ui.add_space(8.0);
            self.sched_block(ui, now);
        }
        ui.add_space(6.0);
        if self.chevron_header_sized(ui, self.man_adv_open, self.t(K::AddAdvanced), if big { 17.0 } else { 14.0 }) {
            self.man_adv_open = !self.man_adv_open;
        }
        if self.man_adv_open {
            ui.label(self.t(K::AddName));
            let mut name = std::mem::take(&mut self.man_name);
            self.wide_field(ui, &mut name, K::TipManName, false);
            self.man_name = name;
            if !self.man_is_file {
                ui.label(self.t(K::FilterLabel));
                let mut f = std::mem::take(&mut self.man_filter);
                let r = ui.add(egui::TextEdit::singleline(&mut f).hint_text(self.t(K::FilterHint)).desired_width(f32::INFINITY));
                self.deco_kind(ui, r, Some(K::TipFilter), ItemKind::Text);
                self.man_filter = f;
            }
            let mut extras = std::mem::take(&mut self.man_extras);
            let mut rm: Option<usize> = None;
            for (i, l) in extras.iter_mut().enumerate() {
                if self.loc_editor(ui, l, &format!("add_{}", i)) {
                    rm = Some(i);
                }
            }
            if let Some(i) = rm {
                extras.remove(i);
            }
            self.man_extras = extras;
            let b = ui.button(self.t(K::LocAdd));
            if self.deco(ui, b, Some(K::TipLocAdd)).clicked() {
                let n = self.man_extras.len() + 2;
                let l = self.new_location(n);
                self.man_extras.push(l);
            }
            if !addform::schedule_required(&self.man_exe) {
                ui.add_space(6.0);
                self.sched_block(ui, now);
            }
        }
    }

    /// Page Ajouter, mode expert : profils integres (liste complete, variables), dans une carte repliable.
    pub(super) fn add_profiles_expert(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        u::card(ui, None, |ui| {
            let open = self.add_ui.profiles_open;
            if self.chevron_header(ui, open, self.tm(K::VKnown)) {
                self.add_ui.profiles_open = !open;
            }
            if self.add_ui.profiles_open {
                self.profiles_card(ui, ctx);
            }
        });
    }

    /// Bloc de planification (interrupteur + editeur) ; mis en evidence un court instant apres un clic invalide.
    fn sched_block(&mut self, ui: &mut egui::Ui, now: f64) {
        let flash = self.man_flash.map(|t| now - t < 2.5).unwrap_or(false);
        let frame = if flash { egui::Stroke::new(2.0_f32, rgb(self.palette.accent)) } else { egui::Stroke::NONE };
        egui::Frame::new().stroke(frame).corner_radius(8.0).inner_margin(6.0).show(ui, |ui| {
            ui.label(RichText::new(self.t(K::SchedTitle)).strong());
            let mut on = self.man_sched.is_some();
            if self.toggle(ui, &mut on, self.t(K::SchedToggle), K::TipSched).changed() {
                self.man_sched = if on { Some(crate::schedule::Schedule::fixed(vec![12 * 3600])) } else { None };
            }
            if let Some(mut s) = self.man_sched.take() {
                self.schedule_editor(ui, &mut s, "man");
                self.man_sched = Some(s);
            }
        });
        if flash {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }
}

/// Programme de demonstration (captures) : `AGSS_DEMO_EXE` s'il est defini, sinon un programme du systeme.
fn demo_program() -> PathBuf {
    if let Some(p) = std::env::var_os("AGSS_DEMO_EXE").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    return PathBuf::from("C:/Windows/System32/notepad.exe");
    #[cfg(not(windows))]
    return PathBuf::from("/usr/bin/xterm");
}

impl App {
    /// Aide au diagnostic (captures d'ecran) : `AGSS_DEMO=open_all,support,form_ready,form_file` (liste separee par des virgules).
    pub(super) fn apply_demo_env(&mut self) {
        let Ok(v) = std::env::var("AGSS_DEMO") else { return };
        for part in v.split(',').map(|s| s.trim()) {
            match part {
                "open_all" => self.demo_open_all = true,
                "profiles" => self.demo_open_all = true,
                "support" => self.support = Some(Instant::now()),
                "usage_ask" => self.late_card_open = true,
                "usage_view" => self.usage_view = Some(crate::usage::pending_text(&self.shared.data_dir, true)),
                "usage_card" => {
                    self.demo_open_all = false;
                    self.demo_usage_card = true;
                }
                "form_ready" => {
                    self.set_man_exe(demo_program());
                    self.man_path = std::env::temp_dir().display().to_string();
                }
                "form_file" => {
                    self.set_man_exe(demo_program());
                    self.man_is_file = true;
                    #[cfg(windows)]
                    {
                        self.man_path = "C:/Windows/win.ini".into();
                    }
                    #[cfg(not(windows))]
                    {
                        self.man_path = "/etc/hostname".into();
                    }
                }
                // Programme choisi, emplacement a trouver (connaissance intelligente).
                "form_exe" => self.set_man_exe(demo_program()),
                "form_noexe" => {
                    let dir = std::env::var_os("AGSS_DEMO_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
                    self.add_start_target(dir, false);
                }
                "adv" => self.man_adv_open = true,
                _ => {}
            }
        }
    }
}
