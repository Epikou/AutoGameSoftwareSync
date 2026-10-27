//! Diagnostic de developpement (captures de validation des lots, jamais dans l'interface) : capture de la fenetre rendue
//! par egui, meme si elle depasse de l'ecran. `AGSS_UI_SHOT=fichier.bmp` (BMP 24 bits), `AGSS_UI_SHOT_AFTER=4` (secondes),
//! `AGSS_UI_SHOT_QUIT=1` (quitte ensuite) ; taille de la fenetre : `AGSS_UI_SIZE=1280x800` (voir `gui::run`).
use eframe::egui;
use std::path::PathBuf;

pub struct Shot {
    path: PathBuf,
    /// Instants des captures (`AGSS_UI_SHOT_AFTER=4` ou `4,9,15` : plusieurs captures, `{}` du nom = numero 1, 2, 3...).
    afters: Vec<f64>,
    idx: usize,
    asked: bool,
    quit: bool,
}

impl Shot {
    pub fn from_env() -> Option<Shot> {
        let path = std::env::var("AGSS_UI_SHOT").ok().filter(|s| !s.trim().is_empty())?;
        let mut afters: Vec<f64> = std::env::var("AGSS_UI_SHOT_AFTER").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect()).unwrap_or_default();
        if afters.is_empty() {
            afters.push(4.0);
        }
        Some(Shot { path: PathBuf::from(path), afters, idx: 0, asked: false, quit: std::env::var("AGSS_UI_SHOT_QUIT").as_deref() == Ok("1") })
    }

    fn path_for(&self, i: usize) -> PathBuf {
        let s = self.path.to_string_lossy();
        if s.contains("{}") {
            PathBuf::from(s.replace("{}", &(i + 1).to_string()))
        } else {
            self.path.clone()
        }
    }

    /// A appeler a chaque image : demande la capture a l'echeance, l'enregistre quand elle arrive. Vrai = quitter.
    pub fn tick(&mut self, ctx: &egui::Context) -> bool {
        let now = ctx.input(|i| i.time);
        let Some(after) = self.afters.get(self.idx).copied() else { return false };
        if !self.asked {
            if now >= after {
                self.asked = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64((after - now).max(0.05)));
            }
            return false;
        }
        let img = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = img {
            let _ = std::fs::write(self.path_for(self.idx), bmp(&img));
            self.idx += 1;
            self.asked = false;
            return self.idx >= self.afters.len() && self.quit;
        }
        ctx.request_repaint();
        false
    }
}

/// Image egui -> BMP 24 bits (lignes de bas en haut, alignees sur 4 octets).
pub fn bmp(img: &egui::ColorImage) -> Vec<u8> {
    let (w, h) = (img.size[0] as u32, img.size[1] as u32);
    let row = (w * 3 + 3) & !3;
    let data = row * h;
    let mut out = Vec::with_capacity((54 + data) as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&data.to_le_bytes());
    out.extend_from_slice(&[0x13, 0x0b, 0, 0, 0x13, 0x0b, 0, 0]);
    out.extend_from_slice(&[0; 8]);
    for y in (0..h).rev() {
        for x in 0..w {
            let c = img.pixels[(y * w + x) as usize];
            out.extend_from_slice(&[c.b(), c.g(), c.r()]);
        }
        for _ in 0..(row - w * 3) {
            out.push(0);
        }
    }
    out
}

/// Diagnostic de captures : `AGSS_UI_SCROLL=<px>` fait defiler la page Reglages / Ajouter de ce nombre de pixels (jamais utilise
/// par l'interface).
pub fn scroll_offset() -> Option<f32> {
    std::env::var("AGSS_UI_SCROLL").ok().and_then(|v| v.trim().parse::<f32>().ok())
}
