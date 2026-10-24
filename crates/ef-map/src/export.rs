//! Export Map files: raw copies, individual files and full maps, with `manifest.json` / `.csv`.
//!
//! Layout of the output folder:
//! * `raw/`: every map file byte for byte, by its path in the copy (IMG entries under
//!   `raw/<archive>.img/<entry>`);
//! * `<level>/`: per 2D city `map.json` (every block, column, zone, object, route, light, animation),
//!   `overlay.svg` (zones, objects, lights, bases over the render), `topdown-<style>.png` (the whole
//!   city at 64 pixels per block) and `tiles-<style>/` (16 x 16-block PNG tiles);
//! * `radar_map.png`, `radar_tiles/`, `menu_map.png` for GTA III, Vice City and San Andreas.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ef_core::GameInstall;
use ef_core::vfs::Vfs;
use serde::Serialize;

use crate::catalog::{LevelKind, MapCatalog};
use crate::topdown::{self, Tiles};
use crate::{Error, cmp, gmp, radar};

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Every map file byte for byte.
    pub raw: bool,
    /// Individual files: map data as JSON and SVG, radar tiles as PNG.
    pub individual: bool,
    /// 2D cities: the whole city as one PNG.
    pub topdown: bool,
    /// 2D cities: also 16 x 16-block PNG tiles.
    pub tiles: bool,
    /// 3D games: radar map and pause-menu map.
    pub premade: bool,
    /// 3D games: the whole placed world as a glTF scene.
    pub scene: bool,
    /// 3D games: a top-down render of the placed world.
    pub world_topdown: bool,
    /// Pixels per world unit of the world top-down render (0 = 1).
    pub world_ppu: f32,
    /// Styles chosen for maps the sources do not pair (map path -> style paths).
    pub styles: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
pub struct ExportReport {
    pub written: usize,
    pub failed: Vec<(String, String)>,
    pub cancelled: bool,
}

#[derive(Serialize)]
struct Output {
    path: String,
    kind: String,
    support: &'static str,
    source: String,
    notes: Vec<String>,
}

const FROM_SPEC: &str = "Implemented from spec";
const CITYSCAPE: &str =
    "DMA CityScape Data Structure 12.10 (CMP 2.1-2.9, styles 3.6-3.8); OpenGTA 1ae34ae3 lid rotation and column order";
const GBH: &str = "DMA GBH Map Format 8.2; gta2_re edb484d3 MapRenderer.cpp lid orientation and shapes";

fn safe(s: &str) -> String {
    ef_texture::export::sanitize(s)
}

/// A VFS path (with IMG entries as `x.img/entry`) as a relative output path.
fn rel_path(p: &str) -> PathBuf {
    p.split('/').map(safe).collect()
}

/// Reads a file of the copy; `archive.img/entry` reads the IMG entry.
pub fn read(vfs: &std::sync::Arc<dyn Vfs>, path: &str) -> Result<Vec<u8>, Error> {
    if let Some((archive, entry)) = split_archive(path) {
        let img = ef_core::img::ImgVfs::open(vfs, archive).map_err(|e| Error::Unsupported(e.to_string()))?;
        return Ok(img.read_all(entry)?);
    }
    Ok(vfs.read_all(path)?)
}

fn split_archive(path: &str) -> Option<(&str, &str)> {
    let i = path.to_ascii_lowercase().find(".img/")?;
    Some((&path[..i + 4], &path[i + 5..]))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("partial");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn json<T: Serialize>(v: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec_pretty(v).map_err(|e| Error::Io(std::io::Error::other(e)))
}

/// The map data of a GTA1 city as JSON.
pub fn city_json(m: &cmp::CityMap) -> serde_json::Value {
    let blocks: Vec<_> = m
        .blocks
        .iter()
        .map(|b| {
            serde_json::json!({
                "type_map": b.type_map, "type_map_ext": b.type_map_ext,
                "left": b.left, "right": b.right, "top": b.top, "bottom": b.bottom, "lid": b.lid,
                "block_type": b.block_type(), "directions": b.directions(), "flat": b.flat(),
                "slope": b.slope(), "rotation": b.rotation(), "remap": b.remap(),
                "traffic_lights": b.traffic_lights(), "flip_top_bottom": b.flip_top_bottom(),
                "flip_left_right": b.flip_left_right(), "railway": b.railway(),
            })
        })
        .collect();
    serde_json::json!({
        "format": "GTA1 CMP 331 (CityScape Data Structure 12.10)",
        "style_number": m.style, "sample_number": m.sample,
        "columns_note": "columns[y * 256 + x]: block numbers, ground first",
        "columns": m.columns, "blocks": blocks,
        "objects": m.objects.iter().filter(|o| !o.is_car()).collect::<Vec<_>>(),
        "cars": m.objects.iter().filter(|o| o.is_car()).collect::<Vec<_>>(),
        "routes": m.routes, "locations": m.locations, "nav": m.nav,
    })
}

/// The map data of a GTA2 map as JSON.
pub fn gbh_json(m: &gmp::GbhMap) -> serde_json::Value {
    let face = |w: u16| {
        let f = gmp::Face(w);
        serde_json::json!({"tile": f.tile(), "flat": f.flat(), "flip": f.flip(), "rotation": f.rotation(), "bits_10_11": (w >> 10) & 3})
    };
    let blocks: Vec<_> = m
        .blocks
        .iter()
        .map(|b| {
            serde_json::json!({
                "left": face(b.left), "right": face(b.right), "top": face(b.top), "bottom": face(b.bottom),
                "lid": face(b.lid), "arrows": b.arrows, "ground_type": b.ground_type(), "slope": b.slope(),
            })
        })
        .collect();
    let zones: Vec<_> = m
        .zones
        .iter()
        .map(|z| serde_json::json!({"type": z.zone_type, "type_name": z.type_name(), "x": z.x, "y": z.y, "w": z.w, "h": z.h, "name": z.name}))
        .collect();
    serde_json::json!({
        "format": format!("GTA2 GMP 500 (GBH Map Format 8.2), map chunk {}", m.map_chunk),
        "columns_note": "columns[y * 256 + x][z]: block number at height z (0 = air)",
        "columns": m.columns, "blocks": blocks, "zones": zones,
        "objects": m.objects.iter().map(|o| serde_json::json!({"x_blocks": gmp::fix16(o.x), "y_blocks": gmp::fix16(o.y), "rotation": o.rotation, "object_type": o.object_type})).collect::<Vec<_>>(),
        "lights": m.lights.iter().map(|l| serde_json::json!({"argb": format!("{:08X}", l.argb), "x": gmp::fix16(l.x), "y": gmp::fix16(l.y), "z": gmp::fix16(l.z), "radius": gmp::fix16(l.radius), "intensity": l.intensity, "shape": l.shape, "on_time": l.on_time, "off_time": l.off_time})).collect::<Vec<_>>(),
        "animations": m.animations,
        "chunks": m.chunks.iter().map(|(n, s)| serde_json::json!({"name": n, "size": s})).collect::<Vec<_>>(),
    })
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// An SVG of named rectangles and points over `background` (block co-ordinates, 64 px per block,
/// relative to `origin`).
fn overlay(
    origin: (usize, usize),
    size: (usize, usize),
    background: Option<&str>,
    rects: &[(f32, f32, f32, f32, String, &str)],
    points: &[(f32, f32, String, &str)],
) -> String {
    let px = |v: f32, o: usize| (v - o as f32) * 64.0;
    let (w, h) = (size.0 * 64, size.1 * 64);
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n"
    );
    if let Some(bg) = background {
        let _ = writeln!(
            s,
            "<image xlink:href=\"{}\" href=\"{}\" x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\"/>",
            xml(bg),
            xml(bg)
        );
    }
    for (x, y, rw, rh, label, colour) in rects {
        let _ = writeln!(
            s,
            "<g><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{colour}\" fill-opacity=\"0.12\" stroke=\"{colour}\" stroke-width=\"3\"/><text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"28\" fill=\"{colour}\">{}</text></g>",
            px(*x, origin.0),
            px(*y, origin.1),
            rw * 64.0,
            rh * 64.0,
            px(*x, origin.0) + 6.0,
            px(*y, origin.1) + 32.0,
            xml(label)
        );
    }
    for (x, y, label, colour) in points {
        let _ = writeln!(
            s,
            "<g><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"10\" fill=\"{colour}\" stroke=\"black\" stroke-width=\"2\"/><title>{}</title></g>",
            px(*x, origin.0),
            px(*y, origin.1),
            xml(label)
        );
    }
    s.push_str("</svg>\n");
    s
}

fn style_label(p: &str) -> String {
    let name = p.rsplit('/').next().unwrap_or(p);
    safe(&name.replace('.', "-"))
}

pub fn export(
    install: &GameInstall,
    cat: &MapCatalog,
    opts: &ExportOptions,
    out: &Path,
    progress: &(dyn Fn(usize, usize, &str) + Sync),
    cancel: &AtomicBool,
) -> Result<ExportReport, Error> {
    fs::create_dir_all(out)?;
    let vfs = &install.vfs;
    let mut rep = ExportReport::default();
    let mut outputs: Vec<Output> = Vec::new();
    let mut notices = cat.notices.clone();
    let level_steps = if opts.individual || opts.topdown || opts.tiles { cat.levels.len() * 64 } else { 0 };
    let total = if opts.raw { cat.files.len() } else { 0 } + level_steps + if opts.premade { 64 } else { 0 };
    let total = total.max(1);
    let mut done = 0usize;

    if opts.raw {
        for f in &cat.files {
            if cancel.load(Ordering::Relaxed) {
                rep.cancelled = true;
                break;
            }
            progress(done, total, &f.path);
            match read(vfs, &f.path).and_then(|b| write_atomic(&out.join("raw").join(rel_path(&f.path)), &b)) {
                Ok(()) => {
                    rep.written += 1;
                    outputs.push(Output {
                        path: format!("raw/{}", f.path),
                        kind: format!("raw copy: {}", f.kind),
                        support: "Byte for byte",
                        source: f.path.clone(),
                        notes: vec![],
                    });
                }
                Err(e) => rep.failed.push((f.path.clone(), e.to_string())),
            }
            done += 1;
        }
    }

    if (opts.individual || opts.topdown || opts.tiles) && !rep.cancelled {
        for level in &cat.levels {
            if cancel.load(Ordering::Relaxed) {
                rep.cancelled = true;
                break;
            }
            progress(done, total, &level.map);
            let dir = out.join(safe(&level.name));
            let styles: Vec<String> = match opts.styles.get(&level.map) {
                Some(s) if !s.is_empty() => s.clone(),
                _ => level.styles.iter().map(|s| s.path.clone()).collect(),
            };
            let d0 = done;
            let step = |frac: f32| progress(d0 + (frac * 63.0) as usize, total, &level.map);
            let result = export_level(vfs, level.kind, &level.map, &styles, opts, &dir, &step, cancel);
            match result {
                Ok((mut outs, mut notes)) => {
                    rep.written += outs.len();
                    for o in &mut outs {
                        o.path = format!("{}/{}", safe(&level.name), o.path);
                    }
                    outputs.append(&mut outs);
                    notices.append(&mut notes);
                }
                Err(e) => rep.failed.push((level.map.clone(), e.to_string())),
            }
            done += 64;
        }
    }

    if (opts.scene || opts.world_topdown || opts.individual)
        && !rep.cancelled
        && matches!(install.game, ef_core::Game::Gta3 | ef_core::Game::ViceCity | ef_core::Game::SanAndreas)
    {
        progress(done, total, "placed world");
        match export_world(
            install,
            opts,
            out,
            &|d, t| progress(done + d * 63 / t.max(1), total, "placed world"),
            cancel,
        ) {
            Ok((mut outs, mut notes)) => {
                rep.written += outs.len();
                outputs.append(&mut outs);
                notices.append(&mut notes);
            }
            Err(e) => rep.failed.push(("placed world".into(), e.to_string())),
        }
        done += 64;
    }

    if opts.premade && !rep.cancelled {
        if let Some((grid, files)) = cat.radar.as_ref().filter(|(_, f)| f.found() > 0) {
            progress(done, total, "radar map");
            match radar::radar_map(vfs, *grid, files, &|d, t| progress(done + d * 48 / t.max(1), total, "radar map")) {
                Ok(pic) => {
                    ef_texture::export::write_png(&out.join("radar_map.png"), &pic.image)?;
                    rep.written += 1;
                    let src = format!(
                        "radar{{00..{}}}.txd in the IMG archives; {} x {} tiles of {} px",
                        grid.side * grid.side - 1,
                        grid.side,
                        grid.side,
                        pic.tile
                    );
                    outputs.push(Output {
                        path: "radar_map.png".into(),
                        kind: format!("radar map (world X {} .. {}, north up)", grid.min, -grid.min),
                        support: FROM_SPEC,
                        source: src.clone(),
                        notes: pic.problems.clone(),
                    });
                    if opts.individual {
                        fs::create_dir_all(out.join("radar_tiles"))?;
                        for (k, t) in files.tiles.iter().enumerate() {
                            let Some((img, entry)) = t else { continue };
                            let s = pic.tile as usize;
                            let (tx, ty) = (k % grid.side, k / grid.side);
                            let mut px = Vec::with_capacity(s * s * 4);
                            let w = pic.image.width as usize;
                            for y in 0..s {
                                let o = ((ty * s + y) * w + tx * s) * 4;
                                px.extend_from_slice(&pic.image.pixels[o..o + s * 4]);
                            }
                            let tile = ef_texture::decode::Rgba { width: s as u32, height: s as u32, pixels: px };
                            ef_texture::export::write_png(
                                &out.join("radar_tiles").join(format!("radar{k:02}.png")),
                                &tile,
                            )?;
                            rep.written += 1;
                            outputs.push(Output {
                                path: format!("radar_tiles/radar{k:02}.png"),
                                kind: format!("radar tile {tx},{ty}"),
                                support: FROM_SPEC,
                                source: format!("{img}/{entry}"),
                                notes: vec![],
                            });
                        }
                    }
                    if install.game == ef_core::Game::SanAndreas {
                        let menu = radar::sa_menu_map(&pic);
                        ef_texture::export::write_png(&out.join("menu_map.png"), &menu.image)?;
                        rep.written += 1;
                        outputs.push(Output {
                            path: "menu_map.png".into(),
                            kind: "pause-menu map (radar tiles over MENU_MAP_BACKGROUND)".into(),
                            support: FROM_SPEC,
                            source: src,
                            notes: vec![],
                        });
                    }
                }
                Err(e) => rep.failed.push(("radar map".into(), e.to_string())),
            }
        }
        if cat.vc_menu {
            match radar::vc_menu_map(vfs.as_ref()) {
                Ok(pic) => {
                    ef_texture::export::write_png(&out.join("menu_map.png"), &pic.image)?;
                    rep.written += 1;
                    outputs.push(Output {
                        path: "menu_map.png".into(),
                        kind: "pause-menu map (3 x 3 frontend textures)".into(),
                        support: FROM_SPEC,
                        source: format!("{} mapTop01..mapBot03", radar::VC_MENU_TXD),
                        notes: pic.problems,
                    });
                }
                Err(e) => rep.failed.push((radar::VC_MENU_TXD.into(), e.to_string())),
            }
        }
        if install.game == ef_core::Game::Gta3 {
            notices.push("GTA III (PC) has no pause-menu map; the radar map is its premade map.".into());
        }
        done += 64;
    }
    progress(done.min(total), total, "");

    let manifest = serde_json::json!({
        "tool": format!("Eyefinder {}", env!("CARGO_PKG_VERSION")),
        "game": install.game.name(),
        "platform": install.platform.name(),
        "edition": install.edition,
        "user_choice": install.user_choice,
        "source": install.vfs.describe(),
        "detection": install.evidence,
        "options": {"raw": opts.raw, "individual": opts.individual, "topdown": opts.topdown, "tiles": opts.tiles, "premade": opts.premade, "scene": opts.scene, "world_topdown": opts.world_topdown, "world_ppu": opts.world_ppu, "styles": opts.styles},
        "outputs": outputs,
        "notices": notices,
        "failures": rep.failed,
        "cancelled": rep.cancelled,
    });
    fs::write(out.join("manifest.json"), json(&manifest)?)?;
    let mut csv = BufWriter::new(fs::File::create(out.join("manifest.csv"))?);
    writeln!(csv, "path,kind,support,source,notes")?;
    let esc = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for o in &outputs {
        writeln!(
            csv,
            "{},{},{},{},{}",
            esc(&o.path),
            esc(&o.kind),
            esc(o.support),
            esc(&o.source),
            esc(&o.notes.join("; "))
        )?;
    }
    csv.flush()?;
    Ok(rep)
}

#[allow(clippy::too_many_arguments)]
fn export_level(
    vfs: &std::sync::Arc<dyn Vfs>,
    kind: LevelKind,
    map_path: &str,
    styles: &[String],
    opts: &ExportOptions,
    dir: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> Result<(Vec<Output>, Vec<String>), Error> {
    let bytes = read(vfs, map_path)?;
    let mut outs = Vec::new();
    let mut notes = Vec::new();
    let (city, gbh) = match kind {
        LevelKind::City => (Some(cmp::parse(&bytes)?), None),
        LevelKind::Gbh => (None, Some(gmp::parse(&bytes)?)),
    };
    let source = if kind == LevelKind::City { CITYSCAPE } else { GBH };
    let topdown_src: Box<dyn topdown::Source + '_>;
    let mut rects = Vec::new();
    let mut points = Vec::new();
    if let Some(m) = &city {
        topdown_src = Box::new(topdown::CitySource { map: m, variants: true });
        for n in &m.nav {
            rects.push((n.x as f32, n.y as f32, n.w as f32, n.h as f32, n.name.clone(), "#ffd400"));
        }
        // OpenGTA gl_cityview.cpp: object positions are 64 units per block.
        for o in &m.objects {
            let what = if o.is_car() { format!("car type {}", o.kind) } else { format!("object type {}", o.kind) };
            points.push((o.x as f32 / 64.0, o.y as f32 / 64.0, what, if o.is_car() { "#33aaff" } else { "#ff5533" }));
        }
        for (name, bases) in [
            ("police station", &m.locations.police_station),
            ("hospital", &m.locations.hospital),
            ("fire station", &m.locations.fire_station),
        ] {
            for b in bases.iter().filter(|b| **b != [0, 0, 0]) {
                points.push((b[0] as f32 + 0.5, b[1] as f32 + 0.5, name.to_string(), "#ffffff"));
            }
        }
        if opts.individual {
            write_atomic(&dir.join("map.json"), &json(&city_json(m))?)?;
            outs.push(Output {
                path: "map.json".into(),
                kind: "map data (blocks, columns, objects, cars, routes, bases, navigation zones)".into(),
                support: FROM_SPEC,
                source: source.into(),
                notes: vec![],
            });
        }
    } else {
        let m = gbh.as_ref().unwrap();
        topdown_src = Box::new(topdown::GbhSource { map: m });
        for z in &m.zones {
            let colour = match z.zone_type {
                1 | 15 => "#ffd400",
                14 => "#ff3366",
                16 | 20 => "#33ff99",
                _ => "#66ccff",
            };
            rects.push((
                z.x as f32,
                z.y as f32,
                z.w as f32,
                z.h as f32,
                format!("{} ({})", z.name, z.type_name()),
                colour,
            ));
        }
        for o in &m.objects {
            points.push((gmp::fix16(o.x), gmp::fix16(o.y), format!("object type {}", o.object_type), "#ff5533"));
        }
        for l in &m.lights {
            points.push((
                gmp::fix16(l.x),
                gmp::fix16(l.y),
                format!("light {:06X}, radius {}", l.argb & 0xFFFFFF, gmp::fix16(l.radius)),
                "#ffee88",
            ));
        }
        if opts.individual {
            write_atomic(&dir.join("map.json"), &json(&gbh_json(m))?)?;
            outs.push(Output {
                path: "map.json".into(),
                kind: "map data (blocks, columns, zones, objects, lights, animations, chunks)".into(),
                support: FROM_SPEC,
                source: source.into(),
                notes: vec![],
            });
        }
    }
    let extent = topdown::extent(topdown_src.as_ref());
    let Some(extent) = extent else {
        notes.push(format!("{map_path}: no block draws a lid."));
        return Ok((outs, notes));
    };
    let origin = (extent.0, extent.1);
    let size = (extent.2 - extent.0, extent.3 - extent.1);
    if styles.is_empty() && (opts.topdown || opts.tiles) {
        notes.push(format!("{map_path}: no style is paired with this map; choose one to render it."));
    }
    let mut first_png = None;
    for (si, style_path) in styles.iter().enumerate() {
        if !(opts.topdown || opts.tiles) || cancel.load(Ordering::Relaxed) {
            break;
        }
        let sb = read(vfs, style_path)?;
        let tiles = match kind {
            LevelKind::City => Tiles::gta1_lids(&ef_texture::style::parse(&sb)?)?,
            LevelKind::Gbh => Tiles::gta2_tiles(&ef_texture::sty::parse(&sb)?)?,
        };
        let label = style_label(style_path);
        let png = format!("topdown-{label}.png");
        let tile_dir = dir.join(format!("tiles-{label}"));
        fs::create_dir_all(dir)?;
        let combined = dir.join(&png);
        let o = topdown::Output {
            combined: opts.topdown.then_some(combined.as_path()),
            tiles: opts.tiles.then_some((tile_dir.as_path(), 16)),
        };
        let n = styles.len() as f32;
        let r = topdown::render(
            topdown_src.as_ref(),
            &tiles,
            extent,
            &o,
            &|d, t| progress((si as f32 + d as f32 / t as f32) / n),
            cancel,
        )?;
        let Some((w, h)) = r else { break };
        let missing = tiles.missing.lock().unwrap().clone();
        let mut on = vec![format!(
            "blocks x {}..{}, y {}..{} (north up, 64 px per block)",
            extent.0,
            extent.2 - 1,
            extent.1,
            extent.3 - 1
        )];
        if !missing.is_empty() {
            on.push(format!(
                "{} tiles the map uses are not in the style: {:?}",
                missing.len(),
                missing.iter().take(20).collect::<Vec<_>>()
            ));
        }
        if opts.topdown {
            first_png.get_or_insert(png.clone());
            outs.push(Output {
                path: png,
                kind: format!("top-down map, {w} x {h} px"),
                support: FROM_SPEC,
                source: format!("{source}; style {style_path}"),
                notes: on.clone(),
            });
        }
        if opts.tiles {
            outs.push(Output {
                path: format!("tiles-{label}/"),
                kind: "top-down map in 16 x 16-block tiles (x{block}_y{block}.png)".into(),
                support: FROM_SPEC,
                source: format!("{source}; style {style_path}"),
                notes: on,
            });
        }
    }
    if opts.individual {
        let svg = overlay(origin, size, first_png.as_deref(), &rects, &points);
        write_atomic(&dir.join("overlay.svg"), svg.as_bytes())?;
        outs.push(Output {
            path: "overlay.svg".into(),
            kind: "zones, objects, lights and bases over the top-down map".into(),
            support: FROM_SPEC,
            source: source.into(),
            notes: vec![],
        });
    }
    Ok((outs, notes))
}

const WORLD: &str = "re3 / reVC / gta-reversed FileLoader.cpp (IDE, IPL, instance matrix), IplStore.cpp, tBinaryIplFile.h; librw 18532c2e (DFF clumps, geometry, rotation)";

/// The placed world of a 3D game: `world.json` (individual), `scene/` glTF, `world_topdown.png`.
fn export_world(
    install: &GameInstall,
    opts: &ExportOptions,
    out: &Path,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<(Vec<Output>, Vec<String>), Error> {
    let mut outs = Vec::new();
    let world = crate::world::build(install);
    let mut notes = world.problems.clone();
    if opts.individual {
        let instances: Vec<_> = world
            .instances
            .iter()
            .map(|i| {
                serde_json::json!({
                    "model": i.model, "name": i.name, "area": i.area, "lod": i.is_lod, "lod_index": i.lod_index,
                    "file": world.ipl_files.get(i.file), "position": i.matrix[3],
                    "right": i.matrix[0], "forward": i.matrix[1], "up": i.matrix[2],
                })
            })
            .collect();
        let models: Vec<_> = world
            .models
            .values()
            .map(|m| serde_json::json!({"id": m.id, "name": m.name, "txd": m.txd, "draw_distance": m.draw_distance, "flags": m.flags}))
            .collect();
        let doc = serde_json::json!({
            "note": "Matrices are in game space (Z up): right, forward, up are the images of the x, y and z axes.",
            "ipl_files": world.ipl_files, "txd_parents": world.txd_parents, "global_txds": world.global_txds,
            "models": models, "instances": instances,
        });
        write_atomic(&out.join("world.json"), &json(&doc)?)?;
        outs.push(Output {
            path: "world.json".into(),
            kind: format!("placed world data: {} models, {} instances", world.models.len(), world.instances.len()),
            support: FROM_SPEC,
            source: WORLD.into(),
            notes: vec![],
        });
    }
    if !(opts.scene || opts.world_topdown) || install.platform != ef_core::Platform::Pc {
        return Ok((outs, notes));
    }
    let all: Vec<usize> = (0..world.instances.len()).collect();
    let detail: Vec<usize> =
        all.iter().copied().filter(|&i| world.instances[i].area == 0 && !world.instances[i].is_lod).collect();
    let wanted = if opts.scene { &all } else { &detail };
    let scene = crate::scene::load(install, &world, wanted, &|d, t| progress(d * 20 / t.max(1), 64), cancel);
    notes.extend(scene.problems.iter().cloned());
    if opts.scene && !cancel.load(Ordering::Relaxed) {
        let dir = out.join("scene");
        let n = crate::gltf::write(&world, &scene, &all, &dir, &|d, t| progress(20 + d * 20 / t.max(1), 64), cancel)?;
        outs.push(Output {
            path: "scene/scene.gltf".into(),
            kind: format!(
                "3D map scene (glTF 2.0): {n} placed instances, {} models, {} textures",
                scene.meshes.len(),
                scene.textures.len()
            ),
            support: FROM_SPEC,
            source: WORLD.into(),
            notes: vec![
                "Grouped by area (0 = the outside world) and full / low detail; Z-up game space under a Y-up root."
                    .into(),
            ],
        });
    }
    if opts.world_topdown && !cancel.load(Ordering::Relaxed) {
        let grid = crate::radar::grid(install.game).expect("3D game");
        let ppu = if opts.world_ppu > 0.0 { opts.world_ppu } else { 1.0 };
        let view = crate::render3d::View::of(grid, ppu);
        let path = out.join("world_topdown.png");
        if let Some(px) = crate::render3d::render(
            &world,
            &scene,
            &detail,
            &view,
            &path,
            &|d, t| progress(40 + d * 23 / t.max(1), 64),
            cancel,
        )? {
            outs.push(Output {
                path: "world_topdown.png".into(),
                kind: format!("top-down render of the placed world, {px} x {px} px ({ppu} px per unit, world {} .. {})", grid.min, -grid.min),
                support: FROM_SPEC,
                source: WORLD.into(),
                notes: vec!["Full-detail models of the outside world, nearest texels, prelit colours; no lighting, water or sky.".into()],
            });
        }
    }
    Ok((outs, notes))
}
