//! Lot Linux 2 : ALLER-RETOUR Linux <-> Windows du stockage cloud-safe. Les dossiers `tests/fixtures/crossplat/from_windows`
//! et `from_linux` sont de VRAIES sauvegardes ecrites par la version Windows et par la version Linux (generees par le test
//! ignore `genere_fixture_de_cette_plateforme`). Sur chaque plateforme, les deux sont restaurees et verifiees octet par
//! octet, puis une nouvelle version locale est ajoutee dans la meme chaine (aucun conflit, rien d'ecrase).
//! Les gabarits universels (`%APPDATA%/...`) passent d'un prefixe Proton a un profil Windows et inversement.
use agss::backup::{self, BackupOutcome};
use agss::config::Game;
use agss::hints::{Hint, KnownFolders};
use agss::Ctx;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const GAME: &str = "CrossJeu";

/// Contenu de la sauvegarde de reference : noms avec espaces, accents, ideogrammes, fichier cache, fichier vide, arborescence.
fn reference_files(os: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("Profil 1/Sauvegarde été.sav".to_string(), format!("progression 80 h — écrit sous {}", os));
    m.insert("日本語/セーブ.dat".to_string(), "データ".to_string());
    m.insert(".reglages-caches".to_string(), "option=1\n".to_string());
    m.insert("vide.bin".to_string(), String::new());
    m.insert("sub/deep/a/b/c.txt".to_string(), "profond\r\nfin de ligne Windows conservée".to_string());
    m
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("crossplat")
}

fn ctx(root: &Path, machine: &str) -> Ctx {
    let mut c = Ctx::new(root.join(format!("data-{}", machine)));
    c.machine = machine.to_string();
    std::fs::create_dir_all(&c.data_dir).unwrap();
    c
}

fn game(path: &Path) -> Game {
    Game { name: GAME.into(), path: path.to_path_buf(), exe: Some("crossjeu.exe".into()), ..Default::default() }
}

fn write_tree(dir: &Path, files: &BTreeMap<String, String>) {
    for (rel, content) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
}

fn copy_dir(from: &Path, to: &Path) {
    for e in walkdir::WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        let rel = e.path().strip_prefix(from).unwrap();
        let t = to.join(rel);
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&t).unwrap();
        } else {
            std::fs::copy(e.path(), &t).unwrap();
        }
    }
}

/// Toutes les meta.json d'un dossier partage.
fn metas(dest: &Path) -> Vec<serde_json::Value> {
    walkdir::WalkDir::new(dest)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "meta.json")
        .map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap())
        .collect()
}

/// GENERATEUR (lance a la main une fois par plateforme, jamais par defaut) :
///   cargo test --test v33 -- --ignored genere_fixture_de_cette_plateforme
/// Ecrit tests/fixtures/crossplat/from_<os>/dest (dossier partage reel) et expected.json.
#[test]
#[ignore]
fn genere_fixture_de_cette_plateforme() {
    let os = if cfg!(windows) { "windows" } else { "linux" };
    let out = fixtures().join(format!("from_{}", os));
    let _ = std::fs::remove_dir_all(&out);
    let t = TempDir::new().unwrap();
    // Source dans un emplacement « universel » pour que meta.json porte un gabarit : Windows -> %APPDATA% reel (dossier
    // temporaire retire ensuite) ; Linux -> prefixe Proton (drive_c/users/steamuser/AppData/Roaming).
    #[cfg(windows)]
    let src = dirs::config_dir().unwrap().join("AGSS-fixture-CrossJeu").join("Saves");
    #[cfg(not(windows))]
    let src = t.path().join("steamapps/compatdata/4242/pfx/drive_c/users/steamuser/AppData/Roaming/AGSS-fixture-CrossJeu/Saves");
    let _ = std::fs::remove_dir_all(&src);
    write_tree(&src, &reference_files(os));
    let dest = out.join("dest");
    let c = ctx(t.path(), if cfg!(windows) { "PC-WINDOWS" } else { "STEAMDECK" });
    match backup::backup(&c, &game(&src), &dest).unwrap() {
        BackupOutcome::Backed { .. } => {}
        o => panic!("{:?}", o),
    }
    let tpl = agss::hints::to_template(&src, &KnownFolders::real());
    let exp = serde_json::json!({ "os": os, "files": reference_files(os), "save_path_tpl": tpl });
    std::fs::write(out.join("expected.json"), serde_json::to_string_pretty(&exp).unwrap()).unwrap();
    #[cfg(windows)]
    let _ = std::fs::remove_dir_all(src.parent().unwrap());
}

#[test]
fn sauvegardes_windows_et_linux_restaurees_sur_cette_plateforme() {
    let mut seen = 0;
    for os in ["windows", "linux"] {
        let fx = fixtures().join(format!("from_{}", os));
        let exp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fx.join("expected.json")).expect("fixture presente")).unwrap();
        seen += 1;
        let t = TempDir::new().unwrap();
        // On travaille sur une COPIE : la fixture n'est jamais modifiee.
        let dest = t.path().join("Mon Drive").join("Sauvegardes partagées");
        copy_dir(&fx.join("dest"), &dest);
        // Format : chemins relatifs, separateur '/', aucun chemin absolu ni lettre de lecteur ; machine d'origine.
        for m in metas(&dest) {
            assert_eq!(m["machine"], if os == "windows" { "PC-WINDOWS" } else { "STEAMDECK" });
            assert_eq!(m["format"], 2);
            for f in m["files"].as_array().unwrap() {
                let p = f["path"].as_str().unwrap();
                assert!(!p.contains('\\') && !p.starts_with('/') && !p.contains(':'), "{} : chemin non portable", p);
            }
            assert_eq!(m["save_path_tpl"], exp["save_path_tpl"], "gabarit universel conserve");
        }
        // Restauration sur CE PC, dans un dossier different (espaces + accents dans le chemin).
        let local = t.path().join("Jeux à moi").join("Cross Jeu");
        let c = ctx(t.path(), "CE-PC");
        let r = backup::restore(&c, &game(&local), &dest).unwrap_or_else(|e| panic!("restauration de la sauvegarde {} : {}", os, e));
        let files = exp["files"].as_object().unwrap();
        assert_eq!(r.files_restored, files.len(), "{}", os);
        for (rel, content) in files {
            let got = std::fs::read(local.join(rel)).unwrap_or_else(|e| panic!("{} / {} : {}", os, rel, e));
            assert_eq!(got, content.as_str().unwrap().as_bytes(), "{} / {} : contenu different", os, rel);
        }
        // Puis ce PC modifie et sauvegarde : nouvelle version dans la MEME chaine (pas de conflit), l'originale reste.
        std::fs::write(local.join("Profil 1/Sauvegarde été.sav"), "progression 81 h").unwrap();
        match backup::backup(&c, &game(&local), &dest).unwrap() {
            BackupOutcome::Backed { .. } => {}
            o => panic!("{} : {:?}", os, o),
        }
        let h = backup::history(&c, &game(&local), &dest);
        assert_eq!(h.rows.len(), 2, "{} : version d'origine + nouvelle", os);
        assert!(h.conflict_heads.is_empty(), "{} : aucun conflit", os);
        assert_eq!(h.rows[0].machine, "CE-PC");
        assert!(h.rows[1].machine == "PC-WINDOWS" || h.rows[1].machine == "STEAMDECK");
        // Retour : un autre PC (l'autre systeme) restaure la nouvelle version.
        let back = t.path().join("retour");
        let c2 = ctx(t.path(), "AUTRE-PC");
        backup::restore(&c2, &game(&back), &dest).unwrap();
        assert_eq!(std::fs::read_to_string(back.join("Profil 1/Sauvegarde été.sav")).unwrap(), "progression 81 h");
        assert_eq!(std::fs::read_to_string(back.join("日本語/セーブ.dat")).unwrap(), "データ");
    }
    assert_eq!(seen, 2);
}

#[test]
fn gabarits_universels_entre_prefixe_proton_et_profil_windows() {
    // Linux (Proton) -> gabarit -> Windows.
    let p = Path::new("/home/deck/.local/share/Steam/steamapps/compatdata/1245620/pfx/drive_c/users/steamuser/AppData/Roaming/EldenRing/76561198000000000");
    let tpl = agss::proton::template_from_prefix_path(p).unwrap();
    assert_eq!(tpl, "%APPDATA%/EldenRing/76561198000000000");
    let win = KnownFolders { roaming_app_data: Some(PathBuf::from(r"C:\Users\Epikou\AppData\Roaming")), ..Default::default() };
    let w = agss::hints::expand(&tpl, &win).unwrap();
    assert!(w.to_string_lossy().replace('\\', "/").ends_with("Users/Epikou/AppData/Roaming/EldenRing/76561198000000000"), "{}", w.display());
    // Windows -> gabarit -> prefixe Proton de cette machine (la ou le dossier existe deja).
    let t = TempDir::new().unwrap();
    let pfx_a = t.path().join("lib/steamapps/compatdata/111/pfx");
    let pfx_b = t.path().join("lib/steamapps/compatdata/1245620/pfx");
    std::fs::create_dir_all(pfx_a.join("drive_c/users/steamuser")).unwrap();
    let there = pfx_b.join("drive_c/users/steamuser/AppData/Roaming/EldenRing");
    std::fs::create_dir_all(&there).unwrap();
    let wtpl = agss::hints::to_template(Path::new(r"C:\Users\Epikou\AppData\Roaming\EldenRing"), &win);
    #[cfg(windows)]
    assert_eq!(wtpl.as_deref(), Some("%APPDATA%/EldenRing"));
    let wtpl = wtpl.unwrap_or_else(|| "%APPDATA%/EldenRing".to_string());
    let hints = vec![Hint { game_name: "ELDEN RING".into(), exe: "eldenring.exe".into(), save_path_tpl: Some(wtpl) }];
    let s = agss::hints::suggestions_ext(&hints, "eldenring.exe", &KnownFolders::default(), &[pfx_a.clone(), pfx_b.clone()], &|p: &Path| p.is_dir());
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].local_path.as_deref(), Some(there.as_path()));
    assert!(s[0].exists);
    // Sans prefixe (Windows) : expansion ordinaire, comportement inchange.
    let s2 = agss::hints::suggestions_ext(&hints, "eldenring.exe", &win, &[], &|p: &Path| p.is_dir());
    assert_eq!(s2, agss::hints::suggestions(&hints, "eldenring.exe", &win, &|p: &Path| p.is_dir()));
}

/// Course entre deux PC (constatee sur un dossier partage monte sans evenements, ou la resynchro periodique des deux PC
/// tombe au meme instant) : B, a jour, enregistre un « point de synchro » pendant que A vient d'ecrire une version plus
/// recente. Aucune version ne doit etre creee par B (sinon sa copie de l'ancien etat devient une branche concurrente).
#[test]
fn point_de_synchro_pendant_une_nouvelle_version_d_un_autre_pc() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("partage");
    let (sa, sb) = (t.path().join("save A"), t.path().join("save B"));
    write_tree(&sa, &reference_files("A"));
    let (ca, cb) = (ctx(t.path(), "PC-A"), ctx(t.path(), "PC-B"));
    backup::backup(&ca, &game(&sa), &dest).unwrap();
    backup::restore(&cb, &game(&sb), &dest).unwrap();
    assert_eq!(backup::plan(&cb, &game(&sb), &dest).unwrap(), backup::Plan::InSync);
    // A avance.
    std::fs::write(sa.join("Profil 1/Sauvegarde été.sav"), "v2 depuis A").unwrap();
    backup::backup(&ca, &game(&sa), &dest).unwrap();
    let before = backup::list_versions(&dest, &game(&sb)).len();
    // B (qui croyait etre a jour) enregistre son point de synchro : rien n'est ecrit.
    assert!(matches!(backup::backup_in_sync(&cb, &game(&sb), &dest).unwrap(), BackupOutcome::UpToDate));
    assert_eq!(backup::list_versions(&dest, &game(&sb)).len(), before);
    assert_eq!(backup::plan(&cb, &game(&sb), &dest).unwrap(), backup::Plan::Restore);
    backup::restore(&cb, &game(&sb), &dest).unwrap();
    assert_eq!(std::fs::read_to_string(sb.join("Profil 1/Sauvegarde été.sav")).unwrap(), "v2 depuis A");
    // La sauvegarde explicite garde son sens : un local reellement modifie part en branche (conflit), jamais perdu.
    assert!(backup::history(&cb, &game(&sb), &dest).conflict_heads.is_empty());
}

#[test]
fn chemins_avec_espaces_et_unicode_scannes_avec_separateur_universel() {
    let t = TempDir::new().unwrap();
    let d = t.path().join("Sauvegardes du jeu ✓");
    write_tree(&d, &reference_files("test"));
    let (files, _) = agss::store::scan(&d).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(names.contains(&"Profil 1/Sauvegarde été.sav"), "{:?}", names);
    assert!(names.contains(&"日本語/セーブ.dat"));
    assert!(names.contains(&"sub/deep/a/b/c.txt"));
    assert!(names.iter().all(|n| !n.contains('\\')));
}
