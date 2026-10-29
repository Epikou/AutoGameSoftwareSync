//! Travaux en arriere-plan (fil dedie) : synchronisation de tous les elements ou d'un seul, remise d'une version
//! (Remonter le temps), annulation, resolution d'un conflit, relecture des faits, et TOUTES les lectures du stockage qui
//! peuvent durer (candidats de coffre, propositions d'elements, preparation du coffre, historique d'un element, emulateurs
//! trouves) : le fil d'interface ne lit jamais le stockage en profondeur. L'interface envoie des `Job`, recoit des `Done`,
//! et redessine quand quelque chose arrive. Le fil ne touche jamais a l'interface ni au JNI.
use crate::model::{self, Candidate, ItemInfo, ItemState, Proposal, SyncResult, TtRow};
use agss::backup::{self, UndoTicket};
use agss::config::Game;
use agss::emulators::{self, Found};
use agss::watch::Logger;
use agss::{perf, util, vaults};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

/// Ce que le fil a besoin de savoir pour un travail (copie : le fil ne partage rien avec l'interface).
#[derive(Clone, Debug)]
pub struct Env {
    pub data_dir: PathBuf,
    pub device: String,
    pub vault: Option<PathBuf>,
    pub auto_cloud: bool,
    /// Remise d'une version plus recente du coffre autorisee (seulement sur confirmation explicite de l'utilisateur).
    pub auto_restore: bool,
}

#[derive(Clone, Debug)]
pub enum Job {
    /// Synchronise tous les elements puis relit les faits.
    SyncAll { env: Env, games: Vec<Game> },
    SyncOne { env: Env, game: Game },
    /// Remet une version du coffre (copie de securite avant, annulable).
    RestoreVersion { env: Env, game: Game, version: String },
    /// Remet une copie de securite locale (annulable).
    RestoreSafety { env: Env, game: Game, dir: PathBuf },
    Undo { env: Env, game: Game, ticket: UndoTicket },
    /// Conflit : garder la version locale (true) ou prendre celle du coffre (false).
    Resolve { env: Env, game: Game, keep_mine: bool },
    /// Relit les faits de tous les elements (sans rien copier).
    Refresh { env: Env, games: Vec<Game> },
    /// Cherche les coffres possibles dans les stockages (premier lancement, etape coffre).
    ScanVaults { roots: Vec<PathBuf> },
    /// Emulateurs et dossiers perso a proposer.
    Proposals { roots: Vec<PathBuf>, existing: Vec<Game> },
    /// Prepare le dossier choisi comme coffre (marqueur, sous-dossier) ; `from_setup` : premier lancement ou reglages.
    PrepareVault { path: PathBuf, from_setup: bool },
    /// Lignes de « Remonter le temps » d'un element.
    LoadHistory { env: Env, game: Game },
    /// Emulateurs trouves sur l'appareil (ecran Emulateurs).
    FindEmus { roots: Vec<PathBuf> },
}

#[derive(Clone, Debug)]
pub enum Done {
    Synced { name: String, result: SyncResult },
    Restored { name: String, ticket: UndoTicket, files: usize },
    Undone { name: String, put_back: usize },
    Resolved { name: String },
    Error { name: String, msg: String },
    Facts(Vec<ItemInfo>),
    Vaults(Vec<Candidate>),
    Proposals(Vec<Proposal>),
    VaultReady { from_setup: bool, result: Result<PathBuf, String> },
    History { name: String, rows: Vec<TtRow> },
    Emus(Vec<Found>),
    /// Fin d'un travail (la barre « copie en cours » disparait) ; `synced` : une synchro vient de se terminer.
    Finished { synced: bool },
}

pub struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Done>,
    busy: Arc<AtomicBool>,
    /// Element en cours de copie (nom).
    current: Arc<Mutex<Option<String>>>,
    pub progress: Arc<perf::Progress>,
    /// Dernier etat connu par element (issu des synchros), garde entre deux relectures.
    states: Arc<Mutex<HashMap<String, ItemState>>>,
}

impl Worker {
    /// Demarre le fil ; `wake` est appele a chaque message pour redessiner l'interface.
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Worker {
        let (tx, jrx) = channel::<Job>();
        let (dtx, rx) = channel::<Done>();
        let busy = Arc::new(AtomicBool::new(false));
        let current = Arc::new(Mutex::new(None));
        let progress = perf::Progress::new();
        let states: Arc<Mutex<HashMap<String, ItemState>>> = Arc::new(Mutex::new(HashMap::new()));
        let (b2, c2, p2, s2) = (busy.clone(), current.clone(), progress.clone(), states.clone());
        std::thread::Builder::new()
            .name("agss-worker".into())
            .spawn(move || {
                for job in jrx {
                    let synced = matches!(job, Job::SyncAll { .. } | Job::SyncOne { .. });
                    b2.store(true, Ordering::SeqCst);
                    run(job, &dtx, &c2, &p2, &s2, &*wake);
                    b2.store(false, Ordering::SeqCst);
                    *c2.lock().unwrap() = None;
                    let _ = dtx.send(Done::Finished { synced });
                    wake();
                }
            })
            .expect("fil de travail");
        Worker { tx, rx, busy, current, progress, states }
    }

    pub fn send(&self, j: Job) {
        let _ = self.tx.send(j);
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn current(&self) -> Option<String> {
        self.current.lock().unwrap().clone()
    }

    pub fn state_of(&self, name: &str) -> Option<ItemState> {
        self.states.lock().unwrap().get(name).cloned()
    }

    pub fn forget(&self, name: &str) {
        self.states.lock().unwrap().remove(name);
    }

    /// Messages arrives depuis le dernier passage.
    pub fn poll(&self) -> Vec<Done> {
        let mut v = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(d) => v.push(d),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        v
    }
}

fn logger(env: &Env) -> Logger {
    let dir = env.data_dir.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    Logger::new(dir.join("android.log"), false)
}

fn ctx_of(env: &Env, progress: &Arc<perf::Progress>) -> agss::Ctx {
    let mut ctx = model::make_ctx(&env.data_dir, &env.device);
    ctx.progress = Some(progress.clone());
    ctx
}

fn run(job: Job, out: &Sender<Done>, current: &Arc<Mutex<Option<String>>>, progress: &Arc<perf::Progress>, states: &Arc<Mutex<HashMap<String, ItemState>>>, wake: &dyn Fn()) {
    match job {
        Job::SyncAll { env, games } => {
            let ctx = ctx_of(&env, progress);
            let lg = logger(&env);
            let log = |m: &str| lg.log(m);
            log(&format!("synchro de {} element(s), appareil « {} »", games.len(), env.device));
            if let Some(v) = env.vault.as_ref().filter(|v| v.is_dir()) {
                for g in &games {
                    *current.lock().unwrap() = Some(g.name.clone());
                    states.lock().unwrap().insert(g.name.clone(), ItemState::Syncing);
                    wake();
                    let r = model::sync_item(&ctx, g, v, env.auto_cloud, env.auto_restore, &log);
                    log(&format!("[{}] {:?}", g.name, r));
                    states.lock().unwrap().insert(g.name.clone(), r.state());
                    let _ = out.send(Done::Synced { name: g.name.clone(), result: r });
                    wake();
                }
            } else {
                log("coffre injoignable : rien n'est copie");
            }
            *current.lock().unwrap() = None;
            let _ = out.send(Done::Facts(facts(&ctx, &env, &games, states)));
        }
        Job::SyncOne { env, game } => {
            let ctx = ctx_of(&env, progress);
            let lg = logger(&env);
            let log = |m: &str| lg.log(m);
            *current.lock().unwrap() = Some(game.name.clone());
            states.lock().unwrap().insert(game.name.clone(), ItemState::Syncing);
            wake();
            let r = match env.vault.as_ref().filter(|v| v.is_dir()) {
                Some(v) => model::sync_item(&ctx, &game, v, env.auto_cloud, env.auto_restore, &log),
                None => SyncResult::Error("vault unreachable".into()),
            };
            log(&format!("[{}] {:?}", game.name, r));
            states.lock().unwrap().insert(game.name.clone(), r.state());
            let _ = out.send(Done::Synced { name: game.name.clone(), result: r });
            let _ = out.send(Done::Facts(facts(&ctx, &env, std::slice::from_ref(&game), states)));
        }
        Job::RestoreVersion { env, game, version } => {
            let ctx = ctx_of(&env, progress);
            let lg = logger(&env);
            *current.lock().unwrap() = Some(game.name.clone());
            wake();
            match env.vault.as_ref().filter(|v| v.is_dir()).ok_or_else(|| "vault unreachable".to_string()).and_then(|v| backup::restore_version_undoable(&ctx, &game, v, &version).map_err(|e| e.to_string())) {
                Ok((o, ticket)) => {
                    lg.log(&format!("[{}] version « {} » remise ({} fichiers, copie de securite : {:?})", game.name, version, o.files_restored, o.safety_dir));
                    states.lock().unwrap().insert(game.name.clone(), ItemState::Safe);
                    let _ = out.send(Done::Restored { name: game.name.clone(), ticket, files: o.files_restored });
                }
                Err(e) => {
                    lg.log(&format!("[{}] ERREUR remise de version : {}", game.name, e));
                    let _ = out.send(Done::Error { name: game.name.clone(), msg: e });
                }
            }
            let _ = out.send(Done::Facts(facts(&ctx, &env, std::slice::from_ref(&game), states)));
            let _ = out.send(Done::History { name: game.name.clone(), rows: model::history_rows(&ctx, &game, env.vault.as_deref()) });
        }
        Job::RestoreSafety { env, game, dir } => {
            let ctx = ctx_of(&env, progress);
            let lg = logger(&env);
            *current.lock().unwrap() = Some(game.name.clone());
            wake();
            match backup::restore_safety_undoable(&ctx, &game, &dir) {
                Ok((o, ticket)) => {
                    lg.log(&format!("[{}] copie de securite remise ({} fichiers)", game.name, o.files_restored));
                    let _ = out.send(Done::Restored { name: game.name.clone(), ticket, files: o.files_restored });
                }
                Err(e) => {
                    let _ = out.send(Done::Error { name: game.name.clone(), msg: e.to_string() });
                }
            }
            let _ = out.send(Done::Facts(facts(&ctx, &env, std::slice::from_ref(&game), states)));
            let _ = out.send(Done::History { name: game.name.clone(), rows: model::history_rows(&ctx, &game, env.vault.as_deref()) });
        }
        Job::Undo { env, game, ticket } => {
            let ctx = ctx_of(&env, progress);
            let lg = logger(&env);
            *current.lock().unwrap() = Some(game.name.clone());
            wake();
            match backup::undo_restore(&ctx, &game, env.vault.as_deref(), &ticket) {
                Ok(u) => {
                    lg.log(&format!("[{}] remise annulee ({} fichiers remis, {} retires)", game.name, u.files_put_back, u.files_removed));
                    let _ = out.send(Done::Undone { name: game.name.clone(), put_back: u.files_put_back });
                }
                Err(e) => {
                    let _ = out.send(Done::Error { name: game.name.clone(), msg: e.to_string() });
                }
            }
            let _ = out.send(Done::Facts(facts(&ctx, &env, std::slice::from_ref(&game), states)));
            let _ = out.send(Done::History { name: game.name.clone(), rows: model::history_rows(&ctx, &game, env.vault.as_deref()) });
        }
        Job::Resolve { env, game, keep_mine } => {
            let ctx = ctx_of(&env, progress);
            let lg = logger(&env);
            *current.lock().unwrap() = Some(game.name.clone());
            wake();
            match env.vault.as_ref().filter(|v| v.is_dir()).ok_or_else(|| "vault unreachable".to_string()).and_then(|v| backup::resolve_conflict(&ctx, &game, v, keep_mine).map_err(|e| e.to_string())) {
                Ok(_) => {
                    lg.log(&format!("[{}] conflit regle (garder la mienne : {})", game.name, keep_mine));
                    states.lock().unwrap().insert(game.name.clone(), ItemState::Safe);
                    let _ = out.send(Done::Resolved { name: game.name.clone() });
                }
                Err(e) => {
                    lg.log(&format!("[{}] ERREUR conflit : {}", game.name, e));
                    let _ = out.send(Done::Error { name: game.name.clone(), msg: e });
                }
            }
            let _ = out.send(Done::Facts(facts(&ctx, &env, std::slice::from_ref(&game), states)));
            let _ = out.send(Done::History { name: game.name.clone(), rows: model::history_rows(&ctx, &game, env.vault.as_deref()) });
        }
        Job::Refresh { env, games } => {
            let ctx = ctx_of(&env, progress);
            let _ = out.send(Done::Facts(facts(&ctx, &env, &games, states)));
        }
        Job::ScanVaults { roots } => {
            let _ = out.send(Done::Vaults(model::vault_candidates(&roots)));
        }
        Job::Proposals { roots, existing } => {
            let _ = out.send(Done::Proposals(model::proposals(&roots, &existing)));
        }
        Job::PrepareVault { path, from_setup } => {
            let _ = out.send(Done::VaultReady { from_setup, result: vaults::prepare(&path) });
        }
        Job::LoadHistory { env, game } => {
            let ctx = ctx_of(&env, progress);
            let _ = out.send(Done::History { name: game.name.clone(), rows: model::history_rows(&ctx, &game, env.vault.as_deref()) });
        }
        Job::FindEmus { roots } => {
            let _ = out.send(Done::Emus(emulators::find_on_device(&roots)));
        }
    }
    let _ = util::now_ms();
}

fn facts(ctx: &agss::Ctx, env: &Env, games: &[Game], states: &Arc<Mutex<HashMap<String, ItemState>>>) -> Vec<ItemInfo> {
    let st = states.lock().unwrap().clone();
    games.iter().map(|g| model::info_of(ctx, g, env.vault.as_deref(), st.get(&g.name))).collect()
}
