//! Briques de stockage : scan, metadonnees, copie atomique verifiee.
use crate::{util, Ctx, Res};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileInfo {
    /// Chemin relatif, separateur '/'.
    pub path: String,
    pub size: u64,
    pub hash: String,
    /// Identifiant de l emplacement supplementaire (facultatif : absent = emplacement principal, format historique).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// Metadonnees d'un etat sauvegarde (meta.json, une par version).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Meta {
    pub game: String,
    pub machine: String,
    pub created_ms: u64,
    pub created_at: String,
    /// Plus recente date de modification parmi les fichiers sources.
    pub latest_mtime_ms: u64,
    pub global_hash: String,
    pub files: Vec<FileInfo>,
    #[serde(default)]
    pub note: Option<String>,
    /// Identifiant unique de cette sauvegarde (vide = ancien format : voir `Meta::version_id`).
    #[serde(default)]
    pub id: String,
    /// Sauvegarde sur laquelle CE PC s'est base (derniere version synchronisee connue localement).
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Autres parents (resolution d'un conflit : toutes les branches fusionnees).
    #[serde(default)]
    pub merged_parents: Vec<String>,
    /// Ancetres (parent en premier, 64 max) : la filiation survit a l'elagage d'un ancetre.
    #[serde(default)]
    pub lineage: Vec<String>,
    /// Heure corrigee (UTC, ms) : horloge locale + ecart mesure sur internet.
    #[serde(default)]
    pub saved_at_ms: u64,
    #[serde(default)]
    pub saved_at: String,
    /// Meme instant en heure locale du PC (ISO 8601 avec decalage).
    #[serde(default)]
    pub saved_at_local: String,
    /// Ecart d'horloge applique (ms), pour memoire.
    #[serde(default)]
    pub clock_offset_ms: i64,
    /// 2 = disposition `saves/` (cloud-safe) ; 0 = ancien format.
    #[serde(default)]
    pub format: u32,
    /// Indices facultatifs (nouvelles sauvegardes seulement ; ne font PAS partie de l'empreinte du contenu) : nom de fichier
    /// de l'exe, nom d'affichage du jeu, dossier de sauvegarde en forme universelle (voir `hints`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_path_tpl: Option<String>,
    /// Emplacements supplementaires de cette sauvegarde (facultatif ; absent pour un jeu a un seul emplacement).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<LocMeta>,
    /// Indices sur l'emplacement principal (facultatif, nouvelles sauvegardes ; hors empreinte) : fichier unique, filtre de
    /// noms, exclusions, systeme de l'appareil. Servent a retrouver l'element tel quel sur un nouveau PC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_hint: Option<MainHint>,
}

/// Indices sur l'emplacement principal (voir `Meta::main_hint`). Jamais de chemin.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct MainHint {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_file: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
    /// `windows` ou `linux`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os: String,
}

/// Systeme de cet appareil, tel qu'il est note dans les indices.
pub fn this_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// Description d un emplacement supplementaire dans meta.json : id stable, nom, fichier ou dossier, gabarit de chemin universel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct LocMeta {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub is_file: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_tpl: Option<String>,
    /// Filtre de noms et exclusions de cet emplacement (facultatif).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
}

impl Meta {
    /// Identifiant logique : `id` ou, pour l'ancien format, derive du hash (deux copies du meme contenu = meme version).
    pub fn version_id(&self) -> String {
        if self.id.is_empty() {
            format!("legacy-{}", self.global_hash.chars().take(16).collect::<String>())
        } else {
            self.id.clone()
        }
    }

    /// Date de tri : heure corrigee si presente, sinon date de creation (ancien format).
    pub fn sort_ms(&self) -> u64 {
        if self.saved_at_ms > 0 { self.saved_at_ms } else { self.created_ms }
    }

    /// Tous les ancetres connus (parent, parents fusionnes, filiation).
    pub fn ancestors(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.extend(self.parent_id.iter().cloned());
        v.extend(self.merged_parents.iter().cloned());
        v.extend(self.lineage.iter().cloned());
        v
    }
}

pub fn global_hash(files: &[FileInfo]) -> String {
    let mut s = String::new();
    for f in files {
        s.push_str(&format!("{}\0{}\n", f.path, f.hash));
    }
    util::hash_bytes(s.as_bytes())
}

/// Liste (triee) des fichiers d'un dossier avec hash + plus recente mtime.
/// Ne suit pas les liens symboliques.
pub fn scan(dir: &Path) -> Res<(Vec<FileInfo>, u64)> {
    let mut out = Vec::new();
    let mut latest = 0u64;
    for e in WalkDir::new(dir).follow_links(false) {
        let e = e.map_err(|e| format!("parcours de {} : {}", dir.display(), e))?;
        if !e.file_type().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy();
        if name.ends_with(".agss-tmp") || name.ends_with(crate::legacy::TMP_SUFFIX) {
            continue; // reste de notre propre ecriture interrompue
        }
        let md = e.metadata().map_err(|e| format!("metadonnees : {}", e))?;
        if let Ok(t) = md.modified() {
            latest = latest.max(util::mtime_ms(t));
        }
        let rel = e.path().strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        out.push(FileInfo { path: rel, size: md.len(), hash: util::hash_file(e.path())?, location: None });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((out, latest))
}

/// Comme `scan`, mais les hash de fichiers inchanges (meme chemin, taille, date, et stables au moment
/// du calcul) viennent du cache d'empreintes LOCAL. A utiliser pour les dossiers locaux uniquement.
pub fn scan_cached(ctx: &Ctx, dir: &Path) -> Res<(Vec<FileInfo>, u64)> {
    let (v, latest) = scan_cached_mt(ctx, dir)?;
    Ok((v.into_iter().map(|x| x.0).collect(), latest))
}

/// Idem avec la date de modification (ms) de chaque fichier (apercu de restauration).
pub fn scan_cached_mt(ctx: &Ctx, dir: &Path) -> Res<(Vec<(FileInfo, u64)>, u64)> {
    scan_target_mt(ctx, &Target::dir(dir))
}

/// Scan (avec cache) de l ENSEMBLE de fichiers d une cible : dossier entier, dossier filtre, ou fichier unique.
pub fn scan_target_mt(ctx: &Ctx, t: &Target) -> Res<(Vec<(FileInfo, u64)>, u64)> {
    let dir: &Path = &t.root;
    let mut out: Vec<(FileInfo, u64)> = Vec::new();
    let mut latest = 0u64;
    let mut walk = WalkDir::new(dir).follow_links(false);
    if t.only_file.is_some() {
        walk = walk.max_depth(1);
    }
    // Les dossiers exclus ne sont pas parcourus (DerivedDataCache, caches... potentiellement enormes).
    let walker = walk.into_iter().filter_entry(|e| {
        if e.depth() == 0 || !e.file_type().is_dir() {
            return true;
        }
        let rel = e.path().strip_prefix(dir).map(|r| r.to_string_lossy().replace('\\', "/").to_lowercase()).unwrap_or_default();
        !t.excluded(&rel) && !crate::sensitive::rel_is_sensitive(&format!("{}/x", rel))
    });
    for e in walker {
        let e = e.map_err(|e| format!("parcours de {} : {}", dir.display(), e))?;
        if !e.file_type().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy();
        if name.ends_with(".agss-tmp") || name.ends_with(crate::legacy::TMP_SUFFIX) {
            continue;
        }
        let rel = e.path().strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        if !t.accepts(&rel) {
            continue;
        }
        let md = e.metadata().map_err(|e| format!("metadonnees : {}", e))?;
        let mtime = md.modified().map(util::mtime_ms).unwrap_or(0);
        latest = latest.max(mtime);
        let hash = match if mtime > 0 { ctx.hash_cache.lookup(e.path(), md.len(), mtime) } else { None } {
            Some(h) => h,
            None => {
                let h = util::hash_file(e.path())?;
                if mtime > 0 {
                    ctx.hash_cache.store(e.path(), md.len(), mtime, &h, util::now_ms());
                }
                h
            }
        };
        out.push((FileInfo { path: rel, size: md.len(), hash, location: None }, mtime));
    }
    out.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    ctx.hash_cache.save();
    Ok((out, latest))
}


pub fn make_meta(ctx: &Ctx, game: &str, files: Vec<FileInfo>, latest: u64, note: Option<String>) -> Meta {
    let now = ctx.clock.system_ms();
    let corrected = ctx.clock.corrected_ms();
    Meta {
        game: game.to_string(),
        machine: ctx.machine.clone(),
        created_ms: now,
        created_at: util::timestamp(now),
        latest_mtime_ms: latest,
        global_hash: global_hash(&files),
        files,
        note,
        id: util::new_id(&ctx.machine),
        parent_id: None,
        merged_parents: vec![],
        lineage: vec![],
        saved_at_ms: corrected,
        saved_at: util::timestamp(corrected),
        saved_at_local: crate::timefmt::iso_local(corrected, &crate::timefmt::WinTz),
        clock_offset_ms: ctx.clock.offset_ms(),
        format: 2,
        exe: None,
        game_name: None,
        save_path_tpl: None,
        locations: vec![],
        main_hint: None,
    }
}

pub fn read_meta(path: &Path) -> Res<Meta> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("lecture {} : {}", path.display(), e))?;
    serde_json::from_str(&s).map_err(|e| format!("meta illisible {} : {}", path.display(), e).into())
}

/// Copie en UN SEUL passage : lit `src` une fois, ecrit dans `out` et calcule le hash des octets lus.
/// Retourne (hash, octets). Met a jour la progression et honore l'annulation (avant le point de non-retour).
pub fn stream_copy(ctx: &Ctx, src: &Path, out: &mut impl std::io::Write) -> Res<(String, u64)> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut inp = std::fs::File::open(src).map_err(|e| format!("ouverture {} : {}", src.display(), e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        if let Some(p) = &ctx.progress {
            p.check_cancel()?;
        }
        let n = inp.read(&mut buf).map_err(|e| format!("lecture {} : {}", src.display(), e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("ecriture : {}", e))?;
        total += n as u64;
        if let Some(p) = &ctx.progress {
            p.add_bytes(n as u64);
        }
    }
    Ok((util::hex_digest(h.finalize().as_slice()), total))
}

/// Copie `src` vers `dst` : fichier temporaire, lecture UNIQUE de la source (hash calcule sur les octets
/// lus), puis relecture de controle du seul fichier ecrit, puis renommage atomique.
/// En cas d'echec : seul le temporaire est supprime, `dst` reste intact. Retourne le hash verifie.
pub fn copy_verified(ctx: &Ctx, src: &Path, dst: &Path) -> Res<String> {
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut tmp = dst.as_os_str().to_owned();
    tmp.push(".agss-tmp");
    let tmp = PathBuf::from(tmp);
    let mut h_src = String::new();
    let attempt = (|| -> Res<()> {
        use std::io::Write;
        let mut out = std::fs::File::create(&tmp).map_err(|e| format!("copie {} -> {} : {}", src.display(), tmp.display(), e))?;
        h_src = stream_copy(ctx, src, &mut out)?.0;
        if ctx.fault_corrupt_copy {
            out.write_all(b"\0CORRUPTION-SIMULEE")?;
        }
        drop(out);
        let h_dst = util::hash_file(&tmp)?;
        if h_dst != h_src {
            return Err(format!("ECHEC DE VERIFICATION pour {} (hash different apres ecriture)", dst.display()).into());
        }
        std::fs::rename(&tmp, dst).map_err(|e| format!("renommage vers {} : {}", dst.display(), e))?;
        Ok(())
    })();
    if let Err(e) = attempt {
        let _ = std::fs::remove_file(&tmp); // notre temporaire uniquement
        return Err(e);
    }
    Ok(h_src)
}

/// Copie verifiee d'une liste de fichiers `files` de `src_root` vers `dst_root`.
/// Echoue si un fichier a change depuis le scan (hash different de celui attendu) ; dans ce cas son
/// entree du cache d'empreintes est oubliee (dans le doute, on recalcule).
pub fn copy_tree_verified(ctx: &Ctx, src_root: &Path, dst_root: &Path, files: &[FileInfo]) -> Res<()> {
    for f in files {
        let src = src_root.join(&f.path);
        let h = copy_verified(ctx, &src, &dst_root.join(&f.path))?;
        if h != f.hash {
            ctx.hash_cache.forget(&src);
            ctx.hash_cache.save();
            return Err(format!("{} a change pendant la copie (fichier en cours d'ecriture ?)", f.path).into());
        }
        if let Some(p) = &ctx.progress {
            p.file_done();
        }
    }
    Ok(())
}

/// Suppression d'un dossier de staging que NOUS avons cree (nom ".tmp-*"). Refuse tout autre nom.
pub fn remove_staging(dir: &Path) {
    let ok = dir.file_name().map(|n| n.to_string_lossy().starts_with(".tmp-")).unwrap_or(false);
    if ok && dir.is_dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Nom de dossier unique dans `parent` (suffixe -001, -002... en cas de collision).
pub fn unique_dir(parent: &Path, base: &str) -> PathBuf {
    let first = parent.join(base);
    if !first.exists() {
        return first;
    }
    for i in 1.. {
        let p = parent.join(format!("{}-{:03}", base, i));
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

/// Copie `src` vers `dst` DANS LA DESTINATION (cloud) : `dst` ne doit pas exister (jamais d'ecrasement),
/// aucun renommage (Google Drive les traduit en suppression + creation). La source (copie locale deja
/// verifiee) est lue UNE fois ; le fichier ecrit est relu UNE fois pour controle. En cas d'echec le
/// fichier partiel (cree par nous a l'instant) est supprime. Retourne le hash verifie.
pub fn copy_new_verified(ctx: &Ctx, src: &Path, dst: &Path) -> Res<String> {
    use std::io::Write;
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut h_src = String::new();
    let attempt = (|| -> Res<()> {
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(dst).map_err(|e| format!("creation {} : {}", dst.display(), e))?;
        h_src = stream_copy(ctx, src, &mut out)?.0;
        if ctx.fault_corrupt_copy {
            out.write_all(b"\0CORRUPTION-SIMULEE")?;
        }
        out.sync_all().ok();
        drop(out);
        let h_dst = util::hash_file(dst)?;
        if h_dst != h_src {
            return Err(format!("ECHEC DE VERIFICATION pour {} (hash different apres ecriture)", dst.display()).into());
        }
        Ok(())
    })();
    if let Err(e) = attempt {
        let _ = std::fs::remove_file(dst); // fichier que NOUS venons de creer
        return Err(e);
    }
    Ok(h_src)
}

/// Ecrit un fichier NOUVEAU (jamais d'ecrasement) directement a son emplacement final, sans renommage.
/// Utilise pour `meta.json` (ecrit en dernier : marqueur de completude) et `game.json`.
pub fn write_new(path: &Path, data: &[u8]) -> Res<()> {
    use std::io::Write;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("creation {} : {}", path.display(), e))?;
    let r = f.write_all(data).and_then(|_| f.sync_all());
    drop(f);
    if let Err(e) = r {
        let _ = std::fs::remove_file(path);
        return Err(format!("ecriture {} : {}", path.display(), e).into());
    }
    Ok(())
}

// ------------------------------------------------------------------ cible de sauvegarde (dossier, dossier filtre, fichier)

/// Ensemble de fichiers sauvegarde pour un jeu : un DOSSIER entier, un dossier avec FILTRE de noms (`*.srm;Jeu.srm`),
/// ou un FICHIER unique (ex. memcard1.mcd). Les chemins relatifs (`files/`) sont relatifs a `root`.
#[derive(Clone, Debug)]
pub struct Target {
    pub root: PathBuf,
    /// Mode fichier : nom du fichier unique (le dossier parent est `root`).
    pub only_file: Option<String>,
    /// Motifs en minuscules (`*` et `?`), vide = tout. Un motif avec `/` s'applique au chemin relatif, sinon au nom du fichier.
    pub filter: Vec<String>,
    /// Motifs exclus (nom de fichier ou de dossier a n importe quel niveau, ou chemin relatif).
    pub exclude: Vec<String>,
}

/// Correspondance de motif `*` / `?` (insensible a la casse : `pat` et `text` en minuscules).
pub fn glob_match(pat: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Motifs d'un champ de filtre (separes par `;`), en minuscules.
pub fn parse_filter(s: &str) -> Vec<String> {
    s.split(';').map(|x| x.trim().to_lowercase().replace('\\', "/")).filter(|x| !x.is_empty()).collect()
}

impl Target {
    /// Tout le dossier.
    pub fn dir(root: &Path) -> Target {
        Target { root: root.to_path_buf(), only_file: None, filter: vec![], exclude: vec![] }
    }

    pub fn of(g: &crate::config::Game) -> Target {
        if g.save_is_file {
            let root = g.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let name = g.path.file_name().map(|n| n.to_string_lossy().to_string());
            Target { root, only_file: name, filter: vec![], exclude: vec![] }
        } else {
            Target { root: g.path.clone(), only_file: None, filter: g.save_filter.as_deref().map(parse_filter).unwrap_or_default(), exclude: g.save_exclude.as_deref().map(parse_filter).unwrap_or_default() }
        }
    }

    pub fn accepts(&self, rel: &str) -> bool {
        let rel_l = rel.to_lowercase();
        // Liste noire dure : jamais d identifiants / cles / cookies, quelle que soit la configuration.
        if crate::sensitive::rel_is_sensitive(&rel_l) {
            return false;
        }
        if let Some(f) = &self.only_file {
            return rel_l == f.to_lowercase();
        }
        if self.excluded(&rel_l) {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        let name = rel_l.rsplit('/').next().unwrap_or(&rel_l).to_string();
        self.filter.iter().any(|p| if p.contains('/') { glob_match(p, &rel_l) } else { glob_match(p, &name) })
    }

    /// Un chemin relatif (minuscules) est-il exclu ? Motif sans `/` : nom du fichier ou de n importe quel dossier parent ; avec `/` : chemin ou prefixe.
    pub fn excluded(&self, rel_l: &str) -> bool {
        self.exclude.iter().any(|p| {
            if p.contains('/') {
                glob_match(p, rel_l) || glob_match(&format!("{}/*", p.trim_end_matches('/')), rel_l)
            } else {
                rel_l.split('/').any(|c| glob_match(p, c))
            }
        })
    }

    /// La cible existe-t-elle (dossier ou fichier) ?
    pub fn exists(&self) -> bool {
        match &self.only_file {
            Some(f) => self.root.join(f).is_file(),
            None => self.root.is_dir(),
        }
    }

    /// Scan avec cache (voir `scan_target_mt`) sans les dates.
    pub fn scan(&self, ctx: &Ctx) -> Res<(Vec<FileInfo>, u64)> {
        let (v, latest) = scan_target_mt(ctx, self)?;
        Ok((v.into_iter().map(|x| x.0).collect(), latest))
    }
}

// ------------------------------------------------------------------ cible multi-emplacements

/// Un emplacement actif d'un jeu. `id` None = emplacement principal (chemins sans prefixe, format historique) ;
/// sinon les fichiers sont ranges sous `@<id>/` dans la sauvegarde.
#[derive(Clone, Debug)]
pub struct Part {
    pub id: Option<String>,
    pub label: String,
    pub target: Target,
}

/// UNION des emplacements actives d'un jeu. Pour un jeu a un seul emplacement, se comporte exactement comme `Target`.
#[derive(Clone, Debug)]
pub struct GameTarget {
    pub parts: Vec<Part>,
    /// Identifiants de TOUS les emplacements configures sur ce PC (actifs ou non) : distingue « desactive » de « inconnu ».
    pub known_ids: Vec<String>,
}

/// `@id/reste` -> (Some(id), reste) ; sinon (None, chemin).
pub fn split_loc(path: &str) -> (Option<&str>, &str) {
    if let Some(r) = path.strip_prefix('@') {
        if let Some((id, rest)) = r.split_once('/') {
            return (Some(id), rest);
        }
    }
    (None, path)
}

impl GameTarget {
    pub fn of(g: &crate::config::Game) -> GameTarget {
        let mut parts = Vec::new();
        for l in g.enabled_locations() {
            let t = Target::of(&g.pseudo_for(&l));
            parts.push(Part { id: if l.id == crate::config::MAIN_LOCATION { None } else { Some(l.id.clone()) }, label: l.label.clone(), target: t });
        }
        GameTarget { parts, known_ids: g.all_locations().into_iter().map(|l| l.id).collect() }
    }

    /// Au moins un emplacement actif existe-t-il ?
    pub fn exists(&self) -> bool {
        self.parts.iter().any(|p| p.target.exists())
    }

    /// Union des fichiers (chemins prefixes `@id/` pour les emplacements supplementaires), avec dates.
    pub fn scan_mt(&self, ctx: &Ctx) -> Res<(Vec<(FileInfo, u64)>, u64)> {
        let mut out: Vec<(FileInfo, u64)> = Vec::new();
        let mut latest = 0u64;
        for p in &self.parts {
            if !p.target.exists() {
                continue;
            }
            let (v, l) = scan_target_mt(ctx, &p.target)?;
            latest = latest.max(l);
            for (mut f, m) in v {
                if let Some(id) = &p.id {
                    f.path = format!("@{}/{}", id, f.path);
                    f.location = Some(id.clone());
                }
                out.push((f, m));
            }
        }
        out.sort_by(|a, b| a.0.path.cmp(&b.0.path));
        Ok((out, latest))
    }

    pub fn scan(&self, ctx: &Ctx) -> Res<(Vec<FileInfo>, u64)> {
        let (v, l) = self.scan_mt(ctx)?;
        Ok((v.into_iter().map(|x| x.0).collect(), l))
    }

    /// Chemin local d'un fichier de la sauvegarde (None : emplacement inconnu / desactive sur ce PC, ou fichier hors ensemble).
    pub fn local_path(&self, cloud_path: &str) -> Option<PathBuf> {
        let (id, rel) = split_loc(cloud_path);
        let p = self.parts.iter().find(|p| p.id.as_deref() == id)?;
        if !p.target.accepts(rel) {
            return None;
        }
        Some(p.target.root.join(rel))
    }

    pub fn accepts(&self, cloud_path: &str) -> bool {
        self.local_path(cloud_path).is_some()
    }

    /// Emplacement de la sauvegarde absent de la configuration de CE PC (id inconnu).
    pub fn is_unknown(&self, cloud_path: &str) -> bool {
        match split_loc(cloud_path).0 {
            Some(id) => !self.known_ids.iter().any(|k| k == id),
            None => false,
        }
    }

    /// Dossiers a creer avant restauration (racines des emplacements actifs).
    pub fn roots(&self) -> Vec<PathBuf> {
        self.parts.iter().map(|p| p.target.root.clone()).collect()
    }
}

/// Copie verifiee fichier par fichier avec des chemins source / destination calcules (meme controle d'empreinte que
/// `copy_tree_verified`). `map` retourne None pour un fichier a ignorer.
pub fn copy_files_mapped(ctx: &Ctx, files: &[FileInfo], map: &dyn Fn(&FileInfo) -> Option<(PathBuf, PathBuf)>) -> Res<()> {
    for f in files {
        let Some((src, dst)) = map(f) else { continue };
        let h = copy_verified(ctx, &src, &dst)?;
        if h != f.hash {
            ctx.hash_cache.forget(&src);
            ctx.hash_cache.save();
            return Err(format!("{} a change pendant la copie (fichier en cours d'ecriture ?)", f.path).into());
        }
        if let Some(p) = &ctx.progress {
            p.file_done();
        }
    }
    Ok(())
}
