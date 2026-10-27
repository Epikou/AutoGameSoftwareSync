use super::*;
use std::io::Cursor;
use std::net::TcpListener;
use tempfile::tempdir;

const SEED: [u8; 32] = [7u8; 32];

fn asset(url: &str, size: u64, sha: &str, member: &str) -> Asset {
    Asset { url: url.into(), size, sha256: sha.into(), member: member.into(), exe_sha256: None }
}

fn manifest(version: &str) -> Manifest {
    let mut platforms = BTreeMap::new();
    let h = "a".repeat(64);
    platforms.insert("win-x64".to_string(), asset("https://example.org/a.zip", 1000, &h, "AutoGameSoftwareSync.exe"));
    platforms.insert("linux-x86_64".to_string(), asset("https://example.org/a.tar.gz", 1000, &h, "AutoGameSoftwareSync"));
    platforms.insert("linux-x86_64-tray".to_string(), asset("https://example.org/a.tar.gz", 1000, &h, "AutoGameSoftwareSync-tray"));
    Manifest { schema: 1, version: version.into(), date: "2026-10-26".into(), notes_url: "https://example.org/notes".into(), min_version: None, platforms }
}

fn signed(m: &Manifest) -> String {
    sign_manifest_file(&serde_json::to_string(m).unwrap(), &SEED)
}

// ------------------------------------------------------------------ signature

#[test]
fn signature_valide_acceptee() {
    let key = public_from_seed(&SEED);
    let m = manifest("1.4.1");
    let got = verify_signed(signed(&m).as_bytes(), &key).unwrap();
    assert_eq!(got, m);
    assert!(got.check(false).is_ok());
}

#[test]
fn signature_invalide_refusee() {
    let key = public_from_seed(&SEED);
    let file = signed(&manifest("1.4.1"));
    // contenu modifie apres signature (version changee)
    let tampered = file.replace("1.4.1", "9.9.9");
    assert_ne!(tampered, file);
    assert_eq!(verify_signed(tampered.as_bytes(), &key).unwrap_err(), UpdateError::BadSignature);
    // autre cle
    let other = public_from_seed(&[9u8; 32]);
    assert_eq!(verify_signed(file.as_bytes(), &other).unwrap_err(), UpdateError::BadSignature);
    // signature alteree / tronquee / absente
    let sf: SignedFile = serde_json::from_str(&file).unwrap();
    let mut bad = sf.signature.clone();
    bad.replace_range(0..2, if &bad[0..2] == "00" { "01" } else { "00" });
    let f2 = serde_json::to_string(&SignedFile { payload: sf.payload.clone(), signature: bad }).unwrap();
    assert_eq!(verify_signed(f2.as_bytes(), &key).unwrap_err(), UpdateError::BadSignature);
    let f3 = serde_json::to_string(&SignedFile { payload: sf.payload.clone(), signature: sf.signature[..100].to_string() }).unwrap();
    assert_eq!(verify_signed(f3.as_bytes(), &key).unwrap_err(), UpdateError::BadSignature);
    let f4 = serde_json::to_string(&SignedFile { payload: sf.payload, signature: String::new() }).unwrap();
    assert_eq!(verify_signed(f4.as_bytes(), &key).unwrap_err(), UpdateError::BadSignature);
    // pas du JSON
    assert!(matches!(verify_signed(b"n'importe quoi", &key), Err(UpdateError::Format(_))));
    // tous les echecs de signature donnent le code d'usage attendu
    assert_eq!(UpdateError::BadSignature.code(), "signature_ko");
}

#[test]
fn cle_publique_integree_valide_et_distincte_de_la_cle_de_test() {
    let k = public_key();
    assert_ne!(k, [0u8; 32]);
    assert!(VerifyingKey::from_bytes(&k).is_ok());
    // un manifeste signe avec une autre cle n'est pas accepte par la cle integree
    assert_eq!(verify_signed(signed(&manifest("1.4.1")).as_bytes(), &k).unwrap_err(), UpdateError::BadSignature);
}

#[test]
fn manifeste_controles_de_coherence() {
    assert!(manifest("1.4.1").check(false).is_ok());
    let mut m = manifest("1.4.1");
    m.platforms.get_mut("win-x64").unwrap().url = "http://example.org/a.zip".into();
    assert!(m.check(false).is_err(), "http refuse");
    m.platforms.get_mut("win-x64").unwrap().url = "http://127.0.0.1:8000/a.zip".into();
    assert!(m.check(false).is_err(), "http local refuse hors debogage");
    m.notes_url = "http://127.0.0.1:8000/notes".into();
    assert!(m.check(true).is_ok(), "http local accepte en debogage");
    for member in ["../x.exe", "a/b.exe", "a\\b.exe", "", ".."] {
        let mut m = manifest("1.4.1");
        m.platforms.get_mut("win-x64").unwrap().member = member.into();
        assert!(m.check(false).is_err(), "{:?}", member);
    }
    let mut m = manifest("1.4.1");
    m.platforms.get_mut("win-x64").unwrap().sha256 = "xyz".into();
    assert!(m.check(false).is_err());
    let mut m = manifest("1.4.1");
    m.platforms.get_mut("win-x64").unwrap().size = 0;
    assert!(m.check(false).is_err());
    let mut m = manifest("1.4.1");
    m.platforms.get_mut("win-x64").unwrap().size = MAX_ARCHIVE_BYTES + 1;
    assert!(m.check(false).is_err());
    let mut m = manifest("pas-une-version");
    assert!(m.check(false).is_err());
    m = manifest("1.4.1");
    m.schema = 2;
    assert!(m.check(false).is_err());
    m = manifest("1.4.1");
    m.platforms.clear();
    assert!(m.check(false).is_err());
}

// ------------------------------------------------------------------ versions et plateforme

#[test]
fn comparaison_des_versions() {
    assert!(update::is_newer("1.4.0", "1.4.0-dev"), "la version finale est plus recente que sa pre-version");
    assert!(!update::is_newer("1.4.0-dev", "1.4.0"));
    assert!(!update::is_newer("1.4.0", "1.4.0"));
    assert!(update::is_newer("1.4.1", "1.4.0-dev"));
    assert!(update::is_newer("v1.4.1", "1.4.0"));
    assert!(update::is_newer("1.10.0", "1.9.9"));
    assert!(update::is_newer("2.0.0", "1.99.99"));
    assert!(!update::is_newer("1.3.3", "1.4.0-dev"));
    assert!(update::is_newer("1.4.0-rc.1", "1.4.0-dev"));
    assert!(!update::is_newer("n'importe quoi", "1.4.0"));
}

#[test]
fn choix_de_la_plateforme() {
    assert_eq!(platform_key("windows", "x86_64", false), Some("win-x64"));
    assert_eq!(platform_key("windows", "x86_64", true), Some("win-x64"), "Windows : toujours une icone, une seule plateforme");
    assert_eq!(platform_key("linux", "x86_64", false), Some("linux-x86_64"));
    assert_eq!(platform_key("linux", "x86_64", true), Some("linux-x86_64-tray"));
    assert_eq!(platform_key("linux", "aarch64", false), None);
    assert_eq!(platform_key("macos", "x86_64", false), None);
    assert_eq!(platform_key("windows", "aarch64", false), None);
    // la plateforme de ce binaire existe dans un manifeste complet
    let p = current_platform().expect("plateforme de test prise en charge");
    assert!(manifest("1.4.1").platforms.contains_key(p));
}

#[test]
fn decision_de_mise_a_jour() {
    let m = manifest("1.4.1");
    let win = Some("win-x64");
    assert_eq!(decide(&m, "1.4.0-dev", &[], win), Decision::Available);
    assert_eq!(decide(&m, "1.4.1", &[], win), Decision::UpToDate);
    assert_eq!(decide(&m, "1.5.0", &[], win), Decision::UpToDate);
    assert_eq!(decide(&m, "1.4.0", &["1.4.1".into()], win), Decision::Blocked);
    assert_eq!(decide(&m, "1.4.0", &["v1.4.1".into()], win), Decision::Blocked, "forme « v1.4.1 » reconnue");
    assert_eq!(decide(&m, "1.4.0", &["1.4.0".into()], win), Decision::Available, "seule la version fautive est bloquee");
    assert_eq!(decide(&m, "1.4.0", &[], None), Decision::NoPlatform);
    assert_eq!(decide(&m, "1.4.0", &[], Some("linux-arm")), Decision::NoPlatform);
    let mut m2 = manifest("1.4.1");
    m2.min_version = Some("1.4.0".into());
    assert_eq!(decide(&m2, "1.3.3", &[], win), Decision::ManualStep("1.4.0".into()));
    assert_eq!(decide(&m2, "1.4.0-dev", &[], win), Decision::ManualStep("1.4.0".into()), "la pre-version 1.4.0-dev est plus ancienne que 1.4.0");
    assert_eq!(decide(&m2, "1.4.0", &[], win), Decision::Available);
}

#[test]
fn controle_quotidien() {
    let h = 3600 * 1000;
    let mut st = State::default();
    assert!(due(&st, 100 * h), "jamais controle");
    st.last_check_ms = 100 * h;
    assert!(!due(&st, 101 * h), "controle il y a 1 h");
    assert!(!due(&st, 123 * h), "controle il y a 23 h");
    assert!(due(&st, 125 * h), "controle il y a 25 h");
    // echec reseau : nouvel essai au bout d'une heure, pas avant
    let st2 = State { last_attempt_ms: 100 * h, ..Default::default() };
    assert!(!due(&st2, 100 * h + 30 * 60 * 1000));
    assert!(due(&st2, 102 * h));
    // horloge reculee : on controle
    let st3 = State { last_check_ms: 500 * h, ..Default::default() };
    assert!(due(&st3, 100 * h));
}

// ------------------------------------------------------------------ SHA-256 et taille

#[test]
fn sha256_et_taille_verifies() {
    let d = tempdir().unwrap();
    let f = d.path().join("x.bin");
    std::fs::write(&f, b"contenu du fichier").unwrap();
    let h = util::hash_file(&f).unwrap();
    assert!(verify_file(&f, 18, &h).is_ok());
    assert!(verify_file(&f, 18, &h.to_uppercase()).is_ok());
    assert_eq!(verify_file(&f, 18, &"0".repeat(64)).unwrap_err(), UpdateError::BadHash);
    assert_eq!(verify_file(&f, 17, &h).unwrap_err(), UpdateError::BadSize);
    assert_eq!(verify_file(&f, 19, &h).unwrap_err(), UpdateError::BadSize);
    assert_eq!(UpdateError::BadHash.code(), "hash_ko");
    assert_eq!(UpdateError::BadSize.code(), "hash_ko");
    assert_eq!(UpdateError::Download("x".into()).code(), "download_failed");
}

/// Petit serveur HTTP local : `routes` = (chemin, corps, avec Content-Length ?). Repond a `n` requetes.
fn serve(routes: Vec<(&'static str, Vec<u8>, bool)>, n: usize) -> (String, std::thread::JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let h = std::thread::spawn(move || {
        for _ in 0..n {
            let Ok((mut s, _)) = l.accept() else { return };
            let mut req = Vec::new();
            let mut b = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => req.extend_from_slice(&b[..k]),
                }
            }
            let line = String::from_utf8_lossy(&req).lines().next().unwrap_or("").to_string();
            let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
            match routes.iter().find(|(p, _, _)| *p == path) {
                Some((_, body, with_len)) => {
                    let head = if *with_len { format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()) } else { "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string() };
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(body);
                }
                None => {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        }
    });
    (base, h)
}

fn zip_bytes(name: &str, data: &[u8]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
    z.write_all(data).unwrap();
    z.finish().unwrap().into_inner()
}

fn targz_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut t = tar::Builder::new(gz);
    for (name, data) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        t.append_data(&mut h, name, *data).unwrap();
    }
    t.into_inner().unwrap().finish().unwrap()
}

fn manifest_for(base: &str, file: &str, archive: &[u8], member: &str, exe: &[u8]) -> Manifest {
    let mut m = manifest("1.4.1");
    let a = Asset { url: format!("{}/{}", base, file), size: archive.len() as u64, sha256: util::hash_bytes(archive), member: member.into(), exe_sha256: Some(util::hash_bytes(exe)) };
    m.platforms.insert("win-x64".into(), a);
    m
}

#[test]
fn telechargement_verifie_extrait_et_prepare() {
    let exe = b"MZ faux exe 1.4.1".to_vec();
    let zip = zip_bytes("AutoGameSoftwareSync.exe", &exe);
    let (base, srv) = serve(vec![("/AutoGameSoftwareSync-v1.4.1-win-x64.zip", zip.clone(), true)], 1);
    let d = tempdir().unwrap();
    let m = manifest_for(&base, "AutoGameSoftwareSync-v1.4.1-win-x64.zip", &zip, "AutoGameSoftwareSync.exe", &exe);
    let mut last = (0, 0);
    let p = stage(d.path(), &m, "win-x64", &mut |a, b| last = (a, b)).unwrap();
    srv.join().unwrap();
    assert_eq!(last, (zip.len() as u64, zip.len() as u64), "progression jusqu'au bout");
    assert_eq!(std::fs::read(&p.staged).unwrap(), exe);
    assert_eq!(p.version, "1.4.1");
    assert_eq!(p.exe_size, exe.len() as u64);
    assert_eq!(p.exe_sha256, util::hash_bytes(&exe));
    assert!(p.staged.starts_with(update_dir(d.path()).join("1.4.1")));
    assert!(!update_dir(d.path()).join("1.4.1").join("AutoGameSoftwareSync-v1.4.1-win-x64.zip").exists(), "archive supprimee apres extraction");
    assert!(verify_file(&p.staged, p.exe_size, &p.exe_sha256).is_ok());
}

#[test]
fn telechargement_sha256_incorrect_refuse() {
    let exe = b"MZ faux exe".to_vec();
    let zip = zip_bytes("AutoGameSoftwareSync.exe", &exe);
    let (base, srv) = serve(vec![("/a.zip", zip.clone(), true)], 1);
    let d = tempdir().unwrap();
    let mut m = manifest_for(&base, "a.zip", &zip, "AutoGameSoftwareSync.exe", &exe);
    m.platforms.get_mut("win-x64").unwrap().sha256 = "0".repeat(64);
    let e = stage(d.path(), &m, "win-x64", &mut |_, _| {}).unwrap_err();
    srv.join().unwrap();
    assert_eq!(e, UpdateError::BadHash);
    assert!(!update_dir(d.path()).join("1.4.1").join("AutoGameSoftwareSync.exe").exists(), "rien d'extrait");
}

#[test]
fn telechargement_taille_incorrecte_refusee() {
    let exe = b"MZ faux exe".to_vec();
    let zip = zip_bytes("AutoGameSoftwareSync.exe", &exe);
    // 1) taille annoncee differente du Content-Length
    let (base, srv) = serve(vec![("/a.zip", zip.clone(), true)], 1);
    let d = tempdir().unwrap();
    let mut m = manifest_for(&base, "a.zip", &zip, "AutoGameSoftwareSync.exe", &exe);
    m.platforms.get_mut("win-x64").unwrap().size += 5;
    assert_eq!(stage(d.path(), &m, "win-x64", &mut |_, _| {}).unwrap_err(), UpdateError::BadSize);
    srv.join().unwrap();
    // 2) serveur sans Content-Length qui envoie PLUS que la taille annoncee : coupe, aucun fichier laisse
    let (base, srv) = serve(vec![("/a.zip", zip.clone(), false)], 1);
    let mut m = manifest_for(&base, "a.zip", &zip, "AutoGameSoftwareSync.exe", &exe);
    m.platforms.get_mut("win-x64").unwrap().size -= 5;
    let d2 = tempdir().unwrap();
    assert_eq!(stage(d2.path(), &m, "win-x64", &mut |_, _| {}).unwrap_err(), UpdateError::BadSize);
    srv.join().unwrap();
    let dir = update_dir(d2.path()).join("1.4.1");
    assert!(std::fs::read_dir(&dir).map(|r| r.count()).unwrap_or(0) == 0, "aucun fichier partiel");
    // 3) serveur sans Content-Length qui envoie MOINS : refuse aussi
    let (base, srv) = serve(vec![("/a.zip", zip.clone(), false)], 1);
    let mut m = manifest_for(&base, "a.zip", &zip, "AutoGameSoftwareSync.exe", &exe);
    m.platforms.get_mut("win-x64").unwrap().size += 5;
    let d3 = tempdir().unwrap();
    assert_eq!(stage(d3.path(), &m, "win-x64", &mut |_, _| {}).unwrap_err(), UpdateError::BadSize);
    srv.join().unwrap();
}

#[test]
fn telechargement_exe_extrait_different_de_l_annonce_refuse() {
    let exe = b"MZ vrai".to_vec();
    let zip = zip_bytes("AutoGameSoftwareSync.exe", &exe);
    let (base, srv) = serve(vec![("/a.zip", zip.clone(), true)], 1);
    let d = tempdir().unwrap();
    // l'archive est intacte (SHA correct) mais le programme qu'elle contient n'est pas celui du manifeste
    let m = manifest_for(&base, "a.zip", &zip, "AutoGameSoftwareSync.exe", b"autre contenu");
    assert_eq!(stage(d.path(), &m, "win-x64", &mut |_, _| {}).unwrap_err(), UpdateError::BadHash);
    srv.join().unwrap();
}

#[test]
fn telechargement_erreur_http_et_serveur_absent() {
    let (base, srv) = serve(vec![], 1);
    let d = tempdir().unwrap();
    let m = manifest_for(&base, "absent.zip", b"x", "AutoGameSoftwareSync.exe", b"x");
    assert!(matches!(stage(d.path(), &m, "win-x64", &mut |_, _| {}).unwrap_err(), UpdateError::Download(_)));
    srv.join().unwrap();
    // plateforme absente du manifeste
    assert!(matches!(stage(d.path(), &m, "linux-arm", &mut |_, _| {}).unwrap_err(), UpdateError::Format(_)));
    // manifeste volumineux refuse
    let (base, srv) = serve(vec![("/m.json", vec![b'x'; 70_000], true)], 1);
    assert!(matches!(fetch_bytes(&format!("{}/m.json", base), MAX_MANIFEST_BYTES), Err(UpdateError::Format(_))));
    srv.join().unwrap();
}

#[test]
fn extraction_zip_et_targz() {
    let d = tempdir().unwrap();
    // zip : fichier a la racine, ou dans un sous-dossier
    let z = d.path().join("a.zip");
    std::fs::write(&z, zip_bytes("dossier/AutoGameSoftwareSync.exe", b"EXE")).unwrap();
    let out = d.path().join("out1");
    assert_eq!(extract_member(&z, "AutoGameSoftwareSync.exe", &out).unwrap(), 3);
    assert_eq!(std::fs::read(&out).unwrap(), b"EXE");
    assert!(matches!(extract_member(&z, "absent.exe", &d.path().join("o")), Err(UpdateError::Install(_))));
    // tar.gz : le bon fichier parmi plusieurs (binaire et variante tray)
    let t = d.path().join("a.tar.gz");
    std::fs::write(&t, targz_bytes(&[("AutoGameSoftwareSync", b"PLAIN"), ("AutoGameSoftwareSync-tray", b"TRAY!"), ("install-linux.sh", b"#!/bin/sh")])).unwrap();
    let o2 = d.path().join("out2");
    extract_member(&t, "AutoGameSoftwareSync-tray", &o2).unwrap();
    assert_eq!(std::fs::read(&o2).unwrap(), b"TRAY!");
    let o3 = d.path().join("out3");
    extract_member(&t, "AutoGameSoftwareSync", &o3).unwrap();
    assert_eq!(std::fs::read(&o3).unwrap(), b"PLAIN");
    // nom dangereux ou format inconnu
    assert!(extract_member(&t, "../evil", &d.path().join("o4")).is_err());
    assert!(extract_member(&t, "a/b", &d.path().join("o5")).is_err());
    let rar = d.path().join("a.rar");
    std::fs::write(&rar, b"x").unwrap();
    assert!(matches!(extract_member(&rar, "x", &d.path().join("o6")), Err(UpdateError::Format(_))));
    // archive corrompue
    let bad = d.path().join("bad.zip");
    std::fs::write(&bad, b"pas un zip").unwrap();
    assert!(extract_member(&bad, "x", &d.path().join("o7")).is_err());
}

// ------------------------------------------------------------------ remplacement

fn fake_install(dir: &Path) -> (PathBuf, PathBuf) {
    let exe = dir.join("AutoGameSoftwareSync.exe");
    std::fs::write(&exe, b"ANCIEN 1.4.0").unwrap();
    let staged = dir.join("staged-nouveau.exe");
    std::fs::write(&staged, b"NOUVEAU 1.4.1").unwrap();
    (exe, staged)
}

#[test]
fn remplacement_garde_le_chemin_et_l_ancienne_version() {
    let d = tempdir().unwrap();
    let (exe, staged) = fake_install(d.path());
    let old = replace_exe(&exe, &staged).unwrap();
    assert_eq!(old, d.path().join("AutoGameSoftwareSync.exe.old"));
    assert_eq!(std::fs::read(&exe).unwrap(), b"NOUVEAU 1.4.1", "le chemin de l'exe ne change pas");
    assert_eq!(std::fs::read(&old).unwrap(), b"ANCIEN 1.4.0", "ancienne version gardee");
    assert!(!d.path().join("AutoGameSoftwareSync.exe.new").exists(), "aucun fichier temporaire laisse");
    assert!(staged.exists(), "le fichier telecharge n'est pas deplace");
    // une 2e mise a jour remplace l'ancienne sauvegarde par la version qui tournait
    std::fs::write(&staged, b"NOUVEAU 1.4.2").unwrap();
    replace_exe(&exe, &staged).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"NOUVEAU 1.4.2");
    assert_eq!(std::fs::read(&old).unwrap(), b"NOUVEAU 1.4.1");
}

#[cfg(unix)]
#[test]
fn remplacement_linux_atomique_et_executable() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempdir().unwrap();
    let (exe, staged) = fake_install(d.path());
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = replace_exe(&exe, &staged).unwrap();
    assert_eq!(std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755, "nouveau programme executable");
    assert_eq!(std::fs::metadata(&old).unwrap().permissions().mode() & 0o777, 0o755, "ancien programme executable");
    // un processus qui tient l'ancien fichier ouvert n'est pas perturbe : l'ancien contenu reste lisible
    assert_eq!(std::fs::read(&old).unwrap(), b"ANCIEN 1.4.0");
}

#[test]
fn remplacement_dossier_non_inscriptible_ou_exe_absent() {
    let d = tempdir().unwrap();
    let ghost = d.path().join("inexistant").join("AutoGameSoftwareSync.exe");
    let staged = d.path().join("s.exe");
    std::fs::write(&staged, b"N").unwrap();
    assert_eq!(replace_exe(&ghost, &staged).unwrap_err(), UpdateError::NotWritable);
    assert!(!can_replace(&ghost));
    assert!(can_replace(&d.path().join("x.exe")));
    // exe courant absent : rien n'est casse, erreur claire, pas de fichier temporaire
    let exe = d.path().join("AutoGameSoftwareSync.exe");
    assert!(matches!(replace_exe(&exe, &staged), Err(UpdateError::Install(_))));
    assert!(!exe.exists());
    assert!(!d.path().join("AutoGameSoftwareSync.exe.new").exists());
}

#[test]
fn retour_arriere_remet_l_ancienne_version() {
    let d = tempdir().unwrap();
    let (exe, staged) = fake_install(d.path());
    replace_exe(&exe, &staged).unwrap();
    rollback_exe(&exe).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"ANCIEN 1.4.0");
    assert!(!old_path(&exe).exists(), "la sauvegarde est consommee");
    assert!(!d.path().join("AutoGameSoftwareSync.exe.failed").exists());
    // sans sauvegarde : erreur, exe intact
    assert!(rollback_exe(&exe).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"ANCIEN 1.4.0");
}

#[test]
fn installation_complete_avec_faux_veilleur() {
    let d = tempdir().unwrap();
    let dd = d.path().join("data");
    let (exe, staged) = fake_install(d.path());
    let bytes = std::fs::read(&staged).unwrap();
    let p = Pending { version: "1.4.1".into(), notes_url: "https://example.org/notes".into(), staged: staged.clone(), exe_size: bytes.len() as u64, exe_sha256: util::hash_bytes(&bytes) };
    let called = std::cell::RefCell::new(None);
    let spawn = |old: &Path, args: &[String]| -> std::io::Result<()> {
        *called.borrow_mut() = Some((old.to_path_buf(), args.to_vec()));
        Ok(())
    };
    apply_pending(&dd, &exe, &p, true, 12345, &spawn).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"NOUVEAU 1.4.1");
    let (old, args) = called.borrow().clone().unwrap();
    assert_eq!(old, old_path(&exe), "le veilleur est lance depuis l'ancienne version");
    assert_eq!(args[0], "--update-watchdog");
    assert_eq!(args[1], dd.display().to_string());
    assert_eq!(args[2], exe.display().to_string());
    assert_eq!(args[3], std::process::id().to_string());
    assert_eq!(args[5], "tray", "l'application tournait en arriere-plan");
    let st = load_state(&dd);
    let t = st.trial.unwrap();
    assert_eq!((t.from.as_str(), t.to.as_str(), t.started_ms), (update::current_version(), "1.4.1", 12345));
    assert!(!marker_present(&dd, "1.4.1"));
}

#[test]
fn installation_echec_du_veilleur_remet_l_ancienne_version() {
    let d = tempdir().unwrap();
    let dd = d.path().join("data");
    let (exe, staged) = fake_install(d.path());
    let bytes = std::fs::read(&staged).unwrap();
    let p = Pending { version: "1.4.1".into(), notes_url: "https://example.org/notes".into(), staged, exe_size: bytes.len() as u64, exe_sha256: util::hash_bytes(&bytes) };
    let spawn = |_: &Path, _: &[String]| -> std::io::Result<()> { Err(std::io::Error::other("lancement impossible")) };
    let e = apply_pending(&dd, &exe, &p, false, 1, &spawn).unwrap_err();
    assert!(matches!(e, UpdateError::Install(_)));
    assert_eq!(std::fs::read(&exe).unwrap(), b"ANCIEN 1.4.0", "retour arriere immediat");
    assert!(load_state(&dd).trial.is_none());
}

#[test]
fn installation_refuse_un_fichier_modifie_depuis_la_verification() {
    let d = tempdir().unwrap();
    let dd = d.path().join("data");
    let (exe, staged) = fake_install(d.path());
    let bytes = std::fs::read(&staged).unwrap();
    let p = Pending { version: "1.4.1".into(), notes_url: "https://example.org/notes".into(), staged: staged.clone(), exe_size: bytes.len() as u64, exe_sha256: util::hash_bytes(&bytes) };
    std::fs::write(&staged, b"NOUVEAU 1.4.X").unwrap(); // meme taille, contenu change
    let spawn = |_: &Path, _: &[String]| -> std::io::Result<()> { panic!("ne doit pas etre appele") };
    assert_eq!(apply_pending(&dd, &exe, &p, false, 1, &spawn).unwrap_err(), UpdateError::BadHash);
    assert_eq!(std::fs::read(&exe).unwrap(), b"ANCIEN 1.4.0");
    assert!(!old_path(&exe).exists());
}

// ------------------------------------------------------------------ veilleur et retour arriere

struct Fake {
    exits_after: Option<u32>,
    polls: u32,
    killed: bool,
}

impl ChildProc for Fake {
    fn has_exited(&mut self) -> bool {
        self.polls += 1;
        self.exits_after.map(|n| self.polls > n).unwrap_or(false)
    }
    fn kill(&mut self) {
        self.killed = true;
    }
}

#[test]
fn veilleur_verdicts() {
    // marqueur ecrit au 3e controle : sain
    let mut n = 0;
    let mut c = Fake { exits_after: None, polls: 0, killed: false };
    assert_eq!(supervise(&mut c, &mut || { n += 1; n > 3 }, 240, &mut || {}), Verdict::Healthy);
    // jamais de marqueur, le programme tourne toujours : delai depasse
    let mut c = Fake { exits_after: None, polls: 0, killed: false };
    let mut sleeps = 0;
    assert_eq!(supervise(&mut c, &mut || false, 240, &mut || sleeps += 1), Verdict::Timeout);
    assert_eq!(sleeps, 240);
    // s'arrete sans marqueur : plantage, detecte tout de suite (pas apres 60 s)
    let mut c = Fake { exits_after: Some(2), polls: 0, killed: false };
    let mut sleeps = 0;
    assert_eq!(supervise(&mut c, &mut || false, 240, &mut || sleeps += 1), Verdict::Crashed);
    assert!(sleeps < 5);
    // marqueur ecrit puis arret (l'utilisateur quitte aussitot) : sain
    let mut c = Fake { exits_after: Some(0), polls: 0, killed: false };
    let mut seen = 0;
    assert_eq!(supervise(&mut c, &mut || { seen += 1; seen > 1 }, 240, &mut || {}), Verdict::Healthy);
}

#[test]
fn retour_arriere_complet_bloque_la_version() {
    let d = tempdir().unwrap();
    let dd = d.path().join("data");
    let (exe, staged) = fake_install(d.path());
    replace_exe(&exe, &staged).unwrap();
    std::fs::create_dir_all(update_dir(&dd).join("1.4.1")).unwrap();
    with_state(&dd, |st| {
        st.trial = Some(Trial { from: "1.4.0".into(), to: "1.4.1".into(), notes_url: "https://example.org/notes".into(), started_ms: 1 });
        st.pending = Some(Pending { version: "1.4.1".into(), notes_url: String::new(), staged: PathBuf::from("x"), exe_size: 1, exe_sha256: "0".repeat(64) });
    });
    perform_rollback(&dd, &exe, "timeout", 99).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"ANCIEN 1.4.0");
    let st = load_state(&dd);
    assert!(st.trial.is_none() && st.pending.is_none());
    assert_eq!(st.blocked, vec!["1.4.1".to_string()]);
    let r = st.report.clone().unwrap();
    assert_eq!((r.kind.as_str(), r.from.as_str(), r.to.as_str(), r.reason.as_str()), ("rollback", "1.4.0", "1.4.1", "timeout"));
    assert!(!update_dir(&dd).join("1.4.1").exists(), "dossier de travail de la version fautive nettoye");
    // la version bloquee n'est plus jamais proposee
    assert_eq!(decide(&manifest("1.4.1"), "1.4.0", &st.blocked, Some("win-x64")), Decision::Blocked);
    assert_eq!(decide(&manifest("1.4.2"), "1.4.0", &st.blocked, Some("win-x64")), Decision::Available, "une version plus recente reste proposee");
    // l'ancienne version annonce le retour arriere au demarrage, une seule fois
    let rep = confirm_started(&dd, "1.4.0", 100).unwrap();
    assert_eq!(rep.kind, "rollback");
    assert!(confirm_started(&dd, "1.4.0", 101).is_none());
}

#[test]
fn nouvelle_version_confirme_son_demarrage() {
    let d = tempdir().unwrap();
    let dd = d.path();
    std::fs::create_dir_all(update_dir(dd).join("1.4.1")).unwrap();
    with_state(dd, |st| st.trial = Some(Trial { from: "1.4.0".into(), to: "1.4.1".into(), notes_url: "https://example.org/n".into(), started_ms: 1 }));
    assert!(!marker_present(dd, "1.4.1"));
    let r = confirm_started(dd, "1.4.1", 50).unwrap();
    assert_eq!((r.kind.as_str(), r.from.as_str(), r.to.as_str(), r.notes_url.as_str()), ("ok", "1.4.0", "1.4.1", "https://example.org/n"));
    assert!(marker_present(dd, "1.4.1"), "marqueur « demarrage OK » ecrit");
    assert!(!marker_present(dd, "1.4.2"));
    assert!(load_state(dd).trial.is_none());
    assert!(!update_dir(dd).join("1.4.1").exists(), "fichiers telecharges nettoyes");
    assert!(confirm_started(dd, "1.4.1", 51).is_none(), "annonce unique");
    // essai obsolete (autre version) : clos sans annonce ni marqueur
    with_state(dd, |st| st.trial = Some(Trial { from: "1.4.0".into(), to: "1.4.5".into(), notes_url: String::new(), started_ms: 1 }));
    assert!(confirm_started(dd, "1.4.1", 52).is_none());
    assert!(!marker_present(dd, "1.4.5"));
    assert!(load_state(dd).trial.is_none());
    // instance deja ouverte apres une mise a jour : seul le marqueur est ecrit
    with_state(dd, |st| st.trial = Some(Trial { from: "1.4.1".into(), to: "1.4.2".into(), notes_url: String::new(), started_ms: 1 }));
    confirm_marker_only(dd, "1.4.2");
    assert!(marker_present(dd, "1.4.2"));
}

#[test]
fn version_prete_toujours_valable() {
    let d = tempdir().unwrap();
    let dd = d.path();
    let f = dd.join("staged.exe");
    std::fs::write(&f, b"NOUVEAU").unwrap();
    let p = Pending { version: "1.4.1".into(), notes_url: String::new(), staged: f.clone(), exe_size: 7, exe_sha256: util::hash_bytes(b"NOUVEAU") };
    with_state(dd, |st| st.pending = Some(p.clone()));
    assert_eq!(valid_pending(dd, "1.4.0"), Some(p.clone()));
    assert_eq!(valid_pending(dd, "1.4.1"), None, "deja a jour");
    std::fs::write(&f, b"CORROMPU").unwrap();
    assert_eq!(valid_pending(dd, "1.4.0"), None, "fichier modifie");
    std::fs::write(&f, b"NOUVEAU").unwrap();
    with_state(dd, |st| st.blocked.push("1.4.1".into()));
    assert_eq!(valid_pending(dd, "1.4.0"), None, "version bloquee");
}

// ------------------------------------------------------------------ bandeau, reglage, usage

#[test]
fn bandeau_selon_le_mode_et_l_etat() {
    use crate::settings::UpdateMode::*;
    let rel = ReleaseInfo { tag: "v1.4.1".into(), url: "https://example.org/r".into() };
    let ready = |now| Phase::Ready { version: "1.4.1".into(), now };
    assert_eq!(banner_view(Never, &Phase::Available { version: "1.4.1".into(), url: "u".into(), installable: true }, Some(&rel)), BannerView::None);
    // Automatique : silencieux (telechargement, version prete), sauf etape manuelle ou echec
    assert_eq!(banner_view(Auto, &Phase::Idle, Some(&rel)), BannerView::None);
    assert_eq!(banner_view(Auto, &Phase::Downloading { version: "1.4.1".into(), done: 1, total: 2 }, None), BannerView::None);
    assert_eq!(banner_view(Auto, &ready(false), None), BannerView::None);
    assert!(matches!(banner_view(Auto, &Phase::Available { version: "1.4.1".into(), url: "u".into(), installable: false }, None), BannerView::Offer { installable: false, .. }));
    assert!(matches!(banner_view(Auto, &Phase::Failed { version: "1.4.1".into() }, Some(&rel)), BannerView::Failed { .. }));
    // Me demander : annonce, progression, installation
    assert!(matches!(banner_view(Ask, &Phase::Idle, Some(&rel)), BannerView::Offer { installable: false, .. }));
    assert!(matches!(banner_view(Ask, &Phase::Available { version: "1.4.1".into(), url: "u".into(), installable: true }, None), BannerView::Offer { installable: true, .. }));
    assert_eq!(banner_view(Ask, &Phase::Downloading { version: "1.4.1".into(), done: 50, total: 200 }, None), BannerView::Progress { version: "1.4.1".into(), percent: Some(25) });
    assert_eq!(banner_view(Ask, &Phase::Downloading { version: "1.4.1".into(), done: 0, total: 0 }, None), BannerView::Progress { version: "1.4.1".into(), percent: None });
    assert_eq!(banner_view(Ask, &ready(true), None), BannerView::Installing { version: "1.4.1".into() });
    assert!(matches!(banner_view(Ask, &ready(false), Some(&rel)), BannerView::Offer { installable: true, .. }), "version deja telechargee mais mode change en « Me demander »");
}

#[test]
fn demande_d_installation_de_l_utilisateur() {
    let rt = Runtime::default();
    rt.request_install(); // rien d'offert : sans effet
    assert_eq!(rt.phase(), Phase::Idle);
    assert!(!rt.install_requested.load(Ordering::SeqCst));
    rt.set(Phase::Available { version: "1.4.1".into(), url: "u".into(), installable: true });
    rt.request_install();
    assert!(rt.install_requested.load(Ordering::SeqCst));
    assert!(matches!(rt.phase(), Phase::Downloading { .. }));
    rt.set(Phase::Ready { version: "1.4.1".into(), now: false });
    rt.request_install();
    assert_eq!(rt.phase(), Phase::Ready { version: "1.4.1".into(), now: true });
}

#[test]
fn reglage_mode_de_mise_a_jour() {
    use crate::settings::{Settings, UpdateMode};
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.update_mode(), UpdateMode::Auto, "Automatique par defaut (anciens fichiers compris)");
    assert_eq!(Settings::default().update_mode(), UpdateMode::Auto);
    let s: Settings = serde_json::from_str(r#"{"check_updates":false}"#).unwrap();
    assert_eq!(s.update_mode(), UpdateMode::Never, "ancien interrupteur coupe => Jamais");
    let mut s = Settings::default();
    s.set_update_mode(UpdateMode::Ask);
    assert!(s.check_updates);
    assert_eq!(s.update_mode(), UpdateMode::Ask);
    s.set_update_mode(UpdateMode::Never);
    assert!(!s.check_updates);
    s.set_update_mode(UpdateMode::Auto);
    assert!(s.check_updates);
    let j = serde_json::to_string(&s).unwrap();
    assert!(j.contains("\"update_mode\":\"auto\""));
    let back: Settings = serde_json::from_str(&j).unwrap();
    assert_eq!(back.update_mode(), UpdateMode::Auto);
}

#[test]
fn evenement_d_usage_update_seulement_avec_consentement() {
    use crate::usage::{push, Event};
    let d = tempdir().unwrap();
    let now = util::now_ms();
    for res in ["ok", "rollback", "signature_ko", "hash_ko", "download_failed"] {
        let e = Event::new("update", &[("from", "1.4.0-dev"), ("to", "1.4.1"), ("result", res)], now);
        assert!(!push(d.path(), e.clone(), None), "sans reponse : rien");
        assert!(!push(d.path(), e.clone(), Some(false)), "refus : rien");
        assert!(push(d.path(), e, Some(true)), "{}", res);
    }
    assert!(!d.path().join("usage-queue.jsonl").exists() || std::fs::read_to_string(d.path().join("usage-queue.jsonl")).unwrap().lines().count() == 5);
    // les codes d'erreur correspondent a la liste blanche
    for e in [UpdateError::BadSignature, UpdateError::Format("x".into()), UpdateError::BadHash, UpdateError::BadSize, UpdateError::Download("x".into()), UpdateError::NotWritable, UpdateError::Install("x".into())] {
        let ev = Event::new("update", &[("from", "1.4.0"), ("to", "1.4.1"), ("result", e.code())], now);
        assert!(crate::usage::validate(&ev, now).is_ok(), "{:?}", e);
    }
}

#[test]
fn etat_persistant_et_arguments_du_veilleur() {
    let d = tempdir().unwrap();
    assert_eq!(load_state(d.path()), State::default());
    with_state(d.path(), |st| {
        st.last_check_ms = 5;
        st.blocked.push("1.4.9".into());
    });
    let st = load_state(d.path());
    assert_eq!((st.last_check_ms, st.blocked.clone()), (5, vec!["1.4.9".to_string()]));
    // fichier illisible : etat par defaut, jamais de plantage
    std::fs::write(update_dir(d.path()).join("state.json"), b"{pas du json").unwrap();
    assert_eq!(load_state(d.path()), State::default());
    let a = watchdog_args(Path::new("D"), Path::new("E"), 42, 60, false);
    assert_eq!(a, vec!["--update-watchdog", "D", "E", "42", "60", "normal"]);
    assert!(watch_secs() >= 3);
    assert!(url_allowed("https://x", false) && !url_allowed("http://x", true) && url_allowed("http://127.0.0.1:80/x", true) && !url_allowed("http://127.0.0.1:80/x", false));
    assert!(!url_allowed("http://127.0.0.1.evil.com/x", true), "faux hote local refuse");
    assert!(url_allowed("http://localhost:8000/a", true));
}
