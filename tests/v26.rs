//! 1.1 : detection automatique des jeux (classification, machine a etats, modes, memoire, file de propositions).
use agss::gamedetect::*;
use agss::process::ProcInfo;
use std::path::PathBuf;

fn p(pid: u32, exe: &str, path: &str, start: u64) -> ProcInfo {
    ProcInfo { pid, exe: exe.to_lowercase(), path: if path.is_empty() { None } else { Some(PathBuf::from(path)) }, start }
}

fn db(exe: &str) -> Vec<DbHit> {
    match exe {
        "eldenring.exe" => vec![DbHit { name: "Elden Ring".into(), install_dirs: vec!["ELDEN RING".into()] }],
        "game.exe" => vec![DbHit { name: "Mon Jeu".into(), install_dirs: vec!["MonJeu".into()] }],
        "fieldofmistria.exe" => vec![DbHit { name: "Field of Mistria".into(), install_dirs: vec![] }],
        _ => vec![],
    }
}

fn env<'a>(roots: &'a [String], known: &'a [String]) -> Env<'a> {
    Env { db: &db, launcher_roots: roots, known_exes: known }
}

#[cfg(windows)]
#[test]
fn classification_base_lanceur_emulateur_liste_noire() {
    let (roots, known) = (vec!["e:/jeux/steamlibrary/steamapps/common".to_string()], vec!["dejala.exe".to_string()]);
    let e = env(&roots, &known);
    // (a) base embarquee
    let c = classify(&p(1, "EldenRing.exe", r"D:\Steam\steamapps\common\ELDEN RING\Game\eldenring.exe", 1), &e).unwrap();
    assert_eq!((c.why, c.name.as_str()), (Why::InDb, "Elden Ring"));
    assert_eq!(classify(&p(2, "fieldofmistria.exe", r"C:\Jeux\Mistria\fieldofmistria.exe", 1), &e).unwrap().why, Why::InDb);
    // Nom generique : refuse, sauf si le dossier d'installation correspond.
    assert!(classify(&p(3, "game.exe", r"C:\Truc\Autre\game.exe", 1), &e).is_none());
    assert_eq!(classify(&p(4, "game.exe", r"C:\Jeux\MonJeu\game.exe", 1), &e).unwrap().name, "Mon Jeu");
    assert!(classify(&p(5, "launcher.exe", r"D:\Steam\steamapps\common\Machin\launcher.exe", 1), &e).is_none(), "generique meme dans un dossier de lanceur");
    // (b) dossier de jeux d'un lanceur (Steam, bibliotheque secondaire, Epic, GOG)
    let c = classify(&p(6, "inconnu.exe", r"D:\Steam\steamapps\common\Super Truc\bin\inconnu.exe", 1), &e).unwrap();
    assert_eq!((c.why, c.name.as_str()), (Why::LauncherFolder, "Super Truc"));
    assert!(classify(&p(7, "x.exe", r"E:\Jeux\SteamLibrary\steamapps\common\Autre\x.exe", 1), &e).is_some());
    assert!(classify(&p(8, "y.exe", r"C:\Program Files\Epic Games\UnJeu\y.exe", 1), &e).is_some());
    assert!(classify(&p(9, "z.exe", r"C:\GOG Games\Gwent\z.exe", 1), &e).is_some());
    assert!(classify(&p(10, "outil.exe", r"C:\Outils\outil.exe", 1), &e).is_none(), "ni base, ni lanceur, ni emulateur");
    // (c) emulateurs
    assert_eq!(classify(&p(11, "pcsx2-qt.exe", r"C:\Emu\PCSX2\pcsx2-qt.exe", 1), &e).unwrap().why, Why::Emulator);
    assert_eq!(classify(&p(12, "RetroArch.exe", r"C:\RetroArch\retroarch.exe", 1), &e).unwrap().why, Why::Emulator);
    assert!(classify(&p(13, "dolphinbar.exe", r"C:\x\dolphinbar.exe", 1), &e).is_none(), "prefixe + lettre : autre programme");
    // Exclusions : lanceurs, navigateurs, systeme, appli elle-meme, jeux deja dans la liste, chemin illisible.
    for (exe, path) in [
        ("steam.exe", r"D:\Steam\steam.exe"),
        ("steamwebhelper.exe", r"D:\Steam\steamapps\common\x\steamwebhelper.exe"),
        ("EpicGamesLauncher.exe", r"C:\Program Files\Epic Games\Launcher\EpicGamesLauncher.exe"),
        ("Battle.net.exe", r"C:\Program Files\Battle.net\Battle.net.exe"),
        ("chrome.exe", r"C:\Program Files\Google\Chrome\chrome.exe"),
        ("AutoGameSoftwareSync.exe", r"D:\Outils\AutoGameSoftwareSync.exe"),
        ("eldenring.exe", r"C:\Windows\System32\eldenring.exe"),
        ("eldenring.exe", r"C:\Program Files\WindowsApps\Truc\eldenring.exe"),
        ("dejala.exe", r"D:\Steam\steamapps\common\Deja\dejala.exe"),
    ] {
        assert!(classify(&p(20, exe, path, 1), &e).is_none(), "{}", exe);
    }
    assert!(classify(&p(21, "eldenring.exe", "", 1), &e).is_none());
}


const ER: &str = r"D:\Steam\steamapps\common\ELDEN RING\eldenring.exe";

fn cand(exe: &str, path: &str) -> Candidate {
    let (roots, known) = (vec![], vec![]);
    classify(&p(1, exe, path, 1), &env(&roots, &known)).unwrap()
}

#[test]
fn machine_a_etats_carte_a_la_detection_bulle_apres_15_s() {
    let (roots, known) = (vec![], vec![]);
    let e = env(&roots, &known);
    let mem = Memory::default();
    let mut d = Detector::default();
    let game = p(100, "eldenring.exe", ER, 5);
    // Premiere liste = reference : un jeu deja ouvert n'est jamais propose.
    assert!(sc(&mut d, &[game.clone()], &e, &mem, true, 0).is_empty());
    // Nouveau jeu : la carte est ajoutee tout de suite, la bulle seulement 15 s apres.
    let new = p(200, "eldenring.exe", ER, 99);
    let ev = sc(&mut d, &[game.clone(), new.clone()], &e, &mem, true, 10_000);
    assert!(matches!(ev.as_slice(), [Event::Launched(c)] if c.exe == "eldenring.exe"), "{:?}", ev);
    assert!(sc(&mut d, &[game.clone(), new.clone()], &e, &mem, true, 10_000 + NOTIFY_DELAY_MS - 1).is_empty(), "pas de bulle avant 15 s");
    let ev = sc(&mut d, &[game.clone(), new.clone()], &e, &mem, true, 10_000 + NOTIFY_DELAY_MS);
    assert!(matches!(ev.as_slice(), [Event::Notify(c)] if c.name == "Elden Ring"), "{:?}", ev);
    // Une seule bulle par jeu et par session, meme si le jeu est relance.
    assert!(sc(&mut d, &[game.clone(), new.clone()], &e, &mem, true, 60_000).is_empty());
    assert!(sc(&mut d, &[game.clone()], &e, &mem, true, 70_000).is_empty());
    let again = p(300, "eldenring.exe", ER, 150);
    assert!(sc(&mut d, &[game, again.clone()], &e, &mem, true, 80_000).is_empty());
    assert!(sc(&mut d, &[again], &e, &mem, true, 100_000).is_empty(), "plus de carte ni de bulle pour ce jeu dans la session");
}

#[test]
fn bulle_annulee_si_le_jeu_se_ferme_avant_15_s_ou_notifications_coupees() {
    let (roots, known) = (vec![], vec![]);
    let e = env(&roots, &known);
    let mem = Memory::default();
    let mut d = Detector::default();
    sc(&mut d, &[], &e, &mem, true, 0);
    let g = p(1, "eldenring.exe", ER, 9);
    assert_eq!(sc(&mut d, &[g.clone()], &e, &mem, true, 1000).len(), 1, "carte ajoutee des la detection");
    assert!(sc(&mut d, &[], &e, &mem, true, 6000).is_empty(), "ferme avant 15 s");
    assert!(sc(&mut d, &[], &e, &mem, true, 40_000).is_empty(), "aucune bulle : la carte reste simplement en file");
    // Interrupteur des notifications coupe : la carte est ajoutee, jamais de bulle.
    let mut d2 = Detector::default();
    sc(&mut d2, &[], &e, &mem, false, 0);
    let g2 = p(2, "eldenring.exe", ER, 9);
    assert!(matches!(sc(&mut d2, &[g2.clone()], &e, &mem, false, 1000).as_slice(), [Event::Launched(_)]));
    assert!(sc(&mut d2, &[g2], &e, &mem, false, 60_000).is_empty());
    // Jeu ignore (memoire) : ni carte ni bulle.
    let mut m = Memory::default();
    m.answer(&cand("eldenring.exe", ER), "no", 0);
    let mut d3 = Detector::default();
    sc(&mut d3, &[], &e, &m, true, 0);
    assert!(sc(&mut d3, &[p(3, "eldenring.exe", ER, 9)], &e, &m, true, 1000).is_empty());
}

#[test]
fn les_deux_comportements_sont_actifs_ensemble_carte_bulle_15_s_et_fin_de_jeu() {
    let (roots, known) = (vec![], vec![]);
    let e = env(&roots, &known);
    let mem = Memory::default();
    let mut d = Detector::default();
    assert!(d.scan(&[], &e, &mem, true, 0).is_empty());
    let a = p(1, "eldenring.exe", ER, 10);
    let b = p(2, "eldenring.exe", ER, 11);
    // Lancement : carte tout de suite ; bulle 15 s apres ; pas d'autre carte pour le meme jeu.
    let ev = d.scan(&[a.clone(), b.clone()], &e, &mem, true, 1000);
    assert!(matches!(ev.as_slice(), [Event::Launched(_)]), "{:?}", ev);
    let ev = d.scan(&[a.clone(), b.clone()], &e, &mem, true, 1000 + NOTIFY_DELAY_MS);
    assert!(matches!(ev.as_slice(), [Event::Notify(_)]), "{:?}", ev);
    // Fermeture du dernier processus : evenement de fin (l'appli passe au premier plan APRES le jeu).
    assert!(d.scan(&[b.clone()], &e, &mem, true, 31_000).is_empty(), "un processus reste : pas ferme");
    let ev = d.scan(&[], &e, &mem, true, 32_000);
    assert!(matches!(ev.as_slice(), [Event::Ended(c)] if c.name == "Elden Ring"), "{:?}", ev);
}

#[test]
fn faux_positifs_exes_auxiliaires_et_dossiers_d_outils_table_de_cas() {
    let (roots, known) = (vec![], vec![]);
    let e = env(&roots, &known);
    // (exe, chemin) qui ne doivent JAMAIS etre proposes.
    for (exe, path) in [
        ("EOSBootstrapper.exe", r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win32\EOSBootstrapper.exe"),
        ("EpicGamesUpdater.exe", r"D:\Epic Games\Gear.Club Unlimited 3\EpicGamesUpdater.exe"),
        ("Launcher.exe", r"D:\Steam\steamapps\common\Jeu\Launcher.exe"),
        ("UnityCrashHandler64.exe", r"D:\Steam\steamapps\common\Jeu\UnityCrashHandler64.exe"),
        ("CrashReportClient.exe", r"D:\Epic Games\Jeu\Engine\Binaries\CrashReportClient.exe"),
        ("EasyAntiCheat_EOS.exe", r"D:\Steam\steamapps\common\Jeu\EasyAntiCheat\EasyAntiCheat_EOS.exe"),
        ("BEService.exe", r"D:\Steam\steamapps\common\Jeu\BattlEye\BEService.exe"),
        ("vgc.exe", r"C:\Program Files\Riot Vanguard\vgc.exe"),
        ("Setup.exe", r"D:\Steam\steamapps\common\Jeu\Setup.exe"),
        ("unins000.exe", r"D:\Jeux\Jeu\unins000.exe"),
        ("vc_redist.x64.exe", r"D:\Steam\steamapps\common\Jeu\_CommonRedist\vcredist\vc_redist.x64.exe"),
        ("dotnet.exe", r"D:\Steam\steamapps\common\Jeu\dotnet.exe"),
        ("UnrealCEFSubProcess.exe", r"D:\Epic Games\Jeu\Engine\Binaries\ThirdParty\CEF3\Win64\UnrealCEFSubProcess.exe"),
        ("steamwebhelper.exe", r"D:\Steam\steamapps\common\Jeu\steamwebhelper.exe"),
        ("GameService.exe", r"D:\Steam\steamapps\common\Jeu\GameService.exe"),
        ("Agent.exe", r"D:\Steam\steamapps\common\Jeu\Agent.exe"),
        ("jeu.exe", r"C:\Program Files\Common Files\Jeu\jeu.exe"),
        ("jeu.exe", r"C:\Program Files (x86)\Epic Online Services\jeu.exe"),
        ("game_helper.exe", r"D:\Steam\steamapps\common\Jeu\game_helper.exe"),
    ] {
        assert!(classify(&p(1, exe, path, 1), &e).is_none(), "faux positif : {}", exe);
    }
    // L'exe PRINCIPAL reste propose.
    assert!(classify(&p(2, "GearClub.exe", r"D:\Epic Games\GearClub\GearClub.exe", 1), &e).is_some());
    assert!(classify(&p(3, "eldenring.exe", ER, 1), &e).is_some());
    // Un seul candidat par dossier d'installation, l'exe de la base d'abord (meme dans le desordre).
    let mem = Memory::default();
    let mut d = Detector::default();
    d.scan(&[], &e, &mem, true, 0);
    let dir = r"D:\Steam\steamapps\common\ELDEN RING";
    let other = p(5, "autre.exe", &format!(r"{}\autre.exe", dir), 7);
    let main = p(6, "eldenring.exe", &format!(r"{}\eldenring.exe", dir), 7);
    let ev = d.scan(&[other, main], &e, &mem, true, 1000);
    assert_eq!(ev.len(), 1, "{:?}", ev);
    assert!(matches!(&ev[0], Event::Launched(c) if c.exe == "eldenring.exe"));
}

#[test]
fn applications_proposees_seulement_si_elles_ont_un_profil_suggere() {
    use agss::profiles::{detection_index, load_embedded, parse};
    let emb = load_embedded();
    let idx = detection_index(&emb, true);
    // Les entrees de la liste curee deja couvertes par un profil existant sont bien proposees.
    for exe in ["code.exe", "notepad++.exe", "windowsterminal.exe", "pwsh.exe", "blender.exe", "unrealeditor.exe", "obs64.exe"] {
        assert!(idx.get(exe).map(|v| v.2).unwrap_or(false), "{} doit etre propose comme application", exe);
    }
    assert!(!idx.contains_key("chrome.exe") && !idx.contains_key("firefox.exe") && !idx.contains_key("calc.exe"), "jamais une application quelconque");
    // Profil personnalise : non propose par defaut, propose avec `suggest: true`.
    let j = |s: &str| format!(r#"{{"schema":1,"id":"cust","name":"C","category":"app","version":"1","author":"a","license":"MIT","warnings":["w"],{}"detection":{{"exes":["cust.exe"]}},"locations":[{{"label":"L","path":"%APPDATA%\\C\\cfg"}}]}}"#, s);
    let off = parse(&j("")).unwrap();
    let on = parse(&j(r#""suggest":true,"#)).unwrap();
    assert!(detection_index(&[off.clone()], false).is_empty());
    assert!(detection_index(&[on], false).contains_key("cust.exe"));
    let hidden = parse(&j(r#""suggest":false,"#)).unwrap();
    assert!(detection_index(&[hidden], true).is_empty(), "un profil embarque peut etre masque explicitement");
}

#[test]
fn animations_des_cartes_reglage_duree_et_remplacement_journalise() {
    assert_eq!((exit_ms(Answer::Yes, false), exit_ms(Answer::No, false), exit_ms(Answer::Later, false)), (800, 700, 600));
    assert!(exit_ms(Answer::Yes, true) < 200, "fondu tres court");
    assert_eq!(CardAnim::default(), CardAnim::Always, "defaut : toujours (pour qu'elles se voient)");
    assert_eq!(reduce_decision(CardAnim::Always, false), (false, None), "Toujours : meme si Windows reduit les animations");
    assert_eq!(reduce_decision(CardAnim::Auto, true), (false, None));
    let (r, why) = reduce_decision(CardAnim::Auto, false);
    assert!(r && why.unwrap().contains(if cfg!(windows) { "Windows" } else { "bureau" }), "remplacee par un fondu, raison journalisee");
    let (r, why) = reduce_decision(CardAnim::Reduced, true);
    assert!(r && why.unwrap().contains("reduites"));
    // La carte reste a l'ecran jusqu'a la fin : la progression n'atteint 1 qu'apres la duree complete, et on repeint jusque-la.
    for a in [Answer::Yes, Answer::No, Answer::Later] {
        let d = exit_ms(a, false);
        assert!(exit_progress(a, false, 1000, 1000 + d - 1) < 1.0 && exit_progress(a, false, 1000, 1000 + d) == 1.0);
        assert!(animating(a, false, Some(1000), 1000 + d - 1));
    }
    // Phases du « Oui » : bond (monte) puis envol (monte encore, retrecit) ; « Non » : tremblement puis chute.
    let hop = exit_frame(Answer::Yes, 0.15, false, 1);
    let fly = exit_frame(Answer::Yes, 0.7, false, 1);
    assert!(hop.scale > 1.0 && fly.scale < 1.0 && fly.dy < hop.dy);
    let shake = exit_frame(Answer::No, 0.1, false, 1);
    let fall = exit_frame(Answer::No, 0.9, false, 1);
    assert!(shake.dy == 0.0 && shake.dx.abs() > 1.0 && fall.dy > 200.0);
    // Reglage persiste ; anciens fichiers (avec detect_mode / detect_show_on_close) lus sans casse.
    use agss::settings::Settings;
    let t = tempfile::TempDir::new().unwrap();
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr","detect_mode":"close","detect_show_on_close":false,"detect_scope":"apps"}"#).unwrap();
    let s = Settings::load_from(t.path()).unwrap();
    assert!(s.detect_enabled && s.detect_scope == DetectScope::Apps && s.card_anim == CardAnim::Always);
    let mut s2 = Settings::default();
    s2.card_anim = CardAnim::Reduced;
    std::fs::write(t.path().join("settings.json"), serde_json::to_string(&s2).unwrap()).unwrap();
    assert_eq!(Settings::load_from(t.path()).unwrap().card_anim, CardAnim::Reduced);
}


#[test]
fn memoire_effet_de_oui_non_plus_tard_et_persistance() {
    let t = tempfile::TempDir::new().unwrap();
    let c = cand("eldenring.exe", ER);
    let mut m = Memory::default();
    let mut q = Proposals::default();
    q.push(c.clone(), 0);
    // Plus tard : jamais memorise comme un refus, la carte reste en file mais est mise de cote.
    apply_answer(&mut m, &mut q, &c, Answer::Later, 1_000);
    assert!(m.ignored().is_empty() && q.queue.len() == 1);
    assert!(q.current(Blockers::default(), 2_000).is_none(), "mise de cote");
    assert!(q.current(Blockers::default(), 1_000 + SNOOZE_MS).is_some(), "revient plus tard");
    assert!(!m.may_propose(&c.key, 1_000 + LATER_MS - 1) && m.may_propose(&c.key, 1_000 + LATER_MS));
    // Non : memorise, retiree de la file.
    apply_answer(&mut m, &mut q, &c, Answer::No, 5_000);
    assert!(q.queue.is_empty() && m.ignored().len() == 1 && !m.may_propose(&c.key, u64::MAX / 2));
    m.save(t.path()).unwrap();
    assert_eq!(Memory::load(t.path()), m);
    // Oui : memorise, retiree.
    let mut m2 = Memory::default();
    let mut q2 = Proposals::default();
    q2.push(c.clone(), 0);
    apply_answer(&mut m2, &mut q2, &c, Answer::Yes, 1);
    assert!(q2.queue.is_empty() && m2.decisions[&c.key].state == "yes");
    // « Redemander » : on oublie la decision.
    m.decisions.remove(&c.key);
    assert!(m.may_propose(&c.key, 0));
}

#[test]
fn file_ordre_dedoublonnage_persistance_et_expiration() {
    let t = tempfile::TempDir::new().unwrap();
    let a = cand("eldenring.exe", ER);
    let b = cand("pcsx2-qt.exe", r"C:\Emu\pcsx2-qt.exe");
    let c = cand("factorio.exe", r"D:\Steam\steamapps\common\Factorio\bin\x64\factorio.exe");
    let mut q = Proposals::default();
    for (i, x) in [&a, &b, &c, &a].iter().enumerate() {
        q.push((*x).clone(), 100 * i as u64);
    }
    assert_eq!(q.queue.len(), 3, "pas de doublon, pas de limite arbitraire");
    let free = Blockers::default();
    let (cur, rank, total) = q.current(free, 1000).unwrap();
    assert_eq!((cur.cand.key.as_str(), rank, total), (a.key.as_str(), 1, 3), "une a la fois, indicateur 1/3");
    for blk in [Blockers { tutorial: true, ..free }, Blockers { confirm_open: true, ..free }, Blockers { restoring: true, ..free }, Blockers { window_hidden: true, ..free }] {
        assert!(q.current(blk, 1000).is_none());
    }
    // Plus tard : la premiere passe en fin de file, la suivante devient active.
    q.snooze(&a.key, 1000);
    assert_eq!(q.current(free, 1001).unwrap().0.cand.key, b.key);
    assert_eq!(q.queue.back().unwrap().cand.key, a.key);
    // Persistance + expiration 7 jours.
    q.save(t.path()).unwrap();
    let r = Proposals::load(t.path(), &Memory::default(), 2000);
    assert_eq!(r.queue.len(), 3);
    let old = Proposals::load(t.path(), &Memory::default(), EXPIRY_MS + 1000);
    assert_eq!(old.queue.len(), 0, "expirees apres 7 jours");
    let mut m = Memory::default();
    m.answer(&b, "no", 0);
    assert_eq!(Proposals::load(t.path(), &m, 2000).queue.len(), 2, "un jeu deja refuse est retire au rechargement");
    // Resolution.
    q.resolve(&b.key);
    assert_eq!(q.queue.len(), 2);
}

#[test]
fn etat_de_la_carte_fondu_confirmation_et_pile() {
    let mut s = CardState::default();
    s.sync(None, 0);
    assert_eq!(s, CardState::Idle);
    s.sync(Some("k1"), 100);
    assert_eq!(s, CardState::Showing { key: "k1".into(), since_ms: 100 });
    s.sync(Some("k1"), 900);
    assert_eq!(s, CardState::Showing { key: "k1".into(), since_ms: 100 }, "meme carte : le fondu ne repart pas");
    assert!(fade(100, 100) < 0.01 && fade(100, 100 + FADE_MS) > 0.99 && fade(100, 5000) == 1.0);
    // Oui : confirmation breve, qui passe avant la carte suivante ; Non / Plus tard : on passe a la suivante.
    s.answer(Answer::Yes, "Jeu", 1000);
    s.sync(Some("k2"), 1500);
    assert!(matches!(s, CardState::Confirmed { .. }), "la confirmation reste affichee");
    s.sync(Some("k2"), 1000 + CONFIRM_MS);
    assert_eq!(s, CardState::Showing { key: "k2".into(), since_ms: 1000 + CONFIRM_MS });
    s.answer(Answer::No, "x", 0);
    assert_eq!(s, CardState::Idle);
    // Pile : 3 couches maximum + badge « +N ».
    assert_eq!(visible_layers(0), (0, 0));
    assert_eq!(visible_layers(2), (2, 0));
    assert_eq!(visible_layers(3), (3, 0));
    assert_eq!(visible_layers(7), (3, 4));
    let g0 = layer_geom(0.0);
    assert_eq!((g0.dx, g0.dy, g0.rot_deg, g0.scale, g0.dim), (0.0, 0.0, 0.0, 1.0, 0.0), "la carte active est droite et pleine");
    let (g1, g2, g3) = (layer_geom(1.0), layer_geom(2.0), layer_geom(3.0));
    assert!(g1.scale > g2.scale && g2.scale > g3.scale && g1.dim < g2.dim && g2.dim < g3.dim, "plus petites et plus sombres en s'eloignant");
    assert!(g1.rot_deg != 0.0 && g1.rot_deg.signum() != g2.rot_deg.signum(), "legere rotation alternee");
    let mid = layer_geom(0.5);
    assert!(mid.scale < 1.0 && mid.scale > g1.scale, "interpolation pendant la montee");
}

#[test]
fn animations_des_reponses_phases_durees_et_enchainement() {
    // Durees : Oui 450, Non 400, Plus tard 300 ; fondu tres court si les animations Windows sont reduites.
    assert_eq!((exit_ms(Answer::Yes, false), exit_ms(Answer::No, false), exit_ms(Answer::Later, false)), (800, 700, 600));
    assert!(Answer::Yes != Answer::No && exit_ms(Answer::Yes, true) <= 160 && exit_ms(Answer::No, true) == exit_ms(Answer::Later, true));
    assert_eq!(exit_progress(Answer::Later, false, 1000, 1300), 0.5);
    assert_eq!(exit_progress(Answer::Later, false, 1000, 9000), 1.0);
    // OUI : bond joyeux (monte, gonfle un peu, eclat d'accent) PUIS part vers le haut / l'onglet Mes jeux en retrecissant.
    let hop = exit_frame(Answer::Yes, 0.2, false, 2);
    assert!(hop.dy < -5.0 && hop.scale > 1.0 && hop.burst > 0.3 && hop.alpha == 1.0, "{:?}", hop);
    let fly = exit_frame(Answer::Yes, 0.9, false, 2);
    assert!(fly.dy < -300.0 && fly.dx < 0.0 && fly.scale < 0.6, "vers le haut et l'onglet en haut a gauche : {:?}", fly);
    let end = exit_frame(Answer::Yes, 1.0, false, 2);
    assert!(end.alpha < 0.01);
    // NON : tremblement de refus (va-et-vient horizontal, sans rotation) puis jetee de cote en tournant et en tombant.
    let shake: Vec<f32> = (1..8).map(|i| exit_frame(Answer::No, i as f32 * 0.04, false, 2).dx).collect();
    assert!(shake.iter().any(|d| *d > 3.0) && shake.iter().any(|d| *d < -3.0), "tremblement : {:?}", shake);
    assert_eq!(exit_frame(Answer::No, 0.1, false, 2).rot_deg, 0.0);
    let thrown = exit_frame(Answer::No, 1.0, false, 2);
    assert!(thrown.dx > 400.0 && thrown.dy > 300.0 && thrown.rot_deg > 25.0 && thrown.alpha < 0.4, "{:?}", thrown);
    // PLUS TARD : glisse derriere le paquet, sans drame (ni rotation, ni deplacement lateral).
    let later = exit_frame(Answer::Later, 1.0, false, 3);
    assert_eq!((later.dx, later.rot_deg, later.burst), (0.0, 0.0, 0.0));
    assert_eq!(later.to_depth, Some(4.0), "derriere les 3 couches visibles");
    assert!(exit_frame(Answer::Later, 0.0, false, 3).to_depth == Some(0.0));
    // Animations reduites : fondu seulement, quelle que soit la reponse.
    for a in [Answer::Yes, Answer::No, Answer::Later] {
        let f = exit_frame(a, 0.5, true, 3);
        assert_eq!((f.dx, f.dy, f.rot_deg, f.scale, f.burst, f.to_depth), (0.0, 0.0, 0.0, 1.0, 0.0, None));
        assert!((f.alpha - 0.5).abs() < 1e-6);
    }
    // Enchainement : la carte suivante monte (profondeur 1 -> 0, la suivante 2 -> 1) pendant que la premiere part.
    assert_eq!(rise_depth(1, 0.0), 1.0);
    assert_eq!(rise_depth(1, 1.0), 0.0);
    assert_eq!(rise_depth(2, 1.0), 1.0);
    assert!(rise_depth(1, 0.5) < 1.0 && rise_depth(1, 0.5) > 0.0);
    // Repaint seulement pendant l'animation ; la confirmation du « Oui » attend la fin du vol.
    assert!(animating(Answer::Yes, false, Some(1000), 1000 + 800) && !animating(Answer::Yes, false, Some(1000), 1000 + 1000) && !animating(Answer::No, false, None, 5));
    assert_eq!(confirm_start_ms(false), 800);
}

#[test]
fn reglages_de_detection_par_defaut_et_anciens_fichiers() {
    use agss::settings::Settings;
    let s = Settings::default();
    assert!(s.detect_enabled);
    let t = tempfile::TempDir::new().unwrap();
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr"}"#).unwrap();
    let o = Settings::load_from(t.path()).unwrap();
    assert!(o.detect_enabled);
    // Aucune fonction du detecteur ne manipule de fenetre : le module ne reference pas winutil.
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("gamedetect.rs")).unwrap();
    assert!(!src.contains("winutil") && !src.contains("SetForegroundWindow") && !src.contains("show_window"));
}


/// Comme `Detector::scan` sans les evenements de fin de jeu (testes a part).
fn sc(d: &mut Detector, procs: &[ProcInfo], e: &Env, m: &Memory, notif: bool, now: u64) -> Vec<Event> {
    d.scan(procs, e, m, notif, now).into_iter().filter(|x| !matches!(x, Event::Ended(_))).collect()
}

fn ctx_ok() -> ShowCtx {
    ShowCtx { enabled: true, cards_pending: true, ..Default::default() }
}

#[test]
fn afficher_l_appli_a_la_fermeture_regles() {
    let granted = &mut || true;
    // Declenchement a la fermeture (apres ~3 s), une seule fois.
    let mut s = ShowOnClose::default();
    assert_eq!(s.tick(&ctx_ok(), 0, granted), ShowAction::None, "rien sans fermeture");
    s.on_closed(1000);
    assert_eq!(s.tick(&ctx_ok(), 1000 + SHOW_ON_CLOSE_DELAY_MS - 1, granted), ShowAction::None, "delai court apres la fermeture");
    assert_eq!(s.tick(&ctx_ok(), 1000 + SHOW_ON_CLOSE_DELAY_MS, granted), ShowAction::BringToFront);
    assert_eq!(s.tick(&ctx_ok(), 60_000, granted), ShowAction::None, "une seule fois par fermeture");
    // Report si un AUTRE jeu tourne, un plein ecran est actif, tutoriel ou confirmation : puis declenchement a la fin.
    for blocked in [ShowCtx { other_game_running: true, ..ctx_ok() }, ShowCtx { fullscreen_active: true, ..ctx_ok() }, ShowCtx { tutorial: true, ..ctx_ok() }, ShowCtx { confirm_open: true, ..ctx_ok() }] {
        let mut s = ShowOnClose::default();
        s.on_closed(0);
        assert_eq!(s.tick(&blocked, 10_000, granted), ShowAction::None);
        assert_eq!(s.tick(&blocked, 20_000, granted), ShowAction::None, "reporte tant que ca dure");
        assert_eq!(s.tick(&ctx_ok(), 30_000, granted), ShowAction::BringToFront, "des que l'autre jeu / le plein ecran est fini");
    }
    // Interrupteur coupe ou aucune carte en attente : jamais, et la fermeture est oubliee.
    for off in [ShowCtx { enabled: false, ..ctx_ok() }, ShowCtx { cards_pending: false, ..ctx_ok() }] {
        let mut s = ShowOnClose::default();
        s.on_closed(0);
        assert_eq!(s.tick(&off, 10_000, granted), ShowAction::None);
        assert_eq!(s.tick(&ctx_ok(), 20_000, granted), ShowAction::None);
    }
    // Deja au premier plan : rien a demander a Windows (2.0.3 : signale, l'ecran de fermeture s'ouvre). Premier plan refuse
    // par Windows : clignotement, sans insister.
    let mut s = ShowOnClose::default();
    s.on_closed(0);
    assert_eq!(s.tick(&ShowCtx { already_foreground: true, ..ctx_ok() }, 10_000, granted), ShowAction::AlreadyFront);
    assert_eq!(s.tick(&ShowCtx { already_foreground: true, ..ctx_ok() }, 20_000, granted), ShowAction::None, "une seule fois");
    let mut s = ShowOnClose::default();
    s.on_closed(0);
    let mut tries = 0;
    let refused = &mut || {
        tries += 1;
        false
    };
    assert_eq!(s.tick(&ctx_ok(), 10_000, refused), ShowAction::FlashTaskbar);
    assert_eq!(s.tick(&ctx_ok(), 20_000, refused), ShowAction::None);
    assert_eq!(tries, 1, "une seule tentative");
    // Detecteur : evenement de fin de jeu quand le dernier processus se ferme, quel que soit le mode.
    let (roots, known) = (vec![], vec![]);
    let e = env(&roots, &known);
    let mem = Memory::default();
    let mut d = Detector::default();
    d.scan(&[], &e, &mem, true, 0);
    let g = p(1, "eldenring.exe", ER, 9);
    d.scan(&[g], &e, &mem, true, 1000);
    assert_eq!(d.running_count(), 1);
    let ev = d.scan(&[], &e, &mem, true, 6000);
    assert!(matches!(ev.as_slice(), [Event::Ended(_)]), "{:?}", ev);
    assert_eq!(d.running_count(), 0);
}
