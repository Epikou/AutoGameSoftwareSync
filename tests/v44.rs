//! 2.0 lot 11 : Reglages simple / expert (six cartes, un seul « Mode expert » pour toute l'application, aucune option
//! perdue) et page Ajouter modernisee (trois grandes tuiles, formulaire en mots simples, plus jamais « exe » en mode simple).
use agss::addpage::{self, Dropped, FormKind};
use agss::gui::ui::{self, anim::Motion, ChoiceSpec, Glyph, Tokens};
use agss::i18n::{expert_variant, tr, tr_linux, tr_raw, Lang, ALL_KEYS, K, SIMPLE_KEYS};
use agss::setmode::{self, Card, Exposure, EXPERT_CARDS, OPTIONS, SETTING_FIELDS, SIMPLE_ADD_KEYS, SIMPLE_CARDS, SIMPLE_SETTINGS_KEYS};
use eframe::egui;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ================================================================== outils de lecture du code

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(src_dir().join(rel)).unwrap_or_else(|e| panic!("{} : {}", rel, e))
}

/// gui.rs + src/gui/**.
fn gui_source() -> String {
    let src = src_dir();
    let mut s = std::fs::read_to_string(src.join("gui.rs")).unwrap();
    for e in walkdir::WalkDir::new(src.join("gui")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        s.push('\n');
        s.push_str(&std::fs::read_to_string(e.path()).unwrap());
    }
    s
}

/// Corps (avec accolades) de la fonction `name` : premiere definition `fn name`. Les accolades des litteraux texte sont ignorees.
fn fn_body(text: &str, name: &str) -> String {
    let pat = format!("fn {}(", name);
    let at = text.find(&pat).unwrap_or_else(|| panic!("fonction {} introuvable", name));
    body_from(text, at)
}

fn body_from(text: &str, from: usize) -> String {
    let bytes = text.as_bytes();
    let open = text[from..].find('{').map(|i| from + i).expect("accolade ouvrante");
    let (mut depth, mut i, mut in_str) = (0i32, open, false);
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'\'' && i + 2 < bytes.len() && bytes[i + 2] == b'\'' {
            i += 2; // caractere entre apostrophes ('{', '"'...)
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return text[from..=i].to_string();
            }
        }
        i += 1;
    }
    panic!("accolades non appariees");
}

/// Texte du fichier sans le corps des fonctions dont le nom contient « expert » (leur vocabulaire est technique par nature),
/// sans les commentaires.
fn without_expert_fns(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("fn ") {
        let before_ok = rest[..i].chars().next_back().map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(true);
        let name: String = rest[i + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if before_ok && !name.is_empty() && name.contains("expert") {
            out.push_str(&rest[..i]);
            let body = body_from(rest, i);
            rest = &rest[i + body.len()..];
        } else {
            out.push_str(&rest[..i + 3]);
            rest = &rest[i + 3..];
        }
    }
    out.push_str(rest);
    out.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

/// Cles `K::Nom` citees dans un texte de code.
fn keys_in(text: &str) -> BTreeSet<String> {
    let mut v = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find("K::") {
        let before = rest[..i].chars().next_back();
        let name: String = rest[i + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if !name.is_empty() && before.map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(true) {
            v.insert(name);
        }
        rest = &rest[i + 3..];
    }
    v
}

fn key_by_name(name: &str) -> Option<K> {
    ALL_KEYS.iter().copied().find(|k| format!("{:?}", k) == name)
}

fn is_v_key(name: &str) -> bool {
    let mut c = name.chars();
    c.next() == Some('V') && c.next().map(|x| x.is_ascii_uppercase()).unwrap_or(false)
}

// ================================================================== vocabulaire du mode simple

fn word(s: &str, w: &str) -> bool {
    let s = s.to_lowercase();
    let mut from = 0;
    while let Some(i) = s[from..].find(w) {
        let a = from + i;
        let b = a + w.len();
        let before = s[..a].chars().next_back().map(|c| c.is_alphanumeric()).unwrap_or(false);
        let after = s[b..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false);
        if !before && !after {
            return true;
        }
        from = b;
    }
    false
}

/// Mot interdit en mode simple (« exe », « cloud » et leurs equivalents), ou None.
fn simple_problem(l: Lang, s: &str) -> Option<&'static str> {
    let low = s.to_lowercase();
    if word(&low.replace(".exe", " exe"), "exe") {
        return Some("exe");
    }
    if low.contains("cloud") {
        return Some("cloud");
    }
    let bad: &[(&str, &'static str)] = match l {
        Lang::Fr => &[("synchro", "synchro"), ("synchros", "synchros"), ("save", "save")],
        Lang::En => &[],
        Lang::Es => &[("nube", "nube")],
        Lang::Ru => &[("облак", "облако")],
        Lang::Zh => &[("云", "云")],
    };
    for (w, why) in bad {
        let hit = if l == Lang::Zh || l == Lang::Ru { low.contains(w) } else { word(&low, w) };
        if hit {
            return Some(why);
        }
    }
    None
}

fn check_keys(keys: &[K]) -> Vec<String> {
    let mut bad = Vec::new();
    for l in Lang::ALL {
        for k in keys {
            for s in [tr_raw(l, *k), tr_linux(l, *k)] {
                if let Some(why) = simple_problem(l, s) {
                    bad.push(format!("{:?}/{:?} ({}) : {}", l, k, why, s));
                }
            }
        }
    }
    bad
}

#[test]
fn listes_de_textes_des_ecrans_simples_sans_exe_ni_cloud_dans_les_5_langues() {
    let mut keys: Vec<K> = SIMPLE_SETTINGS_KEYS.to_vec();
    keys.extend(SIMPLE_ADD_KEYS);
    let bad = check_keys(&keys);
    assert!(bad.is_empty(), "{} texte(s) interdits en mode simple :\n{}", bad.len(), bad.join("\n"));
    // Le controle detecte bien les mots interdits.
    assert!(simple_problem(Lang::Fr, "Choisis ton raccourci ou ton exe").is_some());
    assert!(simple_problem(Lang::En, "Cloud plus récent").is_some());
    assert!(simple_problem(Lang::En, "Open the .exe").is_some());
    assert_eq!(simple_problem(Lang::Fr, "Un programme ou un jeu"), None);
}

/// Cles citees par le code des ecrans du mode simple (fonctions `*_expert` exclues).
fn simple_screen_keys() -> BTreeSet<String> {
    let mut v = BTreeSet::new();
    for f in ["gui/settings_simple.rs", "gui/add_page.rs", "gui/program_form_ui.rs"] {
        v.extend(keys_in(&without_expert_fns(&read(f))));
    }
    // Carte « Coffre » (lot 8) : fonction du fichier du coffre.
    let vault = read("gui/vault_ui.rs");
    v.extend(keys_in(&fn_body(&vault, "vault_card")));
    v.extend(keys_in(&fn_body(&vault, "vault_card_info")));
    v
}

#[test]
fn le_code_des_ecrans_simples_ne_cite_que_des_textes_verifies() {
    let allowed: BTreeSet<String> = SIMPLE_SETTINGS_KEYS.iter().chain(SIMPLE_ADD_KEYS).map(|k| format!("{:?}", k)).collect();
    let mut unknown = Vec::new();
    for name in simple_screen_keys() {
        if is_v_key(&name) {
            let k = key_by_name(&name).unwrap_or_else(|| panic!("cle {} inconnue", name));
            assert!(SIMPLE_KEYS.contains(&k), "{} : cle 2.0 absente de SIMPLE_KEYS", name);
            continue; // les cles V sont verifiees par SIMPLE_KEYS (v38)
        }
        // Tooltips : un « Tip » cite par un ecran simple est un texte du mode simple, donc verifie ici.
        if !allowed.contains(&name) {
            unknown.push(name);
        }
    }
    // Les infobulles citees dans les ecrans simples passent le meme controle de vocabulaire.
    assert!(unknown.is_empty() || unknown.iter().all(|n| n.starts_with("Tip")), "cles non verifiees dans les ecrans simples : {:?}", unknown);
    let mut tips: Vec<K> = unknown.iter().filter_map(|n| key_by_name(n)).collect();
    tips.sort_by_key(|k| format!("{:?}", k));
    let bad = check_keys(&tips);
    assert!(bad.is_empty(), "infobulles interdites en mode simple :\n{}", bad.join("\n"));
    // Chaque cle V citee est bien un texte du mode simple verifie (v38) : on rejoue le controle sur celles de ces ecrans.
    let vkeys: Vec<K> = simple_screen_keys().iter().filter(|n| is_v_key(n)).filter_map(|n| key_by_name(n)).collect();
    assert!(vkeys.len() > 30, "{} cles V seulement", vkeys.len());
    assert!(check_keys(&vkeys).is_empty());
}

#[test]
fn aucune_mention_d_exe_ni_de_cloud_dans_le_code_visible_des_ecrans_simples() {
    // Aucun litteral de texte « exe » / « cloud » dans les fichiers du mode simple (hors commentaires et fonctions expertes).
    for f in ["gui/settings_simple.rs", "gui/add_page.rs", "gui/program_form_ui.rs"] {
        let code = without_expert_fns(&read(f));
        for line in code.lines() {
            // Verifications techniques (extension d'un fichier) : pas un texte visible.
            if line.contains("ends_with(") || line.contains("extension(") {
                continue;
            }
            // Identifiants Rust (man_exe, exe_path, set_man_exe...) : seuls les litteraux entre guillemets comptent.
            let mut in_str = false;
            let mut lit = String::new();
            for c in line.chars() {
                if c == '"' {
                    in_str = !in_str;
                    lit.push(' ');
                } else if in_str {
                    lit.push(c);
                }
            }
            assert!(simple_problem(Lang::En, &lit).is_none(), "{} : litteral interdit : {}", f, line.trim());
        }
    }
}

#[test]
fn les_textes_expert_gardent_leur_vocabulaire_technique() {
    // Les equivalents techniques de la page Ajouter (mode expert) sont conserves.
    for (simple, expert) in [(K::VAddExistingTitle, K::AddExisting), (K::VAddRejoin, K::BtnJoin), (K::VAddSimilar, K::SimilarAlert), (K::VAddOverlapTitle, K::OverlapTitle), (K::VAddFormProgram, K::AddManual)] {
        assert_eq!(expert_variant(simple), expert);
        assert_ne!(tr(Lang::Fr, simple), tr(Lang::Fr, expert));
    }
    // Le formulaire expert garde le mot « exe » (chemin du programme) : fonction dediee.
    assert!(tr_raw(Lang::Fr, K::AddExe).to_lowercase().contains("exe"));
    assert!(read("gui/manual_ui.rs").contains("K::AddExe"));
}

// ================================================================== cartes simple / expert

#[test]
fn six_cartes_en_mode_simple_et_dix_en_mode_expert() {
    assert_eq!(SIMPLE_CARDS.len(), 6);
    assert_eq!(EXPERT_CARDS.len(), 4);
    assert_eq!(setmode::cards(false), SIMPLE_CARDS.to_vec());
    let e = setmode::cards(true);
    assert_eq!(e.len(), 10);
    for c in SIMPLE_CARDS {
        assert!(e.contains(&c), "{:?} disparait en mode expert", c);
    }
    // Ordre demande : Langue et apparence, Coffre, Appareils, Protection automatique, Mises a jour, Donnees d'usage.
    assert_eq!(SIMPLE_CARDS, [Card::Look, Card::Vault, Card::Devices, Card::Protect, Card::Updates, Card::Usage]);
    let titles: Vec<&str> = SIMPLE_CARDS.iter().map(|c| tr(Lang::Fr, c.title())).collect();
    assert_eq!(titles, vec!["Langue et apparence", "Ton coffre", "Mes appareils", "Protection automatique", "Mises à jour", "Données d'usage"]);
    // Les cartes expertes sont celles de la 1.x que le mode simple ne montre pas.
    let ex: Vec<&str> = EXPERT_CARDS.iter().map(|c| tr(Lang::Fr, c.title())).collect();
    assert_eq!(ex[0], tr(Lang::Fr, K::SetConflictNotif));
    assert_eq!(ex[1], tr(Lang::Fr, K::SetWatch));
    assert!(Card::Watch.expert_only() && !Card::Look.expert_only());
    // Chaque titre existe dans les 5 langues, et celui du coffre garde son vocabulaire technique en mode expert.
    for l in Lang::ALL {
        for c in setmode::cards(true) {
            assert!(!tr(l, c.title()).trim().is_empty());
        }
    }
    assert_eq!(expert_variant(K::VVault), K::SetDest);
}

#[test]
fn le_mode_expert_est_un_seul_reglage_global() {
    let s = agss::settings::Settings::default();
    assert!(!s.expert_mode, "mode simple par defaut");
    // Un ancien settings.json (lot 1) sans le champ reste en mode simple ; avec le champ, il est repris tel quel.
    let old: agss::settings::Settings = serde_json::from_str("{}").unwrap();
    assert!(!old.expert_mode);
    let on: agss::settings::Settings = serde_json::from_str("{\"expert_mode\": true}").unwrap();
    assert!(on.expert_mode);
    let back: agss::settings::Settings = serde_json::from_str(&serde_json::to_string(&on).unwrap()).unwrap();
    assert!(back.expert_mode);
    // Le reglage du lot 1 (carte « Detection ») ET le nouvel interrupteur ne font qu'un : un seul champ, ecrit a un seul endroit.
    let gui = gui_source();
    let writes = gui.matches("settings.expert_mode = ").count();
    assert_eq!(writes, 1, "le mode expert ne doit etre ecrit qu'a un endroit");
    assert!(read("gui/settings_simple.rs").contains("settings.expert_mode = ex"));
    // Plus d'interrupteur « Mode expert » dans la carte de detection.
    assert!(!read("gui/detect_ui.rs").contains("expert_mode ="));
    assert!(ALL_KEYS.iter().all(|k| !format!("{:?}", k).contains("DetExpert")));
    // Libelle unique de l'interrupteur, dans les 5 langues.
    assert_eq!(tr(Lang::Fr, K::VExpertMode), "Mode expert");
    for l in Lang::ALL {
        assert!(!tr_raw(l, K::VSetExpertHint).trim().is_empty());
    }
}

#[test]
fn le_mode_expert_est_le_meme_pour_les_autres_ecrans() {
    // Accueil, fiche, selecteur de dossier, scan, assistant : tous lisent `settings.expert_mode`.
    let gui = gui_source();
    assert!(gui.matches("settings.expert_mode").count() >= 12);
    assert!(read("gui/home_ui.rs").contains("settings.expert_mode"));
    assert!(read("gui/fiche_ui.rs").contains("settings.expert_mode"));
    assert!(read("gui/picker_ui.rs").contains("settings.expert_mode"));
    assert!(read("gui/scan_ui.rs").contains("settings.expert_mode"));
}

// ================================================================== aucune option perdue

/// Toutes les options de la page Reglages de la 1.x (dernier etat avant le lot 11), identifiees par leur nom stable.
const OPTIONS_1X: &[&str] = &[
    // Langue, theme, apparence
    "lang", "theme", "accent_preset", "accent_custom", "restore_appearance",
    // Dossier partage / coffre
    "vault", "vault_path", "vault_size",
    // Mes appareils
    "device_colors", "device_colors_all",
    // Detection
    "detect_enabled", "detect_scope", "card_anim", "detect_ignored", "scan",
    // Demarrage
    "startup_app", "startup_background", "startup_advanced",
    // Conflits et notifications
    "conflict_mode", "notify_conflict", "notify_global",
    // Surveillance
    "watch_buttons", "auto_watch", "reactivity", "big_mode", "pad_mode",
    // Configuration
    "export", "import", "data_dir", "quit", "support", "update_mode",
    // Journal
    "log",
    // Donnees d'usage
    "usage_toggle", "usage_view", "usage_erase", "usage_id",
    // Mode expert (lot 1) et relance de l'assistant (lot 10)
    "expert_mode", "redo_setup",
];

#[test]
fn aucune_option_1x_perdue_toutes_presentes_en_mode_expert() {
    let ids: BTreeSet<&str> = OPTIONS.iter().map(|o| o.id).collect();
    assert_eq!(ids.len(), OPTIONS.len(), "identifiants d'options en double");
    let expert_cards = setmode::cards(true);
    for id in OPTIONS_1X {
        let o = OPTIONS.iter().find(|o| o.id == *id).unwrap_or_else(|| panic!("option 1.x « {} » absente du registre", id));
        assert!(expert_cards.contains(&o.card), "{} : sa carte {:?} n'est pas affichee en mode expert", id, o.card);
        assert!(setmode::options(true).any(|x| x.id == *id), "{} absente en mode expert", id);
    }
    // Le mode simple est un SOUS-ensemble du mode expert, et il cache bien les options techniques.
    let simple: BTreeSet<&str> = setmode::options(false).map(|o| o.id).collect();
    let expert: BTreeSet<&str> = setmode::options(true).map(|o| o.id).collect();
    assert!(simple.is_subset(&expert));
    assert!(expert.len() >= simple.len() + 20, "{} / {}", simple.len(), expert.len());
    for id in ["reactivity", "auto_watch", "conflict_mode", "notify_global", "export", "import", "log", "pad_mode", "vault_path", "detect_scope", "detect_ignored", "usage_id", "accent_custom", "startup_advanced", "data_dir"] {
        assert!(!simple.contains(id), "{} ne doit pas etre en mode simple", id);
        assert!(expert.contains(id));
    }
    for id in ["lang", "theme", "vault", "device_colors", "detect_enabled", "startup_app", "card_anim", "update_mode", "usage_toggle", "usage_view", "usage_erase", "redo_setup", "expert_mode", "scan"] {
        assert!(simple.contains(id), "{} doit etre en mode simple", id);
    }
    // Les options des cartes du mode simple ne sortent jamais d'une carte du mode simple.
    for o in OPTIONS.iter().filter(|o| !o.expert_only) {
        assert!(SIMPLE_CARDS.contains(&o.card) || o.id == "expert_mode", "{} : option simple dans une carte experte", o.id);
    }
}

#[test]
fn chaque_champ_de_settings_json_est_classe_et_chaque_option_a_son_ecran() {
    let v = serde_json::to_value(agss::settings::Settings::default()).unwrap();
    let fields: BTreeSet<String> = v.as_object().unwrap().keys().cloned().collect();
    let known: BTreeSet<String> = SETTING_FIELDS.iter().map(|(n, _)| n.to_string()).collect();
    let missing: Vec<&String> = fields.difference(&known).collect();
    let stale: Vec<&String> = known.difference(&fields).collect();
    assert!(missing.is_empty(), "champs de settings.json non classes (a rattacher a une option ou a marquer internes) : {:?}", missing);
    assert!(stale.is_empty(), "champs classes mais inexistants : {:?}", stale);
    for (name, e) in SETTING_FIELDS {
        if let Exposure::Of(id) = e {
            assert!(OPTIONS.iter().any(|o| o.id == *id), "{} -> option inconnue {}", name, id);
        }
    }
    // Les reglages 1.x les plus visibles sont bien relies a leur option.
    for (f, id) in [("reactivity", "reactivity"), ("conflict_mode", "conflict_mode"), ("pad_mode", "pad_mode"), ("auto_watch", "auto_watch"), ("expert_mode", "expert_mode"), ("card_anim", "card_anim"), ("detect_scope", "detect_scope")] {
        assert!(SETTING_FIELDS.iter().any(|(n, e)| *n == f && *e == Exposure::Of(id)), "{}", f);
    }
}

#[test]
fn le_code_des_options_experts_cite_toujours_leurs_libelles() {
    // Chaque libelle d'option est cite par le code (cartes experts, cartes simples, titres de cartes) : une option dont le
    // libelle n'est plus affiche nulle part serait une fonction perdue.
    let gui = gui_source();
    let setmode_src = read("setmode.rs");
    let cited = keys_in(&gui);
    for o in OPTIONS {
        let name = format!("{:?}", o.label);
        let via_title = SIMPLE_CARDS.iter().chain(EXPERT_CARDS.iter()).any(|c| c.title() == o.label);
        let via_variant = ALL_KEYS.iter().any(|k| expert_variant(*k) == o.label && (cited.contains(&format!("{:?}", k)) || SIMPLE_CARDS.iter().any(|c| c.title() == *k)));
        assert!(cited.contains(&name) || via_title || via_variant, "{} : libelle {} jamais affiche", o.id, name);
    }
    assert!(setmode_src.contains("OPTIONS"));
    // Quelques reglages techniques de la 1.x, nommes un par un : toujours dessines (fonctions *_expert ou cartes expertes).
    let settings = read("gui/settings.rs");
    for k in ["SetReact", "WatchStart", "SetAutoWatch", "SetBig", "SetConflict", "SetNotifConflict", "SetNotif", "BtnExport", "BtnImport", "BtnOpenDataDir", "TrayQuit", "SetSupport", "LogFilterLevel", "BtnRestoreAppearance", "TipAccentCustom", "TipDest", "StartupBackground", "BtnAdvancedStartup", "UsageId"] {
        assert!(settings.contains(&format!("K::{}", k)), "{} n'est plus dessine dans les reglages experts", k);
    }
    assert!(read("gui/detect_ui.rs").contains("K::DetScopeTitle") && read("gui/detect_ui.rs").contains("K::DetIgnoredTitle"));
}

#[test]
fn la_page_reglages_parcourt_les_cartes_du_mode_et_chaque_carte_est_dessinee() {
    let page = read("gui/settings_simple.rs");
    assert!(page.contains("setmode::matching_cards"));
    // Chaque carte du mode simple a sa section ; chaque carte experte est dessinee par la fonction `expert_card_expert`.
    for f in ["look_section", "vault_section", "devices_section", "protect_section", "updates_section", "usage_section"] {
        assert!(page.contains(&format!("fn {}", f)), "{}", f);
    }
    let ex = fn_body(&read("gui/settings.rs"), "expert_card_expert");
    for c in ["Card::Conflicts", "Card::Watch", "Card::Config", "Card::Log"] {
        assert!(ex.contains(c), "{} n'est pas dessinee", c);
    }
    // L'interrupteur est en bas de la page, apres la boucle des cartes.
    assert!(page.find("matching_cards").unwrap() < page.find("K::VExpertMode").unwrap());
}

// ================================================================== recherche (mode expert)

#[test]
fn recherche_des_reglages_en_mode_expert() {
    // Requete vide : toutes les cartes du mode.
    assert_eq!(setmode::matching_cards(Lang::Fr, true, "").len(), 10);
    assert_eq!(setmode::matching_cards(Lang::Fr, false, "  ").len(), 6);
    // Un mot d'une option technique trouve sa carte (casse ignoree).
    assert_eq!(setmode::matching_cards(Lang::Fr, true, "réactivité"), vec![Card::Watch]);
    assert_eq!(setmode::matching_cards(Lang::En, true, "RESPONSIVENESS"), vec![Card::Watch]);
    assert!(setmode::matching_cards(Lang::Fr, true, "journal").contains(&Card::Log));
    assert!(setmode::matching_cards(Lang::Fr, true, "exporter").contains(&Card::Config));
    assert!(setmode::matching_cards(Lang::Fr, true, "conflit").contains(&Card::Conflicts));
    assert!(setmode::matching_cards(Lang::Fr, true, "langue").contains(&Card::Look));
    assert!(setmode::matching_cards(Lang::Fr, true, "zzzz").is_empty());
    // Hors mode expert, la recherche ne revele jamais une carte experte.
    assert!(setmode::matching_cards(Lang::Fr, false, "réactivité").is_empty());
    assert!(setmode::matching_cards(Lang::Fr, false, "journal").is_empty());
    // Textes de la recherche dans les 5 langues.
    for l in Lang::ALL {
        for k in [K::SetSearch, K::SetNoMatch, K::SetExpertGroup, K::SetExpertInside] {
            assert!(!tr_raw(l, k).trim().is_empty());
        }
    }
    assert!(read("gui/settings_simple.rs").contains("search_expert"));
}

// ================================================================== page Ajouter

#[test]
fn trois_grandes_tuiles_et_leurs_textes_dans_les_5_langues() {
    assert_eq!(addpage::TILES.len(), 3);
    assert_eq!(tr(Lang::Fr, K::VFindOnPc), "Chercher sur mon PC");
    assert_eq!(tr(Lang::Fr, K::VAddTileProgram), "Un programme ou un jeu");
    assert_eq!(tr(Lang::Fr, K::VAddTileTarget), "Un dossier ou un fichier");
    assert_eq!(tr(Lang::En, K::VAddTileProgram), "A program or a game");
    for l in Lang::ALL {
        for k in [K::VAddTileProgram, K::VAddTileProgramHint, K::VAddTileProgramHintLx, K::VAddTileTarget, K::VAddTileTargetHint, K::VAddIntro, K::VAddDropHot] {
            assert!(!tr_raw(l, k).trim().is_empty(), "{:?} {}", k, l.code());
        }
    }
    // Sous Linux : lanceur ou AppImage ; sous Windows : le programme ou son raccourci. Jamais « exe ».
    assert!(tr_linux(Lang::Fr, K::VAddTileProgramHint).contains("AppImage"));
    assert!(!tr_raw(Lang::Fr, K::VAddTileProgramHint).contains("AppImage"));
    // La page est construite sur ces trois tuiles.
    let page = read("gui/add_page.rs");
    for k in ["K::VFindOnPc", "K::VAddTileProgram", "K::VAddTileTarget"] {
        assert!(page.contains(k), "{}", k);
    }
    assert!(page.contains("scan_open") && page.contains("open_picker") && page.contains("program_dialog"));
    // Glisser-deposer sur la fenetre.
    assert!(page.contains("dropped_files") && page.contains("classify_drop"));
}

#[test]
fn ancienne_carte_jeux_detectes_supprimee_car_doublon_du_scan() {
    let gui = gui_source();
    for k in ["AddDetected", "AddDetectedHint", "AddInCatalog", "AddNotInCatalog", "AddSelected", "load_detected", "add_detected", "DetRow"] {
        assert!(!gui.contains(k), "{} existe encore", k);
    }
    for name in ["AddDetected", "AddSelected", "TipDetectedCheck", "TipRefreshDetected", "TipAddSelected", "AddInCatalog"] {
        assert!(key_by_name(name).is_none(), "la cle {} devrait avoir disparu", name);
    }
    // Le scan (lot 9) couvre les bibliotheques Steam, Epic, GOG... : la page Ajouter le propose en premiere tuile.
    assert!(read("scan/sources.rs").contains("detect_all"));
}

#[test]
fn glisser_deposer_dossier_programme_ou_fichier() {
    let t = tempfile::TempDir::new().unwrap();
    let folder = t.path().join("Mes notes");
    std::fs::create_dir_all(&folder).unwrap();
    let file = t.path().join("profil.sav");
    std::fs::write(&file, b"x").unwrap();
    let prog = t.path().join("jeu.exe");
    std::fs::write(&prog, b"x").unwrap();
    let resolve = |p: &Path| -> Option<PathBuf> { if p.extension().map(|e| e == "exe").unwrap_or(false) { Some(p.to_path_buf()) } else { None } };
    assert_eq!(addpage::classify_drop(&folder, &resolve), Dropped::Folder(folder.clone()));
    assert_eq!(addpage::classify_drop(&prog, &resolve), Dropped::Program(prog.clone()));
    assert_eq!(addpage::classify_drop(&file, &resolve), Dropped::File(file.clone()));
    // Un raccourci qui pointe ailleurs : le programme resolu remplace le raccourci.
    let target = PathBuf::from("C:/Jeux/vrai.exe");
    let lnk_resolve = |_: &Path| Some(target.clone());
    assert_eq!(addpage::classify_drop(&file, &lnk_resolve), Dropped::Program(target));
}

#[test]
fn nature_du_formulaire_et_nom_propose() {
    assert_eq!(addpage::form_kind("", false), FormKind::Hidden);
    assert_eq!(addpage::form_kind("C:/x/jeu.exe", false), FormKind::Program);
    assert_eq!(addpage::form_kind("", true), FormKind::Target);
    assert_eq!(addpage::target_name(Path::new("C:/Users/a/Documents/mes notes"), false), "Mes notes");
    assert_eq!(addpage::target_name(Path::new("/home/a/save.dat"), true), "Save");
    // Un dossier ou un fichier sans programme : copie quotidienne a midi, valide sans rien d'autre a regler.
    use agss::addform::{validity, FormIn, Validity};
    let sched = agss::schedule::Schedule::fixed(vec![12 * 3600]);
    let ok = FormIn { exe: "", name: "Mes notes", path: "C:/x", path_ok: true, schedule: Some(&sched) };
    assert_eq!(validity(&ok), Validity::Ready);
    assert_eq!(validity(&FormIn { name: "", ..ok }), Validity::NeedName);
    assert_eq!(validity(&FormIn { path_ok: false, ..ok }), Validity::NeedPath);
    assert_eq!(validity(&FormIn { schedule: None, ..ok }), Validity::NeedSchedule);
    // Un programme n'a pas besoin de planification.
    assert_eq!(validity(&FormIn { exe: "C:/x/jeu.exe", schedule: None, ..ok }), Validity::Ready);
}

#[test]
fn le_formulaire_utilise_les_couches_de_connaissance_et_le_selecteur_guide() {
    let form = read("gui/program_form_ui.rs");
    // « Emplacement trouve · Confiance : elevee » (couches de connaissance du lot 2) et selecteur guide (lot 8).
    for k in ["K::KnowFound", "K::KnowConfidence", "K::KnowHigh", "K::KnowSrcLauncher", "K::VAddChoosePlace", "K::VAddWhereTitle"] {
        assert!(form.contains(k), "{}", k);
    }
    assert!(form.contains("open_picker") && form.contains("pick_opts_for_program") && form.contains("man_found"));
    assert!(form.contains("knowledge::level"));
    assert_eq!(tr(Lang::Fr, K::KnowFound), "Emplacement trouvé");
    assert_eq!(tr(Lang::Fr, K::KnowConfidence).replace("{}", tr(Lang::Fr, K::KnowHigh)), "Confiance : élevée");
    // Plus de boite du systeme pour choisir l'emplacement (lot 8), plus de zone « exe ».
    assert!(!form.contains("pick_folder"));
    let manual = read("gui/manual_ui.rs");
    assert!(!manual.contains("fn manual_card") && !manual.contains("fn exe_drop_zone"));
    assert!(!gui_source().contains("K::AddStep1") && !gui_source().contains("K::AddDropHere"));
    // Les options avancees sont reservees au mode expert.
    assert!(form.contains("if expert {") && form.contains("form_advanced_expert"));
    assert!(fn_body(&manual, "form_advanced_expert").contains("K::AddAdvanced"));
}

#[test]
fn les_options_avancees_du_formulaire_ne_sont_pas_dans_les_fichiers_du_mode_simple() {
    let simple = [read("gui/add_page.rs"), read("gui/program_form_ui.rs")].join("\n");
    let code = without_expert_fns(&simple);
    for k in ["K::AddAdvanced", "K::FilterLabel", "K::LocAdd", "K::SchedTitle", "K::AddExe", "K::AddName", "K::TargetFolder", "K::AddSavePath"] {
        assert!(!code.contains(k), "{} est une option avancee : elle ne doit pas etre dans le formulaire simple", k);
    }
    // ... mais le mode expert les a toujours.
    let adv = fn_body(&read("gui/manual_ui.rs"), "form_advanced_expert");
    for k in ["K::AddAdvanced", "K::FilterLabel", "K::LocAdd", "K::AddExe", "K::AddName", "K::TargetFolder", "K::AddSavePath"] {
        assert!(adv.contains(k), "{} perdu en mode expert", k);
    }
    assert!(read("gui/manual_ui.rs").contains("sched_block"));
}

#[test]
fn l_etape_5_de_l_assistant_reutilise_le_nouveau_formulaire() {
    let wiz = read("gui/wizard_ui.rs");
    assert!(wiz.contains("self.add_form(ui, ctx)"), "l'etape 5 doit afficher le nouveau formulaire");
    assert!(!wiz.contains("manual_card"));
    assert!(wiz.contains("add_start_program"));
    assert!(gui_source().matches("fn add_form(").count() == 1);
}

#[test]
fn textes_du_formulaire_et_des_alertes_dans_les_5_langues() {
    let keys = [
        K::VAddFormProgram, K::VAddFormTarget, K::VAddChangeProgram, K::VAddWhereTitle, K::VAddWhereHint, K::VAddUseThis, K::VAddPlaceChosen, K::VAddChoosePlace,
        K::VAddTargetFolder, K::VAddTargetFile, K::VAddNameLabel, K::VAddNameHint, K::VAddDailyNote, K::VAddNeedName, K::VAddNeedPath, K::VAddedToast,
        K::VAddFailedToast, K::VAddExistingTitle, K::VAddExistingHint, K::VAddRejoin, K::VAddSimilar, K::VAddOverlapTitle, K::VAddOverlapBody, K::VProgramFilter,
        K::VAddPlaceMissing, K::VAddPlaceSeveral, K::VAddFromOther,
    ];
    for k in keys {
        let en = tr_raw(Lang::En, k);
        for l in Lang::ALL {
            let s = tr_raw(l, k);
            assert!(!s.trim().is_empty(), "{:?} {}", k, l.code());
            assert_eq!(s.matches("{}").count(), en.matches("{}").count(), "{:?} {}", k, l.code());
        }
        assert!(SIMPLE_KEYS.contains(&k), "{:?} absente de SIMPLE_KEYS", k);
    }
    assert_eq!(tr(Lang::Fr, K::VAddedToast).replace("{}", "Celeste"), "« Celeste » est maintenant protégé.");
    assert_eq!(tr(Lang::Fr, K::VProtectIt), "Protège-le !");
}

// ================================================================== composant : grande tuile de choix

fn draw_choice(width: f32, horizontal: bool, hot: bool) -> egui::Rect {
    let ctx = egui::Context::default();
    let mut out = egui::Rect::NOTHING;
    for t in [0.0, 1.0] {
        let raw = egui::RawInput { time: Some(t), screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))), ..Default::default() };
        let _ = ctx.run(raw, |ctx| {
            ui::install(ctx, Tokens::default_dark(), false, Motion::default());
            egui::CentralPanel::default().show(ctx, |ui| {
                let spec = ChoiceSpec { glyph: Glyph::Folder, title: tr(Lang::Fr, K::VAddTileTarget), hint: tr(Lang::Fr, K::VAddTileTargetHint), hot, index: 2, epoch: 1, width, horizontal };
                out = ui::choice_tile(ui, "t", &spec).rect;
            });
        });
    }
    out
}

#[test]
fn grande_tuile_de_choix_dessinee_sur_un_vrai_egui() {
    let r = draw_choice(280.0, false, false);
    assert_eq!(r.width(), 280.0);
    assert_eq!(r.height(), ui::choice_height(false, false));
    let h = draw_choice(500.0, true, true);
    assert_eq!(h.width(), 500.0);
    assert_eq!(h.height(), ui::choice_height(true, false));
    assert!(ui::choice_height(false, true) > ui::choice_height(false, false), "plus grande en grand format");
    // Trois tuiles tiennent sur une ligne des 720 px, et une tuile par ligne en dessous.
    for w in [720.0_f32, 900.0, 1100.0] {
        let cols = addpage::tile_columns(w);
        assert_eq!(cols, 3);
        let tw = addpage::tile_width(w, cols, ui::tokens::TILE_GAP);
        assert!((tw * 3.0 + ui::tokens::TILE_GAP * 2.0 - w).abs() < 0.01);
    }
    assert_eq!(addpage::tile_columns(420.0), 1);
    assert_eq!(addpage::tile_columns(600.0), 1);
}
