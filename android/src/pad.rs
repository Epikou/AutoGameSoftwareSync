//! Logique pure de la manette « a axes » (croix « chapeau » AXIS_HAT_X / Y et stick gauche AXIS_X / Y -> fleches),
//! reprise A L'IDENTIQUE dans la copie locale de winit (`vendor/winit/src/platform_impl/android/mod.rs`,
//! `handle_pad_axes`) qui ne compile que sur Android : ici elle est testee sur PC. Garder les deux copies identiques.

/// Seuil de la croix « chapeau » (valeurs -1 / 0 / 1 en pratique).
pub const HAT_THRESHOLD: f32 = 0.5;
/// Zone morte large du stick (un stick au repos legerement decentre ne doit jamais bouger la selection).
pub const STICK_DEAD: f32 = 0.6;

/// Directions tenues [gauche, droite, haut, bas] d'apres les quatre axes.
pub fn dirs_from_axes(hx: f32, hy: f32, sx: f32, sy: f32) -> [bool; 4] {
    [
        hx < -HAT_THRESHOLD || sx < -STICK_DEAD,
        hx > HAT_THRESHOLD || sx > STICK_DEAD,
        hy < -HAT_THRESHOLD || sy < -STICK_DEAD,
        hy > HAT_THRESHOLD || sy > STICK_DEAD,
    ]
}

/// Touches a emettre entre deux etats : (direction, enfoncee).
pub fn changes(prev: [bool; 4], now: [bool; 4]) -> Vec<(usize, bool)> {
    (0..4).filter(|i| prev[*i] != now[*i]).map(|i| (i, now[i])).collect()
}

/// L'evenement est « gere » (Android ne doit pas fabriquer ses propres touches DPAD) des qu'une touche est emise ou
/// qu'une direction reste tenue ; sinon il est rendu au systeme.
pub fn handled(prev: [bool; 4], now: [bool; 4]) -> bool {
    !changes(prev, now).is_empty() || now.iter().any(|d| *d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn croix_chapeau_et_stick_deviennent_des_fleches() {
        assert_eq!(dirs_from_axes(0.0, 0.0, 0.0, 0.0), [false; 4]);
        assert_eq!(dirs_from_axes(-1.0, 0.0, 0.0, 0.0), [true, false, false, false]);
        assert_eq!(dirs_from_axes(1.0, 0.0, 0.0, 0.0), [false, true, false, false]);
        assert_eq!(dirs_from_axes(0.0, -1.0, 0.0, 0.0), [false, false, true, false]);
        assert_eq!(dirs_from_axes(0.0, 1.0, 0.0, 0.0), [false, false, false, true]);
        // Stick : zone morte large, puis la meme direction que la croix.
        assert_eq!(dirs_from_axes(0.0, 0.0, 0.3, -0.5), [false; 4], "stick dans la zone morte");
        assert_eq!(dirs_from_axes(0.0, 0.0, 0.9, 0.0), [false, true, false, false]);
        assert_eq!(dirs_from_axes(0.0, 0.0, 0.0, -0.95), [false, false, true, false]);
        // Diagonale : deux directions.
        assert_eq!(dirs_from_axes(-1.0, 1.0, 0.0, 0.0), [true, false, false, true]);
    }

    #[test]
    fn appui_puis_relachement_une_seule_fois_et_evenement_gere_ou_rendu() {
        let rest = [false; 4];
        let right = dirs_from_axes(1.0, 0.0, 0.0, 0.0);
        assert_eq!(changes(rest, right), vec![(1, true)], "un seul appui Droite");
        assert_eq!(changes(right, right), vec![], "tenir la croix : rien de plus (pas de repetition)");
        assert_eq!(changes(right, rest), vec![(1, false)], "un seul relachement");
        assert!(handled(rest, right));
        assert!(handled(right, right), "direction tenue : toujours gere (jamais de doublon DPAD d'Android)");
        assert!(handled(right, rest), "relachement : gere");
        assert!(!handled(rest, rest), "rien d'emis ni tenu : rendu a Android");
        // Le stick qui tremble dans la zone morte ne produit rien.
        assert!(!handled(rest, dirs_from_axes(0.0, 0.0, 0.2, -0.4)));
    }
}
