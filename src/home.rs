//! Accueil en tuiles (2.0, lot 6) : logique PURE et testee de l'ecran d'accueil (etat d'une tuile, etat de synchro entre
//! appareils, tri par priorite, resume global, filtres, navigation au clavier) + collecte des faits en arriere-plan et
//! marques de position des appareils dans le coffre.
//!
//! Marques de position : `<coffre>/.agss-devices/<appareil>/<cle>~<version>.json`, ecrites UNE fois (jamais renommees ni
//! reecrites), seulement par l'appareil concerne, quand son contenu local est identique a la version active. Elles disent
//! « cet appareil a cette version » meme apres une simple restauration (qui n'ecrit aucune version). Le dossier commence
//! par un point : il est ignore par la liste des elements du coffre et par toutes les versions precedentes.
use crate::config::{Config, Game};
use crate::gui::ui::State;
use crate::settings::Settings;
use crate::vm::GameStatus;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Dossier des marques de position dans le coffre.
pub const DEVICES_DIR: &str = ".agss-devices";

// ================================================================== categories et filtres

/// Categorie d'un element (filtres de l'accueil).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Game,
    App,
    Folder,
}

/// Filtre de l'accueil : Tout, Jeux, Logiciels, Dossiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Games,
    Apps,
    Folders,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Games, Filter::Apps, Filter::Folders];

    pub fn matches(self, k: Kind) -> bool {
        match self {
            Filter::All => true,
            Filter::Games => k == Kind::Game,
            Filter::Apps => k == Kind::App,
            Filter::Folders => k == Kind::Folder,
        }
    }
}

/// Compteurs des filtres (meme ordre que `Filter::ALL`).
pub fn filter_counts(kinds: &[Kind]) -> [usize; 4] {
    let mut c = [0; 4];
    for (i, f) in Filter::ALL.iter().enumerate() {
        c[i] = kinds.iter().filter(|k| f.matches(**k)).count();
    }
    c
}

/// Categorie d'un element : profil connu (logiciel / reglages = Logiciels ; jeu / emulateur = Jeux), puis base des jeux
/// (nom ou programme), puis sans programme = Dossier, sinon Jeu.
pub fn kind_of(game: &Game, profile_category: Option<&str>, in_gamedb: bool) -> Kind {
    match profile_category {
        Some("app") | Some("config") => return Kind::App,
        Some("game") | Some("emulator") => return Kind::Game,
        _ => {}
    }
    if in_gamedb {
        return Kind::Game;
    }
    if game.exe.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Kind::Folder;
    }
    Kind::Game
}

// ================================================================== faits d'un element

/// Version active du coffre (la plus recente des tetes).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Head {
    pub machine: String,
    pub id: String,
    pub hash: String,
    pub saved_ms: u64,
}

/// Tout ce que l'accueil sait d'un element (calcule hors de l'interface).
#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    /// Nom d'origine (cle de la config).
    pub name: String,
    /// Nom affiche.
    pub shown: String,
    pub kind: Kind,
    pub status: GameStatus,
    /// Mis en pause par l'utilisateur (menu « ... »).
    pub user_paused: bool,
    /// Derniere synchro reussie sur cet appareil (ms UTC).
    pub last_sync_ms: Option<u64>,
    pub head: Option<Head>,
    /// Versions du coffre : (appareil, identifiant, empreinte du contenu, date ms UTC).
    pub writers: Vec<(String, String, String, u64)>,
    /// Marques de position : (appareil, identifiant de version).
    pub marks: Vec<(String, String)>,
    pub versions: usize,
    /// Taille de la version active du coffre (octets) : alerte poids (pastille « Lourd »).
    pub bytes: u64,
    /// Emplacement principal (mode expert, infobulle).
    pub path: PathBuf,
}

impl Facts {
    /// Faits minimaux (element pas encore analyse) : etat seul.
    pub fn bare(g: &Game, kind: Kind, status: GameStatus) -> Facts {
        Facts { name: g.name.clone(), shown: g.shown().to_string(), kind, status, user_paused: false, last_sync_ms: None, head: None, writers: vec![], marks: vec![], versions: 0, bytes: 0, path: g.path.clone() }
    }
}

// ================================================================== etat d'une tuile

/// Pourquoi une tuile est dans son etat (texte court de la tuile).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// A l'abri, derniere copie (ms UTC) si connue.
    Safe(Option<u64>),
    /// Copie en cours (pourcentage 0..100 si connu).
    Copying(Option<u8>),
    /// Copie a faire (jamais copie, ou modifie depuis).
    Pending,
    /// Programme en cours d'utilisation : en pause jusqu'a sa fermeture.
    InUse,
    /// Mis en pause par l'utilisateur.
    PausedByUser,
    /// Coffre injoignable ou non choisi.
    WaitingVault,
    /// Fichier verrouille.
    Locked,
    /// Autre probleme (texte technique pour le mode expert).
    Problem(String),
    /// Deux appareils ont modifie en meme temps.
    Conflict,
    /// Version plus recente arrivee d'un autre appareil.
    NewerFrom(String),
}

/// Erreur d'acces a un fichier verrouille (un autre programme l'utilise).
pub fn is_lock_error(e: &str) -> bool {
    let l = e.to_lowercase();
    ["verrouill", "locked", "used by another process", "utilise par un autre", "utilisé par un autre", "sharing violation", "os error 32", "os error 33"].iter().any(|w| l.contains(w))
}

/// Etat d'une tuile. `busy` = copie en cours pour cet element (progression 0..1 si connue).
pub fn item_state(f: &Facts, busy: Option<Option<f32>>, reachable: bool, this_machine: &str) -> (State, Why) {
    if let Some(p) = busy {
        return (State::Busy, Why::Copying(p.map(|x| (x.clamp(0.0, 1.0) * 100.0).round() as u8)));
    }
    if matches!(f.status, GameStatus::Running) {
        return (State::Paused, Why::InUse);
    }
    if f.user_paused {
        return (State::Paused, Why::PausedByUser);
    }
    if !reachable || matches!(f.status, GameStatus::NoDest) {
        return (State::Paused, Why::WaitingVault);
    }
    match &f.status {
        GameStatus::UpToDate => (State::Safe, Why::Safe(f.last_sync_ms)),
        GameStatus::Never | GameStatus::LocalChanged => (State::Todo, Why::Pending),
        GameStatus::CloudNewer => {
            let m = f.head.as_ref().map(|h| h.machine.clone()).filter(|m| !m.eq_ignore_ascii_case(this_machine)).unwrap_or_default();
            (State::Incoming, Why::NewerFrom(m))
        }
        GameStatus::Conflict => (State::Problem, Why::Conflict),
        GameStatus::Error(e) if is_lock_error(e) => (State::Problem, Why::Locked),
        GameStatus::Error(e) => (State::Problem, Why::Problem(e.clone())),
        GameStatus::Running | GameStatus::NoDest => unreachable!(),
    }
}

// ================================================================== synchro entre appareils

/// Etat de la synchro entre appareils (deuxieme ligne de la tuile).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceSync {
    /// Meme version sur N appareils (N >= 2).
    UpToDateOn(usize),
    /// A jour ici ; ces appareils n'ont pas encore la derniere version.
    Waiting(Vec<String>),
    /// Version plus recente sur cet autre appareil.
    NewerOn(String),
    /// Version plus recente dans le coffre (ecrite par cet appareil, contenu local revenu en arriere).
    NewerInVault,
    /// Cet appareil a des modifications pas encore copiees.
    NewerHere,
    /// Aucun autre appareil ne l'utilise.
    OnlyHere,
    /// Deux appareils ont modifie en meme temps.
    Conflict,
    /// Rien de connu (coffre injoignable, pas encore analyse).
    Unknown,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Appareils qui ont la version active : celui qui l'a ecrite, ceux dont une marque la designe, ceux dont la derniere
/// version ecrite a le meme contenu, et cet appareil s'il est a jour. Tries, sans doublon (casse ignoree).
pub fn devices_at_head(f: &Facts, this_machine: &str) -> Vec<String> {
    let Some(h) = &f.head else { return vec![] };
    let mut out: Vec<String> = Vec::new();
    let mut push = |m: &str| {
        if !m.is_empty() && !out.iter().any(|x| same(x, m)) {
            out.push(m.to_string());
        }
    };
    push(&h.machine);
    for (m, id) in &f.marks {
        if *id == h.id {
            push(m);
        }
    }
    // Derniere version ecrite par chaque appareil : meme contenu que la tete = meme etat.
    let mut last: HashMap<String, (u64, &str, &str)> = HashMap::new();
    for (m, _, hash, ms) in &f.writers {
        let e = last.entry(m.to_lowercase()).or_insert((*ms, m.as_str(), hash.as_str()));
        if *ms >= e.0 {
            *e = (*ms, m.as_str(), hash.as_str());
        }
    }
    let mut same_content: Vec<&str> = last.values().filter(|(_, _, hash)| !h.hash.is_empty() && *hash == h.hash).map(|(_, m, _)| *m).collect();
    same_content.sort();
    for m in same_content {
        push(m);
    }
    if matches!(f.status, GameStatus::UpToDate) {
        push(this_machine);
    }
    out.sort_by_key(|s| s.to_lowercase());
    out
}

/// Tous les appareils connus pour cet element (versions + marques + cet appareil).
pub fn devices_known(f: &Facts, this_machine: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![this_machine.to_string()];
    for m in f.writers.iter().map(|w| &w.0).chain(f.marks.iter().map(|m| &m.0)) {
        if !m.is_empty() && !out.iter().any(|x| same(x, m)) {
            out.push(m.clone());
        }
    }
    out.sort_by_key(|s| s.to_lowercase());
    out
}

/// Etat de la synchro entre appareils, vu depuis cet appareil.
pub fn device_sync(f: &Facts, this_machine: &str, reachable: bool) -> DeviceSync {
    if !reachable || matches!(f.status, GameStatus::NoDest) {
        return DeviceSync::Unknown;
    }
    match &f.status {
        GameStatus::Conflict => return DeviceSync::Conflict,
        GameStatus::CloudNewer => {
            return match &f.head {
                Some(h) if !same(&h.machine, this_machine) => DeviceSync::NewerOn(h.machine.clone()),
                Some(_) => DeviceSync::NewerInVault,
                None => DeviceSync::Unknown,
            };
        }
        GameStatus::LocalChanged => return DeviceSync::NewerHere,
        GameStatus::Never if f.head.is_none() => return DeviceSync::OnlyHere,
        _ => {}
    }
    let Some(_) = &f.head else { return DeviceSync::Unknown };
    let at = devices_at_head(f, this_machine);
    let known = devices_known(f, this_machine);
    let here = at.iter().any(|m| same(m, this_machine));
    let others_behind: Vec<String> = known.iter().filter(|m| !same(m, this_machine) && !at.iter().any(|a| same(a, m))).cloned().collect();
    if !here {
        // Etat local inconnu (programme ouvert, erreur) : on dit seulement ce qui est sur pour les autres appareils.
        if matches!(f.status, GameStatus::Running | GameStatus::Error(_)) {
            return if at.len() >= 2 { DeviceSync::UpToDateOn(at.len()) } else { DeviceSync::Unknown };
        }
        return match &f.head {
            Some(h) if !same(&h.machine, this_machine) => DeviceSync::NewerOn(h.machine.clone()),
            _ => DeviceSync::Unknown,
        };
    }
    if at.len() >= 2 && others_behind.is_empty() {
        return DeviceSync::UpToDateOn(at.len());
    }
    if !others_behind.is_empty() {
        return DeviceSync::Waiting(others_behind);
    }
    DeviceSync::OnlyHere
}

// ================================================================== tri

/// Ligne triable de l'accueil.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub shown: String,
    pub state: State,
    /// Activite la plus recente (ms UTC) : derniere synchro ou derniere version du coffre.
    pub recent_ms: u64,
}

/// Tri de l'accueil : a verifier (probleme, a faire) > en cours > version recue > en pause > le reste ; a egalite, le plus
/// recemment modifie d'abord, puis l'ordre alphabetique du nom affiche.
pub fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        a.state
            .sort_rank()
            .cmp(&b.state.sort_rank())
            .then(b.recent_ms.cmp(&a.recent_ms))
            .then_with(|| a.shown.to_lowercase().cmp(&b.shown.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Activite la plus recente d'un element (tri).
pub fn recent_ms(f: &Facts) -> u64 {
    f.last_sync_ms.unwrap_or(0).max(f.head.as_ref().map(|h| h.saved_ms).unwrap_or(0))
}

// ================================================================== tri choisi et ordre fige (2.0.3)

/// Tri de l'accueil choisi par l'utilisateur (menu a gauche des filtres, memorise dans les reglages).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// Nom affiche, A-Z (accents et casse ignores).
    Name,
    /// Derniere synchro reussie sur cet appareil, la plus recente d'abord.
    LastSync,
    /// Derniere modification (version la plus recente du coffre), la plus recente d'abord.
    Modified,
    /// Taille de la version active du coffre, la plus grosse d'abord.
    Size,
    /// Etat : problemes d'abord (tri historique de l'accueil).
    #[default]
    State,
}

impl SortKey {
    pub const ALL: [SortKey; 5] = [SortKey::Name, SortKey::LastSync, SortKey::Modified, SortKey::Size, SortKey::State];
}

/// Ligne triable avec tout ce que les tris choisis utilisent.
#[derive(Clone, Debug, PartialEq)]
pub struct SortRow {
    pub name: String,
    pub shown: String,
    pub state: State,
    /// Activite la plus recente (tri par etat, comme avant).
    pub recent_ms: u64,
    pub last_sync_ms: u64,
    pub modified_ms: u64,
    pub bytes: u64,
    /// Vrai probleme (conflit, erreur) : seul changement d'etat qui recalcule l'ordre pendant que l'accueil est affiche.
    pub real_problem: bool,
}

/// Ordre (noms d'origine) selon le tri choisi. A egalite : nom affiche puis nom d'origine (toujours deterministe).
pub fn sorted_names(rows: &[SortRow], key: SortKey) -> Vec<String> {
    if key == SortKey::State {
        let mut r: Vec<Row> = rows.iter().map(|x| Row { name: x.name.clone(), shown: x.shown.clone(), state: x.state, recent_ms: x.recent_ms }).collect();
        sort_rows(&mut r);
        return r.into_iter().map(|x| x.name).collect();
    }
    let mut v: Vec<&SortRow> = rows.iter().collect();
    let by_name = |a: &&SortRow, b: &&SortRow| fold(&a.shown).cmp(&fold(&b.shown)).then_with(|| a.name.cmp(&b.name));
    v.sort_by(|a, b| {
        let o = match key {
            SortKey::LastSync => b.last_sync_ms.cmp(&a.last_sync_ms),
            SortKey::Modified => b.modified_ms.cmp(&a.modified_ms),
            SortKey::Size => b.bytes.cmp(&a.bytes),
            SortKey::Name | SortKey::State => std::cmp::Ordering::Equal,
        };
        o.then_with(|| by_name(a, b))
    });
    v.into_iter().map(|x| x.name.clone()).collect()
}

/// Ordre fige des tuiles pendant que l'accueil est affiche : une copie qui commence ou finit ne deplace plus aucune tuile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrozenOrder {
    order: Vec<String>,
    key: Option<SortKey>,
    epoch: u64,
    problems: Vec<String>,
}

impl FrozenOrder {
    /// Ordre a afficher. Recalcule seulement : au retour sur l'accueil (`epoch` change), au changement de tri, ou quand un
    /// element passe en vrai probleme (conflit, erreur). Un element ajoute va a sa place triee ; un element retire disparait.
    pub fn apply(&mut self, rows: &[SortRow], key: SortKey, epoch: u64) -> Vec<String> {
        let mut problems: Vec<String> = rows.iter().filter(|r| r.real_problem).map(|r| r.name.clone()).collect();
        problems.sort();
        let new_problem = problems.iter().any(|p| !self.problems.contains(p));
        let names: std::collections::HashSet<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        let added = rows.iter().any(|r| !self.order.contains(&r.name));
        if self.key != Some(key) || self.epoch != epoch || new_problem || added {
            let full = sorted_names(rows, key);
            if self.key == Some(key) && self.epoch == epoch && !new_problem {
                // Seulement des ajouts : les tuiles deja la gardent leur ordre relatif, la nouvelle s'insere a sa place.
                let mut out: Vec<String> = self.order.iter().filter(|n| names.contains(n.as_str())).cloned().collect();
                let missing: Vec<String> = full.iter().filter(|n| !out.contains(n)).cloned().collect();
                for n in &missing {
                    let pos = full.iter().position(|x| x == n).unwrap_or(0);
                    let before: Vec<&String> = full[..pos].iter().collect();
                    let at = out.iter().rposition(|x| before.contains(&x)).map(|i| i + 1).unwrap_or(0);
                    out.insert(at, n.clone());
                }
                self.order = out;
            } else {
                self.order = full;
            }
            self.key = Some(key);
            self.epoch = epoch;
        } else {
            self.order.retain(|n| names.contains(n.as_str()));
        }
        self.problems = problems;
        self.order.clone()
    }

    /// L'ordre fige differe-t-il de l'ordre trie actuel ? (au calme, l'accueil avance alors d'une epoque pour se reordonner)
    pub fn stale(&self, rows: &[SortRow], key: SortKey) -> bool {
        self.key == Some(key) && sorted_names(rows, key) != self.order
    }
}

/// Duree de calme (aucune copie en cours) avant que l'accueil se reordonne tout seul.
pub const REORDER_CALM_MS: u64 = 30_000;

/// Duree d'activite avant d'annoncer « Synchronisation… » et duree minimale d'affichage de tout titre passager.
pub const HEADLINE_HOLD_MS: u64 = 2000;

/// Titre passager (copie en cours, ou seulement des copies a faire) : soumis a l'hysteresis.
fn transient(h: Headline, state: State) -> bool {
    matches!(h, Headline::Copying(_)) || (matches!(h, Headline::ToCheck(_)) && state == State::Todo)
}

/// Titre calme (rien a signaler) : a l'abri ou version recue.
fn calm(h: Headline) -> bool {
    matches!(h, Headline::AllSafe | Headline::Incoming(_))
}

/// Hysteresis du titre de l'accueil : « Synchronisation… » seulement si l'activite dure plus de 2 s, et tout titre passager
/// reste au moins 2 s : plus d'aller-retour bouclier / « Tout est a l'abri » pendant les synchros rapides. Les titres
/// importants (coffre, protection arretee ou en pause, vrai probleme) s'affichent tout de suite.
#[derive(Clone, Debug, Default)]
pub struct HeadlineHold {
    shown: Option<Summary>,
    shown_at: u64,
    busy_since: Option<u64>,
}

impl HeadlineHold {
    pub fn apply(&mut self, raw: Summary, now_ms: u64) -> Summary {
        let raw_t = transient(raw.headline, raw.state);
        if raw_t {
            self.busy_since.get_or_insert(now_ms);
        } else {
            self.busy_since = None;
        }
        let Some(prev) = self.shown else {
            // Premiere image : jamais un titre passager (on l'annoncera s'il dure).
            let s = if raw_t { Summary { state: State::Safe, headline: Headline::AllSafe, action: Some(MainAction::SyncAll), ..raw } } else { raw };
            self.shown = Some(s);
            self.shown_at = now_ms;
            return s;
        };
        let prev_t = transient(prev.headline, prev.state);
        let held = now_ms.saturating_sub(self.shown_at) < HEADLINE_HOLD_MS;
        let next = if raw_t {
            if prev_t {
                // Deja annonce : on suit les compteurs ; changement de nature (a faire <-> copie) seulement apres 2 s.
                if std::mem::discriminant(&prev.headline) == std::mem::discriminant(&raw.headline) || !held { raw } else { Summary { total: raw.total, last_copy_ms: raw.last_copy_ms, ..prev } }
            } else if now_ms.saturating_sub(self.busy_since.unwrap_or(now_ms)) >= HEADLINE_HOLD_MS {
                raw
            } else if calm(prev.headline) {
                Summary { total: raw.total, last_copy_ms: prev.last_copy_ms, ..prev }
            } else {
                Summary { state: State::Safe, headline: Headline::AllSafe, action: Some(MainAction::SyncAll), ..raw }
            }
        } else if prev_t && held && calm(raw.headline) {
            Summary { total: raw.total, ..prev }
        } else {
            raw
        };
        if std::mem::discriminant(&next.headline) != std::mem::discriminant(&prev.headline) || next.state != prev.state {
            self.shown_at = now_ms;
        }
        self.shown = Some(next);
        next
    }

    /// Un redessin est-il attendu pour faire evoluer le titre ? Seulement pendant les deux fenetres d'hysteresis (les 2 s
    /// avant d'annoncer une activite, les 2 s de tenue d'un titre passager) : un « a faire » durable (element planifie,
    /// surveillance arretee) ne demande AUCUN redessin au repos.
    pub fn pending(&self, now_ms: u64) -> bool {
        let starting = self.busy_since.map(|b| now_ms.saturating_sub(b) < HEADLINE_HOLD_MS + 100).unwrap_or(false);
        let holding = self.shown.map(|s| transient(s.headline, s.state)).unwrap_or(false) && now_ms.saturating_sub(self.shown_at) < HEADLINE_HOLD_MS + 100;
        starting || holding
    }
}

// ================================================================== resume global (bandeau)

/// Etat de la surveillance vu par l'accueil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    Running,
    /// Pause generale (menu de la zone de notification).
    Paused,
    /// Arretee sur une erreur.
    Failed,
    /// Pas encore demarree (lancement) : rien a signaler.
    Starting,
}

/// Phrase du bandeau.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Headline {
    NoVault,
    VaultUnreachable,
    WatchFailed,
    WatchPaused,
    ToCheck(usize),
    Copying(usize),
    Incoming(usize),
    AllSafe,
    /// Aucun element : l'accueil vide prend le relais.
    Empty,
}

/// Action principale du bandeau.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainAction {
    SyncAll,
    ChooseVault,
    Resume,
    Restart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub state: State,
    pub headline: Headline,
    pub action: Option<MainAction>,
    /// Nombre d'elements proteges.
    pub total: usize,
    /// Derniere copie (ms UTC) parmi tous les elements.
    pub last_copy_ms: Option<u64>,
}

/// Resume global : UN seul message (le plus important d'abord).
pub fn summary(states: &[State], configured: bool, reachable: bool, watch: Watch, last_copy_ms: Option<u64>) -> Summary {
    let total = states.len();
    let n = |s: State| states.iter().filter(|x| **x == s).count();
    let mk = |state, headline, action| Summary { state, headline, action, total, last_copy_ms };
    if !configured {
        return mk(State::Todo, Headline::NoVault, Some(MainAction::ChooseVault));
    }
    if !reachable {
        return mk(State::Problem, Headline::VaultUnreachable, None);
    }
    if total == 0 {
        return mk(State::Safe, Headline::Empty, None);
    }
    if watch == Watch::Failed {
        return mk(State::Problem, Headline::WatchFailed, Some(MainAction::Restart));
    }
    if watch == Watch::Paused {
        return mk(State::Paused, Headline::WatchPaused, Some(MainAction::Resume));
    }
    let check = n(State::Problem) + n(State::Todo);
    if check > 0 {
        return mk(if n(State::Problem) > 0 { State::Problem } else { State::Todo }, Headline::ToCheck(check), Some(MainAction::SyncAll));
    }
    if n(State::Busy) > 0 {
        return mk(State::Busy, Headline::Copying(n(State::Busy)), Some(MainAction::SyncAll));
    }
    if n(State::Incoming) > 0 {
        return mk(State::Incoming, Headline::Incoming(n(State::Incoming)), Some(MainAction::SyncAll));
    }
    mk(State::Safe, Headline::AllSafe, Some(MainAction::SyncAll))
}

// ================================================================== durees relatives

/// Duree relative (« il y a 5 min », « hier », « il y a 3 j »).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ago {
    JustNow,
    Min(u64),
    Hour(u64),
    Yesterday,
    Day(u64),
}

pub fn ago(now_ms: u64, then_ms: u64) -> Ago {
    let s = now_ms.saturating_sub(then_ms) / 1000;
    match s {
        0..=59 => Ago::JustNow,
        60..=3599 => Ago::Min(s / 60),
        3600..=86_399 => Ago::Hour(s / 3600),
        86_400..=172_799 => Ago::Yesterday,
        _ => Ago::Day(s / 86_400),
    }
}

// ================================================================== navigation au clavier

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// Tuile voisine dans une grille de `cols` colonnes et `n` tuiles (la tuile « ajouter » comprise) ; None = sortir de la
/// grille (vers les filtres au-dessus). Gauche/droite passent d'une ligne a l'autre ; bas sur la derniere ligne
/// incomplete va a la derniere tuile.
pub fn grid_neighbor(i: usize, cols: usize, n: usize, d: Dir) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let cols = cols.max(1);
    let i = i.min(n - 1);
    match d {
        Dir::Left => i.checked_sub(1),
        Dir::Right => (i + 1 < n).then_some(i + 1),
        Dir::Up => i.checked_sub(cols),
        Dir::Down => {
            if i + cols < n {
                Some(i + cols)
            } else if i / cols < (n - 1) / cols {
                Some(n - 1)
            } else {
                Some(i)
            }
        }
        Dir::Home => Some(0),
        Dir::End => Some(n - 1),
    }
}

/// Largeur d'une tuile pour remplir la ligne (fenetre large) sans depasser +35 % de la largeur nominale.
pub fn tile_width(avail: f32, cols: usize, base: f32, gap: f32) -> f32 {
    let cols = cols.max(1) as f32;
    (((avail - gap * (cols - 1.0)) / cols).floor()).clamp(base, base * 1.35)
}

/// Fonds sobres des initiales (icone du programme inconnue), comme les maquettes.
const INITIAL_BG: [[u8; 3]; 10] = [
    [0x5b, 0x4a, 0x2a], [0x24, 0x38, 0x4f], [0x6a, 0x2d, 0x4a], [0x5a, 0x40, 0x22], [0x1f, 0x4f, 0x7a],
    [0x3a, 0x3a, 0x3a], [0x2f, 0x4a, 0x3b], [0x2b, 0x3d, 0x5c], [0x3d, 0x5a, 0x2a], [0x4a, 0x2a, 0x2a],
];

/// Couleur de fond STABLE des initiales d'un element (meme nom = meme couleur, sur tous les appareils).
pub fn initial_color(name: &str) -> [u8; 3] {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.to_lowercase().bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    INITIAL_BG[(h % INITIAL_BG.len() as u32) as usize]
}

// ================================================================== marques de position (coffre)

fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(80).collect()
}

fn device_dir(dest: &Path, machine: &str) -> PathBuf {
    dest.join(DEVICES_DIR).join(clean(machine))
}

/// Nom du fichier de marque d'une version.
pub fn mark_file_name(key: &str, version_id: &str) -> String {
    format!("{}~{}.json", clean(key), clean(version_id))
}

/// Note que cet appareil a la version `version_id` de l'element `key` : ecrit une marque (une seule fois) et retire ses
/// propres marques plus anciennes du meme element. Retourne vrai si une marque a ete ecrite.
pub fn note_position(dest: &Path, machine: &str, key: &str, version_id: &str, now_ms: u64) -> bool {
    if machine.trim().is_empty() || key.is_empty() || version_id.is_empty() || !dest.is_dir() {
        return false;
    }
    let dir = device_dir(dest, machine);
    let name = mark_file_name(key, version_id);
    let file = dir.join(&name);
    let prefix = format!("{}~", clean(key));
    let mut wrote = false;
    if !file.exists() {
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }
        let body = serde_json::json!({ "device": machine, "item": key, "version": version_id, "at_ms": now_ms }).to_string();
        wrote = std::fs::write(&file, body).is_ok();
        if !wrote {
            return false;
        }
    }
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_string();
            if n != name && n.starts_with(&prefix) && n.ends_with(".json") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    wrote
}

/// Marques de l'element `key` (tous appareils) : (fichier, identifiant de version, date d'ecriture en ms si connue).
fn mark_files(dest: &Path, key: &str) -> Vec<(PathBuf, String, Option<u64>)> {
    let mut out = Vec::new();
    let prefix = format!("{}~", clean(key));
    let Ok(rd) = std::fs::read_dir(dest.join(DEVICES_DIR)) else { return out };
    for d in rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
        let Ok(files) = std::fs::read_dir(d.path()) else { continue };
        for f in files.filter_map(|e| e.ok()) {
            let n = f.file_name().to_string_lossy().to_string();
            if !n.starts_with(&prefix) || !n.ends_with(".json") || n.len() <= prefix.len() + 5 {
                continue;
            }
            let v: Option<serde_json::Value> = std::fs::read_to_string(f.path()).ok().and_then(|t| serde_json::from_str(&t).ok());
            let id = v.as_ref().and_then(|v| v.get("version")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| n[prefix.len()..n.len() - 5].to_string());
            let at = v.as_ref().and_then(|v| v.get("at_ms")).and_then(|x| x.as_u64());
            out.push((f.path(), id, at));
        }
    }
    out
}

/// Nettoyage des marques : retire, pour TOUS les appareils, les marques de l'element `key` qui designent une des
/// versions `pruned` (versions que l'elagage vient de supprimer). Les autres marques ne sont jamais touchees ni
/// reecrites. Retourne le nombre de marques retirees.
pub fn prune_marks(dest: &Path, key: &str, pruned: &[String]) -> usize {
    if pruned.is_empty() {
        return 0;
    }
    let ids: std::collections::HashSet<String> = pruned.iter().map(|s| clean(s)).collect();
    let mut n = 0;
    for (file, id, _) in mark_files(dest, key) {
        if ids.contains(&clean(&id)) && std::fs::remove_file(&file).is_ok() {
            n += 1;
        }
    }
    n
}

/// Marques deja orphelines (versions elaguees avant ce nettoyage) : une marque est retiree seulement si sa version
/// n'existe plus dans le coffre ET qu'elle a ete ecrite avant la plus ancienne version encore presente (donc sa version
/// etait plus ancienne que tout ce qui reste : elle a ete elaguee) ET depuis plus d'un jour. Rien n'est fait si le coffre
/// contient une version incomplete (synchronisation du service en cours) ou moins de versions que le plancher
/// d'elagage : une marque d'une version pas encore arrivee n'est jamais retiree.
pub fn prune_stale_marks(dest: &Path, key: &str, snap: &crate::cloud::Snapshot, now_ms: u64) -> usize {
    if !snap.incomplete.is_empty() || snap.versions.len() < crate::backup::MIN_KEEP + 1 {
        return 0;
    }
    let present: std::collections::HashSet<String> = snap.versions.iter().map(|v| clean(&v.id)).collect();
    let oldest = snap.versions.iter().map(|v| v.sort_ms()).min().unwrap_or(0);
    let mut n = 0;
    for (file, id, at) in mark_files(dest, key) {
        let Some(at) = at else { continue };
        if !present.contains(&clean(&id)) && at < oldest && now_ms.saturating_sub(at) > 24 * 3600 * 1000 && std::fs::remove_file(&file).is_ok() {
            n += 1;
        }
    }
    n
}

/// Marques de position de l'element `key` : (appareil, identifiant de version). L'appareil est lu dans le fichier
/// (nom exact), a defaut le nom du dossier.
pub fn positions(dest: &Path, key: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let prefix = format!("{}~", clean(key));
    let Ok(rd) = std::fs::read_dir(dest.join(DEVICES_DIR)) else { return out };
    for d in rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
        let Ok(files) = std::fs::read_dir(d.path()) else { continue };
        for f in files.filter_map(|e| e.ok()) {
            let n = f.file_name().to_string_lossy().to_string();
            if !n.starts_with(&prefix) || !n.ends_with(".json") {
                continue;
            }
            let v: Option<serde_json::Value> = std::fs::read_to_string(f.path()).ok().and_then(|t| serde_json::from_str(&t).ok());
            let machine = v.as_ref().and_then(|v| v.get("device")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| d.file_name().to_string_lossy().to_string());
            let id = v.as_ref().and_then(|v| v.get("version")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| n[prefix.len()..n.len() - 5].to_string());
            out.push((machine, id));
        }
    }
    out
}

// ================================================================== collecte (thread de fond)

/// Categories des profils integres et personnalises : (nom en minuscules -> categorie, programme en minuscules -> categorie).
fn profile_categories(data_dir: &Path) -> (HashMap<String, String>, HashMap<String, String>) {
    let emb = crate::profiles::load_embedded();
    let (custom, _) = crate::profiles::load_custom(data_dir, &emb);
    let mut by_name = HashMap::new();
    let mut by_exe = HashMap::new();
    for p in emb.iter().chain(custom.iter()) {
        by_name.insert(p.name.to_lowercase(), p.category.clone());
        for e in &p.detection.exes {
            by_exe.entry(e.to_lowercase()).or_insert_with(|| p.category.clone());
        }
    }
    (by_name, by_exe)
}

/// Nom de fichier d'un programme (« C:\...\Code.exe » -> « code.exe »).
fn exe_file(e: &str) -> String {
    e.rsplit(['/', '\\']).next().unwrap_or(e).trim().to_lowercase()
}

/// Faits de tous les elements (lecture seule du coffre ; a appeler hors de l'interface).
pub fn gather(ctx: &crate::Ctx, cfg: &Config, settings: &Settings, statuses: &HashMap<String, GameStatus>) -> Vec<Facts> {
    let dest = cfg.dest.clone().filter(|d| !d.as_os_str().is_empty() && d.is_dir());
    let state = crate::config::State::load(ctx).unwrap_or_default();
    let (by_name, by_exe) = profile_categories(&ctx.data_dir);
    let db = crate::gamedb::embedded(&ctx.data_dir);
    cfg.games
        .iter()
        .map(|g| {
            let exe = g.exe.as_deref().map(exe_file).filter(|e| !e.is_empty());
            let in_db = !db.find_by_name(&g.name).is_empty() || exe.as_deref().map(|e| !db.find_by_exe(e).is_empty()).unwrap_or(false);
            let cat = by_name.get(&g.name.to_lowercase()).or_else(|| exe.as_ref().and_then(|e| by_exe.get(e)));
            let kind = kind_of(g, cat.map(|s| s.as_str()), in_db);
            let status = statuses.get(&g.name).cloned().unwrap_or(GameStatus::Never);
            let mut f = Facts::bare(g, kind, status);
            f.user_paused = settings.per_game.get(&g.name).map(|s| s.paused).unwrap_or(false);
            f.last_sync_ms = state.last_sync_ms.get(&g.name).copied();
            if let Some(d) = dest.as_deref() {
                let snap = crate::cloud::scan_dest(d, g);
                f.versions = snap.versions.len();
                f.writers = snap.versions.iter().map(|v| (v.meta.machine.clone(), v.id.clone(), v.meta.global_hash.clone(), v.sort_ms())).collect();
                f.head = crate::backup::active_version(&snap).map(|v| Head { machine: v.meta.machine.clone(), id: v.id.clone(), hash: v.meta.global_hash.clone(), saved_ms: v.sort_ms() });
                f.bytes = crate::backup::active_version(&snap).map(|v| v.meta.files.iter().map(|x| x.size).sum()).unwrap_or(0);
                f.marks = positions(d, &crate::cloud::dest_key(g));
            }
            f
        })
        .collect()
}

/// Thread d'etat : note la position de cet appareil pour chaque element a jour (contenu local = version active).
pub fn note_positions(dest: &Path, machine: &str, games: &[Game], statuses: &HashMap<String, GameStatus>) -> usize {
    let mut n = 0;
    for g in games {
        if !matches!(statuses.get(&g.name), Some(GameStatus::UpToDate)) {
            continue;
        }
        let snap = crate::cloud::scan_dest(dest, g);
        if let Some(v) = crate::backup::active_version(&snap) {
            if note_position(dest, machine, &crate::cloud::dest_key(g), &v.id, crate::util::now_ms()) {
                n += 1;
            }
        }
    }
    n
}

// ================================================================== recherche (2.0.1)

/// La recherche n'apparait qu'a partir de ce nombre d'elements.
pub const SEARCH_MIN_ITEMS: usize = 12;

/// La recherche est-elle proposee pour ce nombre d'elements ?
pub fn search_visible(items: usize) -> bool {
    items >= SEARCH_MIN_ITEMS
}

/// Forme comparable d'un texte : minuscules, sans accents ni signes combinants (« Pokémon » = « pokemon »), espaces
/// simples. Les autres ecritures (russe, chinois) ne changent que de casse. Pur.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        for l in c.to_lowercase() {
            match l {
                '\u{300}'..='\u{36f}' => {}
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
                'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => out.push('c'),
                'ď' | 'đ' => out.push('d'),
                'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => out.push('e'),
                'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
                'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => out.push('i'),
                'ĺ' | 'ļ' | 'ľ' | 'ł' => out.push('l'),
                'ñ' | 'ń' | 'ņ' | 'ň' => out.push('n'),
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => out.push('o'),
                'ŕ' | 'ř' => out.push('r'),
                'ś' | 'ŝ' | 'ş' | 'š' => out.push('s'),
                'ţ' | 'ť' => out.push('t'),
                'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => out.push('u'),
                'ý' | 'ÿ' => out.push('y'),
                'ź' | 'ż' | 'ž' => out.push('z'),
                'ß' => out.push_str("ss"),
                'æ' => out.push_str("ae"),
                'œ' => out.push_str("oe"),
                'ё' => out.push('е'),
                'й' => out.push('и'),
                c if c.is_whitespace() => {
                    if !out.ends_with(' ') && !out.is_empty() {
                        out.push(' ');
                    }
                }
                c => out.push(c),
            }
        }
    }
    out.trim_end().to_string()
}

/// Un element correspond-il a la recherche ? Chaque mot tape doit se trouver dans un des noms (nom affiche, nom d'origine),
/// sans tenir compte de la casse ni des accents. Recherche vide : tout correspond. Pur.
pub fn matches_query(names: &[&str], query: &str) -> bool {
    let q = fold(query);
    if q.is_empty() {
        return true;
    }
    let hay: Vec<String> = names.iter().map(|n| fold(n)).collect();
    q.split(' ').all(|w| hay.iter().any(|h| h.contains(w)))
}

/// Indices des elements affiches : filtre de categorie ET recherche combines. Pur.
pub fn visible_indices(rows: &[(Kind, Vec<&str>)], filter: Filter, query: &str) -> Vec<usize> {
    rows.iter().enumerate().filter(|(_, (k, names))| filter.matches(*k) && matches_query(names, query)).map(|(i, _)| i).collect()
}

/// Compteurs des filtres pour une recherche donnee (meme ordre que `Filter::ALL`). Pur.
pub fn filter_counts_for(rows: &[(Kind, Vec<&str>)], query: &str) -> [usize; 4] {
    let kinds: Vec<Kind> = rows.iter().filter(|(_, n)| matches_query(n, query)).map(|(k, _)| *k).collect();
    filter_counts(&kinds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories() {
        let mut g = Game { name: "X".into(), exe: Some("x.exe".into()), ..Default::default() };
        assert_eq!(kind_of(&g, Some("app"), true), Kind::App);
        assert_eq!(kind_of(&g, None, true), Kind::Game);
        assert_eq!(kind_of(&g, None, false), Kind::Game);
        g.exe = None;
        assert_eq!(kind_of(&g, None, false), Kind::Folder);
        assert_eq!(kind_of(&g, Some("emulator"), false), Kind::Game);
    }
}
