//! Dreamcast GD-ROM images described by a `.gdi` sheet.
//!
//! Layout facts from AltoRetrato gditools3 `gditools3.py` (commit 79030a47), `parse_gdi` and
//! `CdImage`: the first line is the track count; every further line is
//! `track lba type sector_size file offset`, where type 0 is audio and 4 is data. The high-density
//! area starts at LBA 45000 with track 3. Its ISO 9660 filesystem uses absolute disc sector
//! numbers, and later data tracks (the last track, after an audio track) continue it. Raw
//! 2352-byte sectors carry their 2048 bytes of user data at offset 16 (Mode 1, as for CD
//! images). File names may be quoted when they contain spaces.

use std::path::{Path, PathBuf};

use crate::vfs::{FileLocation, IsoError, SectorLayout};

fn bad(message: &str) -> IsoError {
    IsoError::Unsupported(format!("GDI: {message}"))
}

/// Splits a sheet line into fields, keeping a quoted file name together.
fn fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        if let Some(q) = rest.strip_prefix('"') {
            let (name, tail) = q.split_once('"').unwrap_or((q, ""));
            out.push(name.to_string());
            rest = tail.trim_start();
        } else {
            let (word, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            out.push(word.to_string());
            rest = tail.trim_start();
        }
    }
    out
}

pub struct GdiTrack {
    pub number: u32,
    pub lba: u64,
    pub data: bool,
    pub sector_size: u64,
    pub file: PathBuf,
}

/// Every track of the sheet.
pub fn tracks(gdi: &Path) -> Result<Vec<GdiTrack>, IsoError> {
    let text = std::fs::read_to_string(gdi)?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let count: usize = lines.next().and_then(|l| l.parse().ok()).ok_or_else(|| bad("missing track count"))?;
    let dir = gdi.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for line in lines {
        let f = fields(line);
        if f.len() < 5 {
            return Err(bad("a track line needs: track lba type sector_size file"));
        }
        let num = |i: usize| f[i].parse::<u64>().map_err(|_| bad("invalid number"));
        let kind = num(2)?;
        out.push(GdiTrack {
            number: num(0)? as u32,
            lba: num(1)?,
            data: match kind {
                4 => true,
                0 => false,
                _ => return Err(bad("track type must be 0 (audio) or 4 (data)")),
            },
            sector_size: num(3)?,
            file: crate::vfs::sibling(dir, &f[4]),
        });
    }
    if out.len() != count {
        return Err(bad("the track count does not match the track lines"));
    }
    Ok(out)
}

/// The high-density data tracks as `(first LBA, logical 2048-byte-sector byte range)`, track 3
/// (LBA 45000) first.
pub fn data_tracks(gdi: &Path) -> Result<Vec<(u64, FileLocation)>, IsoError> {
    let mut out = Vec::new();
    for t in tracks(gdi)?.into_iter().filter(|t| t.data && t.lba >= 45000) {
        let size = std::fs::metadata(&t.file)?.len();
        let (sectors, len) = match t.sector_size {
            2048 => (None, size),
            2352 => (Some(SectorLayout { stride: 2352, payload: 16, track_offset: 0 }), size / 2352 * 2048),
            _ => {
                return Err(bad(&format!(
                    "track {}: sector size {} is not supported (2048 or 2352)",
                    t.number, t.sector_size
                )));
            }
        };
        out.push((t.lba, FileLocation { host: t.file, offset: 0, len, sectors }));
    }
    if out.first().is_none_or(|t| t.0 != 45000) {
        return Err(bad("no data track starts at LBA 45000 (the high-density area)"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_fields_and_quoted_names() {
        assert_eq!(
            fields(r#"3 45000 4 2352 "Grand Theft Auto 2 (USA) (Track 3).bin" 0"#)[4],
            "Grand Theft Auto 2 (USA) (Track 3).bin"
        );
        assert_eq!(fields("1 0 4 2352 track01.bin 0"), ["1", "0", "4", "2352", "track01.bin", "0"]);
    }
}
