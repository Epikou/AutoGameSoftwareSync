//! Dialogue de confirmation : titre, texte, « Annuler » TOUJOURS present (a gauche de l'action), action principale ou
//! danger a droite. Voile sombre, apparition en fondu + 10 px (back_out). Echap ou clic hors du dialogue = Annuler.
use super::{anim, button_with, col, col_a, layout, tokens, tokens::*, Btn};
use eframe::egui::{self, Sense};
use std::hash::Hash;

/// Contenu du dialogue.
pub struct ConfirmSpec<'a> {
    pub title: &'a str,
    pub body: &'a str,
    /// Libelle de l'action (« Remettre cette version », « Ne plus proteger »).
    pub confirm: &'a str,
    /// Libelle de l'annulation (« Annuler »).
    pub cancel: &'a str,
    /// Action destructrice : bouton rouge.
    pub danger: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogAnswer {
    Confirm,
    Cancel,
}

/// Affiche le dialogue tant qu'il est appele chaque image ; renvoie la reponse quand l'utilisateur choisit.
pub fn confirm_dialog(ctx: &egui::Context, id_salt: impl Hash, spec: &ConfirmSpec) -> Option<DialogAnswer> {
    let t = tokens(ctx);
    let id = egui::Id::new(("agss_dialog", id_salt));
    let (op, dy) = open_anim(ctx, id);
    let mut answer = None;
    let frame = egui::Frame::new()
        .fill(col(t.card))
        .stroke(egui::Stroke::new(1.0_f32, col(if t.dark { [0x47, 0x47, 0x47] } else { t.line })))
        .corner_radius(R_LG)
        .shadow(egui::epaint::Shadow { offset: [0, 14], blur: 34, spread: 0, color: col_a([0, 0, 0], t.shadow_alpha as f32 / 255.0 * 1.5) })
        .inner_margin(egui::Margin::symmetric(20, 18));
    let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, dy));
    let resp = egui::Modal::new(id).area(area).frame(frame).backdrop_color(col_a([0, 0, 0], t.scrim_alpha as f32 / 255.0 * op)).show(ctx, |ui| {
        ui.set_opacity(op);
        let w = DIALOG_W.min(ctx.screen_rect().width() - 48.0).max(220.0);
        ui.set_width(w);
        let g = layout(ui, spec.title, FS_H2, col(t.text), w, 3);
        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        ui.painter().galley(r.min, g, col(t.text));
        ui.add_space(SP_S);
        let g2 = layout(ui, spec.body, FS_BODY, col(t.weak), w, 12);
        let (r2, _) = ui.allocate_exact_size(g2.size(), Sense::hover());
        ui.painter().galley(r2.min, g2, col(t.weak));
        ui.add_space(SP_L + 2.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = button_with(ui, if spec.danger { Btn::Danger } else { Btn::Primary }, spec.confirm, None, false);
            if ok.clicked() {
                answer = Some(DialogAnswer::Confirm);
            }
            if button_with(ui, Btn::Secondary, spec.cancel, None, false).clicked() {
                answer = Some(DialogAnswer::Cancel);
            }
        });
    });
    if answer.is_none() && resp.should_close() {
        answer = Some(DialogAnswer::Cancel);
    }
    answer
}

/// Animation d'ouverture (fondu + 10 px) : rejouee a chaque ouverture (dialogue absent a l'image precedente).
fn open_anim(ctx: &egui::Context, id: egui::Id) -> (f32, f32) {
    let m = anim::motion(ctx);
    let pass = ctx.cumulative_pass_nr();
    let now = anim::now(ctx);
    let t0 = ctx.data_mut(|d| {
        let e = d.get_temp_mut_or_insert_with::<(u64, f64)>(id.with("open"), || (pass, now));
        if e.0 + 1 < pass {
            e.1 = now;
        }
        e.0 = pass;
        e.1
    });
    let p = anim::run(ctx, now - t0, m.dur(DUR_SLOW));
    anim::dialog_in(p, m.reduced)
}

/// Dialogue au contenu libre (selecteur de dossier guide, choix du coffre) : meme habillage, meme voile et meme animation
/// que `confirm_dialog`. `width` : largeur voulue (reduite pour tenir dans la fenetre). Renvoie true quand l'utilisateur
/// ferme (Echap ou clic hors du dialogue).
pub fn custom_dialog(ctx: &egui::Context, id_salt: impl Hash, width: f32, add: impl FnOnce(&mut egui::Ui)) -> bool {
    let t = tokens(ctx);
    let id = egui::Id::new(("agss_dialog", id_salt));
    let (op, dy) = open_anim(ctx, id);
    let frame = egui::Frame::new()
        .fill(col(t.card))
        .stroke(egui::Stroke::new(1.0_f32, col(if t.dark { [0x47, 0x47, 0x47] } else { t.line })))
        .corner_radius(R_LG)
        .shadow(egui::epaint::Shadow { offset: [0, 14], blur: 34, spread: 0, color: col_a([0, 0, 0], t.shadow_alpha as f32 / 255.0 * 1.5) })
        .inner_margin(egui::Margin::symmetric(20, 18));
    let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, dy));
    let resp = egui::Modal::new(id).area(area).frame(frame).backdrop_color(col_a([0, 0, 0], t.scrim_alpha as f32 / 255.0 * op)).show(ctx, |ui| {
        ui.set_opacity(op);
        ui.set_width(width.min(ctx.screen_rect().width() - 48.0).max(220.0));
        add(ui);
    });
    resp.should_close()
}
