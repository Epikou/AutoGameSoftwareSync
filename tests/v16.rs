//! Alerte de nouvelle version (hors ligne tolere, jamais d'installation automatique).
use agss::i18n::{tr, Lang, K};
use agss::settings::Settings;
use agss::update::*;

#[test]
fn comparaison_semver_dont_preversions() {
    let v = |s: &str| parse_version(s).unwrap();
    assert!(v("0.10.1") > v("0.10.0") && v("0.11.0") > v("0.10.9") && v("1.0.0") > v("0.99.99"));
    assert!(v("0.10.0") > v("0.9.9"), "comparaison numerique, pas lexicographique");
    assert_eq!(v("v1.2.3"), v("1.2.3"));
    assert_eq!(v("1.2"), v("1.2.0"));
    assert_eq!(v("1.2.3+build5"), v("1.2.3"), "metadonnees ignorees");
    // Pre-versions : 1.0.0-beta < 1.0.0 ; beta.2 < beta.11 ; alpha < beta ; numerique < alphanumerique.
    assert!(v("1.0.0-beta") < v("1.0.0"));
    assert!(v("1.0.0-beta.2") < v("1.0.0-beta.11"));
    assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
    assert!(v("1.0.0-1") < v("1.0.0-alpha"));
    assert!(v("1.0.0-beta") < v("1.0.0-beta.1"));
    assert!(v("1.0.1-beta") > v("1.0.0"));
    for bad in ["", "abc", "1.x.0", "1.2.3.4", "1.0.0-", "1..0"] {
        assert!(parse_version(bad).is_none(), "{:?}", bad);
    }
    assert!(is_newer("v0.10.1", "0.10.0") && !is_newer("0.10.0", "0.10.0") && !is_newer("0.9.0", "0.10.0"));
    assert!(!is_newer("n'importe quoi", "0.10.0"), "illisible : jamais d'alerte");
    assert!(!is_newer("1.0.0-rc.1", "1.0.0"));
}

#[test]
fn la_version_de_l_exe_est_celle_de_cargo_toml() {
    let cargo = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let line = cargo.lines().find(|l| l.starts_with("version = ")).unwrap();
    assert!(line.contains(&format!("\"{}\"", current_version())), "{} / {}", line, current_version());
    assert!(parse_version(current_version()).is_some());
}

const SAMPLE: &str = r#"{
  "url": "https://api.github.com/repos/Epikou/AutoGameSoftwareSync/releases/1",
  "html_url": "https://github.com/Epikou/AutoGameSoftwareSync/releases/tag/v0.11.0",
  "tag_name": "v0.11.0", "name": "Version 0.11.0", "draft": false, "prerelease": false,
  "assets": [], "body": "Notes"
}"#;

#[test]
fn lecture_du_json_de_l_api() {
    let r = parse_release(SAMPLE).unwrap();
    assert_eq!((r.tag.as_str(), r.url.as_str()), ("v0.11.0", "https://github.com/Epikou/AutoGameSoftwareSync/releases/tag/v0.11.0"));
    assert!(parse_release(&SAMPLE.replace("\"draft\": false", "\"draft\": true")).is_none(), "brouillon ignore");
    assert!(parse_release(&SAMPLE.replace("\"prerelease\": false", "\"prerelease\": true")).is_none(), "pre-version ignoree");
    assert!(parse_release(&SAMPLE.replace("https://github.com", "http://github.com")).is_none(), "https obligatoire");
    assert!(parse_release(&SAMPLE.replace("v0.11.0\", \"name", "pas-une-version\", \"name")).is_none());
    assert!(parse_release("").is_none() && parse_release("{}").is_none() && parse_release("<html>").is_none());
}

#[test]
fn etat_du_bandeau_et_ignorer() {
    let r = ReleaseInfo { tag: "v0.11.0".into(), url: "https://x/y".into() };
    let cur = "0.10.0";
    assert_eq!(banner(Some(&r), cur, None, true), Some(&r));
    assert_eq!(banner(Some(&r), cur, None, false), None, "interrupteur desactive : aucun bandeau");
    assert_eq!(banner(None, cur, None, true), None);
    assert_eq!(banner(Some(&r), "0.11.0", None, true), None, "deja a jour");
    assert_eq!(banner(Some(&r), "0.12.0", None, true), None, "exe plus recent que la Release");
    // Ignorer cette version : plus de bandeau ; une version encore plus recente est annoncee de nouveau.
    assert_eq!(banner(Some(&r), cur, Some("v0.11.0"), true), None);
    assert_eq!(banner(Some(&r), cur, Some("0.11.0"), true), None, "ecriture avec ou sans v");
    let r2 = ReleaseInfo { tag: "v0.12.0".into(), url: "https://x/z".into() };
    assert_eq!(banner(Some(&r2), cur, Some("v0.11.0"), true), Some(&r2));
}

#[test]
fn hors_ligne_conserve_le_dernier_resultat_connu_et_ne_leve_rien() {
    // Aucun serveur n'ecoute sur ce port : echec silencieux.
    let f = fetch("http://127.0.0.1:9/releases/latest", None);
    assert!(matches!(f, Fetched::Offline(_)), "{:?}", f);
    let dir = tempfile::TempDir::new().unwrap();
    let mut c = load_cache(dir.path());
    assert!(c.latest.is_none() && c.etag.is_none());
    apply(&mut c, Fetched::New { info: ReleaseInfo { tag: "v0.11.0".into(), url: "https://x/y".into() }, etag: Some("W/\"abc\"".into()) });
    save_cache(dir.path(), &c);
    let mut c2 = load_cache(dir.path());
    assert_eq!(c2.etag.as_deref(), Some("W/\"abc\""));
    apply(&mut c2, Fetched::Offline("pas de reseau".into()));
    apply(&mut c2, Fetched::NotModified);
    assert_eq!(c2.latest.as_ref().map(|r| r.tag.as_str()), Some("v0.11.0"), "le bandeau reste affiche hors ligne");
    std::fs::write(dir.path().join("update-cache.json"), "{pas du json").unwrap();
    assert!(load_cache(dir.path()).latest.is_none(), "cache illisible ignore");
}

#[test]
fn reglage_verifier_les_mises_a_jour_par_defaut_et_compatibilite() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.check_updates && s.ignored_update.is_none(), "active par defaut, anciens fichiers inchanges");
    let s2: Settings = serde_json::from_str(r#"{"check_updates":false,"ignored_update":"v0.11.0"}"#).unwrap();
    assert!(!s2.check_updates);
    assert_eq!(s2.ignored_update.as_deref(), Some("v0.11.0"));
}

#[test]
fn textes_de_l_alerte_cinq_langues() {
    for k in [K::UpdAvailable, K::BtnUpdOpen, K::BtnUpdIgnore, K::TipUpdOpen, K::TipUpdIgnore, K::SetCheckUpdates, K::TipCheckUpdates, K::UpdTray, K::NotifUpdate] {
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr(l, k) };
            assert!(!s.is_empty());
        }
    }
    assert!(tr(Lang::Fr, K::UpdAvailable).contains("{}"));
    assert!(LATEST_URL.starts_with("https://api.github.com/repos/Epikou/AutoGameSoftwareSync/"));
}
