//! RIFF/WAVE parsing (format, data range, markers) and writing.

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::pcm::{PcmFormat, SampleKind};
use crate::{Error, Result};

pub const TAG_PCM: u16 = 0x0001;
pub const TAG_MS_ADPCM: u16 = 0x0002;
pub const TAG_FLOAT: u16 = 0x0003;
pub const TAG_IMA_ADPCM: u16 = 0x0011;
pub const TAG_MPEG: u16 = 0x0050;
pub const TAG_MPEGLAYER3: u16 = 0x0055;
pub const TAG_XBOX_ADPCM: u16 = 0x0069;
pub const TAG_EXTENSIBLE: u16 = 0xFFFE;

#[derive(Debug, Clone, Default)]
pub struct Marker {
    pub id: u32,
    /// `dwSampleOffset` of the cue point, in sample frames.
    pub frame: u32,
    /// `ltxt` sample length, 0 if none.
    pub length: u32,
    pub label: String,
    pub note: String,
}

#[derive(Debug, Clone, Default)]
pub struct Markers {
    /// Sorted by frame.
    pub points: Vec<Marker>,
    /// Verbatim `cue ` chunk body and `LIST`(`adtl`) body, re-embedded in outputs.
    pub cue_raw: Option<Vec<u8>>,
    pub adtl_raw: Option<Vec<u8>>,
    /// `smpl` loops (start, end) in frames.
    pub loops: Vec<(u32, u32)>,
}

impl Markers {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct WavInfo {
    pub tag: u16,
    /// Format tag after resolving WAVE_FORMAT_EXTENSIBLE's SubFormat.
    pub effective_tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub block_align: u16,
    pub bits: u16,
    pub samples_per_block: u16,
    pub fmt_raw: Vec<u8>,
    pub data_offset: u64,
    pub data_len: u64,
    pub fact_frames: Option<u32>,
    pub ms_coefs: Vec<(i16, i16)>,
    pub markers: Markers,
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Parses a RIFF/WAVE header. `Ok(None)` if the stream is not RIFF/WAVE.
pub fn parse<R: Read + Seek>(r: &mut R) -> Result<Option<WavInfo>> {
    let size = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut h = [0u8; 12];
    if r.read_exact(&mut h).is_err() || &h[0..4] != b"RIFF" || &h[8..12] != b"WAVE" {
        return Ok(None);
    }
    let mut info = WavInfo {
        tag: 0,
        effective_tag: 0,
        channels: 0,
        rate: 0,
        block_align: 0,
        bits: 0,
        samples_per_block: 0,
        fmt_raw: Vec::new(),
        data_offset: 0,
        data_len: 0,
        fact_frames: None,
        ms_coefs: Vec::new(),
        markers: Markers::default(),
    };
    let (mut have_fmt, mut have_data) = (false, false);
    let mut pos = 12u64;
    while pos + 8 <= size {
        r.seek(SeekFrom::Start(pos))?;
        let mut ch = [0u8; 8];
        if r.read_exact(&mut ch).is_err() {
            break;
        }
        let len = rd32(&ch, 4) as u64;
        let id = &ch[0..4];
        if id == b"data" {
            info.data_offset = pos + 8;
            // tolerate a size field larger than the file (truncated files)
            info.data_len = len.min(size - info.data_offset);
            have_data = true;
        } else if len <= 1 << 24 {
            let mut d = vec![0u8; len as usize];
            let got = r.read(&mut d)?;
            d.truncate(got);
            match id {
                b"fmt " if d.len() >= 16 => {
                    info.tag = rd16(&d, 0);
                    info.channels = rd16(&d, 2);
                    info.rate = rd32(&d, 4);
                    info.block_align = rd16(&d, 12);
                    info.bits = rd16(&d, 14);
                    info.effective_tag =
                        if info.tag == TAG_EXTENSIBLE && d.len() >= 26 { rd16(&d, 24) } else { info.tag };
                    if d.len() >= 20 && matches!(info.tag, TAG_IMA_ADPCM | TAG_XBOX_ADPCM | TAG_MS_ADPCM) {
                        info.samples_per_block = rd16(&d, 18);
                    }
                    if info.tag == TAG_MS_ADPCM && d.len() >= 22 {
                        let n = rd16(&d, 20) as usize;
                        info.ms_coefs = (0..n.min(64))
                            .take_while(|i| 22 + i * 4 + 4 <= d.len())
                            .map(|i| (rd16(&d, 22 + i * 4) as i16, rd16(&d, 24 + i * 4) as i16))
                            .collect();
                    }
                    info.fmt_raw = d;
                    have_fmt = true;
                }
                b"fact" if d.len() >= 4 => info.fact_frames = Some(rd32(&d, 0)),
                b"cue " => parse_cue(&mut info.markers, &d),
                b"LIST" if d.len() >= 4 && &d[0..4] == b"adtl" => info.markers.adtl_raw = Some(d),
                b"smpl" => parse_smpl(&mut info.markers, &d),
                _ => {}
            }
        }
        pos += 8 + len + (len & 1);
    }
    if let Some(adtl) = info.markers.adtl_raw.clone() {
        parse_adtl(&mut info.markers, &adtl);
    }
    info.markers.points.sort_by_key(|m| m.frame);
    if !(have_fmt && have_data) {
        return Err(Error::Format("RIFF/WAVE file without fmt or data chunk".into()));
    }
    Ok(Some(info))
}

fn parse_cue(m: &mut Markers, d: &[u8]) {
    if d.len() < 4 {
        return;
    }
    let n = (rd32(d, 0) as usize).min((d.len() - 4) / 24);
    m.points = (0..n)
        .map(|i| {
            let e = 4 + i * 24;
            Marker { id: rd32(d, e), frame: rd32(d, e + 20), ..Default::default() }
        })
        .collect();
    m.cue_raw = Some(d.to_vec());
}

fn parse_adtl(m: &mut Markers, d: &[u8]) {
    let mut pos = 4;
    while pos + 8 <= d.len() {
        let len = rd32(d, pos + 4) as usize;
        if pos + 8 + len > d.len() {
            break;
        }
        let body = &d[pos + 8..pos + 8 + len];
        if len >= 4 {
            let id = rd32(body, 0);
            if let Some(mk) = m.points.iter_mut().find(|p| p.id == id) {
                match &d[pos..pos + 4] {
                    b"labl" => mk.label = text(&body[4..]),
                    b"note" => mk.note = text(&body[4..]),
                    b"ltxt" if len >= 8 => {
                        mk.length = rd32(body, 4);
                        if len > 20 && mk.note.is_empty() {
                            mk.note = text(&body[20..]);
                        }
                    }
                    _ => {}
                }
            }
        }
        pos += 8 + len + (len & 1);
    }
}

fn parse_smpl(m: &mut Markers, d: &[u8]) {
    if d.len() < 36 {
        return;
    }
    let n = (rd32(d, 28) as usize).min((d.len() - 36) / 24);
    m.loops = (0..n).map(|i| (rd32(d, 36 + i * 24 + 8), rd32(d, 36 + i * 24 + 12))).collect();
}

// ---------------------------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------------------------

/// Streams a WAV file to `<path>.partial` and renames it into place on `finish`, so an interrupted
/// export never leaves a truncated file under the final name.
pub struct WavWriter {
    out: BufWriter<File>,
    tmp: PathBuf,
    path: PathBuf,
    format: PcmFormat,
    data_bytes: u64,
    fact_pos: Option<u64>,
    data_size_pos: u64,
}

impl WavWriter {
    pub fn create(path: &Path, format: &PcmFormat) -> Result<Self> {
        let tmp = path.with_extension("wav.partial");
        let f = File::create(&tmp).map_err(|e| Error::io_at(&tmp, e))?;
        let mut out = BufWriter::with_capacity(1 << 20, f);
        out.write_all(b"RIFF\0\0\0\0WAVE")?;
        let tag = match format.kind {
            SampleKind::F32 => TAG_FLOAT,
            _ => TAG_PCM,
        };
        match &format.fmt_chunk {
            Some(raw) => {
                out.write_all(b"fmt ")?;
                out.write_all(&(raw.len() as u32).to_le_bytes())?;
                out.write_all(raw)?;
                if raw.len() & 1 == 1 {
                    out.write_all(&[0])?;
                }
            }
            None => {
                let fb = format.frame_bytes() as u32;
                let mut f = Vec::with_capacity(18);
                f.extend(tag.to_le_bytes());
                f.extend(format.channels.to_le_bytes());
                f.extend(format.rate.to_le_bytes());
                f.extend((format.rate * fb).to_le_bytes());
                f.extend((fb as u16).to_le_bytes());
                f.extend(((format.kind.bytes() * 8) as u16).to_le_bytes());
                if tag != TAG_PCM {
                    f.extend(0u16.to_le_bytes()); // cbSize
                }
                out.write_all(b"fmt ")?;
                out.write_all(&(f.len() as u32).to_le_bytes())?;
                out.write_all(&f)?;
            }
        }
        let effective_tag = format.fmt_chunk.as_ref().map(|r| {
            let t = rd16(r, 0);
            if t == TAG_EXTENSIBLE && r.len() >= 26 { rd16(r, 24) } else { t }
        });
        let mut header_len = 12
            + 8
            + format.fmt_chunk.as_ref().map_or(if tag == TAG_PCM { 16 } else { 18 }, |r| r.len() + (r.len() & 1))
                as u64;
        // non-PCM WAV files carry a fact chunk with the frame count
        let fact_pos = if effective_tag.unwrap_or(tag) != TAG_PCM {
            out.write_all(b"fact\x04\0\0\0\0\0\0\0")?;
            header_len += 12;
            Some(header_len - 4)
        } else {
            None
        };
        out.write_all(b"data\0\0\0\0")?;
        let data_size_pos = header_len + 4;
        Ok(Self { out, tmp, path: path.to_path_buf(), format: format.clone(), data_bytes: 0, fact_pos, data_size_pos })
    }

    pub fn format(&self) -> &PcmFormat {
        &self.format
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if self.data_bytes + bytes.len() as u64 > 0xFFFF_FF00 {
            return Err(Error::Format("output exceeds the 4 GB WAV size limit".into()));
        }
        self.out.write_all(bytes).map_err(|e| Error::io_at(&self.tmp, e))?;
        self.data_bytes += bytes.len() as u64;
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.data_bytes / self.format.frame_bytes() as u64
    }

    /// Writes trailing chunks (already RIFF-encoded), patches sizes and renames into place.
    pub fn finish(mut self, trailing: &[u8]) -> Result<PathBuf> {
        if self.data_bytes & 1 == 1 {
            self.out.write_all(&[0])?;
        }
        self.out.write_all(trailing)?;
        let mut f = self.out.into_inner().map_err(|e| Error::io_at(&self.tmp, e.into_error()))?;
        let end = f.stream_position()?;
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&((end - 8) as u32).to_le_bytes())?;
        f.seek(SeekFrom::Start(self.data_size_pos))?;
        f.write_all(&(self.data_bytes as u32).to_le_bytes())?;
        if let Some(p) = self.fact_pos {
            f.seek(SeekFrom::Start(p))?;
            f.write_all(&((self.data_bytes / self.format.frame_bytes() as u64) as u32).to_le_bytes())?;
        }
        f.sync_all().ok();
        drop(f);
        std::fs::rename(&self.tmp, &self.path).map_err(|e| Error::io_at(&self.path, e))?;
        Ok(self.path)
    }

    /// Deletes the partial file.
    pub fn abort(self) {
        let tmp = self.tmp.clone();
        drop(self);
        std::fs::remove_file(tmp).ok();
    }
}

/// Builds RIFF sub-chunks for appending after the data chunk.
#[derive(Default)]
pub struct ChunkBuf(pub Vec<u8>);

impl ChunkBuf {
    pub fn chunk(&mut self, id: &[u8; 4], body: &[u8]) {
        self.0.extend_from_slice(id);
        self.0.extend_from_slice(&(body.len() as u32).to_le_bytes());
        self.0.extend_from_slice(body);
        if body.len() & 1 == 1 {
            self.0.push(0);
        }
    }

    /// `LIST`/`INFO` with title, product, comment and software.
    pub fn info(&mut self, title: &str, product: &str, comment: &str) {
        let mut l = ChunkBuf::default();
        l.0.extend_from_slice(b"INFO");
        let z = |s: &str| {
            let mut v = s.as_bytes().to_vec();
            v.push(0);
            v
        };
        l.chunk(b"INAM", &z(title));
        if !product.is_empty() {
            l.chunk(b"IPRD", &z(product));
        }
        if !comment.is_empty() {
            l.chunk(b"ICMT", &z(comment));
        }
        l.chunk(b"ISFT", &z(concat!("Eyefinder ", env!("CARGO_PKG_VERSION"))));
        self.chunk(b"LIST", &l.0);
    }

    pub fn markers(&mut self, m: &Markers) {
        if let Some(c) = &m.cue_raw {
            self.chunk(b"cue ", c);
        }
        if let Some(a) = &m.adtl_raw {
            self.chunk(b"LIST", a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_parse_roundtrip_with_markers() {
        let dir = std::env::temp_dir().join(format!("ef-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        let fmt = PcmFormat::new(SampleKind::F32, 2, 44100);
        let mut w = WavWriter::create(&p, &fmt).unwrap();
        w.write(&[0u8; 8 * 10]).unwrap();
        let mut cue = Vec::new();
        cue.extend(1u32.to_le_bytes());
        cue.extend(7u32.to_le_bytes());
        cue.extend(0u32.to_le_bytes());
        cue.extend(b"data");
        cue.extend([0u8; 8]);
        cue.extend(5u32.to_le_bytes());
        let mut adtl = b"adtl".to_vec();
        adtl.extend(b"labl");
        adtl.extend(8u32.to_le_bytes());
        adtl.extend(7u32.to_le_bytes());
        adtl.extend(b"Hi!\0");
        let m = Markers { cue_raw: Some(cue), adtl_raw: Some(adtl), ..Default::default() };
        let mut t = ChunkBuf::default();
        t.markers(&m);
        t.info("Title", "GTA", "Comment");
        w.finish(&t.0).unwrap();
        let info = parse(&mut File::open(&p).unwrap()).unwrap().unwrap();
        assert_eq!((info.tag, info.channels, info.rate, info.bits), (TAG_FLOAT, 2, 44100, 32));
        assert_eq!(info.fact_frames, Some(10));
        assert_eq!(info.data_len, 80);
        assert_eq!(info.markers.points.len(), 1);
        assert_eq!(info.markers.points[0].frame, 5);
        assert_eq!(info.markers.points[0].label, "Hi!");
        std::fs::remove_dir_all(dir).ok();
    }
}
