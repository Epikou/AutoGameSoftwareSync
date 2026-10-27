//! Cache des empreintes (hash) de fichiers LOCAUX, pour ne pas relire tout un dossier de sauvegardes
//! a chaque analyse. Cle : chemin complet + taille + date de modification. Dans le doute : on recalcule.
//!
//! Regles de securite :
//! - jamais utilise pour un dossier du partage cloud ni pour verifier une copie ecrite (ces hash sont
//!   toujours calcules sur les octets reellement lus / ecrits) ;
//! - une entree n'est valable que si le fichier n'a pas change (taille ET date) ET s'il etait deja
//!   « stable » quand il a ete hache (date de modification anterieure d'au moins `RACY_MS` au calcul) :
//!   un fichier modifie juste apres le calcul, dans la meme tranche d'horloge, n'est jamais confondu ;
//! - fichier de cache illisible ou version inconnue = cache vide.
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Marge (ms) entre la date de modification d'un fichier et le moment ou on l'a hache.
pub const RACY_MS: u64 = 2_000;
const MAX_ENTRIES: usize = 50_000;
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Entry {
    /// taille
    s: u64,
    /// date de modification (ms)
    m: u64,
    /// moment du calcul (ms)
    t: u64,
    /// hash
    h: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Disk {
    v: u32,
    entries: HashMap<String, Entry>,
}

struct Inner {
    loaded: bool,
    map: HashMap<String, Entry>,
    dirty: bool,
}

pub struct HashCache {
    path: PathBuf,
    state: Mutex<Inner>,
}

fn key(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Entree valable ? (fonction pure, testee)
pub fn entry_valid(size: u64, mtime_ms: u64, e_size: u64, e_mtime: u64, e_hashed_at: u64) -> bool {
    size == e_size && mtime_ms == e_mtime && e_hashed_at >= e_mtime.saturating_add(RACY_MS)
}

impl HashCache {
    pub fn new(path: PathBuf) -> Self {
        HashCache { path, state: Mutex::new(Inner { loaded: false, map: HashMap::new(), dirty: false }) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !g.loaded {
            g.loaded = true;
            g.map = std::fs::read_to_string(&self.path)
                .ok()
                .and_then(|s| serde_json::from_str::<Disk>(&s).ok())
                .filter(|d| d.v == VERSION)
                .map(|d| d.entries)
                .unwrap_or_default();
        }
        f(&mut g)
    }

    /// Hash en cache pour ce fichier, SEULEMENT s'il est sur et a jour.
    pub fn lookup(&self, p: &Path, size: u64, mtime_ms: u64) -> Option<String> {
        self.with(|i| i.map.get(&key(p)).filter(|e| entry_valid(size, mtime_ms, e.s, e.m, e.t)).map(|e| e.h.clone()))
    }

    /// Memorise un hash calcule (sur les octets lus) a l'instant `now_ms`.
    pub fn store(&self, p: &Path, size: u64, mtime_ms: u64, hash: &str, now_ms: u64) {
        self.with(|i| {
            if i.map.len() >= MAX_ENTRIES {
                i.map.clear();
            }
            i.map.insert(key(p), Entry { s: size, m: mtime_ms, t: now_ms, h: hash.to_string() });
            i.dirty = true;
        });
    }

    /// Oublie un fichier (doute, copie dont le hash ne correspondait pas...).
    pub fn forget(&self, p: &Path) {
        self.with(|i| {
            if i.map.remove(&key(p)).is_some() {
                i.dirty = true;
            }
        });
    }

    pub fn len(&self) -> usize {
        self.with(|i| i.map.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ecrit le cache s'il a change (best effort : un echec ne gene rien).
    pub fn save(&self) {
        let snapshot = self.with(|i| {
            if !i.dirty {
                return None;
            }
            i.dirty = false;
            Some(Disk { v: VERSION, entries: i.map.clone() })
        });
        if let Some(disk) = snapshot {
            if let Ok(s) = serde_json::to_string(&disk) {
                let _ = util::write_atomic(&self.path, s.as_bytes());
            }
        }
    }
}
