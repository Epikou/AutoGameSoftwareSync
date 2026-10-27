//! Apercu AVANT une restauration : fichiers qui seront AJOUTES / REMPLACES / IDENTIQUES, fichiers seulement locaux
//! (conserves, jamais supprimes) et fichiers dont la version locale est PLUS RECENTE que la sauvegarde cloud.
//! Calcul par comparaison du scan local (cache d'empreintes) avec la liste + empreintes du meta.json de la version
//! choisie : le contenu du cloud n'est PAS relu.
use crate::backup;
use crate::cloud::{self, Version};
use crate::config::Game;
use crate::store::{self, FileInfo};
use crate::{Ctx, Res};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Absent en local : sera ajoute.
    Added,
    /// Contenu different : sera remplace (apres copie de securite).
    Replaced,
    /// Remplace, ET la version locale est plus recente que la sauvegarde cloud : a regarder de pres.
    LocalNewer,
    /// Meme contenu : rien a faire.
    Identical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub kind: DiffKind,
    pub cloud_size: u64,
    pub local_size: Option<u64>,
    pub local_mtime_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    /// Fichiers de la sauvegarde cloud (ajoutes, remplaces, plus recents en local en premier ; identiques ensuite).
    pub files: Vec<FileDiff>,
    /// Fichiers presents seulement en local : conserves (jamais supprimes).
    pub local_only: Vec<String>,
    /// Date de la sauvegarde cloud (ms, heure corrigee).
    pub cloud_time_ms: u64,
    pub added: usize,
    pub replaced: usize,
    pub local_newer: usize,
    pub identical: usize,
    pub added_bytes: u64,
    pub replaced_bytes: u64,
    /// PC qui a fait la sauvegarde cloud (vide = inconnu).
    pub cloud_machine: String,
}

/// Calcul pur. `cloud` : fichiers du meta.json ; `local` : scan local avec dates (ms) ; `cloud_time_ms` : date de la sauvegarde.
pub fn compute(cloud: &[FileInfo], cloud_time_ms: u64, local: &[(FileInfo, u64)]) -> Preview {
    let by_path: std::collections::HashMap<&str, &(FileInfo, u64)> = local.iter().map(|l| (l.0.path.as_str(), l)).collect();
    let cloud_paths: std::collections::HashSet<&str> = cloud.iter().map(|f| f.path.as_str()).collect();
    let mut p = Preview { cloud_time_ms, ..Default::default() };
    for f in cloud {
        let (kind, ls, lm) = match by_path.get(f.path.as_str()) {
            None => (DiffKind::Added, None, None),
            Some((lf, mt)) if lf.hash == f.hash => (DiffKind::Identical, Some(lf.size), Some(*mt)),
            Some((lf, mt)) => (if *mt > cloud_time_ms { DiffKind::LocalNewer } else { DiffKind::Replaced }, Some(lf.size), Some(*mt)),
        };
        match kind {
            DiffKind::Added => {
                p.added += 1;
                p.added_bytes += f.size;
            }
            DiffKind::Replaced => {
                p.replaced += 1;
                p.replaced_bytes += f.size;
            }
            DiffKind::LocalNewer => {
                p.replaced += 1;
                p.local_newer += 1;
                p.replaced_bytes += f.size;
            }
            DiffKind::Identical => p.identical += 1,
        }
        p.files.push(FileDiff { path: f.path.clone(), kind, cloud_size: f.size, local_size: ls, local_mtime_ms: lm });
    }
    // Les plus importants d'abord : plus recents en local, remplaces, ajoutes, puis identiques.
    let rank = |k: DiffKind| match k {
        DiffKind::LocalNewer => 0,
        DiffKind::Replaced => 1,
        DiffKind::Added => 2,
        DiffKind::Identical => 3,
    };
    p.files.sort_by(|a, b| rank(a.kind).cmp(&rank(b.kind)).then_with(|| a.path.cmp(&b.path)));
    p.local_only = local.iter().filter(|l| !cloud_paths.contains(l.0.path.as_str())).map(|l| l.0.path.clone()).collect();
    p.local_only.sort();
    p
}

pub fn for_version(ctx: &Ctx, game: &Game, v: &Version) -> Res<Preview> {
    let target = store::GameTarget::of(game);
    let local = if target.exists() { target.scan_mt(ctx)?.0 } else { vec![] };
    let cloud: Vec<store::FileInfo> = v.meta.files.iter().filter(|f| target.accepts(&f.path)).cloned().collect();
    let mut p = compute(&cloud, v.meta.sort_ms(), &local);
    p.cloud_machine = v.meta.machine.clone();
    Ok(p)
}

/// Apercu de la restauration de `version` (nom exact) ou, sans nom, de la version active (la plus recente des tetes).
pub fn for_game(ctx: &Ctx, game: &Game, dest: &Path, version: Option<&str>) -> Option<Preview> {
    let snap = cloud::scan_dest(dest, game);
    let v = match version {
        Some(n) => snap.versions.iter().find(|v| v.name == n).cloned(),
        None => backup::active_version(&snap),
    }?;
    for_version(ctx, game, &v).ok()
}

/// Apercu de la remise d'une copie de securite (dossier local `safety/<jeu>/<horodatage>`, meme calcul).
pub fn for_safety(ctx: &Ctx, game: &Game, dir: &Path) -> Option<Preview> {
    let meta = store::read_meta(&dir.join("meta.json")).ok()?;
    let target = store::GameTarget::of(game);
    let local = if target.exists() { target.scan_mt(ctx).ok()?.0 } else { vec![] };
    let cloud: Vec<store::FileInfo> = meta.files.iter().filter(|f| target.accepts(&f.path)).cloned().collect();
    let mut p = compute(&cloud, meta.sort_ms(), &local);
    p.cloud_machine = meta.machine.clone();
    Some(p)
}

/// Resume court pour le Journal : « N ajoutes, M remplaces ».
pub fn journal_summary(p: &Preview) -> String {
    format!("{} ajoutes, {} remplaces ({} identiques, {} seulement locaux conserves{})", p.added, p.replaced, p.identical, p.local_only.len(), if p.local_newer > 0 { format!(", dont {} plus recents en local", p.local_newer) } else { String::new() })
}
