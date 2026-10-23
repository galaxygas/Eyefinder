//! Detection of the 2D games' releases through the CLI, on synthetic trees and images laid out
//! like the documented retail releases (Marschall's GTA1/London inventory, gta2_re's GTA2 paths,
//! PlayStation SYSTEM.CNF, Dreamcast GD-ROM sheets and IP.BIN, cartridge headers).
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("ef-legacy-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cli(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_eyefinder-cli")).arg(args[0]).arg(path).args(&args[1..]).output().unwrap()
}

fn ok(o: &Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn gta1_banks(root: &Path, dir: &str) {
    for bank in ["LEVEL000", "LEVEL001"] {
        for ext in ["SDT", "RAW"] {
            let src = repo().join(format!("crates/ef-audio/tests/fixtures/gta1_pc/GTADATA/AUDIO/{bank}.{ext}"));
            put(root, &format!("{dir}/{bank}.{ext}"), &std::fs::read(src).unwrap());
        }
    }
}

#[test]
fn gta1_with_both_london_packs_offers_each_game() {
    let root = temp("gta1-london");
    gta1_banks(&root, "GTADATA/AUDIO");
    gta1_banks(&root, "GTADATA/UK/AUDIO");
    for f in ["GTADATA/NYC.CMP", "GTADATA/MIAMI.CMP", "GTADATA/SANB.CMP", "GTADATA/ENGLISH.FXT"] {
        put(&root, f, b"x");
    }
    for f in ["GTADATA/UK/UK.CMP", "GTADATA/UK/ENGUK.FXT", "GTADATA/UK/MISSUK.INI"] {
        put(&root, f, b"x");
    }
    // London 1961 files are lower case on its download (Marschall inventory).
    for f in ["GTADATA/UK/uke.cmp", "GTADATA/UK/enguke.fxt", "GTADATA/UK/missuke.ini"] {
        put(&root, f, b"x");
    }
    let versions = ok(&cli(&["versions"], &root));
    for g in ["--game gta1 ", "--game london1969 ", "--game london1961 "] {
        assert!(versions.contains(g), "{versions}");
    }
    let refused = cli(&["info"], &root);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Choose which to add"));
    let info = ok(&cli(&["info", "--game", "london1961"], &root));
    assert!(info.starts_with("Grand Theft Auto: London 1961 (PC)"), "{info}");
    // London 1961 plays London 1969's banks.
    assert!(ok(&cli(&["list", "--game", "london1961"], &root)).contains("GTADATA/UK/AUDIO/LEVEL001.RAW"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn london_1969_and_gta2_are_detected_without_a_choice() {
    let root = temp("london69");
    for f in ["GTADATA/UK/UK.CMP", "GTADATA/UK/ENGUK.FXT", "GTADATA/UK/MISSUK.INI"] {
        put(&root, f, b"x");
    }
    gta1_banks(&root, "GTADATA/UK/AUDIO");
    assert!(ok(&cli(&["info"], &root)).starts_with("Grand Theft Auto: London 1969 (PC)"));
    std::fs::remove_dir_all(root).unwrap();

    // GTA2 as installed: lower-case names, a level bank but no frontend bank.
    let root = temp("gta2");
    put(&root, "gta2.exe", b"MZ");
    put(&root, "data/fstyle.sty", &std::fs::read(repo().join("crates/ef-texture/tests/fixtures/style.sty")).unwrap());
    put(&root, "data/e.gxt", b"GBLE\x64\x00");
    for ext in ["sdt", "raw"] {
        let src =
            repo().join(format!("crates/ef-audio/tests/fixtures/gta2_pc/DATA/AUDIO/FSTYLE.{}", ext.to_uppercase()));
        put(&root, &format!("data/audio/wil.{ext}"), &std::fs::read(src).unwrap());
    }
    let info = ok(&cli(&["info"], &root));
    assert!(info.starts_with("Grand Theft Auto 2 (PC)"), "{info}");
    assert!(info.contains("gta2.exe"), "{info}");
    assert!(ok(&cli(&["list"], &root)).contains("data/audio/wil.raw"));
    // A loose level bank alone is a GTA2 bank (24-byte records).
    let loose = temp("gta2-loose");
    std::fs::copy(root.join("data/audio/wil.sdt"), loose.join("wil.sdt")).unwrap();
    std::fs::copy(root.join("data/audio/wil.raw"), loose.join("wil.raw")).unwrap();
    assert!(ok(&cli(&["info"], &loose.join("wil.sdt"))).starts_with("Grand Theft Auto 2 (PC)"));
    std::fs::remove_dir_all(loose).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A flat ECMA-119 image whose sector numbers start at `base` (45000 for a GD-ROM track 3).
fn iso(files: &[(&str, &[u8])], base: u32, system_area: &[u8]) -> Vec<u8> {
    fn record(name: &[u8], sector: u32, size: u32, directory: bool) -> Vec<u8> {
        let mut r = vec![0; 33 + name.len() + (1 - name.len() % 2)];
        r[0] = r.len() as u8;
        r[2..6].copy_from_slice(&sector.to_le_bytes());
        r[6..10].copy_from_slice(&sector.to_be_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[14..18].copy_from_slice(&size.to_be_bytes());
        r[25] = if directory { 2 } else { 0 };
        r[32] = name.len() as u8;
        r[33..].copy_from_slice(&[name, &[0][..1 - name.len() % 2]].concat());
        r
    }
    let mut disc = vec![0u8; 19 * 2048];
    disc[..system_area.len()].copy_from_slice(system_area);
    let mut dir = record(b"\0", base + 18, 2048, true);
    dir.extend(record(b"\x01", base + 18, 2048, true));
    for (name, bytes) in files {
        let start = disc.len() as u32 / 2048;
        dir.extend(record(format!("{name};1").as_bytes(), base + start, bytes.len() as u32, false));
        disc.extend(*bytes);
        disc.resize(disc.len().div_ceil(2048) * 2048, 0);
    }
    disc[18 * 2048..18 * 2048 + dir.len()].copy_from_slice(&dir);
    let pvd = &mut disc[16 * 2048..17 * 2048];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    pvd[128..130].copy_from_slice(&2048u16.to_le_bytes());
    let root = record(b"\0", base + 18, 2048, true);
    pvd[156..156 + root.len()].copy_from_slice(&root);
    disc[17 * 2048] = 255;
    disc[17 * 2048 + 1..17 * 2048 + 6].copy_from_slice(b"CD001");
    disc
}

/// Mode 1 raw sectors (sync, mode byte 1, 2048 bytes of user data at 16).
fn raw_mode1(plain: &[u8]) -> Vec<u8> {
    let mut raw = Vec::new();
    for sector in plain.as_chunks::<2048>().0 {
        let mut block = [0; 2352];
        block[1..11].fill(255);
        block[15] = 1;
        block[16..2064].copy_from_slice(sector);
        raw.extend(block);
    }
    raw
}

#[test]
fn playstation_discs_are_identified_by_serial() {
    let root = temp("ps1");
    let gta2 =
        iso(&[("SYSTEM.CNF", b"BOOT = cdrom:\\SLUS_007.89;1\r\nTCB = 4\r\n"), ("SLUS_007.89", b"PS-X EXE")], 0, &[]);
    std::fs::write(root.join("gta2.bin"), raw_mode1(&gta2)).unwrap();
    std::fs::write(root.join("gta2.cue"), "FILE \"GTA2.BIN\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n")
        .unwrap();
    // The CUE names GTA2.BIN; the file is gta2.bin (case-insensitive track lookup).
    let info = ok(&cli(&["info"], &root.join("gta2.cue")));
    assert!(info.starts_with("Grand Theft Auto 2 (PlayStation), version: USA (SLUS-00789)"), "{info}");
    let list = ok(&cli(&["list"], &root.join("gta2.cue")));
    assert!(list.is_empty() || !list.contains("Implemented"), "{list}");
    // An unknown serial is offered only as an explicit choice.
    let other = iso(&[("SYSTEM.CNF", b"BOOT = cdrom:\\SLUS_999.99;1\r\n")], 0, &[]);
    std::fs::write(root.join("other.iso"), other).unwrap();
    let refused = cli(&["info"], &root.join("other.iso"));
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not in Eyefinder's table"));
    let chosen = ok(&cli(&["info", "--game", "gta1", "--platform", "ps1"], &root.join("other.iso")));
    assert!(chosen.starts_with("Grand Theft Auto (PlayStation)"), "{chosen}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dreamcast_gdi_and_cartridge_roms_are_identified() {
    let root = temp("dc");
    let mut ip = vec![b' '; 0x100];
    ip[..16].copy_from_slice(b"SEGA SEGAKATANA ");
    ip[0x40..0x48].copy_from_slice(b"T-42102N");
    ip[0x4A..0x50].copy_from_slice(b"V1.002");
    let track3 = iso(&[("1ST_READ.BIN", b"boot")], 45000, &ip);
    std::fs::write(root.join("track03.bin"), raw_mode1(&track3)).unwrap();
    std::fs::write(root.join("track01.bin"), vec![0u8; 2352 * 4]).unwrap();
    std::fs::write(root.join("track02.raw"), vec![0u8; 2352 * 4]).unwrap();
    std::fs::write(
        root.join("disc.gdi"),
        "3\n1 0 4 2352 track01.bin 0\n2 450 0 2352 track02.raw 0\n3 45000 4 2352 \"Track03.bin\" 0\n",
    )
    .unwrap();
    let info = ok(&cli(&["info"], &root.join("disc.gdi")));
    assert!(info.starts_with("Grand Theft Auto 2 (Dreamcast), version: USA (T-42102N)"), "{info}");
    std::fs::remove_dir_all(&root).unwrap();

    // A GBA ROM with a valid header that is not a No-Intro dump: recognised as GBA, game chosen.
    let root = temp("gba");
    let mut rom = vec![0u8; 0x400];
    rom[0xA0..0xAC].copy_from_slice(b"GTA ADVANCE\0");
    rom[0xAC..0xB0].copy_from_slice(b"BGTE");
    rom[0xB2] = 0x96;
    rom[0xBD] = rom[0xA0..=0xBC].iter().fold(0u8, |c, b| c.wrapping_sub(*b)).wrapping_sub(0x19);
    std::fs::write(root.join("gta.gba"), &rom).unwrap();
    let refused = cli(&["info"], &root.join("gta.gba"));
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Game Boy Advance release"));
    let chosen = ok(&cli(&["info", "--game", "advance", "--platform", "gba"], &root.join("gta.gba")));
    assert!(chosen.contains("game code BGTE"), "{chosen}");
    std::fs::remove_dir_all(root).unwrap();
}

/// Mode 2 Form 1 sectors: raw 2352 bytes (sync, header mode 2, 8-byte subheader, data at 24), or
/// the 2336 bytes after the sync and header (subheader, data at 8).
fn mode2(plain: &[u8], raw: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for sector in plain.as_chunks::<2048>().0 {
        let mut block = vec![0; 2352];
        block[1..11].fill(255);
        block[15] = 2;
        block[16..24].copy_from_slice(&[0, 0, 0x08, 0, 0, 0, 0x08, 0]);
        block[24..2072].copy_from_slice(sector);
        out.extend(if raw { &block[..] } else { &block[16..] });
    }
    out
}

/// A POPStarter VCD as cue2pops writes it (cue2pops.c @3f2be612: 0x100000-byte header with the
/// disc descriptors A0/A1/A2, the first track entry at 30 and the sector count at 1032 and 1036),
/// followed by the raw BIN.
fn vcd(bin: &[u8]) -> Vec<u8> {
    let mut h = vec![0u8; 0x100000];
    h[..30].copy_from_slice(&[
        0x41, 0, 0xA0, 0, 0, 0, 0, 0x01, 0x20, 0, 0x41, 0, 0xA1, 0, 0, 0, 0, 0x01, 0, 0, 0x41, 0, 0xA2, 0, 0, 0, 0,
        0x00, 0x04, 0x00,
    ]);
    h[30..40].copy_from_slice(&[0x41, 0, 0x01, 0, 0, 0, 0, 0, 0x02, 0]);
    let sectors = (bin.len() / 2352) as u32;
    h[1024..1028].copy_from_slice(b"kHn ");
    h[1032..1036].copy_from_slice(&sectors.to_le_bytes());
    h[1036..1040].copy_from_slice(&sectors.to_le_bytes());
    [h, bin.to_vec()].concat()
}

/// A DiscJuggler image laid out as DeXT's cdirip parser reads it (Flycast @fa9d39a3
/// `core/deps/chdpsr/cdipsr.cpp`): tracks back to back, then the session / track descriptor, then
/// `u32 version`, `u32 header_offset`. `sessions` holds `(mode, sector size code, pregap, start
/// LBA, sectors)` per track.
/// `(mode, sector size code, pregap, start LBA, sectors)`.
type CdiTrack = (u32, u32, u32, u32, Vec<u8>);

fn cdi(version: u32, sessions: &[Vec<CdiTrack>]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut desc = Vec::new();
    desc.extend((sessions.len() as u16).to_le_bytes());
    let mark = [0u8, 0, 0x01, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
    for tracks in sessions {
        desc.extend((tracks.len() as u16).to_le_bytes());
        for (mode, size_code, pregap, start, sectors) in tracks {
            let size = [2048usize, 2336, 2352][*size_code as usize];
            let length = (sectors.len() / size) as u32;
            data.extend(vec![0u8; *pregap as usize * size]);
            data.extend(sectors);
            desc.extend(0u32.to_le_bytes());
            desc.extend(mark);
            desc.extend(mark);
            desc.extend([0u8; 4]);
            desc.push(4);
            desc.extend(b"x.cd");
            desc.extend([0u8; 11 + 4 + 4]);
            desc.extend(0u32.to_le_bytes()); // not the DJ4 marker
            desc.extend([0u8; 2]);
            desc.extend(pregap.to_le_bytes());
            desc.extend(length.to_le_bytes());
            desc.extend([0u8; 6]);
            desc.extend(mode.to_le_bytes());
            desc.extend([0u8; 12]);
            desc.extend(start.to_le_bytes());
            desc.extend((pregap + length).to_le_bytes());
            desc.extend([0u8; 16]);
            desc.extend(size_code.to_le_bytes());
            desc.extend([0u8; 29]);
            if version != 0x8000_0004 {
                desc.extend([0u8; 5]);
                desc.extend(0u32.to_le_bytes());
            }
        }
        desc.extend(vec![0u8; if version == 0x8000_0004 { 12 } else { 13 }]);
    }
    let offset = if version == 0x8000_0006 { (desc.len() + 8) as u32 } else { data.len() as u32 };
    [data, desc, version.to_le_bytes().to_vec(), offset.to_le_bytes().to_vec()].concat()
}

#[test]
fn popstarter_vcd_and_discjuggler_cdi_images_are_read() {
    let root = temp("vcd-cdi");
    let gta2 =
        iso(&[("SYSTEM.CNF", b"BOOT = cdrom:\\SLUS_007.89;1\r\nTCB = 4\r\n"), ("SLUS_007.89", b"PS-X EXE")], 0, &[]);
    std::fs::write(root.join("gta2.vcd"), vcd(&mode2(&gta2, true))).unwrap();
    let info = ok(&cli(&["info"], &root.join("gta2.vcd")));
    assert!(info.starts_with("Grand Theft Auto 2 (PlayStation), version: USA (SLUS-00789)"), "{info}");
    let files = ok(&cli(&["files"], &root.join("gta2.vcd")));
    assert!(files.contains("8\tSLUS_007.89"), "{files}");
    // A file that only has the extension is refused with the reason.
    std::fs::write(root.join("fake.vcd"), vec![0u8; 0x100000 + 2352 * 20]).unwrap();
    let refused = cli(&["info"], &root.join("fake.vcd"));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("POPStarter"), "{refused:?}");

    // Dreamcast selfboot layout: an audio session, then a Mode 2 data session (2336-byte sectors)
    // whose first data sector is LBA start + pregap - 150 and whose ISO uses absolute sector numbers.
    let mut ip = vec![b' '; 0x100];
    ip[..16].copy_from_slice(b"SEGA SEGAKATANA ");
    ip[0x40..0x48].copy_from_slice(b"T-42102N");
    ip[0x4A..0x50].copy_from_slice(b"V1.002");
    let audio: Vec<u8> = (0..2352 * 3).map(|i| (i % 251) as u8).collect();
    for (version, size_code) in [(0x8000_0006u32, 1u32), (0x8000_0005, 2), (0x8000_0004, 1)] {
        let track = iso(&[("1ST_READ.BIN", b"boot"), ("MUSIC.STR", b"str")], 11702, &ip);
        let sectors = mode2(&track, size_code == 2);
        let image = cdi(version, &[vec![(0, 2, 150, 0, audio.clone())], vec![(2, size_code, 150, 11702, sectors)]]);
        let path = root.join(format!("gta2-{version:x}.cdi"));
        std::fs::write(&path, image).unwrap();
        let info = ok(&cli(&["info"], &path));
        assert!(info.starts_with("Grand Theft Auto 2 (Dreamcast), version: USA (T-42102N)"), "{version:x}: {info}");
        let files = ok(&cli(&["files"], &path));
        assert!(files.contains("3\tMUSIC.STR"), "{files}");
        assert!(files.contains(&format!("{}\tCD Audio/Track 01.cdda", audio.len())), "{files}");
    }
    std::fs::remove_dir_all(root).unwrap();
}
