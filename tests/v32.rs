//! Lot Linux 2 : vocabulaire ADAPTE a Linux / SteamOS. Sous Linux, aucun texte d'interface ne parle d'« exe » (les
//! programmes Linux n'en ont pas), de l'Explorateur, du registre, de SmartScreen, du planificateur de taches ni d'une
//! autorisation administrateur, et « Windows » n'apparait que pour les programmes Windows lances par Proton / Wine.
//! Les textes Windows ne changent pas. Tests executes sur TOUTES les plateformes (`tr_linux`).
use agss::i18n::{linux_variant, tr_linux, tr_raw, Lang, ALL_KEYS, K};

/// Cles jamais affichees sous Linux (fonctions propres a Windows), avec la raison.
const WINDOWS_ONLY: &[(K, &str)] = &[
    (K::StartupBackground, "regles de demarrage Windows (taches planifiees) : sous Linux, un seul interrupteur d'autostart"),
    (K::TipStartupBackground, "idem : demarrage Windows"),
    (K::BtnAdvancedStartup, "demarrage avance Windows"),
    (K::TipAdvancedStartup, "demarrage avance Windows"),
    (K::ConfApplyStartup, "planificateur de taches Windows"),
    (K::LegacyRunFound, "cle Run du registre Windows"),
    (K::BtnMigrateRun, "cle Run du registre Windows"),
    (K::ConfMigrateRun, "cle Run du registre Windows"),
    (K::TipMigrateRun, "cle Run du registre Windows"),
    (K::StartupMigratePending, "taches planifiees Windows"),
    (K::BtnStartupMigrate, "taches planifiees Windows"),
    (K::TipStartupMigrate, "taches planifiees Windows"),
    (K::ConfMigrateTasks, "taches planifiees Windows"),
    (K::TipStartupToggle, "regles de demarrage Windows"),
    (K::TipStartupDelay, "regles de demarrage Windows"),
    (K::TipStartupMinimized, "regles de demarrage Windows"),
    (K::TipTestNow, "tache planifiee Windows"),
    (K::TipApplyStartup, "taches planifiees Windows"),
    (K::TipRemoveRule, "tache planifiee Windows"),
    (K::SetPadHelp, "lecture directe de la manette (Windows) : sous Linux, Steam Input s'en charge"),
    (K::TipPadState, "lecture directe de la manette (Windows)"),
    (K::TipPadMode, "lecture directe de la manette (Windows)"),
    (K::TipPadPrev, "boutons d'epaule de la lecture directe de la manette (Windows)"),
    (K::TipPadNext, "boutons d'epaule de la lecture directe de la manette (Windows)"),
    // 2.0 (lot 8) : conseil du mode « a la demande » de Google Drive pour Windows (Explorateur Windows) : jamais affiche sous Linux.
    (K::VVaultStreamBody, "conseil de l'Explorateur Windows pour Google Drive (le mode a la demande n'existe que sous Windows)"),
    // 2.0 (lot 9) : elements « Windows » du scan propres a Windows (registre, barre des taches, fond d'ecran de Windows).
    (K::VSysPrefs, "preferences du registre Windows (categorie prevue du scan, jamais listee sous Linux)"),
    (K::VSysTaskbar, "epingles de la barre des taches Windows (jamais listees sous Linux)"),
    (K::VSysWallpaper, "fond d'ecran de Windows (fichier TranscodedWallpaper, jamais liste sous Linux)"),
    // 2.0.2 (lot 15) : Reglages Windows (registre, Explorateur, fichier hosts avec droits administrateur, liste « A refaire »
    // du Nouveau PC sous Windows) : aucun de ces elements n'existe sous Linux (`winprefs::real()` = None).
    (K::VPrefTheme, "Reglages Windows"),
    (K::VPrefMouse, "Reglages Windows"),
    (K::VPrefKeyboard, "Reglages Windows"),
    (K::VPrefExplorer, "Reglages Windows (Explorateur)"),
    (K::VPrefTaskbar, "Reglages Windows"),
    (K::VPrefRegion, "Reglages Windows"),
    (K::VPrefSound, "Reglages Windows"),
    (K::VPrefPower, "Reglages Windows"),
    (K::VPrefEnv, "Reglages Windows"),
    (K::VPrefAssoc, "Reglages Windows"),
    (K::VPrefFonts, "Reglages Windows"),
    (K::VPrefOffice, "Reglages Windows"),
    (K::VPrefDict, "Reglages Windows"),
    (K::VPrefTerminal, "Reglages Windows"),
    (K::VPrefHosts, "Reglages Windows (droits administrateur)"),
    (K::VPrefQuick, "Reglages Windows (Explorateur)"),
    (K::VPrefTodo, "Reglages Windows"),
    (K::VPrefDevice, "Reglages Windows"),
    (K::VPrefAdmin, "Reglages Windows (droits administrateur)"),
    (K::VPrefPartial, "Reglages Windows"),
    (K::VPrefListOnly, "Reglages Windows"),
    (K::VPrefTitle, "Reglages Windows"),
    (K::VPrefLast, "Reglages Windows"),
    (K::VPrefOtherOs, "Reglages Windows (Windows 10 / 11)"),
    (K::VPrefFailed, "Reglages Windows"),
    (K::VPrefRedo, "Reglages Windows (Parametres)"),
    (K::VPrefOpenSettings, "Reglages Windows (Parametres)"),
    (K::VPrefDryRun, "Reglages Windows"),
    (K::VPrefHostsPending, "Reglages Windows"),
    (K::VPrefHostsBody, "Reglages Windows (droits administrateur)"),
    (K::VPrefHostsApply, "Reglages Windows (droits administrateur)"),
    (K::VPrefHostsCancelled, "Reglages Windows (droits administrateur)"),
    (K::VPrefRestoreNote, "Reglages Windows"),
    (K::VPrefOpenDrives, "Reglages Windows (Ce PC)"),
];

fn word(s: &str, w: &str) -> bool {
    let s = s.to_lowercase();
    let mut from = 0;
    while let Some(i) = s[from..].find(w) {
        let a = from + i;
        let b = a + w.len();
        let before = s[..a].chars().next_back().map(|c| c.is_alphanumeric()).unwrap_or(false);
        let after = s[b..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false);
        if !before && !after {
            return true;
        }
        from = b;
    }
    false
}

/// Raison du refus d'un texte affiche sous Linux, ou None.
fn linux_problem(l: Lang, s: &str) -> Option<&'static str> {
    let low = s.to_lowercase();
    // « exe » comme nom commun (« ton exe », « l'exe ») ; « .exe » seulement pour Proton / Wine.
    let mut t = low.clone();
    let proton = low.contains("proton") || low.contains("wine");
    if proton {
        t = t.replace(".exe", "");
    }
    if word(&t.replace(".exe", " exe"), "exe") {
        return Some("« exe » sous Linux (dire « programme »)");
    }
    if low.contains("windows") && !proton {
        return Some("« Windows » hors contexte Proton / Wine");
    }
    if low.contains("smartscreen") || low.contains("uac") && word(&low, "uac") {
        return Some("SmartScreen / UAC");
    }
    let bad: &[&str] = match l {
        Lang::Fr => &["explorateur", "registre", "tâche planifiée", "tâches planifiées", "planificateur", "administrateur"],
        Lang::En => &["explorer", "registry", "scheduled task", "task scheduler", "administrator"],
        Lang::Es => &["explorador", "registro de windows", "tarea programada", "tareas programadas", "programador de tareas", "administrador"],
        Lang::Ru => &["проводник", "реестр", "планировщик", "администратор"],
        Lang::Zh => &["资源管理器", "注册表", "计划任务", "任务计划", "管理员"],
    };
    // Mots entiers (« enregistre » n'est pas « registre ») ; le chinois n'a pas d'espaces : sous-chaine.
    bad.iter().any(|b| if l == Lang::Zh { low.contains(b) } else { word(&low, b) || low.contains(&format!("{}s", b)) && word(&low, &format!("{}s", b)) }).then_some("vocabulaire propre a Windows")
}

#[test]
fn vocabulaire_linux_adapte_dans_les_5_langues() {
    let skip: Vec<K> = WINDOWS_ONLY.iter().map(|(k, _)| *k).collect();
    let mut bad = Vec::new();
    for l in Lang::ALL {
        for k in ALL_KEYS {
            if skip.contains(k) {
                continue;
            }
            let s = tr_linux(l, *k);
            if let Some(why) = linux_problem(l, s) {
                bad.push(format!("{:?}/{:?} ({}) : {}", l, k, why, s.chars().take(100).collect::<String>()));
            }
        }
    }
    assert!(bad.is_empty(), "{} texte(s) inadaptes sous Linux :\n{}", bad.len(), bad.join("\n"));
    for (_, why) in WINDOWS_ONLY {
        assert!(!why.is_empty());
    }
}

#[test]
fn variantes_linux_completes_coherentes_et_idempotentes() {
    let mut n = 0;
    for k in ALL_KEYS {
        let lx = linux_variant(*k);
        assert_eq!(linux_variant(lx), lx, "{:?} : variante de variante", k);
        if lx == *k {
            continue;
        }
        n += 1;
        for l in Lang::ALL {
            let (base, var) = (tr_raw(l, *k), tr_raw(l, lx));
            assert!(!var.trim().is_empty(), "{:?}/{:?} vide", l, lx);
            // Memes emplacements {0} {1} {} (le code remplace ces marques).
            for m in ["{0}", "{1}", "{}"] {
                assert_eq!(base.matches(m).count(), var.matches(m).count(), "{:?}/{:?} : marque {} differente", l, lx, m);
            }
            // Meme ponctuation finale (infobulles terminees par un point...).
            assert_eq!(base.ends_with('.'), var.ends_with('.'), "{:?}/{:?} : ponctuation finale", l, lx);
        }
        assert_ne!(tr_raw(Lang::Fr, lx), tr_raw(Lang::En, lx), "{:?} identique en FR et EN", lx);
    }
    assert!(n >= 30, "{} variantes Linux seulement", n);
    // Exemples de reference.
    assert_eq!(tr_linux(Lang::Fr, K::AddStep1), "Choisis ton programme ou son raccourci");
    assert_eq!(tr_linux(Lang::En, K::ThemeSystem), "Follow the system");
    assert_eq!(tr_linux(Lang::Fr, K::VStartHint), "À l'ouverture de ta session de bureau, discrètement en arrière-plan.");
    assert_eq!(tr_linux(Lang::Zh, K::SetNotif), "备份后显示桌面通知");
    assert_eq!(tr_linux(Lang::Ru, K::LocTitle), "Расположение программы");
    assert_eq!(tr_linux(Lang::Es, K::CardAnimAuto), "Auto (sigue al escritorio)");
}

// ------------------------------------------------------------------ console portable, raccourcis d'onglets

#[test]
fn console_portable_et_mode_jeu_reconnus() {
    use agss::uilogic::handheld_from;
    let none = |_: &str| -> Option<String> { None };
    assert!(!handheld_from(&none, "Micro-Star International", "MS-7C02"));
    assert!(handheld_from(&none, "Valve", "Jupiter"));
    assert!(handheld_from(&none, "LENOVO", "83L3"));
    let deck = |k: &str| if k == "SteamDeck" { Some("1".to_string()) } else { None };
    assert!(handheld_from(&deck, "", ""));
    let off = |k: &str| if k == "SteamDeck" { Some("0".to_string()) } else { None };
    assert!(!handheld_from(&off, "", ""));
    let gs = |k: &str| if k == "XDG_CURRENT_DESKTOP" { Some("gamescope".to_string()) } else { None };
    assert!(handheld_from(&gs, "", ""));
}

#[test]
fn raccourcis_d_onglets() {
    use agss::uilogic::tab_shortcut;
    assert_eq!(tab_shortcut("Tab", true, false), 1);
    assert_eq!(tab_shortcut("Tab", true, true), -1);
    assert_eq!(tab_shortcut("PageDown", true, false), 1);
    assert_eq!(tab_shortcut("PageUp", true, false), -1);
    assert_eq!(tab_shortcut("Tab", false, false), 0, "Tab seul deplace le focus, il ne change pas d'onglet");
    assert_eq!(tab_shortcut("A", true, false), 0);
}

// ------------------------------------------------------------------ noms de machines Linux

#[test]
fn noms_de_machines_linux_distincts() {
    use agss::util::linux_machine_name;
    assert_eq!(linux_machine_name("steamdeck", "3f2a9c0b11e94d7a8c6f5e4d3c2b1a00\n"), "steamdeck-3f2a");
    assert_eq!(linux_machine_name("steamdeck", "9b1e00000000000000000000000000aa"), "steamdeck-9b1e", "deux Steam Deck ne se confondent pas");
    assert_eq!(linux_machine_name("LEGION-GO", "3f2a9c0b"), "LEGION-GO", "un nom choisi reste tel quel");
    assert_eq!(linux_machine_name("bazzite", ""), "bazzite", "sans machine-id : nom d'hote seul");
}

// ------------------------------------------------------------------ Heroic

#[test]
fn jeux_heroic_installes() {
    let t = tempfile::TempDir::new().unwrap();
    let h = t.path();
    let leg = h.join(".config/heroic/legendaryConfig/legendary");
    std::fs::create_dir_all(&leg).unwrap();
    std::fs::write(leg.join("installed.json"), r#"{"Fortnite":{"app_name":"Fortnite","title":"Fortnite","install_path":"/home/deck/Games/Heroic/Fortnite","executable":"FortniteGame/Binaries/Win64/FortniteLauncher.exe"},"abc":{"app_name":"abc","title":"Celeste","install_path":"/home/deck/Games/Heroic/Celeste","executable":"Celeste.exe"}}"#).unwrap();
    let gog = h.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/gog_store");
    std::fs::create_dir_all(&gog).unwrap();
    let game = h.join("Games/Heroic/Stardew Valley");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("goggame-1453375253.info"), r#"{"playTasks":[{"isPrimary":true,"path":"Stardew Valley.exe"}]}"#).unwrap();
    std::fs::write(gog.join("installed.json"), format!(r#"{{"installed":[{{"appName":"1453375253","install_path":"{}","platform":"windows"}}]}}"#, game.display().to_string().replace('\\', "\\\\"))).unwrap();
    let mut v = agss::detect::heroic_games(h);
    v.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<&str> = v.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, vec!["Celeste", "Fortnite", "Stardew Valley"]);
    assert_eq!(v[0].exe.as_deref(), Some("Celeste.exe"));
    assert_eq!(v[0].store, agss::detect::Store::Epic);
    assert_eq!(v[2].store, agss::detect::Store::Gog);
    assert_eq!(v[2].exe.as_deref(), Some("Stardew Valley.exe"));
    assert!(agss::detect::heroic_games(&h.join("absent")).is_empty());
}

// ------------------------------------------------------------------ detection Linux (processus vus dans /proc)

#[cfg(not(windows))]
mod detection_linux {
    use agss::gamedetect::{classify, DbHit, Env, Why};
    use agss::process::ProcInfo;
    use std::path::PathBuf;

    fn p(exe: &str, path: &str) -> ProcInfo {
        ProcInfo { pid: 1, exe: exe.to_lowercase(), path: if path.is_empty() { None } else { Some(PathBuf::from(path)) }, start: 1 }
    }

    fn run(pr: &ProcInfo) -> Option<agss::gamedetect::Candidate> {
        let db = |e: &str| -> Vec<DbHit> { if e == "eldenring.exe" { vec![DbHit { name: "ELDEN RING".into(), install_dirs: vec!["ELDEN RING".into()] }] } else { vec![] } };
        let roots = vec!["/home/deck/games".to_string()];
        let env = Env { db: &db, launcher_roots: &roots, known_exes: &[] };
        classify(pr, &env)
    }

    #[test]
    fn jeux_proton_heroic_bottles_et_lutris_proposes() {
        let c = run(&p("eldenring.exe", "/home/deck/.local/share/Steam/steamapps/common/ELDEN RING/Game/eldenring.exe")).unwrap();
        assert_eq!((c.why, c.name.as_str()), (Why::InDb, "ELDEN RING"));
        let h = run(&p("celeste.exe", "/home/deck/Games/Heroic/Celeste/Celeste.exe")).unwrap();
        assert_eq!((h.why, h.name.as_str()), (Why::LauncherFolder, "Celeste"));
        let b = run(&p("hades.exe", "/home/deck/.var/app/com.usebottles.bottles/data/bottles/bottles/Jeux/drive_c/Program Files/Hades/x64/Hades.exe")).unwrap();
        assert_eq!(b.name, "Hades");
        let l = run(&p("factorio", "/home/deck/Games/factorio/bin/x64/factorio")).unwrap();
        assert_eq!((l.why, l.name.as_str()), (Why::LauncherFolder, "factorio"));
        let n = run(&p("hollow_knight.x86_64", "/home/deck/.local/share/Steam/steamapps/common/Hollow Knight/hollow_knight.x86_64")).unwrap();
        assert_eq!(n.name, "Hollow Knight");
    }

    #[test]
    fn faux_positifs_linux_exclus() {
        for (exe, path) in [
            ("wineserver", "/home/deck/.steam/steam/steamapps/common/Proton 9.0 (Beta)/files/bin/wineserver"),
            ("services.exe", "/home/deck/.steam/steam/steamapps/compatdata/42/pfx/drive_c/windows/system32/services.exe"),
            ("steam.exe", "/home/deck/.steam/steam/steamapps/compatdata/42/pfx/drive_c/windows/system32/steam.exe"),
            ("pressure-vessel-wrap", "/home/deck/.steam/steam/steamapps/common/SteamLinuxRuntime_sniper/pressure-vessel/bin/pressure-vessel-wrap"),
            ("reaper", "/home/deck/.local/share/Steam/ubuntu12_32/reaper"),
            ("steamwebhelper", "/home/deck/.local/share/Steam/ubuntu12_64/steamwebhelper"),
            ("dolphin", "/usr/bin/dolphin"),
            ("bash", "/home/deck/Games/jeu/start.sh"),
            ("gamescope", "/usr/bin/gamescope"),
            ("cemu", "/tmp/.mount_CemuAb12/usr/bin/Cemu"),
            ("winedevice.exe", "/home/deck/Games/Heroic/Prefixes/default/Jeu/drive_c/windows/system32/winedevice.exe"),
            ("iexplore.exe", "/home/deck/Games/Heroic/Prefixes/default/Jeu/drive_c/Program Files/Internet Explorer/iexplore.exe"),
            ("plasmashell", "/usr/bin/plasmashell"),
            ("autogamesoftwaresync", "/home/deck/.local/bin/AutoGameSoftwareSync"),
        ] {
            assert!(run(&p(exe, path)).is_none(), "{} ({}) ne doit pas etre propose", exe, path);
        }
    }

    #[test]
    fn emulateurs_linux_flatpak_et_appimage() {
        let d = run(&p("dolphin-emu", "/usr/bin/dolphin-emu")).unwrap();
        assert_eq!(d.why, Why::Emulator);
        assert_eq!(d.install_dir, Some(PathBuf::from("/usr/bin/dolphin-emu")), "un programme de /usr/bin est son propre element");
        let r = run(&p("retroarch", "/var/lib/flatpak/app/org.libretro.RetroArch/x86_64/stable/abc/files/bin/retroarch")).unwrap();
        assert_eq!(r.why, Why::Emulator);
        let ppsspp = run(&p("ppssppsdl", "/var/lib/flatpak/app/org.ppsspp.PPSSPP/current/active/files/bin/PPSSPPSDL")).unwrap();
        assert_eq!(ppsspp.why, Why::Emulator);
        let a = run(&p("cemu.appimage", "/home/deck/Applications/Cemu.AppImage")).unwrap();
        let b = run(&p("rpcs3.appimage", "/home/deck/Applications/rpcs3.AppImage")).unwrap();
        assert_eq!((a.why, a.name.as_str()), (Why::Emulator, "cemu"));
        assert_ne!(a.key, b.key);
        assert_ne!(a.install_dir, b.install_dir, "deux AppImage du meme dossier restent deux elements distincts");
    }
}

#[test]
fn textes_windows_inchanges() {
    // La table de base garde exactement les textes Windows ; seul Linux voit les variantes.
    assert_eq!(tr_raw(Lang::Fr, K::AddStep1), "Choisis ton raccourci ou ton exe");
    assert_eq!(tr_raw(Lang::Fr, K::ThemeSystem), "Suivre Windows");
    assert_eq!(tr_raw(Lang::En, K::SetNotif), "Windows notifications after a backup");
    assert_eq!(tr_raw(Lang::Fr, K::VStartHint), "Au démarrage de Windows, discrètement en arrière-plan.");
    for l in Lang::ALL {
        for k in ALL_KEYS {
            #[cfg(windows)]
            assert_eq!(agss::i18n::tr(l, *k), tr_raw(l, *k), "{:?}/{:?} : Windows doit rester identique", l, k);
            #[cfg(not(windows))]
            assert_eq!(agss::i18n::tr(l, *k), tr_linux(l, *k), "{:?}/{:?}", l, k);
        }
    }
}
