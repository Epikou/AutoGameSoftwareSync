//! Scan du PC sur un PC SIMULE (dossier temporaire) : classement, liste noire, deja protege, nouveautes, ajout en lot,
//! annulation, tailles.
use super::*;
use crate::gamedb::Db;
use crate::profiles::{Detection, ProfLocation};
use std::time::{Duration, SystemTime};

const DAY: u64 = 86_400_000;

/// Dossier temporaire HORS du dossier temporaire du systeme (qui est un dossier « outils de developpement » pour le
/// classement : tout y serait masque).
fn tmp() -> tempfile::TempDir {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("scan-tests");
    std::fs::create_dir_all(&d).unwrap();
    tempfile::Builder::new().prefix("pc").tempdir_in(d).unwrap()
}

fn file(p: &Path, bytes: usize) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, vec![b'x'; bytes]).unwrap();
}

fn age(p: &Path, days: u64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_millis(days * DAY)).unwrap();
}

struct Pc {
    _t: tempfile::TempDir,
    root: PathBuf,
    f: Folders,
}

impl Pc {
    fn new() -> Pc {
        let t = tmp();
        let root = t.path().to_path_buf();
        let f = Folders::under(&root);
        Pc { _t: t, root, f }
    }
    fn user(&self) -> PathBuf {
        self.f.profile.clone().unwrap()
    }
}

fn vars_of(f: &Folders) -> Vars {
    Vars {
        kf: crate::hints::KnownFolders { local_app_data: f.local.clone(), roaming_app_data: f.roaming.clone(), local_low: f.locallow.clone(), documents: f.documents.clone(), saved_games: f.saved_games.clone() },
        user_profile: f.profile.clone(),
        ..Default::default()
    }
}

fn profile(id: &str, name: &str, cat: &str, locs: &[(&str, &str)]) -> Profile {
    Profile {
        schema: 1,
        id: id.into(),
        name: name.into(),
        category: cat.into(),
        version: "1".into(),
        author: "t".into(),
        license: "CC0-1.0".into(),
        detection: Detection { exes: vec![format!("{}.exe", id)], paths: vec![locs[0].1.to_string()] },
        locations: locs.iter().map(|(l, p)| ProfLocation { label: l.to_string(), path: p.to_string(), default_enabled: true, ..Default::default() }).collect(),
        ..Default::default()
    }
}

fn dbg(name: &str, steam: Option<u64>, paths: &[&str]) -> DbGame {
    DbGame { name: name.into(), steam, paths: paths.iter().map(|s| s.to_string()).collect(), ..Default::default() }
}

/// PC simule : jeux Steam (sauvegardes recentes, anciennes, absentes), titre de la base non installe, profils, logiciels
/// (outils de developpement, navigateur, systeme, lanceur, logiciel deduit), dossiers perso, raccourcis, fond d'ecran.
fn sim_env(pc: &Pc, data: &Path, games: Vec<Game>) -> ScanEnv {
    let u = pc.user();
    let steam = pc.root.join("Steam").join("steamapps").join("common");
    // Jeux Steam
    file(&steam.join("Hollow Knight").join("hollow_knight.exe"), 10);
    file(&u.join("AppData/LocalLow/Team Cherry/Hollow Knight/user1.dat"), 2000);
    file(&steam.join("Old Game").join("old.exe"), 10);
    let old = u.join("AppData/Roaming/OldGame/save.sav");
    file(&old, 100);
    age(&old, 400);
    file(&steam.join("No Save Game").join("nosave.exe"), 10);
    // Titre de la base non installe (sauvegardes seulement), et bruit
    file(&u.join("AppData/Roaming/StardewValley/Saves/Farm_1/Farm_1"), 500);
    file(&u.join("AppData/Roaming/Microsoft/Thing/x.dat"), 5);
    file(&u.join(".cargo/registry/index.json"), 5);
    file(&u.join("AppData/Roaming/Mozilla/Firefox/Profiles/a/logins.json"), 5);
    file(&u.join("AppData/Roaming/FoxGame/.ssh/id_rsa"), 5);
    // Profils : logiciel a deux emplacements, navigateur interdit
    file(&u.join("AppData/Roaming/Notepad++/config.xml"), 50);
    file(&u.join("AppData/Roaming/Notepad++/shortcuts.xml"), 50);
    file(&u.join("AppData/Roaming/Notepad++/plugins/config/p.ini"), 20);
    // Logiciel installe : reglages deduits par leur nom
    file(&u.join("AppData/Roaming/paint.net/settings.xml"), 30);
    // Dossiers perso
    file(&u.join("Documents/rapport.docx"), 4000);
    std::fs::create_dir_all(u.join("Pictures")).unwrap();
    file(&u.join("Desktop/raccourci.lnk"), 3);
    file(&u.join("Desktop/note.txt"), 3);
    file(&u.join("AppData/Roaming/Microsoft/Windows/Themes/TranscodedWallpaper"), 900);
    let installed = vec![
        Installed { name: "Hollow Knight".into(), store: Store::Steam, id: "367520".into(), install_dir: steam.join("Hollow Knight"), exe: Some("hollow_knight.exe".into()) },
        Installed { name: "Old Game".into(), store: Store::Steam, id: "1".into(), install_dir: steam.join("Old Game"), exe: Some("old.exe".into()) },
        Installed { name: "No Save Game".into(), store: Store::Steam, id: "2".into(), install_dir: steam.join("No Save Game"), exe: Some("nosave.exe".into()) },
    ];
    let apps = vec![
        AppEntry { name: "Git".into(), publisher: Some("The Git Development Community".into()), location: Some(pc.root.join("Program Files/Git")), ..Default::default() },
        AppEntry { name: "Rust 1.80 (MSVC 64-bit)".into(), publisher: Some("The Rust Project Developers".into()), ..Default::default() },
        AppEntry { name: "Python 3.12.1 (64-bit)".into(), publisher: Some("Python Software Foundation".into()), ..Default::default() },
        AppEntry { name: "Google Chrome".into(), publisher: Some("Google LLC".into()), ..Default::default() },
        AppEntry { name: "Microsoft Visual C++ 2015-2022 Redistributable (x64)".into(), publisher: Some("Microsoft Corporation".into()), ..Default::default() },
        AppEntry { name: "Steam".into(), publisher: Some("Valve Corporation".into()), ..Default::default() },
        AppEntry { name: "paint.net".into(), publisher: Some("dotPDN LLC".into()), ..Default::default() },
        AppEntry { name: "Logiciel Inconnu".into(), publisher: None, ..Default::default() },
    ];
    let db = GameDb::from_db(
        Db {
            format: 1,
            generated: "t".into(),
            games: vec![
                dbg("Hollow Knight", Some(367520), &["<home>/AppData/LocalLow/Team Cherry/Hollow Knight"]),
                dbg("Old Game", Some(1), &["<winAppData>/OldGame"]),
                dbg("Stardew Valley", None, &["<winAppData>/StardewValley/Saves"]),
                dbg("Thing", None, &["<winAppData>/Microsoft/Thing"]),
                dbg("Cargo Quest", None, &["<home>/.cargo/registry"]),
                dbg("Fox Game", None, &["<winAppData>/FoxGame/.ssh"]),
            ],
        },
        vec![],
    );
    let folders = pc.f.clone();
    let lf = folders.clone();
    let vault = pc.root.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    ScanEnv {
        data_dir: data.to_path_buf(),
        personal: vec![(Personal::Documents, u.join("Documents")), (Personal::Pictures, u.join("Pictures")), (Personal::Desktop, u.join("Desktop"))],
        system: vec![
            (SysItem::DesktopShortcuts, u.join("Desktop"), false, Some("*.lnk;*.url".into())),
            (SysItem::Wallpaper, u.join("AppData/Roaming/Microsoft/Windows/Themes/TranscodedWallpaper"), true, None),
            (SysItem::RegistryPrefs, PathBuf::new(), false, None),
        ],
        prefs: vec![],
        launcher_games: Box::new(move || installed.clone()),
        apps: Box::new(move || apps.clone()),
        steam_libs: vec![pc.root.join("Steam")],
        profiles: vec![
            profile("notepad-plus-plus", "Notepad++", "app", &[("Settings", "%APPDATA%\\Notepad++"), ("Plugins", "%APPDATA%\\Notepad++\\plugins\\config")]),
            profile("firefox-fake", "Fox Browser", "app", &[("Profile", "%APPDATA%\\Mozilla\\Firefox\\Profiles")]),
        ],
        vars: vars_of(&folders),
        db: Arc::new(db),
        locate: Box::new(move |p: &Program| crate::knowledge::dedupe(crate::knowledge::deduce::find(p, &lf, crate::util::now_ms(), &crate::knowledge::deduce::NoRegistry))),
        folders,
        games,
        vault: Some(vault),
        last: LastScan::load(data),
        now_ms: crate::util::now_ms(),
        db_probe: true,
        prefix_of: Box::new(|_| None),
    }
}

fn scan(env: &ScanEnv) -> Progress {
    let h = Handle { state: Arc::new(Mutex::new(Progress::default())), cancel: Arc::new(AtomicBool::new(false)) };
    run(env, &h, &|| {});
    h.view()
}

fn find<'a>(p: &'a Progress, name: &str) -> &'a Item {
    p.items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{} absent : {:?}", name, p.items.iter().map(|i| &i.name).collect::<Vec<_>>()))
}

#[test]
fn classement_par_score_de_confiance() {
    let pc = Pc::new();
    let data = tmp();
    let p = scan(&sim_env(&pc, data.path(), vec![]));
    assert!(p.finished && !p.cancelled && p.step == 4);
    // Recommandes : emplacement verifie et donnees recentes.
    for n in ["Hollow Knight", "Stardew Valley", "Notepad++", "Documents"] {
        let it = find(&p, n);
        assert_eq!(it.tier, Tier::Recommended, "{} : {:?} score {} {:?}", n, it.tier, it.score, it.signs);
        assert!(it.default_checked(), "{}", n);
    }
    // Categorie Windows : proposee mais JAMAIS cochee d'office (2.0.1), dans « Peut-etre ».
    for n in ["Desktop shortcuts"] {
        let it = find(&p, n);
        assert_eq!(it.tier, Tier::Maybe, "{} : {:?}", n, it.tier);
        assert!(!it.default_checked() && it.selectable(), "{} : decoche mais choisissable", n);
    }
    assert_eq!(find(&p, "Hollow Knight").origin, Origin::Launcher(Store::Steam));
    assert_eq!(find(&p, "Stardew Valley").origin, Origin::Base);
    // Peut-etre : sauvegardes anciennes, logiciel deduit par son nom, bureau, fond d'ecran.
    for n in ["Old Game", "paint.net", "Desktop", "Wallpaper"] {
        let it = find(&p, n);
        assert_eq!(it.tier, Tier::Maybe, "{} : score {} {:?}", n, it.score, it.signs);
        assert!(!it.default_checked(), "{}", n);
    }
    // Masques, avec la raison.
    for (n, h) in [
        ("No Save Game", Hide::NoData),
        ("Git", Hide::DevTool),
        ("Rust 1.80 (MSVC 64-bit)", Hide::DevTool),
        ("Python 3.12.1 (64-bit)", Hide::DevTool),
        ("Google Chrome", Hide::Sensitive),
        ("Microsoft Visual C++ 2015-2022 Redistributable (x64)", Hide::System),
        ("Steam", Hide::Launcher),
        ("Logiciel Inconnu", Hide::NoData),
        ("Pictures", Hide::Empty),
        ("Windows preferences", Hide::Later),
    ] {
        let it = find(&p, n);
        assert_eq!((it.tier, it.hide), (Tier::Hidden, Some(h)), "{}", n);
        assert!(!it.selectable() && !it.default_checked(), "{}", n);
    }
    // Profil a deux emplacements gardes tels quels.
    assert_eq!(find(&p, "Notepad++").locs.len(), 2);
    // Aucun dossier generique (Microsoft) ni outil de developpement (.cargo) pris pour un jeu de la base.
    assert!(p.items.iter().all(|i| i.name != "Thing" && i.name != "Cargo Quest"));
    // Aucun outil de developpement recommande ; compteurs = resultats proposables.
    assert!(p.items.iter().filter(|i| i.tier == Tier::Recommended).all(|i| i.hide.is_none() && !matches!(i.name.as_str(), "Git" | "Rust 1.80 (MSVC 64-bit)" | "Python 3.12.1 (64-bit)")));
    assert_eq!(p.counts.iter().sum::<usize>(), p.items.iter().filter(|i| i.tier != Tier::Hidden).count());
    // Scores : ordres de grandeur attendus.
    assert!(find(&p, "Hollow Knight").score >= 80);
    assert_eq!(find(&p, "Stardew Valley").score, 70);
}

#[test]
fn liste_noire_toujours_appliquee() {
    let pc = Pc::new();
    let data = tmp();
    let p = scan(&sim_env(&pc, data.path(), vec![]));
    // Profil de navigateur (profil personnalise) et titre de la base qui pointe dans un profil Firefox : jamais proposes.
    let fox = find(&p, "Fox Browser");
    assert_eq!((fox.tier, fox.hide), (Tier::Hidden, Some(Hide::Sensitive)));
    assert!(fox.locs.is_empty(), "aucun emplacement garde");
    let fg = find(&p, "Fox Game");
    assert_eq!(fg.hide, Some(Hide::Sensitive));
    for it in p.items.iter().filter(|i| i.tier != Tier::Hidden) {
        for l in &it.locs {
            let t = crate::gamedetect::norm_path(&l.path);
            assert!(!t.contains("mozilla") && !t.contains(".ssh") && !t.contains("chrome"), "{} : {}", it.name, t);
        }
    }
    // Taille : les fichiers sensibles ne sont jamais comptes.
    let d = pc.root.join("mix");
    file(&d.join("save.sav"), 100);
    file(&d.join(".ssh/id_rsa"), 5000);
    file(&d.join("login data"), 5000);
    let s = measure(&[Loc::dir(d)], &AtomicBool::new(false));
    assert_eq!((s.files, s.bytes, s.partial), (1, 100, false));
}

#[test]
fn deja_protege_grise() {
    let pc = Pc::new();
    let data = tmp();
    let u = pc.user();
    // Documents deja protege entierement ; Hollow Knight protege par un dossier parent ; Old Game avec un filtre (partiel).
    let existing = vec![
        Game { name: "Mes documents".into(), path: u.join("Documents"), ..Default::default() },
        Game { name: "Team Cherry".into(), path: u.join("AppData/LocalLow/Team Cherry"), ..Default::default() },
        Game { name: "Old".into(), path: u.join("AppData/Roaming/OldGame"), save_filter: Some("*.cfg".into()), ..Default::default() },
    ];
    let p = scan(&sim_env(&pc, data.path(), existing));
    assert_eq!(find(&p, "Documents").already.as_deref(), Some("Mes documents"));
    assert_eq!(find(&p, "Hollow Knight").already.as_deref(), Some("Team Cherry"));
    assert_eq!(find(&p, "Old Game").already, None, "filtre different : pas entierement protege");
    let d = find(&p, "Documents");
    assert!(!d.selectable() && !d.default_checked());
    // Le dernier scan ne retient pas les elements deja proteges.
    let last = LastScan::load(data.path());
    assert!(!last.keys.contains(&d.key));
    assert!(last.keys.contains(&find(&p, "Stardew Valley").key));
}

#[test]
fn nouveau_scan_seulement_les_nouveautes() {
    let pc = Pc::new();
    let data = tmp();
    let p1 = scan(&sim_env(&pc, data.path(), vec![]));
    assert!(p1.items.iter().all(|i| !i.seen), "premier scan : tout est nouveau");
    let n1 = p1.items.iter().filter(|i| i.default_checked()).count();
    assert!(n1 >= 4);
    assert!(data.path().join(LAST_FILE).is_file());
    // Nouveau jeu installe entre deux scans.
    file(&pc.user().join("AppData/Roaming/Celeste/Saves/0.celeste"), 80);
    let mut env = sim_env(&pc, data.path(), vec![]);
    let mut db: Vec<DbGame> = env.db.games().to_vec();
    db.push(dbg("Celeste", None, &["<winAppData>/Celeste/Saves"]));
    env.db = Arc::new(GameDb::from_db(Db { format: 1, generated: "t".into(), games: db }, vec![]));
    let p2 = scan(&env);
    let checked: Vec<&str> = p2.items.iter().filter(|i| i.default_checked()).map(|i| i.name.as_str()).collect();
    assert_eq!(checked, vec!["Celeste"], "seule la nouveaute est cochee");
    assert!(find(&p2, "Hollow Knight").seen && find(&p2, "Hollow Knight").selectable(), "deja vu : reste choisissable");
    assert!(!find(&p2, "Celeste").seen);
    // Fusion : un element vu une fois le reste.
    let a = LastScan { at_ms: 1, keys: vec!["game:x".into()] };
    let b = LastScan { at_ms: 2, keys: vec!["game:y".into()] };
    assert_eq!(a.merged(&b), LastScan { at_ms: 2, keys: vec!["game:x".into(), "game:y".into()] });
}

#[test]
fn ajout_en_lot_avec_emplacements_multiples() {
    let pc = Pc::new();
    let data = tmp();
    let p = scan(&sim_env(&pc, data.path(), vec![]));
    let ctx = crate::Ctx::new(data.path().join("appdata"));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    // Un element existant porte deja le nom « Documents » (autre dossier) : nouveau nom, rien d'ecrase.
    let other = pc.root.join("autre");
    file(&other.join("a.txt"), 1);
    let mut cfg = crate::config::Config::default();
    cfg.games.push(Game { name: "Documents".into(), path: other.clone(), exe: Some("x.exe".into()), ..Default::default() });
    cfg.save(&ctx).unwrap();
    let pick: Vec<Item> = ["Hollow Knight", "Notepad++", "Documents", "Desktop", "Desktop shortcuts", "Stardew Valley"].iter().map(|n| find(&p, n).clone()).collect();
    let mut sizes = HashMap::new();
    sizes.insert(find(&p, "Documents").key.clone(), Size { bytes: BIG_BYTES + 1, files: 10, partial: false });
    let rep = add_items(&ctx, &pick, &sizes, &|it| (it.cat == Cat::Folders).then(|| format!("{} (fr)", it.name)));
    assert_eq!(rep.added, vec!["Hollow Knight", "Notepad++", "Documents 2", "Desktop", "Stardew Valley"]);
    assert_eq!(rep.skipped, vec![("Desktop shortcuts".to_string(), Skip::Already("Desktop (fr)".into()))], "compris dans le Bureau choisi");
    assert_eq!(rep.multi, 1);
    let cfg = crate::config::Config::load(&ctx).unwrap();
    assert_eq!(cfg.games.len(), 6);
    let np = cfg.find("Notepad++").unwrap();
    assert_eq!(np.extra_locations.len(), 1, "deux emplacements : principal + supplementaire");
    assert_eq!(np.primary_label.as_deref(), Some("Settings"));
    assert_eq!(np.extra_locations[0].label, "Plugins");
    assert!(np.schedule.is_some() || np.exe.is_some());
    let hk = cfg.find("Hollow Knight").unwrap();
    assert_eq!(hk.exe.as_deref(), Some("hollow_knight.exe"));
    assert!(hk.schedule.is_none());
    let docs = cfg.find("Documents 2").unwrap();
    assert_eq!(docs.display_name.as_deref(), Some("Documents (fr)"));
    assert!(docs.exe.is_none() && docs.schedule.as_ref().map(|s| s.is_valid()).unwrap_or(false), "sans programme : planification");
    assert_eq!(cfg.find("Documents").unwrap().path, other, "l'element existant n'est pas touche");
    let st = crate::settings::Settings::load(&ctx).unwrap();
    assert_eq!(st.per_game.get("Documents 2").and_then(|g| g.keep_versions), Some(BIG_KEEP), "gros dossier : 3 versions");
    assert!(st.per_game.get("Hollow Knight").and_then(|g| g.exe_path.clone()).is_some(), "icone du programme");
    // Deuxieme ajout : tout est deja protege.
    let rep2 = add_items(&ctx, &pick[..2], &sizes, &|_| None);
    assert!(rep2.added.is_empty());
    assert!(rep2.skipped.iter().all(|(_, s)| matches!(s, Skip::Already(_))));
    // Rescan : grise.
    let env = sim_env(&pc, data.path(), cfg.games.clone());
    let p3 = scan(&env);
    assert!(find(&p3, "Hollow Knight").already.is_some());
    assert_eq!(scan_done_props(12_000, 7, 5), [("duration", "10-30s"), ("proposed", "6-20"), ("accepted", "1-5")]);
}

#[test]
fn annulation_arrete_le_scan_sans_rien_memoriser() {
    let pc = Pc::new();
    let data = tmp();
    let mut env = sim_env(&pc, data.path(), vec![]);
    let h = Handle { state: Arc::new(Mutex::new(Progress::default())), cancel: Arc::new(AtomicBool::new(false)) };
    // Annule pendant l'etape des jeux (comme un clic sur « Annuler »).
    let c = h.cancel.clone();
    let inner = std::mem::replace(&mut env.launcher_games, Box::new(Vec::new));
    env.launcher_games = Box::new(move || {
        c.store(true, Ordering::SeqCst);
        inner()
    });
    run(&env, &h, &|| {});
    let p = h.view();
    assert!(p.finished && p.cancelled);
    assert!(p.items.iter().all(|i| i.cat == Cat::Games), "les etapes suivantes ne sont pas lancees");
    assert!(!data.path().join(LAST_FILE).exists(), "un scan annule n'est pas memorise");
    measure_sizes(&h, &|| {});
    assert!(h.view().sizes.is_empty());
    // Dans un thread : le scan rend la main, la progression avance, l'annulation est prise en compte.
    let env = sim_env(&pc, data.path(), vec![]);
    let hh = start(env, Arc::new(|| {}));
    let t0 = std::time::Instant::now();
    while !hh.view().sizes_done && t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let v = hh.view();
    assert!(v.finished && !v.cancelled && v.sizes_done);
    assert_eq!(v.sizes.get(&find(&v, "Hollow Knight").key).map(|s| s.bytes), Some(2000));
}

#[test]
fn classement_pur() {
    let now = 1_000 * DAY;
    let recent = Some(now - DAY);
    let old = Some(now - 400 * DAY);
    assert_eq!(classify(&[Sign::Library, Sign::BaseKnown], None, true, recent, now), (80, Tier::Recommended));
    assert_eq!(classify(&[Sign::Library, Sign::BaseKnown], None, true, old, now), (80, Tier::Maybe), "donnees anciennes");
    assert_eq!(classify(&[Sign::Library, Sign::BaseKnown], None, false, recent, now).1, Tier::Hidden, "emplacement non verifie");
    assert_eq!(classify(&[Sign::Installed, Sign::Deduced], None, true, recent, now), (50, Tier::Maybe));
    assert_eq!(classify(&[Sign::Installed], None, true, recent, now), (30, Tier::Hidden));
    assert_eq!(classify(&[Sign::Profile, Sign::ProfileInstalled, Sign::ProfileWarning], None, true, recent, now), (60, Tier::Maybe));
    assert_eq!(classify(&[Sign::PersonalMain], Some(Hide::DevTool), true, recent, now), (0, Tier::Hidden));
    // Outils de developpement reconnus par leur nom, leur editeur ou leur dossier ; vrais logiciels gardes.
    let e = |n: &str, p: Option<&str>| AppEntry { name: n.into(), publisher: p.map(|s| s.into()), ..Default::default() };
    for n in ["Git", "GitHub CLI", "Rustup", "Node.js", "Visual Studio Build Tools 2022", "JetBrains Rider", "Docker Desktop", "Windows Subsystem for Linux", "Microsoft .NET SDK 8.0"] {
        assert!(matches!(app_hide(&e(n, None)), Some(Hide::DevTool) | Some(Hide::System)), "{}", n);
    }
    for n in ["Digital Paint", "Audacity", "OBS Studio", "VLC media player", "GIMP 2.10", "Spotify"] {
        assert_eq!(app_hide(&e(n, Some("X"))), None, "{}", n);
    }
    assert_eq!(app_hide(&AppEntry { name: "Truc".into(), location: Some(PathBuf::from("C:/Users/x/.cargo/bin")), ..Default::default() }), Some(Hide::DevTool));
}

#[test]
fn reglages_windows_proposes_decoches_dans_peut_etre() {
    use crate::winprefs::{self as wp, Data, Kind, MemSys, Root};
    let pc = Pc::new();
    let data = tmp();
    let s = Arc::new(MemSys::new(22631, Folders::default(), data.path().join("hosts")));
    s.set(Root::Hkcu, r"Control Panel\Mouse", "MouseSensitivity", Data::Sz("10".into()));
    s.set(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme", Data::Dword(0));
    let mut env = sim_env(&pc, data.path(), vec![]);
    env.prefs = wp::offers(data.path(), &*s);
    assert_eq!(env.prefs.len(), 2);
    let p = scan(&env);
    for k in [Kind::Mouse, Kind::Theme] {
        let it = find(&p, k.stable());
        assert_eq!(it.cat, Cat::System);
        assert_eq!(it.origin, Origin::System(SysItem::Pref(k)));
        assert_eq!(it.tier, Tier::Maybe, "{:?}", k);
        assert!(!it.default_checked() && it.selectable(), "{:?} : decoche mais choisissable", k);
        assert_eq!(it.locs[0].path, wp::staging(data.path(), k));
    }
    // Ajout : export local pret, element ordinaire (copie chaque jour a midi), nom stable = cle du coffre.
    let mut ctx = crate::Ctx::new(data.path().to_path_buf());
    ctx.sys = Some(s.clone());
    let it = find(&p, Kind::Mouse.stable()).clone();
    let rep = add_items(&ctx, &[it], &HashMap::new(), &|_| Some("Souris".into()));
    assert_eq!(rep.added, vec![Kind::Mouse.stable().to_string()], "{:?}", rep);
    let cfg = crate::config::Config::load(&ctx).unwrap();
    let g = cfg.find(Kind::Mouse.stable()).unwrap();
    assert_eq!(g.display_name.as_deref(), Some("Souris"));
    assert!(g.schedule.is_some() && g.exe.is_none() && g.dest_key.is_none());
    assert_eq!(wp::kind_of_game(data.path(), g), Some(Kind::Mouse));
}