//! Jetons de design de l'interface 2.0 (module UNIQUE) : couleurs (theme sombre par defaut, theme clair), espacements,
//! rayons, typographie, tailles et durees. Valeurs tirees de docs/refonte/maquettes.html. Calcul PUR (teste).
use crate::theme::{contrast_ratio, mix, Rgb};

/// Accent par defaut (#028463).
pub const ACCENT: Rgb = [0x02, 0x84, 0x63];

// ------------------------------------------------------------------ espacements (px logiques)
pub const SP_XXS: f32 = 2.0;
pub const SP_XS: f32 = 4.0;
pub const SP_S: f32 = 8.0;
pub const SP_M: f32 = 12.0;
pub const SP_L: f32 = 16.0;
pub const SP_XL: f32 = 24.0;
/// Marge interieure des cartes (vertical, horizontal).
pub const CARD_PAD: (f32, f32) = (14.0, 16.0);

// ------------------------------------------------------------------ rayons
/// Boutons, cases.
pub const R_SM: f32 = 6.0;
/// Champs, lignes de liste, bandeaux secondaires, toasts.
pub const R_MD: f32 = 8.0;
/// Cartes, tuiles, dialogues.
pub const R_LG: f32 = 12.0;

// ------------------------------------------------------------------ typographie (tailles en px)
pub const FS_TITLE: f32 = 26.0;
pub const FS_H2: f32 = 19.0;
pub const FS_H3: f32 = 15.0;
pub const FS_BODY: f32 = 14.0;
pub const FS_SMALL: f32 = 12.0;
pub const FS_BIG_BUTTON: f32 = 16.0;

// ------------------------------------------------------------------ tailles
pub const TILE_W: f32 = 168.0;
/// 148 dans l'etude, + 20 pour la ligne « synchro entre appareils » (lot 6).
pub const TILE_H: f32 = 168.0;
/// Tuile en grand format (manette, Steam Deck).
pub const TILE_W_BIG: f32 = 192.0;
pub const TILE_H_BIG: f32 = 192.0;
pub const TILE_GAP: f32 = 12.0;
pub const TILE_ICON: f32 = 48.0;
pub const STATE_ICON: f32 = 28.0;
pub const TOGGLE_W: f32 = 38.0;
pub const TOGGLE_H: f32 = 22.0;
pub const TOGGLE_KNOB: f32 = 16.0;
/// Remplissage des boutons (vertical, horizontal) : normal et grand.
pub const BTN_PAD: (f32, f32) = (7.0, 12.0);
pub const BTN_PAD_BIG: (f32, f32) = (11.0, 20.0);
pub const TOAST_W: f32 = 330.0;
pub const DIALOG_W: f32 = 520.0;

// ------------------------------------------------------------------ durees (secondes)
/// Survol, appui, interrupteurs (ease_out).
pub const DUR_FAST: f64 = 0.120;
/// Transitions de page, ouverture de bloc (ease_out_cubic).
pub const DUR_NORMAL: f64 = 0.200;
/// Apparition de tuile, coche, toast (back_out).
pub const DUR_SLOW: f64 = 0.320;
/// Decalage de la cascade par element, et nombre maximal d'elements decales.
pub const DUR_CASCADE_STEP: f64 = 0.030;
pub const CASCADE_MAX: usize = 10;
/// Animations d'ambiance (seulement pendant un etat « en cours ») : anneau de copie, pulsation.
pub const DUR_RING: f64 = 1.2;
pub const DUR_PULSE: f64 = 1.6;
/// Secousse d'erreur (une fois), eclat de reussite (une fois).
pub const DUR_SHAKE: f64 = 0.420;
pub const DUR_BURST: f64 = 0.700;
/// Toast : duree d'affichage (barre de temps qui se vide).
pub const DUR_TOAST: f64 = 6.0;
/// Mouvement reduit : toute transition devient un fondu de cette duree.
pub const DUR_REDUCED_FADE: f64 = 0.120;
/// Animations d'ambiance : 30 images par seconde suffisent.
pub const AMBIENT_FRAME: f64 = 1.0 / 30.0;

/// Couleurs d'un theme (sombre ou clair) pour un accent donne.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    /// Fond de la fenetre, panneaux, cartes, cartes secondaires (champs, carte de version), widgets (boutons secondaires).
    pub bg: Rgb,
    pub panel: Rgb,
    pub card: Rgb,
    pub card2: Rgb,
    pub widget: Rgb,
    pub widget_hover: Rgb,
    /// Fond d'une ligne de liste survolee.
    pub row_hover: Rgb,
    /// Bordures fines.
    pub line: Rgb,
    /// Textes : principal, secondaire (contraste >= 4,5:1), discret (>= 3:1, jamais pour une information seule).
    pub text: Rgb,
    pub weak: Rgb,
    pub faint: Rgb,
    pub accent: Rgb,
    pub accent_hover: Rgb,
    /// Fond teinte d'accent (element de navigation actif, choix selectionne) : couleur deja melangee au fond.
    pub accent_soft: Rgb,
    pub on_accent: Rgb,
    /// Etats : a l'abri, information (version recue), a faire, probleme.
    pub ok: Rgb,
    pub blue: Rgb,
    pub orange: Rgb,
    pub red: Rgb,
    /// Texte d'erreur / lien rouge (plus lisible que `red` sur le fond).
    pub red_text: Rgb,
    pub purple: Rgb,
    pub teal: Rgb,
    /// Voile des dialogues (noir, opacite 0..255) et ombre des elements flottants.
    pub scrim_alpha: u8,
    pub shadow_alpha: u8,
}

impl Tokens {
    /// Jetons du theme sombre (defaut) ou clair, pour l'accent `accent` (reglage de l'utilisateur).
    pub fn new(dark: bool, accent: Rgb) -> Tokens {
        if dark {
            Tokens {
                dark,
                bg: [0x14, 0x14, 0x14],
                panel: [0x1b, 0x1b, 0x1b],
                card: [0x24, 0x24, 0x24],
                card2: [0x2b, 0x2b, 0x2b],
                widget: [0x3a, 0x3a, 0x3a],
                widget_hover: [0x47, 0x47, 0x47],
                row_hover: [0x26, 0x26, 0x26],
                line: [0x35, 0x35, 0x35],
                text: [0xeb, 0xeb, 0xeb],
                weak: [0xa9, 0xa9, 0xa9],
                faint: [0x7d, 0x7d, 0x7d],
                accent,
                accent_hover: mix(accent, [255, 255, 255], 0.14),
                accent_soft: mix([0x24, 0x24, 0x24], accent, 0.18),
                on_accent: crate::theme::text_on(accent),
                ok: [0x3c, 0xb4, 0x64],
                blue: [0x4a, 0x90, 0xe2],
                orange: [0xe6, 0xaa, 0x28],
                red: [0xdc, 0x50, 0x50],
                red_text: [0xf0, 0x7a, 0x7a],
                purple: [0x9b, 0x6a, 0xd6],
                teal: [0x1f, 0xb5, 0xc9],
                scrim_alpha: 158,
                shadow_alpha: 90,
            }
        } else {
            Tokens {
                dark,
                bg: [0xee, 0xf0, 0xee],
                panel: [0xf6, 0xf7, 0xf6],
                card: [0xff, 0xff, 0xff],
                card2: [0xf1, 0xf3, 0xf1],
                widget: [0xe2, 0xe5, 0xe2],
                widget_hover: [0xd6, 0xda, 0xd6],
                row_hover: [0xee, 0xf1, 0xee],
                line: [0xd5, 0xd9, 0xd5],
                text: [0x1d, 0x1f, 0x1e],
                weak: [0x54, 0x5a, 0x56],
                faint: [0x80, 0x86, 0x82],
                accent,
                accent_hover: mix(accent, [0, 0, 0], 0.16),
                accent_soft: mix([0xff, 0xff, 0xff], accent, 0.12),
                on_accent: crate::theme::text_on(accent),
                ok: [0x23, 0x86, 0x47],
                blue: [0x2a, 0x6c, 0xc0],
                orange: [0xb0, 0x6c, 0x00],
                red: [0xc8, 0x3a, 0x3a],
                red_text: [0xb4, 0x2c, 0x2c],
                purple: [0x7a, 0x4c, 0xbc],
                teal: [0x0f, 0x83, 0x94],
                scrim_alpha: 110,
                shadow_alpha: 40,
            }
        }
    }

    /// Theme sombre, accent par defaut.
    pub fn default_dark() -> Tokens {
        Tokens::new(true, ACCENT)
    }

    /// Couleur d'un etat (pastille, anneau, texte d'etat).
    pub fn state(&self, s: crate::gui::ui::State) -> Rgb {
        use crate::gui::ui::State;
        match s {
            State::Safe => self.ok,
            State::Busy => self.accent_hover,
            State::Todo => self.orange,
            State::Problem => self.red,
            State::Incoming => self.blue,
            State::Paused => self.faint,
        }
    }

    /// Texte d'etat lisible sur une carte (les couleurs vives sont eclaircies / assombries pour garder 4,5:1).
    pub fn state_text(&self, s: crate::gui::ui::State) -> Rgb {
        let base = self.state(s);
        let target = if self.dark { [255, 255, 255] } else { [0, 0, 0] };
        let mut c = base;
        let mut t = 0.0;
        while contrast_ratio(c, self.card) < crate::theme::TEXT_MIN_CONTRAST && t < 1.0 {
            t += 0.05;
            c = mix(base, target, t);
        }
        c
    }

    /// Fond assombri a l'appui (6 %).
    pub fn pressed(&self, c: Rgb) -> Rgb {
        mix(c, [0, 0, 0], 0.06)
    }

    /// Couleurs proposees pour les appareils (ordre stable).
    pub fn device_palette(&self) -> [Rgb; 6] {
        [self.teal, self.purple, self.blue, self.orange, self.ok, self.red]
    }
}

/// Taille d'une tuile selon le grand format.
pub fn tile_size(big: bool) -> (f32, f32) {
    if big {
        (TILE_W_BIG, TILE_H_BIG)
    } else {
        (TILE_W, TILE_H)
    }
}

/// Nombre de colonnes de la grille de tuiles pour une largeur disponible (au moins 1).
pub fn grid_columns(width: f32, big: bool) -> usize {
    let (w, _) = tile_size(big);
    (((width + TILE_GAP) / (w + TILE_GAP)).floor() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrastes_des_deux_themes() {
        for dark in [true, false] {
            let t = Tokens::new(dark, ACCENT);
            for bg in [t.panel, t.card, t.card2] {
                assert!(contrast_ratio(t.text, bg) >= 7.0, "texte principal ({})", dark);
                assert!(contrast_ratio(t.weak, bg) >= 4.5, "texte secondaire ({})", dark);
                assert!(contrast_ratio(t.faint, bg) >= 2.9, "texte discret ({})", dark);
            }
            assert!(contrast_ratio(t.on_accent, t.accent) >= 4.5);
            assert!(contrast_ratio(t.red_text, t.card) >= 4.5, "lien rouge ({})", dark);
        }
    }

    #[test]
    fn grille_et_tailles() {
        assert_eq!(grid_columns(168.0, false), 1);
        assert_eq!(grid_columns(168.0 * 3.0 + 24.0, false), 3);
        assert_eq!(grid_columns(10.0, true), 1);
        assert_eq!(tile_size(false), (168.0, 168.0), "148 + la ligne des appareils (lot 6)");
    }
}
