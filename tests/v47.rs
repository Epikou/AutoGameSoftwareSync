//! 2.0.1 : alerte poids (seuil unique, impact, choix), recherche de l'accueil, statistiques en bas de la derniere etape
//! (jamais de pre-selection, rappel unique), elements masques du scan (emplacement choisi), categorie Windows decochee,
//! fluidite de l'interface (aucun travail lourd ni attente de fil de fond dans le fil de l'interface).
//! UNIQUEMENT des dossiers temporaires ; aucun envoi reel (transport simule).
use agss::home::{self, Filter, Kind};
use agss::i18n::{tr, Lang, K};
use agss::onboarding::{self as ob, Facts, Moved, Nav, Onboarding};
use agss::process::ProcessChecker;
use agss::scan::{self, Hide, Origin, Personal, Size, Tier};
use agss::settings::Settings;
use agss::usage::{self, Transport};
use agss::weight::{self, Choice};
use agss::{Ctx, Res};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use tempfile::TempDir;

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(false)
    }
}

fn src(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

fn gui_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("gui");
    for e in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push((p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(&p).unwrap()));
        }
    }
    out
}

// ------------------------------------------------------------------ alerte poids : seuil et impact

#[test]
fn seuil_500_mo_par_defaut_unique_et_reglable() {
    assert_eq!(Settings::default().heavy_limit_mb, 500);
    assert_eq!(weight::limit_mb(), 500, "seuil par defaut du processus");
    // strictement au-dessus du seuil
    assert!(!weight::is_heavy(500 * MB) && weight::is_heavy(500 * MB + 1));
    assert!(!weight::is_heavy_at(2 * GB, 4096) && weight::is_heavy_at(2 * GB, 1024));
    // un ancien settings.json sans le champ garde 500 ; une valeur absurde est ramenee dans les bornes
    let t = TempDir::new().unwrap();
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr"}"#).unwrap();
    assert_eq!(Settings::load_from(t.path()).unwrap().heavy_limit_mb, 500);
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr","heavy_limit_mb":0}"#).unwrap();
    assert_eq!(Settings::load_from(t.path()).unwrap().heavy_limit_mb, weight::MIN_LIMIT_MB);
    // la regle des 500 Mo du lot 10 (dossiers perso) et l'alerte partagent CE seuil : plus aucune constante en dur
    let scan_rs = src("src/scan.rs");
    assert!(scan_rs.contains("crate::weight::is_heavy(s.bytes)") && scan_rs.contains("crate::weight::is_heavy(bytes)"));
    assert!(!scan_rs.contains("s.bytes > PERSONAL_AUTO_MAX"), "la regle des dossiers perso lit le seuil unique");
    assert!(src("src/migrate.rs").contains("crate::weight::is_heavy(plan.bytes)"));
    assert_eq!(scan::PERSONAL_AUTO_MAX, 500 * MB, "valeur par defaut seulement");
}

#[test]
fn impact_taille_fois_versions_comme_dans_l_exemple_de_l_auteur() {
    let b = (4.2 * GB as f64) as u64;
    let a = weight::alert(b, 3).expect("4,2 Go est lourd");
    assert_eq!((a.bytes, a.keep), (b, 3));
    assert_eq!(a.impact, b * 3);
    // « 4,2 Go · environ 12,6 Go dans ton coffre avec 3 versions »
    let line = tr(Lang::Fr, K::VHeavyLine).replace("{0}", &agss::i18n::format_bytes(Lang::Fr, b)).replace("{1}", &agss::i18n::format_bytes(Lang::Fr, a.impact)).replace("{2}", "3");
    assert_eq!(line, "4,2 Go · environ 12,6 Go dans ton coffre avec 3 versions");
    assert!(weight::alert(400 * MB, 3).is_none(), "sous le seuil : pas d'alerte");
    // les 5 langues portent les trois marqueurs
    for l in Lang::ALL {
        let s = if l == Lang::Zh { agss::i18n_zh::tr(K::VHeavyLine) } else { agss::i18n::tr_raw(l, K::VHeavyLine) };
        for m in ["{0}", "{1}", "{2}"] {
            assert!(s.contains(m), "{} : {}", l.code(), s);
        }
        assert!(!agss::i18n::tr_raw(l, K::VHeavyKeep).trim().is_empty());
    }
}

#[test]
fn trois_choix_garder_moins_de_versions_annuler() {
    assert_eq!(weight::keep_for_choice(Choice::Keep), Some(weight::HEAVY_KEEP));
    assert_eq!(weight::keep_for_choice(Choice::Fewer), Some(agss::backup::MIN_KEEP), "jamais sous le plancher de securite");
    assert_eq!(weight::keep_for_choice(Choice::Cancel), None);
    assert!(weight::fewer_keep() < weight::HEAVY_KEEP);
    assert_eq!(weight::default_keep(10 * MB), agss::backup::DEFAULT_KEEP);
    assert_eq!(weight::default_keep(600 * MB), weight::HEAVY_KEEP);
}

fn personal(p: &Path, which: Personal) -> scan::Item {
    let mut it = scan::folder_item(p);
    it.origin = Origin::Personal(which);
    it.name = "Documents".into();
    it.key = "folders:documents".into();
    it.newest_ms = Some(agss::util::now_ms());
    it
}

#[test]
fn ajout_lourd_avec_versions_choisies_et_deja_signale() {
    let t = TempDir::new().unwrap();
    let mut ctx = Ctx::new(t.path().join("data"));
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let mk = |n: &str| {
        let d = t.path().join(n);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "x").unwrap();
        scan::folder_item(&d)
    };
    let (keep3, fewer, light) = (mk("Gros3"), mk("Gros2"), mk("Petit"));
    let mut sizes: HashMap<String, Size> = HashMap::new();
    sizes.insert(keep3.key.clone(), Size { bytes: 4 * GB, files: 2, partial: false });
    sizes.insert(fewer.key.clone(), Size { bytes: 4 * GB, files: 2, partial: false });
    sizes.insert(light.key.clone(), Size { bytes: 10 * MB, files: 2, partial: false });
    let mut choice: HashMap<String, usize> = HashMap::new();
    choice.insert(keep3.key.clone(), weight::keep_for_choice(Choice::Keep).unwrap());
    choice.insert(fewer.key.clone(), weight::keep_for_choice(Choice::Fewer).unwrap());
    let rep = scan::add_items_with_keep(&ctx, &[keep3, fewer, light], &sizes, &|_| None, "manual", &choice);
    assert_eq!(rep.added.len(), 3, "{:?}", rep.skipped);
    let s = Settings::load(&ctx).unwrap();
    assert_eq!(s.per_game.get("Gros3").and_then(|g| g.keep_versions), Some(3));
    assert_eq!(s.per_game.get("Gros2").and_then(|g| g.keep_versions), Some(2));
    assert_eq!(s.per_game.get("Petit").and_then(|g| g.keep_versions), None, "petit : reglage par defaut");
    // les lourds ont ete signales a l'ajout : pas de toast « devenu lourd » plus tard
    assert!(s.heavy_notified.contains(&"Gros3".to_string()) && s.heavy_notified.contains(&"Gros2".to_string()));
    assert!(!s.heavy_notified.contains(&"Petit".to_string()));
}

#[test]
fn element_qui_grossit_signale_une_seule_fois() {
    let sizes = vec![("Gros".to_string(), 600 * MB), ("Petit".to_string(), 10 * MB)];
    let mut notified: BTreeSet<String> = BTreeSet::new();
    assert_eq!(weight::newly_heavy(&sizes, &notified), vec!["Gros".to_string()]);
    notified.insert("Gros".into());
    assert!(weight::newly_heavy(&sizes, &notified).is_empty(), "un seul toast par element");
    // un autre element franchit le seuil plus tard
    let later = vec![("Gros".to_string(), 700 * MB), ("Petit".to_string(), 900 * MB)];
    assert_eq!(weight::newly_heavy(&later, &notified), vec!["Petit".to_string()]);
    // l'ecran : pastille « Lourd » sur la tuile, toast groupe, bloc de la fiche, alerte au scan et a l'ajout
    let w = src("src/gui/weight_ui.rs");
    assert!(w.contains("heavy_notified") && w.contains("VHeavyGrownOne") && w.contains("fiche_heavy_block"));
    assert!(src("src/gui/ui/tile.rs").contains("spec.heavy"));
    assert!(src("src/gui/home_ui.rs").contains("heavy: it.heavy.then"));
    assert!(src("src/gui/program_form_ui.rs").contains("self.weight_gate_game(ctx, g, ep)"), "tout ajout passe par l'alerte poids");
    assert!(src("src/gui/wizard_ui.rs").contains("self.weight_gate_items(ctx, items, source)"));
}

// ------------------------------------------------------------------ recherche de l'accueil

#[test]
fn recherche_insensible_aux_accents_et_a_la_casse() {
    assert_eq!(home::fold("Pokémon  ÉCLAIR"), "pokemon eclair");
    assert_eq!(home::fold("Crème Brûlée Œuf Straße"), "creme brulee oeuf strasse");
    assert!(home::matches_query(&["Pokémon Violet"], "pokemon"));
    assert!(home::matches_query(&["Pokémon Violet"], "POKÉMON vio"));
    assert!(home::matches_query(&["Hollow Knight"], "knight hollow"), "chaque mot, dans n'importe quel ordre");
    assert!(!home::matches_query(&["Hollow Knight"], "celeste"));
    assert!(home::matches_query(&["Hollow Knight"], "   "), "recherche vide : tout");
    // plusieurs noms (nom affiche, nom d'origine)
    assert!(home::matches_query(&["Mon jeu", "hollow-knight"], "hollow"));
    // les autres ecritures ne changent que de casse
    assert!(home::matches_query(&["Мои Документы"], "документы"));
    assert!(home::matches_query(&["我的文档"], "文档"));
}

#[test]
fn recherche_combinee_aux_filtres_et_compteurs() {
    let rows: Vec<(Kind, Vec<&str>)> = vec![
        (Kind::Game, vec!["Hollow Knight"]),
        (Kind::Game, vec!["Pokémon Violet"]),
        (Kind::App, vec!["OBS Studio"]),
        (Kind::App, vec!["Pokémon Home Tool"]),
        (Kind::Folder, vec!["Documents"]),
    ];
    assert_eq!(home::visible_indices(&rows, Filter::All, "pokemon"), vec![1, 3]);
    assert_eq!(home::visible_indices(&rows, Filter::Games, "pokemon"), vec![1], "filtre Jeux + recherche");
    assert_eq!(home::visible_indices(&rows, Filter::Apps, "pokémon"), vec![3]);
    assert!(home::visible_indices(&rows, Filter::Folders, "pokemon").is_empty(), "etat vide");
    assert_eq!(home::visible_indices(&rows, Filter::All, ""), vec![0, 1, 2, 3, 4]);
    assert_eq!(home::filter_counts_for(&rows, "pokemon"), [2, 1, 1, 0], "compteurs de chaque filtre pour la recherche");
    assert_eq!(tr(Lang::Fr, K::VSearchEmpty).replace("{}", "xyz"), "Aucun résultat pour « xyz »");
    for l in Lang::ALL {
        let s = if l == Lang::Zh { agss::i18n_zh::tr(K::VSearchEmpty) } else { agss::i18n::tr_raw(l, K::VSearchEmpty) };
        assert!(s.contains("{}"), "{}", l.code());
    }
}

#[test]
fn recherche_visible_a_partir_de_douze_elements_clavier() {
    assert!(!home::search_visible(11) && home::search_visible(12) && home::search_visible(40));
    let h = src("src/gui/home_ui.rs");
    assert!(h.contains("home::search_visible(items.len())"));
    // Ctrl+F, « / » et Echap
    assert!(h.contains("egui::Key::F") && h.contains("egui::Key::Slash") && h.contains("egui::Key::Escape"));
    assert!(h.contains("self.home.query.clear()"));
}

// ------------------------------------------------------------------ statistiques : rien de pre-selectionne, rappel unique

struct Mock {
    calls: std::cell::RefCell<Vec<String>>,
}
impl Transport for Mock {
    fn post(&self, path: &str, _body: &[u8]) -> Result<u16, String> {
        self.calls.borrow_mut().push(path.to_string());
        Ok(200)
    }
    fn get(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
        self.calls.borrow_mut().push(path.to_string());
        Ok((200, br#"{"v":1,"entries":[]}"#.to_vec()))
    }
}

#[test]
fn consentement_non_pre_selectionne_aucun_envoi_sans_clic() {
    // rien n'est pre-coche (RGPD, arret Planet49)
    let s = Settings::default();
    assert_eq!(s.usage_consent, None);
    assert!(usage::needs_question(s.usage_asked.as_deref(), "2.0.1"));
    // tant qu'on n'a pas clique : rien en file, rien d'envoye
    let t = TempDir::new().unwrap();
    let now = agss::util::ms_from_civil(2026, 10, 27, 12, 0, 0, 0) as u64;
    assert!(!usage::push(t.path(), usage::Event::new("app_start", &[("version", "2.0.1")], now), None), "sans reponse : rien en file");
    let m = Mock { calls: Default::default() };
    let o = usage::run_once(t.path(), None, now, &m, &|| false);
    assert_eq!((o.sent_events, o.sent_records), (0, 0));
    assert!(m.calls.borrow().is_empty(), "aucun contact avec le serveur sans consentement : {:?}", m.calls.borrow());
    // « Non merci » : meme chose
    let o = usage::run_once(t.path(), Some(false), now, &m, &|| false);
    assert_eq!(o.sent_events, 0);
    assert!(m.calls.borrow().is_empty());
    // le code : aucun bouton n'a le focus d'office, aucune valeur par defaut a Some(true)
    let u = src("src/gui/usage_ui.rs");
    assert!(u.contains("fn consent_line") && !u.contains("set_default"));
    assert!(!src("src/settings.rs").contains("usage_consent: Some"));
}

#[test]
fn statistiques_en_bas_de_la_derniere_etape_et_plus_dans_l_etape_1() {
    // l'etape 1 ne porte plus la question ; la ligne est dessinee par l'assistant sur le dernier ecran seulement
    let w = src("src/gui/wizard_ui.rs");
    let start = w.find("fn wizard_step_welcome").unwrap();
    let end = w[start..].find("fn wizard_step_vault").unwrap();
    let step1 = &w[start..start + end];
    for banned in ["BtnUsageYes", "BtnUsageNo", "VConsentBody", "consent_line", "set_usage_consent"] {
        assert!(!step1.contains(banned), "etape 1 : {}", banned);
    }
    assert!(w.contains("self.consent_line(ui)") && w.contains("fn wizard_is_last"));
    // une mise a jour depuis la 1.x : plus d'etape pour les statistiques, la question est sous « Quoi de neuf »
    let upd = Facts { first_install: false, has_vault: true, has_items: true, consent_needed: true };
    assert_eq!(ob::plan_for(&upd), (0, true));
    let upd2 = Facts { first_install: false, has_vault: false, has_items: false, consent_needed: true };
    let (plan, wn) = ob::plan_for(&upd2);
    assert!(wn && plan & ob::bit(ob::WELCOME) == 0, "jamais l'etape 1 pour les statistiques");
    assert_eq!(ob::plan_for(&Facts { first_install: true, has_vault: false, has_items: false, consent_needed: true }), (ob::ALL, false));
    // texte : question, boutons, lien (5 langues), detail derriere le lien
    assert_eq!(tr(Lang::Fr, K::VConsentTitle), "Aider l'auteur avec des statistiques anonymes ?");
    assert_eq!(tr(Lang::Fr, K::BtnUsageYes), "Oui, j'aide");
    assert_eq!(tr(Lang::Fr, K::BtnUsageNo), "Non merci");
    assert_eq!(tr(Lang::Fr, K::BtnUsageView), "Voir ce qui est envoyé");
    assert!(tr(Lang::Fr, K::VConsentBody).contains("CONNUS") && tr(Lang::Fr, K::VConsentNever).contains("adresse IP"));
    let u = src("src/gui/usage_ui.rs");
    assert!(u.contains("tk.ok") && u.contains("tk.widget"), "Oui en vert, Non en gris discret (jamais rouge)");
    assert!(!u.contains("Danger"));
    // le detail complet n'est pas affiche sur la ligne : infobulle et fenetre
    assert!(u.contains("on_hover_text(format!(\"{}\\n\\n{}\", self.t(K::VConsentBody)") && u.contains("fn consent_info_window"));
}

#[test]
fn la_question_revient_une_seule_fois_puis_plus_jamais() {
    let day = 24 * 3_600_000u64;
    // assistant termine sans reponse : rappel programme (une fois)
    let l = usage::late_after_wizard(true, usage::LATE_NONE);
    assert_eq!(l, usage::LATE_PENDING);
    assert_eq!(usage::late_after_wizard(true, usage::LATE_SHOWN), usage::LATE_SHOWN, "jamais reprogramme");
    assert_eq!(usage::late_after_wizard(false, usage::LATE_NONE), usage::LATE_NONE, "deja repondu : rien");
    // pas dans la session qui l'a programmee ; au prochain lancement ou apres 24 h
    let t0 = 1_000_000u64;
    assert!(!usage::late_card_due(true, l, false, true, t0, t0 + 1000));
    assert!(usage::late_card_due(true, l, false, true, t0, t0 + day));
    assert!(usage::late_card_due(true, l, false, false, t0, t0 + 1000), "nouveau lancement");
    // jamais pendant l'assistant, ni si repondu, ni une fois montree
    assert!(!usage::late_card_due(true, l, true, false, t0, t0 + day));
    assert!(!usage::late_card_due(false, l, false, false, t0, t0 + day));
    assert!(!usage::late_card_due(true, usage::LATE_SHOWN, false, false, t0, t0 + 10 * day), "plus jamais");
    // au lancement : une question restee sans reponse (2.0.0) est programmee une fois ; l'assistant a faire = non
    assert_eq!(usage::late_at_start(true, usage::LATE_NONE, false), usage::LATE_PENDING);
    assert_eq!(usage::late_at_start(true, usage::LATE_NONE, true), usage::LATE_NONE);
    assert_eq!(usage::late_at_start(true, usage::LATE_SHOWN, false), usage::LATE_SHOWN);
    // le parcours complet : terminer l'assistant sans choisir programme le rappel, « Terminer plus tard » non
    let mut o = Onboarding::default();
    ob::begin(&mut o, &Facts { first_install: true, has_vault: false, has_items: false, consent_needed: true });
    let mut last = Moved::Stay;
    for _ in 0..6 {
        last = ob::navigate(&mut o, Nav::Later, false);
    }
    assert_eq!(last, Moved::Finished);
    // le code : montree des l'affichage (marquee « montree »), repondre vide le rappel
    let u = src("src/gui/usage_ui.rs");
    assert!(u.contains("usage::LATE_SHOWN") && u.contains("s.usage_late = usage::LATE_NONE"));
    assert!(src("src/gui/wizard_ui.rs").contains("self.usage_schedule_late()"));
    assert!(!src("src/gui/wizard_ui.rs").split("fn wizard_close_for_now").nth(1).unwrap().split("fn ").next().unwrap().contains("usage_schedule_late"));
}

// ------------------------------------------------------------------ elements masques du scan

fn handle_with(items: Vec<scan::Item>) -> scan::Handle {
    scan::Handle { state: std::sync::Arc::new(std::sync::Mutex::new(scan::Progress { items, finished: true, step: 4, ..Default::default() })), cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)) }
}

fn hidden(name: &str, hide: Hide, exe: Option<&str>) -> scan::Item {
    let mut it = scan::folder_item(Path::new("."));
    it.key = format!("app:{}", name.to_lowercase());
    it.name = name.into();
    it.cat = scan::Cat::Apps;
    it.origin = Origin::Installed;
    it.locs.clear();
    it.tier = Tier::Hidden;
    it.hide = Some(hide);
    it.exe = exe.map(|e| e.to_string());
    it
}

#[test]
fn masque_rien_trouve_passe_en_peut_etre_des_qu_un_emplacement_est_connu() {
    let t = TempDir::new().unwrap();
    let data = t.path().join("Zorg");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("save.dat"), "x").unwrap();
    let h = handle_with(vec![hidden("Zorg", Hide::NoData, Some("zorg.exe")), hidden("Git", Hide::DevTool, None), hidden("Pilote", Hide::System, None)]);
    let v = h.view();
    assert!(!v.items[0].selectable(), "masque : pas de case");
    // emplacement inexistant : refus
    assert!(!scan::place_item(&h, "app:zorg", scan::Loc::dir(t.path().join("absent"))));
    // emplacement connu : « Peut-etre », avec sa case, la raison de masquage disparait
    assert!(scan::place_item(&h, "app:zorg", scan::Loc::dir(data.clone())));
    let v = h.view();
    let z = &v.items[0];
    assert_eq!((z.tier, z.hide), (Tier::Maybe, None));
    assert!(z.selectable() && z.signs.contains(&scan::Sign::UserPlace));
    assert_eq!(z.locs[0].path, data);
    assert!(z.newest_ms.is_some() && (40..=69).contains(&z.score));
    assert_eq!(v.counts[scan::Cat::Apps.index()], 1);
    // taille mesuree ensuite
    scan::measure_one(&h, "app:zorg", &|| {});
    assert_eq!(h.view().sizes.get("app:zorg").map(|s| s.bytes), Some(1));
    // outils de developpement et programmes systeme : AUCUNE action, ils restent masques avec leur raison
    assert!(!scan::place_item(&h, "app:git", scan::Loc::dir(data.clone())));
    assert!(!scan::place_item(&h, "app:pilote", scan::Loc::dir(data.clone())));
    let v = h.view();
    assert_eq!((v.items[1].tier, v.items[1].hide), (Tier::Hidden, Some(Hide::DevTool)));
    assert_eq!((v.items[2].tier, v.items[2].hide), (Tier::Hidden, Some(Hide::System)));
}

#[test]
fn masques_deux_actions_seulement_pour_rien_trouve() {
    let s = src("src/gui/scan_ui.rs");
    // choisir l'emplacement (selecteur guide du lot 8) et l'apprendre (mode apprentissage du lot 2, comme la page Ajouter)
    assert!(s.contains("VScanPlace") && s.contains("VScanLearn") && s.contains("self.open_picker(ctx, PLACE_KEY"));
    assert!(s.contains("LearnList::load") && s.contains("l.add(&exe"));
    assert!(s.contains("it.tier == Tier::Hidden && matches!(it.hide, Some(Hide::NoData))"), "seulement « rien a proteger trouve »");
    assert!(s.contains("scan::place_item"));
    // la raison reste affichee
    assert!(s.contains("hide_text(scan::hide_reason(it))"));
    for l in Lang::ALL {
        assert_ne!(agss::i18n::tr_raw(l, K::VScanPlace), agss::i18n::tr_raw(l, K::VScanLearn));
    }
    // la page Ajouter utilise la meme liste d'apprentissage
    assert!(src("src/gui/program_form_ui.rs").contains("LearnList::load"));
}

// ------------------------------------------------------------------ categorie Windows

#[test]
fn elements_windows_jamais_coches_d_office() {
    let t = TempDir::new().unwrap();
    for sys in [scan::SysItem::DesktopShortcuts, scan::SysItem::StartMenu, scan::SysItem::Taskbar, scan::SysItem::Wallpaper, scan::SysItem::RegistryPrefs] {
        let mut it = scan::folder_item(t.path());
        it.cat = scan::Cat::System;
        it.origin = Origin::System(sys);
        it.tier = Tier::Recommended;
        it.score = 90;
        assert!(!it.default_checked(), "{:?} : jamais coche d'office", sys);
        assert!(it.selectable(), "{:?} : reste propose et choisissable", sys);
    }
    // les autres categories recommandees restent cochees d'office
    let mut it = scan::folder_item(t.path());
    it.cat = scan::Cat::Games;
    assert!(it.default_checked());
    // premier lancement : meme regle (l'assistant lit `default_checked`)
    assert!(src("src/gui/scan_ui.rs").contains("it.default_checked()"));
}

// ------------------------------------------------------------------ fluidite de l'interface

#[test]
fn aucune_attente_de_fil_de_fond_dans_l_interface() {
    // La cause du « Ne repond pas » : l'interface attendait (join) la fin d'une grosse copie pour redemarrer la surveillance.
    let st = src("src/gui/state.rs");
    assert!(st.contains("fn change_watch_async") && st.contains("pub fn stop_watch_async"));
    let restart = st.split("pub fn restart_watch").nth(1).unwrap().split("fn change_watch_async").next().unwrap();
    assert!(restart.contains("change_watch_async(true)") && !restart.contains("stop_watch()"), "restart_watch ne bloque plus");
    // nulle part ailleurs dans les ecrans : ni stop_watch() bloquant ni join()
    for (name, text) in gui_sources() {
        if name == "state.rs" {
            continue;
        }
        assert!(!text.contains(".stop_watch()"), "{} : stop_watch() bloquant dans l'interface", name);
        assert!(!text.contains(".join()"), "{} : join() dans l'interface", name);
    }
    // fermeture : la fenetre disparait avant d'attendre la fin d'une copie
    let quit = st.split("pub fn quit").nth(1).unwrap();
    assert!(quit.find("hide_window").unwrap() < quit.find("stop_watch").unwrap());
    // garde-fou : mesure de chaque image + veilleur
    assert!(src("src/gui.rs").contains("frame_guard::begin") && src("src/gui/init.rs").contains("frame_guard::spawn_watchdog"));
}

#[test]
fn les_ecrans_mesurent_dans_un_fil_de_fond() {
    // la taille d'un element lourd est mesuree hors du fil de l'interface
    let w = src("src/gui/weight_ui.rs");
    assert!(w.contains("std::thread::Builder::new().name(\"agss-weight\"") && w.contains("scan::measure("));
    let measure_calls = w.matches("scan::measure(").count();
    assert_eq!(measure_calls, 1, "un seul appel, dans le fil de fond");
    // aucun parcours de dossier ni hachage direct dans les ecrans de l'alerte et de la recherche
    for banned in ["walkdir", "hash_file", "read_dir", "fs::copy"] {
        assert!(!w.contains(banned), "weight_ui : {}", banned);
    }
}

// ------------------------------------------------------------------ textes

#[test]
fn textes_2_0_1_dans_les_cinq_langues() {
    let keys = [
        K::VHeavyPill, K::VHeavyTitle, K::VHeavyLine, K::VHeavyBody, K::VHeavyKeep, K::VHeavyFewer, K::VHeavyFewerShort, K::VHeavyFewerTip, K::VHeavyCancelAdd, K::VHeavyCancelled,
        K::VHeavyDontProtect, K::VHeavyGrownOne, K::VHeavyGrownMany, K::VHeavyGrownManyHint, K::VHeavyKeptToast, K::VHeavyFewerToast, K::VHeavyFewerToastHint, K::VHeavyTip,
        K::VHeavyLimit, K::VHeavyLimitTip, K::VSearchHint, K::VSearchEmpty, K::VSearchTip, K::VSearchClear, K::VScanPlace, K::VScanLearn, K::VScanLearnArmed, K::VScanPlaced,
        K::VScanUnticked,
    ];
    for k in keys {
        let fr = agss::i18n::tr_raw(Lang::Fr, k);
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { agss::i18n::tr_raw(l, k) };
            assert!(!s.trim().is_empty(), "{:?} vide en {}", k, l.code());
            if l != Lang::Fr {
                assert_ne!(s, fr, "{:?} non traduit en {}", k, l.code());
            }
        }
    }
    // le vocabulaire du mode simple reste propre (« synchro », « exe », « cloud », « save »)
    for k in keys {
        for l in Lang::ALL {
            let s = (if l == Lang::Zh { agss::i18n_zh::tr(k) } else { agss::i18n::tr_raw(l, k) }).to_lowercase();
            for bad in ["synchro ", " exe", "cloud", " save"] {
                assert!(!s.contains(bad), "{:?}/{}: {}", k, l.code(), s);
            }
        }
    }
    // jamais l'ancien vocabulaire dans les nouveaux ecrans
    for f in ["weight_ui.rs", "usage_ui.rs"] {
        let s = src(&format!("src/gui/{}", f));
        assert!(!s.to_lowercase().contains("claude"));
    }
    let _personal = personal;
}
