//! Settings remembered between runs (stored by eframe as RON).

use std::path::PathBuf;

use ef_core::ImportChoice;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    System,
}

/// A game in the library: where it is, and the game / platform / version the user chose for it
/// (`None`: use what detection finds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "LibraryItemRepr")]
pub struct LibraryItem {
    pub path: PathBuf,
    pub choice: Option<ImportChoice>,
}

/// Version 0.1 stored the library as plain paths; both forms are read.
#[derive(Deserialize)]
#[serde(untagged)]
enum LibraryItemRepr {
    Full {
        path: PathBuf,
        #[serde(default)]
        choice: Option<ImportChoice>,
    },
    Plain(PathBuf),
}

impl From<LibraryItemRepr> for LibraryItem {
    fn from(r: LibraryItemRepr) -> Self {
        match r {
            LibraryItemRepr::Full { path, choice } => Self { path, choice },
            LibraryItemRepr::Plain(path) => Self { path, choice: None },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub library: Vec<LibraryItem>,
    pub out_root: Option<PathBuf>,
    pub theme: ThemeChoice,
    pub float_output: bool,
    pub split_markers: bool,
    pub keep_bitstream: bool,
    pub volume: f32,
    /// Sidebar shows the raw file tree instead of asset sections.
    pub file_tree: bool,
    /// Texture browser: grid (true) or list.
    pub texture_grid: bool,
    pub tile_size: f32,
    pub texture_mipmaps: bool,
    pub texture_dds: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library: Vec::new(),
            out_root: None,
            theme: ThemeChoice::Dark,
            float_output: true,
            split_markers: true,
            keep_bitstream: true,
            volume: 0.8,
            file_tree: false,
            texture_grid: true,
            tile_size: 132.0,
            texture_mipmaps: false,
            texture_dds: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_0_1_library() {
        // eyefinder 0.1 wrote `library: ["path", ...]`
        let old = r#"(library: ["/games/gta3", "/discs/vc.iso"], out_root: None, theme: Light, float_output: true, split_markers: true, keep_bitstream: false, volume: 0.5)"#;
        let s: Settings = ron::from_str(old).unwrap();
        assert_eq!(s.library.len(), 2);
        assert_eq!(s.library[1], LibraryItem { path: "/discs/vc.iso".into(), choice: None });
        assert_eq!(s.theme, ThemeChoice::Light);
        assert!(s.texture_grid, "new fields take their defaults");
        let new = ron::to_string(&s).unwrap();
        let back: Settings = ron::from_str(&new).unwrap();
        assert_eq!(back.library, s.library);
    }

    #[test]
    fn keeps_import_choices() {
        let mut s = Settings::default();
        s.library.push(LibraryItem {
            path: "/g/sa".into(),
            choice: Some(ImportChoice {
                game: ef_core::Game::SanAndreas,
                platform: ef_core::Platform::Ps2,
                edition: "ps2-SLUS-20946".into(),
            }),
        });
        let back: Settings = ron::from_str(&ron::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.library, s.library);
    }
}
