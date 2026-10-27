//! Connaissance intelligente dans l'interface (minimale : la refonte viendra) : thread de recherche en ligne discret
//! (jamais pendant l'utilisation d'un programme detecte ni en plein ecran), bloc « Emplacement trouve » avec un niveau
//! de confiance (carte de detection et ajout manuel), file locale des emplacements acceptes.
use super::*;
use crate::knowledge::{self, Found, Layer, Program};

/// Qui attend le resultat : la carte de detection (cle de la carte) ou l'ajout manuel (exe saisi en minuscules).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KnowTarget {
    Card(String),
    Manual(String),
}

/// Acces reseau interrompu des qu'un programme detecte demarre ou qu'une application passe en plein ecran (la recherche
/// est alors comptee comme « hors ligne » : ni pause de moteur, ni quota, nouvel essai plus tard).
struct Guarded<'a> {
    inner: knowledge::web::Http,
    stop: &'a dyn Fn() -> bool,
}

impl knowledge::web::Fetch for Guarded<'_> {
    fn get(&self, url: &str) -> Result<knowledge::web::Resp, String> {
        if (self.stop)() {
            return Err("interrompue : un programme est en cours d'utilisation".into());
        }
        self.inner.get(url)
    }
}

impl Shared {
    /// Demande une recherche en ligne (une seule demande en attente par programme).
    pub fn request_web(&self, t: KnowTarget, p: Program) {
        if p.key.is_empty() {
            return;
        }
        if let Ok(mut q) = self.know_jobs.lock() {
            if let Some(e) = q.iter_mut().find(|(_, x)| x.key == p.key) {
                e.0 = t; // la demande la plus recente recoit le resultat
            } else {
                q.push((t, p));
            }
            while q.len() > 20 {
                q.remove(0);
            }
        }
    }

    pub(super) fn knowledge_busy(&self) -> bool {
        self.games_running.load(Ordering::SeqCst) || self.det_running.load(Ordering::SeqCst) || winutil::foreground_is_fullscreen()
    }

    /// Thread de recherche en ligne : une demande a la fois, seulement quand aucun programme detecte ne tourne.
    pub(super) fn spawn_knowledge_thread(self: &Arc<Self>, tx: Sender<Msg>, ctx: egui::Context) {
        let me = self.clone();
        let _ = std::thread::Builder::new().name("agss-knowledge".into()).spawn(move || {
            let lg = Logger::new(me.data_dir.join("logs").join("watch.log"), false);
            loop {
                std::thread::sleep(Duration::from_secs(3));
                if me.quitting.load(Ordering::SeqCst) {
                    return;
                }
                if me.knowledge_busy() {
                    continue;
                }
                let job = me.know_jobs.lock().ok().and_then(|mut q| if q.is_empty() { None } else { Some(q.remove(0)) });
                let Some((t, p)) = job else { continue };
                // Une couche precedente a trouve entre-temps (apprentissage a la fermeture) : pas de recherche en ligne.
                let local = knowledge::find_local(&p, &me.data_dir, crate::util::now_ms());
                if !local.is_empty() {
                    let _ = tx.send(Msg::Known(t, p, local));
                    ctx.request_repaint();
                    continue;
                }
                // Verification manuelle sur dossiers simules : aucun nom de test n'est envoye en ligne.
                if knowledge::paths::Folders::test_root().is_some() {
                    lg.log(&format!("[Connaissance] {} : recherche en ligne desactivee (dossiers simules)", p.exe));
                    continue;
                }
                let f = p.folders();
                let stop = || me.knowledge_busy() || me.quitting.load(Ordering::SeqCst);
                let fetch = Guarded { inner: knowledge::web::Http::new(), stop: &stop };
                let r = knowledge::web::run(&p, &me.data_dir, &fetch, &f, crate::util::now_ms(), 1500);
                match r.skipped {
                    Some(why) => lg.log(&format!("[Connaissance] {} : recherche non faite ({})", p.exe, why)),
                    None => lg.log(&format!("[Connaissance] {} : recherche terminee ({}), {} emplacement(s) verifie(s)", p.exe, r.engine.as_deref().unwrap_or("aucune reponse"), r.found.len())),
                }
                let _ = tx.send(Msg::Known(t, p, r.found));
                ctx.request_repaint();
            }
        });
    }
}

/// Apprentissages en cours dans le thread de detection : cle de carte -> (programme, observation).
pub(super) type Learners = HashMap<String, (Program, knowledge::learn::Learner)>;

impl Shared {
    /// Programme detecte dont l'emplacement n'est connu ni de la base, ni des couches 1 et 2, ni du cache : a apprendre.
    pub(super) fn needs_learning(&self, c: &crate::gamedetect::Candidate, db: &crate::gamedb::GameDb) -> Option<Program> {
        let libs: Vec<PathBuf> = detect::steam_root().map(|r| detect::steam_libraries(&r)).unwrap_or_default();
        let s = crate::autofill::lookup(db, &c.exe, c.path.as_deref(), &[], &libs, &|p| p.exists());
        if s.iter().any(|x| x.exists && x.local_path.is_some()) {
            return None;
        }
        let mut p = Program::describe(c.path.as_deref(), &c.exe, &c.name, c.app);
        if p.install_dir.is_none() {
            p.install_dir = c.install_dir.clone();
        }
        if !knowledge::find_local(&p, &self.data_dir, crate::util::now_ms()).is_empty() {
            return None;
        }
        Some(p)
    }

    /// Fin d'un apprentissage : dossiers candidats mis en cache ; la carte affichee sera re-resolue.
    pub(super) fn learn_done(&self, key: &str, p: &Program, l: knowledge::learn::Learner, lg: &Logger, now: u64) -> Vec<Found> {
        let v = l.finish(p);
        lg.log(&format!("[Connaissance] {} : fin d'utilisation, {} dossier(s) candidat(s) appris", p.exe, v.len()));
        if !v.is_empty() {
            let mut c = knowledge::store::Cache::load(&self.data_dir);
            c.set_learned(&p.key, v.clone(), now);
            let _ = c.save(&self.data_dir);
            if let Ok(mut r) = self.know_refresh.lock() {
                if !r.iter().any(|k| k == key) {
                    r.push(key.to_string());
                }
            }
        }
        v
    }

    /// Programmes ajoutes sans emplacement : apprentissage quand ils tournent ; a leur fermeture, carte « Emplacement
    /// trouve » s'il y a des dossiers candidats (le programme sort alors de la liste).
    pub(super) fn learn_list_tick(&self, learners: &mut Learners, list: &mut knowledge::store::LearnList, procs: &[crate::process::ProcInfo], lg: &Logger, now: u64) {
        for it in &list.items {
            let key = format!("{}|learn", it.exe);
            if learners.contains_key(&key) {
                continue;
            }
            if let Some(pr) = procs.iter().find(|p| Program::key_of(&p.exe) == it.exe) {
                let p = Program::describe(pr.path.as_deref().or(it.path.as_deref()), &pr.exe, &it.name, false);
                lg.log(&format!("[Connaissance] {} : programme a apprendre en cours d'utilisation", p.exe));
                let l = knowledge::learn::Learner::start(&p, p.folders(), now);
                learners.insert(key, (p, l));
            }
        }
        let ended: Vec<String> = learners
            .keys()
            .filter(|k| k.ends_with("|learn"))
            .filter(|k| {
                let exe = k.trim_end_matches("|learn");
                !procs.iter().any(|p| Program::key_of(&p.exe) == exe)
            })
            .cloned()
            .collect();
        for k in ended {
            let Some((p, l)) = learners.remove(&k) else { continue };
            let v = self.learn_done(&k, &p, l, lg, now);
            if v.is_empty() {
                continue; // rien appris : on reessaiera a la prochaine utilisation
            }
            list.remove(&p.exe);
            let _ = list.save(&self.data_dir);
            let name = p.names().into_iter().next().unwrap_or_else(|| p.name.clone());
            let cand = crate::gamedetect::Candidate { key: k.clone(), exe: p.key.clone(), path: p.exe_path.clone(), why: crate::gamedetect::Why::Learned, name, install_dir: p.install_dir.clone(), profile_id: None, app: p.app, confidence: None };
            self.queue_card(cand, now);
        }
    }
}

impl App {
    pub(super) fn on_known(&mut self, t: KnowTarget, prog: Program, found: Vec<Found>) {
        match t {
            KnowTarget::Card(k) => {
                if self.prop_key.as_deref() != Some(k.as_str()) {
                    return;
                }
                let base_found = self.prop_sugg.iter().any(|s| s.exists && s.local_path.is_some());
                if !base_found && !found.is_empty() && self.prop_path.trim().is_empty() {
                    self.prop_path = found[0].path.display().to_string();
                }
                self.prop_prog = Some(prog);
                if !found.is_empty() || self.prop_found.is_empty() {
                    self.prop_found = found;
                }
            }
            KnowTarget::Manual(k) => {
                if self.auto_key != k {
                    return;
                }
                self.man_prog = Some(prog);
                self.man_found_key = k;
                if !found.is_empty() || self.man_found.is_empty() {
                    self.man_found = found;
                }
            }
        }
    }

    fn know_level(&self, c: u8) -> &'static str {
        self.t(match knowledge::level(c) {
            knowledge::Level::High => K::KnowHigh,
            knowledge::Level::Medium => K::KnowMedium,
            knowledge::Level::Low => K::KnowLow,
        })
    }

    fn know_source(&self, l: Layer) -> &'static str {
        // La recherche en ligne n'est jamais annoncee dans l'interface : « verifie sur ce PC ».
        self.t(match l {
            Layer::Launcher => K::KnowSrcLauncher,
            Layer::Deduce => K::KnowSrcDeduce,
            Layer::Learn => K::KnowSrcLearn,
            Layer::Web => K::KnowSrcChecked,
            Layer::Home => K::KnowSrcHome,
        })
    }

    /// Bloc « Emplacement trouve · Confiance : elevee » puis une ligne par emplacement (coche, chemin, nature) ;
    /// `use_buttons` : bouton « Utiliser » par ligne. Retourne l'indice clique.
    pub(super) fn known_block(&self, ui: &mut egui::Ui, found: &[Found], use_buttons: bool) -> Option<usize> {
        let Some(best) = found.first() else { return None };
        let green = Color32::from_rgb(60, 180, 100);
        let head = format!("{} · {}", self.t(K::KnowFound), self.t(K::KnowConfidence).replace("{}", self.know_level(best.confidence)));
        let r = ui.label(RichText::new(head).small().strong().color(green));
        self.deco(ui, r, Some(K::TipKnowFound));
        let mut clicked = None;
        for (i, f) in found.iter().enumerate() {
            let kind = self.t(if f.kind == knowledge::Kind::Save { K::KnowSaves } else { K::KnowSettings });
            let mut tip = format!("{} · {} · {}", kind, self.know_source(f.layer), self.t(K::KnowConfidence).replace("{}", self.know_level(f.confidence)));
            if self.model.settings.expert_mode {
                tip = format!("{} ({}/100)", tip, f.confidence);
            }
            ui.horizontal_wrapped(|ui| {
                let r = icon_label(ui, Icon::Check, &f.path.display().to_string(), green);
                r.on_hover_text(&tip);
                if self.model.settings.expert_mode {
                    ui.label(RichText::new(format!("{} · {}/100", self.know_source(f.layer), f.confidence)).small().weak());
                }
                if use_buttons {
                    let b = ui.small_button(self.t(K::BtnHintUse));
                    if self.deco(ui, b, Some(K::TipHintUse)).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        }
        clicked
    }

    /// File locale de la future base maison : l'emplacement choisi (s'il vient de la connaissance intelligente).
    pub(super) fn accept_known(&self, prog: Option<&Program>, found: &[Found], chosen: &std::path::Path, name_typed: bool) {
        let Some(p) = prog else { return };
        let key = |x: &std::path::Path| crate::gamedetect::norm_path(x).trim_end_matches('/').to_string();
        let acc: Vec<Found> = found.iter().filter(|f| key(&f.path) == key(chosen)).cloned().collect();
        if acc.is_empty() {
            return;
        }
        let n = knowledge::queue::accept(&self.shared.data_dir, p, &acc, name_typed);
        if n > 0 {
            self.pad_log(&format!("[Connaissance] {} : emplacement accepte ajoute a la file locale (couche {})", p.exe, acc[0].layer.code()));
        }
    }
}
