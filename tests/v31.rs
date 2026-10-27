//! 1.3.2 : animation (machine a etats, etape 1 du premier lancement en 2.0), formulaire d'ajout en trois gestes (validite, surbrillance, message conditionnel,
//! nom deduit, raccourci .lnk), cartes des Reglages, popup cafe.
use agss::addform::{self, ClickResult, FormIn, Validity};
use agss::banner::*;
use agss::i18n::{tr, Lang, K};
use agss::schedule::Schedule;
use std::path::PathBuf;

// ------------------------------------------------------------------ bandeau

#[test]
fn bandeau_quatre_phases_de_2_6_s_en_boucle() {
    assert_eq!(phase_at(0.0).0, 0);
    assert_eq!(phase_at(2.59).0, 0);
    assert_eq!(phase_at(2.6).0, 1);
    assert_eq!(phase_at(5.3).0, 2);
    assert_eq!(phase_at(7.9).0, 3);
    assert_eq!(phase_at(10.4).0, 0, "boucle");
    assert_eq!(phase_at(10.4 * 3.0 + 1.0).0, 0);
    assert!((phase_at(1.3).1 - 0.5).abs() < 1e-9);
    // Entrees invalides : jamais de panique.
    assert_eq!(phase_at(-5.0).0, 0);
    assert_eq!(phase_at(f64::NAN).0, 0);
    assert_eq!(caption_index(5.3), 2);
}

#[test]
fn bandeau_un_seul_element_actif_les_autres_estompes() {
    for step in 0..400 {
        let t = step as f64 * 0.0521;
        let w = weights(t);
        let sum: f32 = w.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3, "somme des poids = 1 a t={} : {:?}", t, w);
        assert!(w.iter().all(|x| (0.0..=1.0).contains(x)));
        // L'element actif a le plus gros poids pendant le gros de la phase.
        let (p, q) = phase_at(t);
        if q < 0.8 {
            assert_eq!(w[p], 1.0);
        }
    }
    // Mode reduit : bascule nette, sans agrandissement ni rotation (fondu seulement).
    let r = weights_reduced(3.0);
    assert_eq!(r, [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(scale_of(1.0, true), 1.0);
    assert!(scale_of(1.0, false) > 1.05);
    assert_eq!(spin_angle(6.0, true), 0.0);
    assert_eq!(spin_angle(1.0, false), 0.0, "les fleches ne tournent que pendant leur phase");
    assert!(spin_angle(6.0, false) != 0.0);
    assert_eq!(opacity_of(0.0), 0.5);
    assert_eq!(opacity_of(1.0), 1.0);
}

#[test]
fn bandeau_escalier_de_tailles_et_repaint_seulement_si_necessaire() {
    assert_eq!(SIZES, [46.0, 58.0, 76.0, 96.0]);
    assert!(SIZES.windows(2).all(|w| w[0] < w[1]), "tailles croissantes");
    assert!(natural_width() > 300.0 && natural_width() < 500.0);
    assert_eq!(repaint_after(1.0, false), None, "animation fluide : image a image");
    let d = repaint_after(1.0, true).unwrap();
    assert!(d > 0.0 && d <= PHASE_SECS, "mode reduit : une image au changement de phase ({})", d);
}

// ------------------------------------------------------------------ formulaire d'ajout

fn form<'a>(exe: &'a str, name: &'a str, path: &'a str, ok: bool, s: Option<&'a Schedule>) -> FormIn<'a> {
    FormIn { exe, name, path, path_ok: ok, schedule: s }
}

#[test]
fn formulaire_validite_surbrillance_et_message_conditionnel() {
    // Avec exe : valide des que nom + emplacement existant.
    let v = addform::validity(&form("C:\\a\\x.exe", "X", "C:\\s", true, None));
    assert_eq!(v, Validity::Ready);
    assert!(addform::button_clickable(v) && addform::button_highlighted(v));
    // Sans exe ni planification : cliquable (pour expliquer) mais JAMAIS en surbrillance, message seulement apres un clic.
    let v = addform::validity(&form("", "X", "C:\\s", true, None));
    assert_eq!(v, Validity::NeedSchedule);
    assert!(addform::button_clickable(v) && !addform::button_highlighted(v));
    assert!(!addform::show_schedule_message(v, false), "jamais affiche en permanence");
    assert_eq!(addform::on_click(v), ClickResult::ExplainSchedule);
    assert!(addform::show_schedule_message(v, true));
    // Planification invalide (intervalle trop court) : meme chose ; valide : surbrillance, quelle que soit la methode.
    let bad = Schedule::interval(5);
    assert_eq!(addform::validity(&form("", "X", "C:\\s", true, Some(&bad))), Validity::NeedSchedule);
    let good = Schedule::interval(900);
    let v = addform::validity(&form("", "X", "C:\\s", true, Some(&good)));
    assert!(addform::button_highlighted(v) && addform::on_click(v) == ClickResult::Add);
    assert!(!addform::show_schedule_message(v, true), "plus de message une fois la planification valide");
    // Nom ou emplacement manquants : bouton inactif, aucun message de planification.
    let v = addform::validity(&form("", "", "C:\\s", true, None));
    assert_eq!(v, Validity::NeedName);
    assert!(!addform::button_clickable(v));
    let v = addform::validity(&form("C:\\a\\x.exe", "X", "", false, None));
    assert_eq!(v, Validity::NeedPath);
    assert_eq!(addform::on_click(v), ClickResult::Nothing);
    assert_eq!(addform::validity(&form("C:\\a\\x.exe", "X", "C:\\absent", false, None)), Validity::NeedPath);
    // La planification n'est obligatoire que sans exe.
    assert!(addform::schedule_required("  ") && !addform::schedule_required("a.exe"));
}

#[test]
fn nom_deduit_des_informations_de_version() {
    assert_eq!(addform::derive_name(Some("Blender"), Some("Blender 3D"), "blender-launcher.exe"), "Blender");
    assert_eq!(addform::derive_name(None, Some("Super Truc"), "truc.exe"), "Super Truc");
    assert_eq!(addform::derive_name(Some("  "), None, "MonJeu.exe"), "MonJeu");
    assert_eq!(addform::derive_name(Some("Système d'exploitation Microsoft® Windows®"), Some("Bloc-notes"), "notepad.exe"), "Bloc-notes");
    assert_eq!(addform::derive_name(None, None, ""), "");
    assert!(addform::accepts_extension(&PathBuf::from("C:\\a\\B.EXE")) && addform::accepts_extension(&PathBuf::from("x.lnk")));
    assert!(!addform::accepts_extension(&PathBuf::from("x.txt")));
}

/// Fabrique un raccourci .lnk minimal (en-tete + LinkInfo) pointant vers `target`.
fn fake_lnk(target: &str, unicode: bool) -> Vec<u8> {
    let mut b = vec![0u8; 0x4C];
    b[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
    b[0x14..0x18].copy_from_slice(&2u32.to_le_bytes()); // HasLinkInfo seulement
    let (base, suffix) = target.split_at(target.len().min(3)); // « C:\ » + reste
    let header: usize = if unicode { 0x24 } else { 0x1C };
    let ansi = |s: &str| {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    };
    let wide = |s: &str| {
        let mut v: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        v.extend([0, 0]);
        v
    };
    let (b1, b2) = if unicode { (wide(base), wide(suffix)) } else { (ansi(base), ansi(suffix)) };
    let off1 = header as u32;
    let off2 = off1 + b1.len() as u32;
    let size = off2 + b2.len() as u32;
    let mut info = Vec::new();
    info.extend(size.to_le_bytes());
    info.extend((header as u32).to_le_bytes());
    info.extend(1u32.to_le_bytes()); // VolumeIDAndLocalBasePath
    info.extend(0u32.to_le_bytes()); // VolumeIDOffset
    info.extend(if unicode { 0u32 } else { off1 }.to_le_bytes()); // LocalBasePathOffset
    info.extend(0u32.to_le_bytes()); // CommonNetworkRelativeLinkOffset
    info.extend(if unicode { 0u32 } else { off2 }.to_le_bytes()); // CommonPathSuffixOffset
    if unicode {
        info.extend(off1.to_le_bytes());
        info.extend(off2.to_le_bytes());
    }
    info.extend(b1);
    info.extend(b2);
    b.extend(info);
    b
}

#[test]
fn raccourci_lnk_resolu_vers_son_exe() {
    for unicode in [false, true] {
        let lnk = fake_lnk("C:\\Games\\Mon Jeu\\jeu.exe", unicode);
        assert_eq!(addform::parse_lnk(&lnk), Some(PathBuf::from("C:\\Games\\Mon Jeu\\jeu.exe")), "unicode={}", unicode);
    }
    assert_eq!(addform::parse_lnk(b"pas un raccourci"), None);
    assert_eq!(addform::parse_lnk(&[]), None);
    // Fichiers reels : .lnk -> exe, .exe tel quel, autre -> rien.
    let t = tempfile::TempDir::new().unwrap();
    let lnk = t.path().join("jeu.lnk");
    std::fs::write(&lnk, fake_lnk("C:\\Games\\jeu.exe", true)).unwrap();
    assert_eq!(addform::resolve_input(&lnk), Some(PathBuf::from("C:\\Games\\jeu.exe")));
    let not_exe = t.path().join("doc.lnk");
    std::fs::write(&not_exe, fake_lnk("C:\\Docs\\lisez-moi.txt", true)).unwrap();
    assert_eq!(addform::resolve_input(&not_exe), None, "un raccourci vers autre chose qu'un exe est refuse");
    assert_eq!(addform::resolve_input(&PathBuf::from("C:\\x\\y.exe")), Some(PathBuf::from("C:\\x\\y.exe")));
    assert_eq!(addform::resolve_input(&PathBuf::from("C:\\x\\y.txt")), None);
}

// ------------------------------------------------------------------ interface

#[test]
fn reglages_ordre_et_etat_par_defaut_des_cartes() {
    // 2.0 (lot 11) : six cartes en mode simple (Langue et apparence, Coffre, Appareils, Protection automatique, Mises a jour,
    // Donnees d'usage), puis en mode expert Conflits, Surveillance, Configuration, Journal (repliees par defaut).
    use agss::setmode::{cards, Card};
    assert_eq!(cards(false), vec![Card::Look, Card::Vault, Card::Devices, Card::Protect, Card::Updates, Card::Usage]);
    assert_eq!(cards(true).len(), 10);
    assert!(!agss::settings::Settings::default().expert_mode, "mode simple par defaut");
}

#[test]
fn popup_cafe_equipe_et_libelles_dans_les_5_langues() {
    for l in Lang::ALL {
        let s = if l == Lang::Zh { agss::i18n_zh::tr(K::SupBody) } else { tr(l, K::SupBody) };
        assert!(s.contains('\n') || s.contains("\\n"), "deux phrases");
        assert!(!tr(l, K::SupTitle).is_empty() && !tr(l, K::BtnPropLater).is_empty());
    }
    assert_eq!(tr(Lang::Fr, K::CoffeeBtn), "Offre-nous un café");
    assert_eq!(tr(Lang::En, K::CoffeeBtn), "Buy us a coffee");
    assert_eq!(tr(Lang::Ru, K::CoffeeBtn), "Угости нас кофе");
    assert_eq!(tr(Lang::Es, K::CoffeeBtn), "Invítanos a un café");
    assert_eq!(tr(Lang::Zh, K::CoffeeBtn), "请我们喝杯咖啡");
    // On parle d'equipe, jamais du « dev ».
    for l in [Lang::Fr, Lang::En, Lang::Es] {
        let s = tr(l, K::SupBody).to_lowercase();
        assert!(!s.contains(" dev ") && !s.contains("le dev") && !s.contains("al dev") && !s.contains("the dev"), "{:?}", l);
        assert!(!tr(l, K::SupTitle).to_lowercase().contains("dev"));
    }
    assert!(tr(Lang::Fr, K::SupBody).contains("l'équipe"));
}

#[test]
fn legendes_de_l_animation_du_premier_lancement_dans_les_5_langues() {
    // 2.0 (lot 10) : l'ancien bandeau du tutoriel devient l'animation de l'etape 1, avec le vocabulaire du mode simple
    // (plus de « Ton exe » ni de « Ta save »).
    for l in Lang::ALL {
        for k in [K::VAnimProgram, K::VAnimData, K::VAnimWatch, K::VAnimSync, K::VAnimSafe, K::EmptyTitle] {
            assert!(!tr(l, k).trim().is_empty());
        }
    }
    assert_eq!(tr(Lang::Fr, K::VAnimProgram), "Ton programme");
    assert_eq!(tr(Lang::Fr, K::VAnimSync), "On synchronise");
    assert_eq!(tr(Lang::En, K::VAnimSafe), "Safe!");
    // L'animation tient dans une petite place (version compacte, jamais sous la moitie de sa taille).
    assert!((0.5..=1.0).contains(&fit_scale(300.0, 80.0)));
    assert_eq!(fit_scale(2000.0, 2000.0), 1.0);
    assert!(natural_height() > 100.0 && natural_height() < 150.0);
}
