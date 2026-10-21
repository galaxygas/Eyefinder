//! Audio demuxing from MPEG-1/MPEG-2 program streams (`movies\*.mpg` on PC).
//!
//! Pack headers (0xBA) are skipped by their fixed size (12 bytes MPEG-1, 14 + stuffing MPEG-2),
//! every other start code >= 0xBB carries a 16-bit length. Audio streams are ids 0xC0-0xDF;
//! their PES headers are parsed in both MPEG-1 and MPEG-2 syntax (ISO/IEC 11172-1, 13818-1).

use std::io::{Read, Seek, SeekFrom};

use crate::Result;

#[derive(Debug, Clone)]
pub struct AudioElementaryStream {
    pub stream_id: u8,
    /// Untouched audio bitstream (MPEG audio frames).
    pub data: Vec<u8>,
    /// First presentation time stamp, 90 kHz units.
    pub first_pts: Option<u64>,
}

fn read_pts(p: &[u8]) -> u64 {
    (((p[0] >> 1) & 7) as u64) << 30
        | (p[1] as u64) << 22
        | ((p[2] >> 1) as u64) << 15
        | (p[3] as u64) << 7
        | (p[4] >> 1) as u64
}

pub fn demux_audio<R: Read + Seek>(r: &mut R) -> Result<Vec<AudioElementaryStream>> {
    let size = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut data = Vec::with_capacity(size as usize);
    r.read_to_end(&mut data)?;
    let mut out: Vec<AudioElementaryStream> = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= data.len() {
        if data[pos] != 0 || data[pos + 1] != 0 || data[pos + 2] != 1 {
            pos += 1; // resync to the next start code
            continue;
        }
        let code = data[pos + 3];
        if code == 0xB9 {
            break; // program end
        }
        if code == 0xBA {
            match data.get(pos + 4) {
                Some(b) if b & 0xC0 == 0x40 => {
                    let stuffing = data.get(pos + 13).map_or(0, |b| (b & 7) as usize);
                    pos += 14 + stuffing;
                }
                Some(_) => pos += 12,
                None => break,
            }
            continue;
        }
        if code < 0xBB {
            pos += 1;
            continue;
        }
        let Some(lb) = data.get(pos + 4..pos + 6) else { break };
        let len = u16::from_be_bytes([lb[0], lb[1]]) as usize;
        let body_start = pos + 6;
        let body_end = (body_start + len).min(data.len());
        if (0xC0..=0xDF).contains(&code) && body_end > body_start {
            let b = &data[body_start..body_end];
            let mut i = 0usize;
            let mut pts = None;
            if b.len() >= 3 && b[0] & 0xC0 == 0x80 {
                // MPEG-2 PES header
                if b[1] & 0x80 != 0 && b.len() >= 8 {
                    pts = Some(read_pts(&b[3..8]));
                }
                i = 3 + b[2] as usize;
            } else {
                // MPEG-1 PES header
                while i < b.len() && b[i] == 0xFF {
                    i += 1;
                }
                if i < b.len() && b[i] & 0xC0 == 0x40 {
                    i += 2;
                }
                if i < b.len() && b[i] & 0xF0 == 0x20 {
                    if i + 5 <= b.len() {
                        pts = Some(read_pts(&b[i..i + 5]));
                    }
                    i += 5;
                } else if i < b.len() && b[i] & 0xF0 == 0x30 {
                    if i + 5 <= b.len() {
                        pts = Some(read_pts(&b[i..i + 5]));
                    }
                    i += 10;
                } else {
                    i += 1;
                }
            }
            let es = match out.iter_mut().position(|e| e.stream_id == code) {
                Some(k) => &mut out[k],
                None => {
                    out.push(AudioElementaryStream { stream_id: code, data: Vec::new(), first_pts: None });
                    out.last_mut().unwrap()
                }
            };
            if es.first_pts.is_none() {
                es.first_pts = pts;
            }
            if i < b.len() {
                es.data.extend_from_slice(&b[i..]);
            }
        }
        pos = body_start + len;
    }
    Ok(out)
}
