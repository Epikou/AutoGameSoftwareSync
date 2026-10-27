//! Reglages simple / expert (2.0, lot 11). Calcul PUR et teste : quelles cartes pour quel mode, registre de TOUTES les
//! options (aucune fonction perdue : tout ce qui existait en 1.x se retrouve en mode expert), classement des champs de
//! `settings.json`, recherche (mode expert seulement), cles de texte autorisees dans les ecrans du mode simple.
use crate::i18n::{tr, Lang, K};

/// Une carte de la page Reglages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Card {
    // ---- les 6 cartes du mode simple
    Look,
    Vault,
    Devices,
    Protect,
    Updates,
    Usage,
    // ---- cartes du mode expert seulement
    Conflicts,
    Watch,
    Config,
    Log,
}

/// Cartes du mode simple, dans l'ordre d'affichage.
pub const SIMPLE_CARDS: [Card; 6] = [Card::Look, Card::Vault, Card::Devices, Card::Protect, Card::Updates, Card::Usage];
/// Cartes qui n'apparaissent qu'en mode expert (apres les six autres).
pub const EXPERT_CARDS: [Card; 4] = [Card::Conflicts, Card::Watch, Card::Config, Card::Log];

impl Card {
    /// Carte reservee au mode expert ?
    pub fn expert_only(self) -> bool {
        EXPERT_CARDS.contains(&self)
    }

    /// Titre de la carte (le mode simple passe par `tr_mode` pour les cartes qui ont un equivalent technique).
    pub fn title(self) -> K {
        match self {
            Card::Look => K::VSetLook,
            Card::Vault => K::VVault,
            Card::Devices => K::VSetDevices,
            Card::Protect => K::VSetProtect,
            Card::Updates => K::SetCheckUpdates,
            Card::Usage => K::SetUsage,
            Card::Conflicts => K::SetConflictNotif,
            Card::Watch => K::SetWatch,
            Card::Config => K::SetImportExport,
            Card::Log => K::NavLog,
        }
    }

    /// Rang dans `EXPERT_CARDS` (etat replie / deplie des cartes expertes).
    pub fn expert_index(self) -> Option<usize> {
        EXPERT_CARDS.iter().position(|c| *c == self)
    }
}

/// Cartes affichees : les six du mode simple, puis (mode expert) les quatre cartes expertes. Le mode expert est un
/// SURENSEMBLE du mode simple : aucune carte ne disparait.
pub fn cards(expert: bool) -> Vec<Card> {
    let mut v: Vec<Card> = SIMPLE_CARDS.to_vec();
    if expert {
        v.extend(EXPERT_CARDS);
    }
    v
}

/// Une option de la page Reglages.
#[derive(Clone, Copy, Debug)]
pub struct Opt {
    /// Identifiant stable (tests, recherche).
    pub id: &'static str,
    pub card: Card,
    /// Libelle affiche (cle de texte) ; sert aussi a la recherche.
    pub label: K,
    /// Visible seulement en mode expert.
    pub expert_only: bool,
    /// Propre a Windows (n'existe pas sous Linux).
    pub windows_only: bool,
}

const fn o(id: &'static str, card: Card, label: K, expert_only: bool) -> Opt {
    Opt { id, card, label, expert_only, windows_only: false }
}

const fn ow(id: &'static str, card: Card, label: K, expert_only: bool) -> Opt {
    Opt { id, card, label, expert_only, windows_only: true }
}

/// TOUTES les options de la page Reglages (1.x + 2.0), avec la carte qui les porte.
pub const OPTIONS: &[Opt] = &[
    // Langue et apparence
    o("lang", Card::Look, K::SetLang, false),
    o("theme", Card::Look, K::SetTheme, false),
    o("accent_preset", Card::Look, K::SetAccent, false),
    o("accent_custom", Card::Look, K::SetAccent, true),
    o("restore_appearance", Card::Look, K::BtnRestoreAppearance, true),
    // Coffre
    o("vault", Card::Vault, K::VVault, false),
    o("vault_path", Card::Vault, K::SetDest, true),
    o("vault_size", Card::Vault, K::SetDestSize, true),
    o("heavy_limit", Card::Vault, K::VHeavyLimit, true),
    // Appareils
    o("device_colors", Card::Devices, K::VSetDevices, false),
    o("device_colors_all", Card::Devices, K::BtnDeviceReset, true),
    // Protection automatique
    o("detect_enabled", Card::Protect, K::VSuggestNew, false),
    o("scan", Card::Protect, K::VFindOnPc, false),
    o("startup_app", Card::Protect, K::VStartWithPc, false),
    o("card_anim", Card::Protect, K::CardAnimTitle, false),
    o("detect_scope", Card::Protect, K::DetScopeTitle, true),
    o("detect_ignored", Card::Protect, K::DetIgnoredTitle, true),
    ow("startup_background", Card::Protect, K::StartupBackground, true),
    ow("startup_advanced", Card::Protect, K::BtnAdvancedStartup, true),
    // Mises a jour
    o("update_mode", Card::Updates, K::SetCheckUpdates, false),
    // Donnees d'usage
    o("usage_toggle", Card::Usage, K::SetUsageToggle, false),
    o("usage_view", Card::Usage, K::BtnUsageView, false),
    o("usage_erase", Card::Usage, K::BtnUsageErase, false),
    o("usage_id", Card::Usage, K::UsageId, true),
    // Conflits et notifications
    o("conflict_mode", Card::Conflicts, K::SetConflict, true),
    o("notify_conflict", Card::Conflicts, K::SetNotifConflict, true),
    o("notify_global", Card::Conflicts, K::SetNotif, true),
    // Surveillance
    o("watch_buttons", Card::Watch, K::WatchStart, true),
    o("auto_watch", Card::Watch, K::SetAutoWatch, true),
    o("reactivity", Card::Watch, K::SetReact, true),
    o("big_mode", Card::Watch, K::SetBig, true),
    ow("pad_mode", Card::Watch, K::SetPadMode, true),
    // Configuration
    o("export", Card::Config, K::BtnExport, true),
    o("import", Card::Config, K::BtnImport, true),
    o("data_dir", Card::Config, K::BtnOpenDataDir, true),
    o("quit", Card::Config, K::TrayQuit, true),
    o("support", Card::Config, K::SetSupport, true),
    // Journal
    o("log", Card::Log, K::NavLog, true),
    // Hors cartes (toujours visibles)
    o("redo_setup", Card::Look, K::VRedoSetup, false),
    o("expert_mode", Card::Look, K::VExpertMode, false),
];

/// Les options visibles dans un mode.
pub fn options(expert: bool) -> impl Iterator<Item = &'static Opt> {
    OPTIONS.iter().filter(move |o| expert || !o.expert_only)
}

/// Comment un champ de `settings.json` est expose dans l'interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    /// Reglable depuis cette option du registre.
    Of(&'static str),
    /// Etat interne (compteurs, positions, memoire), jamais un reglage.
    Internal,
}

use Exposure::{Internal, Of};

/// Classement de CHAQUE champ de `Settings` (le test compare ce tableau aux champs reels : un nouveau champ doit etre
/// classe, donc ne peut pas rester sans interface par oubli).
pub const SETTING_FIELDS: &[(&str, Exposure)] = &[
    ("lang", Of("lang")),
    ("theme", Of("theme")),
    ("accent", Of("accent_preset")),
    ("big_mode", Of("big_mode")),
    ("big_auto", Of("big_mode")),
    ("notify_global", Of("notify_global")),
    ("conflict_mode", Of("conflict_mode")),
    ("notify_conflict_resolved", Of("notify_conflict")),
    ("tutorial_seen", Internal),
    ("tutorial_step", Internal),
    ("startup_rules", Of("startup_app")),
    ("auto_watch", Of("auto_watch")),
    ("reactivity", Of("reactivity")),
    ("check_updates", Of("update_mode")),
    ("support_reminders", Of("support")),
    ("support_added", Internal),
    ("support_last_ms", Internal),
    ("support_long", Internal),
    ("conflict_migrated", Internal),
    ("ignored_update", Internal),
    ("reactivity_custom", Of("reactivity")),
    ("pad_mode", Of("pad_mode")),
    ("machine_colors", Of("device_colors")),
    ("poll_custom_s", Of("reactivity")),
    ("detect_enabled", Of("detect_enabled")),
    ("card_anim", Of("card_anim")),
    ("detect_scope", Of("detect_scope")),
    ("expert_mode", Of("expert_mode")),
    ("log_height", Of("log")),
    ("prof_height", Internal),
    ("per_game", Internal),
    ("usage_consent", Of("usage_toggle")),
    ("usage_asked", Internal),
    ("update_mode", Of("update_mode")),
    ("onboarding", Of("redo_setup")),
    ("heavy_limit_mb", Of("heavy_limit")),
    ("heavy_notified", Internal),
    ("usage_late", Internal),
    ("usage_late_ms", Internal),
];

/// Cartes dont le titre ou une option (visible dans ce mode) contient `query` (sans tenir compte de la casse). Requete
/// vide : toutes les cartes du mode. La recherche n'existe qu'en mode expert, mais la fonction reste pure.
pub fn matching_cards(lang: Lang, expert: bool, query: &str) -> Vec<Card> {
    let q = query.trim().to_lowercase();
    let all = cards(expert);
    if q.is_empty() {
        return all;
    }
    let has = |k: K| tr(lang, k).to_lowercase().contains(&q);
    all.into_iter().filter(|c| has(c.title()) || options(expert).any(|o| o.card == *c && has(o.label))).collect()
}

/// Textes qui peuvent apparaitre dans les ecrans du mode simple (les 6 cartes et le pied de page). Verifie par test :
/// ni « exe », ni « cloud », ni vocabulaire technique, dans les 5 langues, sous Windows et sous Linux ; le code des
/// ecrans simples ne peut citer aucune autre cle.
pub const SIMPLE_SETTINGS_KEYS: &[K] = &[
    K::VRedoSetup, K::TipRedoSetup, K::VTagline, K::VTaglineSub, K::VSetLook, K::SetLang, K::SetTheme, K::ThemeSystem, K::ThemeDark, K::ThemeLight,
    K::SetAccent, K::VVault, K::VSetDevices, K::VSetDevicesHint, K::VSetDevicesNone, K::VThisDevice, K::BtnDeviceReset, K::VSetProtect, K::VSuggestNew,
    K::VSuggestHint, K::VStartWithPc, K::VStartHint, K::CardAnimTitle, K::CardAnimAlways, K::CardAnimAuto, K::CardAnimReduced, K::VFindOnPc, K::VScanPcHint,
    K::SetCheckUpdates, K::UpdModeAuto, K::UpdModeAsk, K::UpdModeNever, K::VUpdHintAuto, K::VUpdHintAsk, K::VUpdHintNever, K::VUpdVersion, K::UpdManualHint,
    K::SetUsage, K::SetUsageToggle, K::UsageWhat, K::UsageOff, K::BtnUsageView, K::BtnUsageErase, K::UsagePrivacyLink, K::VExpertMode, K::VSetExpertHint,
    K::VNavSettings, K::DestUnreachable, K::VChooseVault, K::VVaultChange, K::VVaultConnected, K::VVaultFree, K::VVaultNoneSet, K::VVaultOpen,
    K::VVaultSeeTip, K::VVaultStreamBody, K::VVaultStreamTag, K::VVaultStreamTitle, K::VVaultUnreachableDetail,
];

/// Textes qui peuvent apparaitre dans la page Ajouter et le formulaire d'ajout du mode simple (memes garanties).
pub const SIMPLE_ADD_KEYS: &[K] = &[
    K::VProtectSomething, K::VAddIntro, K::VFindOnPc, K::VScanPcHint, K::VAddTileProgram, K::VAddTileProgramHint, K::VAddTileTarget, K::VAddTileTargetHint,
    K::VAddDropHot, K::VAddFormProgram, K::VAddFormTarget, K::VAddChangeProgram, K::VAddWhereTitle, K::VAddWhereHint, K::VAddUseThis, K::VAddPlaceChosen,
    K::VAddChoosePlace, K::VAddTargetFolder, K::VAddTargetFile, K::VAddNameLabel, K::VAddNameHint, K::VAddDailyNote, K::VAddNeedName, K::VAddNeedPath,
    K::VAddedToast, K::VAddFailedToast, K::VAddExistingTitle, K::VAddExistingHint, K::VAddRejoin, K::VAddSimilar, K::VAddOverlapTitle, K::VAddOverlapBody,
    K::VProgramFilter, K::KnowFound, K::KnowConfidence, K::KnowHigh, K::KnowMedium, K::KnowLow, K::KnowSaves, K::KnowSettings, K::KnowSrcLauncher,
    K::KnowSrcDeduce, K::KnowSrcLearn, K::KnowSrcChecked, K::KnowSrcHome, K::AddRecognized, K::HintTitle, K::BtnLearn, K::KnowLearnArmed, K::BtnCancel,
    K::BtnOverlapContinue, K::BtnUseName, K::BtnKeepName, K::VProtectIt, K::TipJoin, K::VKnown, K::VWhy, K::VAddMyself, K::VSavedGames, K::VDevicesLabel,
    K::AddChange, K::BtnHintUse, K::BtnHintIgnore,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_cartes_en_simple_dix_en_expert() {
        assert_eq!(cards(false), SIMPLE_CARDS.to_vec());
        assert_eq!(cards(false).len(), 6);
        let e = cards(true);
        assert_eq!(e.len(), 10);
        assert_eq!(&e[..6], &SIMPLE_CARDS[..], "le mode expert garde les six cartes simples dans le meme ordre");
        assert!(cards(false).iter().all(|c| !c.expert_only()));
    }
}
