//! Acces direct au registre Windows (API native). Remplace les appels a `reg.exe`, qui ouvraient
//! une fenetre de terminal puisque l'exe est en sous-systeme "windows".
//! Tout est "best effort" : echec => `None` / `false`, jamais de panique.
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    fn raw(self) -> HKEY {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Cle ouverte, refermee automatiquement.
struct Key(HKEY);

impl Key {
    fn open(hive: Hive, path: &str, access: REG_SAM_FLAGS) -> Option<Key> {
        let p = wide(path);
        let mut h: HKEY = std::ptr::null_mut();
        let r = unsafe { RegOpenKeyExW(hive.raw(), p.as_ptr(), 0, access, &mut h) };
        if r == ERROR_SUCCESS {
            Some(Key(h))
        } else {
            None
        }
    }

    /// Ouvre la cle en ecriture, en la creant si besoin.
    fn create(hive: Hive, path: &str) -> Option<Key> {
        let p = wide(path);
        let mut h: HKEY = std::ptr::null_mut();
        let r = unsafe { RegCreateKeyExW(hive.raw(), p.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE | KEY_QUERY_VALUE, std::ptr::null(), &mut h, std::ptr::null_mut()) };
        if r == ERROR_SUCCESS {
            Some(Key(h))
        } else {
            None
        }
    }

    /// (type, octets bruts) d'une valeur.
    fn raw_value(&self, name: &str) -> Option<(u32, Vec<u8>)> {
        let n = wide(name);
        let mut ty: u32 = 0;
        let mut len: u32 = 0;
        let r = unsafe { RegQueryValueExW(self.0, n.as_ptr(), std::ptr::null(), &mut ty, std::ptr::null_mut(), &mut len) };
        if r != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; len as usize + 2];
        let mut len2 = len;
        let r = unsafe { RegQueryValueExW(self.0, n.as_ptr(), std::ptr::null(), &mut ty, buf.as_mut_ptr(), &mut len2) };
        if r != ERROR_SUCCESS {
            return None;
        }
        buf.truncate(len2 as usize);
        Some((ty, buf))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

/// Octets UTF-16 LE d'une valeur REG_SZ -> texte (NUL de fin et suivants retires).
pub fn decode_reg_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// Valeur texte (REG_SZ / REG_EXPAND_SZ non developpee).
pub fn read_string(hive: Hive, path: &str, name: &str) -> Option<String> {
    let k = Key::open(hive, path, KEY_QUERY_VALUE)?;
    let (ty, data) = k.raw_value(name)?;
    if ty == REG_SZ || ty == REG_EXPAND_SZ {
        Some(decode_reg_string(&data))
    } else {
        None
    }
}

/// Valeur entiere 32 bits.
pub fn read_dword(hive: Hive, path: &str, name: &str) -> Option<u32> {
    let k = Key::open(hive, path, KEY_QUERY_VALUE)?;
    let (ty, data) = k.raw_value(name)?;
    if ty == REG_DWORD && data.len() >= 4 {
        Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    } else {
        None
    }
}

/// La valeur existe-t-elle (quel que soit son type) ?
pub fn value_exists(hive: Hive, path: &str, name: &str) -> bool {
    Key::open(hive, path, KEY_QUERY_VALUE).and_then(|k| k.raw_value(name)).is_some()
}

/// Noms des sous-cles (vide si la cle n'existe pas).
pub fn subkeys(hive: Hive, path: &str) -> Vec<String> {
    let Some(k) = Key::open(hive, path, KEY_ENUMERATE_SUB_KEYS) else { return vec![] };
    let mut out = Vec::new();
    for i in 0u32.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(k.0, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        if r != ERROR_SUCCESS {
            break; // ERROR_NO_MORE_ITEMS ou erreur : on s'arrete
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

/// Ecrit une valeur REG_SZ (cree la cle si besoin).
pub fn write_string(hive: Hive, path: &str, name: &str, value: &str) -> bool {
    let Some(k) = Key::create(hive, path) else { return false };
    let n = wide(name);
    let v = wide(value);
    let bytes = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2) };
    unsafe { RegSetValueExW(k.0, n.as_ptr(), 0, REG_SZ, bytes.as_ptr(), bytes.len() as u32) == ERROR_SUCCESS }
}

/// La cle existe-t-elle ?
pub fn key_exists(hive: Hive, path: &str) -> bool {
    Key::open(hive, path, KEY_QUERY_VALUE).is_some()
}

/// Toutes les valeurs d'une cle : (nom, type, octets bruts). None = cle absente. Nom vide = valeur par defaut.
pub fn values(hive: Hive, path: &str) -> Option<Vec<(String, u32, Vec<u8>)>> {
    let k = Key::open(hive, path, KEY_QUERY_VALUE)?;
    let mut out = Vec::new();
    for i in 0u32..4096 {
        let mut name = vec![0u16; 16_384];
        let mut nlen = name.len() as u32;
        let mut ty: u32 = 0;
        let mut dlen: u32 = 0;
        let r = unsafe { RegEnumValueW(k.0, i, name.as_mut_ptr(), &mut nlen, std::ptr::null(), &mut ty, std::ptr::null_mut(), &mut dlen) };
        if r != ERROR_SUCCESS {
            break; // ERROR_NO_MORE_ITEMS ou erreur
        }
        let n = String::from_utf16_lossy(&name[..nlen as usize]);
        if let Some((t, data)) = k.raw_value(&n) {
            out.push((n, t, data));
        }
    }
    Some(out)
}

/// Ecrit une valeur brute (type + octets), en creant la cle si besoin.
pub fn write_raw(hive: Hive, path: &str, name: &str, ty: u32, data: &[u8]) -> bool {
    let Some(k) = Key::create(hive, path) else { return false };
    let n = wide(name);
    unsafe { RegSetValueExW(k.0, n.as_ptr(), 0, ty, data.as_ptr(), data.len() as u32) == ERROR_SUCCESS }
}

/// Supprime une cle de TEST et tout son contenu. Refuse tout chemin hors de `Software\AGSS-Test` (jamais une vraie cle).
pub fn delete_test_tree(path: &str) -> bool {
    let low = path.to_ascii_lowercase();
    if !low.starts_with("software\\agss-test\\") || low.contains("..") {
        return false;
    }
    let p = wide(path);
    let ok = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr()) } == ERROR_SUCCESS;
    ok && !key_exists(Hive::CurrentUser, path)
}

/// Supprime une valeur. true si elle n'existe plus apres l'appel (y compris si elle n'existait pas).
pub fn delete_value(hive: Hive, path: &str, name: &str) -> bool {
    let Some(k) = Key::open(hive, path, KEY_SET_VALUE | KEY_QUERY_VALUE) else {
        // Cle absente : rien a supprimer.
        return Key::open(hive, path, KEY_QUERY_VALUE).is_none();
    };
    let n = wide(name);
    let r = unsafe { RegDeleteValueW(k.0, n.as_ptr()) };
    r == ERROR_SUCCESS || !value_exists(hive, path, name)
}
