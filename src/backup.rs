//! backup / restore / elagage / decision de synchro, avec toutes les regles anti-perte.
//!
//! Disposition dans la destination (voir `cloud`) : un dossier UNIQUE par sauvegarde,
//! `<dest>/<cle_jeu>/saves/<horodatage>-<machine>/`, ecrit une seule fois, jamais renomme, deplace,
//! ecrase ni modifie ensuite ; `meta.json` est ecrit en dernier (marqueur de completude).
//! Construction dans un dossier de travail LOCAL (`<donnees>/tmp/`), verification par hash, puis
//! copie dans la destination. Dans le dossier de donnees local :
//!   safety/<jeu>/<horodatage>/files/...   (snapshot avant toute restauration)
//!
//! DECISION de synchro : une chaine de versions (`id`, `parent_id`) remplace l'horloge. L'heure
//! n'intervient que pour choisir quelle branche afficher comme « version active ».
use crate::cloud::{self, Snapshot, VKind, Version};
use crate::config::{Game, State};
use crate::store::{self, Meta};
use crate::{perf, util, Ctx, Res};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_KEEP: usize = 10;
/// Plancher absolu : l'elagage ne descend jamais sous 2 anciennes versions (en plus de l'active).
pub const MIN_KEEP: usize = 2;

#[derive(Debug)]
pub enum BackupOutcome {
    /// Rien a faire : local identique a une version deja presente.
    UpToDate,
    /// Nouvelle sauvegarde creee (dossier unique).
    Backed { dir: PathBuf },
    /// VRAI conflit (les deux cotes ont evolue) : la version locale est sauvegardee comme branche
    /// separee ; rien n'est ecrase ni restaure ; a resoudre explicitement (voir `resolve_conflict`).
    Conflict { dir: PathBuf },
}

#[derive(Debug)]
pub struct RestoreOutcome {
    pub safety_dir: Option<PathBuf>,
    pub files_restored: usize,
    /// Le local avait des modifications jamais synchronisees (conservees dans safety).
    pub local_had_unsynced_changes: bool,
    /// Fichiers locaux absents de la version restauree : conserves (une restauration ne supprime jamais rien).
    pub local_only_kept: usize,
    /// Emplacements de la sauvegarde absents de ce PC (noms) : leurs fichiers n ont pas ete restaures.
    pub unknown_locations: Vec<String>,
}

fn check_not_running(ctx: &Ctx, game: &Game) -> Res<()> {
    if let Some(exe) = game.exe.as_deref().filter(|e| !e.trim().is_empty()) {
        match ctx.procs.is_running(exe) {
            Ok(true) => {
                return Err(format!(
                    "REFUS : {} est en cours d'execution ({}). Fermez le programme avant de restaurer/ecraser.",
                    exe, game.name
                )
                .into())
            }
            Ok(false) => {}
            // Doute => on refuse (securite d'abord).
            Err(e) => return Err(format!("REFUS : impossible de verifier si {} tourne ({}).", exe, e).into()),
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ decision (pure)

/// Noeud de la chaine de versions, sans dependance au disque (testable).
#[derive(Clone, Debug)]
pub struct VNode {
    pub id: String,
    pub hash: String,
    /// parent, parents fusionnes et filiation.
    pub ancestors: Vec<String>,
    pub sort_ms: u64,
}

impl VNode {
    pub fn of(v: &Version) -> VNode {
        VNode { id: v.id.clone(), hash: v.meta.global_hash.clone(), ancestors: v.meta.ancestors(), sort_ms: v.sort_ms() }
    }
}

/// Tetes de la chaine : versions dont aucune autre ne descend (indices dans `vs`, une par id).
pub fn heads(vs: &[VNode]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for (i, v) in vs.iter().enumerate() {
        let is_ancestor = vs.iter().any(|w| w.id != v.id && w.ancestors.contains(&v.id));
        if !is_ancestor && !out.iter().any(|j| vs[*j].id == v.id) {
            out.push(i);
        }
    }
    out
}

/// Ce qu'il faut faire pour synchroniser un jeu FERME (decision pure, ne modifie rien).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Plan {
    /// Ni local ni cloud : rien a faire.
    Nothing,
    /// Local et cloud identiques (on met juste a jour l'etat connu).
    InSync,
    /// Local modifie depuis la derniere synchro, cloud inchange (ou pas de cloud).
    Backup,
    /// Cloud modifie, local inchange depuis la derniere synchro (ou local absent/vide).
    Restore,
    /// Les deux ont evolue independamment (branches) : on garde les deux, aucune restauration automatique.
    Conflict,
}

/// Decision de synchro. `known_hash` / `known_id` : derniere version synchronisee connue sur CE PC.
/// Aucune comparaison d'horloges.
/// `others` : (id, hash) des versions HORS chaine de decision (ancien format) : un local identique a l'une d'elles,
/// quand toutes les tetes en descendent, est un ancetre inchange (cloud plus recent).
pub fn decide(local: Option<&str>, known_hash: Option<&str>, known_id: Option<&str>, vs: &[VNode], others: &[(String, String)]) -> Plan {
    if vs.is_empty() {
        return if local.is_some() { Plan::Backup } else { Plan::Nothing };
    }
    let Some(l) = local else { return Plan::Restore };
    let hs = heads(vs);
    let mut distinct: Vec<&str> = hs.iter().map(|i| vs[*i].hash.as_str()).collect();
    distinct.sort();
    distinct.dedup();
    // Le local est identique a une tete : a jour (ou conflit non resolu s'il existe plusieurs branches).
    if distinct.contains(&l) {
        return if distinct.len() == 1 { Plan::InSync } else { Plan::Conflict };
    }
    // Plusieurs branches et un local different de toutes : on ne choisit pas.
    if distinct.len() > 1 {
        return Plan::Conflict;
    }
    // Le local n'a pas bouge depuis la derniere synchro : le cloud a avance (la version locale est un ancetre).
    let ancestor_of_all_heads = others.iter().any(|(id, h)| h == l && hs.iter().all(|i| vs[*i].ancestors.contains(id)));
    if known_hash == Some(l) || ancestor_of_all_heads {
        return Plan::Restore;
    }
    // Le local a change. Si la tete du cloud est exactement ce sur quoi il se base : sauvegarde simple.
    let head = &vs[hs[0]];
    let based = known_id == Some(head.id.as_str()) || known_hash == Some(head.hash.as_str());
    if based {
        Plan::Backup
    } else {
        Plan::Conflict
    }
}

/// Fichiers de la version `meta` que CE PC ne gere pas (emplacement inconnu ou desactive, hors filtre) : conserves tels quels
/// dans toute nouvelle version, et comptes dans l empreinte locale (sinon deux PC s echangeraient des versions sans fin).
pub fn carried_files(target: &store::GameTarget, meta: &store::Meta) -> Vec<store::FileInfo> {
    meta.files.iter().filter(|f| !target.accepts(&f.path)).cloned().collect()
}

/// Etat local + fichiers conserves de la version active du cloud (voir `carried_files`).
fn local_state_carry(ctx: &Ctx, game: &Game, snap: &Snapshot) -> Res<Option<(Vec<store::FileInfo>, u64, String)>> {
    let Some((mut f, latest, _)) = local_state(ctx, game)? else { return Ok(None) };
    if let Some(h) = active_version(snap) {
        f.extend(carried_files(&store::GameTarget::of(game), &h.meta));
        f.sort_by(|a, b| a.path.cmp(&b.path));
    }
    let h = store::global_hash(&f);
    Ok(Some((f, latest, h)))
}

fn local_state(ctx: &Ctx, game: &Game) -> Res<Option<(Vec<store::FileInfo>, u64, String)>> {
    // Reglages Windows : export de l'etat actuel des seules cles listees avant toute lecture.
    crate::winprefs::refresh(ctx, game);
    let target = store::GameTarget::of(game);
    if !target.exists() {
        return Ok(None);
    }
    let (f, latest) = target.scan(ctx)?;
    if f.is_empty() {
        return Ok(None);
    }
    let h = store::global_hash(&f);
    Ok(Some((f, latest, h)))
}

/// (id, hash) des versions qui ne servent pas a la decision (ancien format quand le nouveau existe).
fn others_of(snap: &Snapshot) -> Vec<(String, String)> {
    let logic: Vec<String> = snap.logic_versions().iter().map(|v| v.name.clone()).collect();
    snap.versions.iter().filter(|v| !logic.contains(&v.name)).map(|v| (v.id.clone(), v.meta.global_hash.clone())).collect()
}

pub fn plan(ctx: &Ctx, game: &Game, dest: &Path) -> Res<Plan> {
    let mut op = perf::Op::start(ctx, "Analyse", &game.name).with_min(perf::PLAN_LOG_MIN);
    let snap = cloud::scan_dest(dest, game);
    op.step("lecture du dossier partage");
    let local = local_state_carry(ctx, game, &snap)?.map(|x| x.2);
    op.step("scan local");
    let vs: Vec<VNode> = snap.logic_versions().into_iter().map(VNode::of).collect();
    let state = State::load(ctx)?;
    let r = decide(local.as_deref(), state.last_synced.get(&game.name).map(|s| s.as_str()), state.last_id.get(&game.name).map(|s| s.as_str()), &vs, &others_of(&snap));
    op.finish();
    Ok(r)
}

/// Version « active » : la plus recente (heure corrigee, a defaut de mieux) parmi les tetes.
pub fn active_version(snap: &Snapshot) -> Option<Version> {
    let logic = snap.logic_versions();
    let nodes: Vec<VNode> = logic.iter().map(|v| VNode::of(v)).collect();
    heads(&nodes).into_iter().map(|i| logic[i]).max_by_key(|v| (v.sort_ms(), v.name.clone())).cloned()
}

// ------------------------------------------------------------------ creation d'une sauvegarde

fn local_tmp_root(ctx: &Ctx) -> PathBuf {
    ctx.data_dir.join("tmp")
}

/// Nettoie les dossiers de travail locaux orphelins (crashs) : UNIQUEMENT dans `<donnees>/tmp`,
/// et seulement ceux plus anciens que `min_age` (un autre processus peut en utiliser un).
pub fn cleanup_local_tmp(ctx: &Ctx, min_age: std::time::Duration) -> usize {
    let root = local_tmp_root(ctx);
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p == claimed_file(ctx) || !p.is_dir() {
                continue; // note du dossier reserve : traitee par cleanup_claimed
            }
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d >= min_age).unwrap_or(false);
            if old && p.starts_with(&root) && std::fs::remove_dir_all(&p).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// Note du dossier de version RESERVE dans le dossier partage pendant son ecriture (chemin complet, une ligne) : ecrite
/// des que le dossier est cree, retiree une fois `meta.json` ecrit. Si le processus meurt entre les deux (arret brutal,
/// batterie), `cleanup_claimed` retire ce dossier incomplet au demarrage suivant.
fn claimed_file(ctx: &Ctx) -> PathBuf {
    local_tmp_root(ctx).join("claimed.txt")
}

/// Retire, au demarrage, le dossier de version reste incomplet apres un arret brutal de CE programme (note par
/// `claimed_file`). Conditions, toutes obligatoires : le dossier est note, il existe, il porte le nom de cette machine,
/// il est directement sous un dossier `saves/`, il n'a PAS de `meta.json` (jamais une version complete), et il n'a pas
/// bouge depuis `min_age` (un autre processus de la meme machine pourrait encore y ecrire). La note est retiree des
/// qu'elle ne designe plus un dossier incomplet de cette machine. Retourne le dossier supprime.
pub fn cleanup_claimed(ctx: &Ctx, min_age: std::time::Duration) -> Option<PathBuf> {
    let file = claimed_file(ctx);
    let text = std::fs::read_to_string(&file).ok()?;
    let p = PathBuf::from(text.lines().next().unwrap_or("").trim());
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mine = name.contains(&format!("-{}", util::sanitize(&ctx.machine)));
    let under_saves = p.parent().and_then(|d| d.file_name()).map(|n| n == cloud::SAVES_DIR).unwrap_or(false);
    let incomplete = p.is_dir() && !p.join("meta.json").exists();
    let old = min_age.is_zero() || std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d >= min_age).unwrap_or(false);
    let removed = if mine && under_saves && incomplete && old && std::fs::remove_dir_all(&p).is_ok() { Some(p) } else { None };
    if removed.is_some() || !incomplete || !mine || !under_saves {
        let _ = std::fs::remove_file(&file);
    }
    removed
}

/// Cree une sauvegarde (dossier unique) a partir de `game.path`. `parent` : version de base ;
/// `merged` : autres parents (resolution de conflit).
fn create_version(ctx: &Ctx, game: &Game, dest: &Path, snap: &Snapshot, parent: Option<String>, merged: Vec<String>, note: Option<String>, op: &mut perf::Op) -> Res<PathBuf> {
    create_version_ex(ctx, game, dest, snap, None, parent, merged, note, op)
}

/// `from` : contenu repris d'une version du coffre (fichiers relus et verifies) au lieu de l'etat local (annulation
/// d'une restauration : la version active d'avant redevient la tete, sans toucher au local).
#[allow(clippy::too_many_arguments)]
fn create_version_ex(ctx: &Ctx, game: &Game, dest: &Path, snap: &Snapshot, from: Option<&Version>, parent: Option<String>, merged: Vec<String>, note: Option<String>, op: &mut perf::Op) -> Res<PathBuf> {
    let (files, latest) = match from {
        Some(v) => (v.meta.files.clone(), v.meta.latest_mtime_ms),
        None => {
            let (f, l, _) = local_state_carry(ctx, game, snap)?.ok_or("dossier de sauvegardes introuvable ou vide : on ne remplace jamais une sauvegarde par du vide")?;
            (f, l)
        }
    };
    if files.is_empty() {
        return Err("version vide : on ne remplace jamais une sauvegarde par du vide".into());
    }
    let head_ver = active_version(snap);
    let carried_paths: Vec<String> = head_ver.as_ref().map(|h| carried_files(&store::GameTarget::of(game), &h.meta).into_iter().map(|f| f.path).collect()).unwrap_or_default();
    op.step("scan local");
    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    op.set_counts(files.len(), total_bytes);
    let mut meta = store::make_meta(ctx, &game.name, files, latest, note);
    // Indices pour retrouver ce jeu depuis un autre PC (ecrits dans le meta.json de CETTE nouvelle sauvegarde seulement).
    meta.exe = game.exe.as_deref().map(crate::hints::exe_file_name).filter(|e| !e.is_empty());
    meta.game_name = Some(game.name.clone());
    meta.save_path_tpl = crate::hints::to_template(&game.path, &crate::hints::KnownFolders::real());
    // Emplacements supplementaires : liste (id stable, nom, gabarit de chemin universel) ; absente pour un jeu a un seul emplacement.
    meta.locations = game
        .enabled_locations()
        .into_iter()
        .filter(|l| l.id != crate::config::MAIN_LOCATION)
        .map(|l| store::LocMeta { id: l.id.clone(), label: l.label.clone(), is_file: l.is_file, path_tpl: crate::hints::to_template(&l.path, &crate::hints::KnownFolders::real()), filter: l.filter.clone(), exclude: l.exclude.clone() })
        .collect();
    // Indices sur l'emplacement principal (nouveau PC : element retrouve tel quel, filtre compris).
    meta.main_hint = Some(store::MainHint { is_file: game.save_is_file, filter: game.save_filter.clone(), exclude: game.save_exclude.clone(), os: store::this_os().to_string() });
    // Emplacements conserves tels quels (inconnus ou desactives sur ce PC) : leur description est reprise de la version precedente.
    if let Some(h) = &head_ver {
        for lm in &h.meta.locations {
            if !meta.locations.iter().any(|x| x.id == lm.id) && carried_paths.iter().any(|p| store::split_loc(p).0 == Some(lm.id.as_str())) {
                meta.locations.push(lm.clone());
            }
        }
    }
    // Contenu repris d'une version du coffre : sa description des emplacements l'accompagne.
    if let Some(v) = from {
        meta.locations = v.meta.locations.clone();
    }
    meta.parent_id = parent.clone();
    meta.merged_parents = merged.clone();
    // Filiation : parent, puis ses propres ancetres (survit a l'elagage d'un ancetre).
    let mut lineage: Vec<String> = Vec::new();
    for pid in parent.iter().chain(merged.iter()) {
        if !lineage.contains(pid) {
            lineage.push(pid.clone());
        }
        if let Some(pv) = snap.versions.iter().find(|v| &v.id == pid) {
            for a in pv.meta.ancestors() {
                if !lineage.contains(&a) {
                    lineage.push(a);
                }
            }
        }
    }
    lineage.truncate(64);
    meta.lineage = lineage;

    let key = cloud::dest_key(game);
    let saves = dest.join(&key).join(cloud::SAVES_DIR);
    let base = format!("{}-{}", util::timestamp(meta.saved_at_ms), util::sanitize(&ctx.machine));
    let tmp = local_tmp_root(ctx).join(format!("{}-{}", base, &meta.id[..8]));
    let mut claimed: Option<PathBuf> = None;

    let run = (|| -> Res<()> {
        // 1. Construction + verification en LOCAL (rien n'a encore touche la destination).
        std::fs::create_dir_all(&tmp)?;
        if let Some(p) = &ctx.progress {
            p.begin(perf::Phase::Prepare, meta.files.len() as u64, total_bytes);
        }
        let gt = store::GameTarget::of(game);
        let files_dir = tmp.join("files");
        store::copy_files_mapped(ctx, &meta.files, &|f| {
            if let Some(v) = from {
                return Some((v.dir.join("files").join(&f.path), files_dir.join(&f.path)));
            }
            let src = gt.local_path(&f.path).or_else(|| if carried_paths.contains(&f.path) { head_ver.as_ref().map(|h| h.dir.join("files").join(&f.path)) } else { None });
            src.map(|s| (s, files_dir.join(&f.path)))
        })?;
        op.step("copie locale verifiee");
        let meta_json = serde_json::to_string_pretty(&meta)?;
        util::write_atomic(&tmp.join("meta.json"), meta_json.as_bytes())?;
        // 2. On RESERVE le dossier de destination (create_dir echoue s'il existe : jamais de partage d'un dossier
        //    avec un autre ecrivain), puis fichiers d'abord, meta.json EN DERNIER.
        std::fs::create_dir_all(&saves)?;
        let mut dirname = base.clone();
        let mut n = 0;
        let final_dir = loop {
            let cand = saves.join(&dirname);
            match std::fs::create_dir(&cand) {
                Ok(()) => break cand,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    n += 1;
                    dirname = format!("{}-{}", base, &util::new_id(&meta.id)[..4 + n.min(4)]);
                }
                Err(e) => return Err(format!("creation de {} : {}", cand.display(), e).into()),
            }
        };
        claimed = Some(final_dir.clone());
        let _ = std::fs::write(claimed_file(ctx), final_dir.to_string_lossy().as_bytes());
        if let Some(p) = &ctx.progress {
            p.lock(); // on ecrit dans le dossier partage : plus d'annulation
            p.begin(perf::Phase::Write, meta.files.len() as u64, total_bytes);
        }
        for f in &meta.files {
            store::copy_new_verified(ctx, &tmp.join("files").join(&f.path), &final_dir.join("files").join(&f.path))?;
            if let Some(p) = &ctx.progress {
                p.file_done();
            }
        }
        op.step("ecriture dans le dossier partage (copie + relecture)");
        store::write_new(&final_dir.join("meta.json"), meta_json.as_bytes())?;
        let _ = std::fs::remove_file(claimed_file(ctx));
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp); // notre dossier de travail local
    if let Err(e) = run {
        // Dossier RESERVE par CET appel (create_dir a reussi : il nous appartient) et sans meta.json : echec -> on le retire.
        if let Some(d) = &claimed {
            if !d.join("meta.json").exists() && d.parent() == Some(saves.as_path()) {
                let _ = std::fs::remove_dir_all(d);
            }
            let _ = std::fs::remove_file(claimed_file(ctx));
        }
        return Err(e);
    }
    let final_dir = claimed.ok_or("dossier de destination non cree")?;
    // game.json : nom d'affichage d'origine, ecrit une seule fois.
    let gj = dest.join(&key).join(cloud::GAME_JSON);
    if !gj.exists() {
        let _ = store::write_new(&gj, serde_json::json!({ "name": game.name, "key": key }).to_string().as_bytes());
    }
    let mut state = State::load(ctx)?;
    state.last_synced.insert(game.name.clone(), meta.global_hash.clone());
    state.last_sync_ms.insert(game.name.clone(), util::now_ms());
    state.last_id.insert(game.name.clone(), meta.id.clone());
    state.note_action(&game.name, "backup");
    state.save(ctx)?;
    Ok(final_dir)
}

/// Id de la version sur laquelle ce PC se base : derniere synchro connue (id, ou a defaut hash).
fn base_id(snap: &Snapshot, state: &State, game: &Game) -> Option<String> {
    if let Some(id) = state.last_id.get(&game.name) {
        return Some(id.clone());
    }
    let h = state.last_synced.get(&game.name)?;
    snap.versions.iter().filter(|v| &v.meta.global_hash == h).max_by_key(|v| v.sort_ms()).map(|v| v.id.clone())
}

/// Sauvegarde `game.path` dans la destination (nouveau dossier unique).
pub fn backup(ctx: &Ctx, game: &Game, dest: &Path) -> Res<BackupOutcome> {
    backup_ex(ctx, game, dest, false)
}

/// Sauvegarde de « point de synchro » (le local n'a pas change depuis la derniere synchro) : si, entre-temps, une version
/// plus recente d'un autre PC est devenue complete dans le dossier partage, RIEN n'est ecrit (sinon une copie de l'ancien
/// etat local deviendrait une branche concurrente et pourrait remplacer la nouvelle version ailleurs) ; la prochaine
/// verification restaure la nouvelle version.
pub fn backup_in_sync(ctx: &Ctx, game: &Game, dest: &Path) -> Res<BackupOutcome> {
    backup_ex(ctx, game, dest, true)
}

fn backup_ex(ctx: &Ctx, game: &Game, dest: &Path, only_if_in_sync: bool) -> Res<BackupOutcome> {
    let mut op = perf::Op::start(ctx, "Sauvegarde", &game.name);
    if !store::GameTarget::of(game).exists() {
        return Err(format!("dossier ou fichier de sauvegardes introuvable : {}", game.path.display()).into());
    }
    let snap = cloud::scan_dest(dest, game);
    op.step("lecture du dossier partage");
    let (lf, _, local_hash) = local_state_carry(ctx, game, &snap)?.ok_or("dossier de sauvegardes vide : on ne remplace jamais une sauvegarde par du vide")?;
    op.set_counts(lf.len(), lf.iter().map(|f| f.size).sum());
    op.step("scan local");
    let mut state = State::load(ctx)?;
    let logic = snap.logic_versions();
    let nodes: Vec<VNode> = logic.iter().map(|v| VNode::of(v)).collect();
    let plan = decide(Some(&local_hash), state.last_synced.get(&game.name).map(|s| s.as_str()), state.last_id.get(&game.name).map(|s| s.as_str()), &nodes, &others_of(&snap));

    // Deja stocke tel quel : rien a ecrire (on retient seulement ce point de synchro).
    // Seule une TETE identique dispense d ecrire : un ancetre de meme contenu (local = ancien etat + fichiers en plus,
    // cloud passe a une version plus petite) doit produire une nouvelle tete, sinon Restore et Backup s alternent sans fin.
    let head_ids: Vec<String> = heads(&nodes).into_iter().map(|i| nodes[i].id.clone()).collect();
    if let Some(v) = logic.iter().filter(|v| v.meta.global_hash == local_hash && head_ids.contains(&v.id)).max_by_key(|v| v.sort_ms()) {
        state.last_synced.insert(game.name.clone(), local_hash);
        state.last_sync_ms.insert(game.name.clone(), util::now_ms());
        state.last_id.insert(game.name.clone(), v.id.clone());
        state.save(ctx)?;
        op.finish();
        return Ok(BackupOutcome::UpToDate);
    }
    if only_if_in_sync && plan == Plan::Restore {
        op.finish();
        return Ok(BackupOutcome::UpToDate);
    }
    let parent = base_id(&snap, &state, game);
    let note = if plan == Plan::Conflict { Some("branche locale en conflit (les deux versions sont conservees)".to_string()) } else { None };
    let dir = create_version(ctx, game, dest, &snap, parent, vec![], note, &mut op)?;
    prune_keep(dest, game, keep_for(ctx, &game.name));
    op.step("elagage");
    op.finish();
    Ok(if plan == Plan::Conflict { BackupOutcome::Conflict { dir } } else { BackupOutcome::Backed { dir } })
}

// ------------------------------------------------------------------ elagage

/// Nombre de versions a garder pour un jeu : reglage par jeu sinon global, JAMAIS moins de MIN_KEEP.
pub fn keep_for(ctx: &Ctx, game: &str) -> usize {
    ctx.keep_overrides.get(game).copied().unwrap_or(ctx.keep_versions).max(MIN_KEEP)
}

/// Dossiers que l'elagage supprimerait en gardant `keep` anciennes versions + l'active (plancher MIN_KEEP).
/// Ne concerne QUE des dossiers conformes crees par AutoGameSoftwareSync dans `<jeu>/saves/` ; les tetes de la
/// chaine (branches non resolues) ne sont jamais supprimees ; l'ancien format n'est jamais touche.
pub fn prune_plan(dest: &Path, game: &Game, keep: usize) -> Vec<PathBuf> {
    let snap = cloud::scan_dest(dest, game);
    prune_plan_snap(&snap, keep)
}

pub fn prune_plan_snap(snap: &Snapshot, keep: usize) -> Vec<PathBuf> {
    let keep_total = keep.max(MIN_KEEP) + 1;
    let nv = snap.new_versions(); // ancien -> recent
    let nodes: Vec<VNode> = nv.iter().map(|v| VNode::of(v)).collect();
    let head_ids: Vec<String> = heads(&nodes).into_iter().map(|i| nodes[i].id.clone()).collect();
    let mut out = Vec::new();
    for (rank, v) in nv.iter().rev().enumerate() {
        if rank < keep_total || head_ids.contains(&v.id) {
            continue;
        }
        let in_saves = v.dir.parent().and_then(|p| p.file_name()).map(|n| n == cloud::SAVES_DIR).unwrap_or(false);
        if v.kind == VKind::New && in_saves && cloud::is_save_dir_name(&v.name) {
            out.push(v.dir.clone());
        }
    }
    out
}

/// Execute l'elagage ; retourne les dossiers supprimes.
pub fn prune_keep(dest: &Path, game: &Game, keep: usize) -> Vec<PathBuf> {
    let snap = cloud::scan_dest(dest, game);
    let mut done = Vec::new();
    let mut pruned_ids = Vec::new();
    for old in prune_plan_snap(&snap, keep) {
        match std::fs::remove_dir_all(&old) {
            Ok(()) => {
                if let Some(v) = snap.versions.iter().find(|v| v.dir == old) {
                    pruned_ids.push(v.id.clone());
                }
                done.push(old)
            }
            Err(e) => eprintln!("avertissement : elagage de {} impossible : {}", old.display(), e),
        }
    }
    // Marques de position (.agss-devices) des versions supprimees : retirees (jamais reecrites), puis celles deja
    // orphelines d'un elagage precedent.
    let key = cloud::dest_key(game);
    crate::home::prune_marks(dest, &key, &pruned_ids);
    let after = if done.is_empty() { snap } else { cloud::scan_dest(dest, game) };
    crate::home::prune_stale_marks(dest, &key, &after, crate::util::now_ms());
    done
}

// ------------------------------------------------------------------ restauration

/// Restaure la version active (la plus recente verifiee) vers `game.path`.
pub fn restore(ctx: &Ctx, game: &Game, src: &Path) -> Res<RestoreOutcome> {
    let mut op = perf::Op::start(ctx, "Restauration", &game.name);
    let snap = cloud::scan_dest(src, game);
    op.step("lecture du dossier partage");
    let logic = snap.logic_versions();
    let nodes: Vec<VNode> = logic.iter().map(|v| VNode::of(v)).collect();
    // Tetes d'abord (la plus recente en premier), puis le reste du plus recent au plus ancien :
    // une version dont les hash ne correspondent pas est ignoree.
    let mut order: Vec<&Version> = heads(&nodes).into_iter().map(|i| logic[i]).collect();
    order.sort_by(|a, b| (b.sort_ms(), &b.name).cmp(&(a.sort_ms(), &a.name)));
    let mut rest: Vec<&Version> = snap.versions.iter().filter(|v| !order.iter().any(|o| o.dir == v.dir)).collect();
    rest.sort_by(|a, b| (b.sort_ms(), &b.name).cmp(&(a.sort_ms(), &a.name)));
    order.extend(rest);
    let mut last_err = match snap.incomplete.first() {
        Some(i) => format!("sauvegarde cloud corrompue ou incomplete ({} : {}). Rien n'a ete modifie", i.dir.display(), i.reason),
        None => String::from("aucune sauvegarde exploitable"),
    };
    for v in order {
        // La source est lue UNE fois (copie de travail locale verifiee) ; un dossier corrompu passe a la version suivante.
        check_not_running(ctx, game)?;
        match stage_version(ctx, v, &mut op) {
            Ok(staging) => return restore_staged(ctx, game, v, staging, &mut op).map(|r| {
                op.finish();
                r
            }),
            Err(e) if e.to_string().contains(perf::CANCELLED) => return Err(e),
            Err(e) => last_err = format!("sauvegarde cloud corrompue ({}) : {}. Rien n'a ete modifie.", v.name, e),
        }
    }
    Err(format!("{} (dans {})", last_err, src.display()).into())
}

/// Restaure une version precise (nom exact : voir `history` / `versions`). Memes protections que `restore`.
pub fn restore_version(ctx: &Ctx, game: &Game, src: &Path, version: &str) -> Res<RestoreOutcome> {
    let mut op = perf::Op::start(ctx, "Restauration (version choisie)", &game.name);
    let snap = cloud::scan_dest(src, game);
    op.step("lecture du dossier partage");
    // Le nom doit correspondre EXACTEMENT a une version existante (pas de chemin arbitraire).
    let v = snap.versions.iter().find(|v| v.name == version).ok_or_else(|| format!("version '{}' introuvable (voir `versions`)", version))?.clone();
    check_not_running(ctx, game)?;
    let staging = stage_version(ctx, &v, &mut op).map_err(|e| {
        if e.to_string().contains(perf::CANCELLED) { e } else { format!("sauvegarde cloud corrompue : {}. Rien n'a ete modifie.", e).into() }
    })?;
    let out = restore_staged(ctx, game, &v, staging, &mut op)?;
    // Une ancienne version restauree a la main devient la version ACTIVE : une nouvelle sauvegarde (dossier
    // unique, descendant de la tete actuelle) en est creee. Sans cela la synchro automatique re-restaurerait
    // aussitot la version la plus recente par-dessus le choix de l'utilisateur. Echec tolere (rien n'est perdu :
    // le choix reste dans le local et la version precedente dans le dossier de securite).
    if let Some(active) = active_version(&snap) {
        if active.dir != v.dir && active.meta.global_hash != v.meta.global_hash {
            let note = format!("restauration manuelle de la version {}", v.name);
            if create_version(ctx, game, src, &snap, Some(active.id.clone()), vec![v.id.clone()], Some(note), &mut op).is_ok() {
                prune_keep(src, game, keep_for(ctx, &game.name));
            }
        }
    }
    op.finish();
    Ok(out)
}

/// Verifie ET rapatrie une version du dossier partage en LOCAL, en une seule lecture de chaque fichier :
/// les octets lus sont ecrits dans un dossier de travail local (`<donnees>/tmp/restore-*`) et leur hash
/// est calcule au passage ; il doit egaler celui de meta.json. Le fichier de travail est ensuite relu
/// (en local, rapide) pour controle. Rien n'est ecrit dans le dossier de sauvegardes ici.
/// Retourne le dossier de travail (`files/...`), a supprimer apres usage.
fn stage_version(ctx: &Ctx, v: &Version, op: &mut perf::Op) -> Res<PathBuf> {
    use std::io::Write;
    let staging = store::unique_dir(&local_tmp_root(ctx), &format!("restore-{}", util::timestamp(util::now_ms())));
    let total: u64 = v.meta.files.iter().map(|f| f.size).sum();
    let run = (|| -> Res<()> {
        std::fs::create_dir_all(staging.join("files"))?;
        if let Some(p) = &ctx.progress {
            p.begin(perf::Phase::Read, v.meta.files.len() as u64, total);
        }
        // Dossier partage « a la demande » ? (attributs seuls : aucune lecture de donnees)
        let on_demand = v.meta.files.iter().filter(|f| std::fs::metadata(v.dir.join("files").join(&f.path)).map(|m| perf::attrs_say_on_demand(perf::file_attrs(&m))).unwrap_or(false)).count();
        if on_demand > 0 {
            perf::log(ctx, &format!("[Perf] {} fichier(s) sur {} marque(s) « a la demande / hors ligne » dans le dossier partage. {}", on_demand, v.meta.files.len(), perf::STREAMING_WARNING));
            perf::mark_streaming();
            if let Some(p) = &ctx.progress {
                p.flag_streaming();
            }
        }
        let t_read = std::time::Instant::now();
        let mut read_bytes = 0u64;
        for f in &v.meta.files {
            let src = v.dir.join("files").join(&f.path);
            let dst = staging.join("files").join(&f.path);
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut out = std::fs::File::create(&dst).map_err(|e| format!("creation {} : {}", dst.display(), e))?;
            let (h, n) = store::stream_copy(ctx, &src, &mut out).map_err(|e| if e.to_string().contains(perf::CANCELLED) { e } else { format!("{} : {}", f.path, e).into() })?;
            if ctx.fault_corrupt_copy {
                out.write_all(b"\0CORRUPTION-SIMULEE")?;
            }
            drop(out);
            if h != f.hash {
                return Err(format!("{} ne correspond pas a son hash", f.path).into());
            }
            // Controle de l'ecriture de travail (relecture LOCALE du seul fichier ecrit).
            if util::hash_file(&dst)? != h {
                return Err(format!("ECHEC DE VERIFICATION pour {} (hash different apres ecriture)", dst.display()).into());
            }
            read_bytes += n;
            if let Some(p) = &ctx.progress {
                p.file_done();
            }
        }
        // Mesure reelle : lecture lente du dossier partage ? (indice probable d'un mode « a la demande »)
        if perf::read_is_slow(read_bytes, t_read.elapsed()) {
            perf::log(ctx, &format!("[Perf] lecture lente du dossier partage ({} en {}). {}", perf::fmt_mb(read_bytes), perf::fmt_secs(t_read.elapsed()), perf::STREAMING_WARNING));
            perf::mark_streaming();
            if let Some(p) = &ctx.progress {
                p.flag_streaming();
            }
        }
        Ok(())
    })();
    op.step("lecture + verification de la source (1 passage)");
    match run {
        Ok(()) => Ok(staging),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging); // notre dossier de travail
            Err(e)
        }
    }
}

/// Etapes protegees de la restauration, a partir d'une source DEJA verifiee et rapatriee en local :
/// snapshot de securite verifie de l'etat local, puis ecriture fichier par fichier (atomique + verifiee).
/// Aucun fichier local n'est supprime. Supprime le dossier de travail `staging` dans tous les cas.
fn restore_staged(ctx: &Ctx, game: &Game, v: &Version, staging: PathBuf, op: &mut perf::Op) -> Res<RestoreOutcome> {
    let r = restore_staged_inner(ctx, game, v, &staging, op);
    let _ = std::fs::remove_dir_all(&staging);
    r
}

/// Copie de securite VERIFIEE de l'etat local (`<donnees>/safety/<jeu>/<horodatage>/files/...` + meta.json ecrit en
/// dernier). `lfiles` : scan local non vide. `what` complete le message d'echec (« restauration annulee »...).
fn safety_snapshot(ctx: &Ctx, game: &Game, target: &store::GameTarget, lfiles: Vec<store::FileInfo>, llatest: u64, note: &str, what: &str) -> Res<PathBuf> {
    let lmeta = store::make_meta(ctx, &game.name, lfiles, llatest, Some(note.into()));
    // 2.0.3 : pas de copie en double. Contenu identique a une copie de securite existante de cet element (empreinte globale,
    // la plus recente d'abord) et cette copie est intacte (relue) : on la reutilise ; « Annuler » s'appuie alors sur elle.
    // Elle est marquee « utilisee maintenant » (`used.txt`) : le nettoyage la traite comme une copie toute neuve.
    // Jamais une copie que la regle de nettoyage supprimerait, ni pendant un « Liberer de la place » (verrou) : dans ces
    // cas, copie neuve (une suppression en cours ne peut pas emporter la copie dont « Annuler » aura besoin).
    if !safety_free_running(ctx) {
        let all = safety_copies(ctx, game);
        let doomed: Vec<PathBuf> = safety_prune_plan_used(&all, &used_map(&all), util::now_ms()).into_iter().map(|c| c.dir).collect();
        for prev in all.into_iter().rev().filter(|p| !p.hash.is_empty() && p.hash == lmeta.global_hash && !doomed.contains(&p.dir)) {
            if store::read_meta(&prev.dir.join("meta.json")).ok().map(|m| verify_safety(&prev.dir, &m).is_ok()).unwrap_or(false) {
                let _ = util::write_atomic(&prev.dir.join(SAFETY_USED_FILE), util::now_ms().to_string().as_bytes());
                if prev.dir.join("meta.json").is_file() {
                    return Ok(prev.dir);
                }
            }
        }
    }
    let dir = store::unique_dir(&safety_root(ctx, game), &util::timestamp(util::now_ms()));
    if let Some(p) = &ctx.progress {
        p.begin(perf::Phase::Safety, lmeta.files.len() as u64, lmeta.files.iter().map(|f| f.size).sum());
    }
    let snap_files = dir.join("files");
    store::copy_files_mapped(ctx, &lmeta.files, &|f| target.local_path(&f.path).map(|s| (s, snap_files.join(&f.path)))).map_err(|e| {
        if e.to_string().contains(perf::CANCELLED) {
            e.to_string()
        } else {
            format!("snapshot de securite impossible, {}, rien modifie : {}", what, e)
        }
    })?;
    util::write_atomic(&dir.join("meta.json"), serde_json::to_string_pretty(&lmeta)?.as_bytes())?;
    // Nettoyage automatique (regle prudente) des anciennes copies de cet element ; la nouvelle est toujours gardee.
    let _ = prune_safety_of(ctx, &safety_root(ctx, game), util::now_ms());
    Ok(dir)
}

// ------------------------------------------------------------------ copies de securite : nettoyage (2.0.3)

/// Copies de securite toujours gardees par element : les plus recentes...
pub const SAFETY_KEEP: usize = 3;
/// ... et tout ce qui a moins de 30 jours.
pub const SAFETY_KEEP_DAYS: u64 = 30;
/// Copie interrompue (sans meta.json) : nettoyee seulement apres un jour (une copie en cours n'a pas encore son meta.json).
pub const SAFETY_STALE_MS: u64 = 86_400_000;

/// Copies a supprimer selon la regle : jamais la seule copie, jamais une des 3 plus recentes, jamais une copie de moins de
/// 30 jours. Pure.
pub fn safety_prune_plan(copies: &[SafetyCopy], now_ms: u64) -> Vec<SafetyCopy> {
    safety_prune_plan_used(copies, &HashMap::new(), now_ms)
}

/// Meme regle ; `used` : copie reutilisee (pas de doublon) -> date de sa derniere utilisation, qui compte comme sa date.
pub fn safety_prune_plan_used(copies: &[SafetyCopy], used: &HashMap<PathBuf, u64>, now_ms: u64) -> Vec<SafetyCopy> {
    if copies.len() <= 1 {
        return vec![];
    }
    let eff = |c: &SafetyCopy| c.ms.max(used.get(&c.dir).copied().unwrap_or(0));
    let mut v: Vec<&SafetyCopy> = copies.iter().collect();
    v.sort_by(|a, b| (eff(b), &b.dir).cmp(&(eff(a), &a.dir)));
    v.into_iter().enumerate().filter(|(i, c)| *i >= SAFETY_KEEP && now_ms.saturating_sub(eff(c)) >= SAFETY_KEEP_DAYS * 86_400_000).map(|(_, c)| c.clone()).collect()
}

/// Marque d'une copie reutilisee (date de derniere utilisation, ms).
pub const SAFETY_USED_FILE: &str = "used.txt";
/// Une copie utilisee il y a moins d'une heure (remise en cours, « Annuler » recent) n'est jamais supprimee, meme si un
/// plan de nettoyage calcule avant la decidait.
pub const SAFETY_USED_GRACE_MS: u64 = 3_600_000;
/// Verrou de « Liberer de la place » (`<donnees>/safety/.free.lock`) : pendant qu'il est pose (moins de 10 min), aucune
/// copie existante n'est reutilisee par une remise (copie neuve a la place).
pub const SAFETY_FREE_LOCK: &str = ".free.lock";
const SAFETY_FREE_LOCK_MAX_MS: u64 = 10 * 60 * 1000;

fn used_ms(dir: &Path) -> Option<u64> {
    std::fs::read_to_string(dir.join(SAFETY_USED_FILE)).ok().and_then(|t| t.trim().parse::<u64>().ok())
}

fn used_map(copies: &[SafetyCopy]) -> HashMap<PathBuf, u64> {
    copies.iter().filter_map(|c| used_ms(&c.dir).map(|ms| (c.dir.clone(), ms))).collect()
}

fn free_lock_path(ctx: &Ctx) -> PathBuf {
    ctx.data_dir.join("safety").join(SAFETY_FREE_LOCK)
}

/// « Liberer de la place » est-il en cours (verrou recent) ? Un verrou oublie (plantage) expire apres 10 min.
pub fn safety_free_running(ctx: &Ctx) -> bool {
    std::fs::read_to_string(free_lock_path(ctx)).ok().and_then(|t| t.trim().parse::<u64>().ok()).map(|at| util::now_ms().saturating_sub(at) < SAFETY_FREE_LOCK_MAX_MS).unwrap_or(false)
}

/// Copies completes d'un dossier `<donnees>/safety/<element>/` (meme lecture que `safety_copies`).
fn safety_copies_in(root: &Path) -> Vec<SafetyCopy> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.filter_map(|e| e.ok()) {
            let dir = e.path();
            let Ok(m) = store::read_meta(&dir.join("meta.json")) else { continue };
            if m.files.is_empty() || !dir.join("files").is_dir() {
                continue;
            }
            out.push(SafetyCopy { ms: m.sort_ms(), files: m.files.len(), bytes: m.files.iter().map(|f| f.size).sum(), hash: m.global_hash.clone(), note: m.note.clone(), dir });
        }
    }
    out.sort_by(|a, b| (a.ms, &a.dir).cmp(&(b.ms, &b.dir)));
    out
}

/// Dossiers interrompus (sans meta.json) de plus d'un jour sous `root`.
fn safety_stale_in(root: &Path, now_ms: u64) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.filter_map(|e| e.ok()) {
            let dir = e.path();
            if !dir.is_dir() || dir.join("meta.json").exists() {
                continue;
            }
            let age = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| now_ms.saturating_sub(d.as_millis() as u64)).unwrap_or(0);
            if age >= SAFETY_STALE_MS {
                out.push(dir);
            }
        }
    }
    out
}

/// Supprime UNE copie de securite, avec garde-fou : seulement un dossier `<donnees>/safety/<element>/<copie>`, et jamais
/// une copie utilisee il y a moins d'une heure (`used.txt` relu juste avant : une remise a pu la reutiliser entre le
/// calcul du plan et la suppression).
fn remove_safety_dir(ctx: &Ctx, dir: &Path, now_ms: u64) -> bool {
    let base = ctx.data_dir.join("safety");
    let ok = dir.parent().and_then(|p| p.parent()).map(|g| g == base.as_path()).unwrap_or(false) && dir.file_name().is_some();
    if !ok || used_ms(dir).map(|u| now_ms.saturating_sub(u) < SAFETY_USED_GRACE_MS).unwrap_or(false) {
        return false;
    }
    std::fs::remove_dir_all(dir).is_ok()
}

/// Applique la regle a un element. Retourne (copies supprimees, octets liberes).
fn prune_safety_of(ctx: &Ctx, root: &Path, now_ms: u64) -> (usize, u64) {
    let mut n = (0, 0);
    let all = safety_copies_in(root);
    for c in safety_prune_plan_used(&all, &used_map(&all), now_ms) {
        let b = crate::actions::dir_size(&c.dir);
        if remove_safety_dir(ctx, &c.dir, now_ms) {
            n.0 += 1;
            n.1 += b;
        }
    }
    for d in safety_stale_in(root, now_ms) {
        let b = crate::actions::dir_size(&d);
        if remove_safety_dir(ctx, &d, now_ms) {
            n.0 += 1;
            n.1 += b;
        }
    }
    n
}

/// Place utilisee par les copies de securite de cet appareil et ce que « Libérer de la place » supprimerait.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SafetyUsage {
    /// Octets reellement occupes sur le disque (dossier `safety` entier).
    pub bytes: u64,
    /// Copies completes, tous elements confondus.
    pub copies: usize,
    /// A supprimer : (dossier de l'element, copie). Copies interrompues : `files == 0`.
    pub plan: Vec<(String, SafetyCopy)>,
}

impl SafetyUsage {
    pub fn plan_bytes(&self) -> u64 {
        self.plan.iter().map(|(_, c)| c.bytes).sum()
    }
}

/// Etat des copies de securite (lecture seule).
pub fn safety_usage(ctx: &Ctx, now_ms: u64) -> SafetyUsage {
    let base = ctx.data_dir.join("safety");
    let mut u = SafetyUsage { bytes: crate::actions::dir_size(&base), ..Default::default() };
    let Ok(rd) = std::fs::read_dir(&base) else { return u };
    let mut roots: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    roots.sort();
    for root in roots {
        let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let all = safety_copies_in(&root);
        u.copies += all.len();
        for c in safety_prune_plan_used(&all, &used_map(&all), now_ms) {
            u.plan.push((name.clone(), c));
        }
        for d in safety_stale_in(&root, now_ms) {
            let bytes = crate::actions::dir_size(&d);
            u.plan.push((name.clone(), SafetyCopy { dir: d, ms: 0, files: 0, bytes, hash: String::new(), note: None }));
        }
    }
    u
}

/// « Libérer de la place » : applique la regle a tous les elements (y compris ceux qui ne sont plus proteges). Retourne
/// (copies supprimees, octets liberes).
pub fn safety_free(ctx: &Ctx, now_ms: u64) -> (usize, u64) {
    let base = ctx.data_dir.join("safety");
    let Ok(rd) = std::fs::read_dir(&base) else { return (0, 0) };
    // Verrou : les remises en cours font une copie neuve au lieu de reutiliser une copie que ce nettoyage pourrait emporter.
    let lock = free_lock_path(ctx);
    let _ = util::write_atomic(&lock, util::now_ms().to_string().as_bytes());
    let mut n = (0, 0);
    for root in rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
        let (a, b) = prune_safety_of(ctx, &root, now_ms);
        n.0 += a;
        n.1 += b;
    }
    let _ = std::fs::remove_file(&lock);
    n
}

/// Dossier des copies de securite d'un element (dans le dossier de donnees LOCAL).
pub fn safety_root(ctx: &Ctx, game: &Game) -> PathBuf {
    ctx.data_dir.join("safety").join(util::sanitize(&game.name))
}

fn restore_staged_inner(ctx: &Ctx, game: &Game, v: &Version, staging: &Path, op: &mut perf::Op) -> Res<RestoreOutcome> {
    let meta = &v.meta;
    op.set_counts(meta.files.len(), meta.files.iter().map(|f| f.size).sum());

    // Snapshot de securite de l'etat local actuel.
    let mut state = State::load(ctx)?;
    let known = state.last_synced.get(&game.name).cloned();
    let mut safety_dir = None;
    let mut unsynced = false;
    let mut local_only_kept = 0usize;
    // Reglages Windows : la copie de securite contient l'etat ACTUEL de Windows (export juste avant).
    crate::winprefs::refresh(ctx, game);
    let target = store::GameTarget::of(game);
    if target.exists() {
        let (lfiles, llatest) = target.scan(ctx)?;
        op.step("scan local");
        if !lfiles.is_empty() {
            let mut lall = lfiles.clone();
            lall.extend(carried_files(&target, meta));
            lall.sort_by(|a, b| a.path.cmp(&b.path));
            let lhash = store::global_hash(&lall);
            local_only_kept = lfiles.iter().filter(|f| !meta.files.iter().any(|m| m.path == f.path)).count();
            unsynced = lhash != meta.global_hash && known.as_deref() != Some(lhash.as_str());
            safety_dir = Some(safety_snapshot(ctx, game, &target, lfiles, llatest, "snapshot de securite avant restauration", "restauration annulee")?);
        }
    }
    op.step("snapshot de securite verifie");
    if let Some(p) = &ctx.progress {
        p.check_cancel()?;
        p.lock(); // a partir d'ici on ecrit dans le dossier de sauvegardes : plus d'annulation
        p.begin(perf::Phase::Write, meta.files.len() as u64, meta.files.iter().map(|f| f.size).sum());
    }

    // Ecriture fichier par fichier (atomique + verifiee, EN LOCAL). Aucun fichier local n'est supprime.
    for r in target.roots() {
        std::fs::create_dir_all(&r)?;
    }
    // La restauration ne touche QUE les fichiers de l ensemble du jeu (fichier unique / filtre), jamais le reste du dossier.
    let restore_files: Vec<store::FileInfo> = meta.files.iter().filter(|f| target.accepts(&f.path)).cloned().collect();
    // Emplacements de la sauvegarde absents de ce PC : fichiers non restaures, signales (journal + fiche More).
    let mut unknown: Vec<(String, String)> = Vec::new();
    for f in meta.files.iter().filter(|f| target.is_unknown(&f.path)) {
        if let Some(id) = store::split_loc(&f.path).0 {
            if !unknown.iter().any(|u| u.0 == id) {
                let label = meta.locations.iter().find(|l| l.id == id).map(|l| l.label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| id.to_string());
                unknown.push((id.to_string(), label));
            }
        }
    }
    let stg = staging.join("files");
    store::copy_files_mapped(ctx, &restore_files, &|f| target.local_path(&f.path).map(|d| (stg.join(&f.path), d)))
        .map_err(|e| format!("restauration interrompue ({}). L'etat precedent est dans {:?}", e, safety_dir))?;
    op.step("ecriture dans le dossier de sauvegardes");

    state.last_synced.insert(game.name.clone(), meta.global_hash.clone());

    state.last_sync_ms.insert(game.name.clone(), util::now_ms());
    state.last_id.insert(game.name.clone(), v.id.clone());
    state.note_action(&game.name, "restore");
    if unknown.is_empty() {
        state.unknown_locs.remove(&game.name);
    } else {
        state.unknown_locs.insert(game.name.clone(), unknown.clone());
    }
    state.save(ctx)?;
    // Reglages Windows : ecriture ciblee des valeurs remises + notification (bilan garde pour la fiche).
    crate::winprefs::after_restore(ctx, game);
    Ok(RestoreOutcome { safety_dir, files_restored: meta.files.len(), local_had_unsynced_changes: unsynced, local_only_kept, unknown_locations: unknown.iter().map(|u| u.1.clone()).collect() })
}

/// Restauration d'une version precise deja choisie (utilise par la resolution de conflit).
fn restore_dir(ctx: &Ctx, game: &Game, v: &Version, op: &mut perf::Op) -> Res<RestoreOutcome> {
    // Jamais pendant que le jeu tourne.
    check_not_running(ctx, game)?;
    // La source cloud doit etre intacte (hash de chaque fichier == meta), AVANT de toucher au local.
    let staging = stage_version(ctx, v, op).map_err(|e| {
        if e.to_string().contains(perf::CANCELLED) { e } else { format!("sauvegarde cloud corrompue : {}. Rien n'a ete modifie.", e).into() }
    })?;
    restore_staged(ctx, game, v, staging, op)
}

// ------------------------------------------------------------------ historique

#[derive(Clone, Debug)]
pub struct HistoryRow {
    pub name: String,
    pub kind: VKind,
    pub machine: String,
    pub saved_at_ms: u64,
    pub files: usize,
    pub id: String,
    /// « Version active » : la plus recente des tetes.
    pub active: bool,
    /// Tete de la chaine (non resolue si plusieurs).
    pub head: bool,
    /// Identique au contenu local actuel.
    pub is_local: bool,
    pub note: Option<String>,
    /// Taille totale des fichiers de la version (octets) et empreinte de son contenu.
    pub bytes: u64,
    pub hash: String,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    /// Du plus recent au plus ancien.
    pub rows: Vec<HistoryRow>,
    pub incomplete: Vec<cloud::Incomplete>,
    pub tmp_dirs: Vec<PathBuf>,
    /// Vrai conflit : noms des tetes en concurrence (au moins 2).
    pub conflict_heads: Vec<String>,
}

pub fn history(ctx: &Ctx, game: &Game, dest: &Path) -> History {
    let snap = cloud::scan_dest(dest, game);
    let local = local_state_carry(ctx, game, &snap).ok().flatten().map(|x| x.2);
    let logic = snap.logic_versions();
    let nodes: Vec<VNode> = logic.iter().map(|v| VNode::of(v)).collect();
    let head_idx = heads(&nodes);
    let head_dirs: Vec<PathBuf> = head_idx.iter().map(|i| logic[*i].dir.clone()).collect();
    let active = active_version(&snap);
    let mut rows: Vec<HistoryRow> = snap
        .versions
        .iter()
        .map(|v| HistoryRow {
            name: v.name.clone(),
            kind: v.kind,
            machine: v.meta.machine.clone(),
            saved_at_ms: v.sort_ms(),
            files: v.meta.files.len(),
            id: v.id.clone(),
            active: active.as_ref().map(|a| a.dir == v.dir).unwrap_or(false),
            head: head_dirs.contains(&v.dir),
            is_local: local.as_deref() == Some(v.meta.global_hash.as_str()),
            note: v.meta.note.clone(),
            bytes: v.meta.files.iter().map(|f| f.size).sum(),
            hash: v.meta.global_hash.clone(),
        })
        .collect();
    rows.sort_by(|a, b| (b.saved_at_ms, &b.name).cmp(&(a.saved_at_ms, &a.name)));
    let state = State::load(ctx).unwrap_or_default();
    let plan = decide(local.as_deref(), state.last_synced.get(&game.name).map(|s| s.as_str()), state.last_id.get(&game.name).map(|s| s.as_str()), &nodes, &others_of(&snap));
    let conflict_heads = if plan == Plan::Conflict { head_idx.iter().map(|i| logic[*i].name.clone()).collect() } else { vec![] };
    History { rows, incomplete: snap.incomplete.clone(), tmp_dirs: snap.tmp_dirs.clone(), conflict_heads }
}

/// Dossiers des versions du nouveau format (de la plus ancienne a la plus recente).
pub fn list_versions(dest: &Path, game: &Game) -> Vec<PathBuf> {
    cloud::scan_dest(dest, game).new_versions().into_iter().map(|v| v.dir.clone()).collect()
}

// ------------------------------------------------------------------ resolution d'un vrai conflit

#[derive(Debug)]
pub struct ResolveOutcome {
    pub merged_dir: PathBuf,
    pub restored: Option<RestoreOutcome>,
}

/// Resout un VRAI conflit de facon explicite. Les deux versions restent conservees dans tous les cas :
/// * `keep_mine` : la version locale est conservee (sauvegardee si besoin) puis une sauvegarde de
///   resolution, descendant de toutes les branches, devient l'unique tete ;
/// * sinon : la version locale non sauvegardee est d'abord conservee comme branche, la version du
///   cloud est restauree (snapshot de securite, jeu ferme) puis la sauvegarde de resolution est creee.
pub fn resolve_conflict(ctx: &Ctx, game: &Game, dest: &Path, keep_mine: bool) -> Res<ResolveOutcome> {
    let mut op = perf::Op::start(ctx, "Resolution de conflit", &game.name);
    let snap = cloud::scan_dest(dest, game);
    let logic = snap.logic_versions();
    let nodes: Vec<VNode> = logic.iter().map(|v| VNode::of(v)).collect();
    let (_, _, local_hash) = local_state_carry(ctx, game, &snap)?.ok_or("dossier de sauvegardes vide : rien a resoudre")?;
    op.step("scan local + lecture du dossier partage");
    let state = State::load(ctx)?;
    let plan = decide(Some(&local_hash), state.last_synced.get(&game.name).map(|s| s.as_str()), state.last_id.get(&game.name).map(|s| s.as_str()), &nodes, &others_of(&snap));
    if plan != Plan::Conflict {
        return Err("aucun conflit a resoudre pour cet element".into());
    }
    if !keep_mine {
        check_not_running(ctx, game)?;
    }
    let head_versions: Vec<&Version> = heads(&nodes).into_iter().map(|i| logic[i]).collect();
    // 1. Le local est-il deja dans une version ? Sinon on le conserve comme branche.
    let mut snap2 = snap.clone();
    let mine_id = match logic.iter().find(|v| v.meta.global_hash == local_hash) {
        Some(v) => v.id.clone(),
        None => {
            let parent = base_id(&snap, &state, game);
            let dir = create_version(ctx, game, dest, &snap, parent, vec![], Some("version locale conservee avant resolution d'un conflit".into()), &mut op)?;
            snap2 = cloud::scan_dest(dest, game);
            snap2.versions.iter().find(|v| v.dir == dir).map(|v| v.id.clone()).ok_or("version locale introuvable apres sauvegarde")?
        }
    };
    let mut all_ids: Vec<String> = head_versions.iter().map(|v| v.id.clone()).collect();
    if !all_ids.contains(&mine_id) {
        all_ids.push(mine_id.clone());
    }
    // 2. Version du cloud a restaurer (la plus recente des tetes differentes du local).
    let mut restored = None;
    let parent = if keep_mine {
        mine_id.clone()
    } else {
        let cloud_head = head_versions.iter().filter(|v| v.meta.global_hash != local_hash).max_by_key(|v| (v.sort_ms(), v.name.clone())).ok_or("version du cloud introuvable")?;
        let cloud_head = (*cloud_head).clone();
        restored = Some(restore_dir(ctx, game, &cloud_head, &mut op)?);
        cloud_head.id.clone()
    };
    let merged: Vec<String> = all_ids.into_iter().filter(|i| *i != parent).collect();
    let note = if keep_mine { "resolution de conflit : version locale conservee" } else { "resolution de conflit : version du cloud conservee" };
    let dir = create_version(ctx, game, dest, &snap2, Some(parent), merged, Some(note.into()), &mut op)?;
    prune_keep(dest, game, keep_for(ctx, &game.name));
    op.finish();
    if let Ok(mut st) = State::load(ctx) {
        st.note_action(&game.name, "conflict_resolved");
        let _ = st.save(ctx);
    }
    Ok(ResolveOutcome { merged_dir: dir, restored })
}

/// Meta de la version active (utilitaire : tests et interface).
pub fn active_meta(dest: &Path, game: &Game) -> Option<Meta> {
    active_version(&cloud::scan_dest(dest, game)).map(|v| v.meta)
}

/// Contexte d'un conflit pour le journal : premiere synchro de ce PC ? combien de fichiers de chaque cote ?
#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub first_sync: bool,
    pub local_files: usize,
    pub cloud_files: usize,
    pub cloud_machine: String,
}

pub fn conflict_info(ctx: &Ctx, game: &Game, dest: &Path) -> Option<ConflictInfo> {
    let state = State::load(ctx).ok()?;
    let local_files = local_state(ctx, game).ok()??.0.len();
    let snap = cloud::scan_dest(dest, game);
    let head = active_version(&snap)?;
    Some(ConflictInfo {
        first_sync: !state.last_synced.contains_key(&game.name),
        local_files,
        cloud_files: head.meta.files.len(),
        cloud_machine: head.meta.machine.clone(),
    })
}

/// (nombre de fichiers, taille totale en octets) de l'ensemble local du jeu (apres filtre) ; (0, 0) si vide ou absent.
pub fn local_summary(ctx: &Ctx, game: &Game) -> (u64, u64) {
    match local_state(ctx, game) {
        Ok(Some((f, _, _))) => (f.len() as u64, f.iter().map(|x| x.size).sum()),
        _ => (0, 0),
    }
}

// ------------------------------------------------------------------ remonter le temps : copies de securite et annulation

/// Copie de securite d'un element (dossier LOCAL `<donnees>/safety/<jeu>/<horodatage>/`), prise avant toute restauration.
#[derive(Clone, Debug, PartialEq)]
pub struct SafetyCopy {
    pub dir: PathBuf,
    /// Date de la copie (ms UTC, heure corrigee).
    pub ms: u64,
    pub files: usize,
    pub bytes: u64,
    pub hash: String,
    pub note: Option<String>,
}

/// Copies de securite completes (meta.json present, fichiers presents) d'un element, de la plus ancienne a la plus recente.
pub fn safety_copies(ctx: &Ctx, game: &Game) -> Vec<SafetyCopy> {
    safety_copies_in(&safety_root(ctx, game))
}

/// Tous les fichiers d'une copie de securite sont presents et intacts (empreinte relue) ; sinon rien ne doit etre touche.
fn verify_safety(dir: &Path, meta: &Meta) -> Res<()> {
    for f in &meta.files {
        let p = dir.join("files").join(&f.path);
        match util::hash_file(&p) {
            Ok(h) if h == f.hash => {}
            Ok(_) => return Err(format!("copie de securite abimee ({}) : rien n'a ete modifie", f.path).into()),
            Err(e) => return Err(format!("copie de securite incomplete ({} : {}) : rien n'a ete modifie", f.path, e).into()),
        }
    }
    Ok(())
}

/// Ce qu'il faut pour ANNULER une restauration faite depuis l'interface (toast « Version remise · Annuler »).
#[derive(Clone, Debug, PartialEq)]
pub struct UndoTicket {
    /// Nom d'origine de l'element.
    pub game: String,
    /// Copie de securite prise juste avant (None : rien en local avant la restauration).
    pub safety_dir: Option<PathBuf>,
    /// Fichiers que la restauration a CREES (absents avant) : chemin local + empreinte ecrite.
    pub created: Vec<(PathBuf, String)>,
    /// Dossiers que la restauration a crees (du plus profond au moins profond).
    pub created_dirs: Vec<PathBuf>,
    /// Etat de synchro de ce PC avant la restauration.
    pub prev_synced: Option<String>,
    pub prev_id: Option<String>,
    pub prev_sync_ms: Option<u64>,
    /// Version active du coffre avant (nom exact), quand la restauration a cree une nouvelle version active.
    pub prev_active: Option<String>,
}

/// Bilan d'une annulation.
#[derive(Clone, Debug, Default)]
pub struct UndoOutcome {
    pub files_put_back: usize,
    pub files_removed: usize,
    /// Fichiers ajoutes par la restauration puis modifies depuis : gardes (et dans la copie de securite d'avant l'annulation).
    pub kept_changed: usize,
    /// Copie de securite de l'etat juste avant l'annulation (l'annulation est elle-meme sans perte).
    pub safety_dir: Option<PathBuf>,
    /// Le coffre a retrouve sa version active d'avant (nouvelle version, rien n'est supprime).
    pub vault_restored: bool,
    /// Le coffre n'a pas pu etre remis (le local, lui, l'est) : la prochaine synchro copiera l'etat local.
    pub vault_error: Option<String>,
}

fn push_missing_dirs(p: &Path, dirs: &mut Vec<PathBuf>) {
    let mut cur = Some(p);
    while let Some(d) = cur {
        if d.as_os_str().is_empty() || std::fs::symlink_metadata(d).is_ok() {
            break;
        }
        if !dirs.iter().any(|x| x == d) {
            dirs.push(d.to_path_buf());
        }
        cur = d.parent();
    }
}

/// Fichiers et dossiers que l'ecriture de `files` (chemins de la sauvegarde) creerait : absents maintenant.
fn would_create(target: &store::GameTarget, files: &[store::FileInfo]) -> (Vec<(PathBuf, String)>, Vec<PathBuf>) {
    let mut created = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for r in target.roots() {
        push_missing_dirs(&r, &mut dirs);
    }
    for f in files {
        if let Some(lp) = target.local_path(&f.path) {
            if std::fs::symlink_metadata(&lp).is_err() {
                if let Some(parent) = lp.parent() {
                    push_missing_dirs(parent, &mut dirs);
                }
                created.push((lp, f.hash.clone()));
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    (created, dirs)
}

fn ticket_for(ctx: &Ctx, game: &Game, files: &[store::FileInfo], prev_active: Option<String>) -> Res<UndoTicket> {
    let target = store::GameTarget::of(game);
    let (created, created_dirs) = would_create(&target, files);
    let st = State::load(ctx)?;
    Ok(UndoTicket {
        game: game.name.clone(),
        safety_dir: None,
        created,
        created_dirs,
        prev_synced: st.last_synced.get(&game.name).cloned(),
        prev_id: st.last_id.get(&game.name).cloned(),
        prev_sync_ms: st.last_sync_ms.get(&game.name).copied(),
        prev_active,
    })
}

/// « Remettre cette version » (Remonter le temps) : `restore_version` (programme ferme, source verifiee, copie de securite
/// AVANT) + tout ce qu'il faut pour l'annuler ensuite a l'identique (`undo_restore`).
pub fn restore_version_undoable(ctx: &Ctx, game: &Game, dest: &Path, version: &str) -> Res<(RestoreOutcome, UndoTicket)> {
    check_not_running(ctx, game)?;
    let snap = cloud::scan_dest(dest, game);
    let v = snap.versions.iter().find(|v| v.name == version).ok_or_else(|| format!("version '{}' introuvable (voir `versions`)", version))?.clone();
    let target = store::GameTarget::of(game);
    let files: Vec<store::FileInfo> = v.meta.files.iter().filter(|f| target.accepts(&f.path)).cloned().collect();
    // Meme condition que `restore_version` : une nouvelle version active est creee si la version choisie n'est pas l'active.
    let prev_active = active_version(&snap).filter(|a| a.dir != v.dir && a.meta.global_hash != v.meta.global_hash).map(|a| a.name);
    let mut ticket = ticket_for(ctx, game, &files, prev_active)?;
    let out = restore_version(ctx, game, dest, version)?;
    ticket.safety_dir = out.safety_dir.clone();
    Ok((out, ticket))
}

/// Restauration de la version ACTIVE (nouveau PC) : `restore` (programme ferme, source verifiee, copie de securite AVANT)
/// + de quoi l'annuler a l'identique (`undo_restore`). Le coffre n'est pas modifie (aucune nouvelle version).
pub fn restore_undoable(ctx: &Ctx, game: &Game, dest: &Path) -> Res<(RestoreOutcome, UndoTicket)> {
    check_not_running(ctx, game)?;
    let snap = cloud::scan_dest(dest, game);
    let target = store::GameTarget::of(game);
    let files: Vec<store::FileInfo> = active_version(&snap).map(|v| v.meta.files.iter().filter(|f| target.accepts(&f.path)).cloned().collect()).unwrap_or_default();
    let mut ticket = ticket_for(ctx, game, &files, None)?;
    let out = restore(ctx, game, dest)?;
    ticket.safety_dir = out.safety_dir.clone();
    Ok((out, ticket))
}

/// Remet une copie de securite (Remonter le temps) : programme ferme, copie verifiee AVANT d'ecrire, copie de securite de
/// l'etat actuel, ecriture atomique verifiee. Rien n'est supprime ; le coffre n'est pas touche (la prochaine synchro
/// copie l'etat remis comme une modification ordinaire).
pub fn restore_safety_undoable(ctx: &Ctx, game: &Game, dir: &Path) -> Res<(RestoreOutcome, UndoTicket)> {
    let mut op = perf::Op::start(ctx, "Restauration (copie de securite)", &game.name);
    check_not_running(ctx, game)?;
    // Seulement une copie de securite de CET element (aucun chemin arbitraire).
    if dir.parent() != Some(safety_root(ctx, game).as_path()) {
        return Err("copie de securite inconnue pour cet element".into());
    }
    let meta = store::read_meta(&dir.join("meta.json"))?;
    verify_safety(dir, &meta)?;
    op.step("verification de la copie de securite");
    let target = store::GameTarget::of(game);
    let files: Vec<store::FileInfo> = meta.files.iter().filter(|f| target.accepts(&f.path)).cloned().collect();
    if files.is_empty() {
        return Err("rien a remettre : aucun fichier de cette copie ne correspond aux emplacements actuels".into());
    }
    let mut ticket = ticket_for(ctx, game, &files, None)?;
    crate::winprefs::refresh(ctx, game);
    if target.exists() {
        let (lfiles, llatest) = target.scan(ctx)?;
        if !lfiles.is_empty() {
            ticket.safety_dir = Some(safety_snapshot(ctx, game, &target, lfiles, llatest, "snapshot de securite avant de remettre une copie de securite", "rien n'a ete remis")?);
        }
    }
    if let Some(p) = &ctx.progress {
        p.check_cancel()?;
        p.lock();
        p.begin(perf::Phase::Write, files.len() as u64, files.iter().map(|f| f.size).sum());
    }
    for r in target.roots() {
        std::fs::create_dir_all(&r)?;
    }
    store::copy_files_mapped(ctx, &files, &|f| target.local_path(&f.path).map(|d| (dir.join("files").join(&f.path), d)))
        .map_err(|e| format!("remise interrompue ({}). L'etat precedent est dans {:?}", e, ticket.safety_dir))?;
    let mut st = State::load(ctx)?;
    st.note_action(&game.name, "restore");
    st.save(ctx)?;
    crate::winprefs::after_restore(ctx, game);
    op.finish();
    Ok((RestoreOutcome { safety_dir: ticket.safety_dir.clone(), files_restored: files.len(), local_had_unsynced_changes: false, local_only_kept: 0, unknown_locations: vec![] }, ticket))
}

/// ANNULE une restauration (`restore_version_undoable` / `restore_safety_undoable`) : l'etat local redevient identique,
/// octet par octet, a celui d'avant ; le coffre retrouve sa version active d'avant ; l'etat de synchro de ce PC aussi.
/// Programme ferme ; copie de securite verifiee AVANT de toucher a quoi que ce soit ; copie de securite de l'etat actuel
/// (rien n'est jamais perdu) ; un fichier ajoute par la restauration n'est retire que s'il est reste tel quel.
pub fn undo_restore(ctx: &Ctx, game: &Game, dest: Option<&Path>, t: &UndoTicket) -> Res<UndoOutcome> {
    let mut op = perf::Op::start(ctx, "Annulation d'une restauration", &game.name);
    if t.game != game.name {
        return Err("annulation : element different".into());
    }
    check_not_running(ctx, game)?;
    // 1. Tout est verifie avant d'ecrire : copie d'avant intacte, version d'avant presente dans le coffre.
    let back = match &t.safety_dir {
        Some(d) => {
            let m = store::read_meta(&d.join("meta.json")).map_err(|e| format!("copie de securite illisible ({}) : rien n'a ete modifie", e))?;
            verify_safety(d, &m)?;
            Some((d.clone(), m))
        }
        None => None,
    };
    let prev = match (&t.prev_active, dest) {
        (Some(name), Some(d)) => {
            let snap = cloud::scan_dest(d, game);
            Some(snap.versions.iter().find(|v| &v.name == name).cloned().ok_or("version d'avant introuvable dans le coffre : rien n'a ete modifie")?)
        }
        (Some(_), None) => return Err("coffre injoignable : rien n'a ete modifie".into()),
        (None, _) => None,
    };
    op.step("verifications");
    let target = store::GameTarget::of(game);
    let mut out = UndoOutcome::default();
    // 2. Copie de securite de l'etat actuel (Reglages Windows : exportes juste avant).
    crate::winprefs::refresh(ctx, game);
    if target.exists() {
        let (lf, ll) = target.scan(ctx)?;
        if !lf.is_empty() {
            out.safety_dir = Some(safety_snapshot(ctx, game, &target, lf, ll, "snapshot de securite avant annulation d'une restauration", "annulation abandonnee")?);
        }
    }
    if let Some(p) = &ctx.progress {
        p.check_cancel()?;
        p.lock();
    }
    // 3. Fichiers d'avant remis (ecriture atomique verifiee).
    let mut put_back: Vec<PathBuf> = Vec::new();
    if let Some((d, m)) = &back {
        let files: Vec<store::FileInfo> = m.files.iter().filter(|f| target.accepts(&f.path)).cloned().collect();
        if let Some(p) = &ctx.progress {
            p.begin(perf::Phase::Write, files.len() as u64, files.iter().map(|f| f.size).sum());
        }
        for r in target.roots() {
            std::fs::create_dir_all(&r)?;
        }
        store::copy_files_mapped(ctx, &files, &|f| target.local_path(&f.path).map(|dst| (d.join("files").join(&f.path), dst)))
            .map_err(|e| format!("annulation interrompue ({}). L'etat d'avant l'annulation est dans {:?}", e, out.safety_dir))?;
        put_back = files.iter().filter_map(|f| target.local_path(&f.path)).collect();
        out.files_put_back = files.len();
    }
    // 4. Fichiers ajoutes par la restauration : retires seulement s'ils n'ont pas change (ils restent dans le coffre).
    for (p, h) in &t.created {
        if put_back.contains(p) {
            continue;
        }
        match util::hash_file(p) {
            Ok(x) if &x == h => {
                if std::fs::remove_file(p).is_ok() {
                    out.files_removed += 1;
                }
            }
            Ok(_) => out.kept_changed += 1,
            Err(_) => {}
        }
    }
    for d in &t.created_dirs {
        let _ = std::fs::remove_dir(d); // seulement s'il est vide
    }
    // Reglages Windows : les valeurs d'avant sont reecrites dans le registre.
    if back.is_some() {
        crate::winprefs::after_restore(ctx, game);
    }
    op.step("fichiers d'avant remis");
    // 5. Coffre : la version active d'avant redevient la tete (nouvelle version qui en reprend le contenu verifie).
    if let (Some(p), Some(d)) = (&prev, dest) {
        let snap = cloud::scan_dest(d, game);
        let head = active_version(&snap);
        let r = if head.as_ref().map(|h| h.meta.global_hash != p.meta.global_hash).unwrap_or(true) {
            let note = format!("annulation d'une restauration : retour a la version {}", p.name);
            create_version_ex(ctx, game, d, &snap, Some(p), head.map(|h| h.id), vec![p.id.clone()], Some(note), &mut op).map(|_| {
                prune_keep(d, game, keep_for(ctx, &game.name));
            })
        } else {
            Ok(())
        };
        match r {
            Ok(()) => out.vault_restored = true,
            Err(e) => out.vault_error = Some(e.to_string()),
        }
    }
    // 6. Etat de synchro de ce PC : a jour si le local est la version d'avant, sinon exactement l'etat d'avant.
    let mut st = State::load(ctx)?;
    if out.vault_error.is_none() {
        let mut in_sync = None;
        if let (Some(p), Some(d)) = (&prev, dest) {
            let snap = cloud::scan_dest(d, game);
            let local = local_state_carry(ctx, game, &snap).ok().flatten().map(|x| x.2);
            if local.as_deref() == Some(p.meta.global_hash.as_str()) {
                in_sync = active_version(&snap).filter(|h| h.meta.global_hash == p.meta.global_hash).map(|h| (h.meta.global_hash.clone(), h.id.clone()));
            }
        }
        let g = game.name.clone();
        match in_sync {
            Some((h, id)) => {
                st.last_synced.insert(g.clone(), h);
                st.last_id.insert(g.clone(), id);
                st.last_sync_ms.insert(g.clone(), util::now_ms());
            }
            None => {
                match &t.prev_synced {
                    Some(v) => st.last_synced.insert(g.clone(), v.clone()),
                    None => st.last_synced.remove(&g),
                };
                match &t.prev_id {
                    Some(v) => st.last_id.insert(g.clone(), v.clone()),
                    None => st.last_id.remove(&g),
                };
                match t.prev_sync_ms {
                    Some(v) => st.last_sync_ms.insert(g.clone(), v),
                    None => st.last_sync_ms.remove(&g),
                };
            }
        }
    }
    st.note_action(&game.name, "undo_restore");
    st.save(ctx)?;
    op.finish();
    Ok(out)
}
