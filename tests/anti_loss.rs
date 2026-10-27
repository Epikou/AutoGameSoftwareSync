//! Tests anti-perte. UNIQUEMENT des dossiers temporaires, jamais de vraies sauvegardes.
use agss::backup::{self, BackupOutcome};
use agss::config::Game;
use agss::process::ProcessChecker;
use agss::{Ctx, Res};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

struct FakeProc(Arc<AtomicBool>);
impl ProcessChecker for FakeProc {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct Machine {
    ctx: Ctx,
    game: Game,
    running: Arc<AtomicBool>,
}

fn machine(root: &Path, name: &str) -> Machine {
    let running = Arc::new(AtomicBool::new(false));
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(FakeProc(running.clone()));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "TestGame".into(), path: root.join(format!("saves-{}", name)), exe: Some("testgame.exe".into()), dest_key: None, ..Default::default() };
    Machine { ctx, game, running }
}

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}
fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}
fn game_of(dest_unused: &Path) -> Game {
    let _ = dest_unused;
    Game { name: "TestGame".into(), path: PathBuf::new(), exe: None, dest_key: None, ..Default::default() }
}
/// Toutes les sauvegardes (nouvelle disposition) : la plus ancienne en premier, l'ACTIVE en dernier.
fn versions(dest: &Path) -> Vec<PathBuf> {
    backup::list_versions(dest, &game_of(dest))
}
fn active_dir(dest: &Path) -> PathBuf {
    versions(dest).pop().expect("au moins une sauvegarde")
}
fn current_save(dest: &Path) -> String {
    read(&active_dir(dest).join("files/save.dat"))
}
/// Reste de dossier de travail (.tmp-*) ou de fichier temporaire, dans la destination ou dans le dossier local de travail.
fn staging_left(dest: &Path, m: &Machine) -> bool {
    let mut left = false;
    for e in walkdir::WalkDir::new(dest).into_iter().filter_map(|e| e.ok()) {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(".tmp-") || n.ends_with(".agss-tmp") {
            left = true;
        }
    }
    let t = m.ctx.data_dir.join("tmp");
    left || (t.exists() && std::fs::read_dir(t).unwrap().next().is_some())
}

#[test]
fn backup_cree_versions_et_elague_a_10() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let m = machine(t.path(), "A");
    for i in 1..=13 {
        write(&m.game.path, "save.dat", &format!("v{}", i));
        write(&m.game.path, "sub/extra.dat", "x");
        backup::backup(&m.ctx, &m.game, &dest).unwrap();
    }
    assert_eq!(current_save(&dest), "v13");
    let v = versions(&dest);
    assert_eq!(v.len(), 11, "10 anciennes versions + l'active");
    // les plus recentes anciennes versions sont v12 et v11
    assert_eq!(read(&v[9].join("files/save.dat")), "v12");
    assert_eq!(read(&v[8].join("files/save.dat")), "v11");
    assert!(v[10].join("meta.json").exists());
    assert!(v[9].join("meta.json").exists());
}

#[test]
fn elagage_garde_toujours_au_moins_2_versions() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let mut m = machine(t.path(), "A");
    m.ctx.keep_versions = 0; // tentative d'elagage agressif
    for i in 1..=8 {
        write(&m.game.path, "save.dat", &format!("v{}", i));
        backup::backup(&m.ctx, &m.game, &dest).unwrap();
    }
    let v = versions(&dest);
    assert_eq!(v.len(), 3, "plancher de 2 anciennes versions + l'active");
    assert_eq!(read(&v[2].join("files/save.dat")), "v8");
    assert_eq!(read(&v[1].join("files/save.dat")), "v7");
    assert_eq!(read(&v[0].join("files/save.dat")), "v6");
}

#[test]
fn backup_identique_ne_fait_rien() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let m = machine(t.path(), "A");
    write(&m.game.path, "save.dat", "v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    assert!(matches!(backup::backup(&m.ctx, &m.game, &dest).unwrap(), BackupOutcome::UpToDate));
    assert_eq!(versions(&dest).len(), 1, "aucune nouvelle sauvegarde creee");
}

#[test]
fn restore_fait_un_snapshot_safety_et_ne_supprime_rien() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "cloud-state");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();

    let b = machine(t.path(), "B");
    write(&b.game.path, "save.dat", "etat-local-precieux");
    write(&b.game.path, "autre.dat", "fichier-local-seul");
    let r = backup::restore(&b.ctx, &b.game, &dest).unwrap();
    assert_eq!(read(&b.game.path.join("save.dat")), "cloud-state");
    let safety = r.safety_dir.expect("snapshot safety");
    assert!(safety.starts_with(b.ctx.data_dir.join("safety")));
    assert_eq!(read(&safety.join("files/save.dat")), "etat-local-precieux");
    assert_eq!(read(&safety.join("files/autre.dat")), "fichier-local-seul");
    assert!(r.local_had_unsynced_changes);
    // aucun fichier local supprime
    assert_eq!(read(&b.game.path.join("autre.dat")), "fichier-local-seul");
}

#[test]
fn restore_refuse_si_le_jeu_tourne() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "cloud");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();

    let b = machine(t.path(), "B");
    write(&b.game.path, "save.dat", "local");
    b.running.store(true, Ordering::SeqCst);
    let err = backup::restore(&b.ctx, &b.game, &dest).unwrap_err().to_string();
    assert!(err.contains("REFUS"), "{}", err);
    assert_eq!(read(&b.game.path.join("save.dat")), "local");
    assert!(!b.ctx.data_dir.join("safety").exists());
    // une fois ferme, ca passe
    b.running.store(false, Ordering::SeqCst);
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    assert_eq!(read(&b.game.path.join("save.dat")), "cloud");
}

#[test]
fn conflit_local_plus_recent_garde_les_deux() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    let b = machine(t.path(), "B");
    write(&a.game.path, "save.dat", "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap(); // B connait v1
    write(&b.game.path, "save.dat", "v2-depuis-B");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    write(&a.game.path, "save.dat", "v3-depuis-A"); // A n'a jamais vu v2
    let out = backup::backup(&a.ctx, &a.game, &dest).unwrap();
    assert!(matches!(out, BackupOutcome::Conflict { .. }), "{:?}", out);
    let all: Vec<String> = versions(&dest).iter().map(|v| read(&v.join("files/save.dat"))).collect();
    assert!(all.contains(&"v2-depuis-B".to_string()), "{:?}", all);
    assert!(all.contains(&"v1".to_string()), "{:?}", all);
}

#[test]
fn conflit_cloud_plus_recent_garde_les_deux() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    let b = machine(t.path(), "B");
    write(&a.game.path, "save.dat", "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    write(&b.game.path, "save.dat", "v2-cloud");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    write(&a.game.path, "save.dat", "v3-local-ancien");
    // fichier local A date de tres longtemps
    let f = std::fs::OpenOptions::new().write(true).open(a.game.path.join("save.dat")).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000)).unwrap();
    drop(f);
    let out = backup::backup(&a.ctx, &a.game, &dest).unwrap();
    assert!(matches!(out, BackupOutcome::Conflict { .. }), "{:?}", out);
    let all: Vec<String> = versions(&dest).iter().map(|v| read(&v.join("files/save.dat"))).collect();
    assert!(all.contains(&"v3-local-ancien".to_string()), "{:?}", all);
    assert_eq!(read(&a.game.path.join("save.dat")), "v3-local-ancien"); // local intact
}

#[test]
fn premiere_synchro_avec_cloud_different_est_un_conflit() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    let b = machine(t.path(), "B");
    write(&a.game.path, "save.dat", "de-A");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    write(&b.game.path, "save.dat", "de-B"); // B n'a jamais synchronise
    let out = backup::backup(&b.ctx, &b.game, &dest).unwrap();
    assert!(matches!(out, BackupOutcome::Conflict { .. }));
    let all: Vec<String> = versions(&dest).iter().map(|v| read(&v.join("files/save.dat"))).collect();
    assert!(all.contains(&"de-A".to_string()) && all.contains(&"de-B".to_string()), "{:?}", all);
}

#[test]
fn echec_de_verification_backup_ne_supprime_rien() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let mut m = machine(t.path(), "A");
    write(&m.game.path, "save.dat", "v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    write(&m.game.path, "save.dat", "v2");
    m.ctx.fault_corrupt_copy = true;
    let err = backup::backup(&m.ctx, &m.game, &dest).unwrap_err().to_string();
    assert!(err.contains("VERIFICATION"), "{}", err);
    assert_eq!(current_save(&dest), "v1", "cloud inchange");
    assert_eq!(versions(&dest).len(), 1, "aucune sauvegarde fantome");
    assert_eq!(read(&m.game.path.join("save.dat")), "v2", "local intact");
    assert!(!staging_left(&dest, &m), "dossier de travail nettoye");
    // et apres reparation, tout refonctionne
    m.ctx.fault_corrupt_copy = false;
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    assert_eq!(current_save(&dest), "v2");
    assert_eq!(versions(&dest).len(), 2);
}

#[test]
fn echec_de_verification_restore_ne_supprime_rien() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "cloud");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    let mut b = machine(t.path(), "B");
    write(&b.game.path, "save.dat", "local-precieux");
    b.ctx.fault_corrupt_copy = true;
    let res = backup::restore(&b.ctx, &b.game, &dest);
    assert!(res.is_err());
    // Le snapshot de securite lui-meme echoue (copie verifiee) => rien n'est touche.
    assert_eq!(read(&b.game.path.join("save.dat")), "local-precieux");
    assert_eq!(current_save(&dest), "cloud");
    assert!(!b.game.path.join("save.dat.agss-tmp").exists());
}

#[test]
fn restore_refuse_une_sauvegarde_cloud_corrompue() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = machine(t.path(), "A");
    write(&a.game.path, "save.dat", "cloud");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    std::fs::write(active_dir(&dest).join("files/save.dat"), "corrompu").unwrap();
    let b = machine(t.path(), "B");
    write(&b.game.path, "save.dat", "local");
    let err = backup::restore(&b.ctx, &b.game, &dest).unwrap_err().to_string();
    assert!(err.contains("corrompue"), "{}", err);
    assert_eq!(read(&b.game.path.join("save.dat")), "local");
    assert!(!b.ctx.data_dir.join("safety").exists());
}

#[test]
fn backup_refuse_un_dossier_vide() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let m = machine(t.path(), "A");
    write(&m.game.path, "save.dat", "v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    std::fs::remove_file(m.game.path.join("save.dat")).unwrap();
    assert!(backup::backup(&m.ctx, &m.game, &dest).is_err());
    assert_eq!(current_save(&dest), "v1");
}

#[cfg(windows)]
#[test]
fn catalog_resolution_placeholders() {
    use agss::catalog::{expand_glob, resolve, Entry};
    let e: Entry = serde_yaml::from_str(
        "files:\n  '<winDocuments>/My Game/<storeUserId>/save.dat': {tags: [save]}\n  '<winAppData>/G': {tags: [save], when: [{os: windows}]}\n  '<home>/.linux': {when: [{os: linux}]}\n  '<xdgData>/x': {}\n  '<winDir>/../x': {}\n",
    )
    .unwrap();
    let r = resolve(&e, "G");
    assert!(r.paths.iter().any(|p| p.contains("My Game") && p.contains('*')), "{:?}", r.paths);
    assert!(r.paths.iter().any(|p| p.ends_with("\\G")));
    assert!(!r.paths.iter().any(|p| p.contains(".linux")));
    assert!(r.unresolved.contains(&"xdgData".to_string()));
    // glob
    let t = TempDir::new().unwrap();
    write(t.path(), "My Game/12345/save.dat", "x");
    let pat = format!("{}\\My Game\\*\\save.dat", t.path().display());
    assert_eq!(expand_glob(&pat).len(), 1);
}

#[test]
fn horodatage_connu() {
    assert_eq!(agss::util::timestamp(0), "19700101-000000-000");
    assert_eq!(agss::util::timestamp(1_700_000_000_123), "20231114-221320-123");
}

/// Linux : les marqueurs Windows du catalogue sont resolus dans le prefixe Proton de l'AppID Steam du jeu.
#[cfg(not(windows))]
#[test]
fn catalog_resolution_placeholders_proton() {
    use agss::catalog::{expand_glob, resolve_with, Entry, StoreCtx};
    let t = TempDir::new().unwrap();
    let lib = t.path().join("lib");
    let pfx = lib.join("steamapps/compatdata/42/pfx");
    write(&pfx.join("drive_c/users/steamuser/Documents"), "My Game/12345/save.dat", "x");
    std::fs::create_dir_all(pfx.join("drive_c/users/steamuser/AppData/Roaming/G")).unwrap();
    let e: Entry = serde_yaml::from_str(
        "steam: {id: 42}\nfiles:\n  '<winDocuments>/my game/<storeUserId>/save.dat': {tags: [save]}\n  '<winAppData>/G': {tags: [save], when: [{os: windows}]}\n  '<home>/.linux': {when: [{os: linux}]}\n  '<xdgData>/x': {}\n",
    )
    .unwrap();
    let sc = StoreCtx { steam_libs: vec![lib.clone()], install_dir: None, game_id: None };
    let r = resolve_with(&e, "G", &sc);
    let docs = pfx.join("drive_c/users/steamuser/Documents").to_string_lossy().to_string();
    assert!(r.paths.iter().any(|p| p.starts_with(&docs) && p.contains('*')), "{:?}", r.paths);
    assert!(r.paths.iter().any(|p| p.ends_with("AppData/Roaming/G")), "{:?}", r.paths);
    assert!(!r.paths.iter().any(|p| p.contains(".linux")));
    assert!(r.unresolved.contains(&"xdgData".to_string()));
    // Le motif (casse differente, joker) retrouve le fichier reel dans le prefixe.
    let hit = r.paths.iter().find(|p| p.contains('*')).unwrap();
    eprintln!("HIT={}", hit);
    let found = expand_glob(hit);
    assert_eq!(found.len(), 1, "{:?}", found);
    assert!(found[0].ends_with("My Game/12345/save.dat"));
    // Sans prefixe connu (AppID inconnu ou pas de bibliotheque), rien n'est invente.
    let none = resolve_with(&e, "G", &StoreCtx::default());
    assert!(none.paths.is_empty(), "{:?}", none.paths);
}
