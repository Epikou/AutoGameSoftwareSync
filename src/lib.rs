//! AutoGameSoftwareSync v0.3 - bibliotheque : sauvegarde versionnee anti-perte + logique de l'interface.
pub mod actions;
pub mod addform;
pub mod addpage;
pub mod appdirs;
pub mod autofill;
pub mod backup;
pub mod banner;
pub mod cloud;
pub mod cloudpoll;
pub mod config;
pub mod detect;
pub mod dirwatch;
pub mod emulators;
pub mod gamepad;
#[cfg(windows)]
pub mod gamepad_sys;
#[cfg(not(windows))]
#[path = "gamepad_sys_linux.rs"]
pub mod gamepad_sys;
pub mod gui;
pub mod gamedb;
pub mod gamedetect;
pub mod hashcache;
pub mod home;
pub mod legacy;
pub mod linuxapps;
pub mod hints;
pub mod i18n;
pub mod i18n_es;
pub mod i18n_ru;
pub mod i18n_zh;
pub mod catalog;
pub mod knowledge;
pub mod machinecolor;
pub mod migrate;
pub mod notify;
pub mod onboarding;
pub mod padctl;
pub mod padfocus;
pub mod perf;
pub mod picker;
pub mod playnite;
pub mod preview;
pub mod process;
pub mod procfacts;
pub mod profiles;
pub mod profiles_data;
pub mod reactive;
#[cfg(target_os = "android")]
pub mod rfd_shim;
#[cfg(windows)]
pub mod registry;
pub mod proton;
pub mod procscan;
pub mod autostart;
pub mod settings;
pub mod startup;
pub mod statusfile;
pub mod support;
pub mod scan;
pub mod setmode;
pub mod schedule;
pub mod selfupdate;
pub mod sensitive;
pub mod sharedfolder;
pub mod store;
pub mod theme;
pub mod timefmt;
pub mod timetravel;
pub mod transitions;
pub mod trayicon;
pub mod timesync;
pub mod uilogic;
pub mod update;
pub mod usage;
pub mod util;
pub mod vaults;
pub mod vm;
pub mod watch;
pub mod weight;
pub mod winprefs;
#[cfg(windows)]
pub mod winutil;
#[cfg(not(windows))]
#[path = "winutil_linux.rs"]
pub mod winutil;

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Resultat generique (erreurs en texte lisible).
pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Contexte d'execution : tout ce qui depend de la machine est injectable (tests).
pub struct Ctx {
    /// Dossier de donnees de l'outil (config.json, state.json, safety/, cache catalog).
    pub data_dir: PathBuf,
    /// Nom de la machine (ecrit dans les metadonnees).
    pub machine: String,
    /// Detection de processus (abstraite pour les tests).
    pub procs: Box<dyn process::ProcessChecker>,
    /// Nombre de versions a conserver (plancher absolu : 2, voir backup::MIN_KEEP).
    pub keep_versions: usize,
    /// Reglages par jeu (nom -> versions a garder). Le plancher MIN_KEEP s'applique toujours.
    pub keep_overrides: BTreeMap<String, usize>,
    /// TEST UNIQUEMENT : corrompt chaque copie avant verification pour simuler un echec.
    pub fault_corrupt_copy: bool,
    /// Horloge (heure corrigee par internet) ; simulee en test.
    pub clock: Box<dyn timesync::Clock>,
    /// Progression partagee avec l'interface (restauration / sauvegarde lancees depuis la fenetre).
    pub progress: Option<std::sync::Arc<perf::Progress>>,
    /// Cache des empreintes des fichiers LOCAUX (voir `hashcache`).
    pub hash_cache: hashcache::HashCache,
    /// Acces aux preferences de Windows (registre, commandes) : None = le vrai Windows ; tests : memoire ou cle de test.
    pub sys: Option<std::sync::Arc<dyn winprefs::Sys>>,
}

impl Ctx {
    pub fn new(data_dir: PathBuf) -> Self {
        let data_dir_cache = data_dir.join("hashcache.json");
        Ctx {
            data_dir,
            machine: util::machine_name(),
            procs: Box::new(process::ToolhelpChecker),
            keep_versions: backup::DEFAULT_KEEP,
            keep_overrides: BTreeMap::new(),
            fault_corrupt_copy: false,
            clock: Box::new(timesync::SystemClock),
            progress: None,
            hash_cache: hashcache::HashCache::new(data_dir_cache),
            sys: None,
        }
    }
}
