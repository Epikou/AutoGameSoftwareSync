//! Detection automatique des jeux (logique pure, testee) : un processus demarre apres le lancement de l'appli est un
//! « jeu candidat » s'il est dans la base embarquee des jeux, s'execute depuis un dossier de jeux de lanceur, ou est
//! un emulateur connu. Tout est local : aucune donnee n'est envoyee. Ne fait JAMAIS passer une fenetre au premier plan.
use crate::process::ProcInfo;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// Delai avant de reproposer un jeu mis de cote avec « Plus tard » (a un prochain lancement).
pub const LATER_MS: u64 = 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Why {
    /// Exe present dans la base embarquee des jeux.
    InDb,
    /// Exe d un profil communautaire (application / configuration).
    Profile,
    /// Execute depuis un dossier de jeux d'un lanceur (Steam, Epic, GOG...).
    LauncherFolder,
    /// Emulateur connu.
    Emulator,
    /// Programme ajoute sans emplacement : ses dossiers ont ete appris pendant son utilisation.
    Learned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Cle de memoire : nom d'exe normalise + dossier d'installation.
    pub key: String,
    pub exe: String,
    pub path: Option<PathBuf>,
    pub why: Why,
    /// Nom propose (nom du jeu de la base, ou nom de l'exe).
    pub name: String,
    pub install_dir: Option<PathBuf>,
    /// Profil communautaire correspondant (applications / configurations), s il y en a un.
    #[serde(default)]
    pub profile_id: Option<String>,
    /// Application ou configuration (categorie du profil app / config) ; faux pour les jeux, emulateurs et profils de jeux.
    #[serde(default)]
    pub app: bool,
    /// Score de confiance et ses raisons (absent des cartes enregistrees avant la 1.4).
    #[serde(default)]
    pub confidence: Option<Confidence>,
}

/// Noms trop generiques pour identifier un jeu a eux seuls (acceptes seulement si le dossier d'installation correspond).
pub const GENERIC_EXES: &[&str] = &[
    "game.exe", "launcher.exe", "setup.exe", "install.exe", "installer.exe", "uninstall.exe", "unins000.exe", "start.exe", "run.exe", "play.exe", "main.exe", "app.exe",
    "client.exe", "server.exe", "updater.exe", "update.exe", "patcher.exe", "config.exe", "settings.exe", "editor.exe", "bootstrapper.exe", "crashreporter.exe",
    "unitycrashhandler64.exe", "unitycrashhandler32.exe", "dxsetup.exe", "vcredist_x64.exe", "vcredist_x86.exe", "cmd.exe", "java.exe", "javaw.exe", "python.exe",
    "node.exe", "nodejs.exe", "pythonw.exe", "python3.exe", "dotnet.exe", "electron.exe", "powershell.exe", "pwsh.exe", "conhost.exe", "code.exe", "chrome.exe",
    "msedge.exe", "firefox.exe", "explorer.exe", "steam.exe", "wscript.exe", "cscript.exe", "rundll32.exe", "svchost.exe", "nw.exe", "ruby.exe", "perl.exe",
    "php.exe", "go.exe", "npm.exe", "git.exe", "bash.exe", "wsl.exe", "mshta.exe", "msiexec.exe", "taskhostw.exe", "dllhost.exe", "cmake.exe", "gradle.exe",
];

/// Un nom d'exe present pour plus de jeux que cela dans la base est trop ambigu pour identifier un jeu seul.
/// Exes generiques que les profils curés (applications) ont le droit de cibler.
pub const PROFILE_OK_EXES: &[&str] = &["code.exe", "pwsh.exe", "powershell.exe"];

pub const MAX_AMBIGUOUS_HITS: usize = 3;

/// Nom d'exe (insensible a la casse) generique : runtime, interpreteur, outil, navigateur ou lanceur. Jamais reconnu par son seul nom.
pub fn is_generic_name(exe: &str) -> bool {
    let e = exe.trim().to_lowercase();
    GENERIC_EXES.contains(&e.as_str()) || EXCLUDED_EXES.contains(&e.as_str()) || (cfg!(not(windows)) && (LINUX_GENERIC.contains(&e.as_str()) || LINUX_EXCLUDED.contains(&e.as_str())))
}

/// Navigateurs, lanceurs de boutiques, outils : jamais proposes.
pub const EXCLUDED_EXES: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "iexplore.exe", "explorer.exe", "steam.exe", "steamwebhelper.exe", "steamservice.exe",
    "steamerrorreporter.exe", "epicgameslauncher.exe", "epicwebhelper.exe", "eosoverlayrenderer-win64-shipping.exe", "battle.net.exe", "agent.exe", "galaxyclient.exe",
    "galaxyclient helper.exe", "origin.exe", "eadesktop.exe", "eabackgroundservice.exe", "upc.exe", "ubisoftconnect.exe", "ubisoftgamelauncher.exe", "riotclientservices.exe",
    "riotclientux.exe", "xboxapp.exe", "gamingservices.exe", "discord.exe", "spotify.exe", "teams.exe", "code.exe", "devenv.exe", "autogamesoftwaresync.exe", "autogamesoftwaresync.exe", "agss.exe",
    "nvcontainer.exe", "nvidia share.exe", "obs64.exe", "obs32.exe", "playnite.desktopapp.exe", "playnite.fullscreenapp.exe", "easyanticheat.exe", "beservice.exe",
];

/// Emulateurs connus (nom d'exe exact ou prefixe avant `-qt`, `-win64`... teste par `starts_with`).
pub const EMULATOR_EXES: &[&str] = &[
    "pcsx2", "rpcs3", "yuzu", "ryujinx", "dolphin", "cemu", "duckstation", "citra", "ppsspp", "melonds", "mgba", "snes9x", "retroarch", "xemu", "vita3k", "xenia", "bsnes",
    "mesen", "desmume", "project64", "flycast", "redream", "scummvm", "nestopia", "fceux", "visualboyadvance", "bizhawk", "mame", "higan", "ares", "sudachi", "suyu", "lime3d",
    "azahar", "shadps4", "cxbx", "xqemu", "pcsx-redux", "epsxe", "mednafen", "kega", "genesisplus", "citron", "eden",
];

/// Linux : emulateurs sous leur nom de programme Linux (en plus de `EMULATOR_EXES`).
pub const LINUX_EMULATORS: &[&str] = &[
    "ppssppsdl", "ppssppqt", "torzu", "lime3ds", "mupen64plus", "rmg", "simple64", "gopher64", "primehack", "dosbox", "dosbox-staging", "dosbox-x", "bigpemu", "supermodel",
    "skyemu", "stella", "gearboy", "sameboy", "blastem",
];

/// Linux : programmes du systeme, du bureau, de Steam et de Wine jamais proposes (meme dans un dossier de lanceur).
pub const LINUX_EXCLUDED: &[&str] = &[
    // bureau et services
    "systemd", "dbus-daemon", "dbus-broker", "pipewire", "pipewire-pulse", "wireplumber", "pulseaudio", "xwayland", "xorg", "x", "gamescope", "gamescope-wl", "mangoapp", "gamemoded",
    "gnome-shell", "gnome-session-binary", "nautilus", "plasmashell", "kwin_x11", "kwin_wayland", "krunner", "kded5", "kded6", "konsole", "dolphin", "kate", "okular", "ksmserver",
    "baloo_file", "akonadi_control", "xdg-desktop-portal", "xdg-desktop-portal-kde", "xdg-desktop-portal-gnome", "xdg-desktop-portal-gtk", "xdg-document-portal", "xdg-permission-store",
    "gvfsd", "ibus-daemon", "fcitx5", "at-spi-bus-launcher", "at-spi2-registryd", "polkitd", "packagekitd", "flatpak", "flatpak-session-helper", "flatpak-portal", "bwrap", "xdg-dbus-proxy",
    "networkmanager", "bluetoothd", "udisksd", "upowerd", "sshd", "sudo", "cron", "steamos-manager", "jupiter-biosupdate", "sdgyrodsu", "powerbuttond", "inputplumber", "handygccs",
    // navigateurs, discussions
    "firefox", "firefox-bin", "chrome", "chromium", "brave", "vivaldi", "opera", "discord", "vesktop", "spotify", "thunderbird", "obs",
    // Steam, lanceurs, Wine / Proton
    "steam", "steamwebhelper", "steam-runtime-launcher-service", "steam-runtime-launch-client", "reaper", "pressure-vessel-wrap", "pressure-vessel-adverb", "srt-bwrap", "fossilize_replay",
    "gameoverlayui", "wineserver", "wine", "wine64", "wine-preloader", "wine64-preloader", "proton", "winedevice.exe", "services.exe", "plugplay.exe", "rpcss.exe", "svchost.exe", "winedbg.exe",
    "wineboot.exe", "winemenubuilder.exe", "conhost.exe", "start.exe", "tabtip.exe", "mscorsvw.exe", "rundll32.exe", "explorer.exe", "steam.exe", "lsteamclient.exe", "heroic", "lutris",
    "bottles", "legendary", "gogdl", "nile", "umu-run", "autogamesoftwaresync", "autogamesoftwaresync-tray",
];

/// Linux : interpreteurs, coquilles et enveloppes, jamais reconnus par leur seul nom.
pub const LINUX_GENERIC: &[&str] = &[
    "sh", "bash", "dash", "zsh", "fish", "python", "python3", "perl", "ruby", "node", "java", "mono", "dotnet", "env", "gamemoderun", "mangohud", "start.sh", "run.sh", "launch.sh", "game.sh",
    "start", "run", "launcher", "game", "main", "app", "apprun", "love", "godot", "electron",
];

/// Linux : dossiers de Steam, de Proton, des environnements d'execution et du systeme Wine ; rien n'y est un jeu.
const LINUX_AUX_DIRS: &[&str] = &[
    "/steamapps/common/proton", "/steamapps/common/steamlinuxruntime", "/steamapps/common/steam linux runtime", "/compatibilitytools.d/", "/pressure-vessel/", "/steam/ubuntu12_32/",
    "/steam/ubuntu12_64/", "/steam/linux32/", "/steam/linux64/", "/drive_c/windows/", "/drive_c/program files (x86)/steam/", "/drive_c/program files/common files/",
    "/drive_c/program files (x86)/common files/", "/drive_c/program files/internet explorer/", "/drive_c/program files (x86)/internet explorer/", "/drive_c/program files/windows media player/",
    "/drive_c/program files (x86)/windows media player/", "/drive_c/program files/windows nt/", "/drive_c/program files (x86)/windows nt/", "/.mount_", "/usr/lib/", "/usr/libexec/",
    "/heroic/tools/", "/lutris/runners/", "/lutris/runtime/", "/bottles/runners/", "/bottles/dxvk/",
];

/// Linux : dossiers de jeux des lanceurs et des prefixes Wine (Heroic, Lutris, Bottles, GOG).
const LINUX_LAUNCHER_MARKERS: &[&str] = &["/games/heroic/", "/drive_c/program files/", "/drive_c/program files (x86)/", "/drive_c/games/", "/drive_c/gog games/"];

/// Linux : dossiers de programmes partages (chaque programme y est un element distinct, pas un dossier d'installation).
pub fn is_shared_bin_dir(dir: &Path) -> bool {
    let n = norm_path(dir);
    let n = n.trim_end_matches('/');
    ["/usr/bin", "/usr/local/bin", "/usr/games", "/usr/local/games", "/bin", "/app/bin", "/opt/bin", "/snap/bin"].contains(&n) || n.ends_with("/.local/bin") || n.ends_with("/applications")
}

fn launcher_markers() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = LAUNCHER_MARKERS.to_vec();
    if cfg!(not(windows)) {
        v.extend_from_slice(LINUX_LAUNCHER_MARKERS);
    }
    v
}

pub fn norm_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbHit {
    pub name: String,
    pub install_dirs: Vec<String>,
}

/// Contexte de classification.
pub struct Env<'a> {
    /// Cherche un exe (minuscules) dans la base embarquee.
    pub db: &'a dyn Fn(&str) -> Vec<DbHit>,
    /// Dossiers de jeux de lanceurs supplementaires (bibliotheques Steam secondaires...), normalises.
    pub launcher_roots: &'a [String],
    /// Exe des jeux deja dans la liste (minuscules).
    pub known_exes: &'a [String],
}

const LAUNCHER_MARKERS: &[&str] = &[
    "/steamapps/common/", "/epic games/", "/gog games/", "/gog galaxy/games/", "/xboxgames/", "/ubisoft game launcher/games/", "/ea games/", "/origin games/", "/itch/apps/", "/amazon games/library/",
];

fn is_system_path(np: &str) -> bool {
    np.contains("/windows/") || np.contains("/windowsapps/") || np.contains("/system32/") || np.contains("/syswow64/")
}

/// Dossier d'installation d'un exe : sous un dossier de jeux de lanceur, le dossier du jeu ; sinon le dossier de l'exe.
fn install_dir_of(path: &Path, roots: &[String]) -> PathBuf {
    let np = norm_path(path);
    // Linux : une AppImage, ou un programme d'un dossier partage (/usr/bin, ~/.local/bin...), est son propre element.
    if cfg!(not(windows)) && (np.ends_with(".appimage") || path.parent().map(is_shared_bin_dir).unwrap_or(false)) {
        return path.to_path_buf();
    }
    let mut markers: Vec<String> = launcher_markers().iter().map(|s| s.to_string()).collect();
    markers.extend(roots.iter().map(|r| format!("{}/", r.trim_end_matches('/'))));
    for m in markers {
        if let Some(i) = np.find(&m) {
            let rest = &np[i + m.len()..];
            if let Some(first) = rest.split('/').next() {
                if !first.is_empty() && rest.contains('/') {
                    // Reconstruit avec la casse d'origine : meme longueur de prefixe.
                    let orig: String = path.to_string_lossy().replace('\\', "/");
                    let cut = i + m.len() + first.len();
                    return PathBuf::from(orig.chars().take(cut).collect::<String>());
                }
            }
        }
    }
    path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// Emulateur connu : l'entree de la liste reconnue (« pcsx2 » pour « pcsx2-qt.exe »).
fn emulator_of(exe: &str) -> Option<&'static str> {
    let stem = exe.trim_end_matches(".exe");
    let hit = |list: &'static [&'static str]| list.iter().copied().find(|e| stem == *e || (stem.starts_with(e) && !stem[e.len()..].chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)));
    hit(EMULATOR_EXES).or_else(|| if cfg!(not(windows)) { hit(LINUX_EMULATORS) } else { None })
}

/// Linux : programme exclu, generique ou dans un dossier technique (Steam, Proton, Wine, montage d'AppImage...).
fn linux_excluded(exe: &str, np: &str) -> bool {
    if !cfg!(not(windows)) {
        return false;
    }
    LINUX_EXCLUDED.contains(&exe) || exe.starts_with("gsd-") || exe.starts_with("xdg-") || exe.starts_with("kwin") || exe.starts_with("systemd-") || LINUX_AUX_DIRS.iter().any(|d| np.contains(d))
}

/// Nom affichable d'un programme : sans « .exe » ni (Linux) extension technique (« Cemu.AppImage », « Jeu.x86_64 », « jeu.sh »).
fn pretty_name(exe: &str) -> String {
    let mut stem = exe.trim_end_matches(".exe").trim_end_matches(".EXE");
    if cfg!(not(windows)) {
        for ext in [".appimage", ".x86_64", ".x86", ".sh", ".bin", ".py", ".jar"] {
            if stem.to_lowercase().ends_with(ext) {
                stem = &stem[..stem.len() - ext.len()];
            }
        }
        for suf in ["-x86_64", "_x86_64", "-linux"] {
            if stem.to_lowercase().ends_with(suf) {
                stem = &stem[..stem.len() - suf.len()];
            }
        }
    }
    stem.to_string()
}

/// Classe un processus : Some(candidat) ou None (exclu / pas un jeu / score sous le seuil). Sans indices d'execution
/// (`Facts` inconnus = neutres) : seuls le chemin, le nom et la base comptent.
pub fn classify(p: &ProcInfo, env: &Env) -> Option<Candidate> {
    classify_with(p, env, &Facts::default())
}

/// `classify` avec les indices d'execution (sous-systeme, editeur, parent, duree de vie, bibliotheques graphiques...).
/// Seuls les candidats dont le score atteint `PROPOSE_THRESHOLD` (sans veto) sont retenus.
pub fn classify_with(p: &ProcInfo, env: &Env, f: &Facts) -> Option<Candidate> {
    assess(p, env, f).filter(is_proposable)
}

/// Le candidat a-t-il un score suffisant (et aucun veto) pour une carte de proposition ?
pub fn is_proposable(c: &Candidate) -> bool {
    c.confidence.as_ref().map(|s| s.proposable()).unwrap_or(false)
}

fn under(np: &str, root: &str) -> bool {
    let r = root.trim_end_matches('/');
    !r.is_empty() && np.starts_with(&format!("{}/", r))
}

/// Evalue un processus. None : exclu d'office (listes, dossiers systeme ou techniques, chemin illisible) ou aucun indice ne
/// le rapproche d'un jeu. Sinon le candidat AVEC son score, meme sous le seuil (journal, section « Peut-etre » du scan).
/// Un nom de programme ne suffit jamais seul : sans indice de dossier, le score est plafonne sous le seuil.
pub fn assess(p: &ProcInfo, env: &Env, f: &Facts) -> Option<Candidate> {
    let exe = p.exe.to_lowercase();
    if is_auxiliary(&exe) || exe.is_empty() || EXCLUDED_EXES.contains(&exe.as_str()) || env.known_exes.iter().any(|k| *k == exe) {
        return None;
    }
    let path = p.path.as_ref()?; // sans chemin lisible on ne peut ni verifier ni proposer
    let np = norm_path(path);
    if is_system_path(&np) || is_aux_dir(&np) || linux_excluded(&exe, &np) {
        return None;
    }
    // Dossier confirme par le manifeste d'un lanceur (Steam, Epic, GOG, Heroic) : c'est le dossier d'installation.
    let manifest = f.manifest.as_ref().filter(|(d, _)| under(&np, &norm_path(d)));
    let install = manifest.map(|(d, _)| d.clone()).unwrap_or_else(|| install_dir_of(path, env.launcher_roots));
    let key = format!("{}|{}", exe, norm_path(&install));
    let generic = GENERIC_EXES.contains(&exe.as_str()) || (cfg!(not(windows)) && LINUX_GENERIC.contains(&exe.as_str()));
    let in_launcher_dir = launcher_markers().iter().any(|m| np.contains(m)) || env.launcher_roots.iter().any(|r| under(&np, r));
    // Linux : une AppImage ou un programme de /usr/bin est son propre element (son nom n'est pas un dossier).
    let own_element = install == *path;
    let pretty = pretty_name(&p.exe);
    let stem = alnum(&pretty);
    let stem_len = stem.chars().count();
    let mut rs: Vec<Reason> = Vec::new();
    let (why, name) = if let Some(emu) = emulator_of(&exe) {
        // (c) emulateur connu : son dossier (ou son nom de produit) doit le confirmer.
        rs.push(Reason::Emulator);
        let mut names = dir_names(path, &install, 8, true);
        names.extend([&f.product, &f.company].into_iter().filter_map(|s| s.as_ref()).map(|s| s.to_lowercase()));
        if names.iter().any(|n| emu_match(n, emu)) {
            rs.push(Reason::EmulatorFolder);
        }
        (Why::Emulator, pretty.clone())
    } else {
        // (a) base embarquee des jeux : le nom ne compte que peu ; le dossier doit correspondre au titre.
        let hits = (env.db)(&exe);
        let mut dir_hit: Option<&DbHit> = None;
        if !hits.is_empty() {
            let mut names: Vec<String> = dir_names(path, &install, 3, !own_element).iter().map(|n| alnum(n)).collect();
            if let Some((d, n)) = manifest {
                names.push(alnum(n));
                names.extend(d.file_name().map(|x| alnum(&x.to_string_lossy())));
            }
            let exact = |h: &&DbHit| names.iter().any(|n| h.install_dirs.iter().any(|d| alnum(d) == *n) || alnum(&h.name) == *n);
            let partial = |h: &&DbHit| names.iter().any(|n| std::iter::once(alnum(&h.name)).chain(h.install_dirs.iter().map(|d| alnum(d))).any(|g| partial_match(n, &g, &stem)));
            if let Some(h) = hits.iter().find(exact) {
                rs.push(Reason::DbDir);
                dir_hit = Some(h);
            } else if let Some(h) = hits.iter().find(partial) {
                rs.push(Reason::DbDirPartial);
                dir_hit = Some(h);
            }
            if !generic {
                if hits.len() == 1 && stem_len >= 5 {
                    rs.push(Reason::DbUnique);
                } else if hits.len() <= MAX_AMBIGUOUS_HITS {
                    rs.push(Reason::DbFew);
                }
            }
        }
        // (b) dossier de jeux d'un lanceur, ou dossier confirme par son manifeste
        if in_launcher_dir && !generic {
            rs.push(Reason::Library);
        }
        if manifest.is_some() && !generic {
            rs.push(Reason::Manifest);
        }
        if rs.is_empty() {
            return None;
        }
        if stem_len <= 4 && dir_hit.is_none() && !in_launcher_dir && manifest.is_none() {
            rs.push(Reason::ShortName);
        }
        let folder = || install.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| pretty.clone());
        match (dir_hit, manifest) {
            (Some(h), _) => (Why::InDb, h.name.clone()),
            (None, Some((_, n))) if !generic => (Why::LauncherFolder, n.clone()),
            (None, _) if in_launcher_dir && !generic => (Why::LauncherFolder, folder()),
            (None, _) => (Why::InDb, hits.first().map(|h| h.name.clone()).unwrap_or_else(|| pretty.clone())),
        }
    };
    runtime_reasons(f, &mut rs);
    // Hors lanceur, reconnu par la seule base : un titre qui tourne sans aucune bibliotheque graphique est un utilitaire
    // homonyme (ex. l'outil de recherche « Everything » range dans son dossier « Everything »).
    if why == Why::InDb && f.graphics == Some(false) && !rs.contains(&Reason::Library) && !rs.contains(&Reason::Manifest) {
        rs.push(Reason::NoGraphics);
    }
    let veto = veto_of(&np, f, true, true);
    Some(Candidate { key, exe, path: Some(path.clone()), why, name, install_dir: Some(install), profile_id: None, app: false, confidence: Some(Confidence::new(rs, veto)) })
}

/// Profil integre ou personnalise reconnu par son nom de programme : identifiant, nom, application ?, programme installe
/// sur ce PC (dossier de detection ou emplacement du profil present).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileHint {
    pub id: String,
    pub name: String,
    pub app: bool,
    pub installed: bool,
}

/// Mots trop vagues pour rapprocher un dossier d'un profil.
const PROFILE_STOP: &[&str] = &["windows", "microsoft", "profile", "profil", "preferences", "settings", "config", "studio", "edition", "classic", "retail", "game", "games"];

/// Evalue un processus reconnu par l'index des profils (applications, configurations, jeux et emulateurs a profil).
/// Les profils sont des listes curees : pas de veto « dossier / editeur d'outils de developpement » (VS Code, PowerShell,
/// l'editeur Unreal en sont), mais le nom seul ne suffit pas non plus (profil installe ou dossier correspondant).
pub fn assess_profile(p: &ProcInfo, f: &Facts, h: &ProfileHint) -> Option<Candidate> {
    let exe = p.exe.to_lowercase();
    let path = p.path.clone()?;
    let np = norm_path(&path);
    let dir = path.parent().map(|d| d.to_path_buf());
    let mut rs = vec![Reason::Profile];
    if h.installed {
        rs.push(Reason::ProfileInstalled);
    }
    let mut want: Vec<String> = vec![alnum(&h.name), alnum(&h.id)];
    want.extend(h.name.split(|c: char| !c.is_alphanumeric()).chain(h.id.split('-')).map(alnum).filter(|t| t.chars().count() >= 4 && !PROFILE_STOP.contains(&t.as_str())));
    want.retain(|w| w.chars().count() >= 3);
    let names = dir_names(&path, dir.as_deref().unwrap_or(&path), 3, true);
    let folder = names.iter().any(|n| {
        let a = alnum(n);
        want.iter().any(|w| a.contains(w.as_str()))
    });
    if folder {
        rs.push(Reason::ProfileFolder);
    }
    if alnum(&pretty_name(&p.exe)).chars().count() <= 4 && !folder && !h.installed {
        rs.push(Reason::ShortName);
    }
    runtime_reasons(f, &mut rs);
    // Terminal / editeur cibles explicitement par un profil (PowerShell, VS Code) : console et parent terminal sont normaux.
    let curated_tool = PROFILE_OK_EXES.contains(&exe.as_str());
    let veto = veto_of(&np, f, false, !curated_tool);
    Some(Candidate { key: format!("{}|profile:{}", exe, h.id), exe, path: Some(path), why: Why::Profile, name: h.name.clone(), install_dir: dir, profile_id: Some(h.id.clone()), app: h.app, confidence: Some(Confidence::new(rs, veto)) })
}

// ------------------------------------------------------------------ score de confiance (0 a 100)

/// Seuil de la carte de proposition.
pub const PROPOSE_THRESHOLD: u8 = 70;
/// Entre ce seuil et `PROPOSE_THRESHOLD` : seulement dans le scan du PC (section « Peut-etre »), jamais en carte.
pub const MAYBE_THRESHOLD: u8 = 40;
/// Duree de vie minimale avant toute proposition : les sous-programmes d'outils (Git...) vivent quelques millisecondes.
pub const MIN_AGE_MS: u64 = 20_000;
/// Duree de vie qui ajoute un indice (le programme reste ouvert).
pub const LONG_AGE_MS: u64 = 60_000;

/// Indice retenu dans le score (points fixes, voir `points`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Sous un dossier de bibliotheque d'un lanceur (steamapps\common, Epic Games, GOG Games, XboxGames...).
    Library,
    /// Dossier d'installation confirme par le manifeste d'un lanceur (appmanifest Steam, Epic, GOG, Heroic).
    Manifest,
    /// Profil integre / personnalise qui cite ce programme.
    Profile,
    /// Le programme du profil est installe sur ce PC.
    ProfileInstalled,
    /// Le dossier du programme correspond au profil.
    ProfileFolder,
    /// Emulateur connu.
    Emulator,
    /// Dossier (ou nom de produit) de l'emulateur.
    EmulatorFolder,
    /// Base embarquee : nom du programme ET dossier d'installation du titre (ou dossier au nom exact du titre).
    DbDir,
    /// Base embarquee : dossier portant une partie du nom du titre.
    DbDirPartial,
    /// Base embarquee : nom de programme propre a un seul titre (5 caracteres ou plus).
    DbUnique,
    /// Base embarquee : nom partage par 3 titres au plus.
    DbFew,
    /// Bibliotheque graphique chargee (d3d9/10/11/12, Vulkan, OpenGL).
    Graphics,
    /// Ouvert depuis au moins une minute.
    LongLived,
    /// Editeur (infos de version) dans la liste des editeurs de jeux.
    Publisher,
    /// Nom de programme tres court (4 caracteres ou moins) sans dossier correspondant.
    ShortName,
    /// Hors lanceur : aucune bibliotheque graphique chargee (utilitaire homonyme d'un titre de la base).
    NoGraphics,
}

impl Reason {
    pub fn points(self) -> i32 {
        match self {
            Reason::Library | Reason::Manifest | Reason::DbDir => 70,
            Reason::Emulator => 55,
            Reason::Profile | Reason::DbDirPartial => 50,
            Reason::ProfileInstalled | Reason::ProfileFolder => 30,
            Reason::EmulatorFolder | Reason::DbUnique => 20,
            Reason::Graphics => 15,
            Reason::DbFew | Reason::LongLived | Reason::Publisher => 10,
            Reason::ShortName => -30,
            Reason::NoGraphics => -40,
        }
    }

    /// Indice de DOSSIER (ou de produit) : sans aucun d'eux, le score reste sous le seuil (un nom ne suffit jamais).
    pub fn is_location(self) -> bool {
        matches!(self, Reason::Library | Reason::Manifest | Reason::DbDir | Reason::DbDirPartial | Reason::ProfileInstalled | Reason::ProfileFolder | Reason::EmulatorFolder)
    }

    /// Libelle du journal (francais, stable).
    pub fn label(self) -> &'static str {
        match self {
            Reason::Library => "dossier d'un lanceur",
            Reason::Manifest => "installation confirmee par le lanceur",
            Reason::Profile => "profil integre",
            Reason::ProfileInstalled => "programme du profil installe",
            Reason::ProfileFolder => "dossier du profil",
            Reason::Emulator => "emulateur connu",
            Reason::EmulatorFolder => "dossier ou produit de l'emulateur",
            Reason::DbDir => "nom et dossier reconnus dans la base",
            Reason::DbDirPartial => "dossier portant le nom du titre",
            Reason::DbUnique => "nom propre a un seul titre de la base",
            Reason::DbFew => "nom partage par quelques titres de la base",
            Reason::Graphics => "bibliotheque graphique chargee",
            Reason::LongLived => "ouvert depuis plus d'une minute",
            Reason::Publisher => "editeur reconnu",
            Reason::ShortName => "nom tres court",
            Reason::NoGraphics => "aucune bibliotheque graphique (hors lanceur)",
        }
    }
}

/// Refus dur : score force a 0, quelle que soit la somme des indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Veto {
    /// Sous-systeme PE console : outil en ligne de commande (les jeux sont en sous-systeme fenetre).
    Console,
    /// Dossier d'outils de developpement (Git, Rust, Node, Python, Visual Studio, JetBrains, msys/mingw...).
    DevFolder,
    /// Editeur ou produit d'outils de developpement (infos de version).
    DevPublisher,
    /// Lance depuis un terminal, un interpreteur ou un environnement de developpement.
    ShellParent,
    /// Processus trop court (moins de `MIN_AGE_MS`).
    ShortLived,
    /// Service du systeme (autre session) ou processus d'un autre utilisateur.
    System,
}

impl Veto {
    pub fn label(self) -> &'static str {
        match self {
            Veto::System => "service du systeme ou autre utilisateur",
            Veto::Console => "programme console (outil en ligne de commande)",
            Veto::DevFolder => "dossier d'outils de developpement",
            Veto::DevPublisher => "editeur ou produit d'outils de developpement",
            Veto::ShellParent => "lance depuis un terminal ou un outil de developpement",
            Veto::ShortLived => "processus trop court (moins de 20 s)",
        }
    }
}

/// Niveau d'un score : carte de proposition, « Peut-etre » (scan du PC seulement) ou jamais.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Propose,
    Maybe,
    Never,
}

/// Score de confiance (0 a 100) et ses raisons, gardees pour le journal, l'infobulle et le mode expert.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confidence {
    pub score: u8,
    #[serde(default)]
    pub reasons: Vec<Reason>,
    #[serde(default)]
    pub veto: Option<Veto>,
}

impl Confidence {
    /// Somme des points bornee a 0..100 ; plafonnee sous le seuil sans indice de dossier ; 0 en cas de veto.
    pub fn new(reasons: Vec<Reason>, veto: Option<Veto>) -> Confidence {
        let mut score = reasons.iter().map(|r| r.points()).sum::<i32>().clamp(0, 100);
        if !reasons.iter().any(|r| r.is_location()) {
            score = score.min(PROPOSE_THRESHOLD as i32 - 1);
        }
        if veto.is_some() {
            score = 0;
        }
        Confidence { score: score as u8, reasons, veto }
    }

    pub fn proposable(&self) -> bool {
        self.veto.is_none() && self.score >= PROPOSE_THRESHOLD
    }

    pub fn tier(&self) -> Tier {
        if self.proposable() {
            Tier::Propose
        } else if self.veto.is_none() && self.score >= MAYBE_THRESHOLD {
            Tier::Maybe
        } else {
            Tier::Never
        }
    }

    /// Resume du journal : « 90/100 (dossier d'un lanceur +70, nom propre a un seul titre de la base +20) ».
    pub fn summary(&self) -> String {
        let rs: Vec<String> = self.reasons.iter().map(|r| format!("{} {:+}", r.label(), r.points())).collect();
        match self.veto {
            Some(v) => format!("{}/100, refus : {} (indices : {})", self.score, v.label(), if rs.is_empty() { "aucun".into() } else { rs.join(", ") }),
            None => format!("{}/100 ({})", self.score, rs.join(", ")),
        }
    }
}

/// Indices d'execution et de fichier d'un processus. Tout est optionnel : inconnu = neutre (ni bonus ni veto).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// Sous-systeme PE : Some(true) = console, Some(false) = fenetre, None = inconnu (ou pas un fichier PE).
    pub console: Option<bool>,
    /// Infos de version : editeur (CompanyName) et produit (ProductName).
    pub company: Option<String>,
    pub product: Option<String>,
    /// Nom du programme parent (minuscules), s'il est connu et toujours vivant.
    pub parent: Option<String>,
    /// Linux : le processus a un terminal de controle.
    pub tty: bool,
    /// Age du processus (ms).
    pub age_ms: Option<u64>,
    /// Bibliotheque graphique chargee.
    pub graphics: Option<bool>,
    /// Dossier d'installation confirme par le manifeste d'un lanceur, et nom du titre (seulement si l'exe est dessous).
    pub manifest: Option<(PathBuf, String)>,
    /// Processus d'une autre session (service du systeme) ou, sous Linux, d'un autre utilisateur.
    pub other_user: bool,
}

fn runtime_reasons(f: &Facts, rs: &mut Vec<Reason>) {
    if f.graphics == Some(true) {
        rs.push(Reason::Graphics);
    }
    if f.age_ms.map(|a| a >= LONG_AGE_MS).unwrap_or(false) {
        rs.push(Reason::LongLived);
    }
    if f.company.as_deref().map(is_game_publisher).unwrap_or(false) {
        rs.push(Reason::Publisher);
    }
}

/// Veto eventuel. `dev` : refus des dossiers / editeurs d'outils de developpement ; `console_parent` : refus des programmes
/// console et des lancements depuis un terminal. La duree de vie s'applique toujours.
pub fn veto_of(np: &str, f: &Facts, dev: bool, console_parent: bool) -> Option<Veto> {
    if f.other_user {
        return Some(Veto::System);
    }
    if dev && is_dev_dir(np) {
        return Some(Veto::DevFolder);
    }
    if dev && (f.company.as_deref().map(is_dev_publisher).unwrap_or(false) || f.product.as_deref().map(is_dev_product).unwrap_or(false)) {
        return Some(Veto::DevPublisher);
    }
    if console_parent && f.console == Some(true) {
        return Some(Veto::Console);
    }
    if console_parent && from_shell(f) {
        return Some(Veto::ShellParent);
    }
    if f.age_ms.map(|a| a < MIN_AGE_MS).unwrap_or(false) {
        return Some(Veto::ShortLived);
    }
    None
}

/// Forme comparable d'un nom : lettres et chiffres seulement, en minuscules (« ELDEN RING » -> « eldenring »).
pub fn alnum(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Dossiers trop generiques pour porter le nom d'un titre (forme `alnum`).
const GENERIC_DIRS: &[&str] = &[
    "bin", "bin32", "bin64", "x64", "x86", "x8664", "win32", "win64", "windows", "binaries", "game", "games", "jeux", "app", "apps", "release", "debug", "build", "programfiles",
    "programfilesx86", "programs", "common", "steamapps", "files", "data", "current", "active", "stable", "shipping", "retail", "client", "content", "engine", "system", "system32",
    "usr", "local", "opt", "lib", "share", "home", "users", "appdata", "roaming", "applications", "tools", "outils", "portable", "drivec", "prefixes",
    "pfx", "launcher", "default", "64bit", "32bit",
];

/// Noms (minuscules, bruts) du dossier d'installation (si `with_install`) et des `depth` dossiers parents de l'exe, sans les
/// dossiers generiques.
/// Decoupage sur le texte normalise : les chemins Windows (« C:\Jeux\... ») se lisent aussi sous Linux.
fn dir_names(path: &Path, install: &Path, depth: usize, with_install: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let a = alnum(raw);
        if a.chars().count() >= 2 && !GENERIC_DIRS.contains(&a.as_str()) && !v.iter().any(|x| x == raw) {
            v.push(raw.to_string());
        }
    };
    if with_install {
        if let Some(n) = norm_path(install).trim_end_matches('/').rsplit('/').next() {
            push(n);
        }
    }
    let np = norm_path(path);
    let parts: Vec<&str> = np.split('/').filter(|s| !s.is_empty()).collect();
    let parents = &parts[..parts.len().saturating_sub(1)];
    for n in parents.iter().rev().take(depth) {
        push(n);
    }
    v
}

/// Correspondance partielle dossier / titre (formes `alnum`) : le dossier est un morceau du titre (5 caracteres ou plus) ou le
/// contient. Un dossier qui porte simplement le nom du programme ne prouve rien (cas « .cargo\cargo.exe »).
fn partial_match(dir: &str, title: &str, stem: &str) -> bool {
    if dir.is_empty() || title.is_empty() || dir == stem {
        return false;
    }
    (dir.chars().count() >= 5 && title.contains(dir)) || (title != stem && title.chars().count() >= 5 && dir.contains(title))
}

/// Dossiers d'emulateurs (forme `alnum`) : « C:\Emu\pcsx2-qt.exe », « D:\Emulateurs\... ».
const EMU_DIRS: &[&str] = &["emu", "emus", "emulator", "emulators", "emulateur", "emulateurs", "emulation", "emulador", "emuladores", "emulatoren"];

/// Le dossier (ou le nom de produit) designe-t-il cet emulateur ? (« PCSX2 », « org.ppsspp.PPSSPP », « Dolphin-x64 », « Emu »)
fn emu_match(raw: &str, emu: &str) -> bool {
    let e = alnum(emu);
    let a = alnum(raw);
    a.contains(&e) || EMU_DIRS.contains(&a.as_str()) || raw.split(|c: char| !c.is_alphanumeric()).any(|t| t.chars().count() >= 4 && e.starts_with(t))
}

/// Dossiers d'outils de developpement (Windows, chemins normalises).
const WIN_DEV_DIRS: &[&str] = &[
    "/program files/git/", "/program files (x86)/git/", "/programs/git/", "/git/usr/", "/git/mingw64/", "/git/mingw32/", "/mingw64/", "/mingw32/", "/msys64/", "/msys32/", "/msys2/",
    "/cygwin/", "/cygwin64/", "/usr/bin/", "/usr/lib/", "/.cargo/", "/.rustup/", "/rustup/", "/nodejs/", "/node_modules/", "/.nvm/", "/nvm/", "/appdata/roaming/npm/", "/.npm/",
    "/pnpm/", "/.bun/", "/.deno/", "/yarn/", "/python/", "/python2", "/python3", "/pypy", "/anaconda", "/miniconda", "/.venv/", "/venv/", "/site-packages/", "/.pyenv/", "/pipx/",
    "/jetbrains/", "/microsoft visual studio", "/windows kits/", "/microsoft sdks/", "/android/sdk/", "/android/android studio/", "/android studio/", "/.android/", "/dotnet/",
    "/.dotnet/", "/.nuget/", "/go/bin/", "/golang/", "/java/", "/jdk", "/jre", "/eclipse adoptium/", "/zulu/", "/amazon corretto/", "/.gradle/", "/gradle", "/.m2/", "/apache-maven",
    "/cmake/", "/llvm/", "/ninja/", "/strawberry/", "/perl/", "/ruby", "/php/", "/docker/", "/github cli/", "/githubdesktop/", "/github desktop/", "/vagrant/", "/hashicorp/",
    "/.vscode/", "/.vscode-insiders/", "/programs/cursor/", "/.cursor/", "/target/debug/", "/target/release/", "/bin/debug/", "/x64/debug/", "/obj/debug/", "/appdata/local/temp/",
    "/temp/", "/sysinternals", "/wsl/", "/scoop/shims/", "/chocolatey/bin/", "/unity/hub/", "/unity hub/", "/epic games/ue_", "/engine/binaries/",
];

/// Dossiers d'outils de developpement (Linux).
const LINUX_DEV_DIRS: &[&str] = &[
    "/.cargo/", "/.rustup/", "/node_modules/", "/.nvm/", "/.npm/", "/.bun/", "/.deno/", "/.venv/", "/venv/", "/site-packages/", "/dist-packages/", "/.pyenv/", "/miniconda", "/anaconda",
    "/jetbrains/", "/.vscode/", "/.vscode-server/", "/.cursor/", "/usr/lib/jvm/", "/.gradle/", "/.m2/", "/go/bin/", "/.dotnet/", "/android/sdk/", "/android-studio/", "/target/debug/",
    "/target/release/", "/tmp/", "/unity/hub/", "/unityhub/", "/linuxbrew/",
];

/// Chemin (normalise) sous un dossier d'outils de developpement ? Linux : aussi les dossiers Windows dans un prefixe Wine.
pub fn is_dev_dir(np: &str) -> bool {
    if cfg!(windows) {
        WIN_DEV_DIRS.iter().any(|d| np.contains(d))
    } else {
        LINUX_DEV_DIRS.iter().any(|d| np.contains(d)) || (np.contains("/drive_c/") && WIN_DEV_DIRS.iter().filter(|d| !d.starts_with("/usr/")).any(|d| np.contains(d)))
    }
}

/// Editeurs d'outils de developpement (CompanyName, minuscules, sous-chaines).
const DEV_PUBLISHERS: &[&str] = &[
    "git development community", "github", "python software foundation", "openjs foundation", "node.js", "rust project developers", "rust foundation", "jetbrains", "eclipse adoptium",
    "eclipse foundation", "azul systems", "docker", "hashicorp", "kitware", "llvm", "msys2", "cygwin", "free software foundation", "strawberry perl", "activestate", "anaconda",
    "the go authors", "oracle", "apache software foundation", "gradle", "postman", "jfrog",
];

/// Produits d'outils de developpement (ProductName, minuscules, sous-chaines).
const DEV_PRODUCTS: &[&str] = &[
    "visual studio", "windows kits", "windows software development kit", ".net", "build tools", "sdk", "git for windows", "github cli", "rustup", "cargo", "python", "node.js",
    "intellij", "pycharm", "android studio", "msys2", "mingw", "windows subsystem for linux",
];

pub fn is_dev_publisher(company: &str) -> bool {
    let c = company.to_lowercase();
    DEV_PUBLISHERS.iter().any(|d| c.contains(d))
}

pub fn is_dev_product(product: &str) -> bool {
    let c = product.to_lowercase();
    DEV_PRODUCTS.iter().any(|d| c.contains(d))
}

/// Editeurs de jeux (CompanyName, minuscules, sous-chaines) : petit bonus seulement.
const GAME_PUBLISHERS: &[&str] = &[
    "valve", "electronic arts", "ubisoft", "bethesda", "cd projekt", "fromsoftware", "bandai namco", "square enix", "capcom", "sega", "larian", "paradox", "devolver", "team cherry",
    "re-logic", "mojang", "blizzard", "activision", "epic games", "riot games", "rockstar", "take-two", "2k games", "warner bros", "konami", "sony interactive", "playstation",
    "xbox game studios", "microsoft studios", "thq", "focus entertainment", "deep silver", "koch media", "505 games", "annapurna", "tinybuild", "raw fury", "coffee stain", "klei",
    "supergiant", "hello games", "frontier developments", "creative assembly", "iron gate", "ludeon", "wube", "colossal order", "kalypso", "nacon", "gearbox", "id software", "arkane",
    "obsidian", "nexon", "netease", "tencent", "mihoyo", "hoyoverse", "cognosphere", "krafton", "pearl abyss", "ncsoft", "grinding gear", "digital extremes", "bungie", "techland",
    "remedy", "io interactive", "4a games", "nintendo", "facepunch", "unknown worlds", "landfall", "re-logic",
];

pub fn is_game_publisher(company: &str) -> bool {
    let c = company.to_lowercase();
    GAME_PUBLISHERS.iter().any(|d| c.contains(d))
}

/// Parents qui signalent un lancement depuis un terminal, un interpreteur ou un environnement de developpement.
pub const SHELL_PARENTS: &[&str] = &[
    "cmd.exe", "powershell.exe", "pwsh.exe", "powershell_ise.exe", "bash.exe", "sh.exe", "zsh.exe", "fish.exe", "dash.exe", "mintty.exe", "windowsterminal.exe", "wt.exe",
    "openconsole.exe", "conhost.exe", "alacritty.exe", "wezterm-gui.exe", "wezterm.exe", "conemu64.exe", "conemu.exe", "conemuc64.exe", "cmder.exe", "hyper.exe", "tabby.exe", "code.exe",
    "code - insiders.exe", "cursor.exe", "devenv.exe", "rider64.exe", "idea64.exe", "clion64.exe", "pycharm64.exe", "webstorm64.exe", "goland64.exe", "rustrover64.exe", "studio64.exe",
    "cargo.exe", "rustc.exe", "rustup.exe", "node.exe", "npm.exe", "python.exe", "python3.exe", "py.exe", "git.exe", "wsl.exe", "wslhost.exe", "msbuild.exe", "dotnet.exe", "make.exe",
    "cmake.exe", "ninja.exe", "gdb.exe", "lldb.exe", "vsdebugconsole.exe", "msvsmon.exe",
    // Linux (avec un terminal de controle)
    "bash", "sh", "dash", "zsh", "fish", "tcsh", "csh", "ksh", "konsole", "gnome-terminal-server", "gnome-terminal", "xterm", "alacritty", "kitty", "wezterm-gui", "foot", "tilix",
    "terminator", "xfce4-terminal", "lxterminal", "mate-terminal", "yakuake", "tmux: server", "screen", "code", "cargo", "python", "python3", "node",
];

/// Lance depuis un terminal ? Windows : parent dans `SHELL_PARENTS`. Linux : parent dans la liste ET terminal de controle
/// (les jeux lances par Steam passent par `sh` ou `python3` sans terminal : ils ne sont pas concernes).
pub fn from_shell(f: &Facts) -> bool {
    let p = f.parent.as_deref().map(|s| s.to_lowercase()).unwrap_or_default();
    if p.is_empty() || !SHELL_PARENTS.contains(&p.as_str()) {
        return false;
    }
    cfg!(windows) || f.tty
}

// ------------------------------------------------------------------ memoire des choix

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// "yes" | "no" | "later"
    pub state: String,
    pub exe: String,
    pub name: String,
    /// « Plus tard » : pas de nouvelle proposition avant cette date (ms UTC).
    #[serde(default)]
    pub later_until_ms: u64,
    /// 2.0.3 : nombre de « Plus tard » donnes sur l'ecran de fermeture (au-dela de `CLOSE_LATER_MAX`, plus jamais d'ecran :
    /// seulement la carte de l'accueil).
    #[serde(default)]
    pub close_later: u32,
}

/// Nombre maximal de fois ou l'ecran de fermeture est REPROPOSE apres un « Plus tard » (ensuite : carte de l'accueil seulement).
pub const CLOSE_LATER_MAX: u32 = 2;

/// Memoire persistante des choix (par jeu : cle = exe normalise + dossier d'installation).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct Memory {
    #[serde(default)]
    pub decisions: BTreeMap<String, Decision>,
}

pub const MEMORY_FILE: &str = "detected-games.json";

impl Memory {
    pub fn load(dir: &Path) -> Memory {
        std::fs::read_to_string(dir.join(MEMORY_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> crate::Res<()> {
        crate::util::write_atomic(&dir.join(MEMORY_FILE), serde_json::to_string_pretty(self)?.as_bytes())
    }

    pub fn answer(&mut self, c: &Candidate, state: &str, now_ms: u64) {
        let later = if state == "later" { now_ms + LATER_MS } else { 0 };
        let close_later = self.decisions.get(&c.key).map(|d| d.close_later).unwrap_or(0);
        self.decisions.insert(c.key.clone(), Decision { state: state.to_string(), exe: c.exe.clone(), name: c.name.clone(), later_until_ms: later, close_later });
    }

    /// « Plus tard » sur l'ecran de fermeture : compte, et memorise un « plus tard » ordinaire (jamais un refus).
    pub fn close_later(&mut self, c: &Candidate, now_ms: u64) {
        let n = self.close_later_count(&c.key) + 1;
        self.answer(c, "later", now_ms);
        if let Some(d) = self.decisions.get_mut(&c.key) {
            d.close_later = n;
        }
    }

    pub fn close_later_count(&self, key: &str) -> u32 {
        self.decisions.get(key).map(|d| d.close_later).unwrap_or(0)
    }

    /// L'ecran de fermeture peut-il encore etre propose pour ce programme ? Non apres un « oui » / « jamais », ni au-dela
    /// de `CLOSE_LATER_MAX` « Plus tard » (la carte de l'accueil reste).
    pub fn may_show_close(&self, key: &str) -> bool {
        match self.decisions.get(key) {
            None => true,
            Some(d) => d.state == "later" && d.close_later <= CLOSE_LATER_MAX,
        }
    }

    /// Peut-on (re)proposer ce jeu maintenant ?
    pub fn may_propose(&self, key: &str, now_ms: u64) -> bool {
        match self.decisions.get(key) {
            None => true,
            Some(d) => d.state == "later" && now_ms >= d.later_until_ms,
        }
    }

    /// Jeux ignores (« Non »), pour la liste des Reglages.
    pub fn ignored(&self) -> Vec<(&String, &Decision)> {
        self.decisions.iter().filter(|(_, d)| d.state == "no").collect()
    }
}

// ------------------------------------------------------------------ detecteur (machine a etats)

/// Portee de la detection : tout (defaut), jeux seulement (base embarquee, dossiers de lanceurs, emulateurs, profils de jeux),
/// applications seulement (profils app / config).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectScope {
    #[default]
    All,
    Games,
    Apps,
}

impl DetectScope {
    pub fn code(self) -> &'static str {
        match self {
            DetectScope::All => "all",
            DetectScope::Games => "games",
            DetectScope::Apps => "apps",
        }
    }
}

/// Le candidat est-il dans la portee choisie ?
pub fn in_scope(c: &Candidate, s: DetectScope) -> bool {
    match s {
        DetectScope::All => true,
        DetectScope::Games => !c.app,
        DetectScope::Apps => c.app,
    }
}

/// Delai entre la detection et la bulle Windows (ne pas gener le chargement du jeu). Constante reglable ici.
pub const NOTIFY_DELAY_MS: u64 = 15_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Un jeu candidat vient de demarrer : sa carte est ajoutee a la file tout de suite (mode « notification au lancement »).
    Launched(Candidate),
    /// 15 s apres le lancement et le jeu tourne toujours : envoyer la petite bulle Windows.
    Notify(Candidate),
    /// Tous les processus du jeu candidat se sont fermes (tous modes) : sert a afficher la pile de cartes apres le jeu.
    Ended(Candidate),
}

#[derive(Default)]
pub struct Detector {
    baseline: HashSet<(u32, u64)>,
    baseline_done: bool,
    /// pid -> candidat en cours d'execution (demarre apres l'appli).
    running: HashMap<u32, (u64, Candidate)>,
    /// Cles dont la carte a deja ete ajoutee pendant cette session.
    carded: HashSet<String>,
    /// Cles deja notifiees (bulle) pendant cette session : une seule bulle par jeu et par session.
    notified: HashSet<String>,
    /// cle -> (echeance de la bulle, pid, heure de demarrage du processus) ; annulee si le processus se ferme avant.
    notify_due: HashMap<String, (u64, u32, u64)>,
    /// Dossier d installation -> cle du candidat retenu (un seul exe par dossier).
    dir_owner: HashMap<String, String>,
    /// Classification supplementaire (profils communautaires, voir `assess_profile`) quand la base / les lanceurs ne
    /// donnent pas un score suffisant.
    pub extra: Option<Box<dyn Fn(&ProcInfo, &Facts) -> Option<Candidate> + Send>>,
    /// Portee courante (modifiable sans redemarrage : appliquee au passage suivant).
    pub scope: DetectScope,
    /// Refus notables (processus qui ressemblaient a un jeu, score sous le seuil ou veto), une seule fois par cle et par
    /// session : a vider par l'appelant pour le journal.
    pub refused: Vec<Candidate>,
    refused_keys: HashSet<String>,
    /// Indices d'execution du prochain passage, par (pid, heure de demarrage) : remplis par l'appelant, consommes par `scan`.
    pub facts: HashMap<(u32, u64), Facts>,
    /// Processus refuses par un veto dur : jamais reevalues (le parent terminal peut se fermer entre-temps).
    vetoed: HashSet<(u32, u64)>,
}

impl Detector {
    /// Nombre de processus candidats en cours d execution.
    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    /// Ce processus est-il un candidat en cours (carte de detection) ?
    pub fn is_running(&self, pid: u32, start: u64) -> bool {
        self.running.get(&pid).map(|r| r.0 == start).unwrap_or(false)
    }

    /// Un passage (toutes les ~5 s). La toute premiere liste sert de reference : les processus deja la ne sont jamais
    /// consideres (seuls ceux demarres apres le lancement de l'appli le sont). `notifications` : interrupteur global des
    /// notifications (les cartes sont ajoutees dans tous les cas).
    ///
    /// Les indices d'execution de ce passage sont pris dans `self.facts` (rempli par l'appelant, vide = indices inconnus) puis
    /// effaces. Un candidat refuse (score sous le seuil, processus encore trop jeune...) est reevalue au passage suivant.
    pub fn scan(&mut self, procs: &[ProcInfo], env: &Env, mem: &Memory, notifications: bool, now_ms: u64) -> Vec<Event> {
        let mut facts = std::mem::take(&mut self.facts);
        let mut out = Vec::new();
        if !self.baseline_done {
            self.baseline = procs.iter().map(|p| (p.pid, p.start)).collect();
            self.baseline_done = true;
            return out;
        }
        let alive: HashSet<(u32, u64)> = procs.iter().map(|p| (p.pid, p.start)).collect();
        self.vetoed.retain(|k| alive.contains(k));
        // Nouveaux candidats de ce passage, l'exe PRINCIPAL d'abord (base > emulateur > profil > dossier de lanceur) : un seul par dossier d'installation.
        let mut news: Vec<(u32, u64, Candidate)> = Vec::new();
        for p in procs {
            if self.baseline.contains(&(p.pid, p.start)) || self.vetoed.contains(&(p.pid, p.start)) || self.running.get(&p.pid).map(|r| r.0 == p.start).unwrap_or(false) {
                continue;
            }
            let f = facts.remove(&(p.pid, p.start)).unwrap_or_default();
            let base = assess(p, env, &f);
            let chosen = match base {
                Some(c) if is_proposable(&c) => Some(c),
                other => {
                    let prof = self.extra.as_ref().and_then(|x| x(p, &f));
                    if prof.as_ref().map(is_proposable).unwrap_or(false) {
                        prof
                    } else {
                        // Refus : on garde le meilleur des deux pour le journal (pas les processus encore trop jeunes).
                        let best = [other, prof].into_iter().flatten().max_by_key(|c| c.confidence.as_ref().map(|s| s.score).unwrap_or(0));
                        if let Some(b) = best {
                            let young = b.confidence.as_ref().map(|s| s.veto == Some(Veto::ShortLived)).unwrap_or(false);
                            // Refus dur (console, terminal, dossier d'outils...) : definitif pour ce processus, meme si son
                            // parent se ferme ensuite (« cmd /c start jeu.exe »).
                            if !young && b.confidence.as_ref().map(|s| s.veto.is_some()).unwrap_or(false) {
                                self.vetoed.insert((p.pid, p.start));
                            }
                            if !young && self.refused_keys.insert(b.key.clone()) {
                                self.refused.push(b);
                            }
                        }
                        None
                    }
                }
            };
            let Some(c) = chosen.filter(|c| !env.known_exes.contains(&c.exe)) else { continue };
            if !in_scope(&c, self.scope) || !mem.may_propose(&c.key, now_ms) {
                continue;
            }
            news.push((p.pid, p.start, c));
        }
        news.sort_by_key(|(_, _, c)| match c.why {
            Why::InDb => 0,
            Why::Emulator => 1,
            Why::Profile => 2,
            Why::LauncherFolder => 3,
            Why::Learned => 4,
        });
        for (pid, start, c) in news {
            let dir = c.install_dir.as_ref().map(|d| norm_path(d)).unwrap_or_default();
            if !dir.is_empty() {
                // Un autre exe du meme dossier a deja ete retenu (ex. exe principal + exe auxiliaire) : on garde le premier.
                match self.dir_owner.get(&dir) {
                    Some(k) if *k != c.key => continue,
                    _ => {
                        self.dir_owner.insert(dir, c.key.clone());
                    }
                }
            }
            self.running.insert(pid, (start, c.clone()));
            if self.carded.insert(c.key.clone()) {
                out.push(Event::Launched(c.clone()));
            }
            if notifications && !self.notified.contains(&c.key) && !self.notify_due.contains_key(&c.key) {
                self.notify_due.insert(c.key.clone(), (now_ms + NOTIFY_DELAY_MS, pid, start));
            }
        }
        // Bulles echues : seulement si le processus tourne toujours ; sinon annulees (la carte reste en file).
        let due: Vec<String> = self.notify_due.iter().filter(|(_, (t, _, _))| now_ms >= *t).map(|(k, _)| k.clone()).collect();
        for k in due {
            let (_, pid, st) = self.notify_due.remove(&k).unwrap();
            if alive.contains(&(pid, st)) && mem.may_propose(&k, now_ms) && self.notified.insert(k.clone()) {
                if let Some((_, c)) = self.running.get(&pid) {
                    out.push(Event::Notify(c.clone()));
                }
            }
        }
        let gone: Vec<u32> = self.running.iter().filter(|(pid, (st, _))| !alive.contains(&(**pid, *st))).map(|(p, _)| *p).collect();
        for pid in gone {
            if let Some((st, c)) = self.running.remove(&pid) {
                if self.notify_due.get(&c.key).map(|d| d.1 == pid && d.2 == st).unwrap_or(false) {
                    self.notify_due.remove(&c.key); // ferme avant l'echeance : pas de bulle
                }
                // Plus aucun autre processus du meme jeu : fermeture.
                let still = self.running.values().any(|(_, o)| o.key == c.key);
                if !still {
                    out.push(Event::Ended(c.clone()));
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------ file de propositions (cartes en overlay)

/// Duree de conservation d'une proposition non repondue (fichier leger).
pub const EXPIRY_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// « Plus tard » : la carte revient apres ce delai (sans etre memorisee comme un refus).
pub const SNOOZE_MS: u64 = 10 * 60 * 1000;
pub const PENDING_FILE: &str = "pending-proposals.json";
/// Duree du fondu d'apparition de la carte.
pub const FADE_MS: u64 = 450;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub cand: Candidate,
    pub created_ms: u64,
    /// « Plus tard » : n'est plus affichee avant cette date.
    #[serde(default)]
    pub snooze_until_ms: u64,
}

/// File des cartes : conservee (memoire de session + fichier avec expiration 7 jours) tant que l'utilisateur n'a pas repondu ;
/// affichees UNE APRES L'AUTRE, sans doublon.
#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Proposals {
    pub queue: VecDeque<Pending>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Blockers {
    pub tutorial: bool,
    pub confirm_open: bool,
    pub restoring: bool,
    /// La fenetre de l'appli n'est pas visible (cachee dans la zone de notification) : les cartes attendent.
    pub window_hidden: bool,
    /// Detection desactivee : la file est suspendue (conservee, expiration normale) et aucune carte n est affichee.
    pub detection_off: bool,
}

impl Proposals {
    /// Ajoute une carte (sans doublon : meme cle deja en file = ignoree).
    pub fn push(&mut self, c: Candidate, now_ms: u64) {
        if !self.queue.iter().any(|p| p.cand.key == c.key) {
            self.queue.push_back(Pending { cand: c, created_ms: now_ms, snooze_until_ms: 0 });
        }
    }

    /// Supprime les cartes expirees (7 jours).
    pub fn expire(&mut self, now_ms: u64) {
        self.queue.retain(|p| now_ms.saturating_sub(p.created_ms) < EXPIRY_MS);
    }

    /// Carte a afficher maintenant : la premiere de la file non mise de cote, sauf blocage. Retourne aussi (rang 1-based, total)
    /// pour l'indicateur « 1/3 » (le total compte les cartes en attente, mises de cote comprises).
    pub fn current(&self, b: Blockers, now_ms: u64) -> Option<(&Pending, usize, usize)> {
        if b.tutorial || b.confirm_open || b.restoring || b.window_hidden || b.detection_off {
            return None;
        }
        let total = self.queue.len();
        self.queue.iter().enumerate().find(|(_, p)| now_ms >= p.snooze_until_ms).map(|(i, p)| (p, i + 1, total))
    }

    /// Retire de la file les cartes hors portee (sans memoriser de refus). Retourne le nombre retire.
    pub fn retain_scope(&mut self, s: DetectScope) -> usize {
        let before = self.queue.len();
        self.queue.retain(|p| in_scope(&p.cand, s));
        before - self.queue.len()
    }

    pub fn resolve(&mut self, key: &str) {
        self.queue.retain(|p| p.cand.key != key);
    }

    /// « Plus tard » : la carte passe en fin de file et revient apres `SNOOZE_MS`.
    pub fn snooze(&mut self, key: &str, now_ms: u64) {
        if let Some(i) = self.queue.iter().position(|p| p.cand.key == key) {
            if let Some(mut p) = self.queue.remove(i) {
                p.snooze_until_ms = now_ms + SNOOZE_MS;
                self.queue.push_back(p);
            }
        }
    }

    /// Fermeture du programme (2.0.3) : sa carte passe en tete de file, sans attente ; ajoutee si elle n'y est plus (« Plus
    /// tard » de l'ecran de fermeture la retire sans rien memoriser : elle revient a la fermeture suivante).
    pub fn promote(&mut self, c: &Candidate, now_ms: u64) {
        let p = match self.queue.iter().position(|p| p.cand.key == c.key) {
            Some(i) => self.queue.remove(i),
            None => None,
        };
        let mut p = p.unwrap_or(Pending { cand: c.clone(), created_ms: now_ms, snooze_until_ms: 0 });
        p.snooze_until_ms = 0;
        self.queue.push_front(p);
    }

    pub fn save(&self, dir: &Path) -> crate::Res<()> {
        crate::util::write_atomic(&dir.join(PENDING_FILE), serde_json::to_string_pretty(self)?.as_bytes())
    }

    /// Relit le fichier (expirees retirees) ; les jeux dont le choix est deja memorise (oui / non) sont retires.
    pub fn load(dir: &Path, mem: &Memory, now_ms: u64) -> Proposals {
        let mut p: Proposals = std::fs::read_to_string(dir.join(PENDING_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        p.expire(now_ms);
        p.queue.retain(|e| mem.decisions.get(&e.cand.key).map(|d| d.state == "later").unwrap_or(true));
        p
    }
}

/// Opacite de la carte (0 a 1) selon le temps ecoule depuis son apparition : fondu progressif (ease-out).
pub fn fade(shown_at_ms: u64, now_ms: u64) -> f32 {
    let t = (now_ms.saturating_sub(shown_at_ms) as f32 / FADE_MS as f32).min(1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// Etat de l'overlay : aucune carte, une carte affichee (fondu depuis `since`), ou la confirmation brève apres « Oui ».
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum CardState {
    #[default]
    Idle,
    Showing { key: String, since_ms: u64 },
    Confirmed { name: String, until_ms: u64 },
}

pub const CONFIRM_MS: u64 = 1800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Later,
}

impl CardState {
    /// Met a jour l'etat selon la carte courante (None = rien a afficher). La confirmation passe d'abord.
    pub fn sync(&mut self, current_key: Option<&str>, now_ms: u64) {
        if let CardState::Confirmed { until_ms, .. } = self {
            if now_ms < *until_ms {
                return;
            }
        }
        *self = match current_key {
            None => CardState::Idle,
            Some(k) => match self {
                CardState::Showing { key, .. } if key == k => return,
                _ => CardState::Showing { key: k.to_string(), since_ms: now_ms },
            },
        };
    }

    /// Reponse de l'utilisateur : Oui = confirmation breve ; Non / Plus tard = on passe a la carte suivante (fondu a nouveau).
    pub fn answer(&mut self, a: Answer, name: &str, now_ms: u64) {
        *self = match a {
            Answer::Yes => CardState::Confirmed { name: name.to_string(), until_ms: now_ms + CONFIRM_MS },
            _ => CardState::Idle,
        };
    }
}

/// Effet d'une reponse sur la memoire des choix et la file.
pub fn apply_answer(mem: &mut Memory, q: &mut Proposals, cand: &Candidate, a: Answer, now_ms: u64) {
    match a {
        Answer::Yes => {
            mem.answer(cand, "yes", now_ms);
            q.resolve(&cand.key);
        }
        Answer::No => {
            mem.answer(cand, "no", now_ms);
            q.resolve(&cand.key);
        }
        Answer::Later => {
            // Jamais memorise comme un refus : la carte reste en file, mise de cote.
            mem.answer(cand, "later", now_ms);
            q.snooze(&cand.key, now_ms);
        }
    }
}

// ------------------------------------------------------------------ pile de cartes (geometrie et animations, pures)

/// Couches visibles derriere la carte active (au plus 3 ; un badge « +N » compte le reste).
pub const MAX_LAYERS: usize = 3;
/// Duree des animations de sortie / de montee d'une carte.
pub const ANIM_MS: u64 = 250;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerGeom {
    pub dx: f32,
    pub dy: f32,
    /// Rotation en degres.
    pub rot_deg: f32,
    /// 1.0 pour la carte active, plus petit derriere.
    pub scale: f32,
    /// 0.0 = pleine luminosite ; 1.0 = tres sombre.
    pub dim: f32,
}

fn geom_int(k: u32) -> LayerGeom {
    let s = if k % 2 == 1 { 1.0 } else { -1.0 };
    let kf = k as f32;
    LayerGeom { dx: s * 12.0 * kf, dy: 14.0 * kf, rot_deg: s * 3.5 * kf, scale: 1.0 - 0.045 * kf, dim: (0.22 * kf).min(0.8) }
}

/// Geometrie d'une couche de profondeur `depth` (0 = active ; fractionnaire pendant une animation : interpolation).
pub fn layer_geom(depth: f32) -> LayerGeom {
    let d = depth.max(0.0);
    let k = d.floor() as u32;
    let t = d - d.floor();
    let (a, b) = (geom_int(k), geom_int(k + 1));
    let l = |x: f32, y: f32| x + (y - x) * t;
    LayerGeom { dx: l(a.dx, b.dx), dy: l(a.dy, b.dy), rot_deg: l(a.rot_deg, b.rot_deg), scale: l(a.scale, b.scale), dim: l(a.dim, b.dim) }
}

/// (nombre de couches dessinees derriere la carte active, badge « +N »). `behind` = cartes en attente apres la carte active.
pub fn visible_layers(behind: usize) -> (usize, usize) {
    (behind.min(MAX_LAYERS), behind.saturating_sub(MAX_LAYERS))
}

pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Duree (ms) de l animation de sortie de la carte du dessus : Oui 800, Non 700, Plus tard 600 ; fondu tres court si les
/// animations Windows sont reduites.
pub fn exit_ms(a: Answer, reduce: bool) -> u64 {
    if reduce {
        return 160;
    }
    match a {
        Answer::Yes => 800,
        Answer::No => 700,
        Answer::Later => 600,
    }
}

/// Progression (0 a 1) d'une animation de sortie demarree a `start_ms`.
pub fn exit_progress(a: Answer, reduce: bool, start_ms: u64, now_ms: u64) -> f32 {
    (now_ms.saturating_sub(start_ms) as f32 / exit_ms(a, reduce) as f32).clamp(0.0, 1.0)
}

/// Image d'animation de la carte du dessus qui part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExitFrame {
    pub dx: f32,
    pub dy: f32,
    pub rot_deg: f32,
    /// Multiplicateur d'echelle (1.0 = taille normale).
    pub scale: f32,
    pub alpha: f32,
    /// Eclat de couleur d'accent (0 = aucun ; intensite 0..1).
    pub burst: f32,
    /// Plus tard : profondeur dans la pile vers laquelle la carte glisse (derriere le paquet) ; None sinon.
    pub to_depth: Option<f32>,
}

/// Image de l'animation de sortie. `layers` = nombre de couches derriere (la carte glisse derriere elles pour « Plus tard »).
///  - Oui : petit bond joyeux (saut + rebond + echelle) avec un eclat d'accent, puis la carte glisse vers le haut / l'onglet
///    « Mes jeux » (en haut a gauche) en retrecissant ;
///  - Non : petit tremblement de refus puis la carte est jetee de cote en tournant et tombe hors de l'ecran ;
///  - Plus tard : la carte glisse doucement derriere le paquet, sans drame.
///  `reduce` (animations Windows reduites) : simple fondu tres court.
pub fn exit_frame(a: Answer, progress: f32, reduce: bool, layers: usize) -> ExitFrame {
    let p = progress.clamp(0.0, 1.0);
    if reduce {
        return ExitFrame { dx: 0.0, dy: 0.0, rot_deg: 0.0, scale: 1.0, alpha: 1.0 - p, burst: 0.0, to_depth: None };
    }
    match a {
        Answer::Yes => {
            let hop_end = 0.35;
            if p < hop_end {
                let u = p / hop_end;
                let hop = (u * std::f32::consts::PI).sin();
                // saut + leger rebond (petite oscillation amortie) + echelle qui gonfle
                let bounce = (u * std::f32::consts::PI * 3.0).sin() * (1.0 - u) * 4.0;
                ExitFrame { dx: 0.0, dy: -30.0 * hop + bounce, rot_deg: 0.0, scale: 1.0 + 0.06 * hop, alpha: 1.0, burst: hop, to_depth: None }
            } else {
                let u = ease_out((p - hop_end) / (1.0 - hop_end));
                ExitFrame { dx: -240.0 * u, dy: -440.0 * u, rot_deg: -6.0 * u, scale: 1.0 - 0.72 * u, alpha: if u > 0.7 { 1.0 - (u - 0.7) / 0.3 } else { 1.0 }, burst: (1.0 - u).max(0.0) * 0.4, to_depth: None }
            }
        }
        Answer::No => {
            let shake_end = 0.35;
            if p < shake_end {
                let u = p / shake_end;
                ExitFrame { dx: 14.0 * (u * std::f32::consts::PI * 6.0).sin() * (1.0 - u * 0.4), dy: 0.0, rot_deg: 0.0, scale: 1.0, alpha: 1.0, burst: 0.0, to_depth: None }
            } else {
                let u = (p - shake_end) / (1.0 - shake_end);
                let e = u * u;
                ExitFrame { dx: 520.0 * ease_out(u), dy: 380.0 * e, rot_deg: 38.0 * ease_out(u), scale: 1.0, alpha: 1.0 - e * 0.8, burst: 0.0, to_depth: None }
            }
        }
        Answer::Later => {
            let e = ease_out(p);
            let target = layers as f32 + 1.0;
            ExitFrame { dx: 0.0, dy: 0.0, rot_deg: 0.0, scale: 1.0, alpha: 1.0 - 0.3 * e, burst: 0.0, to_depth: Some(target * e) }
        }
    }
}

/// Profondeur de la carte qui monte pendant la sortie de la carte du dessus (`k` = rang derriere : 1 = la suivante).
pub fn rise_depth(k: usize, progress: f32) -> f32 {
    (k as f32 - ease_out(progress)).max(0.0)
}

/// Une animation est-elle encore en cours ? (repaint seulement pendant ce temps : aucune boucle quand rien ne bouge)
pub fn animating(a: Answer, reduce: bool, start_ms: Option<u64>, now_ms: u64) -> bool {
    start_ms.map(|s| now_ms.saturating_sub(s) < exit_ms(a, reduce) + 30).unwrap_or(false)
}

/// Delai (ms) de la confirmation « ajoute » apres une reponse Oui : elle apparait quand la carte a fini de partir.
pub fn confirm_start_ms(reduce: bool) -> u64 {
    exit_ms(Answer::Yes, reduce)
}

// ------------------------------------------------------------------ afficher l'appli a la fermeture d'un jeu detecte

/// Delai apres la fermeture avant de passer au premier plan.
pub const SHOW_ON_CLOSE_DELAY_MS: u64 = 3000;
/// Au-dela de ce delai sans avoir pu s'afficher (autre programme, plein ecran, mode console...), l'ecran de fermeture est
/// abandonne : la carte reste sur l'accueil, jamais de surgissement tardif.
pub const CLOSE_WAIT_MS: u64 = 10 * 60 * 1000;

/// Contexte evalue a chaque passage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShowCtx {
    /// Interrupteur « Afficher l'appli a la fermeture d'un jeu detecte ».
    pub enabled: bool,
    pub cards_pending: bool,
    /// Un AUTRE jeu surveille ou detecte tourne encore.
    pub other_game_running: bool,
    /// Une application plein ecran semble active.
    pub fullscreen_active: bool,
    pub tutorial: bool,
    pub confirm_open: bool,
    /// L'appli est deja au premier plan : rien a faire.
    pub already_foreground: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowAction {
    None,
    /// Passer l'appli au premier plan.
    BringToFront,
    /// Windows a refuse : faire clignoter le bouton de la barre des taches, sans insister.
    FlashTaskbar,
    /// L'appli etait deja au premier plan : rien a demander a Windows (l'ecran de fermeture s'ouvre quand meme).
    AlreadyFront,
    /// Impossible de s'afficher pendant `CLOSE_WAIT_MS` : ecran abandonne, la carte de l'accueil suffit.
    Expired,
}

/// Machine a etats : une seule fois par fermeture, jamais pendant qu'un jeu tourne, report si un autre jeu / un plein ecran.
#[derive(Default, Debug)]
pub struct ShowOnClose {
    /// Fermeture detectee a cet instant, pas encore traitee.
    closed_at: Option<u64>,
}

impl ShowOnClose {
    /// Un jeu detecte vient de se fermer (tous ses processus).
    pub fn on_closed(&mut self, now_ms: u64) {
        self.closed_at = Some(now_ms);
    }

    /// Un passage. `front` tente le passage au premier plan (true = accorde).
    pub fn tick(&mut self, c: &ShowCtx, now_ms: u64, front: &mut dyn FnMut() -> bool) -> ShowAction {
        let Some(t) = self.closed_at else { return ShowAction::None };
        if !c.enabled || !c.cards_pending {
            self.closed_at = None; // rien a montrer : on oublie cette fermeture
            return ShowAction::None;
        }
        if now_ms < t + SHOW_ON_CLOSE_DELAY_MS {
            return ShowAction::None;
        }
        if now_ms >= t + CLOSE_WAIT_MS {
            self.closed_at = None; // trop tard : jamais de surgissement des heures apres
            return ShowAction::Expired;
        }
        if c.other_game_running || c.fullscreen_active || c.tutorial || c.confirm_open {
            return ShowAction::None; // reporte (a la fermeture de l'autre jeu / du plein ecran), au plus CLOSE_WAIT_MS
        }
        self.closed_at = None; // une seule fois par fermeture
        if c.already_foreground {
            return ShowAction::AlreadyFront;
        }
        if front() {
            ShowAction::BringToFront
        } else {
            ShowAction::FlashTaskbar
        }
    }
}

// ------------------------------------------------------------------ programmes inconnus : apprentissage (2.0.3)

/// Un programme inconnu doit tourner avec une vraie fenetre depuis au moins 2 minutes avant d'etre observe.
pub const UNKNOWN_MIN_MS: u64 = 120_000;

/// Programmes courants jamais appris (navigateurs, messageries, bureautique, creation, lecteurs, outils du systeme, des
/// constructeurs et des lanceurs) : ce ne sont pas des programmes dont on cherche les donnees a leur fermeture, et ils
/// tournent souvent des heures. Noms Windows seulement (l'apprentissage n'existe pas sous Linux). Cette liste n'est qu'un
/// filtre de confort : la vraie protection est la regle « ecran seulement si quelque chose a ete trouve ».
pub const NOT_LEARNED: &[&str] = &[
    "discord.exe", "spotify.exe", "slack.exe", "teams.exe", "ms-teams.exe", "whatsapp.exe", "telegram.exe", "signal.exe", "zoom.exe", "skype.exe",
    "obs64.exe", "obs32.exe", "notepad.exe", "notepad++.exe", "mspaint.exe", "winword.exe", "excel.exe", "powerpnt.exe", "outlook.exe", "olk.exe", "onenote.exe",
    "acrord32.exe", "acrobat.exe", "vlc.exe", "mpc-hc64.exe", "wmplayer.exe", "steamwebhelper.exe", "epicgameslauncher.exe", "galaxyclient.exe",
    "playnite.desktopapp.exe", "playnite.fullscreenapp.exe", "autogamesoftwaresync.exe", "taskmgr.exe", "systemsettings.exe", "applicationframehost.exe",
    "searchhost.exe", "textinputhost.exe", "brave.exe", "opera.exe", "vivaldi.exe", "thunderbird.exe", "7zfm.exe", "winrar.exe", "keepass.exe",
    "keepassxc.exe", "bitwarden.exe", "1password.exe", "battle.net.exe", "eadesktop.exe", "upc.exe", "ubisoftconnect.exe", "gamebar.exe", "xboxpcapp.exe",
    "nvidia app.exe", "nvcontainer.exe", "msiafterburner.exe", "rtss.exe", "wallpaper64.exe", "wallpaper32.exe", "ds4windows.exe", "anydesk.exe",
    "teamviewer.exe", "rustdesk.exe", "everything.exe", "powertoys.exe", "snippingtool.exe", "calculatorapp.exe", "windowsterminal.exe", "mmc.exe",
    "regedit.exe", "control.exe", "mstsc.exe", "explorer.exe", "gimp-2.10.exe", "gimp.exe", "blender.exe", "photoshop.exe", "illustrator.exe", "krita.exe",
    "inkscape.exe", "audacity.exe", "soffice.bin", "soffice.exe", "swriter.exe", "scalc.exe", "virtualbox.exe", "virtualboxvm.exe", "vmware.exe",
    "vmplayer.exe", "lghub.exe", "icue.exe", "armourycrate.exe", "armourycrateservice.exe", "armourycrate.service.exe", "legionspace.exe", "steelseriesgg.exe",
    "razer synapse 3.exe", "razercentral.exe", "nzxt cam.exe", "corsair.service.exe", "amdrsserv.exe", "radeonsoftware.exe", "geforce experience.exe",
    "msedge.exe", "chrome.exe", "firefox.exe", "foobar2000.exe", "itunes.exe", "onedrive.exe", "dropbox.exe", "googledrivefs.exe",
];

/// Programme INCONNU a observer (apprentissage pendant l'utilisation) : demarre apres l'appli, vraie fenetre, au moins 2 min,
/// ni carte de detection (base, lanceur, profil, emulateur), ni outil de developpement, ni programme systeme / console /
/// lance depuis un terminal, ni programme courant (`NOT_LEARNED`), ni deja suivi, ni deja refuse (« Jamais pour celui-ci »).
pub fn unknown_candidate(p: &ProcInfo, f: &Facts, has_window: bool, env: &Env, mem: &Memory) -> Option<Candidate> {
    let exe = p.exe.to_lowercase();
    if !has_window || exe.is_empty() || is_auxiliary(&exe) || EXCLUDED_EXES.contains(&exe.as_str()) || GENERIC_EXES.contains(&exe.as_str()) || NOT_LEARNED.contains(&exe.as_str()) {
        return None;
    }
    if env.known_exes.iter().any(|k| *k == exe) {
        return None;
    }
    if f.age_ms.map(|a| a < UNKNOWN_MIN_MS).unwrap_or(true) {
        return None;
    }
    let path = p.path.as_ref()?;
    let np = norm_path(path);
    if is_system_path(&np) || is_aux_dir(&np) || linux_excluded(&exe, &np) || veto_of(&np, f, true, true).is_some() {
        return None;
    }
    // Deja une carte possible (base, lanceur, emulateur) : la detection ordinaire s'en charge.
    if assess(p, env, f).map(|c| is_proposable(&c)).unwrap_or(false) {
        return None;
    }
    let install = install_dir_of(path, env.launcher_roots);
    let key = format!("{}|{}", exe, norm_path(&install));
    if mem.decisions.get(&key).map(|d| d.state == "no" || d.state == "yes").unwrap_or(false) {
        return None;
    }
    let product = f.product.as_deref().map(str::trim).filter(|s| s.chars().count() >= 3 && !s.to_lowercase().contains("operating system")).map(|s| s.to_string());
    let name = product.unwrap_or_else(|| pretty_name(&p.exe));
    Some(Candidate { key, exe, path: Some(path.clone()), why: Why::Learned, name, install_dir: Some(install), profile_id: None, app: false, confidence: None })
}

/// Evenement de l'observation des programmes inconnus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnknownEvent {
    /// A observer (dossiers modifies) jusqu'a sa fermeture.
    Started(Candidate),
    /// Tous ses processus se sont fermes.
    Ended(Candidate),
}

/// Observation des programmes inconnus (machine a etats pure, comme `Detector`).
#[derive(Default, Debug)]
pub struct UnknownWatch {
    baseline: HashSet<(u32, u64)>,
    baseline_done: bool,
    /// pid -> (heure de demarrage, candidat).
    running: HashMap<u32, (u64, Candidate)>,
    /// Portee de la detection (un programme inconnu n'est jamais un logiciel a profil : hors portee « Logiciels »).
    pub scope: DetectScope,
}

impl UnknownWatch {
    pub fn running_count(&self) -> usize {
        self.running.values().map(|(_, c)| c.key.as_str()).collect::<HashSet<_>>().len()
    }

    /// Un passage. `windows` : processus qui ont une vraie fenetre visible ; `facts` : indices d'execution ; `skip` :
    /// processus deja pris par la detection ordinaire.
    pub fn scan(&mut self, procs: &[ProcInfo], env: &Env, mem: &Memory, windows: &HashSet<u32>, facts: &HashMap<(u32, u64), Facts>, skip: &dyn Fn(&ProcInfo) -> bool) -> Vec<UnknownEvent> {
        let mut out = Vec::new();
        if !self.baseline_done {
            self.baseline = procs.iter().map(|p| (p.pid, p.start)).collect();
            self.baseline_done = true;
            return out;
        }
        let alive: HashSet<(u32, u64)> = procs.iter().map(|p| (p.pid, p.start)).collect();
        for p in procs {
            if self.baseline.contains(&(p.pid, p.start)) || self.running.get(&p.pid).map(|r| r.0 == p.start).unwrap_or(false) || skip(p) {
                continue;
            }
            let f = facts.get(&(p.pid, p.start)).cloned().unwrap_or_default();
            let Some(c) = unknown_candidate(p, &f, windows.contains(&p.pid), env, mem) else { continue };
            if !in_scope(&c, self.scope) {
                continue;
            }
            let already = self.running.values().any(|(_, o)| o.key == c.key);
            self.running.insert(p.pid, (p.start, c.clone()));
            if !already {
                out.push(UnknownEvent::Started(c));
            }
        }
        let gone: Vec<u32> = self.running.iter().filter(|(pid, (st, _))| !alive.contains(&(**pid, *st))).map(|(p, _)| *p).collect();
        for pid in gone {
            if let Some((_, c)) = self.running.remove(&pid) {
                if !self.running.values().any(|(_, o)| o.key == c.key) {
                    out.push(UnknownEvent::Ended(c));
                }
            }
        }
        out
    }
}

/// Les sous-reglages de la carte « Detection » sont-ils interactifs ? Non quand la detection est desactivee : ils sont
/// grises (valeurs visibles et conservees, rien n'est reinitialise).
pub fn sub_options_interactive(detect_enabled: bool) -> bool {
    detect_enabled
}

/// Fragments de noms d'exe AUXILIAIRES (jamais le programme principal) : amorces, mises a jour, lanceurs, assistants, rapporteurs de
/// plantage, anti-triche, installeurs, redistribuables, services / agents, sous-processus CEF / navigateur integre.
const AUX_NAME_PARTS: &[&str] = &[
    "bootstrapper", "bootstrap", "updater", "update_", "launcher", "helper", "crashhandler", "crashreport", "crashpad", "crashsender", "bugsplat", "anticheat", "easyanticheat", "beservice",
    "battleye", "vgc.exe", "vgtray", "unins", "uninst", "installer", "setup", "install.exe", "redist", "vcredist", "vc_redist", "dotnet", "dxsetup", "oalinst", "service", "agent", "webhelper",
    "cefsubprocess", "unrealcef", "cef_", "eosbootstrapper", "eosoverlay", "epicwebhelper", "epicgamesupdater", "overlayhelper", "gamingservices", "gameservice", "nvidia", "msedgewebview",
    "errorreport", "uploader", "uplaywebcore",
];

/// Dossiers d'outils / de redistribuables / de lanceurs : rien de ce qui s'y execute n'est un jeu.
const AUX_DIRS: &[&str] = &[
    "/epic games/launcher/", "/epic online services/", "/common files/", "/_commonredist/", "/commonredist/", "/redist/", "/redistributables/", "/directx/", "/vcredist", "/easyanticheat", "/battleye/",
    "/windows kits/", "/microsoft shared/", "/__installer/", "/crashreport", "/eos_", "/steam/bin/", "/steam/steamapps/common/steamworks shared/", "/plug-ins/", "/plugins/",
];

/// Un nom d'exe (minuscules) est-il auxiliaire ?
pub fn is_auxiliary(exe: &str) -> bool {
    AUX_NAME_PARTS.iter().any(|p| exe.contains(p))
}

fn is_aux_dir(np: &str) -> bool {
    AUX_DIRS.iter().any(|d| np.contains(d))
}

/// Reglage « Animations des cartes » : Toujours (defaut, pour qu'elles se voient), Auto (suit Windows) ou Reduites (fondu).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardAnim {
    #[default]
    Always,
    Auto,
    Reduced,
}

/// (animations reduites ?, raison si l'animation est remplacee par un fondu). `windows_on` : les animations Windows sont activees.
pub fn reduce_decision(mode: CardAnim, windows_on: bool) -> (bool, Option<&'static str>) {
    match mode {
        CardAnim::Always => (false, None),
        CardAnim::Reduced => (true, Some("reglage « Animations des cartes » = reduites")),
        CardAnim::Auto if !windows_on => (true, Some(if cfg!(windows) { "les animations Windows sont desactivees (reglage « Auto »)" } else { "les animations du bureau sont desactivees (reglage « Auto »)" })),
        CardAnim::Auto => (false, None),
    }
}
