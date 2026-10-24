//! Writes textures as PNG (8-bit RGBA, every texel as decoded) plus `manifest.json` and
//! `manifest.csv`. Optionally every mip level, and DXT-compressed textures also untouched as
//! `.dds` (DXT data copied byte for byte; header per Microsoft "DDS_HEADER structure").

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ef_core::GameInstall;
use serde::Serialize;

use crate::Error;
use crate::catalog::TexCatalog;
use crate::decode::Rgba;
use crate::raster::{NativePlatform, Raster};

#[derive(Debug, Clone, Copy)]
pub struct ExportOptions {
    /// Also write every mip level (`name.mip1.png`, ...).
    pub mipmaps: bool,
    /// Also write DXT textures untouched as `.dds`.
    pub originals: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { mipmaps: false, originals: true }
    }
}

#[derive(Debug, Default)]
pub struct ExportReport {
    pub written: usize,
    pub textures: usize,
    pub skipped: Vec<String>,
    pub cancelled: bool,
}

#[derive(Serialize)]
struct ManifestEntry<'a> {
    name: &'a str,
    mask: &'a str,
    dictionary: &'a str,
    platform: &'a str,
    format: String,
    width: u32,
    height: u32,
    mip_levels: usize,
    renderware: String,
    support: &'static str,
    files: Vec<String>,
}

pub fn sanitize(s: &str) -> String {
    let t: String =
        s.chars().map(|c| if c.is_ascii_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() { "_".into() } else { t }
}

pub fn write_png(path: &Path, img: &Rgba) -> Result<(), Error> {
    let tmp = path.with_extension("png.partial");
    let f = BufWriter::new(fs::File::create(&tmp)?);
    let mut enc = png::Encoder::new(f, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| Error::Png(e.to_string()))?;
    w.write_image_data(&img.pixels).map_err(|e| Error::Png(e.to_string()))?;
    w.finish().map_err(|e| Error::Png(e.to_string()))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// A DDS file holding the DXT levels exactly as stored (PC and Xbox DXT data is not swizzled).
pub fn dds_bytes(r: &Raster) -> Option<Vec<u8>> {
    if r.dxt == 0 || r.platform == NativePlatform::Ps2 {
        return None;
    }
    let mips = r.levels.len() as u32;
    let mut h = vec![0u8; 128];
    let put = |h: &mut Vec<u8>, o: usize, v: u32| h[o..o + 4].copy_from_slice(&v.to_le_bytes());
    h[0..4].copy_from_slice(b"DDS ");
    put(&mut h, 4, 124);
    // CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE, MIPMAPCOUNT when there are mips
    put(&mut h, 8, 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000 | if mips > 1 { 0x20000 } else { 0 });
    put(&mut h, 12, r.height);
    put(&mut h, 16, r.width);
    put(&mut h, 20, r.levels.first()?.data.len() as u32);
    put(&mut h, 28, mips);
    put(&mut h, 76, 32);
    put(&mut h, 80, 0x4); // DDPF_FOURCC
    h[84..88].copy_from_slice(&[b'D', b'X', b'T', b'0' + r.dxt]);
    // DDSCAPS_TEXTURE, plus COMPLEX | MIPMAP for mip chains
    put(&mut h, 108, 0x1000 | if mips > 1 { 0x8 | 0x40_0000 } else { 0 });
    for l in &r.levels {
        h.extend_from_slice(&l.data);
    }
    Some(h)
}

fn unique(used: &mut HashSet<PathBuf>, dir: &Path, stem: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let s = if n == 1 { stem.to_string() } else { format!("{stem} ({n})") };
        let p = dir.join(&s);
        if used.insert(p.clone()) {
            return p;
        }
        n += 1;
    }
}

/// Exports the catalog assets `assets` (indices into `catalog.assets`) below `out`.
pub fn export(
    install: &GameInstall,
    catalog: &TexCatalog,
    assets: &[usize],
    out: &Path,
    opts: ExportOptions,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancel: &AtomicBool,
) -> Result<ExportReport, Error> {
    fs::create_dir_all(out)?;
    let mut report = ExportReport::default();
    let mut by_source: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &a in assets {
        by_source.entry(catalog.assets[a].source).or_default().push(a);
    }
    let mut manifest = Vec::new();
    let mut used = HashSet::new();
    let mut done = 0;
    for (source, list) in by_source {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let src = &catalog.sources[source];
        let txd = match catalog.load(install, source) {
            Ok(t) => t,
            Err(e) => {
                report.skipped.push(format!("{}: {e}", src.path));
                done += list.len();
                continue;
            }
        };
        let rel: PathBuf =
            src.path.trim_end_matches(".txd").trim_end_matches(".TXD").split('/').map(sanitize).collect();
        let dir = out.join(rel);
        fs::create_dir_all(&dir)?;
        for a in list {
            done += 1;
            progress(done, assets.len());
            let asset = &catalog.assets[a];
            let Some(tex) = txd.textures.get(asset.index) else {
                report.skipped.push(format!("{}: texture {} is missing", src.path, asset.name));
                continue;
            };
            let r = &tex.raster;
            if !asset.support.can_decode() {
                report.skipped.push(format!("{}/{}: {}", src.path, asset.name, asset.why_unsupported.unwrap_or("")));
                continue;
            }
            let base = unique(&mut used, &dir, &sanitize(&tex.name));
            let mut files = Vec::new();
            let levels = if opts.mipmaps { r.levels.len() } else { 1.min(r.levels.len()) };
            let mut failed = None;
            for l in 0..levels {
                let img = match r.decode(l) {
                    Ok(i) => i,
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                };
                let p = if l == 0 { base.with_extension("png") } else { base.with_extension(format!("mip{l}.png")) };
                write_png(&p, &img)?;
                files.push(p.strip_prefix(out).unwrap_or(&p).to_string_lossy().replace('\\', "/"));
                report.written += 1;
            }
            if let Some(e) = failed {
                report.skipped.push(format!("{}/{}: {e}", src.path, asset.name));
                if files.is_empty() {
                    continue;
                }
            }
            if opts.originals
                && let Some(d) = dds_bytes(r)
            {
                let p = base.with_extension("dds");
                fs::write(&p, d)?;
                files.push(p.strip_prefix(out).unwrap_or(&p).to_string_lossy().replace('\\', "/"));
                report.written += 1;
            }
            report.textures += 1;
            manifest.push(ManifestEntry {
                name: &asset.name,
                mask: &asset.mask,
                dictionary: &src.path,
                platform: r.platform.label(),
                format: r.format_label(),
                width: r.width,
                height: r.height,
                mip_levels: r.levels.len(),
                renderware: if tex.rw_version == 0 { String::new() } else { crate::rw::version_label(tex.rw_version) },
                support: asset.support.label(),
                files,
            });
        }
    }
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "game": install.game.name(),
        "platform": install.platform.name(),
        "edition": install.edition,
        "user_choice": install.user_choice,
        "source": install.vfs.describe(),
        "tool": format!("Eyefinder {}", env!("CARGO_PKG_VERSION")),
        "detection": install.evidence,
        "catalog_notices": catalog.notices,
        "textures": manifest,
        "skipped": report.skipped,
    }))
    .map_err(|e| Error::Io(std::io::Error::other(e)))?;
    fs::write(out.join("manifest.json"), json)?;
    let mut csv = BufWriter::new(fs::File::create(out.join("manifest.csv"))?);
    writeln!(csv, "name,dictionary,platform,format,width,height,mip_levels,renderware,support,files")?;
    let esc = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for m in &manifest {
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{}",
            esc(m.name),
            esc(m.dictionary),
            esc(m.platform),
            esc(&m.format),
            m.width,
            m.height,
            m.mip_levels,
            m.renderware,
            esc(m.support),
            esc(&m.files.join(";"))
        )?;
    }
    csv.flush()?;
    Ok(report)
}
