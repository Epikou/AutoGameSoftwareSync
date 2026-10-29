//! Bibliotheque de composants de l'interface 2.0 (docs/refonte/AVANCEMENT.md, lot 5 : API et exemples).
//!
//! Principe : chaque composant est une fonction libre `f(ui, ...) -> Response` (aucun acces a `App`), qui lit les jetons
//! du theme et les preferences de mouvement installes une fois par image (`install`), et signale ses besoins de redessin
//! a l'horloge centrale (`anim`). Tout est dessine au painter (aucun emoji, aucun glyphe absent de la police).
use eframe::egui::{self, Color32, FontId};
use std::sync::Arc;

pub mod anim;
pub mod button;
pub mod card;
pub mod dialog;
pub mod field;
pub mod glyphs;
pub mod icons;
pub mod list;
pub mod page;
pub mod pill;
pub mod status_banner;
pub mod tile;
pub mod timeline;
pub mod toast;
pub mod toggle;
pub mod tokens;

pub use anim::Motion;
pub use button::{button, button_sized, button_with, Btn};
pub use card::card;
pub use dialog::{confirm_dialog, custom_dialog, ConfirmSpec, DialogAnswer};
pub use field::field;
pub use glyphs::{paint_glyph, paint_state, Glyph};
pub use list::{list_row, ListRow};
pub use page::{cascade_in, page_transition, step_transition};
pub use pill::{chip, device_pill, state_icon, state_pill};
pub use status_banner::{status_banner, BannerSpec};
pub use tile::{add_tile, add_tile_sized, choice_height, choice_tile, tile, ChoiceSpec, TileIcon, TileResponse, TileSpec};
pub use timeline::{timeline, Mark};
pub use toast::{Toast, ToastAction, Toasts};
pub use toggle::toggle;
pub use tokens::Tokens;

/// Etat d'un element protege (pastilles, tuiles, bandeau).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    /// A l'abri : coche tracee une fois puis immobile.
    Safe,
    /// En cours (copie) : anneau qui tourne (seule animation en boucle, 30 i/s).
    Busy,
    /// A faire : pulsation (3 fois puis immobile) et petit rebond a l'apparition.
    Todo,
    /// Probleme : secousse horizontale une fois.
    Problem,
    /// Nouvelle version recue d'un autre appareil : fleche qui descend une fois.
    Incoming,
    /// En pause (programme en cours d'utilisation) : point qui pulse 3 fois.
    Paused,
}

impl State {
    pub const ALL: [State; 6] = [State::Safe, State::Busy, State::Todo, State::Problem, State::Incoming, State::Paused];

    /// Cle stable (memoire d'animation : l'animation d'entree rejoue quand l'etat change).
    pub fn code(self) -> u64 {
        self as u64 + 1
    }

    /// Priorite de tri de l'accueil (plus petit = en tete) : a verifier > en cours > le reste.
    pub fn sort_rank(self) -> u8 {
        match self {
            State::Problem => 0,
            State::Todo => 1,
            State::Busy => 2,
            State::Incoming => 3,
            State::Paused => 4,
            State::Safe => 5,
        }
    }
}

/// Theme installe pour l'image courante.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTheme {
    pub tokens: Tokens,
    /// Grand format (manette, Steam Deck) : tuiles et boutons plus grands.
    pub big: bool,
}

fn theme_id() -> egui::Id {
    egui::Id::new("agss_ui_theme")
}

/// A appeler une fois par image, avant tout composant : jetons du theme et preferences de mouvement.
pub fn install(ctx: &egui::Context, tokens: Tokens, big: bool, motion: Motion) {
    ctx.data_mut(|d| d.insert_temp(theme_id(), UiTheme { tokens, big }));
    anim::set_motion(ctx, motion);
}

pub fn theme(ctx: &egui::Context) -> UiTheme {
    ctx.data(|d| d.get_temp::<UiTheme>(theme_id())).unwrap_or(UiTheme { tokens: Tokens::default_dark(), big: false })
}

/// Textures des icones de programmes (2.0.3) : lissage + mipmaps (nettes une fois reduites, en grand format comme en petit).
pub const ICON_TEX: egui::TextureOptions = egui::TextureOptions { magnification: egui::TextureFilter::Linear, minification: egui::TextureFilter::Linear, wrap_mode: egui::TextureWrapMode::ClampToEdge, mipmap_mode: Some(egui::TextureFilter::Linear) };

pub fn tokens(ctx: &egui::Context) -> Tokens {
    theme(ctx).tokens
}

/// Couleur egui d'un jeton.
pub fn col(c: crate::theme::Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Couleur d'un jeton avec une opacite 0..1.
pub fn col_a(c: crate::theme::Rgb, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Mise en page d'un texte : largeur maximale, nombre de lignes maximal (au-dela : « … »).
pub fn layout(ui: &egui::Ui, text: &str, size: f32, color: Color32, max_w: f32, max_rows: usize) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat::simple(font(size), color));
    job.wrap = egui::text::TextWrapping { max_width: max_w, max_rows, break_anywhere: false, overflow_character: Some('\u{2026}') };
    ui.fonts(|f| f.layout_job(job))
}

/// Texte d'une ligne (pas de retour).
pub fn layout_line(ui: &egui::Ui, text: &str, size: f32, color: Color32) -> Arc<egui::Galley> {
    ui.painter().layout_no_wrap(text.to_string(), font(size), color)
}

/// Cerclage de focus clavier / manette (2 px, accent) autour de `r`.
pub fn focus_ring(ui: &egui::Ui, r: egui::Rect, radius: f32) {
    let t = tokens(ui.ctx());
    ui.painter().rect_stroke(r.expand(2.0), radius + 2.0, egui::Stroke::new(2.0_f32, col(t.accent_hover)), egui::StrokeKind::Outside);
}
