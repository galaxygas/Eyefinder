//! Concrete `PcmSource` implementations and content-based format detection.

use std::io::{Read, Seek, SeekFrom};

use crate::codec::{self, PsxState};
use crate::pcm::{PcmFormat, PcmSource, SampleKind, SourceInfo};
use crate::stream::ByteStream;
use crate::symph::{Container, SymphSource};
use crate::wav::{self, Markers, WavInfo};
use crate::{Error, Result};

const READ_FRAMES: usize = 16384;

/// Opens any of the stream formats the PC games use, detected from the contents:
/// RIFF/WAVE (PCM, float, extensible, IMA/Xbox/MS ADPCM, MPEG) or raw MPEG audio.
/// Returns the source and the markers found in a RIFF file.
pub fn open_stream(mut s: ByteStream, float_mpeg: bool) -> Result<(Box<dyn PcmSource>, Markers)> {
    if let Some(info) = wav::parse(&mut s)? {
        let markers = info.markers.clone();
        let src: Box<dyn PcmSource> = match info.effective_tag {
            wav::TAG_PCM | wav::TAG_FLOAT => Box::new(PcmCopySource::new(s, info)?),
            wav::TAG_IMA_ADPCM | wav::TAG_XBOX_ADPCM | wav::TAG_MS_ADPCM => Box::new(AdpcmSource::new(s, info)?),
            wav::TAG_MPEG | wav::TAG_MPEGLAYER3 => {
                let tag = info.effective_tag;
                let sub = s.sub(info.data_offset, info.data_len)?;
                let mut m = SymphSource::new(Box::new(sub), Container::MpegAudio, float_mpeg)?;
                m.info_mut().notes.push(format!("MPEG audio inside RIFF/WAVE (format tag 0x{tag:04X})"));
                Box::new(m)
            }
            t => return Err(Error::Unsupported(format!("WAV format tag 0x{t:04X}"))),
        };
        return Ok((src, markers));
    }
    s.seek(SeekFrom::Start(0))?;
    let mut head = vec![0u8; 4096];
    let n = read_up_to(&mut s, &mut head)?;
    head.truncate(n);
    s.seek(SeekFrom::Start(0))?;
    if looks_like_mpeg_audio(&head) {
        return Ok((Box::new(SymphSource::new(Box::new(s), Container::MpegAudio, float_mpeg)?), Markers::default()));
    }
    if head.starts_with(b"OggS") {
        return Ok((Box::new(SymphSource::new(Box::new(s), Container::Ogg, float_mpeg)?), Markers::default()));
    }
    Err(Error::Unsupported("not a RIFF/WAVE, MPEG audio or Ogg stream".into()))
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = r.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

/// ID3v2 tag, or an MPEG audio frame sync with a valid layer within the first 4 KB.
pub fn looks_like_mpeg_audio(p: &[u8]) -> bool {
    if p.starts_with(b"ID3") {
        return true;
    }
    p.windows(2).take(4096).any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0 && (w[1] >> 1) & 3 != 0)
}

// ---------------------------------------------------------------------------------------------

/// PCM / float data copied bit-exact, with the source's own `fmt ` chunk.
pub struct PcmCopySource {
    s: ByteStream,
    info_w: WavInfo,
    format: PcmFormat,
    info: SourceInfo,
    pos: u64,
    total: u64,
}

impl PcmCopySource {
    fn new(mut s: ByteStream, w: WavInfo) -> Result<Self> {
        let kind = match (w.effective_tag, w.bits) {
            (wav::TAG_PCM, 8) => SampleKind::U8,
            (wav::TAG_PCM, 16) => SampleKind::I16,
            (wav::TAG_PCM, 24) => SampleKind::I24,
            (wav::TAG_PCM, 32) => SampleKind::I32,
            (wav::TAG_FLOAT, 32) => SampleKind::F32,
            (t, b) => return Err(Error::Unsupported(format!("PCM variant tag 0x{t:04X} with {b} bits"))),
        };
        if w.channels == 0 || w.block_align as usize != kind.bytes() * w.channels as usize {
            return Err(Error::Format(format!(
                "inconsistent block align {} for {} channels",
                w.block_align, w.channels
            )));
        }
        let mut format = PcmFormat::new(kind, w.channels, w.rate);
        format.fmt_chunk = Some(w.fmt_raw.clone());
        let total = w.data_len / w.block_align as u64;
        let info = SourceInfo {
            codec: format!("{}{}", kind.describe(), if w.tag == wav::TAG_EXTENSIBLE { " (extensible)" } else { "" }),
            rate: w.rate,
            channels: w.channels,
            ..Default::default()
        };
        s.seek(SeekFrom::Start(w.data_offset))?;
        Ok(Self { s, info_w: w, format, info, pos: 0, total })
    }
}

impl PcmSource for PcmCopySource {
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
        let n = (self.total - self.pos).min(READ_FRAMES as u64) as usize;
        if n == 0 {
            return Ok(0);
        }
        let fb = self.format.frame_bytes();
        let start = out.len();
        out.resize(start + n * fb, 0);
        let got = read_up_to(&mut self.s, &mut out[start..])?;
        let frames = got / fb;
        out.truncate(start + frames * fb);
        self.pos += frames as u64;
        Ok(frames)
    }
    fn seek(&mut self, frame: u64) -> Result<()> {
        self.pos = frame.min(self.total);
        self.s.seek(SeekFrom::Start(self.info_w.data_offset + self.pos * self.format.frame_bytes() as u64))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------

/// IMA ADPCM, Xbox IMA ADPCM and Microsoft ADPCM in RIFF/WAVE, decoded to 16-bit PCM.
pub struct AdpcmSource {
    s: ByteStream,
    w: WavInfo,
    ms: bool,
    spb: u32,
    coefs: Vec<(i16, i16)>,
    format: PcmFormat,
    info: SourceInfo,
    /// next block to decode
    block: u64,
    blocks: u64,
    /// frames already produced
    produced: u64,
    limit: u64,
    skip: u64,
    blk: Vec<u8>,
    pcm: Vec<i16>,
}

impl AdpcmSource {
    fn new(s: ByteStream, w: WavInfo) -> Result<Self> {
        let ms = w.effective_tag == wav::TAG_MS_ADPCM;
        if w.block_align == 0 || w.channels == 0 || w.channels > if ms { 2 } else { 8 } {
            return Err(Error::Format(format!(
                "invalid ADPCM header: block align {}, {} channels",
                w.block_align, w.channels
            )));
        }
        let spb = if w.samples_per_block != 0 {
            w.samples_per_block as u32
        } else if ms {
            codec::ms_samples_per_block(w.block_align, w.channels)
        } else {
            codec::ima_samples_per_block(w.block_align, w.channels)
        };
        let coefs = if ms && w.ms_coefs.is_empty() { codec::MS_DEFAULT_COEFS.to_vec() } else { w.ms_coefs.clone() };
        let blocks = w.data_len.div_ceil(w.block_align as u64);
        // a full block holds spb frames; the last block may be short
        let max_frames = {
            let full = w.data_len / w.block_align as u64;
            let rem = (w.data_len % w.block_align as u64) as usize;
            let mut v = Vec::new();
            let tail = if rem > 0 {
                let fake = vec![0u8; rem];
                if ms {
                    codec::ms_block(&fake, w.channels as usize, spb, &coefs, &mut v)
                } else {
                    codec::ima_block(&fake, w.channels as usize, spb, &mut v)
                }
            } else {
                0
            };
            full * spb as u64 + tail as u64
        };
        let limit = w.fact_frames.map_or(max_frames, |f| (f as u64).min(max_frames));
        let name = match w.effective_tag {
            wav::TAG_MS_ADPCM => "Microsoft ADPCM",
            wav::TAG_XBOX_ADPCM => "Xbox IMA ADPCM",
            _ => "IMA ADPCM",
        };
        let mut info = SourceInfo {
            codec: format!("{name} (4-bit, {spb} samples/block)"),
            rate: w.rate,
            channels: w.channels,
            ..Default::default()
        };
        if let Some(f) = w.fact_frames {
            info.notes.push(format!("fact chunk frame count {f}"));
        }
        Ok(Self {
            format: PcmFormat::new(SampleKind::I16, w.channels, w.rate),
            info,
            s,
            ms,
            spb,
            coefs,
            block: 0,
            blocks,
            produced: 0,
            limit,
            skip: 0,
            blk: vec![0u8; w.block_align as usize],
            pcm: Vec::new(),
            w,
        })
    }
}

impl PcmSource for AdpcmSource {
    fn format(&self) -> &PcmFormat {
        &self.format
    }
    fn info(&self) -> &SourceInfo {
        &self.info
    }
    fn total_frames(&self) -> Option<u64> {
        Some(self.limit)
    }
    fn read(&mut self, out: &mut Vec<u8>) -> Result<usize> {
        let ch = self.w.channels as usize;
        let mut written = 0usize;
        while written < READ_FRAMES && self.block < self.blocks && self.produced < self.limit {
            let off = self.block * self.w.block_align as u64;
            let n = (self.w.data_len - off).min(self.w.block_align as u64) as usize;
            self.s.seek(SeekFrom::Start(self.w.data_offset + off))?;
            let got = read_up_to(&mut self.s, &mut self.blk[..n])?;
            self.block += 1;
            self.pcm.clear();
            let mut frames = if self.ms {
                codec::ms_block(&self.blk[..got], ch, self.spb, &self.coefs, &mut self.pcm)
            } else {
                codec::ima_block(&self.blk[..got], ch, self.spb, &mut self.pcm)
            } as u64;
            if frames == 0 {
                break;
            }
            frames = frames.min(self.limit - self.produced);
            let drop = self.skip.min(frames);
            self.skip -= drop;
            for &v in &self.pcm[drop as usize * ch..frames as usize * ch] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            self.produced += frames;
            written += (frames - drop) as usize;
        }
        Ok(written)
    }
    fn seek(&mut self, frame: u64) -> Result<()> {
        let frame = frame.min(self.limit);
        self.block = frame / self.spb as u64;
        self.produced = self.block * self.spb as u64;
        self.skip = frame - self.produced;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------

/// Headerless PS2 `.VB` stream: PS-ADPCM, each channel interleaved in 0x2000-byte blocks
/// (re3/reVC `CVbFile`, `VB_BLOCK_SIZE`). Sample rate and channel count come from the game's
/// tables, not from the file.
pub struct VbSource {
    s: ByteStream,
    channels: usize,
    format: PcmFormat,
    info: SourceInfo,
    blocks: u64,
    block: u64,
    skip: u64,
    buf: Vec<u8>,
    pcm: Vec<i16>,
    states: Vec<PsxState>,
}

pub const VB_BLOCK: usize = 0x2000;
const VB_FRAMES_PER_BLOCK: u64 = (VB_BLOCK / 16 * 28) as u64;

impl VbSource {
    pub fn new(s: ByteStream, rate: u32, channels: u16) -> Result<Self> {
        let ch = channels as usize;
        let blocks = s.len() / (VB_BLOCK * ch) as u64;
        let rem = s.len() % (VB_BLOCK * ch) as u64;
        let mut info = SourceInfo { codec: "PS-ADPCM (headerless .VB)".into(), rate, channels, ..Default::default() };
        if rem != 0 {
            info.notes.push(format!(
                "{rem} trailing bytes do not form a complete interleave block and are ignored, as in the game"
            ));
        }
        Ok(Self {
            s,
            channels: ch,
            format: PcmFormat::new(SampleKind::I16, channels, rate),
            info,
            blocks,
            block: 0,
            skip: 0,
            buf: vec![0u8; VB_BLOCK * ch],
            pcm: vec![0i16; VB_FRAMES_PER_BLOCK as usize * ch],
            states: vec![PsxState::default(); ch],
        })
    }
}

impl PcmSource for VbSource {
    fn format(&self) -> &PcmFormat {
        &self.format
    }
    fn info(&self) -> &SourceInfo {
        &self.info
    }
    fn total_frames(&self) -> Option<u64> {
        Some(self.blocks * VB_FRAMES_PER_BLOCK)
    }
    fn read(&mut self, out: &mut Vec<u8>) -> Result<usize> {
        if self.block >= self.blocks {
            return Ok(0);
        }
        // decoder state persists across blocks; it is rebuilt from the start on seek
        self.s.seek(SeekFrom::Start(self.block * (VB_BLOCK * self.channels) as u64))?;
        self.s.read_exact(&mut self.buf)?;
        for c in 0..self.channels {
            let data = &self.buf[c * VB_BLOCK..(c + 1) * VB_BLOCK];
            for (line, f) in data.chunks_exact(16).enumerate() {
                let frame: &[u8; 16] = f.try_into().unwrap();
                let base = line * 28 * self.channels + c;
                self.states[c].frame(frame, &mut self.pcm[base..], self.channels);
            }
        }
        self.block += 1;
        let drop = self.skip.min(VB_FRAMES_PER_BLOCK) as usize;
        self.skip -= drop as u64;
        for &v in &self.pcm[drop * self.channels..] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        Ok(VB_FRAMES_PER_BLOCK as usize - drop)
    }
    fn seek(&mut self, frame: u64) -> Result<()> {
        // PS-ADPCM history carries across blocks, so decode from the start to stay exact
        let frame = frame.min(self.blocks * VB_FRAMES_PER_BLOCK);
        self.block = 0;
        self.states.iter_mut().for_each(|s| *s = PsxState::default());
        self.skip = frame;
        let mut sink = Vec::new();
        while self.skip >= VB_FRAMES_PER_BLOCK && self.block < self.blocks {
            sink.clear();
            self.read(&mut sink)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------

/// Little-endian 16-bit mono PCM at a given rate (SFX bank samples).
pub struct RawPcmSource {
    s: ByteStream,
    format: PcmFormat,
    info: SourceInfo,
    pos: u64,
    total: u64,
}

impl RawPcmSource {
    pub fn new(s: ByteStream, rate: u32, codec: &str) -> Self {
        let total = s.len() / 2;
        Self {
            s,
            format: PcmFormat::new(SampleKind::I16, 1, rate),
            info: SourceInfo { codec: codec.into(), rate, channels: 1, ..Default::default() },
            pos: 0,
            total,
        }
    }
}

impl PcmSource for RawPcmSource {
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
        let n = (self.total - self.pos).min(READ_FRAMES as u64) as usize;
        if n == 0 {
            return Ok(0);
        }
        let start = out.len();
        out.resize(start + n * 2, 0);
        let got = read_up_to(&mut self.s, &mut out[start..])? / 2;
        out.truncate(start + got * 2);
        self.pos += got as u64;
        Ok(got)
    }
    fn seek(&mut self, frame: u64) -> Result<()> {
        self.pos = frame.min(self.total);
        self.s.seek(SeekFrom::Start(self.pos * 2))?;
        Ok(())
    }
}
