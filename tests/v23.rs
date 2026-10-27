//! 1.0.2 A : detection d'une sauvegarde distante par INTERROGATION directe (sans evenement), journal des transitions,
//! reglages par jeu qui masquaient la reactivite.
use agss::backup;
use agss::cloud;
use agss::cloudpoll::{self, CloudPoller};
use agss::config::Game;
use agss::dirwatch::WatchMsg;
use agss::process::ProcessChecker;
use agss::settings::Settings;
use agss::watch::{Logger, WatchCfg, Watcher};
use agss::{Ctx, Res};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

fn machine(root: &Path, name: &str) -> (Ctx, Game) {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let g = Game { name: "Jeu".into(), path: root.join(format!("saves-{}", name)), exe: Some("jeu.exe".into()), ..Default::default() };
    std::fs::create_dir_all(&g.path).unwrap();
    (ctx, g)
}

fn read(g: &Game) -> String {
    std::fs::read_to_string(g.path.join("s.dat")).unwrap_or_default()
}

/// A est a jour avec V1 ; B restaure V1, change et sauvegarde V2 dans `dest`.
fn setup(t: &TempDir) -> (Ctx, Game, Ctx, Game, std::path::PathBuf) {
    let dest = t.path().join("cloud");
    let (a, ga) = machine(t.path(), "PC-DESKTOP");
    std::fs::write(ga.path.join("s.dat"), "1").unwrap();
    backup::backup(&a, &ga, &dest).unwrap();
    let (b, gb) = machine(t.path(), "ALLY");
    backup::restore(&b, &gb, &dest).unwrap();
    (a, ga, b, gb, dest)
}

#[test]
fn poller_pur_detection_dans_l_intervalle_et_tolerance() {
    let mut p = CloudPoller::new(2, 2000);
    let mut sigs = [Some(1u64), Some(7u64)];
    assert!(p.tick(0, &mut |i| sigs[i]).is_empty(), "premiere observation = reference");
    sigs[0] = Some(2);
    assert!(p.tick(1500, &mut |i| sigs[i]).is_empty(), "pas avant l'echeance (pas de boucle serree)");
    assert_eq!(p.tick(2000, &mut |i| sigs[i]), vec![0], "detecte au plus tard a l'echeance suivante");
    assert!(p.tick(4000, &mut |i| sigs[i]).is_empty(), "pas de repetition");
    // Dossier injoignable : aucun faux changement, la reference est conservee.
    assert!(p.tick(6000, &mut |_| None).is_empty());
    assert!(p.tick(8000, &mut |i| sigs[i]).is_empty());
    sigs[1] = Some(8);
    assert_eq!(p.tick(10_000, &mut |i| sigs[i]), vec![1]);
}

#[test]
fn arrivee_sans_evenement_detectee_puis_restauree_horloge_simulee() {
    let t = TempDir::new().unwrap();
    let (a, ga, b, gb, dest) = setup(&t);
    let log = Logger::new(a.data_dir.join("logs").join("watch.log"), false);
    let (tx, rx) = channel();
    let mut w = Watcher::new(&a, vec![ga.clone()], dest.clone(), WatchCfg::default(), &log);
    w.set_events(rx);
    w.startup(0);
    let mut p = CloudPoller::new(1, 2000);
    assert!(p.tick(0, &mut |_| cloud::poll_signature(&dest, &ga)).is_empty());
    // Arrivee de V2 a t = 1500 ms, AUCUN evenement.
    std::fs::write(gb.path.join("s.dat"), "2").unwrap();
    backup::backup(&b, &gb, &dest).unwrap();
    let mut detected = None;
    let mut restored_at = None;
    for now in (100..=10_000u64).step_by(100) {
        for _i in p.tick(now, &mut |_| cloud::poll_signature(&dest, &ga)) {
            detected.get_or_insert(now);
            tx.send(WatchMsg::Changed(format!("{}/saves", cloud::dest_key(&ga)))).unwrap();
        }
        w.tick(now);
        if read(&ga) == "2" && restored_at.is_none() {
            restored_at = Some(now);
        }
    }
    let d = detected.expect("detection");
    assert!(d <= 2000, "detecte en moins d'un intervalle : {} ms", d);
    let r = restored_at.expect("restauration");
    assert!(r <= d + 2000 + 500, "restauration apres le delai de securite : detection {} ms, restauration {} ms", d, r);
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), backup::Plan::InSync);
    let l = std::fs::read_to_string(a.data_dir.join("logs").join("watch.log")).unwrap();
    assert!(l.contains("verification terminee : sauvegarde distante complete"), "{}", l);
}

#[test]
fn secours_periodique_respecte_l_intervalle_sans_evenement_ni_poll() {
    let t = TempDir::new().unwrap();
    let (a, ga, b, gb, dest) = setup(&t);
    let log = Logger::new(a.data_dir.join("logs").join("watch.log"), false);
    let mut w = Watcher::new(&a, vec![ga.clone()], dest.clone(), WatchCfg::default(), &log); // resynchro de secours : 60 s
    w.startup(0);
    std::fs::write(gb.path.join("s.dat"), "2").unwrap();
    backup::backup(&b, &gb, &dest).unwrap();
    let mut at = None;
    for now in (5_000..=130_000u64).step_by(5_000) {
        w.tick(now);
        if read(&ga) == "2" {
            at = Some(now);
            break;
        }
    }
    let at = at.expect("restaure par le controle de secours");
    assert!((60_000..=65_000).contains(&at), "controle de secours a 60 s (pas 5 ou 10 min) : {} ms", at);
}

#[test]
fn reglages_par_jeu_anciens_ne_masquent_plus_la_reactivite() {
    let t = TempDir::new().unwrap();
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr","per_game":{"Jeu":{"resync_secs":300,"stable_secs":10},"Autre":{"resync_secs":900}}}"#).unwrap();
    let s = Settings::load_from(t.path()).unwrap();
    assert_eq!(s.per_game["Jeu"].resync_secs, None);
    assert_eq!(s.per_game["Jeu"].stable_secs, None);
    assert_eq!(s.per_game["Autre"].resync_secs, Some(900), "un vrai reglage personnel est conserve");
}

#[test]
fn intervalles_d_interrogation_selon_la_reactivite() {
    use agss::reactive::{poll_interval_ms, Reactivity};
    assert_eq!(poll_interval_ms(Reactivity::Instant, 5), 2000);
    assert_eq!(poll_interval_ms(Reactivity::Normal, 5), 5000);
    assert_eq!(poll_interval_ms(Reactivity::Slow, 5), 10_000);
    assert_eq!(poll_interval_ms(Reactivity::Custom, 7), 7000);
    assert_eq!(poll_interval_ms(Reactivity::Custom, 0), 1000);
}

/// Mesure reelle (threads, horloge reelle, Instantane) : une sauvegarde apparait dans le dossier partage par COPIE
/// (comme Google Drive), sans evenement ; temps ecoule entre la fin de la copie et la restauration locale.
#[test]
fn mesure_detection_restauration_en_temps_reel() {
    let t = TempDir::new().unwrap();
    let (a, ga, b, gb, dest) = setup(&t);
    let staging = t.path().join("staging");
    std::fs::write(gb.path.join("s.dat"), "2").unwrap();
    backup::backup(&b, &gb, &staging).unwrap(); // V2 prepare hors du dossier partage
    let log = Logger::new(a.data_dir.join("logs").join("watch.log"), false);
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    cloudpoll::spawn(dest.clone(), vec![ga.clone()], 2000, tx, stop.clone());
    let mut w = Watcher::new(&a, vec![ga.clone()], dest.clone(), WatchCfg::default(), &log);
    w.set_events(rx);
    w.startup(0);
    let t0 = Instant::now();
    let (d2, st2) = (dest.clone(), staging.clone());
    let copier = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        let src = walkdir::WalkDir::new(&st2).into_iter().filter_map(|e| e.ok());
        for e in src {
            let rel = e.path().strip_prefix(&st2).unwrap();
            let to = d2.join(rel);
            if e.file_type().is_dir() {
                std::fs::create_dir_all(&to).unwrap();
            } else if e.file_name() != "meta.json" {
                std::fs::copy(e.path(), &to).unwrap();
            }
        }
        // meta.json en dernier, comme Drive l'expose une fois tout recu.
        for e in walkdir::WalkDir::new(&st2).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_name() == "meta.json") {
            std::fs::copy(e.path(), d2.join(e.path().strip_prefix(&st2).unwrap())).unwrap();
        }
        Instant::now()
    });
    let mut restored = None;
    while t0.elapsed() < Duration::from_secs(20) {
        w.tick(t0.elapsed().as_millis() as u64);
        if read(&ga) == "2" {
            restored = Some(Instant::now());
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stop.store(true, Ordering::SeqCst);
    let copied_at = copier.join().unwrap();
    let r = restored.expect("restauration sans evenement");
    let dt = r.duration_since(copied_at).as_secs_f64();
    println!("MESURE : fin de la copie -> restauration locale = {:.1} s (Instantane : interrogation 2 s + securite 2 s)", dt);
    assert!(dt < 8.0, "{:.1} s", dt);
}

/// Pause puis reprise (memes drapeaux que le menu de la zone de notification) : rien n'est synchronise pendant la pause,
/// la reprise verifie tout de suite ce qui est arrive entre-temps, et la pause/reprise sont journalisees.
#[test]
fn pause_puis_reprise_la_surveillance_repart() {
    let t = TempDir::new().unwrap();
    let (a, ga, b, gb, dest) = setup(&t);
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let (st2, pa2, dest2, ga2) = (stop.clone(), paused.clone(), dest.clone(), ga.clone());
    let a_dir = a.data_dir.clone();
    let h = std::thread::spawn(move || {
        let mut ctx = Ctx::new(a_dir);
        ctx.machine = "PC-DESKTOP".into();
        ctx.procs = Box::new(NotRunning);
        let log = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
        let cfg = WatchCfg { stable_ms: 200, resync_ms: 600_000, max_tries: 3, safety_ms: 200, retry_ms: 100, retry_max_ms: 5_000 };
        let mut w = Watcher::new(&ctx, vec![ga2], dest2, cfg, &log);
        agss::watch::run_loop_ex(&mut w, &ctx, 300, &st2, Some(&pa2));
    });
    std::thread::sleep(Duration::from_millis(800));
    paused.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(600));
    std::fs::write(gb.path.join("s.dat"), "2").unwrap();
    backup::backup(&b, &gb, &dest).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(read(&ga), "1", "rien n'est restaure pendant la pause");
    paused.store(false, Ordering::SeqCst);
    let t0 = Instant::now();
    while read(&ga) != "2" && t0.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    stop.store(true, Ordering::SeqCst);
    h.join().unwrap();
    assert_eq!(read(&ga), "2", "la reprise doit restaurer");
    println!("MESURE : reprise -> restauration = {:.1} s", t0.elapsed().as_secs_f64());
    let l = std::fs::read_to_string(a.data_dir.join("logs").join("watch.log")).unwrap();
    assert!(l.contains("[Surveillance] mise en pause") && l.contains("[Surveillance] reprise"), "{}", l);
}
