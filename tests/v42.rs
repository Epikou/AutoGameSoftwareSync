//! 2.0 lot 9 : scan du PC. Donnees d'usage `scan_done` (tranches, consentement), outils de developpement et programmes du
//! systeme jamais recommandes (noms reels releves sur un PC de developpement), dernier scan memorise, textes.
//! Le classement complet, la liste noire, « deja protege », les nouveautes, l'ajout en lot et l'annulation sont testes
//! sur un PC simule dans `src/scan/tests.rs`.
use agss::i18n::{tr_linux, tr_raw, Lang, K};
use agss::scan::{self, AppEntry, Hide, LastScan};
use agss::usage::{self, Event};

#[test]
fn scan_done_en_tranches_et_seulement_avec_consentement() {
    let t = tempfile::tempdir().unwrap();
    let now = agss::util::now_ms();
    for (ms, proposed, accepted, want) in [(3_000, 0, 0, ["<10s", "0", "0"]), (12_000, 9, 5, ["10-30s", "6-20", "1-5"]), (200_000, 80, 30, ["3-10m", "21+", "21+"])] {
        let p = scan::scan_done_props(ms, proposed, accepted);
        assert_eq!([p[0].1, p[1].1, p[2].1], want);
        let e = Event::new("scan_done", &p, now);
        assert!(usage::validate(&e, now).is_ok(), "{:?}", e);
    }
    let e = Event::new("scan_done", &scan::scan_done_props(5_000, 3, 1), now);
    assert!(!usage::push(t.path(), e.clone(), None), "question pas encore posee : rien");
    assert!(!usage::push(t.path(), e.clone(), Some(false)), "refus : rien");
    assert!(usage::push(t.path(), e, Some(true)));
    // Aucun nom, aucun chemin : seulement des tranches.
    let bad = Event::new("scan_done", &[("duration", "12 s"), ("proposed", "Hollow Knight"), ("accepted", "1-5")], now);
    assert!(usage::validate(&bad, now).is_err());
}

#[test]
fn outils_de_developpement_et_systeme_jamais_recommandes() {
    let e = |n: &str, p: Option<&str>| AppEntry { name: n.into(), publisher: p.map(|s| s.into()), ..Default::default() };
    // Noms reels de la liste des logiciels installes d'un PC de developpement.
    let dev = [
        ("Git", Some("The Git Development Community")),
        ("GitHub CLI", Some("GitHub, Inc.")),
        ("Rustup: the Rust toolchain installer", None),
        ("Node.js", Some("Node.js Foundation")),
        ("Python 3.12.1 (64-bit)", Some("Python Software Foundation")),
        ("Docker Desktop", Some("Docker Inc.")),
        ("Microsoft Visual Studio Installer", Some("Microsoft Corporation")),
        ("Visual Studio Community 2022", Some("Microsoft Corporation")),
        ("Microsoft .NET SDK 9.0.318 (x64) from Visual Studio", Some("Microsoft Corporation")),
        ("Unity Hub 3.21.3", Some("Unity Technologies Inc.")),
        ("Windows SDK AddOn", Some("Microsoft Corporation")),
        ("JetBrains Rider 2025.1", Some("JetBrains s.r.o.")),
        ("CMake", Some("Kitware")),
        ("Eclipse Temurin JDK with Hotspot 21", Some("Eclipse Adoptium")),
    ];
    for (n, p) in dev {
        assert!(matches!(scan::app_hide(&e(n, p)), Some(Hide::DevTool) | Some(Hide::System)), "{} doit etre masque", n);
    }
    let sys = [
        ("Microsoft Visual C++ 2015-2022 Redistributable (x64) - 14.40.33810", Some("Microsoft Corporation")),
        ("Microsoft Windows Desktop Runtime - 8.0.22 (x64)", Some("Microsoft Corporation")),
        ("NVIDIA Pilote graphique 617.42", Some("NVIDIA Corporation")),
        ("Realtek Audio Driver", Some("Realtek Semiconductor Corp.")),
        ("AMD Chipset Software", Some("Advanced Micro Devices, Inc.")),
        ("Microsoft GameInput", Some("Microsoft Corporation")),
    ];
    for (n, p) in sys {
        assert_eq!(scan::app_hide(&e(n, p)), Some(Hide::System), "{}", n);
    }
    for n in ["Steam", "Epic Games Launcher", "GOG GALAXY", "EA app", "Ubisoft Connect", "Battle.net", "Playnite"] {
        assert_eq!(scan::app_hide(&e(n, None)), Some(Hide::Launcher), "{}", n);
    }
    for n in ["Google Chrome", "Mozilla Firefox (x64 fr)", "Microsoft Edge", "Opera GX Stable 137.0", "KeePassXC", "Bitwarden"] {
        assert_eq!(scan::app_hide(&e(n, None)), Some(Hide::Sensitive), "{}", n);
    }
    // Vrais logiciels : jamais masques d'office.
    for n in ["Paint.NET", "VLC media player", "OBS Studio", "Audacity", "GIMP 3.0.6-1", "qBittorrent", "ShareX", "Digital Paint", "TreeSize Free V4.8.1", "Discord"] {
        assert_eq!(scan::app_hide(&e(n, Some("Someone"))), None, "{}", n);
    }
}

#[test]
fn dernier_scan_memorise_et_fusionne() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(LastScan::load(t.path()), LastScan::default(), "absent : vide");
    let a = LastScan { at_ms: 10, keys: vec!["game:hollowknight".into(), "folder:documents".into()] };
    a.save(t.path()).unwrap();
    assert_eq!(LastScan::load(t.path()), a);
    let b = LastScan { at_ms: 20, keys: vec!["app:notepad".into(), "game:hollowknight".into()] };
    let m = a.merged(&b);
    assert_eq!(m.at_ms, 20);
    assert_eq!(m.keys, vec!["app:notepad", "folder:documents", "game:hollowknight"]);
    std::fs::write(t.path().join(scan::LAST_FILE), b"{casse").unwrap();
    assert_eq!(LastScan::load(t.path()), LastScan::default(), "fichier illisible : vide, jamais d'erreur");
}

#[test]
fn textes_du_scan_dans_les_5_langues() {
    let keys = [
        K::VScanTitle, K::VScanCatGames, K::VScanCatApps, K::VScanCatFolders, K::VScanCatSystem, K::VScanRecommended, K::VScanMaybe, K::VScanHidden, K::VScanProtect,
        K::VScanDoneMany, K::VHideDev, K::VHideSensitive, K::VSysDesktop, K::VPersPictures,
    ];
    for l in Lang::ALL {
        for k in keys {
            assert!(!tr_raw(l, k).trim().is_empty(), "{:?} {:?}", l, k);
        }
        // Sous Linux, la categorie « Windows » devient le bureau.
        assert!(!tr_linux(l, K::VScanCatSystem).contains("Windows"), "{:?}", l);
        assert!(!tr_linux(l, K::VWhySystem).contains("Windows"), "{:?}", l);
    }
    assert_eq!(tr_raw(Lang::Fr, K::VScanProtect), "Protéger la sélection ({})");
    assert_eq!(tr_raw(Lang::Fr, K::VScanRecommended), "Recommandé");
    assert_eq!(tr_raw(Lang::Fr, K::VScanMaybe), "Peut-être");
    assert_eq!(tr_raw(Lang::Fr, K::VScanHidden), "Masqués");
}
