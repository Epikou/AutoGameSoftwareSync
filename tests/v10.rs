//! v0.10 phase A : interface (interrupteurs, 3 onglets, barre du bas, fenetre adaptative, mode manette maximise,
//! premier lancement). Logique pure + garde-fous de code.
use agss::gamepad::{Connection, PadInfo, PadStatus};
use agss::i18n::{tr, Lang, ALL_KEYS, K};
use agss::settings::{BigChoice, Settings, StartupRuleCfg};
use agss::startup::{build_rules, ID_APP};
use agss::uilogic::*;
use agss::vm::Page;
use std::path::{Path, PathBuf};

fn pad(name: &str, readable: bool) -> PadStatus {
    PadStatus { info: PadInfo { id: format!("x:{}", name), name: name.into(), conn: Connection::Unknown }, readable }
}

#[test]
fn etat_de_la_manette_quatre_libelles() {
    assert_eq!(pad_kind(&[]), PadKind::None);
    assert_eq!(pad_kind(&[pad("DualSense", true)]), PadKind::DualSense);
    assert_eq!(pad_kind(&[pad("Xbox / XInput", true)]), PadKind::Xbox, "la Legion en XInput = manette Xbox");
    assert_eq!(pad_kind(&[pad("DualShock 4", true)]), PadKind::Other);
    assert_eq!(pad_kind(&[pad("Manette de test", true)]), PadKind::Other);
    // Illisible (ex. monopolisee par Steam Input) = pas de manette utilisable.
    assert_eq!(pad_kind(&[pad("DualSense", false)]), PadKind::None);
    // Priorite DualSense > Xbox > autre.
    assert_eq!(pad_kind(&[pad("Xbox / XInput", true), pad("DualSense", true)]), PadKind::DualSense);
    assert_eq!(pad_kind(&[pad("Manette de test", true), pad("Xbox / XInput", true)]), PadKind::Xbox);
    for k in [K::PadDualSense, K::PadXbox, K::PadOther, K::PadNone] {
        assert!(!tr(Lang::Fr, k).is_empty() && !tr(Lang::En, k).is_empty());
    }
    assert_eq!(tr(Lang::En, K::PadNone), "No controller");
}

#[test]
fn onglets_libelles_courts_puis_icones() {
    let full = [90.0, 110.0, 80.0];
    let short = [60.0, 70.0, 80.0];
    assert_eq!(tab_mode(1000.0, &full, &short, 8.0), TabMode::Full);
    assert_eq!(tab_mode(300.0, &full, &short, 8.0), TabMode::Full);
    assert_eq!(tab_mode(250.0, &full, &short, 8.0), TabMode::Short);
    assert_eq!(tab_mode(200.0, &full, &short, 8.0), TabMode::Icons);
    assert!(is_narrow(380.0) && is_narrow(500.0) && !is_narrow(760.0));
}

#[test]
fn trois_onglets_le_journal_est_dans_les_reglages() {
    assert_eq!(Page::ALL.len(), 3);
    assert_eq!(Page::Settings.next(), Page::Games);
    assert_eq!(Page::Games.prev(), Page::Settings);
    assert_eq!(page_after(Page::Games, 1), Page::Add);
}

#[test]
fn mode_manette_maximise_puis_restaure() {
    let mut st = MaxState::default();
    // Pas de manette : rien.
    assert_eq!(max_step(&mut st, false, false, true), MaxAction::None);
    // Manette branchee : on maximise.
    assert_eq!(max_step(&mut st, true, false, true), MaxAction::Maximize);
    assert!(st.we_maximized);
    // Tant que ca dure : plus rien (le redimensionnement de l'utilisateur est respecte).
    assert_eq!(max_step(&mut st, true, false, true), MaxAction::None);
    // Debranchement, fenetre toujours maximisee par nous : on restaure.
    assert_eq!(max_step(&mut st, false, true, true), MaxAction::Restore);
    assert!(!st.we_maximized);
    // Debranchement apres que l'utilisateur a lui-meme repris la main (fenetre non maximisee) : on ne touche a rien.
    let mut st = MaxState::default();
    assert_eq!(max_step(&mut st, true, false, true), MaxAction::Maximize);
    assert_eq!(max_step(&mut st, false, false, true), MaxAction::None);
    // Deja maximisee par l'utilisateur : on ne maximise pas, on ne restaure pas.
    let mut st = MaxState::default();
    assert_eq!(max_step(&mut st, true, true, true), MaxAction::None);
    assert_eq!(max_step(&mut st, false, true, true), MaxAction::None);
    // Application cachee dans la zone de notification : rien ; la decision est reprise a l'ouverture.
    let mut st = MaxState::default();
    assert_eq!(max_step(&mut st, true, false, false), MaxAction::None);
    assert!(!st.prev_want);
    assert_eq!(max_step(&mut st, true, false, true), MaxAction::Maximize);
    // Reglages : Jamais ne maximise pas, Toujours maximise meme sans manette, Auto suit la manette.
    assert!(!want_maximized(BigChoice::Never, true));
    assert!(want_maximized(BigChoice::Always, false));
    assert!(want_maximized(BigChoice::Auto, true) && !want_maximized(BigChoice::Auto, false));
}

#[test]
fn premier_lancement_six_etapes_qui_configurent() {
    // 2.0 (lot 10) : l'ancien tutoriel en superposition (4 etapes, jeu de demonstration) est remplace par l'assistant.
    use agss::onboarding as ob;
    assert_eq!(ob::sequence(ob::ALL), vec![1, 2, 3, 4, 5, 6]);
    for k in [K::VWizName1, K::VWizName2, K::VWizName3, K::VWizName4, K::VWizName5, K::VWizName6] {
        assert!(tr(Lang::Fr, k).len() > 2 && tr(Lang::En, k).len() > 2);
    }
    let g = src_all();
    assert!(!g.contains("tutorial_overlay") && !g.contains("DEMO_NAME") && !g.contains("tut_banner_panel"), "ancien tutoriel et bandeau retires");
}

#[test]
fn demarrage_simple_arguments_de_la_tache_application() {
    let exe = PathBuf::from("C:\\Apps\\AutoGameSoftwareSync.exe");
    let mk = |bg: bool| vec![StartupRuleCfg { id: ID_APP.into(), name: String::new(), exe: None, delay_secs: 40, enabled: true, minimized: bg }];
    let on = build_rules(&[], &mk(true), &exe, &|_| true);
    assert_eq!(on[0].args, "--tray", "en arriere-plan : tache avec --tray");
    let off = build_rules(&[], &mk(false), &exe, &|_| true);
    assert_eq!(off[0].args, "", "sans arriere-plan : tache sans --tray");
    // Defaut (aucun reglage) : arriere-plan.
    assert_eq!(build_rules(&[], &[], &exe, &|_| true)[0].args, "--tray");
}

#[test]
fn reglages_etape_du_tutoriel_et_compatibilite() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.tutorial_step, 0);
    let s2: Settings = serde_json::from_str(r#"{"tutorial_step":6}"#).unwrap();
    assert_eq!(s2.tutorial_step, 6);
}

fn src_all() -> String {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut s = std::fs::read_to_string(src.join("gui.rs")).unwrap();
    for e in walkdir::WalkDir::new(src.join("gui")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        s.push_str(&std::fs::read_to_string(e.path()).unwrap());
    }
    s
}

#[test]
fn plus_aucune_case_a_cocher_et_cafe_dessine() {
    let g = src_all();
    assert!(!g.contains(".checkbox(") && !g.contains("Checkbox::new"), "toutes les cases a cocher sont remplacees par des interrupteurs");
    assert!(g.contains("fn toggle(") && g.contains("fn paint_cup"), "interrupteur et tasse dessinee au painter");
    assert!(g.contains("pub const COFFEE_PAGE: &str = \"https://ko-fi.com/"), "adresse Ko-fi centralisee dans une constante unique");
    assert!(!g.contains("COFFEE_URL"));
    // Le Journal n'est plus un onglet.
    assert!(!g.contains("Page::Log"));
    // Plus de grands titres de page en doublon avec l'onglet.
    assert!(!g.contains("ui.heading(self.t(K::NavGames))") && !g.contains("ui.heading(self.t(K::NavAdd))") && !g.contains("ui.heading(self.t(K::NavSettings))"));
}

#[test]
fn nouvelles_chaines_phase_a_traduites() {
    for name in ["VWizName1", "VWizContinue", "CoffeeBtn", "StartupOpenApp", "StartupBackground", "BtnAdvancedStartup", "BtnOpenDataDir"] {
        let k = ALL_KEYS.iter().find(|k| format!("{:?}", k) == name).unwrap();
        assert!(!tr(Lang::Fr, *k).is_empty() && !tr(Lang::En, *k).is_empty() && tr(Lang::Fr, *k) != tr(Lang::En, *k), "{}", name);
    }
    assert_eq!(tr(Lang::En, K::CoffeeBtn), "Buy us a coffee");
    // Texte de base (Windows) ; la variante Linux parle de la session de bureau.
    assert!(agss::i18n::tr_raw(Lang::En, K::StartupOpenApp).starts_with("Open AutoGameSoftwareSync when my computer starts"));
    assert!(agss::i18n::tr_linux(Lang::En, K::StartupOpenApp).starts_with("Open AutoGameSoftwareSync when my desktop session starts"));
    assert_eq!(tr(Lang::En, K::StartupBackground), "Start in the background");
    assert_eq!(tr(Lang::En, K::BtnAdvancedStartup), "Advanced startup");
}
