//! Lot 13 (finition 2.0.0) : nettoyage des marques de position `.agss-devices`, pluriels et format des tailles selon la
//! langue, version 2.0.0. UNIQUEMENT des dossiers temporaires.
use agss::backup;
use agss::config::Game;
use agss::home;
use agss::i18n::{self, tr, tr_raw, Lang, K, SIMPLE_KEYS};
use agss::process::ProcessChecker;
use agss::{Ctx, Res};
use std::path::Path;
use tempfile::TempDir;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _e: &str) -> Res<bool> {
        Ok(false)
    }
}

fn pc(root: &Path, name: &str) -> (Ctx, Game) {
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(NotRunning);
    ctx.keep_versions = 2;
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "Hollow Knight".into(), path: root.join(format!("saves-{}", name)), exe: None, ..Default::default() };
    std::fs::create_dir_all(&game.path).unwrap();
    (ctx, game)
}

fn save(ctx: &Ctx, g: &Game, dest: &Path, body: &str) {
    std::thread::sleep(std::time::Duration::from_millis(15));
    std::fs::write(g.path.join("slot1.dat"), body).unwrap();
    backup::backup(ctx, g, dest).unwrap();
}

fn marks_dir(dest: &Path, dev: &str) -> std::path::PathBuf {
    dest.join(home::DEVICES_DIR).join(dev)
}

fn files_in(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    v.sort();
    v
}

#[test]
fn elagage_retire_seulement_les_marques_des_versions_supprimees() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("coffre");
    std::fs::create_dir_all(&dest).unwrap();
    let (a, ga) = pc(tmp.path(), "EPIKOU");
    let key = agss::cloud::dest_key(&ga);
    save(&a, &ga, &dest, "v1");
    let first = backup::list_versions(&dest, &ga);
    assert_eq!(first.len(), 1);
    let snap = agss::cloud::scan_dest(&dest, &ga);
    let v1 = snap.versions[0].id.clone();
    // EPIGOS a la version 1 (marque) ; SALON a une marque d'une version pas encore arrivee dans ce coffre (recente).
    assert!(home::note_position(&dest, "EPIGOS", &key, &v1, agss::util::now_ms()));
    assert!(home::note_position(&dest, "SALON", &key, "version-pas-encore-arrivee", agss::util::now_ms()));
    let salon_before = std::fs::read(marks_dir(&dest, "SALON").join(home::mark_file_name(&key, "version-pas-encore-arrivee"))).unwrap();
    // Versions 2 et 3 : rien n'est elague (3 gardees), aucune marque retiree.
    save(&a, &ga, &dest, "v2");
    save(&a, &ga, &dest, "v3");
    assert_eq!(backup::list_versions(&dest, &ga).len(), 3);
    assert_eq!(files_in(&marks_dir(&dest, "EPIGOS")).len(), 1, "version 1 encore la : marque gardee");
    // EPIKOU note la version 3 (la sienne, active).
    let v3 = agss::cloud::scan_dest(&dest, &ga).versions.iter().max_by_key(|v| v.sort_ms()).unwrap().id.clone();
    assert!(home::note_position(&dest, "EPIKOU", &key, &v3, agss::util::now_ms()));
    // Version 4 : la version 1 est elaguee -> la marque d'EPIGOS qui la designait est retiree, les autres restent.
    save(&a, &ga, &dest, "v4");
    assert_eq!(backup::list_versions(&dest, &ga).len(), 3);
    assert!(files_in(&marks_dir(&dest, "EPIGOS")).is_empty(), "marque de la version elaguee retiree : {:?}", files_in(&marks_dir(&dest, "EPIGOS")));
    assert_eq!(files_in(&marks_dir(&dest, "EPIKOU")), [home::mark_file_name(&key, &v3)], "marque d'une version encore presente gardee");
    let salon_after = std::fs::read(marks_dir(&dest, "SALON").join(home::mark_file_name(&key, "version-pas-encore-arrivee"))).unwrap();
    assert_eq!(salon_before, salon_after, "marque recente d'une version pas encore arrivee : gardee, jamais reecrite");
    // Les versions et les fichiers ne sont pas touches par le nettoyage.
    assert_eq!(std::fs::read_to_string(ga.path.join("slot1.dat")).unwrap(), "v4");
}

#[test]
fn marques_orphelines_anciennes_retirees_prudemment() {
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("coffre");
    std::fs::create_dir_all(&dest).unwrap();
    let (a, ga) = pc(tmp.path(), "EPIKOU");
    let key = agss::cloud::dest_key(&ga);
    save(&a, &ga, &dest, "v1");
    save(&a, &ga, &dest, "v2");
    save(&a, &ga, &dest, "v3");
    let snap = agss::cloud::scan_dest(&dest, &ga);
    let now = agss::util::now_ms();
    let day = 24 * 3600 * 1000;
    // Marque d'une version elaguee AVANT ce nettoyage (ecrite il y a 30 jours, version absente) : retiree.
    assert!(home::note_position(&dest, "VIEUXPC", &key, "elaguee-depuis-longtemps", now - 30 * day));
    // Marque recente d'une version absente (pas encore synchronisee par le service) : gardee.
    assert!(home::note_position(&dest, "PORTABLE", &key, "en-route", now));
    // Marque d'une version presente : gardee, meme ancienne.
    let present = snap.versions[0].id.clone();
    assert!(home::note_position(&dest, "DECK", &key, &present, now - 30 * day));
    assert_eq!(home::prune_stale_marks(&dest, &key, &snap, now), 1);
    assert!(files_in(&marks_dir(&dest, "VIEUXPC")).is_empty());
    assert_eq!(files_in(&marks_dir(&dest, "PORTABLE")).len(), 1);
    assert_eq!(files_in(&marks_dir(&dest, "DECK")).len(), 1);
    // Version incomplete dans le coffre (service en train de synchroniser) : aucune marque retiree.
    assert!(home::note_position(&dest, "VIEUXPC", &key, "elaguee-2", now - 30 * day));
    let mut snap2 = snap.clone();
    snap2.incomplete.push(agss::cloud::Incomplete { dir: dest.join("x"), reason: "en cours".into() });
    assert_eq!(home::prune_stale_marks(&dest, &key, &snap2, now), 0);
    // Trop peu de versions (pas d'elagage possible) : rien.
    let mut snap3 = snap.clone();
    snap3.versions.truncate(2);
    assert_eq!(home::prune_stale_marks(&dest, &key, &snap3, now), 0);
    // Liste explicite vide : rien.
    assert_eq!(home::prune_marks(&dest, &key, &[]), 0);
    // Une marque d'un autre element n'est jamais touchee.
    assert!(home::note_position(&dest, "VIEUXPC", "autre-element", "elaguee-depuis-longtemps", now - 30 * day));
    assert_eq!(home::prune_marks(&dest, &key, &["elaguee-depuis-longtemps".to_string(), "elaguee-2".to_string()]), 1);
    assert_eq!(files_in(&marks_dir(&dest, "VIEUXPC")), [home::mark_file_name("autre-element", "elaguee-depuis-longtemps")]);
}

#[test]
fn tailles_selon_la_langue() {
    let mb = 7 * 1024 * 1024 + 110 * 1024;
    assert_eq!(i18n::format_bytes(Lang::Fr, mb), "7,1 Mo");
    assert_eq!(i18n::format_bytes(Lang::En, mb), "7.1 MB");
    assert_eq!(i18n::format_bytes(Lang::Es, mb), "7,1 MB");
    assert_eq!(i18n::format_bytes(Lang::Ru, mb), "7,1 МБ");
    let zh = i18n::format_bytes(Lang::Zh, mb);
    assert!(zh == "7.1 MB", "{}", zh);
    assert_eq!(i18n::format_bytes(Lang::Fr, 0), "0 o");
    assert_eq!(i18n::format_bytes(Lang::En, 512), "512 B");
    assert_eq!(i18n::format_bytes(Lang::Fr, 4_400_000_000), "4,1 Go");
    assert_eq!(i18n::format_bytes(Lang::En, 4_400_000_000), "4.1 GB");
    // l'ancien format (journal, ligne de commande) ne change pas
    assert_eq!(agss::actions::format_bytes(1024), "1.0 Ko");
}

#[test]
fn pluriels_selon_la_langue() {
    assert!(i18n::is_singular(Lang::Fr, 0) && i18n::is_singular(Lang::Fr, 1) && !i18n::is_singular(Lang::Fr, 2));
    assert!(!i18n::is_singular(Lang::En, 0) && i18n::is_singular(Lang::En, 1) && !i18n::is_singular(Lang::En, 2));
    assert!(i18n::is_singular(Lang::Es, 1) && !i18n::is_singular(Lang::Es, 0));
    assert_eq!(tr(Lang::Fr, i18n::pick_n(Lang::Fr, 1, K::VVersionFilesOne, K::VVersionFiles)).replace("{0}", "1").replace("{1}", "6,2 Mo"), "1 fichier · 6,2 Mo");
    assert_eq!(tr(Lang::Fr, i18n::pick_n(Lang::Fr, 4, K::VVersionFilesOne, K::VVersionFiles)).replace("{0}", "4").replace("{1}", "6,2 Mo"), "4 fichiers · 6,2 Mo");
    assert_eq!(i18n::diff_now(Lang::Fr, 1, 0), "Par rapport à maintenant : 1 fichier modifié, 0 ajouté");
    assert_eq!(i18n::diff_now(Lang::Fr, 3, 2), "Par rapport à maintenant : 3 fichiers modifiés, 2 ajoutés");
    assert_eq!(i18n::diff_now(Lang::En, 1, 2), "Compared with now: 1 file changed, 2 added");
    assert_eq!(i18n::diff_now(Lang::Es, 1, 1), "Respecto a ahora: 1 archivo cambiado, 1 añadido");
    for l in Lang::ALL {
        let s = i18n::diff_now(l, 5, 7);
        assert!(s.contains('5') && s.contains('7') && !s.contains('{'), "{} : {}", l.code(), s);
    }
    assert_eq!(tr(Lang::Fr, K::VLocalNewerWarnOne).replace("{}", "1"), "1 fichier est plus récent sur cet appareil : il sera mis de côté dans une copie de sécurité.");
    // Nouvelles cles : mode simple, 5 langues, memes marqueurs que l'anglais.
    for k in [K::VVersionFilesOne, K::VDiffChangedOne, K::VDiffChangedMany, K::VDiffAddedOne, K::VDiffAddedMany, K::VDiffIdenticalOne, K::VLocalNewerWarnOne, K::VMigWingetAskBodyOne] {
        assert!(SIMPLE_KEYS.contains(&k), "{:?}", k);
        let en = tr_raw(Lang::En, k);
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(k) } else { tr_raw(l, k) };
            assert!(!s.trim().is_empty());
            for m in ["{}", "{0}", "{1}"] {
                assert_eq!(s.matches(m).count(), en.matches(m).count(), "{:?} {} {}", k, l.code(), m);
            }
        }
    }
}

/// Plus aucun ecran n'affiche une taille au format fixe : toutes passent par la langue de l'interface.
#[test]
fn ecrans_sans_format_de_taille_fixe() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("gui");
    let mut bad = Vec::new();
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                let t = std::fs::read_to_string(&p).unwrap();
                if t.contains("actions::format_bytes(") || t.contains("fmt_mb(") {
                    bad.push(p.display().to_string());
                }
            }
        }
    }
    assert!(bad.is_empty(), "taille au format fixe dans : {:?}", bad);
}

#[test]
fn elision_du_francais_apres_remplacement() {
    assert_eq!(i18n::elide(Lang::Fr, "La version de aujourd'hui à 16:38 est remise."), "La version d'aujourd'hui à 16:38 est remise.");
    assert_eq!(i18n::elide(Lang::Fr, "Remettre la version de hier à 21:14 ?"), "Remettre la version d'hier à 21:14 ?");
    assert_eq!(i18n::elide(Lang::Fr, "Nouvelle version de EPIGOS"), "Nouvelle version d'EPIGOS");
    assert_eq!(i18n::elide(Lang::Fr, "Prendre celle de Ordi-salon"), "Prendre celle d'Ordi-salon");
    // rien a changer : consonne, « h » aspire d'un nom, mot qui finit par « de »
    assert_eq!(i18n::elide(Lang::Fr, "La version de mardi 20 · copie de sécurité"), "La version de mardi 20 · copie de sécurité");
    assert_eq!(i18n::elide(Lang::Fr, "Prendre celle de Hollow"), "Prendre celle de Hollow");
    assert_eq!(i18n::elide(Lang::Fr, "Le mode à la demande est actif"), "Le mode à la demande est actif");
    assert_eq!(i18n::elide(Lang::Fr, "garde aussi"), "garde aussi");
    // autres langues : inchange
    assert_eq!(i18n::elide(Lang::Es, "La versión de ayer de EPIGOS"), "La versión de ayer de EPIGOS");
    assert_eq!(i18n::elide(Lang::En, "de EPIGOS"), "de EPIGOS");
}

/// Bout en bout d'une mise a jour depuis la 1.3.3 : dossier de donnees a l'ancien nom copie (jamais modifie), coffre a
/// l'ancien nom retrouve, etat « a jour » conserve, synchro entre deux appareils ensuite, aucune perte.
#[test]
fn mise_a_jour_depuis_la_1_3_3_de_bout_en_bout() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    // 1.3.3 : coffre sous un ancien nom dans « Mon Drive », deux versions de Celeste par EPIKOU
    let drive = root.join("Mon Drive");
    let old_vault = drive.join(agss::legacy::VAULT_DIRS[1]);
    std::fs::create_dir_all(&old_vault).unwrap();
    let old_data = root.join("ancien-dossier-de-donnees");
    std::fs::create_dir_all(&old_data).unwrap();
    let saves = root.join("Celeste-saves");
    std::fs::create_dir_all(&saves).unwrap();
    {
        let mut ctx = Ctx::new(old_data.clone());
        ctx.machine = "EPIKOU".into();
        ctx.procs = Box::new(NotRunning);
        let g = Game { name: "Celeste".into(), path: saves.clone(), exe: Some("Celeste.exe".into()), ..Default::default() };
        let cfg = agss::config::Config { games: vec![g.clone()], dest: Some(old_vault.clone()) };
        cfg.save(&ctx).unwrap();
        std::fs::write(old_data.join("settings.json"), r#"{"lang":"fr","theme":"dark","tutorial_seen":true,"usage_asked":"1.3"}"#).unwrap();
        save(&ctx, &g, &old_vault, "chapitre 6");
        save(&ctx, &g, &old_vault, "chapitre 7");
    }
    let before: Vec<(String, Vec<u8>)> = walk(&old_data);
    let vault_before = walk(&old_vault);
    // 2.0 : copie verifiee vers le nouveau dossier, l'ancien n'est pas touche
    let new_data = root.join("AutoGameSoftwareSync");
    let rep = agss::appdirs::migrate(&old_data, &new_data).unwrap();
    assert!(rep.skipped.is_none() && rep.files >= 2, "{:?}", rep);
    assert_eq!(walk(&old_data), before, "ancien dossier de donnees intact");
    let mut a = Ctx::new(new_data.clone());
    a.machine = "EPIKOU".into();
    a.procs = Box::new(NotRunning);
    let cfg = agss::config::Config::load(&a).unwrap();
    assert_eq!(cfg.games.len(), 1);
    assert_eq!(cfg.dest.as_deref(), Some(old_vault.as_path()), "le coffre de la 1.3.3 reste le coffre");
    let g = cfg.games[0].clone();
    assert_eq!(agss::vm::compute_status(&a, &g, Some(&old_vault)), agss::vm::GameStatus::UpToDate, "etat de synchro conserve");
    // premier lancement de la 2.0 : « Quoi de neuf » + consentement, pas l'assistant complet
    let s = agss::settings::Settings::load_from(&new_data).unwrap();
    let f = agss::onboarding::Facts { first_install: !s.tutorial_seen, has_vault: true, has_items: true, consent_needed: agss::usage::needs_question(s.usage_asked.as_deref(), "2.0.0") };
    let (plan, whats_new) = agss::onboarding::plan_for(&f);
    assert!(whats_new && plan == 0, "« Quoi de neuf » seul (la question des statistiques est en bas de cet ecran)");
    // le coffre a l'ancien nom est trouve et propose (repris tel quel)
    let cand = agss::vaults::Candidate { kind: agss::vaults::Kind::GoogleDrive, path: drive.clone(), streaming: Some(false), work: false };
    let found = agss::vaults::find_existing(&[cand], Some(&old_vault));
    assert_eq!(found.first().map(|e| e.path.clone()), Some(old_vault.clone()));
    assert_eq!(agss::vaults::target_for(&old_vault), old_vault);
    // un deuxieme appareil (EPIGOS, 2.0) recupere, joue, recopie ; EPIKOU recoit la nouvelle version
    let (b, mut gb) = pc(root, "EPIGOS");
    gb.name = "Celeste".into();
    backup::restore(&b, &gb, &old_vault).unwrap();
    assert_eq!(std::fs::read_to_string(gb.path.join("slot1.dat")).unwrap(), "chapitre 7");
    save(&b, &gb, &old_vault, "chapitre 8 sur EPIGOS");
    assert_eq!(agss::vm::compute_status(&a, &g, Some(&old_vault)), agss::vm::GameStatus::CloudNewer);
    backup::restore(&a, &g, &old_vault).unwrap();
    assert_eq!(std::fs::read_to_string(saves.join("slot1.dat")).unwrap(), "chapitre 8 sur EPIGOS");
    // aucune perte : toutes les versions de la 1.3.3 sont encore dans le coffre, et la partie d'avant est dans une copie
    // de securite de cet appareil
    let after = walk(&old_vault);
    for (p, bytes) in &vault_before {
        assert!(after.iter().any(|(q, b2)| q == p && b2 == bytes), "fichier du coffre perdu ou modifie : {}", p);
    }
    let safety = backup::safety_copies(&a, &g);
    assert!(!safety.is_empty(), "copie de securite avant la restauration");
}

fn walk(p: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = walkdir::WalkDir::new(p)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().strip_prefix(p).unwrap().to_string_lossy().to_string(), std::fs::read(e.path()).unwrap()))
        .collect();
    v.sort();
    v
}

/// Coffre absent (dossier du service de partage disparu, disque debranche) : la surveillance ne le recree jamais en
/// silence et n'envoie pas de notification d'erreur a chaque essai ; des qu'il revient, la copie se fait.
#[test]
fn coffre_absent_jamais_recree_par_la_surveillance() {
    use agss::watch::{Logger, WatchCfg, Watcher};
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("Dropbox").join("AutoGameSoftwareSync");
    std::fs::create_dir_all(tmp.path().join("Dropbox")).unwrap();
    let (a, mut g) = pc(tmp.path(), "EPIKOU");
    g.name = "Notes".into();
    std::fs::write(g.path.join("notes.txt"), "v1").unwrap();
    let log = Logger::new(a.data_dir.join("logs").join("watch.log"), false);
    let triggers = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Option<String>>::new()));
    let errors = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let mut w = Watcher::new(&a, vec![g.clone()], dest.clone(), WatchCfg::default(), &log);
    w.set_triggers(triggers.clone());
    let e2 = errors.clone();
    w.on_event = Some(Box::new(move |ev: agss::watch::WatchEvent| {
        if ev.kind == agss::watch::EventKind::Error {
            *e2.lock().unwrap() += 1;
        }
    }));
    w.startup(0);
    triggers.lock().unwrap().push(Some(g.name.clone()));
    w.tick(1_000);
    w.tick(2_000);
    assert!(!dest.exists(), "le coffre absent n'est pas recree");
    assert_eq!(*errors.lock().unwrap(), 0, "pas de notification d'erreur tant que le coffre est absent");
    // le coffre revient : la copie demandee se fait
    std::fs::create_dir_all(&dest).unwrap();
    triggers.lock().unwrap().push(Some(g.name.clone()));
    w.tick(3_000);
    w.tick(4_000);
    assert_eq!(backup::list_versions(&dest, &g).len(), 1);
}

#[test]
fn version_2_0_1() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "2.0.3");
}
