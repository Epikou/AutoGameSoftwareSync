//! Demarrage automatique sous Linux : fichier `~/.config/autostart/AutoGameSoftwareSync.desktop` (norme XDG, compris par
//! KDE Plasma / SteamOS en mode bureau, GNOME, XFCE...). Texte et analyse PURS (testes partout) ; l'ecriture du fichier est
//! faite par `winutil` (Linux). Variante sans interface graphique : un service utilisateur systemd (voir docs/LINUX.md).
use std::path::{Path, PathBuf};

/// Nom du fichier d'autostart.
pub const DESKTOP_FILE: &str = "AutoGameSoftwareSync.desktop";

/// Dossier XDG de configuration (`$XDG_CONFIG_HOME` s'il est absolu, sinon `~/.config`).
pub fn config_home(home: &Path, xdg_config_home: Option<&str>) -> PathBuf {
    match xdg_config_home.map(str::trim).filter(|s| s.starts_with('/')) {
        Some(c) => PathBuf::from(c),
        None => home.join(".config"),
    }
}

/// Chemin complet du fichier d'autostart.
pub fn autostart_path(home: &Path, xdg_config_home: Option<&str>) -> PathBuf {
    config_home(home, xdg_config_home).join("autostart").join(DESKTOP_FILE)
}

/// Echappe un argument de la cle `Exec` (specification Desktop Entry : guillemets, `\`, `$`, `` ` `` et `"` echappes).
pub fn exec_quote(arg: &str) -> String {
    let needs = arg.chars().any(|c| " \t\n\"'\\><~|&;$*?#()`".contains(c));
    if !needs {
        return arg.to_string();
    }
    let mut s = String::from("\"");
    for c in arg.chars() {
        if "\"`$\\".contains(c) {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s.replace('%', "%%")
}

/// Contenu du fichier `.desktop` d'autostart : lance l'interface directement dans la zone de notification.
pub fn desktop_entry(exe: &Path, name: &str, comment: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment={comment}\nExec={exec} --tray\nTerminal=false\nX-GNOME-Autostart-enabled=true\nStartupNotify=false\n",
        name = name,
        comment = comment,
        exec = exec_quote(&exe.to_string_lossy()),
    )
}

/// Valeur d'une cle du groupe `[Desktop Entry]`.
pub fn entry_value(text: &str, key: &str) -> Option<String> {
    let mut in_group = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_group = l == "[Desktop Entry]";
            continue;
        }
        if in_group {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// L'entree est-elle active ? (non masquee par `Hidden=true` ni `X-GNOME-Autostart-enabled=false`)
pub fn is_enabled(text: &str) -> bool {
    let off = |k: &str, bad: &str| entry_value(text, k).map(|v| v.eq_ignore_ascii_case(bad)).unwrap_or(false);
    entry_value(text, "Exec").is_some() && !off("Hidden", "true") && !off("X-GNOME-Autostart-enabled", "false")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_follows_xdg() {
        let h = Path::new("/home/deck");
        assert_eq!(autostart_path(h, None), PathBuf::from("/home/deck/.config/autostart/AutoGameSoftwareSync.desktop"));
        assert_eq!(autostart_path(h, Some("/cfg")), PathBuf::from("/cfg/autostart/AutoGameSoftwareSync.desktop"));
        assert_eq!(autostart_path(h, Some("relatif")), PathBuf::from("/home/deck/.config/autostart/AutoGameSoftwareSync.desktop"));
    }

    #[test]
    fn entry_is_valid_and_quoted() {
        let t = desktop_entry(Path::new("/home/deck/.local/bin/AutoGameSoftwareSync"), "AutoGameSoftwareSync", "Synchronisation");
        assert!(t.starts_with("[Desktop Entry]\nType=Application\n"));
        assert_eq!(entry_value(&t, "Exec").as_deref(), Some("/home/deck/.local/bin/AutoGameSoftwareSync --tray"));
        assert!(is_enabled(&t));
        let q = desktop_entry(Path::new("/home/de ck/ex$e"), "N", "C");
        assert_eq!(entry_value(&q, "Exec").as_deref(), Some("\"/home/de ck/ex\\$e\" --tray"));
    }

    #[test]
    fn disabled_flags_are_detected() {
        assert!(!is_enabled("[Desktop Entry]\nExec=x\nHidden=true\n"));
        assert!(!is_enabled("[Desktop Entry]\nExec=x\nX-GNOME-Autostart-enabled=false\n"));
        assert!(!is_enabled("[Desktop Entry]\nName=x\n"));
        assert!(!is_enabled("[Autre]\nExec=x\n"));
    }
}
