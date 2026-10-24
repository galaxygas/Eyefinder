//! GTA1 PC SDT/RAW banks. Layout and metadata rules: Daniel Marschall's firsthand
//! analysis, https://misc.daniel-marschall.de/spiele/gta1/format_sdt.html.
//! This is deliberately separate from GTA III/VC's 20-byte SDT records.
use crate::catalog::{AssetKind, AudioAsset, Catalog, Category, sanitize};
use crate::{Error, Result};
use ef_core::vfs::walk_files;
use ef_core::{GameInstall, SupportLevel};

/// One bank's sounds, plus a notice for every record that had to be skipped. A bad record never
/// discards the rest of the bank.
type Bank = (Vec<AudioAsset>, Vec<String>);

fn bank(data: &[u8], raw_len: u64, raw: &str, name: &str) -> Result<Bank> {
    if !data.len().is_multiple_of(12) {
        return Err(Error::Format("GTA1 SDT has a truncated 12-byte record".into()));
    }
    let (mut assets, mut skipped) = (Vec::new(), Vec::new());
    for (i, row) in data.as_chunks::<12>().0.iter().enumerate() {
        let word = |n| u32::from_le_bytes(row[n..n + 4].try_into().unwrap());
        let (offset, size, rate) = (word(0), word(4), word(8));
        if size == 0 {
            continue;
        }
        let bits = if name == "LEVEL000" { 16 } else { 8 };
        let channels = if name == "LEVEL000" && i < 3 { 2 } else { 1 };
        let fb = (bits / 8) as u32 * channels as u32;
        if rate == 0 || offset as u64 + size as u64 > raw_len || !size.is_multiple_of(fb) {
            skipped.push(format!("GTA1 SDT record {i}: invalid rate, range or frame alignment; skipped"));
            continue;
        }
        let id = format!("{name}_{i:04}");
        assets.push(AudioAsset {
            category: Category::SfxBank,
            group: Some(raw.rsplit_once('/').map_or(name, |(p, _)| p).into()),
            title: id.clone(),
            game_id: id.clone(),
            source: raw.into(),
            bytes: size as u64,
            file_stem: sanitize(&id),
            support: SupportLevel::FromSpec,
            kind: AssetKind::LegacyPcm { raw: raw.into(), offset, size, rate, channels, bits },
        });
    }
    Ok((assets, skipped))
}

/// `path` as spelled on disc: the VFS matches names case-insensitively, but asset sources and
/// exported names keep the stored spelling (GTA2 installs use `data/audio/wil.raw`).
fn stored_path(vfs: &dyn ef_core::vfs::Vfs, path: &str) -> String {
    let mut out = String::new();
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        let name = vfs
            .list(&out)
            .and_then(|l| l.into_iter().find(|e| e.name.eq_ignore_ascii_case(comp)))
            .map_or_else(|| comp.to_string(), |e| e.name);
        out = if out.is_empty() { name } else { format!("{out}/{name}") };
    }
    out
}

fn wav(path: String, size: u64, category: Category, group: Option<&str>) -> AudioAsset {
    let file = path.rsplit('/').next().unwrap_or(&path).to_string();
    let stem = file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s).to_string();
    AudioAsset {
        category,
        group: group.map(str::to_string),
        title: stem.clone(),
        game_id: file.to_ascii_uppercase(),
        source: path.clone(),
        bytes: size,
        file_stem: sanitize(&stem),
        kind: AssetKind::Stream { path },
        support: SupportLevel::FromSpec,
    }
}

pub(crate) fn catalog(install: &GameInstall) -> Result<Catalog> {
    let vfs = install.vfs.as_ref();
    let mut cat = Catalog::default();
    // The London UK subtree may be installed beside GTA1; select the expansion explicitly. Never interpret graphics .RAW as sound.
    let london = matches!(install.game, ef_core::Game::London1969 | ef_core::Game::London1961);
    let gta2 = install.game == ef_core::Game::Gta2;
    let root = if gta2 {
        "DATA/AUDIO"
    } else if london && vfs.dir_exists("GTADATA/UK/AUDIO") {
        "GTADATA/UK/AUDIO"
    } else {
        "GTADATA/AUDIO"
    };
    let root = if vfs.dir_exists(root) {
        root
    } else if gta2 {
        "DATA"
    } else if london && vfs.dir_exists("GTADATA/UK") {
        "GTADATA/UK"
    } else {
        "GTADATA"
    };
    let root = stored_path(vfs, root);
    for (path, _) in walk_files(vfs, &root) {
        let upper = path.to_ascii_uppercase();
        let file = upper.rsplit('/').next().unwrap_or(&upper);
        if file.ends_with(".SDT") {
            let name = file.trim_end_matches(".SDT");
            if !gta2 && !matches!(name, "LEVEL000" | "LEVEL001" | "LEVEL002" | "LEVEL003" | "MISBRIEF" | "VOCALCOM") {
                cat.notices.push(format!("{path}: undocumented bank metadata; not decoded"));
                continue;
            }
            if london && matches!(name, "LEVEL002" | "LEVEL003") {
                cat.notices.push(format!("{path}: leftover GTA1 bank, excluded from London"));
                continue;
            }
            let raw = stored_path(vfs, &format!("{}.RAW", &path[..path.len() - 4]));
            let Some(len) = vfs.size(&raw) else {
                cat.notices.push(format!("{path}: missing companion RAW"));
                continue;
            };
            match vfs
                .read_all(&path)
                .map_err(Error::from)
                .and_then(|d| if gta2 { bank_gta2(&d, len, &raw, name) } else { bank(&d, len, &raw, name) })
            {
                Ok((a, skipped)) => {
                    cat.assets.extend(a);
                    cat.notices.extend(skipped.into_iter().map(|m| format!("{path}: {m}")));
                }
                Err(e) => cat.notices.push(format!("{path}: {e}")),
            }
        }
    }
    // Every WAV of the installation, by where the game keeps it:
    // * GTA1/London: GTADATA/AUDIO (GTADATA/UK/AUDIO) holds the cutscene speech 0.WAV-5.WAV
    //   (Marschall inventory); the Rockstar Classics release plays its music from WAV files
    //   (Marschall, overview_pc_editions.html, problems.html "Track10.wav").
    // * GTA2: music is GTAUDIO\*.wav (gta2_re file.cpp gCdCheckFiles, cSampleManager.cpp
    //   ":\GTAUDIO\"); release 9.6f reads "data\gtaudio\" (gta2_re reccmp/96f-ascii.csv).
    //   Voice clips are data\audio\vocals\*.wav.
    // Other WAVs are listed by their path, without guessing what they are.
    for (path, entry) in walk_files(vfs, "") {
        let upper = path.to_ascii_uppercase();
        if !upper.ends_with(".WAV") {
            continue;
        }
        let in_uk = upper.starts_with("GTADATA/UK/");
        if (london && upper.starts_with("GTADATA/") && !in_uk) || (!london && !gta2 && in_uk) {
            continue;
        }
        let dir = upper.rsplit_once('/').map_or("", |(d, _)| d);
        let (category, group) = if gta2 && (dir == "GTAUDIO" || dir == "DATA/GTAUDIO") {
            (Category::Music, Some("GTAUDIO"))
        } else if gta2 && dir == "DATA/AUDIO/VOCALS" {
            (Category::Announcements, Some("vocals"))
        } else if !gta2 && (dir == "GTADATA/AUDIO" || dir == "GTADATA/UK/AUDIO") {
            (Category::Cutscenes, None)
        } else {
            (Category::Other, dir.is_empty().then_some("installation root").or(Some(dir)))
        };
        let group = group.map(|g| if g == dir { path.rsplit_once('/').map_or("", |(d, _)| d) } else { g });
        cat.assets.push(wav(path.clone(), entry.size, category, group));
    }
    cat.notices
        .push("SDT/RAW banks and WAV files are implemented. CD audio tracks are read from BIN/CUE disc images.".into());
    Ok(cat)
}

/// GTA2 SDT slots played as unsigned 8-bit mono: the police radio words.
///
/// gta2_re edb484d3b71d5cadaf3a9ea0e435f56993c747e7, `sound_obj.cpp`: `EnqueueRadioWord_4271B0`
/// clamps every queued word to 69..=136, and `ProcessPoliceRadioWordsPlayback_427220` /
/// `UpdateRadioChatterLoop_41FCA0` play them through `cSampleManager::PlayAtIdx_58E7F0` /
/// `PlaySampleRange_58E8C0` on `field_98_hSample`. `cSampleManager.cpp` `AllocSample_58DA80` sets
/// that handle to `AIL_set_sample_type(.., 0, 0)`: Miles `DIG_F_MONO_8` without `DIG_PCM_SIGN`
/// (`3rdParty/mss.h`), i.e. unsigned 8-bit mono. Every other sample handle is
/// `AIL_set_sample_type(.., 1, 1)`, signed 16-bit mono (`AllocSamples_58D9F0`).
/// Real `data/audio/WIL.SDT` (Rockstar Games 2004 release): slots 69-136 are the only 5500 Hz
/// records, and 34 of them have odd byte sizes (docs/SUPPORT.md, 2026-10-23).
pub const GTA2_RADIO_SLOTS: std::ops::RangeInclusive<usize> = 69..=136;

// gta2_re edb484d3b71d5cadaf3a9ea0e435f56993c747e7:
// cSampleManager.hpp sdt_entry_0x18 and cSampleManager.cpp LoadWavSdtData_58E980 (up to 320
// 24-byte records): offset, length, playback rate, random displacement, loop start, loop end (-1 = end).
fn bank_gta2(data: &[u8], raw_len: u64, raw: &str, name: &str) -> Result<Bank> {
    if !data.len().is_multiple_of(24) {
        return Err(Error::Format("truncated GTA2 SDT record".into()));
    }
    let (mut assets, mut skipped) = (Vec::new(), Vec::new());
    for (i, row) in data.as_chunks::<24>().0.iter().enumerate() {
        let word = |p| i32::from_le_bytes(row[p..p + 4].try_into().unwrap());
        let (offset, size, rate, loop_start, loop_end) = (word(0), word(4), word(8), word(16), word(20));
        if size == 0 {
            continue;
        }
        let bits = if GTA2_RADIO_SLOTS.contains(&i) { 8 } else { 16 };
        if offset < 0
            || size < 0
            || rate <= 0
            || offset as u64 + size as u64 > raw_len
            || (bits == 16 && size % 2 != 0)
            || loop_start < 0
            || loop_end < -1
        {
            skipped.push(format!("GTA2 SDT record {i}: invalid offset, size, rate or loop; skipped"));
            continue;
        }
        let id = format!("{name}_{i:04}");
        assets.push(AudioAsset {
            category: Category::SfxBank,
            group: Some(name.into()),
            title: id.clone(),
            game_id: id.clone(),
            source: raw.into(),
            bytes: size as u64,
            file_stem: id,
            support: SupportLevel::FromSpec,
            kind: AssetKind::Gta2Sfx {
                random_displacement: word(12),
                raw: raw.into(),
                offset: offset as u32,
                size: size as u32,
                rate: rate as u32,
                bits,
                loop_start: loop_start as u32,
                loop_end,
            },
        });
    }
    Ok((assets, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gta1(o: u32, s: u32, r: u32) -> Vec<u8> {
        [o.to_le_bytes(), s.to_le_bytes(), r.to_le_bytes()].concat()
    }
    fn gta2(o: i32, s: i32, r: i32) -> Vec<u8> {
        [o, s, r, 0, 0, -1].iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn bad_records_are_skipped_without_losing_the_bank() {
        assert!(bank(&[0; 11], 0, "x", "LEVEL000").is_err());
        let rows =
            [gta1(u32::MAX, 4, 22050), gta1(0, 3, 22050), gta1(0, 4, 0), gta1(0, 0, 0), gta1(4, 4, 11025)].concat();
        let (assets, skipped) = bank(&rows, 8, "x", "LEVEL000").unwrap();
        assert_eq!(skipped.len(), 3);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].game_id, "LEVEL000_0004");
    }

    #[test]
    fn gta2_radio_slots_are_unsigned_8_bit_and_may_have_odd_sizes() {
        let mut rows = Vec::new();
        for i in 0..140 {
            let size = if GTA2_RADIO_SLOTS.contains(&i) { 3 } else { 4 };
            rows.extend(gta2(i as i32 * 4, size, if size == 3 { 5500 } else { 22050 }));
        }
        let (assets, skipped) = bank_gta2(&rows, 140 * 4, "data/audio/wil.raw", "WIL").unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(assets.len(), 140);
        for (i, a) in assets.iter().enumerate() {
            let AssetKind::Gta2Sfx { bits, .. } = a.kind else { panic!() };
            assert_eq!(bits, if GTA2_RADIO_SLOTS.contains(&i) { 8 } else { 16 }, "slot {i}");
        }
        // an odd size outside the radio slots is one skipped record, not a lost bank
        let (assets, skipped) = bank_gta2(&[gta2(0, 3, 22050), gta2(4, 4, 22050)].concat(), 8, "r", "WIL").unwrap();
        assert_eq!((assets.len(), skipped.len()), (1, 1));
    }
}
