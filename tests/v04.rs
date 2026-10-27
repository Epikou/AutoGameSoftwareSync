//! Tests v0.4 : plus aucun sous-processus (tasklist / reg), registre et processus via API Windows,
//! couche manette (decodage HID Sony, XInput, evenements normalises, hub), i18n de la ligne Manette.
use agss::gamepad::*;
use agss::i18n::{tr, Lang, K};
use agss::process::{self, ProcessChecker, ToolhelpChecker};
#[cfg(windows)]
use agss::registry::{self, Hive};
use std::collections::VecDeque;
use std::path::Path;

// ================================================================== processus

fn current_exe_name() -> String {
    std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().to_string()
}

#[test]
fn is_running_vrai_pour_le_processus_courant_faux_pour_un_exe_inexistant() {
    let me = current_exe_name();
    assert!(ToolhelpChecker.is_running(&me).unwrap(), "le processus de test ({}) doit etre vu", me);
    assert!(!ToolhelpChecker.is_running("agss-exe-qui-n-existe-pas-12345.exe").unwrap());
}

#[test]
fn is_running_insensible_a_la_casse_accepte_un_chemin_et_refuse_le_vide() {
    let me = current_exe_name();
    assert!(ToolhelpChecker.is_running(&me.to_uppercase()).unwrap());
    assert!(ToolhelpChecker.is_running(&me.to_lowercase()).unwrap());
    let full = std::env::current_exe().unwrap().to_string_lossy().to_string();
    assert!(ToolhelpChecker.is_running(&full).unwrap(), "un chemin complet doit se ramener au nom d'exe");
    // Nom indeterminable => Err => l'appelant considere le jeu "en cours" (regle anti-perte).
    assert!(ToolhelpChecker.is_running("   ").is_err());
    assert!(ToolhelpChecker.is_running("").is_err());
}

#[test]
fn noms_d_exe_complets_sans_troncature_a_15_caracteres() {
    // szExeFile : tampon de 260 u16 termine par NUL.
    let long = "UnNomDExeVraimentTresLongPourUnJeu.exe"; // 38 caracteres
    let mut buf = [0u16; 260];
    for (i, c) in long.encode_utf16().enumerate() {
        buf[i] = c;
    }
    assert!(process::names_match(&buf, &process::normalize_exe_name(long)));
    assert!(process::names_match(&buf, &process::normalize_exe_name(&long.to_uppercase())));
    // Le prefixe de 15 caracteres (troncature de tasklist/WMI) ne doit PAS correspondre.
    assert!(!process::names_match(&buf, &process::normalize_exe_name(&long[..15])));
    assert!(!process::names_match(&buf, &process::normalize_exe_name("autre.exe")));
    // Tampon plein sans NUL : pas de depassement.
    let full = [b'a' as u16; 260];
    assert!(!process::names_match(&full, "a"));
    assert_eq!(process::normalize_exe_name("  C:\\Jeux\\Mon Jeu\\Game.EXE "), "game.exe");
    assert_eq!(process::normalize_exe_name("D:/x/y.exe"), "y.exe");
}

#[test]
fn pid_alive_processus_courant_inexistant_et_termine() {
    assert!(agss::watch::pid_alive(std::process::id()));
    assert!(!agss::watch::pid_alive(4_000_000_000));
    assert!(!agss::watch::pid_alive(0));
    // Un enfant termine (handle encore ouvert cote Rust) doit etre vu comme mort.
    let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.args(["/C", "exit", "0"]).creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert!(!agss::watch::pid_alive(pid));
}

#[test]
fn plus_aucun_command_new_tasklist_reg_explorer_et_tout_sous_processus_est_masque() {
    // Parcourt src\*.rs : aucun lancement de tasklist/reg/explorer, et tout Command::new restant
    // doit etre accompagne de creation_flags / hidden( (CREATE_NO_WINDOW) a proximite.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for e in std::fs::read_dir(&src).unwrap().filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().map(|x| x != "rs").unwrap_or(true) {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let t = l.trim_start();
            if p.file_name().map(|n| n.to_string_lossy().ends_with("_linux.rs")).unwrap_or(false) {
                continue; // code Linux : pas de fenetre de console a masquer
            }
            if t.starts_with("//") || !l.contains("Command::new(") {
                continue;
            }
            checked += 1;
            let lower = l.to_lowercase();
            for banned in ["tasklist", "\"reg\"", "reg.exe", "explorer", "wmic"] {
                assert!(!lower.contains(banned), "{}:{} lance un sous-processus interdit : {}", p.display(), i + 1, l.trim());
            }
            let lo = i.saturating_sub(12);
            let hi = (i + 12).min(lines.len());
            let near = lines[lo..hi].join("\n");
            assert!(near.contains("creation_flags") || near.contains("hidden("), "{}:{} : Command::new sans CREATE_NO_WINDOW : {}", p.display(), i + 1, l.trim());
        }
    }
    // Sanity : on a bien analyse quelque chose (toast PowerShell + relance cachee de watch).
    assert!(checked >= 1, "aucun Command::new trouve : le test ne verifie plus rien");
    // Les anciennes fonctions de lancement ont disparu.
    let proc_src = std::fs::read_to_string(src.join("process.rs")).unwrap();
    assert!(!proc_src.to_lowercase().contains("command::new"));
}

// ================================================================== registre (lecture seule)

#[cfg(windows)]
#[test]
fn registre_lecture_directe() {
    let name = registry::read_string(Hive::LocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "ProductName").expect("ProductName lisible");
    assert!(name.to_lowercase().contains("windows"), "{}", name);
    let subs = registry::subkeys(Hive::LocalMachine, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion");
    assert!(subs.iter().any(|s| s.eq_ignore_ascii_case("Run")), "sous-cle Run attendue parmi {} cles", subs.len());
    assert!(registry::read_dword(Hive::LocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "CurrentMajorVersionNumber").is_some());
    // Absent => None / false / vide, sans panique.
    assert_eq!(registry::read_string(Hive::CurrentUser, "Software\\AutoGameSoftwareSyncCleInexistante", "x"), None);
    assert!(!registry::value_exists(Hive::CurrentUser, "Software\\AutoGameSoftwareSyncCleInexistante", "x"));
    assert!(registry::subkeys(Hive::CurrentUser, "Software\\AutoGameSoftwareSyncCleInexistante").is_empty());
    // Un type inattendu n'est pas pris pour un texte.
    assert_eq!(registry::read_string(Hive::LocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "CurrentMajorVersionNumber"), None);
    // Les sondes de l'application ne paniquent pas.
    use agss::vm::ThemeProbe;
    let _ = agss::winutil::RegThemeProbe.system_is_light();
    let _ = agss::winutil::autostart_enabled();
    let _ = agss::detect::steam_root();
    let _ = agss::detect::gog_games();
}

#[cfg(windows)]
#[test]
fn decodage_chaine_registre_utf16() {
    let b: Vec<u8> = "C:\\Steam\u{e9}\0".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    assert_eq!(registry::decode_reg_string(&b), "C:\\Steam\u{e9}");
    assert_eq!(registry::decode_reg_string(&[]), "");
    assert_eq!(registry::decode_reg_string(&[0, 0, 65, 0]), ""); // s'arrete au premier NUL
    assert_eq!(registry::decode_reg_string(&[65, 0, 66]), "A"); // octet impair ignore
}

// ================================================================== manette : decodage HID

const NEUTRAL: u8 = 0x08;
const CROSS: u8 = 0x20;
const CIRCLE: u8 = 0x40;
const L1: u8 = 0x01;
const R1: u8 = 0x02;

fn ds5_usb(b0: u8, b1: u8, lx: u8, ly: u8) -> Vec<u8> {
    let mut r = vec![0u8; 64];
    r[0] = 0x01;
    r[1] = lx;
    r[2] = ly;
    r[3] = 128;
    r[4] = 128;
    r[8] = b0;
    r[9] = b1;
    r
}
fn ds5_bt(b0: u8, b1: u8, lx: u8, ly: u8) -> Vec<u8> {
    let mut r = vec![0u8; 78];
    r[0] = 0x31;
    r[1] = 0x10;
    r[2] = lx;
    r[3] = ly;
    r[4] = 128;
    r[5] = 128;
    r[9] = b0;
    r[10] = b1;
    r
}
fn simple_bt(b0: u8, b1: u8, lx: u8, ly: u8, len: usize) -> Vec<u8> {
    let mut r = vec![0u8; len];
    r[0] = 0x01;
    r[1] = lx;
    r[2] = ly;
    r[3] = 128;
    r[4] = 128;
    r[5] = b0;
    r[6] = b1;
    r
}
fn ds4_usb(b0: u8, b1: u8, lx: u8, ly: u8) -> Vec<u8> {
    simple_bt(b0, b1, lx, ly, 64)
}
fn ds4_bt(b0: u8, b1: u8, lx: u8, ly: u8) -> Vec<u8> {
    let mut r = vec![0u8; 78];
    r[0] = 0x11;
    r[1] = 0xC0;
    r[3] = lx;
    r[4] = ly;
    r[5] = 128;
    r[6] = 128;
    r[7] = b0;
    r[8] = b1;
    r
}

#[test]
fn dualsense_usb_croix_boutons_et_stick() {
    let r = ds5_usb(0x00 | CROSS, L1, 128, 128); // haut + Croix + L1
    let s = decode_hid_report(HidModel::DualSense, Connection::Usb, &r).unwrap();
    assert!(s.up && !s.down && !s.left && !s.right);
    assert!(s.confirm && !s.back && s.lb && !s.rb);
    assert!(s.lx.abs() < 0.01 && s.ly.abs() < 0.01);

    let r = ds5_usb(NEUTRAL | CIRCLE, R1, 255, 0); // stick a fond a droite et en haut
    let s = decode_hid_report(HidModel::DualSense, Connection::Usb, &r).unwrap();
    assert!(!s.up && !s.down && !s.left && !s.right);
    assert!(s.back && !s.confirm && s.rb && !s.lb);
    assert!(s.lx > 0.99 && s.ly > 0.99, "lx={} ly={} (ly positif = haut)", s.lx, s.ly);

    let r = ds5_usb(NEUTRAL, 0, 0, 255); // stick a gauche et en bas
    let s = decode_hid_report(HidModel::DualSense, Connection::Usb, &r).unwrap();
    assert!(s.lx < -0.99 && s.ly < -0.99);
}

#[test]
fn dualsense_bluetooth_complet_0x31_et_simple_0x01() {
    let r = ds5_bt(0x04 | CROSS, 0, 128, 128); // bas (4) + Croix
    let s = decode_hid_report(HidModel::DualSense, Connection::Bluetooth, &r).unwrap();
    assert!(s.down && !s.up && s.confirm);
    // Mode simple en Bluetooth : meme rapport 0x01, disposition DualShock 4, meme complete a 78 octets par Windows.
    for len in [10usize, 78] {
        let r = simple_bt(0x06 | CIRCLE, R1, 128, 128, len); // gauche (6) + Rond + R1
        let s = decode_hid_report(HidModel::DualSense, Connection::Bluetooth, &r).unwrap();
        assert!(s.left && !s.right && s.back && s.rb, "longueur {}", len);
    }
    // Connexion inconnue : la longueur departage (64 => complet USB, 10 => simple).
    let s = decode_hid_report(HidModel::DualSense, Connection::Unknown, &ds5_usb(0x02, 0, 128, 128)).unwrap();
    assert!(s.right);
    let s = decode_hid_report(HidModel::DualSense, Connection::Unknown, &simple_bt(0x02, 0, 128, 128, 10)).unwrap();
    assert!(s.right);
}

#[test]
fn dualshock4_usb_bluetooth_complet_et_simple() {
    let s = decode_hid_report(HidModel::DualShock4, Connection::Usb, &ds4_usb(0x00 | CROSS, L1, 128, 128)).unwrap();
    assert!(s.up && s.confirm && s.lb);
    let s = decode_hid_report(HidModel::DualShock4, Connection::Bluetooth, &ds4_bt(0x02 | CIRCLE, R1, 128, 128)).unwrap();
    assert!(s.right && s.back && s.rb);
    let s = decode_hid_report(HidModel::DualShock4, Connection::Bluetooth, &simple_bt(0x04, 0, 255, 128, 10)).unwrap();
    assert!(s.down && s.lx > 0.99);
}

#[test]
fn croix_les_huit_directions_et_neutre() {
    let expect = [
        (0u8, (true, false, false, false)), // haut
        (1, (true, false, false, true)),    // haut-droite
        (2, (false, false, false, true)),   // droite
        (3, (false, true, false, true)),    // bas-droite
        (4, (false, true, false, false)),   // bas
        (5, (false, true, true, false)),    // bas-gauche
        (6, (false, false, true, false)),   // gauche
        (7, (true, false, true, false)),    // haut-gauche
        (8, (false, false, false, false)),  // neutre
        (15, (false, false, false, false)), // valeur hors plage => neutre
    ];
    for (hat, (u, d, l, r)) in expect {
        for (m, rep) in [(HidModel::DualSense, ds5_usb(hat, 0, 128, 128)), (HidModel::DualShock4, ds4_usb(hat, 0, 128, 128))] {
            let s = decode_hid_report(m, Connection::Usb, &rep).unwrap();
            assert_eq!((s.up, s.down, s.left, s.right), (u, d, l, r), "hat {} modele {:?}", hat, m);
        }
    }
    // Les bits de poids fort (boutons de face) ne perturbent pas la croix.
    let s = decode_hid_report(HidModel::DualSense, Connection::Usb, &ds5_usb(0xF0 | 2, 0, 128, 128)).unwrap();
    assert!(s.right && !s.up);
}

#[test]
fn rapports_invalides_vide_trop_court_id_inconnu() {
    use DecodeError::*;
    assert_eq!(decode_hid_report(HidModel::DualSense, Connection::Usb, &[]), Err(Empty));
    // DualSense USB : il faut au moins l'octet des boutons 1 (index 9) => 10 octets.
    let full = ds5_usb(NEUTRAL, 0, 128, 128);
    assert_eq!(decode_hid_report(HidModel::DualSense, Connection::Usb, &full[..9]), Err(TooShort { need: 10, got: 9 }));
    assert!(decode_hid_report(HidModel::DualSense, Connection::Usb, &full[..10]).is_ok());
    assert_eq!(decode_hid_report(HidModel::DualSense, Connection::Usb, &[0x01]), Err(TooShort { need: 10, got: 1 }));
    // 0x31 : 11 octets minimum.
    let bt = ds5_bt(NEUTRAL, 0, 128, 128);
    assert_eq!(decode_hid_report(HidModel::DualSense, Connection::Bluetooth, &bt[..10]), Err(TooShort { need: 11, got: 10 }));
    assert!(decode_hid_report(HidModel::DualSense, Connection::Bluetooth, &bt[..11]).is_ok());
    // Simple / DualShock 4 : 7 octets.
    let simple = simple_bt(NEUTRAL, 0, 128, 128, 10);
    assert_eq!(decode_hid_report(HidModel::DualShock4, Connection::Usb, &simple[..6]), Err(TooShort { need: 7, got: 6 }));
    assert!(decode_hid_report(HidModel::DualShock4, Connection::Usb, &simple[..7]).is_ok());
    // DualShock 4 Bluetooth 0x11 : 3 + 6 = 9 octets minimum.
    let d4 = ds4_bt(NEUTRAL, 0, 128, 128);
    assert_eq!(decode_hid_report(HidModel::DualShock4, Connection::Bluetooth, &d4[..8]), Err(TooShort { need: 9, got: 8 }));
    // Identifiants inconnus ou propres a l'autre modele : jamais d'evenement invente.
    for id in [0x00u8, 0x02, 0x05, 0x11, 0x15, 0x32, 0xFF] {
        let mut r = vec![0u8; 78];
        r[0] = id;
        assert!(matches!(decode_hid_report(HidModel::DualSense, Connection::Usb, &r), Err(UnknownReport(x)) if x == id), "id {:#x}", id);
    }
    for id in [0x00u8, 0x31, 0x32] {
        let mut r = vec![0u8; 78];
        r[0] = id;
        assert!(matches!(decode_hid_report(HidModel::DualShock4, Connection::Usb, &r), Err(UnknownReport(x)) if x == id), "id {:#x}", id);
    }
}

#[test]
fn identification_sony_et_connexion_depuis_le_chemin() {
    assert_eq!(sony_model(0x054C, 0x0CE6), Some((HidModel::DualSense, "DualSense")));
    assert_eq!(sony_model(0x054C, 0x0DF2), Some((HidModel::DualSense, "DualSense Edge")));
    assert_eq!(sony_model(0x054C, 0x05C4), Some((HidModel::DualShock4, "DualShock 4")));
    assert_eq!(sony_model(0x054C, 0x09CC), Some((HidModel::DualShock4, "DualShock 4")));
    assert_eq!(sony_model(0x054C, 0x0001), None);
    assert_eq!(sony_model(0x045E, 0x0CE6), None); // autre fabricant
    let usb = "\\\\?\\HID#VID_054C&PID_0CE6#6&1a2b3c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
    let bt = "\\\\?\\HID#{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&0CE6#9&a1b2&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
    assert_eq!(connection_from_path(usb), Connection::Usb);
    assert_eq!(connection_from_path(bt), Connection::Bluetooth);
    assert_eq!(connection_from_path("\\\\?\\HID#truc"), Connection::Unknown);
}

#[test]
fn decodage_xinput() {
    use xinput_bits::*;
    let s = decode_xinput(A | LB | DPAD_UP, 0, 0);
    assert!(s.confirm && s.lb && s.up && !s.back && !s.rb && !s.down);
    let s = decode_xinput(B | RB | DPAD_LEFT | DPAD_RIGHT | DPAD_DOWN, i16::MAX, i16::MIN);
    assert!(s.back && s.rb && s.left && s.right && s.down);
    assert!((s.lx - 1.0).abs() < 1e-4 && (s.ly + 1.0).abs() < 1e-4, "i16::MIN doit etre borne a -1");
    assert_eq!(decode_xinput(0, 0, 0), PadState::default());
}

// ================================================================== manette : evenements normalises

fn st() -> PadState {
    PadState::default()
}

#[test]
fn le_premier_etat_sert_de_reference_sans_evenement() {
    let mut t = NavTracker::default();
    // Bouton deja enfonce a la connexion : rien.
    assert!(t.update(&PadState { confirm: true, up: true, ..st() }).is_empty());
    assert!(t.update(&PadState { confirm: true, up: true, ..st() }).is_empty());
    // Relache puis nouvel appui : evenement.
    assert!(t.update(&st()).is_empty());
    assert_eq!(t.update(&PadState { confirm: true, ..st() }), vec![Nav::Confirm]);
}

#[test]
fn evenements_sur_front_montant_toutes_actions() {
    let mut t = NavTracker::default();
    t.update(&st());
    let all = PadState { up: true, down: true, left: true, right: true, confirm: true, back: true, lb: true, rb: true, ..st() };
    assert_eq!(t.update(&all), vec![Nav::Up, Nav::Down, Nav::Left, Nav::Right, Nav::Confirm, Nav::Back, Nav::PagePrev, Nav::PageNext]);
    assert!(t.update(&all).is_empty(), "pas de repetition tant que c'est maintenu");
    assert!(t.update(&st()).is_empty());
}

#[test]
fn stick_avec_hysteresis_et_axes_independants() {
    let mut t = NavTracker::default();
    t.update(&st());
    assert!(t.update(&PadState { lx: 0.5, ..st() }).is_empty(), "sous le seuil d'activation");
    assert_eq!(t.update(&PadState { lx: 0.7, ..st() }), vec![Nav::Right]);
    // Reste actif entre le seuil de relachement et d'activation : pas de nouvel evenement.
    assert!(t.update(&PadState { lx: 0.5, ..st() }).is_empty());
    assert!(t.update(&PadState { lx: 0.7, ..st() }).is_empty(), "n'a pas ete relache : pas de re-declenchement");
    assert!(t.update(&PadState { lx: 0.1, ..st() }).is_empty()); // relache
    assert_eq!(t.update(&PadState { lx: -0.9, ..st() }), vec![Nav::Left]);
    t.update(&st());
    // ly positif = haut ; negatif = bas.
    assert_eq!(t.update(&PadState { ly: 0.9, ..st() }), vec![Nav::Up]);
    t.update(&st());
    assert_eq!(t.update(&PadState { ly: -0.9, ..st() }), vec![Nav::Down]);
    t.update(&st());
    // Diagonale : deux evenements, un par axe.
    assert_eq!(t.update(&PadState { lx: 0.9, ly: 0.9, ..st() }), vec![Nav::Up, Nav::Right]);
    // Stick + croix sur la meme direction : un seul evenement.
    t.update(&st());
    assert_eq!(t.update(&PadState { up: true, ly: 0.9, ..st() }), vec![Nav::Up]);
}

// ---- hub avec sources simulees

struct Fake {
    script: VecDeque<Vec<PadPoll>>,
}
impl PadSource for Fake {
    fn poll(&mut self, _now: u64) -> Vec<PadPoll> {
        self.script.pop_front().unwrap_or_default()
    }
}
fn info(id: &str, name: &str, conn: Connection) -> PadInfo {
    PadInfo { id: id.into(), name: name.into(), conn }
}
fn poll(i: &PadInfo, states: Vec<PadState>) -> PadPoll {
    PadPoll { info: i.clone(), readable: true, states }
}

#[test]
fn hub_connexion_evenements_deconnexion() {
    let ds = info("hid:1", "DualSense", Connection::Bluetooth);
    let press = PadState { confirm: true, ..st() };
    let script = vec![
        vec![poll(&ds, vec![st()])],              // connexion
        vec![poll(&ds, vec![st(), press, st()])], // appui + relache dans le meme cycle : pas perdu
        vec![poll(&ds, vec![st()])],
        vec![], // debranchee
        vec![poll(&ds, vec![press])], // rebranchee avec A enfonce : reference, pas d'evenement
    ];
    let mut hub = Hub::new(vec![Box::new(Fake { script: script.into() })]);
    let o = hub.tick(0);
    assert_eq!(o.changes.len(), 1);
    assert!(matches!(&o.changes[0], PadChange::Connected(s) if s.readable && s.info == ds));
    assert!(o.navs.is_empty());
    assert_eq!(hub.status().len(), 1);
    let o = hub.tick(10);
    assert_eq!(o.navs, vec![Nav::Confirm]);
    assert!(o.changes.is_empty());
    assert!(hub.tick(20).navs.is_empty());
    let o = hub.tick(30);
    assert_eq!(o.changes, vec![PadChange::Disconnected(ds.clone())]);
    assert!(hub.status().is_empty());
    let o = hub.tick(40);
    assert!(matches!(&o.changes[0], PadChange::Connected(_)));
    assert!(o.navs.is_empty(), "bouton deja enfonce a la reconnexion");
}

#[test]
fn hub_supprime_le_doublon_hid_plus_xinput_virtuelle_mais_pas_deux_appuis_reels() {
    let hid = info("hid:1", "DualSense", Connection::Usb);
    let xi = info("xinput:0", "Xbox / XInput", Connection::Unknown);
    let press = PadState { confirm: true, ..st() };
    let script_hid = vec![vec![poll(&hid, vec![st()])], vec![poll(&hid, vec![press])], vec![poll(&hid, vec![st()])], vec![poll(&hid, vec![st()])], vec![poll(&hid, vec![press])]];
    let script_xi = vec![vec![poll(&xi, vec![st()])], vec![poll(&xi, vec![press])], vec![poll(&xi, vec![st()])], vec![poll(&xi, vec![st()])], vec![poll(&xi, vec![press])]];
    let mut hub = Hub::new(vec![Box::new(Fake { script: script_hid.into() }), Box::new(Fake { script: script_xi.into() })]);
    assert!(hub.tick(0).navs.is_empty());
    assert_eq!(hub.tick(10).navs, vec![Nav::Confirm], "meme appui vu par deux sources = un seul evenement");
    assert!(hub.tick(20).navs.is_empty());
    assert!(hub.tick(30).navs.is_empty());
    // Nouvel appui bien plus tard : de nouveau pris en compte.
    assert_eq!(hub.tick(400).navs, vec![Nav::Confirm]);
}

#[test]
fn hub_deux_appuis_successifs_de_la_meme_manette_ne_sont_pas_fusionnes() {
    let p = info("hid:1", "DualSense", Connection::Usb);
    let press = PadState { down: true, ..st() };
    let script = vec![vec![poll(&p, vec![st()])], vec![poll(&p, vec![press])], vec![poll(&p, vec![st()])], vec![poll(&p, vec![press])]];
    let mut hub = Hub::new(vec![Box::new(Fake { script: script.into() })]);
    hub.tick(0);
    assert_eq!(hub.tick(5).navs, vec![Nav::Down]);
    hub.tick(10);
    assert_eq!(hub.tick(15).navs, vec![Nav::Down], "la deduplication ne concerne que des manettes differentes");
}

#[test]
fn hub_manette_illisible_signalee_sans_evenement_puis_lisible() {
    let p = info("hid:2", "DualSense", Connection::Usb);
    let blocked = PadPoll { info: p.clone(), readable: false, states: vec![PadState { confirm: true, ..st() }] };
    let script = vec![vec![blocked], vec![poll(&p, vec![st()])]];
    let mut hub = Hub::new(vec![Box::new(Fake { script: script.into() })]);
    let o = hub.tick(0);
    assert!(o.navs.is_empty(), "une manette illisible ne produit jamais d'evenement");
    assert!(matches!(&o.changes[0], PadChange::Connected(s) if !s.readable));
    assert!(needs_steam_hint(&hub.status()));
    let o = hub.tick(10);
    assert!(matches!(&o.changes[0], PadChange::Connected(s) if s.readable));
    assert!(!needs_steam_hint(&hub.status()));
}

// ---- ligne de Reglages / journal

#[test]
fn ligne_manette_et_journal() {
    let ds = PadStatus { info: info("a", "DualSense", Connection::Bluetooth), readable: true };
    let xb = PadStatus { info: info("b", "Xbox / XInput", Connection::Unknown), readable: true };
    let bad = PadStatus { info: info("c", "DualShock 4", Connection::Usb), readable: false };
    assert_eq!(status_line(&[], "Aucune", "lecture impossible"), "Aucune");
    assert_eq!(status_line(&[ds.clone()], "Aucune", "x"), "DualSense (Bluetooth)");
    assert_eq!(status_line(&[ds.clone(), xb.clone()], "Aucune", "x"), "DualSense (Bluetooth) ; Xbox / XInput");
    assert_eq!(status_line(&[bad.clone()], "Aucune", "lecture impossible"), "DualShock 4 (USB) - lecture impossible");
    assert!(needs_steam_hint(&[]));
    assert!(needs_steam_hint(&[bad.clone()]));
    assert!(!needs_steam_hint(&[bad.clone(), ds.clone()]));
    assert_eq!(change_log_line(&PadChange::Connected(ds.clone())), "[Manette] connectee : DualSense (Bluetooth)");
    assert_eq!(change_log_line(&PadChange::Disconnected(ds.info.clone())), "[Manette] deconnectee : DualSense (Bluetooth)");
    // Une manette illisible reste au niveau info dans le journal (pas de mot-cle d'erreur).
    let line = change_log_line(&PadChange::Connected(bad));
    let e = agss::vm::parse_log_line(&format!("20260101-120000-000 {}", line)).unwrap();
    assert_eq!(e.level, agss::vm::Level::Info, "{}", line);
    let e = agss::vm::parse_log_line(&format!("20260101-120000-000 {}", change_log_line(&PadChange::Connected(ds)))).unwrap();
    assert_eq!(e.level, agss::vm::Level::Info);
}

#[test]
fn i18n_ligne_manette_fr_en() {
    for k in [K::PadXbox, K::PadOther, K::PadNone, K::PadUnreadable, K::SetPadHelp] {
        let (fr, en) = (tr(Lang::Fr, k), tr(Lang::En, k));
        assert!(!fr.is_empty() && !en.is_empty(), "{:?}", k);
        assert_ne!(fr, en, "{:?} non traduit", k);
    }
    assert_eq!(tr(Lang::Fr, K::PadNone), "Aucune manette");
    assert_eq!(tr(Lang::Fr, K::PadXbox), "Manette Xbox");
    assert!(tr(Lang::Fr, K::SetPadHelp).contains("monopoliser") && tr(Lang::Fr, K::SetPadHelp).contains("Steam Input"));
    assert!(tr(Lang::En, K::SetPadHelp).contains("Steam Input"));
}

#[test]
fn rythme_de_lecture_economique_hors_premier_plan() {
    assert_eq!(poll_interval_ms(true, true), 10);
    assert!(poll_interval_ms(true, false) >= 250, "fenetre reduite : lecture lente");
    assert!(poll_interval_ms(false, true) >= 250 && poll_interval_ms(false, false) >= 250, "aucune manette : lecture lente");
}

#[test]
fn recherche_hid_espacee_hors_premier_plan_et_transmise_aux_sources() {
    // L'enumeration HID ouvre brievement tous les peripheriques HID du PC : rare quand rien ne l'exige.
    assert_eq!(hid_scan_interval_ms(true, false), 10_000);
    assert!(hid_scan_interval_ms(false, true) > hid_scan_interval_ms(true, true));
    assert!(hid_scan_interval_ms(false, false) >= 30_000);
    assert!(hid_scan_interval_ms(false, true) >= hid_scan_interval_ms(false, false));
    struct Rec(std::rc::Rc<std::cell::RefCell<Vec<bool>>>);
    impl PadSource for Rec {
        fn poll(&mut self, _now: u64) -> Vec<PadPoll> {
            vec![]
        }
        fn set_active(&mut self, a: bool) {
            self.0.borrow_mut().push(a);
        }
    }
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut hub = Hub::new(vec![Box::new(Rec(log.clone()))]);
    hub.set_active(false);
    hub.set_active(true);
    assert_eq!(*log.borrow(), vec![false, true]);
}
