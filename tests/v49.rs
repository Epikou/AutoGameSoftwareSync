//! 2.0.3 : accueil sans clignotement (ordre fige, hysteresis du titre), menu de tri, apprentissage des programmes inconnus
//! (filtres), ecran de fermeture (file, premier plan), moins de notifications, liste Playnite, nettoyage des copies de
//! securite (regle, pas de doublon, « Annuler » toujours possible), option plein ecran. UNIQUEMENT des dossiers temporaires.
use agss::backup::{self, SafetyCopy};
use agss::config::Game;
use agss::gamedetect::{self, Candidate, Env, Facts, Memory, ShowAction, ShowCtx, ShowOnClose, UnknownEvent, UnknownWatch, Why};
use agss::gui::ui::State;
use agss::home::{self, FrozenOrder, Headline, HeadlineHold, MainAction, SortKey, SortRow, Summary};
use agss::i18n::{tr, Lang, ALL_KEYS, K};
use agss::knowledge::learn::Session;
use agss::knowledge::paths::Folders;
use agss::knowledge::Program;
use agss::notify::{self, Gate, Route};
use agss::process::{ProcInfo, ProcessChecker};
use agss::settings::Settings;
use agss::watch::{EventKind, WatchEvent};
use agss::{Ctx, Res};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ================================================================== 1. ordre fige et titre stable

fn row(name: &str, st: State, sync: u64, modified: u64, bytes: u64) -> SortRow {
    SortRow { name: name.into(), shown: name.into(), state: st, recent_ms: sync.max(modified), last_sync_ms: sync, modified_ms: modified, bytes, real_problem: false }
}

#[test]
fn ordre_fige_pendant_une_rafale_de_synchros() {
    let mut rows = vec![row("Celeste", State::Safe, 50, 50, 10), row("Factorio", State::Safe, 40, 40, 20), row("Zelda", State::Safe, 30, 30, 30)];
    let mut fz = FrozenOrder::default();
    let first = fz.apply(&rows, SortKey::State, 1);
    assert_eq!(first, ["Celeste", "Factorio", "Zelda"]);
    // Rafale : chaque element passe « a faire » -> « copie » -> « a l'abri » (et devient le plus recent) : rien ne bouge.
    let mut t = 100;
    for i in [2usize, 1, 0, 2, 1] {
        for st in [State::Todo, State::Busy, State::Safe] {
            rows[i].state = st;
            if st == State::Safe {
                t += 10;
                rows[i].last_sync_ms = t;
                rows[i].recent_ms = t;
            }
            assert_eq!(fz.apply(&rows, SortKey::State, 1), first, "la tuile change d'etat, jamais de place");
        }
    }
    // Retour sur l'accueil (nouvelle epoque) : le tri est recalcule (le plus recent d'abord, comme avant).
    assert_eq!(fz.apply(&rows, SortKey::State, 2), ["Factorio", "Zelda", "Celeste"]);
    // Un vrai probleme (conflit, erreur) recalcule tout de suite : il passe en tete.
    rows[1].state = State::Problem;
    rows[1].real_problem = true;
    assert_eq!(fz.apply(&rows, SortKey::State, 2)[0], "Factorio");
    // ... mais une seule fois : tant qu'il reste en probleme, l'ordre ne bouge plus.
    let o = fz.apply(&rows, SortKey::State, 2);
    rows[0].state = State::Busy;
    assert_eq!(fz.apply(&rows, SortKey::State, 2), o);
    // Changement de tri : recalcule.
    assert_eq!(fz.apply(&rows, SortKey::Name, 2), ["Celeste", "Factorio", "Zelda"]);
    // Element ajoute : insere a sa place, les autres ne bougent pas ; element retire : disparait.
    rows.push(row("Doom", State::Todo, 0, 0, 5));
    assert_eq!(fz.apply(&rows, SortKey::Name, 2), ["Celeste", "Doom", "Factorio", "Zelda"]);
    rows.retain(|r| r.name != "Factorio");
    assert_eq!(fz.apply(&rows, SortKey::Name, 2), ["Celeste", "Doom", "Zelda"]);
}

fn sm(h: Headline, st: State) -> Summary {
    Summary { state: st, headline: h, action: Some(MainAction::SyncAll), total: 3, last_copy_ms: None }
}

#[test]
fn titre_synchronisation_seulement_au_dela_de_2_s_et_affiche_au_moins_2_s() {
    let safe = sm(Headline::AllSafe, State::Safe);
    let copy = sm(Headline::Copying(1), State::Busy);
    let todo = sm(Headline::ToCheck(1), State::Todo);
    let mut h = HeadlineHold::default();
    assert_eq!(h.apply(safe, 0).headline, Headline::AllSafe);
    // Synchros rapides (moins de 2 s) : le titre ne bouge JAMAIS.
    let mut t = 0;
    for _ in 0..5 {
        for (s, dt) in [(todo, 300), (copy, 800), (safe, 900)] {
            t += dt;
            assert_eq!(h.apply(s, t).headline, Headline::AllSafe, "a {} ms", t);
        }
    }
    // Copie longue : « Synchronisation… » apres 2 s d'activite...
    t += 1000;
    assert_eq!(h.apply(copy, t).headline, Headline::AllSafe);
    assert_eq!(h.apply(copy, t + 1999).headline, Headline::AllSafe);
    let shown_at = t + 2000;
    assert_eq!(h.apply(copy, shown_at).headline, Headline::Copying(1));
    assert!(h.pending(shown_at));
    // ... puis au moins 2 s a l'ecran meme si la copie finit tout de suite, puis « Tout est a l'abri ».
    assert_eq!(h.apply(safe, shown_at + 100).headline, Headline::Copying(1));
    assert_eq!(h.apply(safe, shown_at + 1999).headline, Headline::Copying(1));
    assert_eq!(h.apply(safe, shown_at + 2000).headline, Headline::AllSafe);
    // Un message important s'affiche tout de suite (pas d'hysteresis).
    let bad = sm(Headline::ToCheck(1), State::Problem);
    assert_eq!(h.apply(bad, shown_at + 2100).headline, Headline::ToCheck(1));
    let off = sm(Headline::VaultUnreachable, State::Problem);
    assert_eq!(h.apply(off, shown_at + 2150).headline, Headline::VaultUnreachable);
    // Premiere image pendant une copie : jamais un titre passager.
    assert_eq!(HeadlineHold::default().apply(copy, 0).headline, Headline::AllSafe);
}

// ================================================================== 2. menu de tri

#[test]
fn tris_nom_synchro_modifie_taille_etat() {
    let rows = vec![
        row("Éclair", State::Safe, 10, 300, 5),
        row("zelda", State::Problem, 30, 100, 50),
        row("Alpha", State::Safe, 20, 200, 500),
        row("beta", State::Busy, 0, 0, 0),
    ];
    assert_eq!(home::sorted_names(&rows, SortKey::Name), ["Alpha", "beta", "Éclair", "zelda"], "A-Z, accents et casse ignores");
    assert_eq!(home::sorted_names(&rows, SortKey::LastSync), ["zelda", "Alpha", "Éclair", "beta"]);
    assert_eq!(home::sorted_names(&rows, SortKey::Modified), ["Éclair", "Alpha", "zelda", "beta"]);
    assert_eq!(home::sorted_names(&rows, SortKey::Size), ["Alpha", "zelda", "Éclair", "beta"]);
    assert_eq!(home::sorted_names(&rows, SortKey::State)[0], "zelda", "problemes d'abord");
    // Compatible avec la recherche et les filtres : l'ordre trie est filtre ensuite (indices de l'accueil).
    let sorted = home::sorted_names(&rows, SortKey::Name);
    let kinds: Vec<(home::Kind, Vec<&str>)> = sorted.iter().map(|n| (home::Kind::Game, vec![n.as_str()])).collect();
    assert_eq!(home::visible_indices(&kinds, home::Filter::All, "ecl"), vec![2]);
    // Choix memorise dans les reglages ; ancien fichier sans le champ = tri par etat (comportement d'avant).
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.home_sort, SortKey::State);
    let mut s2 = Settings::default();
    s2.home_sort = SortKey::Size;
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s2).unwrap()).unwrap();
    assert_eq!(back.home_sort, SortKey::Size);
    assert_eq!(SortKey::ALL.len(), 5);
}

// ================================================================== 3. programmes inconnus : apprentissage

fn proc(pid: u32, exe: &str, path: &str) -> ProcInfo {
    ProcInfo { pid, exe: exe.into(), path: Some(PathBuf::from(path)), start: pid as u64 * 10 }
}

fn facts(age_ms: u64) -> Facts {
    Facts { console: Some(false), age_ms: Some(age_ms), ..Default::default() }
}

fn env_with<'a>(db: &'a dyn Fn(&str) -> Vec<gamedetect::DbHit>, known: &'a [String]) -> Env<'a> {
    Env { db, launcher_roots: &[], known_exes: known }
}

#[test]
fn programme_inconnu_appris_seulement_avec_fenetre_2_min_et_hors_outils() {
    let nodb = |_: &str| vec![];
    let known: Vec<String> = vec!["suivi.exe".into()];
    let env = env_with(&nodb, &known);
    let mem = Memory::default();
    let p = proc(10, "Hexa.exe", "D:/Jeux/Hexa/Hexa.exe");
    let c = gamedetect::unknown_candidate(&p, &facts(130_000), true, &env, &mem).expect("programme inconnu avec fenetre depuis 2 min");
    assert_eq!((c.why, c.name.as_str()), (Why::Learned, "Hexa"));
    assert!(gamedetect::unknown_candidate(&p, &facts(119_000), true, &env, &mem).is_none(), "moins de 2 min");
    assert!(gamedetect::unknown_candidate(&p, &facts(600_000), false, &env, &mem).is_none(), "sans vraie fenetre");
    assert!(gamedetect::unknown_candidate(&p, &Facts { console: Some(true), ..facts(600_000) }, true, &env, &mem).is_none(), "programme console");
    assert!(gamedetect::unknown_candidate(&p, &Facts { other_user: true, ..facts(600_000) }, true, &env, &mem).is_none(), "programme du systeme");
    assert!(gamedetect::unknown_candidate(&p, &Facts { company: Some("JetBrains s.r.o.".into()), ..facts(600_000) }, true, &env, &mem).is_none(), "outil de developpement");
    for (exe, path) in [("discord.exe", "C:/Users/a/AppData/Local/Discord/discord.exe"), ("notepad.exe", "C:/Windows/notepad.exe"), ("x.exe", "C:/Windows/System32/x.exe"), ("suivi.exe", "D:/Jeux/Suivi/suivi.exe"), ("cargo.exe", "C:/Users/a/.cargo/bin/cargo.exe")] {
        assert!(gamedetect::unknown_candidate(&proc(11, exe, path), &facts(600_000), true, &env, &mem).is_none(), "{}", exe);
    }
    // Deja refuse (« Jamais pour celui-ci ») : plus jamais observe.
    let mut m2 = Memory::default();
    m2.answer(&c, "no", 0);
    assert!(gamedetect::unknown_candidate(&p, &facts(600_000), true, &env, &m2).is_none());
}

#[test]
fn observation_demarre_une_fois_et_finit_a_la_fermeture() {
    let nodb = |_: &str| vec![];
    let env = env_with(&nodb, &[]);
    let mem = Memory::default();
    let mut w = UnknownWatch::default();
    let old = proc(1, "Old.exe", "D:/Jeux/Old/Old.exe");
    assert!(w.scan(&[old.clone()], &env, &mem, &HashSet::new(), &HashMap::new(), &|_| false).is_empty(), "premiere liste = reference");
    let a = proc(2, "Hexa.exe", "D:/Jeux/Hexa/Hexa.exe");
    let a2 = proc(3, "Hexa.exe", "D:/Jeux/Hexa/Hexa.exe");
    let wins: HashSet<u32> = [1, 2, 3].into_iter().collect();
    let mut fx: HashMap<(u32, u64), Facts> = HashMap::new();
    for p in [&old, &a, &a2] {
        fx.insert((p.pid, p.start), facts(600_000));
    }
    let ev = w.scan(&[old.clone(), a.clone(), a2.clone()], &env, &mem, &wins, &fx, &|_| false);
    assert!(matches!(ev.as_slice(), [UnknownEvent::Started(c)] if c.exe == "hexa.exe"), "un seul debut par programme : {:?}", ev);
    assert_eq!(w.running_count(), 1);
    assert!(w.scan(&[old.clone(), a2.clone()], &env, &mem, &wins, &fx, &|_| false).is_empty(), "un processus du programme tourne encore");
    let ev = w.scan(&[old.clone()], &env, &mem, &wins, &fx, &|_| false);
    assert!(matches!(ev.as_slice(), [UnknownEvent::Ended(c)] if c.exe == "hexa.exe"), "{:?}", ev);
    // Programme deja pris par la detection ordinaire : ignore ici.
    let b = proc(4, "Other.exe", "D:/Jeux/Other/Other.exe");
    fx.insert((b.pid, b.start), facts(600_000));
    let wins2: HashSet<u32> = [4].into_iter().collect();
    assert!(w.scan(&[old, b], &env, &mem, &wins2, &fx, &|p| p.pid == 4).is_empty());
}

#[test]
fn apprentissage_ignore_temp_cache_logs_crash_et_shaders() {
    let t = TempDir::new().unwrap();
    let f = Folders::under(t.path());
    let roaming = f.roaming.clone().unwrap();
    std::fs::create_dir_all(roaming.join("Hexa Games").join("Saves")).unwrap();
    for d in ["Temp", "HexaCache", "logs", "CrashDumps", "ShaderCache"] {
        std::fs::create_dir_all(f.local.clone().unwrap().join(d)).unwrap();
    }
    let mut s = Session::default();
    let c = |x: &str| x.split('/').map(|y| y.to_string()).collect::<Vec<_>>();
    for (b, p) in [
        ("%LOCALAPPDATA%", "Temp/a.tmp"),
        ("%LOCALAPPDATA%", "HexaCache/x/y.bin"),
        ("%LOCALAPPDATA%", "logs/today/run.txt"),
        ("%LOCALAPPDATA%", "CrashDumps/a/b.dmp"),
        ("%LOCALAPPDATA%", "ShaderCache/a/b.bin"),
        ("%APPDATA%", "Hexa Games/Saves/slot1.sav"),
        ("%APPDATA%", "Hexa Games/Saves/slot2.sav"),
    ] {
        s.record(b, &c(p), &[]);
    }
    let prog = Program { exe: "hexa.exe".into(), name: "Hexa".into(), ..Default::default() };
    let v = s.candidates(&prog, &f);
    assert_eq!(v.iter().map(|x| x.path.clone()).collect::<Vec<_>>(), vec![roaming.join("Hexa Games")], "{:?}", v);
}

// ================================================================== 4. ecran de fermeture

fn cand(key: &str) -> Candidate {
    Candidate { key: key.into(), exe: "hexa.exe".into(), path: None, why: Why::Learned, name: "Hexa".into(), install_dir: None, profile_id: None, app: false, confidence: None }
}

#[test]
fn fermeture_carte_en_tete_et_premier_plan_jamais_pendant_un_autre_programme() {
    let mut q = gamedetect::Proposals::default();
    q.push(cand("a"), 0);
    q.push(cand("b"), 0);
    q.snooze("b", 0);
    q.promote(&cand("b"), 10);
    let cur = q.current(Default::default(), 10).unwrap();
    assert_eq!((cur.0.cand.key.as_str(), cur.2), ("b", 2), "la carte du programme ferme passe devant, sans attente");
    q.resolve("c");
    q.promote(&cand("c"), 20);
    assert_eq!(q.queue.front().unwrap().cand.key, "c", "« Plus tard » de l'ecran l'avait retiree : elle revient a la fermeture");
    // Premier plan : refuse si un autre programme suivi tourne ou si un plein ecran est actif (reporte), une fois.
    let ok = ShowCtx { enabled: true, cards_pending: true, ..Default::default() };
    let mut s = ShowOnClose::default();
    s.on_closed(0);
    let mut asked = 0;
    assert_eq!(s.tick(&ShowCtx { other_game_running: true, ..ok }, 10_000, &mut || { asked += 1; true }), ShowAction::None);
    assert_eq!(s.tick(&ShowCtx { fullscreen_active: true, ..ok }, 11_000, &mut || { asked += 1; true }), ShowAction::None);
    assert_eq!(asked, 0, "jamais de demande de premier plan pendant un autre jeu / plein ecran");
    assert_eq!(s.tick(&ok, 12_000, &mut || false), ShowAction::FlashTaskbar, "refus de Windows : repli (notification cliquable)");
    s.on_closed(20_000);
    assert_eq!(s.tick(&ShowCtx { already_foreground: true, ..ok }, 30_000, &mut || true), ShowAction::AlreadyFront, "deja au premier plan : ecran ouvert sans rien demander");
}

#[test]
fn option_plein_ecran_activee_par_defaut_et_textes() {
    assert!(Settings::default().close_fullscreen);
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.close_fullscreen, "mise a jour depuis une version precedente : activee");
    let mut off = Settings::default();
    off.close_fullscreen = false;
    let back: Settings = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
    assert!(!back.close_fullscreen);
    assert_eq!(tr(Lang::Fr, K::VSetCloseFull), "Ouvrir l'appli en plein écran à la fermeture d'un programme");
    for k in [K::VCloseProtect, K::VCloseOther, K::VCloseNever, K::VCloseLater] {
        for l in Lang::ALL {
            assert!(!tr(l, k).trim().is_empty());
        }
    }
    assert_eq!(tr(Lang::Fr, K::VCloseNever), "Jamais pour celui-ci");
    // Plus de bulle « vient de demarrer » : le detecteur, appele sans notifications (comme le fait l'appli), n'emet jamais
    // d'evenement `Notify`, meme bien apres le delai de la bulle.
    let hit = |exe: &str| if exe == "hexa.exe" { vec![gamedetect::DbHit { name: "Hexa".into(), install_dirs: vec!["Hexa".into()] }] } else { vec![] };
    let env = env_with(&hit, &[]);
    let mem = Memory::default();
    let mut det = gamedetect::Detector::default();
    assert!(det.scan(&[], &env, &mem, false, 0).is_empty());
    let p = proc(30, "Hexa.exe", "D:/Jeux/Hexa/Hexa.exe");
    det.facts.insert((p.pid, p.start), Facts { console: Some(false), age_ms: Some(gamedetect::LONG_AGE_MS + 1), ..Default::default() });
    let ev = det.scan(std::slice::from_ref(&p), &env, &mem, false, 1_000);
    assert!(ev.iter().any(|e| matches!(e, gamedetect::Event::Launched(_))), "{:?}", ev);
    for t in [20_000u64, 60_000, 600_000] {
        assert!(det.scan(std::slice::from_ref(&p), &env, &mem, false, t).iter().all(|e| !matches!(e, gamedetect::Event::Notify(_))), "aucune bulle a {} ms", t);
    }
}

// ================================================================== 4 bis. controle 2.0.3 : jamais coince, jamais intrusif

#[test]
fn ecran_de_fermeture_expire_apres_10_min_et_plus_tard_compte_deux_reprises() {
    use gamedetect::{CLOSE_LATER_MAX, CLOSE_WAIT_MS, SHOW_ON_CLOSE_DELAY_MS};
    let ok = ShowCtx { enabled: true, cards_pending: true, ..Default::default() };
    // Autre programme (ou plein ecran console) pendant plus de 10 min : l'ecran est abandonne, jamais de surgissement tardif.
    let mut s = ShowOnClose::default();
    s.on_closed(0);
    let busy = ShowCtx { other_game_running: true, ..ok };
    assert_eq!(s.tick(&busy, SHOW_ON_CLOSE_DELAY_MS + 1, &mut || true), ShowAction::None);
    assert_eq!(s.tick(&busy, CLOSE_WAIT_MS - 1, &mut || true), ShowAction::None, "toujours reporte");
    let mut asked = false;
    assert_eq!(s.tick(&ok, CLOSE_WAIT_MS, &mut || { asked = true; true }), ShowAction::Expired);
    assert!(!asked, "aucune demande de premier plan apres l'expiration");
    assert_eq!(s.tick(&ok, CLOSE_WAIT_MS + 5_000, &mut || true), ShowAction::None, "oublie");
    // « Plus tard » sur l'ecran : reproposer au plus CLOSE_LATER_MAX fois, ensuite la carte de l'accueil seulement.
    let c = cand("hexa.exe|d:/jeux/hexa");
    let mut m = Memory::default();
    assert!(m.may_show_close(&c.key));
    for i in 1..=CLOSE_LATER_MAX {
        m.close_later(&c, 1_000 * i as u64);
        assert_eq!(m.close_later_count(&c.key), i);
        assert!(m.may_show_close(&c.key), "reprise {} autorisee", i);
    }
    m.close_later(&c, 99_000);
    assert!(!m.may_show_close(&c.key), "au-dela : plus d'ecran de fermeture");
    assert_eq!(m.decisions[&c.key].state, "later", "jamais memorise comme un refus");
    // Le compte survit a l'ecriture / relecture et a un « plus tard » ordinaire ; « oui » / « non » ferment pour de bon.
    let back: Memory = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.close_later_count(&c.key), CLOSE_LATER_MAX + 1);
    let old: gamedetect::Decision = serde_json::from_str(r#"{"state":"later","exe":"x.exe","name":"X"}"#).unwrap();
    assert_eq!(old.close_later, 0, "ancien fichier sans le champ");
    m.answer(&c, "later", 100_000);
    assert_eq!(m.close_later_count(&c.key), CLOSE_LATER_MAX + 1);
    m.answer(&c, "no", 100_000);
    assert!(!m.may_show_close(&c.key));
}

#[test]
fn programmes_inconnus_hors_portee_logiciels_et_carte_retiree_par_la_portee() {
    use gamedetect::DetectScope;
    let nodb = |_: &str| vec![];
    let env = env_with(&nodb, &[]);
    let mem = Memory::default();
    let old = proc(1, "Old.exe", "D:/Jeux/Old/Old.exe");
    let a = proc(2, "Hexa.exe", "D:/Jeux/Hexa/Hexa.exe");
    let wins: HashSet<u32> = [1, 2].into_iter().collect();
    let mut fx: HashMap<(u32, u64), Facts> = HashMap::new();
    for p in [&old, &a] {
        fx.insert((p.pid, p.start), facts(600_000));
    }
    // Portee « Logiciels » : un programme inconnu (jamais un logiciel a profil) n'est pas observe.
    let mut w = UnknownWatch::default();
    w.scope = DetectScope::Apps;
    assert!(w.scan(&[old.clone()], &env, &mem, &wins, &fx, &|_| false).is_empty());
    assert!(w.scan(&[old.clone(), a.clone()], &env, &mem, &wins, &fx, &|_| false).is_empty(), "hors portee");
    assert_eq!(w.running_count(), 0);
    // Portee « Jeux » ou « Tout » : observe.
    for sc in [DetectScope::Games, DetectScope::All] {
        let mut w = UnknownWatch::default();
        w.scope = sc;
        w.scan(&[old.clone()], &env, &mem, &wins, &fx, &|_| false);
        assert!(matches!(w.scan(&[old.clone(), a.clone()], &env, &mem, &wins, &fx, &|_| false).as_slice(), [UnknownEvent::Started(_)]), "{:?}", sc);
    }
    // La carte promue pour l'ecran de fermeture est retiree par la portee « Logiciels » : l'interface ferme alors l'ecran
    // (et quitte le plein ecran) parce que sa cle n'est plus en file.
    let mut q = gamedetect::Proposals::default();
    q.promote(&cand("hexa.exe|d:/jeux/hexa"), 0);
    assert_eq!(q.retain_scope(DetectScope::Apps), 1);
    assert!(!q.queue.iter().any(|p| p.cand.key == "hexa.exe|d:/jeux/hexa"), "plus en file : l'ecran de fermeture doit se fermer");
}

#[test]
fn notifications_vidage_hors_detection_et_copie_faite_gardee_si_un_autre_programme_tourne() {
    use EventKind::*;
    // Copie faite d'un element a programme pendant qu'un AUTRE programme suivi tourne : gardee (jamais perdue), puis « A jour ».
    assert_eq!(notify::route(BackedUp, true, true), Route::Hold);
    assert_eq!(notify::route(BackedUp, true, false), Route::Now);
    assert_eq!(notify::route(BackedUp, false, true), Route::Silent, "sans programme : toujours silencieux");
    let mut g = Gate::default();
    assert!(g.offer(&ev("Celeste", BackedUp), true, true).is_none());
    assert_eq!(g.held(), 1);
    let (_, body) = notify::grouped_text(&[ev("Celeste", BackedUp), ev("Doom", Conflict)], Lang::Fr).unwrap();
    assert!(body.contains("À jour : Celeste"), "{}", body);
    // Detection coupee (ou jamais activee) : le vidage ne depend que de « un programme suivi tourne » ; avec un faux
    // envoyeur, les evenements gardes partent en UNE notification des que plus rien ne tourne.
    struct Fake(std::cell::RefCell<Vec<(String, String)>>);
    impl notify::Notifier for Fake {
        fn notify(&self, t: &str, b: &str) {
            self.0.borrow_mut().push((t.to_string(), b.to_string()));
        }
    }
    let f = Fake(Default::default());
    assert!(!notify::flush_gated(&f, Lang::En, true, &mut g), "un programme tourne encore : on garde");
    assert!(notify::flush_gated(&f, Lang::En, false, &mut g));
    assert_eq!(f.0.borrow().len(), 1);
    assert_eq!(f.0.borrow()[0].0, tr(Lang::En, K::NotifBackedUp), "un seul evenement : son texte habituel");
    assert!(!notify::flush_gated(&f, Lang::En, false, &mut g), "rien de plus");
}

#[test]
fn titre_passager_durable_aucun_redessin_au_repos_et_reordonnancement_au_calme() {
    // « N a verifier » (element planifie, surveillance arretee) qui dure : redessin demande pendant les fenetres
    // d'hysteresis seulement, plus jamais ensuite (regle 2.0 : aucun redessin periodique au repos).
    let todo = sm(Headline::ToCheck(1), State::Todo);
    let mut h = HeadlineHold::default();
    h.apply(sm(Headline::AllSafe, State::Safe), 0);
    h.apply(todo, 100);
    assert!(h.pending(100), "activite qui commence : on attend 2 s");
    h.apply(todo, 2_200);
    assert!(h.pending(2_200), "titre passager tenu 2 s");
    h.apply(todo, 5_000);
    assert!(!h.pending(5_000), "durable : aucun redessin");
    h.apply(todo, 3_600_000);
    assert!(!h.pending(3_600_000));
    // L'ordre fige sait dire s'il differe de l'ordre trie (l'accueil avance alors d'une epoque apres 30 s de calme).
    let mut rows = vec![row("Celeste", State::Safe, 50, 50, 10), row("Factorio", State::Safe, 40, 40, 20)];
    let mut fz = FrozenOrder::default();
    fz.apply(&rows, SortKey::LastSync, 1);
    assert!(!fz.stale(&rows, SortKey::LastSync));
    rows[1].last_sync_ms = 500;
    rows[1].recent_ms = 500;
    assert!(fz.stale(&rows, SortKey::LastSync), "Factorio devrait passer devant");
    assert_eq!(fz.apply(&rows, SortKey::LastSync, 1), ["Celeste", "Factorio"], "mais rien ne bouge sans nouvelle epoque");
    assert_eq!(fz.apply(&rows, SortKey::LastSync, 2), ["Factorio", "Celeste"]);
    assert!(!fz.stale(&rows, SortKey::LastSync));
    assert_eq!(home::REORDER_CALM_MS, 30_000);
}

// ================================================================== 5. moins de notifications

fn ev(game: &str, kind: EventKind) -> WatchEvent {
    WatchEvent { game: game.into(), kind, detail: "disque plein".into() }
}

#[test]
fn une_seule_notification_a_la_fermeture() {
    use EventKind::*;
    assert_eq!(notify::route(BackedUp, true, false), Route::Now, "copie apres la fermeture du programme");
    assert_eq!(notify::route(BackedUp, false, false), Route::Silent, "dossier / reglages / copie planifiee : la tuile suffit");
    assert_eq!(notify::route(BackedUp, true, true), Route::Hold, "un autre programme tourne : gardee, regroupee a sa fermeture");
    for k in [Restored, Conflict, ConflictResolved, Error] {
        assert_eq!(notify::route(k, true, true), Route::Hold);
        assert_eq!(notify::route(k, false, false), Route::Now);
    }
    let mut g = Gate::default();
    // Pendant l'utilisation : rien ne part.
    for e in [ev("Notes", BackedUp), ev("Celeste", Restored), ev("Celeste", ConflictResolved), ev("Celeste", Restored), ev("Doom", Conflict), ev("Doom", Error), ev("Doom", Error)] {
        assert!(g.offer(&e, false, true).is_none());
    }
    assert_eq!(g.held(), 3, "regroupe : version recue + conflit regle = un seul fait ; probleme repete une fois ; copie sans programme silencieuse");
    assert!(g.flush(true).is_empty(), "toujours en cours : on garde");
    let held = g.flush(false);
    let (title, body) = notify::grouped_text(&held, Lang::Fr).unwrap();
    assert_eq!(title, tr(Lang::Fr, K::VNotifHeldTitle));
    assert_eq!(body.lines().count(), 3, "{}", body);
    assert!(body.contains("Celeste") && body.contains("Doom"));
    assert!(g.flush(false).is_empty());
    // Un probleme deja signale ne revient pas dans la meme session.
    assert!(g.offer(&ev("Doom", Error), false, false).is_none());
    assert!(g.offer(&ev("Doom", Conflict), false, false).is_some());
    // Un seul evenement garde : son texte habituel.
    let one = notify::grouped_text(&[ev("Celeste", Restored)], Lang::En).unwrap();
    assert_eq!(one, notify::notification_text(&ev("Celeste", Restored), Lang::En));
}

// ================================================================== 6. liste Playnite

#[test]
fn lecture_de_playnite_library_json() {
    let t = TempDir::new().unwrap();
    let p = t.path().join(agss::playnite::FILE);
    assert!(agss::playnite::read_from(&p).is_empty(), "absent : rien");
    std::fs::write(&p, "pas du json").unwrap();
    assert!(agss::playnite::read_from(&p).is_empty(), "invalide : rien");
    let json = r#"{"schema":1,"generated_at":"2026-10-29T10:00:00Z","games":[
        {"id":"g1","name":"Hollow Knight","source":"Steam","plugin_id":"cb91dfc9-b977-43bf-8e70-55f46e410fab","install_dir":"D:\\Steam\\steamapps\\common\\Hollow Knight","exes":["D:\\Steam\\steamapps\\common\\Hollow Knight\\hollow_knight.exe"],"game_id":"367520","steam_appid":367520,"platforms":["PC (Windows)"]},
        {"id":"g2","name":"Moon Lander","source":"","install_dir":"E:\\Jeux\\Moon Lander","exes":["{InstallDir}\\moonlander.exe"]},
        {"id":"g3","name":"Kept Title","source":"GOG","install_dir":"E:\\GOG\\Kept","exes":["E:\\GOG\\Kept\\kept.exe"],"game_id":"1207658924"},
        {"id":"g4","name":"Epic Games Launcher","source":"Epic","install_dir":"E:\\Epic"},
        {"id":"g5","name":"No Folder","source":""},
        {"id":"g6","name":"Chrono Trigger","source":"","install_dir":"E:\\Roms\\SNES","exes":[],"platforms":["Nintendo SNES"],"emulator":{"name":"Snes9x","profile":"Default"}},
        {"id":"g7","name":"Doom","source":"","install_dir":"E:\\Roms\\Doom","exes":[],"platforms":["Sony PlayStation","PC (DOS)"]}
    ]}"#;
    std::fs::write(&p, json).unwrap();
    let lib = agss::playnite::read_from(&p);
    assert_eq!(lib.len(), 7, "champs inconnus (date de generation, emulateur) toleres");
    assert!(!agss::playnite::is_pc_game(&lib[5]), "ROM emulee : pas de programme, plateforme console");
    assert!(agss::playnite::is_pc_game(&lib[6]), "une plateforme PC parmi d'autres");
    assert!(agss::playnite::is_pc_game(&lib[1]), "programme present");
    assert!(agss::playnite::is_pc_game(&lib[3]), "aucune plateforme renseignee");
    // Fichier en cours de remplacement par l'extension (vide / invalide) : None, le dernier compte est garde.
    assert!(agss::playnite::read_from_opt(&t.path().join("absent.json")).is_none());
    std::fs::write(t.path().join("vide.json"), "").unwrap();
    assert!(agss::playnite::read_from_opt(&t.path().join("vide.json")).is_none());
    assert_eq!(agss::playnite::read_from_opt(&p).map(|v| v.len()), Some(7));
    let hk = agss::playnite::to_installed(&lib[0]);
    assert_eq!((hk.store, hk.id.as_str(), hk.exe.as_deref()), (agss::detect::Store::Steam, "367520", Some("hollow_knight.exe")));
    assert_eq!(agss::playnite::to_installed(&lib[2]).store, agss::detect::Store::Gog);
    // Pas encore proteges : sans doublon, sans lanceur ni jeu sans dossier, et sans ce qui est deja protege (nom ou programme).
    let games = vec![Game { name: "Autre nom".into(), exe: Some("KEPT.EXE".into()), path: PathBuf::from("E:/x"), ..Default::default() }];
    let names: Vec<&str> = agss::playnite::not_protected(&lib, &games).iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Doom", "Hollow Knight", "Moon Lander"], "la ROM SNES n'est pas proposee (rien a proteger ni a apprendre)");
    let games2 = vec![Game { name: "hollow knight".into(), path: PathBuf::from("E:/y"), ..Default::default() }];
    assert_eq!(agss::playnite::not_protected(&lib, &games2).len(), 3, "meme nom (casse ignoree) = deja protege (Doom, Kept Title, Moon Lander restent)");
    // Les lanceurs passent avant : un jeu deja vu par Steam n'est pas ajoute depuis Playnite.
    let seen = vec!["Hollow Knight".to_string()];
    let names: Vec<&str> = agss::playnite::not_seen_by_launchers(&lib, &games, &seen).iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Doom", "Moon Lander"]);
    // Fichier d'exemple de l'extension 1.2.0 (contrat PLAYNITE-LIBRARY.md, copie dans tests/fixtures) : champs optionnels
    // generated_utc, hidden, roms, emulator toleres ; jeu masque ignore ; ROM emulee non proposee ; jeu Steam propose.
    let sample = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/playnite/playnite-library.sample.json")).unwrap();
    let now = agss::playnite::parse_utc_ms("2026-11-10T00:00:00Z").unwrap();
    let lib = agss::playnite::parse_at(&sample, now).expect("exemple lisible");
    assert_eq!(lib.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Celeste", "Super Metroid"], "jeu masque (hidden) retire");
    assert_eq!(lib[1].roms.len(), 1);
    assert_eq!(lib[1].emulator.as_ref().map(|e| e.name.as_str()), Some("RetroArch"));
    let names: Vec<&str> = agss::playnite::not_protected(&lib, &[]).iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Celeste"], "ROM emulee ignoree en 2.0.3");
    assert_eq!(agss::playnite::to_installed(&lib[0]).exe.as_deref(), Some("Celeste.exe"));
    assert!(agss::playnite::parse_at(&sample, now + 40 * DAY).is_none(), "liste de plus de 30 jours : perimee");
    // Dossier non resolu ou tronque : jamais propose.
    let bad = r#"{"schema":1,"games":[{"name":"Var","install_dir":"{InstallDir}\\x","exes":["x.exe"]},{"name":"Cut","install_dir":"\\Jeux\\y","exes":["y.exe"]}]}"#;
    assert!(agss::playnite::not_protected(&agss::playnite::parse(bad).unwrap(), &[]).is_empty());
    // Masquage de la carte par date (30 jours), plus par nombre ; ancien reglage lu sans erreur.
    let s: Settings = serde_json::from_str(r#"{"playnite_card_hidden":4}"#).unwrap();
    assert_eq!((s.playnite_card_hidden, s.playnite_card_hidden_until_ms), (4, 0));
    assert_eq!(agss::settings::PLAYNITE_CARD_HIDE_MS, 30 * DAY);
    // Playnite n'existe que sous Windows : `read` ne propose rien ailleurs.
    std::fs::copy(&p, t.path().join("copy.json")).unwrap();
    assert_eq!(agss::playnite::read(t.path()).is_empty(), !cfg!(windows));
}

// ================================================================== 7. copies de securite : nettoyage

const DAY: u64 = 86_400_000;

fn copy(name: &str, ms: u64) -> SafetyCopy {
    SafetyCopy { dir: PathBuf::from(name), ms, files: 1, bytes: 10, hash: name.into(), note: None }
}

#[test]
fn regle_de_nettoyage_prudente() {
    let now = 400 * DAY;
    assert!(backup::safety_prune_plan(&[copy("seule", 0)], now).is_empty(), "jamais la seule copie");
    let v: Vec<SafetyCopy> = (0..6).map(|i| copy(&format!("c{}", i), i * DAY)).collect();
    let plan: Vec<String> = backup::safety_prune_plan(&v, now).iter().map(|c| c.dir.display().to_string()).collect();
    assert_eq!(plan, ["c2", "c1", "c0"], "les 3 plus recentes gardees");
    let recent: Vec<SafetyCopy> = (0..6).map(|i| copy(&format!("r{}", i), now - i * DAY)).collect();
    assert!(backup::safety_prune_plan(&recent, now).is_empty(), "tout ce qui a moins de 30 jours est garde");
    let mixed = vec![copy("old1", 0), copy("old2", DAY), copy("d29", now - 29 * DAY), copy("n1", now - 2), copy("n2", now - 1), copy("n3", now)];
    let plan: Vec<String> = backup::safety_prune_plan(&mixed, now).iter().map(|c| c.dir.display().to_string()).collect();
    assert_eq!(plan, ["old2", "old1"]);
}

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(false)
    }
}

fn write(dir: &Path, rel: &str, content: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

#[test]
fn pas_de_copie_en_double_nettoyage_et_annuler_toujours_possible() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let mut ctx = Ctx::new(t.path().join("data"));
    ctx.machine = "EPIKOU".into();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "Celeste".into(), path: t.path().join("saves"), exe: Some("celeste.exe".into()), ..Default::default() };
    write(&game.path, "save.dat", b"v1");
    backup::backup(&ctx, &game, &dest).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(&game.path, "save.dat", b"v2 plus long");
    backup::backup(&ctx, &game, &dest).unwrap();
    let v1 = backup::list_versions(&dest, &game)[0].file_name().unwrap().to_string_lossy().to_string();
    // Remettre V1 puis annuler, deux fois : l'etat local avant chaque remise est le meme (V2) -> UNE seule copie de securite.
    let (o1, t1) = backup::restore_version_undoable(&ctx, &game, &dest, &v1).unwrap();
    backup::undo_restore(&ctx, &game, Some(&dest), &t1).unwrap();
    assert_eq!(std::fs::read(game.path.join("save.dat")).unwrap(), b"v2 plus long");
    let n_after_first = backup::safety_copies(&ctx, &game).len();
    let (o2, t2) = backup::restore_version_undoable(&ctx, &game, &dest, &v1).unwrap();
    assert_eq!(o1.safety_dir, o2.safety_dir, "contenu identique a la derniere copie : reutilisee");
    assert_eq!(backup::safety_copies(&ctx, &game).len(), n_after_first, "aucune copie en double");
    // « Libérer de la place » n'enleve rien de recent : « Annuler » reste possible, a l'identique.
    let u = backup::safety_usage(&ctx, agss::util::now_ms());
    assert!(u.bytes > 0 && u.copies >= 1);
    assert!(u.plan.is_empty(), "{:?}", u.plan);
    assert_eq!(backup::safety_free(&ctx, agss::util::now_ms()), (0, 0));
    backup::undo_restore(&ctx, &game, Some(&dest), &t2).unwrap();
    assert_eq!(std::fs::read(game.path.join("save.dat")).unwrap(), b"v2 plus long");
    // Dans 40 jours avec 5 copies differentes : les 3 plus recentes restent, la remise la plus recente reste annulable.
    let mut tickets = Vec::new();
    for i in 0..4 {
        write(&game.path, "save.dat", format!("local {}", i).as_bytes());
        std::thread::sleep(std::time::Duration::from_millis(20));
        tickets.push(backup::restore_version_undoable(&ctx, &game, &dest, &v1).unwrap().1);
    }
    let all = backup::safety_copies(&ctx, &game);
    assert!(all.len() >= 5, "{}", all.len());
    let later = agss::util::now_ms() + 40 * DAY;
    let u = backup::safety_usage(&ctx, later);
    assert_eq!(u.plan.len(), all.len() - backup::SAFETY_KEEP);
    let (n, freed) = backup::safety_free(&ctx, later);
    assert_eq!(n, all.len() - backup::SAFETY_KEEP);
    assert!(freed > 0);
    assert_eq!(backup::safety_copies(&ctx, &game).len(), backup::SAFETY_KEEP);
    let last = tickets.last().unwrap();
    assert!(last.safety_dir.as_ref().unwrap().is_dir(), "la copie de la remise la plus recente est gardee");
    backup::undo_restore(&ctx, &game, Some(&dest), last).unwrap();
    assert_eq!(std::fs::read(game.path.join("save.dat")).unwrap(), b"local 3");
    // Copie interrompue (sans meta.json) : nettoyee seulement apres un jour ; rien hors du dossier des copies.
    let stale = backup::safety_root(&ctx, &game).join("interrompue");
    std::fs::create_dir_all(stale.join("files")).unwrap();
    std::fs::write(stale.join("files/x"), b"x").unwrap();
    assert!(backup::safety_usage(&ctx, agss::util::now_ms()).plan.iter().all(|(_, c)| c.dir != stale));
    assert!(backup::safety_usage(&ctx, agss::util::now_ms() + 2 * DAY).plan.iter().any(|(_, c)| c.dir == stale));
    backup::safety_free(&ctx, agss::util::now_ms() + 2 * DAY);
    assert!(!stale.exists());
    assert!(game.path.join("save.dat").exists() && dest.is_dir());
}

#[test]
fn liberer_de_la_place_n_emporte_jamais_une_copie_en_cours_d_utilisation() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let mut ctx = Ctx::new(t.path().join("data"));
    ctx.machine = "EPIKOU".into();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "Celeste".into(), path: t.path().join("saves"), exe: Some("celeste.exe".into()), ..Default::default() };
    write(&game.path, "save.dat", b"v1");
    backup::backup(&ctx, &game, &dest).unwrap();
    let v1 = backup::list_versions(&dest, &game)[0].file_name().unwrap().to_string_lossy().to_string();
    for i in 0..5 {
        write(&game.path, "save.dat", format!("local {}", i).as_bytes());
        std::thread::sleep(std::time::Duration::from_millis(20));
        backup::restore_version_undoable(&ctx, &game, &dest, &v1).unwrap();
    }
    let all = backup::safety_copies(&ctx, &game);
    assert_eq!(all.len(), 5);
    let later = agss::util::now_ms() + 40 * DAY;
    // (b) Course : le plan (calcule avant) dit de supprimer la plus ancienne, mais une remise vient de la reutiliser
    // (`used.txt` de moins d'une heure, relu juste avant la suppression) : elle est gardee.
    let oldest = all[0].dir.clone();
    assert!(backup::safety_usage(&ctx, later).plan.iter().any(|(_, c)| c.dir == oldest), "sans marque : a supprimer");
    std::fs::write(oldest.join(backup::SAFETY_USED_FILE), (later - 60_000).to_string()).unwrap();
    let (n, _) = backup::safety_free(&ctx, later);
    assert!(oldest.is_dir(), "copie utilisee il y a une minute : jamais supprimee");
    assert_eq!(n, 2, "la marque la rend recente : ce sont les deux copies suivantes qui partent (3 gardees)");
    assert_eq!(backup::safety_copies(&ctx, &game).len(), 3);
    // Meme protection quand la marque arrive APRES le calcul du plan (vraie course) : `remove_safety_dir` relit `used.txt`.
    for i in 10..13 {
        write(&game.path, "save.dat", format!("local {}", i).as_bytes());
        std::thread::sleep(std::time::Duration::from_millis(20));
        backup::restore_version_undoable(&ctx, &game, &dest, &v1).unwrap();
    }
    let plan: Vec<PathBuf> = backup::safety_usage(&ctx, later + 60 * DAY).plan.iter().map(|(_, c)| c.dir.clone()).collect();
    assert!(!plan.is_empty());
    let all_dirs: Vec<PathBuf> = backup::safety_copies(&ctx, &game).into_iter().map(|c| c.dir).collect();
    for d in &all_dirs {
        std::fs::write(d.join(backup::SAFETY_USED_FILE), (later + 60 * DAY - 1000).to_string()).unwrap();
    }
    assert_eq!(backup::safety_free(&ctx, later + 60 * DAY).0, 0, "toutes utilisees il y a une seconde : rien de supprime");
    assert_eq!(backup::safety_copies(&ctx, &game).len(), all_dirs.len());
    for d in &all_dirs {
        let _ = std::fs::remove_file(d.join(backup::SAFETY_USED_FILE));
    }
    assert!(!ctx.data_dir.join("safety").join(backup::SAFETY_FREE_LOCK).exists(), "verrou retire a la fin");
    // (a) Une remise ne reutilise jamais une copie que la regle supprimerait : avec 5 copies anciennes (dates forcees), un
    // etat local identique a la plus ancienne donne une copie NEUVE, pas la reutilisation d'une copie condamnee.
    let second = backup::safety_copies(&ctx, &game)[0].dir.clone();
    let content = std::fs::read(second.join("files").join("save.dat")).unwrap();
    let mut meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(second.join("meta.json")).unwrap()).unwrap();
    let old_ms = 1_000_000_000_000u64; // 2001 : bien plus de 30 jours
    for k in ["saved_at_ms", "created_ms"] {
        if meta.get(k).is_some() {
            meta[k] = serde_json::Value::from(old_ms);
        }
    }
    std::fs::write(second.join("meta.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    let _ = std::fs::remove_file(second.join(backup::SAFETY_USED_FILE));
    let before = backup::safety_copies(&ctx, &game).len();
    write(&game.path, "save.dat", &content);
    let doomed = backup::safety_usage(&ctx, agss::util::now_ms()).plan.iter().any(|(_, c)| c.dir == second);
    assert!(doomed, "date forcee en 2001 et plus de 3 copies : la regle la condamne");
    let (o, _) = backup::restore_version_undoable(&ctx, &game, &dest, &v1).unwrap();
    assert_ne!(o.safety_dir.as_deref(), Some(second.as_path()), "copie condamnee par la regle : jamais reutilisee");
    assert!(o.safety_dir.as_ref().unwrap().join("meta.json").is_file(), "la copie de la remise existe bien");
    // (c) Verrou de « Liberer de la place » pose : aucune reutilisation, copie neuve.
    let lock = ctx.data_dir.join("safety").join(backup::SAFETY_FREE_LOCK);
    std::fs::write(&lock, agss::util::now_ms().to_string()).unwrap();
    assert!(backup::safety_free_running(&ctx));
    let n0 = backup::safety_copies(&ctx, &game).len();
    let (o2, _) = backup::restore_version_undoable(&ctx, &game, &dest, &v1).unwrap();
    assert_ne!(o2.safety_dir, o.safety_dir, "pendant le nettoyage : copie neuve");
    assert_eq!(backup::safety_copies(&ctx, &game).len(), n0 + 1);
    std::fs::write(&lock, (agss::util::now_ms() - 11 * 60_000).to_string()).unwrap();
    assert!(!backup::safety_free_running(&ctx), "verrou oublie (plantage) : expire apres 10 min");
    let _ = before;
}

// ================================================================== textes

#[test]
fn textes_2_0_3_dans_les_5_langues_et_memes_marqueurs() {
    let keys = [
        K::VSyncingTitle, K::VSortName, K::VSortLastSync, K::VSortModified, K::VSortSize, K::VSortState, K::VSortTip, K::VCloseFoundTitle, K::VCloseNoneTitle,
        K::VCloseNoneBody, K::VCloseAsk, K::VCloseSize, K::VCloseHint, K::VCloseNotifTitle, K::VCloseNotifBody, K::VSetCloseFullTip, K::VNotifHeldTitle,
        K::VNotifHeldUpdated, K::VWhyPlaynite, K::VPlayniteSection, K::VPlayniteHint, K::VPlayniteCard, K::VPlayniteCardOne, K::VSafetyTitle, K::VSafetyUsed,
        K::VSafetyRule, K::VSafetyFree, K::VSafetyAskTitle, K::VSafetyAskBody, K::VSafetyFreed, K::VSafetyUsedOne, K::VPlayniteCardFew,
    ];
    let marks = |s: &str| ["{}", "{0}", "{1}"].iter().map(|m| s.matches(m).count()).collect::<Vec<_>>();
    for k in keys {
        assert!(ALL_KEYS.contains(&k));
        let fr = tr(Lang::Fr, k);
        for l in Lang::ALL {
            let s = tr(l, k);
            assert!(!s.trim().is_empty(), "{:?} {:?}", l, k);
            assert_eq!(marks(s), marks(fr), "{:?} {:?}", l, k);
            if l != Lang::Fr && l != Lang::En {
                assert_ne!(s, tr(Lang::En, k), "{:?} {:?} non traduit", l, k);
            }
        }
    }
    // Un seul mot pour la meme chose dans le mode simple : le titre de l'accueil dit « Copie en cours… » comme les tuiles.
    for l in Lang::ALL {
        assert_eq!(tr(l, K::VSyncingTitle), tr(l, K::VCopying), "{:?}", l);
    }
    assert_eq!(tr(Lang::Fr, K::VPlayniteCard).replace("{}", "3"), "3 jeux Playnite pas encore protégés");
    // Singulier des copies de securite (« 1 copies » impossible) ; pluriels russes de la carte Playnite (1 / 2-4 / 5+).
    assert_eq!(tr(Lang::Fr, K::VSafetyUsedOne), "{0} utilisés · 1 copie");
    assert!(!tr(Lang::Es, K::VSafetyUsedOne).contains("copias"));
    use agss::i18n::{plural_form, Plural};
    assert_eq!((plural_form(1, Lang::Ru), plural_form(3, Lang::Ru), plural_form(5, Lang::Ru)), (Plural::One, Plural::Few, Plural::Many));
    assert_eq!((plural_form(11, Lang::Ru), plural_form(21, Lang::Ru), plural_form(22, Lang::Ru), plural_form(112, Lang::Ru)), (Plural::Many, Plural::One, Plural::Few, Plural::Many));
    assert_eq!((plural_form(1, Lang::Fr), plural_form(3, Lang::Fr)), (Plural::One, Plural::Many));
    assert!(tr(Lang::Ru, K::VPlayniteCardFew).contains("игры") && tr(Lang::Ru, K::VPlayniteCardFew).ends_with("защищены"));
    assert!(tr(Lang::Ru, K::VPlayniteCard).contains("игр ") && tr(Lang::Ru, K::VPlayniteCardOne).ends_with("защищена"));
    assert_eq!(tr(Lang::Ru, K::VNotifHeldTitle), "Во время твоего сеанса");
}
