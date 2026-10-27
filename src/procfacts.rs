//! Indices d'execution et de fichier de la detection : sous-systeme PE (console / fenetre), infos de version (editeur,
//! produit), parent, duree de vie, bibliotheques graphiques chargees, dossiers confirmes par les manifestes des lanceurs.
//! Lecture seule, aucun sous-processus ; les indices de fichier sont gardes en cache par chemin.
use crate::gamedetect::{norm_path, Facts};
use crate::process::{ProcExt, ProcInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Sous-systeme PE (2 = fenetre, 3 = console) lu dans l'en-tete d'un exe ; None si ce n'est pas un fichier PE complet.
pub fn pe_subsystem(head: &[u8]) -> Option<u16> {
    if head.len() < 0x40 || &head[..2] != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(head[0x3c..0x40].try_into().ok()?) as usize;
    // signature (4) + en-tete COFF (20) ; le champ Subsystem est a l'octet 68 de l'en-tete optionnel (PE32 et PE32+).
    let at = pe.checked_add(4 + 20 + 68)?;
    if head.get(pe..pe + 4)? != b"PE\0\0" || head.len() < at + 2 {
        return None;
    }
    Some(u16::from_le_bytes([head[at], head[at + 1]]))
}

/// Le fichier est-il un programme console ? Lit au plus 4 Ko. Some(false) = fenetre ; None = autre format ou illisible.
pub fn file_is_console(path: &Path) -> Option<bool> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 4096];
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    buf.truncate(n);
    match pe_subsystem(&buf)? {
        3 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Le processus a-t-il charge une bibliotheque graphique (Direct3D, Vulkan, OpenGL) ? None si la liste des modules est
/// inaccessible (processus protege).
#[cfg(windows)]
pub fn graphics_loaded(pid: u32) -> Option<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    const LIBS: &[&str] = &["ddraw.dll", "d3d8.dll", "d3d9.dll", "d3d10.dll", "d3d10_1.dll", "d3d11.dll", "d3d12.dll", "vulkan-1.dll", "opengl32.dll"];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return None;
        }
        let mut me: MODULEENTRY32W = std::mem::zeroed();
        me.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Module32FirstW(snap, &mut me) != 0;
        let seen = ok;
        while ok {
            let end = me.szModule.iter().position(|&c| c == 0).unwrap_or(me.szModule.len());
            let name = String::from_utf16_lossy(&me.szModule[..end]).to_lowercase();
            if LIBS.contains(&name.as_str()) {
                found = true;
                break;
            }
            ok = Module32NextW(snap, &mut me) != 0;
        }
        CloseHandle(snap);
        if seen {
            Some(found)
        } else {
            None
        }
    }
}

/// Linux : lecture de `/proc/<pid>/maps`.
#[cfg(not(windows))]
pub fn graphics_loaded(pid: u32) -> Option<bool> {
    crate::procscan::graphics_loaded(Path::new("/proc"), pid)
}

/// Le processus tourne-t-il dans une autre session que l'appli (services du systeme : session 0) ? Faux en cas de doute.
#[cfg(windows)]
pub fn other_session(pid: u32) -> bool {
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    let (mut a, mut b) = (0u32, 0u32);
    unsafe {
        if ProcessIdToSessionId(pid, &mut a) == 0 || ProcessIdToSessionId(GetCurrentProcessId(), &mut b) == 0 {
            return false;
        }
    }
    a != b
}

/// Linux : le processus appartient-il a un autre utilisateur (demons du systeme) ? Faux en cas de doute.
#[cfg(not(windows))]
pub fn other_session(pid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(m) = std::fs::metadata(format!("/proc/{}", pid)) else { return false };
    m.uid() != unsafe { libc::getuid() }
}

/// Dossiers d'installation (et noms) confirmes par les manifestes des lanceurs : Steam, Epic, GOG (Windows), Heroic (Linux).
pub fn launcher_manifests() -> Vec<(PathBuf, String)> {
    crate::detect::detect_all().games.into_iter().filter(|g| !crate::detect::is_non_game(g) && g.install_dir.components().count() > 2).map(|g| (g.install_dir, g.name)).collect()
}

/// (console ?, editeur, produit) d'un fichier.
type FileFacts = (Option<bool>, Option<String>, Option<String>);

fn file_facts(path: &Path) -> FileFacts {
    let (company, product) = crate::winutil::exe_company_product(path);
    (file_is_console(path), company, product)
}

/// Delai avant de relire les bibliotheques chargees d'un processus qui n'en avait pas encore (chargement du jeu en cours).
const GFX_RETRY_MS: u64 = 15_000;

/// Cache des indices : fichiers (par chemin), bibliotheques graphiques (par processus), manifestes des lanceurs.
#[derive(Default)]
pub struct FactsCache {
    files: HashMap<PathBuf, FileFacts>,
    gfx: HashMap<(u32, u64), (Option<bool>, u64)>,
    /// Dossiers d'installation confirmes par les lanceurs (voir `launcher_manifests`), recharges par l'appelant.
    pub manifests: Vec<(PathBuf, String)>,
}

impl FactsCache {
    /// L'exe est-il sous un dossier confirme par un lanceur ?
    pub fn under_manifest(&self, p: &ProcInfo) -> bool {
        p.path.as_ref().map(|x| self.manifest_of(&norm_path(x)).is_some()).unwrap_or(false)
    }

    fn manifest_of(&self, np: &str) -> Option<&(PathBuf, String)> {
        self.manifests
            .iter()
            .filter(|(d, _)| {
                let r = norm_path(d);
                let r = r.trim_end_matches('/');
                !r.is_empty() && np.starts_with(&format!("{}/", r))
            })
            .max_by_key(|(d, _)| d.as_os_str().len())
    }

    /// Indices du processus `p`. `table` : liste complete du meme passage (pour le parent), `now_ms` : heure courante.
    pub fn facts(&mut self, p: &ProcInfo, table: &HashMap<u32, ProcExt>, now_ms: u64) -> Facts {
        let mut f = Facts::default();
        let Some(path) = p.path.as_ref() else { return f };
        let (console, company, product) = self.files.entry(path.clone()).or_insert_with(|| file_facts(path)).clone();
        f.console = console;
        f.company = company;
        f.product = product;
        if let Some(me) = table.get(&p.pid) {
            f.tty = me.tty;
            // Parent toujours vivant et plus ancien que l'enfant (un numero de processus peut etre reutilise).
            if let Some(par) = table.get(&me.ppid).filter(|q| q.info.pid != p.pid && q.info.start <= p.start && q.info.start > 0) {
                f.parent = Some(par.info.exe.to_lowercase());
            }
        }
        f.age_ms = crate::process::start_unix_ms(p.start).map(|s| now_ms.saturating_sub(s));
        f.other_user = p.pid != 0 && other_session(p.pid);
        let key = (p.pid, p.start);
        let stale = self.gfx.get(&key).map(|(v, at)| *v != Some(true) && now_ms.saturating_sub(*at) >= GFX_RETRY_MS).unwrap_or(true);
        if stale {
            self.gfx.insert(key, (graphics_loaded(p.pid), now_ms));
        }
        f.graphics = self.gfx.get(&key).and_then(|(v, _)| *v);
        f.manifest = self.manifest_of(&norm_path(path)).cloned();
        f
    }

    /// Oublie les processus disparus (appele a chaque passage avec la liste courante).
    pub fn retain_alive(&mut self, table: &HashMap<u32, ProcExt>) {
        self.gfx.retain(|(pid, st), _| table.get(pid).map(|q| q.info.start == *st).unwrap_or(false));
        if self.files.len() > 4096 {
            self.files.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pe(subsystem: u16) -> Vec<u8> {
        let mut b = vec![0u8; 512];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let at = 0x80 + 4 + 20 + 68;
        b[at..at + 2].copy_from_slice(&subsystem.to_le_bytes());
        b
    }

    #[test]
    fn sous_systeme_pe_console_ou_fenetre() {
        assert_eq!(pe_subsystem(&pe(3)), Some(3));
        assert_eq!(pe_subsystem(&pe(2)), Some(2));
        assert_eq!(pe_subsystem(b"\x7fELF...."), None, "pas un fichier PE");
        let mut tronque = pe(3);
        tronque.truncate(0x90);
        assert_eq!(pe_subsystem(&tronque), None, "en-tete incomplet");
        let mut faux = pe(3);
        faux[0x80] = b'X';
        assert_eq!(pe_subsystem(&faux), None, "signature PE absente");
        let d = std::env::temp_dir().join(format!("agss-pe-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("outil.exe"), pe(3)).unwrap();
        std::fs::write(d.join("jeu.exe"), pe(2)).unwrap();
        std::fs::write(d.join("texte.exe"), b"pas un exe").unwrap();
        assert_eq!(file_is_console(&d.join("outil.exe")), Some(true));
        assert_eq!(file_is_console(&d.join("jeu.exe")), Some(false));
        assert_eq!(file_is_console(&d.join("texte.exe")), None);
        assert_eq!(file_is_console(&d.join("absent.exe")), None);
        let _ = std::fs::remove_file(d.join("outil.exe"));
        let _ = std::fs::remove_file(d.join("jeu.exe"));
        let _ = std::fs::remove_file(d.join("texte.exe"));
        let _ = std::fs::remove_dir(&d);
    }

    #[test]
    fn parent_age_et_manifeste_depuis_la_liste_des_processus() {
        let mk = |pid: u32, ppid: u32, exe: &str, path: &str, start: u64| ProcExt { info: ProcInfo { pid, exe: exe.into(), path: Some(PathBuf::from(path)), start }, ppid, tty: false };
        let shell = mk(10, 1, "cmd.exe", r"C:\Windows\System32\cmd.exe", 100);
        let child = mk(20, 10, "jeu.exe", r"D:\Jeux\Jeu\jeu.exe", 200);
        let orphan = mk(30, 99, "autre.exe", r"D:\Jeux\Autre\autre.exe", 50); // parent disparu
        let reused = mk(40, 50, "x.exe", r"D:\X\x.exe", 10);
        let newer = mk(50, 1, "cmd.exe", r"C:\Windows\System32\cmd.exe", 500); // numero reutilise : plus jeune que l'enfant
        let table: HashMap<u32, ProcExt> = [shell, child.clone(), orphan.clone(), reused.clone(), newer].into_iter().map(|p| (p.info.pid, p)).collect();
        let mut c = FactsCache { manifests: vec![(PathBuf::from(r"D:\Jeux\Jeu"), "Le Jeu".into())], ..Default::default() };
        let f = c.facts(&child.info, &table, 0);
        assert_eq!(f.parent.as_deref(), Some("cmd.exe"));
        assert_eq!(f.manifest.as_ref().map(|m| m.1.as_str()), Some("Le Jeu"));
        assert!(c.under_manifest(&child.info) && !c.under_manifest(&orphan.info));
        assert_eq!(c.facts(&orphan.info, &table, 0).parent, None);
        assert_eq!(c.facts(&reused.info, &table, 0).parent, None, "parent plus recent que l'enfant : numero reutilise");
        c.retain_alive(&table);
    }
}
