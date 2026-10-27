//! config.json (liste des jeux) et state.json (derniere synchro connue par jeu).
use crate::{util, Ctx, Res};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Game {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub exe: Option<String>,
    /// Alias local : dossier de destination utilise par CE PC (action « Lier a un jeu existant »).
    /// None = cle normalisee du nom (voir `cloud::game_key`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_key: Option<String>,
    /// Nom AFFICHE seulement (More > renommer). Ne change jamais la cle du dossier partage, l exe ni les chemins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// La cible de sauvegarde est un FICHIER unique (ex. memcard1.mcd) et non un dossier.
    #[serde(default, skip_serializing_if = "is_false")]
    pub save_is_file: bool,
    /// Filtre optionnel de noms de fichiers du dossier (motifs `*.srm`, `Jeu.srm`, separes par `;`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_filter: Option<String>,
    /// Motifs EXCLUS (separes par `;`) : nom de fichier ou de dossier (n importe quel niveau), ou chemin relatif.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_exclude: Option<String>,
    /// Emplacements SUPPLEMENTAIRES (facultatif). Le couple `path` / `save_is_file` / `save_filter` ci-dessus est l'emplacement
    /// « principal » (id `main`) : un jeu a un seul emplacement garde exactement l'ancien format, aucune migration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_locations: Vec<Location>,
    /// Nom de l'emplacement principal (facultatif).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_label: Option<String>,
    /// L'emplacement principal est desactive (exclu de la synchro, rien n'est supprime).
    #[serde(default, skip_serializing_if = "is_false")]
    pub primary_disabled: bool,
    /// Planification (heures fixes chaque jour ou intervalle) : obligatoire seulement pour un jeu SANS exe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<crate::schedule::Schedule>,
}

/// Identifiant de l'emplacement principal.
pub const MAIN_LOCATION: &str = "main";

fn yes() -> bool {
    true
}

/// Un emplacement de sauvegarde : dossier, dossier filtre ou fichier unique.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Location {
    /// Identifiant STABLE (identique sur tous les PC : sert de sous-espace `files/@<id>/` dans la sauvegarde).
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    #[serde(default)]
    pub is_file: bool,
    /// Motifs de noms separes par `;`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Motifs exclus (`;`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Location {
    /// Nouvel identifiant (6 caracteres hexadecimaux) : stable une fois cree, jamais reutilise.
    pub fn new_id(label: &str) -> String {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        util::hash_bytes(format!("{}|{}", label, n).as_bytes()).chars().take(6).collect()
    }

    pub fn exists(&self) -> bool {
        if self.is_file {
            self.path.is_file()
        } else {
            self.path.is_dir()
        }
    }
}


fn is_false(b: &bool) -> bool {
    !*b
}

impl Game {
    /// Nom affiche partout dans l interface (nom d origine si aucun nom personnalise).
    pub fn shown(&self) -> &str {
        self.display_name.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&self.name)
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    #[serde(default)]
    pub games: Vec<Game>,
    /// Destination de synchro par defaut (dossier Google Drive), utilisee par `watch`.
    #[serde(default)]
    pub dest: Option<PathBuf>,
}

impl Config {
    fn file(ctx: &Ctx) -> PathBuf {
        ctx.data_dir.join("config.json")
    }
    pub fn load(ctx: &Ctx) -> Res<Config> {
        let f = Self::file(ctx);
        if !f.exists() {
            return Ok(Config::default());
        }
        let s = std::fs::read_to_string(&f)?;
        serde_json::from_str(&s).map_err(|e| format!("config.json illisible ({}): {}", f.display(), e).into())
    }
    pub fn save(&self, ctx: &Ctx) -> Res<()> {
        util::write_atomic(&Self::file(ctx), serde_json::to_string_pretty(self)?.as_bytes())
    }
    pub fn find(&self, name: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.name.eq_ignore_ascii_case(name))
    }
}

/// jeu -> hash global du dernier etat synchronise (local == cloud) connu sur CETTE machine.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct State {
    #[serde(default)]
    pub last_synced: BTreeMap<String, String>,
    /// jeu -> identifiant (meta.id) de la derniere version synchronisee (parent de la prochaine sauvegarde).
    #[serde(default)]
    pub last_id: BTreeMap<String, String>,
    /// jeu -> date (ms UTC) de la derniere synchro reussie sur ce PC (fichier d'etat `status.json`).
    #[serde(default)]
    pub last_sync_ms: BTreeMap<String, u64>,
    /// jeu -> date (ms UTC) de la derniere sauvegarde ecrite / restauration faite par ce PC.
    #[serde(default)]
    pub last_backup_ms: BTreeMap<String, u64>,
    #[serde(default)]
    pub last_restore_ms: BTreeMap<String, u64>,
    /// jeu -> (nature : backup | restore | conflict_resolved, date ms UTC) de la derniere action.
    #[serde(default)]
    pub last_action: BTreeMap<String, (String, u64)>,
    /// jeu -> emplacements de la derniere restauration absents de ce PC (id, nom).
    #[serde(default)]
    pub unknown_locs: BTreeMap<String, Vec<(String, String)>>,
}

impl State {
    pub fn note_action(&mut self, game: &str, kind: &str) {
        let now = util::now_ms();
        match kind {
            "backup" => {
                self.last_backup_ms.insert(game.to_string(), now);
            }
            "restore" => {
                self.last_restore_ms.insert(game.to_string(), now);
            }
            _ => {}
        }
        self.last_action.insert(game.to_string(), (kind.to_string(), now));
    }
}

impl State {
    fn file(ctx: &Ctx) -> PathBuf {
        ctx.data_dir.join("state.json")
    }
    pub fn load(ctx: &Ctx) -> Res<State> {
        let f = Self::file(ctx);
        if !f.exists() {
            return Ok(State::default());
        }
        let s = std::fs::read_to_string(&f)?;
        serde_json::from_str(&s).map_err(|e| format!("state.json illisible: {}", e).into())
    }
    pub fn save(&self, ctx: &Ctx) -> Res<()> {
        util::write_atomic(&Self::file(ctx), serde_json::to_string_pretty(self)?.as_bytes())
    }
}

impl Game {
    /// Tous les emplacements (principal d'abord), actives ou non.
    pub fn all_locations(&self) -> Vec<Location> {
        let mut v = vec![Location {
            id: MAIN_LOCATION.to_string(),
            label: self.primary_label.clone().unwrap_or_default(),
            path: self.path.clone(),
            is_file: self.save_is_file,
            filter: self.save_filter.clone(),
            exclude: self.save_exclude.clone(),
            enabled: !self.primary_disabled,
        }];
        v.extend(self.extra_locations.iter().cloned());
        v
    }

    /// Emplacements ACTIVES.
    pub fn enabled_locations(&self) -> Vec<Location> {
        self.all_locations().into_iter().filter(|l| l.enabled).collect()
    }

    /// Jeu « mono-emplacement » equivalent a un emplacement (recouvrement, apercu...).
    pub fn pseudo_for(&self, l: &Location) -> Game {
        Game { name: self.name.clone(), path: l.path.clone(), exe: self.exe.clone(), save_is_file: l.is_file, save_filter: l.filter.clone(), save_exclude: l.exclude.clone(), ..Default::default() }
    }

    /// Plusieurs emplacements actives ? (sinon : format de sauvegarde historique).
    pub fn is_multi(&self) -> bool {
        self.enabled_locations().iter().filter(|l| l.id != MAIN_LOCATION).count() > 0
    }
}
