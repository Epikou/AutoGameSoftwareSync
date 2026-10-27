//! Transitions : page (fondu + glissement de 8 px vers le haut, 200 ms), etape (glissement horizontal de 24 px),
//! apparition en cascade d'un bloc quelconque. La structure des `Ui` est identique pendant et apres l'animation
//! (identifiants stables : focus et etats des widgets conserves).
use super::{anim, tokens::*};
use eframe::egui::{self, Ui};
use std::hash::Hash;

/// Contenu decale de `off` et rendu avec l'opacite `op`.
fn offset_scope<R>(ui: &mut Ui, off: egui::Vec2, op: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = ui.available_rect_before_wrap().translate(off);
    let layout = *ui.layout();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
        if op < 1.0 {
            ui.multiply_opacity(op);
        }
        add(ui)
    })
    .inner
}

/// Page : quand `key` change (nouvel ecran, nouvel onglet), le contenu apparait en fondu et remonte de 8 px.
pub fn page_transition<R>(ui: &mut Ui, id_salt: impl Hash, key: u64, add: impl FnOnce(&mut Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    let m = anim::motion(&ctx);
    let age = anim::age_of(&ctx, ui.id().with(("page", id_salt)), key);
    let p = anim::run(&ctx, age, m.dur(DUR_NORMAL));
    let (op, dy) = anim::page_in(p, m.reduced);
    offset_scope(ui, egui::vec2(0.0, dy), op, add)
}

/// Etape d'un assistant : glissement horizontal de 24 px dans le sens `dir` (+1 = suivant, -1 = retour) quand `key` change.
pub fn step_transition<R>(ui: &mut Ui, id_salt: impl Hash, key: u64, dir: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    let m = anim::motion(&ctx);
    let age = anim::age_of(&ctx, ui.id().with(("step", id_salt)), key);
    let p = anim::run(&ctx, age, m.dur(0.26));
    let (op, dx) = anim::step_in(p, dir, m.reduced);
    offset_scope(ui, egui::vec2(dx, 0.0), op, add)
}

/// Bloc qui apparait en cascade (rang `index`, rejoue quand `epoch` change) : fondu + 6 px.
pub fn cascade_in<R>(ui: &mut Ui, id_salt: impl Hash, index: usize, epoch: u64, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.id().with(("cascade", id_salt));
    let (op, dy) = super::tile::appear(ui, id, index, epoch);
    offset_scope(ui, egui::vec2(0.0, dy), op, add)
}
