//! Elements communs aux ecrans : champ pleine largeur, icones des programmes, pastilles d'appareils, editeur d'emplacement.
use super::picker_ui::PickOpts;
use super::*;

impl App {
    // ------------------------------------------------------------ champ pleine largeur

    /// Champ de texte sur toute la largeur disponible, avec un bouton « Parcourir » facultatif a droite.
    /// Renvoie true si « Parcourir » a ete clique.
    pub(super) fn wide_field(&self, ui: &mut egui::Ui, text: &mut String, tip: K, browse: bool) -> bool {
        if browse {
            self.field_row(ui, text, tip, &[(K::BtnBrowse, K::TipBrowse)]) == Some(0)
        } else {
            let h = ui.spacing().interact_size.y;
            let r = ui.add_sized([ui.available_width().max(60.0), h], egui::TextEdit::singleline(text));
            self.deco_kind(ui, r, Some(tip), ItemKind::Text);
            false
        }
    }

    pub(super) fn icon_for(&mut self, ectx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
        if !self.icons.contains_key(name) {
            let tex = self
                .model
                .settings
                .per_game
                .get(name)
                .and_then(|g| g.exe_path.clone())
                .and_then(|p| winutil::exe_icon_rgba(&p))
                .map(|(rgba, w, h)| ectx.load_texture(format!("icon-{}", name), egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR));
            self.icons.insert(name.to_string(), tex);
        }
        self.icons.get(name).cloned().flatten()
    }
}

impl App {
    pub(crate) fn machine_rgb(&self, name: &str) -> [u8; 3] {
        self.machine_map.get(name).copied().unwrap_or_else(|| crate::machinecolor::color_of(name, &self.model.settings.machine_colors))
    }

    /// Nom de PC en pastille coloree (couleur stable par machine) ; « (ce PC) » a cote de cette machine.
    pub(crate) fn machine_pill(&self, ui: &mut egui::Ui, name: &str) {
        let col = self.machine_rgb(name);
        let txt = crate::machinecolor::text_on(col);
        let font = egui::TextStyle::Small.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(name.to_string(), font, rgb(txt));
        let pad = egui::vec2(7.0, 2.0);
        let (r, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::hover());
        ui.painter().rect_filled(r, r.height() / 2.0, rgb(col));
        ui.painter().galley(r.min + pad, galley, rgb(txt));
        if name.eq_ignore_ascii_case(&self.model.ctx.machine) {
            ui.label(RichText::new(self.t(K::ThisPc)).small().weak());
        }
    }
}

impl App {
    /// Editeur d'UN emplacement supplementaire (actif, nom, dossier/fichier, chemin + Parcourir, existence, filtre replie).
    /// Retourne vrai si « Retirer » est clique.
    pub(crate) fn loc_editor(&mut self, ui: &mut egui::Ui, loc: &mut crate::config::Location, salt: &str) -> bool {
        let mut remove = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                self.toggle(ui, &mut loc.enabled, self.t(K::LocEnabled), K::TipLocEnabled);
                let r = ui.add(egui::TextEdit::singleline(&mut loc.label).hint_text(self.t(K::LocLabel)).desired_width(150.0));
                self.deco_kind(ui, r, Some(K::TipLocLabel), ItemKind::Text);
                let b = ui.button(self.t(K::LocRemove));
                if self.deco(ui, b, Some(K::TipLocRemove)).clicked() {
                    remove = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (is_file, k) in [(false, K::TargetFolder), (true, K::TargetFile)] {
                    let r = ui.selectable_label(loc.is_file == is_file, self.t(k));
                    if self.deco(ui, r, Some(K::TipTargetKind)).clicked() {
                        loc.is_file = is_file;
                    }
                }
            });
            let mut txt = loc.path.display().to_string();
            let pkey = format!("loc:{}", salt);
            if let Some(p) = self.take_picked(&pkey) {
                txt = p.path.display().to_string();
                loc.is_file = p.is_file;
            }
            if self.wide_field(ui, &mut txt, K::TipLocSave, true) {
                let name = loc.label.clone();
                let opts = PickOpts { name: Some(name.clone()).filter(|n| !n.trim().is_empty()), is_file: loc.is_file, names: vec![name], found: vec![], program: None };
                let c = ui.ctx().clone();
                self.open_picker(&c, &pkey, opts);
            }
            loc.path = PathBuf::from(txt.trim());
            if loc.path.as_os_str().is_empty() {
                // pas encore choisi
            } else if loc.exists() {
                icon_label(ui, Icon::Check, self.t(K::LocOk), Color32::from_rgb(60, 180, 100));
            } else {
                icon_label(ui, Icon::Cross, self.t(K::LocMissing), Color32::from_rgb(220, 80, 80));
            }
            if !loc.is_file {
                egui::CollapsingHeader::new(self.t(K::FilterLabel)).id_salt(format!("locf_{}", salt)).default_open(loc.filter.is_some()).show(ui, |ui| {
                    let mut f = loc.filter.clone().unwrap_or_default();
                    let r = ui.add(egui::TextEdit::singleline(&mut f).hint_text(self.t(K::FilterHint)).desired_width(f32::INFINITY));
                    self.deco_kind(ui, r, Some(K::TipFilter), ItemKind::Text);
                    loc.filter = Some(f).filter(|x| !x.trim().is_empty());
                });
            }
        });
        remove
    }

    /// Nouvel emplacement vide (identifiant stable genere).
    pub(crate) fn new_location(&self, n: usize) -> crate::config::Location {
        let label = format!("{} {}", self.t(K::LocNewLabel), n);
        crate::config::Location { id: crate::config::Location::new_id(&label), label, path: PathBuf::new(), is_file: false, filter: None, exclude: None, enabled: true }
    }
}
