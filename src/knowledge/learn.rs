//! Couche 3 : mode apprentissage. Pendant que le programme tourne, observe les ecritures dans les dossiers perso
//! (Windows : surveillance par evenements `dirwatch` sur AppData, Documents et Saved Games ; Linux : fichiers modifies
//! depuis le lancement, releves a la fermeture, pour ne pas epuiser les surveillances inotify). Le bruit est filtre
//! (caches, journaux, rapports de plantage, shaders, temporaires, navigateurs, Windows, liste noire de `sensitive`).
//! A la fermeture, les dossiers candidats sont proposes.
use super::paths::Folders;
use super::{best_match, kind_for, Found, Layer, Program};
use std::collections::{HashMap, HashSet};
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

/// Composants qui designent du bruit a n'importe quel niveau.
pub const NOISE_DIRS: &[&str] = &[
    "temp", "tmp", "cache", "caches", "cache2", "gpucache", "code cache", "shadercache", "shader cache", "shaders", "shadercache.v2", "dxcache", "glcache", "vkcache", "computecache", "d3dscache", "nv_cache",
    "nvidia", "crashdumps", "crashes", "crash", "crashreports", "crashpad", "crash reports", "minidumps", "dumps", "logs", "log", "webcache", "inetcache", "inetcookies", "cookies", "history",
    "service worker", "indexeddb", "local storage", "session storage", "blob_storage", "telemetry", "diagnostics", "thumbnails", "thumbcache", "squirreltemp", "pipelinecache", "psocache", "pso cache",
    "cef", "cefcache", "htmlcache", "webview", "ebwebview", "__pycache__", "httpcache", "webcache_4430", "shadercache_vulkan", "pending", "downloads", "update", "updates", "crashreportclient",
    "intermediate", "deriveddatacache", "ddc", "analytics", "sentry", "bugsplat", "unitycrashhandler", "recent", "jumplistcache",
];

/// Extensions de fichiers de bruit.
pub const NOISE_EXT: &[&str] = &["log", "tmp", "temp", "dmp", "mdmp", "etl", "lock", "lck", "pid", "pf", "crdownload", "part", "cache", "dxvk-cache", "pipeline", "journal", "wal", "shm"];

/// Un chemin relatif a une base (composants) est-il du bruit ? `base` : gabarit de la base (`%LOCALAPPDATA%`...).
pub fn is_noise(base: &str, comps: &[String]) -> bool {
    let Some(first) = comps.first() else { return true };
    let fl = first.trim_start_matches('.').to_lowercase();
    if fl != "my games" && super::deduce::NOISE_TOP.contains(&fl.as_str()) {
        return true;
    }
    // Linux : les autres bases (et le dossier de Steam) ne sont jamais vues depuis le dossier personnel.
    if base == "$HOME" && [".cache", ".local", ".config", ".steam", ".mozilla", ".pki", ".nv", ".dbus", ".gnupg", ".ssh", ".var"].contains(&first.to_lowercase().as_str()) {
        return true;
    }
    for c in comps {
        let l = c.to_lowercase();
        if NOISE_DIRS.contains(&l.as_str()) || l.starts_with("cache_") || l.ends_with("cache") && l.len() > 6 {
            return true;
        }
    }
    let last = comps.last().map(|c| c.to_lowercase()).unwrap_or_default();
    if let Some((_, ext)) = last.rsplit_once('.') {
        if NOISE_EXT.contains(&ext) {
            return true;
        }
    }
    if last.ends_with('~') || last.starts_with("~$") || last.starts_with(".~") {
        return true;
    }
    let rel = comps.join("/").to_lowercase();
    crate::sensitive::rel_is_sensitive(&rel) || crate::sensitive::text_is_sensitive(&rel).is_some()
}

/// Chemin complet -> (base, composants relatifs), base la plus specifique.
pub fn split_under(full: &Path, f: &Folders) -> Option<(&'static str, Vec<String>)> {
    let t = super::paths::to_generic(full, f)?;
    let (ph, rest) = super::paths::split_template(&t)?;
    let ph: &'static str = super::paths::WIN_BASES.iter().chain(super::paths::LX_BASES.iter()).find(|b| b.eq_ignore_ascii_case(&ph))?;
    Some((ph, rest))
}

#[derive(Clone, Debug, Default)]
struct Stat {
    writes: u32,
    files: HashSet<String>,
}

/// Ecritures observees, regroupees par dossier « racine » d'un programme (premier niveau sous la base ; deux pour
/// LocalLow, Documents\My Games et <editeur>\<produit>).
#[derive(Clone, Debug, Default)]
pub struct Session {
    stats: HashMap<(String, String), Stat>,
}

impl Session {
    pub fn record(&mut self, base: &str, comps: &[String], companies: &[String]) {
        if comps.len() < 2 || is_noise(base, comps) {
            return; // un fichier pose directement dans la base n'indique aucun dossier de programme
        }
        let mut depth = 1;
        if base == "%LOCALLOW%" || (base == "%DOCUMENTS%" && comps[0].eq_ignore_ascii_case("my games")) || best_match(&comps[0], companies).map(|s| s >= 85).unwrap_or(false) {
            depth = 2;
        }
        if base == "$XDG_CONFIG_HOME" && comps[0].eq_ignore_ascii_case("unity3d") {
            depth = 3;
        }
        if comps.len() <= depth {
            depth = comps.len() - 1;
        }
        let root = comps[..depth].join("/");
        let s = self.stats.entry((base.to_string(), root)).or_default();
        s.writes = s.writes.saturating_add(1);
        if s.files.len() < 10_000 {
            s.files.insert(comps[depth..].join("/"));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stats.is_empty()
    }

    /// Dossiers candidats : d'abord ceux dont le nom correspond au programme (3 au plus) ; sinon, au plus 2 dossiers avec
    /// plusieurs fichiers ecrits, en confiance faible.
    pub fn candidates(&self, p: &Program, f: &Folders) -> Vec<Found> {
        let names = p.names();
        let install = p.install_dir.clone().or_else(|| p.exe_path.as_deref().and_then(Path::parent).map(Path::to_path_buf));
        let mut matched: Vec<Found> = Vec::new();
        let mut other: Vec<Found> = Vec::new();
        for ((base, root), st) in &self.stats {
            let Some(b) = f.base(base) else { continue };
            let mut dir = b.clone();
            for c in root.split('/') {
                dir.push(c);
            }
            if !dir.is_dir() {
                continue;
            }
            if let Some(i) = &install {
                let (x, y) = (crate::gamedetect::norm_path(&dir), crate::gamedetect::norm_path(i));
                if x == y || x.starts_with(&format!("{}/", y)) || y.starts_with(&format!("{}/", x)) {
                    continue;
                }
            }
            let tpl = format!("{}/{}", base, root);
            let m = root.split('/').filter_map(|c| best_match(c, &names)).max();
            let n = st.files.len() as u32;
            let found = |conf: u32, src: &str| Found { kind: kind_for(&tpl, p.app), template: Some(tpl.clone()), path: dir.clone(), layer: Layer::Learn, confidence: conf.min(100) as u8, source: src.to_string() };
            match m {
                Some(s) => matched.push(found((72 + (s as u32 - 70) / 3 + n.min(10)).min(88), "apprentissage : ecritures dans un dossier a son nom")),
                None if n >= 2 => other.push(found(50 + n.min(5), "apprentissage : ecritures pendant l'utilisation")),
                None => {}
            }
        }
        let by_conf = |v: &mut Vec<Found>| v.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(a.path.cmp(&b.path)));
        by_conf(&mut matched);
        by_conf(&mut other);
        if !matched.is_empty() {
            matched.truncate(3);
            return matched;
        }
        other.truncate(2);
        other
    }
}

/// Apprentissage en cours pour un programme.
pub struct Learner {
    pub started_ms: u64,
    folders: Folders,
    companies: Vec<String>,
    session: Session,
    #[cfg(windows)]
    watches: Vec<(PathBuf, crate::dirwatch::DirWatch, std::sync::mpsc::Receiver<crate::dirwatch::WatchMsg>)>,
}

impl Learner {
    /// Demarre l'observation (Windows : surveillance de AppData, Documents et Saved Games).
    pub fn start(p: &Program, folders: Folders, now_ms: u64) -> Learner {
        #[cfg(windows)]
        let watches = {
            let mut roots: Vec<PathBuf> = Vec::new();
            match folders.profile.as_ref().map(|p| p.join("AppData")).filter(|a| a.is_dir()) {
                Some(a) => roots.push(a),
                None => roots.extend([folders.local.clone(), folders.roaming.clone(), folders.locallow.clone()].into_iter().flatten()),
            }
            roots.extend([folders.documents.clone(), folders.saved_games.clone()].into_iter().flatten());
            roots
                .into_iter()
                .filter(|r| r.is_dir())
                .map(|r| {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let w = crate::dirwatch::DirWatch::spawn(r.clone(), tx, |_| {});
                    (r, w, rx)
                })
                .collect()
        };
        Learner {
            started_ms: now_ms,
            companies: p.companies(),
            folders,
            session: Session::default(),
            #[cfg(windows)]
            watches,
        }
    }

    /// Range les evenements recus (a appeler regulierement : la file ne grossit pas).
    pub fn pump(&mut self) {
        #[cfg(windows)]
        {
            let mut seen: HashSet<PathBuf> = HashSet::new();
            for (root, _, rx) in &self.watches {
                for _ in 0..20_000 {
                    let Ok(m) = rx.try_recv() else { break };
                    if let crate::dirwatch::WatchMsg::Changed(rel) = m {
                        let full = root.join(&rel);
                        if seen.insert(full.clone()) && full.is_file() {
                            if let Some((b, c)) = split_under(&full, &self.folders) {
                                self.session.record(b, &c, &self.companies);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Fin de l'utilisation : dossiers candidats.
    pub fn finish(mut self, p: &Program) -> Vec<Found> {
        self.pump();
        #[cfg(not(windows))]
        for (b, c) in scan_since(&self.folders, self.started_ms.saturating_sub(2000)) {
            self.session.record(b, &c, &self.companies);
        }
        self.session.candidates(p, &self.folders)
    }
}

/// Fichiers modifies depuis `since_ms` sous les bases (bruit saute, profondeur et nombre d'entrees limites).
pub fn scan_since(f: &Folders, since_ms: u64) -> Vec<(&'static str, Vec<String>)> {
    let mut out = Vec::new();
    let mut budget = 80_000usize;
    for (ph, base) in f.bases() {
        if ph == "%USERPROFILE%" {
            continue; // deja couvert par les bases plus precises
        }
        let it = walkdir::WalkDir::new(base).max_depth(if ph == "$HOME" { 3 } else { 5 }).follow_links(false).into_iter().filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let rel: Vec<String> = e.path().strip_prefix(base).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect()).unwrap_or_default();
            !(e.file_type().is_dir() && is_noise(ph, &rel) && rel.len() <= 2 || e.file_type().is_dir() && super::deduce::NOISE_TOP.contains(&rel[0].trim_start_matches('.').to_lowercase().as_str()))
        });
        for e in it.filter_map(|e| e.ok()) {
            if budget == 0 {
                return out;
            }
            budget -= 1;
            if !e.file_type().is_file() {
                continue;
            }
            let t = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
            if t < since_ms {
                continue;
            }
            let rel: Vec<String> = e.path().strip_prefix(base).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect()).unwrap_or_default();
            out.push((ph, rel));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn c(s: &str) -> Vec<String> {
        s.split('/').map(|x| x.to_string()).collect()
    }

    #[test]
    fn filtre_du_bruit() {
        for (b, p) in [
            ("%LOCALAPPDATA%", "Temp/abc.tmp"),
            ("%LOCALAPPDATA%", "Microsoft/Windows/Explorer/thumbcache_1.db"),
            ("%LOCALAPPDATA%", "NVIDIA/DXCache/x.bin"),
            ("%LOCALAPPDATA%", "D3DSCache/a/b"),
            ("%LOCALAPPDATA%", "Jeu/Saved/Logs/Jeu.log"),
            ("%LOCALAPPDATA%", "Jeu/Saved/Crashes/UE4CC/x.dmp"),
            ("%LOCALAPPDATA%", "Jeu/ShaderCache/a.bin"),
            ("%LOCALAPPDATA%", "Google/Chrome/User Data/Default/Cookies"),
            ("%APPDATA%", "Mozilla/Firefox/Profiles/x/places.sqlite"),
            ("%APPDATA%", "discord/Cache/Cache_Data/f_0001"),
            ("%APPDATA%", "Jeu/GPUCache/data_0"),
            ("%LOCALAPPDATA%", "AutoGameSoftwareSync/logs/watch.log"),
            ("%APPDATA%", "Outil/.ssh/id_rsa"),
            ("%LOCALAPPDATA%", "Jeu/session.lock"),
            ("%LOCALLOW%", "Unity/Caches/x"),
            ("$HOME", ".cache/mesa_shader_cache/x"),
            ("$XDG_DATA_HOME", "Steam/steamapps/x"),
            ("%LOCALAPPDATA%", "Jeu/webcache/x"),
        ] {
            assert!(is_noise(b, &c(p)), "{} {}", b, p);
        }
        for (b, p) in [
            ("%APPDATA%", "StardewValley/Saves/Farm_1/Farm_1"),
            ("%LOCALAPPDATA%", "Pal/Saved/SaveGames/7656/Level.sav"),
            ("%LOCALAPPDATA%", "Pal/Saved/Config/Windows/GameUserSettings.ini"),
            ("%LOCALLOW%", "Team Cherry/Hollow Knight/user1.dat"),
            ("%DOCUMENTS%", "My Games/Terraria/Players/a.plr"),
            ("$XDG_DATA_HOME", "Terraria/Players/a.plr"),
        ] {
            assert!(!is_noise(b, &c(p)), "{} {}", b, p);
        }
    }

    #[test]
    fn candidats_a_la_fermeture() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        let ll = f.locallow.clone().unwrap();
        let mk = |d: PathBuf| {
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        let hk = mk(ll.join("Team Cherry").join("Hollow Knight"));
        let other = mk(f.roaming.clone().unwrap().join("Outil"));
        let mut s = Session::default();
        // Ecritures melangees : jeu, outil en arriere-plan, bruit.
        s.record("%LOCALLOW%", &c("Team Cherry/Hollow Knight/user1.dat"), &[]);
        s.record("%LOCALLOW%", &c("Team Cherry/Hollow Knight/user1.dat.bak"), &[]);
        s.record("%APPDATA%", &c("Outil/a.json"), &[]);
        s.record("%APPDATA%", &c("Outil/b.json"), &[]);
        s.record("%LOCALAPPDATA%", &c("Temp/x.tmp"), &[]);
        s.record("%LOCALAPPDATA%", &c("racine.txt"), &[]);
        let p = Program { exe: "hollow_knight.exe".into(), name: "Hollow Knight".into(), ..Default::default() };
        let v = s.candidates(&p, &f);
        assert_eq!(v.iter().map(|x| x.path.clone()).collect::<Vec<_>>(), vec![hk.clone()], "le dossier a son nom gagne");
        assert_eq!(v[0].template.as_deref(), Some("%LOCALLOW%/Team Cherry/Hollow Knight"));
        assert!(v[0].confidence >= 80);
        // Programme au nom inconnu : seuls les dossiers avec plusieurs fichiers, en confiance faible.
        let q = Program { exe: "zz.exe".into(), name: "Zorblax".into(), ..Default::default() };
        let v = s.candidates(&q, &f);
        assert!(v.iter().all(|x| x.confidence <= 55), "{:?}", v);
        assert!(v.iter().any(|x| x.path == other));
        assert!(v.iter().any(|x| x.path == hk));
        // Editeur reconnu : deux niveaux (<editeur>/<produit>).
        let pal = mk(f.local.clone().unwrap().join("Pocketpair").join("Palworld"));
        let mut s2 = Session::default();
        s2.record("%LOCALAPPDATA%", &c("Pocketpair/Palworld/Saved/x.sav"), &["Pocketpair".to_string()]);
        let pp = Program { exe: "Palworld.exe".into(), name: "Palworld".into(), company: Some("Pocketpair".into()), ..Default::default() };
        assert_eq!(s2.candidates(&pp, &f)[0].path, pal);
    }

    #[test]
    fn releve_des_fichiers_modifies() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        let d = f.roaming.clone().unwrap().join("Jeu").join("Saves");
        std::fs::create_dir_all(&d).unwrap();
        let start = crate::util::now_ms().saturating_sub(5000);
        std::fs::write(d.join("slot1.sav"), b"x").unwrap();
        let tmp = f.local.clone().unwrap().join("Temp");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("a.tmp"), b"x").unwrap();
        let v = scan_since(&f, start);
        assert!(v.iter().any(|(b, c)| *b == "%APPDATA%" && c.join("/") == "Jeu/Saves/slot1.sav"), "{:?}", v);
        assert!(!v.iter().any(|(_, c)| c.first().map(|x| x == "Temp").unwrap_or(false)));
        assert!(scan_since(&f, crate::util::now_ms() + 60_000).is_empty());
    }
}
