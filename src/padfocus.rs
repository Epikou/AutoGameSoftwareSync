//! Selection a la manette « maison » (utilisee SEULEMENT quand une manette est detectee ; clavier et
//! souris gardent la navigation native d'egui).
//!
//! Pourquoi : la navigation de focus native d'egui ne considere que des widgets visibles, perd le
//! focus quand un element apparait/disparait, et deplace le curseur texte dans un champ. Ici :
//! * chaque widget interactif s'enregistre (id + rectangle, y compris ceux sous le pli de la
//!   ScrollArea, qui sont mis en page) ;
//! * D-pad / stick deplacent la selection vers le voisin le plus proche dans la direction
//!   (`spatial_next`, fonction pure) ;
//! * il y a TOUJOURS un element selectionne (repli sur le plus proche de l'ancien rectangle) ;
//! * la selection est memorisee par contexte (onglet, dialogue) et restauree ;
//! * la selection est ensuite recopiee dans le focus egui (sauf champs de texte, ou « valider » entre
//!   en edition et « retour » en sort) : le cerclage et l'activation par Entree restent ceux d'egui ;
//! * toute nouvelle selection demande un defilement centre de la ScrollArea de la page.
use eframe::egui::{self, Context, Id, Key, Rect, Response};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadAction {
    Dir(Dir),
    /// A / Croix
    Confirm,
    /// B / Rond
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// Se declenche par Entree (bouton, case, ligne, liste deroulante...).
    Button,
    /// Champ de texte : selectionne sans focus egui ; « valider » entre en edition, « retour » en sort.
    Text,
    /// DragValue : comme un champ de texte, mais haut/bas modifient la valeur pendant l'edition.
    Drag,
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub id: Id,
    pub rect: Rect,
    pub kind: ItemKind,
    /// L'element est dans une ScrollArea (le defilement automatique ne vise que ceux-la).
    pub in_scroll: bool,
    pub modal: bool,
}

// ------------------------------------------------------------------ fonctions pures

/// Voisin le plus proche de `rects[cur]` dans la direction `dir` (None s'il n'y en a pas).
/// Les voisins ALIGNES (intervalles perpendiculaires qui se recouvrent) priment sur les decales ; sinon
/// score = distance le long de l'axe + 2 x ecart perpendiculaire (aligne : distance + 30 % de l'ecart).
/// Un candidat doit etre nettement
/// au-dela du centre courant (au moins la moitie de la plus petite taille) : les voisins de la meme
/// ligne ne comptent pas pour haut/bas.
pub fn spatial_next(rects: &[Rect], cur: usize, dir: Dir) -> Option<usize> {
    let c = *rects.get(cur)?;
    let cc = c.center();
    let mut best: Option<(f32, usize)> = None; // candidats alignes (intervalles perpendiculaires qui se recouvrent)
    let mut best_off: Option<(f32, usize)> = None; // candidats decales
    for (i, r) in rects.iter().enumerate() {
        if i == cur {
            continue;
        }
        let rc = r.center();
        let (along, perp, overlap, min_extent) = match dir {
            Dir::Right => (rc.x - cc.x, (rc.y - cc.y).abs(), c.y_range().intersects(r.y_range()), c.width().min(r.width())),
            Dir::Left => (cc.x - rc.x, (rc.y - cc.y).abs(), c.y_range().intersects(r.y_range()), c.width().min(r.width())),
            Dir::Down => (rc.y - cc.y, (rc.x - cc.x).abs(), c.x_range().intersects(r.x_range()), c.height().min(r.height())),
            Dir::Up => (cc.y - rc.y, (rc.x - cc.x).abs(), c.x_range().intersects(r.x_range()), c.height().min(r.height())),
        };
        if along <= (0.5 * min_extent).max(1.0) {
            continue;
        }
        let (slot, score) = if overlap { (&mut best, along + 0.3 * perp) } else { (&mut best_off, along + 2.0 * perp) };
        if slot.map(|(s, _)| score < s - 1e-3).unwrap_or(true) {
            *slot = Some((score, i));
        }
    }
    // Un voisin ALIGNE passe toujours avant un voisin decale (la ligne / colonne courante d'abord).
    best.or(best_off).map(|(_, i)| i)
}

/// Element dont le centre est le plus proche de celui de `target` (repli quand l'ancienne selection a disparu).
pub fn nearest_to(rects: &[Rect], target: Rect) -> Option<usize> {
    let t = target.center();
    rects
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.center().distance(t).partial_cmp(&b.1.center().distance(t)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
}

/// Appuis de fleche necessaires pour modifier d'une unite un DragValue entier d'egui (vitesse 0,25).
pub const DRAG_KEYS_PER_STEP: usize = 4;

/// Cle de contexte du dialogue modal.
pub const MODAL_CONTEXT: u8 = 100;

// ------------------------------------------------------------------ moteur de selection

pub struct PadFocus {
    items: RefCell<Vec<Item>>,
    modal_scope: Cell<bool>,
    default_id: Cell<Option<Id>>,
    /// (element a centrer, images restantes avant abandon).
    scroll: Cell<Option<(Id, u8)>>,
    queue: Vec<PadAction>,
    inject: Vec<Key>,
    ctx_key: u8,
    sel: Option<Id>,
    sel_rect: Option<Rect>,
    mem: HashMap<u8, (Id, Rect)>,
    last_focus: Option<Id>,
    was_active: bool,
    sel_since: Instant,
}

impl Default for PadFocus {
    fn default() -> Self {
        PadFocus {
            items: RefCell::new(Vec::new()),
            modal_scope: Cell::new(false),
            default_id: Cell::new(None),
            scroll: Cell::new(None),
            queue: Vec::new(),
            inject: Vec::new(),
            ctx_key: 0,
            sel: None,
            sel_rect: None,
            mem: HashMap::new(),
            last_focus: None,
            was_active: false,
            sel_since: Instant::now(),
        }
    }
}

impl PadFocus {
    /// Les widgets enregistres a partir de maintenant appartiennent (ou non) au dialogue modal.
    pub fn set_modal_scope(&self, modal: bool) {
        self.modal_scope.set(modal);
    }

    /// Element a selectionner par defaut dans le contexte courant s'il n'a pas de memoire (ex. « Annuler »).
    pub fn set_default(&self, id: Id) {
        self.default_id.set(Some(id));
    }

    /// Declare un widget interactif de la frame courante (ignore s'il est desactive).
    pub fn register(&self, resp: &Response, kind: ItemKind, in_scroll: bool) {
        if !resp.enabled() {
            return;
        }
        self.items.borrow_mut().push(Item { id: resp.id, rect: resp.rect, kind, in_scroll, modal: self.modal_scope.get() });
    }

    pub fn push(&mut self, a: PadAction) {
        self.queue.push(a);
    }

    /// Touches a injecter au debut de la prochaine frame (Entree / Echap / fleches).
    pub fn take_injected(&mut self) -> Vec<Key> {
        std::mem::take(&mut self.inject)
    }

    pub fn selected(&self) -> Option<Id> {
        self.sel
    }

    pub fn is_selected(&self, id: Id) -> bool {
        self.sel == Some(id)
    }

    /// Duree (ms) depuis la derniere nouvelle selection (pour l'infobulle de focus).
    pub fn selected_for_ms(&self) -> u64 {
        self.sel_since.elapsed().as_millis() as u64
    }

    /// Faut-il centrer cet element dans sa ScrollArea maintenant ? (vrai une seule fois)
    pub fn take_scroll(&self, id: Id, in_scroll: bool) -> bool {
        match self.scroll.get() {
            Some((sid, _)) if sid == id && in_scroll => {
                self.scroll.set(None);
                true
            }
            _ => false,
        }
    }

    fn select(&mut self, it: &Item, scroll: bool) {
        if self.sel != Some(it.id) {
            self.sel_since = Instant::now();
        }
        self.sel = Some(it.id);
        self.sel_rect = Some(it.rect);
        if scroll {
            self.scroll.set(Some((it.id, 3)));
        }
    }

    /// A appeler UNE fois par frame, apres le dessin de tous les panneaux et du dialogue.
    /// `page_key` : onglet courant ; `modal_open` : un dialogue de confirmation est affiche ;
    /// `active` : mode manette (une manette utilisable est detectee). Renvoie true s'il faut redessiner.
    pub fn end_frame(&mut self, ctx: &Context, page_key: u8, modal_open: bool, active: bool) -> bool {
        let mut items: Vec<Item> = std::mem::take(&mut *self.items.borrow_mut());
        items.retain(|i| i.modal == modal_open);
        let mut seen = std::collections::HashSet::new();
        items.retain(|i| seen.insert(i.id));
        let queue = std::mem::take(&mut self.queue);
        if let Some((id, ttl)) = self.scroll.get() {
            self.scroll.set(if ttl == 0 { None } else { Some((id, ttl - 1)) });
        }
        let focused = ctx.memory(|m| m.focused());
        let key = if modal_open { MODAL_CONTEXT } else { page_key };

        // Hors mode manette : aucune selection maison (clavier / souris natifs).
        if !active {
            if self.was_active {
                self.save_ctx();
                self.sel = None;
                self.sel_rect = None;
            }
            self.was_active = false;
            self.ctx_key = key;
            self.last_focus = focused;
            return false;
        }
        let mut dirty = false;
        let activated = !self.was_active;
        self.was_active = true;

        // Changement de contexte (onglet, ouverture / fermeture du dialogue) ou (re)activation : on
        // memorise l'ancienne selection et on restaure celle du nouveau contexte.
        if key != self.ctx_key || activated {
            if !activated {
                self.save_ctx();
            }
            if self.ctx_key == MODAL_CONTEXT && key != MODAL_CONTEXT {
                self.mem.remove(&MODAL_CONTEXT); // le prochain dialogue repart de « Annuler »
            }
            if key == MODAL_CONTEXT {
                self.mem.remove(&MODAL_CONTEXT);
            }
            self.ctx_key = key;
            let m = self.mem.get(&key).copied();
            self.sel = m.map(|x| x.0);
            self.sel_rect = m.map(|x| x.1);
            if self.sel.is_some() {
                self.scroll.set(self.sel.map(|id| (id, 3)));
            }
            dirty = true;
        }

        // Un menu deroulant / selecteur de couleur est ouvert : egui le gere (navigation native).
        if ctx.memory(|m| m.any_popup_open()) {
            self.last_focus = focused;
            return dirty;
        }
        if items.is_empty() {
            self.last_focus = focused;
            return dirty;
        }

        // Focus deplace par le clavier / la souris : la selection le suit (sans defilement force).
        if focused != self.last_focus {
            if let Some(fid) = focused {
                if let Some(it) = items.iter().find(|i| i.id == fid) {
                    if self.sel != Some(fid) {
                        let it = *it;
                        self.select(&it, false);
                        dirty = true;
                    }
                }
            }
        }

        // Il y a toujours une selection valide.
        let rects: Vec<Rect> = items.iter().map(|i| i.rect).collect();
        let mut idx = self.sel.and_then(|s| items.iter().position(|i| i.id == s));
        if idx.is_none() {
            let pick = match self.sel_rect {
                Some(r) => nearest_to(&rects, r),
                None => self.default_id.get().and_then(|d| items.iter().position(|i| i.id == d)).or(Some(0)),
            };
            if let Some(p) = pick {
                let it = items[p];
                self.select(&it, true);
                idx = Some(p);
                dirty = true;
            }
        }
        self.default_id.set(None);
        let Some(mut idx) = idx else { return dirty };

        // Actions manette, dans l'ordre.
        let mut want_focus_text = false;
        for a in queue {
            dirty = true;
            let cur = items[idx];
            let editing = focused == Some(cur.id) && cur.kind != ItemKind::Button;
            match a {
                PadAction::Dir(d) => {
                    if editing && cur.kind == ItemKind::Drag && matches!(d, Dir::Up | Dir::Down) {
                        // egui : un DragValue entier avance de 0,25 par appui de fleche => 4 appuis pour 1 unite.
                        for _ in 0..DRAG_KEYS_PER_STEP {
                            self.inject.push(if d == Dir::Up { Key::ArrowUp } else { Key::ArrowDown });
                        }
                        continue;
                    }
                    if editing {
                        ctx.memory_mut(|m| m.surrender_focus(cur.id)); // on sort du champ proprement
                    }
                    if let Some(n) = spatial_next(&rects, idx, d) {
                        idx = n;
                        let it = items[n];
                        self.select(&it, true);
                    }
                }
                PadAction::Confirm => match cur.kind {
                    ItemKind::Button => self.inject.push(Key::Enter),
                    ItemKind::Text | ItemKind::Drag => {
                        if editing {
                            self.inject.push(Key::Enter);
                        } else {
                            want_focus_text = true;
                        }
                    }
                },
                PadAction::Back => {
                    if editing {
                        ctx.memory_mut(|m| m.surrender_focus(cur.id));
                    } else {
                        self.inject.push(Key::Escape);
                    }
                }
            }
        }

        // Recopie de la selection dans le focus egui.
        let cur = items[idx];
        let f = ctx.memory(|m| m.focused());
        match cur.kind {
            ItemKind::Button => {
                if f != Some(cur.id) {
                    ctx.memory_mut(|m| m.request_focus(cur.id));
                    dirty = true;
                }
            }
            ItemKind::Text | ItemKind::Drag => {
                if cur.kind == ItemKind::Drag && f == Some(cur.id) {
                    // En edition, les fleches haut/bas doivent atteindre le DragValue (et non le focus natif).
                    ctx.memory_mut(|m| m.set_focus_lock_filter(cur.id, egui::EventFilter { vertical_arrows: true, ..Default::default() }));
                }
                if want_focus_text {
                    ctx.memory_mut(|m| m.request_focus(cur.id));
                } else if let Some(other) = f {
                    if other != cur.id {
                        ctx.memory_mut(|m| m.surrender_focus(other));
                        dirty = true;
                    }
                }
            }
        }
        self.sel_rect = Some(cur.rect);
        self.last_focus = ctx.memory(|m| m.focused());
        dirty
    }

    fn save_ctx(&mut self) {
        if let (Some(id), Some(r)) = (self.sel, self.sel_rect) {
            self.mem.insert(self.ctx_key, (id, r));
        }
    }
}

/// Touche egui correspondant a une action (utilise par les tests et le crochet d'entree).
pub fn key_event(k: Key, pressed: bool) -> egui::Event {
    egui::Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: egui::Modifiers::NONE }
}

/// Action de selection correspondant a une touche d'interface issue de la manette.
pub fn action_from_key(k: crate::uilogic::NavKey) -> PadAction {
    use crate::uilogic::NavKey;
    match k {
        NavKey::Up => PadAction::Dir(Dir::Up),
        NavKey::Down => PadAction::Dir(Dir::Down),
        NavKey::Left => PadAction::Dir(Dir::Left),
        NavKey::Right => PadAction::Dir(Dir::Right),
        NavKey::Enter => PadAction::Confirm,
        NavKey::Escape => PadAction::Back,
    }
}

/// Touche egui d'une touche d'interface (navigation native, hors mode manette ou menu ouvert).
pub fn egui_key(k: crate::uilogic::NavKey) -> Key {
    use crate::uilogic::NavKey;
    match k {
        NavKey::Up => Key::ArrowUp,
        NavKey::Down => Key::ArrowDown,
        NavKey::Left => Key::ArrowLeft,
        NavKey::Right => Key::ArrowRight,
        NavKey::Enter => Key::Enter,
        NavKey::Escape => Key::Escape,
    }
}

impl PadFocus {
    /// Oublie la selection et la memoire de selection (changement d'onglet : la page repart de zero).
    pub fn reset(&mut self) {
        self.mem.clear();
        self.sel = None;
        self.sel_rect = None;
        self.scroll.set(None);
        self.default_id.set(None);
    }
}
