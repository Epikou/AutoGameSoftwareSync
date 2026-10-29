//! Lot 4 (1.4) : mise a jour automatique. Textes en 5 langues, reglage, coherence du manifeste signe de reference,
//! hygiene des fichiers du lot. La logique (signature, SHA-256, remplacement, retour arriere...) est testee dans
//! `src/selfupdate/tests.rs`.
use agss::i18n::{tr, Lang, K};
use agss::selfupdate;
use agss::settings::{Settings, UpdateMode};

const NEW_KEYS: &[K] = &[
    K::SetCheckUpdates,
    K::TipCheckUpdates,
    K::TipUpdOpen,
    K::UpdModeAuto,
    K::UpdModeAsk,
    K::UpdModeNever,
    K::BtnUpdInstall,
    K::TipUpdInstall,
    K::UpdDownloading,
    K::UpdInstalling,
    K::UpdFailed,
    K::UpdManualHint,
    K::NotifUpdated,
    K::UpdWhatsNew,
    K::NotifUpdateRolledBack,
    K::UpdRolledBack,
];

fn text(l: Lang, k: K) -> &'static str {
    agss::i18n::tr_raw(l, k)
}

#[test]
fn textes_de_la_mise_a_jour_en_cinq_langues() {
    for k in NEW_KEYS {
        let fr = text(Lang::Fr, *k);
        for l in Lang::ALL {
            let s = text(l, *k);
            assert!(!s.trim().is_empty(), "{:?}/{:?}", l, k);
            if l != Lang::Fr && l != Lang::En {
                assert_ne!(s, fr, "{:?}/{:?} non traduit", l, k);
            }
            // UTF-8 propre : aucun caractere de remplacement
            assert!(!s.contains('\u{fffd}') && !s.contains("Ã"), "{:?}/{:?}: encodage", l, k);
        }
    }
    // espaces reserves
    for k in [K::UpdDownloading, K::UpdInstalling, K::UpdFailed, K::NotifUpdated] {
        for l in Lang::ALL {
            assert_eq!(text(l, k).matches("{}").count(), 1, "{:?}/{:?}", l, k);
        }
    }
    for l in Lang::ALL {
        let s = text(l, K::UpdRolledBack);
        assert!(s.contains("{0}") && s.contains("{1}"), "{:?}", l);
    }
}

#[test]
fn noms_des_trois_modes() {
    let want = [
        (Lang::Fr, "Automatique (recommandé)", "Me demander", "Jamais"),
        (Lang::En, "Automatic (recommended)", "Ask me", "Never"),
        (Lang::Es, "Automático (recomendado)", "Preguntarme", "Nunca"),
        (Lang::Ru, "Автоматически (рекомендуется)", "Спрашивать", "Никогда"),
        (Lang::Zh, "自动（推荐）", "询问我", "从不"),
    ];
    for (l, a, b, c) in want {
        assert_eq!(text(l, K::UpdModeAuto), a);
        assert_eq!(text(l, K::UpdModeAsk), b);
        assert_eq!(text(l, K::UpdModeNever), c);
    }
    assert_eq!(tr(Lang::Fr, K::UpdWhatsNew), "Quoi de neuf ?");
    assert_eq!(tr(Lang::Fr, K::NotifUpdated).replace("{}", "1.4.0"), "Mis à jour en 1.4.0");
}

#[test]
fn le_texte_ne_promet_plus_l_absence_d_installation() {
    for l in Lang::ALL {
        let tip = text(l, K::TipUpdOpen);
        assert!(!tip.to_lowercase().contains("automatic") && !tip.to_lowercase().contains("automátic") && !tip.contains("自动"), "{:?}: {}", l, tip);
    }
}

#[test]
fn mise_a_jour_automatique_par_defaut() {
    let s = Settings::default();
    assert_eq!(s.update_mode(), UpdateMode::Auto);
    assert!(s.check_updates && s.update_mode.is_none());
    let old: Settings = serde_json::from_str(r#"{"lang":"fr","check_updates":true}"#).unwrap();
    assert_eq!(old.update_mode(), UpdateMode::Auto);
}

#[test]
fn constantes_de_publication() {
    assert!(selfupdate::MANIFEST_URL.starts_with("https://github.com/Epikou/AutoGameSoftwareSync/releases/latest/download/"));
    assert!(selfupdate::MANIFEST_URL.ends_with(selfupdate::MANIFEST_NAME));
    assert_eq!(selfupdate::MANIFEST_NAME, "agss-manifest.json");
    assert_eq!(selfupdate::WATCH_SECS, 60);
    assert_eq!(selfupdate::PUBLIC_KEY_HEX.len(), 64);
    assert!(selfupdate::PUBLIC_KEY_HEX.bytes().all(|b| b.is_ascii_hexdigit()));
    // la cle publique integree n'est pas la cle nulle de remplissage
    assert_ne!(selfupdate::PUBLIC_KEY_HEX, "0".repeat(64));
}

#[test]
fn fichiers_du_lot_sans_mention_interdite() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for f in ["src/selfupdate.rs", "src/selfupdate/tests.rs", "src/gui/update_ui.rs", "examples/release_tool.rs", "tools/make-release.ps1"] {
        let t = std::fs::read_to_string(root.join(f)).unwrap_or_else(|_| panic!("{} manquant", f));
        let l = t.to_lowercase();
        for bad in ["claude", "anthropic", "co-authored"] {
            assert!(!l.contains(bad), "{} mentionne {}", f, bad);
        }
        // jamais de cle privee en clair dans les sources
        assert!(!t.contains("BEGIN PRIVATE KEY"), "{}", f);
    }
    // le script de publication ne publie rien : aucun appel direct a `gh release create` hors affichage de la commande
    let ps = std::fs::read_to_string(root.join("tools/make-release.ps1")).unwrap();
    for line in ps.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            continue;
        }
        assert!(!(t.starts_with("gh ") || t.starts_with("& gh") || t.contains("Invoke-Expression")), "le script ne doit rien publier : {}", line);
    }
}
