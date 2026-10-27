//! 2.0.1 : le seuil de l'alerte poids est REGLABLE et unique. Fichier a part : le seuil est un reglage du processus (un
//! seul test ici, aucune course avec les autres tests). Dossiers temporaires seulement.
use agss::config::Game;
use agss::process::ProcessChecker;
use agss::scan::{self, Origin, Personal, Size, Tier};
use agss::{weight, Ctx, Res};
use std::path::Path;
use tempfile::TempDir;

const MB: u64 = 1024 * 1024;

struct NotRunning;
impl ProcessChecker for NotRunning {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(false)
    }
}

#[test]
fn seuil_reglable_pilote_la_regle_des_dossiers_perso_les_versions_et_la_taille_du_coffre() {
    let t = TempDir::new().unwrap();
    let mut docs = scan::folder_item(t.path());
    docs.origin = Origin::Personal(Personal::Documents);
    docs.key = "folder:documents".into();
    docs.newest_ms = Some(agss::util::now_ms());
    // seuil par defaut
    assert_eq!(weight::limit_mb(), 500);
    assert!(!scan::apply_size_rule(&mut docs.clone(), &Size { bytes: 300 * MB, files: 1, partial: false }));
    assert_eq!(scan::keep_for(&docs, 300 * MB), agss::backup::DEFAULT_KEEP);
    // seuil abaisse a 100 Mo : le meme dossier de 300 Mo devient « Peut-être » avec 3 versions
    weight::set_limit_mb(100);
    assert_eq!(weight::limit_mb(), 100);
    assert!(weight::is_heavy(300 * MB));
    assert!(scan::apply_size_rule(&mut docs, &Size { bytes: 300 * MB, files: 1, partial: false }));
    assert_eq!(docs.tier, Tier::Maybe);
    assert!(!docs.default_checked());
    assert_eq!(scan::keep_for(&docs, 300 * MB), scan::BIG_KEEP);
    assert_eq!(weight::default_keep(300 * MB), weight::HEAVY_KEEP);
    // seuil releve : plus rien n'est lourd
    weight::set_limit_mb(2000);
    assert!(!weight::is_heavy(1500 * MB));
    // taille de la version active lue dans le coffre pour la pastille « Lourd » de la tuile
    weight::set_limit_mb(10);
    let mut ctx = Ctx::new(t.path().join("data"));
    ctx.procs = Box::new(NotRunning);
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let vault = t.path().join("coffre");
    std::fs::create_dir_all(&vault).unwrap();
    let src = t.path().join("Gros");
    std::fs::create_dir_all(&src).unwrap();
    let f = std::fs::File::create(src.join("monde.bin")).unwrap();
    f.set_len(12 * MB).unwrap();
    drop(f);
    let g = Game { name: "Gros".into(), path: src.clone(), ..Default::default() };
    agss::backup::backup(&ctx, &g, &vault).unwrap();
    let mut cfg = agss::config::Config::default();
    cfg.dest = Some(vault.clone());
    cfg.games.push(g);
    let facts = agss::home::gather(&ctx, &cfg, &agss::settings::Settings::default(), &Default::default());
    assert_eq!(facts.len(), 1);
    assert!(facts[0].bytes >= 12 * MB, "{}", facts[0].bytes);
    assert!(weight::is_heavy(facts[0].bytes), "pastille « Lourd » au-dessus du seuil reglable");
    weight::set_limit_mb(500);
    assert!(!weight::is_heavy(facts[0].bytes));
    let _ = Path::new("");
}
