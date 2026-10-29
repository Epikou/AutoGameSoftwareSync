//! Tests v0.7 : stockage cloud-safe (un dossier unique par sauvegarde, jamais de renommage dans la
//! destination), chaine de versions (parent/branches), heure corrigee, noms de jeux entre PC,
//! dates lisibles, notifications. UNIQUEMENT des dossiers temporaires.
use agss::backup::{self, decide, heads, BackupOutcome, Plan, VNode};
use agss::cloud::{self, VKind};
use agss::config::Game;
use agss::i18n::Lang;
use agss::notify::{self, Notifier};
use agss::process::ProcessChecker;
use agss::store;
use agss::timefmt::{self, FixedTz};
use agss::timesync::{self, Clock};
use agss::watch::{EventKind, WatchEvent};
use agss::{util, Ctx, Res};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use tempfile::TempDir;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

/// Horloge decalee : `skew` (horloge systeme fausse) ; `offset` = correction mesuree sur internet.
struct SkewClock {
    skew_ms: i64,
    offset_ms: AtomicI64,
}
impl Clock for SkewClock {
    fn system_ms(&self) -> u64 {
        (util::now_ms() as i64 + self.skew_ms) as u64
    }
    fn offset_ms(&self) -> i64 {
        self.offset_ms.load(Ordering::SeqCst)
    }
}

struct Pc {
    ctx: Ctx,
    game: Game,
}

fn pc(root: &Path, name: &str, game_name: &str, skew_ms: i64) -> Pc {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    ctx.clock = Box::new(SkewClock { skew_ms, offset_ms: AtomicI64::new(0) });
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: game_name.into(), path: root.join(format!("saves-{}", name)), exe: None, dest_key: None, ..Default::default() };
    Pc { ctx, game }
}

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}
fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// Empreinte de toute une arborescence : chemin relatif -> (hash, taille).
fn tree(dir: &Path) -> BTreeMap<String, (String, u64)> {
    let mut m = BTreeMap::new();
    for e in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_file() {
            let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            m.insert(rel, (util::hash_file(e.path()).unwrap(), e.metadata().unwrap().len()));
        }
    }
    m
}

fn newest(dest: &Path, game: &Game) -> PathBuf {
    backup::list_versions(dest, game).pop().expect("une sauvegarde")
}

/// Fabrique un dossier au format historique (meta.json + files/) tel que l'ancienne version de AutoGameSoftwareSync l'ecrivait.
fn legacy(parent: &Path, folder: &str, game: &str, machine: &str, created_ms: u64, content: &str) -> PathBuf {
    let dir = parent.join(folder);
    write(&dir, "files/save.dat", content);
    let (files, latest) = store::scan(&dir.join("files")).unwrap();
    let mut ctx = Ctx::new(parent.join("_ctx"));
    ctx.machine = machine.into();
    let mut m = store::make_meta(&ctx, game, files, latest, None);
    m.id = String::new(); // ancien format : pas d'identifiant
    m.format = 0;
    m.saved_at_ms = 0;
    m.saved_at = String::new();
    m.saved_at_local = String::new();
    m.created_ms = created_ms;
    m.created_at = util::timestamp(created_ms);
    write(&dir, "meta.json", &serde_json::to_string_pretty(&m).unwrap());
    dir
}

// ================================================================== noms de jeux

#[test]
fn variantes_de_nom_donnent_la_meme_cle() {
    for n in ["Field of Mistria", "field of mistria", "fieldofmistria", "FIELD OF MISTRIA", "Field-of_Mistria!", "  Field  of   Mistria ", "Field.of.Mistria"] {
        assert_eq!(cloud::game_key(n), "fieldofmistria", "{}", n);
    }
    assert_eq!(cloud::game_key("Pokémon Légendes: Arceus"), "pokemonlegendesarceus");
    assert_eq!(cloud::game_key("Ça va ? Œuvre"), "cavaoeuvre");
    assert_eq!(cloud::game_key("Hades II"), "hadesii");
    assert_eq!(cloud::game_key("2064: Read Only Memories"), "2064readonlymemories");
    assert_eq!(cloud::game_key("???"), "jeu", "nom sans caractere exploitable");
    assert_ne!(cloud::game_key("Hades"), cloud::game_key("Hades II"));
    // alias local : la cle d'ecriture suit l'alias
    let g = Game { name: "Mon Mistria".into(), path: PathBuf::new(), exe: None, dest_key: Some("Field of Mistria".into()), ..Default::default() };
    assert_eq!(cloud::dest_key(&g), "fieldofmistria");
}

#[test]
fn alerte_de_ressemblance() {
    let existing = vec![
        cloud::DestGame { key: "fieldofmistria".into(), name: "Field of Mistria".into(), dirs: vec![] },
        cloud::DestGame { key: "hades".into(), name: "Hades".into(), dirs: vec![] },
        cloud::DestGame { key: "stardewvalley".into(), name: "Stardew Valley".into(), dirs: vec![] },
    ];
    let s = cloud::similar_dest_games("Field of Mysteria", &existing);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].0.name, "Field of Mistria");
    assert!(!cloud::similar_dest_games("mysteria", &[cloud::DestGame { key: "mistria".into(), name: "Mistria".into(), dirs: vec![] }]).is_empty(), "mysteria / mistria");
    // mots dans un autre ordre
    assert_eq!(cloud::similar_dest_games("Valley Stardew", &existing).len(), 1);
    // meme cle => pas « ressemblant » (c'est exactement le meme jeu)
    assert!(cloud::similar_dest_games("field of mistria", &existing).is_empty());
    assert!(cloud::same_key_dest_game("FIELDOFMISTRIA", &existing).is_some());
    // jeux sans rapport
    assert!(cloud::similar_dest_games("Celeste", &existing).is_empty());
    assert!(cloud::similar_dest_games("Hollow Knight", &existing).is_empty());
    assert!(cloud::similarity("abc", "abc") == 1.0 && cloud::similarity("abcdef", "uvwxyz") < 0.3);
    assert_eq!(cloud::levenshtein("kitten", "sitting"), 3);
}

#[test]
fn liste_des_jeux_deja_presents_dans_la_destination() {
    let t = TempDir::new().unwrap();
    let dest = t.path();
    legacy(&dest.join("Field of Mistria"), "current", "Field of Mistria", "EPIKOU", 1_000_000, "a");
    // meme jeu sous une autre graphie, nouveau format + game.json
    std::fs::create_dir_all(dest.join("fieldofmistria/saves")).unwrap();
    std::fs::write(dest.join("fieldofmistria/game.json"), r#"{"name":"Field of Mistria","key":"fieldofmistria"}"#).unwrap();
    std::fs::create_dir_all(dest.join("hades/saves")).unwrap();
    std::fs::write(dest.join("hades/game.json"), r#"{"name":"Hades (GOG)","key":"hades"}"#).unwrap();
    std::fs::create_dir_all(dest.join("dossier-quelconque")).unwrap(); // pas un jeu
    std::fs::create_dir_all(dest.join(".tmp-xyz/saves")).unwrap(); // cache : ignore
    let l = cloud::list_dest_games(dest);
    let keys: Vec<&str> = l.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(keys, vec!["fieldofmistria", "hades"]);
    assert_eq!(l[0].dirs.len(), 2, "les deux graphies sont regroupees en un seul jeu");
    assert_eq!(l[1].name, "Hades (GOG)", "nom d'affichage d'origine (game.json)");
}

#[test]
fn alias_local_lier_a_un_jeu_existant_sans_rien_modifier_dans_la_destination() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    // L'autre PC a deja cree « fieldofmistria ».
    let b = pc(t.path(), "EPIGOS", "Field of Mistria", 0);
    write(&b.game.path, "save.dat", "de-B");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    // CE PC a un jeu nomme autrement, donc une autre cle.
    let mut a = pc(t.path(), "EPIKOU", "Mistria perso", 0);
    write(&a.game.path, "save.dat", "de-A");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    assert!(dest.join("mistriaperso/saves").is_dir() && dest.join("fieldofmistria/saves").is_dir());
    let before = tree(&dest);
    // Config locale + liaison.
    let mut cfg = agss::config::Config::default();
    cfg.games.push(a.game.clone());
    cfg.save(&a.ctx).unwrap();
    agss::actions::link_game(&a.ctx, "Mistria perso", "Field of Mistria").unwrap();
    a.game = agss::config::Config::load(&a.ctx).unwrap().games[0].clone();
    assert_eq!(a.game.dest_key.as_deref(), Some("fieldofmistria"));
    assert_eq!(tree(&dest), before, "lier ne modifie RIEN dans la destination");
    // Les deux historiques restent lisibles et les nouvelles sauvegardes vont dans le dossier choisi.
    let h = backup::history(&a.ctx, &a.game, &dest);
    let contents: Vec<String> = h.rows.iter().map(|r| r.machine.clone()).collect();
    assert!(contents.contains(&"EPIGOS".to_string()) && contents.contains(&"EPIKOU".to_string()), "{:?}", contents);
    write(&a.game.path, "save.dat", "de-A-v2");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    assert_eq!(std::fs::read_dir(dest.join("fieldofmistria/saves")).unwrap().count(), 2 + 0, "B + la nouvelle de A");
    assert_eq!(std::fs::read_dir(dest.join("mistriaperso/saves")).unwrap().count(), 1, "ancien dossier intact");
    // Deux jeux locaux de meme cle sont refuses.
    let t2 = Game { name: "MISTRIA PERSO".into(), path: a.game.path.clone(), exe: None, dest_key: None, ..Default::default() };
    assert!(agss::actions::add_game(&a.ctx, t2).is_err());
}

// ================================================================== disposition cloud-safe

#[test]
fn chaque_sauvegarde_est_un_dossier_unique_sans_renommage_ni_ecrasement() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let m = pc(t.path(), "EPIKOU", "Field of Mistria", 0);
    write(&m.game.path, "save.dat", "v1");
    write(&m.game.path, "sub/extra.dat", "x");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    let t1 = tree(&dest);
    assert!(t1.keys().any(|k| k.starts_with("fieldofmistria/saves/") && k.ends_with("/meta.json")));
    assert!(t1.contains_key("fieldofmistria/game.json"));
    for k in t1.keys() {
        assert!(!k.contains(".tmp-") && !k.ends_with(".agss-tmp") && !k.contains("current"), "{}", k);
    }
    // Le nom du dossier : horodatage + machine.
    let name = newest(&dest, &m.game).file_name().unwrap().to_string_lossy().to_string();
    assert!(name.ends_with("-EPIKOU") && util::parse_timestamp(&name).is_some(), "{}", name);
    for i in 2..=6 {
        write(&m.game.path, "save.dat", &format!("v{}", i));
        backup::backup(&m.ctx, &m.game, &dest).unwrap();
        let now = tree(&dest);
        // Tout ce qui existait existe encore, identique (aucun ecrasement, renommage ni suppression).
        for (k, v) in &t1 {
            assert_eq!(now.get(k), Some(v), "{} a ete modifie", k);
        }
    }
    // meta.json est ecrit EN DERNIER : jamais plus ancien qu'un fichier de sa sauvegarde.
    let dir = newest(&dest, &m.game);
    let meta_t = std::fs::metadata(dir.join("meta.json")).unwrap().modified().unwrap();
    for e in walkdir::WalkDir::new(dir.join("files")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        assert!(e.metadata().unwrap().modified().unwrap() <= meta_t);
    }
    // Rien d'autre dans le dossier local de travail.
    assert!(!m.ctx.data_dir.join("tmp").exists() || std::fs::read_dir(m.ctx.data_dir.join("tmp")).unwrap().next().is_none());
}

#[test]
fn meme_milliseconde_meme_machine_suffixe_aleatoire_sans_collision() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let m = pc(t.path(), "A", "G", 0);
    // Horloge figee : deux sauvegardes au meme instant.
    struct Frozen;
    impl Clock for Frozen {
        fn system_ms(&self) -> u64 {
            1_800_000_000_000
        }
        fn offset_ms(&self) -> i64 {
            0
        }
    }
    let mut m = m;
    m.ctx.clock = Box::new(Frozen);
    write(&m.game.path, "save.dat", "v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    write(&m.game.path, "save.dat", "v2");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    let v = backup::list_versions(&dest, &m.game);
    assert_eq!(v.len(), 2);
    assert_ne!(v[0], v[1]);
    assert_eq!(read(&v[0].join("files/save.dat")), "v1", "la premiere n'a pas ete ecrasee");
}

#[test]
fn echec_en_cours_d_ecriture_dans_la_destination_ne_laisse_aucun_dossier_incomplet() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let mut m = pc(t.path(), "A", "G", 0);
    write(&m.game.path, "save.dat", "v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    let before = tree(&dest);
    write(&m.game.path, "save.dat", "v2");
    m.ctx.fault_corrupt_copy = true;
    assert!(backup::backup(&m.ctx, &m.game, &dest).is_err());
    assert_eq!(tree(&dest), before, "destination strictement inchangee apres un echec");
}

// ================================================================== dossiers incomplets / .tmp / ancien format

#[test]
fn dossier_incomplet_ignore_signale_jamais_supprime() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let m = pc(t.path(), "A", "G", 0);
    write(&m.game.path, "save.dat", "v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    let saves = dest.join("g/saves");
    // (1) fichiers copies mais PAS de meta.json (Drive n'a pas fini) ; (2) meta.json present mais fichier manquant ;
    // (3) meta valide mais contenu altere a taille egale (hash faux).
    write(&saves, "29990101-000000-000-AUTRE/files/save.dat", "partiel");
    let good = newest(&dest, &m.game);
    let dir2 = saves.join("29990101-000001-000-AUTRE");
    write(&dir2, "meta.json", &read(&good.join("meta.json")));
    let dir3 = saves.join("29990101-000002-000-AUTRE");
    write(&dir3, "files/save.dat", "x1"); // meme taille que "v1" mais autre contenu
    write(&dir3, "meta.json", &read(&good.join("meta.json")));
    let before = tree(&dest);
    let h = backup::history(&m.ctx, &m.game, &dest);
    assert_eq!(h.incomplete.len(), 2, "{:?}", h.incomplete);
    assert_eq!(h.rows.len(), 2, "la bonne + celle au hash faux (detectee a la restauration)");
    // La restauration ignore les versions douteuses et prend la bonne (rien n'est modifie ailleurs).
    write(&m.game.path, "save.dat", "local-different");
    let r = backup::restore(&m.ctx, &m.game, &dest).unwrap();
    assert_eq!(read(&m.game.path.join("save.dat")), "v1");
    assert!(r.safety_dir.is_some());
    // Un backup + un elagage agressif ne suppriment jamais un dossier incomplet.
    write(&m.game.path, "save.dat", "v2");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    backup::prune_keep(&dest, &m.game, 0);
    for k in before.keys() {
        if k.contains("AUTRE") {
            assert!(dest.join(k).exists(), "{} supprime !", k);
        }
    }
}

#[test]
fn arborescence_reelle_de_l_utilisateur_current_copies_de_conflit_tmp_et_versions() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let root = dest.join("Field of Mistria");
    legacy(&root, "current", "Field of Mistria", "EPIKOU", 1_800_000_300_000, "etat-courant-lu-par-AutoGameSoftwareSync");
    legacy(&root, "current (1)", "Field of Mistria", "EPIGOS", 1_800_000_500_000, "etat-de-l-autre-PC");
    legacy(&root, "current (2)", "Field of Mistria", "EPIGOS", 1_800_000_700_000, "etat-le-plus-recent");
    write(&root, ".tmp-20261023-001136-166/files/save.dat", "reste-de-staging-synchronise");
    write(&root, ".tmp-20261023-001136-166/meta.json", "{pas fini");
    legacy(&root.join("versions"), "20261023-001136-166", "Field of Mistria", "EPIKOU", 1_800_000_100_000, "ancienne-1");
    legacy(&root.join("versions"), "20261022-235959-001", "Field of Mistria", "EPIGOS", 1_800_000_000_000, "ancienne-0");
    let before = tree(&dest);

    let mut a = pc(t.path(), "EPIKOU", "Field of Mistria", 0);
    write(&a.game.path, "save.dat", "mon-local");
    let snap = cloud::scan_dest(&dest, &a.game);
    assert_eq!(snap.versions.len(), 5, "current + 2 copies + 2 versions");
    assert_eq!(snap.tmp_dirs.len(), 1, ".tmp-* signale et ignore");
    assert!(snap.versions.iter().any(|v| v.kind == VKind::LegacyConflictCopy && v.name == "current (2)"));
    let h = backup::history(&a.ctx, &a.game, &dest);
    assert_eq!(h.rows.len(), 5);
    assert_eq!(h.tmp_dirs.len(), 1);
    assert!(h.rows.iter().all(|r| r.kind != VKind::New));
    // Les machines et dates de l'ancien format sont lisibles.
    assert!(h.rows.iter().any(|r| r.machine == "EPIGOS" && r.saved_at_ms == 1_800_000_700_000));
    // Restauration d'une copie de conflit, nom exact, memes protections (snapshot de securite).
    let r = backup::restore_version(&a.ctx, &a.game, &dest, "current (2)").unwrap();
    assert_eq!(read(&a.game.path.join("save.dat")), "etat-le-plus-recent");
    assert_eq!(read(&r.safety_dir.unwrap().join("files/save.dat")), "mon-local");
    // ... et d'une ancienne version.
    backup::restore_version(&a.ctx, &a.game, &dest, "versions/20261022-235959-001").unwrap();
    assert_eq!(read(&a.game.path.join("save.dat")), "ancienne-0");
    // Prochain backup : nouvelle sauvegarde dans saves/, aucune migration en place.
    write(&a.game.path, "save.dat", "nouvel-etat");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::prune_keep(&dest, &a.game, 0);
    let after = tree(&dest);
    for (k, v) in &before {
        assert_eq!(after.get(k), Some(v), "ancien format modifie ou supprime : {}", k);
    }
    assert!(after.keys().any(|k| k.starts_with("fieldofmistria/saves/")), "{:?}", after.keys().filter(|k| !before.contains_key(*k)).collect::<Vec<_>>());
    // Un jeu dont le nom differe seulement par la casse / les espaces lit le meme ancien format.
    a.game.name = "field of mistria".into();
    assert!(cloud::scan_dest(&dest, &a.game).versions.len() >= 5);
}

#[test]
fn premiere_synchro_avec_ancien_format_reste_prudente_puis_normale() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    legacy(&dest.join("G"), "current", "G", "OLD", 1_800_000_000_000, "ancien");
    let a = pc(t.path(), "A", "G", 0);
    write(&a.game.path, "save.dat", "different");
    // Jamais synchronise + contenu different => conflit (rien n'est restaure automatiquement).
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Conflict);
    // Local identique a l'ancien `current` => a jour.
    write(&a.game.path, "save.dat", "ancien");
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::InSync);
    assert!(matches!(backup::backup(&a.ctx, &a.game, &dest).unwrap(), BackupOutcome::UpToDate));
    // Puis le local evolue : sauvegarde normale (la tete = ce sur quoi on se base).
    write(&a.game.path, "save.dat", "nouveau");
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Backup);
    assert!(matches!(backup::backup(&a.ctx, &a.game, &dest).unwrap(), BackupOutcome::Backed { .. }));
    // Le second PC (meme ancien etat connu) restaure simplement.
    // Cas reel : le 2e PC a un ancien state.json (derniere synchro = l'ancien `current`) et n'a rien change.
    let b = pc(t.path(), "B", "G", 0);
    write(&b.game.path, "save.dat", "ancien");
    let (f, _) = store::scan(&b.game.path).unwrap();
    let mut st = agss::config::State::default();
    st.last_synced.insert("G".into(), store::global_hash(&f));
    st.save(&b.ctx).unwrap();
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::Restore);
    // Meme sans state.json : un local identique a l'ancien `current`, dont descend la tete du cloud, est un ancetre inchange.
    let c = pc(t.path(), "C", "G", 0);
    write(&c.game.path, "save.dat", "ancien");
    assert_eq!(backup::plan(&c.ctx, &c.game, &dest).unwrap(), Plan::Restore);
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    assert_eq!(read(&b.game.path.join("save.dat")), "nouveau");
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::InSync);
}

// ================================================================== chaine de versions (decision pure)

fn node(id: &str, hash: &str, anc: &[&str]) -> VNode {
    VNode { id: id.into(), hash: hash.into(), ancestors: anc.iter().map(|s| s.to_string()).collect(), sort_ms: 0 }
}

#[test]
fn decision_par_la_chaine_et_non_par_l_horloge() {
    let v1 = node("1", "h1", &[]);
    let v2 = node("2", "h2", &["1"]);
    let v3 = node("3", "h3", &["2", "1"]);
    // Cloud : 1 <- 2 <- 3
    let chain = vec![v1.clone(), v2.clone(), v3.clone()];
    assert_eq!(heads(&chain).len(), 1);
    assert_eq!(decide(Some("h3"), Some("h3"), Some("3"), &chain, &[]), Plan::InSync);
    assert_eq!(decide(Some("h1"), Some("h1"), Some("1"), &chain, &[]), Plan::Restore, "local = ancetre inchange : cloud plus recent");
    assert_eq!(decide(Some("hx"), Some("h3"), Some("3"), &chain, &[]), Plan::Backup, "local modifie, cloud inchange");
    assert_eq!(decide(Some("hx"), Some("h1"), Some("1"), &chain, &[]), Plan::Conflict, "les deux ont evolue");
    assert_eq!(decide(Some("hx"), None, None, &chain, &[]), Plan::Conflict, "jamais synchronise");
    assert_eq!(decide(None, Some("h3"), Some("3"), &chain, &[]), Plan::Restore, "local absent");
    assert_eq!(decide(Some("hx"), None, None, &[], &[]), Plan::Backup);
    assert_eq!(decide(None, None, None, &[], &[]), Plan::Nothing);
    // Branches : 3 et 3b descendent de 2 => deux tetes.
    let v3b = node("3b", "h3b", &["2", "1"]);
    let fork = vec![v1.clone(), v2.clone(), v3.clone(), v3b.clone()];
    assert_eq!(heads(&fork).len(), 2);
    assert_eq!(decide(Some("h3"), Some("h3"), Some("3"), &fork, &[]), Plan::Conflict, "local = une des deux branches : conflit non resolu");
    assert_eq!(decide(Some("hz"), Some("h2"), Some("2"), &fork, &[]), Plan::Conflict);
    assert_eq!(decide(Some("h2"), Some("h2"), Some("2"), &fork, &[]), Plan::Conflict, "jamais de restauration automatique entre branches");
    // Fusion (resolution) : 4 descend des deux tetes.
    let v4 = node("4", "h4", &["3", "3b", "2", "1"]);
    let merged = vec![v1.clone(), v2.clone(), v3.clone(), v3b.clone(), v4.clone()];
    assert_eq!(heads(&merged).len(), 1);
    assert_eq!(decide(Some("h4"), Some("h4"), Some("4"), &merged, &[]), Plan::InSync);
    assert_eq!(decide(Some("h3"), Some("h3"), Some("3"), &merged, &[]), Plan::Restore);
    // Deux PC ont ecrit le MEME contenu independamment : pas de conflit.
    let same = vec![node("a", "hs", &[]), node("b", "hs", &[])];
    assert_eq!(decide(Some("hs"), None, None, &same, &[]), Plan::InSync);
    // Ancetre elague : la filiation (lineage) survit, ce n'est pas un conflit.
    let pruned = vec![node("9", "h9", &["8", "7", "1"])];
    assert_eq!(decide(Some("hx"), Some("h1"), Some("1"), &pruned, &[]), Plan::Conflict, "base absente de la chaine ET pas la tete");
    assert_eq!(decide(Some("h1"), Some("h1"), Some("1"), &pruned, &[]), Plan::Restore);
}

// ================================================================== deux PC (horloges decalees, ecritures concurrentes)

#[test]
fn deux_pc_horloges_decalees_de_3_minutes_la_chaine_decide_pas_l_horloge() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    // A est en AVANCE de 3 min, B en RETARD de 3 min.
    let a = pc(t.path(), "EPIKOU", "Field of Mistria", 180_000);
    let b = pc(t.path(), "EPIGOS", "field of mistria", -180_000);
    write(&a.game.path, "save.dat", "v1-A");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::InSync);
    write(&b.game.path, "save.dat", "v2-B");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    // L'horodatage de B (v2) est ANTERIEUR a celui de A (v1) a cause du decalage : A restaure quand meme v2.
    let v: Vec<_> = cloud::scan_dest(&dest, &a.game).new_versions().into_iter().map(|v| (v.meta.machine.clone(), v.sort_ms())).collect();
    assert_eq!(v.len(), 2);
    let ts_a = v.iter().find(|x| x.0 == "EPIKOU").unwrap().1;
    let ts_b = v.iter().find(|x| x.0 == "EPIGOS").unwrap().1;
    assert!(ts_b < ts_a, "le decalage d'horloge inverse bien l'ordre des dates ({} < {})", ts_b, ts_a);
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Restore);
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::InSync);
    backup::restore(&a.ctx, &a.game, &dest).unwrap();
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-B", "la bonne sauvegarde est recuperee malgre les horloges");
    // Meme jeu, meme dossier : une seule cle malgre « Field of Mistria » / « field of mistria ».
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1);
}

#[test]
fn heure_corrigee_par_internet_saved_at_et_trait_horloge() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    // Horloge systeme en avance de 3 min ; l'ecart mesure (-180 s) la corrige.
    let a = pc(t.path(), "A", "G", 180_000);
    write(&a.game.path, "save.dat", "v1");
    let real_before = util::now_ms();
    // Sans correction : saved_at suit l'horloge fausse.
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    let m0 = backup::active_meta(&dest, &a.game).unwrap();
    assert!(m0.saved_at_ms >= real_before + 170_000, "horloge systeme utilisee hors ligne");
    // Avec correction.
    let a2 = {
        let mut p = pc(t.path(), "A2", "G", 180_000);
        p.ctx.clock = Box::new(SkewClock { skew_ms: 180_000, offset_ms: AtomicI64::new(-180_000) });
        p
    };
    write(&a2.game.path, "save.dat", "v2");
    backup::backup(&a2.ctx, &a2.game, &dest).unwrap();
    let d2 = backup::list_versions(&dest, &a2.game).into_iter().find(|p| p.file_name().unwrap().to_string_lossy().ends_with("-A2")).unwrap();
    let m1 = store::read_meta(&d2.join("meta.json")).unwrap();
    assert!((m1.saved_at_ms as i64 - util::now_ms() as i64).abs() < 5_000, "heure corrigee ({} ms d'ecart)", m1.saved_at_ms as i64 - util::now_ms() as i64);
    assert_eq!(m1.clock_offset_ms, -180_000);
    assert!(!m1.saved_at_local.is_empty() && m1.saved_at_local.contains('T'));
    assert_eq!(m1.format, 2);
    assert!(!m1.id.is_empty() && m1.parent_id.is_none());
}

#[test]
fn ecritures_concurrentes_de_deux_pc_avec_latence_aucune_collision_ni_ecrasement() {
    for round in 0..4u64 {
        let t = TempDir::new().unwrap();
        let dest = t.path().join("cloud");
        let a = pc(t.path(), "EPIKOU", "G", 0);
        let b = pc(t.path(), "EPIGOS", "G", 0);
        write(&a.game.path, "save.dat", &format!("A{}", round));
        write(&b.game.path, "save.dat", &format!("B{}", round));
        let barrier = Arc::new(Barrier::new(2));
        let seed = Arc::new(AtomicU64::new(0x9E3779B97F4A7C15 ^ round));
        let rnd = |s: &AtomicU64| {
            let mut x = s.load(Ordering::SeqCst);
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.store(x, Ordering::SeqCst);
            x
        };
        let (d1, d2) = (dest.clone(), dest.clone());
        let (b1, b2) = (barrier.clone(), barrier.clone());
        let (l1, l2) = (rnd(&seed) % 30, rnd(&seed) % 30);
        let (ra, rb) = std::thread::scope(|s| {
            let root = t.path().to_path_buf();
            let r2 = root.clone();
            let ha = s.spawn(move || {
                let p = pc(&root, "EPIKOU", "G", 0); // Ctx propre au thread (il n'est pas Sync)
                b1.wait();
                std::thread::sleep(std::time::Duration::from_millis(l1)); // latence d'arrivee aleatoire
                backup::backup(&p.ctx, &p.game, &d1).map(|_| ()).map_err(|e| e.to_string())
            });
            let hb = s.spawn(move || {
                let p = pc(&r2, "EPIGOS", "G", 0);
                b2.wait();
                std::thread::sleep(std::time::Duration::from_millis(l2));
                backup::backup(&p.ctx, &p.game, &d2).map(|_| ()).map_err(|e| e.to_string())
            });
            (ha.join().unwrap(), hb.join().unwrap())
        });
        ra.unwrap();
        rb.unwrap();
        let v = backup::list_versions(&dest, &a.game);
        assert_eq!(v.len(), 2, "chaque PC a son dossier unique (round {})", round);
        let machines: Vec<String> = v.iter().map(|p| store::read_meta(&p.join("meta.json")).unwrap().machine).collect();
        assert!(machines.contains(&"EPIKOU".to_string()) && machines.contains(&"EPIGOS".to_string()));
        // Deux branches independantes (ni l'une ni l'autre ne se base sur l'autre) : VRAI conflit, rien perdu.
        let h = backup::history(&a.ctx, &a.game, &dest);
        assert_eq!(h.conflict_heads.len(), 2, "{:?}", h.conflict_heads);
        assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Conflict);
        assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::Conflict);
        assert_eq!(read(&a.game.path.join("save.dat")), format!("A{}", round), "local jamais ecrase");
    }
}

#[test]
fn resolution_garder_la_mienne_les_deux_versions_restent_une_seule_tete() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = pc(t.path(), "EPIKOU", "G", 0);
    let b = pc(t.path(), "EPIGOS", "G", 0);
    write(&a.game.path, "save.dat", "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    write(&b.game.path, "save.dat", "v2-B");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    write(&a.game.path, "save.dat", "v2-A"); // A n'a pas vu v2-B
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Conflict);
    let out = backup::backup(&a.ctx, &a.game, &dest).unwrap();
    assert!(matches!(out, BackupOutcome::Conflict { .. }));
    // Resolution : A garde la sienne.
    let before = tree(&dest);
    let r = backup::resolve_conflict(&a.ctx, &a.game, &dest, true).unwrap();
    assert!(r.restored.is_none());
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-A");
    let all: Vec<String> = backup::list_versions(&dest, &a.game).iter().map(|v| read(&v.join("files/save.dat"))).collect();
    for want in ["v1", "v2-A", "v2-B"] {
        assert!(all.contains(&want.to_string()), "{} perdu : {:?}", want, all);
    }
    for (k, v) in &before {
        assert_eq!(tree(&dest).get(k), Some(v), "{} modifie", k);
    }
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::InSync);
    // B voit simplement une version plus recente (pas un nouveau conflit).
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::Restore);
    assert!(backup::history(&b.ctx, &b.game, &dest).conflict_heads.is_empty());
    assert!(backup::resolve_conflict(&a.ctx, &a.game, &dest, true).is_err(), "plus de conflit a resoudre");
}

#[test]
fn resolution_restaurer_le_cloud_snapshot_securite_et_version_locale_conservee() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let a = pc(t.path(), "EPIKOU", "G", 0);
    let b = pc(t.path(), "EPIGOS", "G", 0);
    write(&a.game.path, "save.dat", "v1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    write(&b.game.path, "save.dat", "v2-B");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    write(&a.game.path, "save.dat", "v2-A-jamais-sauvegarde");
    let r = backup::resolve_conflict(&a.ctx, &a.game, &dest, false).unwrap();
    assert_eq!(read(&a.game.path.join("save.dat")), "v2-B");
    let safety = r.restored.unwrap().safety_dir.expect("snapshot de securite");
    assert_eq!(read(&safety.join("files/save.dat")), "v2-A-jamais-sauvegarde");
    let all: Vec<String> = backup::list_versions(&dest, &a.game).iter().map(|v| read(&v.join("files/save.dat"))).collect();
    assert!(all.contains(&"v2-A-jamais-sauvegarde".to_string()), "la version locale est aussi conservee dans le cloud : {:?}", all);
    assert!(all.contains(&"v2-B".to_string()) && all.contains(&"v1".to_string()));
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::InSync);
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::InSync, "B a deja le meme contenu que la version du cloud conservee");
}

#[test]
fn elagage_ne_touche_que_des_dossiers_conformes_et_ne_supprime_jamais_une_tete() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("cloud");
    let mut a = pc(t.path(), "EPIKOU", "G", 0);
    a.ctx.keep_versions = 0;
    let b = pc(t.path(), "EPIGOS", "G", 0);
    write(&b.game.path, "save.dat", "branche-B");
    backup::backup(&b.ctx, &b.game, &dest).unwrap(); // une tete, ancienne
    for i in 0..8 {
        write(&a.game.path, "save.dat", &format!("a{}", i));
        backup::backup(&a.ctx, &a.game, &dest).unwrap();
    }
    // Dossiers etrangers et ancien format a cote.
    write(&dest.join("g/saves"), "notes-perso/fichier.txt", "important");
    legacy(&dest.join("g"), "current", "G", "OLD", 1, "ancien");
    let strangers = tree(&dest.join("g/saves/notes-perso"));
    let legacy_before = tree(&dest.join("g/current"));
    backup::prune_keep(&dest, &a.game, 0);
    let all: Vec<String> = backup::list_versions(&dest, &a.game).iter().map(|v| read(&v.join("files/save.dat"))).collect();
    assert!(all.contains(&"branche-B".to_string()), "une tete (branche non resolue) n'est jamais elaguee : {:?}", all);
    assert!(all.contains(&"a7".to_string()) && all.contains(&"a6".to_string()) && all.contains(&"a5".to_string()), "active + 2 anciennes : {:?}", all);
    assert!(all.len() <= 4, "{:?}", all);
    assert_eq!(tree(&dest.join("g/saves/notes-perso")), strangers);
    assert_eq!(tree(&dest.join("g/current")), legacy_before);
}

#[test]
fn nettoyage_des_dossiers_de_travail_locaux_orphelins_seulement_dans_tmp() {
    let t = TempDir::new().unwrap();
    let m = pc(t.path(), "A", "G", 0);
    let tmp = m.ctx.data_dir.join("tmp");
    write(&tmp, "orphelin/files/x.dat", "x");
    write(&m.ctx.data_dir, "config.json", "{}");
    write(&m.ctx.data_dir, "safety/G/20260101/files/s.dat", "precieux");
    let outside = t.path().join("autre-dossier-a-moi");
    write(&outside, "f.txt", "a moi");
    // Trop recent : conserve (un autre processus peut l'utiliser).
    assert_eq!(backup::cleanup_local_tmp(&m.ctx, std::time::Duration::from_secs(600)), 0);
    assert!(tmp.join("orphelin").exists());
    // Orphelin : supprime, et seulement lui.
    assert_eq!(backup::cleanup_local_tmp(&m.ctx, std::time::Duration::from_secs(0)), 1);
    assert!(!tmp.join("orphelin").exists());
    assert!(m.ctx.data_dir.join("config.json").exists());
    assert!(m.ctx.data_dir.join("safety/G/20260101/files/s.dat").exists(), "les snapshots de securite ne sont jamais nettoyes");
    assert!(outside.join("f.txt").exists());
}

// ================================================================== dates lisibles / horloge

#[test]
fn dates_lisibles_en_heure_locale_selon_la_langue() {
    // 2027-12-31 01:24:15 UTC
    let utc = util::ms_from_civil(2027, 12, 31, 1, 24, 15, 0) as u64;
    assert_eq!(timefmt::format_local(utc, Lang::Fr, &FixedTz(3600)), "31/12/2027 02:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::En, &FixedTz(3600)), "2027-12-31 02:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::Fr, &FixedTz(0)), "31/12/2027 01:24:15");
    // Passage de minuit dans les deux sens (fuseaux negatifs et positifs).
    assert_eq!(timefmt::format_local(utc, Lang::En, &FixedTz(2 * 3600)), "2027-12-31 03:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::En, &FixedTz(-5 * 3600)), "2027-12-30 20:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::Fr, &FixedTz(23 * 3600)), "01/01/2028 00:24:15");
    // Heure d'ete simulee par un fournisseur de fuseau : +1 h l'hiver, +2 h l'ete.
    struct Paris;
    impl timefmt::TzProvider for Paris {
        fn utc_offset_secs(&self, ms: u64) -> i32 {
            let (_, m, _, _, _, _, _) = util::civil_from_ms(ms as i64);
            if (4..=10).contains(&m) { 7200 } else { 3600 }
        }
    }
    let ete = util::ms_from_civil(2027, 7, 1, 12, 0, 0, 0) as u64;
    assert_eq!(timefmt::format_local(ete, Lang::Fr, &Paris), "01/07/2027 14:00:00");
    assert_eq!(timefmt::format_local(utc, Lang::Fr, &Paris), "31/12/2027 02:24:15");
    assert_eq!(timefmt::iso_local(utc, &FixedTz(3600)), "2027-12-31T02:24:15+01:00");
    assert_eq!(timefmt::iso_local(utc, &FixedTz(-5 * 3600 - 1800)), "2027-12-30T19:54:15-05:30");
    // Noms de version et horodatages de journal.
    assert_eq!(timefmt::version_label("20271231-012415-000-EPIKOU", Lang::Fr, &FixedTz(3600)), "31/12/2027 02:24:15");
    assert_eq!(timefmt::version_label("versions/20271231-012415-000", Lang::En, &FixedTz(3600)), "2027-12-31 02:24:15");
    assert_eq!(timefmt::version_label("current (1)", Lang::Fr, &FixedTz(0)), "current (1)");
    assert_eq!(timefmt::log_ts_label("20271231-012415-123", Lang::Fr, &FixedTz(3600)), "31/12/2027 02:24:15");
    assert_eq!(timefmt::log_ts_label("pas-un-horodatage", Lang::Fr, &FixedTz(3600)), "pas-un-horodatage");
    // Aller-retour calendrier.
    for ms in [0i64, 951_782_400_000, 1_700_000_000_123, 4_102_444_799_999] {
        let (y, mo, d, h, mi, s, x) = util::civil_from_ms(ms);
        assert_eq!(util::ms_from_civil(y, mo, d, h, mi, s, x), ms);
        assert_eq!(util::parse_timestamp(&util::timestamp(ms as u64)), Some(ms as u64));
    }
    assert_eq!(util::parse_timestamp("20271331-000000-000"), None);
    assert_eq!(util::parse_timestamp("n'importe quoi"), None);
    // Le fuseau reel de la machine donne un decalage plausible (-14 h .. +14 h).
    use agss::timefmt::TzProvider;
    let off = timefmt::WinTz.utc_offset_secs(utc);
    assert!(off.abs() <= 14 * 3600, "{}", off);
}

#[test]
fn mesure_de_l_horloge_ntp_http_et_hors_ligne() {
    // Reponse NTP construite : serveur en AVANCE de 5 s sur le client, aller-retour de 100 ms.
    let t1 = 1_800_000_000_000u64;
    let t4 = t1 + 100;
    let server_now = t1 + 50 + 5_000; // instant (serveur) au milieu de l'echange
    let mut resp = [0u8; 48];
    resp[0] = 0x24; // LI=0, VN=4, mode=4
    resp[1] = 2; // strate 2
    let ts = |ms: u64| {
        let secs = ms / 1000 + 2_208_988_800;
        let frac = ((ms % 1000) << 32) / 1000;
        let mut o = [0u8; 8];
        o[..4].copy_from_slice(&(secs as u32).to_be_bytes());
        o[4..].copy_from_slice(&(frac as u32).to_be_bytes());
        o
    };
    resp[32..40].copy_from_slice(&ts(server_now));
    resp[40..48].copy_from_slice(&ts(server_now));
    let off = timesync::ntp_offset_ms(t1, t4, &resp).unwrap();
    assert!((off - 5_000).abs() <= 2, "{}", off);
    // Reponses invalides.
    assert!(timesync::ntp_offset_ms(t1, t4, &resp[..40]).is_none());
    let mut bad = resp;
    bad[1] = 0; // strate 0 = « kiss of death »
    assert!(timesync::ntp_offset_ms(t1, t4, &bad).is_none());
    let mut bad = resp;
    bad[0] = 0x23; // mode client, pas serveur
    assert!(timesync::ntp_offset_ms(t1, t4, &bad).is_none());
    let mut bad = resp;
    bad[40..48].fill(0);
    assert!(timesync::ntp_offset_ms(t1, t4, &bad).is_none());
    // Paquet de requete bien forme.
    let rq = timesync::ntp_request(t1);
    assert_eq!(rq[0], 0x1B);
    assert_eq!(rq.len(), 48);
    // En-tete HTTP Date.
    assert_eq!(timesync::parse_http_date("Tue, 15 Nov 1994 08:12:31 GMT"), Some(784_887_151_000));
    assert_eq!(timesync::parse_http_date("Fri, 31 Dec 2027 01:24:15 GMT"), Some(util::ms_from_civil(2027, 12, 31, 1, 24, 15, 0) as u64));
    assert_eq!(timesync::parse_http_date("n'importe quoi"), None);
    assert_eq!(timesync::parse_http_date("Tue, 15 Nov 1994 08:12:31 PST"), None);
    // Source simulee : succes puis hors ligne (repli sur l'horloge systeme + ligne de journal).
    struct Ok42;
    impl timesync::TimeSource for Ok42 {
        fn measure(&self) -> Result<(i64, String), String> {
            Ok((-42_000, "simulee".into()))
        }
    }
    struct Down;
    impl timesync::TimeSource for Down {
        fn measure(&self) -> Result<(i64, String), String> {
            Err("injoignable".into())
        }
    }
    let line = timesync::sync_once(&Ok42);
    assert!(line.contains("-42000") && line.contains("simulee"), "{}", line);
    assert_eq!(timesync::SystemClock.offset_ms(), -42_000);
    assert!(timesync::offset_known());
    let line = timesync::sync_once(&Down);
    assert!(line.contains("hors ligne") && !line.to_uppercase().contains("ERREUR"), "{}", line);
    assert_eq!(timesync::SystemClock.offset_ms(), -42_000, "l'ecart memorise est conserve hors ligne");
    timesync::set_offset(0); // ne pas polluer les autres tests du processus
}

// ================================================================== notifications (abstraction)

struct Rec(Mutex<Vec<(String, String)>>);
impl Notifier for Rec {
    fn notify(&self, title: &str, body: &str) {
        self.0.lock().unwrap().push((title.to_string(), body.to_string()));
    }
}

#[test]
fn notifications_sans_powershell_reglages_global_et_par_jeu_fr_en() {
    let t = TempDir::new().unwrap();
    let dd = t.path();
    let n = Rec(Mutex::new(vec![]));
    let ev = |k: EventKind, g: &str, d: &str| WatchEvent { game: g.into(), kind: k, detail: d.into() };
    assert!(notify::deliver(&n, dd, Lang::Fr, &ev(EventKind::BackedUp, "G", "")));
    assert!(notify::deliver(&n, dd, Lang::En, &ev(EventKind::Conflict, "G", "")));
    assert!(notify::deliver(&n, dd, Lang::Fr, &ev(EventKind::Error, "G", "disque plein")));
    {
        let l = n.0.lock().unwrap();
        assert_eq!(l[0], ("Copie faite : à l'abri dans ton coffre".to_string(), "G".to_string()));
        assert_eq!(l[1].0, "Two devices changed it at the same time: both versions are kept");
        assert_eq!(l[2], ("Problème pendant la copie".to_string(), "G : disque plein".to_string()));
    }
    // Reglage global desactive.
    let mut s = agss::settings::Settings::default();
    s.notify_global = false;
    std::fs::write(dd.join("settings.json"), serde_json::to_string(&s).unwrap()).unwrap();
    assert!(!notify::deliver(&n, dd, Lang::Fr, &ev(EventKind::BackedUp, "G", "")));
    // Reglage par jeu : « oui » pour G seulement.
    s.game_mut("G").notify = Some(true);
    std::fs::write(dd.join("settings.json"), serde_json::to_string(&s).unwrap()).unwrap();
    assert!(notify::deliver(&n, dd, Lang::Fr, &ev(EventKind::Restored, "G", "")));
    assert!(!notify::deliver(&n, dd, Lang::Fr, &ev(EventKind::Restored, "H", "")));
    assert_eq!(n.0.lock().unwrap().len(), 4);
    // Plus aucun PowerShell dans le code (le scan de Command::new de v04 reste en vigueur).
    for f in ["winutil.rs", "notify.rs", "gui.rs", "main.rs"] {
        let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(f)).unwrap();
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        assert!(!code.to_lowercase().contains("powershell"), "{} mentionne encore PowerShell", f);
    }
}
