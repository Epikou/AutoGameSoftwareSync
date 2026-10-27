//! v0.10 phase B : icone de la zone de notification dynamique, renommage AutoGameSoftwareSync, migrations
//! (dossier de donnees, entree Run, taches planifiees). UNIQUEMENT des dossiers temporaires et un planificateur simule.
use agss::actions::{migrate_tasks_auto, migrate_tasks_with, TaskMigration};
use agss::appdirs;
use agss::i18n::{tr, Lang, K};
use agss::startup::*;
use agss::trayicon::*;
use agss::watch::EventKind;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pb(s: &str) -> PathBuf {
    PathBuf::from(s)
}

// ------------------------------------------------------------------ etat de l'icone

#[test]
fn priorite_rouge_jaune_orange_bleu_gris_vert() {
    // Toutes les combinaisons des 6 entrees.
    for bits in 0u32..64 {
        let i = TrayInputs { error: bits & 1 != 0, conflict: bits & 2 != 0, unreachable: bits & 4 != 0, on_demand: bits & 8 != 0, syncing: bits & 16 != 0, paused: bits & 32 != 0 };
        let want = if i.error {
            TrayState::Error
        } else if i.conflict {
            TrayState::Conflict
        } else if i.unreachable {
            TrayState::Unreachable
        } else if i.on_demand {
            TrayState::OnDemand
        } else if i.syncing {
            TrayState::Syncing
        } else if i.paused {
            TrayState::Paused
        } else {
            TrayState::Ok
        };
        assert_eq!(state_of(&i), want, "{:?}", i);
    }
    assert_eq!(state_of(&TrayInputs::default()), TrayState::Ok);
}

#[test]
fn suivi_des_conflits_et_des_erreurs_recentes() {
    let mut t = Tracker::default();
    assert!(!t.conflict() && !t.error_recent(0));
    t.on_event(EventKind::Conflict, "A", 1_000);
    t.on_event(EventKind::Conflict, "B", 1_000);
    assert!(t.conflict());
    t.on_event(EventKind::ConflictResolved, "A", 2_000);
    assert!(t.conflict(), "B reste en conflit");
    t.on_event(EventKind::Restored, "B", 3_000);
    assert!(!t.conflict());
    // Erreur : recente pendant 10 minutes, effacee par un succes du meme jeu.
    t.on_event(EventKind::Error, "A", 10_000);
    assert!(t.error_recent(10_000 + ERROR_RECENT_MS - 1));
    assert!(!t.error_recent(10_000 + ERROR_RECENT_MS), "plus recente apres 10 minutes");
    t.on_event(EventKind::BackedUp, "A", 20_000);
    assert!(!t.error_recent(20_001));
    t.on_event(EventKind::Error, "A", 30_000);
    t.on_event(EventKind::BackedUp, "B", 31_000);
    assert!(t.error_recent(31_001), "le succes d'un autre jeu n'efface pas l'erreur");
}

fn base_icon() -> Vec<u8> {
    include_bytes!("../media/icon_32.rgba").to_vec()
}

fn px(img: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * 32 + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

fn close(a: [u8; 4], b: [u8; 3]) -> bool {
    (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= 6)
}

/// Couleur de la pastille : echantillons sur le pourtour (hors pictogramme central), la valeur la plus frequente.
fn badge_px(img: &[u8]) -> [u8; 4] {
    let (c, r) = (32.0_f32 * 0.69, 32.0_f32 * 0.30);
    let mut v: Vec<[u8; 4]> = (0..8).map(|k| {
        let a = k as f32 * std::f32::consts::FRAC_PI_4 + 0.3;
        px(img, (c + 0.82 * r * a.cos()) as u32, (c + 0.82 * r * a.sin()) as u32)
    }).collect();
    v.sort();
    let mut best = (0, v[0]);
    for x in &v {
        let n = v.iter().filter(|y| close(**y, [x[0], x[1], x[2]])).count();
        if n > best.0 {
            best = (n, *x);
        }
    }
    best.1
}

#[test]
fn icone_pastille_generee_par_etat() {
    let base = base_icon();
    assert_eq!(base.len(), 32 * 32 * 4);
    let expect = [
        (TrayState::Error, [220u8, 50, 47]),
        (TrayState::Conflict, [242, 201, 0]),
        (TrayState::Unreachable, [245, 140, 25]),
        (TrayState::OnDemand, [245, 140, 25]),
        (TrayState::Syncing, [50, 120, 240]),
        (TrayState::Paused, [128, 128, 128]),
        (TrayState::Ok, [46, 184, 92]),
    ];
    for (s, col) in expect {
        let img = render(&base, 32, s, 0);
        assert_eq!(img.len(), base.len());
        // Point de la pastille hors pictogramme (bas gauche du disque) : couleur de l'etat.
        assert!(close(badge_px(&img), col), "{:?} : {:?}", s, badge_px(&img));
        // Le haut gauche de l'icone de base n'est pas modifie.
        assert_eq!(px(&img, 2, 2), px(&base, 2, 2), "{:?} : icone de base intacte", s);
    }
    // Couleurs distinctes entre etats (sauf les deux oranges, volontairement identiques).
    let mut cols: Vec<[u8; 4]> = [TrayState::Error, TrayState::Conflict, TrayState::Unreachable, TrayState::Syncing, TrayState::Paused, TrayState::Ok].iter().map(|s| badge_px(&render(&base, 32, *s, 0))).collect();
    cols.sort();
    cols.dedup();
    assert_eq!(cols.len(), 6);
}

#[test]
fn animation_seulement_pendant_la_synchronisation() {
    let base = base_icon();
    assert_ne!(render(&base, 32, TrayState::Syncing, 0), render(&base, 32, TrayState::Syncing, 1), "deux images differentes");
    for s in [TrayState::Ok, TrayState::Paused, TrayState::Error, TrayState::Conflict, TrayState::Unreachable] {
        assert_eq!(render(&base, 32, s, 0), render(&base, 32, s, 1), "{:?} : pas d'animation", s);
        assert_eq!(anim_frame(s, 123_456), 0);
    }
    assert_eq!((anim_frame(TrayState::Syncing, 0), anim_frame(TrayState::Syncing, 499), anim_frame(TrayState::Syncing, 500), anim_frame(TrayState::Syncing, 1000)), (0, 0, 1, 0));
}

#[test]
fn planche_des_icones_si_demandee() {
    // Diagnostic : TRAY_SHEET=<fichier.png> ecrit une planche (32 px et reduction a 16 px) pour relecture visuelle.
    let Some(path) = std::env::var_os("TRAY_SHEET") else { return };
    let base = base_icon();
    let states = [TrayState::Ok, TrayState::Syncing, TrayState::Paused, TrayState::Unreachable, TrayState::Conflict, TrayState::Error];
    let (cell, w, h) = (64u32, 64u32 * 6, 64u32 * 2);
    let mut sheet = vec![255u8; (w * h * 4) as usize];
    for (n, s) in states.iter().enumerate() {
        for (row, frame) in [(0u32, 0u32), (1, 1)] {
            let img = render(&base, 32, *s, frame);
            // agrandi x2 sur fond gris clair (apercu de la zone de notification)
            for y in 0..cell {
                for x in 0..cell {
                    let (sx, sy) = (x / 2, y / 2);
                    let p = px(&img, sx, sy);
                    let a = p[3] as f32 / 255.0;
                    let o = (((row * cell + y) * w + n as u32 * cell + x) * 4) as usize;
                    for k in 0..3 {
                        sheet[o + k] = (p[k] as f32 * a + 200.0 * (1.0 - a)) as u8;
                    }
                }
            }
        }
    }
    let pm = resvg::tiny_skia::Pixmap::from_vec(sheet, resvg::tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap();
    pm.save_png(path).unwrap();
}

#[test]
fn textes_de_l_infobulle_de_la_zone_de_notification() {
    for k in [K::TrayStOk, K::TrayStSyncing, K::TrayStPaused, K::TrayStUnreachable, K::TrayStOnDemand, K::TrayStConflict, K::TrayStError] {
        assert!(!tr(Lang::Fr, k).is_empty() && !tr(Lang::En, k).is_empty() && tr(Lang::Fr, k) != tr(Lang::En, k));
    }
    assert_eq!(agss::gui::APP_SHORT, "AGSS");
}

// ------------------------------------------------------------------ renommage et migration des donnees

#[test]
fn noms_autogamesoftwaresync_partout() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("name = \"autogamesoftwaresync\"") && cargo.contains("name = \"AutoGameSoftwareSync\"") && cargo.contains("name = \"agss\""));
    let build = std::fs::read_to_string(root.join("build.rs")).unwrap();
    for must in ["ProductName\", \"AutoGameSoftwareSync", "OriginalFilename\", \"AutoGameSoftwareSync.exe", "FileDescription\", \"AutoGameSoftwareSync"] {
        assert!(build.contains(must), "{}", must);
    }
    assert_eq!(agss::gui::APP_TITLE, "AutoGameSoftwareSync");
    assert_eq!(agss::winutil::RUN_VALUE, "AutoGameSoftwareSync");
    assert_eq!(TASK_FOLDER, "AutoGameSoftwareSync");
    assert_eq!(appdirs::APP_DIR, "AutoGameSoftwareSync");
}

#[test]
fn migration_du_dossier_de_donnees_precedent_puis_ancien() {
    let t = TempDir::new().unwrap();
    let (prev, old, new) = (t.path().join("ancien-a"), t.path().join("ancien-b"), t.path().join("AutoGameSoftwareSync"));
    let pick = |a: &PathBuf, b: &PathBuf| appdirs::pick_first_existing(&[a.clone(), b.clone()]);
    assert_eq!(pick(&prev, &old), None);
    std::fs::create_dir_all(&old).unwrap();
    assert_eq!(pick(&prev, &old), Some(old.clone()));
    std::fs::create_dir_all(&prev).unwrap();
    assert_eq!(pick(&prev, &old), Some(prev.clone()), "le premier de la liste est prioritaire");
    std::fs::write(prev.join("config.json"), "{\"games\":[]}").unwrap();
    std::fs::create_dir_all(prev.join("logs")).unwrap();
    std::fs::write(prev.join("logs").join("watch.log"), "ligne").unwrap();
    let r = appdirs::migrate(&prev, &new).unwrap();
    assert!(r.skipped.is_none() && r.files == 2, "{:?}", r);
    assert_eq!(std::fs::read_to_string(new.join("config.json")).unwrap(), "{\"games\":[]}");
    assert!(new.join(appdirs::MIGRATION_MARKER).exists());
    assert_eq!(std::fs::read_to_string(prev.join("config.json")).unwrap(), "{\"games\":[]}", "ancien dossier intact");
    std::fs::write(new.join("config.json"), "{\"games\":[\"modifie\"]}").unwrap();
    let r2 = appdirs::migrate(&prev, &new).unwrap();
    assert!(r2.skipped.is_some());
    assert_eq!(std::fs::read_to_string(new.join("config.json")).unwrap(), "{\"games\":[\"modifie\"]}", "jamais d'ecrasement");
}

#[test]
fn variable_d_environnement_prioritaire() {
    // Un seul test touche a l'environnement (processus partage).
    for k in ["AUTOGAMESOFTWARESYNC_DATA_DIR", "AUTOGAMESOFTWARESYNC_DATA_DIR", "AUTOGAMESOFTWARESYNC_DATA_DIR"] {
        std::env::remove_var(k);
    }
    assert_eq!(appdirs::data_dir_from_env(), None);
    std::env::set_var("AUTOGAMESOFTWARESYNC_DATA_DIR", "C:\\a");
    assert_eq!(appdirs::data_dir_from_env(), Some(pb("C:\\a")));
    std::env::set_var("AUTOGAMESOFTWARESYNC_DATA_DIR", "C:\\b");
    assert_eq!(appdirs::data_dir_from_env(), Some(pb("C:\\b")));
    std::env::set_var("AUTOGAMESOFTWARESYNC_DATA_DIR", "C:\\c");
    assert_eq!(appdirs::data_dir_from_env(), Some(pb("C:\\c")), "AUTOGAMESOFTWARESYNC_DATA_DIR est prioritaire");
    for k in ["AUTOGAMESOFTWARESYNC_DATA_DIR", "AUTOGAMESOFTWARESYNC_DATA_DIR", "AUTOGAMESOFTWARESYNC_DATA_DIR"] {
        std::env::remove_var(k);
    }
}

