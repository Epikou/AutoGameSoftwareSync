//! Lot 3 (1.4) : donnees d'usage. Liste blanche, AUCUN envoi sans consentement, file locale et nettoyage, format des lots,
//! envoi quotidien, renouvellement de l'identifiant, effacement, emplacements acceptes et base maison. Tous les echanges
//! passent par un transport SIMULE : aucune requete ne part vers le serveur pendant les tests.
use agss::config::Game;
use agss::knowledge::{self, home, queue, Kind, Layer};
use agss::usage::{self, Event, State, Transport};
use serde_json::Value;
use std::cell::RefCell;
use std::path::Path;

const H: u64 = 3_600_000;
const D: u64 = 24 * H;

fn now() -> u64 {
    agss::util::ms_from_civil(2026, 10, 26, 12, 0, 0, 0) as u64
}

/// Serveur simule : journal des appels, code de reponse reglable, base maison servie.
struct Mock {
    calls: RefCell<Vec<(String, Vec<u8>)>>,
    status: std::cell::Cell<u16>,
    base: String,
}

impl Mock {
    fn new(status: u16) -> Mock {
        Mock { calls: RefCell::new(Vec::new()), status: std::cell::Cell::new(status), base: r#"{"v":1,"entries":[]}"#.into() }
    }
    fn paths(&self) -> Vec<String> {
        self.calls.borrow().iter().map(|(p, _)| p.clone()).collect()
    }
    fn bodies(&self, path: &str) -> Vec<Value> {
        self.calls.borrow().iter().filter(|(p, _)| p == path).map(|(_, b)| serde_json::from_slice(b).unwrap()).collect()
    }
}

impl Transport for Mock {
    fn post(&self, path: &str, body: &[u8]) -> Result<u16, String> {
        self.calls.borrow_mut().push((path.to_string(), body.to_vec()));
        Ok(self.status.get())
    }
    fn get(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
        self.calls.borrow_mut().push((path.to_string(), Vec::new()));
        Ok((200, self.base.clone().into_bytes()))
    }
}

fn idle() -> bool {
    false
}

fn ev(name: &str, props: &[(&str, &str)], at: u64) -> Event {
    Event::new(name, props, at)
}

fn err_ev(at: u64) -> Event {
    ev("sync_error", &[("category", "file_locked")], at)
}

#[test]
fn liste_blanche_cote_client() {
    let n = now();
    let ok = [
        usage::app_start_event(agss::i18n::Lang::Fr, false, true, false, n),
        ev("daily_state", &[("items", "6-20"), ("devices", "2"), ("vault", "gdrive"), ("detection", "on"), ("update_mode", "notify"), ("theme", "dark")], n),
        ev("item_added", &[("source", "detection"), ("category", "game"), ("multi", "no")], n),
        ev("detection_answer", &[("answer", "later"), ("reason", "launcher"), ("score", "90-100")], n),
        ev("onboarding_step", &[("step", "2"), ("action", "back"), ("duration", "10-30s")], n),
        ev("time_travel", &[("action", "restored"), ("age", "1-7d")], n),
        ev("update", &[("from", "1.3.3"), ("to", "1.4.0"), ("result", "ok")], n),
        ev("support_click", &[("place", "tutorial")], n),
        usage::known_items_event(&[("Hollow Knight".into(), false, true), ("OBS Studio".into(), true, false)], n),
    ];
    for e in &ok {
        assert_eq!(usage::validate(e, n), Ok(()), "{:?}", e);
    }
    // Noms, proprietes et valeurs hors liste : refuses.
    assert!(usage::validate(&ev("page_view", &[], n), n).is_err());
    assert!(usage::validate(&ev("item_added", &[("path", "C:/Users/Joueur")], n), n).is_err());
    assert!(usage::validate(&ev("item_added", &[("source", "bricolage")], n), n).is_err());
    assert!(usage::validate(&ev("update", &[("from", "1.3")], n), n).is_err());
    let mut old = err_ev(n);
    old.day = "2026-10-10".into();
    assert!(usage::validate(&old, n).is_err(), "jour trop ancien");
    // Titres connus : jamais de chemin, d'adresse ni de nom trop long ; doublons retires.
    let k = usage::known_items_event(&[("C:\\Users\\Joueur\\x".into(), false, false), ("moi@exemple.fr".into(), false, false), ("a/b".into(), true, false), ("x".repeat(81), false, false), ("Hollow Knight".into(), false, false), ("HOLLOW KNIGHT".into(), false, false)], n);
    let items = k.props["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "Hollow Knight");
    // Les memes regles que le serveur : liste d'evenements identique (fichier du Worker).
    let spec = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("server").join("src").join("spec.js")).unwrap();
    for (name, props) in usage::EVENTS {
        assert!(spec.contains(&format!("  {}: {{", name)), "evenement absent du serveur : {}", name);
        for (p, _) in *props {
            assert!(spec.contains(&format!("{}:", p)), "propriete absente du serveur : {}.{}", name, p);
        }
    }
}

#[test]
fn aucun_envoi_sans_consentement() {
    let t = tempfile::tempdir().unwrap();
    let dd = t.path();
    let n = now();
    // Pas de reponse ou refus : rien n'est mis en file.
    assert!(!usage::push(dd, err_ev(n), None));
    assert!(!usage::push(dd, err_ev(n), Some(false)));
    assert!(!usage::queue_path(dd).exists());
    // Un emplacement accepte existe dans la file des emplacements : il ne part pas non plus.
    let rec = queue::Record { program: Some("Hollow Knight".into()), publisher: None, exe: Some("hollow_knight.exe".into()), template: "%LOCALLOW%/Team Cherry/Hollow Knight".into(), kind: Kind::Save, layer: Layer::Deduce, app_version: "1.4.0".into() };
    assert!(queue::append(dd, rec, &[], &["Hollow Knight".into()]).unwrap());
    let m = Mock::new(200);
    let o = usage::run_once(dd, None, n, &m, &idle);
    assert!(m.paths().is_empty(), "aucune requete sans reponse : {:?}", m.paths());
    assert_eq!(o.skipped, Some("consentement non donne"));
    // Accepte puis refuse : la file est videe et rien ne part, pas meme la base maison.
    assert!(usage::push(dd, err_ev(n), Some(true)));
    assert!(usage::queue_path(dd).exists());
    let o = usage::run_once(dd, Some(false), n, &m, &idle);
    assert!(m.paths().is_empty(), "{:?}", m.paths());
    assert_eq!(o.skipped, Some("consentement refuse"));
    assert!(usage::read_queue(dd).is_empty());
    // Programme en cours d'utilisation : rien ne part.
    assert!(usage::push(dd, err_ev(n), Some(true)));
    let o = usage::run_once(dd, Some(true), n, &m, &|| true);
    assert!(m.paths().is_empty());
    assert_eq!(o.skipped, Some("programme en cours d'utilisation"));
    // Adresse du serveur : constante de compilation, en https.
    assert!(usage::SERVER.starts_with("https://"));
}

#[test]
fn file_locale_limitee_et_nettoyee() {
    let t = tempfile::tempdir().unwrap();
    let dd = t.path();
    let n = now();
    for i in 0..250u64 {
        assert!(usage::push(dd, err_ev(n + i), Some(true)));
    }
    let q = usage::read_queue(dd);
    assert_eq!(q.len(), usage::MAX_QUEUE);
    assert_eq!(q[0].at, n + 50, "les plus anciens partent");
    // Un par jour : le premier app_start compte, le dernier releve remplace l'ancien.
    let t2 = tempfile::tempdir().unwrap();
    let d2 = t2.path();
    assert!(usage::push(d2, usage::app_start_event(agss::i18n::Lang::En, false, false, false, n), Some(true)));
    assert!(!usage::push(d2, usage::app_start_event(agss::i18n::Lang::En, true, false, false, n + 1), Some(true)));
    assert!(usage::push(d2, ev("daily_state", &[("items", "0")], n), Some(true)));
    assert!(usage::push(d2, ev("daily_state", &[("items", "1-5")], n + 2), Some(true)));
    let q = usage::read_queue(d2);
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].props["mode"], "simple");
    assert_eq!(q[1].props["items"], "1-5");
    // Abandon apres 7 jours sans envoi reussi.
    let m = Mock::new(503);
    let later = n + 8 * D;
    let o = usage::run_once(d2, Some(true), later, &m, &idle);
    assert_eq!(o.sent_events, 0);
    assert!(usage::read_queue(d2).is_empty(), "evenements de plus de 7 jours abandonnes");
    // Lignes illisibles ignorees.
    std::fs::write(usage::queue_path(d2), "pas du json\n{\"name\":\"x\"}\n").unwrap();
    assert!(usage::read_queue(d2).is_empty());
}

#[test]
fn format_des_lots() {
    let n = now();
    let id = usage::new_uuid();
    let mut events: Vec<Event> = (0..230).map(|i| err_ev(n + i)).collect();
    let titles: Vec<(String, bool, bool)> = (0..100).map(|i| (format!("Titre connu numero {:03} avec un nom assez long", i), i % 2 == 0, true)).collect();
    events.push(usage::known_items_event(&titles, n));
    let batches = usage::event_batches(&id, &events);
    assert!(batches.len() >= 3);
    let mut seen = 0;
    for (idx, body) in &batches {
        assert!(body.len() <= usage::MAX_BODY, "lot de {} octets", body.len());
        assert!(idx.len() <= usage::MAX_EVENTS);
        let v: Value = serde_json::from_slice(body).unwrap();
        let mut keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["events", "install_id", "os", "v", "version"]);
        assert_eq!(v["v"], 1);
        assert_eq!(v["install_id"], id.as_str());
        for e in v["events"].as_array().unwrap() {
            let mut k: Vec<&String> = e.as_object().unwrap().keys().collect();
            k.sort();
            assert_eq!(k, vec!["day", "name", "props"], "la date de mise en file n'est jamais envoyee");
        }
        seen += idx.len();
    }
    assert_eq!(seen, events.len());
}

#[test]
fn envoi_quotidien_et_echecs() {
    let t = tempfile::tempdir().unwrap();
    let dd = t.path();
    let n = now();
    for i in 0..3 {
        usage::push(dd, err_ev(n + i), Some(true));
    }
    let m = Mock::new(200);
    let o = usage::run_once(dd, Some(true), n, &m, &idle);
    assert_eq!(o.sent_events, 3);
    assert_eq!(m.paths(), vec!["/v1/knowledge/base".to_string(), "/v1/events".to_string()]);
    assert!(usage::read_queue(dd).is_empty());
    let id = usage::current_id(dd).unwrap();
    assert_eq!(m.bodies("/v1/events")[0]["install_id"], id.as_str());
    // Une fois par jour au plus.
    usage::push(dd, err_ev(n + 2 * H), Some(true));
    let m2 = Mock::new(200);
    let o = usage::run_once(dd, Some(true), n + 2 * H, &m2, &idle);
    assert!(m2.paths().is_empty());
    assert_eq!(o.skipped, Some("deja envoye aujourd'hui"));
    // Echec du serveur : file gardee, nouvel essai 6 h plus tard seulement.
    let m3 = Mock::new(503);
    let o = usage::run_once(dd, Some(true), n + D + H, &m3, &idle);
    assert!(o.error.is_some());
    assert_eq!(usage::read_queue(dd).len(), 1);
    let m4 = Mock::new(200);
    usage::run_once(dd, Some(true), n + D + 2 * H, &m4, &idle);
    assert!(m4.paths().iter().all(|p| p != "/v1/events"), "pas avant 6 h");
    let o = usage::run_once(dd, Some(true), n + D + 8 * H, &m4, &idle);
    assert_eq!(o.sent_events, 1);
    // Lot refuse par le serveur (format) : abandonne, jamais renvoye en boucle.
    usage::push(dd, err_ev(n + 3 * D), Some(true));
    let m5 = Mock::new(400);
    let o = usage::run_once(dd, Some(true), n + 3 * D, &m5, &idle);
    assert_eq!(o.dropped_events, 1);
    assert!(usage::read_queue(dd).is_empty());
}

#[test]
fn identifiant_renouvele_et_effacement() {
    let t = tempfile::tempdir().unwrap();
    let dd = t.path();
    let n = now();
    assert_eq!(usage::current_id(dd), None, "rien n'est cree avant le premier envoi");
    let a = usage::install_id(dd, n);
    assert!(usage::is_uuid_v4(&a));
    assert_eq!(usage::install_id(dd, n + 300 * D), a, "stable pendant 13 mois");
    let b = usage::install_id(dd, n + 397 * D);
    assert_ne!(a, b, "renouvele apres 13 mois");
    assert_eq!(usage::current_id(dd), Some(b.clone()));
    // Jamais lie au nom de l'appareil.
    let raw = std::fs::read_to_string(dd.join(usage::ID_FILE)).unwrap();
    assert!(!raw.to_lowercase().contains(&agss::util::machine_name().to_lowercase()));
    // Effacer mes donnees : demande en attente, file videe, nouvel identifiant ; envoyee meme sans consentement.
    usage::push(dd, err_ev(n), Some(true));
    let old = usage::request_erase(dd, n + 398 * D).unwrap();
    assert_eq!(old, b);
    assert_ne!(usage::current_id(dd).unwrap(), b);
    assert!(usage::read_queue(dd).is_empty());
    assert_eq!(State::load(dd).pending_erase, vec![b.clone()]);
    let off = Mock::new(503);
    usage::run_once(dd, Some(false), n + 398 * D, &off, &idle);
    assert_eq!(State::load(dd).pending_erase.len(), 1, "hors ligne : redemande plus tard");
    let m = Mock::new(200);
    let o = usage::run_once(dd, Some(false), n + 398 * D, &m, &idle);
    assert_eq!(o.erased, 1);
    assert_eq!(m.paths(), vec!["/v1/erase".to_string()]);
    assert_eq!(m.bodies("/v1/erase")[0], serde_json::json!({ "install_id": b }));
    assert!(State::load(dd).pending_erase.is_empty());
}

#[test]
fn emplacements_acceptes_et_base_maison() {
    let t = tempfile::tempdir().unwrap();
    let dd = t.path().join("data");
    let n = now();
    let rec = queue::Record { program: Some("Hollow Knight".into()), publisher: Some("Team Cherry".into()), exe: Some("hollow_knight.exe".into()), template: "%LOCALLOW%/Team Cherry/Hollow Knight".into(), kind: Kind::Save, layer: Layer::Deduce, app_version: "1.4.0".into() };
    assert!(queue::append(&dd, rec, &[], &["Hollow Knight".into()]).unwrap());
    let mut m = Mock::new(200);
    m.base = r#"{"v":1,"entries":[{"program":"Celeste","publisher":"Maddy Makes Games","exe":"Celeste.exe","template":"%LOCALAPPDATA%/Celeste/Saves","kind":"save","n":6,"validated":false},{"program":"X","template":"D:/Jeux/X","kind":"save","n":9}]}"#.into();
    let o = usage::run_once(&dd, Some(true), n, &m, &idle);
    assert_eq!(o.sent_records, 1);
    assert_eq!(o.base_entries, Some(1), "entree non generique ignoree");
    let b = m.bodies("/v1/knowledge");
    assert_eq!(b[0]["records"][0]["template"], "%LOCALLOW%/Team Cherry/Hollow Knight");
    assert!(!serde_json::to_string(&b[0]).unwrap().contains("Users"));
    let base = home::Base::load(&dd);
    assert_eq!(base.entries.len(), 1, "base maison en cache");
    // Jamais renvoye.
    let m2 = Mock::new(200);
    usage::run_once(&dd, Some(true), n + D + H, &m2, &idle);
    assert!(m2.bodies("/v1/knowledge").is_empty());
    assert!(usage::pending_text(&dd, false).contains("knowledge-queue.jsonl (0)"));
    // Base maison en cache, utilisee avant les couches 1 a 4 (dossiers simules).
    let f =knowledge::paths::Folders::under(t.path());
    let save = f.local.clone().unwrap().join("Celeste").join("Saves");
    std::fs::create_dir_all(&save).unwrap();
    std::fs::write(save.join("0.celeste"), b"x").unwrap();
    let p = knowledge::Program { key: "celeste.exe".into(), exe: "Celeste.exe".into(), name: "Celeste".into(), ..Default::default() };
    let v = home::find(&p, &f, &base);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].layer, Layer::Home);
    assert_eq!(v[0].layer.code(), "home");
}

#[test]
fn titres_connus_seulement() {
    let db = agss::gamedb::GameDb::from_db(
        agss::gamedb::Db { format: 1, generated: "2026-10-26".into(), games: vec![agss::gamedb::DbGame { name: "Hollow Knight".into(), exes: vec!["hollow_knight.exe".into()], ..Default::default() }] },
        vec![],
    );
    let profiles = agss::profiles::load_embedded();
    let home = home::Base::default();
    let g = |name: &str, exe: Option<&str>| Game { name: name.into(), exe: exe.map(String::from), ..Default::default() };
    // Nom tape a la main : seul le titre de la base part.
    assert_eq!(usage::known_title(&g("Mes parties a moi", Some("C:/Jeux/hollow_knight.exe")), &db, &profiles, &home), Some(("Hollow Knight".to_string(), false)));
    assert_eq!(usage::known_title(&g("Baldur's Gate 3", None), &db, &profiles, &home), Some(("Baldur's Gate 3".to_string(), false)));
    let obs = usage::known_title(&g("Mon OBS", Some("obs64.exe")), &db, &profiles, &home).unwrap();
    assert!(obs.1, "profil d'application : logiciel");
    // Inconnu : rien.
    assert_eq!(usage::known_title(&g("Mon roman", Some("C:/x/notes-perso.exe")), &db, &profiles, &home), None);
    assert_eq!(usage::known_title(&g("Mes documents", None), &db, &profiles, &home), None);
    // Tranches et categories.
    assert_eq!((usage::bucket_count(0), usage::bucket_count(5), usage::bucket_count(6), usage::bucket_count(21)), ("0", "1-5", "6-20", "21+"));
    assert_eq!(usage::vault_kind(Some(Path::new("G:\\Mon Drive\\AGSS")), false), "gdrive");
    assert_eq!(usage::vault_kind(Some(Path::new("\\\\nas\\partage")), false), "network");
    assert_eq!(usage::vault_kind(Some(Path::new("E:\\Sauvegardes")), true), "usb");
    assert_eq!(usage::vault_kind(None, false), "none");
    assert_eq!(usage::error_category("Le processus ne peut pas acceder au fichier (os error 32)"), "file_locked");
    assert_eq!(usage::error_category("Il n'y a pas assez d'espace sur le disque. (os error 112)"), "disk_full");
    assert_eq!(usage::error_category("Accès refusé. (os error 5)"), "permissions");
    assert_eq!(usage::error_category("bizarre"), "other");
}
