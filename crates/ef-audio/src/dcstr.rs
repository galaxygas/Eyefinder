//! Sega Dreamcast `.STR` streams ("Audio64 AM (AICA Manager) driver"), which GTA2 for Dreamcast
//! uses. Layout and decoding: vgmstream `src/meta/str_sega.c` and `src/coding/yamaha_decoder.c`,
//! `src/base/decode.c` (commit f7f76415):
//!
//! * header (little-endian): `0x00` track count (1-16), `0x04` sample rate, `0x08` bits per sample,
//!   `0x0C` interleave (a multiple of 0x800), `0x10` interleave blocks, `0x14` data size,
//!   `0x18` channels per track; `"\0Sega St"` at `0xD4`; audio from `0x800`;
//! * the file holds exactly `blocks × channels × interleave` bytes after the header;
//! * 4 bits per sample (vgmstream: "common [GTA2 (DC)]") is Yamaha AICA ADPCM, mono per channel,
//!   low nibble first, channels interleaved in `interleave`-byte blocks, step size starting at
//!   0x7F; `data_size × 2 / channels` samples per channel.
//!
//! AICA expansion per nibble (vgmstream `yamaha_aica_expand_nibble`): `hist = hist·254/256`;
//! `delta = ((code&7)·2+1)·step >> 3`, at most 32767, negated if `code&8`; `sample =
//! clamp16(hist + delta)`; `step = step·S[code] >> 8` clamped to 0x7F..0x6000, with
//! `S = 230, 230, 230, 230, 307, 409, 512, 614` (repeated for codes 8-15).
//!
//! Only single-track 4-bit streams are decoded. Which files of the GTA2 disc are such streams is
//! not documented; every `.STR` file with the header signature is offered.
use std::io::{Read, Seek, SeekFrom};

use crate::pcm::{PcmFormat, PcmSource, SampleKind, SourceInfo};
use crate::stream::ByteStream;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrHeader {
    pub tracks: u32,
    pub rate: u32,
    pub bits: u32,
    pub interleave: u64,
    pub blocks: u64,
    pub data_size: u64,
    pub channels: u32,
}

pub const DATA_START: u64 = 0x800;

/// Reads and validates the header; `file_len` is the whole file's length.
pub fn header(head: &[u8], file_len: u64) -> Result<StrHeader> {
    let u = |o: usize| head.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let bad = |m: &str| Error::Format(format!("Dreamcast STR: {m}"));
    if head.get(0xD4..0xDC) != Some(b"\0Sega St") {
        return Err(bad("no \"Sega Stream\" signature at 0xD4"));
    }
    let h = StrHeader {
        tracks: u(0).ok_or(bad("short header"))?,
        rate: u(4).ok_or(bad("short header"))?,
        bits: u(8).ok_or(bad("short header"))?,
        interleave: u(0x0C).ok_or(bad("short header"))? as u64,
        blocks: u(0x10).ok_or(bad("short header"))? as u64,
        data_size: u(0x14).ok_or(bad("short header"))? as u64,
        channels: u(0x18).ok_or(bad("short header"))?,
    };
    if !(1..=16).contains(&h.tracks) {
        return Err(bad("track count outside 1-16"));
    }
    let all = h.channels as u64 * h.tracks as u64;
    if h.rate == 0
        || h.channels == 0
        || h.interleave == 0
        || h.blocks * all * h.interleave != file_len - DATA_START.min(file_len)
    {
        return Err(bad("block layout does not match the file size"));
    }
    Ok(h)
}

const SCALE: [u32; 8] = [230, 230, 230, 230, 307, 409, 512, 614];

#[derive(Clone, Copy)]
struct Aica {
    hist: i16,
    step: i32,
}

impl Aica {
    fn new() -> Self {
        Self { hist: 0, step: 0x7F }
    }
    fn nibble(&mut self, code: u8) -> i16 {
        // C integer arithmetic: `*hist1 * 254 / 256` truncates toward zero.
        self.hist = (self.hist as i32 * 254 / 256) as i16;
        let mut delta = (((code & 7) as i32 * 2 + 1) * self.step) >> 3;
        delta = delta.min(32767);
        if code & 8 != 0 {
            delta = -delta;
        }
        let sample = (self.hist as i32 + delta).clamp(-32768, 32767) as i16;
        self.step = ((self.step as u32 * SCALE[(code & 7) as usize]) >> 8) as i32;
        self.step = self.step.clamp(0x7F, 0x6000);
        self.hist = sample;
        sample
    }
}

/// A single-track 4-bit Dreamcast STR stream.
pub struct DcStrSource {
    s: ByteStream,
    h: StrHeader,
    format: PcmFormat,
    info: SourceInfo,
    total: u64,
    block: u64,
    skip: u64,
    emitted: u64,
    states: Vec<Aica>,
    buf: Vec<u8>,
}

impl DcStrSource {
    pub fn new(mut s: ByteStream) -> Result<Self> {
        let mut head = vec![0u8; DATA_START as usize];
        s.seek(SeekFrom::Start(0))?;
        s.read_exact(&mut head).map_err(|_| Error::Format("Dreamcast STR: short header".into()))?;
        let h = header(&head, s.len())?;
        if h.tracks != 1 || h.bits != 4 {
            return Err(Error::Unsupported(format!(
                "Dreamcast STR with {} tracks and {} bits per sample: only single-track 4-bit AICA streams are decoded",
                h.tracks, h.bits
            )));
        }
        let channels = h.channels as u16;
        let total = h.data_size * 2 / h.channels as u64;
        let mut info = SourceInfo {
            codec: "Yamaha AICA ADPCM (Dreamcast STR)".into(),
            rate: h.rate,
            channels,
            ..Default::default()
        };
        info.notes.push("starts with the stream's lead-in silence, as vgmstream does".into());
        Ok(Self {
            buf: vec![0; (h.interleave * h.channels as u64) as usize],
            s,
            format: PcmFormat::new(SampleKind::I16, channels, h.rate),
            info,
            total,
            block: 0,
            skip: 0,
            emitted: 0,
            states: vec![Aica::new(); channels as usize],
            h,
        })
    }
}

impl PcmSource for DcStrSource {
    fn format(&self) -> &PcmFormat {
        &self.format
    }
    fn info(&self) -> &SourceInfo {
        &self.info
    }
    fn total_frames(&self) -> Option<u64> {
        Some(self.total)
    }
    fn read(&mut self, out: &mut Vec<u8>) -> Result<usize> {
        if self.block >= self.h.blocks || self.emitted >= self.total {
            return Ok(0);
        }
        let ch = self.h.channels as usize;
        let il = self.h.interleave as usize;
        self.s.seek(SeekFrom::Start(DATA_START + self.block * (il * ch) as u64))?;
        self.s.read_exact(&mut self.buf)?;
        let per = il * 2;
        let mut pcm = vec![0i16; per * ch];
        for c in 0..ch {
            for (i, &b) in self.buf[c * il..(c + 1) * il].iter().enumerate() {
                pcm[(2 * i) * ch + c] = self.states[c].nibble(b & 0x0F);
                pcm[(2 * i + 1) * ch + c] = self.states[c].nibble(b >> 4);
            }
        }
        self.block += 1;
        let frames = (per as u64).min(self.total - self.emitted);
        self.emitted += frames;
        let drop = self.skip.min(frames);
        self.skip -= drop;
        for &v in &pcm[drop as usize * ch..frames as usize * ch] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        Ok((frames - drop) as usize)
    }
    fn seek(&mut self, frame: u64) -> Result<()> {
        // ADPCM state carries across blocks, so decode from the start to stay exact.
        let frame = frame.min(self.total);
        self.block = 0;
        self.emitted = 0;
        self.states.iter_mut().for_each(|s| *s = Aica::new());
        self.skip = frame;
        let per = self.h.interleave * 2;
        let mut sink = Vec::new();
        while self.skip >= per && self.emitted < self.total {
            sink.clear();
            self.read(&mut sink)?;
        }
        Ok(())
    }
}
