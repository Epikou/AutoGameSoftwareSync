//! 1.0.3 : emplacements multiples par jeu (union, sous-espace `files/@<id>/`, emplacement inconnu, conservation, convergence).
use agss::actions;
use agss::backup::{self, Plan};
use agss::config::{Config, Game, Location, State};
use agss::process::ProcessChecker;
use agss::{Ctx, Res};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

fn ctx_of(root: &Path, name: &str) -> Ctx {
    let mut c = Ctx::new(root.join(format!("data-{}", name)));
    c.machine = name.to_string();
    c.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&c.data_dir).unwrap();
    c
}

fn w(p: &Path, c: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}

fn rd(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|_| "<absent>".into())
}

fn loc(id: &str, label: &str, path: PathBuf, is_file: bool, filter: Option<&str>) -> Location {
    Location { id: id.into(), label: label.into(), path, is_file, filter: filter.map(|s| s.to_string()), exclude: None, enabled: true }
}

/// Jeu a trois emplacements : dossier principal, dossier de reglages filtre (*.ini), fichier unique.
fn game_on(root: &Path, pc: &str, with_extras: bool) -> Game {
    let r = root.join(pc);
    let mut g = Game { name: "Jeu".into(), path: r.join("saves"), exe: Some("jeu.exe".into()), ..Default::default() };
    if with_extras {
        g.extra_locations = vec![loc("aa11", "Reglages", r.join("cfg"), false, Some("*.ini")), loc("bb22", "Carte", r.join("mem").join("card.mcd"), true, None)];
    }
    g
}

fn cloud_paths(dest: &Path, g: &Game) -> Vec<String> {
    let mut v: Vec<String> = backup::active_meta(dest, g).unwrap().files.iter().map(|f| f.path.clone()).collect();
    v.sort();
    v
}

#[test]
fn ancien_format_inchange_pour_un_seul_emplacement() {
    let old = r#"{"games":[{"name":"Vieux","path":"C:\\S","exe":"v.exe"}],"dest":null}"#;
    let c: Config = serde_json::from_str(old).unwrap();
    assert!(c.games[0].extra_locations.is_empty() && !c.games[0].primary_disabled);
    assert_eq!(c.games[0].all_locations().len(), 1);
    let out = serde_json::to_string(&c).unwrap();
    assert!(!out.contains("extra_locations") && !out.contains("primary"), "aucun champ nouveau ecrit : {}", out);
    // Format cloud : chemins sans prefixe, meta.json sans `locations` ni `location`.
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = ctx_of(t.path(), "A");
    let g = game_on(t.path(), "A", false);
    w(&g.path.join("s.dat"), "1");
    w(&g.path.join("sous").join("t.dat"), "2");
    backup::backup(&a, &g, &dest).unwrap();
    assert_eq!(cloud_paths(&dest, &g), vec!["s.dat".to_string(), "sous/t.dat".to_string()]);
    let meta_txt = std::fs::read_dir(dest.join(agss::cloud::dest_key(&g)).join("saves")).unwrap().filter_map(|e| e.ok()).map(|e| std::fs::read_to_string(e.path().join("meta.json")).unwrap()).next().unwrap();
    assert!(!meta_txt.contains("\"locations\"") && !meta_txt.contains("\"location\""), "{}", meta_txt);
}

#[test]
fn deux_emplacements_distincts_sauvegarde_puis_restauration_vers_deux_endroits() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, b) = (ctx_of(t.path(), "A"), ctx_of(t.path(), "B"));
    let ga = game_on(t.path(), "A", true);
    w(&ga.path.join("slot1.sav"), "S1");
    w(&ga.extra_locations[0].path.join("settings.ini"), "INI");
    w(&ga.extra_locations[0].path.join("cache.tmp"), "HORS-FILTRE");
    w(&ga.extra_locations[1].path, "CARTE");
    backup::backup(&a, &ga, &dest).unwrap();
    assert_eq!(cloud_paths(&dest, &ga), vec!["@aa11/settings.ini".to_string(), "@bb22/card.mcd".to_string(), "slot1.sav".to_string()]);
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), Plan::InSync);
    // L'autre PC a les memes identifiants d'emplacement, mais d'autres chemins.
    let gb = game_on(t.path(), "B", true);
    w(&gb.extra_locations[0].path.join("autre.ini"), "LOCAL-B");
    backup::restore(&b, &gb, &dest).unwrap();
    assert_eq!(rd(&gb.path.join("slot1.sav")), "S1");
    assert_eq!(rd(&gb.extra_locations[0].path.join("settings.ini")), "INI");
    assert_eq!(rd(&gb.extra_locations[0].path.join("cache.tmp")), "<absent>", "hors filtre : jamais copie");
    assert_eq!(rd(&gb.extra_locations[1].path), "CARTE");
    assert_eq!(rd(&gb.extra_locations[0].path.join("autre.ini")), "LOCAL-B", "rien de local supprime");
    // Un seul changement dans un emplacement => nouvelle version ; l'autre PC converge sans boucle.
    w(&ga.extra_locations[0].path.join("settings.ini"), "INI-2");
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), Plan::Backup);
    backup::backup(&a, &ga, &dest).unwrap();
    let n = backup::list_versions(&dest, &ga).len();
    backup::restore(&b, &gb, &dest).unwrap();
    assert_eq!(rd(&gb.extra_locations[0].path.join("settings.ini")), "INI-2");
    w(&gb.extra_locations[0].path.join("autre.ini"), "LOCAL-B"); // B garde son fichier local en plus : il est la meme chose avant/apres
    for _ in 0..3 {
        for (c, g) in [(&a, &ga), (&b, &gb)] {
            if matches!(backup::plan(c, g, &dest).unwrap(), Plan::Backup | Plan::Restore) {
                let _ = if backup::plan(c, g, &dest).unwrap() == Plan::Backup { backup::backup(c, g, &dest).map(|_| ()) } else { backup::restore(c, g, &dest).map(|_| ()) };
            }
        }
    }
    assert!(backup::list_versions(&dest, &ga).len() <= n + 1, "pas de boucle : {} -> {}", n, backup::list_versions(&dest, &ga).len());
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), Plan::InSync);
}

#[test]
fn emplacement_inconnu_non_restaure_signale_et_conserve_sans_boucle() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let (a, b) = (ctx_of(t.path(), "A"), ctx_of(t.path(), "B"));
    let ga = game_on(t.path(), "A", true);
    w(&ga.path.join("slot1.sav"), "S1");
    w(&ga.extra_locations[0].path.join("settings.ini"), "INI");
    w(&ga.extra_locations[1].path, "CARTE");
    backup::backup(&a, &ga, &dest).unwrap();
    // B ne connait QUE l'emplacement principal.
    let gb = game_on(t.path(), "B", false);
    let r = backup::restore(&b, &gb, &dest).unwrap();
    assert_eq!(rd(&gb.path.join("slot1.sav")), "S1");
    assert!(r.unknown_locations.contains(&"Reglages".to_string()) && r.unknown_locations.contains(&"Carte".to_string()), "{:?}", r.unknown_locations);
    assert!(!t.path().join("B").join("cfg").exists());
    let st = State::load(&b).unwrap();
    assert_eq!(st.unknown_locs["Jeu"].len(), 2, "signale pour la fiche More");
    assert_eq!(backup::plan(&b, &gb, &dest).unwrap(), Plan::InSync, "les fichiers inconnus comptent comme identiques (conserves)");
    // B modifie son principal : la nouvelle version CONSERVE les emplacements qu'il ne connait pas.
    w(&gb.path.join("slot1.sav"), "S1-B");
    backup::backup(&b, &gb, &dest).unwrap();
    assert_eq!(cloud_paths(&dest, &gb), vec!["@aa11/settings.ini".to_string(), "@bb22/card.mcd".to_string(), "slot1.sav".to_string()]);
    let m = backup::active_meta(&dest, &gb).unwrap();
    assert_eq!(m.locations.len(), 2, "description des emplacements reprise de la version precedente");
    // A recupere le principal, ses autres emplacements sont intacts, et tout converge.
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), Plan::Restore);
    backup::restore(&a, &ga, &dest).unwrap();
    assert_eq!(rd(&ga.path.join("slot1.sav")), "S1-B");
    assert_eq!(rd(&ga.extra_locations[0].path.join("settings.ini")), "INI");
    assert_eq!(backup::plan(&a, &ga, &dest).unwrap(), Plan::InSync);
    assert_eq!(backup::plan(&b, &gb, &dest).unwrap(), Plan::InSync);
}

#[test]
fn activation_desactivation_sans_rien_supprimer() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = ctx_of(t.path(), "A");
    let mut g = game_on(t.path(), "A", true);
    w(&g.path.join("s.sav"), "1");
    w(&g.extra_locations[0].path.join("x.ini"), "I");
    w(&g.extra_locations[1].path, "C");
    backup::backup(&a, &g, &dest).unwrap();
    // Desactive l'emplacement « Reglages » : exclu de la synchro, fichiers locaux et cloud intacts, pas de nouvelle version.
    g.extra_locations[0].enabled = false;
    assert_eq!(backup::plan(&a, &g, &dest).unwrap(), Plan::InSync);
    w(&g.extra_locations[0].path.join("x.ini"), "MODIFIE-hors-synchro");
    assert_eq!(backup::plan(&a, &g, &dest).unwrap(), Plan::InSync, "un emplacement desactive ne declenche rien");
    assert_eq!(rd(&g.extra_locations[0].path.join("x.ini")), "MODIFIE-hors-synchro");
    // Reactive : le changement est detecte et sauvegarde.
    g.extra_locations[0].enabled = true;
    assert_eq!(backup::plan(&a, &g, &dest).unwrap(), Plan::Backup);
    // Principal desactive : seuls les extras restent.
    g.primary_disabled = true;
    assert_eq!(backup::plan(&a, &g, &dest).unwrap(), Plan::Backup);
    assert!(actions::own_locations_overlap(&g) == false);
}

#[cfg(windows)]
#[test]
fn recouvrement_entre_emplacements_et_jeux_detecte() {
    let a = game_on(Path::new("C:/x"), "PC", true);
    let mut b = Game { name: "Autre".into(), path: PathBuf::from("C:/x/PC/cfg"), ..Default::default() };
    assert!(actions::targets_overlap(&a, &b), "l'emplacement Reglages de A recouvre le principal de B");
    b.path = PathBuf::from("C:/y");
    assert!(!actions::targets_overlap(&a, &b));
    b.extra_locations = vec![loc("zz", "Carte", PathBuf::from("c:\\x\\pc\\mem\\card.mcd"), true, None)];
    assert!(actions::targets_overlap(&a, &b), "meme fichier dans un emplacement supplementaire");
    let mut own = game_on(Path::new("C:/x"), "PC", true);
    own.extra_locations[0].path = own.path.join("sous");
    assert!(actions::own_locations_overlap(&own), "deux emplacements imbriques d'un meme jeu");
}

#[test]
fn apercu_et_resume_sur_l_union() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = ctx_of(t.path(), "A");
    let g = game_on(t.path(), "A", true);
    w(&g.path.join("s.sav"), "1");
    w(&g.extra_locations[0].path.join("x.ini"), "I");
    w(&g.extra_locations[1].path, "C");
    backup::backup(&a, &g, &dest).unwrap();
    assert_eq!(backup::local_summary(&a, &g).0, 3, "resume local = union des emplacements");
    let pv = agss::preview::for_game(&a, &g, &dest, None).unwrap();
    assert_eq!(pv.files.len(), 3);
    assert_eq!(pv.identical, 3);
    let _ = pv;
}

#[test]
fn configuration_des_emplacements_et_status_json() {
    let t = TempDir::new().unwrap();
    let ctx = ctx_of(t.path(), "A");
    let g = game_on(t.path(), "A", false);
    std::fs::create_dir_all(&g.path).unwrap();
    Config { games: vec![g.clone()], dest: None }.save(&ctx).unwrap();
    let extra = loc("cc33", "Reglages", t.path().join("cfg"), false, Some("*.ini"));
    std::fs::create_dir_all(&extra.path).unwrap();
    actions::set_locations(&ctx, "Jeu", &actions::LocEdit { primary_label: Some("Sauvegardes".into()), primary_enabled: true, extras: vec![extra.clone()] }).unwrap();
    let c = Config::load(&ctx).unwrap().games[0].clone();
    assert_eq!((c.primary_label.as_deref(), c.extra_locations.len(), c.is_multi()), (Some("Sauvegardes"), 1, true));
    assert!(actions::set_locations(&ctx, "Jeu", &actions::LocEdit { primary_enabled: true, extras: vec![extra.clone(), extra.clone()], ..Default::default() }).is_err(), "identifiants en double refuses");
    actions::remove_location(&ctx, "Jeu", "cc33").unwrap();
    assert!(Config::load(&ctx).unwrap().games[0].extra_locations.is_empty());
    // status.json : tableau `locations` (additif).
    use agss::statusfile::*;
    use agss::vm::GameStatus;
    let gi = vec![GameIn { name: "Jeu".into(), key: "jeu".into(), exe: None, install_dir: None, save_path: g.path.clone(), status: GameStatus::UpToDate, last_sync_ms: None }];
    let mut doc = build(&Inputs { app_version: "1.0.3", now_ms: 1_000, watching: true, paused: false, reachable: true, shared: None, user: "u", games: gi });
    let x = GameExtra { locations: vec![LocEntry { id: "main".into(), label: "Sauvegardes".into(), path: "C:\\S".into(), is_file: false, filter: None, enabled: true, exists: true, files: 3, size_bytes: 9 }], ..Default::default() };
    enrich(&mut doc, &[x], &GlobalExtra::default());
    let v: serde_json::Value = serde_json::from_str(&to_json(&doc)).unwrap();
    assert_eq!((v["games"][0]["locations"][0]["id"].as_str(), v["games"][0]["locations"][0]["files"].as_u64(), v["games"][0]["name"].as_str()), (Some("main"), Some(3), Some("Jeu")));
}
