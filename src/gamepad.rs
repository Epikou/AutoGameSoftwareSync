//! Couche d'entree manette, independante de l'interface.
//!
//! Architecture :
//! * `PadSource` (trait) : une source de manettes (XInput, HID Sony...). Les sources reelles sont
//!   dans `gamepad_sys` ; les tests utilisent des sources simulees.
//! * `PadState` : etat normalise d'une manette (croix directionnelle, stick gauche, A/B, LB/RB).
//! * `NavTracker` : transforme les etats successifs en evenements `Nav` (haut/bas/gauche/droite,
//!   valider, retour, page precedente/suivante), sur front montant uniquement.
//! * `Hub` : agrege les sources, suit connexions/deconnexions, supprime les doublons
//!   (ex. DualSense lu en HID ET expose par Steam comme manette XInput virtuelle).
//! * `decode_hid_report` : decodage PUR des rapports HID DualSense / DualShock 4 (USB et Bluetooth).
//! * `spawn` : thread d'arriere-plan (~100 Hz) qui alimente l'interface.
//!
//! Pourquoi pas gilrs : sous Windows, gilrs 0.11 passe par Windows.Gaming.Input ; une DualSense y est
//! un "RawGameController" sans table de correspondance des boutons (boutons inconnus => aucun
//! evenement utile), et l'entree n'est livree qu'a la fenetre au premier plan. Lecture HID directe
//! + XInput = comportement previsible, sans dependance lourde.
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

// ------------------------------------------------------------------ evenements normalises

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// A / Croix
    Confirm,
    /// B / Rond
    Back,
    /// LB / L1
    PagePrev,
    /// RB / R1
    PageNext,
}

const NAV_ORDER: [Nav; 8] = [Nav::Up, Nav::Down, Nav::Left, Nav::Right, Nav::Confirm, Nav::Back, Nav::PagePrev, Nav::PageNext];

/// Etat normalise d'une manette. `lx`/`ly` dans [-1, 1], `ly` positif = haut.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub confirm: bool,
    pub back: bool,
    pub lb: bool,
    pub rb: bool,
    pub lx: f32,
    pub ly: f32,
}

/// Seuil d'activation / de relachement du stick (hysteresis : pas de rebond autour du seuil).
pub const STICK_ON: f32 = 0.6;
pub const STICK_OFF: f32 = 0.35;

fn stick_dir(v: f32, cur: i8) -> i8 {
    if v >= STICK_ON {
        1
    } else if v <= -STICK_ON {
        -1
    } else if cur > 0 && v > STICK_OFF {
        1
    } else if cur < 0 && v < -STICK_OFF {
        -1
    } else {
        0
    }
}

/// Transforme une suite d'etats en evenements (fronts montants). Le PREMIER etat observe sert de
/// reference sans produire d'evenement (un bouton deja enfonce a la connexion ne declenche rien).
/// Delai avant la premiere repetition d'une direction maintenue (D-pad ou stick), puis rythme (ms).
pub const REPEAT_DELAY_MS: u64 = 400;
pub const REPEAT_EVERY_MS: u64 = 110;

#[derive(Default)]
pub struct NavTracker {
    prev: [bool; 8],
    stick: [i8; 2],
    primed: bool,
    /// Prochaine repetition (ms) des 4 directions maintenues.
    next_rep: [u64; 4],
}

impl NavTracker {
    /// Comme `update_at` sans horloge (les repetitions ne sont alors armees qu'a t = 0).
    pub fn update(&mut self, s: &PadState) -> Vec<Nav> {
        self.update_at(s, 0)
    }

    /// Repetitions dues a `now_ms` pour les directions encore maintenues (zone morte : memes seuils
    /// d'hysteresis que les fronts montants). Au plus une repetition par direction et par appel.
    pub fn repeats(&mut self, now_ms: u64) -> Vec<Nav> {
        let mut out = Vec::new();
        if !self.primed {
            return out;
        }
        for i in 0..4 {
            if self.prev[i] && now_ms >= self.next_rep[i] {
                out.push(NAV_ORDER[i]);
                self.next_rep[i] = now_ms + REPEAT_EVERY_MS;
            }
        }
        out
    }

    pub fn update_at(&mut self, s: &PadState, now_ms: u64) -> Vec<Nav> {
        self.stick = [stick_dir(s.lx, self.stick[0]), stick_dir(s.ly, self.stick[1])];
        let cur = [
            s.up || self.stick[1] > 0,
            s.down || self.stick[1] < 0,
            s.left || self.stick[0] < 0,
            s.right || self.stick[0] > 0,
            s.confirm,
            s.back,
            s.lb,
            s.rb,
        ];
        let mut out = Vec::new();
        if self.primed {
            for i in 0..8 {
                if cur[i] && !self.prev[i] {
                    out.push(NAV_ORDER[i]);
                    if i < 4 {
                        self.next_rep[i] = now_ms + REPEAT_DELAY_MS;
                    }
                }
            }
        }
        self.prev = cur;
        self.primed = true;
        out
    }
}

// ------------------------------------------------------------------ description des manettes

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Usb,
    Bluetooth,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadInfo {
    /// Identifiant stable pendant la connexion (chemin HID, ou "xinput:N").
    pub id: String,
    /// Nom d'affichage : "DualSense", "DualShock 4", "Xbox / XInput"...
    pub name: String,
    pub conn: Connection,
}

impl PadInfo {
    /// "DualSense (Bluetooth)". La connexion n'est indiquee que si elle est connue.
    pub fn display(&self) -> String {
        match self.conn {
            Connection::Usb => format!("{} (USB)", self.name),
            Connection::Bluetooth => format!("{} (Bluetooth)", self.name),
            Connection::Unknown => self.name.clone(),
        }
    }
}

/// Manette vue par le hub. `readable == false` : presente mais impossible a ouvrir
/// (typiquement : deja monopolisee par un autre logiciel, ex. Steam Input).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadStatus {
    pub info: PadInfo,
    pub readable: bool,
}

/// Resultat d'un cycle de lecture pour une manette.
pub struct PadPoll {
    pub info: PadInfo,
    pub readable: bool,
    /// Tous les etats observes depuis le dernier cycle, dans l'ordre (aucun appui rapide perdu).
    pub states: Vec<PadState>,
}

/// Niveau d acces aux peripheriques. `Off` : aucun peripherique n est ouvert ni liste ; `Passive` : presence seulement
/// (aucune ouverture, aucune lecture de touches) ; `Open` : ouverture et lecture (comportement historique).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Access {
    Off,
    Passive,
    #[default]
    Open,
}

pub trait PadSource {
    fn poll(&mut self, now_ms: u64) -> Vec<PadPoll>;
    /// Change le niveau d acces. En quittant `Open`, la source FERME ses poignees et oublie ses manettes.
    fn set_access(&mut self, _a: Access) {}
    /// Presence passive (mode `Passive`) : manettes branchees, sans ouvrir de peripherique ni lire de touches.
    fn presence(&mut self, _now_ms: u64) -> Vec<PadInfo> {
        Vec::new()
    }
    /// Indique si la fenetre de l'application est au premier plan : les sources espacent leurs
    /// recherches de nouveaux peripheriques (couteuses) quand ce n'est pas le cas.
    fn set_active(&mut self, _active: bool) {}
    /// Notes de diagnostic (ouverture, abandon, silence...) a ecrire dans le journal.
    fn drain_notes(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// Delai entre deux enumerations HID COMPLETES (ms) de securite. L'enumeration ouvre brievement chaque
/// peripherique HID du PC (~70 ms) : elle n'est normalement declenchee que par un changement detecte
/// (liste Raw Input, peu couteuse) ; ceci n'est qu'un filet de rattrapage.
pub fn hid_scan_interval_ms(window_active: bool, have_open_pad: bool) -> u64 {
    match (window_active, have_open_pad) {
        (true, _) => 10_000,
        (false, false) => 30_000,
        (false, true) => 60_000,
    }
}

/// Delai (ms) entre deux tentatives de reouverture d'une manette HID connue mais fermee.
pub fn hid_retry_interval_ms(window_active: bool) -> u64 {
    if window_active {
        1_000
    } else {
        3_000
    }
}

/// Silence (ms) au-dela duquel une manette HID qui diffuse en continu est consideree deconnectee.
pub const HID_SILENCE_MS: u64 = 2_000;

/// Une manette en mode "complet" (USB, ou rapports Bluetooth 0x31 / 0x11) envoie des rapports en
/// continu ; en mode Bluetooth "simple" elle n'en envoie qu'aux changements (silence normal).
pub fn is_streaming_report(conn: Connection, report_id: u8) -> bool {
    conn == Connection::Usb || report_id == 0x31 || report_id == 0x11
}

/// La manette (qui diffuse en continu) est-elle silencieuse depuis trop longtemps ?
pub fn stream_silent(streaming: bool, last_report_ms: u64, now_ms: u64) -> bool {
    streaming && now_ms.saturating_sub(last_report_ms) > HID_SILENCE_MS
}

// ------------------------------------------------------------------ decodage XInput (pur)

pub mod xinput_bits {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
}

fn axis_i16(v: i16) -> f32 {
    (v as f32 / 32767.0).clamp(-1.0, 1.0)
}

/// Etat normalise depuis un `XINPUT_GAMEPAD` (boutons + stick gauche).
pub fn decode_xinput(buttons: u16, lx: i16, ly: i16) -> PadState {
    use xinput_bits::*;
    PadState {
        up: buttons & DPAD_UP != 0,
        down: buttons & DPAD_DOWN != 0,
        left: buttons & DPAD_LEFT != 0,
        right: buttons & DPAD_RIGHT != 0,
        confirm: buttons & A != 0,
        back: buttons & B != 0,
        lb: buttons & LB != 0,
        rb: buttons & RB != 0,
        lx: axis_i16(lx),
        ly: axis_i16(ly),
    }
}

// ------------------------------------------------------------------ decodage HID Sony (pur)

pub const SONY_VENDOR: u16 = 0x054C;
pub const PID_DUALSENSE: u16 = 0x0CE6;
pub const PID_DUALSENSE_EDGE: u16 = 0x0DF2;
pub const PID_DS4_V1: u16 = 0x05C4;
pub const PID_DS4_V2: u16 = 0x09CC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidModel {
    DualSense,
    DualShock4,
}

/// Modele et nom d'affichage d'un couple vendor/product Sony reconnu.
pub fn sony_model(vendor: u16, product: u16) -> Option<(HidModel, &'static str)> {
    if vendor != SONY_VENDOR {
        return None;
    }
    match product {
        PID_DUALSENSE => Some((HidModel::DualSense, "DualSense")),
        PID_DUALSENSE_EDGE => Some((HidModel::DualSense, "DualSense Edge")),
        PID_DS4_V1 | PID_DS4_V2 => Some((HidModel::DualShock4, "DualShock 4")),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Rapport vide.
    Empty,
    /// Identifiant de rapport non gere (rapport de service, mode inconnu...).
    UnknownReport(u8),
    /// Rapport tronque : `need` octets au moins sont necessaires.
    TooShort { need: usize, got: usize },
}

/// Connexion deduite du chemin HID Windows. USB : `...vid_054c&pid_0ce6...` ; Bluetooth :
/// `...{00001124-0000-1000-8000-00805f9b34fb}_vid&0002054c_pid&0ce6...`.
pub fn connection_from_path(path: &str) -> Connection {
    let p = path.to_ascii_lowercase();
    if p.contains("00001124-0000-1000-8000-00805f9b34fb") || p.contains("vid&0002054c") || p.contains("bthenum") || p.contains("bthledevice") {
        Connection::Bluetooth
    } else if p.contains("vid_054c") {
        Connection::Usb
    } else {
        Connection::Unknown
    }
}

/// Disposition commune des 9 premiers octets utiles (apres le decalage `base`) :
/// DualSense : LX LY RX RY L2 R2 seq B0 B1 B2 ; DualShock 4 : LX LY RX RY B0 B1 B2 L2 R2.
#[derive(Clone, Copy)]
struct Layout {
    base: usize,
    /// index (relatif a `base`) de l'octet "boutons 0" (croix + Carre/Croix/Rond/Triangle).
    b0: usize,
}

const DS5_COMMON: usize = 7; // L2/R2/seq intercales avant les boutons
const DS4_COMMON: usize = 4;

fn decode_common(r: &[u8], l: Layout) -> Result<PadState, DecodeError> {
    let need = l.base + l.b0 + 2; // il faut B0 et B1
    if r.len() < need {
        return Err(DecodeError::TooShort { need, got: r.len() });
    }
    let stick = |b: u8| (b as f32 - 127.5) / 127.5;
    let b0 = r[l.base + l.b0];
    let b1 = r[l.base + l.b0 + 1];
    // Croix : valeur 0..7 dans le quartet bas (0 = haut, sens horaire), 8+ = neutre.
    let hat = b0 & 0x0F;
    let (up, right, down, left) = match hat {
        0 => (true, false, false, false),
        1 => (true, true, false, false),
        2 => (false, true, false, false),
        3 => (false, true, true, false),
        4 => (false, false, true, false),
        5 => (false, false, true, true),
        6 => (false, false, false, true),
        7 => (true, false, false, true),
        _ => (false, false, false, false),
    };
    Ok(PadState {
        up,
        down,
        left,
        right,
        confirm: b0 & 0x20 != 0, // Croix
        back: b0 & 0x40 != 0,    // Rond
        lb: b1 & 0x01 != 0,      // L1
        rb: b1 & 0x02 != 0,      // R1
        lx: stick(r[l.base]),
        ly: -stick(r[l.base + 1]), // octet 0 = haut
    })
}

/// Decode un rapport d'entree HID (octet 0 = identifiant de rapport, comme renvoye par hidapi).
///
/// Formats pris en charge (spec publique, cf. documentation Linux hid-playstation / hid-sony) :
/// * DualSense USB : rapport 0x01 (64 octets), donnees des l'octet 1.
/// * DualSense Bluetooth complet : rapport 0x31 (78 octets), donnees des l'octet 2.
/// * DualSense Bluetooth "simple" (avant bascule en mode complet) : rapport 0x01 court,
///   disposition DualShock 4.
/// * DualShock 4 USB : rapport 0x01 (64 octets) ; Bluetooth complet : 0x11 (donnees des l'octet 3) ;
///   Bluetooth simple : 0x01 court.
///
/// `conn` leve l'ambiguite du rapport 0x01 d'une DualSense (complet en USB, simple en Bluetooth) :
/// on ne se fie pas a la longueur, car Windows peut completer les rapports courts.
pub fn decode_hid_report(model: HidModel, conn: Connection, report: &[u8]) -> Result<PadState, DecodeError> {
    let Some(&id) = report.first() else { return Err(DecodeError::Empty) };
    let ds5 = Layout { base: 1, b0: DS5_COMMON };
    let ds4 = Layout { base: 1, b0: DS4_COMMON };
    match (model, id) {
        (HidModel::DualSense, 0x01) => {
            let full = match conn {
                Connection::Usb => true,
                Connection::Bluetooth => false,
                Connection::Unknown => report.len() >= 32,
            };
            decode_common(report, if full { ds5 } else { ds4 })
        }
        (HidModel::DualSense, 0x31) => decode_common(report, Layout { base: 2, ..ds5 }),
        (HidModel::DualShock4, 0x01) => decode_common(report, ds4),
        (HidModel::DualShock4, 0x11) => decode_common(report, Layout { base: 3, ..ds4 }),
        (_, other) => Err(DecodeError::UnknownReport(other)),
    }
}

// ------------------------------------------------------------------ hub

/// Deux manettes distinctes qui produisent la meme action dans cette fenetre (ms) = doublon.
pub const DEDUP_MS: u64 = 80;

#[derive(Clone, Debug, PartialEq)]
pub enum PadChange {
    Connected(PadStatus),
    Disconnected(PadInfo),
}

#[derive(Default)]
pub struct TickOut {
    pub navs: Vec<Nav>,
    pub changes: Vec<PadChange>,
    pub notes: Vec<String>,
    /// Mode passif : liste complete des manettes presentes a ce cycle (None dans les autres modes).
    pub presence: Option<Vec<PadInfo>>,
}

#[derive(Default)]
pub struct Hub {
    sources: Vec<Box<dyn PadSource>>,
    trackers: HashMap<String, NavTracker>,
    known: BTreeMap<String, PadStatus>,
    recent: VecDeque<(Nav, u64, String)>,
    access: Access,
}

impl Hub {
    pub fn new(sources: Vec<Box<dyn PadSource>>) -> Hub {
        Hub { sources, ..Default::default() }
    }

    /// Un cycle : lit les sources, produit les evenements, detecte connexions/deconnexions.
    pub fn tick(&mut self, now_ms: u64) -> TickOut {
        let mut out = TickOut::default();
        match self.access {
            Access::Off => return out,
            Access::Passive => {
                let mut present: BTreeMap<String, PadInfo> = BTreeMap::new();
                for s in &mut self.sources {
                    for i in s.presence(now_ms) {
                        present.insert(i.id.clone(), i);
                    }
                    out.notes.extend(s.drain_notes());
                }
                out.presence = Some(present.into_values().collect());
                return out;
            }
            Access::Open => {}
        }
        let mut seen: Vec<String> = Vec::new();
        let mut polls: Vec<PadPoll> = Vec::new();
        for s in &mut self.sources {
            polls.extend(s.poll(now_ms));
            out.notes.extend(s.drain_notes());
        }
        while self.recent.front().map(|(_, t, _)| now_ms.saturating_sub(*t) > 500).unwrap_or(false) {
            self.recent.pop_front();
        }
        for p in polls {
            seen.push(p.info.id.clone());
            let status = PadStatus { info: p.info.clone(), readable: p.readable };
            // Nouvelle manette, ou passage "illisible" -> "lisible" : on le signale.
            if self.known.get(&p.info.id).map(|old| old.readable != p.readable).unwrap_or(true) {
                out.changes.push(PadChange::Connected(status.clone()));
            }
            self.known.insert(p.info.id.clone(), status);
            if !p.readable {
                self.trackers.remove(&p.info.id);
                continue;
            }
            let tr = self.trackers.entry(p.info.id.clone()).or_default();
            let mut navs: Vec<Nav> = Vec::new();
            for st in &p.states {
                navs.extend(tr.update_at(st, now_ms));
            }
            // Repetition des directions maintenues (D-pad / stick), meme sans nouveau rapport.
            navs.extend(tr.repeats(now_ms));
            for nav in navs {
                let dup = self.recent.iter().any(|(n, t, id)| *n == nav && *id != p.info.id && now_ms.saturating_sub(*t) < DEDUP_MS);
                if !dup {
                    self.recent.push_back((nav, now_ms, p.info.id.clone()));
                    out.navs.push(nav);
                }
            }
        }
        let gone: Vec<String> = self.known.keys().filter(|k| !seen.contains(k)).cloned().collect();
        for id in gone {
            if let Some(s) = self.known.remove(&id) {
                out.changes.push(PadChange::Disconnected(s.info));
            }
            self.trackers.remove(&id);
        }
        out
    }

    pub fn status(&self) -> Vec<PadStatus> {
        self.known.values().cloned().collect()
    }

    /// Change le niveau d'acces des sources. Quitter `Open` ferme les poignees et vide l'etat (sans evenement de deconnexion).
    pub fn set_access(&mut self, a: Access) {
        if a == self.access {
            return;
        }
        for s in &mut self.sources {
            s.set_access(a);
        }
        if a != Access::Open {
            self.trackers.clear();
            self.known.clear();
            self.recent.clear();
        }
        self.access = a;
    }

    pub fn access(&self) -> Access {
        self.access
    }

    /// Transmet l'etat "fenetre au premier plan" aux sources.
    pub fn set_active(&mut self, active: bool) {
        for s in &mut self.sources {
            s.set_active(active);
        }
    }
}

/// Texte de la ligne « Manette détectée : ... » (sans le libelle). `none` = texte « Aucune »,
/// `unreadable` = texte « lecture impossible » (traduits par l'appelant).
pub fn status_line(pads: &[PadStatus], none: &str, unreadable: &str) -> String {
    if pads.is_empty() {
        return none.to_string();
    }
    pads.iter()
        .map(|p| if p.readable { p.info.display() } else { format!("{} - {}", p.info.display(), unreadable) })
        .collect::<Vec<_>>()
        .join(" ; ")
}

/// Faut-il afficher l'aide « Steam peut monopoliser la manette » ? Oui s'il n'y a aucune manette
/// utilisable (aucune detectee, ou toutes illisibles).
pub fn needs_steam_hint(pads: &[PadStatus]) -> bool {
    pads.iter().all(|p| !p.readable)
}

// ------------------------------------------------------------------ service (thread d'arriere-plan)

/// Reglages partages entre l'interface et le thread de lecture (decident de ce qui est ouvert).
pub struct PadShared {
    mode: AtomicU8,
    session_yes: AtomicBool,
    /// Un jeu surveille tourne (renseigne par la surveillance).
    pub game_running: Arc<AtomicBool>,
}

impl PadShared {
    pub fn new(mode: crate::settings::PadMode, game_running: Arc<AtomicBool>) -> Arc<PadShared> {
        Arc::new(PadShared { mode: AtomicU8::new(crate::padctl::mode_to_u8(mode)), session_yes: AtomicBool::new(false), game_running })
    }

    pub fn set_mode(&self, m: crate::settings::PadMode) {
        self.mode.store(crate::padctl::mode_to_u8(m), Ordering::SeqCst);
    }

    pub fn mode(&self) -> crate::settings::PadMode {
        crate::padctl::mode_from_u8(self.mode.load(Ordering::SeqCst))
    }

    pub fn set_session_yes(&self, v: bool) {
        self.session_yes.store(v, Ordering::SeqCst);
    }

    pub fn session_yes(&self) -> bool {
        self.session_yes.load(Ordering::SeqCst)
    }
}

pub struct GamepadService {
    /// Manettes actuellement OUVERTES et lues (mis a jour par le thread). Vide si desactivee, en attente, ou liberee.
    pub status: Arc<Mutex<Vec<PadStatus>>>,
    /// Manettes presentes mais non ouvertes : detection passive, ou derniere manette connue pendant la liberation.
    presence: Arc<Mutex<Vec<PadInfo>>>,
    phase: Arc<Mutex<crate::padctl::Phase>>,
    pub shared: Arc<PadShared>,
    rx: Receiver<Nav>,
    stop: Arc<AtomicBool>,
}

impl GamepadService {
    /// Evenements en attente (non bloquant).
    pub fn drain(&self) -> Vec<Nav> {
        self.rx.try_iter().collect()
    }

    pub fn status(&self) -> Vec<PadStatus> {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn presence(&self) -> Vec<PadInfo> {
        self.presence.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn phase(&self) -> crate::padctl::Phase {
        self.phase.lock().map(|p| *p).unwrap_or(crate::padctl::Phase::Disabled)
    }
}

impl Drop for GamepadService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Phrase de journal (niveau info) pour un changement. ASCII uniquement (comme le reste du journal).
pub fn change_log_line(c: &PadChange) -> String {
    match c {
        PadChange::Connected(s) if s.readable => format!("[Manette] connectee : {}", s.info.display()),
        PadChange::Connected(s) => format!("[Manette] detectee mais illisible (deja utilisee par un autre logiciel ?) : {}", s.info.display()),
        PadChange::Disconnected(i) => format!("[Manette] deconnectee : {}", i.display()),
    }
}

/// Pause entre deux cycles de lecture (ms) : rapide seulement quand l'interface est au premier plan
/// avec une manette (reactivite), lent sinon.
pub fn poll_interval_ms(have_pads: bool, window_active: bool) -> u64 {
    match (have_pads, window_active) {
        (false, _) => 250,
        (true, true) => 10,
        (true, false) => 500,
    }
}

/// Pause (ms) quand rien n'est ouvert : detection passive espacee, ou veille complete.
pub fn idle_interval_ms(a: Access) -> u64 {
    match a {
        Access::Passive => 750,
        _ => 500,
    }
}

/// Demarre le thread de lecture. `on_activity` est appele quand l'interface doit se rafraichir ;
/// `is_active` dit si la fenetre de l'application est au premier plan : hors premier plan la manette est LIBEREE
/// (aucune lecture) ; `log` recoit les lignes de journal (connexions, liberation, reprise).
pub fn spawn(shared: Arc<PadShared>, on_activity: impl Fn() + Send + 'static, is_active: impl Fn() -> bool + Send + 'static, log: impl Fn(&str) + Send + 'static) -> GamepadService {
    spawn_ctl(shared, |l| crate::gamepad_sys::real_sources(l), on_activity, is_active, log)
}

/// Comme `spawn_ctl` avec la politique « Toujours activee » (tests de reconnexion).
pub fn spawn_with<F>(make_sources: F, on_activity: impl Fn() + Send + 'static, is_active: impl Fn() -> bool + Send + 'static, log: impl Fn(&str) + Send + 'static) -> GamepadService
where
    F: FnOnce(&dyn Fn(&str)) -> Vec<Box<dyn PadSource>> + Send + 'static,
{
    spawn_ctl(PadShared::new(crate::settings::PadMode::Always, Arc::new(AtomicBool::new(false))), make_sources, on_activity, is_active, log)
}

/// Thread de lecture pilote par `PadShared` ; les sources sont construites DANS le thread (pas besoin d'etre `Send`)
/// et n'ouvrent RIEN a leur construction.
pub fn spawn_ctl<F>(shared: Arc<PadShared>, make_sources: F, on_activity: impl Fn() + Send + 'static, is_active: impl Fn() -> bool + Send + 'static, log: impl Fn(&str) + Send + 'static) -> GamepadService
where
    F: FnOnce(&dyn Fn(&str)) -> Vec<Box<dyn PadSource>> + Send + 'static,
{
    use crate::padctl::{self, Env, Phase};
    let status = Arc::new(Mutex::new(Vec::new()));
    let presence: Arc<Mutex<Vec<PadInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let phase = Arc::new(Mutex::new(Phase::Disabled));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = channel();
    let (st2, pr2, ph2, stop2, sh2) = (status.clone(), presence.clone(), phase.clone(), stop.clone(), shared.clone());
    // Si le thread ne peut pas demarrer, le service reste vide (aucune manette) : l'application continue.
    let _ = std::thread::Builder::new().name("agss-gamepad".into()).spawn(move || {
        let mut hub = Hub::new(make_sources(&log));
        hub.set_access(Access::Off); // rien n'est ouvert tant que la politique ne l'a pas decide
        let t0 = std::time::Instant::now();
        let mut have_pads = false;
        let mut was_active: Option<bool> = None;
        let mut cur = Phase::Disabled;
        let mut first = true;
        while !stop2.load(Ordering::SeqCst) {
            let active = is_active();
            let env = Env { game_running: sh2.game_running.load(Ordering::SeqCst), foreground: active };
            let next = padctl::phase(sh2.mode(), sh2.session_yes(), env);
            if next != cur || first {
                first = false;
                if let Some(line) = padctl::journal_line(cur, next) {
                    log(&line);
                }
                let leaving_open = hub.access() == Access::Open && padctl::access_of(next) != Access::Open;
                let last: Vec<PadInfo> = hub.status().into_iter().map(|s| s.info).collect();
                hub.set_access(padctl::access_of(next));
                if padctl::access_of(next) != Access::Open {
                    have_pads = false;
                    if let Ok(mut s) = st2.lock() {
                        s.clear();
                    }
                }
                if let Ok(mut p) = pr2.lock() {
                    if matches!(next, Phase::Disabled) {
                        p.clear();
                    } else if leaving_open && matches!(next, Phase::Released(_)) {
                        *p = last; // pour l'infobulle « DualSense (liberee) »
                    }
                }
                if let Ok(mut p) = ph2.lock() {
                    *p = next;
                }
                cur = next;
                on_activity();
            }
            if was_active != Some(active) {
                hub.set_active(active);
                was_active = Some(active);
            }
            let access = hub.access();
            if access != Access::Off {
                let out = hub.tick(t0.elapsed().as_millis() as u64);
                for n in &out.notes {
                    log(&format!("[Manette] diag : {}", n));
                }
                if let Some(list) = out.presence {
                    let changed = pr2.lock().map(|p| *p != list).unwrap_or(false);
                    if changed {
                        if let Ok(mut p) = pr2.lock() {
                            *p = list;
                        }
                        on_activity(); // la question « mode manette ? » peut devoir apparaitre
                    }
                }
                if !out.changes.is_empty() {
                    for c in &out.changes {
                        log(&change_log_line(c));
                    }
                    let cur_status = hub.status();
                    have_pads = !cur_status.is_empty();
                    if let Ok(mut s) = st2.lock() {
                        *s = cur_status;
                    }
                    on_activity(); // rafraichit la ligne de Reglages
                }
                if active && !out.navs.is_empty() {
                    for n in out.navs {
                        let _ = tx.send(n);
                    }
                    on_activity();
                }
            }
            // Sommeil fractionne : retour rapide quand la fenetre revient au premier plan ou que la politique change.
            let pause = if access == Access::Open { poll_interval_ms(have_pads, active) } else { idle_interval_ms(access) };
            let mut slept = 0;
            while slept < pause && !stop2.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(25.min(pause - slept)));
                slept += 25;
                let env2 = Env { game_running: sh2.game_running.load(Ordering::SeqCst), foreground: is_active() };
                if padctl::phase(sh2.mode(), sh2.session_yes(), env2) != cur {
                    break;
                }
                if have_pads && !active && is_active() {
                    break;
                }
            }
        }
    });
    GamepadService { status, presence, phase, shared, rx, stop }
}

/// Identifiants constructeur / produit lus dans un nom de peripherique Windows (`VID_054C&PID_0CE6` en USB,
/// `VID&0002054c_PID&0ce6` en Bluetooth). Aucune ouverture : sert a la detection passive.
pub fn parse_vid_pid(name: &str) -> Option<(u16, u16)> {
    let n = name.to_ascii_lowercase();
    let hex = |s: &str, len: usize| -> Option<u32> { s.get(..len).and_then(|h| u32::from_str_radix(h, 16).ok()) };
    let vid = if let Some(i) = n.find("vid_") {
        hex(&n[i + 4..], 4)? as u16
    } else if let Some(i) = n.find("vid&") {
        (hex(&n[i + 4..], 8)? & 0xffff) as u16
    } else {
        return None;
    };
    let pid = if let Some(i) = n.find("pid_") {
        hex(&n[i + 4..], 4)? as u16
    } else if let Some(i) = n.find("pid&") {
        hex(&n[i + 4..], 4)? as u16
    } else {
        return None;
    };
    Some((vid, pid))
}
