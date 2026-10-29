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
    // Textes d'interface : plus aucune trace des anciens noms (ils ne vivent que dans `legacy`).
    for k in agss::i18n::ALL_KEYS {
        for l in [Lang::Fr, Lang::En, Lang::Ru, Lang::Es, Lang::Zh] {
            let s = tr(l, *k).to_lowercase();
            for old in agss::legacy::DATA_DIRS.iter().chain(agss::legacy::TASK_FOLDERS.iter()) {
                let o = old.to_lowercase();
                let bad = if o == agss::legacy::DATA_DIRS[2].to_lowercase() { s.split(|c: char| !c.is_alphanumeric()).any(|w| w == o) } else { s.contains(&o) };
                assert!(!bad, "{:?}/{:?} cite un ancien nom", l, k);
            }
        }
    }
}

#[test]
fn migration_du_dossier_de_donnees_precedent_puis_ancien() {
    let t = TempDir::new().unwrap();
    let (prev, old, new) = (t.path().join(agss::legacy::DATA_DIRS[0]), t.path().join(agss::legacy::DATA_DIRS[2]), t.path().join("AutoGameSoftwareSync"));
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

// ------------------------------------------------------------------ entree Run et taches planifiees

fn old(id: &str) -> String {
    format!("{}\\{}", agss::legacy::TASK_FOLDERS[0], if id == "agss" { agss::legacy::APP_RULE_ID } else { id })
}

fn newt(id: &str) -> String {
    format!("AutoGameSoftwareSync\\{}", id)
}

#[cfg(windows)]
#[test]
fn entree_run_reecrite_seulement_si_elle_vise_notre_ancien_exe() {
    let new = pb("C:\\Apps\\AutoGameSoftwareSync.exe");
    for n in agss::legacy::EXE_NAMES {
        assert_eq!(rewrite_run_command(&format!("\"C:\\Old\\{}\" --tray", n), &new), Some("\"C:\\Apps\\AutoGameSoftwareSync.exe\" --tray".into()));
    }
    assert_eq!(rewrite_run_command(&format!("C:\\Old\\{}", agss::legacy::EXE_NAMES[1]), &new), Some("\"C:\\Apps\\AutoGameSoftwareSync.exe\"".into()));
    assert_eq!(rewrite_run_command("\"C:\\Tools\\OtherApp.exe\" --tray", &new), None, "pas a nous : intact");
}

#[test]
fn anciennes_taches_reconnues_seulement_si_elles_sont_les_notres() {
    for f in agss::legacy::TASK_FOLDERS {
        for ok in ["agss", "gdrive", "onedrive", "dropbox", "custom-12"] {
            let id = if ok == "agss" { agss::legacy::APP_RULE_ID } else { ok };
            assert!(is_legacy_task(&format!("{}\\{}", f, id)), "{} {}", f, ok);
            assert!(is_legacy_task(&format!("\\{}\\{}", f, id)));
        }
        for bad in ["tache-d-un-autre", "custom-", "custom-x", "", "gdrive\\x", "..\\gdrive"] {
            assert!(!is_legacy_task(&format!("{}\\{}", f, bad)), "{} {}", f, bad);
        }
        assert!(!is_legacy_task(f));
    }
    assert!(!is_legacy_task(&newt("gdrive")), "les taches du dossier actuel ne sont pas des anciennes");
    assert!(!is_legacy_task("Microsoft\\Windows\\gdrive"));
}


struct FakeSched {
    tasks: RefCell<Vec<String>>,
    deny_create: bool,
    deny_delete: bool,
    ops: RefCell<Vec<String>>,
}

impl FakeSched {
    fn new(tasks: &[&str]) -> Self {
        FakeSched { tasks: RefCell::new(tasks.iter().map(|s| s.to_string()).collect()), deny_create: false, deny_delete: false, ops: RefCell::new(vec![]) }
    }
}

impl Scheduler for FakeSched {
    fn list(&self) -> Result<Vec<String>, SchedError> {
        Ok(self.tasks.borrow().iter().filter(|n| is_own_task(n)).cloned().collect())
    }
    fn list_legacy(&self) -> Result<Vec<String>, SchedError> {
        Ok(self.tasks.borrow().iter().filter(|n| is_legacy_task(n)).cloned().collect())
    }
    fn create(&self, name: &str, _x: &[u8]) -> Result<(), SchedError> {
        if self.deny_create {
            return Err(SchedError::AccessDenied("refuse".into()));
        }
        self.ops.borrow_mut().push(format!("create {}", name));
        self.tasks.borrow_mut().push(name.to_string());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), SchedError> {
        if self.deny_delete {
            return Err(SchedError::AccessDenied("refuse".into()));
        }
        self.ops.borrow_mut().push(format!("delete {}", name));
        self.tasks.borrow_mut().retain(|t| t != name);
        Ok(())
    }
    fn run(&self, _n: &str) -> Result<(), SchedError> {
        Ok(())
    }
    fn query_xml(&self, _n: &str) -> Result<String, SchedError> {
        Ok(String::new())
    }
}


fn rules(found: &[&str]) -> Vec<Rule> {
    ["agss", "gdrive", "custom-2"]
        .iter()
        .map(|id| Rule { id: id.to_string(), kind: ToolKind::Custom, name: id.to_string(), command: pb("C:\\Apps\\AutoGameSoftwareSync.exe"), args: "--tray".into(), delay_secs: 40, enabled: false, found: found.contains(id), minimized: false })
        .collect()
}

#[test]
fn plan_de_migration_ne_touche_que_nos_anciennes_taches() {
    let legacy = vec![old("agss"), old("gdrive"), old("custom-2")];
    let p = plan_migrate(&legacy, &rules(&["agss", "gdrive"]));
    assert_eq!(p.create.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["agss", "gdrive"]);
    assert!(p.create.iter().all(|r| r.enabled), "la tache existait : la regle est activee");
    assert_eq!(p.delete, vec![old("agss"), old("gdrive")]);
    // custom-2 : programme introuvable -> l'ancienne tache est LAISSEE (rien n'est perdu).
    assert!(!p.delete.iter().any(|d| d.contains("custom-2")));
    // Une tache inconnue du dossier n'est jamais prise en compte.
    let p2 = plan_migrate(&[old("tache-d-un-autre")], &rules(&["agss"]));
    assert!(p2.create.is_empty() && p2.delete.is_empty());
    // Le second ancien dossier de taches est migre aussi.
    let other = format!("{}\\{}", agss::legacy::TASK_FOLDERS[1], "gdrive");
    let p3 = plan_migrate(&[other.clone()], &rules(&["gdrive"]));
    assert_eq!(p3.delete, vec![other]);
}

#[test]
fn migration_automatique_sans_elevation() {
    let s = FakeSched::new(&[&old("agss"), &old("gdrive"), &old("tache-d-un-autre"), "Microsoft\\Windows\\x"]);
    let r = migrate_tasks_auto(&s, &rules(&["agss", "gdrive"]), "PC\\moi");
    assert_eq!(r, TaskMigration::Done(2));
    let t = s.tasks.borrow().clone();
    assert!(t.contains(&newt("agss")) && t.contains(&newt("gdrive")), "nouvelles taches creees");
    assert!(!t.contains(&old("agss")) && !t.contains(&old("gdrive")), "anciennes taches retirees");
    assert!(t.contains(&old("tache-d-un-autre")) && t.contains(&"Microsoft\\Windows\\x".to_string()), "autres taches jamais touchees");
    // Creations AVANT suppressions.
    let ops = s.ops.borrow().clone();
    let first_delete = ops.iter().position(|o| o.starts_with("delete")).unwrap();
    assert!(ops[..first_delete].iter().all(|o| o.starts_with("create")) && first_delete == 2);
    // Idempotent : une 2e execution n'a plus rien a faire.
    assert_eq!(migrate_tasks_auto(&s, &rules(&["agss", "gdrive"]), "PC\\moi"), TaskMigration::NothingToDo);
}

#[test]
fn migration_refusee_garde_les_anciennes_taches() {
    // Creation refusee (droits) : invite dans les Reglages, l'ancienne tache reste.
    let mut s = FakeSched::new(&[&old("agss")]);
    s.deny_create = true;
    assert_eq!(migrate_tasks_auto(&s, &rules(&["agss"]), "u"), TaskMigration::NeedsElevation(1));
    assert_eq!(*s.tasks.borrow(), vec![old("agss")]);
    // Creation reussie mais suppression refusee : la nouvelle existe, l'ancienne reste, invite dans les Reglages.
    let mut s = FakeSched::new(&[&old("agss")]);
    s.deny_delete = true;
    assert_eq!(migrate_tasks_auto(&s, &rules(&["agss"]), "u"), TaskMigration::NeedsElevation(1));
    assert!(s.tasks.borrow().contains(&old("agss")) && s.tasks.borrow().contains(&newt("agss")));
}

#[test]
fn migration_avec_elevation_une_seule_fenetre() {
    let mut s = FakeSched::new(&[&old("agss"), &old("gdrive")]);
    s.deny_create = true;
    let calls = RefCell::new(Vec::<ApplyFile>::new());
    let msg = migrate_tasks_with(&s, &rules(&["agss", "gdrive"]), "u", &|f| {
        calls.borrow_mut().push(ApplyFile { creates: f.creates.clone(), deletes: f.deletes.clone() });
        Ok(())
    })
    .unwrap();
    assert!(msg.contains("2"), "{}", msg);
    let c = calls.borrow();
    assert_eq!(c.len(), 1, "une seule elevation (une seule fenetre UAC)");
    assert_eq!(c[0].creates.iter().map(|x| x.0.clone()).collect::<Vec<_>>(), vec![newt("agss"), newt("gdrive")]);
    assert_eq!(c[0].deletes, vec![old("agss"), old("gdrive")]);
    // Refus de l'utilisateur dans la fenetre Windows : rien n'est modifie.
    let r = migrate_tasks_with(&s, &rules(&["agss"]), "u", &|_| Err("annule".into()));
    assert!(r.is_err());
    assert!(s.ops.borrow().is_empty() && s.tasks.borrow().len() == 2);
}

#[test]
fn cote_eleve_l_ancienne_tache_n_est_supprimee_que_si_les_nouvelles_sont_creees() {
    // Etape avec droits eleves : une creation echoue -> les anciennes taches restent.
    let mut s = FakeSched::new(&[&old("agss")]);
    s.deny_create = true;
    let f = ApplyFile { creates: vec![(newt("agss"), "<x/>".into())], deletes: vec![old("agss")] };
    let out = apply_file(&s, &f);
    assert!(out.deleted.is_empty() && !out.failed.is_empty());
    assert_eq!(*s.tasks.borrow(), vec![old("agss")]);
    // Tout va bien : creation puis suppression. Une tache hors de nos dossiers est refusee.
    let s = FakeSched::new(&[&old("agss")]);
    let f = ApplyFile { creates: vec![(newt("agss"), "<x/>".into())], deletes: vec![old("agss"), old("inconnue"), "Microsoft\\y".into()] };
    let out = apply_file(&s, &f);
    assert_eq!(out.deleted, vec![old("agss")]);
    assert_eq!(out.failed.len(), 2, "tache inconnue et tache hors dossier refusees");
}
