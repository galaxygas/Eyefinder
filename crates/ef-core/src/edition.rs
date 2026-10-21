//! Which release of a game an installation is: PC executable version, or PS2 disc serial.
//!
//! The edition is shown to the user and recorded in export manifests. It does not change how any
//! data is decoded.
//!
//! * PC: DK22Pac plugin-sdk `shared/GameVersion.cpp` (commit 15f15b60) identifies each executable
//!   by one 32-bit value at a fixed virtual address of the loaded image. Eyefinder reads the same
//!   value from the executable file, mapping the address through the PE section table (Microsoft
//!   "PE Format": optional header ImageBase, section headers VirtualAddress / PointerToRawData).
//!   An executable that is still packed or encrypted on disk (for example by copy protection)
//!   matches none of them and is reported as unknown.
//! * PS2: the serial is the boot executable named by `BOOT2` in `SYSTEM.CNF` (PCSX2
//!   `CDVD.cpp` `ExecutablePathToSerial`), looked up in PCSX2 `bin/resources/GameIndex.yaml`
//!   (commit 5b6e6e23).

use std::io::{Read, Seek, SeekFrom};

use serde::{Deserialize, Serialize};

use crate::vfs::Vfs;
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

/// plugin-sdk `detect_game_id`, in its order (the first match wins).
const SA_CHECKS: &[ExeCheck] = &[
    ExeCheck { id: "pc-1.0-us", label: "1.0 US", va: 0x401000, value: 0x53EC8B55 },
    ExeCheck { id: "pc-1.0-us-hoodlum", label: "1.0 US (HOODLUM executable)", va: 0x401000, value: 0x16197BE9 },
    ExeCheck { id: "pc-1.0-eu", label: "1.0 EU", va: 0x8245BC, value: 0x94BF },
    ExeCheck { id: "pc-1.01-us", label: "1.01 US", va: 0x8252FC, value: 0x94BF },
    ExeCheck { id: "pc-1.01-eu", label: "1.01 EU", va: 0x82533C, value: 0x94BF },
    ExeCheck { id: "pc-steam", label: "Steam", va: 0x858D51, value: 0x3539F633 },
    ExeCheck { id: "pc-steam-lv", label: "Steam (low violence)", va: 0x858C61, value: 0x3539F633 },
];
const VC_CHECKS: &[ExeCheck] = &[
    ExeCheck { id: "pc-1.0", label: "1.0", va: 0x667BF0, value: 0x53E58955 },
    ExeCheck { id: "pc-1.1", label: "1.1", va: 0x667C40, value: 0x53E58955 },
    ExeCheck { id: "pc-steam", label: "Steam", va: 0x666BA0, value: 0x53E58955 },
];
const III_CHECKS: &[ExeCheck] = &[
    ExeCheck { id: "pc-1.0", label: "1.0", va: 0x5C1E70, value: 0x53E58955 },
    ExeCheck { id: "pc-1.1", label: "1.1", va: 0x5C2130, value: 0x53E58955 },
    ExeCheck { id: "pc-steam", label: "Steam", va: 0x5C6FD0, value: 0x53E58955 },
];

fn checks(game: Game) -> &'static [ExeCheck] {
    match game {
        Game::Gta3 => III_CHECKS,
        Game::ViceCity => VC_CHECKS,
        Game::SanAndreas => SA_CHECKS,
    }
}

/// Executable names: `gta3.exe`, `gta-vc.exe`, `gta_sa.exe` (retail) and `gta-sa.exe` (the Steam
/// release, Steam app 12120 launch configuration).
pub fn exe_names(game: Game) -> &'static [&'static str] {
    match game {
        Game::Gta3 => &["gta3.exe"],
        Game::ViceCity => &["gta-vc.exe"],
        Game::SanAndreas => &["gta_sa.exe", "gta-sa.exe"],
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
        Platform::Pc => v.extend(checks(game).iter().map(|c| Edition { id: c.id.into(), label: c.label.into() })),
        Platform::Ps2 => {
            v.extend(PS2_SERIALS.iter().filter(|s| s.1 == game).map(|(serial, _, region)| Edition {
                id: format!("ps2-{serial}"),
                label: format!("{region} ({serial})"),
            }))
        }
        Platform::Xbox | Platform::Unknown => {}
    }
    v
}

/// Maps a virtual address to a file offset through the PE section table.
fn pe_offset(head: &[u8], va: u32) -> Option<u64> {
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
    let sections = u16_at(coff + 2)? as usize;
    let opt_size = u16_at(coff + 16)? as usize;
    let opt = coff + 20;
    if u16_at(opt)? != 0x10B {
        return None; // the games are 32-bit (PE32) executables
    }
    let image_base = u32_at(opt + 28)?;
    let rva = va.checked_sub(image_base)?;
    for i in 0..sections {
        let s = opt + opt_size + i * 40;
        let (vsize, vaddr, raw_size, raw_ptr) = (u32_at(s + 8)?, u32_at(s + 12)?, u32_at(s + 16)?, u32_at(s + 20)?);
        if rva >= vaddr && rva < vaddr + vsize.max(raw_size) {
            let within = rva - vaddr;
            return if within + 4 <= raw_size { Some(raw_ptr as u64 + within as u64) } else { None };
        }
    }
    None
}

/// Identifies a PC executable. Returns the edition and the evidence line.
pub fn identify_exe(vfs: &dyn Vfs, game: Game) -> (Edition, String) {
    for name in exe_names(game) {
        let Ok(mut f) = vfs.open(name) else { continue };
        let mut head = vec![0u8; 4096];
        let n = f.read(&mut head).unwrap_or(0);
        head.truncate(n);
        for c in checks(game) {
            let Some(off) = pe_offset(&head, c.va) else { continue };
            let mut v = [0u8; 4];
            if f.seek(SeekFrom::Start(off)).is_ok() && f.read_exact(&mut v).is_ok() && u32::from_le_bytes(v) == c.value
            {
                return (
                    Edition { id: c.id.into(), label: c.label.into() },
                    format!("{name}: {:#010x} at {:#x}, as in the {} executable (plugin-sdk)", c.value, c.va, c.label),
                );
            }
        }
        return (
            Edition::unknown(),
            format!("{name} matches no known version; it may be modified, packed or encrypted on disk"),
        );
    }
    (Edition::unknown(), format!("no {} was found", exe_names(game).join(" or ")))
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

/// Detects the edition for an already-identified game and platform.
pub fn identify(vfs: &dyn Vfs, game: Game, platform: Platform) -> (Edition, String) {
    match platform {
        Platform::Pc => identify_exe(vfs, game),
        Platform::Ps2 => identify_ps2(vfs, game),
        Platform::Xbox | Platform::Unknown => {
            (Edition::unknown(), "no version identification for this platform".into())
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
        assert_eq!(identify_exe(&vfs, Game::SanAndreas).0.id, "pc-1.0-us-hoodlum");
        std::fs::write(dir.join("gta_sa.exe"), pe(1)).unwrap();
        assert!(identify_exe(&vfs, Game::SanAndreas).0.is_unknown());
        std::fs::remove_dir_all(dir).ok();
    }
}
