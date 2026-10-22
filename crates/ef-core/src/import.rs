//! Resolve user-selected assets and data folders to a catalog root.
//! File-format identities are documented in docs/FORMATS.md; path normalization
//! here is importer policy, not evidence of a particular retail game version.
use crate::vfs::{DirEntry, FileLocation, FolderVfs, IsoError, Vfs};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Files that can be selected. Selecting a game file imports the installation or data folder it
/// belongs to; disc images and cartridge ROMs are imported as they are.
pub const FILE_EXTENSIONS: &[&str] = &[
    // discs and cartridges
    "iso", "bin", "cue", "gdi", "img", "gb", "gbc", "cgb", "gba", "agb",
    // GTA III / Vice City / San Andreas
    "txd", "dir", "exe", "wav", "mp3", "adf", "vb",
    // GTA1 / London (DMA CityScape, Marschall inventory) and GTA2 (gta2_re, DMA GBH documents)
    "gry", "g24", "sty", "fon", "sdt", "raw", "cmp", "fxt", "ini", "smk", "gmp", "gxt", "scr", "seq", "gci", "tga",
    "bik",
];

/// Extensions imported as a disc or cartridge image rather than as a file of an installation.
pub const IMAGE_EXTENSIONS: &[&str] = &["iso", "bin", "cue", "gdi", "gb", "gbc", "cgb", "gba", "agb"];

/// Selecting an asset imports its containing game/data folder, keeping companion
/// index/palette/bank files available. Multiple selections in that folder coalesce.
pub fn resolve_path(path: &Path) -> Result<PathBuf, IsoError> {
    if !path.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "selected path no longer exists").into());
    }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if path.is_file() && ext == "img" && crate::vfs::IsoVfs::open(path).is_ok() {
        return Ok(path.to_path_buf());
    }
    if path.is_file() && IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Ok(path.to_path_buf());
    }
    if path.is_file() && !FILE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(IsoError::Unsupported(format!(
            ".{ext} files are not read by Eyefinder. Select the game's folder, one of its data files, a disc image (ISO, BIN/CUE, GDI) or a cartridge ROM (GB, GBC, GBA)."
        )));
    }
    let folder = if path.is_dir() { path } else { path.parent().unwrap_or(Path::new(".")) };
    // Prefer a complete installation over a partial asset directory. Bound the
    // parent search so importing a file never scans the entire host filesystem.
    for p in folder.ancestors().take(4) {
        if crate::has_install_layout(&FolderVfs::new(p)) {
            return Ok(p.to_path_buf());
        }
    }
    // Common direct data-folder selections retain their on-disk hierarchy.
    for p in folder.ancestors().take(3) {
        if p.file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("GTADATA") || s.eq_ignore_ascii_case("DATA"))
        {
            return Ok(p.parent().unwrap_or(p).to_path_buf());
        }
    }
    let fs = FolderVfs::new(folder);
    if !crate::possible_choices(open_folder(folder).as_ref()).is_empty() {
        return Ok(folder.to_path_buf());
    }
    // A selected download/disc folder can wrap one installation or one CUE.
    // Never silently choose between multiple copies/images.
    let entries = fs.list("").unwrap_or_default();
    let mut candidates = Vec::new();
    for e in &entries {
        if !e.is_dir && (e.name.to_ascii_lowercase().ends_with(".cue") || e.name.to_ascii_lowercase().ends_with(".gdi"))
        {
            candidates.push(folder.join(&e.name));
        }
    }
    if candidates.is_empty() {
        for e in entries.iter().filter(|e| {
            !e.is_dir
                && matches!(
                    e.name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
                    "bin" | "iso" | "img" | "gdi"
                )
        }) {
            let path = folder.join(&e.name);
            if crate::vfs::IsoVfs::open(&path).is_ok() {
                candidates.push(path);
            }
        }
    }
    if candidates.is_empty() {
        for e in entries.iter().filter(|e| e.is_dir).take(128) {
            let child = folder.join(&e.name);
            if !crate::possible_choices(open_folder(&child).as_ref()).is_empty() {
                candidates.push(child);
            }
        }
    }
    match candidates.len() {
        0 => Ok(folder.to_path_buf()),
        1 => Ok(candidates.remove(0)),
        _ => Err(IsoError::Unsupported("This folder contains multiple games or disc images. Select their individual folders/files using Open files or drag and drop.".into())),
    }
}

pub fn open_path(path: &Path) -> Result<Arc<dyn Vfs>, IsoError> {
    let root = resolve_path(path)?;
    if root.is_file() {
        let fs = mount_assets(crate::vfs::open_path(&root)?);
        if fs.exists("SYSTEM.CNF") || !crate::possible_choices(fs.as_ref()).is_empty() {
            return Ok(fs);
        }
        let mut candidates = Vec::new();
        for entry in fs.list("").unwrap_or_default().into_iter().filter(|e| e.is_dir).take(128) {
            let child = mount_assets(Arc::new(SubdirVfs { inner: fs.clone(), directory: entry.name }));
            if !crate::possible_choices(child.as_ref()).is_empty() {
                candidates.push(child);
            }
        }
        return match candidates.len() {
            0 => Ok(fs), 1 => Ok(candidates.remove(0)),
            _ => Err(IsoError::Unsupported("Multiple installations were found inside this disc. Extract and select the desired installation folder.".into())),
        };
    }
    Ok(open_folder(&root))
}

fn open_folder(root: &Path) -> Arc<dyn Vfs> {
    mount_assets(Arc::new(FolderVfs::new(root)))
}

fn mount_assets(fs: Arc<dyn Vfs>) -> Arc<dyn Vfs> {
    if crate::has_install_layout(fs.as_ref()) || fs.dir_exists("GTADATA") || fs.dir_exists("DATA") {
        return fs;
    }
    // Loose legacy assets use their documented catalog subtrees, without moving
    // or copying the user's files. Identity is checked separately by detection.
    let entries = fs.list("").unwrap_or_default();
    let any = |ext: &str| entries.iter().any(|e| !e.is_dir && e.name.to_ascii_lowercase().ends_with(ext));
    const GTA1_BANKS: [&str; 6] = ["LEVEL000", "LEVEL001", "LEVEL002", "LEVEL003", "MISBRIEF", "VOCALCOM"];
    let gta1_bank = GTA1_BANKS.iter().any(|b| fs.exists(&format!("{b}.SDT")));
    // A GTA2 bank is any other SDT (gta2_re loads DATA/AUDIO/<style>.SDT).
    let gta2_bank = entries.iter().any(|e| {
        let n = e.name.to_ascii_uppercase();
        !e.is_dir && n.ends_with(".SDT") && !GTA1_BANKS.iter().any(|b| n == format!("{b}.SDT"))
    });
    let prefix = if any(".sty") || any(".gxt") || any(".gmp") {
        "DATA"
    } else if gta2_bank && !gta1_bank {
        "DATA/AUDIO"
    } else if any(".gry") || any(".g24") || any(".fon") || any(".cmp") || any(".fxt") {
        "GTADATA"
    } else if gta1_bank {
        "GTADATA/AUDIO"
    } else {
        return fs;
    };
    Arc::new(PrefixVfs { inner: fs, prefix: prefix.into() })
}

struct PrefixVfs {
    inner: Arc<dyn Vfs>,
    prefix: String,
}
impl Vfs for PrefixVfs {
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn list(&self, dir: &str) -> Option<Vec<DirEntry>> {
        let d = dir.trim_matches('/');
        if d.eq_ignore_ascii_case(&self.prefix) {
            return self.inner.list("");
        }
        let prefix = self.prefix.to_ascii_lowercase();
        let lower = d.to_ascii_lowercase();
        if let Some(tail) = lower.strip_prefix(&(prefix.clone() + "/")) {
            return self.inner.list(tail);
        }
        let remaining = if d.is_empty() {
            self.prefix.as_str()
        } else {
            self.prefix.get(d.len() + 1..).filter(|_| prefix.starts_with(&(lower + "/")))?
        };
        Some(vec![DirEntry { name: remaining.split('/').next()?.into(), is_dir: true, size: 0 }])
    }
    fn locate(&self, path: &str) -> Option<FileLocation> {
        let lower = path.to_ascii_lowercase();
        self.inner.locate(lower.strip_prefix(&(self.prefix.to_ascii_lowercase() + "/"))?)
    }
    fn system_area(&self) -> Option<Vec<u8>> {
        self.inner.system_area()
    }
}

struct SubdirVfs {
    inner: Arc<dyn Vfs>,
    directory: String,
}
impl Vfs for SubdirVfs {
    fn describe(&self) -> String {
        format!("{} / {}", self.inner.describe(), self.directory)
    }
    fn list(&self, path: &str) -> Option<Vec<DirEntry>> {
        // The disc's CD audio tracks stay visible at the root of the selected installation.
        let cd = crate::vfs::CD_AUDIO_DIR;
        if path.trim_matches('/').eq_ignore_ascii_case(cd) {
            return self.inner.list(cd);
        }
        let mut v = self.inner.list(&format!("{}/{path}", self.directory))?;
        if path.trim_matches('/').is_empty() && self.inner.dir_exists(cd) {
            v.push(DirEntry { name: cd.into(), is_dir: true, size: 0 });
        }
        Some(v)
    }
    fn locate(&self, path: &str) -> Option<FileLocation> {
        let cd = crate::vfs::CD_AUDIO_DIR;
        if path.len() > cd.len() && path[..cd.len()].eq_ignore_ascii_case(cd) && path.as_bytes()[cd.len()] == b'/' {
            return self.inner.locate(path);
        }
        self.inner.locate(&format!("{}/{path}", self.directory))
    }
    fn system_area(&self) -> Option<Vec<u8>> {
        self.inner.system_area()
    }
}
