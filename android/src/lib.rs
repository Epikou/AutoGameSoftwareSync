//! AutoGameSoftwareSync pour consoles portables Android (0.1.0, preversion).
//! Point d'entree `android_main` (NativeActivity) : journal logcat, dossier de donnees interne de l'appli, nettoyage des
//! restes d'un arret brutal, interface eframe. Le moteur (coffre, versions immuables, copies de securite, profils
//! d'emulateurs) est la bibliotheque `agss` du PC.
pub mod app;
pub mod i18n;
pub mod model;
pub mod pad;
pub mod screens;
pub mod sys;
pub mod worker;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android_app: android_activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("agss"));
    // Une panique Rust avec `panic = "abort"` ne laisse dans logcat qu'un « Fatal signal 6 » : on journalise d'abord.
    std::panic::set_hook(Box::new(|info| {
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "panique".into());
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        log::error!("PANIQUE : {} ({})", msg, loc);
    }));
    let data_dir = android_app.internal_data_path().map(|p| p.join("AutoGameSoftwareSync")).unwrap_or_else(|| std::path::PathBuf::from(format!("/data/data/{}/files/AutoGameSoftwareSync", sys::PACKAGE)));
    let _ = std::fs::create_dir_all(&data_dir);
    // Le moteur lit ce dossier de donnees et ce nom d'appareil (comme sur le PC) ; aucun reglage systeme.
    std::env::set_var("AUTOGAMESOFTWARESYNC_DATA_DIR", &data_dir);
    let prefs = model::Prefs::load(&data_dir);
    if !prefs.device_name.is_empty() {
        std::env::set_var("AUTOGAMESOFTWARESYNC_MACHINE", &prefs.device_name);
    }
    log::info!("demarrage {} ; donnees : {}", app::VERSION, data_dir.display());
    // Restes d'un arret brutal pendant une copie (un seul processus sur Android : aucun autre ecrivain) : dossiers de
    // travail locaux et version de CET appareil restee sans meta.json dans le coffre (jamais une version complete).
    let cleaned = model::startup_cleanup(&data_dir, &prefs.device_name);
    if cleaned.0 > 0 || cleaned.1.is_some() {
        log::info!("nettoyage apres arret brutal : {} dossier(s) de travail, version incomplete retiree : {:?}", cleaned.0, cleaned.1);
    }
    let options = eframe::NativeOptions { android_app: Some(android_app), ..Default::default() };
    let dd = data_dir.clone();
    if let Err(e) = eframe::run_native("AutoGameSoftwareSync", options, Box::new(move |cc| Ok(Box::new(app::App::new(dd, &cc.egui_ctx, cleaned))))) {
        log::error!("interface : {}", e);
    }
}
