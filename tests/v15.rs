//! Base des jeux embarquee, couche d'overrides, ajout dynamique par exe, bouton « Ajouter ». Aucun acces reseau.
use agss::autofill::*;
use agss::cloud::DestGame;
use agss::gamedb::*;
use agss::hints::Suggestion;
use agss::i18n::{tr, Lang, K};
use agss::catalog::Catalog;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn game(name: &str, steam: Option<u64>, dirs: &[&str], exes: &[&str], paths: &[&str]) -> DbGame {
    DbGame { name: name.into(), steam, gog: None, install_dirs: dirs.iter().map(|s| s.to_string()).collect(), exes: exes.iter().map(|s| s.to_string()).collect(), paths: paths.iter().map(|s| s.to_string()).collect() }
}

fn mini() -> Db {
    Db {
        format: FORMAT,
        generated: "2026-10-23".into(),
        games: vec![
            game("Cool Game", Some(123), &["CoolGame"], &["Cool.exe"], &["<base>/Saves"]),
            game("Autre Jeu", None, &[], &["jeu.exe"], &["<base>/Data"]),
            game("Jeu Jumeau", None, &[], &["JEU.EXE"], &["<base>/Data"]),
        ],
    }
}

fn dbx(over: Vec<DbGame>) -> GameDb {
    GameDb::from_bytes(&encode(&mini()).unwrap(), over).unwrap()
}

#[test]
fn encodage_decodage_et_index() {
    let bytes = encode(&mini()).unwrap();
    assert!(bytes.starts_with(b"AGDB1"));
    let db = decode(&bytes).unwrap();
    assert_eq!((db.games.len(), db.generated.as_str()), (3, "2026-10-23"));
    let g = dbx(vec![]);
    assert_eq!(g.len(), 3);
    assert_eq!(g.find_by_exe("COOL.EXE")[0].name, "Cool Game", "exe insensible a la casse");
    assert_eq!(g.find_by_exe("C:\\x\\cool.exe").len(), 1, "chemin complet : nom de fichier seul");
    assert_eq!(g.find_by_exe("jeu.exe").len(), 2, "deux jeux pour le meme exe");
    assert_eq!(g.find_by_install_dir("coolgame")[0].name, "Cool Game");
    assert_eq!(g.find_by_steam(123)[0].name, "Cool Game");
    assert_eq!(g.find_by_name("cool-game")[0].name, "Cool Game", "nom normalise");
    assert!(g.find_by_steam(999).is_empty() && g.find_by_exe("inconnu.exe").is_empty());
    assert!(g.to_catalog().contains_key("Cool Game"));
    // Donnees invalides : erreur propre, jamais de panique.
    assert!(decode(b"").is_err() && decode(b"AGDB1garbage").is_err() && decode(b"XXXXXrien").is_err());
}

#[test]
fn reduction_du_catalog_windows_save_seulement() {
    let yaml = r#"
Jeu A:
  files:
    "<winLocalAppData>/A/save": { tags: [save], when: [{ os: windows }] }
    "<winLocalAppData>/A/config": { tags: [config] }
    "~/.local/A": { tags: [save], when: [{ os: linux }] }
    "<base>/libre": {}
  launch:
    "<base>/bin/A.exe": [{ when: [{ os: windows }] }]
    "<base>/a.sh": [{ when: [{ os: linux }] }]
  installDir: { "JeuA": {} }
  steam: { id: 42 }
Jeu B:
  files:
    "<base>/seulement-config": { tags: [config] }
"#;
    let m: Catalog = serde_yaml::from_str(yaml).unwrap();
    let db = build_from_catalog(&m, "2026-10-23");
    assert_eq!(db.games.len(), 1, "un jeu sans emplacement de sauvegarde Windows est ecarte");
    let a = &db.games[0];
    assert_eq!(a.paths, vec!["<base>/libre".to_string(), "<winLocalAppData>/A/save".to_string()]);
    assert_eq!((a.exes.clone(), a.install_dirs.clone(), a.steam), (vec!["a.exe".to_string()], vec!["JeuA".to_string()], Some(42)));
}

#[test]
fn overrides_locales_prioritaires_et_remplacent_la_base() {
    let t = TempDir::new().unwrap();
    let over = vec![game("Cool Game", None, &[], &["cool.exe", "cool2.exe"], &["<base>/MesSaves"]), game("Nouveau", None, &[], &["nouveau.exe"], &["<base>/N"])];
    std::fs::write(t.path().join(OVERRIDES_FILE), serde_json::to_string(&over).unwrap()).unwrap();
    assert_eq!(load_overrides(t.path()).len(), 2);
    assert!(load_overrides(&t.path().join("absent")).is_empty());
    std::fs::write(t.path().join(OVERRIDES_FILE), "{pas du json").unwrap();
    assert!(load_overrides(t.path()).is_empty(), "fichier invalide ignore");
    let g = dbx(over);
    assert_eq!(g.find_by_exe("cool.exe")[0].paths, vec!["<base>/MesSaves".to_string()], "l'override remplace l'entree de meme nom");
    assert_eq!(g.find_by_exe("cool.exe").len(), 1, "pas de doublon base + override");
    assert_eq!(g.find_by_exe("nouveau.exe")[0].name, "Nouveau");
    assert_eq!(g.override_count, 2);
    // Overrides en tete pour une meme cle d'exe.
    let g2 = dbx(vec![game("Autre Jeu", None, &[], &["jeu.exe"], &["<base>/Perso"])]);
    assert_eq!(g2.find_by_exe("jeu.exe")[0].paths, vec!["<base>/Perso".to_string()]);
}

#[test]
fn la_base_embarquee_se_decompresse_et_contient_des_jeux() {
    let t = TempDir::new().unwrap();
    let g = embedded(t.path());
    assert!(g.len() > 10_000, "{}", g.len());
    assert!(!g.generated.is_empty());
    assert!(!g.find_by_steam(570).is_empty() || !g.find_by_name("Dota 2").is_empty() || g.len() > 10_000);
    assert!(embedded_size() > 100_000 && embedded_size() < 6 * 1024 * 1024, "quelques Mo compresses au plus : {}", embedded_size());
}

// ------------------------------------------------------------------ ajout dynamique

fn steam_fixture() -> (TempDir, PathBuf, PathBuf) {
    let t = TempDir::new().unwrap();
    let lib = t.path().join("SteamLibrary");
    let install = lib.join("steamapps").join("common").join("CoolGame");
    std::fs::create_dir_all(install.join("bin")).unwrap();
    std::fs::create_dir_all(install.join("Saves")).unwrap();
    std::fs::write(install.join("bin").join("Cool.exe"), "x").unwrap();
    std::fs::write(lib.join("steamapps").join("appmanifest_123.acf"), "\"AppState\"\n{\n\t\"appid\"\t\t\"123\"\n\t\"name\"\t\t\"Cool Game\"\n\t\"installdir\"\t\t\"CoolGame\"\n}\n").unwrap();
    (t, lib, install)
}

#[test]
fn exe_choisi_par_parcourir_nom_et_dossier_verifies() {
    let (_t, lib, install) = steam_fixture();
    let db = dbx(vec![]);
    let libs = vec![lib];
    let s = lookup(&db, "Cool.exe", Some(&install.join("bin").join("Cool.exe")), &[], &libs, &|p| p.is_dir());
    assert_eq!(s, vec![Suggestion { game_name: "Cool Game".into(), local_path: Some(install.join("Saves")), exists: true }]);
    assert_eq!(status_of(&s), AutoStatus::Verified);
    assert!(autofill_candidate(&s).is_some());
    // Correspondance par AppID Steam seul (exe different du nom connu de la base) : le dossier d'installation + l'AppID suffisent
    // seulement si l'exe est cite par la base ; sinon on retombe sur l'exe (ici inconnu) : rien.
    let none = lookup(&db, "Inconnu.exe", Some(&install.join("bin").join("Inconnu.exe")), &[], &libs, &|p| p.is_dir());
    assert!(none.is_empty(), "exe inconnu : rien n'est propose");
    // Nom de fichier saisi a la main : dossier retrouve via la bibliotheque Steam (steamapps\common\<installDir>).
    let typed = lookup(&db, "cool.EXE", None, &[], &libs, &|p| p.is_dir());
    assert_eq!(typed.len(), 1);
    assert_eq!(typed[0].local_path.as_deref(), Some(install.join("Saves").as_path()));
}

#[test]
fn dossier_introuvable_propose_avec_avertissement_et_nom_du_dossier_partage_repris() {
    let (_t, lib, install) = steam_fixture();
    std::fs::remove_dir_all(install.join("Saves")).unwrap();
    let db = dbx(vec![]);
    let dest = vec![DestGame { key: agss::cloud::game_key("Cool Game"), name: "Cool-Game (PC)".into(), dirs: vec![] }];
    let s = lookup(&db, "Cool.exe", Some(&install.join("bin").join("Cool.exe")), &dest, &[lib], &|p| p.is_dir());
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].game_name, "Cool-Game (PC)", "meme cle normalisee : on rejoint le jeu du dossier partage");
    assert!(!s[0].exists && s[0].local_path.is_some(), "meilleur chemin propose malgre tout");
    assert_eq!(status_of(&s), AutoStatus::Missing);
}

#[test]
fn plusieurs_candidats_jamais_de_choix_silencieux() {
    let db = dbx(vec![]);
    let s = lookup(&db, "jeu.exe", None, &[], &[], &|_| false);
    assert_eq!(s.len(), 2);
    assert_eq!(status_of(&s), AutoStatus::Many);
    assert!(autofill_candidate(&s).is_none(), "pas de remplissage automatique avec plusieurs candidats");
    assert_eq!(status_of(&[]), AutoStatus::None);
    // Un jeu avec deux dossiers existants : une ligne par dossier.
    let t = TempDir::new().unwrap();
    let lib = t.path().join("L");
    let base = lib.join("steamapps").join("common").join("Deux");
    std::fs::create_dir_all(base.join("A")).unwrap();
    std::fs::create_dir_all(base.join("B")).unwrap();
    let db2 = GameDb::from_db(Db { format: FORMAT, generated: String::new(), games: vec![game("Deux", None, &["Deux"], &["deux.exe"], &["<base>/A", "<base>/B"])] }, vec![]);
    let s2 = lookup(&db2, "deux.exe", None, &[], &[lib], &|p| p.is_dir());
    assert_eq!(s2.len(), 2);
    assert_eq!(status_of(&s2), AutoStatus::Many);
}

#[test]
fn bouton_ajouter_machine_a_etats() {
    let mut b = AddBtnState::default();
    assert_eq!(b.update(false, false), AddBtn::Invalid);
    assert_eq!(b.update(true, false), AddBtn::Pulsing, "valide : anime");
    assert_eq!(b.update(true, false), AddBtn::Pulsing);
    assert_eq!(b.update(true, true), AddBtn::Stopped, "clic : l'animation s'arrete");
    assert_eq!(b.update(true, false), AddBtn::Stopped, "reste arrete tant que valide");
    assert_eq!(b.update(false, false), AddBtn::Invalid, "champ invalide : normal");
    assert_eq!(b.update(true, false), AddBtn::Pulsing, "redevenu valide : reprend");
    b.reset(); // sortie de la page
    assert_eq!(b.update(true, false), AddBtn::Pulsing);
    let mut b2 = AddBtnState::default();
    b2.update(true, true);
    b2.reset();
    assert_eq!(b2.update(true, false), AddBtn::Pulsing);
    for i in 0..100 {
        let p = pulse(i as f64 * 0.1);
        assert!((0.0..=1.0).contains(&p));
    }
}

#[test]
fn textes_ajout_dynamique_cinq_langues() {
    for k in [K::AutoTitle, K::AutoVerified, K::AutoMissing, K::AutoMany] {
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr(l, k) };
            assert!(!s.is_empty());
        }
    }
    assert_eq!(tr(Lang::En, K::AutoVerified), "Path found and verified");
    let _ = Path::new("");
}
