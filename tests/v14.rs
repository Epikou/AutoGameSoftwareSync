//! Indices exe / nom / dossier de sauvegarde dans le dossier partage et reconnaissance sur un autre PC.
use agss::backup;
use agss::cloud;
use agss::config::Game;
use agss::hints::*;
use agss::i18n::{tr, Lang, K};
use agss::process::ProcessChecker;
use agss::store::Meta;
use agss::{Ctx, Res};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn pb(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn bob() -> KnownFolders {
    KnownFolders {
        local_app_data: Some(pb("C:\\Users\\bob\\AppData\\Local")),
        roaming_app_data: Some(pb("C:\\Users\\bob\\AppData\\Roaming")),
        local_low: Some(pb("C:\\Users\\bob\\AppData\\LocalLow")),
        // Documents redirige (OneDrive sur un autre lecteur).
        documents: Some(pb("D:\\OneDrive\\Documents")),
        saved_games: Some(pb("C:\\Users\\bob\\Saved Games")),
    }
}

fn alice() -> KnownFolders {
    KnownFolders {
        local_app_data: Some(pb("E:\\Profils\\alice\\AppData\\Local")),
        roaming_app_data: Some(pb("E:\\Profils\\alice\\AppData\\Roaming")),
        local_low: Some(pb("E:\\Profils\\alice\\AppData\\LocalLow")),
        documents: Some(pb("E:\\Profils\\alice\\Documents")),
        saved_games: Some(pb("E:\\Profils\\alice\\Saved Games")),
    }
}

#[cfg(windows)]
#[test]
fn chemin_vers_gabarit_pour_toutes_les_bases() {
    let k = bob();
    let t = |p: &str| to_template(Path::new(p), &k);
    assert_eq!(t("C:\\Users\\bob\\AppData\\Local\\Mistria\\Saves").as_deref(), Some("%LOCALAPPDATA%/Mistria/Saves"));
    assert_eq!(t("C:\\Users\\bob\\AppData\\Roaming\\Jeu").as_deref(), Some("%APPDATA%/Jeu"));
    assert_eq!(t("C:\\Users\\bob\\AppData\\LocalLow\\Studio\\Jeu").as_deref(), Some("%LOCALLOW%/Studio/Jeu"));
    assert_eq!(t("D:\\OneDrive\\Documents\\My Games\\Jeu").as_deref(), Some("%DOCUMENTS%/My Games/Jeu"), "Documents redirige");
    assert_eq!(t("C:\\Users\\bob\\Saved Games\\Jeu").as_deref(), Some("%SAVEDGAMES%/Jeu"));
    assert_eq!(t("c:\\users\\BOB\\appdata\\local\\mistria").as_deref(), Some("%LOCALAPPDATA%/mistria"), "casse ignoree pour la base");
    // Hors des bases : aucun indice de chemin.
    for p in ["D:\\Games\\Mistria\\Saves", "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Jeu", "C:\\Users\\bob\\Desktop\\Jeu", "C:\\Users\\bob\\Documents\\Jeu"] {
        assert_eq!(t(p), None, "{}", p);
    }
    assert_eq!(t("C:\\Users\\bob\\AppData\\Local"), None, "la base elle-meme est refusee");
    assert_eq!(t("C:\\Users\\bob\\AppData\\Local\\..\\Roaming\\x"), None, "`..` refuse");
    // Aucun nom d'utilisateur ni lettre de lecteur dans le gabarit.
    for p in ["C:\\Users\\bob\\AppData\\Local\\Mistria", "D:\\OneDrive\\Documents\\Jeu", "C:\\Users\\bob\\Saved Games\\Jeu"] {
        let tpl = t(p).unwrap();
        assert!(!tpl.to_lowercase().contains("bob") && !tpl.contains(':') && !tpl.contains('\\'), "{}", tpl);
    }
}

#[cfg(windows)]
#[test]
fn base_la_plus_specifique() {
    let mut k = bob();
    k.documents = Some(pb("C:\\Users\\bob\\AppData\\Local\\Docs"));
    assert_eq!(to_template(Path::new("C:\\Users\\bob\\AppData\\Local\\Docs\\Jeu"), &k).as_deref(), Some("%DOCUMENTS%/Jeu"));
    assert_eq!(to_template(Path::new("C:\\Users\\bob\\AppData\\Local\\Autre\\Jeu"), &k).as_deref(), Some("%LOCALAPPDATA%/Autre/Jeu"));
}

#[cfg(windows)]
#[test]
fn gabarit_vers_chemin_de_cette_machine_et_expansion_securisee() {
    let a = alice();
    assert_eq!(expand("%LOCALAPPDATA%/Mistria/Saves", &a), Some(pb("E:\\Profils\\alice\\AppData\\Local\\Mistria\\Saves")));
    assert_eq!(expand("%DOCUMENTS%/My Games/Jeu", &a), Some(pb("E:\\Profils\\alice\\Documents\\My Games\\Jeu")));
    assert_eq!(expand("%SAVEDGAMES%/Jeu", &a), Some(pb("E:\\Profils\\alice\\Saved Games\\Jeu")));
    // Aller-retour d'un PC a l'autre.
    let tpl = to_template(Path::new("C:\\Users\\bob\\AppData\\LocalLow\\Studio\\Jeu"), &bob()).unwrap();
    assert_eq!(expand(&tpl, &a), Some(pb("E:\\Profils\\alice\\AppData\\LocalLow\\Studio\\Jeu")));
    // Refus : `..`, lecteur, separateur en tete, base inconnue, base seule, composants vides, pourcentages caches.
    for bad in [
        "%LOCALAPPDATA%/../Windows",
        "%LOCALAPPDATA%/a/../../b",
        "%LOCALAPPDATA%/C:/Windows",
        "%LOCALAPPDATA%//x",
        "%LOCALAPPDATA%/",
        "%LOCALAPPDATA%",
        "%INCONNU%/x",
        "C:/Windows/x",
        "/x",
        "%LOCALAPPDATA%/%APPDATA%/x",
        "x/%LOCALAPPDATA%/y",
        "",
    ] {
        assert_eq!(expand(bad, &a), None, "{:?} doit etre refuse", bad);
    }
    assert_eq!(expand("%LOCALAPPDATA%/Jeu", &KnownFolders::default()), None, "base inconnue sur cette machine");
}

#[test]
fn ancien_meta_json_sans_champs_et_champs_facultatifs_omis() {
    let tmp = TempDir::new().unwrap();
    let (a, g) = machine(tmp.path(), "A", "Jeu.exe");
    write(&g.path, "save.dat", "1");
    let dest = tmp.path().join("cloud");
    backup::backup(&a, &g, &dest).unwrap();
    let dir = newest(&dest, &g);
    let txt = std::fs::read_to_string(dir.join("meta.json")).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["exe"], "Jeu.exe");
    assert_eq!(v["game_name"], "TestGame");
    // Ancien meta.json : les trois champs absents -> lisible, valeurs par defaut.
    for k in ["exe", "game_name", "save_path_tpl"] {
        v.as_object_mut().unwrap().remove(k);
    }
    let old: Meta = serde_json::from_value(v).unwrap();
    assert!(old.exe.is_none() && old.game_name.is_none() && old.save_path_tpl.is_none());
    // Sans valeur, un champ n'est pas ecrit.
    assert!(!serde_json::to_string(&old).unwrap().contains("save_path_tpl"));
}

#[test]
fn nouvelle_sauvegarde_ecrit_les_indices_sans_toucher_a_l_existant_ni_a_l_empreinte() {
    let tmp = TempDir::new().unwrap();
    let (a, g) = machine(tmp.path(), "A", "C:\\Jeux\\Mistria.exe");
    write(&g.path, "save.dat", "v1");
    let dest = tmp.path().join("cloud");
    backup::backup(&a, &g, &dest).unwrap();
    let before = tree(&dest);
    let h1 = backup::active_meta(&dest, &g).unwrap();
    assert_eq!(h1.exe.as_deref(), Some("Mistria.exe"), "nom de fichier seul");
    if let Some(t) = &h1.save_path_tpl {
        assert!(t.starts_with('%') && !t.contains(':') && !t.contains('\\'), "{}", t);
    }
    write(&g.path, "save.dat", "v2");
    std::thread::sleep(std::time::Duration::from_millis(30));
    backup::backup(&a, &g, &dest).unwrap();
    let after = tree(&dest);
    // Sous-ensemble strict : aucun fichier existant n'a ete modifie, renomme ou supprime ; seuls des fichiers neufs apparaissent.
    for (p, c) in &before {
        assert_eq!(after.get(p), Some(c), "{} modifie ou disparu", p);
    }
    assert!(after.len() > before.len());
    // L'empreinte du contenu ne depend pas des indices.
    let h2 = backup::active_meta(&dest, &g).unwrap();
    let mut files = h2.files.clone();
    files.sort_by(|x, y| x.path.cmp(&y.path));
    assert_eq!(agss::store::global_hash(&files), h2.global_hash);
}

#[test]
fn reconnaissance_de_l_exe_insensible_a_la_casse_plusieurs_candidats() {
    let hints = vec![
        Hint { game_name: "Field of Mistria".into(), exe: "Mistria.exe".into(), save_path_tpl: Some("%LOCALAPPDATA%/Mistria".into()) },
        Hint { game_name: "Mistria Demo".into(), exe: "mistria.EXE".into(), save_path_tpl: None },
        Hint { game_name: "Autre".into(), exe: "Autre.exe".into(), save_path_tpl: Some("%APPDATA%/Autre".into()) },
        Hint { game_name: "Field of Mistria".into(), exe: "Mistria.exe".into(), save_path_tpl: Some("%LOCALAPPDATA%/Mistria".into()) },
    ];
    let m = matches(&hints, "mistria.exe");
    assert_eq!(m.iter().map(|h| h.game_name.as_str()).collect::<Vec<_>>(), vec!["Field of Mistria", "Mistria Demo"], "plusieurs candidats, sans doublon, jamais choisi a la place de l'utilisateur");
    assert_eq!(matches(&hints, "D:\\Jeux\\MISTRIA.EXE").len(), 2, "chemin complet : seul le nom de fichier compte");
    assert_eq!(matches(&hints, "\"C:\\x\\Autre.exe\"").len(), 1);
    assert!(matches(&hints, "").is_empty() && matches(&hints, "inconnu.exe").is_empty());
    assert_eq!(exe_file_name("C:/a/b/Jeu.exe"), "Jeu.exe");
}

#[cfg(windows)]
#[test]
fn modele_de_l_encart_nom_dossier_local_et_existence() {
    let hints = vec![
        Hint { game_name: "Field of Mistria".into(), exe: "Mistria.exe".into(), save_path_tpl: Some("%LOCALAPPDATA%/Mistria/Saves".into()) },
        Hint { game_name: "Ancien".into(), exe: "Mistria.exe".into(), save_path_tpl: None },
        Hint { game_name: "Piege".into(), exe: "Mistria.exe".into(), save_path_tpl: Some("%LOCALAPPDATA%/../Windows".into()) },
    ];
    let exists = |p: &Path| p == Path::new("E:\\Profils\\alice\\AppData\\Local\\Mistria\\Saves");
    let s = suggestions(&hints, "mistria.exe", &alice(), &exists);
    assert_eq!(s.len(), 3);
    assert_eq!((s[0].game_name.as_str(), s[0].local_path.clone(), s[0].exists), ("Field of Mistria", Some(pb("E:\\Profils\\alice\\AppData\\Local\\Mistria\\Saves")), true));
    assert_eq!((s[1].local_path.clone(), s[1].exists), (None, false), "pas d'indice de chemin : nom seulement");
    assert_eq!(s[2].local_path, None, "gabarit dangereux refuse");
    let s2 = suggestions(&hints[..1], "Mistria.exe", &alice(), &|_| false);
    assert!(!s2[0].exists, "dossier absent : propose quand meme, avec avertissement");
    assert!(suggestions(&[], "Mistria.exe", &alice(), &exists).is_empty(), "aucun indice (anciennes sauvegardes) : rien");
}

#[test]
fn lecture_legere_du_dernier_meta_valide_du_dossier_partage() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("cloud");
    let (a, g) = machine(tmp.path(), "A", "Mistria.exe");
    write(&g.path, "save.dat", "1");
    backup::backup(&a, &g, &dest).unwrap();
    // Une sauvegarde plus recente mais INCOMPLETE (fichiers absents) : ignoree, la valide precedente fait foi.
    write(&g.path, "save.dat", "22");
    std::thread::sleep(std::time::Duration::from_millis(30));
    backup::backup(&a, &g, &dest).unwrap();
    let newest_dir = newest(&dest, &g);
    std::fs::rename(newest_dir.join("files"), newest_dir.join("files-ailleurs")).unwrap();
    // Un jeu ancien format / sans indice : rien.
    std::fs::create_dir_all(dest.join("vieuxjeu").join("saves")).unwrap();
    let h = cloud::latest_hints(&dest);
    assert_eq!(h.len(), 1, "{:?}", h);
    assert_eq!((h[0].game_name.as_str(), h[0].exe.as_str()), ("TestGame", "Mistria.exe"));
}

#[test]
fn textes_de_l_encart_dans_les_cinq_langues() {
    for k in [K::HintTitle, K::HintMany, K::HintExistsYes, K::HintExistsNo, K::HintNoPath, K::BtnHintUse, K::BtnHintIgnore, K::TipHintUse, K::TipHintIgnore] {
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr(l, k) };
            assert!(!s.is_empty());
        }
    }
    assert_eq!(tr(Lang::Fr, K::HintTitle), "Trouvé sur un autre PC");
}

// ------------------------------------------------------------------ outils

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

fn machine(root: &Path, name: &str, exe: &str) -> (Ctx, Game) {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "TestGame".into(), path: root.join(format!("saves-{}", name)), exe: Some(exe.into()), dest_key: None, ..Default::default() };
    (ctx, game)
}

fn write(dir: &Path, rel: &str, c: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}

fn newest(dest: &Path, g: &Game) -> PathBuf {
    backup::list_versions(dest, g).pop().unwrap()
}

fn tree(dest: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    walkdir::WalkDir::new(dest).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).map(|e| (e.path().display().to_string(), std::fs::read(e.path()).unwrap())).collect()
}
