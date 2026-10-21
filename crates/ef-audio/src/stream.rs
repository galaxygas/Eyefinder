//! Byte streams over (a window of) a game file, with the games' obfuscation transforms.

use std::io::{self, Read, Seek, SeekFrom};

use ef_core::vfs::{RangeFile, Vfs};

/// Byte transform applied on read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Xform {
    None,
    /// Every byte XOR a constant. Vice City PC `.ADF` radio files use `0x22`
    /// (reVC `sampman_miles.cpp`, `vfs_read_callback`).
    Xor(u8),
    /// San Andreas `AUDIO\STREAMS` files: XOR with a 16-byte key indexed by the byte's offset in the
    /// stream file (gta-reversed `CAEStreamTransformer::TransformBuffer`).
    SaStreamKey,
}

pub const SA_STREAM_KEY: [u8; 16] =
    [0xEA, 0x3A, 0xC4, 0xA1, 0x9A, 0xA8, 0x14, 0xF3, 0x48, 0xB0, 0xD7, 0x23, 0x9D, 0xE8, 0xFF, 0xF1];

/// A window `[base, base+len)` of a file, read through a transform.
pub struct ByteStream {
    file: RangeFile,
    base: u64,
    len: u64,
    pos: u64,
    xform: Xform,
}

impl ByteStream {
    pub fn open(vfs: &dyn Vfs, path: &str) -> io::Result<Self> {
        let file = vfs.open(path)?;
        let len = file.len();
        Ok(Self { file, base: 0, len, pos: 0, xform: Xform::None })
    }

    pub fn open_range(vfs: &dyn Vfs, path: &str, offset: u64, len: u64, xform: Xform) -> io::Result<Self> {
        let mut file = vfs.open(path)?;
        let total = file.len();
        if offset > total {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("{path}: offset {offset} is beyond the end ({total} bytes)"),
            ));
        }
        let len = len.min(total - offset);
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self { file, base: offset, len, pos: 0, xform })
    }

    pub fn with_xform(mut self, xform: Xform) -> Self {
        self.xform = xform;
        self
    }

    /// A sub-window of this stream (offsets relative to this stream), keeping the transform.
    pub fn sub(mut self, offset: u64, len: u64) -> io::Result<Self> {
        let offset = offset.min(self.len);
        self.base += offset;
        self.len = len.min(self.len - offset);
        self.pos = 0;
        self.file.seek(SeekFrom::Start(self.base))?;
        Ok(self)
    }

    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// An independent stream over the same window, positioned at its start.
    pub fn reopen(&self) -> io::Result<Self> {
        let mut file = self.file.reopen()?;
        file.seek(SeekFrom::Start(self.base))?;
        Ok(Self { file, base: self.base, len: self.len, pos: 0, xform: self.xform })
    }

    pub fn read_vec(&mut self, n: usize) -> io::Result<Vec<u8>> {
        let mut v = vec![0u8; n];
        self.read_exact(&mut v)?;
        Ok(v)
    }
}

impl Read for ByteStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.pos);
        if left == 0 {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(left) as usize;
        let got = self.file.read(&mut buf[..n])?;
        match self.xform {
            Xform::None => {}
            Xform::Xor(k) => buf[..got].iter_mut().for_each(|b| *b ^= k),
            Xform::SaStreamKey => {
                let start = self.base + self.pos;
                for (i, b) in buf[..got].iter_mut().enumerate() {
                    *b ^= SA_STREAM_KEY[((start + i as u64) & 0xF) as usize];
                }
            }
        }
        self.pos += got as u64;
        Ok(got)
    }
}

impl Seek for ByteStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let t = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if t < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = t as u64;
        self.file.seek(SeekFrom::Start(self.base + self.pos))?;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for ByteStream {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

/// A media source that can produce a fresh copy of itself, used for exact seeking.
pub trait ReopenMedia: symphonia::core::io::MediaSource {
    fn reopen_media(&self) -> io::Result<Box<dyn ReopenMedia>>;
}

impl ReopenMedia for ByteStream {
    fn reopen_media(&self) -> io::Result<Box<dyn ReopenMedia>> {
        Ok(Box::new(self.reopen()?))
    }
}

/// Bytes held in memory (e.g. audio demuxed from a movie).
pub struct MemStream {
    data: std::sync::Arc<[u8]>,
    pos: u64,
}

impl MemStream {
    pub fn new(data: impl Into<std::sync::Arc<[u8]>>) -> Self {
        Self { data: data.into(), pos: 0 }
    }
}

impl Read for MemStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = (self.pos as usize).min(self.data.len());
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for MemStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let t = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.data.len() as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if t < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = t as u64;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for MemStream {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

impl ReopenMedia for MemStream {
    fn reopen_media(&self) -> io::Result<Box<dyn ReopenMedia>> {
        Ok(Box::new(MemStream { data: self.data.clone(), pos: 0 }))
    }
}
