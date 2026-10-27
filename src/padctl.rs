//! Politique d'acces a la manette (logique pure, testee) : « Me demander » / « Toujours activee » / « Desactivee »,
//! liberation de la manette pendant un jeu ou en arriere-plan, question « Passer en mode manette ? ».
//!
//! Principe : la manette n'est OUVERTE (et ses touches lues) que lorsque l'utilisateur l'a voulu (reglage
//! « Toujours activee » ou reponse « Oui ») ET que la fenetre est au premier plan ET qu'aucun jeu surveille ne tourne.
use crate::gamepad::{Access, PadInfo};
use crate::settings::PadMode;
use std::collections::BTreeSet;

/// Pourquoi la manette est liberee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    GameRunning,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Reglage « Desactivee » : aucun peripherique n'est jamais ouvert ni liste.
    Disabled,
    /// « Me demander » sans accord : detection PASSIVE (presence seulement) quand `detecting`, sinon rien.
    Ask { detecting: bool },
    /// Manette ouverte et lue.
    Active,
    /// Mode manette voulu mais manette relachee (poignees fermees, plus aucune lecture).
    Released(Why),
}

/// Etat de l'environnement qui decide de la liberation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Env {
    pub game_running: bool,
    pub foreground: bool,
}

/// `session_yes` : l'utilisateur a repondu « Oui » dans cette session (mode « Me demander »).
pub fn phase(mode: PadMode, session_yes: bool, env: Env) -> Phase {
    let wants_open = match mode {
        PadMode::Disabled => return Phase::Disabled,
        PadMode::Always => true,
        PadMode::Ask => session_yes,
    };
    if !wants_open {
        return Phase::Ask { detecting: env.foreground && !env.game_running };
    }
    if env.game_running {
        Phase::Released(Why::GameRunning)
    } else if !env.foreground {
        Phase::Released(Why::Background)
    } else {
        Phase::Active
    }
}

/// Niveau d'acces demande aux sources de peripheriques.
pub fn access_of(p: Phase) -> Access {
    match p {
        Phase::Disabled | Phase::Released(_) | Phase::Ask { detecting: false } => Access::Off,
        Phase::Ask { detecting: true } => Access::Passive,
        Phase::Active => Access::Open,
    }
}

/// Ligne de journal (ASCII, comme le reste du journal) pour une transition, s'il y en a une.
pub fn journal_line(prev: Phase, next: Phase) -> Option<String> {
    match (prev, next) {
        (Phase::Active, Phase::Released(Why::GameRunning)) => Some("[Manette] liberee (en cours d'utilisation)".into()),
        (Phase::Active, Phase::Released(Why::Background)) => Some("[Manette] liberee (fenetre en arriere-plan)".into()),
        (Phase::Released(Why::Background), Phase::Released(Why::GameRunning)) => Some("[Manette] liberee (en cours d'utilisation)".into()),
        (Phase::Released(_), Phase::Active) => Some("[Manette] reprise".into()),
        _ => None,
    }
}

pub fn mode_to_u8(m: PadMode) -> u8 {
    match m {
        PadMode::Ask => 0,
        PadMode::Always => 1,
        PadMode::Disabled => 2,
    }
}

pub fn mode_from_u8(v: u8) -> PadMode {
    match v {
        1 => PadMode::Always,
        2 => PadMode::Disabled,
        _ => PadMode::Ask,
    }
}

// ------------------------------------------------------------------ question « mode manette ? »

/// Ce qui empeche d'afficher la question maintenant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PromptCtx {
    /// Fenetre cachee dans la zone de notification (ou non visible).
    pub hidden: bool,
    pub tutorial: bool,
    pub game_running: bool,
    /// Une autre boite de dialogue est deja ouverte.
    pub other_dialog: bool,
}

impl PromptCtx {
    pub fn blocked(&self) -> bool {
        self.hidden || self.tutorial || self.game_running || self.other_dialog
    }
}

/// Une question par connexion : une manette deja demandee n'est redemandee qu'apres deconnexion/reconnexion.
#[derive(Default, Debug)]
pub struct Prompter {
    handled: BTreeSet<String>,
}

impl Prompter {
    /// `present` : manettes actuellement detectees (passivement). Retourne la manette a propos de laquelle la
    /// question doit etre affichee maintenant (au plus une a la fois), sinon None (rien, ou reportee).
    pub fn on_presence(&mut self, present: &[PadInfo], c: &PromptCtx) -> Option<PadInfo> {
        self.handled.retain(|id| present.iter().any(|p| &p.id == id));
        if c.blocked() {
            return None; // reportee : la manette n'est PAS marquee comme traitee
        }
        let p = present.iter().find(|p| !self.handled.contains(&p.id))?;
        self.handled.insert(p.id.clone());
        Some(p.clone())
    }

    /// Reglage change a la main : on repart de zero.
    pub fn reset(&mut self) {
        self.handled.clear();
    }

    /// La manette a deja ete demandee pendant cette connexion ?
    pub fn is_handled(&self, id: &str) -> bool {
        self.handled.contains(id)
    }
}

/// Reponse a la question : (nouveau reglage persistant, accord pour la session).
/// Oui + case = « Toujours activee » ; Non + case = « Desactivee » ; sans case, le reglage reste « Me demander ».
pub fn apply_answer(yes: bool, remember: bool) -> (PadMode, bool) {
    match (yes, remember) {
        (true, true) => (PadMode::Always, true),
        (true, false) => (PadMode::Ask, true),
        (false, true) => (PadMode::Disabled, false),
        (false, false) => (PadMode::Ask, false),
    }
}

/// Etat de la manette pour la barre du bas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarState {
    /// « Manette : desactivee ».
    Disabled,
    /// Mode actif ou question en attente : l'affichage habituel (famille de la manette lue).
    Normal,
    /// Manette liberee : nom de la derniere manette connue.
    Released(String),
}

pub fn bar_state(p: Phase, last_name: Option<&str>) -> BarState {
    match p {
        Phase::Disabled => BarState::Disabled,
        Phase::Released(_) => BarState::Released(last_name.unwrap_or("").to_string()),
        _ => BarState::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::Connection;

    fn pad(id: &str) -> PadInfo {
        PadInfo { id: id.into(), name: "DualSense".into(), conn: Connection::Usb }
    }

    const FG: Env = Env { game_running: false, foreground: true };

    #[test]
    fn phases_selon_le_reglage() {
        assert_eq!(phase(PadMode::Disabled, true, FG), Phase::Disabled);
        assert_eq!(phase(PadMode::Always, false, FG), Phase::Active);
        assert_eq!(phase(PadMode::Ask, false, FG), Phase::Ask { detecting: true });
        assert_eq!(phase(PadMode::Ask, true, FG), Phase::Active);
        assert_eq!(access_of(Phase::Disabled), Access::Off);
        assert_eq!(access_of(Phase::Ask { detecting: true }), Access::Passive);
        assert_eq!(access_of(Phase::Active), Access::Open);
    }

    #[test]
    fn liberation_jeu_en_cours_et_arriere_plan() {
        let game = Env { game_running: true, foreground: true };
        let bg = Env { game_running: false, foreground: false };
        assert_eq!(phase(PadMode::Always, false, game), Phase::Released(Why::GameRunning));
        assert_eq!(phase(PadMode::Always, false, bg), Phase::Released(Why::Background));
        assert_eq!(access_of(Phase::Released(Why::GameRunning)), Access::Off);
        // Un jeu prime sur l'arriere-plan.
        assert_eq!(phase(PadMode::Always, false, Env { game_running: true, foreground: false }), Phase::Released(Why::GameRunning));
        // « Me demander » sans accord : aucune detection en arriere-plan ni pendant un jeu.
        assert_eq!(phase(PadMode::Ask, false, bg), Phase::Ask { detecting: false });
        assert_eq!(access_of(phase(PadMode::Ask, false, game)), Access::Off);
        // Le mode Desactivee ne change jamais.
        assert_eq!(phase(PadMode::Disabled, false, bg), Phase::Disabled);
    }

    #[test]
    fn journal_des_transitions() {
        assert_eq!(journal_line(Phase::Active, Phase::Released(Why::GameRunning)).unwrap(), "[Manette] liberee (en cours d'utilisation)");
        assert!(journal_line(Phase::Active, Phase::Released(Why::Background)).unwrap().contains("arriere-plan"));
        assert_eq!(journal_line(Phase::Released(Why::GameRunning), Phase::Active).unwrap(), "[Manette] reprise");
        assert!(journal_line(Phase::Disabled, Phase::Active).is_none());
        assert!(journal_line(Phase::Ask { detecting: true }, Phase::Active).is_none());
        assert!(journal_line(Phase::Active, Phase::Active).is_none());
    }

    #[test]
    fn une_question_par_connexion_et_report() {
        let mut p = Prompter::default();
        let ok = PromptCtx::default();
        let a = [pad("a")];
        // Fenetre cachee / tutoriel / jeu / autre dialogue : reporte, rien n'est marque comme traite.
        for c in [PromptCtx { hidden: true, ..ok }, PromptCtx { tutorial: true, ..ok }, PromptCtx { game_running: true, ..ok }, PromptCtx { other_dialog: true, ..ok }] {
            assert!(p.on_presence(&a, &c).is_none());
            assert!(!p.is_handled("a"));
        }
        assert_eq!(p.on_presence(&a, &ok).unwrap().id, "a");
        assert!(p.on_presence(&a, &ok).is_none(), "une seule fois par connexion");
        // Deconnexion puis reconnexion = nouvelle connexion.
        assert!(p.on_presence(&[], &ok).is_none());
        assert_eq!(p.on_presence(&a, &ok).unwrap().id, "a");
        // Une seconde manette est une nouvelle question.
        let ab = [pad("a"), pad("b")];
        assert_eq!(p.on_presence(&ab, &ok).unwrap().id, "b");
    }

    #[test]
    fn reponses_et_case_a_cocher() {
        assert_eq!(apply_answer(true, true), (PadMode::Always, true));
        assert_eq!(apply_answer(false, true), (PadMode::Disabled, false));
        assert_eq!(apply_answer(true, false), (PadMode::Ask, true));
        assert_eq!(apply_answer(false, false), (PadMode::Ask, false));
    }

    #[test]
    fn barre_du_bas() {
        assert_eq!(bar_state(Phase::Disabled, None), BarState::Disabled);
        assert_eq!(bar_state(Phase::Active, Some("DualSense")), BarState::Normal);
        assert_eq!(bar_state(Phase::Released(Why::GameRunning), Some("DualSense (USB)")), BarState::Released("DualSense (USB)".into()));
    }

    #[test]
    fn codage_du_reglage() {
        for m in [PadMode::Ask, PadMode::Always, PadMode::Disabled] {
            assert_eq!(mode_from_u8(mode_to_u8(m)), m);
        }
    }
}
