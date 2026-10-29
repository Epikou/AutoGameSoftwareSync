//! Lot 17 (Android) : correspondance PC <-> Android des profils d'emulateurs et aller-retour du coffre entre un « PC » et
//! une « console Android » simules. UNIQUEMENT des dossiers temporaires.
use agss::backup::{self, BackupOutcome, Plan};
use agss::cloud;
use agss::config::Game;
use agss::emulators::{self, Compat, EMULATORS};
use agss::process::ProcessChecker;
use agss::profiles::{self, Vars};
use agss::{Ctx, Res};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct Closed;
impl ProcessChecker for Closed {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(false)
    }
}

fn ctx(root: &Path, name: &str) -> Ctx {
    let mut c = Ctx::new(root.join(format!("data-{}", name)));
    c.machine = name.to_string();
    c.procs = Box::new(Closed);
    std::fs::create_dir_all(&c.data_dir).unwrap();
    c
}

fn write(p: &Path, content: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

/// Element PC d'un emulateur : profil embarque deplie avec un dossier Documents simule (ou `<install>` pour RetroArch).
fn pc_game(id: &str, docs: &Path, enabled: &[bool]) -> Game {
    let p = profiles::load_embedded().into_iter().find(|p| p.id == id).unwrap();
    let mut values = BTreeMap::new();
    values.insert("install".to_string(), docs.join("RetroArch").to_string_lossy().to_string());
    values.insert("user".to_string(), docs.join("Azahar").to_string_lossy().to_string());
    let kf = agss::hints::KnownFolders { documents: Some(docs.to_path_buf()), roaming_app_data: Some(docs.to_path_buf()), ..Default::default() };
    let v = Vars { kf, values, ..Default::default() };
    profiles::to_game(&p, &v, enabled).unwrap()
}

#[test]
fn table_des_emulateurs_coherente_avec_les_profils_pc() {
    assert!(EMULATORS.len() >= 6);
    let mut ids = std::collections::HashSet::new();
    for e in EMULATORS {
        assert!(ids.insert(e.id), "identifiant en double : {}", e.id);
        let pc = emulators::pc_profile(e).unwrap_or_else(|| panic!("profil PC embarque manquant : {}", e.id));
        assert_eq!(pc.category, "emulator", "{}", e.id);
        assert_eq!(pc.locations.len(), e.locations.len(), "{} : nombre d'emplacements", e.id);
        for (l, a) in pc.locations.iter().zip(e.locations.iter()) {
            assert_eq!(l.label, a.label, "{} : libelle different entre le profil PC et la table Android", e.id);
            assert!(!a.rel.is_empty() && !a.rel.starts_with('/') && !a.rel.contains('\\'), "{} : sous-dossier Android relatif avec des /", e.id);
        }
        assert!(!e.android_roots.is_empty(), "{} : aucun dossier Android (un emulateur Android/data seulement ne doit pas etre dans la table)", e.id);
        assert!(e.android_roots.iter().all(|r| !r.starts_with('/') && !r.starts_with("Android/data")), "{} : dossiers relatifs, jamais Android/data", e.id);
        assert!(!e.note.is_empty());
        // Le profil « vu de la console » est un profil valide (memes regles que les profils PC).
        let ap = emulators::android_profile(e).unwrap();
        assert!(profiles::validate(&ap).is_empty(), "{} : {:?}", e.id, profiles::validate(&ap));
        assert_eq!(ap.name, pc.name);
        assert!(ap.locations.iter().all(|l| l.path.starts_with("<root>/")));
        // Les etats instantanes ne sont jamais coches par defaut (gros et lies a la version de l'emulateur).
        for l in &ap.locations {
            if l.label.to_lowercase().contains("state") {
                assert!(!l.default_enabled, "{} : « {} » doit etre decoche par defaut", e.id, l.label);
            }
        }
        if e.compat == Compat::Same {
            assert!(e.id == "ppsspp", "seul PPSSPP est declare strictement identique (a elargir avec des preuves)");
        }
    }
    // Le tableau de la documentation couvre toute la table.
    assert_eq!(emulators::compat_table().len(), EMULATORS.len());
}

#[test]
fn meme_element_sur_pc_et_sur_android_meme_cle_memes_emplacements() {
    let t = TempDir::new().unwrap();
    let docs = t.path().join("Documents");
    for e in EMULATORS {
        let all = vec![true; e.locations.len()];
        let pc = pc_game(e.id, &docs, &all);
        let root = t.path().join("storage").join(e.android_roots[0]);
        let an = emulators::android_game(e, &root, &all).unwrap();
        assert_eq!(pc.name, an.name, "{} : le nom (donc la cle du coffre) doit etre identique", e.id);
        assert_eq!(cloud::dest_key(&pc), cloud::dest_key(&an));
        assert_eq!(pc.primary_label, an.primary_label);
        assert_eq!(pc.save_is_file, an.save_is_file);
        assert_eq!(pc.save_filter, an.save_filter, "{} : memes motifs inclus", e.id);
        assert_eq!(pc.save_exclude, an.save_exclude);
        assert_eq!(pc.extra_locations.len(), an.extra_locations.len());
        for (a, b) in pc.extra_locations.iter().zip(an.extra_locations.iter()) {
            assert_eq!(a.id, b.id, "{} : identifiant d'emplacement (sous-espace files/@id) identique", e.id);
            assert_eq!(a.label, b.label);
            assert_eq!(a.filter, b.filter);
            assert_eq!(a.is_file, b.is_file);
        }
        assert!(an.exe.is_none(), "la console ne surveille aucun programme");
        assert!(an.path.starts_with(&root));
        // Valeurs par defaut (cases du profil) : meme emplacement principal des deux cotes.
        let pc_d = pc_game(e.id, &docs, &[]);
        let an_d = emulators::android_game(e, &root, &[]).unwrap();
        assert_eq!(pc_d.primary_label, an_d.primary_label);
        assert_eq!(pc_d.extra_locations.len(), an_d.extra_locations.len());
        // Le nom affiche sur la console ne change jamais la cle.
        assert_eq!(cloud::dest_key(&an_d), cloud::game_key(&an_d.name));
    }
}

#[test]
fn detection_sur_la_console_dossiers_presents_seulement() {
    let t = TempDir::new().unwrap();
    let internal = t.path().join("emulated/0");
    let sdcard = t.path().join("1234-5678");
    std::fs::create_dir_all(internal.join("RetroArch/saves")).unwrap();
    std::fs::create_dir_all(internal.join("duckstation")).unwrap(); // vide : appli desinstallee, pas propose
    std::fs::create_dir_all(sdcard.join("PSP/SAVEDATA")).unwrap();
    std::fs::create_dir_all(sdcard.join("PSP/PPSSPP_STATE")).unwrap();
    std::fs::create_dir_all(internal.join("Android/data/org.dolphinemu.dolphinemu/files/GC")).unwrap(); // jamais regarde
    let found = emulators::find_on_device(&[internal.clone(), sdcard.clone()]);
    let ids: Vec<&str> = found.iter().map(|f| f.emu.id).collect();
    assert_eq!(ids, vec!["retroarch", "ppsspp"]);
    assert_eq!(found[0].root, internal.join("RetroArch"));
    assert_eq!(found[0].present, vec![true, false]);
    assert_eq!(found[1].root, sdcard.join("PSP"));
    assert_eq!(found[1].present, vec![true, true]);
    assert!(emulators::find_on_device(&[t.path().join("nulle-part")]).is_empty());
    // Dossiers perso : jamais rien de sensible, chemins relatifs.
    for (n, rel) in emulators::PERSONAL_FOLDERS {
        assert!(!n.is_empty() && !rel.starts_with('/'));
        let g = emulators::personal_game(n, &internal.join(rel));
        assert!(g.exe.is_none() && g.extra_locations.is_empty());
    }
}

/// Un PC et une console partagent le MEME coffre et la MEME cle : la partie continue de l'un a l'autre, chaque cote garde
/// une copie de securite avant d'ecraser, aucune version n'est jamais supprimee.
#[test]
fn aller_retour_coffre_entre_pc_et_console_sans_perte() {
    let t = TempDir::new().unwrap();
    let vault = t.path().join("Sync/AutoGameSoftwareSync");
    std::fs::create_dir_all(&vault).unwrap();
    let docs = t.path().join("pc/Documents");
    let storage = t.path().join("android/storage/emulated/0");
    let e = emulators::by_id("retroarch").unwrap();
    let both = [true, true];
    let pc_ctx = ctx(t.path(), "PC-BUREAU");
    let an_ctx = ctx(t.path(), "Retroid Pocket 4");
    let pc = pc_game("retroarch", &docs, &both);
    let an = emulators::android_game(e, &storage.join("RetroArch"), &both).unwrap();
    assert_eq!(cloud::dest_key(&pc), cloud::dest_key(&an));
    let srm_pc = pc.path.join("Celeste.srm");
    let state_pc = pc.extra_locations[0].path.join("Celeste.state");
    let srm_an = an.path.join("Celeste.srm");
    let state_an = an.extra_locations[0].path.join("Celeste.state");

    // 1. Partie commencee sur le PC.
    write(&srm_pc, "partie-pc-1");
    write(&state_pc, "etat-pc-1");
    assert!(matches!(backup::backup(&pc_ctx, &pc, &vault).unwrap(), BackupOutcome::Backed { .. }));
    // 2. La console n'a encore rien : elle doit RESTAURER (jamais ecraser le coffre avec du vide).
    std::fs::create_dir_all(&an.path).unwrap();
    std::fs::create_dir_all(&an.extra_locations[0].path).unwrap();
    assert!(backup::backup(&an_ctx, &an, &vault).is_err(), "un dossier vide ne remplace jamais une sauvegarde");
    assert_eq!(backup::plan(&an_ctx, &an, &vault).unwrap(), Plan::Restore);
    let r = backup::restore(&an_ctx, &an, &vault).unwrap();
    assert_eq!(r.files_restored, 2);
    assert_eq!(std::fs::read_to_string(&srm_an).unwrap(), "partie-pc-1");
    assert_eq!(std::fs::read_to_string(&state_an).unwrap(), "etat-pc-1", "l'emplacement supplementaire (files/@id) est retrouve sur la console");
    // Point de synchro : rien de nouveau a ecrire.
    assert!(matches!(backup::backup(&an_ctx, &an, &vault).unwrap(), BackupOutcome::UpToDate));
    // 3. On joue sur la console.
    write(&srm_an, "partie-console-2");
    assert!(matches!(backup::backup(&an_ctx, &an, &vault).unwrap(), BackupOutcome::Backed { .. }));
    // 4. Retour sur le PC : la console a une version plus recente -> restauration avec copie de securite.
    assert_eq!(backup::plan(&pc_ctx, &pc, &vault).unwrap(), Plan::Restore);
    let r = backup::restore(&pc_ctx, &pc, &vault).unwrap();
    assert!(r.safety_dir.is_some(), "copie de securite AVANT d'ecraser la version du PC");
    assert_eq!(std::fs::read_to_string(&srm_pc).unwrap(), "partie-console-2");
    assert_eq!(std::fs::read_to_string(&state_pc).unwrap(), "etat-pc-1");
    // Historique : 2 versions, une par appareil, rien de supprime ; la plus recente vient de la console.
    let h = backup::history(&pc_ctx, &pc, &vault);
    assert_eq!(h.rows.len(), 2);
    assert_eq!(h.rows[0].machine, "Retroid Pocket 4");
    assert_eq!(h.rows[1].machine, "PC-BUREAU");
    assert!(h.rows[0].active && h.rows[0].is_local);
    assert!(h.conflict_heads.is_empty());
    let machines = cloud::machines_seen(&vault);
    assert!(machines.contains(&"PC-BUREAU".to_string()) && machines.contains(&"Retroid Pocket 4".to_string()));
    // Marques de position : les deux appareils sont a la version active.
    let key = cloud::dest_key(&pc);
    let vid = h.rows[0].id.clone();
    assert!(agss::home::note_position(&vault, &pc_ctx.machine, &key, &vid, 1_000));
    assert!(agss::home::note_position(&vault, &an_ctx.machine, &key, &vid, 2_000));
    let pos = agss::home::positions(&vault, &key);
    assert_eq!(pos.len(), 2);
    assert!(pos.iter().all(|(_, v)| v == &vid));
    // 5. Remonter le temps sur la console : revenir a la version du PC, copie de securite, puis annuler.
    let old = h.rows[1].name.clone();
    let (out, ticket) = backup::restore_version_undoable(&an_ctx, &an, &vault, &old).unwrap();
    assert!(out.safety_dir.is_some());
    assert_eq!(std::fs::read_to_string(&srm_an).unwrap(), "partie-pc-1");
    let u = backup::undo_restore(&an_ctx, &an, Some(&vault), &ticket).unwrap();
    assert!(u.files_put_back >= 1);
    assert_eq!(std::fs::read_to_string(&srm_an).unwrap(), "partie-console-2");
    // Toujours aucune version supprimee.
    assert!(backup::history(&an_ctx, &an, &vault).rows.len() >= 2);
}

#[test]
fn deux_appareils_modifient_en_meme_temps_les_deux_versions_restent() {
    let t = TempDir::new().unwrap();
    let vault = t.path().join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    let e = emulators::by_id("duckstation").unwrap();
    let pc_ctx = ctx(t.path(), "PC");
    let an_ctx = ctx(t.path(), "Odin");
    let pc = pc_game("duckstation", &t.path().join("Documents"), &[]);
    let an = emulators::android_game(e, &t.path().join("sd/duckstation"), &[]).unwrap();
    assert_eq!(pc.save_filter.as_deref(), Some("*.mcd"));
    assert_eq!(an.save_filter.as_deref(), Some("*.mcd"));
    write(&pc.path.join("shared_card_1.mcd"), "v1");
    write(&pc.path.join("notes.txt"), "ignore : hors filtre");
    backup::backup(&pc_ctx, &pc, &vault).unwrap();
    std::fs::create_dir_all(&an.path).unwrap();
    backup::restore(&an_ctx, &an, &vault).unwrap();
    assert!(!an.path.join("notes.txt").exists(), "le filtre *.mcd s'applique des deux cotes");
    // Les deux jouent sans se synchroniser.
    write(&pc.path.join("shared_card_1.mcd"), "v2-pc");
    write(&an.path.join("shared_card_1.mcd"), "v2-console");
    backup::backup(&pc_ctx, &pc, &vault).unwrap();
    assert_eq!(backup::plan(&an_ctx, &an, &vault).unwrap(), Plan::Conflict);
    let o = backup::backup(&an_ctx, &an, &vault).unwrap();
    assert!(matches!(o, BackupOutcome::Conflict { .. }));
    let h = backup::history(&an_ctx, &an, &vault);
    assert_eq!(h.conflict_heads.len(), 2, "les deux versions sont gardees, rien n'est ecrase");
    assert_eq!(std::fs::read_to_string(an.path.join("shared_card_1.mcd")).unwrap(), "v2-console", "le local n'est jamais touche sans decision");
    let _: PathBuf = vault;
}
