//! CUE FILE/TRACK/INDEX semantics: libcdio lib/driver/image/bincue.c.
//! Sector layouts: psx-spx CDROM Format, Mode1 and Mode2/Form1 (FORMATS.md).
use crate::vfs::{IsoError, SectorLayout};
use std::path::{Path, PathBuf};

struct Track {
    file: PathBuf,
    mode: String,
    start: Option<u64>,
    pregap: Option<u64>,
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
pub fn data_track(cue: &Path) -> Result<(PathBuf, SectorLayout, u64), IsoError> {
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
                    cue.parent().unwrap_or(Path::new(".")).join(name.replace('\\', "/")),
                    kind.to_ascii_uppercase(),
                ));
            }
            "TRACK" => {
                let _number = parts.next().ok_or_else(|| bad("missing track number"))?;
                let mode = parts.next().ok_or_else(|| bad("missing track mode"))?.to_ascii_uppercase();
                let (path, kind) = file.as_ref().ok_or_else(|| bad("TRACK before FILE"))?;
                if mode != "AUDIO" && kind != "BINARY" {
                    return Err(bad("data FILE must be BINARY"));
                }
                tracks.push(Track { file: path.clone(), mode, start: None, pregap: None });
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
            _ => {} // metadata, flags and generated PREGAP do not occupy FILE bytes
        }
    }
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
