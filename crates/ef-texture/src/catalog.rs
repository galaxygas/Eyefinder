//! Finds every texture of a game: each `.txd` in the game folder or disc, and each `.txd` entry
//! inside the IMG archives there (`models/gta3.img` and friends). Files are recognised by their
//! contents (a TXD starts with chunk 0x16, an archive is VER2 or has a `.dir` beside it), so no
//! list of file names has to be assumed.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ef_core::img::{ImgVfs, is_img_name};
use ef_core::vfs::{Vfs, walk_files};
use ef_core::{GameInstall, SupportLevel};

use crate::Error;
use crate::raster::NativePlatform;
use crate::txd::{self, Txd};

/// Where a texture dictionary lives.
#[derive(Debug, Clone)]
pub struct TxdSource {
    /// Path of the TXD inside the game (archive entries: `models/gta3.img/name.txd`).
    pub path: String,
    /// The archive holding it, as an index into [`TexCatalog::archives`].
    pub archive: Option<usize>,
    /// Path inside its file system (the archive entry name, or the game path).
    pub inner: String,
    pub bytes: u64,
    /// Display group: the folder for loose files, the archive for archive entries.
    pub group: String,
    pub rw_version: u32,
}

#[derive(Debug, Clone)]
pub struct TexAsset {
    pub source: usize,
    /// Index of the texture inside its dictionary.
    pub index: usize,
    pub name: String,
    pub mask: String,
    pub width: u32,
    pub height: u32,
    pub levels: usize,
    pub format: String,
    pub platform: NativePlatform,
    pub support: SupportLevel,
    pub why_unsupported: Option<&'static str>,
}

#[derive(Default)]
pub struct TexCatalog {
    pub sources: Vec<TxdSource>,
    pub assets: Vec<TexAsset>,
    pub archives: Vec<(String, Arc<ImgVfs>)>,
    pub notices: Vec<String>,
}

/// Progress of [`build`]: `done` of `total` dictionaries read.
pub type Progress<'a> = &'a (dyn Fn(usize, usize) + Sync);

impl TexCatalog {
    pub fn vfs_of(&self, install: &GameInstall, s: &TxdSource) -> Arc<dyn Vfs> {
        match s.archive {
            Some(a) => self.archives[a].1.clone() as Arc<dyn Vfs>,
            None => install.vfs.clone(),
        }
    }
    /// Reads and parses the dictionary of `source` again (for previews and export).
    pub fn load(&self, install: &GameInstall, source: usize) -> Result<Txd, Error> {
        let s = &self.sources[source];
        let bytes = self.vfs_of(install, s).read_all(&s.inner)?;
        txd::parse(&bytes)
    }
    /// Number of textures per group, in display order.
    pub fn groups(&self) -> Vec<(String, usize)> {
        let mut m: BTreeMap<&str, usize> = BTreeMap::new();
        for a in &self.assets {
            *m.entry(&self.sources[a.source].group).or_default() += 1;
        }
        m.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }
}

fn parent(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => "(game folder)".to_string(),
    }
}

/// Lists every texture. Reads each dictionary once; `cancel` stops early.
pub fn build(install: &GameInstall, progress: Progress, cancel: &AtomicBool) -> TexCatalog {
    let vfs = &install.vfs;
    let mut cat = TexCatalog::default();
    let mut candidates: Vec<(Option<usize>, String, String, u64, String)> = Vec::new();
    for (path, e) in walk_files(vfs.as_ref(), "") {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".txd") {
            candidates.push((None, path.clone(), path.clone(), e.size, parent(&path)));
        } else if is_img_name(&path) {
            match ImgVfs::open(vfs, &path) {
                Ok(img) => {
                    let idx = cat.archives.len();
                    for en in img.entries() {
                        if en.name.to_ascii_lowercase().ends_with(".txd") {
                            candidates.push((
                                Some(idx),
                                format!("{path}/{}", en.name),
                                en.name.clone(),
                                en.len,
                                path.clone(),
                            ));
                        }
                    }
                    cat.archives.push((path.clone(), Arc::new(img)));
                }
                Err(ef_core::img::ImgError::NotImg(_)) => {}
                Err(e) => cat.notices.push(format!("{path}: {e}")),
            }
        }
    }
    let total = candidates.len();
    let mut unreadable = 0usize;
    for (n, (archive, path, inner, bytes, group)) in candidates.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            cat.notices.push("Texture indexing was cancelled.".into());
            break;
        }
        progress(n, total);
        let fs: Arc<dyn Vfs> = match archive {
            Some(a) => cat.archives[a].1.clone(),
            None => vfs.clone(),
        };
        let data = match fs.read_all(&inner) {
            Ok(d) => d,
            Err(e) => {
                cat.notices.push(format!("{path}: {e}"));
                continue;
            }
        };
        if !txd::looks_like_txd(&data) {
            unreadable += 1;
            continue;
        }
        let t = match txd::parse(&data) {
            Ok(t) => t,
            Err(e) => {
                cat.notices.push(format!("{path}: {e}"));
                continue;
            }
        };
        for p in &t.problems {
            cat.notices.push(format!("{path}: {p}"));
        }
        let source = cat.sources.len();
        cat.sources.push(TxdSource { path, archive, inner, bytes, group, rw_version: t.rw_version });
        for (index, tex) in t.textures.iter().enumerate() {
            let r = &tex.raster;
            let (support, why) = r.support();
            cat.assets.push(TexAsset {
                source,
                index,
                name: tex.name.clone(),
                mask: tex.mask.clone(),
                width: r.width,
                height: r.height,
                levels: r.levels.len(),
                format: r.format_label(),
                platform: r.platform,
                support,
                why_unsupported: why,
            });
        }
    }
    progress(total, total);
    if unreadable > 0 {
        cat.notices
            .push(format!("{unreadable} files named .txd do not start with a texture dictionary and were skipped"));
    }
    cat
}
