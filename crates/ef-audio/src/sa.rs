//! San Andreas (PC) audio containers, per gta-reversed `source/game_sa/Audio/Loaders`:
//!
//! * `AUDIO\CONFIG\STRMPAKS.DAT`: `StreamPack { char name[16]; }`
//! * `AUDIO\CONFIG\TRAKLKUP.DAT`: `tTrackLookup { u8 pack; (3 pad) u32 offset; u32 size; }`
//! * `AUDIO\STREAMS\<pack>`: XOR-obfuscated (see `stream::Xform::SaStreamKey`). Each track starts
//!   with `tTrackInfo` (0x1F84 bytes: 1000 x `{u32 time; u32 key}` beats, 16 x u32, u16), followed
//!   by the Ogg Vorbis stream.
//! * `AUDIO\CONFIG\PAKFILES.DAT`: `AEPakLookup { char name[12]; u32 lsn[10]; }` (52 bytes)
//! * `AUDIO\CONFIG\BANKLKUP.DAT`: `AEBankLookup { u8 pak; (3 pad) u32 offset; u32 bytes; }`
//! * Each bank in `AUDIO\SFX\<pak>`: `AEAudioStream { i16 count; i16 pad; 400 x
//!   CAEBankSlotItem { u32 offset; u32 loop_start; u16 rate; i16 headroom; } }` (0x12C4 bytes)
//!   followed by `bytes` bytes of 16-bit mono PCM; sound offsets are relative to that data.

use std::io::{Read, Seek, SeekFrom};

use ef_core::vfs::Vfs;
use serde_json::json;

use crate::stream::{ByteStream, Xform};
use crate::{Error, Result};

pub const TRACK_HEADER_LEN: u64 = 0x1F84;
pub const BANK_HEADER_LEN: u64 = 0x12C4;
pub const BANK_MAX_SOUNDS: usize = 400;

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

#[derive(Debug, Clone)]
pub struct SaTrack {
    pub index: usize,
    pub pack: String,
    pub offset: u32,
    pub size: u32,
}

/// Reads the stream pack and track lookup tables.
pub fn read_tracks(vfs: &dyn Vfs) -> Result<Vec<SaTrack>> {
    let packs: Vec<String> =
        vfs.read_all("AUDIO/CONFIG/STRMPAKS.DAT")?.as_chunks::<16>().0.iter().map(|e| cstr(e)).collect();
    let lk = vfs.read_all("AUDIO/CONFIG/TRAKLKUP.DAT")?;
    if lk.len() % 12 != 0 {
        return Err(Error::Format(format!("TRAKLKUP.DAT size {} is not a multiple of 12", lk.len())));
    }
    let mut out = Vec::with_capacity(lk.len() / 12);
    for (i, e) in lk.as_chunks::<12>().0.iter().enumerate() {
        let pack = e[0] as usize;
        let Some(name) = packs.get(pack) else {
            return Err(Error::Format(format!(
                "track {i} refers to stream pack {pack}, but STRMPAKS.DAT lists {}",
                packs.len()
            )));
        };
        out.push(SaTrack { index: i, pack: name.clone(), offset: u32le(e, 4), size: u32le(e, 8) });
    }
    Ok(out)
}

/// Opens a track: returns the Ogg Vorbis stream and the decoded track header as JSON.
pub fn open_track(vfs: &dyn Vfs, pack: &str, offset: u32, size: u32) -> Result<(ByteStream, serde_json::Value)> {
    let path = format!("AUDIO/STREAMS/{pack}");
    let mut s = ByteStream::open_range(vfs, &path, offset as u64, size as u64, Xform::SaStreamKey)?;
    if s.len() < TRACK_HEADER_LEN + 4 {
        return Err(Error::Format(format!(
            "track at {offset} in {pack} is shorter than its {TRACK_HEADER_LEN}-byte header"
        )));
    }
    let h = s.read_vec(TRACK_HEADER_LEN as usize)?;
    let beats: Vec<[u32; 2]> = (0..1000).map(|i| [u32le(&h, i * 8), u32le(&h, i * 8 + 4)]).collect();
    let words: Vec<u32> = (0..16).map(|i| u32le(&h, 0x1F40 + i * 4)).collect();
    let last = u16::from_le_bytes([h[0x1F80], h[0x1F81]]);
    let mut magic = [0u8; 4];
    s.read_exact(&mut magic)?;
    if &magic != b"OggS" {
        return Err(Error::Format(format!(
            "track at {offset} in {pack}: expected an Ogg stream after the {TRACK_HEADER_LEN}-byte header, found {:02X?}",
            magic
        )));
    }
    s.seek(SeekFrom::Start(0))?;
    let ogg = s.sub(TRACK_HEADER_LEN, size as u64 - TRACK_HEADER_LEN)?;
    // field names follow gta-reversed's tTrackInfo; the meaning of the values is not documented
    let header = json!({
        "beats": beats.iter().filter(|b| b[0] != 0 || b[1] != 0).map(|b| json!({"m_nTime": b[0], "m_nKey": b[1]})).collect::<Vec<_>>(),
        "beat_slots_nonzero": beats.iter().filter(|b| b[0] != 0 || b[1] != 0).count(),
        "field_1F40": words,
        "field_1F80": last,
    });
    Ok((ogg, header))
}

#[derive(Debug, Clone)]
pub struct SaSound {
    pub pak: String,
    pub bank: usize,
    pub sound: usize,
    /// byte range of the 16-bit mono PCM inside the pak file
    pub offset: u64,
    pub size: u64,
    pub rate: u16,
    pub loop_start: u32,
    pub headroom: i16,
}

/// Reads every sound of every SFX bank. Banks whose header is inconsistent are reported, not guessed.
pub fn read_sfx(vfs: &dyn Vfs) -> Result<(Vec<SaSound>, Vec<String>)> {
    let paks: Vec<String> =
        vfs.read_all("AUDIO/CONFIG/PAKFILES.DAT")?.as_chunks::<52>().0.iter().map(|e| cstr(&e[..12])).collect();
    let lk = vfs.read_all("AUDIO/CONFIG/BANKLKUP.DAT")?;
    let mut sounds = Vec::new();
    let mut problems = Vec::new();
    let mut open: Option<(usize, ByteStream)> = None;
    for (bank, e) in lk.as_chunks::<12>().0.iter().enumerate() {
        let pak = e[0] as usize;
        let (off, bytes) = (u32le(e, 4) as u64, u32le(e, 8) as u64);
        let Some(pak_name) = paks.get(pak) else {
            problems.push(format!("bank {bank}: pak index {pak} is not listed in PAKFILES.DAT"));
            continue;
        };
        if open.as_ref().is_none_or(|(p, _)| *p != pak) {
            match ByteStream::open(vfs, &format!("AUDIO/SFX/{pak_name}")) {
                Ok(s) => open = Some((pak, s)),
                Err(err) => {
                    problems.push(format!("bank {bank}: cannot open AUDIO/SFX/{pak_name}: {err}"));
                    open = None;
                    continue;
                }
            }
        }
        let s = &mut open.as_mut().unwrap().1;
        if off + BANK_HEADER_LEN + bytes > s.len() {
            problems.push(format!("bank {bank}: range {off}+{} exceeds AUDIO/SFX/{pak_name}", BANK_HEADER_LEN + bytes));
            continue;
        }
        s.seek(SeekFrom::Start(off))?;
        let h = s.read_vec(BANK_HEADER_LEN as usize)?;
        let count = i16::from_le_bytes([h[0], h[1]]);
        if count <= 0 || count as usize > BANK_MAX_SOUNDS {
            problems.push(format!("bank {bank} in {pak_name}: sound count {count} is outside 1..=400; skipped"));
            continue;
        }
        let item = |i: usize| {
            let o = 4 + i * 12;
            (
                u32le(&h, o) as u64,
                u32le(&h, o + 4),
                u16::from_le_bytes([h[o + 8], h[o + 9]]),
                i16::from_le_bytes([h[o + 10], h[o + 11]]),
            )
        };
        for i in 0..count as usize {
            let (start, loop_start, rate, headroom) = item(i);
            let end = if i + 1 < count as usize { item(i + 1).0 } else { bytes };
            if start >= end || end > bytes || rate == 0 {
                problems.push(format!(
                    "bank {bank} sound {i} in {pak_name}: invalid range {start}..{end} or rate {rate}; skipped"
                ));
                continue;
            }
            sounds.push(SaSound {
                pak: pak_name.clone(),
                bank,
                sound: i,
                offset: off + BANK_HEADER_LEN + start,
                size: (end - start) & !1,
                rate,
                loop_start,
                headroom,
            });
        }
    }
    Ok((sounds, problems))
}
