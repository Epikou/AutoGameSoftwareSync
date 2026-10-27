//! 1.2.3 : vocabulaire GENERIQUE. Aucun texte d'interface ne dit « jeu / game / juego / игра / 游戏 » en dehors d'une liste blanche
//! explicite (distinction jeux / logiciels VOLONTAIRE), avec une justification par cle.
use agss::i18n::{tr, Lang, ALL_KEYS, K};

/// (cle, justification) : ces textes parlent VOLONTAIREMENT de jeux.
const WHITELIST: &[(K, &str)] = &[
    (K::VSearchHint, "champ de recherche de l'accueil : « jeu » face à « logiciel » : distinction volontaire"),
    (K::DetScopeGames, "tuile de portee « Jeux » face à « Logiciels » : distinction volontaire"),
    (K::TipDetScope, "explique la difference entre les portees Jeux et Logiciels : distinction volontaire"),
    (K::ProfCatGame, "categorie « Jeux » des profils : distinction volontaire"),
    (K::AppTagline, "accroche officielle « jeux et logiciels » : distinction volontaire"),
    (K::AddExe, "libelle impose par l'utilisateur : « Exe du jeu/emulateur/programme » : distinction volontaire"),
    (K::LocExe, "meme libelle que AddExe (fiche de suivi) : distinction volontaire"),
    // Variantes Linux (memes raisons que leurs cles d'origine, ou nom officiel du « mode jeu » de SteamOS).
    (K::AddExeLx, "variante Linux de AddExe : « Programme du jeu/emulateur/logiciel » : distinction volontaire"),
    (K::LocExeLx, "variante Linux de LocExe : distinction volontaire"),
    (K::TipDetScopeLx, "variante Linux de TipDetScope (portees Jeux / Logiciels) : distinction volontaire"),
    (K::TipDetEnableLx, "jeux Windows lances par Proton / Wine : distinction volontaire"),
    (K::SetBigAutoLx, "nom officiel du « mode jeu » de SteamOS : distinction volontaire"),
    (K::TipBigLx, "nom officiel du « mode jeu » de SteamOS : distinction volontaire"),
    (K::TipStartupOpenAppLinux, "nom officiel du « mode jeu » de SteamOS (le demarrage auto ne vaut qu'en mode bureau) : distinction volontaire"),
    // 2.0 (lot 5) : vocabulaire du mode simple.
    (K::VSavedGames, "type d'emplacement « Parties enregistrees » (jeux) face aux « Reglages du logiciel » : distinction volontaire"),
    (K::VFilterGames, "filtre « Jeux » de l'accueil face a « Logiciels » : distinction volontaire"),
    (K::VEmptyBody, "accueil vide : on cherche les jeux ET les logiciels du PC : distinction volontaire"),
    (K::VTipAdd, "onglet Ajouter : un jeu, un logiciel ou un dossier : distinction volontaire"),
    (K::VPausedPlaying, "etat d'une tuile de JEU en cours (les logiciels ont « pendant l'utilisation ») : distinction volontaire"),
    (K::VSuggestNew, "interrupteur « Me proposer les nouveaux jeux et logiciels » : distinction volontaire"),
    // 2.0 (lot 7) : fiche d'un element.
    (K::VPlaceSavedGames, "nom officiel du dossier Windows « Parties enregistrees » (Saved Games) : distinction volontaire"),
    // 2.0 (lot 8) : raccourcis du selecteur de dossier.
    (K::VPickSavedGames, "raccourci vers le dossier Windows « Parties enregistrees » (Saved Games) : distinction volontaire"),
    (K::VPickMyGames, "raccourci vers le dossier « Mes jeux » (Documents\\My Games) de Windows : distinction volontaire"),
    // 2.0 (lot 9) : scan du PC, categories et raisons qui distinguent les jeux des logiciels.
    (K::VScanCatGames, "categorie « Jeux » du scan (face a « Logiciels », « Dossiers perso ») : distinction volontaire"),
    (K::VScanPcHint, "le scan cherche les jeux, les logiciels et les dossiers : distinction volontaire"),
    (K::VWhyLauncher, "raison « Jeu Steam reconnu » (bibliotheque d'un lanceur de jeux) : distinction volontaire"),
    (K::VWhyBase, "raison « Parties enregistrees d'un jeu connu » (base des jeux) : distinction volontaire"),
    (K::VHideLauncher, "raison de masquage d'un lanceur de jeux (ses jeux sont listes a part) : distinction volontaire"),
    (K::VWhatBoth, "type d'emplacement « Parties et reglages » (jeux) face aux « Reglages du logiciel » : distinction volontaire"),
    // 2.0 (lot 10) : premier lancement.
    (K::VConsentBody, "donnees d'usage : les noms des jeux et logiciels CONNUS font partie de l'envoi (decision de l'auteur, mention obligatoire) : distinction volontaire"),
    // 2.0 (lot 11) : page Ajouter.
    (K::VAddTileProgram, "grande tuile « Un programme ou un jeu » (texte demande par l auteur) : distinction volontaire"),
];

fn strip(s: &str) -> String {
    s.replace("AutoGameSoftwareSync", "").replace("AutoGameSoftwareSync", "")
}

fn has_game_word(l: Lang, s: &str) -> bool {
    let s = strip(s).to_lowercase();
    let word = |w: &str| -> bool {
        // mot entier (lettres) : « gamepad » n'est pas « game »
        let mut from = 0;
        while let Some(i) = s[from..].find(w) {
            let a = from + i;
            let b = a + w.len();
            let before = s[..a].chars().next_back().map(|c| c.is_alphabetic()).unwrap_or(false);
            let after = s[b..].chars().next().map(|c| c.is_alphabetic()).unwrap_or(false);
            if !before && !after {
                return true;
            }
            from = b;
        }
        false
    };
    match l {
        Lang::Fr => word("jeu") || word("jeux"),
        Lang::En => word("game") || word("games"),
        Lang::Es => word("juego") || word("juegos"),
        Lang::Ru => s.contains("игр"),
        Lang::Zh => s.contains("游戏"),
    }
}

#[test]
fn aucun_texte_d_interface_ne_parle_de_jeu_hors_liste_blanche() {
    let wl: Vec<K> = WHITELIST.iter().map(|(k, _)| *k).collect();
    let mut bad = Vec::new();
    for l in [Lang::Fr, Lang::En, Lang::Ru, Lang::Es, Lang::Zh] {
        for k in ALL_KEYS {
            if wl.contains(k) {
                continue;
            }
            // Texte de base (Windows) ET texte affiche sous Linux (variante eventuelle, elle aussi en liste blanche si besoin).
            let lx = agss::i18n::linux_variant(*k);
            for s in [agss::i18n::tr_raw(l, *k), if wl.contains(&lx) { "" } else { agss::i18n::tr_raw(l, lx) }] {
                if has_game_word(l, s) {
                    bad.push(format!("{:?}/{:?}: {}", l, k, s.chars().take(90).collect::<String>()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} textes avec « jeu » hors liste blanche :\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn liste_blanche_justifiee_et_glossaire_applique() {
    for (_, why) in WHITELIST {
        assert!(why.contains("distinction volontaire"), "{}", why);
    }
    let want: [(Lang, &str, &str, &str, &str, &str); 5] = [
        (Lang::Fr, "Mes synchros", "Ajouter une synchro", "Fiche de suivi", "Synchronise-le !", "Ignorés"),
        (Lang::En, "My syncs", "Add a sync", "Tracking details", "Sync it!", "Ignored"),
        (Lang::Ru, "Мои синхронизации", "Добавить синхронизацию", "Карточка отслеживания", "Синхронизировать!", "Игнорируемые"),
        (Lang::Es, "Mis sincronizaciones", "Añadir una sincronización", "Ficha de seguimiento", "¡Sincronízalo!", "Ignorados"),
        (Lang::Zh, "我的同步项", "添加同步项", "跟踪详情", "同步！", "已忽略"),
    ];
    for (l, nav, add, detail, yes, ign) in want {
        assert_eq!(tr(l, K::NavGames), nav);
        assert_eq!(tr(l, K::NavAdd), add);
        assert_eq!(tr(l, K::DetailTitle), detail);
        assert_eq!(tr(l, K::BtnPropYes), yes);
        assert_eq!(tr(l, K::DetIgnoredTitle), ign);
    }
    assert_eq!(tr(Lang::Fr, K::DetScopeGames), "Jeux");
    assert_eq!(tr(Lang::Fr, K::DetScopeApps), "Logiciels");
    assert_eq!(tr(Lang::Fr, K::PropHeadLaunch), "Hé ! On a repéré {} !");
}
