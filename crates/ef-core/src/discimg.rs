//! Disc image containers that wrap raw CD sectors: PS2 POPStarter `.VCD` and DiscJuggler `.CDI`.
//!
//! Both give the data track that holds the ISO 9660 filesystem (read by [`crate::vfs::IsoVfs`])
//! and, for CDI, the Red Book audio tracks.
//!
//! **POPStarter VCD**: krHACKen cue2pops v2.0 (2013-05-16), mirror makefu/cue2pops-linux
//! commit 3f2be6126bd50dfe6b998bc8926f88ce9139c19a, `cue2pops.c`:
//! * `HEADERSIZE` 0x100000 (line 20); the BIN follows unchanged as raw 2352-byte sectors
//!   (`SECTORSIZE`, line 19; lines 1088-1158). The input must be one `MODE2/2352` BIN (lines 659-737),
//!   so LBA *n* of the data track is at `0x100000 + n × 2352`, its user data 24 bytes in.
//! * descriptor bytes (lines 590-649): `[0] = 0x41`, `[2] = 0xA0`, `[8] = 0x20` (CD-XA001),
//!   `[12] = 0xA1`, `[22] = 0xA2`; the sector count is written twice as a little-endian u32 at
//!   1032 and 1036 (lines 1072-1073).
//! * Audio tracks are stored inline, but their header positions depend on the converter options
//!   (`gap++` / `gap--`, the pregap it may inject, lines 740-960), so they are not mapped back to
//!   the stream: a VCD gives its data track only.
//!
//! **DiscJuggler CDI**: DeXT's cdirip parser as kept in Flycast, commit
//! fa9d39a355ed06cbc27ea8d807f1cb6b8d766cf0, `core/deps/chdpsr/cdipsr.cpp` and
//! `core/imgread/cdi.cpp`; cross-checked with Redream ffb7302245ff40515cb9f0f0b0e233a4b39342d3
//! `src/guest/gdrom/cdi.c`:
//! * the file ends with `u32 version` (0x80000004 V2, 0x80000005 V3, 0x80000006 V3.5) and
//!   `u32 header_offset`; the descriptor starts at `file_len - header_offset` for V3.5 and at
//!   `header_offset` otherwise (`CDI_init`, `CDI_get_sessions`);
//! * `u16 sessions`, then per session `u16 tracks`, each track read by `CDI_read_track` (the skips
//!   below), and 12 bytes after a session (13 unless V2, `CDI_skip_next_session`);
//! * track data is stored back to back from offset 0, `total_length × sector_size` bytes per
//!   track; index 1 starts `pregap × sector_size` into it, at FAD `start_lba + pregap`, i.e. disc
//!   LBA `start_lba + pregap - 150` (`cdi.cpp`; Flycast reads ISO 9660 extents at LBA + 150);
//! * sector size codes 0 / 1 / 2 = 2048 / 2336 / 2352; mode 0 audio, 1 Mode 1, 2 Mode 2. User data
//!   is 16 bytes into a Mode 1/2352 sector, 24 into Mode 2/2352 and 8 into Mode 2/2336 (Redream
//!   `disc.c`).
//! * A Dreamcast disc keeps IP.BIN and the filesystem on the first data track of the last session
//!   (Redream `disc.c` uses session index 1 of a two-session image).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::cue::AudioTrack;
use crate::vfs::{FileLocation, IsoError, SectorLayout};

/// The tracks of an image that [`crate::vfs::IsoVfs`] needs.
pub struct DiscTracks {
    /// Disc LBA of the data track's first sector, and the track as 2048-byte logical sectors.
    pub data: (u64, FileLocation),
    pub audio: Vec<AudioTrack>,
    /// A short description of the container for the evidence list, e.g. "DiscJuggler CDI V3.5".
    pub container: String,
}

fn bad(kind: &str, message: &str) -> IsoError {
    IsoError::Unsupported(format!("{kind}: {message}"))
}

pub const VCD_HEADER: u64 = 0x100000;

/// Whether `head` (at least 1040 bytes) is a POPStarter VCD header.
pub fn is_vcd_header(head: &[u8]) -> bool {
    head.len() >= 1040
        && head[0] == 0x41
        && head[2] == 0xA0
        && head[8] == 0x20
        && head[12] == 0xA1
        && head[22] == 0xA2
        && head[1032..1036] == head[1036..1040]
}

pub fn vcd(path: &Path) -> Result<DiscTracks, IsoError> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut head = vec![0u8; 1040];
    f.read_exact(&mut head).map_err(|_| bad("VCD", "file is shorter than its header"))?;
    if !is_vcd_header(&head) || len < VCD_HEADER + 2352 {
        return Err(bad("VCD", "no POPStarter header (cue2pops layout) at the start of the file"));
    }
    let sectors = (len - VCD_HEADER) / 2352;
    let counted = u32::from_le_bytes(head[1032..1036].try_into().unwrap()) as u64;
    if counted < sectors {
        return Err(bad("VCD", "the header's sector count is smaller than the file"));
    }
    let data = FileLocation {
        host: path.to_path_buf(),
        offset: 0,
        len: sectors * 2048,
        sectors: Some(SectorLayout { stride: 2352, payload: 24, track_offset: VCD_HEADER }),
    };
    Ok(DiscTracks { data: (0, data), audio: Vec::new(), container: "POPStarter VCD (cue2pops layout)".into() })
}

const TRACK_START_MARK: [u8; 10] = [0, 0, 0x01, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];

#[derive(Debug, Clone)]
pub struct CdiTrack {
    pub session: usize,
    pub number: usize,
    pub mode: u32,
    pub sector_size: u64,
    pub pregap: u64,
    pub length: u64,
    pub start_lba: u64,
    /// Byte offset of index 1 in the file.
    pub data_offset: u64,
}

impl CdiTrack {
    /// Disc LBA of index 1.
    pub fn lba(&self) -> u64 {
        (self.start_lba + self.pregap).saturating_sub(150)
    }
}

struct Cursor<'a> {
    f: &'a mut File,
}

impl Cursor<'_> {
    fn skip(&mut self, n: i64) -> Result<(), IsoError> {
        self.f.seek(SeekFrom::Current(n))?;
        Ok(())
    }
    fn u8(&mut self) -> Result<u8, IsoError> {
        let mut b = [0; 1];
        self.f.read_exact(&mut b)?;
        Ok(b[0])
    }
    fn u16(&mut self) -> Result<u16, IsoError> {
        let mut b = [0; 2];
        self.f.read_exact(&mut b)?;
        Ok(u16::from_le_bytes(b))
    }
    fn u32(&mut self) -> Result<u32, IsoError> {
        let mut b = [0; 4];
        self.f.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn mark(&mut self) -> Result<bool, IsoError> {
        let mut b = [0; 10];
        self.f.read_exact(&mut b)?;
        Ok(b == TRACK_START_MARK)
    }
}

/// Every track of a DiscJuggler image, and its version label.
pub fn cdi_tracks(path: &Path) -> Result<(Vec<CdiTrack>, &'static str), IsoError> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len < 8 {
        return Err(bad("CDI", "file is too short"));
    }
    f.seek(SeekFrom::Start(len - 8))?;
    let mut c = Cursor { f: &mut f };
    let (version, header_offset) = (c.u32()?, c.u32()? as u64);
    let (label, v2) = match version {
        0x8000_0004 => ("DiscJuggler CDI V2", true),
        0x8000_0005 => ("DiscJuggler CDI V3", false),
        0x8000_0006 => ("DiscJuggler CDI V3.5", false),
        _ => return Err(bad("CDI", "no DiscJuggler version marker at the end of the file")),
    };
    let start = if version == 0x8000_0006 { len.checked_sub(header_offset) } else { Some(header_offset) };
    let start = start.filter(|s| header_offset != 0 && *s < len).ok_or_else(|| bad("CDI", "bad descriptor offset"))?;
    c.f.seek(SeekFrom::Start(start))?;
    let sessions = c.u16()? as usize;
    if sessions == 0 || sessions > 99 {
        return Err(bad("CDI", "invalid session count"));
    }
    let mut out = Vec::new();
    let mut position = 0u64;
    for session in 0..sessions {
        let tracks = c.u16()? as usize;
        if tracks > 99 {
            return Err(bad("CDI", "invalid track count"));
        }
        for number in 0..tracks {
            if c.u32()? != 0 {
                c.skip(8)?; // extra data (DJ 3.00.780 and up)
            }
            if !c.mark()? || !c.mark()? {
                return Err(bad("CDI", "track start mark not found"));
            }
            c.skip(4)?;
            let name_len = c.u8()? as i64;
            c.skip(name_len + 11 + 4 + 4)?;
            if c.u32()? == 0x8000_0000 {
                c.skip(8)?; // DJ4
            }
            c.skip(2)?;
            let pregap = c.u32()? as u64;
            let length = c.u32()? as u64;
            c.skip(6)?;
            let mode = c.u32()?;
            c.skip(12)?;
            let start_lba = c.u32()? as u64;
            let total = c.u32()? as u64;
            c.skip(16)?;
            let sector_size = match c.u32()? {
                0 => 2048,
                1 => 2336,
                2 => 2352,
                other => return Err(bad("CDI", &format!("sector size code {other} is not supported"))),
            };
            if mode > 2 {
                return Err(bad("CDI", "track mode is not audio, Mode 1 or Mode 2"));
            }
            if total < pregap + length {
                return Err(bad("CDI", "a track is shorter than its pregap and length (truncated image)"));
            }
            c.skip(29)?;
            if !v2 {
                c.skip(5)?;
                if c.u32()? == 0xFFFF_FFFF {
                    c.skip(78)?; // extra data (DJ 3.00.780 and up)
                }
            }
            let data_offset = position + pregap * sector_size;
            if data_offset + length * sector_size > len {
                return Err(bad("CDI", "a track extends past the end of the file"));
            }
            out.push(CdiTrack {
                session,
                number: number + 1,
                mode,
                sector_size,
                pregap,
                length,
                start_lba,
                data_offset,
            });
            position += total * sector_size;
        }
        c.skip(if v2 { 12 } else { 13 })?;
    }
    Ok((out, label))
}

pub fn cdi(path: &Path) -> Result<DiscTracks, IsoError> {
    let (tracks, label) = cdi_tracks(path)?;
    let last = tracks.iter().map(|t| t.session).max().unwrap_or(0);
    let data = tracks
        .iter()
        .filter(|t| t.mode != 0)
        .find(|t| t.session == last)
        .or_else(|| tracks.iter().find(|t| t.mode != 0))
        .ok_or_else(|| bad("CDI", "the image has no data track"))?;
    let (len, sectors) = match (data.sector_size, data.mode) {
        (2048, _) => (data.length * 2048, None),
        (2336, 2) => {
            (data.length * 2048, Some(SectorLayout { stride: 2336, payload: 8, track_offset: data.data_offset }))
        }
        (2352, 1) => {
            (data.length * 2048, Some(SectorLayout { stride: 2352, payload: 16, track_offset: data.data_offset }))
        }
        (2352, 2) => {
            (data.length * 2048, Some(SectorLayout { stride: 2352, payload: 24, track_offset: data.data_offset }))
        }
        (size, mode) => return Err(bad("CDI", &format!("a Mode {mode} data track with {size}-byte sectors"))),
    };
    let offset = if sectors.is_some() { 0 } else { data.data_offset };
    let location = FileLocation { host: PathBuf::from(path), offset, len, sectors };
    let audio = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.mode == 0 && t.sector_size == 2352)
        .map(|(i, t)| AudioTrack {
            number: (i + 1) as u32,
            file: path.to_path_buf(),
            offset: t.data_offset,
            bytes: t.length * 2352,
        })
        .collect();
    let sessions = last + 1;
    Ok(DiscTracks {
        data: (data.lba(), location),
        audio,
        container: format!("{label}, {sessions} session(s), data track at LBA {}", data.lba()),
    })
}
