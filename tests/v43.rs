//! 2.0 lot 10 : premier lancement en 6 etapes qui configurent (parcours, reprise, saut d'etapes, mise a jour depuis la 1.x
//! sans assistant complet, garde « pas pendant un jeu »), coffres EXISTANTS proposes en premier (dont un ancien nom), regle
//! des 500 Mo du scan, textes. UNIQUEMENT des dossiers temporaires.
use agss::backup;
use agss::config::Game;
use agss::i18n::{tr, tr_linux, Lang, K};
use agss::onboarding::{self as ob, CopyState, Facts, Moved, Nav, Onboarding};
use agss::process::ProcessChecker;
use agss::scan::{self, Origin, Personal, Size, Tier};
use agss::settings::Settings;
use agss::vaults::{self as vl, Candidate, Kind};
use agss::{Ctx, Res};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const NEW: Facts = Facts { first_install: true, has_vault: false, has_items: false, consent_needed: true };

// ------------------------------------------------------------------ parcours

#[test]
fn installation_neuve_six_etapes_dans_l_ordre() {
    assert_eq!(ob::plan_for(&NEW), (ob::ALL, false));
    let mut o = Onboarding::default();
    ob::begin(&mut o, &NEW);
    assert!(o.started && !o.finished && !o.whats_new);
    assert_eq!(o.step, ob::WELCOME);
    assert!(ob::pending(&o));
    let mut seen = vec![o.step];
    while let Moved::To(s) = ob::navigate(&mut o, Nav::Continue, true) {
        seen.push(s);
    }
    assert_eq!(seen, vec![1, 2, 3, 4, 5, 6]);
    assert!(o.finished && !ob::pending(&o));
    assert_eq!(o.done, ob::ALL);
    assert_eq!(ob::position(ob::ALL, 4), Some((4, 6)));
    // un parcours deja commence n'est jamais recalcule
    let before = o.clone();
    ob::begin(&mut o, &Facts { first_install: false, has_vault: true, has_items: true, consent_needed: false });
    assert_eq!(o, before);
}

#[test]
fn reprise_de_l_assistant_apres_fermeture() {
    let t = TempDir::new().unwrap();
    let mut s = Settings::default();
    ob::begin(&mut s.onboarding, &NEW);
    ob::navigate(&mut s.onboarding, Nav::Continue, true); // -> 2
    ob::navigate(&mut s.onboarding, Nav::Later, false); // -> 3
    assert_eq!(s.onboarding.step, ob::DEVICE);
    std::fs::write(t.path().join("settings.json"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    // fermeture de l'appli puis relance : meme etape, memes etapes faites / passees
    let back = Settings::load_from(t.path()).unwrap();
    assert_eq!(back.onboarding, s.onboarding);
    assert_eq!(back.onboarding.step, ob::DEVICE);
    assert!(ob::pending(&back.onboarding));
    assert_eq!(back.onboarding.done, ob::bit(1));
    assert_eq!(back.onboarding.later, ob::bit(2));
    // ancien settings.json sans le champ : rien de commence
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr","tutorial_step":3}"#).unwrap();
    let old = Settings::load_from(t.path()).unwrap();
    assert_eq!(old.onboarding, Onboarding::default());
    assert!(!ob::pending(&old.onboarding));
}

#[test]
fn saut_d_etapes_plus_tard_et_retour() {
    let mut o = Onboarding::default();
    ob::begin(&mut o, &NEW);
    // « Plus tard » partout : on avance sans rien perdre, l'assistant se termine
    for expected in 2..=6u8 {
        assert_eq!(ob::navigate(&mut o, Nav::Later, false), Moved::To(expected));
    }
    assert_eq!(ob::navigate(&mut o, Nav::Later, false), Moved::Finished);
    assert_eq!(o.later, ob::ALL);
    assert_eq!(o.done, 0);
    // « Continuer » sans action reelle compte comme « Plus tard » ; une etape faite ensuite efface le « Plus tard »
    let mut o = Onboarding::default();
    ob::begin(&mut o, &NEW);
    ob::navigate(&mut o, Nav::Continue, false);
    assert_eq!(o.later, ob::bit(1));
    assert_eq!(ob::navigate(&mut o, Nav::Back, false), Moved::To(1));
    ob::navigate(&mut o, Nav::Continue, true);
    assert_eq!((o.done, o.later), (ob::bit(1), 0));
    // « Retour » au tout premier ecran : rien ne bouge
    let mut o = Onboarding::default();
    ob::begin(&mut o, &NEW);
    assert_eq!(ob::navigate(&mut o, Nav::Back, false), Moved::Stay);
    assert_eq!(o.step, 1);
    // « Refaire la configuration » : les 6 etapes depuis le debut
    let mut o = Onboarding { finished: true, started: true, plan: ob::bit(1), ..Default::default() };
    ob::restart(&mut o);
    assert_eq!((o.step, o.plan, o.finished, o.whats_new), (1, ob::ALL, false, false));
}

#[test]
fn mise_a_jour_depuis_la_1x_sans_assistant_complet() {
    // Un utilisateur 1.x : settings.json sans `tutorial_seen` (donc vu), un coffre et des elements, consentement de la 1.4.
    let t = TempDir::new().unwrap();
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr","usage_consent":true,"usage_asked":"1.4"}"#).unwrap();
    let s = Settings::load_from(t.path()).unwrap();
    assert!(s.tutorial_seen);
    let f = Facts { first_install: !s.tutorial_seen, has_vault: true, has_items: true, consent_needed: agss::usage::needs_question(s.usage_asked.as_deref(), "2.0.0") };
    let (plan, wn) = ob::plan_for(&f);
    assert!(wn, "« Quoi de neuf en 2.0 » d'abord");
    assert_eq!(plan, 0, "2.0.1 : plus d'etape pour les statistiques, la question est en bas de « Quoi de neuf »");
    let mut o = Onboarding::default();
    ob::begin(&mut o, &f);
    assert_eq!(o.step, ob::WHATS_NEW);
    assert_eq!(ob::position(o.plan, o.step), None);
    assert_eq!(ob::navigate(&mut o, Nav::Back, false), Moved::Stay);
    assert_eq!(ob::navigate(&mut o, Nav::Continue, true), Moved::Finished);
    // sans coffre : coffre + premiere copie ; sans element : scan + premiere copie ; jamais le parcours complet
    assert_eq!(ob::plan_for(&Facts { first_install: false, has_vault: false, has_items: true, consent_needed: false }), (ob::bit(2) | ob::bit(6), true));
    assert_eq!(ob::plan_for(&Facts { first_install: false, has_vault: true, has_items: false, consent_needed: true }), (ob::bit(4) | ob::bit(6), true));
    assert_eq!(ob::sequence(ob::bit(4) | ob::bit(6)), vec![4, 6]);
    // deja tout regle : seulement « Quoi de neuf », puis termine
    let all_set = Facts { first_install: false, has_vault: true, has_items: true, consent_needed: false };
    let mut o = Onboarding::default();
    ob::begin(&mut o, &all_set);
    assert_eq!((o.step, o.plan), (ob::WHATS_NEW, 0));
    assert_eq!(ob::navigate(&mut o, Nav::Continue, true), Moved::Finished);
    // une installation neuve avec deja des elements (tutoriel jamais ferme en 1.x) n'est pas « neuve »
    assert!(ob::plan_for(&Facts { first_install: true, has_vault: true, has_items: true, consent_needed: true }).1);
}

#[test]
fn jamais_par_dessus_un_jeu_et_donnees_d_usage() {
    assert!(ob::may_show(false, false));
    assert!(!ob::may_show(true, false));
    assert!(!ob::may_show(false, true));
    // evenement `onboarding_step` de la liste blanche existante, pour chaque etape et chaque bouton
    for s in 1..=6u8 {
        for nav in [Nav::Continue, Nav::Later, Nav::Back] {
            let e = agss::usage::Event::new("onboarding_step", &[("step", ob::usage_step(s).unwrap()), ("action", ob::usage_action(nav)), ("duration", "10-30s")], 1_800_000_000_000);
            assert!(agss::usage::validate(&e, e.at).is_ok(), "etape {} {:?}", s, nav);
        }
    }
    assert_eq!(ob::usage_step(ob::WHATS_NEW), None, "« Quoi de neuf » n'est pas une etape de l'entonnoir");
    // sans consentement, rien n'est mis en file
    let t = TempDir::new().unwrap();
    let e = agss::usage::Event::new("onboarding_step", &[("step", "1"), ("action", "done"), ("duration", "<10s")], 1_800_000_000_000);
    assert!(!agss::usage::push(t.path(), e.clone(), None));
    assert!(!agss::usage::push(t.path(), e.clone(), Some(false)));
    assert!(agss::usage::push(t.path(), e, Some(true)));
}

#[test]
fn premiere_copie_progression() {
    use CopyState::*;
    let t = ob::tally(&[Safe, Busy, Queued]);
    assert_eq!((t.total, t.safe, t.busy), (3, 1, 1));
    assert!(!t.settled() && !t.all_safe());
    assert!((t.fraction() - 1.0 / 3.0).abs() < 1e-6);
    let t = ob::tally(&[Safe, Safe, Waiting]);
    assert!(t.settled() && !t.all_safe());
    let t = ob::tally(&[Safe, Safe]);
    assert!(t.settled() && t.all_safe() && t.fraction() == 1.0);
    assert!(!ob::tally(&[]).all_safe());
}

// ------------------------------------------------------------------ coffres existants

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

/// L'appareil `machine` protege `name` dans `vault` (vrai moteur de copie).
fn protect(root: &Path, machine: &str, vault: &Path, name: &str) {
    let mut ctx = Ctx::new(root.join(format!("data-{}", machine)));
    ctx.machine = machine.to_string();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let dir = root.join(format!("src-{}-{}", machine, name.replace(' ', "")));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("slot.dat"), format!("{} {}", machine, name)).unwrap();
    let g = Game { name: name.into(), path: dir, exe: Some("jeu.exe".into()), ..Default::default() };
    backup::backup(&ctx, &g, vault).unwrap();
}

fn drive(root: &Path) -> Candidate {
    let p = root.join("Mon Drive");
    std::fs::create_dir_all(&p).unwrap();
    Candidate { kind: Kind::GoogleDrive, path: p, streaming: Some(true), work: false }
}

#[test]
fn coffre_existant_sous_un_ancien_nom_propose_en_premier() {
    let t = TempDir::new().unwrap();
    let d = drive(t.path());
    // cas reel de l'auteur : le coffre de la version precedente est un sous-dossier de « Mon Drive »
    let old = d.path.join(agss::legacy::VAULT_DIRS[1]);
    std::fs::create_dir_all(&old).unwrap();
    protect(t.path(), "EPIKOU", &old, "Celeste");
    protect(t.path(), "EPIGOS", &old, "Hades");
    std::fs::write(d.path.join("notes.txt"), "rien a voir").unwrap();
    std::fs::create_dir_all(d.path.join("Photos")).unwrap();
    assert!(vl::is_vault(&old) && vl::has_vault_items(&old));
    assert!(!vl::is_vault(&d.path) && !vl::is_vault(&d.path.join("Photos")));
    let found = vl::find_existing(std::slice::from_ref(&d), None);
    assert_eq!(found.len(), 1, "{:?}", found);
    let e = &found[0];
    assert_eq!(e.path, old);
    assert!(e.legacy && !e.marker);
    assert_eq!(e.items, 2);
    assert_eq!(e.devices, vec!["EPIGOS".to_string(), "EPIKOU".to_string()]);
    assert_eq!(e.cand.as_ref().map(|c| c.kind), Some(Kind::GoogleDrive));
    // choisir la tuile « Google Drive » creerait un coffre neuf VIDE a cote : l'interface le dit d'abord
    assert_eq!(vl::target_for(&d.path), d.path.join(vl::VAULT_DIR));
    assert_eq!(vl::existing_beside(&d.path, &found).map(|x| x.path.clone()), Some(old.clone()));
    // reprendre l'existant : il sert tel quel (rien d'imbrique, rien de deplace)
    assert_eq!(vl::target_for(&old), old);
    let target = vl::prepare(&old).unwrap();
    assert_eq!(target, old);
    assert!(!old.join(vl::VAULT_DIR).exists());
    assert_eq!(agss::cloud::list_dest_games(&old).len(), 2, "elements intacts");
    assert!(old.join(vl::MARKER).is_file(), "marqueur ecrit une fois");
    // une fois marque, il n'est plus « ancien » et reste repris tel quel
    let again = vl::find_existing(std::slice::from_ref(&d), None);
    assert!(again[0].marker && !again[0].legacy);
    assert_eq!(vl::target_for(&old), old);
}

#[test]
fn coffres_existants_actuel_marque_et_nom_actuel_ordonnes() {
    let t = TempDir::new().unwrap();
    let d = drive(t.path());
    // coffre actuel (nom 2.0) avec 1 element, coffre marque au nom choisi a la main avec 2 elements
    let cur = d.path.join(vl::VAULT_DIR);
    std::fs::create_dir_all(&cur).unwrap();
    protect(t.path(), "EPIKOU", &cur, "Celeste");
    let custom = d.path.join("Sauvegardes jeux");
    let p = vl::prepare(&custom.join("..").join("Sauvegardes jeux")).err();
    assert!(p.is_some(), "dossier introuvable : rien n'est cree");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::write(custom.join(vl::MARKER), "AutoGameSoftwareSync vault v1\n").unwrap();
    protect(t.path(), "EPIGOS", &custom, "Hades");
    protect(t.path(), "EPIGOS", &custom, "Balatro");
    // un dossier vide au nom de l'appli n'est pas un coffre a reprendre
    let usb = t.path().join("CLE");
    std::fs::create_dir_all(usb.join(vl::VAULT_DIR)).unwrap();
    let usb_c = Candidate { kind: Kind::Usb, path: usb.clone(), streaming: None, work: false };
    let found = vl::find_existing(&[d.clone(), usb_c], Some(&cur));
    let paths: Vec<PathBuf> = found.iter().map(|e| e.path.clone()).collect();
    assert_eq!(paths, vec![cur.clone(), custom.clone()], "le coffre actuel d'abord, puis le plus fourni");
    assert!(found[1].marker && found[1].items == 2);
    // le marqueur suffit a reprendre un dossier au nom libre (aucun sous-dossier imbrique)
    assert_eq!(vl::target_for(&custom), custom);
    // la tuile du Drive : le sous-dossier `AutoGameSoftwareSync` existe et sera repris ; l'autre coffre est signale
    assert_eq!(vl::target_for(&d.path), cur);
    assert_eq!(vl::existing_beside(&d.path, &found).map(|x| x.path.clone()), Some(custom.clone()));
    // choisir directement un coffre existant n'a rien « a cote »
    assert!(vl::existing_beside(&custom, &found).is_none());
    // PC sans rien : aucun coffre existant
    assert!(vl::find_existing(&[], None).is_empty());
}

#[test]
fn ancien_dossier_partage_a_la_racine_d_un_service_reconnu() {
    // ancien usage : le dossier partage etait directement la racine (cle USB, dossier Syncthing dedie)
    let t = TempDir::new().unwrap();
    let root = t.path().join("Sync");
    std::fs::create_dir_all(&root).unwrap();
    protect(t.path(), "DECK", &root, "Celeste");
    let c = Candidate { kind: Kind::Syncthing, path: root.clone(), streaming: None, work: false };
    let found = vl::find_existing(&[c], None);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, root);
    assert_eq!(vl::target_for(&root), root, "repris tel quel");
    assert!(vl::existing_beside(&root, &found).is_none());
}

// ------------------------------------------------------------------ regle des 500 Mo

fn personal(p: &Path, which: Personal) -> scan::Item {
    let mut it = scan::folder_item(p);
    it.origin = Origin::Personal(which);
    it.name = "Documents".into();
    it.key = "folders:documents".into();
    it.newest_ms = Some(agss::util::now_ms());
    it
}

const MB: u64 = 1024 * 1024;

#[test]
fn regle_des_500_mo_dossier_perso_en_peut_etre_avec_l_impact() {
    let t = TempDir::new().unwrap();
    let mut docs = personal(t.path(), Personal::Documents);
    assert_eq!(docs.tier, Tier::Recommended);
    assert!(scan::check_waits_for_size(&docs), "la coche attend la mesure");
    // 400 Mo : reste recommande et coche, versions par defaut
    let mut small = docs.clone();
    assert!(!scan::apply_size_rule(&mut small, &Size { bytes: 400 * MB, files: 10, partial: false }));
    assert!(small.default_checked());
    assert_eq!(scan::keep_for(&small, 400 * MB), backup::DEFAULT_KEEP);
    // 500 Mo tout juste : encore recommande ; 501 Mo : « Peut-être », decoche
    assert!(!scan::apply_size_rule(&mut docs.clone(), &Size { bytes: scan::PERSONAL_AUTO_MAX, files: 1, partial: false }));
    assert!(scan::apply_size_rule(&mut docs, &Size { bytes: scan::PERSONAL_AUTO_MAX + MB, files: 1, partial: false }));
    assert_eq!(docs.tier, Tier::Maybe);
    assert!(!docs.default_checked() && docs.selectable(), "toujours choisissable a la main");
    assert!(!scan::check_waits_for_size(&docs));
    // impact : taille x versions gardees (3 pour un dossier perso de plus de 500 Mo) ; cas reel : Documents 4,2 Go
    let b = 4300 * MB;
    assert_eq!(scan::keep_for(&docs, b), scan::BIG_KEEP);
    assert_eq!(scan::vault_impact(&docs, b), b * 3);
    assert_eq!(agss::actions::format_bytes(scan::vault_impact(&docs, b)).split_whitespace().last(), agss::actions::format_bytes(b * 3).split_whitespace().last());
    // un dossier de sauvegardes d'un jeu de 600 Mo n'est pas concerne
    let mut game = scan::folder_item(t.path());
    game.cat = scan::Cat::Games;
    game.origin = Origin::Base;
    assert!(!scan::apply_size_rule(&mut game, &Size { bytes: 600 * MB, files: 1, partial: false }));
    assert_eq!(game.tier, Tier::Recommended);
    assert_eq!(scan::keep_for(&game, 600 * MB), scan::BIG_KEEP, "2.0.1 : un seul seuil pour tout element (3 versions au-dela)");
    assert_eq!(scan::keep_for(&game, 400 * MB), backup::DEFAULT_KEEP);
    assert_eq!(scan::keep_for(&game, scan::BIG_BYTES), scan::BIG_KEEP, "au-dela de 2 Go : 3 versions pour tout le monde");
    // un dossier deja en « Peut-être » ne change pas
    let mut maybe = personal(t.path(), Personal::Pictures);
    maybe.tier = Tier::Maybe;
    assert!(!scan::apply_size_rule(&mut maybe, &Size { bytes: 900 * MB, files: 1, partial: false }));
}

#[test]
fn regle_des_500_mo_appliquee_a_la_mesure_du_scan_et_a_l_ajout() {
    // mesure du scan : Documents (perso) passe en « Peut-être » ; meme regle dans le scan normal et dans l'assistant
    let t = TempDir::new().unwrap();
    let docs_dir = t.path().join("Documents");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(docs_dir.join("a.txt"), "x").unwrap();
    let docs = personal(&docs_dir, Personal::Documents);
    let h = scan::Handle { state: std::sync::Arc::new(std::sync::Mutex::new(scan::Progress { items: vec![docs.clone()], finished: true, step: 4, ..Default::default() })), cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)) };
    scan::measure_sizes(&h, &|| {});
    let v = h.view();
    assert!(v.sizes_done);
    assert_eq!(v.items[0].tier, Tier::Recommended, "1 octet : reste recommande");
    // plus de 500 Mo (fichier dont seule la longueur est fixee : rien n'est ecrit) : « Peut-être » des la mesure
    let f = std::fs::File::create(docs_dir.join("gros.bin")).unwrap();
    f.set_len(scan::PERSONAL_AUTO_MAX + MB).unwrap();
    drop(f);
    let h = scan::Handle { state: std::sync::Arc::new(std::sync::Mutex::new(scan::Progress { items: vec![docs.clone()], finished: true, step: 4, ..Default::default() })), cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)) };
    scan::measure_sizes(&h, &|| {});
    let v = h.view();
    assert_eq!(v.items[0].tier, Tier::Maybe);
    assert!(!v.items[0].default_checked());
    std::fs::remove_file(docs_dir.join("gros.bin")).unwrap();
    // ajout : 3 versions gardees pour un dossier perso de plus de 500 Mo (taille connue par le scan)
    let mut ctx = Ctx::new(t.path().join("data"));
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let mut sizes: HashMap<String, Size> = HashMap::new();
    sizes.insert(docs.key.clone(), Size { bytes: 600 * MB, files: 2, partial: false });
    let other_dir = t.path().join("Notes");
    std::fs::create_dir_all(&other_dir).unwrap();
    std::fs::write(other_dir.join("b.txt"), "y").unwrap();
    let other = scan::folder_item(&other_dir);
    sizes.insert(other.key.clone(), Size { bytes: 600 * MB, files: 2, partial: false });
    let rep = scan::add_items_from(&ctx, &[docs.clone(), other.clone()], &sizes, &|_| None, "manual");
    assert_eq!(rep.added.len(), 2, "{:?}", rep.skipped);
    let s = Settings::load(&ctx).unwrap();
    assert_eq!(s.per_game.get("Documents").and_then(|g| g.keep_versions), Some(scan::BIG_KEEP));
    assert_eq!(s.per_game.get("Notes").and_then(|g| g.keep_versions), Some(scan::BIG_KEEP), "dossier ajoute a la main de 600 Mo : 3 versions (seuil unique de l'alerte poids)");
    // dossier ajoute depuis le premier lancement : nom du dossier, copie planifiee (aucun programme)
    let cfg = agss::config::Config::load(&ctx).unwrap();
    let notes = cfg.games.iter().find(|g| g.name == "Notes").unwrap();
    assert!(notes.exe.is_none() && notes.schedule.is_some());
    assert_eq!(notes.path, other_dir);
    // deuxieme ajout du meme dossier : refuse (deja protege), rien n'est ecrase
    let rep = scan::add_items_from(&ctx, &[other], &sizes, &|_| None, "drop");
    assert!(rep.added.is_empty() && rep.skipped.len() == 1);
}

// ------------------------------------------------------------------ textes et code

#[test]
fn textes_du_premier_lancement() {
    // texte de l'etape 1 valide par l'auteur, mot pour mot
    assert_eq!(tr(Lang::Fr, K::VTagline), "Ne perds plus jamais rien.");
    assert_eq!(tr(Lang::Fr, K::VWelcomeBody), "Tes sauvegardes et réglages, protégés et synchronisés entre tous tes appareils : PC, portable, console. Tu reprends ta partie là où tu l'as laissée. Tu retrouves tes configurations logicielles, où que tu sois.");
    // consentement : les noms des jeux et logiciels CONNUS font partie de l'envoi (5 langues), deux boutons de meme poids
    for (l, w) in [(Lang::Fr, "CONNUS"), (Lang::En, "KNOWN"), (Lang::Es, "CONOCIDOS"), (Lang::Ru, "ИЗВЕСТНЫХ")] {
        assert!(tr(l, K::VConsentBody).contains(w), "{:?}", l);
    }
    assert!(agss::i18n_zh::tr(K::VConsentBody).contains("已知"));
    assert_eq!(tr(Lang::Fr, K::BtnUsageYes), "Oui, j'aide");
    assert_eq!(tr(Lang::Fr, K::BtnUsageNo), "Non merci");
    // demarrage : Windows / session de bureau sous Linux
    assert!(tr_linux(Lang::Fr, K::VStartHint).contains("session de bureau"));
    assert!(agss::i18n::tr_raw(Lang::Fr, K::VStartHint).contains("Windows"));
    for l in Lang::ALL {
        for k in [K::VNewTitle, K::VVaultExistingTitle, K::VScanImpact, K::VFirstCopyDone, K::VRedoSetup] {
            assert!(!tr(l, k).trim().is_empty());
        }
        let imp = if l == Lang::Zh { agss::i18n_zh::tr(K::VScanImpact) } else { tr(l, K::VScanImpact) };
        assert!(imp.contains("{0}") && imp.contains("{1}") && imp.contains("{2}"), "{:?}", l);
    }
}

fn gui_src() -> String {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut s = std::fs::read_to_string(src.join("gui.rs")).unwrap();
    for e in walkdir::WalkDir::new(src.join("gui")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        s.push_str(&std::fs::read_to_string(e.path()).unwrap());
    }
    s
}

#[test]
fn ancien_tutoriel_retire_et_assistant_branche() {
    let g = gui_src();
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gui/tutorial.rs").exists());
    for old in ["tutorial_overlay", "tutorial_lang_screen", "tut_banner_panel", "Demo Sync", "place_bubble", "panel_height"] {
        assert!(!g.contains(old), "reste de l'ancien tutoriel : {}", old);
    }
    // l'assistant : 6 etapes, reprise, relance depuis les Reglages, jamais par-dessus un jeu, donnees d'usage
    for want in ["fn wizard_step_welcome", "fn wizard_step_vault", "fn wizard_step_device", "fn wizard_step_scan", "fn wizard_step_extras", "fn wizard_step_finish", "fn wizard_whats_new", "fn wizard_restart", "fn wizard_blocked", "\"onboarding_step\"", "\"onboarding_done\"", "step_transition"] {
        assert!(g.contains(want), "{}", want);
    }
    // coffres existants proposes en premier dans l'ecran de choix
    assert!(g.contains("find_existing") && g.contains("existing_beside"));
    // l'animation de l'etape 1 passe par l'horloge centrale (aucun redessin en continu)
    let b = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gui/banner.rs")).unwrap();
    assert!(!b.contains("request_repaint()") && b.contains("need_ambient"));
}
