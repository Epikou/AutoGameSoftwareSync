//! 1.0.2 : emulateurs (cible fichier / dossier filtre, plusieurs jeux par exe, recouvrements), nom affiche,
//! status.json schema 2, pictogrammes (aucun glyphe de police), couleurs des machines.
use agss::actions;
use agss::backup::{self, Plan};
use agss::config::{Config, Game};
use agss::process::ProcessChecker;
use agss::store::{glob_match, parse_filter, Target};
use agss::watch::{Logger, WatchCfg, Watcher};
use agss::{Ctx, Res};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

struct Flag(Arc<AtomicBool>);
impl ProcessChecker for Flag {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

fn ctx_of(root: &Path, name: &str, running: Arc<AtomicBool>) -> Ctx {
    let mut c = Ctx::new(root.join(format!("data-{}", name)));
    c.machine = name.to_string();
    c.procs = Box::new(Flag(running));
    std::fs::create_dir_all(&c.data_dir).unwrap();
    c
}

fn w(p: &Path, c: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}

fn names_in_cloud(ctx: &Ctx, g: &Game, dest: &Path) -> Vec<String> {
    let _ = ctx;
    let mut f: Vec<String> = backup::active_meta(dest, g).unwrap().files.iter().map(|x| x.path.clone()).collect();
    f.sort();
    f
}

#[test]
fn motifs_de_filtre() {
    assert!(glob_match("*.srm", "jeu.srm") && glob_match("jeu.srm", "jeu.srm") && glob_match("sav?.dat", "sav1.dat"));
    assert!(!glob_match("*.srm", "jeu.sav") && !glob_match("jeu.srm", "jeu2.srm"));
    assert_eq!(parse_filter(" *.SRM ; Jeu.srm;; "), vec!["*.srm".to_string(), "jeu.srm".to_string()]);
    let t = Target { root: PathBuf::from("x"), only_file: None, filter: parse_filter("*.srm;saves/*.dat"), exclude: vec![] };
    assert!(t.accepts("A.SRM") && t.accepts("sub/b.srm") && t.accepts("saves/x.dat") && !t.accepts("other/x.dat") && !t.accepts("a.txt"));
}

#[test]
fn fichier_unique_sauvegarde_et_restauration_limitees_au_fichier() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let run = Arc::new(AtomicBool::new(false));
    let a = ctx_of(t.path(), "A", run.clone());
    let card = t.path().join("emuA").join("memcard1.mcd");
    w(&card, "CARTE-1");
    w(&card.with_file_name("memcard2.mcd"), "AUTRE-CARTE");
    let g = Game { name: "Jeu emule".into(), path: card.clone(), exe: Some("emu.exe".into()), save_is_file: true, ..Default::default() };
    backup::backup(&a, &g, &dest).unwrap();
    assert_eq!(names_in_cloud(&a, &g, &dest), vec!["memcard1.mcd".to_string()], "seul le fichier est dans la sauvegarde");
    // Un autre fichier du meme dossier ne change rien.
    w(&card.with_file_name("memcard2.mcd"), "MODIFIEE");
    assert_eq!(backup::plan(&a, &g, &dest).unwrap(), Plan::InSync);
    // Autre PC : le fichier est restaure, le reste du dossier n'est pas touche.
    let b = ctx_of(t.path(), "B", run);
    let card_b = t.path().join("emuB").join("memcard1.mcd");
    w(&card_b.with_file_name("notes.txt"), "a moi");
    let gb = Game { name: "Jeu emule".into(), path: card_b.clone(), exe: Some("emu.exe".into()), save_is_file: true, ..Default::default() };
    backup::restore(&b, &gb, &dest).unwrap();
    assert_eq!(std::fs::read_to_string(&card_b).unwrap(), "CARTE-1");
    assert_eq!(std::fs::read_to_string(card_b.with_file_name("notes.txt")).unwrap(), "a moi");
    assert!(!card_b.with_file_name("memcard2.mcd").exists());
    assert_eq!(backup::plan(&b, &gb, &dest).unwrap(), Plan::InSync);
    // Modification du fichier sur B : nouvelle version, A la recupere.
    w(&card_b, "CARTE-2");
    backup::backup(&b, &gb, &dest).unwrap();
    assert_eq!(backup::plan(&a, &g, &dest).unwrap(), Plan::Restore);
    backup::restore(&a, &g, &dest).unwrap();
    assert_eq!(std::fs::read_to_string(&card).unwrap(), "CARTE-2");
    assert_eq!(std::fs::read_to_string(card.with_file_name("memcard2.mcd")).unwrap(), "MODIFIEE", "autre fichier intact");
}

#[test]
fn dossier_avec_filtre_ne_touche_que_les_fichiers_filtres() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let run = Arc::new(AtomicBool::new(false));
    let (a, b) = (ctx_of(t.path(), "A", run.clone()), ctx_of(t.path(), "B", run));
    let da = t.path().join("sa");
    w(&da.join("jeu1.srm"), "S1");
    w(&da.join("sous").join("jeu2.srm"), "S2");
    w(&da.join("config.ini"), "CFG-A");
    let g = Game { name: "Rom".into(), path: da.clone(), exe: Some("emu.exe".into()), save_filter: Some("*.srm".into()), ..Default::default() };
    backup::backup(&a, &g, &dest).unwrap();
    assert_eq!(names_in_cloud(&a, &g, &dest), vec!["jeu1.srm".to_string(), "sous/jeu2.srm".to_string()]);
    w(&da.join("config.ini"), "CFG-A2");
    assert_eq!(backup::plan(&a, &g, &dest).unwrap(), Plan::InSync, "un fichier hors filtre ne declenche rien");
    let db = t.path().join("sb");
    w(&db.join("config.ini"), "CFG-B");
    w(&db.join("jeu1.srm"), "ANCIEN");
    let gb = Game { name: "Rom".into(), path: db.clone(), exe: Some("emu.exe".into()), save_filter: Some("*.srm".into()), ..Default::default() };
    backup::restore(&b, &gb, &dest).unwrap();
    assert_eq!(std::fs::read_to_string(db.join("jeu1.srm")).unwrap(), "S1");
    assert_eq!(std::fs::read_to_string(db.join("sous").join("jeu2.srm")).unwrap(), "S2");
    assert_eq!(std::fs::read_to_string(db.join("config.ini")).unwrap(), "CFG-B", "fichier hors filtre jamais touche");
    assert_eq!(backup::plan(&b, &gb, &dest).unwrap(), Plan::InSync);
}

#[test]
fn deux_jeux_meme_exe_traites_a_la_fermeture_jamais_restaures_pendant_l_execution() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    std::fs::create_dir_all(&dest).unwrap(); // 2.0 : la surveillance ne cree jamais le coffre lui-meme
    let run = Arc::new(AtomicBool::new(false));
    let a = ctx_of(t.path(), "A", run.clone());
    let mk = |n: &str| {
        let d = t.path().join(n);
        w(&d.join("s.dat"), "v1");
        Game { name: n.into(), path: d, exe: Some("emu.exe".into()), ..Default::default() }
    };
    let (g1, g2) = (mk("Jeu1"), mk("Jeu2"));
    let log = Logger::new(a.data_dir.join("logs").join("watch.log"), false);
    let mut wt = Watcher::new(&a, vec![g1.clone(), g2.clone()], dest.clone(), WatchCfg::default(), &log);
    wt.startup(0);
    assert_eq!(backup::plan(&a, &g1, &dest).unwrap(), Plan::InSync);
    assert_eq!(backup::plan(&a, &g2, &dest).unwrap(), Plan::InSync);
    // L'emulateur tourne : pas de restauration possible pour AUCUN des jeux lies, et rien n'est sauvegarde en cours de route.
    run.store(true, Ordering::SeqCst);
    wt.tick(10_000);
    w(&g1.path.join("s.dat"), "v2");
    w(&g2.path.join("s.dat"), "v2");
    assert!(backup::restore(&a, &g1, &dest).is_err() && backup::restore(&a, &g2, &dest).is_err(), "restauration refusee pendant que l'exe tourne");
    assert_eq!(std::fs::read_to_string(g1.path.join("s.dat")).unwrap(), "v2");
    // Fermeture : TOUS les jeux lies a l'exe sont compares et sauvegardes.
    run.store(false, Ordering::SeqCst);
    for now in [20_000u64, 25_000, 30_000, 40_000] {
        wt.tick(now);
    }
    for g in [&g1, &g2] {
        assert_eq!(backup::plan(&a, g, &dest).unwrap(), Plan::InSync, "{}", g.name);
        let v = backup::list_versions(&dest, g);
        assert_eq!(v.len(), 2, "{} : version initiale + sauvegarde a la fermeture", g.name);
    }
    // Rien de modifie a la fermeture suivante => aucune nouvelle version (comparaison, pas de sauvegarde systematique).
    run.store(true, Ordering::SeqCst);
    wt.tick(50_000);
    run.store(false, Ordering::SeqCst);
    for now in [60_000u64, 65_000, 70_000, 80_000] {
        wt.tick(now);
    }
    assert_eq!(backup::list_versions(&dest, &g1).len(), 2);
}

#[test]
fn recouvrements_detectes() {
    let d = |p: &str| Game { name: p.into(), path: PathBuf::from(p), exe: Some("e.exe".into()), ..Default::default() };
    let f = |p: &str| Game { save_is_file: true, ..d(p) };
    let filt = |p: &str, fl: &str| Game { save_filter: Some(fl.into()), ..d(p) };
    assert!(actions::targets_overlap(&d("C:/Saves/Jeu"), &d("c:\\saves\\jeu")), "meme dossier");
    assert!(actions::targets_overlap(&d("C:/Saves"), &d("C:/Saves/Jeu")), "dossier imbrique");
    assert!(!actions::targets_overlap(&d("C:/Saves/A"), &d("C:/Saves/B")));
    assert!(actions::targets_overlap(&f("C:/Emu/memcard1.mcd"), &f("C:/emu/MEMCARD1.mcd")), "meme fichier");
    assert!(!actions::targets_overlap(&f("C:/Emu/memcard1.mcd"), &f("C:/Emu/memcard2.mcd")));
    assert!(actions::targets_overlap(&f("C:/Emu/memcard1.mcd"), &d("C:/Emu")), "fichier dans un dossier entier");
    assert!(actions::targets_overlap(&f("C:/Emu/memcard1.mcd"), &filt("C:/Emu", "*.mcd")));
    assert!(!actions::targets_overlap(&f("C:/Emu/memcard1.mcd"), &filt("C:/Emu", "*.srm")), "filtre qui n'inclut pas le fichier");
    assert!(!actions::targets_overlap(&filt("C:/Emu", "*.srm"), &filt("C:/Emu", "*.mcd")), "filtres disjoints");
    assert!(actions::targets_overlap(&filt("C:/Emu", "*.srm"), &filt("C:/Emu", "jeu.srm")), "motif inclus");
    let cfg = Config { games: vec![d("C:/Saves/Jeu")], dest: None };
    assert_eq!(actions::overlapping_games(&cfg, &d("C:/Saves/Jeu/sous")), vec!["C:/Saves/Jeu".to_string()]);
    assert!(actions::overlapping_games(&cfg, &d("C:/Saves/Autre")).is_empty());
}

#[test]
fn anciens_config_json_et_nom_affiche() {
    let old = r#"{"games":[{"name":"Vieux","path":"C:\\S","exe":"v.exe"}],"dest":null}"#;
    let c: Config = serde_json::from_str(old).unwrap();
    let g = &c.games[0];
    assert!(!g.save_is_file && g.save_filter.is_none() && g.display_name.is_none());
    assert_eq!(g.shown(), "Vieux");
    let out = serde_json::to_string(&c).unwrap();
    assert!(!out.contains("save_is_file") && !out.contains("display_name") && !out.contains("save_filter"), "champs absents tant qu'inutilises : {}", out);
    // Nom affiche : cle du dossier partage, exe et chemins inchanges.
    let t = TempDir::new().unwrap();
    let ctx = Ctx::new(t.path().to_path_buf());
    let dir = t.path().join("s");
    std::fs::create_dir_all(&dir).unwrap();
    Config { games: vec![Game { name: "Vieux".into(), path: dir.clone(), exe: Some("v.exe".into()), ..Default::default() }], dest: None }.save(&ctx).unwrap();
    let key0 = agss::cloud::dest_key(&Config::load(&ctx).unwrap().games[0]);
    actions::set_display_name(&ctx, "Vieux", Some("  Mon super jeu ")).unwrap();
    let g = Config::load(&ctx).unwrap().games[0].clone();
    assert_eq!((g.name.as_str(), g.shown(), g.path.clone(), g.exe.clone()), ("Vieux", "Mon super jeu", dir, Some("v.exe".to_string())));
    assert_eq!(agss::cloud::dest_key(&g), key0, "la cle du dossier partage ne change JAMAIS");
    actions::set_display_name(&ctx, "Vieux", None).unwrap();
    assert_eq!(Config::load(&ctx).unwrap().games[0].shown(), "Vieux");
}

#[test]
fn status_json_schema_2_additif() {
    use agss::statusfile::*;
    use agss::vm::GameStatus;
    let games = vec![GameIn { name: "Vieux".into(), key: "vieux".into(), exe: Some("v.exe".into()), install_dir: None, save_path: PathBuf::from("C:\\S"), status: GameStatus::UpToDate, last_sync_ms: Some(1_000_000) }];
    let mut doc = build(&Inputs { app_version: "1.0.2", now_ms: 2_000_000, watching: true, paused: false, reachable: true, shared: None, user: "u", games });
    let x = GameExtra { display_name: "Mon jeu".into(), last_backup_ms: Some(1_500_000), last_action: Some(("backup".into(), 1_500_000)), remote_machine: Some("ALLY".into()), remote_ms: Some(1_400_000), local_files: 3, local_size: 99, versions: 4, machine_color: Some("#0072B2".into()), next_check_ms: Some(2_002_000), ..Default::default() };
    enrich(&mut doc, &[x], &GlobalExtra { machine: "PC-DESKTOP".into(), machine_color: "#D55E00".into(), reactivity_mode: "instant".into(), conflict_mode: "cloud".into(), ..Default::default() });
    let v: serde_json::Value = serde_json::from_str(&to_json(&doc)).unwrap();
    assert_eq!(v["schema"], 2);
    let g = &v["games"][0];
    // Champs du schema 1 conserves (name = nom affiche, key inchangee).
    assert_eq!((g["name"].as_str(), g["key"].as_str(), g["exe"].as_str(), g["state"].as_str()), (Some("Mon jeu"), Some("vieux"), Some("v.exe"), Some("synced")));
    assert_eq!((g["original_name"].as_str(), g["display_name"].as_str(), g["last_action"].as_str()), (Some("Vieux"), Some("Mon jeu"), Some("backup")));
    assert_eq!((g["local_files"].as_u64(), g["versions_count"].as_u64(), g["remote_head_machine"].as_str(), g["machine_color"].as_str()), (Some(3), Some(4), Some("ALLY"), Some("#0072B2")));
    assert!(g["last_backup_utc"].is_string() && g["last_restore_utc"].is_null() && g["next_check_utc"].is_string());
    assert_eq!((v["machine"].as_str(), v["reactivity_mode"].as_str(), v["conflict_mode"].as_str()), (Some("PC-DESKTOP"), Some("instant"), Some("cloud")));
    // Un lecteur du schema 1 (champs inconnus ignores) lit toujours le document.
    #[derive(serde::Deserialize)]
    struct Old {
        schema: u32,
        games: Vec<OldG>,
    }
    #[derive(serde::Deserialize)]
    struct OldG {
        name: String,
        state: String,
    }
    let o: Old = serde_json::from_str(&to_json(&doc)).unwrap();
    assert_eq!((o.schema, o.games[0].name.as_str(), o.games[0].state.as_str()), (2, "Mon jeu", "synced"));
}

#[test]
fn aucun_glyphe_de_police_dans_les_textes_de_l_interface() {
    // Coches, croix, fleches, triangles, formes, emojis : dessines au painter, jamais en texte.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "rs").unwrap_or(false)).collect();
    files.extend(walkdir::WalkDir::new(src.join("gui")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).map(|e| e.path().to_path_buf()));
    let bad = |c: char| matches!(c as u32, 0x2190..=0x21FF | 0x2300..=0x23FF | 0x25A0..=0x27BF | 0x2900..=0x297F | 0x2B00..=0x2BFF | 0x1F000..=0x1FFFF);
    let mut found = Vec::new();
    for f in files {
        for (i, l) in std::fs::read_to_string(&f).unwrap().lines().enumerate() {
            if l.trim_start().starts_with("//") {
                continue;
            }
            if let Some(c) = l.chars().find(|c| bad(*c)) {
                found.push(format!("{}:{} U+{:04X}", f.file_name().unwrap().to_string_lossy(), i + 1, c as u32));
            }
        }
    }
    assert!(found.is_empty(), "glyphes interdits : {:?}", found);
}

#[test]
fn couleurs_de_machines_persistees() {
    use agss::settings::Settings;
    let t = TempDir::new().unwrap();
    let mut s = Settings::default();
    s.machine_colors.insert("PC-DESKTOP".into(), [10, 20, 30]);
    std::fs::write(t.path().join("settings.json"), serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(Settings::load_from(t.path()).unwrap().machine_colors["PC-DESKTOP"], [10, 20, 30]);
    std::fs::write(t.path().join("settings.json"), r#"{"lang":"fr"}"#).unwrap();
    assert!(Settings::load_from(t.path()).unwrap().machine_colors.is_empty());
}
