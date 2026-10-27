//! Regles de demarrage : taches planifiees « A l'ouverture de session » (LogonTrigger).
//!
//! Pourquoi : en mode Windows 11 « Xbox Full Screen Experience » (ex. Legion Go S), explorer.exe n'est
//! pas charge comme shell ; or les entrees `HKCU\...\Run` et le dossier Demarrage sont traites PAR
//! l'Explorateur, donc ni AutoGameSoftwareSync ni les outils cloud (Google Drive...) ne demarrent. Les taches
//! planifiees ne dependent pas de l'Explorateur. (HYPOTHESE : non verifiee sur la machine de l'utilisateur.)
//!
//! Tout ce qui est decision est PUR et teste : detection des outils (sondes abstraites), generation du
//! XML de tache, plan d'application (diff actuel / souhaite). Le planificateur reel (`schtasks`, toujours
//! sans fenetre) est derriere le trait `Scheduler` : aucune vraie tache pendant `cargo test`.
//! Seules les taches du dossier `\AutoGameSoftwareSync\` sont creees, modifiees ou supprimees.
use crate::settings::StartupRuleCfg;
use std::path::{Path, PathBuf};

pub const TASK_FOLDER: &str = "AutoGameSoftwareSync";
/// Dossier de taches de la version precedente (migre automatiquement, voir `plan_migrate`).
pub const DELAY_CLOUD_SECS: u32 = 20;
pub const DELAY_APP_SECS: u32 = 40;
pub const ID_APP: &str = "agss";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    App,
    GoogleDrive,
    OneDrive,
    Dropbox,
    Custom,
}

impl ToolKind {
    pub fn from_id(id: &str) -> ToolKind {
        match id {
            ID_APP => ToolKind::App,
            "gdrive" => ToolKind::GoogleDrive,
            "onedrive" => ToolKind::OneDrive,
            "dropbox" => ToolKind::Dropbox,
            _ => ToolKind::Custom,
        }
    }
}

/// Programme detecte sur ce PC.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub id: String,
    pub kind: ToolKind,
    pub name: String,
    pub exe: PathBuf,
    pub args: String,
}

/// Sondes de la machine (registre, disque, environnement) : abstraites pour les tests.
pub trait ToolProbe {
    /// Valeur de `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
    fn run_value(&self, name: &str) -> Option<String>;
    fn file_exists(&self, p: &Path) -> bool;
    /// Noms des sous-dossiers de `p`.
    fn subdirs(&self, p: &Path) -> Vec<String>;
    fn env(&self, key: &str) -> Option<String>;
}

pub struct RealProbe;
impl ToolProbe for RealProbe {
    fn run_value(&self, name: &str) -> Option<String> {
        #[cfg(windows)]
        {
            crate::registry::read_string(crate::registry::Hive::CurrentUser, "Software\\Microsoft\\Windows\\CurrentVersion\\Run", name)
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            None
        }
    }
    fn file_exists(&self, p: &Path) -> bool {
        p.is_file()
    }
    fn subdirs(&self, p: &Path) -> Vec<String> {
        std::fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default()
    }
    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// `"C:\x\a.exe" --arg` ou `C:\x\a.exe --arg` -> (exe, args).
pub fn parse_command_line(s: &str) -> (PathBuf, String) {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (PathBuf::from(&rest[..end]), rest[end + 1..].trim().to_string());
        }
    }
    // Sans guillemets : le chemin va jusqu'au premier « .exe ».
    let low = s.to_lowercase();
    match low.find(".exe") {
        Some(i) => (PathBuf::from(&s[..i + 4]), s[i + 4..].trim().to_string()),
        None => (PathBuf::from(s), String::new()),
    }
}

/// Cle de comparaison de versions `132.0.0.0` -> [132,0,0,0] (les morceaux non numeriques valent 0).
pub fn version_key(name: &str) -> Vec<u32> {
    name.split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect()
}

/// `GoogleDriveFS.exe` de la version la PLUS RECENTE installee (le dossier de version change a chaque mise a jour).
pub fn newest_gdrive_exe(probe: &dyn ToolProbe) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for k in ["ProgramFiles", "ProgramW6432"] {
        if let Some(pf) = probe.env(k) {
            roots.push(Path::new(&pf).join("Google").join("Drive File Stream"));
        }
    }
    roots.push(PathBuf::from("C:\\Program Files\\Google\\Drive File Stream"));
    roots.dedup();
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for root in roots {
        for d in probe.subdirs(&root) {
            let exe = root.join(&d).join("GoogleDriveFS.exe");
            if probe.file_exists(&exe) {
                let key = version_key(&d);
                if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) {
                    best = Some((key, exe));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Outils cloud detectes sur ce PC.
pub fn detect_tools(probe: &dyn ToolProbe) -> Vec<Tool> {
    let mut v = Vec::new();
    // Google Drive : version la plus recente installee ; a defaut le chemin de la valeur Run existante.
    let run = probe.run_value("GoogleDriveFS").map(|s| parse_command_line(&s));
    let gd = newest_gdrive_exe(probe).or_else(|| run.as_ref().map(|r| r.0.clone()).filter(|p| probe.file_exists(p)));
    if let Some(exe) = gd {
        let args = run.map(|r| r.1).filter(|a| !a.is_empty()).unwrap_or_else(|| "--startup_mode".into());
        v.push(Tool { id: "gdrive".into(), kind: ToolKind::GoogleDrive, name: "Google Drive".into(), exe, args });
    }
    // OneDrive.
    let od_run = probe.run_value("OneDrive").map(|s| parse_command_line(&s));
    let mut od_cands: Vec<PathBuf> = od_run.iter().map(|r| r.0.clone()).collect();
    if let Some(l) = probe.env("LOCALAPPDATA") {
        od_cands.push(Path::new(&l).join("Microsoft").join("OneDrive").join("OneDrive.exe"));
    }
    for k in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = probe.env(k) {
            od_cands.push(Path::new(&pf).join("Microsoft OneDrive").join("OneDrive.exe"));
        }
    }
    if let Some(exe) = od_cands.into_iter().find(|p| probe.file_exists(p)) {
        let args = od_run.map(|r| r.1).filter(|a| !a.is_empty()).unwrap_or_else(|| "/background".into());
        v.push(Tool { id: "onedrive".into(), kind: ToolKind::OneDrive, name: "OneDrive".into(), exe, args });
    }
    // Dropbox.
    let db_run = probe.run_value("Dropbox").map(|s| parse_command_line(&s));
    let mut db_cands: Vec<PathBuf> = db_run.iter().map(|r| r.0.clone()).collect();
    for k in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = probe.env(k) {
            db_cands.push(Path::new(&pf).join("Dropbox").join("Client").join("Dropbox.exe"));
        }
    }
    if let Some(l) = probe.env("LOCALAPPDATA") {
        db_cands.push(Path::new(&l).join("Dropbox").join("Client").join("Dropbox.exe"));
    }
    if let Some(exe) = db_cands.into_iter().find(|p| probe.file_exists(p)) {
        let args = db_run.map(|r| r.1).unwrap_or_default();
        v.push(Tool { id: "dropbox".into(), kind: ToolKind::Dropbox, name: "Dropbox".into(), exe, args });
    }
    v
}

/// Regle complete (ce qui sera ecrit dans la tache).
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub id: String,
    pub kind: ToolKind,
    pub name: String,
    /// Programme lance par la tache.
    pub command: PathBuf,
    pub args: String,
    pub delay_secs: u32,
    pub enabled: bool,
    /// Le programme vise existe-t-il ? (« Introuvable » sinon)
    pub found: bool,
    /// Demarrer reduit sans activer (outils cloud et programmes personnalises ; sans objet pour AutoGameSoftwareSync).
    pub minimized: bool,
}

pub fn default_delay(kind: ToolKind) -> u32 {
    if kind == ToolKind::App { DELAY_APP_SECS } else { DELAY_CLOUD_SECS }
}

/// Regles affichees : AutoGameSoftwareSync + outils detectes + outils personnalises, avec les reglages enregistres.
/// Les outils cloud passent par `AutoGameSoftwareSync --launch-tool <id>` (le chemin est re-resolu a chaque demarrage :
/// il survit aux mises a jour de Google Drive).
pub fn build_rules(tools: &[Tool], cfg: &[StartupRuleCfg], self_exe: &Path, file_exists: &dyn Fn(&Path) -> bool) -> Vec<Rule> {
    let find = |id: &str| cfg.iter().find(|c| c.id == id);
    let mut out = Vec::new();
    let app = find(ID_APP);
    out.push(Rule {
        id: ID_APP.into(),
        kind: ToolKind::App,
        name: "AutoGameSoftwareSync".into(),
        command: self_exe.to_path_buf(),
        args: if app.map(|c| c.minimized).unwrap_or(true) { "--tray".into() } else { String::new() },
        delay_secs: app.map(|c| c.delay_secs).unwrap_or(DELAY_APP_SECS),
        enabled: app.map(|c| c.enabled).unwrap_or(false),
        found: file_exists(self_exe),
        minimized: false,
    });
    for t in tools {
        let c = find(&t.id);
        let minimized = c.map(|c| c.minimized).unwrap_or(true);
        out.push(Rule {
            id: t.id.clone(),
            kind: t.kind,
            name: t.name.clone(),
            command: self_exe.to_path_buf(),
            args: launch_tool_args(&t.id, minimized),
            delay_secs: c.map(|c| c.delay_secs).unwrap_or(DELAY_CLOUD_SECS),
            enabled: c.map(|c| c.enabled).unwrap_or(false),
            found: file_exists(&t.exe),
            minimized,
        });
    }
    for c in cfg.iter().filter(|c| ToolKind::from_id(&c.id) == ToolKind::Custom) {
        let exe = c.exe.clone().unwrap_or_default();
        out.push(Rule {
            id: c.id.clone(),
            kind: ToolKind::Custom,
            name: if c.name.is_empty() { exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| c.id.clone()) } else { c.name.clone() },
            found: file_exists(&exe),
            command: if c.minimized { self_exe.to_path_buf() } else { exe.clone() },
            args: if c.minimized { launch_program_args(&exe) } else { String::new() },
            delay_secs: c.delay_secs,
            enabled: c.enabled,
            minimized: c.minimized,
        });
    }
    out
}

/// Argument ajoute quand l'option « Demarrer reduit » est DESACTIVEE (par defaut les outils demarrent reduits).
pub const ARG_NORMAL: &str = "--normal";

/// Arguments de la tache d'un outil cloud : `--launch-tool <id>` (+ `--normal` si « Demarrer reduit » est decoche).
pub fn launch_tool_args(id: &str, minimized: bool) -> String {
    if minimized { format!("--launch-tool {}", id) } else { format!("--launch-tool {} {}", id, ARG_NORMAL) }
}

/// Arguments de la tache d'un programme personnalise lance reduit (AutoGameSoftwareSync le lance avec la demande « reduit »).
pub fn launch_program_args(exe: &Path) -> String {
    format!("--launch-program \"{}\"", exe.display())
}

// ------------------------------------------------------------------ XML de tache

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Nom de tache (dossier + id) ; l'id est filtre : jamais d'autre dossier que `\AutoGameSoftwareSync\`.
pub fn task_name(id: &str) -> String {
    let clean: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    format!("{}\\{}", TASK_FOLDER, if clean.is_empty() { "rule".into() } else { clean })
}

/// Une tache est-elle une des NOTRES ? (dossier `AutoGameSoftwareSync\`, nom sans separateur supplementaire)
pub fn is_own_task(name: &str) -> bool {
    let n = name.trim_start_matches('\\');
    match n.strip_prefix(&format!("{}\\", TASK_FOLDER)) {
        Some(rest) => !rest.is_empty() && !rest.contains('\\') && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        None => false,
    }
}

/// Une tache est-elle une ANCIENNE tache a nous ? (dossier `\AutoGameSoftwareSync\` de la version precedente, identifiant
/// que nous avons cree : agss, gdrive, onedrive, dropbox, custom-N). Toute autre tache est laissee.
pub fn is_legacy_task(name: &str) -> bool {
    crate::legacy::is_task(name)
}

/// Plan de migration des taches de l'ancien dossier : pour chaque ancienne tache dont la regle existe
/// toujours (programme trouve), on CREE la nouvelle tache (nouvel exe / nouveau dossier) puis on SUPPRIME
/// l'ancienne. Sans regle de remplacement, l'ancienne tache est laissee telle quelle.
pub fn plan_migrate(legacy_existing: &[String], rules: &[Rule]) -> ApplyPlan {
    let mut plan = ApplyPlan::default();
    for n in legacy_existing.iter().filter(|n| is_legacy_task(n)) {
        let id = crate::legacy::task_rule_id(n.trim_start_matches('\\').rsplit('\\').next().unwrap_or(""));
        if let Some(r) = rules.iter().find(|r| r.id == id && r.found) {
            let mut r = r.clone();
            r.enabled = true;
            plan.create.push(r);
            plan.delete.push(n.trim_start_matches('\\').to_string());
        }
    }
    plan
}

/// Reecrit la commande d'une entree Run de l'ancienne version (`...\AutoGameSoftwareSync.exe args`) pour pointer vers le
/// nouvel exe, arguments conserves. None si la commande ne vise pas notre ancien exe (pas a nous : on n'y touche pas).
pub fn rewrite_run_command(old_cmd: &str, new_exe: &Path) -> Option<String> {
    let (exe, args) = parse_command_line(old_cmd);
    let name = exe.file_name()?.to_string_lossy().to_lowercase();
    if !crate::legacy::is_exe_name(&name) {
        return None;
    }
    Some(if args.is_empty() { format!("\"{}\"", new_exe.display()) } else { format!("\"{}\" {}", new_exe.display(), args) })
}


/// Definition XML de la tache : LogonTrigger de l'utilisateur courant avec delai, principal
/// InteractiveToken + LeastPrivilege (jamais de droits admin permanents), pas d'arret sur batterie,
/// pas de limite de duree, rattrapage si manque, une seule instance, tache masquee.
pub fn task_xml(rule: &Rule, user: &str) -> String {
    let dir = rule.command.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let wd = if dir.is_empty() { String::new() } else { format!("<WorkingDirectory>{}</WorkingDirectory>", esc(&dir)) };
    let args = if rule.args.is_empty() { String::new() } else { format!("<Arguments>{}</Arguments>", esc(&rule.args)) };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>AutoGameSoftwareSync : {desc}</Description>
    <URI>\{name}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT{delay}S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{cmd}</Command>
      {args}
      {wd}
    </Exec>
  </Actions>
</Task>
"#,
        desc = esc(&rule.name),
        name = esc(&task_name(&rule.id)),
        user = esc(user),
        delay = rule.delay_secs,
        cmd = esc(&rule.command.display().to_string()),
        args = args,
        wd = wd
    )
}

/// XML -> octets UTF-16 LE avec BOM (encodage declare, attendu par le planificateur).
pub fn xml_to_utf16(xml: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

// ------------------------------------------------------------------ plan d'application (pur)

#[derive(Debug, Default, PartialEq)]
pub struct ApplyPlan {
    /// Taches a creer / ecraser (regles activees).
    pub create: Vec<Rule>,
    /// Taches existantes a supprimer (regle desactivee ou retiree) : SES taches uniquement.
    pub delete: Vec<String>,
}

/// Diff actuel / souhaite. `existing` : noms des taches presentes dans `\AutoGameSoftwareSync\` (id seul ou nom complet).
/// On ne supprime que les taches correspondant a une regle CONNUE (`all`) : une tache inconnue du dossier est laissee.
pub fn plan_apply(existing: &[String], all: &[Rule]) -> ApplyPlan {
    let mut plan = ApplyPlan::default();
    for r in all {
        let name = task_name(&r.id);
        let present = existing.iter().any(|e| e.trim_start_matches('\\') == name || *e == r.id);
        if r.enabled && r.found {
            plan.create.push(r.clone());
        } else if present {
            plan.delete.push(name);
        }
    }
    plan
}

// ------------------------------------------------------------------ planificateur (abstrait)

#[derive(Debug, Clone, PartialEq)]
pub enum SchedError {
    /// Droits insuffisants : une elevation (UAC) est necessaire.
    AccessDenied(String),
    Other(String),
}

impl std::fmt::Display for SchedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedError::AccessDenied(s) | SchedError::Other(s) => write!(f, "{}", s),
        }
    }
}

pub trait Scheduler {
    /// Noms complets (`AutoGameSoftwareSync\id`) des taches du dossier.
    fn list(&self) -> Result<Vec<String>, SchedError>;
    /// Anciennes taches a nous (dossier `AutoGameSoftwareSync` de la version precedente) : a migrer.
    fn list_legacy(&self) -> Result<Vec<String>, SchedError> {
        Ok(vec![])
    }
    fn create(&self, name: &str, xml_utf16: &[u8]) -> Result<(), SchedError>;
    fn delete(&self, name: &str) -> Result<(), SchedError>;
    fn run(&self, name: &str) -> Result<(), SchedError>;
    fn query_xml(&self, name: &str) -> Result<String, SchedError>;
}

#[derive(Debug, Default, PartialEq)]
pub struct ApplyOutcome {
    pub created: Vec<String>,
    pub deleted: Vec<String>,
    /// Echecs (nom, erreur). Rien d'autre n'est modifie pour une tache en echec.
    pub failed: Vec<(String, SchedError)>,
}

impl ApplyOutcome {
    pub fn needs_elevation(&self) -> bool {
        self.failed.iter().any(|(_, e)| matches!(e, SchedError::AccessDenied(_)))
    }
}

/// Applique le plan avec le planificateur donne. Refuse toute tache hors de `\AutoGameSoftwareSync\`.
pub fn apply_plan(s: &dyn Scheduler, plan: &ApplyPlan, user: &str) -> ApplyOutcome {
    let mut out = ApplyOutcome::default();
    for r in &plan.create {
        let name = task_name(&r.id);
        match s.create(&name, &xml_to_utf16(&task_xml(r, user))) {
            Ok(()) => out.created.push(name),
            Err(e) => out.failed.push((name, e)),
        }
    }
    for n in &plan.delete {
        if !is_own_task(n) && !is_legacy_task(n) {
            out.failed.push((n.clone(), SchedError::Other("tache refusee : hors du dossier AutoGameSoftwareSync".into())));
            continue;
        }
        match s.delete(n.trim_start_matches('\\')) {
            Ok(()) => out.deleted.push(n.clone()),
            Err(e) => out.failed.push((n.clone(), e)),
        }
    }
    out
}

/// Fichier echange avec le processus eleve (`--apply-startup-rules <fichier>`).
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct ApplyFile {
    pub creates: Vec<(String, String)>, // (nom de tache, XML)
    pub deletes: Vec<String>,
}

pub fn plan_to_file(plan: &ApplyPlan, user: &str) -> ApplyFile {
    ApplyFile { creates: plan.create.iter().map(|r| (task_name(&r.id), task_xml(r, user))).collect(), deletes: plan.delete.clone() }
}

/// Execute un fichier d'application (cote eleve) : validations strictes des noms.
pub fn apply_file(s: &dyn Scheduler, f: &ApplyFile) -> ApplyOutcome {
    let mut out = ApplyOutcome::default();
    for (name, xml) in &f.creates {
        if !is_own_task(name) {
            out.failed.push((name.clone(), SchedError::Other("tache refusee : hors du dossier AutoGameSoftwareSync".into())));
            continue;
        }
        match s.create(name, &xml_to_utf16(xml)) {
            Ok(()) => out.created.push(name.clone()),
            Err(e) => out.failed.push((name.clone(), e)),
        }
    }
    let creates_ok = out.failed.is_empty();
    for n in &f.deletes {
        if !is_own_task(n) && !is_legacy_task(n) {
            out.failed.push((n.clone(), SchedError::Other("tache refusee : hors du dossier AutoGameSoftwareSync".into())));
            continue;
        }
        if is_legacy_task(n) && !creates_ok {
            // On ne retire une ancienne tache que si TOUTES les nouvelles ont ete creees.
            out.failed.push((n.clone(), SchedError::Other("ancienne tache conservee : la creation des nouvelles a echoue".into())));
            continue;
        }
        match s.delete(n.trim_start_matches('\\')) {
            Ok(()) => out.deleted.push(n.clone()),
            Err(e) => out.failed.push((n.clone(), e)),
        }
    }
    out
}

// ------------------------------------------------------------------ planificateur reel (schtasks, sans fenetre)

pub struct SchtasksScheduler {
    pub tmp_dir: PathBuf,
}

#[cfg(windows)]
fn classify(text: &str) -> SchedError {
    let l = text.to_lowercase();
    if l.contains("denied") || l.contains("refus") || l.contains("0x80070005") {
        SchedError::AccessDenied(text.trim().to_string())
    } else {
        SchedError::Other(text.trim().to_string())
    }
}

impl SchtasksScheduler {
    #[cfg(not(windows))]
    fn run_cmd(&self, _args: &[&str]) -> Result<String, SchedError> {
        Err(SchedError::Other("planificateur de taches indisponible sous Linux".into()))
    }

    #[cfg(windows)]
    fn run_cmd(&self, args: &[&str]) -> Result<String, SchedError> {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("schtasks")
            .args(args)
            .stdin(std::process::Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .map_err(|e| SchedError::Other(format!("schtasks introuvable : {}", e)))?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if out.status.success() { Ok(text) } else { Err(classify(&text)) }
    }
}

impl SchtasksScheduler {
    fn list_where(&self, keep: &dyn Fn(&str) -> bool) -> Result<Vec<String>, SchedError> {
        // Un dossier de taches inexistant n'est pas une erreur : aucune tache.
        let text = match self.run_cmd(&["/query", "/fo", "csv", "/nh"]) {
            Ok(t) => t,
            Err(e) => return Err(e),
        };
        let mut v = Vec::new();
        for l in text.lines() {
            let first = l.trim().trim_start_matches('"').split("\",").next().unwrap_or("").trim_matches('"').to_string();
            let n = first.trim_start_matches('\\').to_string();
            if keep(&n) {
                v.push(n);
            }
        }
        v.sort();
        v.dedup();
        Ok(v)
    }
}

impl Scheduler for SchtasksScheduler {
    fn list(&self) -> Result<Vec<String>, SchedError> {
        self.list_where(&is_own_task)
    }

    fn list_legacy(&self) -> Result<Vec<String>, SchedError> {
        self.list_where(&is_legacy_task)
    }

    fn create(&self, name: &str, xml_utf16: &[u8]) -> Result<(), SchedError> {
        if !is_own_task(name) {
            return Err(SchedError::Other("tache refusee : hors du dossier AutoGameSoftwareSync".into()));
        }
        std::fs::create_dir_all(&self.tmp_dir).map_err(|e| SchedError::Other(e.to_string()))?;
        let f = self.tmp_dir.join(format!("task-{}-{}.xml", std::process::id(), crate::util::now_ms()));
        std::fs::write(&f, xml_utf16).map_err(|e| SchedError::Other(e.to_string()))?;
        let r = self.run_cmd(&["/create", "/xml", &f.display().to_string(), "/tn", name, "/f"]);
        let _ = std::fs::remove_file(&f); // notre fichier temporaire
        r.map(|_| ())
    }

    fn delete(&self, name: &str) -> Result<(), SchedError> {
        if !is_own_task(name) && !is_legacy_task(name) {
            return Err(SchedError::Other("tache refusee : hors du dossier AutoGameSoftwareSync".into()));
        }
        self.run_cmd(&["/delete", "/tn", name, "/f"]).map(|_| ())
    }

    fn run(&self, name: &str) -> Result<(), SchedError> {
        if !is_own_task(name) {
            return Err(SchedError::Other("tache refusee : hors du dossier AutoGameSoftwareSync".into()));
        }
        self.run_cmd(&["/run", "/tn", name]).map(|_| ())
    }

    fn query_xml(&self, name: &str) -> Result<String, SchedError> {
        self.run_cmd(&["/query", "/tn", name, "/xml"])
    }
}

// ------------------------------------------------------------------ attente du dossier partage (pur)

/// Attend que `ok()` devienne vrai (dossier partage accessible), sans boucle serree : `sleep(step)`
/// entre deux essais, abandon apres `timeout_secs` ou si `stop()`. Renvoie true si accessible.
pub fn wait_until(ok: &dyn Fn() -> bool, stop: &dyn Fn() -> bool, sleep: &dyn Fn(u64), timeout_secs: u64, step_secs: u64) -> bool {
    let mut waited = 0;
    loop {
        if ok() {
            return true;
        }
        if stop() || waited >= timeout_secs {
            return false;
        }
        sleep(step_secs);
        waited += step_secs;
    }
}

// ------------------------------------------------------------------ etat affiche

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleStatus {
    /// La tache existe.
    Active,
    /// Pas de tache.
    Inactive,
    /// Le programme vise n'existe pas.
    NotFound,
}

pub fn rule_status(rule: &Rule, existing: &[String]) -> RuleStatus {
    if !rule.found {
        return RuleStatus::NotFound;
    }
    let name = task_name(&rule.id);
    if existing.iter().any(|e| e.trim_start_matches('\\') == name) { RuleStatus::Active } else { RuleStatus::Inactive }
}

/// Identifiant d'un nouvel outil personnalise.
pub fn next_custom_id(cfg: &[StartupRuleCfg]) -> String {
    (1..).map(|n| format!("custom-{}", n)).find(|id| !cfg.iter().any(|c| &c.id == id)).unwrap()
}

/// Utilisateur courant (`DOMAIN\user`).
pub fn current_user() -> String {
    let d = std::env::var("USERDOMAIN").unwrap_or_default();
    let u = std::env::var("USERNAME").unwrap_or_default();
    if d.is_empty() { u } else { format!("{}\\{}", d, u) }
}

/// Resout un outil cloud a lancer (`--launch-tool <id>`) : chemin + arguments, re-resolus A CHAQUE demarrage.
pub fn resolve_tool(id: &str, probe: &dyn ToolProbe) -> Option<Tool> {
    detect_tools(probe).into_iter().find(|t| t.id == id)
}
