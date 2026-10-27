//! AutoGameSoftwareSync - CLI + interface graphique. Voir README.md.
//! Sous-systeme "windows" : pas de console au double-clic. En mode CLI (arguments), on s'attache
//! a la console du terminal parent (voir winutil::attach_parent_console).
#![windows_subsystem = "windows"]
use agss::backup::{self, BackupOutcome};
use agss::config::{Config, Game};
use agss::detect::{self, CatalogIndex};
use agss::catalog::{self, StoreCtx};
use agss::watch::{self, InstanceLock, Logger, WatchCfg, Watcher};
use agss::{Ctx, Res};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const USAGE: &str = concat!("AutoGameSoftwareSync ", env!("CARGO_PKG_VERSION"), "

Usage :
  AutoGameSoftwareSync --version                                # version du programme
  AutoGameSoftwareSync detect                                   # jeux installes Steam/Epic/GOG + presence dans le catalog
  AutoGameSoftwareSync game add <nom> --path <dossier_saves> [--exe <nom.exe>] [--location nom=chemin ...] [--schedule-interval HH:MM:SS | --schedule-at HH:MM:SS ...]   # ajout manuel (emplacements repetables ; planification)
  AutoGameSoftwareSync game add <nom> --from-catalog [--yes] [--exe <nom.exe>]     # chemins issus du catalogue des jeux
  AutoGameSoftwareSync game list
  AutoGameSoftwareSync config set-dest <dossier>                # dossier partage par defaut (NAS, Google Drive, Dropbox, etc.)
  AutoGameSoftwareSync config set-conflict <cloud|ask>          # conflits : prendre la version du cloud automatiquement (defaut) ou demander
  AutoGameSoftwareSync game set-conflict <nom> <cloud|ask|inherit>   # reglage par jeu
  AutoGameSoftwareSync backup  <nom> --dest <dossier>
  AutoGameSoftwareSync restore <nom> --src  <dossier> [--version <horodatage>]
  AutoGameSoftwareSync versions <nom> --dest <dossier>          # historique (nouveau + ancien format), conflit, dossiers ignores
  AutoGameSoftwareSync resolve <nom> --keep-mine | --take-cloud  # resolution explicite d'un vrai conflit
  AutoGameSoftwareSync game link <nom> --to <cle>               # relie ce jeu a un dossier existant de la destination (alias local)
  AutoGameSoftwareSync game existing [--dest <dossier>]         # jeux deja presents dans la destination
  AutoGameSoftwareSync prune <nom> [--dest <dossier>] [--keep N] [--yes]   # sans --yes : apercu seulement (minimum 2 versions garanti)
  AutoGameSoftwareSync gui [--tray]                             # interface graphique (aussi : lancer sans argument)
  AutoGameSoftwareSync quit                                     # demande a l'interface en cours de se fermer
  AutoGameSoftwareSync watch [--dest <dossier>] [--hidden] [--poll 5] [--stable 10] [--resync 300] [--quiet]
  AutoGameSoftwareSync watch --stop                             # arret propre d'un watch (meme cache)

Variable optionnelle : AUTOGAMESOFTWARESYNC_DATA_DIR (dossier de donnees, defaut %LOCALAPPDATA%\\AutoGameSoftwareSync sous Windows, ~/.local/share/AutoGameSoftwareSync sous Linux).");

/// `cloud` | `ask` | `inherit` (None = herite).
fn parse_conflict(s: &str) -> Res<Option<agss::settings::ConflictMode>> {
    use agss::settings::ConflictMode::*;
    match s.to_lowercase().as_str() {
        "cloud" => Ok(Some(Cloud)),
        "ask" => Ok(Some(Ask)),
        "inherit" => Ok(None),
        _ => Err("mode attendu : cloud, ask (ou inherit pour un jeu)".into()),
    }
}
fn conflict_label(m: agss::settings::ConflictMode) -> &'static str {
    match m {
        agss::settings::ConflictMode::Cloud => "prendre la version du cloud automatiquement",
        agss::settings::ConflictMode::Ask => "me demander",
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}
fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}
fn num(args: &[String], name: &str, default: u64) -> Res<u64> {
    match flag(args, name) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| format!("{} attend un nombre de secondes", name).into()),
    }
}

fn run() -> Res<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let data_dir = match agss::appdirs::data_dir_from_env() {
        Some(d) => d,
        None => {
            let d = agss::appdirs::default_data_dir().ok_or("dossier de donnees introuvable")?;
            // Premiere execution apres le renommage : COPIE verifiee de l'ancien dossier (jamais deplace ni supprime).
            let src = agss::appdirs::pick_first_existing(&agss::appdirs::old_data_dirs());
            if let Some(old) = src {
                if let Ok(r) = agss::appdirs::migrate(&old, &d) {
                    if r.skipped.is_none() {
                        Logger::new(d.join("logs").join("watch.log"), false).log(&agss::appdirs::report_line(&old, &r));
                    }
                }
            }
            d
        }
    };
    std::fs::create_dir_all(&data_dir)?;
    let mut ctx = Ctx::new(data_dir);
    // Ecart d'horloge memorise (utile hors ligne) ; la mesure reseau est lancee par l'interface et par `watch`.
    let _ = agss::timesync::load_saved_offset(&ctx.data_dir);
    // Reglages par jeu (versions a garder) : appliques aussi au CLI.
    if let Ok(s) = agss::settings::Settings::load(&ctx) {
        s.apply_to_ctx(&mut ctx);
    }

    match a.as_slice() {
        [] | ["gui", ..] | ["--tray", ..] => run_gui(&ctx, &args)?,
        ["migrate-data", old, new] => {
            // Diagnostic : meme code que la migration automatique, avec des dossiers de test explicites.
            let r = agss::appdirs::migrate(std::path::Path::new(old), std::path::Path::new(new))?;
            println!("{}", agss::appdirs::report_line(std::path::Path::new(old), &r));
        }
        ["--update-watchdog", dd, exe, old_pid, secs, mode] => {
            // Veilleur de mise a jour (lance par l'ancienne version, depuis son exe `.old`) : voir selfupdate.rs.
            let code = agss::selfupdate::run_watchdog(Path::new(dd), Path::new(exe), old_pid.parse().unwrap_or(0), secs.parse().unwrap_or(agss::selfupdate::WATCH_SECS), *mode == "tray");
            std::process::exit(code);
        }
        ["--version"] | ["version"] => println!("AutoGameSoftwareSync {}", env!("CARGO_PKG_VERSION")),
        ["--version-file", path] => {
            // Diagnostic (tache planifiee de test) : prouve qu'un lancement a eu lieu sans console.
            std::fs::write(path, format!("AutoGameSoftwareSync {} lance a {}
", env!("CARGO_PKG_VERSION"), agss::util::timestamp(agss::util::now_ms())))?;
        }
        ["--launch-tool", id, rest @ ..] => {
            let minimized = !rest.contains(&agss::startup::ARG_NORMAL);
            // Lance un outil cloud (Google Drive, OneDrive, Dropbox) en RE-RESOLVANT son chemin a chaque demarrage
            // (le dossier de version de Google Drive change a chaque mise a jour).
            let log = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
            match agss::startup::resolve_tool(id, &agss::startup::RealProbe) {
                Some(t) => {
                    let how = agss::winutil::launch_program(&t.exe, &t.args, minimized);
                    log.log(&format!("[Demarrage] outil {} lance : {} {} ({})", t.name, t.exe.display(), t.args, launch_how_text(how, minimized)));
                }
                None => log.log(&format!("[Demarrage] outil '{}' introuvable sur ce PC : rien lance", id)),
            }
        }
        ["--launch-program", exe, rest @ ..] => {
            // Programme personnalise d'une regle de demarrage, lance reduit si demande.
            let minimized = !rest.contains(&agss::startup::ARG_NORMAL);
            let log = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
            let how = agss::winutil::launch_program(std::path::Path::new(exe), "", minimized);
            log.log(&format!("[Demarrage] programme {} lance ({})", exe, launch_how_text(how, minimized)));
        }
        ["--apply-startup-rules", file] => {
            // Etape AVEC DROITS ELEVES, lancee par l'application (qui, elle, n'est jamais admin). Ne touche qu'aux taches
            // du dossier AutoGameSoftwareSync ; ecrit un fichier .result lisible par l'appelant.
            let f: agss::startup::ApplyFile = serde_json::from_str(&std::fs::read_to_string(file)?)?;
            let sched = agss::startup::SchtasksScheduler { tmp_dir: PathBuf::from(file).parent().map(|p| p.to_path_buf()).unwrap_or_default() };
            let out = agss::startup::apply_file(&sched, &f);
            let msg = if out.failed.is_empty() { "ok".to_string() } else { out.failed.iter().map(|(n, e)| format!("{} : {}", n, e)).collect::<Vec<_>>().join(" ; ") };
            let _ = std::fs::write(format!("{}.result", file), &msg);
            if !out.failed.is_empty() {
                std::process::exit(1);
            }
        }
        #[cfg(windows)]
        ["--apply-hosts", file] => {
            // Etape AVEC DROITS ELEVES, lancee seulement apres l'accord explicite de l'utilisateur (fiche « fichier hosts ») :
            // ecrit la copie recue du fichier hosts ; l'ancien fichier est garde a cote. Resultat dans `<fichier>.result`.
            let hosts = agss::winprefs::WinSys::from_env();
            let target = <agss::winprefs::WinSys as agss::winprefs::Sys>::hosts_path(&hosts);
            let r = agss::winprefs::write_hosts_elevated(Path::new(file), &target);
            if let Some(out) = Path::new(file).parent().and_then(|p| p.parent()) {
                let _ = std::fs::write(out.join(agss::winprefs::HOSTS_RESULT), match &r { Ok(()) => "ok".to_string(), Err(e) => e.to_string() });
            }
            if r.is_err() {
                std::process::exit(1);
            }
        }
        ["startup", "test-task", action] => cmd_startup_test(&ctx, action)?,
        ["notify-test", ..] => {
            // Diagnostic : envoie une notification Windows (sans PowerShell) et attend son affichage.
            let title = a.get(1).copied().unwrap_or("AutoGameSoftwareSync");
            let body = a.get(2).copied().unwrap_or("Notification de test");
            agss::notify::Notifier::notify(&agss::notify::WinNotifier, title, body);
            #[cfg(windows)]
            {
                std::thread::sleep(std::time::Duration::from_secs(8));
                println!("Icone de l application dans la bulle : {}", match agss::notify::ICON_RESULT.load(std::sync::atomic::Ordering::SeqCst) { 1 => "chargee (NIIF_USER + hBalloonIcon)", -1 => "ABSENTE (icone generique)", _ => "non appele" });
                println!("Shell_NotifyIconW : {}", match agss::notify::LAST_RESULT.load(std::sync::atomic::Ordering::SeqCst) { 1 => "accepte par Windows (balloon envoye)", -1 => "REFUSE", _ => "non appele" });
            }
            #[cfg(not(windows))]
            {
                std::thread::sleep(std::time::Duration::from_secs(3));
                println!("Service de notifications du bureau (D-Bus) : {}", match agss::notify::LAST_RESULT.load(std::sync::atomic::Ordering::SeqCst) { 1 => "notification acceptee", -1 => "ABSENT ou REFUS (notifications ignorees, l'appli continue)", _ => "pas de reponse" });
            }
        }
        ["quit"] => {
            agss::util::write_atomic(&ctx.data_dir.join("gui.quit"), b"quit")?;
            println!("Demande de fermeture envoyee.");
        }
        ["prune", name, ..] => {
            let cfg = Config::load(&ctx)?;
            let dest = flag(&args, "--dest").map(PathBuf::from).or(cfg.dest.clone()).ok_or("--dest requis")?;
            if let Some(k) = flag(&args, "--keep") {
                let k: usize = k.parse().map_err(|_| "--keep attend un nombre")?;
                ctx.keep_overrides.insert(name.to_string(), k.max(backup::MIN_KEEP));
            }
            let g = cfg.find(name).ok_or_else(|| format!("jeu '{}' inconnu", name))?.clone();
            let plan = agss::actions::prune_preview(&ctx, &g, &dest);
            println!("Versions gardees : au moins {} + l'active (plancher {}).", backup::keep_for(&ctx, name), backup::MIN_KEEP);
            if plan.is_empty() {
                println!("Rien a supprimer.");
            } else if has(&args, "--yes") {
                let done = agss::actions::do_prune(&ctx, &g, &dest);
                println!("{} version(s) supprimee(s).", done.len());
            } else {
                println!("Apercu (rien supprime) - ajoutez --yes pour supprimer :");
                for p in plan {
                    println!("  {}", p.display());
                }
            }
        }
        ["catalog", "find", name, ..] => cmd_find(&ctx, name)?,
        ["detect"] => cmd_detect(&ctx)?,
        ["game", "add", name, ..] if has(&args, "--from-catalog") => cmd_add_from_catalog(&ctx, name, &args)?,
        ["game", "add", name, ..] => {
            let path = flag(&args, "--path").ok_or("--path <dossier_saves> requis (ou --from-catalog)")?;
            let mut cfg = Config::load(&ctx)?;
            if cfg.find(name).is_some() {
                return Err(format!("le jeu '{}' existe deja dans la config", name).into());
            }
            // --location <nom>=<chemin> (repetable) : emplacements supplementaires (dossier ou fichier existant).
            let mut extras = Vec::new();
            for (i, a) in args.iter().enumerate() {
                if a == "--location" {
                    let v = args.get(i + 1).ok_or("--location attend nom=chemin")?;
                    let (label, p) = v.split_once('=').ok_or("--location attend nom=chemin")?;
                    let pb = PathBuf::from(p);
                    extras.push(agss::config::Location { id: agss::config::Location::new_id(label), label: label.to_string(), is_file: pb.is_file(), path: pb, filter: None, exclude: None, enabled: true });
                }
            }
            let ats: Vec<String> = args.iter().enumerate().filter(|(_, a)| *a == "--schedule-at").filter_map(|(i, _)| args.get(i + 1).cloned()).collect();
            let schedule = agss::schedule::from_cli(flag(&args, "--schedule-interval").as_deref(), &ats)?;
            cfg.games.push(Game { name: name.to_string(), path: PathBuf::from(path), exe: flag(&args, "--exe"), dest_key: None, extra_locations: extras, schedule, ..Default::default() });
            cfg.save(&ctx)?;
            println!("Jeu '{}' ajoute.", name);
        }
        ["game", "list"] => {
            let cfg = Config::load(&ctx)?;
            if cfg.games.is_empty() {
                println!("(aucun jeu configure)");
            }
            for g in &cfg.games {
                println!("{} | {} | exe: {}", g.name, g.path.display(), g.exe.as_deref().unwrap_or("-"));
            }
        }
        ["config", "set-conflict", mode] => {
            let m = parse_conflict(mode)?.ok_or("mode attendu : cloud ou ask")?;
            let mut s = agss::settings::Settings::load(&ctx)?;
            s.conflict_mode = m;
            s.save(&ctx)?;
            println!("Conflits : {}", conflict_label(m));
        }
        ["game", "set-conflict", name, mode] => {
            let cfg = Config::load(&ctx)?;
            let g = cfg.find(name).ok_or_else(|| format!("jeu '{}' inconnu", name))?;
            let mut s = agss::settings::Settings::load(&ctx)?;
            s.game_mut(&g.name).conflict = parse_conflict(mode)?;
            s.save(&ctx)?;
            println!("Jeu '{}' : conflits = {}", g.name, parse_conflict(mode)?.map(conflict_label).unwrap_or("herite du reglage global"));
        }
        ["config", "set-dest", dir] => {
            let mut cfg = Config::load(&ctx)?;
            cfg.dest = Some(PathBuf::from(dir));
            cfg.save(&ctx)?;
            println!("Dossier partage par defaut : {}", dir);
        }
        ["backup", name, ..] => {
            let cfg = Config::load(&ctx)?;
            let dest = flag(&args, "--dest").map(PathBuf::from).or(cfg.dest.clone()).ok_or("--dest <dossier> requis")?;
            let g = cfg.find(name).ok_or_else(|| format!("jeu '{}' inconnu (voir `game list`)", name))?;
            match backup::backup(&ctx, g, &dest)? {
                BackupOutcome::UpToDate => println!("Deja a jour."),
                BackupOutcome::Backed { dir } => println!("Sauvegarde OK : {} (aucune version existante n'a ete modifiee).", dir.display()),
                BackupOutcome::Conflict { dir } => println!(
                    "CONFLIT : les deux PC ont modifie la sauvegarde separement. Votre version est conservee comme branche : {}
Rien n'est ecrase. Resolvez avec `AutoGameSoftwareSync resolve {} --keep-mine` ou `--take-cloud`.",
                    dir.display(),
                    name
                ),
            }
        }
        ["game", "link", name, ..] => {
            let key = flag(&args, "--to").ok_or("--to <cle ou nom du jeu existant> requis")?;
            agss::actions::link_game(&ctx, name, &key)?;
            println!("Jeu '{}' lie au dossier de la destination '{}' (rien n'a ete modifie dans la destination).", name, agss::cloud::game_key(&key));
        }
        ["game", "existing"] => {
            let cfg = Config::load(&ctx)?;
            let dest = flag(&args, "--dest").map(PathBuf::from).or(cfg.dest.clone()).ok_or("--dest requis")?;
            for g in agss::cloud::list_dest_games(&dest) {
                println!("{}  (cle : {})", g.name, g.key);
            }
        }
        ["resolve", name, ..] => {
            let cfg = Config::load(&ctx)?;
            let dest = flag(&args, "--dest").map(PathBuf::from).or(cfg.dest.clone()).ok_or("--dest requis")?;
            let g = cfg.find(name).ok_or_else(|| format!("jeu '{}' inconnu", name))?;
            if has(&args, "--keep-mine") == has(&args, "--take-cloud") {
                return Err("precisez --keep-mine OU --take-cloud".into());
            }
            println!("{}", agss::actions::do_resolve(&ctx, g, &dest, has(&args, "--keep-mine"))?);
        }
        ["restore", name, ..] => {
            let cfg = Config::load(&ctx)?;
            let src = flag(&args, "--src").map(PathBuf::from).or(cfg.dest.clone()).ok_or("--src <dossier> requis")?;
            let g = cfg.find(name).ok_or_else(|| format!("jeu '{}' inconnu", name))?;
            let r = match flag(&args, "--version") {
                Some(v) => backup::restore_version(&ctx, g, &src, &v)?,
                None => backup::restore(&ctx, g, &src)?,
            };
            println!("Restauration OK : {} fichier(s).", r.files_restored);
            if let Some(s) = r.safety_dir {
                println!("Snapshot de securite de l'ancien etat local : {}", s.display());
            }
            if r.local_had_unsynced_changes {
                println!("ATTENTION : le local contenait des modifications non synchronisees (conservees dans le snapshot ci-dessus).");
            }
        }
        ["versions", name, ..] => {
            let cfg = Config::load(&ctx)?;
            let dest = flag(&args, "--dest").map(PathBuf::from).or(cfg.dest.clone()).ok_or("--dest requis")?;
            let g = cfg.find(name).ok_or_else(|| format!("jeu '{}' inconnu", name))?;
            let lang = agss::settings::Settings::load(&ctx).map(|s| s.lang).unwrap_or(agss::i18n::Lang::Fr);
            let h = backup::history(&ctx, g, &dest);
            if !h.conflict_heads.is_empty() {
                println!("CONFLIT en cours entre : {}", h.conflict_heads.join(" | "));
            }
            for r in &h.rows {
                let kind = match r.kind {
                    agss::cloud::VKind::New => "",
                    agss::cloud::VKind::LegacyCurrent | agss::cloud::VKind::LegacyVersion => " [ancien format]",
                    agss::cloud::VKind::LegacyConflictCopy => " [copie de conflit Drive]",
                };
                println!("{}{}  {}  {}  (nom : {}){}", if r.active { "* " } else { "  " }, agss::timefmt::format_now_tz(r.saved_at_ms, lang), r.machine, if r.head { "[tete]" } else { "" }, r.name, kind);
            }
            for i in &h.incomplete {
                println!("  incomplet (ignore) : {} ({})", i.dir.display(), i.reason);
            }
            for t in &h.tmp_dirs {
                println!("  temporaire (ignore) : {}", t.display());
            }
        }
        ["watch", ..] => cmd_watch(&ctx, &args)?,
        #[cfg(windows)]
        _ => println!("{}", USAGE),
        #[cfg(not(windows))]
        _ => println!(
            "{}",
            USAGE
                .replace("%LOCALAPPDATA%\\AutoGameSoftwareSync", "$XDG_DATA_HOME/AutoGameSoftwareSync, par defaut ~/.local/share/AutoGameSoftwareSync")
                .replace("--exe <nom.exe>", "--exe <programme>")
                .replace("Steam/Epic/GOG", "Steam/Heroic")
        ),
    }
    Ok(())
}

/// Diagnostic : cree / lance / affiche / supprime UNE tache de test `AutoGameSoftwareSync__test__` (lance `--version-file`).
fn cmd_startup_test(ctx: &Ctx, action: &str) -> Res<()> {
    use agss::startup::*;
    let sched = SchtasksScheduler { tmp_dir: ctx.data_dir.join("tmp") };
    let exe = std::env::current_exe()?;
    let marker = ctx.data_dir.join("startup-test.txt");
    let rule = Rule { id: "__test__".into(), kind: ToolKind::Custom, name: "test".into(), command: exe, args: format!("--version-file \"{}\"", marker.display()), delay_secs: 5, enabled: true, found: true, minimized: false };
    let name = task_name(&rule.id);
    match action {
        "create" => {
            sched.create(&name, &xml_to_utf16(&task_xml(&rule, &current_user()))).map_err(|e| e.to_string())?;
            println!("tache creee : {}", name);
        }
        "show" => println!("{}", sched.query_xml(&name).map_err(|e| e.to_string())?),
        "run" => {
            sched.run(&name).map_err(|e| e.to_string())?;
            println!("tache lancee ; marqueur attendu : {}", marker.display());
        }
        "delete" => {
            sched.delete(&name).map_err(|e| e.to_string())?;
            println!("tache supprimee : {}", name);
        }
        _ => return Err("action attendue : create | show | run | delete".into()),
    }
    Ok(())
}

// ------------------------------------------------------------------ watch

fn cmd_watch(ctx: &Ctx, args: &[String]) -> Res<()> {
    if has(args, "--stop") {
        agss::util::write_atomic(&watch::stop_file(ctx), b"stop")?;
        println!("Demande d'arret envoyee (le watch s'arrete a son prochain cycle).");
        return Ok(());
    }
    let mut cfg = Config::load(ctx)?;
    // Elements mis en pause depuis l'accueil : ni surveilles ni copies.
    {
        let s = agss::settings::Settings::load(ctx).unwrap_or_default();
        cfg.games.retain(|g| !s.is_paused(&g.name));
    }
    let dest = flag(args, "--dest").map(PathBuf::from).or(cfg.dest.clone()).ok_or("destination requise : --dest <dossier> ou `config set-dest`")?;
    if cfg.games.is_empty() {
        return Err("aucun jeu configure (`game add`)".into());
    }

    if has(args, "--hidden") {
        // Pre-verification pour donner un retour clair, puis relance sans console.
        drop(InstanceLock::acquire(&ctx.data_dir, &watch::pid_alive)?);
        let mut child_args: Vec<String> = args.iter().filter(|a| *a != "--hidden").cloned().collect();
        child_args.push("--quiet".into());
        let mut cmd = std::process::Command::new(std::env::current_exe()?);
        cmd.args(&child_args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0); // detache du terminal
        }
        let child = cmd.spawn()?;
        println!("watch lance en arriere-plan (pid {}). Logs : {}", child.id(), ctx.data_dir.join("logs").join("watch.log").display());
        println!("Arret : AutoGameSoftwareSync watch --stop");
        return Ok(());
    }

    let _lock = InstanceLock::acquire(&ctx.data_dir, &watch::pid_alive)?;
    let _ = std::fs::remove_file(watch::stop_file(ctx));
    let log = Logger::new(ctx.data_dir.join("logs").join("watch.log"), !has(args, "--quiet"));
    {
        let lg = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
        agss::timesync::spawn_sync(ctx.data_dir.clone(), move |m| lg.log(m));
    }
    let poll = num(args, "--poll", 5)?.max(1);
    let settings = agss::settings::Settings::load(ctx).unwrap_or_default();
    let rp = agss::reactive::params(settings.reactivity, &settings.reactivity_custom);
    let wc = WatchCfg { stable_ms: num(args, "--stable", rp.stable_ms / 1000)? * 1000, resync_ms: num(args, "--resync", rp.fallback_ms / 1000)? * 1000, ..WatchCfg::from_params(&rp) };
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    ctrlc::set_handler(move || s2.store(true, std::sync::atomic::Ordering::SeqCst))?;
    log.log(&format!("watch demarre : {} jeu(x), destination {}", cfg.games.len(), dest.display()));
    if !dest.is_dir() {
        log.log("[Demarrage] dossier partage inaccessible : attente (5 min au plus)");
        let (d2, s3) = (dest.clone(), stop.clone());
        let ok = agss::startup::wait_until(&|| d2.is_dir(), &|| s3.load(std::sync::atomic::Ordering::SeqCst), &|s| std::thread::sleep(std::time::Duration::from_secs(s)), 300, 3);
        log.log(if ok { "[Demarrage] dossier partage accessible" } else { "[Demarrage] dossier partage toujours inaccessible : la surveillance demarre quand meme" });
    }
    let (ev_tx, ev_rx) = std::sync::mpsc::channel();
    // Linux : interrogation directe du dossier partage, comme l'interface. Indispensable pour un dossier monte (FUSE de
    // Google Drive / Nextcloud, CIFS, NFS, SSHFS...) ou inotify ne voit pas les fichiers deposes par l'autre PC.
    #[cfg(not(windows))]
    let _poll_stop = {
        let poll_ms = agss::reactive::poll_interval_ms(settings.reactivity, settings.poll_custom_s);
        let st = Arc::new(AtomicBool::new(false));
        agss::cloudpoll::spawn(dest.clone(), cfg.games.clone(), poll_ms, ev_tx.clone(), st.clone());
        log.log(&format!("[Surveillance] interrogation du dossier partage toutes les {} s", (poll_ms / 1000).max(1)));
        st
    };
    let lg2 = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
    let _dirwatch = agss::dirwatch::DirWatch::spawn(dest.clone(), ev_tx, move |m| lg2.log(m));
    let mut w = Watcher::new(ctx, cfg.games.clone(), dest, wc, &log);
    w.set_events(ev_rx);
    for g in &cfg.games {
        w.auto_cloud.insert(g.name.clone(), settings.conflict_for(&g.name) == agss::settings::ConflictMode::Cloud);
    }
    watch::run_loop(&mut w, ctx, poll * 1000, &stop);
    Ok(())
}

// ------------------------------------------------------------------ detect

fn cmd_detect(ctx: &Ctx) -> Res<()> {
    let det = detect::detect_all();
    println!(
        "Bibliotheques Steam : {}",
        if det.steam_libraries.is_empty() { "(Steam non trouve)".into() } else { det.steam_libraries.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" ; ") }
    );
    let m = catalog::load_best(ctx).ok();
    if m.is_none() {
        println!("(catalogue des jeux indisponible : rapprochement ignore)");
    }
    let idx = m.as_ref().map(CatalogIndex::new);
    let mut matched = 0;
    for g in &det.games {
        let mm = idx.as_ref().and_then(|i| i.find(g));
        if mm.is_some() {
            matched += 1;
        }
        println!(
            "[{}] {} (id {}) -> {}{}",
            g.store.label(),
            g.name,
            g.id,
            g.install_dir.display(),
            match mm {
                Some(k) => format!("  | catalog: {}", k),
                None if idx.is_some() => "  | pas dans le catalog".into(),
                None => String::new(),
            }
        );
    }
    println!("\n{} jeu(x) detecte(s), {} avec une entree catalog.", det.games.len(), matched);
    Ok(())
}

// ------------------------------------------------------------------ game add --from-catalog

fn cmd_add_from_catalog(ctx: &Ctx, name: &str, args: &[String]) -> Res<()> {
    let m = catalog::load_best(ctx)?;
    let found = catalog::search(&m, name);
    let (mname, entry) = match found.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
        Some(x) => *x,
        None if found.len() == 1 => found[0],
        None if found.is_empty() => return Err(format!("'{}' introuvable dans le catalog", name).into()),
        None => {
            let l: Vec<&str> = found.iter().take(10).map(|(k, _)| k.as_str()).collect();
            return Err(format!("nom ambigu, precisez : {}", l.join(" | ")).into());
        }
    };
    println!("Catalog : {}", mname);

    let det = detect::detect_all();
    let idx = CatalogIndex::new(&m);
    let inst = det.games.iter().find(|g| idx.find(g).map(|k| k == mname).unwrap_or(false));
    if let Some(i) = inst {
        println!("Installe ({}) : {}", i.store.label(), i.install_dir.display());
    }
    let sc = StoreCtx { steam_libs: det.steam_libraries.clone(), install_dir: inst.map(|i| i.install_dir.clone()), game_id: inst.map(|i| i.id.clone()) };
    let r = catalog::resolve_with(entry, mname, &sc);
    if !r.unresolved.is_empty() {
        println!("Placeholders non resolus (ignores) : {}", r.unresolved.join(", "));
    }

    let mut dirs_found: Vec<(PathBuf, usize)> = Vec::new();
    for p in &r.paths {
        let hits: Vec<PathBuf> = if p.contains('*') || p.contains('?') {
            catalog::expand_glob(p)
        } else if Path::new(p).exists() {
            vec![PathBuf::from(p)]
        } else {
            vec![]
        };
        for h in hits {
            let d = if h.is_dir() { h } else { h.parent().map(|x| x.to_path_buf()).unwrap_or(h) };
            if catalog::is_generic_root(&d) {
                println!("  (ignore, dossier trop generique pour etre sauvegarde en entier : {})", d.display());
                continue;
            }
            if !dirs_found.iter().any(|(x, _)| x == &d) {
                let n = walkdir::WalkDir::new(&d).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).take(5000).count();
                dirs_found.push((d, n));
            }
        }
    }
    if dirs_found.is_empty() {
        return Err("aucun chemin de sauvegarde existant trouve pour ce jeu (lancez-le une fois, ou ajoutez-le a la main avec --path)".into());
    }
    for (i, (d, n)) in dirs_found.iter().enumerate() {
        println!("  [{}] {}  ({}{} fichier(s))", i + 1, d.display(), n, if *n >= 5000 { "+" } else { "" });
    }

    let chosen: Vec<usize> = if has(args, "--yes") {
        (0..dirs_found.len()).collect()
    } else {
        print!("Numeros a ajouter (ex: 1,3 ou 'tous', vide = annuler) : ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        catalog::parse_selection(&line, dirs_found.len())?
    };
    if chosen.is_empty() {
        println!("Annule, rien ajoute.");
        return Ok(());
    }

    let exe = flag(args, "--exe").or_else(|| inst.and_then(|i| i.exe.clone())).or_else(|| inst.and_then(|i| detect::guess_exe(&i.install_dir)));
    match &exe {
        Some(e) => println!("Exe surveille : {}{}", e, if flag(args, "--exe").is_some() { "" } else { " (deduit, a verifier)" }),
        None => println!("Exe inconnu : precisez-le avec --exe pour que `watch` et la protection 'jeu en cours' fonctionnent."),
    }
    let mut cfg = Config::load(ctx)?;
    for (k, i) in chosen.iter().enumerate() {
        let gname = if chosen.len() == 1 { name.to_string() } else { format!("{} #{}", name, k + 1) };
        if cfg.find(&gname).is_some() {
            println!("'{}' existe deja : ignore.", gname);
            continue;
        }
        cfg.games.push(Game { name: gname.clone(), path: dirs_found[*i].0.clone(), exe: exe.clone(), dest_key: None, ..Default::default() });
        println!("Ajoute : {} -> {}", gname, dirs_found[*i].0.display());
    }
    cfg.save(ctx)?;
    Ok(())
}

fn cmd_find(ctx: &Ctx, q: &str) -> Res<()> {
    let m = catalog::load_best(ctx)?;
    let found = catalog::search(&m, q);
    if found.is_empty() {
        println!("Aucun jeu ne correspond a '{}'.", q);
        return Ok(());
    }
    println!("{} resultat(s){}", found.len(), if found.len() > 15 { " (15 premiers affiches)" } else { "" });
    let det = detect::detect_all();
    let idx = CatalogIndex::new(&m);
    for (name, entry) in found.into_iter().take(15) {
        let steam = entry.steam.as_ref().and_then(|s| s.id).map(|i| format!(" [Steam {}]", i)).unwrap_or_default();
        println!("\n== {}{}", name, steam);
        let inst = det.games.iter().find(|g| idx.find(g).map(|k| k == name).unwrap_or(false));
        let sc = StoreCtx { steam_libs: det.steam_libraries.clone(), install_dir: inst.map(|i| i.install_dir.clone()), game_id: inst.map(|i| i.id.clone()) };
        let r = catalog::resolve_with(entry, name, &sc);
        if r.paths.is_empty() {
            println!("  (aucun chemin Windows resolu)");
        }
        for p in &r.paths {
            if p.contains('*') || p.contains('?') {
                let hits = catalog::expand_glob(p);
                if hits.is_empty() {
                    println!("  {}  [aucune correspondance]", p);
                } else {
                    println!("  {}  [{} correspondance(s)]", p, hits.len());
                    for h in hits.iter().take(5) {
                        println!("      -> {}", h.display());
                    }
                }
            } else {
                println!("  {}  [{}]", p, if Path::new(p).exists() { "present" } else { "absent" });
            }
        }
        if !r.unresolved.is_empty() {
            println!("  Placeholders non resolus (chemins ignores) : {}", r.unresolved.join(", "));
        }
    }
    Ok(())
}

/// Texte de journal : comment le programme a ete lance (la demande « reduit » peut etre ignoree par le programme).
fn launch_how_text(how: agss::winutil::LaunchHow, wanted_min: bool) -> &'static str {
    use agss::winutil::LaunchHow::*;
    match (how, wanted_min) {
        (Minimized, _) => "ok, demarrage reduit demande",
        (Normal, true) => "ok, lancement normal (repli : demarrage reduit impossible)",
        (Normal, false) => "ok",
        (Failed, _) => "echec du lancement",
    }
}

fn run_gui(ctx: &Ctx, args: &[String]) -> Res<()> {
    // Une seule interface a la fois : la 2e demande simplement a la 1ere de s'afficher.
    let lock = match InstanceLock::acquire_named(&ctx.data_dir, "gui.lock", &watch::pid_alive) {
        Ok(l) => l,
        Err(_) => {
            // Redemarrage apres une mise a jour alors qu'une instance tourne deja : le programme est sain, le veilleur est rassure.
            if has(args, "--after-update") {
                agss::selfupdate::confirm_marker_only(&ctx.data_dir, agss::update::current_version());
            }
            // Lancement automatique (--tray) en double : on quitte sans rien afficher.
            if !has(args, "--tray") {
                agss::util::write_atomic(&ctx.data_dir.join("gui.show"), b"show")?;
            }
            return Ok(());
        }
    };
    {
        let lg = Logger::new(ctx.data_dir.join("logs").join("watch.log"), false);
        agss::timesync::spawn_sync(ctx.data_dir.clone(), move |m| lg.log(m));
    }
    let opts = agss::gui::LaunchOpts { start_hidden: has(args, "--tray"), auto_quit_secs: flag(args, "--auto-quit-secs").and_then(|v| v.parse().ok()) };
    agss::gui::run(ctx.data_dir.clone(), lock, opts).map_err(|e| e.into())
}

fn main() {
    // Mode GUI (sans argument, `gui`, `--tray`) : pas de console. Mode CLI : on rattache le terminal parent.
    let first = std::env::args().nth(1);
    let gui_mode = matches!(first.as_deref(), None | Some("gui") | Some("--tray"));
    if !gui_mode {
        agss::winutil::attach_parent_console();
    }
    if let Err(e) = run() {
        eprintln!("ERREUR : {}", e);
        std::process::exit(1);
    }
}
