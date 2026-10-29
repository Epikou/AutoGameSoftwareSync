//! Regression « Cloud plus recent qui reste » : premier contact d'un PC (etat vide) avec un cloud different.
use agss::backup::{self, Plan};
use agss::config::Game;
use agss::process::ProcessChecker;
use agss::vm::{compute_status, GameStatus};
use agss::watch::{Logger, WatchCfg, Watcher};
use agss::{Ctx, Res};
use std::path::Path;
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
    let g = Game { name: "Fieldofmysteria".into(), path: root.join(format!("saves-{}", name)), exe: Some("fom.exe".into()), dest_key: None, ..Default::default() };
    std::fs::create_dir_all(&g.path).unwrap();
    (ctx, g)
}

fn w(dir: &Path, rel: &str, c: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}

fn count(dir: &Path) -> usize {
    walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).count()
}

fn log_of(ctx: &Ctx) -> String {
    std::fs::read_to_string(ctx.data_dir.join("logs").join("watch.log")).unwrap_or_default()
}

fn round(ctx: &Ctx, g: &Game, dest: &Path, auto: bool, now: u64) {
    let log = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
    let mut wt = Watcher::new(ctx, vec![g.clone()], dest.to_path_buf(), WatchCfg::default(), &log);
    wt.auto_cloud.insert(g.name.clone(), auto);
    wt.startup(now);
    wt.tick(now + 300_000);
}

fn assert_a_jour(ctx: &Ctx, g: &Game, dest: &Path) {
    assert_eq!(backup::plan(ctx, g, dest).unwrap(), Plan::InSync);
    assert_eq!(compute_status(ctx, g, Some(dest)), GameStatus::UpToDate);
}

#[test]
fn premier_contact_local_22_cloud_11_converge_et_journalise() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (b, gb) = machine(t.path(), "EPIGOS");
    for i in 0..11 {
        w(&gb.path, &format!("slot{}.dat", i), &format!("B-{}", i));
    }
    backup::backup(&b, &gb, &dest).unwrap();
    let (a, ga) = machine(t.path(), "EPIKOU");
    for i in 0..11 {
        w(&ga.path, &format!("slot{}.dat", i), &if i < 6 { format!("B-{}", i) } else { format!("A-{}", i) });
        w(&ga.path, &format!("extra{}.dat", i), "extra");
    }
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), Plan::Conflict);
    for k in 0..3 {
        round(&a, &ga, &dest, true, k * 1_000_000);
    }
    assert_a_jour(&a, &ga, &dest);
    assert_eq!(count(&ga.path), 22, "aucun fichier local supprime");
    // La version locale d'origine (slot6..10 = A-i) reste retrouvable dans l'historique.
    let hist = backup::history(&a, &ga, &dest);
    assert!(hist.rows.iter().any(|r| r.files == 22 && r.machine == "EPIKOU"));
    let log = log_of(&a);
    assert!(log.contains("premiere synchro : versions differentes (local 22 fichiers / cloud 11 fichiers de EPIGOS) = conflit"), "{}", log);
    assert!(log.contains("conflit : version locale gardee en branche, version du cloud appliquee (mode cloud automatique)"), "{}", log);
    // B converge a son tour.
    round(&b, &gb, &dest, true, 5_000_000);
    round(&b, &gb, &dest, true, 6_000_000);
    assert_a_jour(&b, &gb, &dest);
    assert_a_jour(&a, &ga, &dest);
}

#[test]
fn local_ancetre_exact_plus_fichiers_ne_boucle_plus() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    std::fs::create_dir_all(&dest).unwrap(); // 2.0 : la surveillance ne cree jamais le coffre lui-meme
    let (a, ga) = machine(t.path(), "EPIKOU");
    for i in 0..11 {
        w(&ga.path, &format!("slot{}.dat", i), &format!("S-{}", i));
        w(&ga.path, &format!("extra{}.dat", i), "extra");
    }
    round(&a, &ga, &dest, true, 0); // V1 = 22 fichiers
    let (b, gb) = machine(t.path(), "EPIGOS");
    round(&b, &gb, &dest, true, 1_000_000);
    for i in 0..11 {
        std::fs::remove_file(gb.path.join(format!("extra{}.dat", i))).unwrap();
    }
    round(&b, &gb, &dest, true, 2_000_000); // V2 = 11 fichiers, descendant de V1
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), Plan::Restore);
    round(&a, &ga, &dest, true, 10_000_000);
    round(&a, &ga, &dest, true, 20_000_000);
    assert_a_jour(&a, &ga, &dest);
    let n = backup::history(&a, &ga, &dest).rows.len();
    for k in 0..3 {
        round(&a, &ga, &dest, true, 30_000_000 + k * 1_000_000);
        assert_a_jour(&a, &ga, &dest);
    }
    assert_eq!(backup::history(&a, &ga, &dest).rows.len(), n, "plus aucune version creee une fois converge");
    assert_eq!(count(&ga.path), 22, "aucun fichier local supprime");
    let log = log_of(&a);
    assert!(log.contains("restauration terminee, 11 fichiers locaux supplementaires conserves"), "{}", log);
    assert_eq!(log.matches("cloud plus recent detecte").count(), 1, "une seule restauration, pas d'aller-retour : {}", log);
    // B reprend la nouvelle tete et converge aussi.
    round(&b, &gb, &dest, true, 40_000_000);
    assert_a_jour(&b, &gb, &dest);
    assert_a_jour(&a, &ga, &dest);
}

#[test]
fn sauvegarde_identique_tete_ignoree_ancetre_ecrit() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, ga) = machine(t.path(), "EPIKOU");
    w(&ga.path, "s.dat", "1");
    assert!(matches!(backup::backup(&a, &ga, &dest).unwrap(), backup::BackupOutcome::Backed { .. }));
    assert!(matches!(backup::backup(&a, &ga, &dest).unwrap(), backup::BackupOutcome::UpToDate));
    w(&ga.path, "s.dat", "2");
    assert!(matches!(backup::backup(&a, &ga, &dest).unwrap(), backup::BackupOutcome::Backed { .. }));
    // Retour au contenu de la premiere version (ancetre, plus une tete) : nouvelle tete, pas un « ignore ».
    w(&ga.path, "s.dat", "1");
    assert!(matches!(backup::backup(&a, &ga, &dest).unwrap(), backup::BackupOutcome::Backed { .. }));
    assert_a_jour(&a, &ga, &dest);
}
