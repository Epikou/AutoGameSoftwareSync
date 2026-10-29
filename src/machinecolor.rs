//! Couleur STABLE par machine (nom du PC) : palette a fort contraste, choisie par hachage du nom, deux machines
//! differentes recoivent des couleurs differentes ; modifiable par l'utilisateur (reglages, persiste).
use std::collections::BTreeMap;

/// Palette (16 teintes) : distinctes entre elles (delta E CIE76 minimal teste) et lisibles avec un texte noir OU blanc
/// (le meilleur des deux est choisi par `text_on`, rapport de contraste >= 4,5 : AA).
pub const PALETTE: [[u8; 3]; 16] = [
    [0, 114, 178],   // bleu
    [213, 94, 0],    // vermillon
    [0, 158, 115],   // vert
    [204, 121, 167], // rose
    [240, 228, 66],  // jaune
    [86, 180, 233],  // bleu ciel
    [148, 103, 189], // violet
    [188, 34, 50],   // rouge
    [23, 190, 207],  // cyan
    [140, 86, 75],   // brun
    [166, 206, 57],  // vert clair
    [250, 150, 30],  // orange
    [60, 60, 140],   // indigo
    [30, 100, 60],   // vert fonce
    [235, 235, 235], // blanc casse
    [120, 120, 120], // gris
];

/// FNV-1a 64 bits (stable d'une version a l'autre, contrairement a `DefaultHasher`).
pub fn hash_name(name: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in name.trim().to_lowercase().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// Couleur par defaut d'un nom seul (indice de palette = hachage).
pub fn default_color(name: &str) -> [u8; 3] {
    PALETTE[(hash_name(name) % PALETTE.len() as u64) as usize]
}

/// Couleurs de TOUTES les machines connues : defaut par hachage, collisions resolues par sondage (ordre = hachage puis
/// nom : deterministe), puis choix de l'utilisateur par-dessus.
pub fn assign(names: &[String], overrides: &BTreeMap<String, [u8; 3]>) -> BTreeMap<String, [u8; 3]> {
    let mut order: Vec<&String> = names.iter().collect();
    order.sort_by_key(|n| (hash_name(n), n.to_lowercase()));
    order.dedup_by_key(|n| n.to_lowercase());
    let mut used: Vec<usize> = Vec::new();
    let mut out = BTreeMap::new();
    for n in order {
        if let Some(c) = overrides.get(n.as_str()) {
            out.insert(n.clone(), *c);
            continue;
        }
        let mut i = (hash_name(n) % PALETTE.len() as u64) as usize;
        if used.len() < PALETTE.len() {
            while used.contains(&i) {
                i = (i + 1) % PALETTE.len();
            }
            used.push(i);
        }
        out.insert(n.clone(), PALETTE[i]);
    }
    out
}

/// Couleur d'une machine : choix de l'utilisateur, sinon defaut par hachage (sans connaitre les autres machines).
pub fn color_of(name: &str, overrides: &BTreeMap<String, [u8; 3]>) -> [u8; 3] {
    overrides.get(name).copied().unwrap_or_else(|| default_color(name))
}

fn lin(c: u8) -> f64 {
    let v = c as f64 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Luminance relative (WCAG).
pub fn luminance(c: [u8; 3]) -> f64 {
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// Rapport de contraste WCAG entre deux couleurs.
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Texte lisible sur cette couleur : noir ou blanc, au meilleur contraste.
pub fn text_on(c: [u8; 3]) -> [u8; 3] {
    if contrast(c, [0, 0, 0]) >= contrast(c, [255, 255, 255]) {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    }
}

/// Ecart de couleur perceptuel (CIE76, espace Lab).
pub fn delta_e(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (lab(a), lab(b));
    ((la.0 - lb.0).powi(2) + (la.1 - lb.1).powi(2) + (la.2 - lb.2).powi(2)).sqrt()
}

fn lab(c: [u8; 3]) -> (f64, f64, f64) {
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

pub fn to_hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Nom de machine d'un dossier de sauvegarde `AAAAMMJJ-HHMMSS-mmm-<machine>`.
pub fn machine_of_dirname(name: &str) -> Option<&str> {
    let mut it = name.splitn(4, '-');
    let (d, t, ms, m) = (it.next()?, it.next()?, it.next()?, it.next()?);
    if d.len() == 8 && t.len() == 6 && ms.len() == 3 && d.bytes().all(|b| b.is_ascii_digit()) && !m.is_empty() {
        Some(m)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_contrastee_et_distincte() {
        for c in PALETTE {
            assert!(contrast(c, text_on(c)) >= 4.5, "texte lisible sur {:?}", c);
        }
        let mut min = f64::MAX;
        for i in 0..PALETTE.len() {
            for j in i + 1..PALETTE.len() {
                min = min.min(delta_e(PALETTE[i], PALETTE[j]));
            }
        }
        assert!(min >= 15.0, "teintes bien distinctes : ecart minimal {:.1}", min);
    }

    #[test]
    fn couleur_stable_et_noms_differents_distincts() {
        assert_eq!(default_color("EPIKOU"), default_color("epikou"));
        assert_eq!(default_color("EPIKOU"), default_color("EPIKOU"));
        let names: Vec<String> = ["EPIKOU", "EPIGOS", "LEGION", "PC-SALON", "BUREAU", "PORTABLE"].iter().map(|s| s.to_string()).collect();
        let m = assign(&names, &BTreeMap::new());
        let mut cols: Vec<[u8; 3]> = m.values().copied().collect();
        cols.sort();
        cols.dedup();
        assert_eq!(cols.len(), names.len(), "deux machines = deux couleurs");
        // Ordre d'entree sans effet.
        let mut rev = names.clone();
        rev.reverse();
        assert_eq!(assign(&rev, &BTreeMap::new()), m);
    }

    #[test]
    fn choix_de_l_utilisateur_prioritaire() {
        let mut o = BTreeMap::new();
        o.insert("EPIKOU".to_string(), [1, 2, 3]);
        assert_eq!(color_of("EPIKOU", &o), [1, 2, 3]);
        assert_eq!(assign(&["EPIKOU".to_string()], &o)["EPIKOU"], [1, 2, 3]);
        assert_eq!(color_of("AUTRE", &o), default_color("AUTRE"));
    }

    #[test]
    fn machine_depuis_le_nom_du_dossier() {
        assert_eq!(machine_of_dirname("20261025-010519-973-EPIGOS"), Some("EPIGOS"));
        assert_eq!(machine_of_dirname("20261025-010519-973-PC-SALON"), Some("PC-SALON"));
        assert_eq!(machine_of_dirname("files"), None);
        assert_eq!(to_hex([0, 114, 178]), "#0072B2");
    }
}
