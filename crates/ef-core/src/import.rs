//! Resolve user-selected assets and data folders to a catalog root.
//! File-format identities are documented in docs/FORMATS.md; path normalization
//! here is importer policy, not evidence of a particular retail game version.
use crate::vfs::{DirEntry, FileLocation, FolderVfs, IsoError, Vfs};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const FILE_EXTENSIONS: &[&str] = &[
    "iso", "bin", "cue", "gry", "g24", "sty", "fon", "sdt", "raw", "txd", "img", "dir", "exe", "wav", "mp3", "adf",
    "vb",
];

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
    if path.is_file() && matches!(ext.as_str(), "iso" | "bin" | "cue") {
        return Ok(path.to_path_buf());
    }
    if path.is_file() && !FILE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(IsoError::Unsupported(format!(
            ".{ext} is not a supported asset or disc image. Select a game folder, SDT/RAW bank, GRY/G24/STY/FON texture file, TXD, or supported disc image."
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
        if !e.is_dir && e.name.to_ascii_lowercase().ends_with(".cue") {
            candidates.push(folder.join(&e.name));
        }
    }
    if candidates.is_empty() {
        for e in entries.iter().filter(|e| {
            !e.is_dir
                && matches!(
                    e.name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
                    "bin" | "iso" | "img"
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
        return crate::vfs::open_path(&root);
    }
    Ok(open_folder(&root))
}

fn open_folder(root: &Path) -> Arc<dyn Vfs> {
    let fs: Arc<dyn Vfs> = Arc::new(FolderVfs::new(root));
    if crate::has_install_layout(fs.as_ref()) || fs.dir_exists("GTADATA") || fs.dir_exists("DATA") {
        return fs;
    }
    // Loose legacy assets use their documented catalog subtrees, without moving
    // or copying the user's files. Identity is checked separately by detection.
    let entries = fs.list("").unwrap_or_default();
    let any = |ext: &str| entries.iter().any(|e| !e.is_dir && e.name.to_ascii_lowercase().ends_with(ext));
    let prefix = if any(".sty") || fs.exists("FSTYLE.SDT") {
        if any(".sty") { "DATA" } else { "DATA/AUDIO" }
    } else if any(".gry") || any(".g24") || any(".fon") {
        "GTADATA"
    } else if fs.exists("LEVEL000.SDT")
        || fs.exists("LEVEL001.SDT")
        || fs.exists("MISBRIEF.SDT")
        || fs.exists("VOCALCOM.SDT")
    {
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
}
