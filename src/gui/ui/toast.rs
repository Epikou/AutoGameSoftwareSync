//! Toasts : message bref en bas a droite (« C'est fait. » + « Annuler »), arrivee par le bas (16 px) avec un leger
//! depassement, barre de temps qui se vide en 6 s. File PURE (testee) + affichage egui.
use super::{anim, button_with, col, col_a, layout, paint_state, tokens, tokens::*, Btn, State};
use eframe::egui::{self, Align2, Sense, Ui};

/// Un toast.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    /// Texte principal (« C'est fait. ») et detail (« La version d'hier 21:14 est remise. »).
    pub title: String,
    pub detail: String,
    pub state: State,
    /// Libelle du bouton d'annulation (aucun bouton si None).
    pub undo: Option<String>,
    pub born: f64,
    pub dur: f64,
}

/// Action de l'utilisateur sur un toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastAction {
    Undo(u64),
}

/// File de toasts (le plus recent est affiche ; un nouveau remplace l'ancien a l'ecran).
#[derive(Clone, Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next: u64,
}

impl Toasts {
    /// Ajoute un toast a l'instant `now` ; renvoie son identifiant (pour relier « Annuler » a l'action).
    pub fn push(&mut self, now: f64, title: &str, detail: &str, state: State, undo: Option<&str>) -> u64 {
        self.next += 1;
        self.items.push(Toast { id: self.next, title: title.to_string(), detail: detail.to_string(), state, undo: undo.map(|s| s.to_string()), born: now, dur: DUR_TOAST });
        // au plus 3 en memoire : les plus anciens ne s'afficheront plus
        if self.items.len() > 3 {
            self.items.remove(0);
        }
        self.next
    }

    /// Retire les toasts expires a l'instant `now`.
    pub fn expire(&mut self, now: f64) {
        self.items.retain(|t| now < t.born + t.dur);
    }

    pub fn dismiss(&mut self, id: u64) {
        self.items.retain(|t| t.id != id);
    }

    /// Toast affiche (le plus recent).
    pub fn current(&self) -> Option<&Toast> {
        self.items.last()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Fraction de temps restante du toast affiche (barre de temps), 0..1.
    pub fn remaining(&self, now: f64) -> f32 {
        self.current().map(|t| (1.0 - (now - t.born) / t.dur).clamp(0.0, 1.0) as f32).unwrap_or(0.0)
    }

    /// Affiche le toast courant ; renvoie l'action « Annuler » si elle est cliquee (le toast disparait alors).
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ToastAction> {
        let now = anim::now(ctx);
        self.expire(now);
        let toast = self.current()?.clone();
        let t = tokens(ctx);
        let m = anim::motion(ctx);
        let p = anim::run(ctx, now - toast.born, m.dur(DUR_SLOW));
        let (op, dy) = anim::toast_in(p, m.reduced);
        // fin du toast : un redessin a l'expiration ; barre de temps a 30 i/s (rien en mouvement reduit)
        anim::wake_at(ctx, toast.born + toast.dur);
        if m.ambient_on() {
            anim::need_ambient(ctx);
        }
        let rem = self.remaining(now);
        let mut action = None;
        egui::Area::new(egui::Id::new(("agss_toast", toast.id)))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -16.0 + dy))
            .interactable(true)
            .show(ctx, |ui: &mut Ui| {
                ui.set_opacity(op);
                let bg = if t.dark { [0x2f, 0x2f, 0x2f] } else { t.card };
                let frame = egui::Frame::new()
                    .fill(col(bg))
                    .stroke(egui::Stroke::new(1.0_f32, col(if t.dark { [0x44, 0x44, 0x44] } else { t.line })))
                    .corner_radius(R_MD)
                    .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 20, spread: 0, color: col_a([0, 0, 0], t.shadow_alpha as f32 / 255.0 * 1.3) })
                    .inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 14 });
                let inner = frame.show(ui, |ui| {
                    ui.set_width(TOAST_W - 28.0);
                    ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
                        paint_state(ui.painter(), ir, toast.state, now - toast.born, None);
                        let btn_w = toast.undo.as_ref().map(|u| super::layout_line(ui, u, FS_BODY, col(t.text)).size().x + 2.0 * BTN_PAD.1 + SP_S).unwrap_or(0.0);
                        let w = (ui.available_width() - btn_w).max(60.0);
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            let g = layout(ui, &toast.title, FS_BODY, col(t.text), w, 2);
                            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                            ui.painter().galley(r.min, g, col(t.text));
                            if !toast.detail.is_empty() {
                                let g2 = layout(ui, &toast.detail, FS_SMALL + 1.0, col(t.weak), w, 3);
                                let (r2, _) = ui.allocate_exact_size(g2.size(), Sense::hover());
                                ui.painter().galley(r2.min, g2, col(t.weak));
                            }
                        });
                        if let Some(u) = &toast.undo {
                            if button_with(ui, Btn::Ghost, u, None, false).clicked() {
                                action = Some(ToastAction::Undo(toast.id));
                            }
                        }
                    });
                });
                // barre de temps (3 px) au bas du toast
                let r = inner.response.rect;
                let bar = egui::Rect::from_min_size(egui::pos2(r.left() + 1.0, r.bottom() - 4.0), egui::vec2((r.width() - 2.0) * rem, 3.0));
                ui.painter().rect_filled(bar, egui::CornerRadius { nw: 0, ne: 0, sw: R_MD as u8, se: if rem >= 0.99 { R_MD as u8 } else { 0 } }, col(t.accent));
            });
        if let Some(ToastAction::Undo(id)) = action {
            self.dismiss(id);
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_de_toasts() {
        let mut q = Toasts::default();
        assert!(q.current().is_none() && q.remaining(0.0) == 0.0);
        let a = q.push(10.0, "C'est fait.", "", State::Safe, Some("Annuler"));
        assert_eq!(q.current().unwrap().id, a);
        assert!((q.remaining(13.0) - 0.5).abs() < 1e-6, "barre a moitie apres 3 s");
        let b = q.push(11.0, "Copie faite", "", State::Safe, None);
        assert_eq!(q.current().unwrap().id, b, "le plus recent s'affiche");
        q.expire(16.5);
        assert_eq!(q.current().unwrap().id, b, "a expire (10 + 6), b reste");
        q.dismiss(b);
        assert!(q.is_empty());
        for i in 0..5 {
            q.push(20.0 + i as f64, "x", "", State::Safe, None);
        }
        q.expire(20.0);
        assert_eq!(q.items.len(), 3, "au plus 3 en memoire");
    }
}
