//! Which release of a game an installation is: PC executable build, or console disc serial.
//!
//! The edition is shown to the user and recorded in export manifests. It does not change how any
//! data is decoded.
//!
//! * PC: each executable build is identified by one 32-bit value at a fixed virtual address of the
//!   loaded image. Two sources are combined:
//!   * DK22Pac plugin-sdk `shared/GameVersion.cpp` (commit 15f15b60): III/VC 1.0, 1.1 and Steam,
//!     SA 1.0 US "compact" and "HOODLUM" (both at 0x401000), 1.0 EU, 1.01 US/EU, Steam and Steam
//!     low violence.
//!   * CookiePLMonster SilentPatch `SilentPatch/MemoryMgr.GTA.h` (MIT, commit 74976544): the same
//!     III/VC values, and for SA also 1.0 US (any), 3.0, "newsteam r1", "newsteam r2" (plugin-sdk's
//!     Steam value), "newsteam r2 lv" and the Rockstar Games Launcher build 1.0.22.0.
//!
//!   SilentPatch knows only three III/VC builds and its Rockstar Games Launcher announcement
//!   (silentsblog.com, 2019-09-22) says only San Andreas needed an update for the launcher, so the
//!   III/VC Steam value is labelled "Steam / Rockstar Games Launcher": the executable alone cannot
//!   tell the two stores apart. The store is identified from where the copy is installed
//!   ([`crate::store`]).
//!
//!   Eyefinder reads the value from the executable file, mapping the address through the PE section
//!   table (Microsoft "PE Format": optional header ImageBase, section headers VirtualAddress /
//!   PointerToRawData). An executable that is still packed or encrypted on disk (for example by
//!   copy protection) matches none of them and is reported as unknown. Every executable present
//!   is examined and reported; the edition is the one the store launches.
//!
//!   The `VS_FIXEDFILEINFO` file version from the executable's version resource (Microsoft
//!   "VS_FIXEDFILEINFO structure", "VS_VERSIONINFO structure", PE Format ".rsrc Section") is added
//!   as evidence. It is shown as stored and not mapped to a build.
//! * PS2: the serial is the boot executable named by `BOOT2` in `SYSTEM.CNF` (PCSX2
//!   `CDVD.cpp` `ExecutablePathToSerial`), looked up in PCSX2 `bin/resources/GameIndex.yaml`
//!   (commit 5b6e6e23).
//! * PS1, Dreamcast, Game Boy Color and Game Boy Advance: see [`crate::console`].

use std::io::{Read, Seek, SeekFrom};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::store::Store;
use crate::vfs::{RangeFile, Vfs};
use crate::{Game, Platform};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edition {
    /// Stable identifier, e.g. `pc-1.0`, `ps2-SLUS-20062`, `unknown`.
    pub id: String,
    /// Shown to the user, e.g. "1.0", "NTSC-U (SLUS-20062)".
    pub label: String,
}

impl Edition {
    pub fn unknown() -> Self {
        Self { id: "unknown".into(), label: "Unknown version".into() }
    }
    pub fn is_unknown(&self) -> bool {
        self.id == "unknown"
    }
}

struct ExeCheck {
    id: &'static str,
    label: &'static str,
    va: u32,
    value: u32,
}

/// San Andreas, most specific first: plugin-sdk's two 1.0 US variants at 0x401000, then
/// SilentPatch `InitializeVersions` order (`TryMatch_10`, `_11`, `_30`, newsteam r1, r2, r2 lv,
/// RGL). plugin-sdk's `GAME_STEAM` / `GAME_STEAM_LV` are SilentPatch's newsteam r2 / r2 lv.
const SA_CHECKS: &[ExeCheck] = &[
    ExeCheck { id: "pc-1.0-us", label: "1.0 US (compact executable)", va: 0x401000, value: 0x53EC8B55 },
    ExeCheck { id: "pc-1.0-us-hoodlum", label: "1.0 US (HOODLUM executable)", va: 0x401000, value: 0x16197BE9 },
    ExeCheck { id: "pc-1.0-us-other", label: "1.0 US", va: 0x82457C, value: 0x94BF },
    ExeCheck { id: "pc-1.0-eu", label: "1.0 EU", va: 0x8245BC, value: 0x94BF },
    ExeCheck { id: "pc-1.01-us", label: "1.01 US", va: 0x8252FC, value: 0x94BF },
    ExeCheck { id: "pc-1.01-eu", label: "1.01 EU", va: 0x82533C, value: 0x94BF },
    ExeCheck { id: "pc-3.0", label: "3.0 (first Steam release)", va: 0x85EC4A, value: 0x94BF },
    ExeCheck { id: "pc-steam-r1", label: "Steam update (newsteam r1)", va: 0x858D21, value: 0x3539F633 },
    ExeCheck { id: "pc-steam", label: "Steam update (newsteam r2)", va: 0x858D51, value: 0x3539F633 },
    ExeCheck {
        id: "pc-steam-lv",
        label: "Steam update, low violence (newsteam r2 lv)",
        va: 0x858C61,
        value: 0x3539F633,
    },
    ExeCheck { id: "pc-rgl", label: "Rockstar Games Launcher 1.0.22.0", va: 0x858501, value: 0x3539F633 },
];
const VC_CHECKS: &[ExeCheck] = &[
    ExeCheck { id: "pc-1.0", label: "1.0", va: 0x667BF0, value: 0x53E58955 },
    ExeCheck { id: "pc-1.1", label: "1.1", va: 0x667C40, value: 0x53E58955 },
    ExeCheck { id: "pc-steam", label: "Steam / Rockstar Games Launcher", va: 0x666BA0, value: 0x53E58955 },
];
const III_CHECKS: &[ExeCheck] = &[
    ExeCheck { id: "pc-1.0", label: "1.0", va: 0x5C1E70, value: 0x53E58955 },
    ExeCheck { id: "pc-1.1", label: "1.1", va: 0x5C2130, value: 0x53E58955 },
    ExeCheck { id: "pc-steam", label: "Steam / Rockstar Games Launcher", va: 0x5C6FD0, value: 0x53E58955 },
];

fn checks(game: Game) -> &'static [ExeCheck] {
    match game {
        Game::Gta3 => III_CHECKS,
        Game::ViceCity => VC_CHECKS,
        Game::SanAndreas => SA_CHECKS,
        _ => &[],
    }
}

/// Executable names, relative to the installation root.
///
/// * III/VC/SA: `gta3.exe`, `gta-vc.exe`, `gta_sa.exe` (retail, and the executable the Rockstar
///   Games Launcher starts: Galaxy-Plugin-Rockstar `game_cache.py` `launchEXE`, commit 5518f066)
///   and `gta-sa.exe` (the Steam release, Steam app 12120 launch configuration).
/// * GTA1 (Windows): `gtawin\gtawin.exe` (Classics / Steam: Daniel Marschall,
///   `spiele/gta1/patches.html`: "any GTA executable (gtawin.exe, gta_uk.exe etc.)") and
///   `GTAWIN\GRAND THEFT AUTO.EXE` (the retail disc: DiscImageCreator log of redump disc 33511,
///   and the maintainer's disc, docs/SUPPORT.md); DOS: `GTADOS\GTA24.EXE`, `GTA8.EXE`, `GTAFX.EXE`
///   (both discs).
/// * London 1969 / 1961: `gtawin\gta_uk.exe`, `gtawin\gta_61.exe` (Marschall, `misc.html`:
///   "gtawin\gta_uk.exe and gtawin\gta_61.exe create log files"); London 1969 DOS:
///   `GTADOS\GTA24_UK.EXE`, `GTA8_UK.EXE`, `GTAFX_UK.EXE` (the disc's file list on archive.org,
///   item `grandtheftautomissionpack1london1969pc`). That disc holds its Windows executable only
///   inside the installer's `SETUP\DATA1.CAB`.
/// * GTA2: `gta2.exe` (gta2_re `reccmp/96f-ascii.csv`, commit edb484d3).
pub fn exe_names(game: Game) -> &'static [&'static str] {
    match game {
        Game::Gta1 => &[
            "GTAWIN/GTAWIN.EXE",
            "GTAWIN/GRAND THEFT AUTO.EXE",
            "GTADOS/GTA24.EXE",
            "GTADOS/GTA8.EXE",
            "GTADOS/GTAFX.EXE",
        ],
        Game::London1969 => &["GTAWIN/GTA_UK.EXE", "GTADOS/GTA24_UK.EXE", "GTADOS/GTA8_UK.EXE", "GTADOS/GTAFX_UK.EXE"],
        Game::London1961 => &["GTAWIN/GTA_61.EXE"],
        Game::Gta2 => &["gta2.exe"],
        Game::Gta3 => &["gta3.exe"],
        Game::ViceCity => &["gta-vc.exe"],
        Game::SanAndreas => &["gta_sa.exe", "gta-sa.exe"],
        Game::GtaAdvance => &[],
    }
}

/// Executables identified by their size and SHA-1, from real files the maintainer reported
/// (docs/SUPPORT.md, "Real-file observations"): `(game, path, size, SHA-1, id, label)`. The label
/// says only what the files show.
const REAL_FILE_BUILDS: &[(Game, &str, u64, &str, &str, &str)] = &[
    (
        Game::Gta2,
        "gta2.exe",
        1_667_072,
        "646e1d479fa2aea12b3ff448fa23f6362f9e012f",
        "pc-9.6-2004",
        "9.6.0.0, Rockstar Games 2004 re-release",
    ),
    (
        Game::Gta1,
        "GTAWIN/GRAND THEFT AUTO.EXE",
        909_824,
        "038561860ce80eda794a491935d64878f925376e",
        "pc-retail-909824",
        "Retail disc, Windows executable of 909,824 bytes",
    ),
];

/// The executable each store starts, examined first.
fn launched_by(game: Game, store: &Store) -> Option<&'static str> {
    match (game, store) {
        (Game::SanAndreas, Store::Steam { .. }) => Some("gta-sa.exe"),
        (Game::SanAndreas, Store::RockstarLauncher) => Some("gta_sa.exe"),
        _ => None,
    }
}

/// PS2 serials (PCSX2 GameIndex.yaml names and regions).
const PS2_SERIALS: &[(&str, Game, &str)] = &[
    ("SLUS-20062", Game::Gta3, "NTSC-U"),
    ("SLES-50330", Game::Gta3, "PAL"),
    ("SLES-50793", Game::Gta3, "PAL"),
    ("SLPM-65382", Game::Gta3, "NTSC-J"),
    ("SLPM-65760", Game::Gta3, "NTSC-J, CapColle"),
    ("SLPM-66789", Game::Gta3, "NTSC-J, Best Price"),
    ("SLPM-55293", Game::Gta3, "NTSC-J, Rockstar Classics"),
    ("SLUS-20552", Game::ViceCity, "NTSC-U"),
    ("SLES-51061", Game::ViceCity, "PAL"),
    ("SLES-51316", Game::ViceCity, "PAL (Australia)"),
    ("SLES-51595", Game::ViceCity, "PAL"),
    ("SLPM-65488", Game::ViceCity, "NTSC-J"),
    ("SLPM-66034", Game::ViceCity, "NTSC-J, CapColle"),
    ("SLPM-66790", Game::ViceCity, "NTSC-J, Best Price"),
    ("SLPM-55291", Game::ViceCity, "NTSC-J, Rockstar Classics"),
    ("SLUS-20946", Game::SanAndreas, "NTSC-U"),
    ("SLES-52541", Game::SanAndreas, "PAL"),
    ("SLES-52927", Game::SanAndreas, "PAL (Germany)"),
    ("SLPM-65984", Game::SanAndreas, "NTSC-J"),
    ("SLPM-66788", Game::SanAndreas, "NTSC-J, Best Price"),
    ("SLPM-55092", Game::SanAndreas, "NTSC-J, Best Price"),
    ("SLPM-55292", Game::SanAndreas, "NTSC-J, Rockstar Classics"),
];

/// Every edition the user can pick for a game and platform (the detected one is among them).
pub fn known(game: Game, platform: Platform) -> Vec<Edition> {
    let mut v = vec![Edition::unknown()];
    match platform {
        Platform::Pc => {
            v.extend(checks(game).iter().map(|c| Edition { id: c.id.into(), label: c.label.into() }));
            v.extend(
                REAL_FILE_BUILDS.iter().filter(|b| b.0 == game).map(|b| Edition { id: b.4.into(), label: b.5.into() }),
            );
            v.extend(pc_discs(game).into_iter().map(|(id, rs)| Edition { label: disc_label(&rs), id }));
        }
        Platform::Ps2 => {
            v.extend(PS2_SERIALS.iter().filter(|s| s.1 == game).map(|(serial, _, region)| Edition {
                id: format!("ps2-{serial}"),
                label: format!("{region} ({serial})"),
            }))
        }
        Platform::Ps1 | Platform::Dreamcast | Platform::GameBoyColor | Platform::GameBoyAdvance => {
            v.extend(crate::console::known(game, platform))
        }
        Platform::Xbox | Platform::Unknown => {}
    }
    v
}

/// The parts of a PE32 header that are needed: image base, sections and the resource directory.
struct Pe {
    image_base: u32,
    /// `(virtual address, virtual size, raw size, raw pointer)`.
    sections: Vec<(u32, u32, u32, u32)>,
    /// Data directory 2 (resource table): `(rva, size)`.
    resources: Option<(u32, u32)>,
}

impl Pe {
    fn parse(head: &[u8]) -> Option<Pe> {
        let u16_at = |o: usize| head.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let u32_at = |o: usize| head.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        if head.get(0..2)? != b"MZ" {
            return None;
        }
        let pe = u32_at(0x3C)? as usize;
        if head.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let coff = pe + 4;
        let count = u16_at(coff + 2)? as usize;
        let opt_size = u16_at(coff + 16)? as usize;
        let opt = coff + 20;
        if u16_at(opt)? != 0x10B {
            return None; // the games are 32-bit (PE32) executables
        }
        let image_base = u32_at(opt + 28)?;
        // PE32: NumberOfRvaAndSizes at 92, data directories from 96, 8 bytes each.
        let resources = (u32_at(opt + 92)? > 2 && opt_size >= 96 + 24)
            .then(|| Some((u32_at(opt + 96 + 16)?, u32_at(opt + 96 + 20)?)))
            .flatten()
            .filter(|r| r.0 != 0);
        let mut sections = Vec::new();
        for i in 0..count {
            let s = opt + opt_size + i * 40;
            sections.push((u32_at(s + 12)?, u32_at(s + 8)?, u32_at(s + 16)?, u32_at(s + 20)?));
        }
        Some(Pe { image_base, sections, resources })
    }

    /// File offset of `len` bytes at a relative virtual address, if they are stored in the file.
    fn rva_offset(&self, rva: u32, len: u32) -> Option<u64> {
        for &(vaddr, vsize, raw_size, raw_ptr) in &self.sections {
            if rva >= vaddr && rva < vaddr.saturating_add(vsize.max(raw_size)) {
                let within = rva - vaddr;
                return (within.checked_add(len)? <= raw_size).then_some(raw_ptr as u64 + within as u64);
            }
        }
        None
    }

    fn va_offset(&self, va: u32) -> Option<u64> {
        self.rva_offset(va.checked_sub(self.image_base)?, 4)
    }
}

/// Maps a virtual address to a file offset through the PE section table.
#[cfg(test)]
fn pe_offset(head: &[u8], va: u32) -> Option<u64> {
    Pe::parse(head)?.va_offset(va)
}

/// `dwFileVersionMS`/`LS` of the `VS_FIXEDFILEINFO` in the first `RT_VERSION` (16) resource.
/// Resource directory layout: Microsoft "PE Format", ".rsrc Section" (16-byte directory tables,
/// 8-byte entries whose high bit marks a subdirectory, 16-byte data entries holding an RVA).
/// `VS_VERSIONINFO` starts with three `u16`s and the UTF-16 key `VS_VERSION_INFO`, then pads to
/// 32 bits; `VS_FIXEDFILEINFO` starts with the signature `0xFEEF04BD`.
fn file_version(f: &mut crate::vfs::RangeFile, pe: &Pe) -> Option<[u16; 4]> {
    let (rsrc_rva, rsrc_size) = pe.resources?;
    let base = pe.rva_offset(rsrc_rva, rsrc_size.min(16))?;
    let mut rd = |off: u64, n: usize| -> Option<Vec<u8>> {
        let mut b = vec![0; n];
        f.seek(SeekFrom::Start(off)).ok()?;
        f.read_exact(&mut b).ok()?;
        Some(b)
    };
    let le32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    // Returns the first entry of a directory table (with `id`, if given): its offset field.
    let mut entry = |dir: u32, id: Option<u32>| -> Option<u32> {
        let h = rd(base + dir as u64, 16)?;
        let n = u16::from_le_bytes([h[12], h[13]]) as usize + u16::from_le_bytes([h[14], h[15]]) as usize;
        let e = rd(base + dir as u64 + 16, n.min(4096) * 8)?;
        (0..n.min(4096))
            .map(|i| (le32(&e, i * 8), le32(&e, i * 8 + 4)))
            .find(|(name, _)| id.is_none_or(|id| *name == id))
            .map(|(_, off)| off)
    };
    let types = entry(0, Some(16))?;
    let names = entry(types & 0x7FFF_FFFF, None)?;
    let langs = entry(names & 0x7FFF_FFFF, None)?;
    if langs & 0x8000_0000 != 0 {
        return None;
    }
    let data = rd(base + langs as u64, 8)?;
    let (rva, size) = (le32(&data, 0), le32(&data, 4));
    let off = pe.rva_offset(rva, size.min(0x10000))?;
    let v = rd(off, (size as usize).min(0x10000))?;
    // 6 bytes of lengths/type, 32 bytes of "VS_VERSION_INFO\0" in UTF-16, padded to 4: 40.
    let key: Vec<u8> = "VS_VERSION_INFO\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
    if v.get(6..38)? != key.as_slice() || le32(v.get(40..44)?, 0) != 0xFEEF04BD {
        return None;
    }
    let (ms, ls) = (le32(v.get(48..52)?, 0), le32(v.get(52..56)?, 0));
    Some([(ms >> 16) as u16, ms as u16, (ls >> 16) as u16, ls as u16])
}

/// One executable's identification: the matching build, if any, and the evidence.
fn identify_one(vfs: &dyn Vfs, game: Game, name: &str) -> Option<(Option<Edition>, String)> {
    let mut f = vfs.open(name).ok()?;
    let mut head = vec![0u8; 4096];
    let n = f.read(&mut head).unwrap_or(0);
    head.truncate(n);
    if let Some(b) = REAL_FILE_BUILDS.iter().find(|b| b.0 == game && b.1.eq_ignore_ascii_case(name) && b.2 == f.len()) {
        let mut all = Vec::with_capacity(f.len() as usize);
        if f.seek(SeekFrom::Start(0)).is_ok() && f.read_to_end(&mut all).is_ok() {
            let digest: String = Sha1::digest(&all).iter().map(|x| format!("{x:02x}")).collect();
            if digest == b.3 {
                return Some((
                    Some(Edition { id: b.4.into(), label: b.5.into() }),
                    format!("{name}: SHA-1 {digest}, the \"{}\" executable of a real-file report", b.5),
                ));
            }
        }
    }
    let Some(pe) = Pe::parse(&head) else {
        let what = if head.starts_with(b"MZ") { "an MS-DOS executable" } else { "not a Windows executable" };
        return Some((None, format!("{name} is {what} ({} bytes); no build table exists for it", f.len())));
    };
    let version = file_version(&mut f, &pe)
        .map(|v| format!("; file version {}.{}.{}.{}", v[0], v[1], v[2], v[3]))
        .unwrap_or_default();
    for c in checks(game) {
        let Some(off) = pe.va_offset(c.va) else { continue };
        let mut v = [0u8; 4];
        if f.seek(SeekFrom::Start(off)).is_ok() && f.read_exact(&mut v).is_ok() && u32::from_le_bytes(v) == c.value {
            return Some((
                Some(Edition { id: c.id.into(), label: c.label.into() }),
                format!("{name}: {:#010x} at {:#x}, as in the {} executable{version}", c.value, c.va, c.label),
            ));
        }
    }
    let why = if checks(game).is_empty() {
        "no executable build table exists for this game".to_string()
    } else {
        "matches no known build; it may be modified, packed or encrypted on disk".to_string()
    };
    Some((None, format!("{name}: {why}{version}")))
}

/// Identifies the PC executables. Every executable present is examined and reported; the edition
/// is the one the store starts if that matched, otherwise the first that matched.
pub fn identify_exe(vfs: &dyn Vfs, game: Game, store: &Store) -> (Edition, Vec<String>) {
    let names = exe_names(game);
    if names.is_empty() {
        return (Edition::unknown(), vec!["no executable is known for this game".into()]);
    }
    let mut order: Vec<&str> = names.to_vec();
    if let Some(first) = launched_by(game, store) {
        order.sort_by_key(|n| !n.eq_ignore_ascii_case(first));
    }
    let mut evidence = Vec::new();
    let mut edition = None;
    for name in order {
        if let Some((e, why)) = identify_one(vfs, game, name) {
            evidence.push(why);
            if edition.is_none() {
                edition = e;
            }
        }
    }
    if evidence.is_empty() {
        evidence.push(format!("no {} was found", names.join(" or ")));
        if game == Game::London1969 && !vfs.dump_tracks().is_empty() {
            evidence.push("the London 1969 disc keeps its Windows executable inside SETUP/DATA1.CAB".into());
        }
    } else if evidence.len() > 1
        && let Some(first) = launched_by(game, store)
    {
        evidence.push(format!("{} starts {first}, so its build is the version shown", store.label()));
    }
    (edition.unwrap_or_else(Edition::unknown), evidence)
}

/// Serial from SYSTEM.CNF, e.g. `BOOT2 = cdrom0:\SLUS_200.62;1` -> `SLUS-20062`.
pub fn ps2_serial(cnf: &str) -> Option<String> {
    let line = cnf.lines().find(|l| l.trim_start().to_ascii_uppercase().starts_with("BOOT2"))?;
    let path = line.split_once('=')?.1.trim();
    let file = path.rsplit(['\\', '/', ':']).next()?;
    let file = file.split(';').next()?;
    let (prefix, digits) = file.split_once('_')?;
    let digits: String = digits.chars().filter(|c| *c != '.').collect();
    if prefix.len() != 4 || digits.len() != 5 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{digits}", prefix.to_ascii_uppercase()))
}

pub fn identify_ps2(vfs: &dyn Vfs, game: Game) -> (Edition, String) {
    let Ok(raw) = vfs.read_all("SYSTEM.CNF") else {
        return (Edition::unknown(), "SYSTEM.CNF could not be read".into());
    };
    let text = String::from_utf8_lossy(&raw);
    let Some(serial) = ps2_serial(&text) else {
        return (Edition::unknown(), "SYSTEM.CNF has no BOOT2 executable name".into());
    };
    match PS2_SERIALS.iter().find(|s| s.0 == serial) {
        Some((s, g, region)) if *g == game => (
            Edition { id: format!("ps2-{s}"), label: format!("{region} ({s})") },
            format!("SYSTEM.CNF boots {s} ({region} in the PCSX2 game database)"),
        ),
        Some((s, g, _)) => (Edition::unknown(), format!("SYSTEM.CNF boots {s}, which is listed for {}", g.name())),
        None => (Edition::unknown(), format!("SYSTEM.CNF boots {serial}, which is not in Eyefinder's serial list")),
    }
}

/// The redump PC disc releases of `game` in the release index, keyed by the SHA-1 of their first
/// track: `(edition id, label, releases)`.
fn pc_discs(game: Game) -> Vec<(String, Vec<&'static crate::index::Release>)> {
    let Some(g) = crate::index::index().of(game) else { return Vec::new() };
    let mut out: Vec<(String, Vec<&crate::index::Release>)> = Vec::new();
    for r in g.releases.iter().filter(|r| r.platform == "pc" && r.source == "redump-pc") {
        let Some(first) = r.files.iter().find(|f| f.name.ends_with(".bin") || f.name.ends_with(".iso")) else {
            continue;
        };
        let id = format!("pc-disc-{}", &first.sha1[..8]);
        match out.iter_mut().find(|(i, _)| *i == id) {
            Some((_, v)) => v.push(r),
            None => out.push((id, vec![r])),
        }
    }
    out
}

fn disc_label(releases: &[&crate::index::Release]) -> String {
    let labels: Vec<String> = releases.iter().map(|r| r.label()).collect();
    format!("{} (redump disc)", labels.join(" / "))
}

/// Whether dumped tracks `(bytes in the FILE, sectors of a generated PREGAP)` are redump's track
/// files of these sizes.
fn tracks_fit(redump: &[u64], dumped: &[(u64, u64)]) -> bool {
    redump.len() == dumped.len() && redump.iter().zip(dumped).all(|(&r, &(b, g))| r == b || r == b + g * 2352)
}

/// Identifies a PC BIN/CUE image by its tracks, against redump's IBM PC DAT in the release index:
/// every track's size must equal the redump track file's (which includes the track's pregap, so a
/// `PREGAP` the sheet generates is added back), then the data track's SHA-1 must equal redump's.
pub fn identify_pc_disc(vfs: &dyn Vfs, game: Game) -> Option<(Option<Edition>, Vec<String>)> {
    let tracks = vfs.dump_tracks();
    let data = tracks.first().filter(|t| !t.audio)?;
    let dumped: Vec<(u64, u64)> = tracks.iter().map(|t| (t.bytes, t.generated)).collect();
    let fits = |r: &crate::index::Release| {
        tracks_fit(&r.files.iter().filter(|f| f.name.ends_with(".bin")).map(|f| f.size).collect::<Vec<_>>(), &dumped)
    };
    let sized: Vec<(String, Vec<&crate::index::Release>)> = pc_discs(game)
        .into_iter()
        .map(|(id, rs)| (id, rs.into_iter().filter(|r| fits(r)).collect::<Vec<_>>()))
        .filter(|(_, rs)| !rs.is_empty())
        .collect();
    if sized.is_empty() {
        return Some((
            None,
            vec![format!("the disc's {} track sizes match no redump PC disc of {}", tracks.len(), game.name())],
        ));
    }
    let mut f = RangeFile::open(&crate::vfs::FileLocation {
        host: data.file.clone(),
        offset: data.offset,
        len: data.bytes,
        sectors: None,
    })
    .ok()?;
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let digest: String = h.finalize().iter().map(|x| format!("{x:02x}")).collect();
    let hit = sized.iter().find(|(_, rs)| rs[0].files.iter().any(|f| f.sha1 == digest));
    let mut ev = vec![format!(
        "track sizes match redump PC disc(s): {}",
        sized.iter().flat_map(|(_, rs)| rs.iter().map(|r| format!("\"{}\"", r.title))).collect::<Vec<_>>().join(", ")
    )];
    Some(match hit {
        Some((id, rs)) => {
            ev.push(format!("data track SHA-1 {digest} is redump's track 1 of {}", disc_label(rs)));
            if rs.iter().any(|r| r.files.iter().filter(|f| f.name.ends_with(".bin")).skip(1).any(|f| f.size == 0)) {
                ev.push("some audio tracks are empty in the DAT".into());
            }
            (Some(Edition { id: id.clone(), label: disc_label(rs) }), ev)
        }
        None => {
            ev.push(format!("but the data track's SHA-1 {digest} matches none of them (a different or modified disc)"));
            (None, ev)
        }
    })
}

/// Detects the edition for an already-identified game and platform.
pub fn identify(vfs: &dyn Vfs, game: Game, platform: Platform, store: &Store) -> (Edition, Vec<String>) {
    match platform {
        Platform::Pc => {
            let (mut edition, mut evidence) = identify_exe(vfs, game, store);
            if let Some((disc, why)) = identify_pc_disc(vfs, game) {
                evidence.extend(why);
                if let Some(d) = disc {
                    edition = d;
                }
            }
            (edition, evidence)
        }
        Platform::Ps2 => {
            let (e, why) = identify_ps2(vfs, game);
            (e, vec![why])
        }
        Platform::Ps1 | Platform::Dreamcast | Platform::GameBoyColor | Platform::GameBoyAdvance => {
            let (e, why) = crate::console::identify(vfs, game, platform);
            (e, vec![why])
        }
        Platform::Xbox | Platform::Unknown => {
            (Edition::unknown(), vec!["no version identification for this platform".into()])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serials_follow_the_pcsx2_rule() {
        assert_eq!(
            ps2_serial("BOOT2 = cdrom0:\\SLUS_200.62;1\r\nVER = 1.00\r\nVMODE = NTSC").as_deref(),
            Some("SLUS-20062")
        );
        assert_eq!(ps2_serial("boot2=cdrom0:\\SLES_525.41;1").as_deref(), Some("SLES-52541"));
        assert_eq!(ps2_serial("BOOT = cdrom:\\SLUS_200.62;1"), None);
    }

    /// A minimal PE32 image with one section mapping VA 0x401000 to file offset 0x400.
    fn pe(value_at_401000: u32) -> Vec<u8> {
        let mut f = vec![0u8; 0x800];
        f[0..2].copy_from_slice(b"MZ");
        f[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        f[0x86..0x88].copy_from_slice(&1u16.to_le_bytes()); // sections
        f[0x94..0x96].copy_from_slice(&0xE0u16.to_le_bytes()); // optional header size
        f[0x98..0x9A].copy_from_slice(&0x10Bu16.to_le_bytes());
        f[0x98 + 28..0x98 + 32].copy_from_slice(&0x400000u32.to_le_bytes());
        let s = 0x98 + 0xE0;
        f[s + 8..s + 12].copy_from_slice(&0x400u32.to_le_bytes()); // virtual size
        f[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // virtual address
        f[s + 16..s + 20].copy_from_slice(&0x400u32.to_le_bytes()); // raw size
        f[s + 20..s + 24].copy_from_slice(&0x400u32.to_le_bytes()); // raw pointer
        f[0x400..0x404].copy_from_slice(&value_at_401000.to_le_bytes());
        f
    }

    #[test]
    fn executables_are_identified_through_the_section_table() {
        assert_eq!(pe_offset(&pe(0), 0x401000), Some(0x400));
        assert_eq!(pe_offset(&pe(0), 0x900000), None);
        let dir = std::env::temp_dir().join(format!("ef-ed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("gta_sa.exe"), pe(0x16197BE9)).unwrap();
        let vfs = crate::vfs::FolderVfs::new(&dir);
        assert_eq!(identify_exe(&vfs, Game::SanAndreas, &Store::Unknown).0.id, "pc-1.0-us-hoodlum");
        std::fs::write(dir.join("gta_sa.exe"), pe(1)).unwrap();
        assert!(identify_exe(&vfs, Game::SanAndreas, &Store::Unknown).0.is_unknown());
        std::fs::remove_dir_all(dir).ok();
    }

    /// A PE32 image with a section holding `value` at `va`, and optionally a version resource.
    fn pe_at(va: u32, value: u32, version: Option<[u16; 4]>) -> Vec<u8> {
        let base = 0x400000u32;
        let page = (va - base) & !0xFFF;
        let mut f = vec![0u8; 0x2400];
        f[0..2].copy_from_slice(b"MZ");
        f[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        f[0x86..0x88].copy_from_slice(&2u16.to_le_bytes()); // sections
        f[0x94..0x96].copy_from_slice(&0xE0u16.to_le_bytes());
        let opt = 0x98;
        f[opt..opt + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        f[opt + 28..opt + 32].copy_from_slice(&base.to_le_bytes());
        f[opt + 92..opt + 96].copy_from_slice(&16u32.to_le_bytes());
        let section = |f: &mut Vec<u8>, i: usize, vaddr: u32, raw: u32| {
            let s = opt + 0xE0 + i * 40;
            f[s + 8..s + 12].copy_from_slice(&0x1000u32.to_le_bytes());
            f[s + 12..s + 16].copy_from_slice(&vaddr.to_le_bytes());
            f[s + 16..s + 20].copy_from_slice(&0x1000u32.to_le_bytes());
            f[s + 20..s + 24].copy_from_slice(&raw.to_le_bytes());
        };
        section(&mut f, 0, page, 0x400);
        let at = 0x400 + (va - base - page) as usize;
        f[at..at + 4].copy_from_slice(&value.to_le_bytes());
        if let Some(v) = version {
            // .rsrc at RVA 0x100000, file 0x1400: type 16 -> name 1 -> lang 0 -> data entry.
            let rva = 0x100000u32;
            section(&mut f, 1, rva, 0x1400);
            f[opt + 96 + 16..opt + 96 + 20].copy_from_slice(&rva.to_le_bytes());
            f[opt + 96 + 20..opt + 96 + 24].copy_from_slice(&0x1000u32.to_le_bytes());
            let r = 0x1400;
            let dir = |f: &mut Vec<u8>, at: usize, id: u32, next: u32| {
                f[at + 14..at + 16].copy_from_slice(&1u16.to_le_bytes());
                f[at + 16..at + 20].copy_from_slice(&id.to_le_bytes());
                f[at + 20..at + 24].copy_from_slice(&next.to_le_bytes());
            };
            dir(&mut f, r, 16, 0x8000_0018);
            dir(&mut f, r + 0x18, 1, 0x8000_0030);
            dir(&mut f, r + 0x30, 0x409, 0x48);
            let data = r + 0x48;
            f[data..data + 4].copy_from_slice(&(rva + 0x60).to_le_bytes());
            f[data + 4..data + 8].copy_from_slice(&0x5Cu32.to_le_bytes());
            let vi = r + 0x60;
            let key: Vec<u8> = "VS_VERSION_INFO\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
            f[vi + 6..vi + 38].copy_from_slice(&key);
            f[vi + 40..vi + 44].copy_from_slice(&0xFEEF04BDu32.to_le_bytes());
            f[vi + 48..vi + 52].copy_from_slice(&(((v[0] as u32) << 16) | v[1] as u32).to_le_bytes());
            f[vi + 52..vi + 56].copy_from_slice(&(((v[2] as u32) << 16) | v[3] as u32).to_le_bytes());
        }
        f
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ef-ed-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_san_andreas_build_is_identified() {
        let dir = temp("sa-builds");
        let vfs = crate::vfs::FolderVfs::new(&dir);
        for c in SA_CHECKS {
            std::fs::write(dir.join("gta_sa.exe"), pe_at(c.va, c.value, None)).unwrap();
            let (e, _) = identify_exe(&vfs, Game::SanAndreas, &Store::Unknown);
            // The two 0x401000 variants come first; any other 1.0 US value is "1.0 US".
            assert_eq!(e.id, c.id, "{}", c.label);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn steam_starts_gta_sa_dash_exe_even_beside_a_downgraded_exe() {
        // Issue #3: a Steam folder holding a downgraded 1.0 gta_sa.exe was reported as HOODLUM.
        let dir = temp("sa-steam");
        std::fs::write(dir.join("gta_sa.exe"), pe_at(0x401000, 0x16197BE9, None)).unwrap();
        std::fs::write(dir.join("gta-sa.exe"), pe_at(0x858D51, 0x3539F633, Some([1, 0, 0, 1]))).unwrap();
        let vfs = crate::vfs::FolderVfs::new(&dir);
        let (e, ev) = identify_exe(&vfs, Game::SanAndreas, &Store::Steam { app: 12120 });
        assert_eq!(e.id, "pc-steam");
        assert!(ev.iter().any(|l| l.starts_with("gta-sa.exe") && l.ends_with("file version 1.0.0.1")), "{ev:?}");
        assert!(ev.iter().any(|l| l.starts_with("gta_sa.exe") && l.contains("HOODLUM")), "{ev:?}");
        // The Rockstar Games Launcher starts gta_sa.exe.
        std::fs::write(dir.join("gta_sa.exe"), pe_at(0x858501, 0x3539F633, None)).unwrap();
        std::fs::remove_file(dir.join("gta-sa.exe")).unwrap();
        assert_eq!(identify_exe(&vfs, Game::SanAndreas, &Store::RockstarLauncher).0.id, "pc-rgl");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn iii_and_vc_steam_builds_name_both_stores() {
        let dir = temp("iii");
        std::fs::write(dir.join("gta3.exe"), pe_at(0x5C6FD0, 0x53E58955, None)).unwrap();
        let vfs = crate::vfs::FolderVfs::new(&dir);
        let (e, _) = identify_exe(&vfs, Game::Gta3, &Store::RockstarLauncher);
        assert_eq!(e.label, "Steam / Rockstar Games Launcher");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pc_disc_tracks_are_compared_with_redump_track_files() {
        // The maintainer's GTA1 disc (docs/SUPPORT.md): one BIN, track 2 with a PREGAP the sheet
        // generates. redump "Grand Theft Auto (USA) (En,Fr,De,It)" stores that pregap in its track 2.
        let redump = [
            99099168, 24877104, 78855504, 102845904, 102140304, 79913904, 116605104, 34049904, 92967504, 11117904,
            12705504,
        ];
        let mut dumped: Vec<(u64, u64)> = [
            99099168u64,
            24524304,
            78855504,
            102845904,
            102140304,
            79913904,
            116605104,
            34049904,
            92967504,
            11117904,
            12705504,
        ]
        .iter()
        .map(|&b| (b, 0))
        .collect();
        assert!(!tracks_fit(&redump, &dumped));
        dumped[1].1 = 150;
        assert!(tracks_fit(&redump, &dumped));
        // redump "Grand Theft Auto (Australia)" has the same data track but longer audio tracks.
        let australia = [
            99099168, 25058208, 79213008, 103203408, 102497808, 80271408, 116962608, 34407408, 93325008, 11475408,
            13058304,
        ];
        assert!(!tracks_fit(&australia, &dumped));
        assert!(!tracks_fit(&redump[..10], &dumped));
    }
}
