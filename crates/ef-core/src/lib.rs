//! Eyefinder core: shared types, virtual file systems, game detection and text tables.

pub mod edition;
pub mod gxt;
pub mod img;
pub mod locate;
pub mod vfs;

use std::fmt;
use std::sync::Arc;

pub use edition::Edition;
use serde::{Deserialize, Serialize};
use vfs::Vfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Game {
    Gta3,
    ViceCity,
    SanAndreas,
}

impl Game {
    pub fn name(self) -> &'static str {
        match self {
            Game::Gta3 => "Grand Theft Auto III",
            Game::ViceCity => "Grand Theft Auto: Vice City",
            Game::SanAndreas => "Grand Theft Auto: San Andreas",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Game::Gta3 => "GTA III",
            Game::ViceCity => "Vice City",
            Game::SanAndreas => "San Andreas",
        }
    }
}

impl fmt::Display for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Platform {
    Pc,
    Ps2,
    Xbox,
    Unknown,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Platform::Pc => "PC",
            Platform::Ps2 => "PlayStation 2",
            Platform::Xbox => "Xbox",
            Platform::Unknown => "Unknown platform",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Platform::Pc => "PC",
            Platform::Ps2 => "PS2",
            Platform::Xbox => "Xbox",
            Platform::Unknown => "?",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How much trust a format implementation has earned. Shown to users next to every asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SupportLevel {
    /// Recognised, but no implementation backed by a documented specification exists yet.
    DetectedOnly,
    /// Implemented from a documented specification / reverse-engineered source, not yet confirmed
    /// against real game files.
    FromSpec,
    /// Confirmed against real game files.
    Verified,
}

impl SupportLevel {
    pub fn label(self) -> &'static str {
        match self {
            SupportLevel::Verified => "Verified",
            SupportLevel::FromSpec => "Implemented from spec",
            SupportLevel::DetectedOnly => "Not supported yet",
        }
    }
    pub fn explanation(self) -> &'static str {
        match self {
            SupportLevel::Verified => "This format has been tested against real game files.",
            SupportLevel::FromSpec => {
                "Implemented from documented reverse-engineering sources, but not yet confirmed against real game files. Please report results."
            }
            SupportLevel::DetectedOnly => {
                "Eyefinder recognises this data but has no verified specification for it yet, so it cannot be decoded."
            }
        }
    }
    pub fn can_decode(self) -> bool {
        self != SupportLevel::DetectedOnly
    }
}

/// A detected game installation or disc.
#[derive(Clone)]
pub struct GameInstall {
    pub game: Game,
    pub platform: Platform,
    /// Release of the game (executable version or disc serial); informational only.
    pub edition: Edition,
    pub vfs: Arc<dyn Vfs>,
    /// Facts that led to the detection, shown in the UI.
    pub evidence: Vec<String>,
    /// Whether the game, platform or version was chosen by the user rather than detected.
    pub user_choice: bool,
}

/// A user's choice of game, platform and version when importing, checked by [`detect_with`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportChoice {
    pub game: Game,
    pub platform: Platform,
    /// An [`Edition::id`] from [`edition::known`].
    pub edition: String,
}

impl fmt::Debug for GameInstall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GameInstall")
            .field("game", &self.game)
            .field("platform", &self.platform)
            .field("edition", &self.edition)
            .field("source", &self.vfs.describe())
            .field("evidence", &self.evidence)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("{0}")]
    Open(#[from] vfs::IsoError),
    #[error("no GTA III, Vice City or San Andreas data was found in {0}")]
    NotRecognised(String),
    #[error("{0}")]
    Mismatch(String),
}

/// File sets that identify each game. Every entry is a path the game itself opens, taken from the
/// reverse-engineered sources cited in `docs/SUPPORT.md`.
struct Marker {
    game: Game,
    layout: Platform,
    files: &'static [&'static str],
    source: &'static str,
}

const MARKERS: &[Marker] = &[
    Marker {
        game: Game::Gta3,
        layout: Platform::Pc,
        files: &[
            "AUDIO/HEAD.WAV",
            "AUDIO/CLASS.WAV",
            "AUDIO/KJAH.WAV",
            "AUDIO/RISE.WAV",
            "AUDIO/LIPS.WAV",
            "AUDIO/GAME.WAV",
            "AUDIO/MSX.WAV",
            "AUDIO/FLASH.WAV",
            "AUDIO/CHAT.WAV",
        ],
        source: "re3 StreamedNameTable",
    },
    Marker {
        game: Game::Gta3,
        layout: Platform::Ps2,
        files: &[
            "AUDIO/MUSIC/HEAD.VB",
            "AUDIO/MUSIC/CLASS.VB",
            "AUDIO/MUSIC/KJAH.VB",
            "AUDIO/MUSIC/RISE.VB",
            "AUDIO/MUSIC/LIPS.VB",
            "AUDIO/MUSIC/GAME.VB",
            "AUDIO/MUSIC/MSX.VB",
            "AUDIO/MUSIC/FLASH.VB",
            "AUDIO/MUSIC/CHAT.VB",
        ],
        source: "re3 PS2StreamedNameTable",
    },
    Marker {
        game: Game::ViceCity,
        layout: Platform::Pc,
        files: &[
            "AUDIO/WILD.ADF",
            "AUDIO/FLASH.ADF",
            "AUDIO/KCHAT.ADF",
            "AUDIO/FEVER.ADF",
            "AUDIO/VROCK.ADF",
            "AUDIO/VCPR.ADF",
            "AUDIO/ESPANT.ADF",
            "AUDIO/EMOTION.ADF",
            "AUDIO/WAVE.ADF",
        ],
        source: "reVC StreamedNameTable",
    },
    Marker {
        game: Game::ViceCity,
        layout: Platform::Ps2,
        files: &[
            "AUDIO/MUSIC/WILD.VB",
            "AUDIO/MUSIC/FLASH.VB",
            "AUDIO/MUSIC/KCHAT.VB",
            "AUDIO/MUSIC/FEVER.VB",
            "AUDIO/MUSIC/VROCK.VB",
            "AUDIO/MUSIC/VCPR.VB",
            "AUDIO/MUSIC/ESPANT.VB",
            "AUDIO/MUSIC/EMOTION.VB",
            "AUDIO/MUSIC/WAVE.VB",
        ],
        source: "reVC PS2 StreamedNameTable",
    },
    Marker {
        game: Game::SanAndreas,
        layout: Platform::Unknown,
        files: &["AUDIO/CONFIG/TRAKLKUP.DAT", "AUDIO/CONFIG/STRMPAKS.DAT"],
        source: "gta-reversed CAEMP3TrackLoader",
    },
];

fn console_of(vfs: &dyn Vfs) -> Option<(Platform, &'static str)> {
    // Console discs carry their boot files at the root: SYSTEM.CNF on PS2, default.xbe on Xbox.
    if vfs.exists("SYSTEM.CNF") {
        Some((Platform::Ps2, "SYSTEM.CNF at the disc root (PlayStation 2 boot file)"))
    } else if vfs.exists("default.xbe") {
        Some((Platform::Xbox, "default.xbe at the disc root (Xbox executable)"))
    } else {
        None
    }
}

/// Marker sets with at least two thirds of their files present, best first.
fn matching_markers(vfs: &dyn Vfs) -> Vec<(&'static Marker, usize)> {
    let mut v: Vec<(&Marker, usize)> = MARKERS
        .iter()
        .map(|m| (m, m.files.iter().filter(|f| vfs.exists(f)).count()))
        // require at least two thirds of the marker files, so a stray file cannot trigger a match
        .filter(|(m, found)| found * 3 >= m.files.len() * 2)
        .collect();
    v.sort_by_key(|(_, found)| std::cmp::Reverse(*found));
    v
}

/// The (game, platform) pairs a folder or disc can be imported as: what its file layout supports.
pub fn possible_choices(vfs: &dyn Vfs) -> Vec<(Game, Platform)> {
    let mut out = Vec::new();
    for (m, _) in matching_markers(vfs) {
        let platforms: &[Platform] = if m.layout == Platform::Unknown {
            &[Platform::Pc, Platform::Ps2, Platform::Xbox]
        } else {
            std::slice::from_ref(&m.layout)
        };
        for &p in platforms {
            if !out.contains(&(m.game, p)) {
                out.push((m.game, p));
            }
        }
    }
    out
}

/// Identifies which game (and platform) a folder or disc image holds.
pub fn detect(vfs: Arc<dyn Vfs>) -> Result<GameInstall, DetectError> {
    detect_with(vfs, None)
}

/// Like [`detect`], but takes the user's choice where one is given. The choice must be one the
/// files support ([`possible_choices`]); otherwise the import is refused instead of decoding data
/// with the wrong game's rules.
pub fn detect_with(vfs: Arc<dyn Vfs>, choice: Option<&ImportChoice>) -> Result<GameInstall, DetectError> {
    let console = console_of(vfs.as_ref());
    let matches = matching_markers(vfs.as_ref());
    let Some(&(m, found)) = matches.first() else {
        return Err(DetectError::NotRecognised(vfs.describe()));
    };
    let mut evidence = vec![format!("{found} of {} files from the {} were found", m.files.len(), m.source)];
    let platform = match (m.layout, console) {
        (Platform::Unknown, Some((p, why))) => {
            evidence.push(why.to_string());
            p
        }
        (Platform::Unknown, None) => {
            evidence.push("no console boot file at the root, so this is treated as the PC version".into());
            Platform::Pc
        }
        (layout, Some((p, why))) if p != layout => {
            evidence.push(format!("{why}, but the file layout matches the {} version", layout.name()));
            Platform::Unknown
        }
        (layout, c) => {
            if let Some((_, why)) = c {
                evidence.push(why.to_string());
            }
            layout
        }
    };
    let (mut game, mut platform) = (m.game, platform);
    let mut user_choice = false;
    let mut edition;
    if let Some(c) = choice {
        if (c.game, c.platform) != (game, platform) {
            if !possible_choices(vfs.as_ref()).contains(&(c.game, c.platform)) {
                return Err(DetectError::Mismatch(format!(
                    "{} cannot be opened as {} ({}): its files match {} ({})",
                    vfs.describe(),
                    c.game.name(),
                    c.platform.name(),
                    game.name(),
                    platform.name()
                )));
            }
            evidence.push(format!("you chose {} ({})", c.game.name(), c.platform.name()));
            (game, platform, user_choice) = (c.game, c.platform, true);
        }
        let (detected, why) = edition::identify(vfs.as_ref(), game, platform);
        evidence.push(why);
        edition = detected.clone();
        if c.edition != detected.id {
            let Some(e) = edition::known(game, platform).into_iter().find(|e| e.id == c.edition) else {
                return Err(DetectError::Mismatch(format!("unknown version {} for {}", c.edition, game.name())));
            };
            evidence.push(format!("you chose the version {}", e.label));
            edition = e;
            user_choice = true;
        }
    } else {
        let (detected, why) = edition::identify(vfs.as_ref(), game, platform);
        evidence.push(why);
        edition = detected;
    }
    Ok(GameInstall { game, platform, edition, vfs, evidence, user_choice })
}

/// Opens a folder or disc image and detects the game in it.
pub fn open_and_detect(path: &std::path::Path) -> Result<GameInstall, DetectError> {
    detect(vfs::open_path(path)?)
}

/// Opens a folder or disc image as the user chose (see [`detect_with`]).
pub fn open_with(path: &std::path::Path, choice: Option<&ImportChoice>) -> Result<GameInstall, DetectError> {
    detect_with(vfs::open_path(path)?, choice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_marker_files() {
        let dir = std::env::temp_dir().join(format!("ef-detect-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        for f in ["WILD.ADF", "FLASH.ADF", "KCHAT.ADF", "FEVER.ADF", "VROCK.ADF", "VCPR.ADF"] {
            std::fs::write(dir.join("audio").join(f), b"x").unwrap();
        }
        let g = detect(Arc::new(vfs::FolderVfs::new(&dir))).unwrap();
        assert_eq!((g.game, g.platform), (Game::ViceCity, Platform::Pc));
        std::fs::remove_file(dir.join("audio/VCPR.ADF")).unwrap();
        std::fs::remove_file(dir.join("audio/FEVER.ADF")).unwrap();
        assert!(detect(Arc::new(vfs::FolderVfs::new(&dir))).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
