//! A top-down (orthographic, north up) render of a placed world: what a camera straight above the
//! map sees, for the area the radar covers ([`crate::radar::Grid`]).
//!
//! Every full-detail instance of the outside world (area 0) is drawn: its mesh placed by its matrix,
//! the highest surface at each pixel winning (z-buffer), textures sampled at the nearest texel
//! (repeating), multiplied by the prelit vertex colour and, for MODULATE geometry, the material
//! colour. Texels with alpha below 128 are not drawn (an alpha test; the game's own alpha reference
//! is a render state, not a file value). There is no lighting, sky or water plane: those are not
//! map files. The picture is rendered in tiles on worker threads and streamed to one PNG.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::radar::Grid;
use crate::scene::Scene;
use crate::world::{Matrix, World};

/// Pixels per tile side.
const TILE: usize = 512;

struct Placed<'a> {
    mesh: &'a crate::scene::Mesh,
    m: Matrix,
    /// World-space bounds in x, y: min x, min y, max x, max y.
    bounds: [f32; 4],
}

fn apply(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|k| m[0][k] * p[0] + m[1][k] * p[1] + m[2][k] * p[2] + m[3][k])
}

/// The picture: world square of `grid`, `ppu` pixels per world unit.
pub struct View {
    pub min: f32,
    pub size: f32,
    pub ppu: f32,
}

impl View {
    pub fn of(grid: Grid, ppu: f32) -> View {
        View { min: grid.min, size: grid.tile * grid.side as f32, ppu }
    }
    pub fn pixels(&self) -> usize {
        (self.size * self.ppu).round() as usize
    }
    /// World (x, y) to pixel co-ordinates (x right, y down).
    fn to_px(&self, x: f32, y: f32) -> (f32, f32) {
        ((x - self.min) * self.ppu, (self.min + self.size - y) * self.ppu)
    }
}

fn render_tile(placed: &[Placed], scene: &Scene, view: &View, tx: usize, ty: usize, w: usize, h: usize) -> Vec<u8> {
    let mut color = vec![0u8; w * h * 4];
    let mut depth = vec![f32::NEG_INFINITY; w * h];
    let (x0, y0) = ((tx * TILE) as f32, (ty * TILE) as f32);
    for p in placed {
        let (bx0, by1) = view.to_px(p.bounds[0], p.bounds[1]);
        let (bx1, by0) = view.to_px(p.bounds[2], p.bounds[3]);
        if bx1 < x0 || bx0 > x0 + w as f32 || by1 < y0 || by0 > y0 + h as f32 {
            continue;
        }
        let verts: Vec<[f32; 3]> = p
            .mesh
            .positions
            .iter()
            .map(|&v| {
                let wv = apply(&p.m, v);
                let (px, py) = view.to_px(wv[0], wv[1]);
                [px - x0, py - y0, wv[2]]
            })
            .collect();
        for part in &p.mesh.parts {
            let mat = &scene.materials[part.material];
            let tex = mat.texture.map(|t| &scene.textures[t].image);
            for tri in part.indices.as_chunks::<3>().0 {
                let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
                let (va, vb, vc) = (verts[a], verts[b], verts[c]);
                let area = (vb[0] - va[0]) * (vc[1] - va[1]) - (vb[1] - va[1]) * (vc[0] - va[0]);
                if area.abs() < 1e-12 {
                    continue;
                }
                let minx = va[0].min(vb[0]).min(vc[0]).floor().max(0.0) as usize;
                let maxx = (va[0].max(vb[0]).max(vc[0]).ceil().min(w as f32)) as usize;
                let miny = va[1].min(vb[1]).min(vc[1]).floor().max(0.0) as usize;
                let maxy = (va[1].max(vb[1]).max(vc[1]).ceil().min(h as f32)) as usize;
                for py in miny..maxy {
                    for px in minx..maxx {
                        let (sx, sy) = (px as f32 + 0.5, py as f32 + 0.5);
                        // Barycentric weights at the pixel centre; inside when all share the sign.
                        let w0 = ((vb[0] - sx) * (vc[1] - sy) - (vb[1] - sy) * (vc[0] - sx)) / area;
                        let w1 = ((vc[0] - sx) * (va[1] - sy) - (vc[1] - sy) * (va[0] - sx)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                            continue;
                        }
                        let z = w0 * va[2] + w1 * vb[2] + w2 * vc[2];
                        let o = py * w + px;
                        if z <= depth[o] {
                            continue;
                        }
                        let mut rgba = if mat.texture.is_none() || mat.modulate { mat.color } else { [255; 4] };
                        if let (Some(img), Some(uvs)) = (tex, &p.mesh.uvs) {
                            let u = w0 * uvs[a][0] + w1 * uvs[b][0] + w2 * uvs[c][0];
                            let v = w0 * uvs[a][1] + w1 * uvs[b][1] + w2 * uvs[c][1];
                            let (iw, ih) = (img.width as usize, img.height as usize);
                            let tu = (((u - u.floor()) * iw as f32) as usize).min(iw - 1);
                            let tv = (((v - v.floor()) * ih as f32) as usize).min(ih - 1);
                            let t = &img.pixels[(tv * iw + tu) * 4..][..4];
                            if t[3] < 128 {
                                continue;
                            }
                            for k in 0..3 {
                                rgba[k] = ((rgba[k] as u32 * t[k] as u32 + 127) / 255) as u8;
                            }
                        }
                        if let Some(cols) = &p.mesh.colors {
                            for k in 0..3 {
                                let c = w0 * cols[a][k] as f32 + w1 * cols[b][k] as f32 + w2 * cols[c][k] as f32;
                                rgba[k] = ((rgba[k] as f32 * c / 255.0) + 0.5).min(255.0) as u8;
                            }
                        }
                        depth[o] = z;
                        color[o * 4..o * 4 + 3].copy_from_slice(&rgba[..3]);
                        color[o * 4 + 3] = 255;
                    }
                }
            }
        }
    }
    color
}

/// Renders the outside world's full-detail instances. Returns the picture size.
pub fn render(
    world: &World,
    scene: &Scene,
    instances: &[usize],
    view: &View,
    out: &Path,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<Option<usize>, Error> {
    let placed: Vec<Placed> = instances
        .iter()
        .filter_map(|&i| {
            let inst = &world.instances[i];
            let mesh = &scene.meshes[(*scene.by_model.get(&inst.model)?)?];
            let [lo, hi] = mesh.bounds;
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for k in 0..8 {
                let c = [
                    if k & 1 == 0 { lo[0] } else { hi[0] },
                    if k & 2 == 0 { lo[1] } else { hi[1] },
                    if k & 4 == 0 { lo[2] } else { hi[2] },
                ];
                let wv = apply(&inst.matrix, c);
                b = [b[0].min(wv[0]), b[1].min(wv[1]), b[2].max(wv[0]), b[3].max(wv[1])];
            }
            Some(Placed { mesh, m: inst.matrix, bounds: b })
        })
        .collect();
    let size = view.pixels();
    let tiles = size.div_ceil(TILE);
    let tmp = out.with_extension("png.partial");
    let mut enc = png::Encoder::new(BufWriter::new(fs::File::create(&tmp)?), size as u32, size as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    if size * size > 64 << 20 {
        enc.set_compression(png::Compression::Fast);
    }
    let mut sw = enc
        .write_header()
        .map_err(|e| Error::Png(e.to_string()))?
        .into_stream_writer()
        .map_err(|e| Error::Png(e.to_string()))?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(tiles.max(1));
    for ty in 0..tiles {
        if cancel.load(Ordering::Relaxed) {
            drop(sw);
            let _ = fs::remove_file(&tmp);
            return Ok(None);
        }
        let h = TILE.min(size - ty * TILE);
        // One row of tiles, rendered in parallel.
        let row: Vec<Vec<u8>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|t| {
                    let placed = &placed;
                    s.spawn(move || {
                        (t..tiles)
                            .step_by(threads)
                            .map(|tx| (tx, render_tile(placed, scene, view, tx, ty, TILE.min(size - tx * TILE), h)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut all: Vec<(usize, Vec<u8>)> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
            all.sort_by_key(|(tx, _)| *tx);
            all.into_iter().map(|(_, v)| v).collect()
        });
        for y in 0..h {
            for (tx, tile) in row.iter().enumerate() {
                let w = TILE.min(size - tx * TILE);
                sw.write_all(&tile[y * w * 4..(y + 1) * w * 4])?;
            }
        }
        progress(ty + 1, tiles);
    }
    sw.finish().map_err(|e| Error::Png(e.to_string()))?;
    fs::rename(&tmp, out)?;
    Ok(Some(size))
}
