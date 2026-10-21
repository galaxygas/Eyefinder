//! Builds the list of audio assets of a detected game installation.

use std::collections::HashSet;

use ef_core::gxt::Gxt;
use ef_core::{Game, GameInstall, Platform, SupportLevel};
use serde::Serialize;

use crate::tables::{self, StreamCategory, StreamDef};
use crate::{Result, sa};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Category {
    Radio,
    Streams,
    PoliceAmbience,
    Announcements,
    Cutscenes,
    MusicCues,
    MissionAudio,
    Movies,
    SfxBank,
    Other,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Radio => "Radio Stations",
            Category::Streams => "Audio Streams",
            Category::PoliceAmbience => "Police Radio & Ambience",
            Category::Announcements => "Announcements & Intros",
            Category::Cutscenes => "Cutscenes",
            Category::MusicCues => "Music Cues",
            Category::MissionAudio => "Mission Audio",
            Category::Movies => "Intro Movies",
            Category::SfxBank => "SFX Bank",
            Category::Other => "Other Audio Files",
        }
    }
    pub const ALL: [Category; 10] = [
        Category::Radio,
        Category::Streams,
        Category::PoliceAmbience,
        Category::Announcements,
        Category::Cutscenes,
        Category::MusicCues,
        Category::MissionAudio,
        Category::Movies,
        Category::SfxBank,
        Category::Other,
    ];
}

impl From<StreamCategory> for Category {
    fn from(c: StreamCategory) -> Self {
        match c {
            StreamCategory::Radio => Category::Radio,
            StreamCategory::PoliceAmbience => Category::PoliceAmbience,
            StreamCategory::Announcements => Category::Announcements,
            StreamCategory::Cutscenes => Category::Cutscenes,
            StreamCategory::MusicCues => Category::MusicCues,
            StreamCategory::MissionAudio => Category::MissionAudio,
        }
    }
}

/// How an asset's bytes are decoded.
#[derive(Debug, Clone)]
pub enum AssetKind {
    /// GTA1/London 12-byte SDT record; PCM metadata comes from the documented bank rules.
    LegacyPcm { raw: String, offset: u32, size: u32, rate: u32, channels: u16, bits: u8 },
    /// RIFF/WAVE, raw MPEG audio or Ogg, detected from the contents.
    Stream { path: String },
    /// Vice City PC radio: MPEG audio XOR 0x22.
    Adf { path: String },
    /// PS2 headerless PS-ADPCM.
    Vb { path: String, rate: u32, channels: u16 },
    /// Audio streams of an MPEG program stream.
    Movie { path: String },
    /// GTA III / Vice City `SFX.SDT` entry (20 bytes) into `SFX.RAW`.
    Sfx { raw: String, offset: u32, size: u32, rate: u32, loop_start: u32, loop_end: i32 },
    /// GTA2 24-byte SDT record; displacement is stored verbatim, not randomized on export.
    Gta2Sfx { raw: String, offset: u32, size: u32, rate: u32, random_displacement: i32, loop_start: u32, loop_end: i32 },
    /// San Andreas stream pack track.
    SaTrack { pack: String, offset: u32, size: u32 },
    /// San Andreas SFX bank sound.
    SaSfx { pak: String, bank: usize, sound: usize, offset: u64, size: u64, rate: u16, loop_start: u32, headroom: i16 },
}

#[derive(Debug, Clone)]
pub struct AudioAsset {
    pub category: Category,
    /// Sub-folder inside the category (SA stream pack / pak name).
    pub group: Option<String>,
    pub title: String,
    /// The game's own identifier or location for this asset.
    pub game_id: String,
    /// Source path inside the installation.
    pub source: String,
    pub bytes: u64,
    /// Output file name without extension.
    pub file_stem: String,
    pub kind: AssetKind,
    pub support: SupportLevel,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub assets: Vec<AudioAsset>,
    /// Things the user should know: unsupported data, missing files, inconsistencies.
    pub notices: Vec<String>,
}

pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 32 { '_' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

fn stem_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(a, _)| a)
}

fn pretty_id(id: &str) -> String {
    let s = id
        .strip_prefix("STREAMED_SOUND_CUTSCENE_")
        .or_else(|| id.strip_prefix("STREAMED_SOUND_MISSION_"))
        .or_else(|| id.strip_prefix("STREAMED_SOUND_"))
        .or_else(|| id.strip_prefix("SFX_MISSION_"))
        .unwrap_or(id);
    s.replace('_', " ")
}

/// Support level of each (game, platform, data) combination. Only GTA III PC has been confirmed
/// against real files so far (see docs/SUPPORT.md).
fn level(game: Game, platform: Platform) -> SupportLevel {
    match (game, platform) {
        (Game::Gta3, Platform::Pc) => SupportLevel::Verified,
        (Game::Gta3 | Game::ViceCity, Platform::Pc | Platform::Ps2) => SupportLevel::FromSpec,
        (Game::Gta1 | Game::Gta2 | Game::London1969 | Game::London1961 | Game::SanAndreas, Platform::Pc) => {
            SupportLevel::FromSpec
        }
        _ => SupportLevel::DetectedOnly,
    }
}

pub fn build(install: &GameInstall) -> Result<Catalog> {
    let vfs = install.vfs.as_ref();
    let support = level(install.game, install.platform);
    let mut cat = Catalog::default();
    if !support.can_decode() {
        cat.notices.push(format!(
            "{} for {} was recognised, but its audio format has no documented specification in Eyefinder yet, so nothing can be decoded. {}",
            install.game.name(),
            install.platform.name(),
            SupportLevel::DetectedOnly.explanation()
        ));
        return Ok(cat);
    }

    match install.game {
        Game::Gta1 | Game::Gta2 | Game::London1969 | Game::London1961 => return crate::legacy::catalog(install),
        Game::Gta3 | Game::ViceCity => {
            let gxt = vfs.read_all("TEXT/AMERICAN.GXT").ok().and_then(|d| Gxt::parse(&d));
            if gxt.is_none() {
                cat.notices
                    .push("TEXT/AMERICAN.GXT could not be read; radio stations are named after their files".into());
            }
            let (table, ps2): (&[StreamDef], bool) = match (install.game, install.platform) {
                (Game::Gta3, Platform::Ps2) => (tables::III_PS2_STREAMS, true),
                (Game::Gta3, _) => (tables::III_PC_STREAMS, false),
                (Game::ViceCity, Platform::Ps2) => (tables::VC_PS2_STREAMS, true),
                _ => (tables::VC_PC_STREAMS, false),
            };
            let mut used = HashSet::new();
            let mut missing = 0usize;
            let mut radio_index = 0usize;
            for d in table {
                let is_radio = d.category == StreamCategory::Radio;
                let station = if is_radio {
                    radio_index += 1;
                    Some(radio_index - 1)
                } else {
                    None
                };
                let Some(loc) = vfs.locate(d.path) else {
                    missing += 1;
                    continue;
                };
                used.insert(d.path.to_ascii_uppercase());
                let code = stem_of(d.path).to_string();
                let (title, file_stem) = match station {
                    Some(i) => {
                        let name = gxt
                            .as_ref()
                            .and_then(|g| g.get(&format!("FEA_FM{i}")))
                            .map(str::to_string)
                            .unwrap_or_else(|| code.clone());
                        (name.clone(), format!("{:02} - {} ({code})", i + 1, sanitize(&name)))
                    }
                    None => {
                        let p = pretty_id(d.id);
                        (p.clone(), format!("{code} - {}", sanitize(&p)))
                    }
                };
                let kind = if ps2 {
                    AssetKind::Vb {
                        path: d.path.into(),
                        rate: tables::vb_rate(install.game, d.id),
                        channels: tables::VB_CHANNELS,
                    }
                } else if d.path.to_ascii_uppercase().ends_with(".ADF") {
                    AssetKind::Adf { path: d.path.into() }
                } else {
                    AssetKind::Stream { path: d.path.into() }
                };
                cat.assets.push(AudioAsset {
                    category: d.category.into(),
                    group: None,
                    title,
                    game_id: d.id.to_string(),
                    source: d.path.into(),
                    bytes: loc.len,
                    file_stem,
                    kind,
                    support,
                });
            }
            if missing > 0 {
                cat.notices.push(format!("{missing} streams listed in the game's table are not present in this copy"));
            }
            other_files(install, &used, support, &mut cat);
            if !ps2 {
                movies(install, support, &mut cat);
                sfx20(install, support, &mut cat)?;
            } else {
                cat.notices.push(
                    "PS2 sound-effect banks, mission audio and movies are not decoded yet: their PS2 formats still need verification against real discs"
                        .into(),
                );
            }
        }
        Game::SanAndreas => {
            match sa::read_tracks(vfs) {
                Ok(tracks) => {
                    for t in tracks {
                        cat.assets.push(AudioAsset {
                            category: Category::Streams,
                            group: Some(t.pack.clone()),
                            title: format!("{} track {:04}", t.pack, t.index),
                            game_id: format!("TRAKLKUP.DAT #{}", t.index),
                            source: format!("AUDIO/STREAMS/{}", t.pack),
                            bytes: t.size as u64,
                            file_stem: format!("track {:04}", t.index),
                            kind: AssetKind::SaTrack { pack: t.pack, offset: t.offset, size: t.size },
                            support,
                        });
                    }
                }
                Err(e) => cat.notices.push(format!("stream tables could not be read: {e}")),
            }
            match sa::read_sfx(vfs) {
                Ok((sounds, problems)) => {
                    cat.notices.extend(problems);
                    for s in sounds {
                        cat.assets.push(AudioAsset {
                            category: Category::SfxBank,
                            group: Some(s.pak.clone()),
                            title: format!("{} bank {:03} sound {:03}", s.pak, s.bank, s.sound),
                            game_id: format!("BANKLKUP.DAT #{} / sound {}", s.bank, s.sound),
                            source: format!("AUDIO/SFX/{}", s.pak),
                            bytes: s.size,
                            file_stem: format!("bank {:03} sound {:03}", s.bank, s.sound),
                            kind: AssetKind::SaSfx {
                                pak: s.pak,
                                bank: s.bank,
                                sound: s.sound,
                                offset: s.offset,
                                size: s.size,
                                rate: s.rate,
                                loop_start: s.loop_start,
                                headroom: s.headroom,
                            },
                            support,
                        });
                    }
                }
                Err(e) => cat.notices.push(format!("SFX bank tables could not be read: {e}")),
            }
            movies(install, support, &mut cat);
        }
    }
    Ok(cat)
}

/// Audio files in AUDIO/ (and AUDIO/MUSIC on PS2) that are not in the game's table.
fn other_files(install: &GameInstall, used: &HashSet<String>, support: SupportLevel, cat: &mut Catalog) {
    let ps2 = install.platform == Platform::Ps2;
    let dirs: &[&str] = if ps2 { &["AUDIO", "AUDIO/MUSIC"] } else { &["AUDIO"] };
    for dir in dirs {
        for e in install.vfs.list(dir).unwrap_or_default() {
            if e.is_dir {
                continue;
            }
            let upper = e.name.to_ascii_uppercase();
            let path = format!("{dir}/{}", e.name);
            if used.contains(&path.to_ascii_uppercase()) {
                continue;
            }
            let kind = if ps2 && upper.ends_with(".VB") {
                // listed but not decodable: a .VB has no header, and files outside the game's table have no
                // known sample rate (see support level below)
                AssetKind::Vb { path: path.clone(), rate: 32000, channels: tables::VB_CHANNELS }
            } else if !ps2 && (upper.ends_with(".WAV") || upper.ends_with(".MP3")) {
                AssetKind::Stream { path: path.clone() }
            } else if !ps2 && upper.ends_with(".ADF") {
                AssetKind::Adf { path: path.clone() }
            } else {
                continue;
            };
            let stem = stem_of(&e.name).to_string();
            cat.assets.push(AudioAsset {
                category: Category::Other,
                group: None,
                title: stem.clone(),
                game_id: e.name.clone(),
                source: path,
                bytes: e.size,
                file_stem: sanitize(&stem),
                kind,
                support: if ps2 { SupportLevel::DetectedOnly } else { support },
            });
        }
    }
}

fn movies(install: &GameInstall, support: SupportLevel, cat: &mut Catalog) {
    for e in install.vfs.list("movies").unwrap_or_default() {
        let upper = e.name.to_ascii_uppercase();
        if e.is_dir || !(upper.ends_with(".MPG") || upper.ends_with(".MPEG")) {
            continue;
        }
        let stem = stem_of(&e.name).to_string();
        cat.assets.push(AudioAsset {
            category: Category::Movies,
            group: None,
            title: format!("{stem} (movie audio)"),
            game_id: e.name.clone(),
            source: format!("movies/{}", e.name),
            bytes: e.size,
            file_stem: sanitize(&stem),
            kind: AssetKind::Movie { path: format!("movies/{}", e.name) },
            support,
        });
    }
}

/// GTA III / Vice City PC `SFX.SDT` + `SFX.RAW`: 20-byte entries
/// `{ offset, size, frequency, loop_start, loop_end }`, 16-bit mono PCM (re3/reVC `tSample`).
fn sfx20(install: &GameInstall, support: SupportLevel, cat: &mut Catalog) -> Result<()> {
    let vfs = install.vfs.as_ref();
    let (Ok(sdt), Some(raw_size)) = (vfs.read_all("AUDIO/SFX.SDT"), vfs.size("AUDIO/SFX.RAW")) else {
        cat.notices.push("AUDIO/SFX.SDT or AUDIO/SFX.RAW not found; the SFX bank is skipped".into());
        return Ok(());
    };
    let names: &[&str] = match install.game {
        Game::Gta3 => tables::III_PC_SFX_NAMES,
        _ => tables::VC_PC_SFX_NAMES,
    };
    let count = sdt.len() / 20;
    let named = count == names.len();
    if !named {
        cat.notices.push(format!(
            "SFX.SDT holds {count} entries but the {} sample list has {}, so samples are numbered instead of named",
            install.game.short(),
            names.len()
        ));
    }
    let mut skipped = 0;
    for (i, e) in sdt.as_chunks::<20>().0.iter().enumerate() {
        let r = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
        let (offset, size, rate, loop_start, loop_end) = (r(0), r(4) & !1, r(8), r(12), r(16) as i32);
        if size == 0 || rate == 0 || offset as u64 + size as u64 > raw_size {
            skipped += 1;
            continue;
        }
        let name = if named { names[i].to_string() } else { "SFX".to_string() };
        cat.assets.push(AudioAsset {
            category: Category::SfxBank,
            group: None,
            title: name.clone(),
            game_id: format!("SFX.SDT #{i}"),
            source: "AUDIO/SFX.RAW".into(),
            bytes: size as u64,
            file_stem: format!("{i:04} {name}"),
            kind: AssetKind::Sfx { raw: "AUDIO/SFX.RAW".into(), offset, size, rate, loop_start, loop_end },
            support,
        });
    }
    if skipped > 0 {
        cat.notices.push(format!("{skipped} SFX.SDT entries are empty or point outside SFX.RAW and were skipped"));
    }
    Ok(())
}
