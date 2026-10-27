//! Lot 2 (1.4) : connaissance intelligente. Pages HTML reelles (extraits courts) -> chemins -> gabarits ; base locale
//! GOG Galaxy (fixture SQLite) ; cache Steam binaire ; nettoyage des donnees de la file ; filtre du bruit ; replis des
//! moteurs de recherche simules (moteur bloque -> moteur suivant) avec de vraies pages de blocage.
use agss::detect::Vdf;
use agss::knowledge::paths::{self, Folders};
use agss::knowledge::web::{self, Engine, Fetch, Parsed, Resp};
use agss::knowledge::{appinfo, launchers, learn, queue, sqlite, Kind, Layer, Program};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("knowledge").join(name)).unwrap()
}

fn templates_of(html: &str) -> Vec<String> {
    let mut v: Vec<String> = paths::extract_paths(&paths::html_to_text(html)).iter().filter_map(|r| paths::normalize(r)).collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn pages_reelles_chemins_extraits_et_normalises() {
    // Discussion publique : « C:\Users\"Your user"\AppData\LocalLow\... » (guillemets HTML).
    assert_eq!(templates_of(&fixture("steam-discussion.html")), vec!["%LOCALLOW%/Lazy Bear Games/Graveyard Keeper".to_string()]);
    // Article : Windows, macOS (ignore), Linux natif, Proton ; le script de la page est ignore.
    let t = templates_of(&fixture("guide-article.html"));
    assert!(t.contains(&"%LOCALLOW%/Lazy Bear Games/Graveyard Keeper".to_string()), "{:?}", t);
    assert!(t.contains(&"$XDG_CONFIG_HOME/unity3d/Lazy Bear Games/Graveyard Keeper".to_string()), "{:?}", t);
    assert!(!t.iter().any(|x| x.contains("Ignore")), "contenu de script ignore : {:?}", t);
    // Le chemin Proton se ramene au meme gabarit Windows.
    assert_eq!(t.iter().filter(|x| *x == "%LOCALLOW%/Lazy Bear Games/Graveyard Keeper").count(), 1);
    assert!(t.iter().all(|x| !x.starts_with("C:") && !x.contains("USERNAME")), "{:?}", t);
}

#[test]
fn vraies_pages_de_blocage_reconnues() {
    let r = |b: &str, s: u16| Resp { status: s, body: b.to_string() };
    assert_eq!(web::parse(Engine::DdgHtml, &r(&fixture("ddg-blocked.html"), 202)), Parsed::Blocked);
    assert_eq!(web::parse(Engine::DdgHtml, &r(&fixture("ddg-blocked.html"), 200)), Parsed::Blocked, "meme servie en 200");
    assert_eq!(web::parse(Engine::Mojeek, &r(&fixture("mojeek-captcha.html"), 200)), Parsed::Blocked);
    assert_eq!(web::parse(Engine::Startpage, &r(&fixture("startpage-check.html"), 200)), Parsed::Blocked);
}

/// Reseau simule par prefixes d'URL.
struct Net(Vec<(String, Resp)>, RefCell<Vec<String>>);
impl Fetch for Net {
    fn get(&self, url: &str) -> Result<Resp, String> {
        self.1.borrow_mut().push(url.to_string());
        Ok(self.0.iter().find(|(p, _)| url.starts_with(p)).map(|(_, r)| r.clone()).unwrap_or(Resp { status: 404, body: String::new() }))
    }
}

#[test]
fn repli_des_moteurs_jusqu_au_premier_qui_repond_puis_verification_disque() {
    let t = tempfile::tempdir().unwrap();
    let f = Folders::under(t.path());
    let save = f.locallow.clone().unwrap().join("Lazy Bear Games").join("Graveyard Keeper");
    std::fs::create_dir_all(&save).unwrap();
    std::fs::write(save.join("1.dat"), b"x").unwrap();
    let ok = |b: String| Resp { status: 200, body: b };
    // DDG HTML et lite : vraie page de blocage ; Brave : 429 ; Mojeek : vraie page « Captcha » ; Startpage : resultats.
    let sp = r#"<div class="w-gl__result"><a class="result-link" href="https://guide.example.org/gk">Graveyard Keeper save location</a><p class="description">Saves are stored locally.</p></div>"#;
    let net = Net(
        vec![
            ("https://html.duckduckgo.com/".into(), Resp { status: 202, body: fixture("ddg-blocked.html") }),
            ("https://lite.duckduckgo.com/".into(), Resp { status: 202, body: fixture("ddg-blocked.html") }),
            ("https://search.brave.com/".into(), Resp { status: 429, body: String::new() }),
            ("https://www.mojeek.com/".into(), ok(fixture("mojeek-captcha.html"))),
            ("https://www.startpage.com/".into(), ok(sp.to_string())),
            ("https://guide.example.org/".into(), ok(fixture("guide-article.html"))),
        ],
        RefCell::new(vec![]),
    );
    let p = Program { key: "graveyard keeper.exe".into(), exe: "Graveyard Keeper.exe".into(), name: "Graveyard Keeper".into(), ..Default::default() };
    let now = agss::util::now_ms();
    let r = web::run(&p, t.path(), &net, &f, now, 0);
    assert_eq!(r.engine.as_deref(), Some("Startpage"), "{:?}", r.log);
    assert_eq!(r.found.len(), 1, "{:?}", r);
    assert_eq!(r.found[0].path, save);
    assert_eq!(r.found[0].layer, Layer::Web);
    assert_eq!(r.found[0].kind, Kind::Save);
    let st = web::State::load(t.path());
    for e in [Engine::DdgHtml, Engine::DdgLite, Engine::Brave, Engine::Mojeek] {
        assert!(st.is_paused(e, now + 1), "{:?} doit etre en pause", e);
    }
    assert!(!st.is_paused(Engine::Startpage, now + 1));
    // Ordre des essais : DDG HTML, DDG lite, Brave, Mojeek, Startpage.
    let asked = net.1.borrow().clone();
    let pos = |h: &str| asked.iter().position(|u| u.contains(h)).unwrap();
    assert!(pos("html.duckduckgo") < pos("lite.duckduckgo") && pos("lite.duckduckgo") < pos("brave") && pos("brave") < pos("mojeek") && pos("mojeek") < pos("startpage"));
    // Sans fichier recent sur le disque : rien n'est propose.
    let t2 = tempfile::tempdir().unwrap();
    let f2 = Folders::under(t2.path());
    std::fs::create_dir_all(f2.locallow.clone().unwrap().join("Lazy Bear Games").join("Graveyard Keeper")).unwrap();
    let r2 = web::run(&p, t2.path(), &net, &f2, now, 0);
    assert!(r2.found.is_empty());
}

#[test]
fn quota_journalier_respecte() {
    let t = tempfile::tempdir().unwrap();
    let f = Folders::under(t.path());
    let net = Net(vec![("https://html.duckduckgo.com/".into(), Resp { status: 200, body: "<div class=\"no-results\">No results.</div>".into() })], RefCell::new(vec![]));
    let now = 20_000 * 86_400_000u64 + 3_600_000;
    for i in 0..web::MAX_PER_DAY {
        let p = Program { key: format!("p{}.exe", i), exe: format!("P{}.exe", i), name: format!("Programme Zorg{}", i), ..Default::default() };
        assert_eq!(web::run(&p, t.path(), &net, &f, now, 0).skipped, None);
    }
    let p = Program { key: "trop.exe".into(), exe: "Trop.exe".into(), name: "Programme Trop".into(), ..Default::default() };
    assert_eq!(web::run(&p, t.path(), &net, &f, now, 0).skipped, Some("quota du jour atteint"));
    assert_eq!(web::run(&p, t.path(), &net, &f, now + 86_400_000, 0).skipped, None, "nouveau jour");
}

#[test]
fn base_locale_gog_galaxy() {
    let db = sqlite::Db::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("knowledge").join("galaxy-mini.db")).unwrap();
    let names: Vec<String> = db.tables().into_iter().map(|t| t.0).collect();
    assert!(names.contains(&"CloudSavesLocations".to_string()) && names.contains(&"LimitedDetails".to_string()), "{:?}", names);
    let all = db.table("CloudSavesLocations").unwrap();
    assert_eq!(all.len(), 804, "arbre B sur plusieurs niveaux lu en entier");
    let long = all.iter().find(|r| r.get("productId").and_then(|v| v.as_i64()) == Some(1999999999)).unwrap();
    assert_eq!(long.get("location").and_then(|v| v.as_text()).map(|s| s.len()), Some(5000 + "<?INSTALL?>/".len()), "pages de debordement");
    let z = launchers::gog_cloud_rows(&db, 1207658925);
    assert_eq!(z.len(), 2);
    let tpl: Vec<Option<String>> = z.iter().map(|(l, _)| launchers::gog_template(l)).collect();
    assert!(tpl.contains(&Some("%LOCALLOW%/Tiny Trinket Games/Zoria - Age of Shattering/Saves".to_string())));
    let det = db.table("LimitedDetails").unwrap();
    assert_eq!(det.iter().find(|r| r.get("title").and_then(|v| v.as_text()) == Some("Zoria")).and_then(|r| r.get("id")).and_then(|v| v.as_i64()), Some(2), "INTEGER PRIMARY KEY = rowid");
}

#[test]
fn gog_de_bout_en_bout() {
    let t = tempfile::tempdir().unwrap();
    let inst = t.path().join("GOG Games").join("Zoria");
    std::fs::create_dir_all(&inst).unwrap();
    std::fs::write(inst.join("goggame-1207658925.info"), b"{}").unwrap();
    let f = Folders::under(t.path());
    let save = f.locallow.clone().unwrap().join("Tiny Trinket Games").join("Zoria - Age of Shattering").join("Saves");
    std::fs::create_dir_all(&save).unwrap();
    std::fs::write(save.join("slot.sav"), b"x").unwrap();
    let env = launchers::Env { gog_db: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("knowledge").join("galaxy-mini.db")), ..Default::default() };
    let p = Program { exe: "Zoria.exe".into(), exe_path: Some(inst.join("Zoria.exe")), install_dir: Some(inst.clone()), ..Default::default() };
    let v = launchers::find(&p, &f, &env);
    assert_eq!(v.iter().map(|x| x.path.clone()).collect::<Vec<PathBuf>>(), vec![save]);
    assert_eq!(v[0].kind, Kind::Save);
    assert_eq!(v[0].template.as_deref(), Some("%LOCALLOW%/Tiny Trinket Games/Zoria - Age of Shattering/Saves"));
}

#[test]
fn cache_steam_binaire_plusieurs_applications() {
    let m = |v: Vec<(&str, Vdf)>| Vdf::Map(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    let st = |s: &str| Vdf::Str(s.to_string());
    let mut apps = Vec::new();
    for id in 1..200u32 {
        apps.push((id, m(vec![("common", m(vec![("name", st(&format!("App {}", id)))])), ("ufs", m(vec![("savefiles", m(vec![("0", m(vec![("root", st("WinAppDataLocal")), ("path", st(&format!("Studio/App{}", id)))]))]))]))])));
    }
    for ver in [27, 28, 29] {
        let a = appinfo::AppInfo::parse(appinfo::writer::appinfo(ver, &apps)).unwrap();
        assert_eq!(a.len(), 199);
        let u = appinfo::ufs_of(&a.app(150).unwrap());
        assert_eq!(u.name.as_deref(), Some("App 150"));
        assert_eq!(launchers::steam_templates(&u, true), vec!["%LOCALAPPDATA%/Studio/App150".to_string()]);
    }
}

#[test]
fn nettoyage_de_la_file_et_bruit() {
    let users = vec!["Joueur".to_string()];
    let names = vec!["Zoria".to_string()];
    let r = queue::Record { program: Some("Zoria".into()), publisher: None, exe: Some("D:\\x\\Zoria.exe".into()), template: "%LOCALLOW%/Tiny Trinket Games/Zoria/Joueur/1234567".into(), kind: Kind::Save, layer: Layer::Launcher, app_version: "1.4.0".into() };
    let s = queue::sanitize(r, &users, &names).unwrap();
    assert_eq!(s.template, "%LOCALLOW%/Tiny Trinket Games/Zoria/*/*");
    assert_eq!(s.exe.as_deref(), Some("Zoria.exe"));
    let c = |s: &str| s.split('/').map(|x| x.to_string()).collect::<Vec<_>>();
    assert!(learn::is_noise("%LOCALAPPDATA%", &c("Zoria/Saved/Logs/z.log")));
    assert!(!learn::is_noise("%LOCALLOW%", &c("Tiny Trinket Games/Zoria/Saves/slot.sav")));
}
