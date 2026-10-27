//! Cache local de Steam `appcache/appinfo.vdf` (format binaire Valve KeyValues, versions 27, 28 et 29) : lecture
//! seule, robuste (bornes verifiees partout, profondeur limitee, aucune panique sur un fichier tronque ou corrompu).
//! Seules les applications demandees sont decodees (chaque entree donne sa taille : les autres sont sautees).
use crate::detect::Vdf;
use std::collections::HashMap;
use std::path::Path;

/// Versions connues (octet de poids faible du nombre magique).
const MAGIC_27: u32 = 0x0756_4427;
const MAGIC_28: u32 = 0x0756_4428;
const MAGIC_29: u32 = 0x0756_4429;
/// Profondeur maximale des sections imbriquees (un fichier corrompu ne fait jamais deborder la pile).
const MAX_DEPTH: usize = 64;

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.pos).ok_or("fin de donnees inattendue")?;
        self.pos += 1;
        Ok(v)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("taille invalide")?;
        let s = self.b.get(self.pos..end).ok_or("fin de donnees inattendue")?;
        self.pos = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, String> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
    /// Chaine terminee par un octet nul (UTF-8 avec remplacement).
    fn cstr(&mut self) -> Result<String, String> {
        let rest = self.b.get(self.pos..).ok_or("fin de donnees inattendue")?;
        let n = rest.iter().position(|&c| c == 0).ok_or("chaine non terminee")?;
        let s = String::from_utf8_lossy(&rest[..n]).to_string();
        self.pos += n + 1;
        Ok(s)
    }
    /// Chaine UTF-16 terminee par deux octets nuls.
    fn wstr(&mut self) -> Result<String, String> {
        let mut units = Vec::new();
        loop {
            let s = self.take(2)?;
            let u = u16::from_le_bytes([s[0], s[1]]);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        Ok(String::from_utf16_lossy(&units))
    }
}

/// Decode une section binaire jusqu'a son marqueur de fin. `keys` : table des cles (version 29), sinon cles en clair.
fn parse_section(r: &mut Reader, keys: Option<&[String]>, depth: usize) -> Result<Vec<(String, Vdf)>, String> {
    if depth > MAX_DEPTH {
        return Err("sections trop imbriquees".into());
    }
    let mut out = Vec::new();
    loop {
        let ty = r.u8()?;
        if ty == 0x08 || ty == 0x0B {
            return Ok(out);
        }
        let key = match keys {
            Some(k) => {
                let i = r.u32()? as usize;
                k.get(i).cloned().ok_or_else(|| format!("index de cle {} hors table", i))?
            }
            None => r.cstr()?,
        };
        let val = match ty {
            0x00 => Vdf::Map(parse_section(r, keys, depth + 1)?),
            0x01 => Vdf::Str(r.cstr()?),
            0x02 => Vdf::Str((r.u32()? as i32).to_string()),
            0x03 => Vdf::Str(f32::from_bits(r.u32()?).to_string()),
            0x04 | 0x06 => Vdf::Str(r.u32()?.to_string()),
            0x05 => Vdf::Str(r.wstr()?),
            0x07 => Vdf::Str(r.u64()?.to_string()),
            0x0A => Vdf::Str((r.u64()? as i64).to_string()),
            t => return Err(format!("type binaire inconnu 0x{:02x}", t)),
        };
        out.push((key, val));
    }
}

/// Decode des KeyValues binaires « simples » (cles en clair), ex. `shortcuts.vdf`, `localconfig` binaire.
pub fn parse_binary_kv(bytes: &[u8]) -> Result<Vdf, String> {
    let mut r = Reader { b: bytes, pos: 0 };
    Ok(Vdf::Map(parse_section(&mut r, None, 0)?))
}

/// Fichier appinfo.vdf indexe : (appid -> position et taille des KeyValues de l'application).
pub struct AppInfo {
    data: Vec<u8>,
    version: u32,
    keys: Option<Vec<String>>,
    index: HashMap<u32, (usize, usize)>,
}

impl AppInfo {
    /// Indexe le fichier (aucune application n'est decodee ici). Erreur claire sur un format inconnu ou tronque.
    pub fn parse(data: Vec<u8>) -> Result<AppInfo, String> {
        let mut r = Reader { b: &data, pos: 0 };
        let magic = r.u32()?;
        let version = match magic {
            MAGIC_27 => 27,
            MAGIC_28 => 28,
            MAGIC_29 => 29,
            m => return Err(format!("appinfo.vdf : version inconnue (0x{:08x})", m)),
        };
        let _universe = r.u32()?;
        let mut keys = None;
        if version >= 29 {
            let off = r.u64()? as usize;
            let mut t = Reader { b: &data, pos: off };
            let n = t.u32()? as usize;
            if n > data.len() {
                return Err("table des cles invalide".into());
            }
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(t.cstr()?);
            }
            keys = Some(v);
        }
        // En-tete fixe de chaque entree, apres la taille : etat, date, jeton, sha1 du texte, numero de modification
        // (+ sha1 binaire depuis la version 28).
        let fixed = if version >= 28 { 60 } else { 40 };
        let mut index = HashMap::new();
        loop {
            let appid = match r.u32() {
                Ok(a) => a,
                Err(_) => break, // fin tronquee : on garde ce qui est deja indexe
            };
            if appid == 0 {
                break;
            }
            let size = r.u32()? as usize;
            let start = r.pos;
            let end = start.checked_add(size).ok_or("taille invalide")?;
            if end > data.len() || size < fixed {
                break; // entree tronquee : arret propre
            }
            index.insert(appid, (start + fixed, size - fixed));
            r.pos = end;
        }
        Ok(AppInfo { data, version, keys, index })
    }

    pub fn load(path: &Path) -> Result<AppInfo, String> {
        let data = std::fs::read(path).map_err(|e| format!("{} : {}", path.display(), e))?;
        Self::parse(data)
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn contains(&self, appid: u32) -> bool {
        self.index.contains_key(&appid)
    }

    /// KeyValues d'une application (racine `appinfo` quand elle existe).
    pub fn app(&self, appid: u32) -> Option<Vdf> {
        let &(start, len) = self.index.get(&appid)?;
        let slice = self.data.get(start..start + len)?;
        let mut r = Reader { b: slice, pos: 0 };
        let root = parse_section(&mut r, self.keys.as_deref(), 0).ok()?;
        let v = Vdf::Map(root);
        match v.get("appinfo") {
            Some(a @ Vdf::Map(_)) => Some(a.clone()),
            _ => Some(v),
        }
    }
}

/// Regle de sauvegarde cloud automatique (`ufs/savefiles/<n>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveRule {
    pub root: String,
    pub path: String,
    pub pattern: String,
    /// Plateformes citees (vide = toutes).
    pub platforms: Vec<String>,
    pub recursive: bool,
}

/// Remplacement de racine pour un autre systeme (`ufs/rootoverrides/<n>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootOverride {
    pub root: String,
    pub os: String,
    pub new_root: String,
    pub add_path: String,
    pub use_instead: bool,
    /// (motif, remplacement) appliques au chemin.
    pub transforms: Vec<(String, String)>,
}

/// Ce que le cache de Steam sait d'une application.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppUfs {
    pub name: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub savefiles: Vec<SaveRule>,
    pub overrides: Vec<RootOverride>,
    /// Quota cloud (octets) : > 0 = l'application utilise le stockage cloud de Steam (dossier `remote`).
    pub quota: u64,
}

fn s(v: &Vdf, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn items(v: Option<&Vdf>) -> Vec<&Vdf> {
    match v {
        Some(Vdf::Map(m)) => m.iter().map(|(_, x)| x).collect(),
        _ => Vec::new(),
    }
}

/// Extrait `common` (nom, editeur) et `ufs` (regles de sauvegarde) d'une application decodee.
pub fn ufs_of(app: &Vdf) -> AppUfs {
    let mut out = AppUfs::default();
    if let Some(c) = app.get("common") {
        out.name = Some(s(c, "name")).filter(|x| !x.is_empty());
        // Les editeurs sont dans `associations` (nouveau format) ou `developer` / `publisher` (ancien).
        out.developer = Some(s(c, "developer")).filter(|x| !x.is_empty());
        out.publisher = Some(s(c, "publisher")).filter(|x| !x.is_empty());
        for a in items(c.get("associations")) {
            let (t, n) = (s(a, "type"), s(a, "name"));
            if n.is_empty() {
                continue;
            }
            if t == "publisher" && out.publisher.is_none() {
                out.publisher = Some(n);
            } else if t == "developer" && out.developer.is_none() {
                out.developer = Some(n);
            }
        }
    }
    let Some(u) = app.get("ufs") else { return out };
    out.quota = s(u, "quota").parse().unwrap_or(0);
    for f in items(u.get("savefiles")) {
        let platforms: Vec<String> = match f.get("platforms") {
            Some(Vdf::Map(m)) => m.iter().filter_map(|(_, x)| x.as_str().map(|s| s.to_string())).collect(),
            Some(Vdf::Str(p)) => p.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            None => Vec::new(),
        };
        let root = s(f, "root");
        if root.is_empty() {
            continue;
        }
        out.savefiles.push(SaveRule { root, path: s(f, "path"), pattern: s(f, "pattern"), platforms, recursive: s(f, "recursive") == "1" });
    }
    for o in items(u.get("rootoverrides")) {
        let transforms = items(o.get("pathtransforms")).into_iter().map(|t| (s(t, "find"), s(t, "replace"))).filter(|(f, _)| !f.is_empty()).collect();
        out.overrides.push(RootOverride { root: s(o, "root"), os: s(o, "os"), new_root: s(o, "newroot"), add_path: s(o, "addpath"), use_instead: s(o, "useinstead") == "1", transforms });
    }
    out
}

static CACHE: std::sync::Mutex<Option<(std::path::PathBuf, Option<std::time::SystemTime>, std::sync::Arc<AppInfo>)>> = std::sync::Mutex::new(None);

/// appinfo.vdf de la machine, relu seulement s'il a change (3 Mo environ).
pub fn cached(path: &Path) -> Option<std::sync::Arc<AppInfo>> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, t, a)) = g.as_ref() {
        if p == path && *t == mtime {
            return Some(a.clone());
        }
    }
    let a = std::sync::Arc::new(AppInfo::load(path).ok()?);
    *g = Some((path.to_path_buf(), mtime, a.clone()));
    Some(a)
}

// ------------------------------------------------------------------ ecriture (tests et outils)

/// Encodeur minimal (tests) : produit un appinfo.vdf de la version demandee avec les applications donnees.
pub mod writer {
    use crate::detect::Vdf;

    fn section(out: &mut Vec<u8>, items: &[(String, Vdf)], keys: &mut Option<Vec<String>>) {
        for (k, v) in items {
            let ty = match v {
                Vdf::Map(_) => 0u8,
                Vdf::Str(_) => 1u8,
            };
            out.push(ty);
            match keys {
                Some(t) => {
                    let i = t.iter().position(|x| x == k).unwrap_or_else(|| {
                        t.push(k.clone());
                        t.len() - 1
                    });
                    out.extend_from_slice(&(i as u32).to_le_bytes());
                }
                None => {
                    out.extend_from_slice(k.as_bytes());
                    out.push(0);
                }
            }
            match v {
                Vdf::Map(m) => section(out, m, keys),
                Vdf::Str(s) => {
                    out.extend_from_slice(s.as_bytes());
                    out.push(0);
                }
            }
        }
        out.push(0x08);
    }

    /// `apps` : (appid, contenu de la section `appinfo`).
    pub fn appinfo(version: u32, apps: &[(u32, Vdf)]) -> Vec<u8> {
        let magic: u32 = 0x0756_4400 + 0x27 + (version - 27);
        let mut keys: Option<Vec<String>> = if version >= 29 { Some(Vec::new()) } else { None };
        let mut body = Vec::new();
        for (id, v) in apps {
            let mut kv = Vec::new();
            section(&mut kv, &[("appinfo".to_string(), v.clone())], &mut keys);
            let fixed = if version >= 28 { 60 } else { 40 };
            body.extend_from_slice(&id.to_le_bytes());
            body.extend_from_slice(&((kv.len() + fixed) as u32).to_le_bytes());
            body.extend(std::iter::repeat_n(0u8, fixed));
            body.extend_from_slice(&kv);
        }
        body.extend_from_slice(&0u32.to_le_bytes());
        let mut out = Vec::new();
        out.extend_from_slice(&magic.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        if let Some(k) = keys {
            let header = 16usize;
            let off = (header + body.len()) as u64;
            out.extend_from_slice(&off.to_le_bytes());
            out.extend_from_slice(&body);
            out.extend_from_slice(&(k.len() as u32).to_le_bytes());
            for s in k {
                out.extend_from_slice(s.as_bytes());
                out.push(0);
            }
        } else {
            out.extend_from_slice(&body);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: Vec<(&str, Vdf)>) -> Vdf {
        Vdf::Map(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }
    fn st(s: &str) -> Vdf {
        Vdf::Str(s.to_string())
    }

    fn sample() -> Vdf {
        m(vec![
            ("appid", st("413150")),
            ("common", m(vec![("name", st("Stardew Valley")), ("associations", m(vec![("0", m(vec![("type", st("publisher")), ("name", st("ConcernedApe"))]))]))])),
            (
                "ufs",
                m(vec![
                    ("quota", st("100000000")),
                    ("savefiles", m(vec![("0", m(vec![("root", st("WinAppDataRoaming")), ("path", st("StardewValley/Saves")), ("pattern", st("*")), ("recursive", st("1")), ("platforms", m(vec![("1", st("Windows"))]))]))])),
                    ("rootoverrides", m(vec![("0", m(vec![("root", st("WinAppDataRoaming")), ("os", st("Linux")), ("newroot", st("LinuxXdgConfigHome")), ("addpath", st("")), ("useinstead", st("1"))]))])),
                ]),
            ),
        ])
    }

    #[test]
    fn lit_les_versions_27_28_29() {
        for ver in [27u32, 28, 29] {
            let bytes = writer::appinfo(ver, &[(10, m(vec![("common", m(vec![("name", st("Autre"))]))])), (413150, sample())]);
            let a = AppInfo::parse(bytes).unwrap_or_else(|e| panic!("v{} : {}", ver, e));
            assert_eq!(a.version(), ver);
            assert_eq!(a.len(), 2);
            let u = ufs_of(&a.app(413150).unwrap());
            assert_eq!(u.name.as_deref(), Some("Stardew Valley"), "v{}", ver);
            assert_eq!(u.publisher.as_deref(), Some("ConcernedApe"));
            assert_eq!(u.quota, 100_000_000);
            assert_eq!(u.savefiles, vec![SaveRule { root: "WinAppDataRoaming".into(), path: "StardewValley/Saves".into(), pattern: "*".into(), platforms: vec!["Windows".into()], recursive: true }]);
            assert_eq!(u.overrides.len(), 1);
            assert_eq!(u.overrides[0].new_root, "LinuxXdgConfigHome");
            assert!(u.overrides[0].use_instead);
            assert!(a.app(999).is_none());
        }
    }

    #[test]
    fn types_numeriques_et_fin_alternative() {
        // Section ecrite a la main : entier 32 bits, 64 bits, flottant, chaine large, fin 0x0B.
        let mut b = Vec::new();
        b.push(0x02);
        b.extend_from_slice(b"n\0");
        b.extend_from_slice(&(-5i32).to_le_bytes());
        b.push(0x07);
        b.extend_from_slice(b"u\0");
        b.extend_from_slice(&76561198000000000u64.to_le_bytes());
        b.push(0x03);
        b.extend_from_slice(b"f\0");
        b.extend_from_slice(&1.5f32.to_bits().to_le_bytes());
        b.push(0x05);
        b.extend_from_slice(b"w\0");
        for u in "Été".encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        b.extend_from_slice(&[0, 0]);
        b.push(0x0B);
        let v = parse_binary_kv(&b).unwrap();
        assert_eq!(v.get("n").and_then(|x| x.as_str()), Some("-5"));
        assert_eq!(v.get("u").and_then(|x| x.as_str()), Some("76561198000000000"));
        assert_eq!(v.get("f").and_then(|x| x.as_str()), Some("1.5"));
        assert_eq!(v.get("w").and_then(|x| x.as_str()), Some("Été"));
    }

    #[test]
    fn fichier_corrompu_ou_tronque_sans_panique() {
        assert!(AppInfo::parse(vec![]).is_err());
        assert!(AppInfo::parse(vec![1, 2, 3, 4, 5, 6, 7, 8]).is_err(), "nombre magique inconnu");
        let good = writer::appinfo(29, &[(413150, sample()), (20, sample())]);
        // Toutes les troncatures : jamais de panique ; ce qui est complet reste lisible.
        for cut in 0..good.len() {
            let _ = AppInfo::parse(good[..cut].to_vec()).map(|a| a.app(413150));
        }
        // Octets aleatoires dans les donnees : jamais de panique.
        let mut bad = writer::appinfo(28, &[(413150, sample())]);
        for i in (20..bad.len()).step_by(7) {
            bad[i] = bad[i].wrapping_mul(31).wrapping_add(7);
        }
        let _ = AppInfo::parse(bad).map(|a| a.app(413150));
        // Imbrication excessive : refusee proprement.
        let mut deep = Vec::new();
        for _ in 0..200 {
            deep.push(0x00);
            deep.extend_from_slice(b"a\0");
        }
        assert!(parse_binary_kv(&deep).is_err());
        // Index de cle hors table (version 29).
        let mut v29 = writer::appinfo(29, &[(7, sample())]);
        let pos = 16 + 8 + 60 + 1; // premier index de cle de la premiere entree
        v29[pos..pos + 4].copy_from_slice(&9999u32.to_le_bytes());
        let a = AppInfo::parse(v29).unwrap();
        assert!(a.app(7).is_none());
    }
}
