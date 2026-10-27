//! View-model : etat et logique de l'interface, SANS aucune dependance a egui (testable sans fenetre).
use crate::actions;
use crate::backup::{self, Plan};
use crate::config::{Config, Game};
use crate::i18n::{tr, Lang, K};
use crate::settings::{Settings, ThemeMode};
use crate::{Ctx, Res};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ navigation

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Games,
    Add,
    Settings,
}

impl Page {
    pub const ALL: [Page; 3] = [Page::Games, Page::Add, Page::Settings];
    fn idx(self) -> usize {
        Page::ALL.iter().position(|p| *p == self).unwrap()
    }
    /// Page suivante (gachette droite), circulaire.
    pub fn next(self) -> Page {
        Page::ALL[(self.idx() + 1) % 3]
    }
    pub fn prev(self) -> Page {
        Page::ALL[(self.idx() + 2) % 3]
    }
    pub fn label(self) -> K {
        match self {
            Page::Games => K::NavGames,
            Page::Add => K::NavAdd,
            Page::Settings => K::NavSettings,
        }
    }
}

// ------------------------------------------------------------------ etat d'un jeu

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum GameStatus {
    Never,
    UpToDate,
    Running,
    LocalChanged,
    CloudNewer,
    Conflict,
    NoDest,
    Error(String),
}

impl GameStatus {
    pub fn label(&self) -> K {
        match self {
            GameStatus::Never => K::StatusNever,
            GameStatus::UpToDate => K::StatusUpToDate,
            GameStatus::Running => K::StatusRunning,
            GameStatus::LocalChanged => K::StatusLocalChanged,
            GameStatus::CloudNewer => K::StatusCloudNewer,
            GameStatus::Conflict => K::StatusConflict,
            GameStatus::NoDest => K::StatusNoDest,
            GameStatus::Error(_) => K::StatusError,
        }
    }
    /// Un jeu en cours ne peut ni etre restaure ni (par prudence) etre sauvegarde depuis l'UI.
    pub fn blocks_actions(&self) -> bool {
        matches!(self, GameStatus::Running)
    }
}

/// Calcule l'etat d'un jeu (lecture seule : ne modifie rien).
pub fn compute_status(ctx: &Ctx, game: &Game, dest: Option<&Path>) -> GameStatus {
    if let Some(exe) = game.exe.as_deref().filter(|e| !e.trim().is_empty()) {
        match ctx.procs.is_running(exe) {
            Ok(true) => return GameStatus::Running,
            Ok(false) => {}
            Err(e) => return GameStatus::Error(e.to_string()),
        }
    }
    let Some(dest) = dest else { return GameStatus::NoDest };
    let cloud = !crate::cloud::scan_dest(dest, game).versions.is_empty();
    match backup::plan(ctx, game, dest) {
        Err(e) => GameStatus::Error(e.to_string()),
        Ok(Plan::Nothing) => GameStatus::Never,
        Ok(Plan::InSync) => GameStatus::UpToDate,
        Ok(Plan::Backup) => {
            if cloud { GameStatus::LocalChanged } else { GameStatus::Never }
        }
        Ok(Plan::Restore) => GameStatus::CloudNewer,
        Ok(Plan::Conflict) => GameStatus::Conflict,
    }
}

// ------------------------------------------------------------------ confirmations

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Confirm {
    Backup { game: String },
    Restore { game: String, version: Option<String> },
    Prune { game: String, count: usize },
    /// Resolution explicite d'un vrai conflit : garder la version locale, ou prendre celle du cloud.
    Resolve { game: String, keep_mine: bool },
    /// « Restaurer l'apparence par defaut » (theme, accent, langue).
    RestoreAppearance,
    /// Appliquer les regles de demarrage (taches planifiees).
    ApplyStartup,
    /// Remplacer l'ancienne entree Run par la regle de demarrage de AutoGameSoftwareSync.
    MigrateRun,
    /// Migration des taches planifiees de la version precedente (avec autorisation si necessaire).
    MigrateTasks,
    /// Lier ce jeu au dossier existant `key` de la destination (alias local).
    Link { game: String, key: String, name: String },
    Import { path: PathBuf },
    Remove { game: String },
    /// Nom affiche seulement (None = nom d origine).
    Rename { game: String, display: Option<String> },
    /// Retrait d un emplacement supplementaire (confirmation).
    RemoveLocation { game: String, id: String, label: String },
    /// Jeu deplace : nouveaux emplacements de l exe et/ou du dossier de sauvegardes (rien n est deplace).
    Relocate { game: String, exe: Option<PathBuf>, save: Option<crate::actions::SaveTarget>, save_changed: bool, overlap: Option<String>, locs: Option<crate::actions::LocEdit> },
}

impl Confirm {
    /// Jeu concerne (nom d origine), s il y en a un.
    pub fn game_name(&self) -> Option<&str> {
        match self {
            Confirm::Backup { game } | Confirm::Restore { game, .. } | Confirm::Prune { game, .. } | Confirm::Resolve { game, .. } | Confirm::Link { game, .. } | Confirm::Remove { game } | Confirm::Relocate { game, .. } | Confirm::Rename { game, .. } | Confirm::RemoveLocation { game, .. }  => Some(game),
            _ => None,
        }
    }

    /// Texte du dialogue, en langage simple.
    pub fn message(&self, lang: Lang) -> String {
        match self {
            Confirm::Backup { game } => tr(lang, K::ConfBackup).replace("{}", game),
            Confirm::Restore { game, version: None } => tr(lang, K::ConfRestore).replace("{}", game),
            Confirm::Restore { game, version: Some(v) } => tr(lang, K::ConfRestoreVersion).replace("{0}", game).replace("{1}", &crate::timefmt::version_label(v, lang, &crate::timefmt::WinTz)),
            Confirm::Prune { game, count } => tr(lang, K::ConfPrune).replace("{0}", &count.to_string()).replace("{1}", game),
            Confirm::Import { path } => tr(lang, K::ConfImport).replace("{}", &path.display().to_string()),
            Confirm::Remove { game } => tr(lang, K::ConfRemove).replace("{}", game),
            Confirm::Rename { game, display: Some(d) } => tr(lang, K::ConfRename).replace("{0}", d).replace("{1}", game),
            Confirm::Rename { game, display: None } => tr(lang, K::ConfRenameReset).replace("{}", game),
            Confirm::RemoveLocation { game, label, .. } => tr(lang, K::ConfRemoveLoc).replace("{0}", label).replace("{1}", game),
            Confirm::Relocate { game, save_changed, overlap, .. } => {
                let mut m = tr(lang, K::ConfRelocate).replace("{}", game);
                if let Some(o) = overlap {
                    m.push_str("

");
                    m.push_str(&tr(lang, K::ConfOverlap).replace("{}", o));
                }
                if *save_changed {
                    m.push_str("

");
                    m.push_str(tr(lang, K::ConfRelocateSave));
                }
                m
            }
            Confirm::Resolve { game, keep_mine: true } => tr(lang, K::ConfResolveMine).replace("{}", game),
            Confirm::Resolve { game, keep_mine: false } => tr(lang, K::ConfResolveCloud).replace("{}", game),
            Confirm::RestoreAppearance => tr(lang, K::ConfRestoreAppearance).to_string(),
            Confirm::ApplyStartup => tr(lang, K::ConfApplyStartup).to_string(),
            Confirm::MigrateRun => tr(lang, K::ConfMigrateRun).to_string(),
            Confirm::MigrateTasks => tr(lang, K::ConfMigrateTasks).to_string(),
            Confirm::Link { game, name, .. } => tr(lang, K::ConfLink).replace("{0}", game).replace("{1}", name),
        }
    }
}

// ------------------------------------------------------------------ journal

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub ts: String,
    pub level: Level,
    pub game: Option<String>,
    pub text: String,
}

/// "AAAAMMJJ-HHMMSS-mmm [Jeu] message" ou "AAAAMMJJ-HHMMSS-mmm message".
pub fn parse_log_line(line: &str) -> Option<LogEntry> {
    let (ts, rest) = line.split_once(' ')?;
    if ts.len() != 19 {
        return None;
    }
    let (game, text) = match rest.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((g, t)) => (Some(g.to_string()), t.to_string()),
        None => (None, rest.to_string()),
    };
    let up = text.to_uppercase();
    let level = if up.contains("ERREUR") || up.contains("ERROR") || up.contains("ABANDON") {
        Level::Error
    } else if up.contains("CONFLIT") || up.contains("CONFLICT") || up.contains("ATTENTION") || up.contains("AVERTISSEMENT") || up.contains("WARNING") {
        Level::Warn
    } else {
        Level::Info
    };
    Some(LogEntry { ts: ts.to_string(), level, game, text })
}

/// Lit les `max` dernieres entrees de logs\watch.log (+ la rotation .1 si besoin).
pub fn read_log(data_dir: &Path, max: usize) -> Vec<LogEntry> {
    let base = data_dir.join("logs").join("watch.log");
    let mut lines: Vec<String> = Vec::new();
    for p in [PathBuf::from(format!("{}.1", base.display())), base] {
        if let Ok(t) = std::fs::read_to_string(&p) {
            lines.extend(t.lines().map(|l| l.to_string()));
        }
    }
    let skip = lines.len().saturating_sub(max);
    lines.iter().skip(skip).filter_map(|l| parse_log_line(l)).collect()
}

#[derive(Clone, Debug, Default)]
pub struct LogFilter {
    pub min_level: Option<Level>,
    pub game: Option<String>,
    pub text: String,
}

pub fn filter_log<'a>(entries: &'a [LogEntry], f: &LogFilter) -> Vec<&'a LogEntry> {
    let t = f.text.to_lowercase();
    entries
        .iter()
        .filter(|e| f.min_level.map(|l| e.level >= l).unwrap_or(true))
        .filter(|e| f.game.as_ref().map(|g| e.game.as_ref() == Some(g)).unwrap_or(true))
        .filter(|e| t.is_empty() || e.text.to_lowercase().contains(&t) || e.game.as_deref().map(|g| g.to_lowercase().contains(&t)).unwrap_or(false))
        .collect()
}

/// Texte copie par « Copier tout le contenu du journal » : lignes visibles apres filtre (a defaut de filtre, tout), une par ligne.
pub fn log_copy_text(entries: &[LogEntry], f: &LogFilter, lang: Lang, tz: &dyn crate::timefmt::TzProvider) -> String {
    filter_log(entries, f)
        .iter()
        .map(|e| format!("{}  {}{}", crate::timefmt::log_ts_label(&e.ts, lang, tz), e.game.as_ref().map(|g| format!("[{}] ", g)).unwrap_or_default(), e.text))
        .collect::<Vec<_>>()
        .join("
")
}

// ------------------------------------------------------------------ theme

pub trait ThemeProbe {
    /// Some(true) si Windows est en theme clair, None si inconnu.
    fn system_is_light(&self) -> Option<bool>;
}

/// Theme sombre effectif. Inconnu => sombre (theme par defaut).
pub fn resolve_dark(mode: ThemeMode, probe: &dyn ThemeProbe) -> bool {
    match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => !probe.system_is_light().unwrap_or(false),
    }
}

/// Sortie de `reg query ...Personalize /v AppsUseLightTheme` -> Some(true) si clair.
pub fn parse_light_theme(output: &str) -> Option<bool> {
    for l in output.lines() {
        if l.contains("AppsUseLightTheme") {
            let v = l.split_whitespace().last()?;
            return match v {
                "0x1" => Some(true),
                "0x0" => Some(false),
                _ => None,
            };
        }
    }
    None
}

pub const ACCENT_PRESETS: [[u8; 3]; 6] = [[0x4c, 0x8d, 0xf6], [0x2e, 0xb8, 0x72], [0xf0, 0x8c, 0x2c], [0xe0, 0x4f, 0x6a], [0x9b, 0x6c, 0xf0], [0x20, 0xb8, 0xc8]];

// ------------------------------------------------------------------ modele global

pub struct AppModel {
    pub ctx: Ctx,
    pub settings: Settings,
    pub config: Config,
    pub page: Page,
    pub selected: Option<String>,
    pub confirm: Option<Confirm>,
    pub statuses: HashMap<String, GameStatus>,
}

impl AppModel {
    pub fn new(mut ctx: Ctx) -> Res<AppModel> {
        let settings = Settings::load(&ctx)?;
        settings.apply_to_ctx(&mut ctx);
        let config = Config::load(&ctx)?;
        Ok(AppModel { ctx, settings, config, page: Page::Games, selected: None, confirm: None, statuses: HashMap::new() })
    }

    pub fn dest(&self) -> Option<PathBuf> {
        self.config.dest.clone().filter(|d| !d.as_os_str().is_empty())
    }

    pub fn game(&self, name: &str) -> Option<&Game> {
        self.config.find(name)
    }

    pub fn reload(&mut self) -> Res<()> {
        self.config = Config::load(&self.ctx)?;
        self.settings = Settings::load(&self.ctx)?;
        self.settings.apply_to_ctx(&mut self.ctx);
        Ok(())
    }

    pub fn refresh_statuses(&mut self) {
        let dest = self.dest();
        self.statuses = self.config.games.iter().map(|g| (g.name.clone(), compute_status(&self.ctx, g, dest.as_deref()))).collect();
    }

    pub fn save_settings(&mut self) -> Res<()> {
        self.settings.sanitize();
        self.settings.save(&self.ctx)?;
        self.settings.apply_to_ctx(&mut self.ctx);
        Ok(())
    }

    pub fn set_dest(&mut self, dest: Option<PathBuf>) -> Res<()> {
        self.config.dest = dest;
        self.config.save(&self.ctx)
    }

    /// Demande une confirmation (rien n'est execute tant que `confirm_pending` n'est pas appelee).
    pub fn request(&mut self, c: Confirm) {
        self.confirm = Some(c);
    }

    pub fn cancel(&mut self) {
        self.confirm = None;
    }

    /// Execute l'action confirmee. Sans confirmation en attente, ne fait RIEN.
    pub fn confirm_pending(&mut self) -> Res<String> {
        let Some(c) = self.confirm.take() else { return Err("aucune action en attente de confirmation".into()) };
        self.execute(&c)
    }

    /// Execute une action confirmee (utilise aussi par les threads de travail de l'UI).
    pub fn execute(&mut self, c: &Confirm) -> Res<String> {
        let dest = self.dest();
        let need_dest = || dest.clone().ok_or("aucune destination configuree (Reglages)");
        match c {
            Confirm::Backup { game } => {
                let g = self.game(game).ok_or("element inconnu")?.clone();
                actions::do_backup(&self.ctx, &g, &need_dest()?)
            }
            Confirm::Restore { game, version } => {
                let g = self.game(game).ok_or("element inconnu")?.clone();
                actions::do_restore(&self.ctx, &g, &need_dest()?, version.as_deref())
            }
            Confirm::Prune { game, .. } => {
                let g = self.game(game).ok_or("element inconnu")?.clone();
                let n = actions::do_prune(&self.ctx, &g, &need_dest()?).len();
                Ok(format!("{} versions supprimees", n))
            }
            Confirm::Resolve { game, keep_mine } => {
                let g = self.game(game).ok_or("element inconnu")?.clone();
                actions::do_resolve(&self.ctx, &g, &need_dest()?, *keep_mine)
            }
            Confirm::Link { game, key, .. } => {
                actions::link_game(&self.ctx, game, key)?;
                self.reload()?;
                Ok(format!("lie au dossier {}", key))
            }
            Confirm::RestoreAppearance => {
                self.settings.reset_appearance();
                self.save_settings()?;
                Ok("ok".into())
            }
            Confirm::ApplyStartup => actions::do_apply_startup(&self.ctx),
            Confirm::MigrateRun => actions::do_migrate_run(&self.ctx),
            Confirm::MigrateTasks => actions::do_migrate_tasks(&self.ctx),
            Confirm::Import { path } => {
                let bak = actions::import_config(&self.ctx, path)?;
                self.reload()?;
                Ok(format!("ancienne configuration copiee dans {}", bak.display()))
            }
            Confirm::Relocate { game, exe, save, locs, .. } => actions::relocate_game(&self.ctx, game, exe.as_deref(), save.as_ref()).and_then(|m| {
                if let Some(e) = locs {
                    actions::set_locations(&self.ctx, game, e)?;
                }
                self.reload()?;
                Ok(m)
            }),
            Confirm::RemoveLocation { game, id, .. } => actions::remove_location(&self.ctx, game, id).and_then(|m| {
                self.reload()?;
                Ok(m)
            }),
            Confirm::Rename { game, display } => actions::set_display_name(&self.ctx, game, display.as_deref()).and_then(|m| {
                self.reload()?;
                Ok(m)
            }),
            Confirm::Remove { game } => {
                actions::remove_game(&self.ctx, game)?;
                self.reload()?;
                if self.selected.as_deref() == Some(game) {
                    self.selected = None;
                }
                Ok("retire de la liste (aucun fichier supprime)".into())
            }
        }
    }
}
