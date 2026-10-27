//! 1.4 (lot 1) : detection fiable par score de confiance (0 a 100, seuil 70). Les faux positifs reels (outils de
//! developpement et logiciels pris pour des jeux de la base) ne sont plus jamais proposes ; les vrais jeux, emulateurs et
//! profils integres le restent. Un nom de programme ne suffit jamais seul.
use agss::gamedetect::*;
use agss::process::ProcInfo;
use std::path::PathBuf;

fn p(exe: &str, path: &str) -> ProcInfo {
    ProcInfo { pid: 1, exe: exe.to_lowercase(), path: Some(PathBuf::from(path)), start: 1 }
}

/// Base embarquee reelle (≈ 20 800 titres).
fn real_db() -> impl Fn(&str) -> Vec<DbHit> {
    let db = agss::gamedb::embedded(&std::env::temp_dir().join("agss-v34-absent"));
    move |exe: &str| db.find_by_exe(exe).into_iter().map(|g| DbHit { name: g.name.clone(), install_dirs: g.install_dirs.clone() }).collect()
}

/// Indices d'un jeu lance normalement : fenetre, ouvert depuis 2 min, lance par l'explorateur, affichage 3D.
fn game_facts() -> Facts {
    Facts { console: Some(false), age_ms: Some(120_000), parent: Some("explorer.exe".into()), graphics: Some(true), ..Default::default() }
}

/// Indices les PLUS favorables qu'un outil puisse avoir (fenetre, ouvert longtemps, 3D, lance par l'explorateur) :
/// meme ainsi, un nom seul ne doit jamais suffire.
fn best_tool_facts() -> Facts {
    Facts { console: Some(false), age_ms: Some(3_600_000), parent: Some("explorer.exe".into()), graphics: Some(true), company: Some("Outils SA".into()), ..Default::default() }
}

fn score(c: &Option<Candidate>) -> String {
    c.as_ref().and_then(|c| c.confidence.as_ref()).map(|s| s.summary()).unwrap_or_else(|| "exclu".into())
}

/// Les 19 faux positifs RELLEMENT trouves sur le PC de developpement (audit `examples/detect_audit.rs`, ancienne regle
/// « nom dans la base, 3 titres au plus = accepte »), plus ceux de l'etude (echo, node, control, cmd, at, cargo de .cargo).
#[cfg(windows)]
const REAL_FALSE_POSITIVES: &[(&str, &str)] = &[
    ("bionic.exe", r"C:\Program Files\Bionic\Bionic.exe"),
    ("grid.exe", r"C:\Program Files\GIMP 3\lib\gimp\3.0\plug-ins\grid\grid.exe"),
    ("sh.exe", r"C:\Program Files\Git\bin\sh.exe"),
    ("arch.exe", r"C:\Program Files\Git\usr\bin\arch.exe"),
    ("dd.exe", r"C:\Program Files\Git\usr\bin\dd.exe"),
    ("df.exe", r"C:\Program Files\Git\usr\bin\df.exe"),
    ("expand.exe", r"C:\Program Files\Git\usr\bin\expand.exe"),
    ("fold.exe", r"C:\Program Files\Git\usr\bin\fold.exe"),
    ("rm.exe", r"C:\Program Files\Git\usr\bin\rm.exe"),
    ("sh.exe", r"C:\Program Files\Git\usr\bin\sh.exe"),
    ("split.exe", r"C:\Program Files\Git\usr\bin\split.exe"),
    ("tr.exe", r"C:\Program Files\Git\usr\bin\tr.exe"),
    ("wc.exe", r"C:\Program Files\Git\usr\bin\wc.exe"),
    ("gh.exe", r"C:\Program Files\GitHub CLI\gh.exe"),
    ("moonlight.exe", r"C:\Program Files\Moonlight Game Streaming\Moonlight.exe"),
    ("everything.exe", r"C:\Program Files (x86)\Everything\Everything.exe"),
    ("cargo.exe", r"C:\Users\player\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe"),
    ("tsgo.exe", r"C:\Users\player\AppData\Local\Programs\Antigravity IDE\resources\app\node_modules\@typescript\native-preview-win32-x64\lib\tsgo.exe"),
    ("openconsole.exe", r"C:\Users\player\AppData\Local\Programs\Antigravity IDE\resources\app\node_modules\node-pty\build\Release\conpty\OpenConsole.exe"),
];

#[cfg(windows)]
const STUDY_FALSE_POSITIVES: &[(&str, &str)] = &[
    ("echo.exe", r"C:\Program Files\Git\usr\bin\echo.exe"),
    ("cargo.exe", r"C:\Users\player\.cargo\bin\cargo.exe"),
    ("node.exe", r"C:\Program Files\nodejs\node.exe"),
    ("control.exe", r"C:\Windows\System32\control.exe"),
    ("cmd.exe", r"C:\Windows\System32\cmd.exe"),
    ("at.exe", r"C:\Windows\System32\at.exe"),
];

/// Indices reels de chaque outil : programmes console pour les outils en ligne de commande ; « Everything » et
/// « Moonlight » sont des fenetres (Moonlight affiche de la video : bibliotheque graphique chargee).
#[cfg(windows)]
fn real_tool_facts(exe: &str) -> Facts {
    let gui = ["bionic.exe", "grid.exe", "moonlight.exe", "everything.exe"].contains(&exe);
    Facts { console: Some(!gui), age_ms: Some(3_600_000), parent: Some("explorer.exe".into()), graphics: Some(exe == "moonlight.exe"), ..Default::default() }
}

#[cfg(windows)]
#[test]
fn les_19_faux_positifs_reels_ne_sont_plus_proposes() {
    let db = real_db();
    let e = Env { db: &db, launcher_roots: &[], known_exes: &[] };
    assert_eq!(REAL_FALSE_POSITIVES.len(), 19);
    let mut bad = Vec::new();
    for (exe, path) in REAL_FALSE_POSITIVES.iter().chain(STUDY_FALSE_POSITIVES) {
        // Collision prouvee : ce nom d'outil est bien celui d'un titre de la base.
        assert!(!db(exe).is_empty(), "{} : plus dans la base, le test ne prouve plus rien", exe);
        let pr = p(exe, path);
        for (label, f) in [("indices reels", real_tool_facts(exe)), ("indices inconnus", Facts::default())] {
            if exe == &"everything.exe" && label == "indices inconnus" {
                continue; // seul cas ou la defense est l'absence de bibliotheque graphique (voir le test dedie)
            }
            if classify_with(&pr, &e, &f).is_some() {
                bad.push(format!("{} ({}) [{}] : {}", exe, path, label, score(&assess(&pr, &e, &f))));
            }
        }
    }
    assert!(bad.is_empty(), "faux positifs proposes :\n{}", bad.join("\n"));
    // Les outils Git / Rust / GitHub / Node sont refuses d'office (veto), pas seulement faute de points.
    for (exe, path) in REAL_FALSE_POSITIVES.iter().filter(|(_, p)| p.contains(r"\Git\") || p.contains(".rustup") || p.contains("GitHub CLI") || p.contains("node_modules")) {
        let c = assess(&p(exe, path), &e, &Facts::default()).unwrap();
        assert_eq!(c.confidence.as_ref().unwrap().veto, Some(Veto::DevFolder), "{}", path);
    }
    // Greffon de GIMP : dossier de greffons, jamais un programme principal.
    assert!(assess(&p("grid.exe", REAL_FALSE_POSITIVES[1].1), &e, &Facts::default()).is_none());
}

#[cfg(windows)]
#[test]
fn utilitaire_homonyme_sans_affichage_3d_refuse() {
    // « Everything » (outil de recherche) dans son dossier « Everything » : meme nom et meme dossier que le titre de la base.
    let db = real_db();
    let e = Env { db: &db, launcher_roots: &[], known_exes: &[] };
    let pr = p("everything.exe", r"C:\Program Files (x86)\Everything\Everything.exe");
    let c = assess(&pr, &e, &real_tool_facts("everything.exe")).unwrap();
    let s = c.confidence.unwrap();
    assert!(!s.proposable() && s.reasons.contains(&Reason::NoGraphics), "{}", s.summary());
    assert_eq!(s.tier(), Tier::Maybe, "au mieux « Peut-etre » dans le scan, jamais une carte");
    // Le meme cas, mais un vrai jeu qui affiche en 3D : propose.
    assert!(classify_with(&pr, &e, &game_facts()).is_some());
}

#[cfg(windows)]
#[test]
fn un_nom_ne_suffit_jamais_seul() {
    // Chaque nom des faux positifs, et 300 noms quelconques de la base, ranges dans un dossier neutre, avec les indices
    // d'execution les plus favorables : jamais proposes (score plafonne sous le seuil sans indice de dossier).
    let db = real_db();
    let e = Env { db: &db, launcher_roots: &[], known_exes: &[] };
    for (exe, _) in REAL_FALSE_POSITIVES.iter().chain(STUDY_FALSE_POSITIVES) {
        let pr = p(exe, &format!(r"D:\Outils\Divers\{}", exe));
        assert!(classify_with(&pr, &e, &best_tool_facts()).is_none(), "{} : {}", exe, score(&assess(&pr, &e, &best_tool_facts())));
    }
    let unique = |_: &str| vec![DbHit { name: "Un Titre Tres Distinctif".into(), install_dirs: vec!["UnTitre".into()] }];
    let e2 = Env { db: &unique, launcher_roots: &[], known_exes: &[] };
    for i in 0..300 {
        let exe = format!("programme{:03}.exe", i);
        let pr = p(&exe, &format!(r"C:\Program Files\Editeur {}\Appli\{}", i, exe));
        let c = assess(&pr, &e2, &best_tool_facts()).unwrap();
        let s = c.confidence.unwrap();
        assert!(!s.proposable() && s.score < PROPOSE_THRESHOLD, "{} : {}", exe, s.summary());
    }
    // Le plafond est structurel : sans indice de dossier, meme une somme elevee reste sous le seuil.
    let s = Confidence::new(vec![Reason::DbUnique, Reason::Graphics, Reason::LongLived, Reason::Publisher, Reason::Emulator], None);
    assert_eq!(s.score, PROPOSE_THRESHOLD - 1);
    assert!(Confidence::new(vec![Reason::DbDir], None).proposable());
    assert_eq!(Confidence::new(vec![Reason::Library, Reason::Manifest, Reason::DbDir], Some(Veto::Console)).score, 0, "veto = 0");
}

#[cfg(windows)]
#[test]
fn vrais_jeux_emulateurs_et_profils_proposes() {
    let db = real_db();
    let roots = vec!["e:/steamlibrary/steamapps/common".to_string()];
    let e = Env { db: &db, launcher_roots: &roots, known_exes: &[] };
    let manifest = |dir: &str, name: &str| Facts { manifest: Some((PathBuf::from(dir), name.to_string())), ..game_facts() };
    let cases: Vec<(&str, &str, Facts)> = vec![
        // Steam (bibliotheques reelles de ce PC)
        ("stardew valley.exe", r"E:\SteamLibrary\steamapps\common\Stardew Valley\Stardew Valley.exe", manifest(r"E:\SteamLibrary\steamapps\common\Stardew Valley", "Stardew Valley")),
        ("portal2.exe", r"E:\SteamLibrary\steamapps\common\Portal 2\portal2.exe", game_facts()),
        ("bloonstd6.exe", r"E:\SteamLibrary\steamapps\common\BloonsTD6\BloonsTD6.exe", game_facts()),
        ("breach.exe", r"E:\SteamLibrary\steamapps\common\Into the Breach\Breach.exe", game_facts()),
        ("cyberpunk2077.exe", r"J:\SteamLibrary\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe", game_facts()),
        ("splitfiction.exe", r"J:\SteamLibrary\steamapps\common\Split Fiction\Split\Binaries\Win64\SplitFiction.exe", game_facts()),
        ("oriwotw.exe", r"J:\SteamLibrary\steamapps\common\Ori and the Will of the Wisps\oriwotw.exe", game_facts()),
        ("eldenring.exe", r"D:\Steam\steamapps\common\ELDEN RING\Game\eldenring.exe", game_facts()),
        ("tl.exe", r"E:\SteamLibrary\steamapps\common\Throne and Liberty\TL\Binaries\Win64\TL.exe", game_facts()),
        // Epic, GOG, Xbox, itch, EA, Ubisoft
        ("fortniteclient-win64-shipping.exe", r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe", game_facts()),
        ("witcher3.exe", r"C:\GOG Games\The Witcher 3 Wild Hunt GOTY\bin\x64\witcher3.exe", game_facts()),
        ("haloinfinite.exe", r"C:\XboxGames\Halo Infinite\Content\HaloInfinite.exe", game_facts()),
        ("celeste.exe", r"C:\Users\player\AppData\Roaming\itch\apps\celeste\Celeste.exe", game_facts()),
        ("fifa23.exe", r"C:\Program Files\EA Games\FIFA 23\FIFA23.exe", game_facts()),
        ("acvalhalla.exe", r"C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\Assassin's Creed Valhalla\ACValhalla.exe", game_facts()),
        // Lanceur installe hors de son dossier par defaut : confirme par son manifeste
        ("hades.exe", r"D:\Jeux\Epic\Hades\x64\Hades.exe", manifest(r"D:\Jeux\Epic\Hades", "Hades")),
        // Hors lanceur, dossier au nom du titre
        ("stardew valley.exe", r"D:\Jeux\Stardew Valley\Stardew Valley.exe", game_facts()),
        ("portal2.exe", r"D:\Jeux\Portal 2\portal2.exe", game_facts()),
        ("graveyard keeper.exe", r"D:\Jeux\Graveyard Keeper\Graveyard Keeper.exe", game_facts()),
        ("bloonstd6.exe", r"D:\Jeux\BloonsTD6\BloonsTD6.exe", game_facts()),
        // Emulateurs
        ("pcsx2-qt.exe", r"C:\Emu\PCSX2\pcsx2-qt.exe", game_facts()),
        ("retroarch.exe", r"C:\RetroArch-Win64\retroarch.exe", game_facts()),
        ("dolphin.exe", r"C:\Program Files\Dolphin-x64\Dolphin.exe", game_facts()),
        ("ryujinx.exe", r"D:\Emulateurs\ryujinx-1.1.1403\publish\Ryujinx.exe", game_facts()),
        ("duckstation-qt-x64-releaseltcg.exe", r"D:\Emus\duckstation\duckstation-qt-x64-ReleaseLTCG.exe", game_facts()),
        ("cemu.exe", r"D:\Cemu_2.0\Cemu.exe", game_facts()),
    ];
    let mut missed = Vec::new();
    for (exe, path, f) in &cases {
        let pr = p(exe, path);
        if classify_with(&pr, &e, f).is_none() {
            missed.push(format!("{} : {}", path, score(&assess(&pr, &e, f))));
        }
    }
    let ok = cases.len() - missed.len();
    assert!(ok * 100 >= cases.len() * 95, "{} / {} vrais jeux proposes (< 95 %) :\n{}", ok, cases.len(), missed.join("\n"));
    assert!(missed.is_empty(), "vrais jeux non proposes :\n{}", missed.join("\n"));
}

#[cfg(windows)]
#[test]
fn profils_integres_gardes_et_verifies() {
    let hint = |id: &str, name: &str, app: bool, installed: bool| ProfileHint { id: id.into(), name: name.into(), app, installed };
    let vs = hint("vscode", "Visual Studio Code settings", true, true);
    let c = assess_profile(&p("code.exe", r"C:\Users\player\AppData\Local\Programs\Microsoft VS Code\Code.exe"), &game_facts(), &vs).unwrap();
    assert!(is_proposable(&c) && c.why == Why::Profile && c.app, "{:?}", c.confidence);
    // PowerShell : programme console lance depuis le Terminal (normal pour ce profil cure).
    let ps = hint("powershell-profile", "PowerShell profile", false, true);
    let f = Facts { console: Some(true), parent: Some("windowsterminal.exe".into()), age_ms: Some(120_000), ..Default::default() };
    assert!(is_proposable(&assess_profile(&p("pwsh.exe", r"C:\Program Files\PowerShell\7\pwsh.exe"), &f, &ps).unwrap()));
    // Un profil reconnu par le seul nom (programme absent de ce PC, dossier sans rapport) : refuse.
    let wow = hint("wow-retail", "World of Warcraft (retail)", false, false);
    let c = assess_profile(&p("wow.exe", r"D:\Outils\wow.exe"), &best_tool_facts(), &wow).unwrap();
    assert!(!is_proposable(&c), "{}", c.confidence.unwrap().summary());
    assert!(is_proposable(&assess_profile(&p("wow.exe", r"C:\Program Files (x86)\World of Warcraft\_retail_\Wow.exe"), &game_facts(), &wow).unwrap()));
    // Un profil ordinaire n'echappe pas aux refus d'execution (console, terminal, processus trop court).
    let er = hint("elden-ring", "ELDEN RING", false, true);
    let path = r"D:\Jeux\ELDEN RING\Game\eldenring.exe";
    assert!(is_proposable(&assess_profile(&p("eldenring.exe", path), &game_facts(), &er).unwrap()));
    for (f, v) in [
        (Facts { console: Some(true), ..game_facts() }, Veto::Console),
        (Facts { parent: Some("cmd.exe".into()), ..game_facts() }, Veto::ShellParent),
        (Facts { age_ms: Some(3_000), ..game_facts() }, Veto::ShortLived),
        (Facts { other_user: true, ..game_facts() }, Veto::System),
    ] {
        assert_eq!(assess_profile(&p("eldenring.exe", path), &f, &er).unwrap().confidence.unwrap().veto, Some(v));
    }
}

#[cfg(windows)]
#[test]
fn vetos_console_terminal_duree_editeur_et_dossiers_de_dev() {
    let db = real_db();
    let e = Env { db: &db, launcher_roots: &[], known_exes: &[] };
    let game = p("stardew valley.exe", r"E:\SteamLibrary\steamapps\common\Stardew Valley\Stardew Valley.exe");
    assert!(classify_with(&game, &e, &game_facts()).is_some());
    let veto = |f: Facts| assess(&game, &e, &f).unwrap().confidence.unwrap().veto;
    assert_eq!(veto(Facts { console: Some(true), ..game_facts() }), Some(Veto::Console), "serveur dedie / outil console dans une bibliotheque");
    for sh in ["cmd.exe", "powershell.exe", "pwsh.exe", "bash.exe", "windowsterminal.exe", "code.exe", "devenv.exe", "cargo.exe", "python.exe"] {
        assert_eq!(veto(Facts { parent: Some(sh.into()), ..game_facts() }), Some(Veto::ShellParent), "{}", sh);
    }
    for ok in ["explorer.exe", "steam.exe", "epicgameslauncher.exe", "playnite.desktopapp.exe", "galaxyclient.exe"] {
        assert_eq!(veto(Facts { parent: Some(ok.into()), ..game_facts() }), None, "{}", ok);
    }
    assert_eq!(veto(Facts { age_ms: Some(MIN_AGE_MS - 1), ..game_facts() }), Some(Veto::ShortLived));
    assert_eq!(veto(Facts { age_ms: Some(MIN_AGE_MS), ..game_facts() }), None);
    for (company, product) in [("The Git Development Community", "Git"), ("GitHub, Inc.", "GitHub CLI"), ("Python Software Foundation", "Python"), ("JetBrains s.r.o.", "IntelliJ IDEA"), ("Microsoft Corporation", "Microsoft® Visual Studio®")] {
        assert_eq!(veto(Facts { company: Some(company.into()), product: Some(product.into()), ..game_facts() }), Some(Veto::DevPublisher), "{}", company);
    }
    assert_eq!(veto(Facts { company: Some("ConcernedApe".into()), product: Some("Stardew Valley".into()), ..game_facts() }), None);
    // Dossiers d'outils de developpement : refus, meme sous un dossier de bibliotheque.
    for path in [
        r"C:\Program Files\Git\mingw64\bin\wish.exe",
        r"C:\msys64\usr\bin\mintty.exe",
        r"C:\Users\x\.cargo\bin\rustup.exe",
        r"C:\Users\x\.rustup\toolchains\stable\bin\rust-analyzer.exe",
        r"C:\Program Files\nodejs\node_modules\npm\bin\x.exe",
        r"C:\Users\x\AppData\Local\Programs\Python\Python312\pythonw.exe",
        r"C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\devenv.exe",
        r"C:\Program Files\JetBrains\Rider\bin\rider64.exe",
        r"C:\Program Files (x86)\Windows Kits\10\bin\x64\signtool.exe",
        r"C:\Users\x\AppData\Local\Android\Sdk\emulator\emulator.exe",
        r"C:\Users\x\AppData\Local\Temp\setup-123\jeu.exe",
        r"D:\Projets\MonJeu\target\release\monjeu.exe",
        r"C:\Program Files\Epic Games\UE_5.4\Engine\Binaries\Win64\UnrealInsights.exe",
        r"C:\Program Files\Unity\Hub\Editor\2022.3.1f1\Editor\Unity.exe",
    ] {
        assert!(is_dev_dir(&norm_path(std::path::Path::new(path))), "{}", path);
    }
    assert!(!is_dev_dir(&norm_path(std::path::Path::new(r"E:\SteamLibrary\steamapps\common\Stardew Valley\Stardew Valley.exe"))));
    let ue = p("unrealinsights.exe", r"C:\Program Files\Epic Games\UE_5.4\Engine\Binaries\Win64\UnrealInsights.exe");
    assert!(classify_with(&ue, &e, &game_facts()).is_none(), "outil du moteur Unreal (dossier Epic Games)");
    // Indices bonus : bibliotheque graphique, une minute de vie, editeur de jeux.
    let c = assess(&game, &e, &Facts { company: Some("Valve Corporation".into()), ..game_facts() }).unwrap().confidence.unwrap();
    assert!(c.reasons.contains(&Reason::Graphics) && c.reasons.contains(&Reason::LongLived) && c.reasons.contains(&Reason::Publisher), "{}", c.summary());
}

#[test]
fn detecteur_attend_20_s_et_journalise_les_refus_une_fois() {
    let db = |exe: &str| if exe == "eldenring.exe" { vec![DbHit { name: "ELDEN RING".into(), install_dirs: vec!["ELDEN RING".into()] }] } else { vec![] };
    let e = Env { db: &db, launcher_roots: &[], known_exes: &[] };
    let mem = Memory::default();
    let mut d = Detector::default();
    d.scan(&[], &e, &mem, false, 0);
    let (gp, tp) = if cfg!(windows) { (r"D:\Jeux\ELDEN RING\Game\eldenring.exe", r"D:\Outils\eldenring.exe") } else { ("/home/deck/Jeux/ELDEN RING/Game/eldenring.exe", "/home/deck/Outils/eldenring.exe") };
    let game = ProcInfo { pid: 7, exe: "eldenring.exe".into(), path: Some(PathBuf::from(gp)), start: 42 };
    let at = |age: u64| Facts { age_ms: Some(age), ..game_facts() };
    // 5 s de vie : rien (et pas de ligne de journal pour un simple processus trop jeune).
    d.facts.insert((7, 42), at(5_000));
    assert!(d.scan(&[game.clone()], &e, &mem, false, 5_000).is_empty());
    assert!(d.refused.is_empty());
    // 25 s : carte, avec son score et ses raisons.
    d.facts.insert((7, 42), at(25_000));
    let ev = d.scan(&[game.clone()], &e, &mem, false, 25_000);
    let Some(Event::Launched(c)) = ev.first() else { panic!("{:?}", ev) };
    let s = c.confidence.as_ref().unwrap();
    assert!(s.proposable() && s.reasons.contains(&Reason::DbDir), "{}", s.summary());
    // Outil console de meme nom : refuse, journalise UNE seule fois par session.
    let tool = ProcInfo { pid: 8, exe: "eldenring.exe".into(), path: Some(PathBuf::from(tp)), start: 50 };
    for t in [30_000, 35_000, 40_000] {
        d.facts.insert((8, 50), Facts { console: Some(true), ..at(t) });
        assert!(d.scan(&[game.clone(), tool.clone()], &e, &mem, false, t).is_empty());
    }
    assert_eq!(d.refused.len(), 1, "{:?}", d.refused);
    assert!(d.refused[0].confidence.as_ref().unwrap().summary().contains("refus"));
    // Lance depuis un terminal qui se ferme ensuite (« cmd /c start ») : le refus reste acquis pour ce processus.
    let gp2 = if cfg!(windows) { r"E:\Autres\ELDEN RING\eldenring.exe" } else { "/home/deck/Autres/ELDEN RING/eldenring.exe" };
    let other = ProcInfo { pid: 9, exe: "eldenring.exe".into(), path: Some(PathBuf::from(gp2)), start: 60 };
    d.facts.insert((9, 60), Facts { parent: Some("cmd.exe".into()), tty: true, ..at(30_000) });
    assert!(d.scan(&[game.clone(), other.clone()], &e, &mem, false, 45_000).is_empty());
    d.facts.insert((9, 60), Facts { parent: None, ..at(35_000) });
    assert!(d.scan(&[game.clone(), other.clone()], &e, &mem, false, 50_000).is_empty(), "parent ferme : toujours refuse");
    let fresh = ProcInfo { pid: 10, start: 70, ..other.clone() };
    d.facts.insert((10, 70), at(30_000));
    assert!(matches!(d.scan(&[game.clone(), other, fresh], &e, &mem, false, 55_000).as_slice(), [Event::Launched(_)]), "le meme titre lance normalement est propose");
    assert!(d.facts.is_empty(), "indices consommes a chaque passage");
}

#[test]
fn score_serialise_et_anciennes_cartes_relues() {
    // Carte enregistree par la 1.3.x (sans score) : relue telle quelle.
    let old = r#"{"queue":[{"cand":{"key":"k","exe":"x.exe","path":null,"why":"InDb","name":"X","install_dir":null},"created_ms":5}]}"#;
    let q: Proposals = serde_json::from_str(old).unwrap();
    assert_eq!(q.queue[0].cand.confidence, None);
    let s = Confidence::new(vec![Reason::Library, Reason::Graphics], None);
    let j = serde_json::to_string(&s).unwrap();
    assert!(j.contains("\"library\"") && j.contains("\"graphics\""), "{}", j);
    assert_eq!(serde_json::from_str::<Confidence>(&j).unwrap(), s);
    assert_eq!(s.tier(), Tier::Propose);
    assert_eq!(Confidence::new(vec![Reason::DbDirPartial], None).tier(), Tier::Maybe);
    assert_eq!(Confidence::new(vec![Reason::DbFew], None).tier(), Tier::Never);
    assert!(s.summary().starts_with("85/100 ("), "{}", s.summary());
}

#[test]
fn textes_de_fiabilite_dans_les_5_langues() {
    use agss::i18n::{tr, Lang, K};
    for l in Lang::ALL {
        assert!(tr(l, K::DetConfidence).contains("{}/100"), "{:?}", l);
        for k in [K::VExpertMode, K::VSetExpertHint, K::RsLibrary, K::RsDbDir, K::RsNoGraphics, K::RsShortName] {
            assert!(!tr(l, k).trim().is_empty(), "{:?}/{:?}", l, k);
        }
    }
    assert_ne!(tr(Lang::Fr, K::RsLibrary), tr(Lang::En, K::RsLibrary));
    let s = agss::settings::Settings::default();
    assert!(!s.expert_mode, "mode expert desactive par defaut");
}

// ------------------------------------------------------------------ Linux

#[cfg(not(windows))]
mod linux {
    use super::*;

    #[test]
    fn linux_terminal_dossiers_de_dev_et_jeux_steam() {
        let db = real_db();
        let roots = vec!["/home/deck/games".to_string()];
        let e = Env { db: &db, launcher_roots: &roots, known_exes: &[] };
        let game = p("stardew valley.exe", "/home/deck/.local/share/Steam/steamapps/common/Stardew Valley/Stardew Valley.exe");
        // Jeu Proton lance par Steam (parent python3 du script Proton, sans terminal) : propose.
        assert!(classify_with(&game, &e, &Facts { parent: Some("python3".into()), tty: false, ..game_facts() }).is_some());
        // Lance a la main depuis un terminal : refuse.
        let c = assess(&game, &e, &Facts { parent: Some("bash".into()), tty: true, ..game_facts() }).unwrap();
        assert_eq!(c.confidence.unwrap().veto, Some(Veto::ShellParent));
        // Outils de developpement homonymes de titres de la base.
        for (exe, path) in [("cargo", "/home/deck/.cargo/bin/cargo"), ("tsgo", "/home/deck/proj/node_modules/.bin/tsgo"), ("rm", "/home/deck/.local/share/venv/bin/rm")] {
            assert!(classify_with(&p(exe, path), &e, &best_tool_facts()).is_none(), "{}", path);
        }
        // Programme de /usr/bin homonyme d'un titre : son nom ne suffit pas.
        assert!(classify_with(&p("split", "/usr/bin/split"), &e, &best_tool_facts()).is_none());
        // Emulateur installe par le systeme : garde.
        assert!(classify_with(&p("dolphin-emu", "/usr/bin/dolphin-emu"), &e, &game_facts()).is_some());
        // Processus d'un autre utilisateur (demon du systeme) : refuse.
        assert_eq!(assess(&game, &e, &Facts { other_user: true, ..game_facts() }).unwrap().confidence.unwrap().veto, Some(Veto::System));
    }

    #[test]
    fn linux_stat_terminal_parent_et_bibliotheques_graphiques() {
        use agss::procscan::*;
        let s = "4242 (game (x)) S 77 2 3 34817 5 6 7 8 9 10 11 12 13 14 15 16 17 18 555 20";
        assert_eq!(parse_stat_ppid_tty(s), Some((77, true)));
        assert_eq!(parse_stat_ppid_tty("4242 (steam) S 1 2 3 0 5"), Some((1, false)));
        assert_eq!(parse_btime("cpu 1 2 3\nbtime 1700000000\nprocesses 5\n"), Some(1_700_000_000));
        assert!(maps_have_graphics("7f00-7f10 r-xp 0 08:01 1 /usr/lib/x86_64-linux-gnu/libvulkan.so.1.3.0"));
        assert!(maps_have_graphics("7f00-7f10 r-xp 0 08:01 1 /home/deck/.steam/steam/steamapps/common/Proton 9.0/files/lib/wine/dxvk/x86_64-windows/d3d11.dll"));
        assert!(!maps_have_graphics("7f00-7f10 r-xp 0 08:01 1 /usr/lib/x86_64-linux-gnu/libc.so.6"));
    }
}
