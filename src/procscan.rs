//! Detection de processus sous Linux a partir de `/proc` (aucun sous-processus). Le parcours prend la racine en parametre
//! pour pouvoir etre teste sur un faux `/proc`. Compile partout, n'est utilisee que sous Linux.
//!
//! Cas reconnus :
//! - programme natif : cible du lien `exe`, `argv[0]`, nom court `comm` (tronque a 15 caracteres par le noyau) ;
//! - jeu Windows sous Proton / Wine : le processus Linux est `wine64-preloader` ; le nom du jeu est dans `comm` et dans la
//!   ligne de commande (`Z:\...\Jeu.exe` ou `C:\...\Jeu.exe`, traduit avec le `WINEPREFIX` du processus) ;
//! - script lance par un interpreteur (`bash jeu.sh`, `python3 jeu.py`, `java -jar jeu.jar`) : le nom du script ;
//! - AppImage : le fichier `.AppImage` (variable `APPIMAGE`), jamais le point de montage temporaire `/tmp/.mount_*` ;
//! - application Flatpak : le programme de `/app/bin`, traduit vers son chemin reel sur la machine.
use crate::proton;
use std::path::{Path, PathBuf};

/// Un processus tel que lu dans `/proc/<pid>`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RawProc {
    pub pid: u32,
    /// Nom court (`comm`, 15 caracteres au plus).
    pub comm: String,
    /// Cible du lien `exe` (None si illisible).
    pub exe_link: Option<PathBuf>,
    /// Arguments de la ligne de commande.
    pub cmdline: Vec<String>,
    /// Heure de demarrage en ticks depuis le boot (champ 22 de `stat`).
    pub start: u64,
    /// Dossier courant (lien `cwd`).
    pub cwd: Option<PathBuf>,
    /// Prefixe Wine du processus (`WINEPREFIX`, lu seulement pour les processus Wine).
    pub wineprefix: Option<PathBuf>,
    /// Fichier AppImage d'origine (`APPIMAGE`, lu seulement pour les processus d'un montage d'AppImage).
    pub appimage: Option<PathBuf>,
    /// Identifiant Flatpak (lu dans `cgroup`, seulement pour les programmes de `/app`).
    pub flatpak: Option<String>,
    /// Processus parent (champ 4 de `stat`).
    pub ppid: u32,
    /// Terminal de controle (champ 7 de `stat` non nul) : lance depuis un terminal.
    pub tty: bool,
}

fn base_name(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or("")
}

fn is_wine_loader(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("wine") || n == "proton" || n.starts_with("pressure-vessel") || n.contains("preloader")
}

/// Interpreteurs : le programme utile est le script passe en argument.
const INTERPRETERS: &[&str] = &["sh", "bash", "dash", "zsh", "fish", "python", "python2", "python3", "perl", "ruby", "lua", "luajit", "node", "java", "mono", "dotnet", "love", "godot", "godot3", "godot4"];

fn is_interpreter(name: &str) -> bool {
    let n = name.to_lowercase();
    INTERPRETERS.contains(&n.as_str()) || (n.starts_with("python3.") && n[8..].chars().all(|c| c.is_ascii_digit()))
}

fn exe_base(p: &RawProc) -> String {
    p.exe_link.as_ref().map(|e| base_name(e.to_string_lossy().trim_end_matches(" (deleted)")).to_lowercase()).unwrap_or_default()
}

/// Script lance par un interpreteur (premier argument qui n'est pas une option ; `-jar x.jar` pour Java).
pub fn script_arg(p: &RawProc) -> Option<&str> {
    let b = exe_base(p);
    let b = if b.is_empty() { p.cmdline.first().map(|a| base_name(a).to_lowercase()).unwrap_or_default() } else { b };
    if !is_interpreter(&b) {
        return None;
    }
    let args = p.cmdline.get(1..)?;
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if a == "-jar" {
            return args.get(i + 1).map(String::as_str);
        }
        if a == "-c" || a == "-m" || a == "-e" {
            return None; // code en ligne : aucun fichier
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(a.as_str());
    }
    None
}

/// Noms (minuscules) sous lesquels le processus peut etre reconnu : cible de `exe`, `argv[0]`, `comm`, tout argument en
/// `.exe`, l'AppImage d'origine et le script d'un interpreteur.
pub fn candidate_names(p: &RawProc) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        let b = base_name(s.trim().trim_matches('"')).trim().to_lowercase();
        if !b.is_empty() && !v.contains(&b) {
            v.push(b);
        }
    };
    if let Some(e) = &p.exe_link {
        // « /chemin/jeu (deleted) » : on retire le suffixe ajoute par le noyau.
        add(e.to_string_lossy().trim_end_matches(" (deleted)"));
    }
    if let Some(a0) = p.cmdline.first() {
        add(a0);
    }
    add(&p.comm);
    for a in &p.cmdline {
        let low = a.to_lowercase();
        if low.ends_with(".exe") || low.ends_with(".exe\"") {
            add(a);
        }
    }
    if let Some(ai) = &p.appimage {
        add(&ai.to_string_lossy());
    }
    if let Some(s) = script_arg(p) {
        add(s);
    }
    v
}

/// Le nom voulu (normalise : minuscules, sans chemin) correspond-il a l'un des noms du processus ? Le nom court du noyau est
/// tronque a 15 caracteres : un nom voulu plus long est donc aussi compare a ce prefixe.
pub fn matches_wanted(p: &RawProc, wanted: &str) -> bool {
    let comm = p.comm.to_lowercase();
    if candidate_names(p).iter().any(|n| n == wanted) {
        return true;
    }
    comm.chars().count() >= 15 && wanted.starts_with(&comm)
}

/// Point de montage temporaire d'une AppImage (`/tmp/.mount_XXXX/...`) : jamais un chemin stable.
pub fn in_appimage_mount(p: &Path) -> bool {
    p.to_string_lossy().contains("/.mount_")
}

/// Nom d'exe a afficher / classer : l'exe Windows du jeu pour un processus Wine, l'AppImage, le script d'un interpreteur,
/// sinon la cible de `exe`, sinon `comm`.
pub fn display_exe(p: &RawProc) -> String {
    let names = candidate_names(p);
    let eb = exe_base(p);
    if eb.is_empty() || is_wine_loader(&eb) {
        if let Some(n) = names.iter().find(|n| n.ends_with(".exe")) {
            return n.clone();
        }
    }
    if let Some(ai) = &p.appimage {
        return base_name(&ai.to_string_lossy()).to_lowercase();
    }
    if let Some(s) = script_arg(p) {
        return base_name(s).to_lowercase();
    }
    if !eb.is_empty() {
        return eb;
    }
    names.into_iter().next().unwrap_or_default()
}

/// Chemin complet connu du programme (None : inconnu ou instable, le processus n'est alors jamais propose) :
/// - Wine / Proton : `Z:\...` traduit, `C:\...` traduit dans le `WINEPREFIX`, sinon un chemin Linux absolu ou relatif au
///   dossier courant ; jamais le chargeur Wine lui-meme ;
/// - AppImage : le fichier `.AppImage` ; Flatpak : le chemin reel du programme ; script : le script ;
/// - sinon la cible de `exe`.
pub fn display_path_with(p: &RawProc, flatpak_roots: &[PathBuf]) -> Option<PathBuf> {
    let eb = exe_base(p);
    if eb.is_empty() || is_wine_loader(&eb) {
        for a in &p.cmdline {
            if !a.to_lowercase().ends_with(".exe") {
                continue;
            }
            if let Some(x) = proton::translate_windows_path(a, Path::new("/nonexistent-prefix")) {
                if !x.starts_with("/nonexistent-prefix") {
                    return Some(x);
                }
                if let Some(pfx) = &p.wineprefix {
                    return proton::translate_windows_path(a, pfx);
                }
                continue;
            }
            if a.starts_with('/') {
                return Some(PathBuf::from(a));
            }
            if let Some(c) = p.cwd.as_ref().map(|c| c.join(a)).filter(|x| x.is_file()) {
                return Some(c);
            }
        }
        // Processus Wine sans .exe exploitable (wineserver, services...) : chemin du chargeur inutile.
        return if eb.is_empty() { None } else if is_wine_loader(&eb) { None } else { p.exe_link.clone() };
    }
    if let Some(ai) = &p.appimage {
        return Some(ai.clone());
    }
    if let Some(s) = script_arg(p) {
        let sp = Path::new(s);
        let full = if sp.is_absolute() { Some(sp.to_path_buf()) } else { p.cwd.as_ref().map(|c| c.join(sp)) };
        if let Some(f) = full.filter(|f| f.is_file()) {
            return Some(f);
        }
    }
    let exe = p.exe_link.as_ref().map(|e| PathBuf::from(e.to_string_lossy().trim_end_matches(" (deleted)")))?;
    if let Some(id) = &p.flatpak {
        if let Some(h) = crate::linuxapps::flatpak_host_path(id, &exe, flatpak_roots) {
            return Some(h);
        }
    }
    if in_appimage_mount(&exe) {
        return None; // le processus parent (le fichier .AppImage) represente deja ce programme
    }
    Some(exe)
}

/// `display_path_with` avec les installations Flatpak de l'utilisateur courant.
pub fn display_path(p: &RawProc) -> Option<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    display_path_with(p, &crate::linuxapps::flatpak_roots(&home))
}

fn read_cmdline(dir: &Path) -> Vec<String> {
    let Ok(b) = std::fs::read(dir.join("cmdline")) else { return vec![] };
    b.split(|&c| c == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect()
}

/// Valeur d'une variable de `environ` (octets separes par des NUL).
pub fn environ_var(environ: &[u8], key: &str) -> Option<String> {
    let k = format!("{}=", key);
    environ.split(|&c| c == 0).find(|e| e.starts_with(k.as_bytes())).map(|e| String::from_utf8_lossy(&e[k.len()..]).into_owned()).filter(|s| !s.is_empty())
}

/// Champ 22 de `stat` (le nom entre parentheses peut contenir des espaces et des parentheses : on coupe apres la derniere).
pub fn parse_stat_start(stat: &str) -> Option<u64> {
    let after = &stat[stat.rfind(')')? + 1..];
    // apres « ) » : etat (champ 3), puis les champs 4.. ; starttime = champ 22 => index 19 apres l'etat.
    after.split_whitespace().nth(19)?.parse().ok()
}

/// (parent, terminal de controle) depuis `stat` : champs 4 (ppid) et 7 (tty_nr, 0 = aucun terminal).
pub fn parse_stat_ppid_tty(stat: &str) -> Option<(u32, bool)> {
    let after = &stat[stat.rfind(')')? + 1..];
    let f: Vec<&str> = after.split_whitespace().collect();
    let ppid = f.get(1)?.parse().ok()?;
    let tty: i64 = f.get(4)?.parse().ok()?;
    Some((ppid, tty != 0))
}

/// Heure de demarrage du systeme (s depuis 1970), ligne `btime` de `/proc/stat`.
pub fn parse_btime(stat: &str) -> Option<u64> {
    stat.lines().find_map(|l| l.strip_prefix("btime ")).and_then(|v| v.trim().parse().ok())
}

/// Bibliotheques graphiques dans `/proc/<pid>/maps` : Vulkan, OpenGL / EGL, ou Direct3D (DXVK / vkd3d sous Proton / Wine).
pub fn maps_have_graphics(maps: &str) -> bool {
    const LIBS: &[&str] = &["libvulkan.so", "libgl.so", "libglx", "libegl.so", "libgles", "d3d9.dll", "d3d10core.dll", "d3d11.dll", "d3d12.dll", "dxgi.dll", "opengl32.dll", "vulkan-1.dll"];
    maps.lines().any(|l| {
        let low = l.to_lowercase();
        LIBS.iter().any(|x| low.contains(x))
    })
}

/// Le processus `pid` a-t-il charge une bibliotheque graphique ? None si `maps` est illisible.
pub fn graphics_loaded(proc_root: &Path, pid: u32) -> Option<bool> {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join("maps")).ok().map(|m| maps_have_graphics(&m))
}

/// Parcourt `proc_root` (normalement `/proc`). Les dossiers non numeriques sont ignores ; un processus qui disparait pendant
/// la lecture est simplement omis. `environ` et `cgroup` ne sont lus que pour les processus Wine, d'AppImage ou Flatpak.
pub fn scan(proc_root: &Path) -> std::io::Result<Vec<RawProc>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(proc_root)?.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(pid) = name.parse::<u32>() else { continue };
        let dir = e.path();
        let comm = std::fs::read_to_string(dir.join("comm")).map(|s| s.trim_end().to_string()).unwrap_or_default();
        let cmdline = read_cmdline(&dir);
        if comm.is_empty() && cmdline.is_empty() {
            continue; // disparu
        }
        let stat = std::fs::read_to_string(dir.join("stat")).unwrap_or_default();
        let start = parse_stat_start(&stat).unwrap_or(0);
        let (ppid, tty) = parse_stat_ppid_tty(&stat).unwrap_or((0, false));
        let exe_link = std::fs::read_link(dir.join("exe")).ok();
        let mut p = RawProc { pid, comm, exe_link, cmdline, start, cwd: std::fs::read_link(dir.join("cwd")).ok(), ppid, tty, ..Default::default() };
        let eb = exe_base(&p);
        let wine = eb.is_empty() || is_wine_loader(&eb) || p.comm.to_lowercase().ends_with(".exe");
        let mount = p.exe_link.as_deref().map(in_appimage_mount).unwrap_or(false);
        if wine || mount {
            if let Ok(env) = std::fs::read(dir.join("environ")) {
                if wine {
                    p.wineprefix = environ_var(&env, "WINEPREFIX").or_else(|| environ_var(&env, "STEAM_COMPAT_DATA_PATH").map(|c| format!("{}/pfx", c.trim_end_matches('/')))).map(PathBuf::from);
                }
                if mount {
                    p.appimage = environ_var(&env, "APPIMAGE").map(PathBuf::from);
                }
            }
        }
        if p.exe_link.as_ref().map(|x| x.starts_with("/app")).unwrap_or(false) {
            p.flatpak = std::fs::read_to_string(dir.join("cgroup")).ok().and_then(|t| crate::linuxapps::flatpak_id_from_cgroup(&t));
        }
        out.push(p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc_(comm: &str, exe: Option<&str>, cmd: &[&str]) -> RawProc {
        RawProc { pid: 1, comm: comm.into(), exe_link: exe.map(PathBuf::from), cmdline: cmd.iter().map(|s| s.to_string()).collect(), start: 0, ..Default::default() }
    }

    #[test]
    fn native_process_is_recognised_by_exe_target() {
        let p = proc_("factorio", Some("/home/deck/games/factorio/bin/x64/factorio"), &["/home/deck/games/factorio/bin/x64/factorio", "--fullscreen"]);
        assert!(matches_wanted(&p, "factorio"));
        assert!(!matches_wanted(&p, "factorio.exe"));
        assert_eq!(display_exe(&p), "factorio");
    }

    #[test]
    fn wine_game_is_recognised_by_exe_in_command_line() {
        let p = proc_(
            "eldenring.exe",
            Some("/home/deck/.local/share/Steam/compatibilitytools.d/GE-Proton9/files/bin/wine64-preloader"),
            &[r"Z:\home\deck\.local\share\Steam\steamapps\common\ELDEN RING\Game\eldenring.exe"],
        );
        assert!(matches_wanted(&p, "eldenring.exe"));
        assert_eq!(display_exe(&p), "eldenring.exe");
        assert_eq!(display_path(&p), Some(PathBuf::from("/home/deck/.local/share/Steam/steamapps/common/ELDEN RING/Game/eldenring.exe")));
    }

    #[test]
    fn wine_c_drive_uses_the_process_prefix_and_never_the_loader() {
        let mut p = proc_("jeu.exe", Some("/usr/lib/wine/wine64-preloader"), &[r"C:\Program Files\Jeu\jeu.exe"]);
        // Sans prefixe connu : aucun chemin (le chargeur Wine n'est jamais pris pour le jeu).
        assert_eq!(display_path(&p), None);
        p.wineprefix = Some(PathBuf::from("/home/deck/Games/Heroic/Prefixes/default/Jeu"));
        assert_eq!(display_path(&p), Some(PathBuf::from("/home/deck/Games/Heroic/Prefixes/default/Jeu/drive_c/Program Files/Jeu/jeu.exe")));
        // Processus systeme de Wine : chemin dans drive_c/windows (exclu ensuite par la detection).
        let mut s = proc_("services.exe", Some("/usr/lib/wine/wine64-preloader"), &[r"C:\windows\system32\services.exe"]);
        s.wineprefix = Some(PathBuf::from("/pfx"));
        assert_eq!(display_path(&s), Some(PathBuf::from("/pfx/drive_c/windows/system32/services.exe")));
        // wineserver : pas de .exe, pas de chemin.
        let w = proc_("wineserver", Some("/home/deck/.steam/steam/steamapps/common/Proton 9.0/files/bin/wineserver"), &["wineserver"]);
        assert_eq!(display_path(&w), None);
    }

    #[test]
    fn scripts_appimages_and_flatpaks() {
        let t = std::env::temp_dir().join(format!("agss-procscan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(t.join("start.sh"), "#!/bin/sh\n").unwrap();
        let mut s = proc_("start.sh", Some("/usr/bin/bash"), &["/bin/bash", "./start.sh", "--x"]);
        s.cwd = Some(t.clone());
        assert!(matches_wanted(&s, "start.sh"));
        assert_eq!(display_exe(&s), "start.sh");
        assert_eq!(display_path(&s), Some(t.join("start.sh")));
        let j = proc_("java", Some("/usr/lib/jvm/java-21/bin/java"), &["java", "-Xmx2G", "-jar", "/opt/jeu/Jeu.jar"]);
        assert_eq!(display_exe(&j), "jeu.jar");
        let c = proc_("python3", Some("/usr/bin/python3.12"), &["python3", "-c", "print(1)"]);
        assert_eq!(display_exe(&c), "python3.12");
        // AppImage : processus interne du montage temporaire -> le fichier .AppImage d'origine.
        let mut a = proc_("Cemu", Some("/tmp/.mount_CemuAb12/usr/bin/Cemu"), &["/tmp/.mount_CemuAb12/usr/bin/Cemu"]);
        assert_eq!(display_path(&a), None);
        a.appimage = Some(PathBuf::from("/home/deck/Applications/Cemu.AppImage"));
        assert_eq!(display_exe(&a), "cemu.appimage");
        assert_eq!(display_path(&a), Some(PathBuf::from("/home/deck/Applications/Cemu.AppImage")));
        assert!(matches_wanted(&a, "cemu.appimage"));
        // Flatpak : /app/bin/x -> chemin reel.
        let fp = t.join("flatpak");
        let files = fp.join("app/org.libretro.RetroArch/current/active/files/bin");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("retroarch"), b"\x7fELF").unwrap();
        let mut f = proc_("retroarch", Some("/app/bin/retroarch"), &["retroarch"]);
        f.flatpak = Some("org.libretro.RetroArch".into());
        assert_eq!(display_path_with(&f, &[fp.clone()]), Some(files.join("retroarch")));
        assert_eq!(display_exe(&f), "retroarch");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn environ_values() {
        let e = b"HOME=/home/deck\0WINEPREFIX=/pfx\0EMPTY=\0";
        assert_eq!(environ_var(e, "WINEPREFIX").as_deref(), Some("/pfx"));
        assert_eq!(environ_var(e, "EMPTY"), None);
        assert_eq!(environ_var(e, "HOM"), None);
    }

    #[test]
    fn truncated_comm_matches_long_names() {
        let p = proc_("verylongnamegame", None, &[]); // 16 car. : le noyau tronque a 15
        let p = RawProc { comm: p.comm[..15].to_string(), ..p };
        assert!(matches_wanted(&p, "verylongnamegame.exe"));
        assert!(!matches_wanted(&p, "otherlongnamegame.exe"));
        // un nom court ne sert pas de prefixe
        let q = proc_("game", None, &[]);
        assert!(!matches_wanted(&q, "game.exe"));
    }

    #[test]
    fn stat_start_field_survives_odd_names() {
        let s = "123 (my (weird) name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 9876543 20";
        assert_eq!(parse_stat_start(s), Some(9876543));
        assert_eq!(parse_stat_start("sans parenthese"), None);
    }

    #[test]
    fn scan_reads_a_fake_proc() {
        let t = std::env::temp_dir().join(format!("agss-fakeproc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let d = t.join("4242");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::create_dir_all(t.join("self-not-a-pid")).unwrap();
        std::fs::write(d.join("comm"), "game.exe\n").unwrap();
        std::fs::write(d.join("cmdline"), b"C:\\games\\game.exe\0--flag\0").unwrap();
        std::fs::write(d.join("environ"), b"A=1\0WINEPREFIX=/home/deck/pfx\0").unwrap();
        std::fs::write(d.join("stat"), "4242 (game.exe) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 555 20").unwrap();
        let v = scan(&t).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].pid, 4242);
        assert_eq!(v[0].start, 555);
        assert_eq!(v[0].cmdline, vec![r"C:\games\game.exe".to_string(), "--flag".to_string()]);
        assert_eq!(v[0].wineprefix, Some(PathBuf::from("/home/deck/pfx")));
        assert!(matches_wanted(&v[0], "game.exe"));
        assert_eq!(display_path(&v[0]), Some(PathBuf::from("/home/deck/pfx/drive_c/games/game.exe")));
        let _ = std::fs::remove_dir_all(&t);
    }
}
