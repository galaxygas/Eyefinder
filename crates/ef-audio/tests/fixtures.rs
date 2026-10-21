//! End-to-end tests over the synthetic fixtures in `tests/fixtures` (built by
//! `tools/fixtures/make_fixtures.py`; no game data). Reference hashes come from independent
//! decoders, see `tests/fixtures/reference.json` → `_provenance`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ef_audio::export::{ExportOptions, export};
use ef_audio::pcm::to_f32;
use ef_audio::stream::{ByteStream, MemStream};
use ef_audio::symph::{Container, SymphSource};
use ef_audio::{AssetKind, Category, DecodeOptions, PcmSource, catalog};
use ef_core::{Game, Platform};
use sha2::{Digest, Sha256};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eyefinder-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// RIFF chunks of a WAV file (first occurrence of each id).
fn chunks(p: &Path) -> BTreeMap<[u8; 4], Vec<u8>> {
    let d = std::fs::read(p).unwrap();
    let mut out = BTreeMap::new();
    let mut pos = 12;
    while pos + 8 <= d.len() {
        let id: [u8; 4] = d[pos..pos + 4].try_into().unwrap();
        let n = u32::from_le_bytes(d[pos + 4..pos + 8].try_into().unwrap()) as usize;
        out.entry(id).or_insert_with(|| d[pos + 8..(pos + 8 + n).min(d.len())].to_vec());
        pos += 8 + n + (n & 1);
    }
    out
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn reference() -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(fixtures().join("reference.json")).unwrap()).unwrap()
}

fn export_all(src: &Path, name: &str) -> (PathBuf, ef_audio::export::ExportReport) {
    let install = ef_core::open_and_detect(src).unwrap();
    let cat = catalog::build(&install).unwrap();
    let assets: Vec<_> = cat.assets.iter().filter(|a| a.support.can_decode()).collect();
    let out = temp(name);
    let opts = ExportOptions {
        out_dir: out.clone(),
        decode: DecodeOptions::default(),
        split_markers: true,
        keep_movie_bitstream: true,
    };
    let report = export(&install, &assets, &opts, |_| {}, &AtomicBool::new(false)).unwrap();
    (out, report)
}

fn decode_all(src: &mut dyn PcmSource) -> Vec<u8> {
    let mut v = Vec::new();
    while src.read(&mut v).unwrap() > 0 {}
    v
}

/// Seeking to any frame must give exactly the samples of a decode from the start.
fn assert_seek_exact(mut open: impl FnMut() -> Box<dyn PcmSource>, what: &str) {
    let mut s = open();
    let fb = s.format().frame_bytes();
    let full = decode_all(s.as_mut());
    let frames = full.len() / fb;
    assert!(frames > 0, "{what}: decoded nothing");
    if let Some(t) = s.total_frames() {
        assert_eq!(t as usize, frames, "{what}: total_frames() differs from the decoded length");
    }
    for f in [0, 1, 27, 1000, 1105, frames / 3, frames / 2, frames.saturating_sub(700), frames - 1]
        .into_iter()
        .filter(|&f| f < frames)
    {
        let mut s = open();
        s.seek(f as u64).unwrap();
        let part = decode_all(s.as_mut());
        assert_eq!(part.len(), full.len() - f * fb, "{what}: length after seek to {f}");
        assert!(part == full[f * fb..], "{what}: samples after seek to {f} differ from a full decode");
    }
}

#[test]
fn gta3_pc_matches_the_verified_c_ripper() {
    let r = reference();
    let (out, report) = export_all(&fixtures().join("gta3_pc"), "iii");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let pcm = r["gta3_pc"].as_object().unwrap();
    for (rel, want) in pcm {
        let c = chunks(&out.join(rel));
        assert_eq!(sha(&c[b"data"]), want.as_str().unwrap(), "{rel}");
    }
    for (rel, frames) in r["gta3_pc_mpeg_frames"].as_object().unwrap() {
        let c = chunks(&out.join(rel));
        let ch = u16::from_le_bytes([c[b"fmt "][2], c[b"fmt "][3]]) as usize;
        assert_eq!((c[b"data"].len() / (4 * ch)) as u64, frames.as_u64().unwrap(), "{rel}");
    }
    // per-marker track files join up to the full station
    let dir = out.join("Radio Stations/02 - DOUBLE CLEF FM (CLASS) - Tracks");
    let mut parts: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    parts.sort();
    assert_eq!(parts.len(), 3);
    let joined: Vec<u8> = parts.iter().flat_map(|p| chunks(p)[b"data"].clone()).collect();
    assert_eq!(joined, chunks(&out.join("Radio Stations/02 - DOUBLE CLEF FM (CLASS).wav"))[b"data"]);
    // the original movie bitstream is saved byte for byte
    assert!(out.join("Intro Movies/GTAtitles.original.mp2").exists());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["files_written"].as_u64().unwrap() as usize, report.written);
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn gta3_ps2_iso_matches_vgmstream() {
    let r = reference();
    let iso = fixtures().join("gta3_ps2.iso");
    let install = ef_core::open_and_detect(&iso).unwrap();
    assert_eq!((install.game, install.platform), (Game::Gta3, Platform::Ps2));
    let (out, report) = export_all(&iso, "ps2");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    for (rel, want) in r["gta3_ps2"].as_object().unwrap() {
        let c = chunks(&out.join(rel));
        assert_eq!(sha(&c[b"data"]), want.as_str().unwrap(), "{rel}");
    }
    // CHAT plays at 16 kHz on the PS2 (IsThisTrackAt16KHz)
    let c = chunks(&out.join("Radio Stations/09 - CHATTERBOX FM (CHAT).wav"));
    assert_eq!(u32::from_le_bytes(c[b"fmt "][4..8].try_into().unwrap()), 16000);
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn vice_city_adf_is_mp3_xor_0x22() {
    let dir = fixtures().join("vc_pc");
    let install = ef_core::open_and_detect(&dir).unwrap();
    assert_eq!((install.game, install.platform), (Game::ViceCity, Platform::Pc));
    let cat = catalog::build(&install).unwrap();
    let radio: Vec<_> = cat.assets.iter().filter(|a| a.category == Category::Radio).collect();
    assert_eq!(radio.len(), 9);
    for a in radio {
        let AssetKind::Adf { path } = &a.kind else { panic!("{} is not ADF", a.title) };
        let via_adf = decode_all(
            ef_audio::open::open(a, install.vfs.as_ref(), DecodeOptions::default()).unwrap().source.as_mut(),
        );
        let plain: Vec<u8> = install.vfs.read_all(path).unwrap().iter().map(|b| b ^ 0x22).collect();
        let mut direct = SymphSource::new(Box::new(MemStream::new(plain)), Container::MpegAudio, true).unwrap();
        assert_eq!(via_adf, decode_all(&mut direct), "{}", a.title);
    }
    assert_eq!(cat.assets.iter().filter(|a| a.category == Category::SfxBank).count(), 749);
    let (out, report) = export_all(&dir, "vc");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn san_andreas_streams_and_sfx_banks() {
    let dir = fixtures().join("sa_pc");
    let install = ef_core::open_and_detect(&dir).unwrap();
    assert_eq!((install.game, install.platform), (Game::SanAndreas, Platform::Pc));
    let cat = catalog::build(&install).unwrap();
    assert!(cat.notices.is_empty(), "{:?}", cat.notices);
    let tracks: Vec<_> = cat.assets.iter().filter(|a| a.category == Category::Streams).collect();
    assert_eq!(tracks.len(), 4);
    // track 0 of pack AA, read through the XOR key and the 0x1F84-byte header, equals the plain Ogg
    let opened = ef_audio::open::open(tracks[0], install.vfs.as_ref(), DecodeOptions::default()).unwrap();
    assert_eq!(opened.extra["track_header"]["beat_slots_nonzero"], 4);
    let mut opened = opened;
    let via_streams = decode_all(opened.source.as_mut());
    let plain = std::fs::read(fixtures().join("sa_AA0_plain.ogg")).unwrap();
    let mut direct = SymphSource::new(Box::new(MemStream::new(plain)), Container::Ogg, true).unwrap();
    assert_eq!(via_streams, decode_all(&mut direct));
    // SFX: 2 paks x 2 banks x 3 sounds, each exactly the PCM bytes after the 0x12C4 bank header
    let sfx: Vec<_> = cat.assets.iter().filter(|a| a.category == Category::SfxBank).collect();
    assert_eq!(sfx.len(), 12);
    for a in sfx {
        let AssetKind::SaSfx { pak, offset, size, rate, .. } = &a.kind else { panic!() };
        let raw = install.vfs.read_all(&format!("AUDIO/SFX/{pak}")).unwrap();
        let mut o = ef_audio::open::open(a, install.vfs.as_ref(), DecodeOptions::default()).unwrap();
        assert_eq!(o.source.format().rate, *rate as u32);
        assert_eq!(decode_all(o.source.as_mut()), raw[*offset as usize..(*offset + *size) as usize]);
    }
    let (out, report) = export_all(&dir, "sa");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(out.join("Audio Streams/AA/track 0000.trackinfo.json").exists());
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn seeking_is_sample_exact_for_every_decoder() {
    for (file, container) in [("seek.mp3", Container::MpegAudio), ("seek.ogg", Container::Ogg)] {
        let data = std::fs::read(fixtures().join(file)).unwrap();
        assert_seek_exact(
            || Box::new(SymphSource::new(Box::new(MemStream::new(data.clone())), container, true).unwrap()),
            file,
        );
    }
    let iii = ef_core::open_and_detect(&fixtures().join("gta3_pc")).unwrap();
    let cat = catalog::build(&iii).unwrap();
    for id in [
        "STREAMED_SOUND_RADIO_HEAD",
        "STREAMED_SOUND_RADIO_KJAH",
        "STREAMED_SOUND_RADIO_CHAT",
        "STREAMED_SOUND_NEWS_INTRO",
        "SFX.SDT #9",
    ] {
        let a = cat.assets.iter().find(|a| a.game_id == id).unwrap();
        assert_seek_exact(|| ef_audio::open::open(a, iii.vfs.as_ref(), DecodeOptions::default()).unwrap().source, id);
    }
    let movie = cat.assets.iter().find(|a| a.category == Category::Movies).unwrap();
    assert_seek_exact(
        || ef_audio::open::open(movie, iii.vfs.as_ref(), DecodeOptions::default()).unwrap().source,
        "movie audio",
    );
    let ps2 = ef_core::open_and_detect(&fixtures().join("gta3_ps2.iso")).unwrap();
    let cat = catalog::build(&ps2).unwrap();
    let a = cat.assets.iter().find(|a| a.game_id == "STREAMED_SOUND_RADIO_HEAD").unwrap();
    assert_seek_exact(|| ef_audio::open::open(a, ps2.vfs.as_ref(), DecodeOptions::default()).unwrap().source, "PS2 VB");
}

#[test]
fn conversion_to_f32_is_symmetric() {
    let mut v = Vec::new();
    to_f32(ef_audio::SampleKind::I16, &[0x00, 0x80, 0xFF, 0x7F], &mut v);
    assert_eq!(v, [-1.0, 32767.0 / 32768.0]);
    let _ = ByteStream::open;
}

#[test]
fn unrecognised_folders_are_rejected() {
    let d = temp("empty");
    assert!(ef_core::open_and_detect(&d).is_err());
    std::fs::remove_dir_all(d).ok();
}
