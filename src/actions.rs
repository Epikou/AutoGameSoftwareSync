//! Actions appelees par l'interface (et testables sans fenetre). Elles reutilisent EXACTEMENT
//! le code du CLI (backup::*), donc toutes les regles anti-perte s'appliquent.
use crate::backup::{self, BackupOutcome};
use crate::config::{Config, Game};
use crate::catalog::{self, Entry, StoreCtx};
use crate::settings::Settings;
use crate::{util, Ctx, Res};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn do_backup(ctx: &Ctx, game: &Game, dest: &Path) -> Res<String> {
    Ok(match backup::backup(ctx, game, dest)? {
        BackupOutcome::UpToDate => "UpToDate".into(),
        BackupOutcome::Backed { dir } => format!("Backed {}", dir.display()),
        BackupOutcome::Conflict { dir } => format!("Conflict {}", dir.display()),
    })
}

/// Restaure la version active (`version` = None) ou une version precise.
pub fn do_restore(ctx: &Ctx, game: &Game, dest: &Path, version: Option<&str>) -> Res<String> {
    let r = match version {
        None => backup::restore(ctx, game, dest)?,
        Some(v) => backup::restore_version(ctx, game, dest, v)?,
    };
    Ok(format!("{} fichiers ; safety: {}", r.files_restored, r.safety_dir.map(|p| p.display().to_string()).unwrap_or_else(|| "-".into())))
}

/// Versions qu'un elagage supprimerait (sans rien supprimer).
pub fn prune_preview(ctx: &Ctx, game: &Game, dest: &Path) -> Vec<PathBuf> {
    backup::prune_plan(dest, game, backup::keep_for(ctx, &game.name))
}

pub fn do_prune(ctx: &Ctx, game: &Game, dest: &Path) -> Vec<PathBuf> {
    backup::prune_keep(dest, game, backup::keep_for(ctx, &game.name))
}

/// Retire un jeu de la liste. Ne supprime AUCUN fichier de sauvegarde (ni local ni cloud).
pub fn remove_game(ctx: &Ctx, name: &str) -> Res<()> {
    let mut cfg = Config::load(ctx)?;
    let before = cfg.games.len();
    cfg.games.retain(|g| !g.name.eq_ignore_ascii_case(name));
    if cfg.games.len() == before {
        return Err(format!("element '{}' inconnu", name).into());
    }
    cfg.save(ctx)
}

pub fn add_game(ctx: &Ctx, mut game: Game) -> Res<()> {
    game.name = game.name.trim().to_string();
    if game.name.trim().is_empty() {
        return Err("nom vide".into());
    }
    if !crate::store::Target::of(&game).exists() {
        return Err(format!("{} introuvable : {}", if game.save_is_file { "fichier" } else { "dossier" }, game.path.display()).into());
    }
    let mut cfg = Config::load(ctx)?;
    if cfg.find(&game.name).is_some() || config_has_key(&cfg, &game.name) {
        return Err(format!("l'element '{}' existe deja (meme nom une fois normalise)", game.name).into());
    }
    cfg.games.push(game);
    cfg.save(ctx)
}

// ------------------------------------------------------------------ taille

pub fn dir_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir).follow_links(false).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
}

pub fn game_size(dest: &Path, game: &Game) -> u64 {
    crate::cloud::roots_for(dest, game).iter().map(|r| dir_size(r)).sum()
}

pub fn format_bytes(b: u64) -> String {
    const U: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{} {}", b, U[0]) } else { format!("{:.1} {}", v, U[i]) }
}

// ------------------------------------------------------------------ import / export

#[derive(Serialize, Deserialize)]
pub struct Bundle {
    pub format: u32,
    pub config: Config,
    pub settings: Settings,
}

pub fn export_config(ctx: &Ctx, path: &Path) -> Res<()> {
    let b = Bundle { format: 1, config: Config::load(ctx)?, settings: Settings::load(ctx)? };
    util::write_atomic(path, serde_json::to_string_pretty(&b)?.as_bytes())
}

/// Lit et valide un fichier de configuration SANS rien modifier.
pub fn read_bundle(path: &Path) -> Res<Bundle> {
    let mut b: Bundle = serde_json::from_str(&std::fs::read_to_string(path)?).map_err(|e| format!("fichier de configuration invalide : {}", e))?;
    if b.format != 1 {
        return Err(format!("format de configuration inconnu : {}", b.format).into());
    }
    let mut seen = std::collections::HashSet::new();
    for g in &b.config.games {
        if g.name.trim().is_empty() || !seen.insert(g.name.to_lowercase()) {
            return Err(format!("entree invalide ou en double dans le fichier : '{}'", g.name).into());
        }
    }
    b.settings.sanitize();
    Ok(b)
}

/// Importe une configuration. L'ancienne config.json / settings.json sont COPIEES dans
/// `imports-backup/<date>/` avant d'etre remplacees. Retourne ce dossier de secours.
pub fn import_config(ctx: &Ctx, path: &Path) -> Res<PathBuf> {
    let b = read_bundle(path)?;
    let bak = ctx.data_dir.join("imports-backup").join(util::timestamp(util::now_ms()));
    std::fs::create_dir_all(&bak)?;
    for f in ["config.json", "settings.json"] {
        let src = ctx.data_dir.join(f);
        if src.exists() {
            std::fs::copy(&src, bak.join(f))?;
        }
    }
    b.config.save(ctx)?;
    b.settings.save(ctx)?;
    Ok(bak)
}

// ------------------------------------------------------------------ chemins du catalog

/// Dossiers de sauvegarde EXISTANTS d'une entree du catalog (+ nombre de fichiers, plafonne a 5000).
/// Les fichiers sont ramenes a leur dossier parent ; les dossiers trop generiques sont ecartes.
pub fn existing_save_dirs(entry: &Entry, mname: &str, sc: &StoreCtx) -> (Vec<(PathBuf, usize)>, Vec<String>) {
    let r = catalog::resolve_with(entry, mname, sc);
    let mut out: Vec<(PathBuf, usize)> = Vec::new();
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
            if catalog::is_generic_root(&d) || out.iter().any(|(x, _)| x == &d) {
                continue;
            }
            let n = walkdir::WalkDir::new(&d).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).take(5000).count();
            out.push((d, n));
        }
    }
    // Linux : rien dans un prefixe Proton -> version native du jeu, dossiers devines par son nom (XDG, Unity, Godot...).
    #[cfg(not(windows))]
    if out.is_empty() {
        let home = dirs::home_dir().unwrap_or_default();
        let x = crate::proton::XdgDirs::from_env(&home, std::env::var("XDG_DATA_HOME").ok().as_deref(), std::env::var("XDG_CONFIG_HOME").ok().as_deref(), dirs::document_dir().as_deref());
        let mut names: Vec<String> = vec![mname.to_string()];
        names.extend(entry.install_dir.keys().cloned());
        if let Some(d) = sc.install_dir.as_ref().and_then(|d| d.file_name()) {
            names.push(d.to_string_lossy().to_string());
        }
        for d in crate::proton::native_save_guesses(&names, &x) {
            let n = walkdir::WalkDir::new(&d).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).take(5000).count();
            out.push((d, n));
        }
    }
    (out, r.unresolved)
}

// ------------------------------------------------------------------ noms de jeux entre PC

/// Ajout d'un jeu : le nom est-il deja pris (meme cle normalisee) dans la configuration locale ?
pub fn config_has_key(cfg: &Config, name: &str) -> bool {
    let k = crate::cloud::game_key(name);
    cfg.games.iter().any(|g| crate::cloud::game_key(&g.name) == k)
}

/// « Lier a un jeu existant » : enregistre un alias LOCAL (config.json) pour que ce PC lise/ecrive dans
/// le dossier `key` de la destination. Ne supprime, ne renomme ni ne deplace RIEN dans la destination.
pub fn link_game(ctx: &Ctx, name: &str, key: &str) -> Res<()> {
    let k = crate::cloud::game_key(key);
    if key.trim().is_empty() {
        return Err("cle d'element vide".into());
    }
    let mut cfg = Config::load(ctx)?;
    let g = cfg.games.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("element '{}' inconnu", name))?;
    g.dest_key = if k == crate::cloud::game_key(&g.name) { None } else { Some(k) };
    cfg.save(ctx)
}

/// Resolution explicite d'un vrai conflit (voir backup::resolve_conflict).
pub fn do_resolve(ctx: &Ctx, game: &Game, dest: &Path, keep_mine: bool) -> Res<String> {
    let r = backup::resolve_conflict(ctx, game, dest, keep_mine)?;
    Ok(format!("conflit resolu : {}", r.merged_dir.display()))
}

// ------------------------------------------------------------------ regles de demarrage

use crate::startup::{self, ApplyFile, ApplyPlan, Rule, Scheduler, ToolProbe};

/// Regles courantes (AutoGameSoftwareSync + outils detectes + outils personnalises) avec les reglages enregistres.
pub fn desired_rules(settings: &Settings, probe: &dyn ToolProbe, self_exe: &Path) -> Vec<Rule> {
    let tools = startup::detect_tools(probe);
    startup::build_rules(&tools, &settings.startup_rules, self_exe, &|p: &Path| p.is_file())
}

/// Applique `plan` ; si le planificateur refuse (droits), `elevate` est appele UNE SEULE fois avec toutes les
/// operations refusees (une seule fenetre UAC) ; son echec / annulation laisse tout en l'etat.
/// Retourne le nombre d'operations effectuees.
pub fn apply_plan_elevating(sched: &dyn Scheduler, plan: &ApplyPlan, user: &str, elevate: &dyn Fn(&ApplyFile) -> std::result::Result<(), String>) -> Res<usize> {
    let out = startup::apply_plan(sched, plan, user);
    let mut done = out.created.len() + out.deleted.len();
    if out.needs_elevation() {
        let denied: Vec<&String> = out.failed.iter().filter(|(_, e)| matches!(e, startup::SchedError::AccessDenied(_))).map(|(n, _)| n).collect();
        let sub = ApplyPlan {
            create: plan.create.iter().filter(|r| denied.contains(&&startup::task_name(&r.id))).cloned().collect(),
            delete: plan.delete.iter().filter(|n| denied.contains(n)).cloned().collect(),
        };
        elevate(&startup::plan_to_file(&sub, user)).map_err(|e| format!("{} (aucune regle modifiee par l'etape avec droits eleves)", e))?;
        done += sub.create.len() + sub.delete.len();
    } else if let Some((n, e)) = out.failed.first() {
        return Err(format!("echec pour {} : {}", n, e).into());
    }
    Ok(done)
}

/// Applique les regles (voir `apply_plan_elevating`).
pub fn apply_startup_with(sched: &dyn Scheduler, rules: &[Rule], user: &str, elevate: &dyn Fn(&ApplyFile) -> std::result::Result<(), String>) -> Res<String> {
    let existing = sched.list().map_err(|e| format!("planificateur de taches indisponible : {}", e))?;
    let plan = startup::plan_apply(&existing, rules);
    let done = apply_plan_elevating(sched, &plan, user, elevate)?;
    Ok(format!("{} regle(s) appliquee(s)", done))
}

/// Resultat d'une migration des taches de la version precedente.
#[derive(Debug, PartialEq, Eq)]
pub enum TaskMigration {
    /// Aucune ancienne tache a nous.
    NothingToDo,
    /// `n` taches migrees (nouvelles creees, anciennes supprimees), sans elevation.
    Done(usize),
    /// Une etape demande des droits d'administrateur : a confirmer depuis les Reglages (`n` taches concernees).
    NeedsElevation(usize),
    Failed(String),
}

/// Migration des taches de `\AutoGameSoftwareSync\` vers `\AutoGameSoftwareSync\`, SANS elevation : cree les nouvelles taches,
/// puis supprime SEULEMENT les anciennes dont la nouvelle existe. Les taches inconnues du dossier ne sont jamais touchees.
pub fn migrate_tasks_auto(sched: &dyn Scheduler, rules: &[Rule], user: &str) -> TaskMigration {
    let legacy = match sched.list_legacy() {
        Ok(l) => l,
        Err(e) => return TaskMigration::Failed(e.to_string()),
    };
    let plan = startup::plan_migrate(&legacy, rules);
    if plan.create.is_empty() {
        return TaskMigration::NothingToDo;
    }
    let n = plan.create.len();
    let created = startup::apply_plan(sched, &ApplyPlan { create: plan.create.clone(), delete: vec![] }, user);
    if let Some((_, e)) = created.failed.first() {
        return if created.needs_elevation() { TaskMigration::NeedsElevation(n) } else { TaskMigration::Failed(e.to_string()) };
    }
    // Les nouvelles taches existent : on peut retirer les anciennes correspondantes.
    let deleted = startup::apply_plan(sched, &ApplyPlan { create: vec![], delete: plan.delete.clone() }, user);
    match deleted.failed.first() {
        None => TaskMigration::Done(n),
        Some((_, e)) if deleted.needs_elevation() => {
            let _ = e;
            TaskMigration::NeedsElevation(n)
        }
        Some((_, e)) => TaskMigration::Failed(e.to_string()),
    }
}

/// Migration AVEC elevation controlee (une seule fenetre UAC), a la demande de l'utilisateur.
pub fn migrate_tasks_with(sched: &dyn Scheduler, rules: &[Rule], user: &str, elevate: &dyn Fn(&ApplyFile) -> std::result::Result<(), String>) -> Res<String> {
    let legacy = sched.list_legacy().map_err(|e| format!("planificateur de taches indisponible : {}", e))?;
    let plan = startup::plan_migrate(&legacy, rules);
    if plan.create.is_empty() {
        return Ok("aucune ancienne tache a migrer".into());
    }
    // Creations d'abord (sans elevation) ; les suppressions suivent seulement si toutes les creations ont reussi.
    let created = startup::apply_plan(sched, &ApplyPlan { create: plan.create.clone(), delete: vec![] }, user);
    if created.failed.is_empty() {
        let m = apply_plan_elevating(sched, &ApplyPlan { create: vec![], delete: plan.delete.clone() }, user, elevate)?;
        return Ok(format!("{} tache(s) migree(s) ({} ancienne(s) supprimee(s))", plan.create.len(), m));
    }
    if created.needs_elevation() {
        // Une seule fenetre UAC : creations puis suppressions dans l'etape avec droits eleves.
        elevate(&startup::plan_to_file(&plan, user)).map_err(|e| format!("{} (aucune tache modifiee par l'etape avec droits eleves)", e))?;
        return Ok(format!("{} tache(s) migree(s)", plan.create.len()));
    }
    Err(format!("echec : {}", created.failed[0].1).into())
}

/// Regles a recreer pour les anciennes taches : memes regles que d'habitude, mais activees (la tache existait = regle active).
pub fn rules_for_legacy(settings: &mut Settings, legacy: &[String], probe: &dyn ToolProbe, self_exe: &Path) -> Vec<Rule> {
    for n in legacy.iter().filter(|n| startup::is_legacy_task(n)) {
        let id = crate::legacy::task_rule_id(n.trim_start_matches('\\').rsplit('\\').next().unwrap_or(""));
        match settings.startup_rules.iter_mut().find(|c| c.id == id) {
            Some(c) => c.enabled = true,
            None => {
                let kind = startup::ToolKind::from_id(&id);
                settings.startup_rules.push(crate::settings::StartupRuleCfg { id, name: String::new(), exe: None, delay_secs: startup::default_delay(kind), enabled: true, minimized: true });
            }
        }
    }
    desired_rules(settings, probe, self_exe)
}

/// Migration automatique au premier lancement (sans elevation). Les reglages ne sont ecrits que si une migration a eu lieu.
pub fn auto_migrate_tasks(ctx: &Ctx) -> Res<TaskMigration> {
    let sched = startup::SchtasksScheduler { tmp_dir: ctx.data_dir.join("tmp") };
    let legacy = sched.list_legacy().map_err(|e| e.to_string())?;
    if legacy.is_empty() {
        return Ok(TaskMigration::NothingToDo);
    }
    let mut settings = Settings::load(ctx)?;
    let exe = std::env::current_exe()?;
    let rules = rules_for_legacy(&mut settings, &legacy, &startup::RealProbe, &exe);
    let r = migrate_tasks_auto(&sched, &rules, &startup::current_user());
    if matches!(r, TaskMigration::Done(_) | TaskMigration::NeedsElevation(_)) {
        settings.save(ctx)?;
    }
    Ok(r)
}

/// Migration demandee depuis les Reglages (avec relance eleve controlee).
pub fn do_migrate_tasks(ctx: &Ctx) -> Res<String> {
    let sched = startup::SchtasksScheduler { tmp_dir: ctx.data_dir.join("tmp") };
    let legacy = sched.list_legacy().map_err(|e| e.to_string())?;
    let mut settings = Settings::load(ctx)?;
    let exe = std::env::current_exe()?;
    let rules = rules_for_legacy(&mut settings, &legacy, &startup::RealProbe, &exe);
    settings.save(ctx)?;
    let tmp = ctx.data_dir.join("tmp");
    migrate_tasks_with(&sched, &rules, &startup::current_user(), &|f: &ApplyFile| elevated_apply(&tmp, f))
}

/// Execute un fichier d'application via une relance eleve (UAC) de ce programme.
fn elevated_apply(tmp: &Path, f: &ApplyFile) -> std::result::Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
    let file = tmp.join(format!("apply-{}.json", std::process::id()));
    let result = PathBuf::from(format!("{}.result", file.display()));
    std::fs::write(&file, serde_json::to_string(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let r = crate::winutil::run_elevated_and_wait(&format!("--apply-startup-rules \"{}\"", file.display()), 90_000);
    let msg = std::fs::read_to_string(&result).unwrap_or_default();
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_file(&result);
    match r {
        crate::winutil::ElevateResult::Cancelled => Err("operation annulee dans la fenetre d'autorisation Windows".into()),
        crate::winutil::ElevateResult::Failed(c) => Err(format!("autorisation Windows impossible (code {})", c)),
        crate::winutil::ElevateResult::Finished(0) => Ok(()),
        crate::winutil::ElevateResult::Finished(_) => Err(format!("echec cote droits eleves : {}", msg.trim())),
    }
}

/// Application reelle (schtasks) avec relance eleve controlee (`--apply-startup-rules`).
pub fn do_apply_startup(ctx: &Ctx) -> Res<String> {
    let settings = Settings::load(ctx)?;
    let exe = std::env::current_exe()?;
    let rules = desired_rules(&settings, &startup::RealProbe, &exe);
    let sched = startup::SchtasksScheduler { tmp_dir: ctx.data_dir.join("tmp") };
    let tmp = ctx.data_dir.join("tmp");
    apply_startup_with(&sched, &rules, &startup::current_user(), &|f: &ApplyFile| elevated_apply(&tmp, f))
}

/// Migre l'ancienne entree Run (`AutoGameSoftwareSync` / `AutoGameSoftwareSync`) vers la regle de demarrage de AutoGameSoftwareSync :
/// active la regle, l'applique, et SEULEMENT SI elle est creee supprime l'entree Run.
pub fn do_migrate_run(ctx: &Ctx) -> Res<String> {
    let mut s = Settings::load(ctx)?;
    match s.startup_rules.iter_mut().find(|r| r.id == startup::ID_APP) {
        Some(r) => r.enabled = true,
        None => s.startup_rules.push(crate::settings::StartupRuleCfg { id: startup::ID_APP.into(), name: String::new(), exe: None, delay_secs: startup::DELAY_APP_SECS, enabled: true, minimized: true }),
    }
    s.save(ctx)?;
    let msg = do_apply_startup(ctx)?;
    let exe = std::env::current_exe()?;
    let rules = desired_rules(&s, &startup::RealProbe, &exe);
    let sched = startup::SchtasksScheduler { tmp_dir: ctx.data_dir.join("tmp") };
    let existing = sched.list().unwrap_or_default();
    if rules.iter().any(|r| r.id == startup::ID_APP && startup::rule_status(r, &existing) == startup::RuleStatus::Active) {
        for n in crate::legacy::RUN_VALUES.iter().copied().chain([crate::winutil::RUN_VALUE]) {
            if crate::winutil::run_entry(n).is_some() {
                crate::winutil::remove_run_entry(n);
            }
        }
        Ok(format!("{} ; ancienne entree de demarrage retiree", msg))
    } else {
        Err("la regle n'a pas pu etre creee : l'ancienne entree de demarrage est conservee".into())
    }
}

/// Met a jour les emplacements d'un jeu deplace (exe et/ou dossier de sauvegardes). Rien n'est deplace ni supprime :
/// seul le suivi change (config.json + chemin de l'exe pour l'icone). La cle du dossier partage reste celle du jeu,
/// donc l'historique deja present dans le dossier partage reste associe au meme jeu.
/// Nouvelle cible de sauvegarde d'un jeu : dossier, dossier avec filtre de noms, ou fichier unique.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SaveTarget {
    pub path: PathBuf,
    pub is_file: bool,
    pub filter: Option<String>,
}

impl SaveTarget {
    pub fn exists(&self) -> bool {
        if self.is_file {
            self.path.is_file()
        } else {
            self.path.is_dir()
        }
    }
}

pub fn relocate_game(ctx: &Ctx, name: &str, exe: Option<&Path>, save: Option<&SaveTarget>) -> Res<String> {
    if let Some(s) = save {
        if !s.exists() {
            return Err(format!("{} introuvable : {}", if s.is_file { "fichier" } else { "dossier" }, s.path.display()).into());
        }
    }
    if let Some(e) = exe {
        if !e.is_file() {
            return Err(format!("fichier introuvable : {}", e.display()).into());
        }
    }
    let mut cfg = Config::load(ctx)?;
    let g = cfg.games.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("element '{}' inconnu", name))?;
    if let Some(s) = save {
        g.path = s.path.clone();
        g.save_is_file = s.is_file;
        g.save_filter = s.filter.as_deref().map(str::trim).filter(|f| !f.is_empty() && !s.is_file).map(|f| f.to_string());
    }
    if let Some(e) = exe {
        g.exe = e.file_name().map(|n| n.to_string_lossy().to_string());
    }
    cfg.save(ctx)?;
    if let Some(e) = exe {
        let mut st = Settings::load(ctx)?;
        st.game_mut(name).exe_path = Some(e.to_path_buf());
        st.save(ctx)?;
    }
    Ok("emplacements mis a jour (aucun fichier deplace ni supprime)".into())
}

/// Nom AFFICHE seulement : ne touche ni a la cle du dossier partage, ni a l'exe, ni aux chemins. `None` = nom d'origine.
pub fn set_display_name(ctx: &Ctx, name: &str, display: Option<&str>) -> Res<String> {
    let mut cfg = Config::load(ctx)?;
    let g = cfg.games.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("element '{}' inconnu", name))?;
    g.display_name = display.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != &g.name);
    cfg.save(ctx)?;
    Ok(if cfg!(windows) { "nom affiche mis a jour (cle, exe et chemins inchanges)" } else { "nom affiche mis a jour (cle, programme et chemins inchanges)" }.into())
}

/// Definit (ou retire) la planification d'un jeu. Refuse une planification invalide.
pub fn set_schedule(ctx: &Ctx, name: &str, schedule: Option<crate::schedule::Schedule>) -> Res<String> {
    if let Some(s) = &schedule {
        s.validate().map_err(|e| format!("planification invalide : {:?}", e))?;
    }
    let mut cfg = Config::load(ctx)?;
    let g = cfg.games.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("element '{}' inconnu", name))?;
    g.schedule = schedule.map(|mut s| {
        s.last_run_utc = g.schedule.as_ref().and_then(|o| o.last_run_utc);
        s
    });
    cfg.save(ctx)?;
    Ok("planification mise a jour".into())
}

fn norm_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn is_under(child: &str, parent: &str) -> bool {
    child == parent || child.strip_prefix(parent).map(|r| r.starts_with('/')).unwrap_or(false)
}

/// Deux ensembles de fichiers (cibles) se recouvrent-ils ? Conservateur : un doute = recouvrement.
fn loc_overlap(a: &Game, b: &Game) -> bool {
    let (ta, tb) = (crate::store::Target::of(a), crate::store::Target::of(b));
    let (ra, rb) = (norm_path(&ta.root), norm_path(&tb.root));
    match (&ta.only_file, &tb.only_file) {
        (Some(fa), Some(fb)) => ra == rb && fa.to_lowercase() == fb.to_lowercase(),
        (Some(f), None) | (None, Some(f)) => {
            // Fichier unique contre dossier : le fichier est-il dans le dossier et accepte par son filtre ?
            let (file_root, dir_t, dir_root) = if ta.only_file.is_some() { (&ra, &tb, &rb) } else { (&rb, &ta, &ra) };
            if !is_under(file_root, dir_root) {
                return false;
            }
            let rel = format!("{}{}", file_root.strip_prefix(dir_root.as_str()).unwrap_or("").trim_start_matches('/'), if file_root == dir_root { "" } else { "/" });
            dir_t.accepts(&format!("{}{}", rel, f))
        }
        (None, None) => {
            if !(is_under(&ra, &rb) || is_under(&rb, &ra)) {
                return false;
            }
            if ta.filter.is_empty() || tb.filter.is_empty() {
                return true;
            }
            ta.filter.iter().any(|p| tb.filter.iter().any(|q| p == q || crate::store::glob_match(p, q) || crate::store::glob_match(q, p)))
        }
    }
}

/// Deux jeux se recouvrent-ils ? (un seul de leurs emplacements actifs suffit)
pub fn targets_overlap(a: &Game, b: &Game) -> bool {
    let (la, lb) = (a.enabled_locations(), b.enabled_locations());
    la.iter().any(|x| lb.iter().any(|y| loc_overlap(&a.pseudo_for(x), &b.pseudo_for(y))))
}

/// Deux emplacements actifs d'un MEME jeu se recouvrent-ils ?
pub fn own_locations_overlap(g: &Game) -> bool {
    let l = g.enabled_locations();
    (0..l.len()).any(|i| (i + 1..l.len()).any(|j| loc_overlap(&g.pseudo_for(&l[i]), &g.pseudo_for(&l[j]))))
}

/// Noms (d'origine) des jeux deja configures dont l'emplacement recouvre celui de `g` (hors `g` lui-meme).
pub fn overlapping_games(cfg: &Config, g: &Game) -> Vec<String> {
    cfg.games.iter().filter(|o| !o.name.eq_ignore_ascii_case(&g.name) && targets_overlap(o, g)).map(|o| o.shown().to_string()).collect()
}

/// Modification des emplacements d'un jeu (hors exe) : nom et etat de l'emplacement principal, liste des supplementaires.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LocEdit {
    pub primary_label: Option<String>,
    pub primary_enabled: bool,
    pub extras: Vec<crate::config::Location>,
}

/// Verifie une liste d'emplacements supplementaires : id et chemin renseignes, ids uniques.
pub fn validate_extras(extras: &[crate::config::Location]) -> Res<()> {
    let mut seen: Vec<&str> = Vec::new();
    for l in extras {
        if l.id.is_empty() || l.id == crate::config::MAIN_LOCATION || seen.contains(&l.id.as_str()) {
            return Err("identifiant d'emplacement invalide ou en double".into());
        }
        if l.path.as_os_str().is_empty() {
            return Err(format!("chemin vide pour l'emplacement « {} »", l.label).into());
        }
        seen.push(&l.id);
    }
    Ok(())
}

/// Enregistre la liste d'emplacements (rien n'est deplace ni supprime : seul le suivi change).
pub fn set_locations(ctx: &Ctx, name: &str, e: &LocEdit) -> Res<String> {
    validate_extras(&e.extras)?;
    let mut cfg = Config::load(ctx)?;
    let g = cfg.games.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("element '{}' inconnu", name))?;
    g.primary_label = e.primary_label.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|s| s.to_string());
    g.primary_disabled = !e.primary_enabled;
    g.extra_locations = e.extras.clone();
    cfg.save(ctx)?;
    Ok("emplacements mis a jour (aucun fichier deplace ni supprime)".into())
}

/// Ajoute un emplacement supplementaire (fiche 2.0 : « Ajouter un emplacement »). Le dossier ou fichier doit exister ;
/// identifiant genere et unique ; rien n'est copie ni deplace.
pub fn add_location(ctx: &Ctx, name: &str, path: &Path, is_file: bool, label: &str) -> Res<crate::config::Location> {
    let ok = if is_file { path.is_file() } else { path.is_dir() };
    if !ok {
        return Err(format!("{} introuvable : {}", if is_file { "fichier" } else { "dossier" }, path.display()).into());
    }
    let mut cfg = Config::load(ctx)?;
    let g = cfg.games.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("element '{}' inconnu", name))?;
    if g.all_locations().iter().any(|l| l.path == path) {
        return Err("cet emplacement est deja protege pour cet element".into());
    }
    let label = if label.trim().is_empty() { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "emplacement".into()) } else { label.trim().to_string() };
    let mut id = crate::config::Location::new_id(&label);
    let mut n = 2;
    while id == crate::config::MAIN_LOCATION || g.extra_locations.iter().any(|l| l.id == id) {
        id = crate::config::Location::new_id(&format!("{} {}", label, n));
        n += 1;
    }
    let loc = crate::config::Location { id, label, path: path.to_path_buf(), is_file, filter: None, exclude: None, enabled: true };
    g.extra_locations.push(loc.clone());
    validate_extras(&g.extra_locations)?;
    cfg.save(ctx)?;
    Ok(loc)
}

/// Retire un emplacement supplementaire de la liste (ses fichiers locaux ne sont pas touches).
pub fn remove_location(ctx: &Ctx, name: &str, id: &str) -> Res<String> {
    let mut cfg = Config::load(ctx)?;
    let g = cfg.games.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("element '{}' inconnu", name))?;
    let before = g.extra_locations.len();
    g.extra_locations.retain(|l| l.id != id);
    if g.extra_locations.len() == before {
        return Err("emplacement inconnu".into());
    }
    cfg.save(ctx)?;
    Ok("emplacement retire de la liste (aucun fichier supprime)".into())
}
