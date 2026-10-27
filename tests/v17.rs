//! Apercu avant restauration : calcul du diff (ajoute / remplace / identique / local seulement / local plus recent).
use agss::backup;
use agss::config::Game;
use agss::i18n::{tr, Lang, K};
use agss::preview::*;
use agss::process::ProcessChecker;
use agss::store::FileInfo;
use agss::{Ctx, Res};
use std::path::Path;
use tempfile::TempDir;

fn fi(p: &str, size: u64, hash: &str) -> FileInfo {
    FileInfo { path: p.into(), size, hash: hash.into(), location: None }
}

const CLOUD_T: u64 = 1_000_000;

#[test]
fn diff_ajoute_remplace_identique_local_seulement_et_local_plus_recent() {
    let cloud = vec![fi("a.dat", 10, "h-a"), fi("b.dat", 20, "h-b"), fi("c.dat", 30, "h-c"), fi("d.dat", 40, "h-d")];
    let local = vec![
        (fi("a.dat", 10, "h-a"), CLOUD_T + 5),    // identique (meme empreinte, meme si plus recent)
        (fi("b.dat", 99, "AUTRE"), CLOUD_T - 100), // remplace, local plus ancien
        (fi("c.dat", 77, "AUTRE"), CLOUD_T + 500), // remplace, local PLUS RECENT que le cloud
        (fi("local.dat", 5, "h-l"), CLOUD_T),      // seulement local : conserve
    ];
    let p = compute(&cloud, CLOUD_T, &local);
    let kind = |n: &str| p.files.iter().find(|f| f.path == n).unwrap().kind;
    assert_eq!(kind("a.dat"), DiffKind::Identical);
    assert_eq!(kind("b.dat"), DiffKind::Replaced);
    assert_eq!(kind("c.dat"), DiffKind::LocalNewer);
    assert_eq!(kind("d.dat"), DiffKind::Added);
    assert_eq!((p.added, p.replaced, p.local_newer, p.identical), (1, 2, 1, 1));
    assert_eq!((p.added_bytes, p.replaced_bytes), (40, 20 + 30));
    assert_eq!(p.local_only, vec!["local.dat".to_string()], "fichier seulement local : conserve, jamais supprime");
    // Tailles et dates locales conservees pour l'affichage ; le fichier ajoute n'a pas de valeurs locales.
    let c = p.files.iter().find(|f| f.path == "c.dat").unwrap();
    assert_eq!((c.cloud_size, c.local_size, c.local_mtime_ms), (30, Some(77), Some(CLOUD_T + 500)));
    let d = p.files.iter().find(|f| f.path == "d.dat").unwrap();
    assert_eq!((d.local_size, d.local_mtime_ms), (None, None));
    // Ordre : plus recents en local d'abord, puis remplaces, ajoutes, identiques.
    let order: Vec<&str> = p.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(order, vec!["c.dat", "b.dat", "d.dat", "a.dat"]);
    assert_eq!(p.cloud_time_ms, CLOUD_T);
}

#[test]
fn dossier_local_vide_et_cloud_vide() {
    let p = compute(&[fi("x", 1, "h")], CLOUD_T, &[]);
    assert_eq!((p.added, p.replaced, p.identical, p.local_only.len()), (1, 0, 0, 0));
    let q = compute(&[], CLOUD_T, &[(fi("seul", 1, "h"), 5)]);
    assert!(q.files.is_empty() && q.local_only == vec!["seul".to_string()]);
}

#[test]
fn resume_du_journal() {
    let p = compute(&[fi("a", 1, "1"), fi("b", 1, "2")], CLOUD_T, &[(fi("b", 1, "X"), CLOUD_T + 1), (fi("z", 1, "z"), 1)]);
    assert_eq!(journal_summary(&p), "1 ajoutes, 1 remplaces (0 identiques, 1 seulement locaux conserves, dont 1 plus recents en local)");
}

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
    let g = Game { name: "TestGame".into(), path: root.join(format!("saves-{}", name)), exe: Some("t.exe".into()), dest_key: None, ..Default::default() };
    std::fs::create_dir_all(&g.path).unwrap();
    (ctx, g)
}

fn w(dir: &Path, rel: &str, c: &str) {
    std::fs::write(dir.join(rel), c).unwrap();
}

#[test]
fn apercu_reel_a_partir_du_dossier_partage_sans_rien_modifier() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, ga) = machine(t.path(), "A");
    w(&ga.path, "same.dat", "identique");
    w(&ga.path, "diff.dat", "version-cloud");
    w(&ga.path, "new.dat", "nouveau");
    backup::backup(&a, &ga, &dest).unwrap();
    let (b, gb) = machine(t.path(), "B");
    w(&gb.path, "same.dat", "identique");
    w(&gb.path, "diff.dat", "version-locale-differente");
    w(&gb.path, "mine.dat", "seulement-ici");
    let before: Vec<_> = walkdir::WalkDir::new(t.path()).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file() && e.path().starts_with(&gb.path)).map(|e| (e.path().to_path_buf(), std::fs::read(e.path()).unwrap())).collect();
    let p = for_game(&b, &gb, &dest, None).unwrap();
    assert_eq!((p.added, p.replaced, p.identical), (1, 1, 1));
    assert_eq!(p.local_only, vec!["mine.dat".to_string()]);
    // Le fichier local vient d'etre ecrit : plus recent que la sauvegarde cloud ? (selon l'ordre d'ecriture) -> au moins « remplace ».
    assert!(p.files.iter().any(|f| f.path == "diff.dat" && matches!(f.kind, DiffKind::Replaced | DiffKind::LocalNewer)));
    // Version inconnue : pas d'apercu ; aucune modification locale.
    assert!(for_game(&b, &gb, &dest, Some("inexistante")).is_none());
    let after: Vec<_> = before.iter().map(|(p, _)| (p.clone(), std::fs::read(p).unwrap())).collect();
    assert_eq!(before, after, "l'apercu ne modifie rien");
}

#[test]
fn textes_de_l_apercu_cinq_langues() {
    for k in [K::PrevTitle, K::PrevSummary, K::PrevLocalOnly, K::PrevLocalNewer, K::PrevLocal, K::PrevCloud, K::PrevMore, K::PrevComputing] {
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr(l, k) };
            assert!(!s.is_empty());
        }
    }
    assert!(tr(Lang::En, K::PrevLocalOnly).contains("never deleted"));
    assert!(tr(Lang::Fr, K::PrevLocalOnly).contains("jamais supprimés"));
}
