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
pub struct SectorLayout {
    pub stride: u64,
    pub payload: u64,
    pub track_offset: u64,
}

#[derive(Debug, Clone)]
pub struct FileLocation {
    pub host: PathBuf,
    pub offset: u64,
    pub len: u64,
    pub sectors: Option<SectorLayout>,
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
    /// The 32 KiB system area of a disc (ECMA-119 6.2.1: logical sectors 0-15), which holds
    /// `IP.BIN` on a Dreamcast disc. `None` for folders and single files.
    fn system_area(&self) -> Option<Vec<u8>> {
        None
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
        let mut n = (buf.len() as u64).min(left) as usize;
        if let Some(layout) = &self.loc.sectors {
            let logical = self.start + self.pos;
            let within = logical % 2048;
            n = n.min((2048 - within) as usize);
            let physical = layout.track_offset + (logical / 2048) * layout.stride;
            self.file.seek(SeekFrom::Start(physical))?;
            if layout.stride == 2352 {
                let mut header = [0; 24];
                self.file.read_exact(&mut header)?;
                let sync = [0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0];
                let valid = header[..12] == sync
                    && match layout.payload {
                        16 => header[15] == 1,
                        24 => header[15] == 2 && header[16..20] == header[20..24] && header[18] & 0x20 == 0,
                        _ => false,
                    };
                if !valid {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "not a Mode 1 or Mode 2 Form 1 data sector (CD audio/XA Form 2 are not decoded by the filesystem reader)",
                    ));
                }
            }
            self.file.seek(SeekFrom::Start(physical + layout.payload + within))?;
        }
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
        Some(FileLocation { host: p, offset: 0, len: md.len(), sectors: None })
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

/// ISO 9660 filesystem in a plain image or a raw CD data track.
pub struct IsoVfs {
    image: PathBuf,
    backing: FileLocation,
    /// Disc LBA of the first sector of `backing` (45000 for a GD-ROM high-density track).
    base_lba: u64,
    /// Further data tracks holding the same filesystem: `(first LBA, track)` (GD-ROM).
    extra: Vec<(u64, FileLocation)>,
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
    #[error("{0}")]
    Unsupported(String),
}

impl IsoVfs {
    pub fn open(image: impl Into<PathBuf>) -> Result<Self, IsoError> {
        let image = image.into();
        if image.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue")) {
            let (host, layout, length) = crate::cue::data_track(&image)?;
            return Self::from_backing(image, FileLocation { host, offset: 0, len: length, sectors: Some(layout) });
        }
        if image.extension().is_some_and(|e| e.eq_ignore_ascii_case("gdi")) {
            let mut tracks = crate::gdi::data_tracks(&image)?.into_iter();
            let (base, first) =
                tracks.next().ok_or_else(|| IsoError::Unsupported("GDI: no high-density data track".into()))?;
            let mut v = Self::from_backing_at(image, first, base)?;
            v.extra = tracks.collect();
            return Ok(v);
        }
        let length = std::fs::metadata(&image)?.len();
        let plain = FileLocation { host: image.clone(), offset: 0, len: length, sectors: None };
        if let Ok(v) = Self::from_backing(image.clone(), plain) {
            return Ok(v);
        }
        for payload in [16, 24] {
            let backing = FileLocation {
                host: image.clone(),
                offset: 0,
                len: length / 2352 * 2048,
                sectors: Some(SectorLayout { stride: 2352, payload, track_offset: 0 }),
            };
            if let Ok(v) = Self::from_backing(image.clone(), backing) {
                return Ok(v);
            }
        }
        Err(IsoError::NotIso)
    }

    fn from_backing(image: PathBuf, backing: FileLocation) -> Result<Self, IsoError> {
        Self::from_backing_at(image, backing, 0)
    }

    /// `backing` starts at disc sector `base_lba`; directory records hold absolute sector numbers.
    fn from_backing_at(image: PathBuf, backing: FileLocation, base_lba: u64) -> Result<Self, IsoError> {
        let mut f = RangeFile::open(&backing)?;
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
                let rel = (root.lba as u64).checked_sub(base_lba).ok_or(IsoError::NotIso)?;
                if block != 2048 || rel * block + root.size > backing.len {
                    return Err(IsoError::NotIso);
                }
                return Ok(Self {
                    image,
                    backing,
                    base_lba,
                    extra: Vec::new(),
                    block,
                    root,
                    dirs: Mutex::new(HashMap::new()),
                });
            }
        }
        Err(IsoError::NotIso)
    }

    /// The byte range of `size` bytes starting at disc sector `lba`, in whichever track holds it.
    fn extent(&self, lba: u32, size: u64) -> Option<FileLocation> {
        let tracks = std::iter::once((self.base_lba, &self.backing)).chain(self.extra.iter().map(|(b, t)| (*b, t)));
        for (base, t) in tracks {
            let Some(rel) = (lba as u64).checked_sub(base) else { continue };
            let offset = rel * self.block;
            if offset + size <= t.len {
                return Some(FileLocation {
                    host: t.host.clone(),
                    offset: t.offset + offset,
                    len: size,
                    sectors: t.sectors.clone(),
                });
            }
        }
        None
    }

    fn read_dir(&self, node: &IsoNode) -> Option<Arc<Vec<IsoNode>>> {
        if let Some(d) = self.dirs.lock().unwrap().get(&node.lba) {
            return Some(d.clone());
        }
        let mut f = RangeFile::open(&self.extent(node.lba, node.size)?).ok()?;
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
        self.extent(n.lba, n.size)
    }
    fn system_area(&self) -> Option<Vec<u8>> {
        let mut f = RangeFile::open(&FileLocation { len: 16 * 2048, ..self.backing.clone() }).ok()?;
        let mut v = vec![0; 16 * 2048];
        f.read_exact(&mut v).ok()?;
        Some(v)
    }
}

/// A single file (a cartridge ROM image) as a one-entry file system.
pub struct SingleFileVfs {
    path: PathBuf,
}

impl SingleFileVfs {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

impl Vfs for SingleFileVfs {
    fn describe(&self) -> String {
        self.path.display().to_string()
    }
    fn list(&self, dir: &str) -> Option<Vec<DirEntry>> {
        if split(dir).next().is_some() {
            return None;
        }
        let size = std::fs::metadata(&self.path).ok()?.len();
        Some(vec![DirEntry { name: self.name(), is_dir: false, size }])
    }
    fn locate(&self, path: &str) -> Option<FileLocation> {
        let mut parts = split(path);
        let (Some(only), None) = (parts.next(), parts.next()) else { return None };
        if !only.eq_ignore_ascii_case(&self.name()) {
            return None;
        }
        let len = std::fs::metadata(&self.path).ok()?.len();
        Some(FileLocation { host: self.path.clone(), offset: 0, len, sectors: None })
    }
}

/// `name` in `dir`, matched case-insensitively when the exact spelling does not exist (CUE and
/// GDI sheets written on Windows name their track files in any case).
pub fn sibling(dir: &Path, name: &str) -> PathBuf {
    let exact = dir.join(name);
    if exact.exists() {
        return exact;
    }
    let mut cur = dir.to_path_buf();
    for comp in split(name) {
        let found = std::fs::read_dir(&cur).ok().and_then(|rd| {
            rd.flatten().map(|e| e.file_name()).find(|n| n.to_string_lossy().eq_ignore_ascii_case(comp))
        });
        cur = cur.join(found.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(comp)));
    }
    cur
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

/// Cartridge ROM file extensions (Game Boy / Game Boy Color / Game Boy Advance).
pub const ROM_EXTENSIONS: &[&str] = &["gb", "gbc", "cgb", "gba", "agb"];

pub fn is_rom(path: &Path) -> bool {
    path.extension().is_some_and(|e| ROM_EXTENSIONS.iter().any(|r| e.eq_ignore_ascii_case(r)))
}

/// Opens a folder, a disc image or a cartridge ROM.
pub fn open_path(path: &Path) -> Result<Arc<dyn Vfs>, IsoError> {
    if path.is_dir() {
        Ok(Arc::new(FolderVfs::new(path)))
    } else if is_rom(path) {
        Ok(Arc::new(SingleFileVfs::new(path)))
    } else {
        Ok(Arc::new(IsoVfs::open(path)?))
    }
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
    fn raw_cd_and_cue_preserve_file_bytes_and_cross_sector_seeks() {
        let dir = std::env::temp_dir().join(format!("ef-raw-cd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let iso = dir.join("plain.iso");
        build_iso(&iso);
        let plain = std::fs::read(&iso).unwrap();
        for (mode, payload) in [(1, 16), (2, 24)] {
            let mut raw = Vec::new();
            for sector in plain.as_chunks::<2048>().0 {
                let mut block = vec![0; 2352];
                block[1..11].fill(255);
                block[15] = mode;
                block[payload..payload + 2048].copy_from_slice(sector);
                raw.extend(block);
            }
            let bin = dir.join("disc track.bin");
            std::fs::write(&bin, &raw).unwrap();
            let v = IsoVfs::open(&bin).unwrap();
            assert_eq!(v.read_all("audio/head.vb").unwrap(), b"abcd");
            let loc = FileLocation {
                host: bin.clone(),
                offset: 2000,
                len: 4000,
                sectors: Some(SectorLayout { stride: 2352, payload: payload as u64, track_offset: 0 }),
            };
            let mut f = RangeFile::open(&loc).unwrap();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, plain[2000..6000]);
            f.seek(SeekFrom::Start(2047)).unwrap();
            let mut bytes = [0; 10];
            f.read_exact(&mut bytes).unwrap();
            assert_eq!(bytes, plain[4047..4057]);
            let mut with_gap = vec![0; 150 * 2352];
            with_gap.extend(&raw);
            std::fs::write(&bin, with_gap).unwrap();
            let cue = dir.join("disc.cue");
            std::fs::write(&cue, format!("FILE \"disc track.bin\" BINARY\n  TRACK 01 MODE{mode}/2352\n    INDEX 00 00:00:00\n    INDEX 01 00:02:00\n")).unwrap();
            let v = IsoVfs::open(&cue).unwrap();
            assert_eq!(v.read_all("audio/head.vb").unwrap(), b"abcd");
            // A Form 2 sector must not be passed off as 2048-byte file data.
            if mode == 2 {
                let mut raw = std::fs::read(&bin).unwrap();
                raw[(150 + 20) * 2352 + 18] = 0x20;
                raw[(150 + 20) * 2352 + 22] = 0x20;
                std::fs::write(&bin, raw).unwrap();
                assert!(v.read_all("audio/head.vb").is_err());
            }
            std::fs::write(&cue, "FILE \"disc track.bin\" BINARY\n TRACK 01 AUDIO\n INDEX 01 00:00:00\n").unwrap();
            assert!(IsoVfs::open(&cue).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
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
