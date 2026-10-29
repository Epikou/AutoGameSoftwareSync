//! Scan du PC (2.0, lot 9) : propose tout seul quoi proteger, classe par score de confiance.
//!
//! Sources, toutes en LECTURE SEULE et sur des emplacements cibles (jamais de parcours recursif du disque) :
//! - jeux : bibliotheques des lanceurs (Steam, Epic, GOG, Xbox, Ubisoft, EA, itch ; Heroic et Lutris sous Linux), base
//!   embarquee croisee avec les dossiers perso (titres dont les sauvegardes existent, meme sans le jeu installe),
//!   profils de jeux et emulateurs ;
//! - logiciels : profils, liste des logiciels installes (cles Uninstall ; raccourcis `.desktop` sous Linux) croisee avec
//!   la base maison et les couches de connaissance locales (lanceurs, deduction) ;
//! - dossiers perso (Documents, Images, Bureau, Musique, Videos, Telechargements) ;
//! - Windows : raccourcis du Bureau, menu Demarrer de l'utilisateur, epingles de la barre des taches, fond d'ecran actuel,
//!   et (2.0.2) les Reglages Windows de `winprefs` (registre, fichiers, liste « A refaire »), toujours decoches.
//!
//! Classement (`classify`) : Recommande = score >= 70 ET emplacement verifie ET donnees recentes (coche) ; Peut-etre =
//! 40 a 69, ou recommande mais ancien (decoche) ; Masques = outils de developpement, systeme, lanceurs, vides, rien
//! trouve, trop incertain, donnees sensibles (raison affichee). La liste noire `sensitive` s'applique toujours.
//! Le dernier scan est memorise (`scan-last.json`) : un nouveau scan ne propose que les nouveautes.
pub mod sources;

use crate::config::{Game, Location};
use crate::detect::{Installed, Store};
use crate::gamedb::{DbGame, GameDb};
use crate::knowledge::{self, paths::Folders, Found, Layer, Program};
use crate::profiles::{Profile, Vars};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Seuils (memes que la detection : ETUDE §8.2).
pub const RECOMMEND: u8 = crate::gamedetect::PROPOSE_THRESHOLD;
pub const MAYBE: u8 = crate::gamedetect::MAYBE_THRESHOLD;
/// Donnees « recentes » : un fichier modifie depuis ce nombre de jours.
pub const RECENT_DAYS: u64 = knowledge::RECENT_DAYS;
/// Gros dossier : avertissement et 3 versions gardees (ETUDE §5).
pub const BIG_BYTES: u64 = crate::picker::HUGE_BYTES;
/// Tres gros dossier : decoche des que sa taille est connue (si l'utilisateur n'y a pas touche).
pub const HUGE_BYTES: u64 = 10 * 1024 * 1024 * 1024;
/// Versions gardees pour un gros dossier.
pub const BIG_KEEP: usize = 3;
/// Seuil par defaut de l'alerte poids (500 Mo). Le seuil reel est UNIQUE et reglable : `weight::limit_bytes()`. Au-dela, un
/// dossier perso n'est jamais coche d'office (« Peut-être » avec l'impact estime dans le coffre : taille x versions gardees),
/// et tout element garde 3 versions a l'ajout.
pub const PERSONAL_AUTO_MAX: u64 = crate::weight::DEFAULT_LIMIT_MB as u64 * crate::weight::MB;
/// Fichier du dernier scan (dossier de donnees).
pub const LAST_FILE: &str = "scan-last.json";

// ================================================================== modele

/// Etapes du scan et categories des resultats (dans cet ordre).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Cat {
    Games,
    Apps,
    Folders,
    System,
}

impl Cat {
    pub const ALL: [Cat; 4] = [Cat::Games, Cat::Apps, Cat::Folders, Cat::System];
    pub fn index(self) -> usize {
        self as usize
    }
    /// Categorie des donnees d'usage (`item_added`).
    pub fn usage(self, app: bool) -> &'static str {
        match self {
            Cat::Games if app => "app",
            Cat::Games => "game",
            Cat::Apps => "app",
            Cat::Folders => "folder",
            Cat::System => "windows",
        }
    }
}

/// Groupe d'un resultat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Recommended,
    Maybe,
    Hidden,
}

/// Pourquoi un resultat est masque.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hide {
    /// Outil de developpement.
    DevTool,
    /// Composant du systeme, pilote, environnement d'execution.
    System,
    /// Lanceur de jeux (ses jeux sont proposes a part).
    Launcher,
    /// Mots de passe, jetons, profil de navigateur : jamais protege.
    Sensitive,
    /// Dossier vide.
    Empty,
    /// Rien trouve a proteger sur ce PC.
    NoData,
    /// Confiance trop faible.
    Weak,
    /// Contient le coffre lui-meme.
    InVault,
    /// Prevu dans une prochaine version (preferences du registre).
    Later,
}

/// Indice retenu dans le score.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sign {
    /// Installe par un lanceur (bibliotheque).
    Library,
    /// Emplacements donnes par la base embarquee.
    BaseKnown,
    /// Dossier d'un titre de la base trouve dans les dossiers perso.
    BaseFolder,
    /// ... et ce dossier porte le nom du titre.
    BaseFolderName,
    /// Profil integre / personnalise.
    Profile,
    /// Le programme du profil est installe.
    ProfileInstalled,
    /// Le profil demande une verification (avertissements).
    ProfileWarning,
    /// Present dans la liste des logiciels installes.
    Installed,
    /// Emplacement confirme par d'autres installations (base maison).
    HomeBase,
    /// Emplacement donne par le lanceur (sauvegarde cloud du lanceur).
    LauncherCloud,
    /// Dossier deduit par son nom.
    Deduced,
    /// Dossier deduit avec une confiance elevee (nom + editeur / registre).
    DeducedStrong,
    /// Documents, Images.
    PersonalMain,
    /// Bureau, Musique, Videos, Telechargements.
    PersonalOther,
    /// Raccourcis, menu Demarrer, epingles.
    Shortcuts,
    /// Fond d'ecran.
    Wallpaper,
    /// Emplacement choisi par l'utilisateur pour un resultat masque « rien trouve » (2.0.1).
    UserPlace,
}

impl Sign {
    pub fn points(self) -> i32 {
        match self {
            Sign::PersonalMain | Sign::Shortcuts => 80,
            Sign::BaseFolder | Sign::Profile | Sign::PersonalOther | Sign::Wallpaper | Sign::UserPlace => 50,
            Sign::Library | Sign::BaseKnown | Sign::HomeBase | Sign::LauncherCloud => 40,
            Sign::ProfileInstalled | Sign::Installed | Sign::DeducedStrong => 30,
            Sign::BaseFolderName | Sign::Deduced => 20,
            Sign::ProfileWarning => -20,
        }
    }
}

/// D'ou vient un resultat (texte « Pourquoi ? »).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Launcher(Store),
    /// Jeu installe dans Playnite (liste ecrite par l'extension) : source Playnite (« Steam », « GOG », vide = ajoute a la main).
    Playnite(String),
    /// Base embarquee, titre non installe (dossier de sauvegardes trouve).
    Base,
    Profile,
    Emulator,
    /// Liste des logiciels installes.
    Installed,
    Personal(Personal),
    System(SysItem),
}

/// Dossiers perso.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Personal {
    Documents,
    Pictures,
    Desktop,
    Music,
    Videos,
    Downloads,
}

impl Personal {
    pub const ALL: [Personal; 6] = [Personal::Documents, Personal::Pictures, Personal::Desktop, Personal::Music, Personal::Videos, Personal::Downloads];
    fn main(self) -> bool {
        matches!(self, Personal::Documents | Personal::Pictures)
    }
    pub fn code(self) -> &'static str {
        match self {
            Personal::Documents => "documents",
            Personal::Pictures => "pictures",
            Personal::Desktop => "desktop",
            Personal::Music => "music",
            Personal::Videos => "videos",
            Personal::Downloads => "downloads",
        }
    }
}

/// Elements « Windows » (fichiers) ; Linux : raccourcis du bureau et lanceurs du menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SysItem {
    DesktopShortcuts,
    StartMenu,
    Taskbar,
    Wallpaper,
    /// Ancienne annonce « preferences du registre » (2.0.0 / 2.0.1) : remplacee par `Pref` en 2.0.2.
    RegistryPrefs,
    /// Reglages Windows (2.0.2) : registre, fichiers, liste « A refaire ».
    Pref(crate::winprefs::Kind),
}

impl SysItem {
    pub fn code(self) -> &'static str {
        match self {
            SysItem::Pref(k) => k.code(),
            SysItem::DesktopShortcuts => "desktop-shortcuts",
            SysItem::StartMenu => "start-menu",
            SysItem::Taskbar => "taskbar",
            SysItem::Wallpaper => "wallpaper",
            SysItem::RegistryPrefs => "registry-prefs",
        }
    }
}

/// Ce qui sera protege.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum What {
    Saves,
    Settings,
    SavesAndSettings,
    Folder,
    Shortcuts,
    File,
}

/// Un emplacement propose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loc {
    pub path: PathBuf,
    pub is_file: bool,
    pub label: String,
    pub filter: Option<String>,
    pub exclude: Option<String>,
}

impl Loc {
    pub fn dir(path: PathBuf) -> Loc {
        Loc { path, is_file: false, label: String::new(), filter: None, exclude: None }
    }
}

/// Un resultat du scan.
#[derive(Clone, Debug)]
pub struct Item {
    /// Cle stable (categorie + nom normalise) : dernier scan, selection.
    pub key: String,
    pub cat: Cat,
    pub name: String,
    pub what: What,
    pub origin: Origin,
    pub locs: Vec<Loc>,
    pub exe: Option<String>,
    pub exe_path: Option<PathBuf>,
    pub signs: Vec<Sign>,
    pub score: u8,
    pub tier: Tier,
    pub hide: Option<Hide>,
    /// Fichier le plus recent (ms).
    pub newest_ms: Option<u64>,
    /// Deja protege : nom affiche de l'element existant.
    pub already: Option<String>,
    /// Deja vu au dernier scan.
    pub seen: bool,
    /// Logiciel (et non jeu) : donnees d'usage.
    pub app: bool,
}

impl Item {
    /// Coche par defaut : recommande, nouveau, pas deja protege. Jamais un element de la categorie Windows (Bureau, menu
    /// Demarrer, epingles, fond d'ecran, futures preferences) : ils restent proposes, decoches, y compris au premier lancement.
    pub fn default_checked(&self) -> bool {
        self.tier == Tier::Recommended && self.already.is_none() && !self.seen && self.cat != Cat::System && (!self.is_playnite() || self.place_sure())
    }
    /// Jeu de la bibliotheque Playnite (section a part du scan).
    pub fn is_playnite(&self) -> bool {
        matches!(self.origin, Origin::Playnite(_))
    }
    /// Emplacement connu de facon sure (profil cure, base embarquee, base maison confirmee) : seul cas coche d'office pour un
    /// jeu Playnite (2.0.3) ; une deduction par le nom reste decochee.
    pub fn place_sure(&self) -> bool {
        self.signs.iter().any(|s| matches!(s, Sign::Profile | Sign::BaseKnown | Sign::HomeBase))
    }
    /// Peut-on le cocher ? (ni masque ni deja protege)
    pub fn selectable(&self) -> bool {
        self.tier != Tier::Hidden && self.already.is_none() && !self.locs.is_empty()
    }
}

/// Taille mesuree en arriere-plan (lecture paresseuse).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub bytes: u64,
    pub files: u64,
    /// Mesure interrompue (plafond) : « au moins ».
    pub partial: bool,
}

/// Dossier perso (Documents, Images, Bureau...).
pub fn is_personal(it: &Item) -> bool {
    matches!(it.origin, Origin::Personal(_))
}

/// Versions gardees a l'ajout d'un element de cette taille : 3 pour un gros dossier (2 Go et plus) ou un dossier perso de
/// plus de 500 Mo, sinon le reglage par defaut.
pub fn keep_for(it: &Item, bytes: u64) -> usize {
    let _ = it;
    if bytes >= BIG_BYTES || crate::weight::is_heavy(bytes) {
        BIG_KEEP
    } else {
        crate::backup::DEFAULT_KEEP
    }
}

/// Place estimee dans le coffre (le coffre recopie chaque version en entier) : taille x versions gardees.
pub fn vault_impact(it: &Item, bytes: u64) -> u64 {
    bytes.saturating_mul(keep_for(it, bytes) as u64)
}

/// Regle des 500 Mo, une fois la taille connue : un dossier perso plus gros n'est plus recommande (« Peut-être », donc pas
/// coche d'office). Vrai si l'element a change de groupe. Pure.
pub fn apply_size_rule(it: &mut Item, s: &Size) -> bool {
    if is_personal(it) && crate::weight::is_heavy(s.bytes) && it.tier == Tier::Recommended {
        it.tier = Tier::Maybe;
        return true;
    }
    false
}

/// La coche par defaut attend la mesure de la taille (dossier perso recommande : il peut depasser 500 Mo).
pub fn check_waits_for_size(it: &Item) -> bool {
    is_personal(it) && it.default_checked()
}

/// Score et groupe. Pur.
pub fn classify(signs: &[Sign], hide: Option<Hide>, verified: bool, newest_ms: Option<u64>, now_ms: u64) -> (u8, Tier) {
    let score = signs.iter().map(|s| s.points()).sum::<i32>().clamp(0, 100) as u8;
    if hide.is_some() {
        return (0, Tier::Hidden);
    }
    let recent = newest_ms.map(|t| now_ms.saturating_sub(t) <= RECENT_DAYS * 86_400_000).unwrap_or(false);
    if score >= RECOMMEND && verified && recent {
        (score, Tier::Recommended)
    } else if score >= MAYBE && verified {
        (score, Tier::Maybe)
    } else {
        (score, Tier::Hidden)
    }
}

/// Raison de masquage d'un score trop faible (classify a rendu Hidden sans raison).
pub fn hide_reason(it: &Item) -> Hide {
    it.hide.unwrap_or(if it.locs.is_empty() { Hide::NoData } else { Hide::Weak })
}

// ================================================================== dernier scan

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct LastScan {
    #[serde(default)]
    pub at_ms: u64,
    /// Cles des resultats proposes (ni masques ni deja proteges).
    #[serde(default)]
    pub keys: Vec<String>,
}

impl LastScan {
    pub fn load(data_dir: &Path) -> LastScan {
        std::fs::read_to_string(data_dir.join(LAST_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self, data_dir: &Path) -> crate::Res<()> {
        crate::util::write_atomic(&data_dir.join(LAST_FILE), serde_json::to_string_pretty(self)?.as_bytes())
    }
    pub fn of(items: &[Item], now_ms: u64) -> LastScan {
        let mut keys: Vec<String> = items.iter().filter(|i| i.tier != Tier::Hidden && i.already.is_none()).map(|i| i.key.clone()).collect();
        keys.sort();
        keys.dedup();
        LastScan { at_ms: now_ms, keys }
    }
    /// Fusion : un element vu une fois reste « deja vu ».
    pub fn merged(&self, newer: &LastScan) -> LastScan {
        let mut keys = self.keys.clone();
        keys.extend(newer.keys.iter().cloned());
        keys.sort();
        keys.dedup();
        LastScan { at_ms: newer.at_ms, keys }
    }
}

// ================================================================== environnement (injectable)

/// Logiciel installe (cle Uninstall ou raccourci `.desktop`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppEntry {
    pub name: String,
    pub publisher: Option<String>,
    pub location: Option<PathBuf>,
    /// Exe principal (icone de la cle Uninstall, cible du raccourci).
    pub exe: Option<PathBuf>,
    /// Entree systeme (Linux : raccourci du systeme).
    pub system: bool,
}

/// Ou chercher les emplacements d'un programme (base maison puis couches 1 et 2 ; cache des couches 3 et 4).
pub type Locator = Box<dyn Fn(&Program) -> Vec<Found> + Send + Sync>;

pub struct ScanEnv {
    pub data_dir: PathBuf,
    pub folders: Folders,
    pub personal: Vec<(Personal, PathBuf)>,
    /// Elements « Windows » (fichiers) : (element, chemin, fichier unique, filtre).
    pub system: Vec<(SysItem, PathBuf, bool, Option<String>)>,
    /// Reglages Windows proposes (2.0.2 ; vide sous Linux).
    pub prefs: Vec<crate::winprefs::Offer>,
    pub launcher_games: Box<dyn Fn() -> Vec<Installed> + Send + Sync>,
    pub apps: Box<dyn Fn() -> Vec<AppEntry> + Send + Sync>,
    pub steam_libs: Vec<PathBuf>,
    pub profiles: Vec<Profile>,
    pub vars: Vars,
    pub db: Arc<GameDb>,
    pub locate: Locator,
    /// Elements deja configures.
    pub games: Vec<Game>,
    pub vault: Option<PathBuf>,
    pub last: LastScan,
    pub now_ms: u64,
    /// Croiser toute la base embarquee avec les dossiers perso.
    pub db_probe: bool,
    /// Linux : prefixe Proton d'un jeu Steam (AppID).
    pub prefix_of: Box<dyn Fn(u64) -> Option<PathBuf> + Send + Sync>,
    /// 2.0.3 : jeux installes dans Playnite (fichier de l'extension ; vide sans Playnite ou sous Linux).
    pub playnite: Vec<crate::playnite::LibGame>,
}

// ================================================================== progression partagee

#[derive(Clone, Debug, Default)]
pub struct Progress {
    /// Etape en cours (index de `Cat::ALL`) ; 4 = fini.
    pub step: usize,
    /// Avancement de chaque etape (0..1).
    pub frac: [f32; 4],
    /// Resultats proposables trouves par etape (compteurs).
    pub counts: [usize; 4],
    pub items: Vec<Item>,
    pub sizes: HashMap<String, Size>,
    pub finished: bool,
    pub cancelled: bool,
    pub elapsed_ms: u64,
    /// Tailles toutes mesurees.
    pub sizes_done: bool,
}

/// Scan en cours (thread de fond) : progression partagee et annulation.
#[derive(Clone)]
pub struct Handle {
    pub state: Arc<Mutex<Progress>>,
    pub cancel: Arc<AtomicBool>,
}

impl Handle {
    pub fn view(&self) -> Progress {
        self.state.lock().map(|p| p.clone()).unwrap_or_default()
    }
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Lance le scan dans un thread de fond. `wake` est appele a chaque changement visible (redessin de l'interface).
pub fn start(env: ScanEnv, wake: Arc<dyn Fn() + Send + Sync>) -> Handle {
    let h = Handle { state: Arc::new(Mutex::new(Progress::default())), cancel: Arc::new(AtomicBool::new(false)) };
    let h2 = h.clone();
    std::thread::Builder::new()
        .name("agss-scan".into())
        .spawn(move || {
            run(&env, &h2, &*wake);
            measure_sizes(&h2, &*wake);
        })
        .ok();
    h
}

/// Comme `start`, mais l'environnement est construit dans le thread de fond (lecture des profils, base embarquee...) :
/// l'interface ne fait jamais aucun acces disque pour lancer le scan.
pub fn start_with(build: impl FnOnce() -> ScanEnv + Send + 'static, wake: Arc<dyn Fn() + Send + Sync>) -> Handle {
    let h = Handle { state: Arc::new(Mutex::new(Progress::default())), cancel: Arc::new(AtomicBool::new(false)) };
    let h2 = h.clone();
    std::thread::Builder::new()
        .name("agss-scan".into())
        .spawn(move || {
            let env = build();
            run(&env, &h2, &*wake);
            measure_sizes(&h2, &*wake);
        })
        .ok();
    h
}

// ================================================================== moteur

struct Run<'a> {
    env: &'a ScanEnv,
    h: &'a Handle,
    wake: &'a dyn Fn(),
    items: Vec<Item>,
    names: HashSet<String>,
    dirs: DirCache,
    used_profiles: HashSet<String>,
    install_dirs: Vec<PathBuf>,
}

/// Lance toutes les etapes (bloquant). Annulable entre deux elements.
pub fn run(env: &ScanEnv, h: &Handle, wake: &dyn Fn()) {
    let t0 = std::time::Instant::now();
    let mut r = Run { env, h, wake, items: Vec::new(), names: HashSet::new(), dirs: DirCache::default(), used_profiles: HashSet::new(), install_dirs: Vec::new() };
    let steps: [fn(&mut Run); 4] = [step_games, step_apps, step_folders, step_system];
    // Diagnostic des captures (jamais dans l'interface) : etapes ralenties pour montrer la progression.
    let slow = std::env::var("AGSS_SCAN_SLOW_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    for (i, f) in steps.iter().enumerate() {
        if h.cancelled() {
            break;
        }
        r.set(|p| p.step = i);
        for k in 0..(slow / 50) {
            if h.cancelled() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            r.set(|p| p.frac[i] = 0.6 * k as f32 / (slow / 50) as f32);
        }
        f(&mut r);
        r.set(|p| p.frac[i] = 1.0);
    }
    let cancelled = h.cancelled();
    if !cancelled {
        let last = LastScan::of(&r.items, env.now_ms);
        let _ = env.last.merged(&last).save(&env.data_dir);
    }
    let ms = t0.elapsed().as_millis() as u64;
    r.set(|p| {
        p.step = 4;
        p.finished = true;
        p.cancelled = cancelled;
        p.elapsed_ms = ms;
    });
}

impl<'a> Run<'a> {
    fn set(&self, f: impl FnOnce(&mut Progress)) {
        if let Ok(mut p) = self.h.state.lock() {
            f(&mut p);
        }
        (self.wake)();
    }

    fn frac(&self, step: Cat, v: f32) {
        self.set(|p| p.frac[step.index()] = v.clamp(0.0, 0.99));
    }

    fn cancelled(&self) -> bool {
        self.h.cancelled()
    }

    /// Ajoute un resultat : liste noire, doublons, deja protege, deja vu, classement.
    fn push(&mut self, mut it: Item) {
        let nk = crate::cloud::game_key(&it.name);
        if nk.is_empty() || self.names.contains(&format!("{:?}:{}", it.cat, nk)) {
            return;
        }
        // Liste noire dure : un emplacement sensible rend tout l'element sensible (jamais propose).
        if it.locs.iter().any(|l| loc_sensitive(l, &self.env.folders)) {
            it.hide = Some(Hide::Sensitive);
            it.locs.clear();
        }
        // Recouvrement avec un resultat deja trouve de la meme categorie (meme dossier sous deux noms) : le premier gagne.
        // Entre categories (Documents et une sauvegarde rangee dedans, Bureau et ses raccourcis), les deux restent proposes.
        if it.hide.is_none() && !it.locs.is_empty() && self.items.iter().any(|o| o.cat == it.cat && o.hide.is_none() && overlaps(&o.locs, &it.locs)) {
            return;
        }
        if it.hide.is_none() {
            if let Some(v) = &self.env.vault {
                if it.locs.iter().any(|l| under(v, &l.path) || under(&l.path, v)) {
                    it.hide = Some(Hide::InVault);
                }
            }
        }
        let verified = !it.locs.is_empty() && it.locs.iter().all(|l| l.path.exists());
        if it.newest_ms.is_none() {
            it.newest_ms = it.locs.iter().filter_map(|l| newest(l)).max();
        }
        let (score, tier) = classify(&it.signs, it.hide, verified, it.newest_ms, self.env.now_ms);
        // Categorie Windows : jamais « recommande » (rien n'y est coche d'office), proposee dans « Peut-etre ».
        let tier = if it.cat == Cat::System && tier == Tier::Recommended { Tier::Maybe } else { tier };
        it.score = score;
        it.tier = tier;
        if tier == Tier::Hidden && it.hide.is_none() {
            it.hide = Some(hide_reason(&it));
        }
        it.already = already_protected(&self.env.games, &it);
        it.key = format!("{}:{}", cat_code(it.cat), nk);
        it.seen = self.env.last.keys.contains(&it.key);
        self.names.insert(format!("{:?}:{}", it.cat, nk));
        let cat = it.cat;
        let counted = it.tier != Tier::Hidden;
        self.items.push(it.clone());
        self.set(|p| {
            p.items.push(it);
            if counted {
                p.counts[cat.index()] += 1;
            }
        });
    }
}

fn cat_code(c: Cat) -> &'static str {
    match c {
        Cat::Games => "game",
        Cat::Apps => "app",
        Cat::Folders => "folder",
        Cat::System => "system",
    }
}

fn base_item(cat: Cat, name: &str, what: What, origin: Origin) -> Item {
    Item {
        key: String::new(),
        cat,
        name: name.trim().to_string(),
        what,
        origin,
        locs: Vec::new(),
        exe: None,
        exe_path: None,
        signs: Vec::new(),
        score: 0,
        tier: Tier::Hidden,
        hide: None,
        newest_ms: None,
        already: None,
        seen: false,
        app: false,
    }
}

// ------------------------------------------------------------------ etape 1 : jeux

fn step_games(r: &mut Run) {
    let env = r.env;
    let mut inst = (env.launcher_games)();
    inst.retain(|g| !crate::detect::is_non_game(g));
    inst.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    r.frac(Cat::Games, 0.15);
    let n = inst.len().max(1);
    for (i, g) in inst.iter().enumerate() {
        if r.cancelled() {
            return;
        }
        r.install_dirs.push(g.install_dir.clone());
        let it = launcher_item(r, g);
        r.push(it);
        r.frac(Cat::Games, 0.15 + 0.55 * (i + 1) as f32 / n as f32);
    }
    // 2.0.3 : jeux installes dans Playnite, APRES ceux des lanceurs (section « Playnite » du scan) : un jeu de Steam / GOG /
    // Epic... garde sa section, son etiquette et son programme ; Playnite n'ajoute que les jeux qu'aucun lanceur n'a vus
    // (jeux PC seulement : les ROM emulees ne sont pas proposees). Meme traitement ensuite (profils, base, connaissance).
    let launcher_names: Vec<String> = inst.iter().map(|g| g.name.clone()).collect();
    for g in crate::playnite::not_seen_by_launchers(&env.playnite, &env.games, &launcher_names) {
        if r.cancelled() {
            return;
        }
        let inst = crate::playnite::to_installed(g);
        if inst.install_dir.as_os_str().is_empty() {
            continue;
        }
        r.install_dirs.push(inst.install_dir.clone());
        let mut it = launcher_item(r, &inst);
        it.origin = Origin::Playnite(g.source.trim().to_string());
        r.push(it);
    }
    // Profils de jeux et emulateurs installes (hors bibliotheques des lanceurs).
    for p in env.profiles.iter().filter(|p| p.category == "game" || p.category == "emulator") {
        if r.cancelled() {
            return;
        }
        if r.used_profiles.contains(&p.id) {
            continue;
        }
        if let Some(it) = profile_item(r, p) {
            r.push(it);
        }
    }
    r.frac(Cat::Games, 0.75);
    // Base embarquee croisee avec les dossiers perso : sauvegardes de titres connus.
    if env.db_probe {
        let db = env.db.clone();
        let total = db.games().len().max(1);
        for (i, dg) in db.games().iter().enumerate() {
            if i % 512 == 0 {
                if r.cancelled() {
                    return;
                }
                r.frac(Cat::Games, 0.75 + 0.24 * i as f32 / total as f32);
            }
            if r.names.contains(&format!("{:?}:{}", Cat::Games, crate::cloud::game_key(&dg.name))) {
                continue;
            }
            if let Some(it) = base_folder_item(r, dg) {
                r.push(it);
            }
        }
    }
}

/// Profil correspondant a un jeu installe (nom ou exe).
fn profile_for<'p>(profiles: &'p [Profile], name: &str, exe: Option<&str>) -> Option<&'p Profile> {
    let n = knowledge::norm(name);
    profiles.iter().filter(|p| p.category == "game").find(|p| {
        knowledge::norm(&p.name) == n || exe.map(|e| p.detection.exes.iter().any(|x| x.eq_ignore_ascii_case(e))).unwrap_or(false)
    })
}

fn launcher_item(r: &mut Run, g: &Installed) -> Item {
    let env = r.env;
    let mut it = base_item(Cat::Games, &g.name, What::Saves, Origin::Launcher(g.store));
    it.signs.push(Sign::Library);
    let exe = g.exe.clone().or_else(|| crate::detect::guess_exe(&g.install_dir));
    it.exe = exe.clone();
    it.exe_path = exe.as_ref().map(|e| g.install_dir.join(e)).filter(|p| p.is_file());
    let np = crate::gamedetect::norm_path(&g.install_dir);
    if crate::gamedetect::is_dev_dir(&format!("{}/", np)) || crate::gamedetect::is_dev_product(&g.name) {
        it.hide = Some(Hide::DevTool);
        return it;
    }
    // 1. profil (emplacements cures, souvent plusieurs)
    if let Some(p) = profile_for(&env.profiles, &g.name, exe.as_deref()) {
        let locs = profile_locs(p, &env.vars);
        if !locs.is_empty() {
            r.used_profiles.insert(p.id.clone());
            it.signs.push(Sign::Profile);
            it.locs = locs;
            it.what = What::SavesAndSettings;
            return it;
        }
    }
    // Linux : jeu Windows de Steam -> bases Windows dans son prefixe Proton.
    let mut folders = env.folders.clone();
    let appid = if g.store == Store::Steam { g.id.parse::<u64>().ok() } else { None };
    if let Some(pfx) = appid.and_then(|id| (env.prefix_of)(id)) {
        folders = folders.with_prefix(&pfx);
    }
    // 2. base embarquee
    let hits: Vec<&DbGame> = {
        let mut v = match (g.store, g.id.parse::<u64>().ok()) {
            (Store::Steam, Some(id)) => env.db.find_by_steam(id),
            (Store::Gog, Some(id)) => env.db.find_by_gog(id),
            _ => Vec::new(),
        };
        if v.is_empty() {
            v = env.db.find_by_name(&g.name);
        }
        if v.is_empty() {
            if let Some(d) = g.install_dir.file_name() {
                v = env.db.find_by_install_dir(&d.to_string_lossy());
            }
        }
        v
    };
    // Programme a surveiller : l'exe de lancement connu de la base (le plus gros exe du dossier est souvent un lanceur).
    if g.exe.is_none() {
        if let Some(p) = hits.first().and_then(|dg| find_exe(&g.install_dir, &dg.exes)) {
            it.exe = p.file_name().map(|n| n.to_string_lossy().to_string());
            it.exe_path = Some(p);
        }
    }
    if let Some(dg) = hits.first() {
        let locs = db_locs(dg, &folders, Some(&g.install_dir), &env.steam_libs, Some(&g.id), &mut r.dirs, &g.install_dir);
        if !locs.is_empty() {
            it.signs.push(Sign::BaseKnown);
            it.locs = locs;
            return it;
        }
    }
    // 3. base maison, lanceurs (sauvegarde cloud), deduction
    let mut prog = match &it.exe_path {
        Some(p) => Program::describe(Some(p.as_path()), &p.to_string_lossy(), &g.name, false),
        None => Program { name: g.name.clone(), ..Default::default() },
    };
    prog.title = Some(g.name.clone());
    if prog.install_dir.is_none() {
        prog.install_dir = Some(g.install_dir.clone());
    }
    if prog.steam_appid.is_none() {
        prog.steam_appid = appid;
    }
    apply_found(&mut it, (env.locate)(&prog), &g.install_dir);
    if it.locs.is_empty() {
        it.hide = Some(Hide::NoData);
    }
    it
}

/// Premier exe de `names` present sous le dossier d'installation (4 niveaux, 6 000 entrees au plus).
fn find_exe(install: &Path, names: &[String]) -> Option<PathBuf> {
    if names.is_empty() {
        return None;
    }
    let want: Vec<String> = names.iter().map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n).trim().to_lowercase()).filter(|n| !n.is_empty()).collect();
    let mut found: Vec<(usize, PathBuf)> = Vec::new();
    for e in walkdir::WalkDir::new(install).max_depth(4).follow_links(false).into_iter().filter_map(|e| e.ok()).take(6_000) {
        if !e.file_type().is_file() {
            continue;
        }
        let n = e.file_name().to_string_lossy().to_lowercase();
        if let Some(i) = want.iter().position(|w| *w == n) {
            found.push((i, e.path().to_path_buf()));
        }
    }
    found.sort_by_key(|(i, p)| (p.to_string_lossy().to_lowercase().contains("launcher"), *i));
    found.into_iter().next().map(|(_, p)| p)
}

/// Emplacements trouves par les couches de connaissance -> emplacements et indices.
fn apply_found(it: &mut Item, found: Vec<Found>, install: &Path) {
    let found: Vec<Found> = found.into_iter().filter(|f| !under(install, &f.path) || f.layer == Layer::Launcher).take(3).collect();
    let Some(best) = found.first() else { return };
    it.signs.push(match best.layer {
        Layer::Home => Sign::HomeBase,
        Layer::Launcher => Sign::LauncherCloud,
        _ if best.confidence >= 80 => Sign::DeducedStrong,
        _ => Sign::Deduced,
    });
    let kinds: HashSet<knowledge::Kind> = found.iter().map(|f| f.kind).collect();
    it.what = match (kinds.contains(&knowledge::Kind::Save), kinds.contains(&knowledge::Kind::Config)) {
        (true, true) => What::SavesAndSettings,
        (false, true) => What::Settings,
        _ => What::Saves,
    };
    it.locs = found.into_iter().map(|f| Loc::dir(f.path)).collect();
}

fn profile_locs(p: &Profile, vars: &Vars) -> Vec<Loc> {
    let mut v = vars.clone();
    for var in &p.variables {
        if let Some(c) = crate::profiles::var_candidates(var, &v).into_iter().next() {
            v.values.insert(var.name.clone(), c);
        }
    }
    let join = |x: &[String]| Some(x.join(";")).filter(|s| !s.is_empty());
    let mut out = Vec::new();
    for l in p.locations.iter().filter(|l| l.default_enabled) {
        let Some(path) = crate::profiles::expand(&l.path, &v) else { continue };
        let ok = if l.is_file { path.is_file() } else { path.is_dir() && has_any_file(&path) };
        if ok {
            out.push(Loc { path, is_file: l.is_file, label: l.label.clone(), filter: if l.is_file { None } else { join(&l.include) }, exclude: if l.is_file { None } else { join(&l.exclude) } });
        }
    }
    out
}

fn profile_item(r: &mut Run, p: &Profile) -> Option<Item> {
    let env = r.env;
    if !crate::profiles::detected(p, &env.vars) {
        return None;
    }
    let (cat, origin, what) = match p.category.as_str() {
        "game" => (Cat::Games, Origin::Profile, What::SavesAndSettings),
        "emulator" => (Cat::Games, Origin::Emulator, What::SavesAndSettings),
        _ => (Cat::Apps, Origin::Profile, What::Settings),
    };
    let mut it = base_item(cat, &p.name, what, origin);
    it.app = p.category != "game" && p.category != "emulator";
    it.signs.extend([Sign::Profile, Sign::ProfileInstalled]);
    // Logiciel dont le profil demande une relecture (ex. reglages pouvant contenir des cles) : propose decoche.
    if p.needs_ack() && !p.warnings.is_empty() {
        it.signs.push(Sign::ProfileWarning);
    }
    it.exe = p.detection.exes.first().cloned();
    it.locs = profile_locs(p, &env.vars);
    r.used_profiles.insert(p.id.clone());
    if it.locs.is_empty() {
        it.hide = Some(Hide::NoData);
    }
    Some(it)
}

/// Titre de la base dont les sauvegardes existent dans les dossiers perso (sans dossier d'installation connu).
fn base_folder_item(r: &mut Run, dg: &DbGame) -> Option<Item> {
    let env = r.env;
    let none = PathBuf::new();
    let locs = db_locs(dg, &env.folders, None, &[], None, &mut r.dirs, &none);
    if locs.is_empty() {
        return None;
    }
    // Un outil de developpement range ses fichiers dans des dossiers connus : jamais propose ici.
    if locs.iter().any(|l| crate::gamedetect::is_dev_dir(&format!("{}/", crate::gamedetect::norm_path(&l.path)))) {
        return None;
    }
    let mut names: Vec<String> = vec![dg.name.clone()];
    names.extend(dg.install_dirs.iter().cloned());
    let bases: Vec<String> = env.folders.bases().iter().map(|(_, p)| crate::gamedetect::norm_path(p)).collect();
    let best = locs
        .iter()
        .flat_map(|l| {
            let np = crate::gamedetect::norm_path(&l.path);
            let rel = bases.iter().filter(|b| np.starts_with(b.as_str())).max_by_key(|b| b.len()).map(|b| np[b.len()..].to_string()).unwrap_or_default();
            rel.split('/').filter(|c| !c.is_empty()).map(|c| knowledge::best_match(c, &names).unwrap_or(0)).collect::<Vec<u8>>()
        })
        .max()
        .unwrap_or(0);
    let mut it = base_item(Cat::Games, &dg.name, What::Saves, Origin::Base);
    it.signs.push(Sign::BaseFolder);
    if best >= 85 {
        it.signs.push(Sign::BaseFolderName);
    }
    it.locs = locs;
    Some(it)
}

// ------------------------------------------------------------------ etape 2 : logiciels

/// Lanceurs de jeux (masques : leurs jeux sont proposes a part).
const LAUNCHERS: &[&str] = &["steam", "epic games launcher", "gog galaxy", "ea app", "origin", "ubisoft connect", "uplay", "battle.net", "xbox", "itch", "heroic", "lutris", "playnite", "amazon games", "rockstar games launcher", "bottles"];
/// Navigateurs, messageries chiffrees et gestionnaires de mots de passe : jamais proposes.
const SENSITIVE_APPS: &[&str] = &["chrome", "chromium", "firefox", "microsoft edge", "brave", "opera", "vivaldi", "tor browser", "thunderbird", "keepass", "bitwarden", "1password", "lastpass", "dashlane", "proton pass", "enpass", "nordpass"];
/// Outils de developpement (noms, sous-chaines en minuscules).
const DEV_APPS: &[&str] = &[
    "git", "github", "gitkraken", "sourcetree", "tortoisegit", "tortoisesvn", "python", "node.js", "nodejs", "rust", "rustup", "cargo", "visual studio", "vs code", "jetbrains", "intellij", "pycharm", "webstorm",
    "rider", "clion", "goland", "rustrover", "android studio", "java", "jdk", "jre", "openjdk", "temurin", "zulu", "corretto", "maven", "gradle", "cmake", "llvm", "clang", "mingw", "msys2", "cygwin",
    "docker", "podman", "vagrant", "virtualbox", "vmware", "hyper-v", "wsl", "windows subsystem", "ubuntu", "debian", "postman", "insomnia", "wireshark", "putty", "winscp", "filezilla", "sdk",
    "build tools", "windows kits", "strawberry perl", "perl", "ruby", "php", "golang", "go programming", "dotnet", ".net sdk", "microsoft .net", "powershell", "terminal", "cursor", "sublime text", "notepad++ plugin", "unreal engine",
    "unity hub", "godot", "anaconda", "miniconda", "conda", "jupyter", "r for windows", "rstudio", "hashicorp", "terraform", "kubectl", "minikube", "nvm", "yarn", "pnpm", "bun", "deno", "sqlite", "mysql",
    "postgresql", "mongodb", "redis", "dbeaver", "heidisql",
];
/// Composants du systeme, pilotes et environnements d'execution.
const SYSTEM_APPS: &[&str] = &[
    "redistributable", "runtime", "driver", "pilote", "chipset", "firmware", "bios", "update", "hotfix", "security", "webview", "physx", "vulkan", "directx", "visual c++", "vc++", "microsoft .net", "framework",
    "codec", "realtek", "nvidia", "amd software", "radeon", "intel(r)", "intel ", "logitech g hub", "razer synapse", "corsair", "armoury", "asus", "msi center", "dolby", "waves maxx", "synaptics", "elan",
    "office 16 click", "click-to-run", "microsoft onedrive", "onedrive", "microsoft teams", "edge update", "windows sdk", "windows app runtime", "windows desktop runtime", "mise a jour", "updater",
    "uninstall", "helper", "service", "agent", "installer", "setup", "bonjour", "openal", "microsoft gameinput", "xna framework", "vcredist", "app installer", "pc health check", "windows pc health",
    "microsoft update health", "teams machine-wide", "autogamesoftwaresync",
];

fn word_in(hay: &str, w: &str) -> bool {
    // Mot entier pour les mots courts (« git » ne doit pas toucher « digital »).
    if w.len() > 5 || w.contains(' ') || w.contains('.') || w.contains('+') || w.contains('(') {
        return hay.contains(w);
    }
    hay.split(|c: char| !c.is_alphanumeric()).any(|t| t == w)
}

/// Classe un logiciel installe : masque (et pourquoi) ou None.
pub fn app_hide(e: &AppEntry) -> Option<Hide> {
    let n = e.name.to_lowercase();
    let p = e.publisher.as_deref().unwrap_or("").to_lowercase();
    if SENSITIVE_APPS.iter().any(|w| word_in(&n, w)) {
        return Some(Hide::Sensitive);
    }
    if LAUNCHERS.iter().any(|w| n == *w || n.starts_with(&format!("{} ", w)) || n.starts_with(&format!("{}:", w))) {
        return Some(Hide::Launcher);
    }
    let loc = e.location.as_deref().map(|l| format!("{}/", crate::gamedetect::norm_path(l))).unwrap_or_default();
    let exe = e.exe.as_deref().map(crate::gamedetect::norm_path).unwrap_or_default();
    if DEV_APPS.iter().any(|w| word_in(&n, w)) || crate::gamedetect::is_dev_publisher(&p) || crate::gamedetect::is_dev_dir(&loc) || crate::gamedetect::is_dev_dir(&exe) {
        return Some(Hide::DevTool);
    }
    if e.system || SYSTEM_APPS.iter().any(|w| word_in(&n, w)) || (p.contains("microsoft") && (n.starts_with("microsoft") || n.starts_with("windows"))) {
        return Some(Hide::System);
    }
    None
}

fn step_apps(r: &mut Run) {
    let env = r.env;
    for p in env.profiles.iter().filter(|p| p.category == "app" || p.category == "config") {
        if r.cancelled() {
            return;
        }
        if r.used_profiles.contains(&p.id) {
            continue;
        }
        if let Some(it) = profile_item(r, p) {
            r.push(it);
        }
    }
    r.frac(Cat::Apps, 0.1);
    let mut apps = (env.apps)();
    r.frac(Cat::Apps, 0.25);
    let mut seen: HashSet<String> = HashSet::new();
    apps.retain(|a| !a.name.trim().is_empty() && seen.insert(knowledge::norm(&a.name)));
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let profile_names: Vec<String> = env.profiles.iter().map(|p| knowledge::norm(&p.name)).collect();
    let n = apps.len().max(1);
    for (i, a) in apps.iter().enumerate() {
        if r.cancelled() {
            return;
        }
        r.frac(Cat::Apps, 0.25 + 0.74 * (i + 1) as f32 / n as f32);
        // Deja vu comme jeu d'un lanceur (meme dossier ou meme nom) ou couvert par un profil.
        let nn = knowledge::norm(&a.name);
        if r.names.contains(&format!("{:?}:{}", Cat::Games, crate::cloud::game_key(&a.name))) || r.install_dirs.iter().any(|d| a.location.as_deref().map(|l| under(d, l) || under(l, d)).unwrap_or(false)) {
            continue;
        }
        if profile_names.iter().any(|p| !p.is_empty() && (nn.starts_with(p.as_str()) || p.starts_with(nn.as_str()))) {
            continue;
        }
        let it = app_item(r, a);
        r.push(it);
    }
}

fn app_item(r: &mut Run, a: &AppEntry) -> Item {
    let env = r.env;
    // Un jeu installe hors lanceur (dans la base) : categorie Jeux.
    let dir_name = a.location.as_deref().and_then(|l| l.file_name()).map(|n| n.to_string_lossy().to_string());
    // Le dossier d'installation ne suffit que pour un editeur de jeux (« Everything » est aussi le nom d'un jeu).
    let game_pub = a.publisher.as_deref().map(crate::gamedetect::is_game_publisher).unwrap_or(false);
    let db_hit = env.db.find_by_name(&a.name).into_iter().next().or_else(|| dir_name.as_deref().filter(|_| game_pub).and_then(|d| env.db.find_by_install_dir(d).into_iter().next())).cloned();
    let game = (db_hit.is_some() || game_pub) && app_hide(a).is_none();
    let mut it = base_item(if game { Cat::Games } else { Cat::Apps }, &a.name, if game { What::Saves } else { What::Settings }, Origin::Installed);
    it.app = !game;
    it.signs.push(Sign::Installed);
    it.exe_path = a.exe.clone().filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(cfg!(not(windows))) && p.is_file());
    it.exe = it.exe_path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
    if let Some(h) = app_hide(a) {
        it.hide = Some(h);
        return it;
    }
    let install = a.location.clone().or_else(|| a.exe.as_deref().and_then(Path::parent).map(Path::to_path_buf)).unwrap_or_default();
    if let Some(dg) = &db_hit {
        let locs = db_locs(dg, &env.folders, a.location.as_deref(), &[], None, &mut r.dirs, &install);
        if !locs.is_empty() {
            it.signs.push(Sign::BaseKnown);
            it.locs = locs;
            return it;
        }
    }
    let mut prog = match &it.exe_path {
        Some(p) => Program::describe(Some(p.as_path()), &p.to_string_lossy(), &a.name, !game),
        None => Program { name: a.name.clone(), app: !game, ..Default::default() },
    };
    prog.title = Some(a.name.clone());
    if prog.publisher.is_none() {
        prog.publisher = a.publisher.clone();
    }
    if prog.install_dir.is_none() {
        prog.install_dir = a.location.clone();
    }
    apply_found(&mut it, (env.locate)(&prog), &install);
    if !game && it.what == What::Saves && !it.locs.is_empty() {
        it.what = What::Settings;
    }
    if it.locs.is_empty() {
        it.hide = Some(Hide::NoData);
    }
    it
}

// ------------------------------------------------------------------ etape 3 : dossiers perso

fn step_folders(r: &mut Run) {
    let env = r.env;
    let n = env.personal.len().max(1);
    let home = env.folders.profile.clone().or_else(|| env.folders.home.clone());
    for (i, (k, p)) in env.personal.iter().enumerate() {
        if r.cancelled() {
            return;
        }
        r.frac(Cat::Folders, (i + 1) as f32 / n as f32);
        if !p.is_dir() || home.as_deref().map(|h| same_path(h, p)).unwrap_or(false) {
            continue;
        }
        let mut it = base_item(Cat::Folders, personal_name(*k), What::Folder, Origin::Personal(*k));
        it.signs.push(if k.main() { Sign::PersonalMain } else { Sign::PersonalOther });
        let mut loc = Loc::dir(p.clone());
        if *k == Personal::Desktop {
            // Les raccourcis du bureau sont un element a part (Windows) : ils restent compris dans le dossier.
            loc.exclude = Some("desktop.ini".into());
        }
        it.locs = vec![loc];
        if !has_any_file(p) {
            it.hide = Some(Hide::Empty);
        }
        it.newest_ms = shallow_newest(p);
        r.push(it);
    }
}

/// Nom STABLE d'un dossier perso (cle du coffre, identique sur tous les appareils) ; le nom affiche est traduit.
pub fn personal_name(k: Personal) -> &'static str {
    match k {
        Personal::Documents => "Documents",
        Personal::Pictures => "Pictures",
        Personal::Desktop => "Desktop",
        Personal::Music => "Music",
        Personal::Videos => "Videos",
        Personal::Downloads => "Downloads",
    }
}

// ------------------------------------------------------------------ etape 4 : Windows (fichiers)

fn step_system(r: &mut Run) {
    let env = r.env;
    let n = env.system.len().max(1);
    for (i, (k, p, is_file, filter)) in env.system.iter().enumerate() {
        if r.cancelled() {
            return;
        }
        r.frac(Cat::System, (i + 1) as f32 / n as f32);
        let what = match k {
            SysItem::Wallpaper => What::File,
            SysItem::RegistryPrefs => What::Settings,
            _ => What::Shortcuts,
        };
        let mut it = base_item(Cat::System, sys_name(*k), what, Origin::System(*k));
        it.app = true;
        if *k == SysItem::RegistryPrefs {
            it.hide = Some(Hide::Later);
            r.push(it);
            continue;
        }
        it.signs.push(if *k == SysItem::Wallpaper { Sign::Wallpaper } else { Sign::Shortcuts });
        let exists = if *is_file { p.is_file() } else { p.is_dir() };
        if !exists {
            continue;
        }
        it.locs = vec![Loc { path: p.clone(), is_file: *is_file, label: String::new(), filter: filter.clone(), exclude: None }];
        let any = if *is_file { true } else { has_matching_file(p, filter.as_deref()) };
        if !any {
            it.hide = Some(Hide::Empty);
        }
        r.push(it);
    }
    // Reglages Windows (2.0.2) : emplacement principal = export local, puis fichiers. Toujours proposes decoches.
    for o in &env.prefs {
        if r.cancelled() {
            return;
        }
        let what = if o.kind.reads_registry() { What::Settings } else { What::File };
        let mut it = base_item(Cat::System, o.kind.stable(), what, Origin::System(SysItem::Pref(o.kind)));
        it.app = true;
        it.signs.push(Sign::Shortcuts);
        it.locs = o.locs.clone();
        r.push(it);
    }
}

/// Nom interne stable (cle) ; le nom affiche vient des textes de l'interface.
pub fn sys_name(k: SysItem) -> &'static str {
    match k {
        SysItem::Pref(p) => p.stable(),
        SysItem::DesktopShortcuts => "Desktop shortcuts",
        SysItem::StartMenu => "Start menu",
        SysItem::Taskbar => "Taskbar pins",
        SysItem::Wallpaper => "Wallpaper",
        SysItem::RegistryPrefs => "Windows preferences",
    }
}

// ================================================================== tailles (lecture paresseuse)

/// Plafonds de mesure d'un element.
pub const SIZE_MAX_FILES: u64 = 300_000;
pub const SIZE_MAX_SECS: u64 = 6;

/// Mesure les tailles des resultats proposables, recommandes d'abord, apres le scan. Annulable.
pub fn measure_sizes(h: &Handle, wake: &dyn Fn()) {
    let p = h.view();
    if p.cancelled {
        return;
    }
    let mut order: Vec<&Item> = p.items.iter().filter(|i| i.tier != Tier::Hidden && i.already.is_none()).collect();
    order.sort_by_key(|i| (i.tier != Tier::Recommended, i.cat == Cat::Folders));
    for it in order {
        if h.cancelled() {
            return;
        }
        let s = measure(&it.locs, &h.cancel);
        if let Ok(mut st) = h.state.lock() {
            st.sizes.insert(it.key.clone(), s);
            if let Some(i) = st.items.iter_mut().find(|i| i.key == it.key) {
                apply_size_rule(i, &s);
            }
        }
        wake();
    }
    if let Ok(mut st) = h.state.lock() {
        st.sizes_done = true;
    }
    wake();
}

/// Un emplacement choisi par l'utilisateur pour un resultat masque « rien trouve » (sélecteur guidé) : la ligne passe dans
/// « Peut-être » avec sa case. Refuse un emplacement absent, sensible (liste noire) ou un resultat deja protege. La taille
/// est mesuree ensuite (`measure_one`). Retourne vrai si la ligne a change.
pub fn place_item(h: &Handle, key: &str, loc: Loc) -> bool {
    if !loc.path.exists() || loc_sensitive(&loc, &crate::picker::real_folders()) {
        return false;
    }
    let newest_ms = newest(&loc);
    let Ok(mut st) = h.state.lock() else { return false };
    let Some(it) = st.items.iter_mut().find(|i| i.key == key) else { return false };
    if it.already.is_some() || it.tier != Tier::Hidden || !matches!(it.hide, Some(Hide::NoData) | Some(Hide::Weak)) {
        return false;
    }
    it.locs = vec![loc];
    it.hide = None;
    it.signs.push(Sign::UserPlace);
    it.score = it.signs.iter().map(|s| s.points()).sum::<i32>().clamp(40, 69) as u8;
    it.tier = Tier::Maybe;
    it.newest_ms = newest_ms;
    let cat = it.cat;
    st.counts[cat.index()] += 1;
    true
}

/// Mesure la taille d'un resultat (apres `place_item`) et la range avec les autres tailles.
pub fn measure_one(h: &Handle, key: &str, wake: &dyn Fn()) {
    let locs = h.state.lock().ok().and_then(|st| st.items.iter().find(|i| i.key == key).map(|i| i.locs.clone())).unwrap_or_default();
    let s = measure(&locs, &h.cancel);
    if let Ok(mut st) = h.state.lock() {
        st.sizes.insert(key.to_string(), s);
        if let Some(i) = st.items.iter_mut().find(|i| i.key == key) {
            apply_size_rule(i, &s);
        }
    }
    wake();
}

/// Taille des emplacements (filtres et liste noire appliques), plafonnee.
pub fn measure(locs: &[Loc], cancel: &AtomicBool) -> Size {
    let t0 = std::time::Instant::now();
    let mut s = Size::default();
    for l in locs {
        if l.is_file {
            if let Ok(m) = std::fs::metadata(&l.path) {
                s.bytes += m.len();
                s.files += 1;
            }
            continue;
        }
        let inc: Vec<String> = split_patterns(l.filter.as_deref());
        let exc: Vec<String> = split_patterns(l.exclude.as_deref());
        let mut it = walkdir::WalkDir::new(&l.path).follow_links(false).into_iter();
        while let Some(e) = it.next() {
            let Ok(e) = e else { continue };
            if e.depth() == 0 {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            let rel = e.path().strip_prefix(&l.path).map(|r| r.to_string_lossy().replace('\\', "/").to_lowercase()).unwrap_or_default();
            if crate::sensitive::rel_is_sensitive(&rel) || exc.iter().any(|x| crate::store::glob_match(x, &name)) {
                if e.file_type().is_dir() {
                    it.skip_current_dir();
                }
                continue;
            }
            if !e.file_type().is_file() || (!inc.is_empty() && !inc.iter().any(|x| crate::store::glob_match(x, &name))) {
                continue;
            }
            s.files += 1;
            s.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            if s.files % 512 == 0 && (s.files >= SIZE_MAX_FILES || t0.elapsed().as_secs() >= SIZE_MAX_SECS || cancel.load(Ordering::SeqCst)) {
                s.partial = true;
                return s;
            }
        }
    }
    s
}

fn split_patterns(s: Option<&str>) -> Vec<String> {
    s.unwrap_or("").split(';').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()
}

// ================================================================== ajout en lot

/// Planification par defaut d'un element sans programme a surveiller : chaque jour a midi (+ rattrapage au demarrage).
pub fn default_schedule() -> crate::schedule::Schedule {
    crate::schedule::Schedule::fixed(vec![12 * 3600])
}

/// Element a ajouter : premier emplacement = principal, les autres = emplacements supplementaires (ids stables).
pub fn to_game(it: &Item, name: &str) -> Option<Game> {
    let first = it.locs.first()?;
    let mut g = Game {
        name: name.to_string(),
        path: first.path.clone(),
        exe: it.exe.clone().filter(|e| !e.trim().is_empty()),
        save_is_file: first.is_file,
        save_filter: first.filter.clone(),
        save_exclude: first.exclude.clone(),
        primary_label: Some(first.label.clone()).filter(|l| !l.is_empty()),
        ..Default::default()
    };
    for l in it.locs.iter().skip(1) {
        let label = if l.label.is_empty() { l.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "emplacement".into()) } else { l.label.clone() };
        let mut id = crate::profiles::location_id(&format!("scan-{}", crate::cloud::game_key(name)), &label);
        let mut n = 2;
        while id == crate::config::MAIN_LOCATION || g.extra_locations.iter().any(|x| x.id == id) {
            id = crate::profiles::location_id(&format!("scan-{}", crate::cloud::game_key(name)), &format!("{} {}", label, n));
            n += 1;
        }
        g.extra_locations.push(Location { id, label, path: l.path.clone(), is_file: l.is_file, filter: l.filter.clone(), exclude: l.exclude.clone(), enabled: true });
    }
    if g.exe.is_none() {
        g.schedule = Some(default_schedule());
    }
    Some(g)
}

/// Bilan d'un ajout en lot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddReport {
    /// Noms ajoutes (dans la configuration).
    pub added: Vec<String>,
    /// (nom, raison) des elements non ajoutes.
    pub skipped: Vec<(String, Skip)>,
    /// Emplacements ajoutes en plus du principal (elements a plusieurs emplacements).
    pub multi: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Deja protege (par cet element).
    Already(String),
    /// Recouvre un element deja protege ou un autre element de la selection.
    Overlap(String),
    /// Emplacement disparu depuis le scan.
    Missing,
    /// Erreur d'ecriture de la configuration.
    Error(String),
}

/// Ajoute les elements choisis (ordre donne) en une fois. Les noms deja pris recoivent un suffixe ; aucun element qui
/// recouvre un element existant n'est ajoute. Reglages : programme (icone), 3 versions pour un gros dossier. Donnees
/// d'usage : `item_added` (source scan) pour chaque element, seulement avec le consentement (`usage::record`).
pub fn add_items(ctx: &crate::Ctx, items: &[Item], sizes: &HashMap<String, Size>, shown: &dyn Fn(&Item) -> Option<String>) -> AddReport {
    add_items_from(ctx, items, sizes, shown, "scan")
}

/// Element « dossier » ajoute a la main (glisser-deposer ou choix d'un dossier, premier lancement) : copie chaque jour a
/// midi et tout de suite, comme un dossier perso du scan.
pub fn folder_item(path: &Path) -> Item {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| path.display().to_string());
    Item {
        key: format!("folder:{}", crate::cloud::game_key(&path.display().to_string())),
        cat: Cat::Folders,
        name,
        what: What::Folder,
        origin: Origin::Installed,
        locs: vec![Loc::dir(path.to_path_buf())],
        exe: None,
        exe_path: None,
        signs: vec![],
        score: 100,
        tier: Tier::Recommended,
        hide: None,
        newest_ms: None,
        already: None,
        seen: false,
        app: false,
    }
}

/// Comme `add_items`, avec la source de l'ajout pour les donnees d'usage (`scan`, `manual`, `drop`).
pub fn add_items_from(ctx: &crate::Ctx, items: &[Item], sizes: &HashMap<String, Size>, shown: &dyn Fn(&Item) -> Option<String>, source: &str) -> AddReport {
    add_items_with_keep(ctx, items, sizes, shown, source, &HashMap::new())
}

/// Comme `add_items_from`, avec le nombre de versions choisi par l'utilisateur pour certains elements (alerte poids : cle de
/// l'element -> versions) ; les autres suivent la regle par defaut (3 versions au-dela du seuil).
pub fn add_items_with_keep(ctx: &crate::Ctx, items: &[Item], sizes: &HashMap<String, Size>, shown: &dyn Fn(&Item) -> Option<String>, source: &str, keep: &HashMap<String, usize>) -> AddReport {
    let mut rep = AddReport::default();
    let mut cfg = match crate::config::Config::load(ctx) {
        Ok(c) => c,
        Err(e) => {
            rep.skipped.extend(items.iter().map(|i| (i.name.clone(), Skip::Error(e.to_string()))));
            return rep;
        }
    };
    let mut settings = crate::settings::Settings::load(ctx).unwrap_or_default();
    let mut new_games: Vec<(Game, &Item)> = Vec::new();
    for it in items {
        if let Some(a) = already_protected(&cfg.games, it) {
            rep.skipped.push((it.name.clone(), Skip::Already(a)));
            continue;
        }
        let base = it.name.clone();
        let mut name = base.clone();
        let mut n = 2;
        while cfg.find(&name).is_some() || crate::actions::config_has_key(&cfg, &name) {
            name = format!("{} {}", base, n);
            n += 1;
        }
        let Some(mut g) = to_game(it, &name) else {
            rep.skipped.push((it.name.clone(), Skip::Missing));
            continue;
        };
        // Reglages Windows : export local pret avant l'ajout ; element propre a cet appareil = dossier du coffre a part.
        if let Origin::System(SysItem::Pref(k)) = it.origin {
            let sys = crate::winprefs::sys_of(ctx);
            crate::winprefs::prepare(&ctx.data_dir, k, sys.as_deref());
            if k.device_specific() {
                g.dest_key = Some(crate::winprefs::device_dest(k, &ctx.machine));
            }
        }
        // Nom affiche (traduit) : la cle du coffre reste le nom stable.
        g.display_name = shown(it).filter(|s| !s.trim().is_empty() && *s != g.name);
        if !crate::store::Target::of(&g).exists() {
            rep.skipped.push((it.name.clone(), Skip::Missing));
            continue;
        }
        if let Some(o) = cfg.games.iter().find(|o| crate::actions::targets_overlap(o, &g)) {
            rep.skipped.push((it.name.clone(), Skip::Overlap(o.shown().to_string())));
            continue;
        }
        rep.multi += g.extra_locations.len();
        if let Some(p) = &it.exe_path {
            settings.game_mut(&name).exe_path = Some(p.clone());
        }
        let big = sizes.get(&it.key).copied().unwrap_or_else(|| if it.cat == Cat::Folders { measure(&it.locs, &AtomicBool::new(false)) } else { Size::default() });
        if let Some(k) = keep.get(&it.key) {
            settings.set_keep(&name, *k);
        } else if keep_for(it, big.bytes) == BIG_KEEP {
            settings.game_mut(&name).keep_versions = Some(BIG_KEEP);
        }
        // Element lourd deja signale a l'ajout : pas de toast « devenu lourd » plus tard.
        if crate::weight::is_heavy(big.bytes) && !settings.heavy_notified.contains(&name) {
            settings.heavy_notified.push(name.clone());
        }
        rep.added.push(name.clone());
        new_games.push((g.clone(), it));
        cfg.games.push(g);
    }
    if new_games.is_empty() {
        return rep;
    }
    if let Err(e) = cfg.save(ctx) {
        let names = std::mem::take(&mut rep.added);
        rep.skipped.extend(names.into_iter().map(|n| (n, Skip::Error(e.to_string()))));
        rep.multi = 0;
        return rep;
    }
    let _ = settings.save(ctx);
    for (g, it) in &new_games {
        crate::usage::record(&ctx.data_dir, "item_added", &[("source", source), ("category", it.cat.usage(it.app)), ("multi", if g.extra_locations.is_empty() { "no" } else { "yes" })]);
    }
    rep
}

/// Proprietes de l'evenement `scan_done` (tranches).
pub fn scan_done_props(duration_ms: u64, proposed: usize, accepted: usize) -> [(&'static str, &'static str); 3] {
    [("duration", crate::usage::bucket_duration(duration_ms / 1000)), ("proposed", crate::usage::bucket_count(proposed)), ("accepted", crate::usage::bucket_count(accepted))]
}

// ================================================================== outils

fn same_path(a: &Path, b: &Path) -> bool {
    crate::gamedetect::norm_path(a).trim_end_matches('/') == crate::gamedetect::norm_path(b).trim_end_matches('/')
}

/// `p` est-il `root` ou dessous ?
pub fn under(root: &Path, p: &Path) -> bool {
    let (r, x) = (crate::gamedetect::norm_path(root), crate::gamedetect::norm_path(p));
    let r = r.trim_end_matches('/');
    !r.is_empty() && (x.trim_end_matches('/') == r || x.starts_with(&format!("{}/", r)))
}

fn overlaps(a: &[Loc], b: &[Loc]) -> bool {
    a.iter().any(|x| b.iter().any(|y| under(&x.path, &y.path) || under(&y.path, &x.path)))
}

/// Emplacement sensible (liste noire) : gabarit generique ou chemin sous le dossier perso, jamais le nom d'utilisateur.
fn loc_sensitive(l: &Loc, f: &Folders) -> bool {
    let text = knowledge::paths::to_generic(&l.path, f).unwrap_or_else(|| {
        let np = crate::gamedetect::norm_path(&l.path);
        let home = f.profile.as_ref().or(f.home.as_ref()).map(|h| crate::gamedetect::norm_path(h)).unwrap_or_default();
        np.strip_prefix(home.trim_end_matches('/')).unwrap_or(&np).to_string()
    });
    crate::sensitive::text_is_sensitive(&text).is_some() || l.filter.as_deref().map(|x| crate::sensitive::text_is_sensitive(x).is_some()).unwrap_or(false)
}

/// Element existant qui protege deja TOUT ce que propose `it` (meme nom, ou chaque emplacement sous un emplacement actif
/// sans filtre / avec le meme filtre).
pub fn already_protected(games: &[Game], it: &Item) -> Option<String> {
    if it.locs.is_empty() {
        return None;
    }
    for g in games {
        let locs = g.enabled_locations();
        let covered = it.locs.iter().all(|l| {
            locs.iter().any(|e| {
                let same = same_path(&e.path, &l.path);
                if same {
                    return e.filter == l.filter || e.filter.as_deref().map(str::trim).unwrap_or("").is_empty();
                }
                !e.is_file && under(&e.path, &l.path) && e.filter.as_deref().map(str::trim).unwrap_or("").is_empty()
            })
        });
        if covered {
            return Some(g.shown().to_string());
        }
    }
    None
}

fn has_any_file(p: &Path) -> bool {
    if p.is_file() {
        return true;
    }
    walkdir::WalkDir::new(p).max_depth(4).follow_links(false).into_iter().filter_map(|e| e.ok()).take(3000).any(|e| e.file_type().is_file())
}

pub fn has_matching_file(p: &Path, filter: Option<&str>) -> bool {
    let pats = split_patterns(filter);
    walkdir::WalkDir::new(p).max_depth(3).follow_links(false).into_iter().filter_map(|e| e.ok()).take(3000).any(|e| {
        e.file_type().is_file() && (pats.is_empty() || pats.iter().any(|x| crate::store::glob_match(x, &e.file_name().to_string_lossy().to_lowercase())))
    })
}

fn newest(l: &Loc) -> Option<u64> {
    knowledge::paths::newest_file_ms(&l.path)
}

/// Fichier le plus recent d'un gros dossier, sans le parcourir en entier (2 niveaux, 2 000 entrees).
fn shallow_newest(p: &Path) -> Option<u64> {
    let ms = |m: std::fs::Metadata| m.modified().ok().map(crate::util::mtime_ms);
    walkdir::WalkDir::new(p).max_depth(2).follow_links(false).into_iter().filter_map(|e| e.ok()).take(2000).filter(|e| e.depth() > 0).filter_map(|e| e.metadata().ok().and_then(ms)).max()
}

// ------------------------------------------------------------------ base embarquee -> emplacements

/// Contenu des dossiers deja lus (une seule lecture par dossier pendant tout le scan).
#[derive(Default)]
pub struct DirCache {
    map: HashMap<PathBuf, Vec<(String, String, bool)>>,
}

impl DirCache {
    /// (nom en minuscules, nom reel, dossier ?) des entrees de `d`.
    fn list(&mut self, d: &Path) -> &[(String, String, bool)] {
        self.map.entry(d.to_path_buf()).or_insert_with(|| {
            std::fs::read_dir(d)
                .map(|rd| rd.filter_map(|e| e.ok()).take(5000).map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    (n.to_lowercase(), n, e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                }).collect())
                .unwrap_or_default()
        })
    }
}

/// Premiers dossiers jamais propres a un jeu precis.
fn noise_comp(c: &str) -> bool {
    crate::knowledge::deduce::NOISE_TOP.contains(&c.trim_start_matches('.').to_lowercase().as_str())
}

/// Emplacements EXISTANTS d'un titre de la base (gabarits `<winAppData>/...`) resolus sur les bases de CE PC.
/// `install` : dossier d'installation connu (`<base>`, `<root>`). Les fichiers sont ramenes a leur dossier avec un filtre.
#[allow(clippy::too_many_arguments)]
pub fn db_locs(dg: &DbGame, f: &Folders, install: Option<&Path>, steam_libs: &[PathBuf], store_id: Option<&str>, dc: &mut DirCache, install_dir: &Path) -> Vec<Loc> {
    let user = f.profile.as_ref().or(f.home.as_ref()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let game_dirs: Vec<String> = match install.and_then(|d| d.file_name()) {
        Some(n) => vec![n.to_string_lossy().to_string()],
        None => dg.install_dirs.clone(),
    };
    let mut out: Vec<Loc> = Vec::new();
    for tpl in &dg.paths {
        let t = tpl.replace('\\', "/");
        let Some(rest) = t.strip_prefix('<') else { continue };
        let Some((ph, tail)) = rest.split_once('>') else { continue };
        let bases: Vec<PathBuf> = match ph {
            "winAppData" => f.roaming.iter().cloned().collect(),
            "winLocalAppData" => f.local.iter().cloned().collect(),
            "winDocuments" => f.documents.iter().cloned().collect(),
            "home" => f.profile.iter().cloned().collect(),
            "base" => install.map(|p| vec![p.to_path_buf()]).unwrap_or_default(),
            "root" if install.is_some() => steam_libs.to_vec(),
            _ => vec![],
        };
        if bases.is_empty() {
            continue;
        }
        let mut comps: Vec<String> = Vec::new();
        let mut ok = true;
        for c in tail.split('/').filter(|c| !c.is_empty()) {
            let c = c.replace("<osUserName>", &user).replace("<storeUserId>", "*");
            let c = match store_id {
                Some(id) => c.replace("<storeGameId>", id),
                None => c,
            };
            if c.contains("<game>") {
                match game_dirs.first() {
                    Some(g) => comps.push(c.replace("<game>", g)),
                    None => ok = false,
                }
                continue;
            }
            if c.contains('<') || c.contains('>') || c == ".." {
                ok = false;
                break;
            }
            comps.push(c);
        }
        if !ok || comps.is_empty() {
            continue;
        }
        // Le premier dossier ne doit pas etre generique (« Microsoft », « Packages », caches...) hors installation.
        if install.is_none() && comps.first().map(|c| noise_comp(c)).unwrap_or(true) {
            continue;
        }
        for b in bases {
            for hit in walk(&b, &comps, dc) {
                let (path, is_file, filter) = hit;
                if crate::catalog::is_generic_root(&path) || same_path(&path, &b) || (!install_dir.as_os_str().is_empty() && same_path(&path, install_dir)) {
                    continue;
                }
                if !is_file && !has_any_file(&path) {
                    continue;
                }
                let loc = Loc { path, is_file, label: String::new(), filter, exclude: None };
                if out.iter().any(|o| o.path == loc.path && o.filter == loc.filter) {
                    continue;
                }
                out.push(loc);
            }
        }
    }
    // Un emplacement contenu dans un autre retenu est inutile.
    let all = out.clone();
    out.retain(|l| !all.iter().any(|o| o.path != l.path && !o.is_file && o.filter.is_none() && under(&o.path, &l.path)));
    out.truncate(4);
    out
}

/// Suit les composants depuis `base` (jokers `*` `?` acceptes) avec le cache. Dernier composant : dossier, fichier exact, ou
/// motif de fichiers (dossier parent + filtre).
fn walk(base: &Path, comps: &[String], dc: &mut DirCache) -> Vec<(PathBuf, bool, Option<String>)> {
    let mut cur: Vec<PathBuf> = vec![base.to_path_buf()];
    let mut out = Vec::new();
    for (i, c) in comps.iter().enumerate() {
        let last = i + 1 == comps.len();
        let cl = c.to_lowercase();
        let wild = cl.contains('*') || cl.contains('?');
        let mut next = Vec::new();
        for d in &cur {
            let entries = dc.list(d).to_vec();
            if wild {
                let hits: Vec<&(String, String, bool)> = entries.iter().filter(|(l, _, _)| crate::store::glob_match(&cl, l)).collect();
                if last && hits.iter().any(|h| !h.2) {
                    // motif de fichiers : le dossier courant, filtre par le motif
                    out.push((d.clone(), false, Some(c.clone())));
                    continue;
                }
                for h in hits.into_iter().filter(|h| h.2).take(8) {
                    next.push(d.join(&h.1));
                }
            } else if let Some(h) = entries.iter().find(|(l, _, _)| *l == cl) {
                let p = d.join(&h.1);
                if last && !h.2 {
                    out.push((p, true, None));
                } else if h.2 {
                    next.push(p);
                }
            }
        }
        if next.is_empty() {
            return out;
        }
        cur = next;
    }
    out.extend(cur.into_iter().map(|p| (p, false, None)));
    out
}

#[cfg(test)]
mod tests;
