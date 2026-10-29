//! 2.0 (lot 7) : fiche d'un element et « Remonter le temps ». Restauration ANNULABLE (etat identique octet par octet
//! apres « Annuler », coffre et etat de synchro d'avant retrouves, rien n'est jamais supprime), garde « programme en
//! cours », copies de securite, logique de la frise (jours, couleurs, branches), synchro par appareil, lieux lisibles.
//! UNIQUEMENT des dossiers temporaires.
use agss::backup::{self, History, HistoryRow, Plan, SafetyCopy};
use agss::cloud::VKind;
use agss::config::{Game, State};
use agss::home::{Facts, Head, Kind};
use agss::process::ProcessChecker;
use agss::timefmt::FixedTz;
use agss::timetravel::{self as tt, Block, DayLabel, DevState, Explain, Place, PointKey, PointKind};
use agss::vm::GameStatus;
use agss::{Ctx, Res};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

struct FakeProc(Arc<AtomicBool>);
impl ProcessChecker for FakeProc {
    fn is_running(&self, _exe: &str) -> Res<bool> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

struct Machine {
    ctx: Ctx,
    game: Game,
    running: Arc<AtomicBool>,
}

fn machine(root: &Path, name: &str) -> Machine {
    let running = Arc::new(AtomicBool::new(false));
    let mut ctx = Ctx::new(root.join(format!("data-{}", name)));
    ctx.machine = name.to_string();
    ctx.procs = Box::new(FakeProc(running.clone()));
    std::fs::create_dir_all(&ctx.data_dir).unwrap();
    let game = Game { name: "Celeste".into(), path: root.join(format!("saves-{}", name)), exe: Some("celeste.exe".into()), dest_key: None, ..Default::default() };
    Machine { ctx, game, running }
}

fn write(dir: &Path, rel: &str, content: &[u8]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

/// Etat complet d'un dossier : fichiers (chemin -> octets) et dossiers.
#[derive(Debug, PartialEq, Eq)]
struct Tree {
    files: BTreeMap<String, Vec<u8>>,
    dirs: Vec<String>,
}

fn tree(root: &Path) -> Tree {
    let mut files = BTreeMap::new();
    let mut dirs = Vec::new();
    if root.exists() {
        for e in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if e.file_type().is_dir() {
                dirs.push(rel);
            } else {
                files.insert(rel, std::fs::read(e.path()).unwrap());
            }
        }
    }
    dirs.sort();
    Tree { files, dirs }
}

/// Octets pseudo-aleatoires (contenu binaire, pas seulement du texte).
fn bytes(seed: u32, n: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).wrapping_add(12345);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x & 0xff) as u8
        })
        .collect()
}

fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(15));
}

fn state_of(m: &Machine) -> (Option<String>, Option<String>, Option<u64>) {
    let s = State::load(&m.ctx).unwrap();
    (s.last_synced.get(&m.game.name).cloned(), s.last_id.get(&m.game.name).cloned(), s.last_sync_ms.get(&m.game.name).copied())
}

fn version_dirs(dest: &Path, g: &Game) -> Vec<PathBuf> {
    backup::list_versions(dest, g)
}

/// Nom de la plus ancienne version du coffre.
fn oldest(dest: &Path, g: &Game) -> String {
    version_dirs(dest, g)[0].file_name().unwrap().to_string_lossy().to_string()
}

fn head_hash(dest: &Path, g: &Game) -> String {
    backup::active_meta(dest, g).unwrap().global_hash
}

#[test]
fn restaurer_puis_annuler_etat_identique_octet_par_octet_et_coffre_d_avant() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let m = machine(t.path(), "EPIKOU");
    // V1 : save.dat + sous-dossier ; V2 : save.dat change, sous-dossier retire, nouveau fichier.
    write(&m.game.path, "save.dat", &bytes(1, 5000));
    write(&m.game.path, "sub/deep/old.bin", &bytes(2, 3000));
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    write(&m.game.path, "save.dat", &bytes(3, 7000));
    std::fs::remove_dir_all(m.game.path.join("sub")).unwrap();
    write(&m.game.path, "new.cfg", b"volume=3\n");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    let before = tree(&m.game.path);
    let st_before = state_of(&m);
    let head_before = head_hash(&dest, &m.game);
    let versions_before = version_dirs(&dest, &m.game);
    assert_eq!(backup::plan(&m.ctx, &m.game, &dest).unwrap(), Plan::InSync);

    // Remettre la version la plus ancienne.
    let v1 = oldest(&dest, &m.game);
    let (out, ticket) = backup::restore_version_undoable(&m.ctx, &m.game, &dest, &v1).unwrap();
    assert!(out.safety_dir.is_some(), "copie de securite AVANT la restauration");
    assert_eq!(std::fs::read(m.game.path.join("save.dat")).unwrap(), bytes(1, 5000));
    assert!(m.game.path.join("sub/deep/old.bin").exists());
    assert!(m.game.path.join("new.cfg").exists(), "une restauration ne supprime jamais rien");
    assert_eq!(ticket.created.len(), 1, "seul old.bin est cree");
    assert!(ticket.created_dirs.iter().any(|d| d.ends_with("sub")) && ticket.created_dirs.iter().any(|d| d.ends_with("deep")));
    assert!(ticket.prev_active.is_some(), "une nouvelle version active est creee : l'annulation devra remettre l'ancienne");
    assert_ne!(tree(&m.game.path), before);

    // Annuler : etat identique octet par octet (fichiers ET dossiers).
    let u = backup::undo_restore(&m.ctx, &m.game, Some(&dest), &ticket).unwrap();
    assert_eq!(tree(&m.game.path), before, "etat local identique octet par octet apres Annuler");
    assert_eq!(u.files_removed, 1);
    assert_eq!(u.kept_changed, 0);
    assert!(u.safety_dir.is_some(), "l'annulation est elle-meme protegee");
    assert!(u.vault_restored && u.vault_error.is_none());
    // Le coffre retrouve son contenu actif d'avant, sans rien supprimer.
    assert_eq!(head_hash(&dest, &m.game), head_before);
    let after = version_dirs(&dest, &m.game);
    for v in &versions_before {
        assert!(after.contains(v), "aucune version supprimee : {:?}", v);
    }
    assert_eq!(after.len(), versions_before.len() + 2, "+ la version remise et l'annulation");
    // A jour, et la synchro automatique ne re-restaure pas la vieille version.
    assert_eq!(backup::plan(&m.ctx, &m.game, &dest).unwrap(), Plan::InSync);
    assert_eq!(state_of(&m).0, st_before.0, "meme contenu connu qu'avant");
    // Une seconde annulation ne casse rien (la copie de securite existe toujours) et redonne le meme etat.
    let again = backup::undo_restore(&m.ctx, &m.game, Some(&dest), &ticket).unwrap();
    assert_eq!(tree(&m.game.path), before);
    assert_eq!(again.files_removed, 0);
}

#[test]
fn annuler_avec_des_modifications_non_copiees_rend_l_etat_de_synchro_d_avant() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let m = machine(t.path(), "EPIKOU");
    write(&m.game.path, "save.dat", &bytes(10, 900));
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    write(&m.game.path, "save.dat", &bytes(11, 1200));
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    // Modification locale pas encore copiee.
    write(&m.game.path, "save.dat", &bytes(12, 1500));
    write(&m.game.path, "slot2.dat", &bytes(13, 10));
    assert_eq!(backup::plan(&m.ctx, &m.game, &dest).unwrap(), Plan::Backup);
    let before = tree(&m.game.path);
    let st_before = state_of(&m);
    let head_before = head_hash(&dest, &m.game);
    let v1 = oldest(&dest, &m.game);
    let (_, ticket) = backup::restore_version_undoable(&m.ctx, &m.game, &dest, &v1).unwrap();
    assert_eq!(std::fs::read(m.game.path.join("save.dat")).unwrap(), bytes(10, 900));
    backup::undo_restore(&m.ctx, &m.game, Some(&dest), &ticket).unwrap();
    assert_eq!(tree(&m.game.path), before, "modifications non copiees retrouvees octet par octet");
    assert_eq!(state_of(&m), st_before, "etat de synchro de ce PC exactement comme avant");
    assert_eq!(head_hash(&dest, &m.game), head_before, "coffre : meme version active qu'avant");
    assert_eq!(backup::plan(&m.ctx, &m.game, &dest).unwrap(), Plan::Backup, "la modification sera copiee comme prevu");
}

#[test]
fn annuler_quand_un_autre_appareil_avait_plus_recent() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let a = machine(t.path(), "EPIKOU");
    let b = machine(t.path(), "EPIGOS");
    write(&a.game.path, "save.dat", b"partie 1");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    pause();
    write(&a.game.path, "save.dat", b"partie 2");
    backup::backup(&a.ctx, &a.game, &dest).unwrap();
    pause();
    // EPIGOS recupere puis joue plus loin.
    backup::restore(&b.ctx, &b.game, &dest).unwrap();
    write(&b.game.path, "save.dat", b"partie 3 sur EPIGOS");
    backup::backup(&b.ctx, &b.game, &dest).unwrap();
    pause();
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Restore, "EPIKOU est en retard");
    let before = tree(&a.game.path);
    let st_before = state_of(&a);
    let head_before = head_hash(&dest, &a.game);
    let v1 = oldest(&dest, &a.game);
    let (_, ticket) = backup::restore_version_undoable(&a.ctx, &a.game, &dest, &v1).unwrap();
    backup::undo_restore(&a.ctx, &a.game, Some(&dest), &ticket).unwrap();
    assert_eq!(tree(&a.game.path), before);
    assert_eq!(state_of(&a), st_before);
    assert_eq!(head_hash(&dest, &a.game), head_before, "la version plus recente d'EPIGOS reste la version active");
    assert_eq!(backup::plan(&a.ctx, &a.game, &dest).unwrap(), Plan::Restore, "comme avant : la version d'EPIGOS arrivera");
    // EPIGOS n'est pas renvoye en arriere.
    assert_eq!(backup::plan(&b.ctx, &b.game, &dest).unwrap(), Plan::InSync);
}

#[test]
fn jamais_pendant_que_le_programme_tourne() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let m = machine(t.path(), "EPIKOU");
    write(&m.game.path, "save.dat", b"v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    write(&m.game.path, "save.dat", b"v2");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    let before = tree(&m.game.path);
    let n = version_dirs(&dest, &m.game).len();
    let v1 = oldest(&dest, &m.game);
    m.running.store(true, Ordering::SeqCst);
    let e = backup::restore_version_undoable(&m.ctx, &m.game, &dest, &v1).unwrap_err().to_string();
    assert!(e.contains("REFUS"), "{}", e);
    assert_eq!(tree(&m.game.path), before, "rien n'est touche");
    assert_eq!(version_dirs(&dest, &m.game).len(), n, "rien n'est ecrit dans le coffre");
    assert!(backup::safety_copies(&m.ctx, &m.game).is_empty());
    // Restauration faite programme ferme, puis programme rouvert : l'annulation attend.
    m.running.store(false, Ordering::SeqCst);
    let (_, ticket) = backup::restore_version_undoable(&m.ctx, &m.game, &dest, &v1).unwrap();
    let restored = tree(&m.game.path);
    m.running.store(true, Ordering::SeqCst);
    assert!(backup::undo_restore(&m.ctx, &m.game, Some(&dest), &ticket).is_err());
    assert_eq!(tree(&m.game.path), restored, "annulation refusee : rien n'est touche");
    m.running.store(false, Ordering::SeqCst);
    backup::undo_restore(&m.ctx, &m.game, Some(&dest), &ticket).unwrap();
    assert_eq!(tree(&m.game.path), before);
    // Garde de l'interface.
    let p = tt::Point { key: PointKey::Version(v1), ms: 0, day: 0, hour: 0, minute: 0, device: "EPIKOU".into(), kind: PointKind::Version, current: false, active: false, files: 1, bytes: 2, legacy: false, note: None };
    assert_eq!(tt::restore_block(&p, &GameStatus::Running, false, true), Some(Block::ProgramOpen));
    assert_eq!(tt::restore_block(&p, &GameStatus::UpToDate, true, true), Some(Block::Busy));
    assert_eq!(tt::restore_block(&p, &GameStatus::UpToDate, false, false), Some(Block::NoVault));
    assert_eq!(tt::restore_block(&p, &GameStatus::UpToDate, false, true), None);
    let cur = tt::Point { current: true, ..p.clone() };
    assert_eq!(tt::restore_block(&cur, &GameStatus::UpToDate, false, true), Some(Block::SameAsNow));
    let safety = tt::Point { kind: PointKind::Safety, ..p };
    assert_eq!(tt::restore_block(&safety, &GameStatus::UpToDate, false, false), None, "une copie de securite est sur cet appareil");
}

#[test]
fn fichier_ajoute_puis_modifie_garde_et_copie_abimee_refusee() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let m = machine(t.path(), "EPIKOU");
    write(&m.game.path, "save.dat", b"v1");
    write(&m.game.path, "extra.dat", b"extra v1");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    std::fs::remove_file(m.game.path.join("extra.dat")).unwrap();
    write(&m.game.path, "save.dat", b"v2");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    let v1 = oldest(&dest, &m.game);
    let (_, ticket) = backup::restore_version_undoable(&m.ctx, &m.game, &dest, &v1).unwrap();
    // Le fichier ajoute est modifie apres la restauration : il n'est jamais retire.
    write(&m.game.path, "extra.dat", b"modifie apres");
    let u = backup::undo_restore(&m.ctx, &m.game, Some(&dest), &ticket).unwrap();
    assert_eq!(u.kept_changed, 1);
    assert_eq!(std::fs::read(m.game.path.join("extra.dat")).unwrap(), b"modifie apres");
    assert_eq!(std::fs::read(m.game.path.join("save.dat")).unwrap(), b"v2");
    // ... et il est aussi dans la copie de securite prise avant l'annulation.
    let snap = u.safety_dir.unwrap();
    assert_eq!(std::fs::read(snap.join("files/extra.dat")).unwrap(), b"modifie apres");

    // Copie de securite abimee : l'annulation refuse et ne touche a rien.
    let (_, t2) = backup::restore_version_undoable(&m.ctx, &m.game, &dest, &v1).unwrap();
    let restored = tree(&m.game.path);
    let sd = t2.safety_dir.clone().unwrap();
    std::fs::write(sd.join("files/save.dat"), b"abime").unwrap();
    let e = backup::undo_restore(&m.ctx, &m.game, Some(&dest), &t2).unwrap_err().to_string();
    assert!(e.contains("rien n'a ete modifie"), "{}", e);
    assert_eq!(tree(&m.game.path), restored);
    // Coffre injoignable alors que la restauration l'avait change : refus, rien n'est touche.
    let e = backup::undo_restore(&m.ctx, &m.game, None, &t2).unwrap_err().to_string();
    assert!(e.contains("rien n'a ete modifie"), "{}", e);
}

#[test]
fn remettre_une_copie_de_securite_puis_annuler() {
    let t = TempDir::new().unwrap();
    let dest = t.path().join("coffre");
    let m = machine(t.path(), "EPIKOU");
    write(&m.game.path, "save.dat", &bytes(20, 4000));
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    write(&m.game.path, "save.dat", &bytes(21, 4100));
    write(&m.game.path, "keep/me.txt", b"garde-moi");
    backup::backup(&m.ctx, &m.game, &dest).unwrap();
    pause();
    let at_v2 = tree(&m.game.path);
    // Remettre V1 (copie de securite de V2 prise avant), la garder.
    let v1 = oldest(&dest, &m.game);
    backup::restore_version_undoable(&m.ctx, &m.game, &dest, &v1).unwrap();
    let copies = backup::safety_copies(&m.ctx, &m.game);
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].files, 2);
    assert!(copies[0].bytes >= 4100);
    let now = tree(&m.game.path);
    // Remettre la copie de securite depuis la frise : etat de V2 retrouve.
    let (out, ticket) = backup::restore_safety_undoable(&m.ctx, &m.game, &copies[0].dir).unwrap();
    assert_eq!(out.files_restored, 2);
    assert_eq!(tree(&m.game.path), at_v2, "copie de securite remise octet par octet");
    assert!(ticket.prev_active.is_none(), "le coffre n'est pas touche");
    // Annuler : retour a l'etat juste avant.
    backup::undo_restore(&m.ctx, &m.game, Some(&dest), &ticket).unwrap();
    assert_eq!(tree(&m.game.path), now);
    // Aucun chemin arbitraire.
    assert!(backup::restore_safety_undoable(&m.ctx, &m.game, t.path()).is_err());
    // Apercu d'une copie de securite.
    let p = agss::preview::for_safety(&m.ctx, &m.game, &copies[0].dir).unwrap();
    assert_eq!(p.replaced, 1);
    assert_eq!(p.identical, 1);
}

// ================================================================== logique de la frise

const H: u64 = 3_600_000;
const D: u64 = 24 * H;

fn row(name: &str, machine: &str, ms: u64) -> HistoryRow {
    HistoryRow { name: name.into(), kind: VKind::New, machine: machine.into(), saved_at_ms: ms, files: 3, id: format!("id-{}", name), active: false, head: false, is_local: false, note: None, bytes: 12 << 20, hash: format!("h-{}", name) }
}

#[test]
fn frise_regroupee_par_jour_couleurs_et_branches() {
    // « maintenant » : 21 oct. 2026 14:00 UTC (mercredi) ; fuseau +2 h.
    let today_ms = agss::util::ms_from_civil(2026, 10, 21, 14, 0, 0, 0) as u64;
    let tz = FixedTz(7200);
    let mut rows = vec![
        row("a", "EPIGOS", today_ms - 9 * D),
        row("b", "EPIKOU", today_ms - 3 * D),
        row("c", "EPIKOU", today_ms - D - 2 * H),
        row("d", "EPIGOS", today_ms - D),
        row("e", "EPIKOU", today_ms - 3 * H),
        row("f", "epikou", today_ms - H),
    ];
    rows[3].note = Some("branche locale en conflit (les deux versions sont conservees)".into());
    rows[5].active = true;
    rows[5].is_local = true;
    rows[4].active = false;
    rows.reverse(); // l'historique est du plus recent au plus ancien
    let mut h = History { rows, conflict_heads: vec!["e".into(), "f".into()], ..Default::default() };
    let safety = vec![SafetyCopy { dir: PathBuf::from("safety/Celeste/x"), ms: today_ms - 2 * H, files: 2, bytes: 100, hash: "s".into(), note: None }];
    let pts = tt::points(&h, &safety, "EPIKOU", &tz);
    assert_eq!(pts.len(), 7);
    assert!(pts.windows(2).all(|w| w[0].ms <= w[1].ms), "du plus ancien au plus recent");
    // jours en heure locale
    let today = tt::local_day(today_ms, 7200);
    let days = tt::days(&pts);
    assert_eq!(days.iter().map(|d| today - d.0).collect::<Vec<_>>(), vec![9, 3, 1, 0]);
    assert_eq!(days.iter().map(|d| d.1).collect::<Vec<_>>(), vec![1, 1, 2, 3]);
    assert_eq!((pts[0].hour, pts[0].minute), (16, 0), "14:00 UTC = 16:00 a +2 h");
    // libelles
    assert_eq!(tt::day_label(today, today), DayLabel::Today);
    assert_eq!(tt::day_label(today - 1, today), DayLabel::Yesterday);
    assert_eq!(tt::day_label(today - 3, today), DayLabel::Weekday { weekday: 6, dom: 18 }, "dimanche 18");
    assert_eq!(tt::day_label(today - 9, today), DayLabel::Date { dom: 12, month: 10, year: None });
    assert_eq!(tt::day_label(today - 300, today), DayLabel::Date { dom: 25, month: 12, year: Some(2025) });
    assert_eq!(tt::day_label(today + 1, today), DayLabel::Date { dom: 22, month: 10, year: None }, "horloge en avance : une date");
    // natures : branches de conflit, version gardee lors d'un conflit, copie de securite
    let kind = |n: &str| pts.iter().find(|p| p.key == PointKey::Version(n.into())).unwrap().kind;
    assert_eq!(kind("e"), PointKind::ConflictOpen);
    assert_eq!(kind("f"), PointKind::ConflictOpen);
    assert_eq!(kind("d"), PointKind::BranchKept);
    assert_eq!(kind("a"), PointKind::Version);
    let s = pts.iter().find(|p| p.kind == PointKind::Safety).unwrap();
    assert_eq!(s.device, "EPIKOU");
    assert_eq!(tt::explain(s), Explain::Safety);
    // couleurs : une par appareil (casse ignoree)
    let cols = tt::colors(&pts, &|d| if d.eq_ignore_ascii_case("EPIKOU") { [0x1f, 0xb5, 0xc9] } else { [0x9b, 0x6a, 0xd6] });
    for (p, c) in pts.iter().zip(&cols) {
        assert_eq!(*c, if p.device.eq_ignore_ascii_case("epikou") { [0x1f, 0xb5, 0xc9] } else { [0x9b, 0x6a, 0xd6] });
    }
    // selection par defaut : ce que tu as en ce moment
    let sel = tt::default_selected(&pts);
    assert_eq!(pts[sel].key, PointKey::Version("f".into()));
    assert_eq!(tt::explain(&pts[sel]), Explain::ConflictOpen, "un conflit en cours s'explique d'abord");
    h.conflict_heads.clear();
    let pts = tt::points(&h, &[], "EPIKOU", &tz);
    let sel = tt::default_selected(&pts);
    assert_eq!(tt::explain(&pts[sel]), Explain::Current);
    assert_eq!(tt::explain(&pts[0]), Explain::Older);
    assert_eq!(tt::index_of(&pts, &PointKey::Version("c".into())), Some(2));
    // aucune version : rien a choisir, pas de panique
    assert_eq!(tt::default_selected(&[]), 0);
    assert!(tt::days(&[]).is_empty());
}

#[test]
fn version_la_plus_recente_pas_encore_ici() {
    let mut h = History { rows: vec![row("x", "EPIGOS", 2 * D), row("y", "EPIKOU", D)], ..Default::default() };
    h.rows[0].active = true;
    let pts = tt::points(&h, &[], "EPIKOU", &FixedTz(0));
    let sel = tt::default_selected(&pts);
    assert_eq!(pts[sel].key, PointKey::Version("x".into()), "rien d'identique ici : la version active");
    assert_eq!(tt::explain(&pts[sel]), Explain::NewestNotHere);
}

#[test]
fn frise_dessinee_puis_selection_deplacee_sans_blocage() {
    // Le curseur a ressort previent l'horloge des animations : jamais pendant que les donnees d'egui sont verrouillees
    // (sinon l'interface se figeait des qu'on choisissait un autre point).
    use agss::gui::ui::{self, anim, anim::AnimClock, Tokens};
    use eframe::egui;
    let ctx = egui::Context::default();
    let mut clock = AnimClock::default();
    let marks: Vec<ui::Mark> = (0..8).map(|i| ui::Mark { conflict: i == 5, current: i == 7, safety: i == 2, label: format!("{:02}:00", 10 + i), ..ui::Mark::new(i / 3, [0x1f, 0xb5, 0xc9]) }).collect();
    let mut sel = 7usize;
    let mut moving = false;
    for (k, t) in [1.0f64, 1.016, 1.2, 1.4, 3.0].iter().enumerate() {
        if k == 1 {
            sel = 2; // autre point choisi : le curseur part vers lui
        }
        clock.begin_frame();
        let raw = egui::RawInput { time: Some(*t), screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 500.0))), ..Default::default() };
        let _ = ctx.run(raw, |c| {
            ui::install(c, Tokens::default_dark(), false, anim::Motion::default());
            egui::CentralPanel::default().show(c, |ui| {
                ui::timeline(ui, "frise", &marks, &|d| format!("jour {}", d), &mut sel);
            });
        });
        clock.merge(anim::take_needs(&ctx));
        if k == 1 || k == 2 {
            moving |= clock.decide(*t) == anim::Repaint::Now;
        }
    }
    assert!(moving, "le curseur anime demande des images pendant son deplacement");
    assert_eq!(sel, 2);
}

fn facts(status: GameStatus, head: Option<(&str, &str, &str)>, writers: &[(&str, &str, &str, u64)], marks: &[(&str, &str)]) -> Facts {
    let g = Game { name: "Celeste".into(), ..Default::default() };
    let mut f = Facts::bare(&g, Kind::Game, status);
    f.head = head.map(|(m, id, h)| Head { machine: m.into(), id: id.into(), hash: h.into(), saved_ms: 10 });
    f.writers = writers.iter().map(|w| (w.0.to_string(), w.1.to_string(), w.2.to_string(), w.3)).collect();
    f.marks = marks.iter().map(|m| (m.0.to_string(), m.1.to_string())).collect();
    f.last_sync_ms = Some(50);
    f
}

#[test]
fn synchro_par_appareil_dans_l_en_tete() {
    let w = [("EPIKOU", "v1", "h1", 5), ("EPIGOS", "v2", "h2", 9)];
    // a jour partout
    let f = facts(GameStatus::UpToDate, Some(("EPIGOS", "v2", "h2")), &w, &[("EPIKOU", "v2")]);
    let r = tt::device_rows(&f, "EPIKOU", true, &[]);
    assert_eq!(r.iter().map(|d| (d.name.as_str(), d.here, d.state)).collect::<Vec<_>>(), vec![("EPIKOU", true, DevState::UpToDate), ("EPIGOS", false, DevState::UpToDate)]);
    assert_eq!(r[0].last_ms, Some(50));
    assert_eq!(r[1].last_ms, Some(9));
    // en retard ici : EPIGOS a plus recent
    let f = facts(GameStatus::CloudNewer, Some(("EPIGOS", "v2", "h2")), &w, &[]);
    let r = tt::device_rows(&f, "EPIKOU", true, &[]);
    assert_eq!((r[0].state, r[1].state), (DevState::Behind, DevState::Newer));
    // plus recent ici : EPIGOS reste a jour avec le coffre
    let f = facts(GameStatus::LocalChanged, Some(("EPIGOS", "v2", "h2")), &w, &[]);
    let r = tt::device_rows(&f, "EPIKOU", true, &[]);
    assert_eq!((r[0].state, r[1].state), (DevState::Newer, DevState::UpToDate));
    // EPIGOS n'a pas la derniere version d'EPIKOU
    let f = facts(GameStatus::UpToDate, Some(("EPIKOU", "v3", "h3")), &[("EPIKOU", "v3", "h3", 12), ("EPIGOS", "v2", "h2", 9)], &[("EPIGOS", "v2")]);
    let r = tt::device_rows(&f, "EPIKOU", true, &[]);
    assert_eq!((r[0].state, r[1].state), (DevState::UpToDate, DevState::Behind));
    // conflit
    let f = facts(GameStatus::Conflict, Some(("EPIGOS", "v2", "h2")), &w, &[]);
    let r = tt::device_rows(&f, "EPIKOU", true, &["EPIGOS".into()]);
    assert_eq!((r[0].state, r[1].state), (DevState::Conflict, DevState::Conflict));
    // coffre injoignable : rien d'affirme
    let r = tt::device_rows(&f, "EPIKOU", false, &[]);
    assert!(r.iter().all(|d| d.state == DevState::Unknown));
    // jamais copie, seul appareil
    let f = facts(GameStatus::Never, None, &[], &[]);
    let r = tt::device_rows(&f, "EPIKOU", true, &[]);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].state, DevState::NotCopied);
}

#[test]
fn lieux_lisibles_de_ce_qui_est_protege() {
    assert_eq!(tt::place_of_template(Some("%LOCALAPPDATA%/Celeste/Saves")), Place::AppData);
    assert_eq!(tt::place_of_template(Some("%APPDATA%/x")), Place::AppData);
    assert_eq!(tt::place_of_template(Some("%LOCALLOW%/Team Cherry/Hollow Knight")), Place::AppData);
    assert_eq!(tt::place_of_template(Some("%DOCUMENTS%/My Games/x")), Place::Documents);
    assert_eq!(tt::place_of_template(Some("%SAVEDGAMES%/x")), Place::SavedGames);
    assert_eq!(tt::place_of_template(None), Place::Elsewhere);
    assert_eq!(tt::place_of_template(Some("%WEIRD%/x")), Place::Elsewhere);
    let kf = agss::hints::KnownFolders { documents: Some(PathBuf::from("/home/u/Documents")), ..Default::default() };
    assert_eq!(tt::place_of(Path::new("/home/u/Documents/Saves"), &kf), Place::Documents);
    assert_eq!(tt::place_of(Path::new("/srv/jeux"), &kf), Place::Elsewhere);
    assert_eq!(tt::short_name(Path::new("/home/u/Documents/Saves")), "Saves");
}
