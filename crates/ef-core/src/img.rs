//! IMG archives, the containers the games stream models, textures and animations from.
//!
//! * Version 1 (GTA III, Vice City): the data in `name.img`, the directory in a separate `name.dir`
//!   of 32-byte entries `{u32 offset, u32 size, char name[24]}`, offsets and sizes in 2048-byte
//!   sectors. Sources: re3 `src/core/Directory.h` (`CDirectory::DirectoryInfo`, asserted 32 bytes in
//!   `CStreaming::LoadCdDirectory`), `src/core/CdStream.h` (`CDSTREAM_SECTOR_SIZE` 2048).
//! * Version 2 (San Andreas): `"VER2"`, `u32 count`, then 32-byte entries
//!   `{u32 offset, u16 streamingSize, u16 sizeInArchive, char name[24]}` in sectors from the start
//!   of the archive. The size is `sizeInArchive` when it is non-zero, otherwise `streamingSize`.
//!   Source: gta-reversed `source/game_sa/Core/Directory.h` and `CStreaming::LoadCdDirectory`.
//!
//! An archive is exposed as a flat [`Vfs`] whose files are the archive entries.

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use crate::vfs::{DirEntry, FileLocation, Vfs};

pub const SECTOR: u64 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImgVersion {
    V1,
    V2,
}

impl ImgVersion {
    pub fn label(self) -> &'static str {
        match self {
            ImgVersion::V1 => "IMG version 1 (.img + .dir)",
            ImgVersion::V2 => "IMG version 2 (VER2)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImgEntry {
    pub name: String,
    /// Byte offset inside the .img file.
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImgError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0} is not an IMG archive (no VER2 header and no matching .dir file)")]
    NotImg(String),
    #[error("{0}: entry {1} lies outside the archive")]
    OutOfRange(String, String),
}

/// One archive inside another file system (a game folder or a disc image).
pub struct ImgVfs {
    path: String,
    version: ImgVersion,
    host: FileLocation,
    entries: Vec<ImgEntry>,
}

fn entry_name(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

impl ImgVfs {
    /// Opens the archive at `path` (an `.img` file) inside `vfs`.
    pub fn open(vfs: &Arc<dyn Vfs>, path: &str) -> Result<Self, ImgError> {
        let host = vfs.locate(path).ok_or_else(|| ImgError::NotImg(path.to_string()))?;
        let mut f = vfs.open(path)?;
        let mut magic = [0u8; 8];
        let has_header = f.read(&mut magic)? == 8;
        let (version, raw) = if has_header && &magic[..4] == b"VER2" {
            let count = u32::from_le_bytes(magic[4..8].try_into().unwrap()) as usize;
            // each entry is 32 bytes; refuse counts that cannot fit in the file
            if (count as u64) * 32 + 8 > host.len {
                return Err(ImgError::NotImg(path.to_string()));
            }
            let mut d = vec![0u8; count * 32];
            f.seek(SeekFrom::Start(8))?;
            f.read_exact(&mut d)?;
            (ImgVersion::V2, d)
        } else {
            let dir = sibling_dir(path);
            if !vfs.exists(&dir) {
                return Err(ImgError::NotImg(path.to_string()));
            }
            (ImgVersion::V1, vfs.read_all(&dir)?)
        };
        let mut entries = Vec::with_capacity(raw.len() / 32);
        for e in raw.as_chunks::<32>().0 {
            let off = u32::from_le_bytes(e[0..4].try_into().unwrap()) as u64;
            let sectors = match version {
                ImgVersion::V1 => u32::from_le_bytes(e[4..8].try_into().unwrap()) as u64,
                ImgVersion::V2 => {
                    let streaming = u16::from_le_bytes([e[4], e[5]]) as u64;
                    let in_archive = u16::from_le_bytes([e[6], e[7]]) as u64;
                    if in_archive != 0 { in_archive } else { streaming }
                }
            };
            let name = entry_name(&e[8..32]);
            let (offset, len) = (off * SECTOR, sectors * SECTOR);
            if offset + len > host.len {
                return Err(ImgError::OutOfRange(path.to_string(), name));
            }
            entries.push(ImgEntry { name, offset, len });
        }
        Ok(Self { path: path.to_string(), version, host, entries })
    }

    pub fn version(&self) -> ImgVersion {
        self.version
    }
    pub fn entries(&self) -> &[ImgEntry] {
        &self.entries
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    fn find(&self, name: &str) -> Option<&ImgEntry> {
        let name = name.trim_start_matches(['/', '\\']);
        self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name))
    }
}

/// `models/gta3.img` -> `models/gta3.dir`, keeping the case style of the extension.
fn sibling_dir(path: &str) -> String {
    let stem = &path[..path.len() - path.rsplit('.').next().map_or(0, |e| e.len() + 1)];
    let upper = path.ends_with(".IMG");
    format!("{stem}.{}", if upper { "DIR" } else { "dir" })
}

impl Vfs for ImgVfs {
    fn describe(&self) -> String {
        format!("{} ({})", self.path, self.version.label())
    }
    fn list(&self, dir: &str) -> Option<Vec<DirEntry>> {
        if !dir.trim_matches(['/', '\\']).is_empty() {
            return None;
        }
        let mut v: Vec<DirEntry> =
            self.entries.iter().map(|e| DirEntry { name: e.name.clone(), is_dir: false, size: e.len }).collect();
        v.sort_by_key(|e| e.name.to_ascii_lowercase());
        Some(v)
    }
    fn locate(&self, path: &str) -> Option<FileLocation> {
        let e = self.find(path)?;
        Some(FileLocation {
            host: self.host.host.clone(),
            offset: self.host.offset + e.offset,
            len: e.len,
            sectors: self.host.sectors.clone(),
        })
    }
}

/// Whether a file name looks like an IMG archive (the extension the games use).
pub fn is_img_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".img")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::FolderVfs;

    fn v1(entries: &[(&str, &[u8])]) -> (Vec<u8>, Vec<u8>) {
        let (mut dir, mut img) = (Vec::new(), Vec::new());
        for (name, data) in entries {
            let sectors = data.len().div_ceil(2048);
            dir.extend_from_slice(&((img.len() / 2048) as u32).to_le_bytes());
            dir.extend_from_slice(&(sectors as u32).to_le_bytes());
            let mut n = name.as_bytes().to_vec();
            n.resize(24, 0);
            dir.extend(n);
            img.extend_from_slice(data);
            img.resize(img.len().div_ceil(2048) * 2048, 0);
        }
        (dir, img)
    }

    #[test]
    fn reads_v1_and_v2_archives() {
        let root = std::env::temp_dir().join(format!("ef-img-{}", std::process::id()));
        std::fs::create_dir_all(root.join("models")).unwrap();
        let (dir, img) = v1(&[("a.txd", b"hello"), ("B.DFF", &[7u8; 3000])]);
        std::fs::write(root.join("models/gta3.dir"), dir).unwrap();
        std::fs::write(root.join("models/gta3.img"), img).unwrap();
        let mut v2 = b"VER2".to_vec();
        v2.extend(1u32.to_le_bytes());
        v2.extend(1u32.to_le_bytes()); // sector 1
        v2.extend(1u16.to_le_bytes()); // streaming size
        v2.extend(0u16.to_le_bytes()); // size in archive 0 -> use streaming size
        let mut n = b"x.txd".to_vec();
        n.resize(24, 0);
        v2.extend(n);
        v2.resize(2048, 0);
        v2.extend(b"world");
        v2.resize(4096, 0);
        std::fs::write(root.join("models/gta_int.img"), v2).unwrap();

        let vfs: Arc<dyn Vfs> = Arc::new(FolderVfs::new(&root));
        let a = ImgVfs::open(&vfs, "MODELS/GTA3.IMG").unwrap();
        assert_eq!(a.version(), ImgVersion::V1);
        assert_eq!(a.list("").unwrap().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a.txd", "B.DFF"]);
        assert_eq!(&a.read_all("A.TXD").unwrap()[..5], b"hello");
        assert_eq!(a.size("b.dff"), Some(4096));
        let b = ImgVfs::open(&vfs, "models/gta_int.img").unwrap();
        assert_eq!(b.version(), ImgVersion::V2);
        assert_eq!(&b.read_all("x.txd").unwrap()[..5], b"world");
        assert!(ImgVfs::open(&vfs, "models/gta3.dir").is_err());
        std::fs::remove_dir_all(&root).ok();
    }
}
