use agss::statusfile::*;
use agss::vm::GameStatus;
use std::path::PathBuf;

fn doc_with(extra: GameExtra, devices: Vec<DeviceEntry>) -> StatusDoc {
    let games = vec![GameIn { name: "Jeu".into(), key: "jeu".into(), exe: None, install_dir: None, save_path: PathBuf::from("C_S"), status: GameStatus::UpToDate, last_sync_ms: Some(1_700_000_000_000) }];
    let mut doc = build(&Inputs { app_version: "2.0.1", now_ms: 1_700_000_100_000, watching: true, paused: false, reachable: true, shared: None, user: "u", games });
    enrich(&mut doc, &[extra], &GlobalExtra { machine: "PC-A".into(), devices, ..Default::default() });
    doc
}

#[test]
fn origine_serialisee_avec_couleur_et_date_iso() {
    let x = GameExtra { origin_device: Some("PC-B".into()), origin_device_color: Some("#D55E00".into()), ..Default::default() };
    let d = doc_with(x, vec![DeviceEntry { id: "PC-A".into(), name: "PC-A".into(), color: "#0072B2".into() }]);
    let v: serde_json::Value = serde_json::from_str(&to_json(&d)).unwrap();
    let g = &v["games"][0];
    assert_eq!((g["origin_device"].as_str(), g["origin_device_color"].as_str(), g["state"].as_str()), (Some("PC-B"), Some("#D55E00"), Some("synced")));
    let ts = g["last_sync_utc"].as_str().unwrap();
    assert!(ts.len() == 20 && ts.ends_with('Z') && ts.as_bytes()[10] == b'T', "{}", ts);
    assert_eq!(v["devices"][0]["name"], "PC-A");
    assert_eq!(v["devices"][0]["color"], "#0072B2");
}

#[test]
fn ancien_status_sans_nouveaux_champs_se_lit() {
    let d = doc_with(GameExtra::default(), vec![]);
    let mut v: serde_json::Value = serde_json::from_str(&to_json(&d)).unwrap();
    v["games"][0].as_object_mut().unwrap().remove("origin_device");
    v["games"][0].as_object_mut().unwrap().remove("origin_device_color");
    v.as_object_mut().unwrap().remove("devices");
    let back: StatusDoc = serde_json::from_value(v).unwrap();
    assert!(back.games[0].origin_device.is_none() && back.games[0].origin_device_color.is_none() && back.devices.is_empty());
    assert_eq!(back.games[0].state, "synced");
}

#[test]
fn origine_locale_ou_distante() {
    // A jour avec la tete du cloud venue d'un autre PC : l'origine est ce PC distant.
    assert_eq!(origin_device_name(true, Some("PC-B"), "PC-A"), "PC-B");
    // Tete produite ici : son propre nom.
    assert_eq!(origin_device_name(true, Some("PC-A"), "PC-A"), "PC-A");
    // Modifie localement / cloud plus recent / pas de tete / tete inconnue : la version en place vient d'ici.
    assert_eq!(origin_device_name(false, Some("PC-B"), "PC-A"), "PC-A");
    assert_eq!(origin_device_name(true, None, "PC-A"), "PC-A");
    assert_eq!(origin_device_name(true, Some("?"), "PC-A"), "PC-A");
}

#[test]
fn couleur_hex_valide_depuis_la_palette() {
    let c = agss::machinecolor::to_hex(agss::machinecolor::default_color("PC-B"));
    assert!(c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit()), "{}", c);
}
