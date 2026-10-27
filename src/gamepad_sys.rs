//! Sources de manettes reelles (Windows) : XInput et HID direct (DualSense / DualShock 4).
//! Non testable sans materiel ; tout le decodage et la logique sont dans `gamepad` (teste).
use crate::gamepad::*;
use hidapi::{BusType, HidApi, HidDevice};
use std::collections::{HashMap, HashSet};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::UI::Input::XboxController::{XInputGetCapabilities, XInputGetState, XINPUT_CAPABILITIES, XINPUT_STATE};

/// Sources reelles. Une erreur d'initialisation HID n'empeche pas XInput de fonctionner.
pub fn real_sources(log: &dyn Fn(&str)) -> Vec<Box<dyn PadSource>> {
    let mut v: Vec<Box<dyn PadSource>> = vec![Box::new(XInputSource::new())];
    if let Some(s) = ScriptPadSource::from_env() {
        v.push(Box::new(s));
    }
    match HidSource::new() {
        Ok(h) => v.push(Box::new(h)),
        Err(e) => log(&format!("[Manette] ATTENTION lecture HID indisponible (DualSense/DualShock non lues) : {}", e)),
    }
    v
}

// ------------------------------------------------------------------ XInput (Xbox, Legion Go S en mode XInput...)

struct Slot {
    connected: bool,
    next_probe_ms: u64,
}

pub struct XInputSource {
    slots: [Slot; 4],
    active: bool,
    access: Access,
}

impl XInputSource {
    pub fn new() -> Self {
        XInputSource { slots: std::array::from_fn(|_| Slot { connected: false, next_probe_ms: 0 }), active: true, access: Access::Open }
    }
}

impl Default for XInputSource {
    fn default() -> Self {
        Self::new()
    }
}

impl PadSource for XInputSource {
    fn set_access(&mut self, a: Access) {
        self.access = a;
        for s in &mut self.slots {
            s.connected = false;
            s.next_probe_ms = 0;
        }
    }

    /// Presence passive : XInputGetCapabilities ne lit AUCUN bouton (simple etat de connexion de l emplacement).
    fn presence(&mut self, now_ms: u64) -> Vec<PadInfo> {
        let mut out = Vec::new();
        if self.access == Access::Off {
            return out;
        }
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if !slot.connected && now_ms < slot.next_probe_ms {
                continue;
            }
            let mut caps: XINPUT_CAPABILITIES = unsafe { std::mem::zeroed() };
            let r = unsafe { XInputGetCapabilities(i as u32, 0, &mut caps) };
            if r == ERROR_SUCCESS {
                slot.connected = true;
                let name = if i == 0 { "Xbox / XInput".to_string() } else { format!("Xbox / XInput #{}", i + 1) };
                out.push(PadInfo { id: format!("xinput:{}", i), name, conn: Connection::Unknown });
            } else {
                slot.connected = false;
                slot.next_probe_ms = now_ms + 2_000;
            }
        }
        out
    }

    fn set_active(&mut self, active: bool) {
        if active && !self.active {
            for s in &mut self.slots {
                s.next_probe_ms = 0; // fenetre revenue au premier plan : on re-sonde tout de suite
            }
        }
        self.active = active;
    }

    fn poll(&mut self, now_ms: u64) -> Vec<PadPoll> {
        if self.access != Access::Open {
            return Vec::new(); // jamais de lecture de touches hors du mode ouvert
        }
        let mut out = Vec::new();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            // Interroger un emplacement vide est lent sur certains pilotes : sondage espace.
            if !slot.connected && now_ms < slot.next_probe_ms {
                continue;
            }
            let mut st: XINPUT_STATE = unsafe { std::mem::zeroed() };
            let r = unsafe { XInputGetState(i as u32, &mut st) };
            if r == ERROR_SUCCESS {
                slot.connected = true;
                let g = &st.Gamepad;
                let name = if i == 0 { "Xbox / XInput".to_string() } else { format!("Xbox / XInput #{}", i + 1) };
                out.push(PadPoll {
                    info: PadInfo { id: format!("xinput:{}", i), name, conn: Connection::Unknown },
                    readable: true,
                    states: vec![decode_xinput(g.wButtons, g.sThumbLX, g.sThumbLY)],
                });
            } else {
                slot.connected = false;
                slot.next_probe_ms = now_ms + if self.active { 500 } else { 2_000 };
            }
        }
        out
    }
}

// ------------------------------------------------------------------ HID direct (Sony)

/// Une manette ouverte. Elle n'est PUBLIEE (visible dans l'interface) qu'une fois `confirmed` :
/// un noeud HID Bluetooth peut rester present et ouvrable alors que la manette est eteinte ; sans
/// confirmation (rapport recu, ou rapport de calibration lu avec succes) on afficherait a tort
/// « connectee ».
struct OpenPad {
    dev: HidDevice,
    model: HidModel,
    info: PadInfo,
    opened_ms: u64,
    last_report_ms: u64,
    streaming: bool,
    confirmed: bool,
}

/// Manette Sony vue lors d'une enumeration (retenue pour pouvoir la rouvrir sans re-enumerer).
#[derive(Clone)]
struct KnownPad {
    model: HidModel,
    info: PadInfo,
}

pub struct HidSource {
    api: Option<HidApi>,
    access: Access,
    open: HashMap<String, OpenPad>,
    /// Manettes presentes d'apres la derniere enumeration (ouvertes ou non).
    known: HashMap<String, KnownPad>,
    /// Chemins deja ouverts avec succes au moins une fois (un echec ulterieur = manette absente, pas « bloquee »).
    ever_opened: HashSet<String>,
    /// Manettes vues mais impossibles a ouvrir (jamais ouvertes avec succes).
    blocked: HashMap<String, PadInfo>,
    next_enum_ms: u64,
    next_retry_ms: u64,
    active: bool,
    /// Signature de la liste Raw Input (changement = peripherique branche/debranche).
    sig: Option<u64>,
    now_ms: u64,
    notes: Vec<String>,
    /// Derniere emission (ms) de chaque note : une meme note n'est pas repetee avant 60 s (journal non inonde).
    note_times: HashMap<String, u64>,
    buf: Vec<u8>,
}

/// Delai (ms) pour qu'une manette fraichement ouverte prouve qu'elle est la (premier rapport).
pub const HID_CONFIRM_MS: u64 = 1_500;

impl HidSource {
    pub fn new() -> Result<Self, String> {
        // Aucune ouverture ici : HidApi n est cree qu au premier passage en mode Open (voir ensure_api).
        Ok(HidSource {
            api: None,
            access: Access::Open,
            open: HashMap::new(),
            known: HashMap::new(),
            ever_opened: HashSet::new(),
            blocked: HashMap::new(),
            next_enum_ms: 0,
            next_retry_ms: 0,
            active: true,
            sig: raw_input_signature(),
            now_ms: 0,
            notes: Vec::new(),
            note_times: HashMap::new(),
            buf: vec![0u8; 1024],
        })
    }

    fn ensure_api(&mut self) -> bool {
        if self.api.is_none() {
            match HidApi::new() {
                Ok(a) => self.api = Some(a),
                Err(e) => {
                    self.note(format!("lecture HID indisponible : {}", e));
                    return false;
                }
            }
        }
        true
    }

    fn note(&mut self, s: String) {
        let now = self.now_ms;
        if self.note_times.get(&s).map(|t| now.saturating_sub(*t) >= 60_000).unwrap_or(true) {
            self.note_times.insert(s.clone(), now);
            self.notes.push(s);
        }
    }

    /// Enumeration complete : met a jour `known`, oublie les manettes disparues, tente d'ouvrir les autres.
    fn enumerate(&mut self) {
        if !self.ensure_api() {
            return;
        }
        let Some(api) = self.api.as_mut() else { return };
        let _ = api.refresh_devices();
        let mut present: HashSet<String> = HashSet::new();
        for d in api.device_list() {
            let Some((model, name)) = sony_model(d.vendor_id(), d.product_id()) else { continue };
            // Garde uniquement la collection "manette" (page 0x01, usage 0x05) quand elle est renseignee.
            if d.usage_page() != 0 && !(d.usage_page() == 0x01 && d.usage() == 0x05) {
                continue;
            }
            let path = d.path().to_string_lossy().into_owned();
            present.insert(path.clone());
            let mut conn = connection_from_path(&path);
            if conn == Connection::Unknown {
                conn = match d.bus_type() {
                    BusType::Usb => Connection::Usb,
                    BusType::Bluetooth => Connection::Bluetooth,
                    _ => Connection::Unknown,
                };
            }
            self.known.entry(path.clone()).or_insert_with(|| KnownPad { model, info: PadInfo { id: path.clone(), name: name.to_string(), conn } });
        }
        // Manettes disparues de la liste : on ferme et on oublie (un retour = nouvelle decouverte).
        self.known.retain(|k, _| present.contains(k));
        self.open.retain(|k, _| present.contains(k));
        self.blocked.retain(|k, _| present.contains(k));
        self.try_open_known();
    }

    /// Tente d'ouvrir chaque manette connue qui ne l'est pas (leger : un simple CreateFile par manette).
    fn try_open_known(&mut self) {
        let todo: Vec<(String, KnownPad)> = self.known.iter().filter(|(k, _)| !self.open.contains_key(*k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        for (path, k) in todo {
            let Ok(cpath) = std::ffi::CString::new(path.clone()) else { continue };
            let Some(api) = self.api.as_ref() else { return };
            match api.open_path(&cpath) {
                Ok(dev) => {
                    let _ = dev.set_blocking_mode(false);
                    // DualShock 4 Bluetooth : pas de rapport garanti au repos, on ne peut pas exiger de preuve.
                    let mut confirmed = k.model == HidModel::DualShock4 && k.info.conn == Connection::Bluetooth;
                    if k.model == HidModel::DualSense && k.info.conn == Connection::Bluetooth {
                        // Lire le rapport de calibration (0x05) fait passer la manette du mode "simple" au mode
                        // complet en Bluetooth ; reussir prouve aussi que la manette repond (un noeud perime echoue).
                        let mut f = [0u8; 64];
                        f[0] = 0x05;
                        if dev.get_feature_report(&mut f).is_ok() {
                            confirmed = true;
                        }
                    }
                    self.ever_opened.insert(path.clone());
                    self.blocked.remove(&path);
                    let ms = self.now_ms;
                    self.open.insert(path, OpenPad { dev, model: k.model, info: k.info, opened_ms: ms, last_report_ms: ms, streaming: false, confirmed });
                }
                Err(e) => {
                    if !self.ever_opened.contains(&path) {
                        self.blocked.insert(path.clone(), k.info.clone());
                    }
                    self.note(format!("ouverture de {} impossible ({})", k.info.display(), e));
                }
            }
        }
    }
}

impl PadSource for HidSource {
    fn set_access(&mut self, a: Access) {
        self.access = a;
        if a != Access::Open {
            // Fermeture de toutes les poignees et oubli : plus rien n est ouvert hors du mode Open.
            self.open.clear();
            self.known.clear();
            self.blocked.clear();
            self.api = None;
        }
        self.next_enum_ms = 0;
        self.next_retry_ms = 0;
    }

    /// Presence passive : liste Raw Input (aucune ouverture de peripherique, aucune lecture).
    fn presence(&mut self, _now_ms: u64) -> Vec<PadInfo> {
        if self.access == Access::Off {
            return Vec::new();
        }
        raw_input_pads()
    }

    fn set_active(&mut self, active: bool) {
        if active && !self.active {
            self.next_enum_ms = 0; // fenetre revenue au premier plan : recherche immediate
            self.next_retry_ms = 0;
        }
        self.active = active;
    }

    fn drain_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    fn poll(&mut self, now_ms: u64) -> Vec<PadPoll> {
        if self.access != Access::Open {
            return Vec::new();
        }
        self.now_ms = now_ms;
        // Branchement / debranchement : la liste Raw Input (peu couteuse) change => enumeration immediate.
        let sig = raw_input_signature();
        if sig != self.sig {
            self.sig = sig;
            self.next_enum_ms = 0;
        }
        if now_ms >= self.next_enum_ms {
            self.next_enum_ms = now_ms + hid_scan_interval_ms(self.active, !self.open.is_empty());
            self.enumerate();
        } else if now_ms >= self.next_retry_ms {
            // Une manette connue mais fermee (debranchee puis revenue, noeud conserve par Windows) est
            // rouverte SANS enumeration complete : c'est ce qui detecte rapidement un rebranchement.
            self.next_retry_ms = now_ms + hid_retry_interval_ms(self.active);
            if self.known.keys().any(|k| !self.open.contains_key(k)) {
                self.try_open_known();
            }
        }
        let mut out = Vec::new();
        let mut dead: Vec<(String, &'static str)> = Vec::new();
        for (path, o) in self.open.iter_mut() {
            let mut states = Vec::new();
            let mut failed = false;
            for _ in 0..64 {
                match o.dev.read(&mut self.buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        o.last_report_ms = now_ms;
                        o.confirmed = true;
                        o.streaming = is_streaming_report(o.info.conn, self.buf[0]);
                        // Rapports inconnus / tronques : ignores (jamais d'evenement invente).
                        if let Ok(s) = decode_hid_report(o.model, o.info.conn, &self.buf[..n]) {
                            states.push(s);
                        }
                    }
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                dead.push((path.clone(), "echec de lecture"));
            } else if stream_silent(o.streaming, o.last_report_ms, now_ms) {
                dead.push((path.clone(), "plus aucun rapport (silence)"));
            } else if !o.confirmed && now_ms.saturating_sub(o.opened_ms) > HID_CONFIRM_MS {
                dead.push((path.clone(), "ouverte mais aucun rapport (manette eteinte ?)"));
            } else if o.confirmed {
                out.push(PadPoll { info: o.info.clone(), readable: true, states });
            }
        }
        for (p, why) in dead {
            if let Some(o) = self.open.remove(&p) {
                // Le noeud reste dans `known` : il sera reessaye (pas d'attente d'un changement de liste).
                self.note(format!("{} ferme : {}", o.info.display(), why));
            }
        }
        for info in self.blocked.values() {
            out.push(PadPoll { info: info.clone(), readable: false, states: vec![] });
        }
        out
    }
}

/// Manettes Sony vues dans la liste Raw Input (noms de peripheriques : aucune ouverture, aucune lecture).
fn raw_input_pads() -> Vec<PadInfo> {
    use windows_sys::Win32::UI::Input::{GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICENAME, RIM_TYPEHID};
    let mut out: Vec<PadInfo> = Vec::new();
    unsafe {
        let size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        let mut n: u32 = 0;
        if GetRawInputDeviceList(std::ptr::null_mut(), &mut n, size) == u32::MAX {
            return out;
        }
        let mut list: Vec<RAWINPUTDEVICELIST> = vec![std::mem::zeroed(); n as usize + 8];
        let mut cap = list.len() as u32;
        let got = GetRawInputDeviceList(list.as_mut_ptr(), &mut cap, size);
        if got == u32::MAX {
            return out;
        }
        for d in &list[..got as usize] {
            if d.dwType != RIM_TYPEHID {
                continue;
            }
            let mut len: u32 = 0;
            if GetRawInputDeviceInfoW(d.hDevice, RIDI_DEVICENAME, std::ptr::null_mut(), &mut len) == u32::MAX || len == 0 {
                continue;
            }
            let mut buf: Vec<u16> = vec![0; len as usize + 1];
            let r = GetRawInputDeviceInfoW(d.hDevice, RIDI_DEVICENAME, buf.as_mut_ptr() as *mut _, &mut len);
            if r == u32::MAX {
                continue;
            }
            let name = String::from_utf16_lossy(&buf[..(r as usize).min(buf.len())]).trim_end_matches(' ').to_string();
            let Some((vid, pid)) = parse_vid_pid(&name) else { continue };
            let Some((_, label)) = sony_model(vid, pid) else { continue };
            let id = format!("hid:{:04x}:{:04x}", vid, pid);
            if out.iter().any(|p| p.id == id) {
                continue;
            }
            out.push(PadInfo { id, name: label.to_string(), conn: connection_from_path(&name) });
        }
    }
    out
}

/// Signature de la liste des peripheriques Raw Input (poignees) : change quand un peripherique HID
/// est branche ou debranche. Appel tres leger (aucune ouverture de peripherique).
fn raw_input_signature() -> Option<u64> {
    use windows_sys::Win32::UI::Input::{GetRawInputDeviceList, RAWINPUTDEVICELIST};
    unsafe {
        let size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        let mut n: u32 = 0;
        if GetRawInputDeviceList(std::ptr::null_mut(), &mut n, size) == u32::MAX {
            return None;
        }
        let mut list: Vec<RAWINPUTDEVICELIST> = vec![std::mem::zeroed(); n as usize + 8];
        let mut cap = list.len() as u32;
        let got = GetRawInputDeviceList(list.as_mut_ptr(), &mut cap, size);
        if got == u32::MAX {
            return None;
        }
        let mut h: u64 = 0xcbf29ce484222325;
        for d in &list[..got as usize] {
            h = (h ^ d.hDevice as usize as u64).wrapping_mul(0x100000001b3);
            h = (h ^ d.dwType as u64).wrapping_mul(0x100000001b3);
        }
        Some(h ^ got as u64)
    }
}

// ------------------------------------------------------------------ source scriptee (MODE DE TEST)

/// MODE DE TEST, actif seulement si la variable d'environnement `AGSS_FAKE_PAD` contient un chemin de
/// fichier : une manette virtuelle « Manette de test » est alors declaree, et chaque jeton ecrit dans ce
/// fichier (`up down left right a b lb rb`, separes par des espaces ou des retours a la ligne) est
/// consomme (le fichier est supprime) et converti en appui + relachement. Sert aux essais manuels et
/// aux captures d'ecran sans toucher a la vraie manette. Inoffensif sinon (aucune source creee).
pub struct ScriptPadSource {
    path: std::path::PathBuf,
    access: Access,
}

impl ScriptPadSource {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("AGSS_FAKE_PAD").filter(|p| !p.is_empty()).map(|p| ScriptPadSource { path: p.into(), access: Access::Open })
    }
}

/// Jeton de script -> etat « bouton enfonce » (None si inconnu).
pub fn script_token_state(tok: &str) -> Option<PadState> {
    let mut s = PadState::default();
    match tok.trim().to_lowercase().as_str() {
        "up" => s.up = true,
        "down" => s.down = true,
        "left" => s.left = true,
        "right" => s.right = true,
        "a" => s.confirm = true,
        "b" => s.back = true,
        "lb" => s.lb = true,
        "rb" => s.rb = true,
        "stickdown" => s.ly = -1.0,
        "stickup" => s.ly = 1.0,
        _ => return None,
    }
    Some(s)
}

impl PadSource for ScriptPadSource {
    fn set_access(&mut self, a: Access) {
        self.access = a;
    }

    fn presence(&mut self, _now_ms: u64) -> Vec<PadInfo> {
        if self.access == Access::Off {
            return Vec::new();
        }
        vec![PadInfo { id: "script:0".into(), name: "Manette de test".into(), conn: Connection::Unknown }]
    }

    fn poll(&mut self, _now_ms: u64) -> Vec<PadPoll> {
        if self.access != Access::Open {
            return Vec::new();
        }
        let mut states = vec![PadState::default()];
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            let _ = std::fs::remove_file(&self.path);
            for tok in text.split_whitespace() {
                if let Some(s) = script_token_state(tok) {
                    states.push(s);
                    states.push(PadState::default());
                }
            }
        }
        vec![PadPoll { info: PadInfo { id: "script:0".into(), name: "Manette de test".into(), conn: Connection::Unknown }, readable: true, states }]
    }
}
