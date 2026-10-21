//! Background texture decoding for thumbnails and previews.
//!
//! Requests go to two worker threads that keep the last few parsed dictionaries; results come back
//! as RGBA and become egui textures on the UI thread. Thumbnails are cached (oldest dropped first).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};
use ef_core::GameInstall;
use ef_texture::Rgba;
use ef_texture::catalog::TexCatalog;
use ef_texture::txd::Txd;
use egui::{ColorImage, TextureHandle, TextureOptions};

/// (library entry id, asset index, level, full size?)
pub type Key = (u64, usize, usize, bool);

pub struct Request {
    pub key: Key,
    pub install: GameInstall,
    pub catalog: Arc<TexCatalog>,
}

pub enum Thumb {
    Ready(TextureHandle, [u32; 2]),
    Failed(String),
}

pub struct Thumbs {
    tx: Sender<Request>,
    rx: Receiver<(Key, Result<Rgba, String>)>,
    cache: HashMap<Key, Thumb>,
    order: VecDeque<Key>,
    pending: HashSet<Key>,
}

pub const THUMB_SIDE: u32 = 160;
const CACHE: usize = 900;

fn worker(rx: Receiver<Request>, out: Sender<(Key, Result<Rgba, String>)>, ctx: egui::Context) {
    let mut txds: VecDeque<((u64, usize), Arc<Txd>)> = VecDeque::new();
    while let Ok(req) = rx.recv() {
        let (entry, asset, level, full) = req.key;
        let a = &req.catalog.assets[asset];
        let txd = match txds.iter().find(|(k, _)| *k == (entry, a.source)) {
            Some((_, t)) => Ok(t.clone()),
            None => req.catalog.load(&req.install, a.source).map(Arc::new).map_err(|e| e.to_string()),
        };
        let result = txd.and_then(|t| {
            if !txds.iter().any(|(k, _)| *k == (entry, a.source)) {
                txds.push_back(((entry, a.source), t.clone()));
                if txds.len() > 6 {
                    txds.pop_front();
                }
            }
            let tex = t.textures.get(a.index).ok_or("texture missing from its dictionary")?;
            let r = &tex.raster;
            let lv = if full {
                level
            } else {
                // the smallest level that is still at least the thumbnail size
                (0..r.levels.len())
                    .rev()
                    .find(|&i| r.levels[i].width.max(r.levels[i].height) >= THUMB_SIDE)
                    .unwrap_or(0)
            };
            let img = r.decode(lv).map_err(|e| e.to_string())?;
            Ok(if full { img } else { shrink(&img, THUMB_SIDE) })
        });
        if out.send((req.key, result)).is_err() {
            break;
        }
        ctx.request_repaint();
    }
}

/// Box-filters `img` so that its longer side is at most `side` (alpha-weighted averages).
fn shrink(img: &Rgba, side: u32) -> Rgba {
    let (w, h) = (img.width, img.height);
    let scale = (w.max(h) as f32 / side as f32).max(1.0);
    if scale <= 1.0 {
        return img.clone();
    }
    let (nw, nh) = (((w as f32 / scale).round() as u32).max(1), ((h as f32 / scale).round() as u32).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
            let (mut acc, mut asum, mut n) = ([0f32; 3], 0f32, 0f32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = &img.pixels[((sy * w + sx) * 4) as usize..][..4];
                    let a = p[3] as f32;
                    for c in 0..3 {
                        acc[c] += p[c] as f32 * a;
                    }
                    asum += a;
                    n += 1.0;
                }
            }
            let o = &mut out[((y * nw + x) * 4) as usize..][..4];
            for c in 0..3 {
                o[c] = if asum > 0.0 { (acc[c] / asum).round() as u8 } else { 0 };
            }
            o[3] = (asum / n).round() as u8;
        }
    }
    Rgba { width: nw, height: nh, pixels: out }
}

impl Thumbs {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, wrx) = unbounded::<Request>();
        let (wtx, rx) = unbounded();
        for i in 0..2 {
            let (r, t, c) = (wrx.clone(), wtx.clone(), ctx.clone());
            thread::Builder::new()
                .name(format!("texture-decode-{i}"))
                .spawn(move || worker(r, t, c))
                .expect("spawn decoder");
        }
        Self { tx, rx, cache: HashMap::new(), order: VecDeque::new(), pending: HashSet::new() }
    }

    /// Moves finished decodes into the cache. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, r)) = self.rx.try_recv() {
            self.pending.remove(&key);
            let t = match r {
                Ok(img) => {
                    let size = [img.width, img.height];
                    let ci = ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
                    let opts = if key.3 { TextureOptions::NEAREST } else { TextureOptions::LINEAR };
                    Thumb::Ready(ctx.load_texture(format!("tex-{key:?}"), ci, opts), size)
                }
                Err(e) => Thumb::Failed(e),
            };
            self.cache.insert(key, t);
            self.order.push_back(key);
            while self.order.len() > CACHE {
                if let Some(k) = self.order.pop_front() {
                    self.cache.remove(&k);
                }
            }
        }
    }

    /// The cached image for `key`, requesting it if it is not there yet.
    pub fn get(&mut self, key: Key, install: &GameInstall, catalog: &Arc<TexCatalog>) -> Option<&Thumb> {
        if !self.cache.contains_key(&key) && self.pending.insert(key) {
            let _ = self.tx.send(Request { key, install: install.clone(), catalog: catalog.clone() });
        }
        self.cache.get(&key)
    }

    /// Drops everything belonging to a library entry.
    pub fn forget(&mut self, entry: u64) {
        self.cache.retain(|k, _| k.0 != entry);
        self.order.retain(|k| k.0 != entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_keeps_aspect_and_averages() {
        let img = Rgba { width: 4, height: 2, pixels: [[255, 0, 0, 255], [0, 0, 255, 255]].repeat(4).concat() };
        let s = shrink(&img, 2);
        assert_eq!((s.width, s.height), (2, 1));
        assert_eq!(&s.pixels[..4], &[128, 0, 128, 255]);
        let small = Rgba { width: 8, height: 8, pixels: vec![9; 256] };
        assert_eq!(shrink(&small, 160), small);
    }
}
