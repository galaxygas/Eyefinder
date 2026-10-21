//! GTA2 PC style v700. DMA GBH Style 4.6 §§3–6,12 (gtamp.com/gta2/
//! gta2-style-sty-graphics-file-format/). Retail version confirmed by gta2_re
//! edb484d3b71d5cadaf3a9ea0e435f56993c747e7, gtx_0x106C.cpp LoadSty_5AB750.
use crate::style::{Palette, rectangle, texture};
use crate::{Error, collection::TextureCollection};
use std::collections::BTreeMap;
const BAD: Error = Error::Malformed("invalid or truncated GTA2 style");
fn word(b: &[u8], p: usize, n: usize) -> Result<usize, Error> {
    let bytes = b.get(p..p.checked_add(n).ok_or(BAD)?).ok_or(BAD)?;
    Ok(bytes.iter().enumerate().fold(0, |v, (i, b)| v | ((*b as usize) << (8 * i))))
}
pub fn parse(b: &[u8]) -> Result<TextureCollection, Error> {
    if b.get(..4) != Some(b"GBST") || word(b, 4, 2)? != 700 {
        return Err(Error::Unsupported("only retail GTA2 style v700 is documented here"));
    }
    let mut chunks = BTreeMap::new();
    let mut p = 6;
    while p < b.len() {
        let id = b.get(p..p + 4).ok_or(BAD)?;
        let size = word(b, p + 4, 4)?;
        p += 8;
        let end = p.checked_add(size).ok_or(BAD)?;
        if chunks.insert(id, b.get(p..end).ok_or(BAD)?).is_some() {
            return Err(Error::Malformed("duplicate GTA2 style chunk"));
        }
        p = end;
    }
    let get = |name: &[u8]| chunks.get(name).copied().ok_or(BAD);
    let tiles = get(b"TILE")?;
    let palette = get(b"PPAL")?;
    let index = get(b"PALX")?;
    let bases = get(b"PALB")?;
    if !tiles.len().is_multiple_of(4096)
        || !palette.len().is_multiple_of(65536)
        || index.len() != 32768
        || bases.len() != 16
    {
        return Err(BAD);
    }
    let pal = Palette { bytes: palette, index, g24: true };
    let mut out = TextureCollection { kind: "GTA2 STY", rw_version: 0, textures: vec![], problems: vec![] };
    for i in 0..tiles.len() / 4096 {
        let pixels = rectangle(tiles, (i / 4) * 16384 + (i % 4) * 64, 64, 64)?;
        out.textures.push(texture(format!("tile_{i:04}"), 64, 64, pal.rgba(&pixels, i, None)?));
    }
    let gfx = get(b"SPRG")?;
    let info = get(b"SPRX")?;
    if !info.len().is_multiple_of(8) {
        return Err(BAD);
    }
    let sprite_base = word(bases, 0, 2)?;
    for (i, row) in info.chunks_exact(8).enumerate() {
        let (w, h) = (row[4] as usize, row[5] as usize);
        let indices = rectangle(gfx, word(row, 0, 4)?, w, h)?;
        out.textures.push(texture(format!("sprite_{i:04}"), w, h, pal.rgba(&indices, sprite_base + i, None)?));
    }
    // Individual stored deltas are exported as complete modified sprites.
    if let Some(didx) = chunks.get(b"DELX".as_slice()) {
        let store = get(b"DELS")?;
        let mut dp = 0;
        let mut sp = 0usize;
        while dp < didx.len() {
            let sprite = word(didx, dp, 2)?;
            let n = word(didx, dp + 2, 1)?;
            dp += 4;
            let row = info.get(sprite * 8..sprite * 8 + 8).ok_or(BAD)?;
            let (w, h) = (row[4] as usize, row[5] as usize);
            let original = rectangle(gfx, word(row, 0, 4)?, w, h)?;
            for d in 0..n {
                let size = word(didx, dp, 2)?;
                dp += 2;
                let end = sp.checked_add(size).ok_or(BAD)?;
                let delta = store.get(sp..end).ok_or(BAD)?;
                sp = end;
                let mut pos = 0;
                let mut cursor = 0usize;
                let mut indices = original.clone();
                while pos < delta.len() {
                    cursor = cursor.checked_add(word(delta, pos, 2)?).ok_or(BAD)?;
                    let len = word(delta, pos + 2, 1)?;
                    pos += 3;
                    let run = delta.get(pos..pos + len).ok_or(BAD)?;
                    pos += len;
                    let (x, y) = (cursor % 256, cursor / 256);
                    if y >= h || x + len > w {
                        return Err(BAD);
                    }
                    indices[y * w + x..y * w + x + len].copy_from_slice(run);
                    cursor += len;
                }
                out.textures.push(texture(
                    format!("sprite_{sprite:04}_delta_{d:02}"),
                    w,
                    h,
                    pal.rgba(&indices, sprite_base + sprite, None)?,
                ));
            }
        }
        if sp != store.len() {
            return Err(Error::Malformed("unreferenced GTA2 delta bytes"));
        }
    }
    out.problems.push("STY: tiles, default sprites and individual stored deltas are included. Remap variants and combined delta states are not enumerated.".into());
    Ok(out)
}
