//! Exporting assets to WAV files with manifests.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ef_core::GameInstall;
use serde_json::{Map, Value, json};

use crate::catalog::{AssetKind, AudioAsset, sanitize};
use crate::open::{self, DecodeOptions, Opened};
use crate::pcm::{PcmFormat, PcmSource, SourceInfo};
use crate::wav::{ChunkBuf, Markers, WavWriter};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub out_dir: PathBuf,
    pub decode: DecodeOptions,
    /// Also write one file per cue-marker segment when a source carries markers.
    pub split_markers: bool,
    /// Save the untouched audio bitstream demuxed from movies.
    pub keep_movie_bitstream: bool,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

#[derive(Debug, Default)]
pub struct ExportReport {
    pub written: usize,
    pub failed: Vec<(String, String)>,
    pub cancelled: bool,
    pub out_dir: PathBuf,
}

fn fmt_time(sec: f64) -> String {
    let ms = (sec * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
}

/// Result of writing one asset.
struct Written {
    path: PathBuf,
    format: PcmFormat,
    frames: u64,
    info: SourceInfo,
    tracks: usize,
    extra_files: Vec<PathBuf>,
}

/// Text written into each file's LIST/INFO chunk.
struct Tags<'a> {
    title: &'a str,
    product: &'a str,
    comment: &'a str,
}

/// Writes the decoded stream to `path`, plus one file per marker segment into `track_dir`.
fn write_stream(
    src: &mut dyn PcmSource,
    path: &Path,
    tags: &Tags,
    markers: &Markers,
    track_dir: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<(u64, PcmFormat, usize)> {
    let Tags { title, product, comment } = *tags;
    let format = src.format().clone();
    let fb = format.frame_bytes();
    let mut full = WavWriter::create(path, &format)?;
    let split = track_dir.filter(|_| !markers.is_empty());
    if let Some(d) = split {
        fs::create_dir_all(d).map_err(|e| Error::io_at(d, e))?;
    }
    // segment i spans [start(i), start(i+1)); segment 0 starts at frame 0, segment k>0 at marker k-1
    let seg_end = |i: usize| markers.points.get(i).map_or(u64::MAX, |m| m.frame as u64);
    let mut seg: Option<(usize, WavWriter)> = None;
    let mut tracks = 0usize;
    let mut buf = Vec::with_capacity(1 << 20);
    let mut pos = 0u64;
    let close = |s: Option<(usize, WavWriter)>, tracks: &mut usize| -> Result<()> {
        if let Some((i, w)) = s {
            let mut t = ChunkBuf::default();
            t.info(&format!("{title} - segment {i}"), product, comment);
            w.finish(&t.0)?;
            *tracks += 1;
        }
        Ok(())
    };
    let result: Result<()> = (|| {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            buf.clear();
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            full.write(&buf)?;
            if let Some(d) = split {
                let mut off = 0usize;
                let mut left = n as u64;
                while left > 0 {
                    let mut i = 0;
                    while i < markers.points.len() && pos >= seg_end(i) {
                        i += 1;
                    }
                    if seg.as_ref().is_none_or(|(k, _)| *k != i) {
                        close(seg.take(), &mut tracks)?;
                        let label = if i == 0 {
                            "before first marker".to_string()
                        } else {
                            let m = &markers.points[i - 1];
                            if m.label.is_empty() { format!("marker {}", m.id) } else { m.label.clone() }
                        };
                        let p = d.join(format!("{i:03} - {}.wav", sanitize(&label)));
                        seg = Some((i, WavWriter::create(&p, &format)?));
                    }
                    let take = (seg_end(i) - pos).min(left);
                    let bytes = take as usize * fb;
                    seg.as_mut().unwrap().1.write(&buf[off..off + bytes])?;
                    off += bytes;
                    left -= take;
                    pos += take;
                }
            } else {
                pos += n as u64;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            close(seg.take(), &mut tracks)?;
            let mut t = ChunkBuf::default();
            t.markers(markers);
            t.info(title, product, comment);
            let frames = full.frames();
            full.finish(&t.0)?;
            Ok((frames, format, tracks))
        }
        Err(e) => {
            if let Some((_, w)) = seg {
                w.abort();
            }
            full.abort();
            Err(e)
        }
    }
}

fn write_marker_sidecars(path: &Path, markers: &Markers, rate: u32, total: u64) -> Result<Vec<PathBuf>> {
    if markers.is_empty() || rate == 0 {
        return Ok(Vec::new());
    }
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let labels = PathBuf::from(format!("{}.labels.txt", path.display()));
    let mut f = fs::File::create(&labels)?;
    for (i, m) in markers.points.iter().enumerate() {
        let a = m.frame as f64 / rate as f64;
        let b = if m.length > 0 {
            (m.frame + m.length) as f64 / rate as f64
        } else {
            markers.points.get(i + 1).map_or(total, |n| n.frame as u64) as f64 / rate as f64
        };
        writeln!(f, "{a:.6}\t{b:.6}\t{}", if m.label.is_empty() { "marker" } else { &m.label })?;
    }
    let cue = PathBuf::from(format!("{}.cue", path.display()));
    let mut f = fs::File::create(&cue)?;
    writeln!(f, "FILE \"{name}\" WAVE")?;
    for (i, m) in markers.points.iter().take(99).enumerate() {
        // CUE sheet INDEX is mm:ss:ff with 75 frames per second
        let ff = m.frame as u64 * 75 / rate as u64;
        writeln!(f, "  TRACK {:02} AUDIO", i + 1)?;
        writeln!(f, "    TITLE \"{}\"", if m.label.is_empty() { "marker" } else { &m.label })?;
        writeln!(f, "    INDEX 01 {:02}:{:02}:{:02}", ff / 75 / 60, ff / 75 % 60, ff % 75)?;
    }
    Ok(vec![labels, cue])
}

pub fn output_dir_for(asset: &AudioAsset, root: &Path) -> PathBuf {
    let mut d = root.join(asset.category.label());
    if let Some(g) = &asset.group {
        d.push(sanitize(g));
    }
    d
}

fn export_one(
    install: &GameInstall,
    asset: &AudioAsset,
    opts: &ExportOptions,
    product: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<Written>, Map<String, Value>, Markers)> {
    let vfs = install.vfs.as_ref();
    let dir = output_dir_for(asset, &opts.out_dir);
    fs::create_dir_all(&dir).map_err(|e| Error::io_at(&dir, e))?;
    let Opened { mut source, markers, extra, bitstream, more_streams } = open::open(asset, vfs, opts.decode)?;
    let path = dir.join(format!("{}.wav", asset.file_stem));
    let track_dir =
        (opts.split_markers && !markers.is_empty()).then(|| dir.join(format!("{} - Tracks", asset.file_stem)));
    let comment = format!("{} ({})", asset.game_id, asset.source);
    let tags = Tags { title: &asset.title, product, comment: &comment };
    let (frames, format, tracks) = write_stream(source.as_mut(), &path, &tags, &markers, track_dir.as_deref(), cancel)?;
    let mut extra_files = write_marker_sidecars(&path, &markers, format.rate, frames)?;
    let mut extra = extra;
    if let Some((ext, data)) = bitstream.filter(|_| opts.keep_movie_bitstream) {
        let p = dir.join(format!("{}.original.{ext}", asset.file_stem));
        fs::write(&p, data).map_err(|e| Error::io_at(&p, e))?;
        extra.insert("original_bitstream".into(), json!(p.file_name().unwrap().to_string_lossy()));
        extra_files.push(p);
    }
    if let (AssetKind::SaTrack { .. }, Some(h)) = (&asset.kind, extra.remove("track_header")) {
        let p = dir.join(format!("{}.trackinfo.json", asset.file_stem));
        fs::write(&p, serde_json::to_vec_pretty(&h)?).map_err(|e| Error::io_at(&p, e))?;
        extra.insert("track_header_file".into(), json!(p.file_name().unwrap().to_string_lossy()));
        extra_files.push(p);
    }
    let mut written = vec![Written { path, format, frames, info: source.info().clone(), tracks, extra_files }];
    for es in more_streams {
        let mut s = open::open_es(&es, opts.decode)?;
        let p = dir.join(format!("{} (audio stream {:02X}).wav", asset.file_stem, es.stream_id));
        let (frames, format, _) = write_stream(s.as_mut(), &p, &tags, &Markers::default(), None, cancel)?;
        written.push(Written { path: p, format, frames, info: s.info().clone(), tracks: 0, extra_files: Vec::new() });
    }
    Ok((written, extra, markers))
}

fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

pub fn export(
    install: &GameInstall,
    assets: &[&AudioAsset],
    opts: &ExportOptions,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<ExportReport> {
    fs::create_dir_all(&opts.out_dir).map_err(|e| Error::io_at(&opts.out_dir, e))?;
    let product = format!("{} ({})", install.game.name(), install.platform.name());
    let mut report = ExportReport { out_dir: opts.out_dir.clone(), ..Default::default() };
    let mut entries = Vec::new();
    let mut csv = String::from(
        "category,title,game_id,source_file,source_codec,source_rate,source_channels,output_file,output_codec,output_rate,output_channels,sample_frames,duration_seconds,duration,cue_markers\n",
    );
    let mut txt = format!(
        "{product} audio exported by Eyefinder {}\nDurations are exact (sample frames / sample rate).\n\n",
        env!("CARGO_PKG_VERSION")
    );
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for (i, a) in assets.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        progress(Progress { done: i, total: assets.len(), current: a.title.clone() });
        match export_one(install, a, opts, &product, cancel) {
            Ok((written, extra, markers)) => {
                for w in written {
                    let dur = if w.format.rate > 0 { w.frames as f64 / w.format.rate as f64 } else { 0.0 };
                    let marker_json: Vec<Value> = markers
                        .points
                        .iter()
                        .map(|m| {
                            json!({"id": m.id, "sample_frame": m.frame, "time": fmt_time(m.frame as f64 / w.format.rate.max(1) as f64),
                                   "label": m.label, "note": m.note, "length_frames": m.length})
                        })
                        .collect();
                    let mut e = json!({
                        "category": a.category.label(),
                        "group": a.group,
                        "title": a.title,
                        "game_id": a.game_id,
                        "support_level": a.support.label(),
                        "source": {"file": a.source, "bytes": a.bytes, "codec": w.info.codec, "sample_rate": w.info.rate,
                                   "channels": w.info.channels, "notes": w.info.notes},
                        "output": {"file": rel(&w.path, &opts.out_dir), "codec": w.format.kind.describe(), "sample_rate": w.format.rate,
                                   "channels": w.format.channels, "sample_frames": w.frames, "duration_seconds": dur, "duration": fmt_time(dur)},
                        "markers": marker_json,
                        "smpl_loops": markers.loops.iter().map(|(s, e)| json!({"start_frame": s, "end_frame": e})).collect::<Vec<_>>(),
                        "track_files": w.tracks,
                        "extra_files": w.extra_files.iter().map(|p| rel(p, &opts.out_dir)).collect::<Vec<_>>(),
                    });
                    e.as_object_mut().unwrap().insert("details".into(), Value::Object(extra.clone()));
                    entries.push(e);
                    csv += &format!(
                        "{},{},{},{},{},{},{},{},{},{},{},{},{:.6},{},{}\n",
                        q(a.category.label()),
                        q(&a.title),
                        q(&a.game_id),
                        q(&a.source),
                        q(&w.info.codec),
                        w.info.rate,
                        w.info.channels,
                        q(&rel(&w.path, &opts.out_dir)),
                        q(w.format.kind.describe()),
                        w.format.rate,
                        w.format.channels,
                        w.frames,
                        dur,
                        fmt_time(dur),
                        markers.points.len()
                    );
                    txt += &format!(
                        "{:<24} {}  {:<44} <- {} [{}, {} Hz, {} ch]\n",
                        a.category.label(),
                        fmt_time(dur),
                        a.title,
                        a.source,
                        w.info.codec,
                        w.info.rate,
                        w.info.channels
                    );
                    report.written += 1;
                }
            }
            Err(Error::Cancelled) => {
                report.cancelled = true;
                break;
            }
            Err(e) => report.failed.push((a.title.clone(), e.to_string())),
        }
    }
    progress(Progress { done: assets.len(), total: assets.len(), current: String::new() });
    let manifest = json!({
        "tool": format!("Eyefinder {}", env!("CARGO_PKG_VERSION")),
        "game": install.game.name(),
        "platform": install.platform.name(),
        "source": install.vfs.describe(),
        "detection": install.evidence,
        "options": {"mpeg_vorbis_output": if opts.decode.float_output { "IEEE float 32-bit" } else { "PCM 16-bit" }, "split_markers": opts.split_markers},
        "files_written": report.written,
        "failures": report.failed.iter().map(|(t, e)| json!({"title": t, "error": e})).collect::<Vec<_>>(),
        "cancelled": report.cancelled,
        "entries": entries,
    });
    let w = |name: &str, data: &[u8]| -> Result<()> {
        let p = opts.out_dir.join(name);
        fs::write(&p, data).map_err(|e| Error::io_at(&p, e))
    };
    w("manifest.json", &serde_json::to_vec_pretty(&manifest)?)?;
    w("manifest.csv", csv.as_bytes())?;
    w("tracklist.txt", txt.as_bytes())?;
    Ok(report)
}
