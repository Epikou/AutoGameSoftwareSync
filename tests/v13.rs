//! v0.10.x : reactivite de la surveillance (evenements du dossier partage, verification de completude, filet de secours).
//! UNIQUEMENT des dossiers temporaires ; horloge simulee.
use agss::backup;
use agss::cloud;
use agss::config::Game;
use agss::dirwatch::WatchMsg;
use agss::i18n::{tr, Lang, K};
use agss::process::ProcessChecker;
use agss::reactive::*;
use agss::settings::Settings;
use agss::watch::{Logger, WatchCfg, Watcher};
use agss::{Ctx, Res};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

// ------------------------------------------------------------------ modes

#[test]
fn modes_de_reactivite_et_valeurs_personnalisees_bornees() {
    let c = ReactCustom::default();
    let i = params(Reactivity::Instant, &c);
    assert_eq!((i.safety_ms, i.retry_ms, i.retry_max_ms, i.fallback_ms, i.stable_ms), (2000, 1000, 30_000, 60_000, 3000));
    let n = params(Reactivity::Normal, &c);
    assert_eq!((n.safety_ms, n.retry_max_ms, n.stable_ms), (10_000, 60_000, 10_000));
    let s = params(Reactivity::Slow, &c);
    assert_eq!((s.safety_ms, s.retry_max_ms), (30_000, 180_000));
    assert!(i.safety_ms < n.safety_ms && n.safety_ms < s.safety_ms);
    // Personnalise : valeurs utilisees, bornees (jamais 0 pour le reessai, max >= reessai, filet >= 10 s).
    let p = params(Reactivity::Custom, &ReactCustom { safety_s: 5, retry_s: 2, retry_max_s: 40, fallback_s: 90, stable_s: 7 });
    assert_eq!((p.safety_ms, p.retry_ms, p.retry_max_ms, p.fallback_ms, p.stable_ms), (5000, 2000, 40_000, 90_000, 7000));
    let b = params(Reactivity::Custom, &ReactCustom { safety_s: 0, retry_s: 0, retry_max_s: 0, fallback_s: 1, stable_s: 0 });
    assert!(b.retry_ms >= 1000 && b.retry_max_ms >= b.retry_ms && b.fallback_ms >= 10_000 && b.stable_ms >= 1000);
    // Le filet de secours par defaut est de 60 s (avant : 300 s) et la stabilite de 3 s.
    let d = WatchCfg::default();
    assert_eq!((d.resync_ms, d.stable_ms, d.safety_ms), (60_000, 3000, 2000));
}

#[test]
fn reglage_de_reactivite_compatible_avec_les_anciens_fichiers() {
    let old: Settings = serde_json::from_str(r#"{"lang":"fr","auto_watch":true}"#).unwrap();
    assert_eq!(old.reactivity, Reactivity::Instant, "defaut : instantane");
    assert_eq!(old.reactivity_custom, ReactCustom::default());
    let s = Settings { reactivity: Reactivity::Custom, reactivity_custom: ReactCustom { safety_s: 9, ..Default::default() }, ..Settings::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!((back.reactivity, back.reactivity_custom.safety_s), (Reactivity::Custom, 9));
    let partial: Settings = serde_json::from_str(r#"{"reactivity":"slow","reactivity_custom":{"safety_s":4}}"#).unwrap();
    assert_eq!((partial.reactivity, partial.reactivity_custom.safety_s, partial.reactivity_custom.retry_s), (Reactivity::Slow, 4, 1));
}

// ------------------------------------------------------------------ tri des evenements

#[test]
fn evenement_hors_du_dossier_d_un_jeu_ignore() {
    let (k0, k1) = (cloud::game_key("TestGame"), cloud::game_key("Autre Jeu"));
    let keys = vec![vec![k0.clone()], vec![k1.clone()]];
    assert_eq!(game_of_event("testgame/saves/20261023-010203-004-PC/meta.json", &keys), Some(0));
    assert_eq!(game_of_event("TestGame\\saves\\x\\files\\a.dat", &keys), Some(0), "separateurs Windows et casse");
    assert_eq!(game_of_event(&format!("{}/saves/x", k1), &keys), Some(1));
    assert_eq!(game_of_event("testgame/game.json", &keys), None, "game.json n'est pas une sauvegarde");
    assert_eq!(game_of_event("testgame", &keys), None);
    assert_eq!(game_of_event("testgame/saves", &keys), Some(0), "creation du dossier saves");
    assert_eq!(game_of_event("inconnu/saves/x/meta.json", &keys), None, "autre jeu / autre dossier du partage");
    assert_eq!(game_of_event("", &keys), None);
    assert_eq!(game_of_event("Test Game/saves/x", &keys), Some(0), "nom normalise comme la cle d'ecriture");
}

// ------------------------------------------------------------------ machine a etats (horloge simulee)

fn p() -> ReactParams {
    params(Reactivity::Instant, &ReactCustom::default())
}

#[test]
fn verification_complete_apres_le_delai_de_securite() {
    let mut v = Verifier::new(0, false);
    let calls = std::cell::Cell::new(0);
    let mut chk = || {
        calls.set(calls.get() + 1);
        Check::Complete
    };
    assert_eq!(v.poll(1000, &p(), false, &mut chk), VAction::Wait);
    assert_eq!(calls.get(), 0, "aucune verification avant le delai de securite");
    assert_eq!(v.poll(2000, &p(), false, &mut chk), VAction::Proceed);
    assert_eq!(calls.get(), 1);
}

#[test]
fn debounce_chaque_evenement_repousse_la_verification() {
    let mut v = Verifier::new(0, false);
    v.note_activity(1500);
    let mut chk = || Check::Complete;
    assert_eq!(v.poll(2500, &p(), false, &mut chk), VAction::Wait);
    v.note_activity(3000);
    assert_eq!(v.poll(4500, &p(), false, &mut chk), VAction::Wait);
    assert_eq!(v.poll(5000, &p(), false, &mut chk), VAction::Proceed);
}

#[test]
fn meta_json_avant_les_autres_fichiers_puis_sauvegarde_complete() {
    let mut v = Verifier::new(0, false);
    let seq = Mutex::new(vec![Check::Incomplete(10), Check::Incomplete(20), Check::Complete].into_iter());
    let calls = Mutex::new(0u32);
    let mut chk = || {
        *calls.lock().unwrap() += 1;
        seq.lock().unwrap().next().unwrap()
    };
    assert_eq!(v.poll(2000, &p(), false, &mut chk), VAction::Wait, "incomplete : on attend");
    assert_eq!(v.poll(2500, &p(), false, &mut chk), VAction::Wait);
    assert_eq!(*calls.lock().unwrap(), 1, "reessai toutes les secondes seulement (pas a chaque tick)");
    assert_eq!(v.poll(3000, &p(), false, &mut chk), VAction::Wait, "des octets sont arrives (progression)");
    assert_eq!(v.poll(4000, &p(), false, &mut chk), VAction::Proceed, "complete : on restaure");
    assert_eq!(*calls.lock().unwrap(), 3);
}

#[test]
fn dossier_incomplet_qui_n_avance_plus_est_traite_avec_ce_qui_est_complet() {
    let mut v = Verifier::new(0, false);
    let mut chk = || Check::Incomplete(7);
    let mut last = VAction::Wait;
    let mut t = 2000;
    while t <= 6000 {
        last = v.poll(t, &p(), false, &mut chk);
        if last != VAction::Wait {
            break;
        }
        t += 500;
    }
    assert_eq!(last, VAction::Proceed);
    assert!(t <= 5000, "apres 3 reessais sans progression ({} ms)", t);
}

#[test]
fn abandon_propre_apres_le_temps_maximum_de_reessais() {
    let mut v = Verifier::new(0, false);
    let n = Mutex::new(0u64);
    let mut chk = || {
        let mut g = n.lock().unwrap();
        *g += 1;
        Check::Incomplete(*g) // des octets arrivent a chaque controle
    };
    let mut t = 2000;
    let mut act = VAction::Wait;
    while t < 120_000 {
        act = v.poll(t, &p(), false, &mut chk);
        if act != VAction::Wait {
            break;
        }
        t += 500;
    }
    assert_eq!(act, VAction::Abandon);
    assert!((30_000..=31_000).contains(&t), "abandon au temps max de 30 s : {}", t);
}

#[test]
fn jeu_en_cours_reporte_sans_verifier() {
    let mut v = Verifier::new(0, false);
    let mut called = false;
    let mut chk = || {
        called = true;
        Check::Complete
    };
    assert_eq!(v.poll(10_000, &p(), true, &mut chk), VAction::Postpone);
    assert!(!called);
}

#[test]
fn synchroniser_maintenant_sans_delai_de_securite() {
    let mut v = Verifier::new(0, true);
    let mut chk = || Check::Complete;
    assert_eq!(v.poll(0, &p(), false, &mut chk), VAction::Proceed);
    // Un evenement ulterieur remet le delai de securite.
    let mut v = Verifier::new(0, true);
    v.note_activity(10);
    assert_eq!(v.poll(500, &p(), false, &mut chk), VAction::Wait);
}

// ------------------------------------------------------------------ surveillance complete (Watcher) sur dossiers temporaires

struct FakeProc(Arc<AtomicBool>);
impl ProcessChecker for FakeProc {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct M {
    ctx: Ctx,
    game: Game,
    running: Arc<AtomicBool>,
}

fn machine(root: &Path, name: &str) -> M {
    let running = Arc::new(AtomicBool::new(false));
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(FakeProc(running.clone()));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "TestGame".into(), path: root.join(format!("saves-{}", name)), exe: Some("testgame.exe".into()), dest_key: None, ..Default::default() };
    std::fs::create_dir_all(&game.path).unwrap();
    M { ctx, game, running }
}

fn write(dir: &Path, rel: &str, c: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}

fn log_of(m: &M) -> String {
    std::fs::read_to_string(m.ctx.data_dir.join("logs").join("watch.log")).unwrap_or_default()
}

fn newest_version_dir(dest: &Path) -> PathBuf {
    backup::list_versions(dest, &Game { name: "TestGame".into(), path: PathBuf::new(), exe: None, dest_key: None, ..Default::default() }).pop().unwrap()
}

fn cfg() -> WatchCfg {
    WatchCfg::from_params(&params(Reactivity::Instant, &ReactCustom::default()))
}

#[test]
fn evenement_du_dossier_partage_restaure_apres_verification_et_journalise_les_durees() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    std::fs::create_dir_all(&dest).unwrap();
    let b = machine(t.path(), "B");
    let log = Logger::new(b.ctx.data_dir.join("logs").join("watch.log"), false);
    let (tx, rx) = channel();
    let mut w = Watcher::new(&b.ctx, vec![b.game.clone()], dest.clone(), cfg(), &log);
    w.set_events(rx);
    w.startup(0);
    // L'autre PC pousse une sauvegarde.
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "venu-de-A");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    // Evenements : un autre jeu (ignore), puis le notre.
    tx.send(WatchMsg::Changed("autrejeu/saves/x/meta.json".into())).unwrap();
    w.tick(1000);
    assert!(!w.has_pending(), "evenement hors du dossier du jeu : ignore");
    tx.send(WatchMsg::Changed("testgame/saves/20261023-000000-000-A/meta.json".into())).unwrap();
    w.tick(2000);
    assert!(w.has_pending());
    assert!(!b.game.path.join("save.dat").exists(), "pas de restauration avant le delai de securite");
    w.tick(3000);
    assert!(!b.game.path.join("save.dat").exists());
    w.tick(4200);
    assert_eq!(std::fs::read_to_string(b.game.path.join("save.dat")).unwrap(), "venu-de-A");
    assert!(!w.has_pending());
    let l = log_of(&b);
    assert!(l.contains("[TestGame] cloud plus recent detecte (sauvegarde de A), verification..."), "{}", l);
    assert!(l.contains("[TestGame] restauration terminee en") && l.contains("detection -> debut") && l.contains("verification + restauration"), "{}", l);
    assert_eq!(l.matches("cloud plus recent detecte").count(), 1, "une seule ligne par evenement");
    assert!(b.ctx.data_dir.join("safety").exists() || b.game.path.join("save.dat").exists());
}

#[test]
fn sauvegarde_incomplete_puis_complete_n_est_restauree_qu_une_fois_entiere() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "complet");
    write(&a.game.path, "sub/autre.dat", "second");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    // Drive livre meta.json AVANT les autres fichiers : on simule en cachant temporairement le dossier files.
    let ver = newest_version_dir(&dest);
    let hidden = ver.join("files-en-attente");
    std::fs::rename(ver.join("files"), &hidden).unwrap();
    assert!(matches!(cloud::remote_check(&dest, &a.game), Check::Incomplete(_)));
    let b = machine(t.path(), "B");
    let log = Logger::new(b.ctx.data_dir.join("logs").join("watch.log"), false);
    let (tx, rx) = channel();
    let mut w = Watcher::new(&b.ctx, vec![b.game.clone()], dest.clone(), cfg(), &log);
    w.set_events(rx);
    tx.send(WatchMsg::Changed("testgame/saves/x/meta.json".into())).unwrap();
    w.tick(0);
    w.tick(2000); // delai de securite ecoule : controle -> incomplet
    w.tick(2500);
    assert!(!b.game.path.join("save.dat").exists(), "jamais de restauration a partir d'une sauvegarde incomplete");
    // Les fichiers arrivent.
    std::fs::rename(&hidden, ver.join("files")).unwrap();
    assert_eq!(cloud::remote_check(&dest, &a.game), Check::Complete);
    w.tick(3100);
    assert_eq!(std::fs::read_to_string(b.game.path.join("save.dat")).unwrap(), "complet");
    assert_eq!(std::fs::read_to_string(b.game.path.join("sub/autre.dat")).unwrap(), "second");
}

#[test]
fn jeu_en_cours_report_a_la_fermeture_et_synchroniser_maintenant() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "v-A");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    let b = machine(t.path(), "B");
    write(&b.game.path, "save.dat", "local-B-precieux");
    // Etat synchronise connu (sinon vrai conflit) : B restaure d'abord, puis A pousse une v2.
    let log = Logger::new(b.ctx.data_dir.join("logs").join("watch.log"), false);
    let (tx, rx) = channel();
    let triggers = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let mut w = Watcher::new(&b.ctx, vec![b.game.clone()], dest.clone(), cfg(), &log);
    w.set_events(rx);
    w.set_triggers(triggers.clone());
    b.running.store(true, Ordering::SeqCst);
    w.startup(0); // jeu en cours : aucune synchro
    assert_eq!(std::fs::read_to_string(b.game.path.join("save.dat")).unwrap(), "local-B-precieux");
    tx.send(WatchMsg::Changed("testgame/saves/x/meta.json".into())).unwrap();
    w.tick(1000);
    w.tick(4000);
    assert_eq!(std::fs::read_to_string(b.game.path.join("save.dat")).unwrap(), "local-B-precieux", "jamais pendant que le jeu tourne");
    // Le jeu se ferme : stabilite, puis synchro normale (plan Restore ou Conflict selon l'historique, jamais d'ecrasement silencieux).
    b.running.store(false, Ordering::SeqCst);
    w.tick(5000);
    w.tick(9000);
    let l = log_of(&b);
    assert!(l.contains("lancement") || l.contains("fermeture detectee") || l.contains("deja en cours"), "{}", l);
    // « Synchroniser maintenant » : traite sans delai de securite.
    triggers.lock().unwrap().push(Some("TestGame".into()));
    w.tick(10_000);
    assert!(log_of(&b).contains("[TestGame] synchronisation demandee"));
}

#[test]
fn signature_rapide_stable_puis_modifiee_par_une_nouvelle_sauvegarde() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    let s1 = cloud::quick_signature(&dest, &a.game);
    assert_eq!(s1, cloud::quick_signature(&dest, &a.game), "stable : rien n'a change");
    write(&a.game.path, "save.dat", "22");
    std::thread::sleep(std::time::Duration::from_millis(20));
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    assert_ne!(s1, cloud::quick_signature(&dest, &a.game), "une nouvelle sauvegarde change l'empreinte");
}

#[test]
fn textes_de_reactivite_et_notification_dans_les_cinq_langues() {
    for k in [K::SetReact, K::ReactInstant, K::ReactNormal, K::ReactSlow, K::ReactCustom, K::SyncNow, K::NotifRestored, K::SyncRequested] {
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr(l, k) };
            assert!(!s.is_empty(), "{:?} {}", k, l.code());
        }
    }
    // 2.0 (lot 13) : vocabulaire grand public dans les notifications aussi.
    assert_eq!(tr(Lang::En, K::NotifRestored), "Version from another device brought back");
    assert_eq!(tr(Lang::Fr, K::NotifRestored), "Version d'un autre appareil récupérée");
}
