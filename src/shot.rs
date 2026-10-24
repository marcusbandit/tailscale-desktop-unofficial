//! Headless self-capture for testing: `TSDU_SHOT=out.png` renders a few seconds,
//! saves the window to a PNG and quits. `TSDU_VIEW` (devices, graph, taildrop)
//! and `TSDU_SELECT` (device name) pick what is on screen.

use std::time::Instant;

use eframe::egui::{self, ColorImage, Event, ViewportCommand};

pub struct Shot {
    pub path: String,
    started: Instant,
    requested: bool,
}

impl Shot {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            path: std::env::var("TSDU_SHOT").ok()?,
            started: Instant::now(),
            requested: false,
        })
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        let delay: f32 = std::env::var("TSDU_DELAY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(6.0);
        if !self.requested && self.started.elapsed().as_secs_f32() > delay {
            self.requested = true;
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
        }
        let img = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = img {
            if let Err(e) = save(&img, &self.path) {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn save(img: &ColorImage, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    let data: Vec<u8> = img
        .pixels
        .iter()
        .flat_map(|c| c.to_srgba_unmultiplied())
        .collect();
    w.write_image_data(&data)?;
    Ok(())
}
