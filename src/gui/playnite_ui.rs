//! 2.0.3 : carte discrete de l'accueil « N jeux Playnite pas encore proteges », qui ouvre la section Playnite du scan du PC.
//! Le compte est fait dans un fil de fond (lecture du fichier ecrit par l'extension), au plus une fois par minute ; il ne
//! compte que les jeux reellement proposables (jeux PC, pas encore proteges : voir `playnite::not_protected`) et reste
//! stable si le fichier est momentanement illisible (remplacement par l'extension). « Masquer » masque pour 30 jours.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph, State};

#[derive(Default)]
pub struct PlayniteUi {
    count: usize,
    slot: Arc<Mutex<Option<Option<usize>>>>,
    loading: bool,
    at: Option<Instant>,
}

/// Cle du titre de la carte selon le nombre et la langue (pluriels russes : 1 / 2-4 / 5+).
pub fn card_key(n: usize, lang: Lang) -> K {
    match crate::i18n::plural_form(n, lang) {
        crate::i18n::Plural::One => K::VPlayniteCardOne,
        crate::i18n::Plural::Few => K::VPlayniteCardFew,
        crate::i18n::Plural::Many => K::VPlayniteCard,
    }
}

impl App {
    pub(super) fn playnite_home_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(r) = self.playnite.slot.lock().ok().and_then(|mut s| s.take()) {
            if let Some(n) = r {
                self.playnite.count = n;
            }
            self.playnite.loading = false;
            self.playnite.at = Some(Instant::now());
        }
        if !self.playnite.loading && self.playnite.at.map(|t| t.elapsed() > Duration::from_secs(60)).unwrap_or(true) {
            self.playnite.loading = true;
            let (slot, dd, games, c) = (self.playnite.slot.clone(), self.shared.data_dir.clone(), self.model.config.games.clone(), ctx.clone());
            std::thread::spawn(move || {
                // Fichier absent ou illisible : None (le dernier compte est garde) ; Playnite n'existe que sous Windows.
                let n = if cfg!(windows) { crate::playnite::read_from_opt(&dd.join(crate::playnite::FILE)).map(|lib| crate::playnite::not_protected(&lib, &games).len()) } else { Some(0) };
                if let Ok(mut s) = slot.lock() {
                    *s = Some(n);
                }
                c.request_repaint();
            });
        }
        let n = self.playnite.count;
        if n == 0 || crate::util::now_ms() < self.model.settings.playnite_card_hidden_until_ms {
            return;
        }
        let tk = u::tokens(ctx);
        let big = self.big_now;
        let (mut open, mut hide) = (false, false);
        egui::Frame::new().fill(u::col(tk.card2)).stroke(egui::Stroke::new(1.0_f32, u::col(tk.line))).corner_radius(u::tokens::R_MD).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                u::state_icon(ui, "playnite_card", State::Incoming, 24.0, None);
                ui.vertical(|ui| {
                    let title = self.t(card_key(n, self.model.settings.lang)).replace("{}", &n.to_string());
                    ui.label(RichText::new(title).strong().color(u::col(tk.text)));
                    ui.label(RichText::new(self.t(K::VPlayniteCardBody)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = u::button_with(ui, Btn::Ghost, self.t(K::VPlayniteCardHide), None, big);
                    if self.deco(ui, r, None).clicked() {
                        hide = true;
                    }
                    let r = u::button_with(ui, Btn::Secondary, self.t(K::VPlayniteCardGo), Some(Glyph::Eye), big);
                    if self.deco(ui, r, None).clicked() {
                        open = true;
                    }
                });
            });
        });
        ui.add_space(u::tokens::SP_S);
        if hide {
            self.model.settings.playnite_card_hidden_until_ms = crate::util::now_ms() + crate::settings::PLAYNITE_CARD_HIDE_MS;
            let _ = self.model.save_settings();
        }
        if open {
            self.scan_open(ctx);
            self.scan.scroll_playnite = true;
        }
    }
}
