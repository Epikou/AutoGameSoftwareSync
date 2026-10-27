//! Manettes sous Linux / SteamOS : aucune lecture directe. Steam Input transforme deja les manettes en clavier et souris
//! pour les applications lancees depuis Steam (y compris les « jeux non-Steam ») ; la navigation clavier de l'interface
//! suffit. La pile directe (XInput / HID) est donc desactivee ; l'etat « gere par Steam Input » est affiche dans la barre.
//! Seule la source scriptee de test (`AGSS_FAKE_PAD`) reste disponible.
use crate::gamepad::*;

/// Vrai : la manette est geree par le systeme (Steam Input), pas par l'application.
pub const MANAGED_BY_SYSTEM: bool = true;

/// Aucune source materielle ; la source de test seulement si `AGSS_FAKE_PAD` est defini.
pub fn real_sources(_log: &dyn Fn(&str)) -> Vec<Box<dyn PadSource>> {
    let mut v: Vec<Box<dyn PadSource>> = Vec::new();
    if let Some(s) = ScriptPadSource::from_env() {
        v.push(Box::new(s));
    }
    v
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
