//! Dossier partage absent ou invalide : bandeau en haut de chaque onglet, bouton qui amene a la carte des Reglages.
use super::*;
use crate::sharedfolder::{self, Banner, SharedState};

impl App {
    /// Recalcule l'etat du dossier partage (a chaque image : tres leger).
    pub(super) fn update_share_state(&mut self) {
        let now = crate::util::now_ms();
        let reach = match self.shared.dest_reach.load(Ordering::SeqCst) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        };
        if reach == Some(false) {
            self.reach_bad_since.get_or_insert(now);
        } else {
            self.reach_bad_since = None;
        }
        self.share_state = sharedfolder::state(self.model.dest().as_deref(), reach, self.reach_bad_since, now);
    }

    /// Ouvre l ecran de choix du coffre (lot 8) : tuiles des dossiers de synchronisation trouves sur ce PC.
    pub(super) fn go_choose_share(&mut self, ctx: &egui::Context) {
        self.vault_open(ctx);
    }

    /// Bandeau d'attention (couleur douce, ton amical) : meme style de carte que le reste ; absent pendant le tutoriel.
    pub(super) fn share_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(b) = sharedfolder::banner(self.share_state, self.wizard.active) else { return };
        // Bandeau d'etat 2.0 (meme composant que l'accueil), vocabulaire du coffre.
        let (state, title, detail) = if b == Banner::NotConfigured {
            (crate::gui::ui::State::Todo, self.t(K::VNoVault), self.t(K::VNoVaultDetail))
        } else {
            (crate::gui::ui::State::Problem, self.t(K::VVaultUnreachable), self.t(K::VVaultUnreachableDetail))
        };
        let spec = crate::gui::ui::BannerSpec { state, title, detail, action: Some(self.t(K::VChooseVault)) };
        let go = crate::gui::ui::status_banner(ui, &spec).map(|r| self.deco(ui, r, Some(K::TipChooseShare)).clicked()).unwrap_or(false);
        ui.add_space(crate::gui::ui::tokens::SP_S);
        if go {
            self.go_choose_share(ctx);
        }
    }

    pub(super) fn share_not_configured(&self) -> bool {
        self.share_state == SharedState::NotConfigured
    }
}
