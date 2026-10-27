//! 2.0.2 (lot 15) : Reglages Windows. Liste noire garantie, tout decoche par defaut, copie de securite AVANT toute
//! restauration (et « Annuler »), Windows 10 <-> 11 sans perte ni echange sans fin, aller-retour export / restauration
//! sur une CLE DE TEST (`HKCU\Software\AGSS-Test\...`), fichier hosts jamais ecrit en silence, applications par defaut
//! (choix protege jamais ecrit), mode d'alimentation propre a l'appareil, liste « A refaire » sans identifiant.
//! JAMAIS le vrai registre en ecriture : systeme en memoire (`MemSys`) ou cle de test ; dossiers temporaires.
use agss::backup;
use agss::config::Game;
use agss::i18n::{tr_raw, Lang, K, SIMPLE_KEYS};
use agss::knowledge::paths::Folders;
use agss::process::ProcessChecker;
use agss::winprefs::{self as wp, Data, Entry, Kind, MemSys, Os, PrefFile, Root, Sys};
use agss::{Ctx, Res};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

const MOUSE: &str = r"Control Panel\Mouse";
const ADV: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

fn folders(root: &Path, user: &str) -> Folders {
    let p = root.join("Users").join(user);
    Folders { local: Some(p.join("AppData").join("Local")), roaming: Some(p.join("AppData").join("Roaming")), documents: Some(p.join("Documents")), profile: Some(p.clone()), home: Some(p), ..Default::default() }
}

fn mem(root: &Path, user: &str, build: u32) -> Arc<MemSys> {
    let hosts = root.join(format!("hosts-{}", user));
    Arc::new(MemSys::new(build, folders(root, user), hosts))
}

/// Un appareil : dossier de donnees, systeme simule.
fn device(root: &Path, name: &str, sys: Arc<MemSys>) -> Ctx {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    ctx.keep_versions = 3;
    ctx.sys = Some(sys);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    ctx
}

fn pref_game(ctx: &Ctx, k: Kind) -> Game {
    wp::prepare(&ctx.data_dir, k, wp::sys_of(ctx).as_deref());
    let mut g = Game { name: k.stable().into(), path: wp::staging(&ctx.data_dir, k), schedule: Some(agss::scan::default_schedule()), ..Default::default() };
    if k.device_specific() {
        g.dest_key = Some(wp::device_dest(k, &ctx.machine));
    }
    g
}

fn mouse_values(s: &MemSys, speed: &str, dbl: &str) {
    s.set(Root::Hkcu, MOUSE, "MouseSensitivity", Data::Sz(speed.into()));
    s.set(Root::Hkcu, MOUSE, "DoubleClickSpeed", Data::Sz(dbl.into()));
    s.set(Root::Hkcu, MOUSE, "SwapMouseButtons", Data::Sz("0".into()));
}

fn sleep() {
    std::thread::sleep(std::time::Duration::from_millis(20));
}

// ================================================================== liste noire

#[test]
fn liste_noire_jamais_lue_ni_ecrite() {
    // 1. Aucune cle de la liste blanche ne touche a la liste noire.
    for k in Kind::ALL {
        for sp in wp::specs(k) {
            assert!(!wp::forbidden(sp.path), "{:?} : {}", k, sp.path);
            let low = sp.path.to_lowercase();
            for w in ["credential", "password", "certificate", "cryptography", "wlan", "license", "licence", "productkey", "identitycrl", "\\services", "driver", "cookie", "chrome", "firefox", "edge"] {
                assert!(!low.contains(w), "{:?} : {} contient {}", k, sp.path, w);
            }
        }
    }
    for f in [
        r"HKCU\Software\Microsoft\SystemCertificates\My",
        r"Software\Microsoft\Protect\S-1-5",
        r"HKCU\Software\Microsoft\IdentityCRL\StoredIdentities",
        r"Software\Microsoft\Windows NT\CurrentVersion\SoftwareProtectionPlatform",
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\DigitalProductId",
        r"SYSTEM\CurrentControlSet\Services\nvlddmkm",
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        r"Software\Google\Chrome\PreferenceMACs",
        r"Software\Microsoft\Office\16.0\Registration",
    ] {
        assert!(wp::forbidden(f), "{}", f);
    }
    // 2. Un fichier venu du coffre qui sort de la liste blanche n'est JAMAIS ecrit (demarrage automatique, comptes...).
    let tmp = TempDir::new().unwrap();
    let s = mem(tmp.path(), "Player", 22631);
    mouse_values(&s, "10", "500");
    let mut pf = PrefFile::new(Kind::Mouse);
    pf.entries.push(Entry { key: format!("HKCU\\{}", MOUSE), name: "MouseSensitivity".into(), value: Data::Sz("15".into()), tpl: false, os: Os::Any });
    pf.entries.push(Entry { key: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run".into(), name: "evil".into(), value: Data::Sz("c:\\evil.exe".into()), tpl: false, os: Os::Any });
    pf.entries.push(Entry { key: r"HKCU\Software\Microsoft\IdentityCRL".into(), name: "x".into(), value: Data::Sz("y".into()), tpl: false, os: Os::Any });
    pf.entries.push(Entry { key: format!("HKCU\\{}", ADV), name: "Hidden".into(), value: Data::Dword(1), tpl: false, os: Os::Any });
    pf.entries.push(Entry { key: format!("HKLM\\{}", MOUSE), name: "MouseSensitivity".into(), value: Data::Sz("1".into()), tpl: false, os: Os::Any });
    let mut unw = BTreeSet::new();
    let r = wp::apply(&*s, Kind::Mouse, &pf, &mut unw);
    assert_eq!((r.applied, r.refused), (1, 4), "{:?}", r);
    let writes = s.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 1);
    assert!(writes[0].0.ends_with(MOUSE) && writes[0].1 == "MouseSensitivity");
    // 3. Variables d'environnement : jetons, cles et mots de passe jamais copies.
    let path_value = format!(r"{}\bin;C:\Tools", tmp.path().join("Users").join("Player").display());
    for (n, v) in [("PATH", path_value.as_str()), ("EDITOR", "code"), ("GITHUB_TOKEN", "ghp_x"), ("OPENAI_API_KEY", "sk-1"), ("DB_PASSWORD", "p"), ("AWS_SECRET_ACCESS_KEY", "a"), ("MY_PAT", "b"), ("SOMETHING", "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789")] {
        s.set(Root::Hkcu, "Environment", n, Data::Sz(v.into()));
    }
    let env = wp::export(&*s, Kind::EnvVars).unwrap();
    let names: Vec<&str> = env.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["EDITOR", "PATH"], "{:?}", names);
    assert!(env.entries.iter().find(|e| e.name == "PATH").unwrap().tpl, "dossier de l'utilisateur ramene a un gabarit");
    // 4. Liste « A refaire » : lettre + chemin des lecteurs (jamais l'identifiant), NOMS des Wi-Fi seulement.
    s.set(Root::Hkcu, r"Network\Z", "RemotePath", Data::Sz(r"\\nas\partage".into()));
    s.set(Root::Hkcu, r"Network\Z", "UserName", Data::Sz("utilisateur-secret".into()));
    s.set(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\Print\Printers\Bureau", "Port", Data::Sz("IP_192.168.1.20".into()));
    s.set(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\Print\Printers\Bureau", "Printer Driver", Data::Sz("Brother HL-L2350DW".into()));
    s.command("netsh wlan show profiles", "Profils sur l'interface Wi-Fi :\n\nProfils utilisateurs\n---------------------\n    Profil Tous les utilisateurs : Maison\n    Profil Tous les utilisateurs : Café du coin\n    Contenu de la clé            : motdepasse123\n    Key Content            : hunter2\n");
    let todo = wp::export(&*s, Kind::TodoList).unwrap();
    let json = String::from_utf8(todo.to_bytes()).unwrap();
    assert!(!json.contains("utilisateur-secret") && !json.contains("hunter2") && !json.contains("motdepasse123"), "{}", json);
    let t = todo.todo.unwrap();
    assert_eq!(t.wifi, vec!["Café du coin".to_string(), "Maison".to_string()]);
    assert_eq!((t.drives[0].letter.as_str(), t.drives[0].path.as_str()), ("Z:", r"\\nas\partage"));
    assert_eq!((t.printers[0].name.as_str(), t.printers[0].driver.as_str()), ("Bureau", "Brother HL-L2350DW"));
    // ... et la liste n'est jamais remise.
    let r = wp::apply(&*s, Kind::TodoList, &todo_file(&t), &mut unw);
    assert_eq!((r.applied, r.effect), (0, wp::Effect::Never));
    // 5. Le code ne demande jamais une cle Wi-Fi en clair, ni le gestionnaire d'identifiants, ni une cle de produit.
    let clear = ["key", "=clear"].concat();
    let wlan_export = ["wlan", " export"].concat();
    for f in ["src/winprefs.rs", "src/gui/winprefs_ui.rs", "src/migrate.rs", "src/migrate/inventory.rs", "src/scan/sources.rs"] {
        let code = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(f)).unwrap();
        for bad in [clear.as_str(), wlan_export.as_str(), "CredRead", "CredEnumerate", "CertOpenStore", "DigitalProductId", "PFXExport", "Login Data"] {
            assert!(!code.contains(bad), "{} contient {}", f, bad);
        }
    }
}

fn todo_file(t: &wp::TodoList) -> PrefFile {
    let mut p = PrefFile::new(Kind::TodoList);
    p.todo = Some(t.clone());
    p
}

// ================================================================== decoche par defaut

#[test]
fn reglages_windows_decoches_partout_et_noms_stables() {
    let tmp = TempDir::new().unwrap();
    let s = mem(tmp.path(), "Player", 22631);
    mouse_values(&s, "10", "500");
    s.set(Root::Hkcu, ADV, "HideFileExt", Data::Dword(0));
    s.set(Root::Hkcu, ADV, "TaskbarAl", Data::Dword(0));
    s.set(Root::Hkcu, r"Control Panel\International", "sShortDate", Data::Sz("dd/MM/yyyy".into()));
    let dd = tmp.path().join("data");
    std::fs::create_dir_all(&dd).unwrap();
    let offers = wp::offers(&dd, &*s);
    let kinds: Vec<Kind> = offers.iter().map(|o| o.kind).collect();
    for k in [Kind::Mouse, Kind::Explorer, Kind::Taskbar, Kind::Region] {
        assert!(kinds.contains(&k), "{:?} absent de {:?}", k, kinds);
    }
    assert!(!kinds.contains(&Kind::Sound), "rien a proteger = pas propose");
    // Emplacement principal = export local, lisible.
    let m = offers.iter().find(|o| o.kind == Kind::Mouse).unwrap();
    assert_eq!(m.locs[0].path, wp::staging(&dd, Kind::Mouse));
    assert_eq!(m.count, 3);
    let pf = PrefFile::read(&m.locs[0].path.join(wp::FILE)).unwrap();
    assert!(pf.entries.iter().any(|e| e.name == "MouseSensitivity" && e.value == Data::Sz("10".into())));
    // Meme etat = meme fichier (aucune date, aucun appareil dedans) : un nouvel export ne change rien.
    let before = std::fs::read(m.locs[0].path.join(wp::FILE)).unwrap();
    assert!(!wp::refresh_kind(&dd, &*s, Kind::Mouse, true));
    assert_eq!(before, std::fs::read(m.locs[0].path.join(wp::FILE)).unwrap());
    // Noms stables (cles du coffre) distincts, codes retrouves.
    let keys: BTreeSet<String> = Kind::ALL.iter().map(|k| agss::cloud::game_key(k.stable())).collect();
    assert_eq!(keys.len(), Kind::ALL.len());
    for k in Kind::ALL {
        assert_eq!(Kind::from_code(k.code()), Some(k));
        assert_eq!(Kind::from_stable(k.stable()), Some(k));
        assert_eq!(wp::kind_of_key(&agss::cloud::game_key(k.stable())), Some(k));
    }
    assert_eq!(wp::kind_of_key(&agss::cloud::game_key(&wp::device_dest(Kind::Power, "Desktop PC"))), Some(Kind::Power));
    assert_eq!(wp::kind_of_key("powerplantsimulator"), None, "pas de confusion avec un jeu");
    // Nouveau PC : les reglages Windows sont retrouves par leur nom, DECOCHES ; ceux d'un autre appareil sont ignores.
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let a = device(tmp.path(), "DESKTOP", s.clone());
    let g = pref_game(&a, Kind::Mouse);
    backup::backup(&a, &g, &vault).unwrap();
    let p = pref_game(&a, Kind::Power);
    s.command("powercfg /getactivescheme", "GUID du mode de gestion de l'alimentation : 381b4222-f694-41f0-9685-ff5bb260df2e  (Utilisation normale)");
    wp::refresh_kind(&a.data_dir, &*s, Kind::Power, true);
    backup::backup(&a, &p, &vault).unwrap();
    let b_sys = mem(tmp.path(), "Bob", 22631);
    let b = device(tmp.path(), "Handheld", b_sys);
    let mut here = agss::migrate::Here::empty(true);
    here.personal = agss::migrate::pref_known(&b.data_dir, true);
    let devs = agss::migrate::devices(&vault, "ALLY", &Default::default());
    let d = devs.iter().find(|d| d.name == "DESKTOP").unwrap();
    assert_eq!(d.todo, 1, "le mode d'alimentation d'un autre appareil n'est jamais propose");
    let plans = agss::migrate::check(&vault, d, &Default::default(), &here, None, &|_, _| {});
    assert_eq!(plans.len(), 1, "{:?}", plans.iter().map(|p| &p.name).collect::<Vec<_>>());
    assert_eq!(plans[0].name, Kind::Mouse.stable());
    if cfg!(windows) {
        // (sous Linux, la version de test est ecrite par un appareil Linux : « emplacement a choisir », comme tout element systeme)
        assert_eq!(plans[0].main.as_ref().unwrap().path, wp::staging(&b.data_dir, Kind::Mouse));
    }
    assert!(!plans[0].checked, "element Windows decoche au Nouveau PC");
    // Sous Linux, aucun reglage Windows n'est propose.
    let mut lx = agss::migrate::Here::empty(false);
    lx.personal = agss::migrate::pref_known(&b.data_dir, false);
    assert!(lx.personal.is_empty());
    assert!(agss::migrate::check(&vault, d, &Default::default(), &lx, None, &|_, _| {}).is_empty());
}

// ================================================================== copie de securite avant restauration

#[test]
fn copie_de_securite_avant_restauration_puis_annuler() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let s = mem(tmp.path(), "Player", 22631);
    mouse_values(&s, "10", "500");
    let ctx = device(tmp.path(), "Desktop PC", s.clone());
    let g = pref_game(&ctx, Kind::Mouse);
    backup::backup(&ctx, &g, &vault).unwrap();
    let v1 = backup::list_versions(&vault, &g).last().unwrap().file_name().unwrap().to_string_lossy().to_string();
    // L'utilisateur change sa souris ; la protection copie la nouvelle version.
    sleep();
    mouse_values(&s, "18", "300");
    assert_eq!(backup::plan(&ctx, &g, &vault).unwrap(), backup::Plan::Backup, "le changement du registre est vu");
    backup::backup(&ctx, &g, &vault).unwrap();
    // Il change encore (pas encore copie), puis remonte le temps vers v1.
    sleep();
    s.set(Root::Hkcu, MOUSE, "MouseSensitivity", Data::Sz("4".into()));
    let (out, ticket) = backup::restore_version_undoable(&ctx, &g, &vault, &v1).unwrap();
    // AVANT : copie de securite de l'etat ACTUEL de Windows (les memes cles).
    let safety = out.safety_dir.clone().expect("copie de securite");
    let sp = PrefFile::read(&safety.join("files").join(wp::FILE)).unwrap();
    assert!(sp.entries.iter().any(|e| e.name == "MouseSensitivity" && e.value == Data::Sz("4".into())), "etat actuel copie avant");
    // APRES : registre remis (seules les valeurs listees), notification a Windows.
    assert_eq!(s.get(Root::Hkcu, MOUSE, "MouseSensitivity"), Some(Data::Sz("10".into())));
    assert_eq!(s.get(Root::Hkcu, MOUSE, "DoubleClickSpeed"), Some(Data::Sz("500".into())));
    assert!(s.notices.lock().unwrap().contains(&wp::Notice::Mouse));
    let rep = wp::load_local(&ctx.data_dir, Kind::Mouse).report.unwrap();
    assert_eq!((rep.applied, rep.same, rep.other_os, rep.failed), (2, 1, 0, 0), "{:?}", rep);
    // « Annuler » : l'etat d'avant revient a l'identique.
    backup::undo_restore(&ctx, &g, Some(&vault), &ticket).unwrap();
    assert_eq!(s.get(Root::Hkcu, MOUSE, "MouseSensitivity"), Some(Data::Sz("4".into())));
    assert_eq!(s.get(Root::Hkcu, MOUSE, "DoubleClickSpeed"), Some(Data::Sz("300".into())));
    // Aucune ecriture hors de la liste.
    assert!(s.writes.lock().unwrap().iter().all(|w| w.0.ends_with(MOUSE)));
}

#[test]
fn synchro_entre_deux_appareils_sans_echange_sans_fin() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let sa = mem(tmp.path(), "Alice", 19045);
    let sb = mem(tmp.path(), "Bob", 22631);
    mouse_values(&sa, "12", "450");
    mouse_values(&sb, "10", "500");
    // Pointeur personnalise dans le dossier de l'utilisateur : chemin ramene a %USERPROFILE% puis developpe chez Bob.
    let cur_a = format!(r"{}\Cursors\fleche.cur", tmp.path().join("Users").join("Alice").display());
    sa.set(Root::Hkcu, r"Control Panel\Cursors", "Arrow", Data::Expand(cur_a));
    sb.add_key(Root::Hkcu, r"Control Panel\Cursors");
    let a = device(tmp.path(), "PCA", sa.clone());
    let b = device(tmp.path(), "PCB", sb.clone());
    let ga = pref_game(&a, Kind::Mouse);
    let gb = pref_game(&b, Kind::Mouse);
    backup::backup(&a, &ga, &vault).unwrap();
    // B recoit la version de A.
    assert_eq!(backup::plan(&b, &gb, &vault).unwrap(), backup::Plan::Conflict, "premiere fois : deux etats differents");
    let r = backup::restore(&b, &gb, &vault).unwrap();
    assert!(r.safety_dir.is_some());
    assert_eq!(sb.get(Root::Hkcu, MOUSE, "MouseSensitivity"), Some(Data::Sz("12".into())));
    let want = format!(r"{}\Cursors\fleche.cur", tmp.path().join("Users").join("Bob").display());
    assert_eq!(sb.get(Root::Hkcu, r"Control Panel\Cursors", "Arrow"), Some(Data::Expand(want)));
    // L'export suivant de B est identique a la version recue : rien de nouveau a copier, ni chez A.
    let p = backup::plan(&b, &gb, &vault).unwrap();
    assert!(matches!(p, backup::Plan::Nothing | backup::Plan::InSync), "{:?}", p);
    let n = backup::list_versions(&vault, &ga).len();
    backup::backup(&b, &gb, &vault).unwrap();
    assert_eq!(backup::list_versions(&vault, &ga).len(), n, "aucune version en plus");
    let p = backup::plan(&a, &ga, &vault).unwrap();
    assert!(matches!(p, backup::Plan::Nothing | backup::Plan::InSync), "{:?}", p);
}

// ================================================================== Windows 10 <-> 11

#[test]
fn windows10_vers_windows11_valeurs_ignorees_et_gardees() {
    let tmp = TempDir::new().unwrap();
    let w10 = mem(tmp.path(), "Alice", 19045);
    let w11 = mem(tmp.path(), "Alice", 22631);
    w10.set(Root::Hkcu, ADV, "ShowTaskViewButton", Data::Dword(0));
    w10.set(Root::Hkcu, ADV, "TaskbarSmallIcons", Data::Dword(1));
    w10.set(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\People", "PeopleBand", Data::Dword(0));
    w10.set(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Search", "SearchboxTaskbarMode", Data::Dword(1));
    w11.set(Root::Hkcu, ADV, "ShowTaskViewButton", Data::Dword(1));
    w11.set(Root::Hkcu, ADV, "TaskbarAl", Data::Dword(1));
    w11.set(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Search", "SearchboxTaskbarMode", Data::Dword(2));
    let f10 = wp::export(&*w10, Kind::Taskbar).unwrap();
    assert!(f10.entries.iter().any(|e| e.name == "TaskbarSmallIcons" && e.os == Os::Win10));
    assert!(!f10.entries.iter().any(|e| e.name == "TaskbarAl"));
    // Windows 10 -> 11 : les valeurs propres a Windows 10 sont ignorees et comptees (bilan), le reste est remis.
    let mut unw = BTreeSet::new();
    let r = wp::apply(&*w11, Kind::Taskbar, &f10, &mut unw);
    assert_eq!((r.applied, r.other_os, r.here), (2, 2, Os::Win11), "{:?}", r);
    assert_eq!(w11.get(Root::Hkcu, ADV, "TaskbarSmallIcons"), None, "valeur de Windows 10 jamais ecrite sous Windows 11");
    assert_eq!(w11.get(Root::Hkcu, ADV, "ShowTaskViewButton"), Some(Data::Dword(0)));
    // L'export suivant garde les valeurs de Windows 10 (aller-retour sans perte) et ajoute celles de Windows 11.
    let merged = wp::merge(Some(&f10), wp::export(&*w11, Kind::Taskbar).unwrap(), &*w11, &unw);
    assert!(merged.entries.iter().any(|e| e.name == "TaskbarSmallIcons"));
    assert!(merged.entries.iter().any(|e| e.name == "PeopleBand"));
    assert!(merged.entries.iter().any(|e| e.name == "TaskbarAl" && e.os == Os::Win11));
    // Retour sous Windows 10 : la valeur de Windows 11 est ignoree ; l'export de Windows 10 retombe sur le meme fichier.
    let r = wp::apply(&*w10, Kind::Taskbar, &merged, &mut unw);
    assert_eq!(r.other_os, 1, "{:?}", r);
    assert_eq!(w10.get(Root::Hkcu, ADV, "TaskbarAl"), None);
    let again = wp::merge(Some(&merged), wp::export(&*w10, Kind::Taskbar).unwrap(), &*w10, &unw);
    assert_eq!(again, merged, "aucun echange sans fin entre Windows 10 et 11");
    // Valeur refusee par Windows (protegee) : comptee, gardee telle quelle dans l'export suivant.
    w11.deny.lock().unwrap().insert(format!("{}|taskbarda", ADV.to_lowercase()));
    let mut f = merged.clone();
    f.entries.push(Entry { key: format!("HKCU\\{}", ADV), name: "TaskbarDa".into(), value: Data::Dword(0), tpl: false, os: Os::Win11 });
    f.sort();
    w11.set(Root::Hkcu, ADV, "TaskbarDa", Data::Dword(1));
    let r = wp::apply(&*w11, Kind::Taskbar, &f, &mut unw);
    assert_eq!(r.failed, 1);
    let m2 = wp::merge(Some(&f), wp::export(&*w11, Kind::Taskbar).unwrap(), &*w11, &unw);
    assert_eq!(m2.entries.iter().find(|e| e.name == "TaskbarDa").unwrap().value, Data::Dword(0), "valeur recue gardee");
}

// ================================================================== hosts, applications par defaut, alimentation

#[test]
fn fichier_hosts_jamais_ecrit_en_silence() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let sa = mem(tmp.path(), "Alice", 22631);
    let sb = mem(tmp.path(), "Bob", 22631);
    std::fs::write(sa.hosts_path(), "127.0.0.1 localhost\n10.0.0.5 nas\n").unwrap();
    std::fs::write(sb.hosts_path(), "127.0.0.1 localhost\n").unwrap();
    let a = device(tmp.path(), "PCA", sa.clone());
    let b = device(tmp.path(), "PCB", sb.clone());
    let ga = pref_game(&a, Kind::Hosts);
    let gb = pref_game(&b, Kind::Hosts);
    backup::backup(&a, &ga, &vault).unwrap();
    backup::restore(&b, &gb, &vault).unwrap();
    // Le vrai fichier de B n'est pas touche ; la version recue attend l'accord.
    assert_eq!(std::fs::read_to_string(sb.hosts_path()).unwrap(), "127.0.0.1 localhost\n");
    let l = wp::load_local(&b.data_dir, Kind::Hosts);
    assert!(l.hosts_pending && l.report.as_ref().unwrap().pending_admin);
    // Tant qu'il attend, l'export ne l'ecrase pas (et rien ne repart vers le coffre).
    assert!(!wp::refresh_kind(&b.data_dir, &*sb, Kind::Hosts, true));
    assert!(std::fs::read_to_string(wp::staging(&b.data_dir, Kind::Hosts).join(wp::HOSTS_FILE)).unwrap().contains("nas"));
    // L'etape avec droits eleves n'accepte que la copie recue.
    assert!(wp::write_hosts_elevated(&tmp.path().join("autre.txt"), &tmp.path().join("h")).is_err());
    let target = tmp.path().join("hosts-cible");
    std::fs::write(&target, "ancien\n").unwrap();
    wp::write_hosts_elevated(&wp::staging(&b.data_dir, Kind::Hosts).join(wp::HOSTS_FILE), &target).unwrap();
    assert!(std::fs::read_to_string(&target).unwrap().contains("nas"));
    assert_eq!(std::fs::read_to_string(target.with_file_name("hosts.agss-before")).unwrap(), "ancien\n");
    // « Garder celui de cet appareil » : plus rien en attente, l'export reprend le fichier d'ici.
    wp::hosts_keep_mine(&b.data_dir);
    assert!(!wp::load_local(&b.data_dir, Kind::Hosts).hosts_pending);
    assert!(wp::refresh_kind(&b.data_dir, &*sb, Kind::Hosts, true));
}

#[test]
fn applications_par_defaut_choix_protege_jamais_ecrit() {
    let tmp = TempDir::new().unwrap();
    let a = mem(tmp.path(), "Alice", 22631);
    let b = mem(tmp.path(), "Bob", 22631);
    for (ext, prog) in [(".txt", "Notepad++_file"), (".pdf", "AcroExch.Document.DC"), (".png", "PhotoViewer.FileAssoc.Png")] {
        a.set(Root::Hkcu, &format!(r"{}\{}\UserChoice", wp::FILE_EXTS, ext), "ProgId", Data::Sz(prog.into()));
        a.set(Root::Hkcu, &format!(r"{}\{}\UserChoice", wp::FILE_EXTS, ext), "Hash", Data::Sz("abc=".into()));
    }
    let f = wp::export(&*a, Kind::FileAssoc).unwrap();
    assert_eq!(f.entries.len(), 3);
    assert!(f.entries.iter().all(|e| e.name == "ProgId"), "l'empreinte n'est jamais copiee");
    // Chez B : .txt protege par un autre choix (a refaire), .pdf sans choix et programme installe (ecrit dans Classes),
    // .png sans programme (a refaire).
    b.set(Root::Hkcu, &format!(r"{}\.txt\UserChoice", wp::FILE_EXTS), "ProgId", Data::Sz("txtfile".into()));
    b.add_key(Root::Hklm, r"SOFTWARE\Classes\AcroExch.Document.DC");
    let mut unw = BTreeSet::new();
    let r = wp::apply(&*b, Kind::FileAssoc, &f, &mut unw);
    assert_eq!(r.applied, 1);
    assert_eq!(r.todo, vec![".png".to_string(), ".txt".to_string()]);
    assert_eq!(b.get(Root::Hkcu, r"Software\Classes\.pdf", ""), Some(Data::Sz("AcroExch.Document.DC".into())));
    assert!(b.writes.lock().unwrap().iter().all(|w| !w.0.to_lowercase().contains("userchoice")), "UserChoice jamais ecrit");
    assert!(b.notices.lock().unwrap().contains(&wp::Notice::Assoc));
}

#[test]
fn mode_d_alimentation_propre_a_l_appareil() {
    let tmp = TempDir::new().unwrap();
    let s = mem(tmp.path(), "Player", 22631);
    s.command("powercfg /getactivescheme", "Power Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)");
    let f = wp::export(&*s, Kind::Power).unwrap();
    assert_eq!(f.entries.iter().find(|e| e.name == "active").unwrap().value, Data::Sz("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".into()));
    assert!(Kind::Power.device_specific() && Kind::TodoList.device_specific() && !Kind::Mouse.device_specific());
    assert_ne!(agss::cloud::game_key(&wp::device_dest(Kind::Power, "Desktop PC")), agss::cloud::game_key(&wp::device_dest(Kind::Power, "Handheld")));
    // Mode absent de cet appareil : a refaire ; present : choisi (commande controlee).
    let t = mem(tmp.path(), "Bob", 22631);
    t.command("powercfg /getactivescheme", "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)");
    t.command("powercfg /list", "381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *");
    let mut unw = BTreeSet::new();
    let r = wp::apply(&*t, Kind::Power, &f, &mut unw);
    assert_eq!((r.applied, r.other_os), (0, 1));
    t.command("powercfg /list", "381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *\n8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)");
    t.command("powercfg /setactive 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c", "");
    let r = wp::apply(&*t, Kind::Power, &f, &mut unw);
    assert_eq!(r.applied, 1);
    assert!(t.runs.lock().unwrap().iter().any(|l| l == "powercfg /setactive 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"));
    // Nom en francais, GUID retrouve.
    assert_eq!(wp::parse_active_scheme("GUID du mode de gestion de l'alimentation : 381b4222-f694-41f0-9685-ff5bb260df2e  (Utilisation normale)").unwrap().1, "Utilisation normale");
}

// ================================================================== aller-retour sur une cle de test (vrai registre)

#[cfg(windows)]
#[test]
fn aller_retour_export_restauration_sur_cle_de_test() {
    use agss::winprefs::TestKeySys;
    let tmp = TempDir::new().unwrap();
    // Vrai reglage de la souris (lecture seule) : il ne doit pas bouger.
    let real_before = agss::registry::values(agss::registry::Hive::CurrentUser, MOUSE);
    let id = std::process::id();
    let a = TestKeySys::new(&format!("v48-a-{}", id), 19045, folders(tmp.path(), "Alice"), tmp.path().join("ha"));
    let b = TestKeySys::new(&format!("v48-b-{}", id), 22631, folders(tmp.path(), "Bob"), tmp.path().join("hb"));
    assert!(a.base.starts_with(r"Software\AGSS-Test\") && b.base.starts_with(r"Software\AGSS-Test\"));
    let user_a = tmp.path().join("Users").join("Alice");
    assert!(a.seed(Root::Hkcu, MOUSE, "MouseSensitivity", &Data::Sz("14".into())));
    assert!(a.seed(Root::Hkcu, MOUSE, "MouseTrails", &Data::Dword(7)));
    assert!(a.seed(Root::Hkcu, MOUSE, "SmoothMouseXCurve", &Data::Binary("0000000000000000156e000000000000".into())));
    assert!(a.seed(Root::Hkcu, r"Control Panel\Cursors", "Arrow", &Data::Expand(format!(r"{}\Cursors\a.cur", user_a.display()))));
    assert!(a.seed(Root::Hkcu, r"Control Panel\Cursors\Schemes", "Mon schéma", &Data::Expand("%SystemRoot%\\cursors\\aero_arrow.cur,,".into())));
    assert!(a.seed(Root::Hkcu, r"Control Panel\Desktop", "WheelScrollLines", &Data::Sz("5".into())));
    assert!(a.seed(Root::Hkcu, r"Control Panel\Desktop", "Wallpaper", &Data::Sz("pas pour la souris".into())));
    assert!(b.seed(Root::Hkcu, MOUSE, "MouseSensitivity", &Data::Sz("10".into())));
    assert!(b.seed(Root::Hkcu, r"Control Panel\Desktop", "WheelScrollLines", &Data::Sz("3".into())));
    let f = wp::export(&a, Kind::Mouse).unwrap();
    assert!(f.entries.iter().all(|e| e.name != "Wallpaper"), "seules les valeurs listees");
    let text = String::from_utf8(f.to_bytes()).unwrap();
    assert!(text.contains("%USERPROFILE%") && !text.contains("Alice"), "aucun nom d'utilisateur dans l'export");
    let mut unw = BTreeSet::new();
    let r = wp::apply(&b, Kind::Mouse, &f, &mut unw);
    assert_eq!((r.failed, r.refused), (0, 0), "{:?}", r);
    assert_eq!(b.read(Root::Hkcu, MOUSE, "MouseSensitivity"), Some(Data::Sz("14".into())));
    assert_eq!(b.read(Root::Hkcu, MOUSE, "MouseTrails"), Some(Data::Dword(7)));
    assert_eq!(b.read(Root::Hkcu, MOUSE, "SmoothMouseXCurve"), Some(Data::Binary("0000000000000000156e000000000000".into())));
    assert_eq!(b.read(Root::Hkcu, r"Control Panel\Cursors", "Arrow"), Some(Data::Expand(format!(r"{}\Cursors\a.cur", tmp.path().join("Users").join("Bob").display()))));
    assert_eq!(b.read(Root::Hkcu, r"Control Panel\Cursors\Schemes", "Mon schéma"), Some(Data::Expand("%SystemRoot%\\cursors\\aero_arrow.cur,,".into())));
    assert_eq!(b.read(Root::Hkcu, r"Control Panel\Desktop", "WheelScrollLines"), Some(Data::Sz("5".into())));
    assert_eq!(b.read(Root::Hkcu, r"Control Panel\Desktop", "Wallpaper"), None);
    assert!(b.notices.lock().unwrap().contains(&wp::Notice::Mouse));
    // Aller-retour : l'export de B redonne exactement le fichier de A.
    let back = wp::merge(Some(&f), wp::export(&b, Kind::Mouse).unwrap(), &b, &unw);
    assert_eq!(back, f);
    // Plusieurs chaines (REG_MULTI_SZ) et nombres 64 bits.
    assert!(b.seed(Root::Hkcu, r"Control Panel\International\User Profile", "Languages", &Data::Multi(vec!["fr-FR".into(), "en-US".into()])));
    assert!(b.seed(Root::Hkcu, r"Control Panel\International\User Profile\fr-FR", "040C:0000040C", &Data::Dword(1)));
    assert!(b.seed(Root::Hkcu, r"Control Panel\Keyboard", "KeyboardDelay", &Data::Sz("1".into())));
    let kb = wp::export(&b, Kind::Keyboard).unwrap();
    assert!(wp::apply(&a, Kind::Keyboard, &kb, &mut unw).applied >= 3);
    assert_eq!(a.read(Root::Hkcu, r"Control Panel\International\User Profile", "Languages"), Some(Data::Multi(vec!["fr-FR".into(), "en-US".into()])));
    assert_eq!(a.read(Root::Hkcu, r"Control Panel\International\User Profile\fr-FR", "040C:0000040C"), Some(Data::Dword(1)));
    // Menage : toute la cle de test disparait ; le vrai reglage n'a pas bouge.
    assert!(a.cleanup() && b.cleanup());
    assert!(!agss::registry::key_exists(agss::registry::Hive::CurrentUser, &a.base));
    assert!(!agss::registry::delete_test_tree(r"Software\Microsoft"), "suppression refusee hors de la cle de test");
    assert_eq!(agss::registry::values(agss::registry::Hive::CurrentUser, MOUSE), real_before);
}

// ================================================================== textes

#[test]
fn textes_des_reglages_windows_dans_les_5_langues() {
    let keys = [
        K::VPrefTheme, K::VPrefMouse, K::VPrefKeyboard, K::VPrefExplorer, K::VPrefTaskbar, K::VPrefRegion, K::VPrefSound, K::VPrefPower, K::VPrefEnv, K::VPrefAssoc,
        K::VPrefFonts, K::VPrefOffice, K::VPrefDict, K::VPrefTerminal, K::VPrefHosts, K::VPrefQuick, K::VPrefTodo, K::VPrefDevice, K::VPrefAdmin, K::VPrefPartial,
        K::VPrefListOnly, K::VPrefCountOne, K::VPrefCountMany, K::VPrefTitle, K::VPrefEffectNow, K::VPrefEffectOpen, K::VPrefEffectSignIn, K::VPrefEffectNever,
        K::VPrefLast, K::VPrefOtherOs, K::VPrefFailed, K::VPrefRedo, K::VPrefOpenSettings, K::VPrefDryRun, K::VPrefHostsPending, K::VPrefHostsBody, K::VPrefHostsApply,
        K::VPrefHostsKeep, K::VPrefHostsDone, K::VPrefHostsCancelled, K::VPrefHostsFailed, K::VPrefRestoreNote, K::VPrefTodoTitle, K::VPrefTodoHint, K::VPrefPrinters,
        K::VPrefDrives, K::VPrefWifi, K::VPrefDriver, K::VPrefOpenPrinters, K::VPrefOpenDrives, K::VPrefOpenWifi,
    ];
    for k in keys {
        assert!(SIMPLE_KEYS.contains(&k), "{:?}", k);
        let en = tr_raw(Lang::En, k);
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr_raw(l, k) };
            assert!(!s.trim().is_empty(), "{:?} {}", k, l.code());
            for m in ["{}", "{0}", "{1}"] {
                assert_eq!(s.matches(m).count(), en.matches(m).count(), "{:?} {} {}", k, l.code(), m);
            }
            // Jamais de mot de passe promis : le Wi-Fi dit « noms seulement ».
            assert!(!s.to_lowercase().contains("registre") && !s.to_lowercase().contains("registry"), "{:?} {} : mot technique", k, l.code());
        }
    }
    assert_eq!(agss::i18n::tr(Lang::Fr, K::VPrefCountMany).replace("{}", "14"), "14 réglages");
    assert!(tr_raw(Lang::Fr, K::VPrefRestoreNote).contains("copie de sécurité") && tr_raw(Lang::Fr, K::VPrefRestoreNote).contains("Annuler"));
}

#[test]
fn rien_sous_linux() {
    let tmp = TempDir::new().unwrap();
    if cfg!(windows) {
        assert!(wp::real().is_some());
    } else {
        assert!(wp::real().is_none());
        assert!(agss::scan::sources::pref_offers(tmp.path()).is_empty());
    }
    assert!(agss::migrate::pref_known(tmp.path(), false).is_empty());
    // Sans systeme : seule la fiche fixe d'un element fait de fichiers est preparee (rien n'est lu ni ecrit ailleurs).
    wp::prepare(tmp.path(), Kind::Office, None);
    assert_eq!(std::fs::read(wp::staging(tmp.path(), Kind::Office).join(wp::ABOUT_FILE)).unwrap(), wp::about_bytes(Kind::Office));
    let g = Game { name: "x".into(), path: wp::staging(tmp.path(), Kind::Office), ..Default::default() };
    assert_eq!(wp::staged_in(tmp.path(), &g), vec![(Kind::Office, wp::staging(tmp.path(), Kind::Office))]);
    let other = Game { name: "x".into(), path: PathBuf::from(tmp.path()).join("winprefs-bis").join("mouse"), ..Default::default() };
    assert!(wp::staged_in(tmp.path(), &other).is_empty());
}

#[test]
fn liste_a_refaire_lue_dans_le_coffre_pour_le_nouveau_pc() {
    let tmp = TempDir::new().unwrap();
    let vault = tmp.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let s = mem(tmp.path(), "Alice", 22631);
    s.set(Root::Hkcu, r"Network\M", "RemotePath", Data::Sz(r"\\nas\musique".into()));
    s.command("netsh wlan show profiles", "Profiles on interface Wi-Fi:\n\nUser profiles\n-------------\n    All User Profile     : HomeNet\n");
    let a = device(tmp.path(), "DESKTOP", s);
    let g = pref_game(&a, Kind::TodoList);
    backup::backup(&a, &g, &vault).unwrap();
    let devs = agss::migrate::devices(&vault, "ALLY", &Default::default());
    let d = devs.iter().find(|d| d.name == "DESKTOP").unwrap();
    let t = agss::migrate::todo_list_of(&vault, d).unwrap();
    assert_eq!(t.wifi, vec!["HomeNet".to_string()]);
    assert_eq!(t.drives[0].path, r"\\nas\musique");
    assert!(t.printers.is_empty());
}
