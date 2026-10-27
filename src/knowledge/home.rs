//! Base maison : emplacements generiques confirmes par plusieurs installations (5 au moins) ou valides par l'auteur,
//! telecharges une fois par jour depuis le serveur (`usage`) et mis en cache dans `<donnees>/knowledge/base.json`.
//! Utilisee juste apres la base embarquee et les profils, avant les couches 1 a 4. Chaque emplacement est re-verifie sur
//! CE PC (le dossier existe et contient au moins un fichier) avant d'etre propose.
use super::paths::{self, Folders, LX_BASES, WIN_BASES};
use super::{best_match, Found, Kind, Layer, Program};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = "base.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub exe: Option<String>,
    pub template: String,
    pub kind: Kind,
    /// Installations distinctes qui l'ont confirme.
    #[serde(default)]
    pub n: u32,
    /// Valide a la main par l'auteur.
    #[serde(default)]
    pub validated: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Base {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

pub fn path(data_dir: &Path) -> PathBuf {
    super::store::dir(data_dir).join(FILE)
}

fn text_ok(s: &Option<String>, max: usize) -> bool {
    s.as_deref().map(|t| !t.trim().is_empty() && t.len() <= max && !t.contains(['\\', '/', '@']) && !t.chars().any(char::is_control)).unwrap_or(true)
}

/// Entree acceptable : gabarit generique sous une base connue, textes courts sans chemin.
pub fn valid(e: &Entry) -> bool {
    let Some((ph, comps)) = paths::split_template(&e.template) else { return false };
    let base_ok = WIN_BASES.iter().chain(LX_BASES.iter()).chain([paths::INSTALL, "%STEAMUSERDATA%"].iter()).any(|b| b.eq_ignore_ascii_case(&ph));
    base_ok && !comps.is_empty() && comps.len() <= 12 && comps[0] != "*" && !e.template.contains('\\') && text_ok(&e.program, 100) && text_ok(&e.publisher, 100) && text_ok(&e.exe, 80) && (e.program.is_some() || e.exe.is_some()) && !crate::sensitive::text_is_sensitive(&comps.join("/")).is_some()
}

impl Base {
    /// Reponse du serveur (`{"v":1,"entries":[...]}`) : les entrees invalides sont ignorees.
    pub fn parse(json: &str) -> Option<Base> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let arr = v.get("entries")?.as_array()?;
        let entries: Vec<Entry> = arr.iter().filter_map(|e| serde_json::from_value::<Entry>(e.clone()).ok()).filter(valid).take(20_000).collect();
        Some(Base { entries })
    }

    pub fn load(data_dir: &Path) -> Base {
        std::fs::read_to_string(path(data_dir)).ok().and_then(|t| serde_json::from_str::<Base>(&t).ok()).map(|mut b| {
            b.entries.retain(valid);
            b
        }).unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> crate::Res<()> {
        std::fs::create_dir_all(super::store::dir(data_dir))?;
        crate::util::write_atomic(&path(data_dir), serde_json::to_string(self)?.as_bytes())
    }

    /// Entrees de ce programme : meme nom d'exe, ou nom du programme tres proche (85 ou plus).
    pub fn matches(&self, p: &Program) -> Vec<&Entry> {
        let exe = crate::hints::exe_file_name(&p.exe).to_lowercase();
        let names = p.names();
        self.entries
            .iter()
            .filter(|e| {
                let by_exe = !exe.is_empty() && e.exe.as_deref().map(|x| x.eq_ignore_ascii_case(&exe)).unwrap_or(false);
                let by_name = e.program.as_deref().and_then(|n| best_match(n, &names)).map(|s| s >= 85).unwrap_or(false);
                by_exe || by_name
            })
            .collect()
    }

    /// Titre connu d'une synchro (nom exact ou exe) : (titre, logiciel ?).
    pub fn title_for(&self, name: &str, exe: Option<&str>) -> Option<(String, bool)> {
        let exe = exe.map(crate::hints::exe_file_name).map(|e| e.to_lowercase()).filter(|e| !e.is_empty());
        let n = super::norm(name);
        self.entries.iter().find_map(|e| {
            let hit = exe.as_deref().map(|x| e.exe.as_deref().map(|y| y.eq_ignore_ascii_case(x)).unwrap_or(false)).unwrap_or(false) || e.program.as_deref().map(|p| n.len() >= 3 && super::norm(p) == n).unwrap_or(false);
            if hit {
                e.program.clone().map(|t| (t, e.kind == Kind::Config))
            } else {
                None
            }
        })
    }
}

/// Emplacements de la base maison pour ce programme, verifies sur ce PC.
pub fn find(p: &Program, f: &Folders, base: &Base) -> Vec<Found> {
    let mut out = Vec::new();
    for e in base.matches(p) {
        for path in paths::expand(&e.template, f, p.install_dir.as_deref()) {
            if paths::newest_file_ms(&path).is_none() {
                continue;
            }
            let template = paths::to_generic(&path, f).or_else(|| Some(e.template.clone()));
            out.push(Found { path, template, kind: e.kind, layer: Layer::Home, confidence: if e.validated { 90 } else { 85 }, source: format!("base maison ({} installation(s){})", e.n, if e.validated { ", validee" } else { "" }) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(t: &str) -> Entry {
        Entry { program: Some("Hollow Knight".into()), publisher: Some("Team Cherry".into()), exe: Some("hollow_knight.exe".into()), template: t.into(), kind: Kind::Save, n: 6, validated: false }
    }

    #[test]
    fn reponse_du_serveur_filtree() {
        let j = r#"{"v":1,"entries":[
            {"program":"Hollow Knight","publisher":"Team Cherry","exe":"hollow_knight.exe","template":"%LOCALLOW%/Team Cherry/Hollow Knight","kind":"save","n":7,"validated":false},
            {"program":"X","exe":"x.exe","template":"C:/Users/Joueur/x","kind":"save","n":9},
            {"program":"Y","exe":"y.exe","template":"%APPDATA%/Y/.ssh","kind":"config","n":9},
            {"program":"Z","template":"%APPDATA%/*","kind":"save","n":9},
            {"program":"W","template":"%APPDATA%/W","kind":"cookie","n":9}
        ]}"#;
        let b = Base::parse(j).unwrap();
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].n, 7);
        assert!(Base::parse("pas du json").is_none());
    }

    #[test]
    fn emplacement_verifie_sur_ce_pc() {
        let t = tempfile::tempdir().unwrap();
        let f = Folders::under(t.path());
        let save = f.locallow.clone().unwrap().join("Team Cherry").join("Hollow Knight");
        std::fs::create_dir_all(&save).unwrap();
        let base = Base { entries: vec![entry("%LOCALLOW%/Team Cherry/Hollow Knight"), entry("%APPDATA%/Absent")] };
        let p = Program { key: "hollow_knight.exe".into(), exe: "hollow_knight.exe".into(), name: "hollow_knight".into(), ..Default::default() };
        assert!(find(&p, &f, &base).is_empty(), "dossier vide : rien");
        std::fs::write(save.join("user1.dat"), b"x").unwrap();
        let v = find(&p, &f, &base);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].layer, Layer::Home);
        assert_eq!(v[0].path, save);
        // Autre programme : rien ; nom tres proche sans exe : trouve.
        let other = Program { exe: "other.exe".into(), name: "Other".into(), ..Default::default() };
        assert!(find(&other, &f, &base).is_empty());
        let by_name = Program { exe: "hk.exe".into(), name: "Hollow Knight".into(), ..Default::default() };
        assert_eq!(find(&by_name, &f, &base).len(), 1);
        assert_eq!(base.title_for("hollow knight", None), Some(("Hollow Knight".to_string(), false)));
        assert_eq!(base.title_for("Mes notes", Some("C:/x/HOLLOW_KNIGHT.exe")), Some(("Hollow Knight".to_string(), false)));
        assert_eq!(base.title_for("Mes notes", None), None);
        // Cache local.
        let dd = t.path().join("data");
        base.save(&dd).unwrap();
        assert_eq!(Base::load(&dd), base);
    }
}
