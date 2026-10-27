//! Chemins sous Linux / SteamOS : traduction PURE (aucun acces disque dans les fonctions `translate_*`) des chemins et
//! gabarits Windows vers un prefixe Wine / Proton, bibliotheques Steam standard et dossiers XDG des jeux Linux natifs.
//!
//! Compile sur toutes les plateformes pour etre testee partout ; n'est utilisee en pratique que sous Linux.
use std::path::{Path, PathBuf};

/// Nom d'utilisateur Windows utilise par Proton dans ses prefixes.
pub const PROTON_USER: &str = "steamuser";

/// Racine `pfx` du prefixe Proton d'un AppID Steam : `<bibliotheque>/steamapps/compatdata/<appid>/pfx`.
pub fn prefix_dir(library: &Path, appid: u64) -> PathBuf {
    library.join("steamapps").join("compatdata").join(appid.to_string()).join("pfx")
}

/// Premier prefixe Proton EXISTANT de l'AppID dans les bibliotheques Steam donnees.
pub fn find_prefix(libraries: &[PathBuf], appid: u64) -> Option<PathBuf> {
    libraries.iter().map(|l| prefix_dir(l, appid)).find(|p| p.join("drive_c").is_dir())
}

/// Dossier du profil utilisateur Windows dans un prefixe.
pub fn user_dir(pfx: &Path) -> PathBuf {
    pfx.join("drive_c").join("users").join(PROTON_USER)
}

/// Base d'un gabarit universel (`%LOCALAPPDATA%`...) ou d'un marqueur de catalogue (`winAppData`...) dans le prefixe.
/// Les noms sont compares sans casse, avec ou sans `%` / `<` `>`.
pub fn placeholder_base(name: &str, pfx: &Path) -> Option<PathBuf> {
    let n = name.trim().trim_matches(|c| c == '%' || c == '<' || c == '>').to_lowercase();
    let u = user_dir(pfx);
    let c = pfx.join("drive_c");
    Some(match n.as_str() {
        "localappdata" | "winlocalappdata" => u.join("AppData").join("Local"),
        "appdata" | "winappdata" => u.join("AppData").join("Roaming"),
        "locallow" => u.join("AppData").join("LocalLow"),
        "documents" | "windocuments" => u.join("Documents"),
        "savedgames" => u.join("Saved Games"),
        "userprofile" | "home" => u,
        "programfiles" => c.join("Program Files"),
        "programfiles(x86)" => c.join("Program Files (x86)"),
        "winpublic" => c.join("users").join("Public"),
        "winprogramdata" | "programdata" => c.join("ProgramData"),
        "windir" | "windows" => c.join("windows"),
        _ => return None,
    })
}

/// Refuse tout composant dangereux (`..`, vide, lecteur).
fn safe_components(rest: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = rest.split(['/', '\\']).filter(|c| !c.is_empty() && *c != ".").collect();
    if parts.iter().any(|c| *c == ".." || c.contains(':')) {
        return None;
    }
    Some(parts)
}

/// Gabarit universel `%LOCALAPPDATA%/Jeu/Saves` -> chemin reel dans le prefixe Proton. None si la base est inconnue
/// ou si le gabarit sort de la base.
pub fn translate_template(tpl: &str, pfx: &Path) -> Option<PathBuf> {
    let t = tpl.trim();
    let (ph, rest) = match t.find(['/', '\\']) {
        Some(i) => (&t[..i], &t[i + 1..]),
        None => (t, ""),
    };
    if !ph.starts_with('%') {
        return None;
    }
    let mut out = placeholder_base(ph, pfx)?;
    for c in safe_components(rest)? {
        out.push(c);
    }
    Some(out)
}

/// Chemin Windows absolu (`C:\Users\Nom\AppData\Local\Jeu`, `Z:\home\...`) -> chemin reel : `C:` est `drive_c` du prefixe
/// (n'importe quel nom d'utilisateur devient `steamuser`), `Z:` est la racine Linux (convention Wine). Autres lecteurs : None.
pub fn translate_windows_path(win: &str, pfx: &Path) -> Option<PathBuf> {
    let w = win.trim().trim_matches('"');
    let mut chars = w.chars();
    let drive = chars.next()?.to_ascii_lowercase();
    if chars.next()? != ':' {
        return None;
    }
    let rest = &w[2..];
    let parts = safe_components(rest)?;
    match drive {
        'z' => {
            let mut out = PathBuf::from("/");
            for c in parts {
                out.push(c);
            }
            Some(out)
        }
        'c' => {
            let mut out = pfx.join("drive_c");
            let mut it = parts.into_iter().peekable();
            let first_is_users = it.peek().map(|c| c.eq_ignore_ascii_case("users")).unwrap_or(false);
            if first_is_users {
                out.push("users");
                it.next();
                if let Some(name) = it.next() {
                    out.push(if name.eq_ignore_ascii_case("public") { "Public" } else { PROTON_USER });
                }
            }
            for c in it {
                out.push(c);
            }
            Some(out)
        }
        _ => None,
    }
}

/// Chemin Linux situe DANS un prefixe Wine / Proton (`.../drive_c/users/<nom>/AppData/Local/Jeu`) -> gabarit universel
/// (`%LOCALAPPDATA%/Jeu`), pour que la meme sauvegarde soit reconnue sur un PC Windows. None sinon.
pub fn template_from_prefix_path(path: &Path) -> Option<String> {
    let comps: Vec<String> = path.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let i = comps.iter().position(|c| c.eq_ignore_ascii_case("drive_c"))?;
    let after = &comps[i + 1..];
    if after.len() < 4 || !after[0].eq_ignore_ascii_case("users") {
        return None;
    }
    let (base, rest): (&str, &[String]) = if after.len() >= 5 && after[2].eq_ignore_ascii_case("AppData") {
        let b = match after[3].to_lowercase().as_str() {
            "local" => "%LOCALAPPDATA%",
            "roaming" => "%APPDATA%",
            "locallow" => "%LOCALLOW%",
            _ => return None,
        };
        (b, &after[4..])
    } else if after[2].eq_ignore_ascii_case("Documents") || after[2].eq_ignore_ascii_case("My Documents") {
        ("%DOCUMENTS%", &after[3..])
    } else if after[2].eq_ignore_ascii_case("Saved Games") {
        ("%SAVEDGAMES%", &after[3..])
    } else {
        return None;
    };
    if rest.is_empty() || rest.iter().any(|c| c.is_empty() || c.contains(':') || c.contains('%') || c == "..") {
        return None;
    }
    Some(format!("{}/{}", base, rest.join("/")))
}

// ------------------------------------------------------------------ jeux Linux natifs (XDG)

/// Dossiers de base d'un utilisateur Linux.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdgDirs {
    pub home: PathBuf,
    pub data: PathBuf,
    pub config: PathBuf,
    pub documents: PathBuf,
}

impl XdgDirs {
    /// Valeurs XDG (variables d'environnement passees en parametres : pas d'acces global, donc testable).
    pub fn from_env(home: &Path, xdg_data_home: Option<&str>, xdg_config_home: Option<&str>, documents: Option<&Path>) -> XdgDirs {
        fn nz(v: Option<&str>) -> Option<&str> {
            v.map(str::trim).filter(|s| s.starts_with('/'))
        }
        XdgDirs {
            home: home.to_path_buf(),
            data: nz(xdg_data_home).map(PathBuf::from).unwrap_or_else(|| home.join(".local").join("share")),
            config: nz(xdg_config_home).map(PathBuf::from).unwrap_or_else(|| home.join(".config")),
            documents: documents.map(Path::to_path_buf).unwrap_or_else(|| home.join("Documents")),
        }
    }

    /// Gabarit universel -> equivalent Linux NATIF : `%LOCALAPPDATA%` -> dossier de donnees XDG, `%APPDATA%` -> dossier de
    /// configuration XDG, `%DOCUMENTS%` -> Documents, `%USERPROFILE%` -> home. `%LOCALLOW%` / `%SAVEDGAMES%` : aucun equivalent.
    pub fn translate_template(&self, tpl: &str) -> Option<PathBuf> {
        let t = tpl.trim();
        let (ph, rest) = match t.find(['/', '\\']) {
            Some(i) => (&t[..i], &t[i + 1..]),
            None => (t, ""),
        };
        let base = match ph.to_lowercase().as_str() {
            "%localappdata%" => self.data.clone(),
            "%appdata%" => self.config.clone(),
            "%documents%" => self.documents.clone(),
            "%userprofile%" => self.home.clone(),
            _ => return None,
        };
        let mut out = base;
        for c in safe_components(rest)? {
            out.push(c);
        }
        Some(out)
    }
}

/// Prefixes Wine / Proton presents sur la machine (dossiers contenant `drive_c`) : Proton de chaque bibliotheque Steam
/// (`compatdata/<appid>/pfx`), Heroic (`~/Games/Heroic/Prefixes/*/*`), Lutris (`~/Games/*`), Bottles (Flatpak ou natif) et
/// `~/.wine`.
pub fn wine_prefixes(home: &Path, steam_libs: &[PathBuf]) -> Vec<PathBuf> {
    let list = |d: &Path| -> Vec<PathBuf> { std::fs::read_dir(d).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default() };
    let mut cands: Vec<PathBuf> = Vec::new();
    for l in steam_libs {
        cands.extend(list(&l.join("steamapps").join("compatdata")).into_iter().map(|d| d.join("pfx")));
    }
    for d in list(&home.join("Games/Heroic/Prefixes")) {
        cands.push(d.clone());
        for g in list(&d) {
            cands.push(g.join("pfx"));
            cands.push(g);
        }
    }
    for d in list(&home.join("Games")) {
        cands.push(d);
    }
    for b in [home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"), home.join(".local/share/bottles/bottles")] {
        cands.extend(list(&b));
    }
    cands.push(home.join(".wine"));
    let mut out: Vec<PathBuf> = Vec::new();
    for c in cands {
        if c.join("drive_c").is_dir() && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

static PREFIX_CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<PathBuf>)>> = std::sync::Mutex::new(None);

/// `wine_prefixes` de l'utilisateur courant (bibliotheques Steam detectees), relu au plus toutes les 60 s.
pub fn wine_prefixes_cached() -> Vec<PathBuf> {
    let mut g = PREFIX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, v)) = g.as_ref() {
        if t.elapsed() < std::time::Duration::from_secs(60) {
            return v.clone();
        }
    }
    let home = dirs::home_dir().unwrap_or_default();
    let libs = crate::detect::steam_root().map(|r| crate::detect::steam_libraries(&r)).unwrap_or_default();
    let v = wine_prefixes(&home, &libs);
    *g = Some((std::time::Instant::now(), v.clone()));
    v
}

/// Gabarit Windows (`%APPDATA%/Jeu`...) -> dossiers EXISTANTS dans les prefixes Wine / Proton donnes.
pub fn find_in_prefixes(tpl: &str, prefixes: &[PathBuf]) -> Vec<PathBuf> {
    prefixes.iter().filter_map(|p| translate_template(tpl, p)).filter(|x| x.exists()).collect()
}

/// Nom compare sans casse, espaces ni ponctuation (« Hollow Knight » = « hollow_knight » = « HollowKnight »).
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Dossiers de sauvegarde EXISTANTS d'un jeu ou logiciel Linux natif, devines par son nom dans les emplacements usuels :
/// `~/.local/share/<nom>`, `~/.config/<nom>`, `~/Documents/<nom>`, `~/.<nom>`, `~/.config/unity3d/<studio>/<nom>`,
/// `~/.local/share/godot/app_userdata/<nom>` et `~/.var/app/<id contenant le nom>/{data,config}`. Comparaison sans casse
/// ni ponctuation ; un nom trop court (moins de 4 caracteres utiles) ne donne rien.
pub fn native_save_guesses(names: &[String], x: &XdgDirs) -> Vec<PathBuf> {
    let wanted: Vec<String> = names.iter().map(|n| norm(n)).filter(|n| n.len() >= 4 && !["steam", "unity3d", "godot", "games", "documents"].contains(&n.as_str())).collect();
    let mut out: Vec<PathBuf> = Vec::new();
    if wanted.is_empty() {
        return out;
    }
    let list = |d: &Path| -> Vec<PathBuf> { std::fs::read_dir(d).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default() };
    let mut bases: Vec<PathBuf> = vec![x.data.clone(), x.config.clone(), x.documents.clone(), x.home.clone(), x.data.join("godot").join("app_userdata")];
    bases.extend(list(&x.config.join("unity3d")));
    for b in bases {
        for d in list(&b) {
            let n = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if wanted.contains(&norm(n.trim_start_matches('.'))) && !out.contains(&d) {
                out.push(d);
            }
        }
    }
    // Flatpak : ~/.var/app/<org.studio.Jeu>/{data,config}
    for d in list(&x.home.join(".var").join("app")) {
        let id = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let last = id.rsplit('.').next().unwrap_or("");
        if wanted.contains(&norm(last)) {
            for sub in ["data", "config"] {
                let s = d.join(sub);
                if s.is_dir() && !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------ bibliotheques Steam Linux

/// Racines Steam standard d'une installation Linux (installation native, Flatpak, lien `~/.steam/steam`).
pub fn steam_root_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".steam").join("steam"),
        home.join(".steam").join("root"),
        home.join(".local").join("share").join("Steam"),
        home.join(".var").join("app").join("com.valvesoftware.Steam").join(".local").join("share").join("Steam"),
        home.join(".var").join("app").join("com.valvesoftware.Steam").join(".steam").join("steam"),
    ]
}

/// Dossiers `steamapps` possibles sur supports amovibles (carte SD du Steam Deck : `/run/media/<utilisateur>/<carte>/`).
/// `media_roots` : contenu de `/run/media/<user>` ou `/run/media/mmcblk0p1`... fourni par l'appelant.
pub fn steam_media_libraries(media_mounts: &[PathBuf]) -> Vec<PathBuf> {
    media_mounts.iter().filter(|m| m.join("steamapps").is_dir()).cloned().collect()
}

/// Sous-dossiers directs de `/run/media/<user>` et de `/run/media` (cartes SD, disques USB) pour la recherche de bibliotheques.
pub fn list_media_mounts(run_media: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(run_media) else { return out };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        out.push(p.clone());
        // /run/media/<utilisateur>/<volume>
        if let Ok(rd2) = std::fs::read_dir(&p) {
            out.extend(rd2.filter_map(|e| e.ok()).map(|e| e.path()).filter(|q| q.is_dir()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pfx() -> PathBuf {
        PathBuf::from("/lib/steamapps/compatdata/1245620/pfx")
    }

    #[test]
    fn templates_map_into_the_prefix() {
        let u = pfx().join("drive_c/users/steamuser");
        assert_eq!(translate_template("%LOCALAPPDATA%/EldenRing/Saves", &pfx()), Some(u.join("AppData/Local/EldenRing/Saves")));
        assert_eq!(translate_template("%APPDATA%\\Game", &pfx()), Some(u.join("AppData/Roaming/Game")));
        assert_eq!(translate_template("%locallow%/Studio/Game", &pfx()), Some(u.join("AppData/LocalLow/Studio/Game")));
        assert_eq!(translate_template("%DOCUMENTS%/My Games/X", &pfx()), Some(u.join("Documents/My Games/X")));
        assert_eq!(translate_template("%SAVEDGAMES%/X", &pfx()), Some(u.join("Saved Games/X")));
        assert_eq!(translate_template("%INCONNU%/X", &pfx()), None);
        assert_eq!(translate_template("%APPDATA%/../etc", &pfx()), None);
        assert_eq!(translate_template("D:/x", &pfx()), None);
    }

    #[test]
    fn windows_paths_map_to_drive_c_and_z() {
        let u = pfx().join("drive_c/users/steamuser");
        assert_eq!(translate_windows_path(r"C:\Users\player\AppData\Local\Jeu", &pfx()), Some(u.join("AppData/Local/Jeu")));
        assert_eq!(translate_windows_path(r"c:/Users/Public/Documents/Jeu", &pfx()), Some(pfx().join("drive_c/users/Public/Documents/Jeu")));
        assert_eq!(translate_windows_path(r"C:\ProgramData\Jeu", &pfx()), Some(pfx().join("drive_c/ProgramData/Jeu")));
        assert_eq!(translate_windows_path(r"Z:\home\deck\jeu.exe", &pfx()), Some(PathBuf::from("/home/deck/jeu.exe")));
        assert_eq!(translate_windows_path(r"D:\Jeux", &pfx()), None);
        assert_eq!(translate_windows_path(r"C:\Users\x\..\..\etc", &pfx()), None);
        assert_eq!(translate_windows_path("pas un chemin", &pfx()), None);
    }

    #[test]
    fn catalog_markers_resolve() {
        let u = pfx().join("drive_c/users/steamuser");
        assert_eq!(placeholder_base("winLocalAppData", &pfx()), Some(u.join("AppData/Local")));
        assert_eq!(placeholder_base("winAppData", &pfx()), Some(u.join("AppData/Roaming")));
        assert_eq!(placeholder_base("winDocuments", &pfx()), Some(u.join("Documents")));
        assert_eq!(placeholder_base("home", &pfx()), Some(u));
        assert_eq!(placeholder_base("root", &pfx()), None);
    }

    #[test]
    fn prefix_paths_become_universal_templates() {
        let p = Path::new("/home/deck/.local/share/Steam/steamapps/compatdata/42/pfx/drive_c/users/steamuser/AppData/Roaming/Jeu/Saves");
        assert_eq!(template_from_prefix_path(p).as_deref(), Some("%APPDATA%/Jeu/Saves"));
        let d = Path::new("/x/pfx/drive_c/users/steamuser/Documents/My Games/Jeu");
        assert_eq!(template_from_prefix_path(d).as_deref(), Some("%DOCUMENTS%/My Games/Jeu"));
        // le dossier de base lui-meme, un dossier hors profil et un chemin natif sont refuses
        assert_eq!(template_from_prefix_path(Path::new("/x/pfx/drive_c/users/steamuser/AppData/Local")), None);
        assert_eq!(template_from_prefix_path(Path::new("/x/pfx/drive_c/Program Files/Jeu")), None);
        assert_eq!(template_from_prefix_path(Path::new("/home/deck/.local/share/Jeu")), None);
    }

    #[test]
    fn xdg_templates_and_defaults() {
        let d = XdgDirs::from_env(Path::new("/home/deck"), None, None, None);
        assert_eq!(d.data, PathBuf::from("/home/deck/.local/share"));
        assert_eq!(d.config, PathBuf::from("/home/deck/.config"));
        assert_eq!(d.translate_template("%LOCALAPPDATA%/Jeu"), Some(PathBuf::from("/home/deck/.local/share/Jeu")));
        assert_eq!(d.translate_template("%APPDATA%/Jeu"), Some(PathBuf::from("/home/deck/.config/Jeu")));
        assert_eq!(d.translate_template("%DOCUMENTS%/Jeu"), Some(PathBuf::from("/home/deck/Documents/Jeu")));
        assert_eq!(d.translate_template("%SAVEDGAMES%/Jeu"), None);
        let x = XdgDirs::from_env(Path::new("/home/deck"), Some("/data"), Some("relatif"), Some(Path::new("/docs")));
        assert_eq!(x.data, PathBuf::from("/data"));
        assert_eq!(x.config, PathBuf::from("/home/deck/.config")); // chemin relatif ignore (regle XDG)
        assert_eq!(x.documents, PathBuf::from("/docs"));
    }

    #[test]
    fn wine_prefixes_are_listed_and_searched() {
        let t = std::env::temp_dir().join(format!("agss-pfx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let home = t.join("home");
        let lib = t.join("lib");
        let steam = prefix_dir(&lib, 42);
        let heroic = home.join("Games/Heroic/Prefixes/default/Jeu");
        let bottle = home.join(".var/app/com.usebottles.bottles/data/bottles/bottles/Jeux");
        for p in [&steam, &heroic, &bottle] {
            std::fs::create_dir_all(p.join("drive_c/users/steamuser")).unwrap();
        }
        std::fs::create_dir_all(steam.join("drive_c/users/steamuser/AppData/Roaming/EldenRing")).unwrap();
        std::fs::create_dir_all(home.join("Games/PasUnPrefixe")).unwrap();
        let v = wine_prefixes(&home, std::slice::from_ref(&lib));
        assert!(v.contains(&steam) && v.contains(&heroic) && v.contains(&bottle), "{:?}", v);
        assert!(!v.iter().any(|p| p.ends_with("PasUnPrefixe")));
        assert_eq!(find_in_prefixes("%APPDATA%/EldenRing", &v), vec![steam.join("drive_c/users/steamuser/AppData/Roaming/EldenRing")]);
        assert!(find_in_prefixes("%APPDATA%/Absent", &v).is_empty());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn native_saves_are_guessed_by_name() {
        let t = std::env::temp_dir().join(format!("agss-native-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let home = t.join("home");
        for d in [".local/share/Celeste/Saves", ".config/unity3d/Team Cherry/Hollow Knight", ".local/share/godot/app_userdata/Brotato", ".factorio", ".var/app/com.mojang.Minecraft/data", ".local/share/Steam"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let x = XdgDirs::from_env(&home, None, None, None);
        let g = |n: &str| native_save_guesses(&[n.to_string()], &x);
        assert_eq!(g("Celeste"), vec![home.join(".local/share/Celeste")]);
        assert_eq!(g("hollow_knight"), vec![home.join(".config/unity3d/Team Cherry/Hollow Knight")]);
        assert_eq!(g("Brotato"), vec![home.join(".local/share/godot/app_userdata/Brotato")]);
        assert_eq!(g("Factorio"), vec![home.join(".factorio")]);
        assert_eq!(g("Minecraft"), vec![home.join(".var/app/com.mojang.Minecraft/data")]);
        assert!(g("Steam").is_empty());
        assert!(g("abc").is_empty());
        assert!(g("Inconnu").is_empty());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn prefix_and_library_discovery() {
        let t = std::env::temp_dir().join(format!("agss-proton-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let lib = t.join("SteamLib");
        std::fs::create_dir_all(prefix_dir(&lib, 7).join("drive_c")).unwrap();
        std::fs::create_dir_all(t.join("card").join("steamapps")).unwrap();
        assert_eq!(find_prefix(&[t.join("autre"), lib.clone()], 7), Some(prefix_dir(&lib, 7)));
        assert_eq!(find_prefix(&[lib.clone()], 8), None);
        assert_eq!(steam_media_libraries(&[t.join("card"), t.join("vide")]), vec![t.join("card")]);
        assert!(steam_root_candidates(Path::new("/home/deck")).contains(&PathBuf::from("/home/deck/.local/share/Steam")));
        let _ = std::fs::remove_dir_all(&t);
    }
}
