//! Horloge de l'application : installe les jetons du theme et les preferences de mouvement pour les composants (`ui`),
//! puis decide a la fin de l'image s'il faut redessiner (voir `ui::anim::AnimClock`). Au repos, aucun redessin
//! d'animation ; seuls les sondages espaces de l'application (dossier partage, etats, theme du systeme) reveillent
//! l'interface, et les threads de fond la reveillent quand un etat visible change.
use super::*;
use crate::gui::ui::anim::Repaint;

/// Sondage pendant une activite (operation, chargement, manette, tutoriel) : comme avant la 2.0.
pub const POLL_BUSY: Duration = Duration::from_millis(250);
/// Plafond du sommeil au repos (filet de securite) et plancher entre deux sondages.
pub const POLL_IDLE_MAX: Duration = Duration::from_secs(10);
pub const POLL_MIN: Duration = Duration::from_millis(250);
/// Cadence d une transition (secondes par image).
pub const TRANSITION_FRAME: f64 = 1.0 / 60.0;

/// Delai avant la prochaine echeance periodique : `period - ecoule`, borne. PUR.
pub fn until(period: Duration, elapsed: Duration) -> Duration {
    period.saturating_sub(elapsed).clamp(POLL_MIN, POLL_IDLE_MAX)
}

impl App {
    /// Debut d'image (apres `apply_theme`) : jetons, grand format et mouvement installes pour les composants.
    pub(super) fn clock_begin(&mut self, ctx: &egui::Context) {
        let sys_on = winutil::client_animations_enabled();
        let mut motion = ui::Motion::from_settings(self.model.settings.card_anim, sys_on, self.slowmo);
        if self.gallery_motion_reduced() {
            motion.reduced = true;
        }
        self.clock.motion = motion;
        self.clock.hidden = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.clock.begin_frame();
        let dark = self.theme_key.map(|k| k.0).unwrap_or(true);
        ui::install(ctx, ui::Tokens::new(dark, self.model.settings.accent), self.big_now, motion);
    }

    /// Fin d'image : besoins des animations + sondage de l'application -> redessin immediat, differe ou aucun.
    pub(super) fn clock_end(&mut self, ctx: &egui::Context) {
        if let Some(s) = self.shot.as_mut() {
            if s.tick(ctx) {
                self.shared.quit();
            }
        }
        self.clock.merge(ui::anim::take_needs(ctx));
        let now = ui::anim::now(ctx);
        let (poll, why) = self.poll_interval();
        let r = self.clock.decide(now).with_poll(Some(poll));
        if let Some((t0, f0, a0, p0)) = self.fps_mark {
            if now - t0 >= 10.0 {
                let (f, a, p) = (self.clock.frames - f0, self.clock.anim_frames - a0, ctx.cumulative_pass_nr() - p0);
                let causes: Vec<String> = ctx.repaint_causes().iter().map(|c| format!("{}:{} {}", c.file, c.line, c.reason)).collect();
                crate::perf::log(&self.model.ctx, &format!("[UI] {} image(s) en {:.1} s, dont {} d'animation ({} passes egui) ; decision {:?} (sondage : {}) ; causes {:?} ; image la plus longue {} ms, {} image(s) lente(s)", f, now - t0, a, p, r, why, causes, frame_guard::MAX_MS.load(Ordering::SeqCst), frame_guard::SLOW_COUNT.load(Ordering::SeqCst)));
                self.fps_mark = Some((now, self.clock.frames, self.clock.anim_frames, ctx.cumulative_pass_nr()));
            }
        }
        // egui retranche la duree prevue d'une image (`predicted_dt`, 1/60 s) du delai demande : on la rajoute, sinon
        // l'ambiance a « 30 i/s » tournerait a 60 i/s.
        let dt = Duration::from_secs_f32(ctx.input(|i| i.predicted_dt).clamp(0.0, 0.1));
        match r {
            // Transition : 60 i/s cadencees par un minuteur plutot qu'un redessin immediat en continu (mesure : 8 % d'un
            // coeur au lieu de 31 %, le pilote graphique attendant la synchronisation verticale en boucle active).
            Repaint::Now => ctx.request_repaint_after(Duration::from_secs_f64(TRANSITION_FRAME) + dt),
            Repaint::After(d) => ctx.request_repaint_after(d + dt),
            Repaint::Idle => {}
        }
    }

    /// Prochain sondage necessaire et sa raison : rapide pendant une activite, sinon a la prochaine echeance periodique.
    fn poll_interval(&self) -> (Duration, &'static str) {
        let busy = [
            (self.op_progress.is_some() || crate::perf::ACTIVE_OPS.load(Ordering::SeqCst) > 0, "operation"),
            (self.status_inflight, "chargement"),
            (self.pad_mode || self.pad_prompt.is_some(), "manette"),
            (self.wizard.active && self.gallery.is_none(), "premier lancement"),
            (self.prop_exit.is_some(), "carte de detection"),
            (self.frame_no < 10, "demarrage"),
        ];
        if let Some((_, why)) = busy.iter().find(|(b, _)| *b) {
            return (POLL_BUSY, why);
        }
        if self.gallery.is_some() {
            // galerie : aucun sondage de l'application (sonde, etats et journal ne sont pas dessines)
            return (POLL_IDLE_MAX, "galerie");
        }
        let mut next = (until(Duration::from_secs(8), self.dest_probe_at.elapsed()), "dossier partage");
        let mut take = |d: Duration, why: &'static str| {
            if d < next.0 {
                next = (d, why);
            }
        };
        if self.model.page == Page::Games {
            take(until(Duration::from_secs(10), self.last_status.elapsed()), "etats");
        }
        if self.model.settings.theme == ThemeMode::System {
            take(until(Duration::from_secs(15), self.last_theme_probe.elapsed()), "theme du systeme");
        }
        if self.model.page == Page::Settings && self.log_open {
            take(until(Duration::from_secs(3), self.last_log.elapsed()), "journal");
        }
        if !self.said.is_empty() {
            take(Duration::ZERO, "message");
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echeances_bornees() {
        assert_eq!(until(Duration::from_secs(8), Duration::ZERO), Duration::from_secs(8));
        assert_eq!(until(Duration::from_secs(8), Duration::from_secs(20)), POLL_MIN, "echeance depassee : bientot");
        assert_eq!(until(Duration::from_secs(60), Duration::ZERO), POLL_IDLE_MAX);
    }
}
