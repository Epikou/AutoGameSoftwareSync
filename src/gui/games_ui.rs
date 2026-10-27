//! Elements de l'accueil hors tuiles : ouverture de la fiche d'un element.
use super::*;

impl App {
    pub(super) fn select_game(&mut self, n: &str, ctx: &egui::Context) {
        self.model.selected = Some(n.to_string());
        self.history = None;
        self.link_open = false;
        self.fiche.reset(n);
        self.load_versions(n, ctx);
        self.load_safety(n, ctx);
        let s = self.model.settings.per_game.get(n).cloned().unwrap_or_default();
        self.edit_keep = s.keep_versions.unwrap_or(self.model.ctx.keep_versions);
        let rp = crate::reactive::params(self.model.settings.reactivity, &self.model.settings.reactivity_custom);
        self.edit_stable = s.stable_secs.unwrap_or(rp.stable_ms / 1000);
        self.edit_resync = s.resync_secs.unwrap_or(rp.fallback_ms / 1000);
        self.edit_notify = match s.notify {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        };
        self.edit_sched = self.model.game(n).and_then(|g| g.schedule.clone());
        self.edit_display = self.model.game(n).map(|g| g.shown().to_string()).unwrap_or_default();
        self.edit_exe = s.exe_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.fill_location_fields(n);
        self.edit_for = Some(n.to_string());
    }

    /// Champs des emplacements (reglages avances de la fiche) relus depuis la configuration.
    pub(super) fn fill_location_fields(&mut self, n: &str) {
        self.edit_save = self.model.game(n).map(|g| g.path.display().to_string()).unwrap_or_default();
        self.edit_save_file = self.model.game(n).map(|g| g.save_is_file).unwrap_or(false);
        self.edit_locs = self.model.game(n).map(|g| g.extra_locations.clone()).unwrap_or_default();
        self.edit_primary_label = self.model.game(n).and_then(|g| g.primary_label.clone()).unwrap_or_default();
        self.edit_primary_enabled = self.model.game(n).map(|g| !g.primary_disabled).unwrap_or(true);
        self.history_unknown = crate::config::State::load(&self.model.ctx).ok().and_then(|s| s.unknown_locs.get(n).cloned()).filter(|v| !v.is_empty());
        self.edit_filter = self.model.game(n).and_then(|g| g.save_filter.clone()).unwrap_or_default();
    }

}
