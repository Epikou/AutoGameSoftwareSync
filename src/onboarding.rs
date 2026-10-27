//! Premier lancement 2.0 (lot 10) : assistant de 6 etapes qui CONFIGURENT l'appli en avancant (1 langue + donnees d'usage,
//! 2 coffre, 3 appareil, 4 scan et protection, 5 ajouts libres, 6 reglages + premiere copie). Logique PURE et testee :
//! parcours, reprise apres fermeture, saut d'etapes (« Plus tard »), mise a jour depuis une ancienne version (ecran
//! « Quoi de neuf » + seulement les etapes non faites), garde « jamais par-dessus un jeu ». Ecran : `gui::wizard_ui`.
use serde::{Deserialize, Serialize};

/// Nombre d'etapes du parcours complet.
pub const STEPS: u8 = 6;
pub const WELCOME: u8 = 1;
pub const VAULT: u8 = 2;
pub const DEVICE: u8 = 3;
pub const SCAN: u8 = 4;
pub const EXTRAS: u8 = 5;
pub const FINISH: u8 = 6;
/// Ecran « Quoi de neuf » (mise a jour depuis une ancienne version), avant les etapes.
pub const WHATS_NEW: u8 = 0;
/// Toutes les etapes (bit n-1 = etape n).
pub const ALL: u8 = 0b11_1111;

/// Bit d'une etape (1..=6).
pub fn bit(step: u8) -> u8 {
    if (1..=STEPS).contains(&step) {
        1 << (step - 1)
    } else {
        0
    }
}

/// Etat persistant de l'assistant (`settings.json`, champ `onboarding`). Absent d'un ancien fichier : tout a zero, le
/// parcours est calcule au premier lancement de la 2.0 (`begin`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Onboarding {
    /// Ecran courant (reprise) : 0 = « Quoi de neuf », 1..=6 = etape.
    #[serde(default)]
    pub step: u8,
    /// Etapes du parcours (bits).
    #[serde(default)]
    pub plan: u8,
    /// Etapes faites (une action reelle a ete faite) et passees avec « Plus tard ».
    #[serde(default)]
    pub done: u8,
    #[serde(default)]
    pub later: u8,
    /// Parcours calcule.
    #[serde(default)]
    pub started: bool,
    /// Assistant termine : il ne s'affiche plus (sauf « Refaire la configuration »).
    #[serde(default)]
    pub finished: bool,
    /// Parcours de mise a jour : l'ecran « Quoi de neuf » vient en premier.
    #[serde(default)]
    pub whats_new: bool,
}

/// Ce que l'appli sait deja au moment de calculer le parcours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// Premiere installation (aucun ancien `settings.json`, ou tutoriel jamais vu).
    pub first_install: bool,
    pub has_vault: bool,
    pub has_items: bool,
    /// La question des donnees d'usage doit etre posee pour cette version. Depuis la 2.0.1 ce n'est plus une etape : elle est
    /// posee en bas de la DERNIERE etape (ou de « Quoi de neuf »), sans pre-selection (voir `usage::late_after_wizard`).
    pub consent_needed: bool,
}

/// Parcours : (etapes, ecran « Quoi de neuf »). Premiere installation sans rien : les 6 etapes. Mise a jour (ou ancien
/// utilisateur) : « Quoi de neuf » puis seulement ce qui manque (coffre, scan ; la premiere copie si l'un des deux manquait).
/// Les statistiques ne sont plus une etape : leur question est en bas de la derniere etape. Un utilisateur deja configure ne refait jamais tout.
pub fn plan_for(f: &Facts) -> (u8, bool) {
    if f.first_install && !f.has_vault && !f.has_items {
        return (ALL, false);
    }
    let mut p = 0;
    let _ = f.consent_needed;
    if !f.has_vault {
        p |= bit(VAULT);
    }
    if !f.has_items {
        p |= bit(SCAN);
    }
    if p & (bit(VAULT) | bit(SCAN)) != 0 {
        p |= bit(FINISH);
    }
    (p, true)
}

/// Etapes du parcours, dans l'ordre.
pub fn sequence(plan: u8) -> Vec<u8> {
    (1..=STEPS).filter(|s| plan & bit(*s) != 0).collect()
}

/// Premier ecran d'un parcours (None = rien a montrer).
pub fn first(plan: u8, whats_new: bool) -> Option<u8> {
    if whats_new {
        return Some(WHATS_NEW);
    }
    sequence(plan).first().copied()
}

/// Ecran suivant (None = fin du parcours).
pub fn next(plan: u8, cur: u8) -> Option<u8> {
    sequence(plan).into_iter().find(|s| *s > cur)
}

/// Ecran precedent (None = deja au debut).
pub fn prev(plan: u8, whats_new: bool, cur: u8) -> Option<u8> {
    if cur == WHATS_NEW {
        return None;
    }
    match sequence(plan).into_iter().filter(|s| *s < cur).last() {
        Some(s) => Some(s),
        None if whats_new => Some(WHATS_NEW),
        None => None,
    }
}

/// Position affichee « Etape k sur n » (None sur « Quoi de neuf »).
pub fn position(plan: u8, cur: u8) -> Option<(usize, usize)> {
    let seq = sequence(plan);
    seq.iter().position(|s| *s == cur).map(|i| (i + 1, seq.len()))
}

/// Calcule le parcours au premier lancement de la 2.0 (une seule fois) ; un parcours vide est termine d'office.
pub fn begin(o: &mut Onboarding, f: &Facts) {
    if o.started || o.finished {
        return;
    }
    let (plan, wn) = plan_for(f);
    o.plan = plan;
    o.whats_new = wn;
    o.started = true;
    match first(plan, wn) {
        Some(s) => o.step = s,
        None => o.finished = true,
    }
}

/// « Refaire la configuration » (Reglages) : les 6 etapes depuis le debut ; les choix actuels sont repris par les ecrans.
pub fn restart(o: &mut Onboarding) {
    *o = Onboarding { step: WELCOME, plan: ALL, done: 0, later: 0, started: true, finished: false, whats_new: false };
}

/// Bouton de bas de page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    /// « Continuer » : etape faite (si `did` : une action reelle a eu lieu), ecran suivant.
    Continue,
    /// « Plus tard » : etape passee sans rien perdre.
    Later,
    /// « Retour ».
    Back,
}

/// Resultat d'une navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    To(u8),
    Finished,
    /// « Retour » au premier ecran : rien ne bouge.
    Stay,
}

/// Applique un bouton : memorise l'etape faite / passee, avance ou recule. `did` : l'etape a vraiment configure quelque
/// chose (langue + choix des donnees d'usage, coffre choisi, elements proteges...).
pub fn navigate(o: &mut Onboarding, nav: Nav, did: bool) -> Moved {
    let cur = o.step;
    match nav {
        Nav::Back => match prev(o.plan, o.whats_new, cur) {
            Some(s) => {
                o.step = s;
                Moved::To(s)
            }
            None => Moved::Stay,
        },
        Nav::Continue | Nav::Later => {
            if (1..=STEPS).contains(&cur) {
                if nav == Nav::Continue && did {
                    o.done |= bit(cur);
                    o.later &= !bit(cur);
                } else {
                    o.later |= bit(cur);
                }
            }
            match next(o.plan, cur) {
                Some(s) => {
                    o.step = s;
                    Moved::To(s)
                }
                None => {
                    o.finished = true;
                    Moved::Finished
                }
            }
        }
    }
}

/// L'assistant doit-il etre propose (parcours commence et pas termine) ?
pub fn pending(o: &Onboarding) -> bool {
    o.started && !o.finished
}

/// Jamais par-dessus un jeu en cours : ni pendant un programme suivi, ni quand une autre appli est en plein ecran.
pub fn may_show(program_running: bool, other_fullscreen: bool) -> bool {
    !program_running && !other_fullscreen
}

/// Valeur `step` de l'evenement d'usage `onboarding_step` (liste blanche : « 1 » a « 6 » ; rien pour « Quoi de neuf »).
pub fn usage_step(step: u8) -> Option<&'static str> {
    ["1", "2", "3", "4", "5", "6"].get((step as usize).wrapping_sub(1)).copied()
}

/// Valeur `action` de l'evenement d'usage.
pub fn usage_action(nav: Nav) -> &'static str {
    match nav {
        Nav::Continue => "done",
        Nav::Later => "later",
        Nav::Back => "back",
    }
}

/// Etat de la premiere copie (etape 6) d'apres l'etat de chaque element. Pur.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyTally {
    pub total: usize,
    /// A l'abri (version a jour dans le coffre).
    pub safe: usize,
    /// En cours de copie.
    pub busy: usize,
    /// Impossible pour l'instant (programme ouvert, erreur, coffre injoignable) : se fera tout seul plus tard.
    pub waiting: usize,
}

impl CopyTally {
    /// Plus rien ne bouge : tout est copie, ou ce qui reste attend.
    pub fn settled(&self) -> bool {
        self.busy == 0 && self.safe + self.waiting >= self.total
    }
    pub fn all_safe(&self) -> bool {
        self.total > 0 && self.safe == self.total
    }
    /// Avancement (0..1) : elements a l'abri ou qui attendent.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            ((self.safe + self.waiting) as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

/// Etat d'un element pendant la premiere copie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyState {
    Safe,
    Busy,
    /// Demande, pas encore commencee.
    Queued,
    Waiting,
}

/// Compte les etats.
pub fn tally(states: &[CopyState]) -> CopyTally {
    let mut t = CopyTally { total: states.len(), ..Default::default() };
    for s in states {
        match s {
            CopyState::Safe => t.safe += 1,
            CopyState::Busy => t.busy += 1,
            CopyState::Waiting => t.waiting += 1,
            CopyState::Queued => {}
        }
    }
    t
}
