//! Base des jeux EMBARQUEE dans l'exe (`data/gamedb.bin`, generee a chaque publication par `examples/build_gamedb.rs`) :
//! emplacements de sauvegarde Windows, ids Steam / GOG, dossiers d'installation, exe de lancement.
//! Format : en-tete `AGDB1` + deflate(postcard(Db)). Decompression PARESSEUSE (thread), index en memoire, couche locale
//! d'overrides (`gamedb-overrides.json`, dossier de donnees) prioritaire sur la base embarquee.
//!
//! Ordre de priorite logique pour retrouver un jeu a partir d'un exe :
//! 1. overrides locales de l'utilisateur / de l'appli (cette couche) ;
//! 2. indices du dossier partage (`hints`, ecrits par les autres PC) ;
//! 3. base embarquee ;
//! 4. (futur, NON active) sources de repli : voir `FallbackSource`.
use crate::catalog::{Entry, FileEntry, Catalog, Steam};
use crate::Res;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, OnceLock};

const MAGIC: &[u8; 5] = b"AGDB1";
pub const FORMAT: u32 = 1;
/// Nom du fichier d'overrides dans le dossier de donnees.
pub const OVERRIDES_FILE: &str = "gamedb-overrides.json";

static EMBEDDED: &[u8] = include_bytes!("../data/gamedb.bin");

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DbGame {
    pub name: String,
    #[serde(default)]
    pub steam: Option<u64>,
    #[serde(default)]
    pub gog: Option<u64>,
    /// Dossiers d'installation (nom de dossier sous steamapps\common ou equivalent).
    #[serde(default)]
    pub install_dirs: Vec<String>,
    /// Noms de fichier des exe de lancement (.exe).
    #[serde(default)]
    pub exes: Vec<String>,
    /// Emplacements de sauvegarde Windows (placeholders `<base>`, `<winLocalAppData>`...).
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Db {
    pub format: u32,
    /// Date de generation (AAAA-MM-JJ).
    pub generated: String,
    pub games: Vec<DbGame>,
}

pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn exe_name(s: &str) -> String {
    s.trim().trim_matches('"').rsplit(['/', '\\']).next().unwrap_or("").trim().to_lowercase()
}

// ------------------------------------------------------------------ encodage / decodage

pub fn encode(db: &Db) -> Res<Vec<u8>> {
    let raw = postcard::to_allocvec(db).map_err(|e| format!("encodage : {}", e))?;
    let mut out = MAGIC.to_vec();
    let mut enc = flate2::write::DeflateEncoder::new(&mut out, flate2::Compression::best());
    enc.write_all(&raw)?;
    enc.finish()?;
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Res<Db> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err("base des elements : en-tete inconnu".into());
    }
    let mut raw = Vec::new();
    flate2::read::DeflateDecoder::new(&bytes[MAGIC.len()..]).read_to_end(&mut raw).map_err(|e| format!("base des elements : decompression : {}", e))?;
    let db: Db = postcard::from_bytes(&raw).map_err(|e| format!("base des elements : {}", e))?;
    if db.format != FORMAT {
        return Err(format!("base des elements : format {} non supporte", db.format).into());
    }
    Ok(db)
}

/// Reduit un catalogue source a ce qui sert ici : Windows seulement, fichiers « save » (ou sans etiquette), pas de registre.
pub fn build_from_catalog(m: &Catalog, generated: &str) -> Db {
    let mut games = Vec::new();
    for (name, e) in m {
        let mut paths: Vec<String> = e
            .files
            .iter()
            .filter(|(_, fe)| match fe {
                None => true,
                Some(fe) => {
                    let win = fe.when.is_empty() || fe.when.iter().any(|w| w.os.as_deref().map_or(true, |o| o == "windows"));
                    let save = fe.tags.is_empty() || fe.tags.iter().any(|t| t == "save");
                    win && save
                }
            })
            .map(|(p, _)| p.clone())
            .collect();
        if paths.is_empty() {
            continue;
        }
        paths.sort();
        let mut exes: Vec<String> = e.launch.keys().filter(|k| k.to_lowercase().ends_with(".exe")).map(|k| exe_name(k)).filter(|k| !k.is_empty()).collect();
        exes.sort();
        exes.dedup();
        let mut install_dirs: Vec<String> = e.install_dir.keys().cloned().collect();
        install_dirs.sort();
        games.push(DbGame { name: name.clone(), steam: e.steam.as_ref().and_then(|s| s.id), gog: e.gog.as_ref().and_then(|s| s.id), install_dirs, exes, paths });
    }
    Db { format: FORMAT, generated: generated.to_string(), games }
}

// ------------------------------------------------------------------ base en memoire + index

pub struct GameDb {
    pub generated: String,
    games: Vec<DbGame>,
    by_exe: HashMap<String, Vec<usize>>,
    by_install: HashMap<String, Vec<usize>>,
    by_steam: HashMap<u64, Vec<usize>>,
    by_gog: HashMap<u64, Vec<usize>>,
    by_name: HashMap<String, Vec<usize>>,
    /// Nombre d'entrees venant des overrides locales.
    pub override_count: usize,
}

impl GameDb {
    pub fn from_db(db: Db, overrides: Vec<DbGame>) -> GameDb {
        let mut games: Vec<DbGame> = Vec::new();
        // Les overrides passent EN PREMIER (priorite) et remplacent une entree de meme nom normalise.
        let over_names: std::collections::HashSet<String> = overrides.iter().map(|g| norm(&g.name)).collect();
        let override_count = overrides.len();
        games.extend(overrides);
        games.extend(db.games.into_iter().filter(|g| !over_names.contains(&norm(&g.name))));
        let mut by_exe: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_install: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_steam: HashMap<u64, Vec<usize>> = HashMap::new();
        let mut by_gog: HashMap<u64, Vec<usize>> = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, g) in games.iter().enumerate() {
            for x in &g.exes {
                by_exe.entry(exe_name(x)).or_default().push(i);
            }
            for d in &g.install_dirs {
                by_install.entry(d.trim().to_lowercase()).or_default().push(i);
            }
            if let Some(id) = g.steam {
                by_steam.entry(id).or_default().push(i);
            }
            if let Some(id) = g.gog {
                by_gog.entry(id).or_default().push(i);
            }
            by_name.entry(norm(&g.name)).or_default().push(i);
        }
        GameDb { generated: db.generated, games, by_exe, by_install, by_steam, by_gog, by_name, override_count }
    }

    pub fn from_bytes(bytes: &[u8], overrides: Vec<DbGame>) -> Res<GameDb> {
        Ok(GameDb::from_db(decode(bytes)?, overrides))
    }

    pub fn len(&self) -> usize {
        self.games.len()
    }

    /// Tous les titres (overrides locales en premier) : scan du PC.
    pub fn games(&self) -> &[DbGame] {
        &self.games
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    fn pick(&self, v: Option<&Vec<usize>>) -> Vec<&DbGame> {
        v.map(|v| v.iter().map(|i| &self.games[*i]).collect()).unwrap_or_default()
    }

    pub fn find_by_exe(&self, exe: &str) -> Vec<&DbGame> {
        self.pick(self.by_exe.get(&exe_name(exe)))
    }

    /// Nom de dossier d'installation (insensible a la casse).
    pub fn find_by_install_dir(&self, dir: &str) -> Vec<&DbGame> {
        self.pick(self.by_install.get(&dir.trim().to_lowercase()))
    }

    pub fn find_by_steam(&self, id: u64) -> Vec<&DbGame> {
        self.pick(self.by_steam.get(&id))
    }

    pub fn find_by_gog(&self, id: u64) -> Vec<&DbGame> {
        self.pick(self.by_gog.get(&id))
    }

    pub fn find_by_name(&self, name: &str) -> Vec<&DbGame> {
        self.pick(self.by_name.get(&norm(name)))
    }

    /// Recherche par sous-chaine du nom (page « Ajouter » : recherche par nom).
    pub fn search(&self, q: &str, limit: usize) -> Vec<&DbGame> {
        let ql = q.to_lowercase();
        let mut exact: Vec<&DbGame> = Vec::new();
        let mut part: Vec<&DbGame> = Vec::new();
        for g in &self.games {
            let l = g.name.to_lowercase();
            if l == ql {
                exact.push(g);
            } else if l.contains(&ql) {
                part.push(g);
            }
        }
        exact.extend(part);
        exact.truncate(limit);
        exact
    }

    /// Vue `Catalog` (resolveur, detection, recherche existants).
    pub fn to_catalog(&self) -> Catalog {
        self.games.iter().map(|g| (g.name.clone(), g.to_entry())).collect()
    }
}

impl DbGame {
    pub fn to_entry(&self) -> Entry {
        Entry {
            files: self.paths.iter().map(|p| (p.clone(), None::<FileEntry>)).collect(),
            install_dir: self.install_dirs.iter().map(|d| (d.clone(), serde::de::IgnoredAny)).collect(),
            steam: self.steam.map(|id| Steam { id: Some(id) }),
            gog: self.gog.map(|id| Steam { id: Some(id) }),
            launch: self.exes.iter().map(|x| (x.clone(), serde::de::IgnoredAny)).collect(),
        }
    }
}

/// Overrides locales (JSON : tableau de `DbGame`). Fichier absent ou invalide : aucune (jamais une erreur bloquante).
pub fn load_overrides(data_dir: &Path) -> Vec<DbGame> {
    std::fs::read_to_string(data_dir.join(OVERRIDES_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Point d'extension (NON active) : source de repli interrogee quand ni les overrides, ni les indices du dossier partage,
/// ni la base embarquee ne connaissent l'exe (futur repli en ligne, future mise a jour silencieuse du fichier de
/// donnees depuis le depot public de l'utilisateur). Rien n'implemente `FallbackSource` pour l'instant.
pub trait FallbackSource: Send + Sync {
    fn name(&self) -> &'static str;
    fn find_by_exe(&self, exe: &str) -> Vec<DbGame>;
}

// ------------------------------------------------------------------ instance globale (decompression paresseuse)

static GLOBAL: OnceLock<Arc<GameDb>> = OnceLock::new();

/// Base embarquee + overrides du dossier de donnees. La premiere utilisation decompresse (quelques dizaines de ms) :
/// a appeler dans un thread (voir `preload`). Les overrides sont lues a cette premiere utilisation.
pub fn embedded(data_dir: &Path) -> Arc<GameDb> {
    GLOBAL
        .get_or_init(|| {
            let overrides = load_overrides(data_dir);
            Arc::new(GameDb::from_bytes(EMBEDDED, overrides).unwrap_or_else(|_| GameDb::from_db(Db { format: FORMAT, generated: String::new(), games: vec![] }, vec![])))
        })
        .clone()
}

/// Demarre la decompression en arriere-plan (l'interface ne bloque jamais).
pub fn preload(data_dir: &Path) {
    let d = data_dir.to_path_buf();
    std::thread::spawn(move || {
        let _ = embedded(&d);
    });
}

/// Taille de la base embarquee (octets compresses).
pub fn embedded_size() -> usize {
    EMBEDDED.len()
}
