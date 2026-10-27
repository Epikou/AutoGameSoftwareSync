//! v0.10 phase C : 5 langues (English, Francais, Russe, Espagnol, Chinois), drapeaux dessines, police CJK systeme.
//! Les traductions RU / ES / ZH sont automatiques et n'ont pas ete relues par des locuteurs natifs.
use agss::i18n::{self, cjk_font_available, set_cjk_font_available, tr, Lang, ALL_KEYS, K};
use agss::settings::Settings;
use agss::timefmt::{self, FixedTz};
use agss::uilogic::{cjk_font_candidates, needs_cjk_font, pick_font};
use std::path::{Path, PathBuf};

/// Texte d'une langue SANS passer par le repli CJK (insensible au drapeau global de police).
fn raw(l: Lang, k: K) -> &'static str {
    match l {
        Lang::Zh => agss::i18n_zh::tr(k),
        other => tr(other, k),
    }
}

fn count(s: &str, pat: &str) -> usize {
    s.matches(pat).count()
}

#[test]
fn cinq_langues_aucune_cle_manquante_ni_vide() {
    assert_eq!(Lang::ALL.len(), 5);
    assert_eq!(Lang::ALL.map(|l| l.code()), ["en", "fr", "ru", "es", "zh"]);
    assert!(ALL_KEYS.len() > 300);
    for k in ALL_KEYS {
        for l in Lang::ALL {
            let s = raw(l, *k);
            assert!(!s.trim().is_empty(), "{:?} vide en {}", k, l.code());
            assert!(!s.contains("TODO") && !s.contains("???"), "{:?} {} : {:?}", k, l.code(), s);
        }
    }
}

#[test]
fn marqueurs_et_sauts_de_ligne_identiques_dans_toutes_les_langues() {
    for k in ALL_KEYS {
        let en = raw(Lang::En, *k);
        for l in Lang::ALL {
            let s = raw(l, *k);
            for m in ["{}", "{0}", "{1}", "\n"] {
                assert_eq!(count(s, m), count(en, m), "{:?} {} : marqueur {:?} different\n  EN: {}\n  {}: {}", k, l.code(), m, en, l.code(), s);
            }
        }
    }
}

#[test]
fn infobulles_finissent_par_un_point_dans_toutes_les_langues() {
    let mut n = 0;
    for k in ALL_KEYS {
        if !format!("{:?}", k).starts_with("Tip") {
            continue;
        }
        n += 1;
        for l in Lang::ALL {
            let s = raw(l, *k);
            assert!(s.ends_with('.') || s.ends_with('。'), "{:?} {} doit finir par un point : {:?}", k, l.code(), s);
            assert!(s.chars().count() > if l == Lang::Zh { 4 } else { 8 }, "{:?} {} trop court", k, l.code());
        }
    }
    assert!(n >= 80, "{}", n);
}

/// Chaines volontairement identiques a l'anglais DANS UNE LANGUE (noms propres, abreviations, mots internationaux).
/// Liste blanche explicite par langue : toute autre chaine fr/es/ru/zh egale a l'anglais fait echouer le test.
fn allowed_same(l: Lang, name: &str) -> bool {
    // Mots identiques legitimes : noms propres (DualSense, PC) et mots que la langue partage avec l anglais.
    // 2.0 (lot 8) : « Documents » (fr), « LocalLow » et « Proton » (noms de dossiers et de produit, jamais traduits).
    // 2.0 (lot 9) : « Documents » (fr), nom du dossier perso.
    // 2.0 (lot 7) : « Options » (mot commun fr/en) et formats de date de la frise faits de marqueurs seuls ({0} {1}).
    let fr: &[&str] = &["PadDualSense", "ProfCatConfig", "PrevLocal", "PrevCloud", "SetImportExport", "SetNotifGame", "ReactNormal", "ConfTitle", "VOptions", "VDayWeekday", "VPickDocuments", "VPickLocalLow", "VPickProton", "VPersDocuments"];
    let es: &[&str] = &["PadDualSense", "PrevLocal", "StatusError", "OptNo", "ReactNormal", "VDayWeekday", "VPickLocalLow", "VPickProton"];
    let ru: &[&str] = &["PadDualSense", "VDayWeekday", "VDayDate", "VDayDateYear", "VPickLocalLow", "VPickProton"];
    let zh: &[&str] = &["PadDualSense", "VAtTime", "VPickLocalLow", "VPickProton"];
    match l {
        Lang::Fr => fr.contains(&name),
        Lang::Es => es.contains(&name),
        Lang::Ru => ru.contains(&name),
        Lang::Zh => zh.contains(&name),
        Lang::En => true,
    }
}

#[test]
fn traductions_differentes_de_l_anglais_et_dans_le_bon_alphabet() {
    let is_cyr = |s: &str| s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
    let is_cjk = |s: &str| s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
    let mut same: Vec<String> = Vec::new();
    let mut alpha: Vec<String> = Vec::new();
    for k in ALL_KEYS {
        let name = format!("{:?}", k);
        let en = raw(Lang::En, *k);
        for l in [Lang::Fr, Lang::Ru, Lang::Es, Lang::Zh] {
            let s = raw(l, *k);
            if !allowed_same(l, &name) && s == en {
                same.push(format!("{}:{}", l.code(), name));
            }
        }
        if !allowed_same(Lang::Ru, &name) && !is_cyr(raw(Lang::Ru, *k)) {
            alpha.push(format!("ru:{}", name));
        }
        if !allowed_same(Lang::Zh, &name) && !is_cjk(raw(Lang::Zh, *k)) {
            alpha.push(format!("zh:{}", name));
        }
    }
    assert!(same.is_empty(), "identiques a l'anglais : {:?}", same);
    assert!(alpha.is_empty(), "mauvais alphabet : {:?}", alpha);
}

#[test]
fn noms_de_langue_dans_leur_propre_langue() {
    assert_eq!(Lang::En.native_name(), "English");
    assert_eq!(Lang::Fr.native_name(), "Français");
    assert_eq!(Lang::Ru.native_name(), "Русский");
    assert_eq!(Lang::Es.native_name(), "Español");
    assert_eq!(Lang::Zh.native_name(), "中文");
}

#[test]
fn reglage_de_langue_serialise_et_anglais_par_defaut() {
    for (l, code) in [(Lang::En, "en"), (Lang::Fr, "fr"), (Lang::Ru, "ru"), (Lang::Es, "es"), (Lang::Zh, "zh")] {
        let s = Settings { lang: l, ..Settings::default() };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains(&format!("\"lang\":\"{}\"", code)), "{}", j);
        let back: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(back.lang, l);
    }
    assert_eq!(Settings::default().lang, Lang::En, "English par defaut a la premiere installation");
    let old: Settings = serde_json::from_str(r#"{"lang":"fr"}"#).unwrap();
    assert_eq!(old.lang, Lang::Fr, "anciens fichiers inchanges");
}

#[test]
fn chinois_replie_sur_l_anglais_sans_police_cjk() {
    // Seul test qui touche au drapeau global : il le rend toujours a true.
    let k = K::NavSettings;
    assert!(cjk_font_available());
    assert_eq!(tr(Lang::Zh, k), "设置");
    set_cjk_font_available(false);
    assert_eq!(tr(Lang::Zh, k), tr(Lang::En, k), "repli lisible");
    assert_eq!(tr(Lang::Ru, k), "Настройки", "les autres langues ne sont pas touchees");
    set_cjk_font_available(true);
    assert_eq!(tr(Lang::Zh, k), "设置");
}

#[cfg(windows)]
#[test]
fn police_chinoise_systeme_candidates_et_choix() {
    let c = cjk_font_candidates(Path::new("C:\\Windows"));
    assert_eq!(c[0], PathBuf::from("C:\\Windows\\Fonts\\msyh.ttc"), "Microsoft YaHei d'abord");
    let names: Vec<String> = c.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    for must in ["msyhbd.ttc", "simsun.ttc"] {
        assert!(names.iter().any(|n| n == must), "{}", must);
    }
    assert!(names.iter().any(|n| n.starts_with("NotoSansSC")));
    // Premiere existante.
    let only_simsun = |p: &Path| p.file_name().map(|n| n == "simsun.ttc").unwrap_or(false);
    assert_eq!(pick_font(&c, &only_simsun).unwrap().file_name().unwrap(), "simsun.ttc");
    assert_eq!(pick_font(&c, &|_| true).unwrap(), c[0]);
    assert_eq!(pick_font(&c, &|_| false), None, "aucune police : le chinois se replie sur l'anglais");
    // Chargement seulement quand utile (rien n'est charge pour rien).
    assert!(!needs_cjk_font(false, false, false));
    assert!(needs_cjk_font(true, false, false) && needs_cjk_font(false, true, false) && needs_cjk_font(false, false, true));
}

#[test]
fn dates_selon_la_langue() {
    let utc = 1_830_216_255_000u64; // 2027-12-31 01:24:15 UTC
    let tz = FixedTz(3600);
    assert_eq!(timefmt::format_local(utc, Lang::Fr, &tz), "31/12/2027 02:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::Es, &tz), "31/12/2027 02:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::Ru, &tz), "31.12.2027 02:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::En, &tz), "2027-12-31 02:24:15");
    assert_eq!(timefmt::format_local(utc, Lang::Zh, &tz), "2027-12-31 02:24:15");
}

#[test]
fn drapeaux_peints_pour_les_cinq_langues_et_aucun_emoji() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut src = std::fs::read_to_string(root.join("gui.rs")).unwrap();
    for e in walkdir::WalkDir::new(root.join("gui")).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
        src.push_str(&std::fs::read_to_string(e.path()).unwrap());
    }
    for f in ["fn paint_flag_en", "fn paint_flag_fr", "fn paint_flag_ru", "fn paint_flag_es", "fn paint_flag_zh", "fn paint_flag("] {
        assert!(src.contains(f), "{}", f);
    }
    assert!(!src.chars().any(|c| ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)), "pas d'emoji de drapeau");
    // Aucune police embarquee dans l'exe pour le chinois.
    assert!(!src.contains("include_bytes!(\"../media/") || !src.to_lowercase().contains(".ttf"));
    let _ = i18n::ALL_KEYS.len();
}
