//! Couche 2 : deduction locale. A partir des infos de version de l'exe (CompanyName, ProductName), du nom du dossier
//! d'installation et des noms donnes par les lanceurs, cherche les dossiers correspondants dans %LOCALAPPDATA%,
//! %APPDATA%, LocalLow (Unity : <Studio>\<Produit>), Documents, Documents\My Games, Saved Games (Linux : dossiers XDG,
//! dossier personnel) et la cle HKCU\Software\<Studio>\<Produit>. Correspondance floue mais prudente ; seuls les dossiers
//! modifies recemment sont gardes.
use super::paths::{self, Folders};
use super::{best_match, kind_for, Found, Layer, Program, RECENT_DAYS};
use std::path::{Path, PathBuf};

/// Lecture du registre de l'utilisateur (injectable pour les tests).
pub trait Registry {
    /// Sous-cles de `HKCU\<chemin>`.
    fn subkeys(&self, path: &str) -> Vec<String>;
}

pub struct RealRegistry;

impl Registry for RealRegistry {
    #[cfg(windows)]
    fn subkeys(&self, path: &str) -> Vec<String> {
        crate::registry::subkeys(crate::registry::Hive::CurrentUser, path)
    }
    #[cfg(not(windows))]
    fn subkeys(&self, _path: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Aucun registre (tests, Linux).
pub struct NoRegistry;

impl Registry for NoRegistry {
    fn subkeys(&self, _path: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Dossiers qui ne sont jamais ceux d'un programme precis (systeme, caches, lanceurs, navigateurs).
pub const NOISE_TOP: &[&str] = &[
    "microsoft", "packages", "temp", "tmp", "programs", "crashdumps", "d3dscache", "nvidia", "nvidia corporation", "amd", "ati", "intel", "google", "mozilla", "bravesoftware", "opera software", "vivaldi",
    "steam", "epicgameslauncher", "epic", "gog.com", "galaxyclient", "discord", "spotify", "autogamesoftwaresync", "onedrive", "dropbox", "connecteddevicesplatform", "comms", "unrealengine", "unity",
    "pip", "npm", "npm-cache", "nuget", "jetbrains", "squirreltemp", "my games", "windowsapps", "virtualstore", "history", "publishers", "assembly", "cache", "caches", "logs", "fontcache", "elevateddiagnostics",
    "peerdistrepub", "placeholdertilelogofolder", "cef", "electron", "ms-playwright", "gtk-3.0", "fontconfig", "pulse", "dconf", "recently-used.xbel", "trash", "flatpak", "applications", "icons", "mime",
    "systemd", "keyrings", "gnome-shell", "kde", "kwalletd", "baloo", "akonadi", "thumbnails", "wine", "lutris", "heroic", "bottles", "proton", "zoom", "teams", "slack", "whatsapp", "telegram desktop",
];

fn noise_name(n: &str) -> bool {
    let l = n.trim_start_matches('.').to_lowercase();
    l.is_empty() || NOISE_TOP.contains(&l.as_str()) || crate::sensitive::text_is_sensitive(&l).is_some()
}

fn subdirs(d: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(d) else { return vec![] };
    rd.filter_map(|e| e.ok()).filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| e.path()).take(3000).collect()
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn related(a: &Path, b: &Path) -> bool {
    let (x, y) = (crate::gamedetect::norm_path(a), crate::gamedetect::norm_path(b));
    x == y || x.starts_with(&format!("{}/", y)) || y.starts_with(&format!("{}/", x))
}

/// Emplacements deduits, verifies (existent et modifies recemment), du plus probable au moins probable.
pub fn find(p: &Program, f: &Folders, now_ms: u64, reg: &dyn Registry) -> Vec<Found> {
    let names = p.names();
    if names.is_empty() {
        return vec![];
    }
    let companies = p.companies();
    // (dossier, correspondance du nom 70-100, bonus editeur / registre)
    let mut cands: Vec<(PathBuf, u8, u8, &'static str)> = Vec::new();
    let push = |c: &mut Vec<(PathBuf, u8, u8, &'static str)>, d: PathBuf, s: u8, bonus: u8, why: &'static str| {
        if let Some(e) = c.iter_mut().find(|x| x.0 == d) {
            if s + bonus > e.1 + e.2 {
                e.1 = s;
                e.2 = bonus;
                e.3 = why;
            }
        } else {
            c.push((d, s, bonus, why));
        }
    };
    // Registre : HKCU\Software\<Studio>\<Produit> confirme le couple (Unity y range ses preferences).
    let mut reg_pairs: Vec<(String, String)> = Vec::new();
    for vendor in reg.subkeys("Software") {
        let vl = vendor.to_lowercase();
        if ["microsoft", "classes", "policies", "wow6432node", "clients", "google", "mozilla", "intel", "nvidia corporation", "amd", "valve", "epic games", "gog.com"].contains(&vl.as_str()) {
            continue;
        }
        let vendor_ok = best_match(&vendor, &companies).map(|s| s >= 85).unwrap_or(false);
        for prod in reg.subkeys(&format!("Software\\{}", vendor)) {
            if best_match(&prod, &names).filter(|s| *s >= 85 || (vendor_ok && *s >= 70)).is_some() {
                reg_pairs.push((vendor.clone(), prod));
            }
        }
    }
    let mut bases: Vec<(PathBuf, bool)> = Vec::new(); // (dossier, deux niveaux toujours examines)
    for (ph, b) in f.bases() {
        match ph {
            "%LOCALLOW%" => bases.push((b.clone(), true)),
            "%LOCALAPPDATA%" | "%APPDATA%" | "%SAVEDGAMES%" | "$XDG_DATA_HOME" => bases.push((b.clone(), false)),
            "%DOCUMENTS%" => {
                bases.push((b.clone(), false));
                bases.push((b.join("My Games"), false));
            }
            "$XDG_CONFIG_HOME" => {
                bases.push((b.clone(), false));
                bases.push((b.join("unity3d"), true));
            }
            "$HOME" => {
                bases.push((b.clone(), false));
                bases.push((b.join("Documents"), false));
                bases.push((b.join(".local").join("share").join("godot").join("app_userdata"), false));
            }
            _ => {}
        }
    }
    for (base, two_levels) in &bases {
        for d in subdirs(base) {
            let dn = name_of(&d);
            if noise_name(&dn) {
                continue;
            }
            if let Some(s) = best_match(dn.trim_start_matches('.'), &names) {
                push(&mut cands, d.clone(), s, 0, "nom du dossier");
                continue;
            }
            let company_ok = best_match(&dn, &companies).map(|s| s >= 85).unwrap_or(false) || reg_pairs.iter().any(|(v, _)| v.eq_ignore_ascii_case(&dn));
            if company_ok || *two_levels {
                for e in subdirs(&d) {
                    let en = name_of(&e);
                    if noise_name(&en) {
                        continue;
                    }
                    if let Some(s) = best_match(&en, &names) {
                        let bonus = if company_ok { 10 } else { 0 };
                        push(&mut cands, e, s, bonus, if company_ok { "editeur et nom du dossier" } else { "nom du dossier (studio)" });
                    }
                }
            }
        }
    }
    // Couples confirmes par le registre : dossiers attendus.
    for (v, pr) in &reg_pairs {
        for (ph, b) in f.bases() {
            if ["%LOCALLOW%", "%LOCALAPPDATA%", "%APPDATA%"].contains(&ph) {
                let d = b.join(v).join(pr);
                if d.is_dir() {
                    push(&mut cands, d, 90, 10, "editeur et produit confirmes par le registre");
                }
            }
        }
    }
    let install = p.install_dir.clone().or_else(|| p.exe_path.as_deref().and_then(Path::parent).map(Path::to_path_buf));
    let mut out: Vec<Found> = Vec::new();
    for (d, s, bonus, why) in cands {
        if install.as_deref().map(|i| related(&d, i)).unwrap_or(false) {
            continue; // le dossier d'installation n'est pas un emplacement de sauvegarde
        }
        let Some(newest) = paths::newest_file_ms(&d) else { continue };
        if now_ms.saturating_sub(newest) > RECENT_DAYS * 86_400_000 {
            continue;
        }
        let mut conf = 60 + (s.saturating_sub(70)) / 2 + bonus;
        if now_ms.saturating_sub(newest) <= 2 * 86_400_000 {
            conf += 5;
        }
        let template = paths::to_generic(&d, f);
        let text = template.clone().unwrap_or_else(|| d.to_string_lossy().to_string());
        out.push(Found { kind: kind_for(&text, p.app), template, path: d, layer: Layer::Deduce, confidence: conf.min(90), source: format!("deduction : {}", why) });
    }
    out.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeReg(Vec<(&'static str, Vec<&'static str>)>);
    impl Registry for FakeReg {
        fn subkeys(&self, path: &str) -> Vec<String> {
            self.0.iter().find(|(p, _)| p.eq_ignore_ascii_case(path)).map(|(_, v)| v.iter().map(|s| s.to_string()).collect()).unwrap_or_default()
        }
    }

    fn touch(d: &Path) {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("f.dat"), b"x").unwrap();
    }

    #[test]
    fn deduction_par_nom_editeur_et_registre() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        let now = crate::util::now_ms();
        let roaming = f.roaming.clone().unwrap();
        let ll = f.locallow.clone().unwrap();
        touch(&roaming.join("StardewValley"));
        touch(&roaming.join("Stardew Valley Mods Helper Tool")); // nom different : jamais
        touch(&ll.join("Team Cherry").join("Hollow Knight"));
        touch(&f.documents.clone().unwrap().join("My Games").join("Terraria"));
        touch(&roaming.join("Microsoft").join("Stardew Valley")); // dossier systeme : jamais
        let p = Program { exe: "Stardew Valley.exe".into(), name: "Stardew Valley".into(), ..Default::default() };
        let v = find(&p, &f, now, &NoRegistry);
        assert_eq!(v.iter().map(|x| x.path.clone()).collect::<Vec<_>>(), vec![roaming.join("StardewValley")]);
        assert_eq!(v[0].template.as_deref(), Some("%APPDATA%/StardewValley"));
        assert_eq!(v[0].layer, Layer::Deduce);
        // Unity : LocalLow\<Studio>\<Produit>, editeur des infos de version.
        let hk = Program { exe: "hollow_knight.exe".into(), name: "hollow_knight".into(), company: Some("Team Cherry".into()), ..Default::default() };
        let v = find(&hk, &f, now, &NoRegistry);
        assert_eq!(v[0].path, ll.join("Team Cherry").join("Hollow Knight"));
        assert!(v[0].confidence >= 80, "{}", v[0].confidence);
        // Sans editeur, LocalLow est quand meme examine sur deux niveaux.
        let hk2 = Program { exe: "hollow_knight.exe".into(), name: "Hollow Knight".into(), ..Default::default() };
        assert_eq!(find(&hk2, &f, now, &NoRegistry)[0].path, ll.join("Team Cherry").join("Hollow Knight"));
        // Documents\My Games.
        let tr = Program { exe: "Terraria.exe".into(), name: "Terraria".into(), ..Default::default() };
        assert_eq!(find(&tr, &f, now, &NoRegistry)[0].path, f.documents.clone().unwrap().join("My Games").join("Terraria"));
        // Registre : studio au nom inattendu confirme par HKCU\Software\<Studio>\<Produit>.
        touch(&ll.join("RedHook").join("Darkest Dungeon II"));
        let reg = FakeReg(vec![("Software", vec!["RedHook", "Microsoft"]), ("Software\\RedHook", vec!["Darkest Dungeon II"])]);
        let dd = Program { exe: "Darkest Dungeon II.exe".into(), name: "Darkest Dungeon II".into(), ..Default::default() };
        let v = find(&dd, &f, now, &reg);
        assert_eq!(v[0].path, ll.join("RedHook").join("Darkest Dungeon II"));
        assert!(v[0].confidence >= 85);
    }

    #[test]
    fn dossiers_anciens_ou_d_installation_ignores() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        let local = f.local.clone().unwrap();
        touch(&local.join("Obsidian"));
        let p = Program { exe: "Obsidian.exe".into(), name: "Obsidian".into(), app: true, ..Default::default() };
        // Fichier modifie il y a plus de RECENT_DAYS jours : la date « maintenant » est decalee dans le futur.
        let later = crate::util::now_ms() + (RECENT_DAYS + 5) * 86_400_000;
        assert!(find(&p, &f, later, &NoRegistry).is_empty());
        // Dossier d'installation (%LOCALAPPDATA%\Obsidian contient l'exe) : jamais propose.
        let inst = Program { exe_path: Some(local.join("Obsidian").join("Obsidian.exe")), ..p.clone() };
        assert!(find(&inst, &f, crate::util::now_ms(), &NoRegistry).is_empty());
        let v = find(&p, &f, crate::util::now_ms(), &NoRegistry);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, super::super::Kind::Config, "logiciel : reglages");
    }

    #[test]
    fn noms_trop_vagues_sans_resultat() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        touch(&f.local.clone().unwrap().join("Game"));
        let p = Program { exe: "game.exe".into(), name: "game".into(), ..Default::default() };
        assert!(find(&p, &f, crate::util::now_ms(), &NoRegistry).is_empty());
    }
}
