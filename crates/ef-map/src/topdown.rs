//! Top-down renders of the 2D cities (GTA1 / London CMP, GTA2 GMP) at the games' native 64 pixels
//! per block: every lid of every column, lowest first, so higher lids cover lower ones. Flat lids are
//! drawn transparently (CityScape 2.1 bit 7, GBH Map 3 "flat"): their palette-0 texels let the lids
//! below show. Other lids are opaque.
//!
//! What a top-down picture leaves out: block sides (seen edge-on from above), GTA2 lid shading
//! (lighting levels 1-3 are darkened by an amount the game sets at run time), tile animations (the
//! first frame is drawn) and the GTA1 lid remap of 24-bit styles beyond the palette variant.
//!
//! A whole city is 16384 x 16384 pixels, so it is rendered in bands of block rows and streamed to the
//! PNG file, optionally also cut into tiles.

use std::fs;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use ef_texture::collection::TextureCollection;

use crate::Error;
use crate::cmp::CityMap;
use crate::gmp::{Face, GbhMap};
use crate::lid::{self, Corners, Shape};

/// Native block size of both games' tiles.
pub const BLOCK: usize = 64;

/// One lid to draw on a block.
#[derive(Debug, Clone, Copy)]
pub struct Draw {
    /// Key into [`Tiles`].
    pub tile: usize,
    pub corners: Corners,
    pub shape: Shape,
    pub flat: bool,
}

/// What a map draws on each block, lowest lid first.
pub trait Source: Sync {
    fn draws(&self, x: usize, y: usize, out: &mut Vec<Draw>);
}

/// Decoded 64x64 RGBA tiles by key.
#[derive(Default)]
pub struct Tiles {
    pub rgba: Vec<Option<Vec<u8>>>,
    /// Keys a map asked for that the style does not have.
    pub missing: std::sync::Mutex<std::collections::BTreeSet<usize>>,
}

impl Tiles {
    fn insert(&mut self, key: usize, rgba: Vec<u8>) {
        if self.rgba.len() <= key {
            self.rgba.resize(key + 1, None);
        }
        self.rgba[key] = Some(rgba);
    }
    fn get(&self, key: usize) -> Option<&[u8]> {
        let t = self.rgba.get(key).and_then(|t| t.as_deref());
        if t.is_none() {
            self.missing.lock().unwrap().insert(key);
        }
        t
    }

    /// GTA1 lids: `lid_{id}_palette_{variant}` of a GRY/G24 style, keyed `id * 4 + variant`.
    pub fn gta1_lids(style: &TextureCollection) -> Result<Tiles, Error> {
        let mut t = Tiles::default();
        for tex in &style.textures {
            let Some(rest) = tex.name.strip_prefix("lid_") else { continue };
            let Some((id, variant)) = rest.split_once("_palette_") else { continue };
            let (Ok(id), Ok(variant)) = (id.parse::<usize>(), variant.parse::<usize>()) else { continue };
            t.insert(id * 4 + variant, tex.raster.decode(0)?.pixels);
        }
        Ok(t)
    }

    /// GTA2 tiles: `tile_{n}` of a STY style, keyed by tile number.
    pub fn gta2_tiles(style: &TextureCollection) -> Result<Tiles, Error> {
        let mut t = Tiles::default();
        for tex in &style.textures {
            let Some(n) = tex.name.strip_prefix("tile_").and_then(|n| n.parse::<usize>().ok()) else { continue };
            t.insert(n, tex.raster.decode(0)?.pixels);
        }
        Ok(t)
    }
}

/// GTA1 / London: the lid of every block of a column, ground first.
pub struct CitySource<'a> {
    pub map: &'a CityMap,
    /// Whether the style has palette variants for lids (G24 always; GRY when it has remap tables).
    pub variants: bool,
}

impl Source for CitySource<'_> {
    fn draws(&self, x: usize, y: usize, out: &mut Vec<Draw>) {
        for b in self.map.column(x, y) {
            if b.lid == 0 {
                continue;
            }
            // CityScape 2.1 x3-4: remap 1-3 selects the lid's remap (the style's palette variant).
            let variant = if self.variants { b.remap() as usize } else { 0 };
            out.push(Draw {
                tile: b.lid as usize * 4 + variant,
                corners: lid::gta1_corners(b.rotation()),
                shape: Shape::Square,
                flat: b.flat(),
            });
        }
    }
}

/// GTA2: the lid of every block of a column, z = 0 first.
pub struct GbhSource<'a> {
    pub map: &'a GbhMap,
}

impl Source for GbhSource<'_> {
    fn draws(&self, x: usize, y: usize, out: &mut Vec<Draw>) {
        for z in 0..8 {
            let b = self.map.block(x, y, z);
            let face = Face(b.lid);
            let tile = face.tile();
            // 0 = blank; 992-1022 are reserved for the game engine and 1023 marks a 3-sided diagonal
            // slope (GBH Map 3); gta2_re InitTileMapping_5AA950 maps 992-1023 to no texture.
            if tile == 0 || tile >= 992 {
                continue;
            }
            out.push(Draw {
                tile: tile as usize,
                corners: lid::gta2_corners(face.rotation(), face.flip()),
                shape: lid::gta2_shape(b.slope()),
                flat: face.flat(),
            });
        }
    }
}

/// The blocks that draw anything: (x0, y0, x1, y1), end exclusive.
pub fn extent(src: &dyn Source) -> Option<(usize, usize, usize, usize)> {
    let mut e: Option<(usize, usize, usize, usize)> = None;
    let mut v = Vec::new();
    for y in 0..256 {
        for x in 0..256 {
            v.clear();
            src.draws(x, y, &mut v);
            if !v.is_empty() {
                let r = e.get_or_insert((x, y, x + 1, y + 1));
                *r = (r.0.min(x), r.1.min(y), r.2.max(x + 1), r.3.max(y + 1));
            }
        }
    }
    e
}

/// RGBA rows of block rows `y0..y1` and columns `x0..x1`.
pub fn render_band(src: &dyn Source, tiles: &Tiles, x0: usize, x1: usize, y0: usize, y1: usize) -> Vec<u8> {
    let w = (x1 - x0) * BLOCK;
    let mut img = vec![0u8; w * (y1 - y0) * BLOCK * 4];
    let mut draws = Vec::new();
    for by in y0..y1 {
        for bx in x0..x1 {
            draws.clear();
            src.draws(bx, by, &mut draws);
            for d in &draws {
                let Some(tex) = tiles.get(d.tile) else { continue };
                for py in 0..BLOCK {
                    for px in 0..BLOCK {
                        if !d.shape.covers(px, py, BLOCK) {
                            continue;
                        }
                        let (u, v) = lid::texel(&d.corners, px, py, BLOCK);
                        let c = &tex[(v * BLOCK + u) * 4..][..4];
                        if c[3] == 0 && d.flat {
                            continue;
                        }
                        let o = (((by - y0) * BLOCK + py) * w + (bx - x0) * BLOCK + px) * 4;
                        img[o..o + 3].copy_from_slice(&c[..3]);
                        img[o + 3] = 255;
                    }
                }
            }
        }
    }
    img
}

/// Where a render goes.
pub struct Output<'a> {
    /// The whole picture as one PNG.
    pub combined: Option<&'a Path>,
    /// Square PNG tiles of this many blocks, named `x{col}_y{row}.png`, in this folder.
    pub tiles: Option<(&'a Path, usize)>,
}

/// Renders `extent` band by band; `progress(done_rows, total_rows)`. Returns the pixel size.
pub fn render(
    src: &dyn Source,
    tiles: &Tiles,
    extent: (usize, usize, usize, usize),
    out: &Output,
    progress: &dyn Fn(usize, usize),
    cancel: &AtomicBool,
) -> Result<Option<(u32, u32)>, Error> {
    let (x0, y0, x1, y1) = extent;
    let (w, h) = (((x1 - x0) * BLOCK) as u32, ((y1 - y0) * BLOCK) as u32);
    let band = out.tiles.map_or(4, |(_, n)| n.max(1));
    let mut writer = match out.combined {
        Some(path) => {
            let tmp = path.with_extension("png.partial");
            let mut enc = png::Encoder::new(BufWriter::new(fs::File::create(&tmp)?), w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if (w as u64) * (h as u64) > 64 << 20 {
                enc.set_compression(png::Compression::Fast);
            }
            let header = enc.write_header().map_err(|e| Error::Png(e.to_string()))?;
            Some((header.into_stream_writer().map_err(|e| Error::Png(e.to_string()))?, tmp, path))
        }
        None => None,
    };
    let mut by = y0;
    while by < y1 {
        if cancel.load(Ordering::Relaxed) {
            if let Some((_, tmp, _)) = writer.take() {
                let _ = fs::remove_file(tmp);
            }
            return Ok(None);
        }
        let end = (by + band).min(y1);
        let rows = render_band(src, tiles, x0, x1, by, end);
        if let Some((sw, _, _)) = writer.as_mut() {
            use std::io::Write;
            sw.write_all(&rows)?;
        }
        if let Some((dir, n)) = out.tiles {
            fs::create_dir_all(dir)?;
            let row_px = (x1 - x0) * BLOCK * 4;
            let mut bx = x0;
            while bx < x1 {
                let bend = (bx + n).min(x1);
                let tw = (bend - bx) * BLOCK;
                let th = (end - by) * BLOCK;
                let mut pixels = Vec::with_capacity(tw * th * 4);
                for r in 0..th {
                    let s = r * row_px + (bx - x0) * BLOCK * 4;
                    pixels.extend_from_slice(&rows[s..s + tw * 4]);
                }
                let img = ef_texture::decode::Rgba { width: tw as u32, height: th as u32, pixels };
                ef_texture::export::write_png(&dir.join(format!("x{:03}_y{:03}.png", bx, by)), &img)?;
                bx = bend;
            }
        }
        progress(end - y0, y1 - y0);
        by = end;
    }
    if let Some((sw, tmp, path)) = writer {
        sw.finish().map_err(|e| Error::Png(e.to_string()))?;
        fs::rename(tmp, path)?;
    }
    Ok(Some((w, h)))
}
