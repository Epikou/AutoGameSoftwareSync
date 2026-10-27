//! Tests v0.5 : barre d'onglets / navigation manette, mode grand format automatique, contraste et
//! cerclage, infobulles, i18n des nouvelles chaines, etat de manette dynamique.
use eframe::egui;
use agss::gamepad::*;
use agss::i18n::{tr, Lang, ALL_KEYS, K};
use agss::settings::{BigChoice, Settings};
use agss::theme::{self, contrast_ratio};
use agss::uilogic::*;
use agss::vm::{Page, ACCENT_PRESETS};
use std::collections::VecDeque;
use std::path::Path;

// ================================================================== onglets et navigation manette

#[test]
fn ordre_des_onglets_et_navigation_circulaire() {
    assert_eq!(Page::ALL, [Page::Games, Page::Add, Page::Settings]);
    assert_eq!(tr(Lang::Fr, Page::Games.label()), "Mes synchros");
    assert_eq!(tr(Lang::Fr, Page::Add.label()), "Ajouter une synchro");
    assert_eq!(tr(Lang::Fr, Page::Settings.label()), "Réglages");
    assert_eq!(page_after(Page::Games, 1), Page::Add);
    assert_eq!(page_after(Page::Games, -1), Page::Settings);
    assert_eq!(page_after(Page::Settings, 1), Page::Games);
    assert_eq!(page_after(Page::Add, 2), Page::Games);
    assert_eq!(page_after(Page::Add, 0), Page::Add);
    assert_eq!(page_after(Page::Add, 3), Page::Add);
}

#[test]
fn la_manette_ne_change_d_onglet_que_par_lb_rb_et_jamais_par_le_dpad_ou_le_stick() {
    for n in [Nav::Up, Nav::Down, Nav::Left, Nav::Right, Nav::Confirm, Nav::Back] {
        assert!(matches!(nav_action(n, false), NavAction::Key(_)), "{:?} ne doit jamais changer d'onglet", n);
        assert!(matches!(nav_action(n, true), NavAction::Key(_)));
    }
    assert_eq!(nav_action(Nav::PagePrev, false), NavAction::Page(-1));
    assert_eq!(nav_action(Nav::PageNext, false), NavAction::Page(1));
    // Derriere un dialogue de confirmation, LB/RB sont ignores.
    assert_eq!(nav_action(Nav::PagePrev, true), NavAction::Ignore);
    assert_eq!(nav_action(Nav::PageNext, true), NavAction::Ignore);
    assert_eq!(nav_action(Nav::Up, false), NavAction::Key(NavKey::Up));
    assert_eq!(nav_action(Nav::Confirm, false), NavAction::Key(NavKey::Enter));
    assert_eq!(nav_action(Nav::Back, false), NavAction::Key(NavKey::Escape));
}

#[test]
fn les_onglets_ne_sont_pas_focalisables_le_curseur_reste_dans_l_onglet() {
    // Rendu egui sans fenetre : un « onglet » (meme Sense que l'interface) et un bouton de contenu.
    use egui::{Event, Key, Modifiers};
    assert!(!agss::gui::tab_sense().contains(egui::Sense::FOCUSABLE));
    let ctx = egui::Context::default();
    let mut tab_id = None;
    let mut btn_ids = Vec::new();
    let mut frame = |events: Vec<Event>| {
        let raw = egui::RawInput { events, ..Default::default() };
        let _ = ctx.run(raw, |ctx| {
            egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
                let (_, r) = ui.allocate_exact_size(egui::vec2(80.0, 24.0), agss::gui::tab_sense());
                tab_id = Some(r.id);
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                btn_ids.clear();
                btn_ids.push(ui.button("un").id);
                btn_ids.push(ui.button("deux").id);
            });
        });
    };
    let key = |k: Key| vec![Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: Modifiers::NONE }, Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: Modifiers::NONE }];
    frame(vec![]);
    frame(vec![]);
    let mut seen_focus = std::collections::HashSet::new();
    for k in [Key::Tab, Key::ArrowDown, Key::ArrowUp, Key::ArrowLeft, Key::ArrowRight, Key::Tab, Key::Tab, Key::ArrowUp, Key::ArrowUp, Key::ArrowLeft] {
        frame(key(k));
        frame(vec![]);
        if let Some(f) = ctx.memory(|m| m.focused()) {
            seen_focus.insert(f);
        }
    }
    assert!(!seen_focus.is_empty(), "le contenu doit pouvoir recevoir le focus");
    assert!(!seen_focus.contains(&tab_id.unwrap()), "le focus n'a jamais le droit d'atteindre un onglet");
    assert!(seen_focus.iter().all(|f| btn_ids.contains(f)));
}

// ================================================================== pictogrammes et mode grand format

fn pad(name: &str, readable: bool) -> PadStatus {
    PadStatus { info: PadInfo { id: name.into(), name: name.into(), conn: Connection::Unknown }, readable }
}

#[test]
fn pictogrammes_selon_le_type_de_manette() {
    assert_eq!(glyph_set(&[]), None);
    assert_eq!(glyph_set(&[pad("Xbox / XInput", true)]), Some(Glyphs::Xbox));
    assert_eq!(glyph_set(&[pad("DualSense", true)]), Some(Glyphs::PlayStation));
    assert_eq!(glyph_set(&[pad("DualSense Edge", true)]), Some(Glyphs::PlayStation));
    assert_eq!(glyph_set(&[pad("DualShock 4", true)]), Some(Glyphs::PlayStation));
    assert_eq!(glyph_set(&[pad("Manette inconnue", true)]), Some(Glyphs::Xbox), "generique = LB/RB");
    assert_eq!(glyph_set(&[pad("Xbox / XInput", true), pad("DualSense", true)]), Some(Glyphs::PlayStation));
    assert_eq!(glyph_set(&[pad("DualSense", false)]), None, "une manette illisible n'affiche pas de pictogrammes");
    assert_eq!(glyph_set(&[pad("DualSense", false), pad("Xbox / XInput", true)]), Some(Glyphs::Xbox));
    assert_eq!(shoulder_labels(Glyphs::Xbox), ("LB", "RB"));
    assert_eq!(shoulder_labels(Glyphs::PlayStation), ("L1", "R1"));
}

#[test]
fn mode_grand_format_auto_toujours_jamais() {
    for present in [false, true] {
        assert_eq!(effective_big(BigChoice::Auto, present), present);
        assert!(effective_big(BigChoice::Always, present));
        assert!(!effective_big(BigChoice::Never, present));
    }
    assert!(!pad_present(&[]));
    assert!(!pad_present(&[pad("DualSense", false)]));
    assert!(pad_present(&[pad("DualSense", false), pad("Xbox / XInput", true)]));
}

#[test]
fn reglage_grand_format_auto_par_defaut_et_compatibilite_anciens_fichiers() {
    let s = Settings::default();
    assert_eq!(s.big_choice(), BigChoice::Auto);
    // Ancien settings.json : big_mode true => « Toujours » ; absent/false => « Auto ».
    let old: Settings = serde_json::from_str(r#"{"big_mode":true}"#).unwrap();
    assert_eq!(old.big_choice(), BigChoice::Always);
    let old: Settings = serde_json::from_str(r#"{"big_mode":false}"#).unwrap();
    assert_eq!(old.big_choice(), BigChoice::Auto);
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.big_choice(), BigChoice::Auto);
    let mut s = Settings::default();
    for c in [BigChoice::Never, BigChoice::Always, BigChoice::Auto, BigChoice::Never] {
        s.set_big_choice(c);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.big_choice(), c);
    }
}

struct Script(VecDeque<Vec<PadPoll>>);
impl PadSource for Script {
    fn poll(&mut self, _now: u64) -> Vec<PadPoll> {
        self.0.pop_front().unwrap_or_default()
    }
}

#[test]
fn deconnexion_met_l_etat_a_aucune_et_le_mode_grand_format_revient_au_choix_utilisateur() {
    let ds = PadInfo { id: "hid:1".into(), name: "DualSense".into(), conn: Connection::Bluetooth };
    let on = || vec![PadPoll { info: ds.clone(), readable: true, states: vec![PadState::default()] }];
    let mut hub = Hub::new(vec![Box::new(Script(VecDeque::from(vec![on(), on(), vec![], vec![]])))]);
    let big = |hub: &Hub, c: BigChoice| effective_big(c, pad_present(&hub.status()));
    // Branchee : ligne « DualSense (Bluetooth) », pictogrammes L1/R1, Auto => grand format.
    hub.tick(0);
    assert_eq!(status_line(&hub.status(), "Aucune", "x"), "DualSense (Bluetooth)");
    assert_eq!(glyph_set(&hub.status()), Some(Glyphs::PlayStation));
    assert!(big(&hub, BigChoice::Auto));
    assert!(!big(&hub, BigChoice::Never), "un choix « Jamais » reste respecte avec une manette");
    hub.tick(10);
    // Debranchee : « Aucune » des le cycle suivant, plus de pictogrammes, retour au mode choisi.
    let o = hub.tick(20);
    assert!(matches!(o.changes.as_slice(), [PadChange::Disconnected(_)]));
    assert_eq!(status_line(&hub.status(), "Aucune", "x"), "Aucune");
    assert_eq!(glyph_set(&hub.status()), None);
    assert!(!big(&hub, BigChoice::Auto));
    assert!(big(&hub, BigChoice::Always));
}

#[test]
fn silence_d_une_manette_qui_diffuse_en_continu_vaut_deconnexion() {
    assert!(is_streaming_report(Connection::Usb, 0x01));
    assert!(is_streaming_report(Connection::Bluetooth, 0x31));
    assert!(is_streaming_report(Connection::Bluetooth, 0x11));
    assert!(!is_streaming_report(Connection::Bluetooth, 0x01), "mode simple : silence normal");
    assert!(!stream_silent(true, 1_000, 2_900));
    assert!(stream_silent(true, 1_000, 3_100));
    assert!(!stream_silent(false, 0, 1_000_000));
    // Filet de securite : enumeration complete rare, jamais en boucle serree.
    assert!(hid_scan_interval_ms(true, true) >= 10_000);
}

// ================================================================== contraste et cerclage

#[test]
fn contraste_wcag_valeurs_connues() {
    assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
    assert!((contrast_ratio([255, 255, 255], [0, 0, 0]) - 21.0).abs() < 0.01, "symetrique");
    assert!((contrast_ratio([120, 33, 7], [120, 33, 7]) - 1.0).abs() < 1e-4);
    // Gris #777 sur blanc : ~4.48 (valeur de reference WCAG).
    assert!((contrast_ratio([0x77, 0x77, 0x77], [255, 255, 255]) - 4.48).abs() < 0.02);
    assert_eq!(theme::mix([0, 0, 0], [200, 100, 50], 0.0), [0, 0, 0]);
    assert_eq!(theme::mix([0, 0, 0], [200, 100, 50], 1.0), [200, 100, 50]);
    assert_eq!(theme::mix([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
}

fn all_accents() -> Vec<[u8; 3]> {
    let mut v: Vec<[u8; 3]> = ACCENT_PRESETS.to_vec();
    for r in (0..=255).step_by(51) {
        for g in (0..=255).step_by(51) {
            for b in (0..=255).step_by(51) {
                v.push([r as u8, g as u8, b as u8]);
            }
        }
    }
    v.extend([[0x1b, 0x1b, 0x1b], [0xf8, 0xf8, 0xf8], [0x3c, 0x3c, 0x3c], [0xe6, 0xe6, 0xe6]]); // accents proches des fonds
    v
}

#[test]
fn cerclage_et_textes_suffisamment_contrastes_dans_tous_les_themes_et_accents() {
    let mut n = 0;
    for dark in [true, false] {
        for accent in all_accents() {
            let p = theme::palette(dark, accent);
            // Cerclage 2 px : >= 3:1 contre le fond de page ET contre le fond des widgets.
            for bg in [p.panel_bg, p.widget_bg] {
                assert!(contrast_ratio(p.ring, bg) >= theme::RING_MIN_CONTRAST, "ring {:?} sur {:?} (sombre={}, accent {:?}) = {:.2}", p.ring, bg, dark, accent, contrast_ratio(p.ring, bg));
            }
            // Texte sur accent (onglet / element selectionne) : >= 4.5:1.
            assert!(contrast_ratio(p.on_accent, p.accent) >= theme::TEXT_MIN_CONTRAST, "texte sur accent {:?}", accent);
            // Texte sur tous les fonds de widgets, y compris survol et focus.
            for bg in [p.panel_bg, p.widget_bg, p.hover_fill, p.active_fill] {
                assert!(contrast_ratio(p.text, bg) >= theme::TEXT_MIN_CONTRAST, "texte {:?} sur {:?} (sombre={}, accent {:?}) = {:.2}", p.text, bg, dark, accent, contrast_ratio(p.text, bg));
            }
            // Le survol / le focus changent bien le fond (visible), sans le confondre avec le fond de page.
            assert_ne!(p.hover_fill, p.widget_bg);
            assert_ne!(p.active_fill, p.widget_bg);
            n += 1;
        }
    }
    assert!(n > 400);
}

#[test]
fn le_cerclage_prend_l_accent_s_il_contraste_sinon_la_couleur_du_texte() {
    let dark = theme::palette(true, [0x4c, 0x8d, 0xf6]);
    assert_eq!(dark.ring, [0x4c, 0x8d, 0xf6], "accent bleu assez contraste sur fond sombre");
    let light_orange = theme::palette(false, [240, 140, 44]);
    assert_eq!(light_orange.ring, light_orange.text, "orange trop clair sur fond clair : repli sur le texte");
    let dark_navy = theme::palette(true, [10, 10, 40]);
    assert_eq!(dark_navy.ring, dark_navy.text);
}

// ================================================================== infobulles

#[test]
fn delai_des_infobulles() {
    assert_eq!(TOOLTIP_DELAY_MS, 1000);
    assert!(!tooltip_due(0));
    assert!(!tooltip_due(999));
    assert!(tooltip_due(1000));
    assert!(tooltip_due(5000));
}

// ================================================================== i18n

fn gui_source() -> String {
    // gui.rs + ses sous-modules (src/gui/*.rs).
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut s = std::fs::read_to_string(src.join("gui.rs")).unwrap();
    for e in walkdir::WalkDir::new(src.join("gui")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        s.push_str("
");
        s.push_str(&std::fs::read_to_string(e.path()).unwrap());
    }
    s
}

#[test]
fn infobulles_toutes_traduites_et_toutes_utilisees() {
    let src = gui_source();
    let mut n = 0;
    for k in ALL_KEYS {
        let name = format!("{:?}", k);
        if !name.starts_with("Tip") {
            continue;
        }
        n += 1;
        let (fr, en) = (tr(Lang::Fr, *k), tr(Lang::En, *k));
        assert!(fr.len() > 10 && en.len() > 10, "{} trop court", name);
        assert_ne!(fr, en, "{} identique en FR et EN", name);
        assert!(fr.ends_with('.') && en.ends_with('.'), "{} doit finir par un point", name);
        // Une variante Linux est utilisee a travers la cle qu'elle remplace (`linux_variant`).
        let via_variant = ALL_KEYS.iter().any(|b| b != k && agss::i18n::linux_variant(*b) == *k && src.contains(&format!("K::{:?}", b)));
        assert!(src.contains(&format!("K::{}", name)) || via_variant, "{} n'est utilisee nulle part dans gui.rs", name);
    }
    assert!(n >= 50, "{} infobulles seulement", n);
    // Textes de reference demandes.
    assert_eq!(tr(Lang::Fr, K::TipBackup), "Copie la sauvegarde actuelle de cette synchro vers votre dossier partagé (NAS, Google Drive, Dropbox, etc.).");
    assert_eq!(tr(Lang::Fr, K::TipVersions), "Affiche l'historique des anciennes versions de cette sauvegarde.");
    assert!(tr(Lang::Fr, K::TipRestore).contains("copie de sécurité"));
}

#[test]
fn nouvelles_chaines_fr_en() {
    for k in [K::SetBigAuto, K::SetBigAlways, K::SetBigNever, K::AddStep1] {
        assert_ne!(tr(Lang::Fr, k), tr(Lang::En, k), "{:?}", k);
    }
    assert_eq!(tr(Lang::En, K::SetBigNever), "Never");
}

#[test]
fn aucun_texte_en_dur_dans_l_interface() {
    // Tout libelle visible passe par tr()/self.t() : on refuse les litteraux directs dans les appels
    // d'affichage courants. Exceptions voulues : noms de langue (endonymes), filtres de fichiers.
    let src = gui_source();
    let banned = ["label(\"", "button(\"", "Button::new(\"", "heading(\"", "on_hover_text(\"", "selected_text(\"", "colored_label(Color32::from_rgb(230, 170, 40), \"", "checkbox(&mut aw, \"", "RichText::new(\"", "selectable_label(", "show_tooltip_text(\""];
    for (i, line) in src.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        for b in banned {
            if !line.contains(b) {
                continue;
            }
            if b == "selectable_label(" {
                // autorise uniquement un identifiant / expression, pas un litteral de texte (hors noms de langue)
                let after = &line[line.find(b).unwrap() + b.len()..];
                if after.contains(", \"") && !after.contains("\"English\"") && !after.contains("n)") {
                    panic!("gui.rs:{} texte en dur : {}", i + 1, line.trim());
                }
                continue;
            }
            panic!("gui.rs:{} texte en dur : {}", i + 1, line.trim());
        }
    }
}
