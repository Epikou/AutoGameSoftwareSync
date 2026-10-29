//! File locale pour la future base maison : quand un emplacement trouve par les couches 1 a 4 est ACCEPTE par
//! l'utilisateur, un enregistrement anonyme est ajoute a `<donnees>/knowledge-queue.jsonl` (une ligne JSON par
//! emplacement). Le nom d'utilisateur, les dossiers perso, les identifiants de compte et tout texte tape a la main sont
//! retires ; un emplacement qui n'est pas un gabarit generique n'est jamais ecrit. La file est envoyee au serveur par
//! `usage` SEULEMENT si les donnees d'usage sont acceptees.
use super::paths::{LX_BASES, WIN_BASES};
use super::{best_match, Found, Kind, Layer, Program};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = "knowledge-queue.jsonl";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Nom du programme (absent s'il a ete tape a la main).
    pub program: Option<String>,
    pub publisher: Option<String>,
    /// Nom de fichier de l'exe (jamais de chemin).
    pub exe: Option<String>,
    /// Gabarit generique (`%LOCALAPPDATA%/Studio/Jeu/Saved/SaveGames/*`).
    pub template: String,
    pub kind: Kind,
    pub layer: Layer,
    pub app_version: String,
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

/// Dossiers personnels : jamais envoyes, meme sous une base generique.
const PERSONAL: &[&str] = &["desktop", "bureau", "pictures", "images", "videos", "music", "musique", "downloads", "telechargements", "onedrive", "dropbox", "google drive", "icloud drive", "contacts", "favorites", "links", "searches"];

/// Composant propre a l'utilisateur (nom, identifiant, adresse) -> joker.
fn private_component(c: &str, users: &[String]) -> bool {
    let l = c.to_lowercase();
    let alnum: String = l.chars().filter(|x| x.is_alphanumeric()).collect();
    users.iter().any(|u| {
        let u = u.to_lowercase();
        u.len() >= 2 && (l == u || (u.len() >= 4 && l.contains(&u)))
    }) || l.contains('@')
        || (alnum.len() >= 5 && alnum.chars().all(|x| x.is_ascii_digit()))
        || (alnum.len() >= 16 && alnum.chars().all(|x| x.is_ascii_hexdigit()))
        || (l.len() >= 20 && l.chars().filter(|x| x.is_ascii_digit()).count() * 2 >= l.len())
}

fn clean_text(s: &str, users: &[String]) -> Option<String> {
    let t: String = s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if t.is_empty() || t.len() > 100 || t.contains(['\\', '/', '@']) {
        return None;
    }
    if users.iter().any(|u| u.len() >= 3 && t.to_lowercase().contains(&u.to_lowercase())) {
        return None;
    }
    Some(t)
}

/// Nettoyage obligatoire avant ecriture. None : rien ne doit partir (gabarit non generique, dossier perso...).
/// `names` : noms et editeurs connus du programme (un dossier appris ou lu en ligne sans aucun lien avec eux peut etre un
/// dossier perso : il n'est pas envoye).
pub fn sanitize(r: Record, users: &[String], names: &[String]) -> Option<Record> {
    let (ph, comps) = super::paths::split_template(&r.template)?;
    let ph_ok = WIN_BASES.iter().chain(LX_BASES.iter()).chain([super::paths::INSTALL, "%STEAMUSERDATA%"].iter()).find(|b| b.eq_ignore_ascii_case(&ph))?;
    if comps.is_empty() || comps.len() > 12 {
        return None;
    }
    let first = comps[0].to_lowercase();
    if PERSONAL.contains(&first.as_str()) || (*ph_ok == "%USERPROFILE%" || *ph_ok == "$HOME") && comps.len() >= 2 && PERSONAL.contains(&comps[1].to_lowercase().as_str()) {
        return None;
    }
    let comps: Vec<String> = comps.into_iter().map(|c| if c == "*" || private_component(&c, users) { "*".to_string() } else { c }).collect();
    if comps[0] == "*" || comps.iter().any(|c| c.contains(':') || c == "..") {
        return None;
    }
    if matches!(r.layer, Layer::Learn | Layer::Web) && *ph_ok != "%STEAMUSERDATA%" && !comps.iter().any(|c| best_match(c, names).is_some()) {
        return None;
    }
    if crate::sensitive::text_is_sensitive(&comps.join("/")).is_some() {
        return None;
    }
    let exe = r.exe.as_deref().map(crate::hints::exe_file_name).and_then(|e| clean_text(&e, users)).filter(|e| e.len() <= 80);
    Some(Record { program: r.program.as_deref().and_then(|s| clean_text(s, users)), publisher: r.publisher.as_deref().and_then(|s| clean_text(s, users)), exe, template: format!("{}/{}", ph_ok, comps.join("/")), ..r })
}

/// Enregistrement d'un emplacement accepte. `name_typed` : le nom a ete saisi a la main (alors il n'est pas envoye).
pub fn record_for(p: &Program, f: &Found, name_typed: bool) -> Option<Record> {
    let template = f.template.clone()?;
    let program = if name_typed { None } else { p.names().into_iter().next().or_else(|| Some(p.name.clone())).filter(|n| !n.trim().is_empty()) };
    let publisher = p.publisher.clone().or_else(|| p.developer.clone()).or_else(|| p.company.clone()).map(|c| super::clean_company(&c)).filter(|c| !c.is_empty());
    Some(Record { program, publisher, exe: Some(p.exe.clone()).filter(|e| !e.is_empty()), template, kind: f.kind, layer: f.layer, app_version: crate::update::current_version().to_string() })
}

/// Ajoute un enregistrement nettoye (sans doublon). Ok(false) : rien a ecrire.
pub fn append(data_dir: &Path, r: Record, users: &[String], names: &[String]) -> crate::Res<bool> {
    let Some(r) = sanitize(r, users, names) else { return Ok(false) };
    let line = serde_json::to_string(&r)?;
    let p = path(data_dir);
    let existing = std::fs::read_to_string(&p).unwrap_or_default();
    if existing.lines().any(|l| serde_json::from_str::<Record>(l).map(|o| o.template == r.template && o.exe == r.exe && o.kind == r.kind).unwrap_or(false)) {
        return Ok(false);
    }
    std::fs::create_dir_all(data_dir)?;
    use std::io::Write;
    let mut fh = std::fs::OpenOptions::new().create(true).append(true).open(&p)?;
    writeln!(fh, "{}", line)?;
    Ok(true)
}

/// Lecture de la file (lignes illisibles ignorees).
pub fn read_all(data_dir: &Path) -> Vec<Record> {
    std::fs::read_to_string(path(data_dir)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Ecrit dans la file les emplacements acceptes (ceux de `accepted` trouves par les couches 1 a 4).
pub fn accept(data_dir: &Path, p: &Program, accepted: &[Found], name_typed: bool) -> usize {
    let f = p.folders();
    let users = f.user_names();
    let mut names = p.names();
    names.extend(p.companies());
    let mut n = 0;
    for a in accepted {
        if let Some(r) = record_for(p, a, name_typed) {
            if append(data_dir, r, &users, &names).unwrap_or(false) {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(t: &str, layer: Layer) -> Record {
        Record { program: Some("Hollow Knight".into()), publisher: Some("Team Cherry".into()), exe: Some("hollow_knight.exe".into()), template: t.into(), kind: Kind::Save, layer, app_version: "1.4.0".into() }
    }
    fn users() -> Vec<String> {
        vec!["Joueur".to_string(), "epikou".to_string()]
    }
    fn names() -> Vec<String> {
        vec!["Hollow Knight".to_string(), "Team Cherry".to_string()]
    }

    #[test]
    fn nettoyage_des_donnees() {
        let ok = sanitize(rec("%LOCALLOW%/Team Cherry/Hollow Knight", Layer::Launcher), &users(), &names()).unwrap();
        assert_eq!(ok.template, "%LOCALLOW%/Team Cherry/Hollow Knight");
        // Nom d'utilisateur, identifiant Steam, identifiant Epic, adresse : jokers.
        let r = sanitize(rec("%LOCALAPPDATA%/Hollow Knight/Joueur/76561198012345678/5894eec90d3c4b9cac0accb22b5d2530/a@b.c/epikou-saves", Layer::Launcher), &users(), &names()).unwrap();
        assert_eq!(r.template, "%LOCALAPPDATA%/Hollow Knight/*/*/*/*/*");
        // Chemin non generique (lecteur, dossier quelconque) : rien.
        assert!(sanitize(rec("D:/Jeux/Hollow Knight", Layer::Launcher), &users(), &names()).is_none());
        assert!(sanitize(rec("C:\\Users\\Joueur\\AppData\\x", Layer::Launcher), &users(), &names()).is_none());
        // Dossiers perso : rien.
        assert!(sanitize(rec("%USERPROFILE%/Desktop/Hollow Knight", Layer::Launcher), &users(), &names()).is_none());
        assert!(sanitize(rec("$HOME/Downloads/Hollow Knight", Layer::Learn), &users(), &names()).is_none());
        // Premier composant = utilisateur : rien d'utile.
        assert!(sanitize(rec("%DOCUMENTS%/Joueur/Hollow Knight", Layer::Launcher), &users(), &names()).is_none());
        // Appris ou lu en ligne sans lien avec le programme : peut etre un dossier perso, pas envoye.
        assert!(sanitize(rec("%DOCUMENTS%/Mes Romans", Layer::Learn), &users(), &names()).is_none());
        assert!(sanitize(rec("%DOCUMENTS%/Mes Romans", Layer::Web), &users(), &names()).is_none());
        assert!(sanitize(rec("%LOCALLOW%/Team Cherry/Hollow Knight", Layer::Learn), &users(), &names()).is_some());
        // Texte du nom : jamais de chemin ni de nom d'utilisateur.
        let mut x = rec("%APPDATA%/Hollow Knight", Layer::Deduce);
        x.program = Some("Partie de Joueur".into());
        x.exe = Some("C:\\Users\\Joueur\\Jeux\\hk.exe".into());
        let s = sanitize(x, &users(), &names()).unwrap();
        assert_eq!(s.program, None);
        assert_eq!(s.exe.as_deref(), Some("hk.exe"));
        // Liste noire dure.
        assert!(sanitize(rec("%APPDATA%/Hollow Knight/.ssh", Layer::Launcher), &users(), &names()).is_none());
    }

    #[test]
    fn file_locale_sans_doublon_et_sans_texte_tape() {
        let t = tempfile::tempdir().unwrap();
        let p = Program { key: "hollow_knight.exe".into(), exe: "hollow_knight.exe".into(), name: "Hollow Knight".into(), company: Some("Team Cherry Pty Ltd".into()), ..Default::default() };
        let f = Found { path: PathBuf::from("x"), template: Some("%LOCALLOW%/Team Cherry/Hollow Knight".into()), kind: Kind::Save, layer: Layer::Deduce, confidence: 80, source: String::new() };
        assert_eq!(accept(t.path(), &p, std::slice::from_ref(&f), false), 1);
        assert_eq!(accept(t.path(), &p, std::slice::from_ref(&f), false), 0, "pas de doublon");
        let v = read_all(t.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].program.as_deref(), Some("Hollow Knight"));
        assert_eq!(v[0].publisher.as_deref(), Some("Team Cherry"));
        assert_eq!(v[0].layer, Layer::Deduce);
        assert!(!v[0].app_version.is_empty());
        // Nom tape a la main : absent ; emplacement sans gabarit : rien.
        let g = Found { template: Some("%APPDATA%/Hollow Knight/Config".into()), kind: Kind::Config, ..f.clone() };
        assert_eq!(accept(t.path(), &p, &[g], true), 1);
        assert_eq!(read_all(t.path())[1].program, None);
        let h = Found { template: None, ..f };
        assert_eq!(accept(t.path(), &p, &[h], false), 0);
        // Le fichier ne contient jamais de chemin absolu.
        let raw = std::fs::read_to_string(path(t.path())).unwrap();
        assert!(!raw.contains(":\\") && !raw.contains("Users"), "{}", raw);
    }
}
