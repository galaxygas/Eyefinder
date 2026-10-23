//! CUE FILE/TRACK/INDEX semantics: libcdio lib/driver/image/bincue.c.
//! Sector layouts: psx-spx CDROM Format, Mode1 and Mode2/Form1 (FORMATS.md).
use crate::vfs::{IsoError, SectorLayout};
use std::path::{Path, PathBuf};

struct Track {
    number: u32,
    file: PathBuf,
    mode: String,
    start: Option<u64>,
    pregap: Option<u64>,
    /// Sectors of a `PREGAP` command: silence the sheet generates, not stored in the FILE.
    generated: u64,
}
fn bad(message: &str) -> IsoError {
    IsoError::Unsupported(format!("CUE: {message}"))
}
fn frames(s: &str) -> Result<u64, IsoError> {
    let v: Vec<u64> = s.split(':').map(str::parse).collect::<Result<_, _>>().map_err(|_| bad("invalid INDEX time"))?;
    if v.len() != 3 || v[0] > 999 || v[1] >= 60 || v[2] >= 75 {
        return Err(bad("invalid INDEX time"));
    }
    Ok((v[0] * 60 + v[1]) * 75 + v[2])
}
fn parse(cue: &Path) -> Result<Vec<Track>, IsoError> {
    let text = std::fs::read_to_string(cue)?;
    let mut file = None;
    let mut tracks: Vec<Track> = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("").to_ascii_uppercase();
        match key.as_str() {
            "FILE" => {
                let tail = line.get(4..).unwrap_or("").trim();
                let (name, kind) = if let Some(t) = tail.strip_prefix('"') {
                    let (name, rest) = t.split_once('"').ok_or_else(|| bad("unterminated FILE name"))?;
                    (name, rest.trim())
                } else {
                    tail.rsplit_once(char::is_whitespace).ok_or_else(|| bad("invalid FILE"))?
                };
                // Audio files may be WAV, but the selected data track must be BINARY.
                file = Some((
                    crate::vfs::sibling(cue.parent().unwrap_or(Path::new(".")), &name.replace('\\', "/")),
                    kind.to_ascii_uppercase(),
                ));
            }
            "TRACK" => {
                let number = parts
                    .next()
                    .ok_or_else(|| bad("missing track number"))?
                    .parse()
                    .map_err(|_| bad("invalid track number"))?;
                let mode = parts.next().ok_or_else(|| bad("missing track mode"))?.to_ascii_uppercase();
                let (path, kind) = file.as_ref().ok_or_else(|| bad("TRACK before FILE"))?;
                if mode != "AUDIO" && kind != "BINARY" {
                    return Err(bad("data FILE must be BINARY"));
                }
                tracks.push(Track { number, file: path.clone(), mode, start: None, pregap: None, generated: 0 });
            }
            "INDEX" => {
                let index = parts.next().ok_or_else(|| bad("missing INDEX number"))?;
                let value = frames(parts.next().ok_or_else(|| bad("missing INDEX time"))?)?;
                let t = tracks.last_mut().ok_or_else(|| bad("INDEX before TRACK"))?;
                match index {
                    "01" => t.start = Some(value),
                    "00" => t.pregap = Some(value),
                    _ => {}
                }
            }
            "PREGAP" => {
                let value = frames(parts.next().ok_or_else(|| bad("missing PREGAP time"))?)?;
                tracks.last_mut().ok_or_else(|| bad("PREGAP before TRACK"))?.generated = value;
            }
            _ => {} // metadata and flags do not occupy FILE bytes
        }
    }
    Ok(tracks)
}

pub fn data_track(cue: &Path) -> Result<(PathBuf, SectorLayout, u64), IsoError> {
    let tracks = parse(cue)?;
    let data: Vec<_> = tracks.iter().enumerate().filter(|(_, t)| t.mode != "AUDIO").collect();
    if data.len() != 1 {
        return Err(bad(
            "select an image with exactly one data track; audio-only and multiple-data-track images are not supported",
        ));
    }
    let (i, t) = data[0];
    let (stride, payload) = match t.mode.as_str() {
        "MODE1/2048" => (2048, 0),
        "MODE1/2352" => (2352, 16),
        "MODE2/2352" => (2352, 24),
        _ => return Err(bad("supported data modes: MODE1/2048, MODE1/2352, MODE2/2352")),
    };
    // Mixed physical sector sizes in one FILE cannot use simple frame offsets.
    if tracks.iter().any(|other| other.file == t.file && other.mode != t.mode && stride != 2352) {
        return Err(bad("mixed sector sizes in one FILE are unsupported"));
    }
    let start = t.start.ok_or_else(|| bad("data track has no INDEX 01"))? * stride;
    let size = std::fs::metadata(&t.file)?.len();
    let end = match tracks.get(i + 1).filter(|next| next.file == t.file) {
        Some(next) => next.pregap.or(next.start).ok_or_else(|| bad("next track has no index"))? * stride,
        None => size,
    };
    if end > size || start >= end || !(end - start).is_multiple_of(stride) {
        return Err(bad("data-track range is invalid or truncated"));
    }
    Ok((t.file.clone(), SectorLayout { stride, payload, track_offset: start }, (end - start) / stride * 2048))
}

/// A Red Book audio track of a CUE sheet: `bytes` bytes of 2352-byte sectors at `offset` in
/// `file`, from INDEX 01 to the next track's INDEX 00 (or 01) in the same FILE, or the end of the
/// FILE. The sector bytes are 16-bit stereo PCM at 44100 Hz, in the little-endian order bchunk
/// 1.2.0 copies unchanged into WAV files (it only swaps bytes on request, `-s`).
#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub number: u32,
    pub file: PathBuf,
    pub offset: u64,
    pub bytes: u64,
}

pub fn audio_tracks(cue: &Path) -> Result<Vec<AudioTrack>, IsoError> {
    let tracks = parse(cue)?;
    let mut out = Vec::new();
    for (i, t) in tracks.iter().enumerate().filter(|(_, t)| t.mode == "AUDIO") {
        let Some(start) = t.start else { continue };
        let Ok(meta) = std::fs::metadata(&t.file) else { continue };
        let end = match tracks.get(i + 1).filter(|next| next.file == t.file) {
            Some(next) => next.pregap.or(next.start).unwrap_or(0) * 2352,
            None => meta.len(),
        };
        let start = start * 2352;
        if end > start && end <= meta.len() {
            let bytes = (end - start) / 2352 * 2352;
            out.push(AudioTrack { number: t.number, file: t.file.clone(), offset: start, bytes });
        }
    }
    Ok(out)
}

/// A track as a disc dump stores it, for comparison with redump's track files: from its INDEX 00
/// (or INDEX 01) to the next track's in the same FILE, or the end of the FILE. A redump track file
/// holds the track's pregap, so `generated` (sectors of a `PREGAP` command, absent from the FILE)
/// is reported to let a dump without it be compared.
#[derive(Debug, Clone)]
pub struct DumpTrack {
    pub number: u32,
    pub audio: bool,
    pub file: PathBuf,
    pub offset: u64,
    pub bytes: u64,
    pub generated: u64,
}

pub fn dump_tracks(cue: &Path) -> Result<Vec<DumpTrack>, IsoError> {
    let tracks = parse(cue)?;
    let mut out = Vec::new();
    for (i, t) in tracks.iter().enumerate() {
        let stride = if t.mode == "MODE1/2048" { 2048 } else { 2352 };
        let start = t.pregap.or(t.start).ok_or_else(|| bad("track has no index"))? * stride;
        let len = std::fs::metadata(&t.file)?.len();
        let end = match tracks.get(i + 1).filter(|next| next.file == t.file) {
            Some(next) => next.pregap.or(next.start).ok_or_else(|| bad("next track has no index"))? * stride,
            None => len,
        };
        if end < start || end > len {
            return Err(bad("track range is invalid or truncated"));
        }
        out.push(DumpTrack {
            number: t.number,
            audio: t.mode == "AUDIO",
            file: t.file.clone(),
            offset: start,
            bytes: end - start,
            generated: t.generated,
        });
    }
    Ok(out)
}
