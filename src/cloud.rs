//! Destination cloud (Google Drive en mode miroir) : disposition, noms de jeux, lecture des versions.
//!
//! Disposition (cloud-safe : RIEN n'est jamais renomme, deplace ni ecrase dans la destination) :
//!   <dest>/<cle_jeu>/game.json                       nom d'affichage d'origine + cle (ecrit une fois)
//!   <dest>/<cle_jeu>/saves/<AAAAMMJJ-HHMMSS-mmm>-<machine>[-xxxx]/{files/..., meta.json}
//!       un dossier par sauvegarde, ecrit UNE fois ; meta.json est ecrit EN DERNIER (marqueur de
//!       completude). Un dossier sans meta.json valide, ou dont des fichiers manquent, est
//!       « incomplet » (peut-etre en cours de telechargement par Drive) : ignore, jamais supprime.
//! Ancien format, lu en lecture seule (jamais migre, renomme ni supprime) :
//!   <dest>/<jeu>/current, `current (N)` (copies de conflit Drive), versions/<horodatage>/ ;
//!   les dossiers `.tmp-*` sont ignores (signales).
use crate::config::Game;
use crate::store::{self, Meta};
use crate::util;
use std::path::{Path, PathBuf};

pub const SAVES_DIR: &str = "saves";
pub const GAME_JSON: &str = "game.json";

// ------------------------------------------------------------------ noms de jeux

/// Cle de dossier normalisee : minuscules, sans espaces, ponctuation ni accents.
/// « Field of Mistria », « field of mistria », « fieldofmistria » => `fieldofmistria`.
pub fn game_key(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(|c| c.to_lowercase()) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            'ß' => out.push_str("ss"),
            c if c.is_alphanumeric() => out.push(c),
            _ => {}
        }
    }
    if out.is_empty() {
        return "jeu".into();
    }
    out.chars().take(80).collect()
}

/// Cle d'ecriture d'un jeu sur CE PC : alias local (liaison) ou cle normalisee du nom.
pub fn dest_key(game: &Game) -> String {
    match game.dest_key.as_deref().filter(|k| !k.trim().is_empty()) {
        Some(k) => game_key(k),
        None => game_key(&game.name),
    }
}

/// Mots normalises d'un nom (pour la comparaison « meme jeu, mots dans un autre ordre »).
fn words(name: &str) -> Vec<String> {
    let mut v: Vec<String> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(game_key).collect();
    v.sort();
    v
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Similarite normalisee (0..1) entre deux noms de jeu : meilleure de (cles, mots tries).
pub fn similarity(a: &str, b: &str) -> f32 {
    let (ka, kb) = (game_key(a), game_key(b));
    if ka == kb {
        return 1.0;
    }
    let sim = |x: &str, y: &str| {
        let m = x.chars().count().max(y.chars().count()).max(1);
        1.0 - levenshtein(x, y) as f32 / m as f32
    };
    let direct = sim(&ka, &kb);
    // Meme jeu avec les mots dans un autre ordre.
    let (wa, wb) = (words(a).concat(), words(b).concat());
    direct.max(sim(&wa, &wb))
}

/// Seuil au-dela duquel deux noms differents sont consideres « ressemblants » (mysteria / mistria...).
pub const SIMILARITY_THRESHOLD: f32 = 0.75;

/// Jeu deja present dans la destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestGame {
    pub key: String,
    /// Nom d'affichage d'origine (game.json) sinon nom du dossier.
    pub name: String,
    pub dirs: Vec<PathBuf>,
}

fn looks_like_game_dir(d: &Path) -> bool {
    d.join(SAVES_DIR).is_dir() || d.join("current").is_dir() || d.join("versions").is_dir() || has_current_copy(d)
}

fn has_current_copy(d: &Path) -> bool {
    std::fs::read_dir(d).map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("current ("))).unwrap_or(false)
}

/// Lit le nom d'affichage d'origine dans `game.json`.
pub fn read_game_json(root: &Path) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join(GAME_JSON)).ok()?).ok()?;
    Some((v.get("name")?.as_str()?.to_string(), v.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string()))
}

/// Jeux deja presents dans la destination (dossiers contenant `saves/` ou l'ancien format),
/// regroupes par cle normalisee (« Field of Mistria » et `fieldofmistria` = un seul jeu).
pub fn list_dest_games(dest: &Path) -> Vec<DestGame> {
    let mut map: std::collections::BTreeMap<String, DestGame> = std::collections::BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(dest) else { return vec![] };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        let fname = e.file_name().to_string_lossy().to_string();
        if !p.is_dir() || fname.starts_with('.') || !looks_like_game_dir(&p) {
            continue;
        }
        let key = game_key(&fname);
        let display = read_game_json(&p).map(|x| x.0).unwrap_or_else(|| fname.clone());
        let g = map.entry(key.clone()).or_insert(DestGame { key, name: display.clone(), dirs: vec![] });
        // Le nom venant d'un game.json prime sur un simple nom de dossier.
        if read_game_json(&p).is_some() {
            g.name = display;
        }
        g.dirs.push(p);
    }
    map.into_values().collect()
}

/// Jeux de la destination dont le nom ressemble a `name` (sans etre la meme cle), du plus proche au moins proche.
pub fn similar_dest_games(name: &str, existing: &[DestGame]) -> Vec<(DestGame, f32)> {
    let key = game_key(name);
    let mut v: Vec<(DestGame, f32)> = existing
        .iter()
        .filter(|g| g.key != key)
        .map(|g| (g.clone(), similarity(name, &g.name).max(similarity(name, &g.key))))
        .filter(|(_, s)| *s >= SIMILARITY_THRESHOLD)
        .collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    v
}

/// Le jeu existe-t-il deja dans la destination sous la meme cle normalisee ?
pub fn same_key_dest_game(name: &str, existing: &[DestGame]) -> Option<DestGame> {
    let key = game_key(name);
    existing.iter().find(|g| g.key == key).cloned()
}

// ------------------------------------------------------------------ versions

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VKind {
    /// `saves/<horodatage>-<machine>` (disposition actuelle).
    New,
    /// ancien `current`.
    LegacyCurrent,
    /// `current (1)`, `current (2)`... : copies de conflit creees par Google Drive.
    LegacyConflictCopy,
    /// ancien `versions/<horodatage>`.
    LegacyVersion,
}

impl VKind {
    pub fn is_legacy(self) -> bool {
        self != VKind::New
    }
}

#[derive(Clone, Debug)]
pub struct Version {
    /// Nom utilisable par `restore --version` : dossier (nouveau format) ou chemin relatif
    /// (`current`, `current (1)`, `versions/20261022-...`).
    pub name: String,
    pub dir: PathBuf,
    pub kind: VKind,
    pub meta: Meta,
    pub id: String,
}

impl Version {
    pub fn sort_ms(&self) -> u64 {
        self.meta.sort_ms()
    }
}

#[derive(Clone, Debug)]
pub struct Incomplete {
    pub dir: PathBuf,
    pub reason: String,
}

/// Contenu de la destination pour un jeu.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Versions completes (nouveau format + ancien format), non triees.
    pub versions: Vec<Version>,
    pub incomplete: Vec<Incomplete>,
    /// Dossiers `.tmp-*` de la destination (ignores, jamais lus ni supprimes).
    pub tmp_dirs: Vec<PathBuf>,
    pub roots: Vec<PathBuf>,
}

/// Dossiers de la destination qui concernent ce jeu : tout dossier dont le nom normalise vaut la
/// cle d'ecriture, la cle du nom, ou une cle d'alias ; le dossier d'ecriture `<dest>/<cle>` en premier.
pub fn roots_for(dest: &Path, game: &Game) -> Vec<PathBuf> {
    let write = dest.join(dest_key(game));
    let mut keys = vec![dest_key(game), game_key(&game.name)];
    keys.dedup();
    let mut roots = vec![write.clone()];
    if let Ok(rd) = std::fs::read_dir(dest) {
        let mut others: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
            .filter(|e| keys.contains(&game_key(&e.file_name().to_string_lossy())) && e.path() != write)
            .map(|e| e.path())
            .collect();
        others.sort();
        roots.extend(others);
    }
    roots
}

/// Verification RAPIDE : meta valide + chaque fichier annonce existe avec la bonne taille.
/// (Le hash complet est verifie a la restauration, voir `verify_hashes`.)
fn check_complete(dir: &Path) -> Result<Meta, String> {
    let meta = store::read_meta(&dir.join("meta.json")).map_err(|e| format!("meta.json absent ou invalide ({})", e))?;
    for f in &meta.files {
        match std::fs::metadata(dir.join("files").join(&f.path)) {
            Ok(m) if m.len() == f.size => {}
            Ok(_) => return Err(format!("{} : taille differente de celle annoncee", f.path)),
            Err(_) => return Err(format!("{} : fichier manquant", f.path)),
        }
    }
    Ok(meta)
}

/// Hash de CHAQUE fichier relu et compare a meta.json.
pub fn verify_hashes(v: &Version) -> Result<(), String> {
    for f in &v.meta.files {
        let h = util::hash_file(&v.dir.join("files").join(&f.path)).map_err(|e| format!("{} : {}", f.path, e))?;
        if h != f.hash {
            return Err(format!("{} ne correspond pas a son hash", f.path));
        }
    }
    Ok(())
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// Nom de dossier conforme a la disposition `saves/` : `AAAAMMJJ-HHMMSS-mmm-<machine>[-xxxx]`.
pub fn is_save_dir_name(name: &str) -> bool {
    util::parse_timestamp(name).is_some() && name.len() > 20 && name.as_bytes()[19] == b'-'
}

/// Scanne la destination pour ce jeu (lecture seule).
pub fn scan_dest(dest: &Path, game: &Game) -> Snapshot {
    let mut snap = Snapshot { roots: roots_for(dest, game), ..Default::default() };
    let roots = snap.roots.clone();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        // Nouveau format.
        for d in subdirs(&root.join(SAVES_DIR)) {
            let fname = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if fname.starts_with(".tmp-") {
                snap.tmp_dirs.push(d);
                continue;
            }
            if !is_save_dir_name(&fname) {
                continue; // pas un dossier cree par AutoGameSoftwareSync : on n'y touche pas
            }
            match check_complete(&d) {
                Ok(meta) => {
                    let id = meta.version_id();
                    snap.versions.push(Version { name: fname, dir: d, kind: VKind::New, meta, id });
                }
                Err(reason) => snap.incomplete.push(Incomplete { dir: d, reason }),
            }
        }
        // Ancien format : current, current (N), versions/*, .tmp-*
        for d in subdirs(&root) {
            let fname = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if fname.starts_with(".tmp-") {
                snap.tmp_dirs.push(d);
                continue;
            }
            let kind = if fname == "current" {
                VKind::LegacyCurrent
            } else if fname.starts_with("current (") {
                VKind::LegacyConflictCopy
            } else {
                continue;
            };
            match check_complete(&d) {
                Ok(meta) => {
                    let id = meta.version_id();
                    snap.versions.push(Version { name: fname, dir: d, kind, meta, id });
                }
                Err(reason) => snap.incomplete.push(Incomplete { dir: d, reason }),
            }
        }
        for d in subdirs(&root.join("versions")) {
            let fname = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            match check_complete(&d) {
                Ok(meta) => {
                    let id = meta.version_id();
                    snap.versions.push(Version { name: format!("versions/{}", fname), dir: d, kind: VKind::LegacyVersion, meta, id });
                }
                Err(reason) => snap.incomplete.push(Incomplete { dir: d, reason }),
            }
        }
    }
    snap
}

impl Snapshot {
    /// Versions du nouveau format, de la plus ancienne a la plus recente.
    pub fn new_versions(&self) -> Vec<&Version> {
        let mut v: Vec<&Version> = self.versions.iter().filter(|v| v.kind == VKind::New).collect();
        v.sort_by(|a, b| (a.sort_ms(), &a.name).cmp(&(b.sort_ms(), &b.name)));
        v
    }

    /// Versions servant a la DECISION de synchro : le nouveau format s'il existe ; sinon (pas encore
    /// de migration) la meilleure version de l'ancien format (`current` si valide, sinon la plus recente),
    /// qui joue le role de tete unique.
    pub fn logic_versions(&self) -> Vec<&Version> {
        let nv = self.new_versions();
        if !nv.is_empty() {
            return nv;
        }
        let legacy: Vec<&Version> = self.versions.iter().filter(|v| v.kind.is_legacy()).collect();
        if let Some(c) = legacy.iter().find(|v| v.kind == VKind::LegacyCurrent) {
            return vec![*c];
        }
        legacy.into_iter().max_by_key(|v| (v.sort_ms(), v.name.clone())).into_iter().collect()
    }
}

// ------------------------------------------------------------------ controles legers (reactivite)

fn mix(h: &mut u64, v: u64) {
    *h = (*h ^ v).wrapping_mul(0x100000001b3);
}

fn mix_str(h: &mut u64, s: &str) {
    for b in s.bytes() {
        mix(h, b as u64);
    }
    mix(h, 0xff);
}

/// Sauvegarde distante complete ? `Incomplete(sig)` : des dossiers annonces mais pas encore entierement la
/// (meta.json livre avant les autres fichiers, par exemple) ; `sig` change quand des octets arrivent.
pub fn remote_check(dest: &Path, game: &Game) -> crate::reactive::Check {
    let snap = scan_dest(dest, game);
    if snap.incomplete.is_empty() {
        return crate::reactive::Check::Complete;
    }
    let mut h = 0xcbf29ce484222325u64;
    for inc in &snap.incomplete {
        mix_str(&mut h, &inc.dir.to_string_lossy());
        for e in walkdir::WalkDir::new(&inc.dir).follow_links(false).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
            mix(&mut h, e.metadata().map(|m| m.len()).unwrap_or(0));
            mix(&mut h, 1);
        }
    }
    crate::reactive::Check::Incomplete(h)
}

/// Empreinte RAPIDE de l'etat distant d'un jeu : noms des dossiers de sauvegarde et taille + date de leur meta.json.
/// Aucun fichier de donnees n'est lu : sert a eviter tout calcul quand rien n'a change (filet de secours).
pub fn quick_signature(dest: &Path, game: &Game) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for root in roots_for(dest, game) {
        mix_str(&mut h, &root.to_string_lossy());
        let mut dirs: Vec<PathBuf> = subdirs(&root);
        let saves = root.join(SAVES_DIR);
        if saves.is_dir() {
            dirs.extend(subdirs(&saves));
        }
        dirs.sort();
        for d in dirs {
            mix_str(&mut h, &d.to_string_lossy());
            if let Ok(m) = std::fs::metadata(d.join("meta.json")) {
                mix(&mut h, m.len());
                mix(&mut h, m.modified().map(crate::util::mtime_ms).unwrap_or(0));
            }
        }
    }
    h
}

/// Signature LEGERE pour l interrogation directe : noms des sous-dossiers de `saves/` + presence de meta.json des plus
/// recents (aucune lecture de fichier, aucun hash). None = dossier partage injoignable (la derniere valeur est conservee).
pub fn poll_signature(dest: &Path, game: &Game) -> Option<u64> {
    if !dest.is_dir() {
        return None;
    }
    let mut h = 0xcbf29ce484222325u64;
    for root in roots_for(dest, game) {
        mix_str(&mut h, &root.to_string_lossy());
        let mut dirs: Vec<PathBuf> = subdirs(&root.join(SAVES_DIR));
        dirs.sort();
        let n = dirs.len();
        for (i, d) in dirs.iter().enumerate() {
            mix_str(&mut h, &d.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            if i + 5 >= n {
                mix(&mut h, d.join("meta.json").is_file() as u64);
            }
        }
    }
    Some(h)
}

/// Indices (exe, nom, dossier de sauvegarde) de chaque jeu du dossier partage : lit SEULEMENT le meta.json de la version
/// la plus recente VALIDE (complete) de chaque jeu, sans hash ni analyse complete. A appeler dans un thread.
pub fn latest_hints(dest: &Path) -> Vec<crate::hints::Hint> {
    let mut out = Vec::new();
    for dg in list_dest_games(dest) {
        'game: for root in &dg.dirs {
            let saves = root.join(SAVES_DIR);
            let mut dirs: Vec<PathBuf> = subdirs(&saves).into_iter().filter(|d| d.file_name().map(|n| is_save_dir_name(&n.to_string_lossy())).unwrap_or(false)).collect();
            dirs.sort();
            dirs.reverse();
            for d in dirs.into_iter().take(3) {
                if let Ok(meta) = check_complete(&d) {
                    if let Some(exe) = meta.exe.clone().filter(|e| !e.is_empty()) {
                        out.push(crate::hints::Hint { game_name: meta.game_name.clone().unwrap_or_else(|| dg.name.clone()), exe, save_path_tpl: meta.save_path_tpl.clone() });
                    }
                    break 'game; // version valide la plus recente trouvee (avec ou sans indice)
                }
            }
        }
    }
    out
}

/// Machines vues dans le dossier partage (suffixe des noms de dossiers de sauvegarde : aucune lecture de fichier).
pub fn machines_seen(dest: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dg in list_dest_games(dest) {
        for root in &dg.dirs {
            for d in subdirs(&root.join(SAVES_DIR)) {
                if let Some(m) = d.file_name().and_then(|n| crate::machinecolor::machine_of_dirname(&n.to_string_lossy()).map(|s| s.to_string())) {
                    if !out.iter().any(|x| x.eq_ignore_ascii_case(&m)) {
                        out.push(m);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// Pour chaque element du coffre, les machines qui y ont ecrit une version (suffixe des dossiers `saves/`) : aucun fichier
/// n'est lu, seulement des noms de dossiers (sans telechargement sous Google Drive en mode streaming).
pub fn machines_by_game(dest: &Path) -> Vec<(DestGame, Vec<String>)> {
    let mut out = Vec::new();
    for dg in list_dest_games(dest) {
        let mut ms: Vec<String> = Vec::new();
        for root in &dg.dirs {
            for d in subdirs(&root.join(SAVES_DIR)) {
                if let Some(m) = d.file_name().and_then(|n| crate::machinecolor::machine_of_dirname(&n.to_string_lossy()).map(|s| s.to_string())) {
                    if !ms.iter().any(|x| x.eq_ignore_ascii_case(&m)) {
                        ms.push(m);
                    }
                }
            }
        }
        ms.sort();
        out.push((dg, ms));
    }
    out
}

/// meta.json de la version COMPLETE la plus recente d'un element du coffre (verification rapide des fichiers annonces).
pub fn latest_complete_meta(dg: &DestGame) -> Option<Meta> {
    let mut best: Option<Meta> = None;
    for root in &dg.dirs {
        let mut dirs: Vec<PathBuf> = subdirs(&root.join(SAVES_DIR)).into_iter().filter(|d| d.file_name().map(|n| is_save_dir_name(&n.to_string_lossy())).unwrap_or(false)).collect();
        dirs.sort();
        dirs.reverse();
        for d in dirs.into_iter().take(3) {
            if let Ok(meta) = check_complete(&d) {
                if best.as_ref().map(|b| meta.sort_ms() > b.sort_ms()).unwrap_or(true) {
                    best = Some(meta);
                }
                break;
            }
        }
    }
    best
}
