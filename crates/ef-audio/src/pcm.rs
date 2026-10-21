//! Decoded audio in its exact output representation, plus conversion for playback.

use crate::Result;

/// Sample encoding of a PCM stream, in WAV terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    /// Unsigned 8-bit (WAV convention).
    U8,
    I16,
    I24,
    I32,
    F32,
}

impl SampleKind {
    pub fn bytes(self) -> usize {
        match self {
            SampleKind::U8 => 1,
            SampleKind::I16 => 2,
            SampleKind::I24 => 3,
            SampleKind::I32 | SampleKind::F32 => 4,
        }
    }
    pub fn describe(self) -> &'static str {
        match self {
            SampleKind::U8 => "PCM 8-bit",
            SampleKind::I16 => "PCM 16-bit",
            SampleKind::I24 => "PCM 24-bit",
            SampleKind::I32 => "PCM 32-bit",
            SampleKind::F32 => "IEEE float 32-bit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmFormat {
    pub kind: SampleKind,
    pub channels: u16,
    pub rate: u32,
    /// Verbatim `fmt ` chunk body to write when the source was PCM and is copied bit-exact
    /// (keeps WAVE_FORMAT_EXTENSIBLE channel masks etc.). `None` writes a canonical header.
    pub fmt_chunk: Option<Vec<u8>>,
}

impl PcmFormat {
    pub fn new(kind: SampleKind, channels: u16, rate: u32) -> Self {
        Self { kind, channels, rate, fmt_chunk: None }
    }
    pub fn frame_bytes(&self) -> usize {
        self.kind.bytes() * self.channels as usize
    }
}

/// Facts about the source encoding, recorded in the manifest.
#[derive(Debug, Clone, Default)]
pub struct SourceInfo {
    pub codec: String,
    pub rate: u32,
    pub channels: u16,
    pub bitrate_kbps: Option<u32>,
    pub notes: Vec<String>,
}

/// A stream of decoded frames.
pub trait PcmSource: Send {
    fn format(&self) -> &PcmFormat;
    fn info(&self) -> &SourceInfo;
    /// Total frames if known in advance.
    fn total_frames(&self) -> Option<u64>;
    /// Appends decoded frames (in `format()` encoding) to `out`; returns the number of frames
    /// appended. `Ok(0)` means end of stream.
    fn read(&mut self, out: &mut Vec<u8>) -> Result<usize>;
    /// Repositions so the next `read` starts at `frame` (clamped to the end).
    fn seek(&mut self, frame: u64) -> Result<()>;
}

/// Converts interleaved samples in `kind` to f32 in [-1, 1].
pub fn to_f32(kind: SampleKind, bytes: &[u8], out: &mut Vec<f32>) {
    match kind {
        SampleKind::U8 => out.extend(bytes.iter().map(|&b| (b as f32 - 128.0) / 128.0)),
        SampleKind::I16 => {
            out.extend(bytes.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0))
        }
        SampleKind::I24 => {
            out.extend(
                bytes.as_chunks::<3>().0.iter().map(|c| {
                    (((c[0] as i32) << 8 | (c[1] as i32) << 16 | (c[2] as i32) << 24) >> 8) as f32 / 8_388_608.0
                }),
            )
        }
        SampleKind::I32 => out.extend(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0),
        ),
        SampleKind::F32 => {
            out.extend(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])))
        }
    }
}

/// Converts f32 samples to 16-bit with rounding and clipping (same rule as the C ripper).
pub fn f32_to_i16(v: f32) -> i16 {
    let s = v * 32768.0;
    let s = if s >= 0.0 { s + 0.5 } else { s - 0.5 } as i32;
    s.clamp(-32768, 32767) as i16
}

pub fn push_i16(out: &mut Vec<u8>, s: i16) {
    out.extend_from_slice(&s.to_le_bytes());
}
