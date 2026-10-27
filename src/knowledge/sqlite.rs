//! Lecteur SQLite minimal en LECTURE SEULE (aucune dependance) : table `sqlite_schema`, parcours des arbres B des
//! tables, enregistrements, debordements et journal WAL (pages les plus recentes validees). Sert a lire les dossiers de
//! sauvegarde cloud de GOG Galaxy. Robuste : bornes verifiees, cycles de pages refuses, aucune panique.
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Valeur d'une colonne.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Text(t) => t.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }
}

pub struct Db {
    data: Vec<u8>,
    page_size: usize,
    usable: usize,
    /// Pages remplacees par le journal WAL (numero -> contenu).
    wal: HashMap<u32, Vec<u8>>,
    utf16: Option<bool>, // None = UTF-8 ; Some(true) = UTF-16LE ; Some(false) = UTF-16BE
}

fn be16(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]) as usize)
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// Entier variable SQLite (1 a 9 octets) : (valeur, octets lus).
fn varint(b: &[u8], i: usize) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for k in 0..9 {
        let c = *b.get(i + k)?;
        if k == 8 {
            v = (v << 8) | c as u64;
            return Some((v, 9));
        }
        v = (v << 7) | (c & 0x7f) as u64;
        if c & 0x80 == 0 {
            return Some((v, k + 1));
        }
    }
    None
}

impl Db {
    pub fn open(path: &Path) -> Result<Db, String> {
        let data = std::fs::read(path).map_err(|e| format!("{} : {}", path.display(), e))?;
        let mut wal_path = path.as_os_str().to_owned();
        wal_path.push("-wal");
        let wal = std::fs::read(std::path::PathBuf::from(wal_path)).ok();
        Db::from_bytes(data, wal.as_deref())
    }

    pub fn from_bytes(data: Vec<u8>, wal: Option<&[u8]>) -> Result<Db, String> {
        if data.len() < 100 || &data[..16] != b"SQLite format 3\0" {
            return Err("pas une base SQLite".into());
        }
        let ps = be16(&data, 16).ok_or("en-tete tronque")?;
        let page_size = if ps == 1 { 65536 } else { ps };
        if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err("taille de page invalide".into());
        }
        let reserved = data[20] as usize;
        let usable = page_size.saturating_sub(reserved);
        if usable < 480 {
            return Err("taille utile invalide".into());
        }
        let utf16 = match be32(&data, 56).unwrap_or(1) {
            2 => Some(true),
            3 => Some(false),
            _ => None,
        };
        let mut db = Db { data, page_size, usable, wal: HashMap::new(), utf16 };
        if let Some(w) = wal {
            db.wal = read_wal(w, page_size);
        }
        Ok(db)
    }

    fn page(&self, n: u32) -> Option<&[u8]> {
        if n == 0 {
            return None;
        }
        if let Some(p) = self.wal.get(&n) {
            return Some(p);
        }
        let start = (n as usize - 1).checked_mul(self.page_size)?;
        self.data.get(start..start + self.page_size)
    }

    /// Lignes d'une table (rowid, valeurs), en parcourant l'arbre B a partir de `root`.
    fn rows(&self, root: u32) -> Vec<(i64, Vec<Value>)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        while let Some(pn) = stack.pop() {
            if !seen.insert(pn) || seen.len() > 1_000_000 {
                continue; // cycle ou base demesuree
            }
            let Some(page) = self.page(pn) else { continue };
            let h = if pn == 1 { 100 } else { 0 };
            let Some(&kind) = page.get(h) else { continue };
            let Some(ncell) = be16(page, h + 3) else { continue };
            match kind {
                0x05 => {
                    // Interieur de table : enfants gauches de chaque cellule puis pointeur de droite.
                    let mut kids = Vec::new();
                    for i in 0..ncell {
                        let Some(off) = be16(page, h + 12 + 2 * i) else { break };
                        if let Some(c) = be32(page, off) {
                            kids.push(c);
                        }
                    }
                    if let Some(r) = be32(page, h + 8) {
                        kids.push(r);
                    }
                    // Ordre naturel des lignes (pile : on empile a l'envers).
                    stack.extend(kids.into_iter().rev());
                }
                0x0D => {
                    for i in 0..ncell {
                        let Some(off) = be16(page, h + 8 + 2 * i) else { break };
                        if let Some(r) = self.leaf_cell(page, off) {
                            out.push(r);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn leaf_cell(&self, page: &[u8], off: usize) -> Option<(i64, Vec<Value>)> {
        let (plen, a) = varint(page, off)?;
        let (rowid, b) = varint(page, off + a)?;
        let p = plen as usize;
        if p > 64 * 1024 * 1024 {
            return None;
        }
        let start = off + a + b;
        let u = self.usable;
        let x = u - 35;
        let payload: Vec<u8> = if p <= x {
            page.get(start..start + p)?.to_vec()
        } else {
            let m = ((u - 12) * 32 / 255) - 23;
            let k = m + ((p - m) % (u - 4));
            let local = if k <= x { k } else { m };
            let mut v = page.get(start..start + local)?.to_vec();
            let mut next = be32(page, start + local)?;
            let mut guard = 0;
            while v.len() < p && next != 0 && guard < 100_000 {
                guard += 1;
                let op = self.page(next)?;
                next = be32(op, 0)?;
                let take = (p - v.len()).min(u - 4);
                v.extend_from_slice(op.get(4..4 + take)?);
            }
            if v.len() < p {
                return None;
            }
            v
        };
        Some((rowid as i64, self.record(&payload)?))
    }

    fn text(&self, b: &[u8]) -> String {
        match self.utf16 {
            None => String::from_utf8_lossy(b).to_string(),
            Some(le) => {
                let units: Vec<u16> = b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
                String::from_utf16_lossy(&units)
            }
        }
    }

    fn record(&self, p: &[u8]) -> Option<Vec<Value>> {
        let (hlen, n) = varint(p, 0)?;
        let hlen = hlen as usize;
        if hlen > p.len() {
            return None;
        }
        let mut types = Vec::new();
        let mut i = n;
        while i < hlen {
            let (t, k) = varint(p, i)?;
            types.push(t);
            i += k;
        }
        let mut pos = hlen;
        let mut out = Vec::with_capacity(types.len());
        for t in types {
            let int = |len: usize, pos: usize| -> Option<i64> {
                let s = p.get(pos..pos + len)?;
                let mut v: i64 = if s[0] & 0x80 != 0 { -1 } else { 0 };
                for &c in s {
                    v = (v << 8) | c as i64;
                }
                Some(v)
            };
            let (val, len) = match t {
                0 => (Value::Null, 0),
                1 => (Value::Int(int(1, pos)?), 1),
                2 => (Value::Int(int(2, pos)?), 2),
                3 => (Value::Int(int(3, pos)?), 3),
                4 => (Value::Int(int(4, pos)?), 4),
                5 => (Value::Int(int(6, pos)?), 6),
                6 => (Value::Int(int(8, pos)?), 8),
                7 => {
                    let s = p.get(pos..pos + 8)?;
                    let mut a = [0u8; 8];
                    a.copy_from_slice(s);
                    (Value::Real(f64::from_be_bytes(a)), 8)
                }
                8 => (Value::Int(0), 0),
                9 => (Value::Int(1), 0),
                t if t >= 12 && t % 2 == 0 => {
                    let l = ((t - 12) / 2) as usize;
                    (Value::Blob(p.get(pos..pos + l)?.to_vec()), l)
                }
                t if t >= 13 => {
                    let l = ((t - 13) / 2) as usize;
                    (Value::Text(self.text(p.get(pos..pos + l)?)), l)
                }
                _ => return None,
            };
            out.push(val);
            pos += len;
        }
        Some(out)
    }

    /// (nom, page racine, SQL) des tables du schema.
    pub fn tables(&self) -> Vec<(String, u32, String)> {
        self.rows(1)
            .into_iter()
            .filter(|(_, v)| v.first().and_then(|x| x.as_text()) == Some("table"))
            .filter_map(|(_, v)| Some((v.get(1)?.as_text()?.to_string(), v.get(3)?.as_i64()? as u32, v.get(4).and_then(|x| x.as_text()).unwrap_or("").to_string())))
            .collect()
    }

    /// Lignes d'une table, colonnes nommees (d'apres son CREATE TABLE). Une colonne INTEGER PRIMARY KEY recoit le rowid.
    pub fn table(&self, name: &str) -> Option<Vec<HashMap<String, Value>>> {
        let (_, root, sql) = self.tables().into_iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name))?;
        let cols = columns(&sql);
        let mut out = Vec::new();
        for (rowid, vals) in self.rows(root) {
            let mut m = HashMap::new();
            for (i, (c, is_rowid)) in cols.iter().enumerate() {
                let v = vals.get(i).cloned().unwrap_or(Value::Null);
                let v = if *is_rowid && v == Value::Null { Value::Int(rowid) } else { v };
                m.insert(c.clone(), v);
            }
            out.push(m);
        }
        Some(out)
    }
}

/// Journal WAL : pour chaque page, la version la plus recente appartenant a une transaction validee.
fn read_wal(w: &[u8], page_size: usize) -> HashMap<u32, Vec<u8>> {
    let mut out = HashMap::new();
    if w.len() < 32 {
        return out;
    }
    let magic = be32(w, 0).unwrap_or(0);
    if magic != 0x377f_0682 && magic != 0x377f_0683 {
        return out;
    }
    if be32(w, 8).map(|p| p as usize) != Some(page_size) {
        return out;
    }
    let (s1, s2) = (be32(w, 16), be32(w, 20));
    let frame = 24 + page_size;
    let mut pending: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut pos = 32;
    while pos + frame <= w.len() {
        let pn = be32(w, pos).unwrap_or(0);
        let commit = be32(w, pos + 4).unwrap_or(0);
        if be32(w, pos + 8) != s1 || be32(w, pos + 12) != s2 || pn == 0 {
            break; // trame d'une ancienne generation : fin du journal valide
        }
        pending.push((pn, w[pos + 24..pos + frame].to_vec()));
        if commit != 0 {
            for (n, d) in pending.drain(..) {
                out.insert(n, d);
            }
        }
        pos += frame;
    }
    out
}

/// Noms des colonnes d'un CREATE TABLE, dans l'ordre ; vrai pour la colonne INTEGER PRIMARY KEY (alias du rowid).
pub fn columns(sql: &str) -> Vec<(String, bool)> {
    let Some(open) = sql.find('(') else { return vec![] };
    let body = &sql[open + 1..];
    let mut parts = Vec::new();
    let (mut depth, mut cur, mut quote) = (0i32, String::new(), None::<char>);
    for c in body.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
                cur.push(c);
                continue;
            }
            None => {}
        }
        match c {
            '\'' | '"' | '`' => {
                quote = Some(c);
                cur.push(c);
            }
            '[' => {
                quote = Some(']');
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
                break;
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    let mut out = Vec::new();
    for p in parts {
        let t = p.trim();
        let up = t.to_uppercase();
        if ["CONSTRAINT", "PRIMARY", "UNIQUE", "FOREIGN", "CHECK"].iter().any(|k| up.starts_with(k)) {
            continue;
        }
        let name: String = if let Some(q) = t.chars().next().filter(|c| matches!(c, '\'' | '"' | '`' | '[')) {
            let close = if q == '[' { ']' } else { q };
            t[1..].split(close).next().unwrap_or("").to_string()
        } else {
            t.split_whitespace().next().unwrap_or("").to_string()
        };
        if name.is_empty() {
            continue;
        }
        let rest = up.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
        out.push((name, rest.starts_with("INTEGER") && rest.contains("PRIMARY KEY")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colonnes_du_create_table() {
        let c = columns("CREATE TABLE 'CloudSavesLocations'(\n'productId' INTEGER NOT NULL,\n'location' TEXT NOT NULL,\n'name' TEXT NOT NULL,\n'regex' TEXT,\nCONSTRAINT 'FK' FOREIGN KEY ('productId') REFERENCES 'X' ('productId'))");
        assert_eq!(c.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["productId", "location", "name", "regex"]);
        let c = columns("CREATE TABLE LimitedDetails(\n'id' INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,\n'title' TEXT, \"x y\" TEXT DEFAULT ('a,b'))");
        assert_eq!(c, vec![("id".to_string(), true), ("title".to_string(), false), ("x y".to_string(), false)]);
    }

    #[test]
    fn varints() {
        assert_eq!(varint(&[0x05], 0), Some((5, 1)));
        assert_eq!(varint(&[0x81, 0x00], 0), Some((128, 2)));
        assert_eq!(varint(&[0xff; 9], 0).map(|x| x.1), Some(9));
        assert_eq!(varint(&[0x81], 0), None);
    }

    #[test]
    fn donnees_invalides_sans_panique() {
        assert!(Db::from_bytes(vec![0; 50], None).is_err());
        let mut h = b"SQLite format 3\0".to_vec();
        h.resize(4096, 0);
        h[16] = 0x10; // 4096
        let db = Db::from_bytes(h.clone(), Some(&[1, 2, 3])).unwrap();
        assert!(db.tables().is_empty());
        // Page 1 declaree « interieure » qui pointe vers elle-meme : pas de boucle infinie.
        let mut c = h;
        c[100] = 0x05;
        c[103] = 0;
        c[104] = 0;
        c[108..112].copy_from_slice(&1u32.to_be_bytes());
        let db = Db::from_bytes(c, None).unwrap();
        assert!(db.tables().is_empty());
    }
}
