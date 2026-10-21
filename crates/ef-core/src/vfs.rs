//! Read-only virtual file systems over a game installation: a plain folder or an ISO 9660 disc image.
//!
//! Paths are `/`-separated, relative to the root, and matched case-insensitively, because the games
//! open their files with mixed case (`AUDIO\\HEAD.WAV` vs. `audio/head.wav` on disk).

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A readable, seekable byte stream.
pub trait ReadSeek: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> ReadSeek for T {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Name as stored (original case).
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// Where the bytes of a file live: a byte range of a host file.
#[derive(Debug, Clone)]
pub struct FileLocation {
    pub host: PathBuf,
    pub offset: u64,
    pub len: u64,
}

pub trait Vfs: Send + Sync {
    /// Human readable description, e.g. the folder path or the image file name.
    fn describe(&self) -> String;
    /// Lists a directory ("" = root). Returns `None` if it does not exist.
    fn list(&self, dir: &str) -> Option<Vec<DirEntry>>;
    /// Resolves a path case-insensitively to the byte range holding the file.
    fn locate(&self, path: &str) -> Option<FileLocation>;

    fn exists(&self, path: &str) -> bool {
        self.locate(path).is_some()
    }
    fn dir_exists(&self, path: &str) -> bool {
        self.list(path).is_some()
    }
    fn size(&self, path: &str) -> Option<u64> {
        self.locate(path).map(|l| l.len)
    }
    /// Opens a file for reading.
    fn open(&self, path: &str) -> io::Result<RangeFile> {
        let loc = self.locate(path).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{path} not found in {}", self.describe()))
        })?;
        RangeFile::open(&loc)
    }
    /// Reads a whole (small) file.
    fn read_all(&self, path: &str) -> io::Result<Vec<u8>> {
        let mut f = self.open(path)?;
        let mut v = Vec::with_capacity(f.len() as usize);
        f.read_to_end(&mut v)?;
        Ok(v)
    }
}

/// A file opened as a window `[offset, offset+len)` of a host file.
pub struct RangeFile {
    file: File,
    loc: FileLocation,
    start: u64,
    len: u64,
    pos: u64,
}

impl RangeFile {
    pub fn open(loc: &FileLocation) -> io::Result<Self> {
        let mut file = File::open(&loc.host)?;
        file.seek(SeekFrom::Start(loc.offset))?;
        Ok(Self { file, loc: loc.clone(), start: loc.offset, len: loc.len, pos: 0 })
    }
    /// Opens an independent handle to the same file, positioned at its start.
    pub fn reopen(&self) -> io::Result<Self> {
        Self::open(&self.loc)
    }
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Offset of the window inside the host file.
    pub fn host_offset(&self) -> u64 {
        self.start
    }
}

impl Read for RangeFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.pos);
        if left == 0 {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(left) as usize;
        let got = self.file.read(&mut buf[..n])?;
        self.pos += got as u64;
        Ok(got)
    }
}

impl Seek for RangeFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = target as u64;
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        Ok(self.pos)
    }
}

fn split(path: &str) -> impl Iterator<Item = &str> {
    path.split(['/', '\\']).filter(|c| !c.is_empty() && *c != ".")
}

// ---------------------------------------------------------------------------------------------
// Folder
// ---------------------------------------------------------------------------------------------

/// A game installed in (or extracted to) a folder.
pub struct FolderVfs {
    root: PathBuf,
    cache: Mutex<HashMap<PathBuf, Arc<Vec<DirEntry>>>>,
}

impl FolderVfs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), cache: Mutex::new(HashMap::new()) }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn list_host(&self, dir: &Path) -> Option<Arc<Vec<DirEntry>>> {
        if let Some(c) = self.cache.lock().unwrap().get(dir) {
            return Some(c.clone());
        }
        let rd = std::fs::read_dir(dir).ok()?;
        let mut v: Vec<DirEntry> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let md = e.metadata().ok()?;
                Some(DirEntry {
                    name: e.file_name().to_string_lossy().into_owned(),
                    is_dir: md.is_dir(),
                    size: md.len(),
                })
            })
            .collect();
        v.sort_by_key(|a| a.name.to_lowercase());
        let v = Arc::new(v);
        self.cache.lock().unwrap().insert(dir.to_path_buf(), v.clone());
        Some(v)
    }

    /// Resolves a relative path case-insensitively to a host path.
    pub fn resolve(&self, path: &str) -> Option<PathBuf> {
        let mut cur = self.root.clone();
        for comp in split(path) {
            let entries = self.list_host(&cur)?;
            let e = entries
                .iter()
                .find(|e| e.name == comp)
                .or_else(|| entries.iter().find(|e| e.name.eq_ignore_ascii_case(comp)))?;
            cur.push(&e.name);
        }
        Some(cur)
    }
}

impl Vfs for FolderVfs {
    fn describe(&self) -> String {
        self.root.display().to_string()
    }
    fn list(&self, dir: &str) -> Option<Vec<DirEntry>> {
        let p = self.resolve(dir)?;
        self.list_host(&p).map(|a| (*a).clone())
    }
    fn locate(&self, path: &str) -> Option<FileLocation> {
        let p = self.resolve(path)?;
        let md = std::fs::metadata(&p).ok()?;
        if !md.is_file() {
            return None;
        }
        Some(FileLocation { host: p, offset: 0, len: md.len() })
    }
}

// ---------------------------------------------------------------------------------------------
// ISO 9660 image (ECMA-119), 2048-byte logical sectors, as used by PS2 and PC game discs.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct IsoNode {
    name: String,
    is_dir: bool,
    lba: u32,
    size: u64,
}

/// A disc image in plain ISO 9660 form (`.iso`). Raw 2352-byte CD images are not supported.
pub struct IsoVfs {
    image: PathBuf,
    block: u64,
    root: IsoNode,
    dirs: Mutex<HashMap<u32, Arc<Vec<IsoNode>>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum IsoError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("not an ISO 9660 image (no primary volume descriptor found)")]
    NotIso,
}

impl IsoVfs {
    pub fn open(image: impl Into<PathBuf>) -> Result<Self, IsoError> {
        let image = image.into();
        let mut f = File::open(&image)?;
        // Volume descriptors start at sector 16 (ECMA-119 8.1); type 255 terminates the set.
        for sector in 16u64..64 {
            let mut vd = [0u8; 2048];
            f.seek(SeekFrom::Start(sector * 2048))?;
            if f.read_exact(&mut vd).is_err() {
                break;
            }
            if &vd[1..6] != b"CD001" {
                break;
            }
            if vd[0] == 255 {
                break;
            }
            if vd[0] == 1 {
                let block = u16::from_le_bytes([vd[128], vd[129]]) as u64;
                let root = parse_record(&vd[156..190]).ok_or(IsoError::NotIso)?;
                if block == 0 {
                    return Err(IsoError::NotIso);
                }
                return Ok(Self { image, block, root, dirs: Mutex::new(HashMap::new()) });
            }
        }
        Err(IsoError::NotIso)
    }

    fn read_dir(&self, node: &IsoNode) -> Option<Arc<Vec<IsoNode>>> {
        if let Some(d) = self.dirs.lock().unwrap().get(&node.lba) {
            return Some(d.clone());
        }
        let mut f = File::open(&self.image).ok()?;
        f.seek(SeekFrom::Start(node.lba as u64 * self.block)).ok()?;
        let mut data = vec![0u8; node.size as usize];
        f.read_exact(&mut data).ok()?;
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos < data.len() {
            let len = data[pos] as usize;
            if len == 0 {
                // records never straddle a sector; zero length means "continue at the next sector"
                pos = (pos / self.block as usize + 1) * self.block as usize;
                continue;
            }
            if pos + len > data.len() {
                break;
            }
            // names 0x00 and 0x01 are the "." and ".." records
            if let Some(n) = parse_record(&data[pos..pos + len]).filter(|n| n.name != "\0" && n.name != "\u{1}") {
                out.push(n);
            }
            pos += len;
        }
        out.sort_by_key(|a| a.name.to_lowercase());
        let out = Arc::new(out);
        self.dirs.lock().unwrap().insert(node.lba, out.clone());
        Some(out)
    }

    fn find(&self, path: &str) -> Option<IsoNode> {
        let mut cur = self.root.clone();
        for comp in split(path) {
            if !cur.is_dir {
                return None;
            }
            let entries = self.read_dir(&cur)?;
            cur = entries.iter().find(|e| e.name.eq_ignore_ascii_case(comp))?.clone();
        }
        Some(cur)
    }
}

/// Parses a directory record (ECMA-119 9.1).
fn parse_record(r: &[u8]) -> Option<IsoNode> {
    if r.len() < 34 {
        return None;
    }
    let lba = u32::from_le_bytes(r[2..6].try_into().ok()?);
    let size = u32::from_le_bytes(r[10..14].try_into().ok()?) as u64;
    let flags = r[25];
    let nlen = r[32] as usize;
    if 33 + nlen > r.len() {
        return None;
    }
    let raw = &r[33..33 + nlen];
    let mut name = String::from_utf8_lossy(raw).into_owned();
    if let Some(i) = name.find(';') {
        name.truncate(i); // version number, e.g. ";1"
    }
    if name.ends_with('.') && flags & 2 == 0 {
        name.pop(); // a file without extension is recorded as "NAME."
    }
    Some(IsoNode { name, is_dir: flags & 2 != 0, lba, size })
}

impl Vfs for IsoVfs {
    fn describe(&self) -> String {
        self.image.display().to_string()
    }
    fn list(&self, dir: &str) -> Option<Vec<DirEntry>> {
        let n = self.find(dir)?;
        if !n.is_dir {
            return None;
        }
        Some(
            self.read_dir(&n)?
                .iter()
                .map(|e| DirEntry { name: e.name.clone(), is_dir: e.is_dir, size: e.size })
                .collect(),
        )
    }
    fn locate(&self, path: &str) -> Option<FileLocation> {
        let n = self.find(path)?;
        if n.is_dir {
            return None;
        }
        Some(FileLocation { host: self.image.clone(), offset: n.lba as u64 * self.block, len: n.size })
    }
}

/// Every file below `dir` ("" = root) as `(path, entry)`, directories first visited in name order.
pub fn walk_files(vfs: &dyn Vfs, dir: &str) -> Vec<(String, DirEntry)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.trim_matches('/').to_string()];
    while let Some(d) = stack.pop() {
        let Some(entries) = vfs.list(&d) else { continue };
        let mut sub = Vec::new();
        for e in entries {
            let p = if d.is_empty() { e.name.clone() } else { format!("{d}/{}", e.name) };
            if e.is_dir { sub.push(p) } else { out.push((p, e)) }
        }
        stack.extend(sub.into_iter().rev());
    }
    out
}

/// Opens a folder or a disc image.
pub fn open_path(path: &Path) -> Result<Arc<dyn Vfs>, IsoError> {
    if path.is_dir() { Ok(Arc::new(FolderVfs::new(path))) } else { Ok(Arc::new(IsoVfs::open(path)?)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal ISO 9660 image: root with AUDIO/ (one file) and SYSTEM.CNF.
    fn build_iso(path: &Path) {
        let mut img = vec![0u8; 2048 * 24];
        let rec = |name: &[u8], lba: u32, size: u32, dir: bool| -> Vec<u8> {
            let mut r = vec![0u8; 33 + name.len() + (1 - name.len() % 2)];
            r[0] = r.len() as u8;
            r[2..6].copy_from_slice(&lba.to_le_bytes());
            r[10..14].copy_from_slice(&size.to_le_bytes());
            r[25] = if dir { 2 } else { 0 };
            r[32] = name.len() as u8;
            r[33..33 + name.len()].copy_from_slice(name);
            r
        };
        // PVD at sector 16, terminator at 17
        img[16 * 2048] = 1;
        img[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"CD001");
        img[16 * 2048 + 128..16 * 2048 + 130].copy_from_slice(&2048u16.to_le_bytes());
        let root = rec(b"\0", 18, 2048, true);
        img[16 * 2048 + 156..16 * 2048 + 156 + root.len()].copy_from_slice(&root);
        img[17 * 2048] = 255;
        img[17 * 2048 + 1..17 * 2048 + 6].copy_from_slice(b"CD001");
        let mut d = Vec::new();
        d.extend(rec(b"\0", 18, 2048, true));
        d.extend(rec(b"\x01", 18, 2048, true));
        d.extend(rec(b"AUDIO", 19, 2048, true));
        d.extend(rec(b"SYSTEM.CNF;1", 21, 5, false));
        img[18 * 2048..18 * 2048 + d.len()].copy_from_slice(&d);
        let mut d = Vec::new();
        d.extend(rec(b"\0", 19, 2048, true));
        d.extend(rec(b"\x01", 18, 2048, true));
        d.extend(rec(b"HEAD.VB;1", 20, 4, false));
        img[19 * 2048..19 * 2048 + d.len()].copy_from_slice(&d);
        img[20 * 2048..20 * 2048 + 4].copy_from_slice(b"abcd");
        img[21 * 2048..21 * 2048 + 5].copy_from_slice(b"BOOT2");
        std::fs::write(path, img).unwrap();
    }

    #[test]
    fn iso_lookup_is_case_insensitive_and_strips_versions() {
        let dir = std::env::temp_dir().join(format!("ef-iso-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.iso");
        build_iso(&p);
        let v = IsoVfs::open(&p).unwrap();
        assert!(v.exists("SYSTEM.CNF"));
        assert_eq!(v.read_all("audio/head.vb").unwrap(), b"abcd");
        assert_eq!(v.list("AUDIO").unwrap()[0].name, "HEAD.VB");
        assert!(v.locate("AUDIO").is_none());
        let mut f = v.open("AUDIO/HEAD.VB").unwrap();
        f.seek(SeekFrom::Start(2)).unwrap();
        let mut b = [0u8; 8];
        assert_eq!(f.read(&mut b).unwrap(), 2);
        assert_eq!(&b[..2], b"cd");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn folder_lookup_is_case_insensitive() {
        let dir = std::env::temp_dir().join(format!("ef-fold-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Audio")).unwrap();
        std::fs::write(dir.join("Audio").join("Head.wav"), b"xyz").unwrap();
        let v = FolderVfs::new(&dir);
        assert_eq!(v.read_all("AUDIO/HEAD.WAV").unwrap(), b"xyz");
        assert_eq!(v.size("audio\\head.wav"), Some(3));
        std::fs::remove_dir_all(dir).ok();
    }
}
