//! Tests v0.6 : reconnexion de manette (cycles multiples), selection manette maison (navigation
//! spatiale pure + rendu egui headless avec evenements injectes), defilement centre, dialogue modal,
//! champs de texte, repetition du stick, barre d'onglets centree en mode manette.
use eframe::egui::{self, Context, Id, Pos2, Rect, Vec2};
use agss::gamepad::*;
use agss::padfocus::*;
use agss::uilogic::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ================================================================== reconnexion (BUG A)

/// Source simulee : la manette est « branchee » tant que l'interrupteur partage est vrai.
struct Switchable {
    on: Arc<AtomicBool>,
    info: PadInfo,
    polls: Arc<AtomicUsize>,
}
impl PadSource for Switchable {
    fn poll(&mut self, _now: u64) -> Vec<PadPoll> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        if self.on.load(Ordering::SeqCst) {
            vec![PadPoll { info: self.info.clone(), readable: true, states: vec![PadState::default()] }]
        } else {
            vec![]
        }
    }
}

fn wait_for(svc: &GamepadService, want_present: bool, secs: u64) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        if svc.status().is_empty() != want_present {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn le_thread_detecte_plusieurs_cycles_debranchement_rebranchement_sans_redemarrer() {
    let on = Arc::new(AtomicBool::new(false));
    let polls = Arc::new(AtomicUsize::new(0));
    let (on2, polls2) = (on.clone(), polls.clone());
    let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let l2 = logs.clone();
    let svc = spawn_with(
        move |_log| vec![Box::new(Switchable { on: on2, info: PadInfo { id: "hid:1".into(), name: "DualSense".into(), conn: Connection::Bluetooth }, polls: polls2 }) as Box<dyn PadSource>],
        || {},
        || true,
        move |m| l2.lock().unwrap().push(m.to_string()),
    );
    assert!(svc.status().is_empty());
    for cycle in 1..=5 {
        on.store(true, Ordering::SeqCst);
        assert!(wait_for(&svc, true, 5), "cycle {} : rebranchement non detecte", cycle);
        assert_eq!(status_line(&svc.status(), "Aucune", "x"), "DualSense (Bluetooth)");
        on.store(false, Ordering::SeqCst);
        assert!(wait_for(&svc, false, 5), "cycle {} : debranchement non detecte", cycle);
        assert_eq!(status_line(&svc.status(), "Aucune", "x"), "Aucune");
    }
    // Le thread tourne toujours (il ne sort pas apres une deconnexion).
    let before = polls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(600));
    assert!(polls.load(Ordering::SeqCst) > before, "le thread de lecture doit continuer a sonder");
    let l = logs.lock().unwrap();
    assert_eq!(l.iter().filter(|m| m.contains("] connectee")).count(), 5);
    assert_eq!(l.iter().filter(|m| m.contains("deconnectee")).count(), 5);
}

#[test]
fn hub_apres_deconnexion_la_manette_revenue_est_une_nouvelle_connexion() {
    let info = PadInfo { id: "p".into(), name: "DualSense".into(), conn: Connection::Usb };
    let press = PadState { confirm: true, ..Default::default() };
    let on = |s: PadState| vec![PadPoll { info: info.clone(), readable: true, states: vec![s] }];
    let script: Vec<Vec<PadPoll>> = (0..4).flat_map(|_| vec![on(PadState::default()), on(press), vec![], vec![]]).collect();
    struct S(VecDeque<Vec<PadPoll>>);
    impl PadSource for S {
        fn poll(&mut self, _n: u64) -> Vec<PadPoll> {
            self.0.pop_front().unwrap_or_default()
        }
    }
    let mut hub = Hub::new(vec![Box::new(S(script.into()))]);
    let (mut connects, mut disconnects, mut confirms) = (0, 0, 0);
    for t in 0..16u64 {
        let o = hub.tick(t * 100);
        connects += o.changes.iter().filter(|c| matches!(c, PadChange::Connected(_))).count();
        disconnects += o.changes.iter().filter(|c| matches!(c, PadChange::Disconnected(_))).count();
        confirms += o.navs.iter().filter(|n| **n == Nav::Confirm).count();
    }
    assert_eq!((connects, disconnects), (4, 4));
    assert_eq!(confirms, 4, "chaque reconnexion repart d'un etat neutre : l'appui est pris en compte");
}

#[test]
fn intervalles_de_reessai_hid() {
    assert!(hid_retry_interval_ms(true) <= 1_000, "reessai rapide au premier plan");
    assert!(hid_retry_interval_ms(false) > hid_retry_interval_ms(true));
    assert!(hid_retry_interval_ms(false) <= 5_000);
}

// ================================================================== repetition du stick / D-pad

#[test]
fn repetition_apres_delai_puis_au_rythme_et_arret_au_relachement() {
    let mut t = NavTracker::default();
    let neutral = PadState::default();
    let held = PadState { ly: -1.0, ..Default::default() }; // stick vers le bas
    t.update_at(&neutral, 0);
    assert_eq!(t.update_at(&held, 1000), vec![Nav::Down]);
    assert!(t.repeats(1000 + REPEAT_DELAY_MS - 1).is_empty(), "pas de repetition avant le delai");
    assert_eq!(t.repeats(1000 + REPEAT_DELAY_MS), vec![Nav::Down]);
    assert!(t.repeats(1000 + REPEAT_DELAY_MS + REPEAT_EVERY_MS - 1).is_empty());
    assert_eq!(t.repeats(1000 + REPEAT_DELAY_MS + REPEAT_EVERY_MS), vec![Nav::Down]);
    // Zone morte : un stick qui retombe a 0.2 (< seuil de relachement) arrete la repetition.
    t.update_at(&PadState { ly: -0.2, ..Default::default() }, 2000);
    assert!(t.repeats(5000).is_empty());
    // Les boutons (valider, retour, pages) ne se repetent jamais.
    t.update_at(&PadState { confirm: true, lb: true, ..Default::default() }, 6000);
    assert!(t.repeats(9000).is_empty());
}

#[test]
fn hub_repete_les_directions_maintenues_sans_nouveau_rapport() {
    let info = PadInfo { id: "p".into(), name: "Xbox / XInput".into(), conn: Connection::Unknown };
    let held = PadState { down: true, ..Default::default() };
    struct S(PadInfo, Vec<Vec<PadState>>);
    impl PadSource for S {
        fn poll(&mut self, _n: u64) -> Vec<PadPoll> {
            let st = if self.1.is_empty() { vec![] } else { self.1.remove(0) };
            vec![PadPoll { info: self.0.clone(), readable: true, states: st }]
        }
    }
    let mut hub = Hub::new(vec![Box::new(S(info, vec![vec![PadState::default()], vec![held]]))]);
    hub.tick(0);
    assert_eq!(hub.tick(10).navs, vec![Nav::Down]);
    assert!(hub.tick(200).navs.is_empty());
    assert_eq!(hub.tick(10 + REPEAT_DELAY_MS).navs, vec![Nav::Down]);
    assert_eq!(hub.tick(10 + REPEAT_DELAY_MS + REPEAT_EVERY_MS).navs, vec![Nav::Down]);
}

// ================================================================== navigation spatiale (pure)

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
}

#[test]
fn voisin_dans_chaque_direction_sur_une_grille_reguliere() {
    // 3 colonnes x 3 lignes de boutons 80x30 (indices 0..8, ligne par ligne).
    let rects: Vec<Rect> = (0..9).map(|i| r((i % 3) as f32 * 100.0, (i / 3) as f32 * 50.0, 80.0, 30.0)).collect();
    assert_eq!(spatial_next(&rects, 4, Dir::Right), Some(5));
    assert_eq!(spatial_next(&rects, 4, Dir::Left), Some(3));
    assert_eq!(spatial_next(&rects, 4, Dir::Down), Some(7));
    assert_eq!(spatial_next(&rects, 4, Dir::Up), Some(1));
    assert_eq!(spatial_next(&rects, 0, Dir::Left), None);
    assert_eq!(spatial_next(&rects, 0, Dir::Up), None);
    assert_eq!(spatial_next(&rects, 8, Dir::Right), None);
    assert_eq!(spatial_next(&rects, 8, Dir::Down), None);
    assert_eq!(spatial_next(&rects, 99, Dir::Down), None);
}

#[test]
fn grille_irreguliere_ligne_large_et_colonne_etroite() {
    let rects = vec![
        r(0.0, 0.0, 600.0, 30.0),   // 0 champ pleine largeur
        r(0.0, 50.0, 100.0, 30.0),  // 1
        r(500.0, 50.0, 100.0, 30.0), // 2 (bouton « Parcourir » a droite)
        r(0.0, 100.0, 200.0, 60.0),  // 3 grand bouton
        r(300.0, 100.0, 50.0, 20.0), // 4 petit
        r(250.0, 300.0, 100.0, 30.0), // 5 tres bas
    ];
    assert_eq!(spatial_next(&rects, 0, Dir::Down), Some(4), "depuis le champ large : la cible la plus proche de son centre");
    assert_eq!(spatial_next(&rects, 1, Dir::Right), Some(2));
    assert_eq!(spatial_next(&rects, 2, Dir::Left), Some(1));
    assert_eq!(spatial_next(&rects, 1, Dir::Down), Some(3), "meme colonne : le grand bouton en dessous");
    assert_eq!(spatial_next(&rects, 2, Dir::Down), Some(4), "depuis la droite : le plus proche en dessous");
    assert_eq!(spatial_next(&rects, 4, Dir::Down), Some(5));
    assert_eq!(spatial_next(&rects, 5, Dir::Up), Some(4));
    // Les voisins de la meme ligne (centres a peu pres alignes) ne comptent pas pour haut / bas.
    let row = vec![r(0.0, 0.0, 80.0, 44.0), r(100.0, 8.0, 80.0, 28.0)];
    assert_eq!(spatial_next(&row, 0, Dir::Down), None);
    assert_eq!(spatial_next(&row, 1, Dir::Up), None);
    assert_eq!(spatial_next(&row, 0, Dir::Right), Some(1));
}

#[test]
fn elements_hors_champ_sont_atteignables() {
    // Colonne de 100 lignes de 30 px : la vue ne montrerait que les 10 premieres, mais la navigation
    // porte sur tous les rectangles (ceux sous le pli compris).
    let rects: Vec<Rect> = (0..100).map(|i| r(10.0, i as f32 * 40.0, 200.0, 30.0)).collect();
    let mut i = 0;
    for _ in 0..99 {
        i = spatial_next(&rects, i, Dir::Down).unwrap();
    }
    assert_eq!(i, 99);
    assert_eq!(spatial_next(&rects, 99, Dir::Down), None);
    for _ in 0..99 {
        i = spatial_next(&rects, i, Dir::Up).unwrap();
    }
    assert_eq!(i, 0);
}

#[test]
fn repli_sur_le_plus_proche_quand_la_selection_disparait() {
    let rects = vec![r(0.0, 0.0, 50.0, 20.0), r(0.0, 100.0, 50.0, 20.0), r(0.0, 200.0, 50.0, 20.0)];
    assert_eq!(nearest_to(&rects, r(0.0, 110.0, 50.0, 20.0)), Some(1));
    assert_eq!(nearest_to(&rects, r(0.0, 900.0, 50.0, 20.0)), Some(2));
    assert_eq!(nearest_to(&[], r(0.0, 0.0, 1.0, 1.0)), None);
}

// ================================================================== barre d'onglets centree (CHANGEMENT D)

#[test]
fn onglets_centres_en_mode_manette_et_a_gauche_sinon() {
    let ws = [100.0, 140.0, 80.0, 90.0];
    let gap = 8.0;
    let total: f32 = ws.iter().sum::<f32>() + gap * 3.0;
    let centered = tab_bar_layout(1000.0, Some(46.0), &ws, gap, true);
    let left_margin = centered.tab_x[0];
    let last = centered.tab_x[3] + ws[3];
    assert!((left_margin - (1000.0 - total) / 2.0).abs() < 0.01);
    assert!((left_margin - (1000.0 - last)).abs() < 0.01, "marges gauche et droite egales");
    assert_eq!(centered.left_glyph_x, 0.0);
    assert!((centered.right_glyph_x - (1000.0 - 46.0)).abs() < 0.01, "pictogramme droit au bord droit");
    // Espacement regulier.
    assert!((centered.tab_x[1] - (centered.tab_x[0] + 100.0 + gap)).abs() < 0.01);
    // Hors mode manette : onglets a gauche (apres le pictogramme s'il existe).
    let left = tab_bar_layout(1000.0, None, &ws, gap, false);
    assert_eq!(left.tab_x[0], 0.0);
    let left_g = tab_bar_layout(1000.0, Some(46.0), &ws, gap, false);
    assert!((left_g.tab_x[0] - 54.0).abs() < 0.01);
    // Barre trop etroite : les onglets ne recouvrent jamais le pictogramme gauche.
    let tight = tab_bar_layout(400.0, Some(46.0), &ws, gap, true);
    assert!(tight.tab_x[0] >= 54.0 - 0.01);
}

// ================================================================== moteur de selection (rendu egui headless)

struct Harness {
    ctx: Context,
    focus: PadFocus,
    time: f64,
    n_buttons: usize,
    with_text: bool,
    with_drag: bool,
    modal: bool,
    page: u8,
    active: bool,
    ids: Vec<Id>,
    rects: Vec<Rect>,
    text_id: Option<Id>,
    drag_id: Option<Id>,
    modal_ids: Vec<Id>,
    clicked: Vec<usize>,
    offset: f32,
    view: Rect,
    content_h: f32,
    drag_value: i32,
    text: String,
    removed: Option<usize>,
}

impl Harness {
    fn new(n: usize) -> Self {
        Harness {
            ctx: Context::default(),
            focus: PadFocus::default(),
            time: 0.0,
            n_buttons: n,
            with_text: false,
            with_drag: false,
            modal: false,
            page: 0,
            active: true,
            ids: vec![],
            rects: vec![],
            text_id: None,
            drag_id: None,
            modal_ids: vec![],
            clicked: vec![],
            offset: 0.0,
            view: Rect::NOTHING,
            content_h: 0.0,
            drag_value: 5,
            text: String::new(),
            removed: None,
        }
    }

    /// Une frame : injecte les touches decidees a la frame precedente, dessine, met a jour la selection.
    fn frame(&mut self, extra: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        let mut events = extra;
        for k in self.focus.take_injected() {
            events.push(key_event(k, true));
            events.push(key_event(k, false));
        }
        let raw = egui::RawInput { events, time: Some(self.time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))), ..Default::default() };
        let (mut ids, mut rects, mut clicked, mut modal_ids) = (vec![], vec![], vec![], vec![]);
        let (mut text_id, mut drag_id) = (None, None);
        let (mut offset, mut view, mut content_h) = (0.0, Rect::NOTHING, 0.0);
        let focus = &self.focus;
        let (n, with_text, with_drag, modal, removed) = (self.n_buttons, self.with_text, self.with_drag, self.modal, self.removed);
        let (text, drag_value) = (&mut self.text, &mut self.drag_value);
        let _ = self.ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let out = egui::ScrollArea::vertical().id_salt("page").auto_shrink([false, false]).show(ui, |ui| {
                    for i in 0..n {
                        if removed == Some(i) {
                            continue;
                        }
                        let resp = ui.add_sized([200.0, 30.0], egui::Button::new(format!("bouton {}", i)));
                        focus.register(&resp, ItemKind::Button, true);
                        if focus.take_scroll(resp.id, true) {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        if resp.clicked() {
                            clicked.push(i);
                        }
                        ids.push(resp.id);
                        rects.push(resp.rect);
                    }
                    if with_text {
                        let resp = ui.add_sized([200.0, 30.0], egui::TextEdit::singleline(text));
                        focus.register(&resp, ItemKind::Text, true);
                        if focus.take_scroll(resp.id, true) {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        text_id = Some(resp.id);
                    }
                    if with_drag {
                        let resp = ui.add(egui::DragValue::new(drag_value));
                        focus.register(&resp, ItemKind::Drag, true);
                        drag_id = Some(resp.id);
                    }
                });
                offset = out.state.offset.y;
                view = out.inner_rect;
                content_h = out.content_size.y;
            });
            if modal {
                focus.set_modal_scope(true);
                egui::Area::new(Id::new("modal")).fixed_pos(Pos2::new(50.0, 50.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                    let cancel = ui.button("Annuler");
                    let ok = ui.button("Confirmer");
                    focus.set_default(cancel.id);
                    focus.register(&cancel, ItemKind::Button, false);
                    focus.register(&ok, ItemKind::Button, false);
                    modal_ids = vec![cancel.id, ok.id];
                });
                focus.set_modal_scope(false);
            }
        });
        self.ids = ids;
        self.rects = rects;
        self.clicked.extend(clicked);
        self.modal_ids = modal_ids;
        self.text_id = text_id;
        self.drag_id = drag_id;
        self.offset = offset;
        self.view = view;
        self.content_h = content_h;
        self.focus.end_frame(&self.ctx, self.page, self.modal, self.active);
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn act(&mut self, a: PadAction) {
        self.focus.push(a);
        self.frames(45); // laisse finir l'animation de defilement
    }

    fn sel_index(&self) -> Option<usize> {
        let s = self.focus.selected()?;
        self.ids.iter().position(|i| *i == s)
    }
}

#[test]
fn il_y_a_toujours_une_selection_au_demarrage_et_hors_mode_manette_aucune() {
    let mut h = Harness::new(10);
    h.frames(4);
    assert_eq!(h.sel_index(), Some(0), "premier element selectionne");
    assert!(h.ctx.memory(|m| m.focused()) == Some(h.ids[0]), "la selection est recopiee dans le focus egui");
    // Hors mode manette : aucune selection maison, focus natif intact.
    let mut h2 = Harness::new(10);
    h2.active = false;
    h2.frames(4);
    assert_eq!(h2.focus.selected(), None);
    assert_eq!(h2.ctx.memory(|m| m.focused()), None);
}

#[test]
fn descente_jusqu_au_dernier_element_sous_le_pli_avec_defilement_centre() {
    let mut h = Harness::new(60);
    h.frames(4);
    assert_eq!(h.offset, 0.0);
    let mut max_off: f32 = 0.0;
    for i in 1..60 {
        h.act(PadAction::Dir(Dir::Down));
        assert_eq!(h.sel_index(), Some(i), "pas {}", i);
        // Centrage vertical de la selection (sauf tout en haut / tout en bas ou le defilement est borne).
        let center = h.rects[i].center().y;
        let view_c = h.view.center().y;
        let max_scroll = (h.content_h - h.view.height()).max(0.0);
        if h.offset > 1.0 && h.offset < max_scroll - 1.0 {
            assert!((center - view_c).abs() < 4.0, "element {} non centre : {:.1} vs {:.1}", i, center, view_c);
        }
        // Toujours visible, jamais perdu.
        assert!(h.rects[i].min.y >= h.view.min.y - 1.0 && h.rects[i].max.y <= h.view.max.y + 1.0, "element {} hors de la zone visible", i);
        max_off = max_off.max(h.offset);
    }
    assert!(max_off > 500.0, "la page a defile vers le bas");
    // Dernier element : defilement au maximum.
    assert!((h.offset - (h.content_h - h.view.height())).abs() < 2.0, "dernier element : bas de page");
    // Retour vers le haut jusqu'au premier : defilement revenu a 0.
    for i in (0..59).rev() {
        h.act(PadAction::Dir(Dir::Up));
        assert_eq!(h.sel_index(), Some(i));
    }
    assert!(h.offset < 1.0, "premier element : haut de page ({})", h.offset);
}

#[test]
fn la_molette_n_est_pas_combattue_et_le_scroll_ne_se_declenche_qu_a_un_changement_de_selection() {
    let mut h = Harness::new(60);
    h.frames(4);
    h.act(PadAction::Dir(Dir::Down));
    let before = h.offset;
    // Defilement manuel (molette) : l'offset change et reste tel quel tant que la selection ne change pas.
    h.frame(vec![egui::Event::PointerMoved(Pos2::new(100.0, 100.0))]);
    h.frame(vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, -300.0), modifiers: egui::Modifiers::NONE }]);
    h.frames(60);
    assert!(h.offset > before + 100.0, "la molette defile ({} -> {})", before, h.offset);
    let after_wheel = h.offset;
    h.frames(60);
    assert!((h.offset - after_wheel).abs() < 0.5, "aucun defilement force sans changement de selection");
}

#[test]
fn valider_active_le_bouton_selectionne_et_retour_ne_fait_rien_sans_dialogue() {
    let mut h = Harness::new(5);
    h.frames(3);
    h.act(PadAction::Dir(Dir::Down));
    h.act(PadAction::Dir(Dir::Down));
    h.act(PadAction::Confirm);
    h.frames(5);
    assert_eq!(h.clicked, vec![2], "Croix/A active exactement le bouton selectionne");
    h.act(PadAction::Back);
    assert_eq!(h.sel_index(), Some(2), "Rond/B ne fait pas perdre la selection");
}

#[test]
fn la_selection_survit_a_la_disparition_d_un_element_et_a_un_changement_de_liste() {
    let mut h = Harness::new(10);
    h.frames(3);
    for _ in 0..5 {
        h.act(PadAction::Dir(Dir::Down));
    }
    assert_eq!(h.sel_index(), Some(5));
    // L'element selectionne disparait (ex. jeu retire de la liste) : on retombe sur son voisin, jamais sur « rien ».
    h.removed = Some(5);
    h.frames(6);
    let s = h.sel_index().expect("une selection existe toujours");
    assert!(s == 4 || s == 5, "voisin immediat (index {})", s);
    // La liste est rechargee avec moins d'elements.
    h.removed = None;
    h.n_buttons = 3;
    h.frames(6);
    assert!(h.sel_index().is_some());
    // Liste vide un instant puis de retour : on revient pres de l'ancienne position.
    h.n_buttons = 0;
    h.frames(3);
    h.n_buttons = 8;
    h.frames(6);
    assert!(h.sel_index().is_some());
    h.act(PadAction::Dir(Dir::Down));
    assert!(h.sel_index().is_some(), "la navigation continue apres le rechargement");
}

#[test]
fn selection_memorisee_par_onglet_et_restauree() {
    let mut h = Harness::new(12);
    h.frames(3);
    for _ in 0..4 {
        h.act(PadAction::Dir(Dir::Down));
    }
    let id = h.focus.selected().unwrap();
    h.page = 1; // autre onglet (meme contenu de test, autre contexte)
    h.frames(6);
    assert!(h.focus.selected().is_some(), "une selection existe dans le nouvel onglet");
    h.act(PadAction::Dir(Dir::Down));
    h.page = 0;
    h.frames(6);
    assert_eq!(h.focus.selected(), Some(id), "retour sur l'onglet : selection precedente restauree");
}

#[test]
fn le_dialogue_modal_capture_la_navigation_annuler_par_defaut_et_la_selection_revient_a_la_fermeture() {
    let mut h = Harness::new(8);
    h.frames(3);
    for _ in 0..3 {
        h.act(PadAction::Dir(Dir::Down));
    }
    let before = h.focus.selected().unwrap();
    h.modal = true;
    h.frames(6);
    assert_eq!(h.focus.selected(), Some(h.modal_ids[0]), "« Annuler » selectionne par defaut");
    // La navigation reste piegee dans le dialogue : jamais vers le contenu de la page derriere.
    for d in [Dir::Down, Dir::Down, Dir::Up, Dir::Up, Dir::Left, Dir::Right, Dir::Down] {
        h.act(PadAction::Dir(d));
        assert!(h.modal_ids.contains(&h.focus.selected().unwrap()), "selection sortie du dialogue apres {:?}", d);
    }
    h.modal = false;
    h.frames(6);
    assert_eq!(h.focus.selected(), Some(before), "fermeture du dialogue : selection de la page restauree");
    // Un second dialogue repart de nouveau sur « Annuler ».
    h.modal = true;
    h.frames(6);
    assert_eq!(h.focus.selected(), Some(h.modal_ids[0]));
}

#[test]
fn champ_de_texte_la_navigation_en_sort_sans_deplacer_le_curseur_texte() {
    let mut h = Harness::new(3);
    h.with_text = true;
    h.frames(4);
    // Descend jusqu'au champ (apres les 3 boutons).
    for _ in 0..3 {
        h.act(PadAction::Dir(Dir::Down));
    }
    let t = h.text_id.unwrap();
    assert_eq!(h.focus.selected(), Some(t));
    assert_ne!(h.ctx.memory(|m| m.focused()), Some(t), "selectionne mais pas en edition tant qu'on ne valide pas");
    h.act(PadAction::Confirm);
    h.frames(3);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(t), "valider entre en edition");
    // Haut : on sort du champ proprement et la selection remonte (aucune fleche injectee).
    h.act(PadAction::Dir(Dir::Up));
    assert_ne!(h.ctx.memory(|m| m.focused()), Some(t));
    assert_eq!(h.sel_index(), Some(2));
    assert!(h.focus.take_injected().is_empty(), "aucune touche fleche injectee");
    // Retour pendant l'edition : sortie du champ, selection conservee.
    h.act(PadAction::Dir(Dir::Down));
    h.act(PadAction::Confirm);
    h.frames(3);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(t));
    h.act(PadAction::Back);
    assert_ne!(h.ctx.memory(|m| m.focused()), Some(t));
    assert_eq!(h.focus.selected(), Some(t));
}

#[test]
fn drag_value_haut_bas_modifient_la_valeur_pendant_l_edition_gauche_droite_sortent() {
    let mut h = Harness::new(1);
    h.with_drag = true;
    h.frames(4);
    h.act(PadAction::Dir(Dir::Down));
    assert_eq!(h.focus.selected(), h.drag_id);
    h.act(PadAction::Confirm);
    h.frames(3);
    assert_eq!(h.ctx.memory(|m| m.focused()), h.drag_id, "en edition");
    let v0 = h.drag_value;
    h.act(PadAction::Dir(Dir::Up));
    h.frames(3);
    assert_eq!(h.focus.selected(), h.drag_id, "haut ne quitte pas le champ en edition");
    assert_eq!(h.drag_value, v0 + 1);
    h.act(PadAction::Dir(Dir::Down));
    h.frames(3);
    assert_eq!(h.drag_value, v0);
    h.act(PadAction::Back);
    assert_ne!(h.ctx.memory(|m| m.focused()), h.drag_id);
}

#[test]
fn un_clic_souris_ou_clavier_sur_un_autre_element_deplace_la_selection() {
    let mut h = Harness::new(6);
    h.frames(4);
    assert_eq!(h.sel_index(), Some(0));
    // Le focus egui est deplace « de l'exterieur » (souris / Tab) : la selection suit.
    let target = h.ids[3];
    h.ctx.memory_mut(|m| m.request_focus(target));
    h.frames(4);
    assert_eq!(h.focus.selected(), Some(target));
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(target));
}

#[test]
fn perte_de_focus_de_la_fenetre_la_selection_est_restauree() {
    let mut h = Harness::new(6);
    h.frames(4);
    h.act(PadAction::Dir(Dir::Down));
    let s = h.focus.selected().unwrap();
    // Focus egui perdu (fenetre desactivee, clic dans le vide...).
    h.ctx.memory_mut(|m| m.surrender_focus(s));
    h.frames(4);
    assert_eq!(h.focus.selected(), Some(s));
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(s), "le focus est re-applique sur la selection");
}

#[test]
fn script_de_test_jetons_connus() {
    use agss::gamepad_sys::script_token_state;
    assert!(script_token_state("down").unwrap().down);
    assert!(script_token_state("A").unwrap().confirm);
    assert!(script_token_state("lb").unwrap().lb);
    assert!(script_token_state("stickdown").unwrap().ly < -0.9);
    assert!(script_token_state("zzz").is_none());
}
