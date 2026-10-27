//! Cache local de la connaissance intelligente (`<donnees>/knowledge/`) : resultats d'apprentissage et de recherche par
//! programme, liste des programmes a apprendre (ajoutes sans emplacement). Ecritures atomiques ; un fichier illisible
//! repart a vide (jamais d'erreur bloquante).
use super::Found;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("knowledge")
}

fn load_json<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> T {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_json<T: Serialize>(p: &Path, v: &T) -> crate::Res<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    crate::util::write_atomic(p, serde_json::to_string_pretty(v)?.as_bytes())
}

/// Resultat de recherche web d'un programme.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct WebEntry {
    /// Derniere tentative (ms).
    pub at: u64,
    /// Nombre de tentatives.
    #[serde(default)]
    pub tries: u32,
    /// Emplacements verifies sur ce PC.
    #[serde(default)]
    pub found: Vec<Found>,
    /// Gabarits lus (verifies ou non), pour un nouvel essai sans reseau.
    #[serde(default)]
    pub templates: Vec<String>,
    /// Moteur qui a repondu (journal).
    #[serde(default)]
    pub engine: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Entry {
    #[serde(default)]
    pub learned: Vec<Found>,
    #[serde(default)]
    pub learned_at: u64,
    #[serde(default)]
    pub web: Option<WebEntry>,
}

/// Delai avant un nouvel essai de recherche quand rien n'a ete trouve.
pub const RETRY_DAYS: u64 = 4;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Cache {
    #[serde(default)]
    pub programs: BTreeMap<String, Entry>,
}

impl Cache {
    pub fn path(data_dir: &Path) -> PathBuf {
        dir(data_dir).join("cache.json")
    }

    pub fn load(data_dir: &Path) -> Cache {
        load_json(&Self::path(data_dir))
    }

    pub fn save(&self, data_dir: &Path) -> crate::Res<()> {
        save_json(&Self::path(data_dir), self)
    }

    /// Emplacements deja appris ou trouves pour ce programme et qui existent toujours sur ce PC.
    pub fn known(&self, key: &str) -> Vec<Found> {
        let Some(e) = self.programs.get(key) else { return vec![] };
        let mut v: Vec<Found> = e.learned.iter().chain(e.web.as_ref().map(|w| w.found.iter()).into_iter().flatten()).filter(|x| x.path.exists()).cloned().collect();
        v.sort_by(|a, b| b.confidence.cmp(&a.confidence));
        v
    }

    pub fn set_learned(&mut self, key: &str, v: Vec<Found>, now_ms: u64) {
        let e = self.programs.entry(key.to_string()).or_default();
        e.learned = v;
        e.learned_at = now_ms;
    }

    /// Une recherche web est-elle permise pour ce programme ? Jamais deux fois s'il y a un resultat ; sinon nouvel essai
    /// apres `RETRY_DAYS` jours.
    pub fn web_due(&self, key: &str, now_ms: u64) -> bool {
        match self.programs.get(key).and_then(|e| e.web.as_ref()) {
            None => true,
            Some(w) if !w.found.is_empty() => false,
            Some(w) => now_ms.saturating_sub(w.at) >= RETRY_DAYS * 86_400_000,
        }
    }

    pub fn set_web(&mut self, key: &str, mut w: WebEntry) {
        let e = self.programs.entry(key.to_string()).or_default();
        w.tries = e.web.as_ref().map(|o| o.tries).unwrap_or(0) + 1;
        e.web = Some(w);
    }
}

/// Programme ajoute sans emplacement : ses dossiers seront appris a sa prochaine utilisation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LearnItem {
    /// Nom de l'exe (minuscules).
    pub exe: String,
    pub name: String,
    #[serde(default)]
    pub path: Option<PathBuf>,
    pub added: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LearnList {
    #[serde(default)]
    pub items: Vec<LearnItem>,
}

impl LearnList {
    pub fn path(data_dir: &Path) -> PathBuf {
        dir(data_dir).join("learn.json")
    }

    pub fn load(data_dir: &Path) -> LearnList {
        load_json(&Self::path(data_dir))
    }

    pub fn save(&self, data_dir: &Path) -> crate::Res<()> {
        save_json(&Self::path(data_dir), self)
    }

    pub fn add(&mut self, exe: &str, name: &str, path: Option<PathBuf>, now_ms: u64) {
        let k = super::Program::key_of(exe);
        if k.is_empty() {
            return;
        }
        self.items.retain(|i| i.exe != k);
        self.items.push(LearnItem { exe: k, name: name.to_string(), path, added: now_ms });
        // Liste courte : les plus anciens partent.
        if self.items.len() > 50 {
            self.items.remove(0);
        }
    }

    pub fn remove(&mut self, exe: &str) -> bool {
        let k = super::Program::key_of(exe);
        let n = self.items.len();
        self.items.retain(|i| i.exe != k);
        n != self.items.len()
    }

    pub fn contains(&self, exe: &str) -> bool {
        let k = super::Program::key_of(exe);
        self.items.iter().any(|i| i.exe == k)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Layer};
    use super::*;

    #[test]
    fn cache_et_delai_de_nouvel_essai() {
        let t = tempfile::tempdir().unwrap();
        let mut c = Cache::load(t.path());
        assert!(c.web_due("jeu.exe", 0));
        c.set_web("jeu.exe", WebEntry { at: 1000, ..Default::default() });
        assert!(!c.web_due("jeu.exe", 1000 + 86_400_000));
        assert!(c.web_due("jeu.exe", 1000 + RETRY_DAYS * 86_400_000));
        let d = t.path().join("Saves");
        std::fs::create_dir_all(&d).unwrap();
        let f = Found { path: d.clone(), template: Some("%APPDATA%/Jeu".into()), kind: Kind::Save, layer: Layer::Web, confidence: 70, source: String::new() };
        c.set_web("jeu.exe", WebEntry { at: 2000, found: vec![f.clone()], ..Default::default() });
        assert_eq!(c.programs["jeu.exe"].web.as_ref().unwrap().tries, 2);
        assert!(!c.web_due("jeu.exe", u64::MAX / 2), "jamais deux fois une recherche qui a trouve");
        c.set_learned("autre.exe", vec![Found { path: t.path().join("absent"), layer: Layer::Learn, ..f.clone() }], 5);
        c.save(t.path()).unwrap();
        let c2 = Cache::load(t.path());
        assert_eq!(c2, c);
        assert_eq!(c2.known("jeu.exe"), vec![f]);
        assert!(c2.known("autre.exe").is_empty(), "dossier disparu : ignore");
        // Fichier corrompu : repart a vide.
        std::fs::write(Cache::path(t.path()), b"{oops").unwrap();
        assert_eq!(Cache::load(t.path()), Cache::default());
    }

    #[test]
    fn liste_d_apprentissage() {
        let t = tempfile::tempdir().unwrap();
        let mut l = LearnList::load(t.path());
        l.add("C:\\Jeux\\Zorg.exe", "Zorg", None, 1);
        l.add("zorg.EXE", "Zorg 2", None, 2);
        assert_eq!(l.items.len(), 1);
        assert!(l.contains("ZORG.exe"));
        l.save(t.path()).unwrap();
        let mut l2 = LearnList::load(t.path());
        assert_eq!(l2.items[0].name, "Zorg 2");
        assert!(l2.remove("zorg.exe"));
        assert!(!l2.remove("zorg.exe"));
    }
}
