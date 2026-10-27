//! Fichier d'etat `status.json` (lu par des outils tiers) : schema, mapping des etats, ecriture conditionnelle et atomique.
use agss::statusfile::*;
use agss::vm::GameStatus;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const NOW: u64 = 1_793_000_000_000; // 2026-10-26T06:13:20Z environ

fn game(name: &str, st: GameStatus) -> GameIn {
    GameIn { name: name.into(), key: name.to_lowercase(), exe: Some("Jeu.exe".into()), install_dir: Some(PathBuf::from("E:\\Steam\\Jeu")), save_path: PathBuf::from("C:\\Saves\\Jeu"), status: st, last_sync_ms: Some(NOW - 60_000) }
}

fn inputs<'a>(games: Vec<GameIn>, shared: Option<&'a Path>) -> Inputs<'a> {
    Inputs { app_version: "1.0.0", now_ms: NOW, watching: true, paused: false, reachable: true, shared, user: "bob", games }
}

#[test]
fn schema_exact_cles_et_ordre() {
    let d = build(&inputs(vec![game("Jeu", GameStatus::UpToDate)], None));
    let j = to_json(&d);
    assert!(!j.starts_with('\u{feff}'), "UTF-8 sans BOM");
    assert!(j.contains("\n  \"schema\": 2,"), "indente");
    let v: serde_json::Value = serde_json::from_str(&j).unwrap();
    // Cles exactes, dans l'ordre du fichier.
    let pos: Vec<usize> = ["schema", "app_version", "updated_at_utc", "watching", "shared_folder_reachable", "games"].iter().map(|k| j.find(&format!("
  \"{}\":", k)).unwrap_or_else(|| panic!("{}", k))).collect();
    assert!(pos.windows(2).all(|w| w[0] < w[1]), "{:?}", pos);
    assert_eq!(v.as_object().unwrap().len(), 14); // schema 2 : 6 champs du schema 1 + 8 additifs (dont `devices`)
    assert_eq!(v["schema"], 2);
    assert_eq!(v["app_version"], "1.0.0");
    assert_eq!(v["watching"], true);
    assert_eq!(v["shared_folder_reachable"], true);
    let g = &v["games"][0];
    let gpos: Vec<usize> = ["name", "key", "exe", "install_dir", "save_path", "state", "last_sync_utc", "detail"].iter().map(|k| j.find(&format!("
      \"{}\":", k)).unwrap_or_else(|| panic!("{}", k))).collect();
    assert!(gpos.windows(2).all(|w| w[0] < w[1]), "{:?}", gpos);
    assert_eq!(g.as_object().unwrap().len(), 24); // 8 champs du schema 1 + 16 additifs (dont `locations`, `origin_device`, `origin_device_color`)
    assert_eq!((g["name"].as_str(), g["key"].as_str(), g["exe"].as_str()), (Some("Jeu"), Some("jeu"), Some("Jeu.exe")));
    assert_eq!((g["install_dir"].as_str(), g["save_path"].as_str(), g["state"].as_str(), g["detail"].as_str()), (Some("E:\\Steam\\Jeu"), Some("C:\\Saves\\Jeu"), Some("synced"), Some("")));
    let ts = v["updated_at_utc"].as_str().unwrap();
    assert!(ts.len() == 20 && ts.ends_with('Z') && ts.as_bytes()[10] == b'T', "{}", ts);
    assert_eq!(g["last_sync_utc"].as_str().unwrap().len(), 20);
}

#[test]
fn exe_dossier_et_date_absents_deviennent_chaine_vide_ou_null() {
    let mut g = game("X", GameStatus::Never);
    g.exe = None;
    g.install_dir = None;
    g.last_sync_ms = None;
    let v: serde_json::Value = serde_json::from_str(&to_json(&build(&inputs(vec![g], None)))).unwrap();
    let e = &v["games"][0];
    assert_eq!((e["exe"].as_str(), e["install_dir"].as_str()), (Some(""), Some("")));
    assert!(e["last_sync_utc"].is_null());
    assert_eq!(e["state"], "never_synced");
    assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
}

#[test]
fn mapping_des_etats() {
    let n = |s: &GameStatus, reach: bool, paused: bool| state_name(s, reach, paused);
    assert_eq!(n(&GameStatus::UpToDate, true, false), "synced");
    assert_eq!(n(&GameStatus::LocalChanged, true, false), "needs_backup");
    assert_eq!(n(&GameStatus::CloudNewer, true, false), "cloud_newer");
    assert_eq!(n(&GameStatus::Conflict, true, false), "conflict");
    assert_eq!(n(&GameStatus::Never, true, false), "never_synced");
    assert_eq!(n(&GameStatus::Error("x".into()), true, false), "error");
    assert_eq!(n(&GameStatus::Running, true, false), "running");
    // Priorites : jeu en cours > injoignable > pause > etat du jeu.
    assert_eq!(n(&GameStatus::Running, false, true), "running");
    assert_eq!(n(&GameStatus::UpToDate, false, true), "unreachable");
    assert_eq!(n(&GameStatus::NoDest, true, false), "unreachable");
    assert_eq!(n(&GameStatus::Conflict, true, true), "paused");
}

#[test]
fn aucun_chemin_du_dossier_partage_ni_nom_d_utilisateur_dans_detail() {
    let shared = Path::new("\\\\NAS\\Saves\\Partage");
    let msgs = ["copie \\\\NAS\\Saves\\Partage\\jeu\\saves\\x -> D:\\tmp : acces refuse", "echec pour //NAS/Saves/Partage/jeu : erreur", "dossier C:\\Users\\bob\\AppData\\Local\\x introuvable (bob)", "ligne1\nligne2 E:/autre/chemin fin"];
    for m in msgs {
        let d = sanitize_detail(m, Some(shared), "bob");
        assert!(!d.contains("NAS") && !d.contains("Partage") && !d.to_lowercase().contains("bob") && !d.contains(":\\") && !d.contains(":/") && !d.contains('\n'), "{:?} -> {:?}", m, d);
    }
    let d = build(&inputs(vec![game("J", GameStatus::Error("lecture \\\\NAS\\Saves\\Partage\\j : refuse (bob)".into()))], Some(shared)));
    assert_eq!(d.games[0].state, "error");
    assert!(!d.games[0].detail.contains("NAS") && !d.games[0].detail.contains("bob"));
    assert!(d.games[0].detail.chars().count() <= 120);
    assert_eq!(build(&inputs(vec![game("J", GameStatus::Error("x".repeat(500)))], None)).games[0].detail.chars().count(), 120);
    // Un etat sans erreur n'a pas de detail.
    assert_eq!(build(&inputs(vec![game("J", GameStatus::Conflict)], Some(shared))).games[0].detail, "");
}

#[test]
fn n_ecrit_que_si_le_contenu_change_au_plus_une_fois_par_seconde() {
    let dir = TempDir::new().unwrap();
    let mut w = StatusWriter::default();
    let d1 = build(&inputs(vec![game("Jeu", GameStatus::UpToDate)], None));
    assert_eq!(w.write_if_changed(dir.path(), &d1, NOW, false), Wrote::Written { first: true });
    let p = StatusWriter::path(dir.path());
    assert!(p.ends_with("status.json") && p.starts_with(dir.path()));
    let t1 = std::fs::metadata(&p).unwrap().modified().unwrap();
    // Meme contenu, seule la date change : rien n'est ecrit.
    let mut d1b = d1.clone();
    d1b.updated_at_utc = iso_utc(NOW + 5000);
    assert_eq!(w.write_if_changed(dir.path(), &d1b, NOW + 5000, false), Wrote::Unchanged);
    assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), t1);
    // Change : ecrit apres 1 s, differe avant.
    let d2 = build(&inputs(vec![game("Jeu", GameStatus::CloudNewer)], None));
    assert_eq!(w.write_if_changed(dir.path(), &d2, NOW + 500, false), Wrote::Throttled);
    assert_eq!(w.write_if_changed(dir.path(), &d2, NOW + 1000, false), Wrote::Written { first: false });
    let on_disk: StatusDoc = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(on_disk.games[0].state, "cloud_newer");
    // Ecriture finale (arret) : immediate meme sous la seconde, watching:false.
    let mut d3 = d2.clone();
    d3.watching = false;
    assert_eq!(w.write_if_changed(dir.path(), &d3, NOW + 1100, true), Wrote::Written { first: false });
    assert!(!serde_json::from_str::<StatusDoc>(&std::fs::read_to_string(&p).unwrap()).unwrap().watching);
}

#[test]
fn ecriture_atomique_aucun_fichier_temporaire_ne_reste() {
    let dir = TempDir::new().unwrap();
    let mut w = StatusWriter::default();
    for (i, st) in [GameStatus::UpToDate, GameStatus::Conflict, GameStatus::Never].into_iter().enumerate() {
        w.write_if_changed(dir.path(), &build(&inputs(vec![game("Jeu", st)], None)), NOW + i as u64 * 2000, false);
    }
    let names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, vec!["status.json".to_string()], "seul le fichier final subsiste (temporaire renomme)");
    let raw = std::fs::read(StatusWriter::path(dir.path())).unwrap();
    assert!(!raw.starts_with(&[0xEF, 0xBB, 0xBF]), "sans BOM");
    assert!(serde_json::from_slice::<StatusDoc>(&raw).is_ok());
}

#[test]
fn dossier_non_inscriptible_ne_casse_rien_et_n_est_signale_qu_une_fois() {
    let dir = TempDir::new().unwrap();
    // Un FICHIER a la place du dossier de donnees : l'ecriture echoue.
    let bad = dir.path().join("fichier-pas-dossier");
    std::fs::write(&bad, "x").unwrap();
    let mut w = StatusWriter::default();
    let d = build(&inputs(vec![game("Jeu", GameStatus::UpToDate)], None));
    assert!(matches!(w.write_if_changed(&bad, &d, NOW, false), Wrote::Failed { first: true, .. }));
    assert!(matches!(w.write_if_changed(&bad, &d, NOW + 2000, false), Wrote::Failed { first: false, .. }), "une seule ligne de journal");
}

#[test]
fn ancien_state_json_sans_date_de_synchro_reste_lisible() {
    let s: agss::config::State = serde_json::from_str(r#"{"last_synced":{"A":"h"}}"#).unwrap();
    assert!(s.last_sync_ms.is_empty());
    assert_eq!(s.last_synced.len(), 1);
}
