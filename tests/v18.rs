//! Page « Ajouter un jeu » (etat transitoire) et absence de toute mention de la source des donnees dans les textes visibles.
use agss::autofill::*;
use agss::i18n::{tr, Lang, ALL_KEYS};
use std::path::Path;

#[test]
fn encadre_masque_apres_utiliser_ou_ignorer_jusqu_au_changement_d_exe() {
    let mut p = AddPage::default();
    p.observe_exe("jeu.exe");
    assert!(p.visible("jeu.exe"), "visible par defaut");
    p.dismiss("jeu.exe"); // « Utiliser » ou « Ignorer »
    p.observe_exe("jeu.exe");
    assert!(!p.visible("jeu.exe"), "ne reapparait pas pour le meme exe");
    for _ in 0..5 {
        p.observe_exe("jeu.exe");
    }
    assert!(!p.visible("jeu.exe"));
    // Exe different : l'encadre est de nouveau possible.
    p.observe_exe("autre.exe");
    assert!(p.visible("autre.exe"));
    // Et il se masque independamment pour chaque exe.
    p.dismiss("autre.exe");
    assert!(!p.visible("autre.exe") && p.visible("jeu.exe"));
}

#[test]
fn changement_d_onglet_reinitialise_l_etat_de_la_page() {
    let mut p = AddPage::default();
    p.dismiss("jeu.exe");
    let mut b = AddBtnState::default();
    assert_eq!(b.update(true, true), AddBtn::Stopped);
    // Sortie puis retour sur la page : tout repart de zero.
    p.reset();
    b.reset();
    assert!(p.visible("jeu.exe"));
    assert_eq!(b.update(true, false), AddBtn::Pulsing);
    assert_eq!(p, AddPage::default());
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn aucune_mention_de_la_source_dans_les_textes_ni_le_readme() {
    let needle = ["ludu", "savi"].concat();
    for k in ALL_KEYS {
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(*k) } else { tr(l, *k) };
            assert!(!s.to_lowercase().contains(&needle), "{:?} {} : {}", k, l.code(), s);
            assert!(!s.to_lowercase().contains("manifest"), "{:?} {} : mot « manifest » visible", k, l.code());
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!read(&root.join("README.md")).to_lowercase().contains(&needle), "README");
    // Sources du projet : seule l'URL de l'outil de construction est tolere.
    let mut files = Vec::new();
    for d in ["src", "examples"] {
        walk(&root.join(d), &mut files);
    }
    for f in files.into_iter().filter(|f| f.extension().map(|e| e == "rs").unwrap_or(false)) {
        for (i, line) in read(&f).lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                assert!(f.ends_with("build_gamedb.rs") && line.contains("https://raw.githubusercontent.com/"), "{}:{} : {}", f.display(), i + 1, line);
            }
        }
    }
}
