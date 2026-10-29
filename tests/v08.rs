//! Tests v0.8 : renommage AutoGameSoftwareSync + migration, valeurs par defaut, conflit « cloud automatique »,
//! regles de demarrage (taches planifiees, parties pures + planificateur simule), icone, wording.
use agss::appdirs;
use agss::backup::{self, Plan};
use agss::config::Game;
use agss::i18n::{tr, Lang, ALL_KEYS, K};
use agss::notify::{self, Notifier};
use agss::process::ProcessChecker;
use agss::settings::{ConflictMode, Settings, StartupRuleCfg, ThemeMode, DEFAULT_ACCENT};
use agss::startup::{self, *};
use agss::watch::{EventKind, Logger, WatchCfg, WatchEvent, Watcher};
use agss::{util, Ctx, Res};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

fn write(p: &Path, c: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}
fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

// ================================================================== renommage + migration

#[test]
fn nom_du_produit_binaire_titre_et_valeur_run() {
    let cargo = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    assert!(cargo.contains("name = \"autogamesoftwaresync\"") && cargo.contains("name = \"AutoGameSoftwareSync\""), "package et binaire renommes");
    assert!(cargo.contains("name = \"agss\""), "le nom de la crate lib interne est conserve");
    assert_eq!(agss::gui::APP_TITLE, "AutoGameSoftwareSync");
    assert_eq!(agss::winutil::RUN_VALUE, "AutoGameSoftwareSync");
    assert_eq!(appdirs::APP_DIR, "AutoGameSoftwareSync");
    // Le nom de tache est dans le dossier AutoGameSoftwareSync du planificateur.
    assert_eq!(startup::TASK_FOLDER, "AutoGameSoftwareSync");
}

#[test]
fn migration_copie_verifiee_sans_toucher_a_l_ancien_dossier() {
    let t = TempDir::new().unwrap();
    let old = t.path().join(agss::legacy::DATA_DIRS[0]);
    let new = t.path().join("AutoGameSoftwareSync");
    write(&old.join("config.json"), r#"{"games":[]}"#);
    write(&old.join("settings.json"), r#"{"lang":"fr"}"#);
    write(&old.join("state.json"), "{}");
    write(&old.join("clock.json"), r#"{"offset_ms":5}"#);
    write(&old.join("catalog.yaml"), "x: 1");
    write(&old.join("safety/G/20260101/files/s.dat"), "precieux");
    write(&old.join("logs/watch.log"), "ligne");
    write(&old.join("watch.lock"), "123"); // verrou : jamais migre
    write(&old.join("tmp/x"), "tmp"); // dossier de travail : jamais migre
    let before: Vec<(String, String)> = walkdir::WalkDir::new(&old).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).map(|e| (e.path().display().to_string(), util::hash_file(e.path()).unwrap())).collect();
    let r = appdirs::migrate(&old, &new).unwrap();
    assert_eq!(r.skipped, None);
    assert_eq!(r.files, 7);
    assert_eq!(read(&new.join("settings.json")), r#"{"lang":"fr"}"#);
    assert_eq!(read(&new.join("safety/G/20260101/files/s.dat")), "precieux");
    assert!(new.join(appdirs::MIGRATION_MARKER).exists());
    assert!(!new.join("watch.lock").exists() && !new.join("tmp").exists());
    // L'ancien dossier est strictement intact.
    let after: Vec<(String, String)> = walkdir::WalkDir::new(&old).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).map(|e| (e.path().display().to_string(), util::hash_file(e.path()).unwrap())).collect();
    assert_eq!(before, after, "ancien dossier modifie");
    // Pas de reste de dossier de travail.
    assert!(!t.path().join("AutoGameSoftwareSync.migrating").exists());
    // Deuxieme appel : deja migre, rien ne change.
    write(&new.join("settings.json"), r#"{"lang":"en"}"#);
    let r2 = appdirs::migrate(&old, &new).unwrap();
    assert_eq!(r2.skipped.as_deref(), Some("deja migre"));
    assert_eq!(read(&new.join("settings.json")), r#"{"lang":"en"}"#, "rien n'est ecrase");
    assert!(appdirs::report_line(&old, &r).contains("7 fichier(s)") && appdirs::report_line(&old, &r2).contains("non effectuee"));
}

#[test]
fn migration_n_ecrase_jamais_un_nouveau_dossier_non_vide_et_gere_les_cas_vides() {
    let t = TempDir::new().unwrap();
    let old = t.path().join(agss::legacy::DATA_DIRS[0]);
    let new = t.path().join("AutoGameSoftwareSync");
    // Ancien dossier absent.
    assert!(appdirs::migrate(&old, &new).unwrap().skipped.is_some());
    write(&old.join("config.json"), "ancien");
    // Nouveau non vide : rien n'est ecrase.
    write(&new.join("settings.json"), "nouveau");
    let r = appdirs::migrate(&old, &new).unwrap();
    assert!(r.skipped.unwrap().contains("pas vide"));
    assert_eq!(read(&new.join("settings.json")), "nouveau");
    assert!(!new.join("config.json").exists());
    // Nouveau vide (existe mais sans rien) : migration possible.
    std::fs::remove_file(new.join("settings.json")).unwrap();
    assert_eq!(appdirs::migrate(&old, &new).unwrap().files, 1);
    assert_eq!(read(&new.join("config.json")), "ancien");
    // Ancien dossier sans rien de migrable.
    let (o2, n2) = (t.path().join("O2"), t.path().join("N2"));
    write(&o2.join("watch.lock"), "1");
    assert_eq!(appdirs::migrate(&o2, &n2).unwrap().skipped.as_deref(), Some("rien a migrer"));
    assert!(!n2.exists());
}

#[test]
fn dossier_de_donnees_variables_d_environnement() {
    let (new_v, old_v) = ("AUTOGAMESOFTWARESYNC_DATA_DIR", agss::legacy::ENV_VARS);
    for v in old_v.iter().copied().chain([new_v]) {
        std::env::remove_var(v);
    }
    assert_eq!(appdirs::data_dir_from_env(), None);
    // Une ancienne variable est encore comprise (par priorite decroissante)...
    std::env::set_var(old_v[2], "C:/ancien-3");
    assert_eq!(appdirs::data_dir_from_env(), Some(PathBuf::from("C:/ancien-3")));
    std::env::set_var(old_v[0], "C:/ancien-1");
    assert_eq!(appdirs::data_dir_from_env(), Some(PathBuf::from("C:/ancien-1")));
    // ... mais la nouvelle variable est TOUJOURS prioritaire.
    std::env::set_var(new_v, "C:/nouveau");
    assert_eq!(appdirs::data_dir_from_env(), Some(PathBuf::from("C:/nouveau")));
    for v in old_v.iter().copied().chain([new_v]) {
        std::env::remove_var(v);
    }
    assert!(appdirs::default_data_dir().unwrap().ends_with("AutoGameSoftwareSync"));
}

// ================================================================== valeurs par defaut / premier lancement

#[test]
fn valeurs_par_defaut_premiere_installation_et_reglages_existants_conserves() {
    let d = Settings::default();
    assert_eq!((d.lang, d.theme, d.accent), (Lang::En, ThemeMode::Dark, [2, 132, 99]));
    assert_eq!(DEFAULT_ACCENT, [0x02, 0x84, 0x63]);
    assert!(!d.tutorial_seen, "premiere installation : choix de la langue puis tutoriel");
    assert_eq!(d.conflict_mode, ConflictMode::Cloud, "nouveau defaut : cloud automatique");
    assert!(d.notify_conflict_resolved);
    // Pas de settings.json => defauts de premiere installation.
    let t = TempDir::new().unwrap();
    let s = Settings::load_from(t.path()).unwrap();
    assert!(!s.tutorial_seen && s.lang == Lang::En);
    // Un settings.json existant : reglages d'apparence conserves tels quels, tutoriel considere vu.
    write(&t.path().join("settings.json"), r#"{"lang":"fr","theme":"light","accent":[10,20,30]}"#);
    let s = Settings::load_from(t.path()).unwrap();
    assert_eq!((s.lang, s.theme, s.accent), (Lang::Fr, ThemeMode::Light, [10, 20, 30]));
    assert!(s.tutorial_seen, "un utilisateur existant ne revoit pas le tutoriel");
    assert_eq!(s.conflict_mode, ConflictMode::Cloud, "reglage absent => nouveau defaut");
    // Migration sans reglages d'apparence : defauts d'apparence.
    write(&t.path().join("settings.json"), r#"{"auto_watch":false}"#);
    let s = Settings::load_from(t.path()).unwrap();
    assert_eq!((s.lang, s.theme, s.accent), (Lang::En, ThemeMode::Dark, [2, 132, 99]));
    assert!(!s.auto_watch);
    // tutorial_seen explicite conserve.
    write(&t.path().join("settings.json"), r#"{"tutorial_seen":false}"#);
    assert!(!Settings::load_from(t.path()).unwrap().tutorial_seen);
}

#[test]
fn restaurer_l_apparence_par_defaut_ne_touche_que_theme_accent_langue() {
    let mut s = Settings::default();
    s.lang = Lang::Fr;
    s.theme = ThemeMode::Light;
    s.accent = [200, 10, 10];
    s.notify_global = false;
    s.big_mode = true;
    s.tutorial_seen = true;
    s.set_keep("G", 25);
    s.reset_appearance();
    assert_eq!((s.lang, s.theme, s.accent), (Lang::En, ThemeMode::Dark, [2, 132, 99]));
    assert!(!s.notify_global && s.big_mode && s.tutorial_seen, "le reste est conserve");
    assert_eq!(s.per_game["G"].keep_versions, Some(25));
    // Confirmation obligatoire (pas d'execution sans confirmation) : le dialogue existe en FR et EN.
    use agss::vm::Confirm;
    for l in [Lang::Fr, Lang::En] {
        assert!(Confirm::RestoreAppearance.message(l).len() > 30 && Confirm::ApplyStartup.message(l).contains("AutoGameSoftwareSync"));
    }
}

#[test]
fn drapeaux_dessines_au_painter_sans_emoji_et_icone_integree() {
    let src = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("gui.rs"));
    assert!(src.contains("fn paint_flag_en") && src.contains("fn paint_flag_fr"));
    // Aucun emoji de drapeau (indicateurs regionaux U+1F1E6..U+1F1FF) : Windows ne les affiche pas.
    assert!(!src.chars().any(|c| ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)));
    // Icone de fenetre et de zone de notification integrees a l'exe.
    assert!(src.contains("icon_64.rgba") && src.contains("icon_32.rgba"));
}

// ================================================================== icone

#[test]
fn icone_ico_multi_tailles_png_embarques_et_svg_vert() {
    let media = Path::new(env!("CARGO_MANIFEST_DIR")).join("media");
    let ico = std::fs::read(media.join("icon.ico")).unwrap();
    assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    let n = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    assert_eq!(n, 7);
    let mut sizes = vec![];
    for i in 0..n {
        let e = &ico[6 + 16 * i..6 + 16 * (i + 1)];
        let s = if e[0] == 0 { 256 } else { e[0] as u32 };
        sizes.push(s);
        let (len, off) = (u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize, u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize);
        assert_eq!(&ico[off..off + 8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A], "PNG embarque");
        // Dimensions lues dans l'en-tete IHDR du PNG.
        let w = u32::from_be_bytes([ico[off + 16], ico[off + 17], ico[off + 18], ico[off + 19]]);
        assert_eq!(w, s);
        assert!(off + len <= ico.len());
    }
    assert_eq!(sizes, vec![16, 24, 32, 48, 64, 128, 256]);
    assert_eq!(std::fs::metadata(media.join("icon_64.rgba")).unwrap().len(), 64 * 64 * 4);
    assert_eq!(std::fs::metadata(media.join("icon_32.rgba")).unwrap().len(), 32 * 32 * 4);
    let svg = read(&media.join("icon.svg")).to_lowercase();
    assert!(svg.contains("#028463"), "vert de l'application dominant");
    assert!(!svg.contains("#3b6fd8") && !svg.contains("#1e3d8f"), "plus de bleu d'origine");
    // 2.0.3 : icone redessinee a plat (aucune ombre portee, floue aux petites tailles) ; fleches de synchronisation en arc.
    assert!(svg.contains("a48 48 0 0 1"), "fleches de synchronisation en arc");
    assert!(!svg.contains("<filter") && !svg.contains("fedropshadow"), "aucune ombre portee");
    for f in ["icon_small.svg", "icon_tiny.svg"] {
        let s = read(&media.join(f)).to_lowercase();
        assert!(!s.contains("<filter"), "{} : aucune ombre portee", f);
        assert!(s.contains(" a") && s.contains("0 0 1 "), "{} : fleches en arc", f);
    }
    for f in ["icon_original.svg", "icon_original.ico", "icon_small.svg", "icon_tiny.svg"] {
        assert!(media.join(f).exists(), "{} manquant", f);
    }
    // Les anciens fichiers sont conserves tels quels (bleu).
    assert!(read(&media.join("icon_original.svg")).contains("#3b6fd8"));
}

// ================================================================== wording

#[test]
fn wording_dossier_partage_google_drive_seulement_dans_une_enumeration() {
    let mut n = 0;
    for k in ALL_KEYS {
        for l in [Lang::Fr, Lang::En] {
            let s = tr(l, *k);
            for (i, _) in s.match_indices("Google Drive") {
                // 2.0 (lot 8) : le conseil du mode « a la demande » ne concerne que Google Drive et doit le nommer.
                if matches!(k, K::VVaultStreamTitle | K::VVaultStreamBody) {
                    continue;
                }
                n += 1;
                assert!(s[..i].ends_with("NAS, ") && s[i..].starts_with("Google Drive, Dropbox"), "{:?} : « Google Drive » hors enumeration : {}", k, s);
            }
            assert!(!s.contains("Drive conflict") && !s.contains("conflit Drive"), "{:?}", k);
            assert!(!s.contains("AutoGameSoftwareSync") || s.contains("AutoGameSoftwareSync"), "{:?} : ancien nom", k);
            // « destination » cote utilisateur remplace par « dossier partage ».
            let low = s.to_lowercase();
            assert!(!low.contains("destination"), "{:?} parle encore de destination : {}", k, s);
        }
    }
    assert!(n >= 3);
    assert_eq!(tr(Lang::Fr, K::SetDest), "Dossier partagé (NAS, Google Drive, Dropbox, etc.)");
    assert_eq!(tr(Lang::En, K::SetDest), "Shared folder (NAS, Google Drive, Dropbox, etc.)");
    assert!(tr(Lang::En, K::StatusNoDest).contains("shared folder"));
}

#[test]
fn nouvelles_chaines_traduites_conflit_demarrage_tutoriel() {
    let must: Vec<(K, &str)> = vec![
        (K::SetConflict, "When there is a conflict"),
        (K::ConflictCloud, "Take the cloud version automatically"),
        (K::ConflictAsk, "Ask me"),
        (K::BtnRestoreAppearance, "Restore default appearance"),
        (K::VRedoSetup, "Run setup again"),
        (K::SetStartup, "Startup"),
        (K::VWizName1, "Welcome"),
        (K::VWizLanguage, "Language"),
    ];
    for (k, en) in must {
        assert_eq!(tr(Lang::En, k), en);
        assert_ne!(tr(Lang::Fr, k), en);
    }
    assert_eq!(tr(Lang::Fr, K::SetConflict), "Quand il y a un conflit");
    assert_eq!(tr(Lang::Fr, K::ConflictCloud), "Prendre la version du cloud automatiquement");
    // Premier lancement 2.0 (lot 10) : 6 etapes, noms tous distincts.
    let titles: HashSet<&str> = [K::VWizName1, K::VWizName2, K::VWizName3, K::VWizName4, K::VWizName5, K::VWizName6].iter().map(|k| tr(Lang::En, *k)).collect();
    assert_eq!(titles.len(), 6);
}

// ================================================================== conflit : cloud automatique

struct Proc(Arc<AtomicBool>);
impl ProcessChecker for Proc {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct Pc {
    ctx: Ctx,
    game: Game,
    running: Arc<AtomicBool>,
    log: Logger,
}

fn pc(root: &Path, name: &str) -> Pc {
    let running = Arc::new(AtomicBool::new(false));
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.into();
    ctx.procs = Box::new(Proc(running.clone()));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "G".into(), path: root.join(format!("saves-{}", name)), exe: Some("g.exe".into()), dest_key: None, ..Default::default() };
    let log = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
    Pc { ctx, game, running, log }
}

fn content(dest: &Path, g: &Game) -> Vec<String> {
    backup::list_versions(dest, g).iter().map(|v| read(&v.join("files/save.dat"))).collect()
}

/// Cree un vrai conflit : A et B partent de v1, chacun sauvegarde sa propre version.
fn conflict(t: &Path, dest: &Path) -> (Pc, Pc) {
    let a = pc(t, "EPIKOU");
    let b = pc(t, "EPIGOS");
    write(&a.game.path.join("save.dat"), "v1");
    backup::backup(&a.ctx, &a.game, dest).unwrap();
    backup::restore(&b.ctx, &b.game, dest).unwrap();
    write(&b.game.path.join("save.dat"), "v2-B");
    backup::backup(&b.ctx, &b.game, dest).unwrap();
    write(&a.game.path.join("save.dat"), "v2-A-local-non-synchronise");
    (a, b)
}

const CFG: WatchCfg = WatchCfg { stable_ms: 10_000, resync_ms: 60_000, max_tries: 3, safety_ms: 2000, retry_ms: 1000, retry_max_ms: 30_000 };

#[test]
fn conflit_cloud_automatique_jeu_ferme_version_locale_retrouvable_et_notification() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, b) = conflict(t.path(), &dest);
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Conflict);
    let events: RefCell<Vec<(String, EventKind, String)>> = RefCell::new(vec![]);
    let mut w = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), CFG, &a.log);
    w.auto_cloud.insert("G".into(), true);
    w.on_event = Some(Box::new(|e: WatchEvent| events.borrow_mut().push((e.game, e.kind, e.detail))));
    w.startup(0); // jeu ferme : resolution automatique
    // (3) la version du cloud (celle de B) est restauree localement
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-B");
    // (1) la version locale non synchronisee est conservee dans le dossier partage (historique) ...
    let all = content(&dest, &a.game);
    assert!(all.contains(&"v2-A-local-non-synchronise".to_string()), "version locale perdue : {:?}", all);
    assert!(all.contains(&"v1".to_string()) && all.contains(&"v2-B".to_string()));
    // (2) ... et dans le snapshot de securite local verifie.
    let safety: Vec<PathBuf> = walkdir::WalkDir::new(a.ctx.data_dir.join("safety")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_name() == "save.dat").map(|e| e.into_path()).collect();
    assert_eq!(safety.len(), 1);
    assert_eq!(read(&safety[0]), "v2-A-local-non-synchronise");
    // (4) sauvegarde de resolution : une seule tete, plus de conflit.
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::InSync);
    assert!(backup::history(&a.ctx, &a.game, &dest).conflict_heads.is_empty());
    // (5) evenement + journal
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].1, EventKind::ConflictResolved);
    let log = read(&a.ctx.data_dir.join("logs").join("watch.log"));
    assert!(log.contains("CONFLIT resolu automatiquement"), "{}", log);
    // L'autre PC voit simplement une version a jour (meme contenu que le cloud).
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::InSync);
    // Rien de ce qui existait dans la destination n'a ete supprime.
    assert!(all.len() >= 4, "{:?}", all);
}

#[test]
fn conflit_pendant_que_le_jeu_tourne_est_reporte_puis_resolu_a_la_fermeture() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, _b) = conflict(t.path(), &dest);
    let mut w = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), CFG, &a.log);
    w.auto_cloud.insert("G".into(), true);
    a.running.store(true, Ordering::SeqCst);
    w.startup(0);
    w.tick(1_000);
    w.tick(60_000);
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-A-local-non-synchronise", "jamais pendant que le jeu tourne");
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Conflict, "reporte");
    // Garde-fou direct : la resolution « cloud » refuse si le jeu tourne.
    let err = backup::resolve_conflict(&a.ctx, &a.game, &dest, false).unwrap_err().to_string();
    assert!(err.contains("REFUS"), "{}", err);
    // Le jeu se ferme : apres stabilite des fichiers, resolution.
    a.running.store(false, Ordering::SeqCst);
    w.tick(61_000);
    w.tick(75_000);
    w.tick(80_000);
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-B");
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::InSync);
}

#[test]
fn conflit_mode_demander_ou_reglage_absent_ne_restaure_rien() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, _b) = conflict(t.path(), &dest);
    let mut w = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), CFG, &a.log);
    w.auto_cloud.insert("G".into(), false); // « Me demander »
    w.startup(0);
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-A-local-non-synchronise", "rien restaure automatiquement");
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Conflict);
    assert_eq!(backup::history(&a.ctx, &a.game, &dest).conflict_heads.len(), 2, "conflit visible dans l'interface");
    // Aucune entree = comportement prudent (comme avant).
    let t2 = TempDir::new().unwrap();
    let dest2 = t2.path().join("cloud");
    let (a2, _b2) = conflict(t2.path(), &dest2);
    let mut w2 = Watcher::new(&a2.ctx, vec![a2.game.clone()], dest2.clone(), CFG, &a2.log);
    w2.startup(0);
    assert_eq!(read(&a2.game.path.join("save.dat")), "v2-A-local-non-synchronise");
}

#[test]
fn echec_en_cours_de_route_ne_supprime_rien_et_les_deux_versions_restent_retrouvables() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (mut a, _b) = conflict(t.path(), &dest);
    let before = content(&dest, &a.game);
    a.ctx.fault_corrupt_copy = true; // toute copie verifiee echoue
    let events: RefCell<Vec<EventKind>> = RefCell::new(vec![]);
    {
        let mut w = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), CFG, &a.log);
        w.auto_cloud.insert("G".into(), true);
        w.on_event = Some(Box::new(|e: WatchEvent| events.borrow_mut().push(e.kind)));
        w.startup(0);
    }
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-A-local-non-synchronise", "local intact");
    assert_eq!(content(&dest, &a.game), before, "destination inchangee, rien supprime");
    assert_eq!(*events.borrow(), vec![EventKind::Error]);
    assert!(read(&a.ctx.data_dir.join("logs").join("watch.log")).contains("rien n'a ete supprime"));
    assert!(!a.ctx.data_dir.join("safety").exists() || walkdir::WalkDir::new(a.ctx.data_dir.join("safety")).into_iter().filter_map(|e| e.ok()).all(|e| !e.file_type().is_file()), "pas de snapshot partiel");
    // La panne disparait : la resolution aboutit au cycle suivant.
    a.ctx.fault_corrupt_copy = false;
    let mut w2 = Watcher::new(&a.ctx, vec![a.game.clone()], dest.clone(), CFG, &a.log);
    w2.auto_cloud.insert("G".into(), true);
    w2.startup(0);
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-B");
    assert!(content(&dest, &a.game).contains(&"v2-A-local-non-synchronise".to_string()));
}

#[test]
fn modes_global_par_jeu_heritage_et_notification_dediee() {
    let mut s = Settings::default();
    assert_eq!(s.conflict_for("G"), ConflictMode::Cloud);
    s.conflict_mode = ConflictMode::Ask;
    assert_eq!(s.conflict_for("G"), ConflictMode::Ask);
    s.game_mut("G").conflict = Some(ConflictMode::Cloud);
    assert_eq!(s.conflict_for("G"), ConflictMode::Cloud, "le jeu remplace le global");
    assert_eq!(s.conflict_for("H"), ConflictMode::Ask, "herite");
    s.game_mut("G").conflict = None;
    assert_eq!(s.conflict_for("G"), ConflictMode::Ask);
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    // Notification « conflit resolu » : ON par defaut, independante des autres notifications.
    struct Rec(Mutex<Vec<String>>);
    impl Notifier for Rec {
        fn notify(&self, title: &str, _b: &str) {
            self.0.lock().unwrap().push(title.to_string());
        }
    }
    let t = TempDir::new().unwrap();
    let n = Rec(Mutex::new(vec![]));
    let ev = |k| WatchEvent { game: "G".into(), kind: k, detail: String::new() };
    let mut s = Settings::default();
    s.notify_global = false;
    write(&t.path().join("settings.json"), &serde_json::to_string(&s).unwrap());
    assert!(notify::deliver(&n, t.path(), Lang::En, &ev(EventKind::ConflictResolved)));
    assert!(!notify::deliver(&n, t.path(), Lang::En, &ev(EventKind::BackedUp)));
    s.notify_conflict_resolved = false;
    write(&t.path().join("settings.json"), &serde_json::to_string(&s).unwrap());
    assert!(!notify::deliver(&n, t.path(), Lang::En, &ev(EventKind::ConflictResolved)));
    assert_eq!(n.0.lock().unwrap()[0], "Changed on two devices: the version from your vault is back");
    assert_eq!(notify::notification_text(&ev(EventKind::ConflictResolved), Lang::Fr).0, "Modifié sur deux appareils : la version de ton coffre est remise");
}

// ================================================================== regles de demarrage (parties pures)

struct FakeProbe {
    run: Vec<(&'static str, &'static str)>,
    files: HashSet<PathBuf>,
    dirs: Vec<(PathBuf, Vec<&'static str>)>,
}
impl ToolProbe for FakeProbe {
    fn run_value(&self, name: &str) -> Option<String> {
        self.run.iter().find(|r| r.0 == name).map(|r| r.1.to_string())
    }
    fn file_exists(&self, p: &Path) -> bool {
        self.files.contains(p)
    }
    fn subdirs(&self, p: &Path) -> Vec<String> {
        self.dirs.iter().find(|d| d.0 == p).map(|d| d.1.iter().map(|s| s.to_string()).collect()).unwrap_or_default()
    }
    fn env(&self, key: &str) -> Option<String> {
        match key {
            "ProgramFiles" | "ProgramW6432" => Some("C:\\Program Files".into()),
            "ProgramFiles(x86)" => Some("C:\\Program Files (x86)".into()),
            "LOCALAPPDATA" => Some("C:\\Users\\u\\AppData\\Local".into()),
            _ => None,
        }
    }
}

fn pb(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[cfg(windows)]
#[test]
fn xml_de_tache_champs_critiques() {
    let r = Rule { id: "agss".into(), kind: ToolKind::App, name: "AutoGameSoftwareSync".into(), command: pb("C:\\Apps\\AutoGameSoftwareSync.exe"), args: "--tray".into(), delay_secs: 40, enabled: true, found: true, minimized: false };
    let x = task_xml(&r, "PC\\Utilisateur & Co");
    for must in [
        "<LogonTrigger>", "<Delay>PT40S</Delay>", "<UserId>PC\\Utilisateur &amp; Co</UserId>",
        "<LogonType>InteractiveToken</LogonType>", "<RunLevel>LeastPrivilege</RunLevel>",
        "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>", "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
        "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>", "<StartWhenAvailable>true</StartWhenAvailable>",
        "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>", "<Hidden>true</Hidden>",
        "<Command>C:\\Apps\\AutoGameSoftwareSync.exe</Command>", "<Arguments>--tray</Arguments>", "<WorkingDirectory>C:\\Apps</WorkingDirectory>",
        "<URI>\\AutoGameSoftwareSync\\agss</URI>",
    ] {
        assert!(x.contains(must), "champ manquant : {}\n{}", must, x);
    }
    assert!(!x.contains("HighestAvailable"), "jamais de droits admin permanents");
    // Echappement XML de la ligne de commande.
    let mut r2 = r.clone();
    r2.args = "--launch-tool \"a&b\" <x>".into();
    let x2 = task_xml(&r2, "u");
    assert!(x2.contains("&quot;a&amp;b&quot; &lt;x&gt;") && !x2.contains("<x>"));
    // Sans arguments : pas de balise vide.
    r2.args.clear();
    assert!(!task_xml(&r2, "u").contains("<Arguments>"));
    // UTF-16 LE avec BOM.
    let b = xml_to_utf16("<a/>");
    assert_eq!(&b[..2], &[0xFF, 0xFE]);
    assert_eq!(String::from_utf16(&b[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap(), "<a/>");
}

#[test]
fn noms_de_taches_toujours_dans_le_dossier_agss() {
    assert_eq!(task_name("gdrive"), "AutoGameSoftwareSync\\gdrive");
    assert_eq!(task_name("..\\..\\Microsoft\\evil"), "AutoGameSoftwareSync\\Microsoftevil", "pas de traversee de dossier");
    assert_eq!(task_name(""), "AutoGameSoftwareSync\\rule");
    assert!(is_own_task("AutoGameSoftwareSync\\gdrive") && is_own_task("\\AutoGameSoftwareSync\\custom-1"));
    for bad in ["Microsoft\\Windows\\x", "AutoGameSoftwareSync", "AutoGameSoftwareSync\\", "AutoGameSoftwareSync\\a\\b", "OtherApp\\AutoGameSoftwareSync\\x", "AutoGameSoftwareSync\\a b", "AutoGameSoftwareSync\\..\\x"] {
        assert!(!is_own_task(bad), "{}", bad);
    }
    assert_eq!(next_custom_id(&[]), "custom-1");
    let cfg = vec![StartupRuleCfg { id: "custom-1".into(), name: String::new(), exe: None, delay_secs: 0, enabled: false, minimized: true }];
    assert_eq!(next_custom_id(&cfg), "custom-2");
}

#[cfg(windows)]
#[test]
fn detection_des_outils_cloud_sur_fixtures_et_derniere_version_de_google_drive() {
    let gd = "C:\\Program Files\\Google\\Drive File Stream";
    let mut files: HashSet<PathBuf> = HashSet::new();
    for v in ["99.0.0.0", "132.0.0.0", "132.0.10.0", "9.9.9.9"] {
        files.insert(pb(&format!("{}\\{}\\GoogleDriveFS.exe", gd, v)));
    }
    let probe = FakeProbe { run: vec![("GoogleDriveFS", "\"C:\\Program Files\\Google\\Drive File Stream\\99.0.0.0\\GoogleDriveFS.exe\" --startup_mode")], files, dirs: vec![(pb(gd), vec!["99.0.0.0", "132.0.0.0", "132.0.10.0", "9.9.9.9", "Drivers"])] };
    // Comparaison NUMERIQUE : 132.0.10.0 > 132.0.0.0 > 99 > 9.9.9.9 (et pas lexicographique).
    assert_eq!(newest_gdrive_exe(&probe), Some(pb("C:\\Program Files\\Google\\Drive File Stream\\132.0.10.0\\GoogleDriveFS.exe")));
    assert_eq!(version_key("132.0.10.0"), vec![132, 0, 10, 0]);
    assert!(version_key("132.0.10.0") > version_key("132.0.9.9") && version_key("99.0.0.0") < version_key("132.0.0.0"));
    let tools = detect_tools(&probe);
    assert_eq!(tools.len(), 1);
    assert_eq!((tools[0].id.as_str(), tools[0].args.as_str()), ("gdrive", "--startup_mode"), "arguments repris de la valeur Run existante");
    // Aucune version sur le disque : on garde le chemin de la valeur Run s'il existe.
    let mut f2 = HashSet::new();
    f2.insert(pb("D:\\Drive\\GoogleDriveFS.exe"));
    let p2 = FakeProbe { run: vec![("GoogleDriveFS", "D:\\Drive\\GoogleDriveFS.exe /x")], files: f2, dirs: vec![] };
    let t2 = detect_tools(&p2);
    assert_eq!(t2[0].exe, pb("D:\\Drive\\GoogleDriveFS.exe"));
    assert_eq!(t2[0].args, "/x");
    // OneDrive et Dropbox : valeur Run, sinon emplacements standard ; introuvables => absents.
    let mut f3 = HashSet::new();
    f3.insert(pb("C:\\Users\\u\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe"));
    f3.insert(pb("C:\\Program Files (x86)\\Dropbox\\Client\\Dropbox.exe"));
    let p3 = FakeProbe { run: vec![], files: f3, dirs: vec![] };
    let t3 = detect_tools(&p3);
    let ids: Vec<&str> = t3.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["onedrive", "dropbox"]);
    assert_eq!(t3[0].args, "/background");
    assert!(detect_tools(&FakeProbe { run: vec![], files: HashSet::new(), dirs: vec![] }).is_empty());
    // Analyse d'une ligne de commande.
    assert_eq!(parse_command_line("\"C:\\a b\\x.exe\" --y z"), (pb("C:\\a b\\x.exe"), "--y z".into()));
    assert_eq!(parse_command_line("C:\\a\\x.exe /background"), (pb("C:\\a\\x.exe"), "/background".into()));
    assert_eq!(parse_command_line("C:\\a\\x.exe"), (pb("C:\\a\\x.exe"), String::new()));
    // --launch-tool : re-resolution a chaque appel.
    assert_eq!(resolve_tool("gdrive", &probe).unwrap().exe.file_name().unwrap(), "GoogleDriveFS.exe");
    assert!(resolve_tool("dropbox", &probe).is_none());
}

#[test]
fn regles_delais_par_defaut_ordre_cloud_puis_application() {
    let self_exe = pb("C:\\Apps\\AutoGameSoftwareSync.exe");
    let tool = Tool { id: "gdrive".into(), kind: ToolKind::GoogleDrive, name: "Google Drive".into(), exe: pb("C:\\gd.exe"), args: String::new() };
    let rules = build_rules(&[tool], &[], &self_exe, &|_| true);
    assert_eq!(rules.len(), 2);
    assert_eq!((rules[0].id.as_str(), rules[0].delay_secs, rules[0].args.as_str()), ("agss", 40, "--tray"));
    assert_eq!((rules[1].id.as_str(), rules[1].delay_secs, rules[1].args.as_str()), ("gdrive", 20, "--launch-tool gdrive"));
    assert!(rules[1].delay_secs < rules[0].delay_secs, "le cloud demarre AVANT AutoGameSoftwareSync");
    assert!(rules.iter().all(|r| !r.enabled), "rien n'est active par defaut");
    assert_eq!(rules[1].command, self_exe, "le chemin de l'outil est re-resolu au demarrage (via --launch-tool)");
    // Reglages enregistres + outil personnalise + programme introuvable.
    let cfg = vec![
        StartupRuleCfg { id: "agss".into(), name: String::new(), exe: None, delay_secs: 55, enabled: true, minimized: true },
        StartupRuleCfg { id: "custom-1".into(), name: "Mon outil".into(), exe: Some(pb("D:\\x\\outil.exe")), delay_secs: 7, enabled: true, minimized: true },
    ];
    let r2 = build_rules(&[], &cfg, &self_exe, &|p| p != Path::new("D:\\x\\outil.exe"));
    assert_eq!(r2.len(), 2);
    assert!(r2[0].enabled && r2[0].delay_secs == 55);
    assert_eq!((r2[1].name.as_str(), r2[1].kind, r2[1].found, r2[1].delay_secs), ("Mon outil", ToolKind::Custom, false, 7));
    assert_eq!(rule_status(&r2[1], &[]), RuleStatus::NotFound);
    assert_eq!(rule_status(&r2[0], &[]), RuleStatus::Inactive);
    assert_eq!(rule_status(&r2[0], &["AutoGameSoftwareSync\\agss".to_string()]), RuleStatus::Active);
}

fn rule(id: &str, enabled: bool, found: bool) -> Rule {
    Rule { id: id.into(), kind: ToolKind::Custom, name: id.into(), command: pb("C:\\x.exe"), args: String::new(), delay_secs: 5, enabled, found, minimized: false }
}

#[test]
fn plan_d_application_diff_actuel_souhaite() {
    let existing = vec!["AutoGameSoftwareSync\\a".to_string(), "AutoGameSoftwareSync\\b".to_string(), "AutoGameSoftwareSync\\inconnue".to_string()];
    let all = vec![rule("a", true, true), rule("b", false, true), rule("c", true, true), rule("d", false, true), rule("e", true, false)];
    let p = plan_apply(&existing, &all);
    let created: Vec<&str> = p.create.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(created, vec!["a", "c"], "regles activees et trouvees : creees / mises a jour");
    assert_eq!(p.delete, vec!["AutoGameSoftwareSync\\b".to_string()], "regle desactivee : SA tache supprimee ; d n'existe pas ; e introuvable ; 'inconnue' jamais touchee");
    // Rien a faire.
    assert_eq!(plan_apply(&[], &[rule("x", false, true)]), ApplyPlan::default());
}

struct FakeSched {
    tasks: RefCell<Vec<String>>,
    deny: RefCell<bool>,
    ops: RefCell<Vec<String>>,
}
impl Scheduler for FakeSched {
    fn list(&self) -> Result<Vec<String>, SchedError> {
        Ok(self.tasks.borrow().clone())
    }
    fn create(&self, name: &str, xml: &[u8]) -> Result<(), SchedError> {
        assert_eq!(&xml[..2], &[0xFF, 0xFE]);
        if *self.deny.borrow() {
            return Err(SchedError::AccessDenied("Access is denied".into()));
        }
        self.ops.borrow_mut().push(format!("create {}", name));
        self.tasks.borrow_mut().push(name.to_string());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), SchedError> {
        if *self.deny.borrow() {
            return Err(SchedError::AccessDenied("Access is denied".into()));
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

#[test]
fn application_avec_planificateur_simule_aucune_vraie_tache() {
    let s = FakeSched { tasks: RefCell::new(vec!["AutoGameSoftwareSync\\b".into()]), deny: RefCell::new(false), ops: RefCell::new(vec![]) };
    let rules = vec![rule("a", true, true), rule("b", false, true)];
    let never = |_: &ApplyFile| -> Result<(), String> { panic!("pas d'elevation necessaire") };
    let msg = agss::actions::apply_startup_with(&s, &rules, "PC\\u", &never).unwrap();
    assert!(msg.contains("2 regle(s)"));
    assert_eq!(*s.ops.borrow(), vec!["create AutoGameSoftwareSync\\a".to_string(), "delete AutoGameSoftwareSync\\b".to_string()]);
    assert_eq!(*s.tasks.borrow(), vec!["AutoGameSoftwareSync\\a".to_string()]);
    // Idempotent : une seconde application recree seulement la tache activee.
    s.ops.borrow_mut().clear();
    agss::actions::apply_startup_with(&s, &rules, "PC\\u", &never).unwrap();
    assert_eq!(*s.ops.borrow(), vec!["create AutoGameSoftwareSync\\a".to_string()]);
    // Un nom hors du dossier AutoGameSoftwareSync est refuse (fichier recu cote eleve).
    let f = ApplyFile { creates: vec![("Microsoft\\Windows\\evil".into(), "<x/>".into())], deletes: vec!["\\Microsoft\\y".into(), "AutoGameSoftwareSync\\ok".into()] };
    let out = apply_file(&s, &f);
    assert_eq!(out.failed.len(), 2, "creation et suppression hors dossier refusees");
    assert_eq!(out.deleted, vec!["AutoGameSoftwareSync\\ok".to_string()]);
    assert!(!s.ops.borrow().iter().any(|o| o.contains("Microsoft")));
}

#[test]
fn elevation_une_seule_fenetre_uac_echec_et_annulation_sans_rien_modifier() {
    let rules = vec![rule("a", true, true), rule("b", true, true), rule("c", false, true)];
    let s = FakeSched { tasks: RefCell::new(vec!["AutoGameSoftwareSync\\c".into()]), deny: RefCell::new(true), ops: RefCell::new(vec![]) };
    let calls = RefCell::new(0);
    let seen: RefCell<Option<ApplyFile>> = RefCell::new(None);
    // L'utilisateur accepte l'UAC : UN SEUL appel regroupant toutes les operations refusees.
    let ok = |f: &ApplyFile| -> Result<(), String> {
        *calls.borrow_mut() += 1;
        *seen.borrow_mut() = Some(ApplyFile { creates: f.creates.clone(), deletes: f.deletes.clone() });
        Ok(())
    };
    let msg = agss::actions::apply_startup_with(&s, &rules, "PC\\u", &ok).unwrap();
    assert_eq!(*calls.borrow(), 1, "une seule fenetre UAC");
    let f = seen.borrow_mut().take().unwrap();
    assert_eq!(f.creates.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["AutoGameSoftwareSync\\a", "AutoGameSoftwareSync\\b"]);
    assert_eq!(f.deletes, vec!["AutoGameSoftwareSync\\c".to_string()]);
    assert!(f.creates[0].1.contains("<LogonTrigger>") && f.creates[0].1.contains("InteractiveToken"));
    assert!(msg.contains("3 regle(s)"));
    // Fichier d'echange : aller-retour JSON.
    let json = serde_json::to_string(&f).unwrap();
    assert_eq!(serde_json::from_str::<ApplyFile>(&json).unwrap(), f);
    // UAC refusee : erreur claire, aucune operation partielle cote application.
    let cancel = |_: &ApplyFile| -> Result<(), String> { Err("operation annulee dans la fenetre d'autorisation Windows".into()) };
    let e = agss::actions::apply_startup_with(&s, &rules, "PC\\u", &cancel).unwrap_err().to_string();
    assert!(e.contains("annulee") && e.contains("aucune regle modifiee"), "{}", e);
    assert!(s.ops.borrow().is_empty() && *s.tasks.borrow() == vec!["AutoGameSoftwareSync\\c".to_string()], "rien modifie");
    // Autre echec que les droits : pas d'elevation, erreur remontee.
    struct Broken;
    impl Scheduler for Broken {
        fn list(&self) -> Result<Vec<String>, SchedError> {
            Ok(vec![])
        }
        fn create(&self, _n: &str, _x: &[u8]) -> Result<(), SchedError> {
            Err(SchedError::Other("XML invalide".into()))
        }
        fn delete(&self, _n: &str) -> Result<(), SchedError> {
            Ok(())
        }
        fn run(&self, _n: &str) -> Result<(), SchedError> {
            Ok(())
        }
        fn query_xml(&self, _n: &str) -> Result<String, SchedError> {
            Ok(String::new())
        }
    }
    let never = |_: &ApplyFile| -> Result<(), String> { panic!("pas d'elevation") };
    assert!(agss::actions::apply_startup_with(&Broken, &rules, "u", &never).unwrap_err().to_string().contains("XML invalide"));
}

#[test]
fn attente_du_dossier_partage_sans_boucle_serree_ni_blocage() {
    use std::cell::Cell;
    // Accessible apres 4 essais : 3 pauses de 3 s (temps simule, aucune vraie attente).
    let tries = Cell::new(0);
    let slept = Cell::new(0u64);
    let ok = wait_until(&|| { tries.set(tries.get() + 1); tries.get() >= 4 }, &|| false, &|s| slept.set(slept.get() + s), 300, 3);
    assert!(ok && slept.get() == 9 && tries.get() == 4);
    // Jamais accessible : abandon apres le delai, et pauses entre chaque essai (pas de boucle serree).
    let slept = Cell::new(0u64);
    let n = Cell::new(0);
    assert!(!wait_until(&|| { n.set(n.get() + 1); false }, &|| false, &|s| slept.set(slept.get() + s), 300, 3));
    assert_eq!(slept.get(), 300);
    assert!(n.get() <= 102, "{} essais pour 300 s", n.get());
    // Arret demande : sortie immediate.
    let n = Cell::new(0);
    assert!(!wait_until(&|| { n.set(n.get() + 1); false }, &|| true, &|_| panic!("ne dort pas"), 300, 3));
    assert_eq!(n.get(), 1);
    // Deja accessible : aucune pause.
    assert!(wait_until(&|| true, &|| false, &|_| panic!("ne dort pas"), 300, 3));
}

#[test]
fn le_code_n_utilise_plus_de_cle_run_pour_le_demarrage_et_le_planificateur_est_masque() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let gui = read(&src.join("gui.rs"));
    assert!(!gui.contains("set_autostart") && !gui.contains("autostart_enabled"), "l'ancienne case « Run » est remplacee par les regles");
    let st = read(&src.join("startup.rs"));
    assert!(st.contains("creation_flags(0x0800_0000)"), "schtasks sans fenetre");
    assert!(st.contains("LogonTrigger") && st.contains("LeastPrivilege") && !st.contains("HighestAvailable"));
}
