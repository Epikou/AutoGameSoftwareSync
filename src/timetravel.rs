//! Fiche d'un element et « Remonter le temps » (2.0, lot 7) : points de la frise (versions du coffre et copies de
//! securite), regroupement par jour en heure locale, libelles des jours, explication en mots simples, synchro par
//! appareil, lieux lisibles de « Ce qui est protege ». Logique PURE (testee), sans egui.
use crate::backup::{History, SafetyCopy};
use crate::cloud::VKind;
use crate::home::{self, Facts};
use crate::timefmt::TzProvider;
use crate::vm::GameStatus;
use std::path::{Path, PathBuf};

const DAY_MS: i64 = 86_400_000;

// ================================================================== points de la frise

/// Ce que designe un point.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PointKey {
    /// Version du coffre (nom exact, voir `backup::restore_version`).
    Version(String),
    /// Copie de securite locale (dossier).
    Safety(PathBuf),
}

/// Nature d'un point (forme sur la frise, explication).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    /// Version ordinaire du coffre.
    Version,
    /// Branche d'un conflit en cours : deux appareils ont modifie en meme temps, les deux versions sont gardees.
    ConflictOpen,
    /// Version gardee lors d'un conflit deja regle.
    BranchKept,
    /// Copie de securite prise sur cet appareil juste avant de remettre une autre version.
    Safety,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub key: PointKey,
    /// Date (ms UTC).
    pub ms: u64,
    /// Jour en heure locale (jours depuis le 1er janvier 1970).
    pub day: i64,
    pub hour: u32,
    pub minute: u32,
    /// Appareil qui a fait la copie.
    pub device: String,
    pub kind: PointKind,
    /// Meme contenu que ce que cet appareil a en ce moment (point cercle).
    pub current: bool,
    /// Version la plus recente du coffre.
    pub active: bool,
    pub files: usize,
    pub bytes: u64,
    /// Ancien format de sauvegarde.
    pub legacy: bool,
    /// Note technique (mode expert).
    pub note: Option<String>,
}

/// Jour local d'un instant UTC (`off_secs` : decalage de l'heure locale).
pub fn local_day(utc_ms: u64, off_secs: i32) -> i64 {
    (utc_ms as i64 + off_secs as i64 * 1000).div_euclid(DAY_MS)
}

/// Heure et minute locales.
pub fn local_hm(utc_ms: u64, off_secs: i32) -> (u32, u32) {
    let (_, _, _, h, mi, _, _) = crate::util::civil_from_ms(utc_ms as i64 + off_secs as i64 * 1000);
    (h, mi)
}

/// Note ecrite par le moteur pour une version gardee lors d'un conflit (voir `backup`).
fn is_branch_note(note: Option<&str>) -> bool {
    note.map(|n| n.starts_with("branche locale en conflit") || n.starts_with("version locale conservee avant resolution")).unwrap_or(false)
}

/// Points de la frise, du plus ancien au plus recent : versions du coffre + copies de securite de cet appareil.
pub fn points(h: &History, safety: &[SafetyCopy], this_machine: &str, tz: &dyn TzProvider) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for r in &h.rows {
        let off = tz.utc_offset_secs(r.saved_at_ms);
        let (hour, minute) = local_hm(r.saved_at_ms, off);
        let kind = if h.conflict_heads.contains(&r.name) {
            PointKind::ConflictOpen
        } else if r.kind == VKind::LegacyConflictCopy || is_branch_note(r.note.as_deref()) {
            PointKind::BranchKept
        } else {
            PointKind::Version
        };
        out.push(Point {
            key: PointKey::Version(r.name.clone()),
            ms: r.saved_at_ms,
            day: local_day(r.saved_at_ms, off),
            hour,
            minute,
            device: r.machine.clone(),
            kind,
            current: r.is_local,
            active: r.active,
            files: r.files,
            bytes: r.bytes,
            legacy: matches!(r.kind, VKind::LegacyCurrent | VKind::LegacyVersion | VKind::LegacyConflictCopy),
            note: r.note.clone(),
        });
    }
    for s in safety {
        let off = tz.utc_offset_secs(s.ms);
        let (hour, minute) = local_hm(s.ms, off);
        out.push(Point {
            key: PointKey::Safety(s.dir.clone()),
            ms: s.ms,
            day: local_day(s.ms, off),
            hour,
            minute,
            device: this_machine.to_string(),
            kind: PointKind::Safety,
            current: false,
            active: false,
            files: s.files,
            bytes: s.bytes,
            legacy: false,
            note: s.note.clone(),
        });
    }
    out.sort_by(|a, b| a.ms.cmp(&b.ms).then_with(|| (a.kind == PointKind::Safety).cmp(&(b.kind == PointKind::Safety))));
    out
}

/// Jours de la frise (jour, nombre de points), dans l'ordre des points.
pub fn days(points: &[Point]) -> Vec<(i64, usize)> {
    let mut out: Vec<(i64, usize)> = Vec::new();
    for p in points {
        match out.last_mut() {
            Some((d, n)) if *d == p.day => *n += 1,
            _ => out.push((p.day, 1)),
        }
    }
    out
}

/// Point choisi a l'ouverture : ce que tu as en ce moment, sinon la version active, sinon le plus recent.
pub fn default_selected(points: &[Point]) -> usize {
    points.iter().rposition(|p| p.current).or_else(|| points.iter().rposition(|p| p.active)).unwrap_or(points.len().saturating_sub(1))
}

/// Position d'un point d'apres sa cle (selection gardee quand la liste est relue).
pub fn index_of(points: &[Point], key: &PointKey) -> Option<usize> {
    points.iter().position(|p| &p.key == key)
}

/// Couleurs des points : couleur de l'appareil (meme appareil = meme couleur, casse ignoree).
pub fn colors(points: &[Point], color_of: &dyn Fn(&str) -> crate::theme::Rgb) -> Vec<crate::theme::Rgb> {
    let mut seen: Vec<(String, crate::theme::Rgb)> = Vec::new();
    points
        .iter()
        .map(|p| {
            let k = p.device.to_lowercase();
            if let Some((_, c)) = seen.iter().find(|(d, _)| *d == k) {
                return *c;
            }
            let c = color_of(&p.device);
            seen.push((k, c));
            c
        })
        .collect()
}

// ================================================================== libelles des jours

/// Libelle d'un jour de la frise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayLabel {
    Today,
    Yesterday,
    /// Cette semaine : jour de la semaine (0 = lundi) et jour du mois (« mardi 21 »).
    Weekday { weekday: u32, dom: u32 },
    /// Plus ancien (ou dans le futur : horloge d'un autre appareil) : date, annee si ce n'est pas l'annee en cours.
    Date { dom: u32, month: u32, year: Option<i64> },
}

/// Jour de la semaine (0 = lundi) d'un jour depuis 1970 (le 1er janvier 1970 etait un jeudi).
pub fn weekday(day: i64) -> u32 {
    (day + 3).rem_euclid(7) as u32
}

fn ymd(day: i64) -> (i64, u32, u32) {
    let (y, m, d, _, _, _, _) = crate::util::civil_from_ms(day * DAY_MS);
    (y, m, d)
}

pub fn day_label(day: i64, today: i64) -> DayLabel {
    let (y, m, d) = ymd(day);
    match today - day {
        0 => DayLabel::Today,
        1 => DayLabel::Yesterday,
        2..=6 => DayLabel::Weekday { weekday: weekday(day), dom: d },
        _ => DayLabel::Date { dom: d, month: m, year: if y != ymd(today).0 { Some(y) } else { None } },
    }
}

// ================================================================== explication en mots simples

/// Ce que l'interface explique pour le point choisi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Explain {
    /// C'est ce que tu as en ce moment.
    Current,
    /// La plus recente du coffre, pas encore sur cet appareil.
    NewestNotHere,
    /// Branche d'un conflit en cours.
    ConflictOpen,
    /// Version gardee lors d'un conflit.
    BranchKept,
    /// Copie de securite de cet appareil.
    Safety,
    /// Version plus ancienne.
    Older,
}

pub fn explain(p: &Point) -> Explain {
    match p.kind {
        PointKind::Safety => Explain::Safety,
        PointKind::ConflictOpen => Explain::ConflictOpen,
        _ if p.current => Explain::Current,
        PointKind::BranchKept => Explain::BranchKept,
        _ if p.active => Explain::NewestNotHere,
        _ => Explain::Older,
    }
}

/// Pourquoi « Remettre cette version » est impossible maintenant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    /// Le programme est ouvert : on ne remet JAMAIS une version pendant qu'il tourne.
    ProgramOpen,
    /// Une copie ou une restauration est deja en cours.
    Busy,
    /// Coffre injoignable (une copie de securite, elle, est sur cet appareil).
    NoVault,
    /// C'est deja ce que tu as en ce moment.
    SameAsNow,
}

/// Garde de « Remettre cette version » (None = possible). Le moteur refuse aussi de restaurer un programme ouvert.
pub fn restore_block(p: &Point, status: &GameStatus, busy: bool, vault: bool) -> Option<Block> {
    if matches!(status, GameStatus::Running) {
        return Some(Block::ProgramOpen);
    }
    if busy {
        return Some(Block::Busy);
    }
    if p.kind != PointKind::Safety && !vault {
        return Some(Block::NoVault);
    }
    if p.current {
        return Some(Block::SameAsNow);
    }
    None
}

// ================================================================== synchro par appareil (en-tete de la fiche)

/// Etat d'un appareil pour cet element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevState {
    /// A la version la plus recente.
    UpToDate,
    /// N'a pas encore la version la plus recente.
    Behind,
    /// A plus recent que les autres (cet appareil : modifications pas encore copiees ; un autre : sa version n'est pas
    /// encore ici).
    Newer,
    /// Rien n'est encore copie.
    NotCopied,
    /// A modifie en meme temps qu'un autre appareil.
    Conflict,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRow {
    pub name: String,
    /// Cet appareil.
    pub here: bool,
    pub state: DevState,
    /// Derniere copie connue faite par cet appareil (ms UTC).
    pub last_ms: Option<u64>,
}

/// Appareils de l'element (cet appareil d'abord, puis les autres par ordre alphabetique) et leur etat.
/// `conflict_devices` : appareils dont une branche est en conflit (historique de la fiche).
pub fn device_rows(f: &Facts, me: &str, reachable: bool, conflict_devices: &[String]) -> Vec<DeviceRow> {
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let known = home::devices_known(f, me);
    let at = home::devices_at_head(f, me);
    let last_of = |m: &str| f.writers.iter().filter(|w| same(&w.0, m)).map(|w| w.3).max();
    let offline = !reachable || matches!(f.status, GameStatus::NoDest);
    let me_state = if offline {
        DevState::Unknown
    } else {
        match &f.status {
            GameStatus::UpToDate => DevState::UpToDate,
            GameStatus::LocalChanged => DevState::Newer,
            GameStatus::Never => DevState::NotCopied,
            GameStatus::CloudNewer => DevState::Behind,
            GameStatus::Conflict => DevState::Conflict,
            _ if at.iter().any(|a| same(a, me)) => DevState::UpToDate,
            _ => DevState::Unknown,
        }
    };
    let mut rows = vec![DeviceRow { name: me.to_string(), here: true, state: me_state, last_ms: last_of(me).max(f.last_sync_ms) }];
    let mut others: Vec<&String> = known.iter().filter(|m| !same(m, me)).collect();
    others.sort_by_key(|s| s.to_lowercase());
    for m in others {
        let state = if offline {
            DevState::Unknown
        } else if conflict_devices.iter().any(|c| same(c, m)) && me_state == DevState::Conflict {
            DevState::Conflict
        } else if at.iter().any(|a| same(a, m)) {
            if me_state == DevState::Behind { DevState::Newer } else { DevState::UpToDate }
        } else {
            DevState::Behind
        };
        rows.push(DeviceRow { name: m.clone(), here: false, state, last_ms: last_of(m) });
    }
    rows
}

// ================================================================== « Ce qui est protege » : lieux lisibles

/// Ou se trouve un emplacement, en mots simples (le chemin complet reste en infobulle et en mode expert).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Donnees des applis (AppData, LocalLow ; sous Linux : ~/.local/share, ~/.config, prefixe Proton).
    AppData,
    Documents,
    /// Dossier « Parties enregistrees » de Windows.
    SavedGames,
    /// Ailleurs sur cet appareil.
    Elsewhere,
}

/// Lieu d'un gabarit universel (`%APPDATA%/...`, voir `hints::to_template`).
pub fn place_of_template(tpl: Option<&str>) -> Place {
    match tpl.and_then(|t| t.split('/').next()) {
        Some("%LOCALAPPDATA%") | Some("%APPDATA%") | Some("%LOCALLOW%") => Place::AppData,
        Some("%DOCUMENTS%") => Place::Documents,
        Some("%SAVEDGAMES%") => Place::SavedGames,
        _ => Place::Elsewhere,
    }
}

/// Lieu d'un chemin de CET appareil.
pub fn place_of(path: &Path, kf: &crate::hints::KnownFolders) -> Place {
    place_of_template(crate::hints::to_template(path, kf).as_deref())
}

/// Nom court d'un emplacement : dernier dossier (ou nom du fichier).
pub fn short_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jours_et_semaine() {
        // 1970-01-01 : jeudi
        assert_eq!(weekday(0), 3);
        assert_eq!(weekday(4), 0, "5 janvier 1970 : lundi");
        assert_eq!(weekday(-1), 2);
        assert_eq!(local_day(0, 0), 0);
        assert_eq!(local_day(DAY_MS as u64 - 1, 0), 0);
        assert_eq!(local_day(DAY_MS as u64 - 1, 3600), 1, "23:59 UTC = le lendemain a Paris");
        assert_eq!(local_hm(DAY_MS as u64 - 60_000, 7200), (1, 59));
    }
}
