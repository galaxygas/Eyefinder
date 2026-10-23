//! Eyefinder core: shared types, virtual file systems, game detection and text tables.

pub mod console;
pub mod console_ids;
mod cue;
pub mod discimg;
pub mod edition;
mod gdi;
pub mod gxt;
pub mod img;
pub mod import;
pub mod locate;
pub mod store;
pub mod text;
pub mod vfs;

use std::fmt;
use std::sync::Arc;

pub use edition::Edition;
use serde::{Deserialize, Serialize};
pub use store::Store;
use vfs::Vfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Game {
    Gta1,
    Gta2,
    London1969,
    London1961,
    Gta3,
    ViceCity,
    SanAndreas,
    GtaAdvance,
}

impl Game {
    pub fn name(self) -> &'static str {
        match self {
            Game::Gta1 => "Grand Theft Auto",
            Game::Gta2 => "Grand Theft Auto 2",
            Game::London1969 => "Grand Theft Auto: London 1969",
            Game::London1961 => "Grand Theft Auto: London 1961",
            Game::Gta3 => "Grand Theft Auto III",
            Game::ViceCity => "Grand Theft Auto: Vice City",
            Game::SanAndreas => "Grand Theft Auto: San Andreas",
            Game::GtaAdvance => "Grand Theft Auto Advance",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Game::Gta1 => "GTA1",
            Game::Gta2 => "GTA2",
            Game::London1969 => "London 1969",
            Game::London1961 => "London 1961",
            Game::Gta3 => "GTA III",
            Game::ViceCity => "Vice City",
            Game::SanAndreas => "San Andreas",
            Game::GtaAdvance => "GTA Advance",
        }
    }
    /// The command-line identifier (`eyefinder-cli --game`).
    pub fn cli_id(self) -> &'static str {
        match self {
            Game::Gta1 => "gta1",
            Game::Gta2 => "gta2",
            Game::London1969 => "london1969",
            Game::London1961 => "london1961",
            Game::Gta3 => "gta3",
            Game::ViceCity => "vc",
            Game::SanAndreas => "sa",
            Game::GtaAdvance => "advance",
        }
    }
    pub fn from_cli_id(s: &str) -> Option<Game> {
        let s = s.to_ascii_lowercase().replace([' ', '-', '_', ':'], "");
        Game::ALL.into_iter().find(|g| g.cli_id() == s).or(match s.as_str() {
            "1" | "gta" => Some(Game::Gta1),
            "2" => Some(Game::Gta2),
            "iii" | "3" => Some(Game::Gta3),
            "vicecity" => Some(Game::ViceCity),
            "sanandreas" => Some(Game::SanAndreas),
            "gtaadvance" | "gba" => Some(Game::GtaAdvance),
            _ => None,
        })
    }
    /// The top-down games (GTA1, the London expansions, GTA2, GTA Advance).
    pub fn is_2d(self) -> bool {
        !matches!(self, Game::Gta3 | Game::ViceCity | Game::SanAndreas)
    }
    pub const ALL: [Game; 8] = [
        Game::Gta1,
        Game::London1969,
        Game::London1961,
        Game::Gta2,
        Game::GtaAdvance,
        Game::Gta3,
        Game::ViceCity,
        Game::SanAndreas,
    ];
}

impl fmt::Display for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Platform {
    /// PC: DOS and Windows.
    Pc,
    Ps2,
    Xbox,
    Unknown,
    Ps1,
    Dreamcast,
    GameBoyColor,
    GameBoyAdvance,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Platform::Pc => "PC",
            Platform::Ps1 => "PlayStation",
            Platform::Ps2 => "PlayStation 2",
            Platform::Xbox => "Xbox",
            Platform::Dreamcast => "Dreamcast",
            Platform::GameBoyColor => "Game Boy Color",
            Platform::GameBoyAdvance => "Game Boy Advance",
            Platform::Unknown => "Unknown platform",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Platform::Pc => "PC",
            Platform::Ps1 => "PS1",
            Platform::Ps2 => "PS2",
            Platform::Xbox => "Xbox",
            Platform::Dreamcast => "DC",
            Platform::GameBoyColor => "GBC",
            Platform::GameBoyAdvance => "GBA",
            Platform::Unknown => "?",
        }
    }
    /// The command-line identifier (`eyefinder-cli --platform`).
    pub fn cli_id(self) -> &'static str {
        match self {
            Platform::Pc => "pc",
            Platform::Ps1 => "ps1",
            Platform::Ps2 => "ps2",
            Platform::Xbox => "xbox",
            Platform::Dreamcast => "dc",
            Platform::GameBoyColor => "gbc",
            Platform::GameBoyAdvance => "gba",
            Platform::Unknown => "unknown",
        }
    }
    pub fn from_cli_id(s: &str) -> Option<Platform> {
        let s = s.to_ascii_lowercase();
        Platform::ALL.into_iter().find(|p| p.cli_id() == s).or(match s.as_str() {
            "psx" | "playstation" => Some(Platform::Ps1),
            "dreamcast" => Some(Platform::Dreamcast),
            _ => None,
        })
    }
    /// Platforms with a disc or ROM identity of their own (see [`console`]).
    pub fn is_console_2d(self) -> bool {
        matches!(self, Platform::Ps1 | Platform::Dreamcast | Platform::GameBoyColor | Platform::GameBoyAdvance)
    }
    pub const ALL: [Platform; 7] = [
        Platform::Pc,
        Platform::Ps1,
        Platform::Ps2,
        Platform::Xbox,
        Platform::Dreamcast,
        Platform::GameBoyColor,
        Platform::GameBoyAdvance,
    ];
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

/// What Eyefinder reads from a game on a platform, for the import dialogs: the best support level
/// of its decoded data and a one-line summary (the details are in `docs/SUPPORT.md`).
pub fn support_summary(game: Game, platform: Platform) -> (SupportLevel, &'static str) {
    use Platform::*;
    match (game, platform) {
        (Game::Gta3, Pc) => {
            (SupportLevel::Verified, "Audio verified on real files; textures implemented from the specification.")
        }
        (Game::Gta3 | Game::ViceCity, Ps2) => (
            SupportLevel::FromSpec,
            "Audio streams and textures. SFX banks, mission audio and movies are not decoded yet.",
        ),
        (Game::ViceCity | Game::SanAndreas, Pc) => (SupportLevel::FromSpec, "Audio and textures."),
        (Game::SanAndreas, Ps2) => (SupportLevel::FromSpec, "Textures. Audio is not decoded yet."),
        (Game::Gta3 | Game::ViceCity | Game::SanAndreas, Xbox) => {
            (SupportLevel::FromSpec, "Textures. Audio is not decoded yet.")
        }
        (Game::Gta1 | Game::London1969 | Game::London1961, Pc) => (
            SupportLevel::FromSpec,
            "Sample banks, speech and music WAVs, CD audio tracks (from BIN/CUE), styles, fonts and text.",
        ),
        (Game::Gta2, Pc) => {
            (SupportLevel::FromSpec, "Sample banks, voice clips, music WAVs, styles, frontend images and text.")
        }
        (Game::Gta1 | Game::London1969 | Game::Gta2, Ps1) => (
            SupportLevel::FromSpec,
            "Identified by serial. CD audio tracks (from BIN/CUE) are decoded; the game's own data formats are undocumented (MISSING.md).",
        ),
        (Game::Gta2, Dreamcast) => (
            SupportLevel::FromSpec,
            "Identified by product number. STR audio streams are decoded; other data is undocumented (MISSING.md).",
        ),
        (Game::Gta1 | Game::Gta2, GameBoyColor) | (Game::GtaAdvance, GameBoyAdvance) => (
            SupportLevel::DetectedOnly,
            "Identified by cartridge header and checksum. The ROM's contents are undocumented (MISSING.md).",
        ),
        _ => (SupportLevel::DetectedOnly, "No decoder exists for this game on this platform."),
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
    /// The store a PC copy belongs to ([`store::identify`]); `Unknown` for discs and ROMs.
    pub store: Store,
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
            .field("store", &self.store)
            .field("source", &self.vfs.describe())
            .field("evidence", &self.evidence)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("{0}")]
    Open(#[from] vfs::IsoError),
    #[error("no supported GTA data was found in {0}")]
    NotRecognised(String),
    #[error("{0}")]
    Mismatch(String),
    /// The files support more than one game: the folder holds several (GTA1 with its London
    /// mission packs), or a loose asset's format is shared. The user chooses.
    #[error("{reason}")]
    Ambiguous { reason: String, choices: Vec<(Game, Platform)> },
}

/// File sets that identify each game. Every entry is a path the game itself opens, or a file of
/// the documented retail inventory, from the sources cited per entry and in `docs/SUPPORT.md`.
/// At least two thirds of a set's files must be present.
struct Marker {
    game: Game,
    layout: Platform,
    files: &'static [&'static str],
    source: &'static str,
}

const MARKERS: &[Marker] = &[
    // gta2_re Frontend.cpp / cSampleManager::LoadWavSdtData_58E980.
    Marker {
        game: Game::Gta2,
        layout: Platform::Pc,
        files: &["DATA/FSTYLE.STY", "DATA/AUDIO/FSTYLE.SDT", "DATA/AUDIO/FSTYLE.RAW"],
        source: "GTA2 frontend style and sample bank",
    },
    // gta2_re reccmp/96f-ascii.csv strings: "GTA2.EXE", "data\\fstyle.sty", "data\\%c.gxt"
    // (text_0x14::Load_5B5E90, languages e/f/g/i/j/s), "data\\nyc.gci".
    Marker {
        game: Game::Gta2,
        layout: Platform::Pc,
        files: &["gta2.exe", "DATA/FSTYLE.STY", "DATA/E.GXT", "DATA/NYC.GCI"],
        source: "GTA2 executable, frontend style, text and font files",
    },
    // GTA1 retail inventory: Daniel Marschall, files/index.html (docs/FORMATS.md).
    Marker {
        game: Game::Gta1,
        layout: Platform::Pc,
        files: &[
            "GTADATA/AUDIO/LEVEL000.SDT",
            "GTADATA/AUDIO/LEVEL000.RAW",
            "GTADATA/AUDIO/LEVEL001.SDT",
            "GTADATA/AUDIO/LEVEL001.RAW",
        ],
        source: "GTA1 retail audio inventory",
    },
    Marker {
        game: Game::Gta1,
        layout: Platform::Pc,
        files: &["GTADATA/NYC.CMP", "GTADATA/MIAMI.CMP", "GTADATA/SANB.CMP"],
        source: "GTA1 retail city maps",
    },
    Marker {
        game: Game::Gta1,
        layout: Platform::Pc,
        files: &["GTADATA/ENGLISH.FXT", "GTADATA/MISSION.INI", "GTADATA/STYLE001.G24", "GTADATA/STYLE001.GRY"],
        source: "GTA1 retail text, mission list and style files",
    },
    // London 1969 lives in GTADATA/UK (Marschall inventory). Its sample banks are also used by
    // London 1961, which ships only GTADATA/UK/AUDIO/0e.wav and requires the London CD.
    Marker {
        game: Game::London1969,
        layout: Platform::Pc,
        files: &[
            "GTADATA/UK/AUDIO/LEVEL000.SDT",
            "GTADATA/UK/AUDIO/LEVEL000.RAW",
            "GTADATA/UK/AUDIO/LEVEL001.SDT",
            "GTADATA/UK/AUDIO/LEVEL001.RAW",
        ],
        source: "London 1969 UK audio inventory",
    },
    Marker {
        game: Game::London1969,
        layout: Platform::Pc,
        files: &["GTADATA/UK/UK.CMP", "GTADATA/UK/ENGUK.FXT", "GTADATA/UK/MISSUK.INI", "GTADATA/UK/STYLE001.G24"],
        source: "London 1969 map, text, mission list and style",
    },
    Marker {
        game: Game::London1961,
        layout: Platform::Pc,
        files: &["GTADATA/UK/UKE.CMP", "GTADATA/UK/ENGUKE.FXT", "GTADATA/UK/MISSUKE.INI", "GTADATA/UK/STY001.G24"],
        source: "London 1961 map, text, mission list and style",
    },
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

/// Files that another game also uses: they allow choosing that game, but do not identify it.
/// London 1961 plays London 1969's sample banks (it ships only GTADATA/UK/AUDIO/0e.wav and
/// requires the London CD: Marschall inventory and editions overview).
const SHARED_MARKERS: &[Marker] = &[Marker {
    game: Game::London1961,
    layout: Platform::Pc,
    files: &[
        "GTADATA/UK/AUDIO/LEVEL000.SDT",
        "GTADATA/UK/AUDIO/LEVEL000.RAW",
        "GTADATA/UK/AUDIO/LEVEL001.SDT",
        "GTADATA/UK/AUDIO/LEVEL001.RAW",
    ],
    source: "London 1969 sample banks, which London 1961 also uses",
}];

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

fn marker_found(vfs: &dyn Vfs, m: &Marker) -> usize {
    m.files.iter().filter(|f| vfs.exists(f)).count()
}

/// Marker sets with at least two thirds of their files present, best first.
fn matching_markers(vfs: &dyn Vfs) -> Vec<(&'static Marker, usize)> {
    let mut v: Vec<(&Marker, usize)> = MARKERS
        .iter()
        .map(|m| (m, marker_found(vfs, m)))
        // require at least two thirds of the marker files, so a stray file cannot trigger a match
        .filter(|(m, found)| found * 3 >= m.files.len() * 2)
        .collect();
    if v.is_empty() {
        v.extend(legacy_asset_markers(vfs));
    }
    // Most complete first (fraction of the set present, then file count).
    v.sort_by(|a, b| (b.1 * a.0.files.len().max(1)).cmp(&(a.1 * b.0.files.len().max(1))).then(b.1.cmp(&a.1)));
    v
}

/// The (game, platform) pairs a folder, disc or ROM can be imported as: what its files support.
pub fn possible_choices(vfs: &dyn Vfs) -> Vec<(Game, Platform)> {
    if let Some((game, platform, _, _)) = console::detect(vfs) {
        return match game {
            Some(g) => vec![(g, platform)],
            None => console::games_on(platform).into_iter().map(|g| (g, platform)).collect(),
        };
    }
    let mut out = Vec::new();
    let shared = SHARED_MARKERS.iter().filter(|m| marker_found(vfs, m) * 3 >= m.files.len() * 2).map(|m| (m, 0));
    for (m, _) in matching_markers(vfs).into_iter().chain(shared) {
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
    detect_in_store(vfs, choice, Store::Unknown)
}

fn choose(
    vfs: &dyn Vfs,
    choice: Option<&ImportChoice>,
    detected: (Game, Platform),
    evidence: &mut Vec<String>,
) -> Result<(Game, Platform, bool), DetectError> {
    let Some(c) = choice else { return Ok((detected.0, detected.1, false)) };
    if (c.game, c.platform) == detected {
        return Ok((c.game, c.platform, false));
    }
    if !possible_choices(vfs).contains(&(c.game, c.platform)) {
        return Err(DetectError::Mismatch(format!(
            "{} cannot be opened as {} ({}): its files match {} ({})",
            vfs.describe(),
            c.game.name(),
            c.platform.name(),
            detected.0.name(),
            detected.1.name()
        )));
    }
    evidence.push(format!("you chose {} ({})", c.game.name(), c.platform.name()));
    Ok((c.game, c.platform, true))
}

fn apply_edition(
    vfs: &dyn Vfs,
    choice: Option<&ImportChoice>,
    game: Game,
    platform: Platform,
    store: &Store,
    evidence: &mut Vec<String>,
) -> Result<(Edition, bool), DetectError> {
    let (detected, why) = edition::identify(vfs, game, platform, store);
    evidence.extend(why);
    match choice {
        Some(c) if c.edition != detected.id => {
            let Some(e) = edition::known(game, platform).into_iter().find(|e| e.id == c.edition) else {
                return Err(DetectError::Mismatch(format!("unknown version {} for {}", c.edition, game.name())));
            };
            evidence.push(format!("you chose the version {}", e.label));
            Ok((e, true))
        }
        _ => Ok((detected, false)),
    }
}

/// Like [`detect_with`], for a copy whose store is already known (used for its executable).
pub fn detect_in_store(
    vfs: Arc<dyn Vfs>,
    choice: Option<&ImportChoice>,
    store: Store,
) -> Result<GameInstall, DetectError> {
    if let Some((known_game, platform, edition, mut evidence)) = console::detect(vfs.as_ref()) {
        let detected = match known_game {
            Some(g) => g,
            None => match choice {
                Some(c) if c.platform == platform && console::games_on(platform).contains(&c.game) => c.game,
                _ => {
                    return Err(DetectError::Ambiguous {
                        reason: format!(
                            "This is a {} release that is not in Eyefinder's table of 2D GTA releases ({}). Choose the game only if you know which it is.",
                            platform.name(),
                            evidence.last().cloned().unwrap_or_default()
                        ),
                        choices: possible_choices(vfs.as_ref()),
                    });
                }
            },
        };
        let (game, platform, chosen) = choose(vfs.as_ref(), choice, (detected, platform), &mut evidence)?;
        let mut user_choice = chosen || known_game.is_none();
        if known_game.is_none() {
            evidence.push(format!("you chose {} ({})", detected.name(), platform.name()));
        }
        let mut edition = edition;
        if let Some(c) = choice
            && c.edition != edition.id
        {
            if let Some(e) = edition::known(game, platform).into_iter().find(|e| e.id == c.edition) {
                evidence.push(format!("you chose the version {}", e.label));
                edition = e;
                user_choice = true;
            } else if c.edition != "unknown" {
                return Err(DetectError::Mismatch(format!("unknown version {} for {}", c.edition, game.name())));
            } else {
                edition = Edition::unknown();
            }
        }
        return Ok(GameInstall { game, platform, edition, vfs, evidence, user_choice, store: Store::Unknown });
    }
    let console = console_of(vfs.as_ref());
    let matches = matching_markers(vfs.as_ref());
    let Some(&(m, _)) = matches.first() else {
        return Err(DetectError::NotRecognised(vfs.describe()));
    };
    // Every game whose own files are present. Several games in one folder (GTA1 with the London
    // mission packs installed into it) or a loose shared-format asset need the user's choice.
    let mut games: Vec<Game> = Vec::new();
    for (mm, _) in &matches {
        if !games.contains(&mm.game) {
            games.push(mm.game);
        }
    }
    if choice.is_none() && (games.len() > 1 || (m.files.is_empty() && m.game != Game::Gta2)) {
        let names: Vec<&str> = games.iter().map(|g| g.name()).collect();
        let reason = if m.files.is_empty() {
            format!(
                "These files use a format shared by {}. A loose asset cannot establish which game it came from: choose the game.",
                names.join(", ")
            )
        } else {
            format!("This folder holds files of {}. Choose which to add (each is added separately).", names.join(", "))
        };
        return Err(DetectError::Ambiguous { reason, choices: possible_choices(vfs.as_ref()) });
    }
    let mut evidence = Vec::new();
    for (mm, f) in matches.iter().filter(|(mm, _)| mm.game == m.game) {
        evidence.push(if mm.files.is_empty() {
            mm.source.to_string()
        } else {
            format!("{f} of {} files from the {} were found", mm.files.len(), mm.source)
        });
    }
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
    let (game, platform, chosen) = choose(vfs.as_ref(), choice, (m.game, platform), &mut evidence)?;
    let mut user_choice = chosen || (m.files.is_empty() && choice.is_some());
    if !store.is_unknown() && platform == Platform::Pc {
        evidence.push(format!("store: {}", store.label()));
    }
    let (edition, picked) = apply_edition(vfs.as_ref(), choice, game, platform, &store, &mut evidence)?;
    user_choice |= picked;
    let store = if platform == Platform::Pc { store } else { Store::Unknown };
    Ok(GameInstall { game, platform, edition, vfs, evidence, user_choice, store })
}

/// Opens a folder or disc image and detects the game in it.
pub fn open_and_detect(path: &std::path::Path) -> Result<GameInstall, DetectError> {
    open_with(path, None)
}

/// Opens a folder or disc image as the user chose (see [`detect_with`]). The store is identified
/// from the folder ([`store::identify`]).
pub fn open_with(path: &std::path::Path, choice: Option<&ImportChoice>) -> Result<GameInstall, DetectError> {
    let vfs = import::open_path(path)?;
    let (store, why) = if path.is_dir() { store::identify(path) } else { (Store::Unknown, None) };
    let mut install = detect_in_store(vfs, choice, store)?;
    if let Some(why) = why
        && install.platform == Platform::Pc
    {
        install.evidence.push(why);
    }
    Ok(install)
}

pub(crate) fn has_install_layout(vfs: &dyn Vfs) -> bool {
    MARKERS.iter().any(|m| marker_found(vfs, m) * 3 >= m.files.len() * 2)
}

// Standalone formats identify a format family, not an edition. GRY/G24/FON and the GTA1 bank
// layout are shared by GTA1 and London; London's own files are in GTADATA/UK (Marschall
// inventory). Layout sources: DMA CityScape, GTA2 GBH Style, Marschall FON/SDT, gta2_re SDT/GXT
// (FORMATS.md).
fn legacy_asset_markers(vfs: &dyn Vfs) -> Vec<(&'static Marker, usize)> {
    use std::io::Read;
    static ASSETS: [Marker; 4] = [
        Marker { game: Game::Gta1, layout: Platform::Pc, files: &[], source: "GTA1/London PC asset format" },
        Marker { game: Game::London1969, layout: Platform::Pc, files: &[], source: "GTA1/London PC asset format" },
        Marker { game: Game::London1961, layout: Platform::Pc, files: &[], source: "GTA1/London PC asset format" },
        Marker {
            game: Game::Gta2,
            layout: Platform::Pc,
            files: &[],
            source: "GTA2 PC asset format (STY 700, GXT or 24-byte SDT bank)",
        },
    ];
    let mut outside_uk = false;
    let mut uk = false;
    let mut gta2 = false;
    for dir in ["GTADATA", "GTADATA/UK", "DATA"] {
        for e in vfs.list(dir).unwrap_or_default().into_iter().filter(|e| !e.is_dir) {
            let path = format!("{dir}/{}", e.name);
            let name = e.name.to_ascii_lowercase();
            let mut head = [0; 6];
            let Ok(mut file) = vfs.open(&path) else { continue };
            if file.read_exact(&mut head).is_err() {
                continue;
            }
            if name.ends_with(".sty") && &head[..4] == b"GBST" && u16::from_le_bytes([head[4], head[5]]) == 700 {
                gta2 = true;
            }
            if name.ends_with(".gxt") && &head[..3] == b"GBL" {
                gta2 = true;
            }
            let version = u32::from_le_bytes(head[..4].try_into().unwrap());
            let style = (name.ends_with(".gry") && version == 325) || (name.ends_with(".g24") && version == 336);
            // FON has no magic. Check every image boundary and the exact palette
            // length before treating the extension as a supported container.
            let fon = name.ends_with(".fon") && valid_fon_header(&mut file);
            if style || fon {
                if dir == "GTADATA/UK" { uk = true } else { outside_uk = true }
            }
        }
    }
    for root in ["GTADATA/AUDIO", "GTADATA/UK/AUDIO"] {
        for bank in ["LEVEL000", "LEVEL001", "LEVEL002", "LEVEL003", "MISBRIEF", "VOCALCOM"] {
            if vfs.exists(&format!("{root}/{bank}.SDT")) && vfs.exists(&format!("{root}/{bank}.RAW")) {
                if root.contains("/UK/") { uk = true } else { outside_uk = true }
            }
        }
    }
    // GTA2 banks: any DATA/AUDIO/<name>.SDT with a .RAW whose index is whole 24-byte records.
    for e in vfs.list("DATA/AUDIO").unwrap_or_default().into_iter().filter(|e| !e.is_dir) {
        let Some(stem) = e.name.strip_suffix(".sdt").or_else(|| e.name.strip_suffix(".SDT")) else { continue };
        if e.size > 0 && e.size % 24 == 0 && vfs.exists(&format!("DATA/AUDIO/{stem}.RAW")) {
            gta2 = true;
        }
    }
    let mut out = Vec::new();
    if outside_uk {
        out.extend(ASSETS[..3].iter().map(|m| (m, 1)));
    } else if uk {
        out.extend(ASSETS[1..3].iter().map(|m| (m, 1)));
    }
    if gta2 {
        out.push((&ASSETS[3], 1));
    }
    out
}

fn valid_fon_header(file: &mut vfs::RangeFile) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let mut h = [0; 2];
    if file.seek(SeekFrom::Start(0)).is_err() || file.read_exact(&mut h).is_err() || h[1] == 0 {
        return false;
    }
    for _ in 0..h[0] {
        let mut w = [0];
        if file.read_exact(&mut w).is_err() || file.seek(SeekFrom::Current(w[0] as i64 * h[1] as i64)).is_err() {
            return false;
        }
    }
    file.stream_position().is_ok_and(|p| p + 768 == file.len())
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
