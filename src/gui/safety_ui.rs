//! 2.0.3 : place utilisee par les copies de securite locales (carte « Appareils » des Reglages) et « Libérer de la place »,
//! qui montre ce qui sera supprime AVANT de supprimer (regle prudente de `backup::safety_prune_plan`). Lectures et
//! suppressions dans des fils de fond : jamais d'acces disque long dans l'interface.
use super::*;
use crate::gui::ui::{self as u, Btn, Glyph};

#[derive(Default)]
pub struct SafetyUi {
    /// Dernier etat lu (place, copies, plan de nettoyage).
    usage: Option<crate::backup::SafetyUsage>,
    slot: Arc<Mutex<Option<crate::backup::SafetyUsage>>>,
    loading: bool,
    read_at: Option<Instant>,
    /// Confirmation ouverte (liste de ce qui sera supprime).
    ask: bool,
    freeing: Option<Arc<Mutex<Option<(usize, u64)>>>>,
    /// Diagnostic des captures : `AGSS_UI_SAFETY_DEMO=ask` ouvre la confirmation une fois l'etat lu.
    demo_ask: Option<bool>,
}

impl App {
    fn safety_refresh(&mut self, ctx: &egui::Context) {
        if self.safety_ui.loading {
            return;
        }
        self.safety_ui.loading = true;
        let (slot, dd, c) = (self.safety_ui.slot.clone(), self.shared.data_dir.clone(), ctx.clone());
        std::thread::spawn(move || {
            let u = crate::backup::safety_usage(&make_ctx(&dd), crate::util::now_ms());
            if let Ok(mut s) = slot.lock() {
                *s = Some(u);
            }
            c.request_repaint();
        });
    }

    /// Bloc « Copies de sécurité sur cet appareil » (carte Appareils, mode simple et expert).
    pub(super) fn safety_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(u2) = self.safety_ui.slot.lock().ok().and_then(|mut s| s.take()) {
            self.safety_ui.usage = Some(u2);
            self.safety_ui.loading = false;
            self.safety_ui.read_at = Some(Instant::now());
        }
        // Lu a l'ouverture de la page et apres « Liberer de la place » (`read_at` remis a None), jamais en boucle.
        if self.safety_ui.read_at.is_none() {
            self.safety_refresh(ctx);
        }
        if let Some(f) = self.safety_ui.freeing.clone() {
            if let Some((n, b)) = f.lock().ok().and_then(|mut g| g.take()) {
                self.safety_ui.freeing = None;
                self.safety_ui.read_at = None;
                self.toasts.push(u::anim::now(ctx), &self.t(K::VSafetyFreed).replace("{}", &self.bytes(b)), "", u::State::Safe, None);
                self.pad_log(&format!("[Copies de securite] {} copie(s) supprimee(s), {} octets liberes", n, b));
            } else {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
        }
        let tk = u::tokens(ui.ctx());
        ui.add_space(u::tokens::SP_M);
        ui.label(RichText::new(self.t(K::VSafetyTitle)).strong().color(u::col(tk.text)));
        let Some(us) = self.safety_ui.usage.clone() else {
            ui.label(RichText::new(self.t(K::VScanMeasuring)).size(u::tokens::FS_SMALL).color(u::col(tk.weak)));
            return;
        };
        let used = if us.copies == 1 { self.t(K::VSafetyUsedOne).replace("{0}", &self.bytes(us.bytes)) } else { self.t(K::VSafetyUsed).replace("{0}", &self.bytes(us.bytes)).replace("{1}", &us.copies.to_string()) };
        ui.label(RichText::new(used).color(u::col(tk.weak)));
        ui.label(RichText::new(self.t(K::VSafetyRule)).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
        ui.add_space(u::tokens::SP_XS);
        ui.horizontal_wrapped(|ui| {
            let busy = self.safety_ui.freeing.is_some();
            let r = ui.add_enabled_ui(!busy, |ui| u::button_with(ui, Btn::Secondary, self.t(K::VSafetyFree), Some(Glyph::Folder), false)).inner;
            if self.deco(ui, r, None).clicked() {
                self.safety_ui.ask = true;
            }
            if us.plan.is_empty() {
                ui.label(RichText::new(self.t(K::VSafetyNothing)).size(u::tokens::FS_SMALL).color(u::col(tk.faint)));
            }
        });
        if self.safety_ui.demo_ask.is_none() {
            self.safety_ui.demo_ask = Some(std::env::var("AGSS_UI_SAFETY_DEMO").map(|v| v == "ask").unwrap_or(false));
        }
        if self.safety_ui.demo_ask == Some(true) {
            self.safety_ui.demo_ask = Some(false);
            self.safety_ui.ask = true;
        }
        if self.safety_ui.ask {
            self.safety_ask(ctx, &us);
        }
    }

    /// Confirmation : liste de ce qui sera supprime (element, date, taille), puis suppression dans un fil de fond.
    fn safety_ask(&mut self, ctx: &egui::Context, us: &crate::backup::SafetyUsage) {
        let lang = self.model.settings.lang;
        if us.plan.is_empty() {
            let spec = u::ConfirmSpec { title: self.t(K::VSafetyFree), body: self.t(K::VSafetyNothing), confirm: self.t(K::BtnClose), cancel: self.t(K::BtnCancel), danger: false };
            if u::confirm_dialog(ctx, "safety_none", &spec).is_some() {
                self.safety_ui.ask = false;
            }
            return;
        }
        let mut lines: Vec<String> = us
            .plan
            .iter()
            .take(12)
            .map(|(item, c)| {
                let when = if c.ms > 0 { crate::timefmt::format_now_tz(c.ms, lang) } else { "—".into() };
                format!("• {} · {} · {}", item, when, self.bytes(c.bytes))
            })
            .collect();
        if us.plan.len() > 12 {
            lines.push(format!("… +{}", us.plan.len() - 12));
        }
        let title = self.t(K::VSafetyAskTitle).replace("{}", &us.plan.len().to_string());
        let body = format!("{}\n\n{}", self.t(K::VSafetyAskBody).replace("{}", &self.bytes(us.plan_bytes())), lines.join("\n"));
        let spec = u::ConfirmSpec { title: &title, body: &body, confirm: self.t(K::VSafetyAskDo), cancel: self.t(K::BtnCancel), danger: true };
        match u::confirm_dialog(ctx, "safety_free", &spec) {
            Some(u::DialogAnswer::Confirm) => {
                self.safety_ui.ask = false;
                let slot: Arc<Mutex<Option<(usize, u64)>>> = Arc::new(Mutex::new(None));
                self.safety_ui.freeing = Some(slot.clone());
                let (dd, c) = (self.shared.data_dir.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let r = crate::backup::safety_free(&make_ctx(&dd), crate::util::now_ms());
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(r);
                    }
                    c.request_repaint();
                });
            }
            Some(u::DialogAnswer::Cancel) => self.safety_ui.ask = false,
            None => {}
        }
    }
}
