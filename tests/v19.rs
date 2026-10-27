//! Lot UI : popup cafe (logique pure), animation « Actualiser », migration des conflits, reglages et textes.
use agss::i18n::{tr, Lang, K};
use agss::settings::{ConflictMode, Settings};
use agss::support::*;
use agss::uilogic::{RefreshAnim, RefreshPhase, REFRESH_DONE_MS, REFRESH_MIN_MS};

const DAY: u64 = 86_400_000;
const NOW: u64 = 1_800_000_000_000;

#[test]
fn affichage_apres_deux_jeux_puis_au_plus_tous_les_dix_jours() {
    let ok = Blockers::default();
    // Jamais affichee : il faut 2 jeux ajoutes.
    assert!(!should_show(true, 0, 0, false, NOW, &ok));
    assert!(!should_show(true, 1, 0, false, NOW, &ok));
    assert!(should_show(true, 2, 0, false, NOW, &ok));
    assert!(should_show(true, 5, 0, false, NOW, &ok));
    // Apres un affichage (compteur remis a 0) : 2 nouveaux jeux ET 10 jours.
    let last = NOW;
    assert!(!should_show(true, 2, last, false, last + 9 * DAY, &ok), "moins de 10 jours");
    assert!(should_show(true, 2, last, false, last + 10 * DAY, &ok));
    assert!(!should_show(true, 1, last, false, last + 30 * DAY, &ok), "pas assez de jeux ajoutes");
    // Apres un clic sur le cafe : 60 jours.
    assert!(!should_show(true, 2, last, true, last + 59 * DAY, &ok));
    assert!(should_show(true, 2, last, true, last + 60 * DAY, &ok));
    assert_eq!((interval_ms(false), interval_ms(true)), (10 * DAY, 60 * DAY));
    // Reglage desactive.
    assert!(!should_show(false, 9, 0, false, NOW, &ok));
}

#[test]
fn jamais_pendant_le_tutoriel_une_operation_une_fenetre_cachee_ou_un_dialogue() {
    for b in [Blockers { tutorial: true, ..Default::default() }, Blockers { busy: true, ..Default::default() }, Blockers { window_hidden: true, ..Default::default() }, Blockers { dialog_open: true, ..Default::default() }] {
        assert!(b.any());
        assert!(!should_show(true, 9, 0, false, NOW, &b), "{:?}", b);
    }
    assert!(!Blockers::default().any());
}

#[test]
fn compteur_de_jeux_ajoutes() {
    assert_eq!(newly_added(3, 5), 2);
    assert_eq!(newly_added(3, 3), 0);
    assert_eq!(newly_added(3, 2), 0, "un retrait n'est pas compte");
}

#[test]
fn animation_du_bouton_actualiser_duree_minimale_puis_confirmation() {
    let mut a = RefreshAnim::default();
    assert_eq!(a.phase(0, false), RefreshPhase::Idle);
    a.click(1000);
    assert_eq!(a.phase(1000, false), RefreshPhase::Spinning);
    assert_eq!(a.phase(1000 + REFRESH_MIN_MS - 1, false), RefreshPhase::Spinning, "au moins 600 ms meme si c'est deja fini");
    assert_eq!(a.phase(1000 + REFRESH_MIN_MS + 5, false), RefreshPhase::Done);
    assert_eq!(a.phase(1000 + REFRESH_MIN_MS + 500, false), RefreshPhase::Done);
    assert_eq!(a.phase(1000 + REFRESH_MIN_MS + 5 + REFRESH_DONE_MS, false), RefreshPhase::Idle, "la confirmation est breve");
    assert_eq!(a.phase(99_999, false), RefreshPhase::Idle);
    // Rafraichissement long : l'icone tourne tant qu'il dure.
    let mut b = RefreshAnim::default();
    b.click(0);
    assert_eq!(b.phase(5000, true), RefreshPhase::Spinning);
    assert_eq!(b.phase(5100, false), RefreshPhase::Done);
}

#[test]
fn migration_herite_vers_cloud_automatique_sans_toucher_aux_choix_explicites() {
    let old = r#"{"per_game":{"A":{"keep":5},"B":{"conflict":"ask"},"C":{"conflict":"cloud"}},"conflict_mode":"ask"}"#;
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("settings.json"), old).unwrap();
    let s = Settings::load_from(dir.path()).unwrap();
    assert_eq!(s.conflict_for("A"), ConflictMode::Cloud, "Herite -> cloud automatique");
    assert_eq!(s.conflict_for("B"), ConflictMode::Ask, "« Me demander » explicite inchange");
    assert_eq!(s.conflict_for("C"), ConflictMode::Cloud);
    assert!(s.conflict_migrated);
    // Une seule fois : un jeu cree ensuite sans choix suit le reglage global.
    let mut s2 = s.clone();
    s2.game_mut("Nouveau");
    s2.migrate_conflicts();
    assert_eq!(s2.conflict_for("Nouveau"), s2.conflict_mode);
    // Nouvelle installation : rien a migrer, defaut cloud automatique.
    assert!(Settings::default().conflict_migrated);
    assert_eq!(Settings::default().conflict_for("X"), ConflictMode::Cloud);
}

#[test]
fn reglages_de_soutien_par_defaut_et_anciens_fichiers() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.support_reminders, "ON par defaut");
    assert_eq!((s.support_added, s.support_last_ms, s.support_long), (0, 0, false));
    let t: Settings = serde_json::from_str(r#"{"support_reminders":false,"support_added":3,"support_last_ms":42,"support_long":true}"#).unwrap();
    assert!(!t.support_reminders && t.support_added == 3 && t.support_last_ms == 42 && t.support_long);
}

#[test]
fn textes_du_lot_cinq_langues() {
    for k in [K::SetSupport, K::TipSupport, K::SupTitle, K::SupBody, K::TipSupLater, K::RefreshDone] {
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr(l, k) };
            assert!(!s.is_empty());
        }
    }
    assert_eq!(tr(Lang::En, K::SetSupport), "Show support reminders");
    assert!(tr(Lang::En, K::SupTitle).contains("caffeine") && !tr(Lang::En, K::SupBody).contains(" dev "), "on parle de l'equipe");
    assert_eq!(tr(Lang::En, K::RefreshDone), "Up to date"); // 1.0.2 : la coche est dessinee au painter, plus un glyphe de police
}
