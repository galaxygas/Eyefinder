//! GXT text tables of GTA III and Vice City.
//!
//! Layout (per the re3 and reVC `CText::Load` implementations): a sequence of chunks
//! `{ char magic[4]; int32 size; u8 data[size] }`. The main table is the first `TKEY` chunk
//! (12-byte entries `{ u32 offset_in_TDAT; char key[8] }`) and the first `TDAT` chunk (UTF-16LE,
//! NUL-terminated strings). Vice City adds a `TABL` chunk (mission text offsets) before them,
//! which is skipped. San Andreas uses a different, hashed format and is not handled here.

use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct Gxt {
    entries: HashMap<String, String>,
}

impl Gxt {
    /// Parses a GTA III / Vice City GXT file. Returns `None` if it is not in that format.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let mut pos = 0usize;
        let (mut tkey, mut tdat): (Option<&[u8]>, Option<&[u8]>) = (None, None);
        while pos + 8 <= data.len() && (tkey.is_none() || tdat.is_none()) {
            let magic = &data[pos..pos + 4];
            let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().ok()?) as usize;
            let body = data.get(pos + 8..pos + 8 + size)?;
            match magic {
                b"TKEY" if tkey.is_none() => tkey = Some(body),
                b"TDAT" if tdat.is_none() => tdat = Some(body),
                b"TKEY" | b"TDAT" | b"TABL" => {}
                _ => return None,
            }
            pos += 8 + size;
        }
        let (tkey, tdat) = (tkey?, tdat?);
        let mut entries = HashMap::new();
        for e in tkey.chunks_exact(12) {
            let off = u32::from_le_bytes(e[0..4].try_into().ok()?) as usize;
            let key_end = e[4..12].iter().position(|&b| b == 0).unwrap_or(8);
            let key = String::from_utf8_lossy(&e[4..4 + key_end]).into_owned();
            let mut units = Vec::new();
            let mut p = off;
            while p + 2 <= tdat.len() {
                let u = u16::from_le_bytes([tdat[p], tdat[p + 1]]);
                if u == 0 {
                    break;
                }
                units.push(u);
                p += 2;
            }
            entries.insert(key, String::from_utf16_lossy(&units));
        }
        Some(Self { entries })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(with_tabl: bool) -> Vec<u8> {
        let items = [("FEA_FM0", "HEAD RADIO"), ("FEA_FM1", "DOUBLE CLEF FM")];
        let (mut tkey, mut tdat) = (Vec::new(), Vec::new());
        for (k, v) in items {
            tkey.extend((tdat.len() as u32).to_le_bytes());
            let mut kb = [0u8; 8];
            kb[..k.len()].copy_from_slice(k.as_bytes());
            tkey.extend(kb);
            for u in v.encode_utf16().chain([0]) {
                tdat.extend(u.to_le_bytes());
            }
        }
        let mut out = Vec::new();
        if with_tabl {
            out.extend(b"TABL");
            out.extend(12u32.to_le_bytes());
            out.extend(b"MAIN\0\0\0\0\0\0\0\0");
        }
        out.extend(b"TKEY");
        out.extend((tkey.len() as u32).to_le_bytes());
        out.extend(tkey);
        out.extend(b"TDAT");
        out.extend((tdat.len() as u32).to_le_bytes());
        out.extend(tdat);
        out
    }

    #[test]
    fn parses_iii_and_vc_layouts() {
        for t in [false, true] {
            let g = Gxt::parse(&build(t)).unwrap();
            assert_eq!(g.get("FEA_FM1"), Some("DOUBLE CLEF FM"));
            assert_eq!(g.len(), 2);
        }
        assert!(Gxt::parse(b"garbage!").is_none());
    }
}
