//! Lot 17 bis (controle Android 0.1.0) : nettoyage prudent, au demarrage, d'une version restee incomplete dans le coffre
//! apres un arret brutal (note `tmp/claimed.txt`). UNIQUEMENT des dossiers temporaires. Meme comportement PC et Android.
use agss::backup;
use agss::config::Game;
use agss::process::ProcessChecker;
use agss::{Ctx, Res};
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

struct Closed;
impl ProcessChecker for Closed {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(false)
    }
}

fn ctx(root: &Path, name: &str) -> Ctx {
    let mut c = Ctx::new(root.join(format!("data-{}", name)));
    c.machine = name.to_string();
    c.procs = Box::new(Closed);
    std::fs::create_dir_all(c.data_dir.join("tmp")).unwrap();
    c
}

fn note(c: &Ctx, dir: &Path) {
    std::fs::write(c.data_dir.join("tmp").join("claimed.txt"), dir.to_string_lossy().as_bytes()).unwrap();
}

fn incomplete_version(vault: &Path, name: &str) -> std::path::PathBuf {
    let d = vault.join("documents").join("saves").join(name);
    std::fs::create_dir_all(d.join("files")).unwrap();
    std::fs::write(d.join("files").join("a.txt"), "partiel").unwrap();
    d
}

#[test]
fn version_incomplete_de_cette_machine_retiree_au_demarrage() {
    let t = TempDir::new().unwrap();
    let c = ctx(t.path(), "Console");
    let vault = t.path().join("vault");
    let d = incomplete_version(&vault, "20260101-000000-000-Console");
    note(&c, &d);
    assert_eq!(backup::cleanup_claimed(&c, Duration::ZERO), Some(d.clone()));
    assert!(!d.exists(), "dossier incomplet retire");
    assert!(!c.data_dir.join("tmp/claimed.txt").exists(), "note retiree");
    // Deuxieme appel : plus rien a faire.
    assert_eq!(backup::cleanup_claimed(&c, Duration::ZERO), None);
}

#[test]
fn jamais_une_version_complete_ni_celle_d_une_autre_machine_ni_hors_saves() {
    let t = TempDir::new().unwrap();
    let c = ctx(t.path(), "Console");
    let vault = t.path().join("vault");
    // 1. Version complete (meta.json present) : jamais touchee, note retiree.
    let full = incomplete_version(&vault, "20260101-000000-000-Console");
    std::fs::write(full.join("meta.json"), "{}").unwrap();
    note(&c, &full);
    assert_eq!(backup::cleanup_claimed(&c, Duration::ZERO), None);
    assert!(full.join("meta.json").exists() && full.join("files/a.txt").exists());
    assert!(!c.data_dir.join("tmp/claimed.txt").exists());
    // 2. Version incomplete d'une AUTRE machine : jamais touchee (elle est peut-etre en cours de transfert par la synchro).
    let other = incomplete_version(&vault, "20260101-000000-000-PC");
    note(&c, &other);
    assert_eq!(backup::cleanup_claimed(&c, Duration::ZERO), None);
    assert!(other.exists());
    // 3. Dossier hors d'un `saves/` : jamais touche.
    let elsewhere = t.path().join("ailleurs").join("20260101-000000-000-Console");
    std::fs::create_dir_all(&elsewhere).unwrap();
    note(&c, &elsewhere);
    assert_eq!(backup::cleanup_claimed(&c, Duration::ZERO), None);
    assert!(elsewhere.exists());
    // 4. Note vers un dossier qui n'existe plus : note simplement retiree.
    note(&c, &vault.join("documents/saves/20260102-000000-000-Console"));
    assert_eq!(backup::cleanup_claimed(&c, Duration::ZERO), None);
    assert!(!c.data_dir.join("tmp/claimed.txt").exists());
    // 5. Age minimal (PC) : un dossier tout frais n'est pas retire, la note est gardee pour le prochain demarrage.
    let fresh = incomplete_version(&vault, "20260103-000000-000-Console");
    note(&c, &fresh);
    assert_eq!(backup::cleanup_claimed(&c, Duration::from_secs(600)), None);
    assert!(fresh.exists() && c.data_dir.join("tmp/claimed.txt").exists());
}

#[test]
fn une_sauvegarde_normale_ne_laisse_aucune_note() {
    let t = TempDir::new().unwrap();
    let c = ctx(t.path(), "Console");
    let vault = t.path().join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    let g = Game { name: "Documents".into(), path: t.path().join("Documents"), ..Default::default() };
    std::fs::create_dir_all(&g.path).unwrap();
    std::fs::write(g.path.join("a.txt"), "1").unwrap();
    backup::backup(&c, &g, &vault).unwrap();
    assert!(!c.data_dir.join("tmp/claimed.txt").exists(), "note retiree une fois meta.json ecrit");
    assert_eq!(backup::cleanup_claimed(&c, Duration::ZERO), None);
    let saves = vault.join("documents").join("saves");
    let n = std::fs::read_dir(&saves).unwrap().count();
    assert_eq!(n, 1);
    // Le nettoyage du dossier de travail local ignore la note et ne touche qu'aux dossiers.
    std::fs::write(c.data_dir.join("tmp").join("claimed.txt"), "x").unwrap();
    assert_eq!(backup::cleanup_local_tmp(&c, Duration::ZERO), 0);
}
