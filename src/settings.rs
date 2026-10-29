//! Reglages de l'interface (settings.json) : theme, langue, notifications, reglages par jeu.
//! Les reglages par jeu respectent TOUJOURS le plancher de 2 versions (backup::MIN_KEEP).
use crate::backup::MIN_KEEP;
use crate::i18n::Lang;
use crate::{util, Ctx, Res};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

/// Que faire quand les deux PC ont modifie la sauvegarde separement (vrai conflit) ?
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ConflictMode {
    /// Prendre la version du cloud automatiquement (jeu ferme ; la version locale est d'abord conservee).
    Cloud,
    /// Ne rien resoudre seul : l'interface demande.
    Ask,
}

/// Mise a jour : installation automatique (defaut), question a chaque version, ou aucune verification.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    #[default]
    Auto,
    Ask,
    Never,
}

/// Regle de demarrage (tache planifiee a l'ouverture de session) : voir `startup`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct StartupRuleCfg {
    /// Identifiant stable : `agss`, `gdrive`, `onedrive`, `dropbox` ou `custom-<n>`.
    pub id: String,
    /// Nom affiche (outils personnalises).
    #[serde(default)]
    pub name: String,
    /// Programme (outils personnalises seulement ; les autres sont detectes).
    #[serde(default)]
    pub exe: Option<PathBuf>,
    /// Delai apres l'ouverture de session (secondes).
    pub delay_secs: u32,
    pub enabled: bool,
    /// Lancer le programme reduit, sans prendre le focus (demande transmise au programme, qu'il peut ignorer).
    #[serde(default = "yes")]
    pub minimized: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct GameSettings {
    /// Conflit : None = herite du reglage global.
    #[serde(default)]
    pub conflict: Option<ConflictMode>,
    /// Versions a garder pour ce jeu (None = reglage global). Toujours >= MIN_KEEP.
    #[serde(default)]
    pub keep_versions: Option<usize>,
    /// Delai de stabilite des fichiers apres fermeture (secondes).
    #[serde(default)]
    pub stable_secs: Option<u64>,
    /// Intervalle de resynchronisation (secondes).
    #[serde(default)]
    pub resync_secs: Option<u64>,
    /// Notification apres sauvegarde (None = reglage global).
    #[serde(default)]
    pub notify: Option<bool>,
    /// Chemin complet de l'exe (pour l'icone uniquement).
    #[serde(default)]
    pub exe_path: Option<PathBuf>,
    /// Mis en pause depuis l'accueil (menu « ... ») : la surveillance ne le copie plus jusqu'a la reprise (rien n'est supprime).
    #[serde(default, skip_serializing_if = "not_set")]
    pub paused: bool,
}

fn not_set(b: &bool) -> bool {
    !*b
}

impl Settings {
    /// Element mis en pause depuis l'accueil ?
    pub fn is_paused(&self, name: &str) -> bool {
        self.per_game.get(name).map(|g| g.paused).unwrap_or(false)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Settings {
    #[serde(default = "default_lang")]
    pub lang: Lang,
    #[serde(default = "default_theme")]
    pub theme: ThemeMode,
    #[serde(default = "default_accent")]
    pub accent: [u8; 3],
    #[serde(default)]
    /// Mode grand format force (reglage manuel "Toujours").
    pub big_mode: bool,
    /// Mode grand format automatique quand une manette est detectee ("Auto" si big_mode est faux ; "Jamais" sinon).
    #[serde(default = "yes")]
    pub big_auto: bool,
    #[serde(default = "yes")]
    pub notify_global: bool,
    /// Conflit : reglage global (defaut : prendre la version du cloud automatiquement).
    #[serde(default = "default_conflict")]
    pub conflict_mode: ConflictMode,
    /// Notification « conflit resolu » : ACTIVE par defaut meme si les autres notifications sont coupees.
    #[serde(default = "yes")]
    pub notify_conflict_resolved: bool,
    /// Premier lancement : le tutoriel a ete vu/ferme. Absent d'un settings.json deja existant => considere vu.
    #[serde(default = "yes")]
    pub tutorial_seen: bool,
    /// Etape du tutoriel en cours (0 = pas commence / langue) : permet de reprendre apres une fermeture.
    #[serde(default)]
    pub tutorial_step: u32,
    /// Regles de demarrage definies par l'utilisateur.
    #[serde(default)]
    pub startup_rules: Vec<StartupRuleCfg>,
    /// Demarre la surveillance avec l'application.
    #[serde(default = "yes")]
    pub auto_watch: bool,
    /// Reactivite de la surveillance (instantane par defaut ; anciens fichiers : instantane).
    #[serde(default)]
    pub reactivity: crate::reactive::Reactivity,
    /// Ancien interrupteur « verifier les mises a jour » : garde pour les anciens fichiers (voir `update_mode`).
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// Rappels de soutien (popup cafe) : activables, compteur de jeux ajoutes, derniere apparition, frequence reduite apres un clic.
    #[serde(default = "yes")]
    pub support_reminders: bool,
    #[serde(default)]
    pub support_added: u32,
    #[serde(default)]
    pub support_last_ms: u64,
    #[serde(default)]
    pub support_long: bool,
    /// Migration « Herite » -> cloud automatique faite (une seule fois).
    #[serde(default)]
    pub conflict_migrated: bool,
    /// Version dont l'alerte a ete ignoree.
    #[serde(default)]
    pub ignored_update: Option<String>,
    #[serde(default)]
    pub reactivity_custom: crate::reactive::ReactCustom,
    /// Manette : un settings.json deja existant sans ce champ (manette toujours active jusqu ici) => Toujours activee.
    #[serde(default = "pad_existing")]
    pub pad_mode: PadMode,
    /// Couleur choisie par l utilisateur pour chaque PC (nom de machine -> RVB) ; absent = couleur par defaut (hachage du nom).
    #[serde(default)]
    pub machine_colors: BTreeMap<String, [u8; 3]>,
    /// Intervalle (s) de l interrogation directe du dossier partage en mode Personnalise.
    #[serde(default = "default_poll_s")]
    pub poll_custom_s: u32,
    /// Detection automatique des jeux (propositions de synchro) : interrupteur global et mode.
    #[serde(default = "yes")]
    pub detect_enabled: bool,
    /// Animations des cartes de detection : Toujours (defaut), Auto (suit Windows) ou Reduites (fondu).
    #[serde(default)]
    pub card_anim: crate::gamedetect::CardAnim,
    /// Portee de la detection (tout / jeux seulement / applications seulement).
    #[serde(default)]
    pub detect_scope: crate::gamedetect::DetectScope,
    /// Mode expert : details techniques visibles (score de fiabilite de la detection sur les cartes) ; sinon infobulle.
    #[serde(default)]
    pub expert_mode: bool,
    /// Hauteur du journal (zone de defilement), en points.
    #[serde(default = "default_log_height")]
    pub log_height: f32,
    /// Hauteur de la liste des profils integres (persistee, comme le journal).
    #[serde(default = "default_prof_height")]
    pub prof_height: f32,
    #[serde(default)]
    pub per_game: BTreeMap<String, GameSettings>,
    /// Donnees d'usage : None = jamais repondu, Some(true) = « Oui, j'aide », Some(false) = « Non merci ».
    #[serde(default)]
    pub usage_consent: Option<bool>,
    /// Version (« majeure.mineure ») pour laquelle la question a ete posee : reposee a chaque mise a jour.
    #[serde(default)]
    pub usage_asked: Option<String>,
    /// Mode de mise a jour choisi (None = jamais choisi : deduit de `check_updates`, donc Automatique par defaut).
    #[serde(default)]
    pub update_mode: Option<UpdateMode>,
    /// Premier lancement 2.0 (assistant en 6 etapes) : etape courante, etapes faites / passees, termine (lot 10).
    #[serde(default)]
    pub onboarding: crate::onboarding::Onboarding,
    /// Alerte poids (2.0.1) : seuil en Mo au-dela duquel un element est « Lourd » (mode expert des Reglages). 500 par defaut.
    #[serde(default = "default_heavy_limit")]
    pub heavy_limit_mb: u32,
    /// Elements deja signales « devenus lourds » (un toast, une seule fois par element).
    #[serde(default)]
    pub heavy_notified: Vec<String>,
    /// Rappel unique de la question des statistiques (2.0.1) : 0 = rien, 1 = a montrer plus tard, 2 = montre (plus jamais).
    #[serde(default)]
    pub usage_late: u8,
    /// Moment (ms) ou le rappel a ete programme (il ne s'affiche pas dans la meme session, sauf apres 24 h).
    #[serde(default)]
    pub usage_late_ms: u64,
    /// Tri de l'accueil (2.0.3) : menu a gauche des filtres. Par defaut : etat (problemes d'abord), comme avant.
    #[serde(default)]
    pub home_sort: crate::home::SortKey,
    /// 2.0.3 : a la fermeture d'un programme pas encore protege, l'appli passe en plein ecran avec la proposition (sinon :
    /// notification seulement). Active par defaut.
    #[serde(default = "yes")]
    pub close_fullscreen: bool,
    /// Ancien masquage par nombre de la carte « jeux Playnite pas encore proteges » (2.0.3 initiale) : lu pour
    /// compatibilite, plus utilise.
    #[serde(default)]
    pub playnite_card_hidden: usize,
    /// Carte « jeux Playnite pas encore proteges » masquee jusqu'a cette date (ms UTC ; 30 jours apres « Masquer »).
    #[serde(default)]
    pub playnite_card_hidden_until_ms: u64,
}

/// Duree du masquage de la carte Playnite de l'accueil apres « Masquer ».
pub const PLAYNITE_CARD_HIDE_MS: u64 = 30 * 86_400_000;

/// Valeurs par defaut de la PREMIERE installation (aucun settings.json) : English, sombre, vert #028463.
pub const DEFAULT_ACCENT: [u8; 3] = [2, 132, 99];
fn default_lang() -> Lang {
    Lang::En
}
fn default_theme() -> ThemeMode {
    ThemeMode::Dark
}
fn default_accent() -> [u8; 3] {
    DEFAULT_ACCENT
}
fn default_conflict() -> ConflictMode {
    ConflictMode::Cloud
}
fn yes() -> bool {
    true
}
fn default_heavy_limit() -> u32 {
    crate::weight::DEFAULT_LIMIT_MB
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: default_lang(),
            theme: default_theme(),
            accent: default_accent(),
            big_mode: false,
            big_auto: true,
            notify_global: true,
            conflict_mode: default_conflict(),
            notify_conflict_resolved: true,
            tutorial_step: 0,
            tutorial_seen: false, // premiere installation : le choix de langue puis le tutoriel s'affichent
            startup_rules: vec![],
            auto_watch: true,
            reactivity: Default::default(),
            check_updates: true,
            support_reminders: true,
            support_added: 0,
            support_last_ms: 0,
            support_long: false,
            conflict_migrated: true, // nouvelle installation : rien a migrer (les anciens fichiers n'ont pas le champ -> false)
            ignored_update: None,
            reactivity_custom: Default::default(),
            per_game: BTreeMap::new(),
            pad_mode: PadMode::Ask, // nouvelle installation : on demande a la premiere manette detectee
            log_height: default_log_height(),
            prof_height: default_prof_height(),
            poll_custom_s: default_poll_s(),
            detect_enabled: true,
            card_anim: Default::default(),
            detect_scope: Default::default(),
            expert_mode: false,
            machine_colors: BTreeMap::new(),
            usage_consent: None,
            usage_asked: None,
            update_mode: None,
            onboarding: Default::default(),
            heavy_limit_mb: default_heavy_limit(),
            heavy_notified: Vec::new(),
            usage_late: 0,
            usage_late_ms: 0,
            home_sort: Default::default(),
            close_fullscreen: true,
            playnite_card_hidden: 0,
            playnite_card_hidden_until_ms: 0,
        }
    }
}

/// Mode manette : « Me demander » (detection passive + question), « Toujours activee », « Desactivee » (aucun peripherique ouvert).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum PadMode {
    #[default]
    Ask,
    Always,
    Disabled,
}

fn pad_existing() -> PadMode {
    PadMode::Always
}

pub const LOG_HEIGHT_MIN: f32 = 120.0;
pub const LOG_HEIGHT_MAX: f32 = 1600.0;

fn default_poll_s() -> u32 {
    5
}

fn default_log_height() -> f32 {
    320.0
}

fn default_prof_height() -> f32 {
    560.0
}

/// Choix utilisateur du mode grand format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BigChoice {
    Auto,
    Always,
    Never,
}

impl Settings {
    /// Mode de mise a jour effectif : le choix de l utilisateur ; sinon l ancien interrupteur (desactive => Jamais), sinon Automatique.
    pub fn update_mode(&self) -> UpdateMode {
        match self.update_mode {
            Some(m) => m,
            None if !self.check_updates => UpdateMode::Never,
            None => UpdateMode::Auto,
        }
    }

    /// Choisit le mode de mise a jour (garde `check_updates` coherent pour les anciens lecteurs).
    pub fn set_update_mode(&mut self, m: UpdateMode) {
        self.update_mode = Some(m);
        self.check_updates = m != UpdateMode::Never;
    }

    pub fn big_choice(&self) -> BigChoice {
        if self.big_mode {
            BigChoice::Always
        } else if self.big_auto {
            BigChoice::Auto
        } else {
            BigChoice::Never
        }
    }

    pub fn set_big_choice(&mut self, c: BigChoice) {
        self.big_mode = c == BigChoice::Always;
        self.big_auto = c == BigChoice::Auto;
    }
}

impl Settings {
    fn file(ctx: &Ctx) -> PathBuf {
        ctx.data_dir.join("settings.json")
    }

    pub fn load(ctx: &Ctx) -> Res<Settings> {
        Self::load_from(&ctx.data_dir)
    }

    pub fn load_from(dir: &std::path::Path) -> Res<Settings> {
        let f = dir.join("settings.json");
        if !f.exists() {
            return Ok(Settings::default());
        }
        let mut s: Settings = serde_json::from_str(&std::fs::read_to_string(&f)?).map_err(|e| format!("settings.json illisible : {}", e))?;
        s.sanitize();
        crate::legacy::fix_rule_ids(&mut s.startup_rules);
        s.migrate_conflicts();
        Ok(s)
    }

    /// « Herite » (anciens fichiers) devient « cloud automatique » pour les jeux ayant une fiche de reglages ; ceux qui
    /// etaient explicitement sur « Me demander » ne changent pas. Une seule fois.
    pub fn migrate_conflicts(&mut self) {
        if self.conflict_migrated {
            return;
        }
        for g in self.per_game.values_mut() {
            if g.conflict.is_none() {
                g.conflict = Some(ConflictMode::Cloud);
            }
        }
        self.conflict_migrated = true;
    }

    pub fn save(&self, ctx: &Ctx) -> Res<()> {
        util::write_atomic(&Self::file(ctx), serde_json::to_string_pretty(self)?.as_bytes())
    }

    /// Remet les valeurs dans les bornes (plancher de versions non contournable, delais > 0).
    pub fn sanitize(&mut self) {
        // Anciennes valeurs PAR DEFAUT figees dans la fiche d un jeu (300 s / 10 s) : elles masquaient la reactivite choisie.
        for g in self.per_game.values_mut() {
            if g.resync_secs == Some(300) {
                g.resync_secs = None;
            }
            if g.stable_secs == Some(10) {
                g.stable_secs = None;
            }
        }
        self.log_height = if self.log_height.is_finite() { self.log_height.clamp(LOG_HEIGHT_MIN, LOG_HEIGHT_MAX) } else { 320.0 };
        self.prof_height = if self.prof_height.is_finite() { self.prof_height.clamp(LOG_HEIGHT_MIN, LOG_HEIGHT_MAX) } else { 560.0 };
        self.heavy_limit_mb = crate::weight::clamp_limit(self.heavy_limit_mb);
        for g in self.per_game.values_mut() {
            if let Some(k) = g.keep_versions.as_mut() {
                *k = (*k).max(MIN_KEEP);
            }
            if let Some(s) = g.stable_secs.as_mut() {
                *s = (*s).max(1);
            }
            if let Some(s) = g.resync_secs.as_mut() {
                *s = (*s).max(10);
            }
        }
    }

    pub fn game_mut(&mut self, name: &str) -> &mut GameSettings {
        self.per_game.entry(name.to_string()).or_default()
    }

    /// Fixe le nombre de versions d'un jeu ; retourne la valeur reellement enregistree (>= MIN_KEEP).
    pub fn set_keep(&mut self, name: &str, n: usize) -> usize {
        let v = n.max(MIN_KEEP);
        self.game_mut(name).keep_versions = Some(v);
        v
    }

    /// Mode de resolution des conflits pour un jeu : reglage du jeu, sinon global.
    pub fn conflict_for(&self, name: &str) -> ConflictMode {
        self.per_game.get(name).and_then(|g| g.conflict).unwrap_or(self.conflict_mode)
    }

    /// « Restore default appearance » : theme, accent et langue par defaut (le reste est conserve).
    pub fn reset_appearance(&mut self) {
        self.theme = default_theme();
        self.accent = default_accent();
        self.lang = default_lang();
    }

    pub fn notify_for(&self, name: &str) -> bool {
        self.per_game.get(name).and_then(|g| g.notify).unwrap_or(self.notify_global)
    }

    /// Applique les reglages par jeu au contexte (versions a garder).
    pub fn apply_to_ctx(&self, ctx: &mut Ctx) {
        ctx.keep_overrides.clear();
        for (n, g) in &self.per_game {
            if let Some(k) = g.keep_versions {
                ctx.keep_overrides.insert(n.clone(), k.max(MIN_KEEP));
            }
        }
    }
}

/// Active / desactive la detection des jeux (SOURCE DE VERITE UNIQUE : `settings.json`, partagee par l'interrupteur des
/// Reglages et l'element du menu de la zone de notification). Retourne la ligne de journal.
pub fn set_detect_enabled(dir: &std::path::Path, enabled: bool, source: &str) -> crate::Res<String> {
    let mut s = Settings::load_from(dir)?;
    s.detect_enabled = enabled;
    crate::util::write_atomic(&dir.join("settings.json"), serde_json::to_string_pretty(&s)?.as_bytes())?;
    Ok(format!("[Detection] detection {} ({})", if enabled { "activee" } else { "desactivee" }, source))
}

/// Bascule la detection a partir de la valeur PERSISTEE (jamais d'un etat de menu memorise : aucune desynchronisation).
pub fn toggle_detect(dir: &std::path::Path, source: &str) -> crate::Res<(bool, String)> {
    let now = !Settings::load_from(dir)?.detect_enabled;
    let line = set_detect_enabled(dir, now, source)?;
    Ok((now, line))
}
