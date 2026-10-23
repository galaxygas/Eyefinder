//! Identification of console and handheld releases of the 2D games.
//!
//! * PlayStation: `SYSTEM.CNF` line `BOOT = cdrom:\SLUS_001.06;1` names the boot executable; its
//!   name is the serial with `-` replaced by `_` and a dot after the third digit (psx-spx, "CDROM
//!   File Playstation EXE and SYSTEM.CNF"). PlayStation 2 discs use `BOOT2` instead
//!   ([`crate::edition::ps2_serial`]).
//! * Dreamcast: the disc's system area (ECMA-119 sectors 0-15 of the high-density data track) is
//!   `IP.BIN`. Its fields, from KallistiOS makeip `src/field.c` (commit 146b310e): hardware ID
//!   `"SEGA SEGAKATANA "` at 0x00 (16 bytes), product number at 0x40 (10 bytes), version at 0x4A
//!   (6 bytes), release date at 0x50 (16 bytes, `YYYYMMDD` then spaces), game title at 0x80 (128
//!   bytes), space padded. The product number is the disc's internal serial, which need not be
//!   the serial printed on the box (redump disc 19717, "Internal Serial : T40502D 93" for the
//!   T-40502D-18 / -61 discs), so discs are matched on the product number, version and date that
//!   redump shows for each disc.
//! * Game Boy Color: the cartridge header at 0x100-0x14F (Pan Docs, "The Cartridge Header":
//!   title at 0x134, CGB flag at 0x143, header checksum at 0x14D over 0x134-0x14C).
//! * Game Boy Advance: the cartridge header (GBATEK, "GBA Cartridge Header": title at 0xA0, 12
//!   bytes; game code at 0xAC, 4 bytes; fixed value 0x96 at 0xB2; complement check at 0xBD over
//!   0xA0-0xBC).
//!
//! Releases come from the release index ([`crate::index`], redump and No-Intro DATs). PlayStation
//! discs are matched by serial, Dreamcast discs by IP.BIN fields, ROMs by SHA-1.
//! None of these platforms' asset formats are decoded; see `MISSING.md`.

use std::io::Read;

use sha1::{Digest, Sha1};

use crate::edition::Edition;
use crate::index::{self, IndexGame, Release};
use crate::vfs::Vfs;
use crate::{Game, Platform};

/// PlayStation serial from SYSTEM.CNF, e.g. `BOOT = cdrom:\SLUS_001.06;1` -> `SLUS-00106`.
pub fn ps1_serial(cnf: &str) -> Option<String> {
    let line = cnf.lines().find(|l| l.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("BOOT")))?;
    let path = line.split_once('=')?.1.trim();
    let path = path.split_whitespace().next()?; // an optional argument may follow
    let file = path.rsplit(['\\', '/', ':']).next()?;
    let file = file.split(';').next()?;
    let (prefix, digits) = file.split_once('_')?;
    let digits: String = digits.chars().filter(|c| *c != '.').collect();
    if prefix.len() != 4 || digits.len() != 5 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{digits}", prefix.to_ascii_uppercase()))
}

/// Whether `SYSTEM.CNF` boots a PlayStation (not PlayStation 2) executable.
pub fn is_ps1_config(cnf: &str) -> bool {
    cnf.lines().any(|l| l.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("BOOT")))
}

/// The `IP.BIN` fields Eyefinder reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpBin {
    pub product: String,
    pub version: String,
    /// `YYYYMMDD`.
    pub date: String,
    pub title: String,
}

/// `IP.BIN` fields, if the hardware ID is a Dreamcast's.
pub fn ip_bin(system_area: &[u8]) -> Option<IpBin> {
    if system_area.get(..16)? != b"SEGA SEGAKATANA " {
        return None;
    }
    let field = |a: usize, n: usize| -> Option<String> {
        Some(String::from_utf8_lossy(system_area.get(a..a + n)?).trim().to_string())
    };
    Some(IpBin {
        product: field(0x40, 10)?,
        version: field(0x4A, 6)?,
        date: field(0x50, 16)?,
        title: field(0x80, 128)?,
    })
}

/// The ROM kind of a cartridge image, from its header.
pub fn rom_platform(head: &[u8]) -> Option<Platform> {
    // GBA: fixed 0x96 at 0xB2 and the header complement check.
    if head.len() >= 0xC0 && head[0xB2] == 0x96 {
        let sum = head[0xA0..=0xBC].iter().fold(0u8, |c, b| c.wrapping_sub(*b)).wrapping_sub(0x19);
        if sum == head[0xBD] {
            return Some(Platform::GameBoyAdvance);
        }
    }
    // GB/GBC: header checksum x = x - byte - 1 over 0x134..=0x14C; CGB flag 0x80 or 0xC0.
    if head.len() >= 0x150 {
        let sum = head[0x134..=0x14C].iter().fold(0u8, |x, b| x.wrapping_sub(*b).wrapping_sub(1));
        if sum == head[0x14D] && matches!(head[0x143], 0x80 | 0xC0) {
            return Some(Platform::GameBoyColor);
        }
    }
    None
}

/// The single file of a ROM VFS ([`crate::vfs::SingleFileVfs`]).
fn rom_file(vfs: &dyn Vfs) -> Option<String> {
    let list = vfs.list("")?;
    let [only] = list.as_slice() else { return None };
    (!only.is_dir).then(|| only.name.clone())
}

/// What a console disc or ROM is: `(game, platform, edition, evidence)`. `game` is `None` when the
/// platform is recognised but the release is not in the table.
pub fn detect(vfs: &dyn Vfs) -> Option<(Option<Game>, Platform, Edition, Vec<String>)> {
    let mut found = detect_release(vfs)?;
    if let Some(c) = vfs.container() {
        found.3.insert(0, format!("image container: {c}"));
    }
    Some(found)
}

fn detect_release(vfs: &dyn Vfs) -> Option<(Option<Game>, Platform, Edition, Vec<String>)> {
    if let Ok(cnf) = vfs.read_all("SYSTEM.CNF") {
        let cnf = String::from_utf8_lossy(&cnf);
        if is_ps1_config(&cnf) {
            let mut ev = vec!["SYSTEM.CNF has a BOOT line (PlayStation boot file)".to_string()];
            let Some(serial) = ps1_serial(&cnf) else {
                ev.push("its boot executable name does not follow the serial convention".into());
                return Some((None, Platform::Ps1, Edition::unknown(), ev));
            };
            return Some(match disc(Platform::Ps1, &serial) {
                Some((g, e, why)) => {
                    ev.push(why);
                    (Some(g), Platform::Ps1, e, ev)
                }
                None => {
                    ev.push(format!("SYSTEM.CNF boots {serial}, which is not a known release of a 2D GTA game"));
                    (None, Platform::Ps1, Edition::unknown(), ev)
                }
            });
        }
    }
    if let Some(ip) = vfs.system_area().as_deref().and_then(ip_bin) {
        let mut ev = vec![format!(
            "IP.BIN hardware ID SEGA SEGAKATANA (Dreamcast); product number {}, version {}, date {}, title \"{}\"",
            ip.product, ip.version, ip.date, ip.title
        )];
        let found = index::index().by_ip_bin(&ip.product, &ip.version, &ip.date);
        return Some(match edition_of(Platform::Dreamcast, &found) {
            Some((g, e)) => {
                let source = found[0].1.ip_bin.as_ref().map_or("", |i| i.source.as_str());
                let url = index::index().sources.get(source).map_or("", |s| s.url.as_str());
                ev.push(format!("these IP.BIN fields are the redump disc {url}: {}", titles(&found)));
                (Some(g), Platform::Dreamcast, e, ev)
            }
            None => {
                ev.push(format!(
                    "no Dreamcast disc in the release index has this product number, version and date ({} {} {})",
                    ip.product, ip.version, ip.date
                ));
                (None, Platform::Dreamcast, Edition::unknown(), ev)
            }
        });
    }
    let name = rom_file(vfs)?;
    let mut f = vfs.open(&name).ok()?;
    let mut head = vec![0u8; 0x150];
    f.read_exact(&mut head).ok()?;
    let platform = rom_platform(&head)?;
    let mut ev = vec![format!("{name} has a valid {} cartridge header", platform.name())];
    if platform == Platform::GameBoyAdvance {
        ev.push(format!(
            "header title \"{}\", game code {}",
            String::from_utf8_lossy(&head[0xA0..0xAC]).trim_end_matches('\0'),
            String::from_utf8_lossy(&head[0xAC..0xB0])
        ));
    } else {
        ev.push(format!("header title \"{}\"", String::from_utf8_lossy(&head[0x134..0x143]).trim_end_matches('\0')));
    }
    let digest = {
        let mut f = vfs.open(&name).ok()?;
        let mut h = Sha1::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let pid = index::platform_id(platform);
    let found: Vec<_> = index::index().by_sha1(&digest).into_iter().filter(|(_, r)| r.platform == pid).collect();
    match found.first().and_then(|(g, _)| g.game()) {
        Some(game) => {
            ev.push(format!("SHA-1 {digest} is the No-Intro dump \"{}\"", found[0].1.title));
            Some((Some(game), platform, Edition { id: format!("rom-{}", &digest[..8]), label: found[0].1.label() }, ev))
        }
        None => {
            ev.push(format!("SHA-1 {digest} matches no No-Intro dump of a GTA game"));
            Some((None, platform, Edition::unknown(), ev))
        }
    }
}

fn titles(found: &[(&IndexGame, &Release)]) -> String {
    let mut v: Vec<String> = Vec::new();
    for (_, r) in found {
        let t = format!("\"{}\" ({})", r.title, r.serial);
        if !v.contains(&t) {
            v.push(t);
        }
    }
    v.join(", ")
}

/// The game and edition of matching disc releases (one game; the edition names every serial).
fn edition_of(platform: Platform, found: &[(&IndexGame, &Release)]) -> Option<(Game, Edition)> {
    let game = found.first()?.0.game()?;
    if found.iter().any(|(g, _)| g.game() != Some(game)) {
        return None;
    }
    let mut unique: Vec<String> = Vec::new();
    for s in found.iter().flat_map(|(_, r)| index::serials(&r.serial, platform)) {
        if !unique.contains(&s) {
            unique.push(s);
        }
    }
    let first = unique.first().map_or("", |s| s.as_str());
    Some((
        game,
        Edition {
            id: format!("{}-{first}", platform.short().to_ascii_lowercase()),
            label: format!("{} ({})", found[0].1.label(), unique.join(" / ")),
        },
    ))
}

fn disc(platform: Platform, serial: &str) -> Option<(Game, Edition, String)> {
    let found = index::index().by_serial(platform, serial);
    let (game, edition) = edition_of(platform, &found)?;
    Some((game, edition, format!("{serial} is the redump release {}", titles(&found))))
}

/// The editions of a game on a console platform.
pub fn known(game: Game, platform: Platform) -> Vec<Edition> {
    let pid = index::platform_id(platform);
    let Some(g) = index::index().of(game) else { return Vec::new() };
    let mut v: Vec<Edition> = Vec::new();
    for r in g.releases.iter().filter(|r| r.platform == pid) {
        let e = if r.media == "rom" {
            r.files.first().map(|f| Edition { id: format!("rom-{}", &f.sha1[..8]), label: r.label() })
        } else {
            let s = index::serials(&r.serial, platform);
            s.first().and_then(|s| disc(platform, s)).map(|x| x.1)
        };
        if let Some(e) = e.filter(|e| !v.iter().any(|x| x.id == e.id)) {
            v.push(e);
        }
    }
    v
}

/// The edition of an already-identified console release.
pub fn identify(vfs: &dyn Vfs, game: Game, platform: Platform) -> (Edition, String) {
    match detect(vfs) {
        Some((Some(g), p, e, ev)) if g == game && p == platform => (e, ev.last().cloned().unwrap_or_default()),
        Some((_, _, _, ev)) => (Edition::unknown(), ev.last().cloned().unwrap_or_default()),
        None => (Edition::unknown(), format!("no {} identification data was found", platform.name())),
    }
}

/// The games Eyefinder reads that were released on a console platform (the choices for an
/// unrecognised release).
pub fn games_on(platform: Platform) -> Vec<Game> {
    let pid = index::platform_id(platform);
    let mut out = Vec::new();
    for (g, _) in index::index().releases().filter(|(_, r)| r.platform == pid) {
        if let Some(game) = g.game().filter(|game| game.is_2d() && !out.contains(game)) {
            out.push(game);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps1_serials_follow_the_boot_file_convention() {
        assert_eq!(ps1_serial("BOOT = cdrom:\\SLUS_001.06;1\r\nTCB = 4").as_deref(), Some("SLUS-00106"));
        assert_eq!(ps1_serial("BOOT=cdrom:\\EXE\\SLES_000.32;1 arg").as_deref(), Some("SLES-00032"));
        assert_eq!(ps1_serial("BOOT2 = cdrom0:\\SLUS_200.62;1"), None);
        assert!(is_ps1_config("BOOT = cdrom:\\SLUS_007.89;1"));
        assert!(!is_ps1_config("BOOT2 = cdrom0:\\SLUS_200.62;1"));
        assert_eq!(disc(Platform::Ps1, "SLUS-00789").unwrap().0, Game::Gta2);
    }

    #[test]
    fn dreamcast_ip_bin_fields() {
        let mut ip = vec![b' '; 0x100];
        ip[..16].copy_from_slice(b"SEGA SEGAKATANA ");
        ip[0x40..0x47].copy_from_slice(b"T42102N");
        ip[0x4A..0x50].copy_from_slice(b"V1.002");
        ip[0x50..0x58].copy_from_slice(b"20000425");
        ip[0x80..0x84].copy_from_slice(b"GTA2");
        let i = ip_bin(&ip).unwrap();
        assert_eq!((i.product.as_str(), i.version.as_str(), i.date.as_str()), ("T42102N", "V1.002", "20000425"));
        let found = index::index().by_ip_bin(&i.product, &i.version, &i.date);
        assert_eq!(edition_of(Platform::Dreamcast, &found).unwrap().1.id, "dc-T-42102N");
        ip[0] = b'X';
        assert!(ip_bin(&ip).is_none());
    }

    #[test]
    fn cartridge_headers() {
        let mut gba = vec![0u8; 0x150];
        gba[0xA0..0xAC].copy_from_slice(b"GTA ADVANCE\0");
        gba[0xAC..0xB0].copy_from_slice(b"BGTE");
        gba[0xB2] = 0x96;
        gba[0xBD] = gba[0xA0..=0xBC].iter().fold(0u8, |c, b| c.wrapping_sub(*b)).wrapping_sub(0x19);
        assert_eq!(rom_platform(&gba), Some(Platform::GameBoyAdvance));
        let mut gbc = vec![0u8; 0x150];
        gbc[0x134..0x13A].copy_from_slice(b"GTA2  ");
        gbc[0x143] = 0x80;
        gbc[0x14D] = gbc[0x134..=0x14C].iter().fold(0u8, |x, b| x.wrapping_sub(*b).wrapping_sub(1));
        assert_eq!(rom_platform(&gbc), Some(Platform::GameBoyColor));
        gbc[0x14D] ^= 1;
        assert_eq!(rom_platform(&gbc), None);
    }
}
