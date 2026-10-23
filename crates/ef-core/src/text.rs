//! Text tables of the 2D games: GTA1 / London `.FXT` and GTA2 `.GXT`.
//!
//! * FXT (GTA1, London 1969, London 1961): Daniel Marschall, "GTA 1 FXT Format"
//!   (misc.daniel-marschall.de/spiele/gta1/format_fxt.html and `GTA1/gta1_fxt_format.md` of
//!   danielmarschall/gta_lang_converter, Apache-2.0, commit 2a6e07e5), checked against that
//!   converter's `FxtToTxt`, which round-trips every official file bit for bit:
//!   * the first 8 bytes are masked: `v = ((b - K[i]) mod 256) + 1`, `K[0] = 100`,
//!     `K[i+1] = (2·K[i] - 1) mod 256`, then decoded like every other byte;
//!   * `0x01` is a line break; `0x84 x` is the Windows-1252 byte `x + 0xBF` (London) and
//!     `0xC4 x` is `x + 0x3F` (GTA1); `0xC3 x` and `0x00 x y` mark characters the game font
//!     lacks and carry `x - 1` (and `y - 1`); `0xE0..0xFF b0 b1` is a Shift-JIS character
//!     `0xZFFE - 0x40·(0xFF - b0) - (0xFF - b1)`; every other byte is `b - 1`;
//!   * the text is a sequence of `[key]text` entries ended by an empty key `[]`.
//!
//!   The result is Windows-1252 (Shift-JIS for the Japanese file). Shift-JIS characters are not
//!   converted: Eyefinder has no Shift-JIS table, so they are shown as `{SJIS:XXXX}`.
//! * GXT (GTA2): gta2_re `text_0x14::Load_5B5E90` (commit edb484d3) and Marschall's
//!   `GTA2/gta2_gxt_format.md`: a 6-byte header `"GBL" + language letter` and `u16` version 100,
//!   then chunks `{char type[4]; i32 size}`; `TKEY` holds 12-byte `{u32 offset; char key[8]}`
//!   records (offsets into `TDAT`), `TDAT` NUL-terminated UTF-16LE strings. For the European
//!   languages the code units are Unicode: gta2_re's `kExtendedWesternCharMap` sends each code
//!   point 0x80-0xFF to the glyph that Marschall's font chart shows for the same Latin-1 character.
//!   The Russian (`R`) and Japanese (`J`) files use other mappings; their code units above 0x7F
//!   are shown as `{U+XXXX}` (see `MISSING.md`).

use std::fmt::Write;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextTable {
    /// The language letter (GXT header) or the file name's language word (FXT).
    pub language: String,
    pub entries: Vec<(String, String)>,
    pub notes: Vec<String>,
}

/// Windows-1252 bytes 0x80-0x9F (WHATWG Encoding Standard, index windows-1252); 0 = unassigned.
const CP1252_80: [u16; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0, 0,
    0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

fn cp1252(b: u8, out: &mut String) {
    match b {
        0x80..=0x9F => match CP1252_80[(b - 0x80) as usize] {
            0 => {
                let _ = write!(out, "{{0x{b:02X}}}");
            }
            c => out.push(char::from_u32(c as u32).unwrap_or('\u{FFFD}')),
        },
        _ => out.push(b as char),
    }
}

/// Decodes a GTA1 / London FXT file.
pub fn parse_fxt(data: &[u8], language: &str) -> Result<TextTable, String> {
    let mut k = 0u8;
    let mut bytes = data.iter().enumerate().map(|(i, &b)| {
        if i < 8 {
            k = if i == 0 { 100 } else { k.wrapping_mul(2).wrapping_sub(1) };
            b.wrapping_sub(k).wrapping_add(1)
        } else {
            b
        }
    });
    let mut text = String::new();
    let mut sjis = 0usize;
    let mut marked = 0usize;
    while let Some(v) = bytes.next() {
        let mut next = || bytes.next().ok_or_else(|| "the file ends inside a character code".to_string());
        match v {
            0x00 => {
                marked += 1;
                let (a, b) = (next()?.wrapping_sub(1), next()?.wrapping_sub(1));
                let _ = write!(text, "{{0x{a:02X}{b:02X}}}");
            }
            0x01 => text.push('\n'),
            0x84 => cp1252(next()?.wrapping_add(0xBF), &mut text),
            0xC3 => {
                marked += 1;
                cp1252(next()?.wrapping_sub(1), &mut text);
            }
            0xC4 => cp1252(next()?.wrapping_add(0x3F), &mut text),
            0xE0..=0xFF => {
                let (b0, b1) = (next()? as u32, next()? as u32);
                let code = (((v as u32 & 0xF) << 12) + 0xFFE).wrapping_sub(0x40 * (0xFF - b0)).wrapping_sub(0xFF - b1);
                sjis += 1;
                let _ = write!(text, "{{SJIS:{:04X}}}", code & 0xFFFF);
            }
            _ => cp1252(v.wrapping_sub(1), &mut text),
        }
    }
    let mut table = TextTable { language: language.to_string(), ..Default::default() };
    let mut rest = text.as_str();
    let mut ended = false;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        let key = &after[..close];
        if key.is_empty() {
            ended = true;
            break;
        }
        let body = &after[close + 1..];
        // The entry runs to the next line that starts with '['.
        let end = body.find("\n[").map_or(body.len(), |e| e);
        table.entries.push((key.to_string(), body[..end].trim_end_matches('\n').to_string()));
        rest = &body[end..];
    }
    if !ended {
        return Err("no closing [] entry: not a GTA1 FXT file".into());
    }
    if sjis > 0 {
        table.notes.push(format!("{sjis} Japanese characters are shown as their Shift-JIS codes"));
    }
    if marked > 0 {
        table.notes.push(format!("{marked} characters are marked as missing from the game font (0x00 / 0xC3 codes)"));
    }
    Ok(table)
}

/// Decodes a GTA2 GXT file.
pub fn parse_gxt2(data: &[u8]) -> Result<TextTable, String> {
    if data.len() < 6 || &data[..3] != b"GBL" {
        return Err("not a GTA2 GXT file (no GBL header)".into());
    }
    let lang = data[3] as char;
    let version = u16::from_le_bytes([data[4], data[5]]);
    if version != 100 {
        return Err(format!("GXT version {version}; GTA2 checks for version 100"));
    }
    let (mut tkey, mut tdat) = (None, None);
    let mut pos = 6usize;
    while pos + 8 <= data.len() {
        let kind = &data[pos..pos + 4];
        let size = i32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap());
        let size = usize::try_from(size).map_err(|_| "negative chunk size".to_string())?;
        let body = data.get(pos + 8..pos + 8 + size).ok_or("truncated chunk")?;
        match kind {
            b"TKEY" => tkey = Some(body),
            b"TDAT" => tdat = Some(body),
            _ => {}
        }
        pos += 8 + size;
    }
    let (tkey, tdat) = (tkey.ok_or("no TKEY chunk")?, tdat.ok_or("no TDAT chunk")?);
    let western = matches!(lang.to_ascii_uppercase(), 'E' | 'F' | 'G' | 'I' | 'S');
    let mut table = TextTable { language: lang.to_string(), ..Default::default() };
    let (mut unmapped, mut prefixes) = (0usize, 0usize);
    for r in tkey.as_chunks::<12>().0 {
        let off = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        let key_end = r[4..12].iter().position(|&b| b == 0).unwrap_or(8);
        let key = String::from_utf8_lossy(&r[4..4 + key_end]).into_owned();
        let mut s = String::new();
        let mut p = off;
        loop {
            let Some(u) = tdat.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]])) else {
                return Err(format!("{key}: string runs past TDAT"));
            };
            if u == 0 {
                break;
            }
            // Real e.gxt / s.gxt (PC 2004 release, Dreamcast T40502D 93): every code unit of 0x100
            // or above is 0x2153-0x217A and starts its string. No source documents what these
            // prefixes select (MISSING.md), so they stay marked codes, never characters.
            if western && (0x2100..0x2200).contains(&u) && p == off {
                prefixes += 1;
                let _ = write!(s, "{{U+{u:04X}}}");
                p += 2;
                continue;
            }
            match char::from_u32(u as u32) {
                Some(c) if u < 0x80 || (western && u < 0x100) => s.push(c),
                _ => {
                    unmapped += 1;
                    let _ = write!(s, "{{U+{u:04X}}}");
                }
            }
            p += 2;
        }
        table.entries.push((key, s));
    }
    if prefixes > 0 {
        table
            .notes
            .push(format!("{prefixes} strings start with an undocumented prefix code (0x21xx), shown as {{U+21XX}}"));
    }
    if unmapped > 0 {
        table.notes.push(format!(
            "{unmapped} characters of language {lang} use a character mapping Eyefinder does not implement; shown as code units"
        ));
    }
    Ok(table)
}

/// Decodes a text file by its name: `.fxt` (GTA1/London) or `.gxt` with a `GBL` header (GTA2).
pub fn parse_named(name: &str, data: &[u8]) -> Option<Result<TextTable, String>> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.rsplit('/').next().unwrap_or(&lower);
    if let Some(s) = stem.strip_suffix(".fxt") {
        return Some(parse_fxt(data, s));
    }
    if stem.ends_with(".gxt") && data.starts_with(b"GBL") {
        return Some(parse_gxt2(data));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes plain Windows-1252 text the way the specification's inverse describes.
    fn encode_fxt(text: &[u8]) -> Vec<u8> {
        let mut k = 0u8;
        text.iter()
            .flat_map(|&c| match c {
                b'\n' => vec![0x01],
                0xC0..=0xFF => vec![0xC4, c - 0x3F],
                _ => vec![c + 1],
            })
            .enumerate()
            .map(|(i, b)| {
                if i < 8 {
                    k = if i == 0 { 100 } else { k.wrapping_mul(2).wrapping_sub(1) };
                    b.wrapping_sub(1).wrapping_add(k)
                } else {
                    b
                }
            })
            .collect()
    }

    #[test]
    fn fxt_header_mask_matches_the_libmagic_signature() {
        // file(1) magic for GTA1 FXT is "\xbf\xf8\xbd\x49\x62\xbe": "[1001]" masked.
        let f = encode_fxt(b"[1001]Hello\n[]");
        assert_eq!(f[..6], [0xBF, 0xF8, 0xBD, 0x49, 0x62, 0xBE]);
        let t = parse_fxt(&f, "english").unwrap();
        assert_eq!(t.entries, [("1001".to_string(), "Hello".to_string())]);
    }

    #[test]
    fn fxt_codes_and_entries() {
        let f = encode_fxt(b"[1]Caf\xe9\n[2]Two\nlines\n[]");
        let t = parse_fxt(&f, "english").unwrap();
        assert_eq!(t.entries[0], ("1".into(), "Caf\u{e9}".into()));
        assert_eq!(t.entries[1], ("2".into(), "Two\nlines".into()));
        // London's 0x84 prefix: 0x84 0x2A is 0x2A + 0xBF = 0xE9. (The codes are appended after
        // the 8 masked header bytes.)
        let mut l = encode_fxt(b"[1001]Caf");
        l.extend([0x84, 0x2A, 0x01, b'[' + 1, b']' + 1]);
        assert_eq!(parse_fxt(&l, "enguk").unwrap().entries[0].1, "Caf\u{e9}");
        // Shift-JIS: E9 8E 86 is 0x8345 (Marschall's worked example).
        let mut j = encode_fxt(b"[1001]Jp");
        j.extend([0xE9, 0x8E, 0x86, 0x01, b'[' + 1, b']' + 1]);
        assert_eq!(parse_fxt(&j, "japanese").unwrap().entries[0].1, "Jp{SJIS:8345}");
        assert!(parse_fxt(&encode_fxt(b"[1]no end"), "x").is_err());
    }

    #[test]
    fn gxt2_tables() {
        let mut tdat = Vec::new();
        let mut tkey = Vec::new();
        for (k, v) in [("credits", "Caf\u{e9}"), ("1001", "\u{2179}Hi \u{2179}")] {
            tkey.extend((tdat.len() as u32).to_le_bytes());
            let mut key = [0u8; 8];
            key[..k.len()].copy_from_slice(k.as_bytes());
            tkey.extend(key);
            tdat.extend(v.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        }
        let mut f = b"GBLE".to_vec();
        f.extend(100u16.to_le_bytes());
        for (id, body) in [(b"TKEY", &tkey), (b"TDAT", &tdat)] {
            f.extend(id);
            f.extend((body.len() as i32).to_le_bytes());
            f.extend(body);
        }
        let t = parse_gxt2(&f).unwrap();
        assert_eq!(t.language, "E");
        // a leading 0x21xx unit is a marked prefix code; elsewhere it is an unmapped character
        assert_eq!(t.entries, [("credits".into(), "Caf\u{e9}".into()), ("1001".into(), "{U+2179}Hi {U+2179}".into())]);
        assert_eq!(t.notes.len(), 2, "{:?}", t.notes);
        f[3] = b'R';
        let r = parse_gxt2(&f).unwrap();
        assert_eq!(r.entries[0].1, "Caf{U+00E9}");
        f[4] = 99;
        assert!(parse_gxt2(&f).is_err());
    }
}
