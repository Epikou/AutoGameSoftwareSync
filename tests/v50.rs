//! 2.0.3 (lot 16, fin) : icones, licences et archives de la release. Comportement verifie quand c'est possible (police
//! Phosphor installee, manette seule dessinee par Phosphor) ; les fichiers de donnees (NOTICE, LICENSE) et les scripts
//! (make-release.ps1, make_icon.rs) ne peuvent etre verifies que par leur contenu.
use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(p: &str) -> String {
    std::fs::read_to_string(root().join(p)).unwrap_or_else(|e| panic!("{} : {}", p, e))
}

#[test]
fn notice_et_licence_presents_phosphor_credite_pour_la_manette_seulement() {
    let notice = read("NOTICE");
    assert!(notice.contains("The controller icon of the interface comes from Phosphor Icons"), "NOTICE : seule la manette vient de Phosphor");
    assert!(!notice.contains("The icons of the interface come from"), "NOTICE : ne pas attribuer toutes les icones a Phosphor");
    assert!(notice.contains("egui-phosphor"), "NOTICE : la caisse egui-phosphor est creditee");
    assert!(notice.matches("MIT License").count() >= 2, "NOTICE : licence MIT des icones et de la caisse");
    assert!(notice.contains("GNU General Public"), "NOTICE : le code reste sous GPL");
    let lic = read("LICENSE");
    assert!(lic.contains("GNU GENERAL PUBLIC LICENSE") && lic.contains("Version 3"), "LICENSE = GPL-3.0");
}

#[test]
fn police_phosphor_regular_installee_et_manette_seule_par_phosphor() {
    use agss::gui::ui::glyphs::{self, Glyph};
    // La police des icones fait partie des definitions de polices utilisees par l'appli (demarrage et rechargement CJK
    // passent par la meme fonction) : famille nommee presente, donnees de police non vides.
    let f = glyphs::fonts_with_icons();
    let fam = eframe::egui::FontFamily::Name(glyphs::ICON_FAMILY.into());
    assert!(f.families.get(&fam).map(|v| v.iter().any(|n| n == glyphs::ICON_FAMILY)).unwrap_or(false), "famille d'icones absente");
    assert!(f.font_data.contains_key(glyphs::ICON_FAMILY), "donnees de la police Phosphor absentes");
    // Variante Regular seule : le caractere de la manette est celui de Phosphor Regular (la caisse ne compile qu'avec
    // cette variante : une variante en plus doublerait la taille de l'exe).
    assert_eq!(glyphs::glyph_char(Glyph::Pad), egui_phosphor::regular::GAME_CONTROLLER);
    // Une fois les definitions construites, la police est declaree prete (la manette se dessine alors par Phosphor).
    assert!(glyphs::icon_font_ready());
}

#[test]
fn archives_de_release_avec_licence_et_notice() {
    let s = read("tools/make-release.ps1");
    assert!(s.contains("pack-zip $zip $WinExe @legal"), "LICENSE et NOTICE dans le zip");
    assert!(s.contains("pack-targz $tgz @linuxFiles @legal"), "LICENSE et NOTICE dans le tar.gz");
    assert!(s.contains("## What is new in 2.0.3"), "notes 2.0.3 en tete");
    let i203 = s.find("## What is new in 2.0.3").unwrap();
    let i202 = s.find("## What is new in 2.0.2").unwrap();
    assert!(i203 < i202, "les nouveautes 2.0.3 precedent les 2.0.2");
    // Notes exactes (controle 2.0.3) : la carte compte, l'ecran de fermeture a ses conditions, rien de cherche en ligne.
    let notes = &s[i203..i202];
    assert!(notes.contains("a card counting them"), "la carte compte, elle ne liste pas");
    assert!(notes.contains("only if something to protect was found"), "ecran seulement si quelque chose a ete trouve");
    assert!(notes.contains("nothing is searched online for them"), "aucune recherche en ligne pour un programme inconnu");
    assert!(notes.contains("two times at most"), "« Plus tard » deux fois au plus");
    assert!(!notes.contains("a card listing them"));
}

#[test]
fn make_icon_exporte_les_png_par_taille() {
    let s = read("examples/make_icon.rs");
    assert!(s.contains("icone-{}.png"), "export PNG par taille pour les captures avant / apres");
    assert!(s.contains("FilterQuality::Nearest"), "planche de controle : petites tailles agrandies pixel par pixel");
}
