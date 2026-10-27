//! 2.0 lot 5 : fondations de l'interface. Vocabulaire du mode simple (5 langues, Windows et Linux), separation simple /
//! expert, horloge d'animation (AUCUN redessin au repos, 30 i/s pour l'ambiance, ecran pour les transitions) verifiee
//! sur de vrais composants egui dessines sans fenetre, logique des composants.
use agss::gui::ui::{self, anim, anim::AnimClock, anim::Motion, anim::Repaint, Btn, State, TileIcon, Tokens};
use agss::i18n::{expert_variant, tr, tr_linux, tr_mode, tr_raw, Lang, Mode, ALL_KEYS, K, SIMPLE_KEYS};
use eframe::egui;
use std::time::Duration;

// ================================================================== vocabulaire

fn word(s: &str, w: &str) -> bool {
    let s = s.to_lowercase();
    let mut from = 0;
    while let Some(i) = s[from..].find(w) {
        let a = from + i;
        let b = a + w.len();
        let before = s[..a].chars().next_back().map(|c| c.is_alphanumeric()).unwrap_or(false);
        let after = s[b..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false);
        if !before && !after {
            return true;
        }
        from = b;
    }
    false
}

/// Mot interdit en mode simple (ETUDE §6 + decision de l'auteur : « synchroniser » reste autorise), ou None.
fn simple_problem(l: Lang, s: &str) -> Option<&'static str> {
    let low = s.to_lowercase();
    if word(&low.replace(".exe", " exe"), "exe") {
        return Some("exe");
    }
    if low.contains("cloud") {
        return Some("cloud");
    }
    let bad: &[(&str, &'static str)] = match l {
        Lang::Fr => &[("synchro", "synchro (dire « synchroniser » ou « protection »)"), ("synchros", "synchros"), ("save", "save (dire « sauvegarde » / « parties enregistrees »)")],
        Lang::En => &[],
        Lang::Es => &[("nube", "nube")],
        Lang::Ru => &[("облак", "облако")],
        Lang::Zh => &[("云", "云")],
    };
    for (w, why) in bad {
        let hit = if l == Lang::Zh || l == Lang::Ru { low.contains(w) } else { word(&low, w) };
        if hit {
            return Some(why);
        }
    }
    None
}

fn is_v_key(k: K) -> bool {
    let n = format!("{:?}", k);
    let mut c = n.chars();
    c.next() == Some('V') && c.next().map(|x| x.is_ascii_uppercase()).unwrap_or(false)
}

#[test]
fn textes_du_mode_simple_sans_exe_ni_cloud_dans_les_5_langues() {
    let mut bad = Vec::new();
    for l in Lang::ALL {
        for k in SIMPLE_KEYS {
            for s in [tr_raw(l, *k), tr_linux(l, *k)] {
                if let Some(why) = simple_problem(l, s) {
                    bad.push(format!("{:?}/{:?} ({}) : {}", l, k, why, s));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} texte(s) du mode simple interdits :\n{}", bad.len(), bad.join("\n"));
    // « synchroniser » (et ses formes) est AUTORISE en mode simple ; « synchro » seul ne l'est pas.
    assert_eq!(simple_problem(Lang::Fr, "Synchroniser maintenant"), None);
    assert_eq!(simple_problem(Lang::Fr, "Protégé, synchronisé, partout."), None);
    assert!(simple_problem(Lang::Fr, "Ajouter une synchro").is_some());
    assert!(simple_problem(Lang::Fr, "Cloud plus récent").is_some());
    assert!(simple_problem(Lang::En, "Pick your exe").is_some());
    assert!(simple_problem(Lang::Fr, "Ta save").is_some());
    assert!(tr(Lang::Fr, K::VTaglineSub).contains("synchronisé"), "la synchro reste un argument du mode simple");
    assert!(tr(Lang::Fr, K::VSyncedOn).starts_with("Synchronisé"));
}

#[test]
fn cles_du_mode_simple_completes_et_traduites() {
    let all_v: Vec<K> = ALL_KEYS.iter().copied().filter(|k| is_v_key(*k)).collect();
    assert!(all_v.len() >= 70, "{} cles 2.0 seulement", all_v.len());
    for k in &all_v {
        assert!(SIMPLE_KEYS.contains(k), "{:?} absente de SIMPLE_KEYS", k);
    }
    assert_eq!(SIMPLE_KEYS.len(), all_v.len(), "SIMPLE_KEYS contient une cle hors vocabulaire 2.0 ou un doublon");
    for k in SIMPLE_KEYS {
        let en = tr_raw(Lang::En, *k);
        for l in Lang::ALL {
            let s = if l == Lang::Zh { agss::i18n_zh::tr(*k) } else { tr_raw(l, *k) };
            assert!(!s.trim().is_empty(), "{:?} vide en {}", k, l.code());
            for m in ["{}", "{0}", "{1}", "{2}", "\n"] {
                assert_eq!(s.matches(m).count(), en.matches(m).count(), "{:?} {} : marqueur {:?}", k, l.code(), m);
            }
        }
        assert_ne!(tr_raw(Lang::Fr, *k).trim(), "", "{:?}", k);
    }
    // Accroche et texte de l'etape 1 valides par l'auteur.
    assert_eq!(tr(Lang::Fr, K::VTagline), "Ne perds plus jamais rien.");
    assert_eq!(tr(Lang::Fr, K::VTaglineSub), "Protégé, synchronisé, partout.");
    assert_eq!(tr(Lang::En, K::VTagline), "Never lose anything again.");
    assert_eq!(tr(Lang::Ru, K::VTagline), "Больше ничего не потеряешь.");
    assert_eq!(tr(Lang::Es, K::VTagline), "No vuelvas a perder nada.");
    assert_eq!(agss::i18n_zh::tr(K::VTagline), "再也不会丢失任何东西。");
    assert_eq!(
        tr(Lang::Fr, K::VWelcomeBody),
        "Tes sauvegardes et réglages, protégés et synchronisés entre tous tes appareils : PC, portable, console. Tu reprends ta partie là où tu l'as laissée. Tu retrouves tes configurations logicielles, où que tu sois."
    );
    // Glossaire (ETUDE §6.1), tutoiement.
    assert_eq!(tr(Lang::Fr, K::VMyProtections), "Mes protections");
    assert_eq!(tr(Lang::Fr, K::VProtectIt), "Protège-le !");
    assert_eq!(tr(Lang::Fr, K::VSafe), "À l'abri");
    assert_eq!(tr(Lang::Fr, K::VRestoreThis), "Remettre cette version");
    assert_eq!(tr(Lang::Fr, K::VTimeTravel), "Remonter le temps");
    assert_eq!(tr(Lang::Fr, K::VStopProtecting), "Ne plus protéger");
}

#[test]
fn separation_mode_simple_et_mode_expert() {
    assert_eq!(Mode::from_expert(false), Mode::Simple);
    assert_eq!(Mode::from_expert(true), Mode::Expert);
    let mut mapped = 0;
    for k in SIMPLE_KEYS {
        let x = expert_variant(*k);
        for l in Lang::ALL {
            assert_eq!(tr_mode(l, Mode::Simple, *k), tr(l, *k));
            assert_eq!(tr_mode(l, Mode::Expert, *k), tr(l, x));
            for m in ["{}", "{0}", "{1}"] {
                assert_eq!(tr(l, *k).matches(m).count(), tr(l, x).matches(m).count(), "{:?} -> {:?} : marqueurs differents", k, x);
            }
        }
        if x != *k {
            mapped += 1;
            assert!(!SIMPLE_KEYS.contains(&x), "{:?} : l'equivalent expert {:?} est un texte du mode simple", k, x);
            assert_ne!(tr(Lang::Fr, *k), tr(Lang::Fr, x), "{:?} : meme texte dans les deux modes", k);
        }
    }
    assert!(mapped >= 12, "{} equivalents experts seulement", mapped);
    // Le mode expert garde les mots techniques d'origine.
    assert_eq!(tr_mode(Lang::Fr, Mode::Expert, K::VMyProtections), "Mes synchros");
    assert!(tr_mode(Lang::Fr, Mode::Expert, K::VNewerArrived).contains("Cloud"));
    assert_eq!(tr_mode(Lang::Fr, Mode::Simple, K::VNewerArrived), "Une version plus récente est arrivée");
    // Textes communs : identiques dans les deux modes.
    assert_eq!(tr_mode(Lang::Fr, Mode::Expert, K::VTagline), tr(Lang::Fr, K::VTagline));
}

// ================================================================== horloge d'animation sur de vrais composants

struct Harness {
    ctx: egui::Context,
    clock: AnimClock,
}

impl Harness {
    fn new(motion: Motion) -> Harness {
        Harness { ctx: egui::Context::default(), clock: AnimClock::new(motion) }
    }

    /// Une image a l'instant `t` : (decision de l'horloge, delai de redessin demande a egui par les widgets eux-memes).
    fn frame(&mut self, t: f64, scene: &mut dyn FnMut(&mut egui::Ui)) -> (Repaint, Duration) {
        let raw = egui::RawInput { time: Some(t), screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 900.0))), ..Default::default() };
        self.clock.begin_frame();
        let motion = self.clock.motion;
        let out = self.ctx.run(raw, |ctx| {
            ui::install(ctx, Tokens::default_dark(), false, motion);
            egui::CentralPanel::default().show(ctx, |ui| scene(ui));
        });
        self.clock.merge(anim::take_needs(&self.ctx));
        let r = self.clock.decide(t);
        let d = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).unwrap_or(Duration::MAX);
        (r, d)
    }
}

/// Ecran au repos : tous les composants, aucun etat « en cours ».
fn rest_scene(ui: &mut egui::Ui, st: State) {
    let t = Tokens::default_dark();
    ui::card(ui, Some("Carte"), |ui| {
        ui::button(ui, Btn::Primary, "Tout copier maintenant");
        ui::button(ui, Btn::Danger, "Ne plus protéger");
        let mut on = true;
        ui::toggle(ui, &mut on, "Démarrer avec l'ordinateur");
        ui::chip(ui, "Jeux", Some(6), true);
        ui::device_pill(ui, "PC-DESKTOP", t.teal);
    });
    for (i, s) in [st, State::Safe, State::Todo, State::Problem, State::Incoming, State::Paused].iter().enumerate() {
        ui::state_pill(ui, ("p", i), *s, "Texte");
    }
    ui.horizontal(|ui| {
        let spec = ui::TileSpec { name: "Celeste", icon: TileIcon::Initial("C", [0x6a, 0x2d, 0x4a]), state: State::Safe, status: "À l'abri", progress: None, index: 3, epoch: 1, sync: None, width: None, heavy: None };
        ui::tile(ui, "t", &spec);
        ui::add_tile(ui, "a", "Protéger autre chose", 4, 1);
    });
    let mut text = String::new();
    ui::field(ui, "f", "Nom de l'appareil", &mut text, "ex. PC du salon");
    ui::list_row(ui, "r", &ui::ListRow { icon: None, title: "Hollow Knight", subtitle: "Parties enregistrées", trailing: "2 Mo", selected: false, appear: Some((2, 1)) });
    let marks: Vec<ui::Mark> = (0..6).map(|i| ui::Mark { conflict: i == 3, current: i == 5, safety: i == 1, label: format!("1{}:00", i), ..ui::Mark::new(i / 2, t.teal) }).collect();
    let mut sel = 5;
    ui::timeline(ui, "tl", &marks, &|d| d.to_string(), &mut sel);
    ui::status_banner(ui, &ui::BannerSpec { state: State::Safe, title: "Tout est à l'abri", detail: "12 éléments", action: Some("Tout copier maintenant") });
    ui::page_transition(ui, "page", 1, |ui| {
        ui.add_space(4.0);
    });
}

#[test]
fn aucun_redessin_au_repos_avec_tous_les_composants() {
    let mut h = Harness::new(Motion::default());
    let mut scene = |ui: &mut egui::Ui| rest_scene(ui, State::Safe);
    // Apparition : coche qui se trace, cascade, transition de page -> redessin a la frequence de l'ecran.
    let (r0, _) = h.frame(0.0, &mut scene);
    assert_eq!(r0, Repaint::Now);
    let (r1, _) = h.frame(0.016, &mut scene);
    assert_eq!(r1, Repaint::Now);
    // Pulsations « a faire » / pause (3 fois) : ambiance 30 i/s, puis immobile.
    let (r2, _) = h.frame(2.0, &mut scene);
    assert_eq!(r2, Repaint::After(Duration::from_secs_f64(1.0 / 30.0)));
    // Repos : AUCUN redessin, ni par l'horloge ni par les widgets egui, image apres image.
    for t in [6.0, 6.5, 30.0, 300.0] {
        let (r, d) = h.frame(t, &mut scene);
        assert_eq!(r, Repaint::Idle, "repos a t = {}", t);
        assert_eq!(d, Duration::MAX, "un widget demande un redessin au repos (t = {})", t);
    }
    assert!(h.clock.anim_frames >= 3 && h.clock.frames == 7);
}

#[test]
fn ambiance_a_30_images_par_seconde_et_transition_au_changement_d_etat() {
    let mut h = Harness::new(Motion::default());
    let mut st = State::Safe;
    let scene = |ui: &mut egui::Ui, s: State| rest_scene(ui, s);
    for t in [0.0, 1.0, 10.0] {
        h.frame(t, &mut |ui| scene(ui, st));
    }
    assert_eq!(h.frame(10.5, &mut |ui| scene(ui, st)).0, Repaint::Idle);
    // Copie en cours : l'anneau tourne a 30 i/s, tant qu'elle dure.
    st = State::Busy;
    for t in [11.0, 12.0, 20.0] {
        assert_eq!(h.frame(t, &mut |ui| scene(ui, st)).0, Repaint::After(Duration::from_secs_f64(1.0 / 30.0)), "t = {}", t);
    }
    // Probleme : secousse une fois (transition), puis repos.
    st = State::Problem;
    assert_eq!(h.frame(21.0, &mut |ui| scene(ui, st)).0, Repaint::Now);
    assert_eq!(h.frame(21.2, &mut |ui| scene(ui, st)).0, Repaint::Now);
    assert_eq!(h.frame(22.0, &mut |ui| scene(ui, st)).0, Repaint::Idle);
}

#[test]
fn mouvement_reduit_aucune_boucle_et_fondus_courts() {
    let mut h = Harness::new(Motion { reduced: true, slow: 1.0 });
    let mut scene = |ui: &mut egui::Ui| rest_scene(ui, State::Busy);
    assert_eq!(h.frame(0.0, &mut scene).0, Repaint::Now, "fondu d'apparition");
    // 120 ms plus tard tout est en place ; l'anneau « en cours » ne tourne pas.
    let (r, d) = h.frame(0.5, &mut scene);
    assert_eq!(r, Repaint::Idle);
    assert_eq!(d, Duration::MAX);
}

#[test]
fn toast_glisse_puis_barre_de_temps_puis_disparait() {
    let mut q = ui::Toasts::default();
    let id = q.push(1.0, "C'est fait.", "La version d'hier 21:14 est remise.", State::Safe, Some("Annuler"));
    // Le toast est affiche par `show` (zone flottante de l'image).
    let ctx = egui::Context::default();
    let mut clock = AnimClock::default();
    let mut step = |t: f64| {
        clock.begin_frame();
        let raw = egui::RawInput { time: Some(t), screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
        let _ = ctx.run(raw, |c| {
            ui::install(c, Tokens::default_dark(), false, Motion::default());
            q.show(c);
        });
        clock.merge(anim::take_needs(&ctx));
        clock.decide(t)
    };
    assert_eq!(step(1.0), Repaint::Now, "arrivee par le bas");
    assert_eq!(step(2.0), Repaint::After(Duration::from_secs_f64(1.0 / 30.0)), "barre de temps");
    assert_eq!(step(7.5), Repaint::Idle, "disparu apres 6 s");
    assert!(id > 0);
}

#[test]
fn logique_des_composants() {
    // Boutons lisibles quel que soit l'accent choisi (6 couleurs proposees + defaut).
    for accent in agss::vm::ACCENT_PRESETS.iter().chain([agss::settings::DEFAULT_ACCENT].iter()) {
        for dark in [true, false] {
            let t = Tokens::new(dark, *accent);
            for k in [Btn::Primary, Btn::Secondary, Btn::Danger, Btn::Ghost] {
                let (bg, hov, fg, _) = ui::button::colors(&t, k);
                assert!(agss::theme::contrast_ratio(fg, bg) >= 4.5 && agss::theme::contrast_ratio(fg, hov) >= 4.5, "{:?} accent {:?} ({})", k, accent, dark);
            }
        }
    }
    // Tuiles, grille, frise, interrupteur.
    assert_eq!(ui::tile::initials("Elden Ring"), "ER");
    assert_eq!(ui::tokens::grid_columns(900.0, false), 5);
    assert_eq!(ui::tokens::grid_columns(900.0, true), 4);
    let xs = ui::timeline::positions(&[ui::Mark::new(1, [0, 0, 0]), ui::Mark { conflict: true, current: true, ..ui::Mark::new(2, [0, 0, 0]) }]);
    assert!(xs[0] < 0.5 && xs[1] > 0.5);
    assert_eq!(ui::timeline::nearest(&xs, 0.9), Some(1));
    assert_eq!(ui::toggle::knob_x(10.0, 0.0), 13.0);
    // Etats tries pour l'accueil : a verifier, en cours, puis le reste.
    let mut v = vec![State::Safe, State::Busy, State::Problem, State::Todo];
    v.sort_by_key(|s| s.sort_rank());
    assert_eq!(v, vec![State::Problem, State::Todo, State::Busy, State::Safe]);
}
