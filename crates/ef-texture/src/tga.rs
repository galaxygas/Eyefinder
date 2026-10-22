//! Truevision TGA images (GTA2 `data/frontend/*.tga`, opened by name in gta2_re
//! `Frontend.cpp`, commit edb484d3).
//!
//! Layout: Truevision Inc., "Truevision TGA File Format Specification, Version 2.0" (1991):
//! an 18-byte header `{u8 id_length; u8 colour_map_type; u8 image_type; u16 cmap_first;
//! u16 cmap_length; u8 cmap_entry_bits; u16 x_origin; u16 y_origin; u16 width; u16 height;
//! u8 pixel_depth; u8 descriptor}`, the image ID, the colour map, then the pixels.
//! * Image types 1 / 2 / 3 are colour-mapped / true-colour / black-and-white; 9 / 10 / 11 are
//!   the same, run-length encoded (packets: a header byte whose top bit marks a run and whose low
//!   7 bits are the count minus one).
//! * Colours are stored B, G, R(, A); 15/16-bit values are `ARRRRRGG GGGBBBBB`, little-endian.
//! * Descriptor bits 0-3 give the number of attribute (alpha) bits, bit 4 right-to-left and
//!   bit 5 top-to-bottom pixel order (the default origin is the lower left).
//!
//! Pixels without alpha bits are opaque. 5-bit channels are scaled like every other format
//! (`docs/FORMATS.md`, "Colour conversion"). The extension area is not read.
use crate::{Error, collection::TextureCollection};

fn scale5(v: u16) -> u8 {
    ((v as u32 * 255 * 2 + 31) / 62) as u8
}

/// One colour of `bits` bits at `b`, as RGBA; `alpha` is whether the attribute bits are alpha.
fn colour(b: &[u8], bits: u8, alpha: bool) -> Result<[u8; 4], Error> {
    Ok(match bits {
        8 => [b[0], b[0], b[0], 255],
        15 | 16 => {
            let v = u16::from_le_bytes([b[0], b[1]]);
            let a = if bits == 16 && alpha { if v & 0x8000 != 0 { 255 } else { 0 } } else { 255 };
            [scale5(v >> 10 & 31), scale5(v >> 5 & 31), scale5(v & 31), a]
        }
        24 => [b[2], b[1], b[0], 255],
        32 => [b[2], b[1], b[0], if alpha { b[3] } else { 255 }],
        _ => return Err(Error::Malformed("unsupported TGA colour depth")),
    })
}

pub fn parse(bytes: &[u8], name: &str) -> Result<TextureCollection, Error> {
    let h = bytes.get(..18).ok_or(Error::Truncated("TGA header"))?;
    let u16_at = |o: usize| u16::from_le_bytes([h[o], h[o + 1]]);
    let (id_len, cmap_type, kind) = (h[0] as usize, h[1], h[2]);
    let (cmap_first, cmap_len, cmap_bits) = (u16_at(3) as usize, u16_at(5) as usize, h[7]);
    let (w, hgt, depth, desc) = (u16_at(12) as usize, u16_at(14) as usize, h[16], h[17]);
    let alpha_bits = desc & 0x0F;
    let (right_to_left, top_to_bottom) = (desc & 0x10 != 0, desc & 0x20 != 0);
    let mapped = matches!(kind, 1 | 9);
    if !matches!(kind, 1 | 2 | 3 | 9 | 10 | 11) {
        return Err(Error::Malformed("TGA image type is not 1, 2, 3, 9, 10 or 11"));
    }
    if w == 0 || hgt == 0 {
        return Err(Error::Malformed("TGA image has no pixels"));
    }
    if (mapped && (cmap_type != 1 || depth != 8)) || (matches!(kind, 3 | 11) && depth != 8) {
        return Err(Error::Malformed("TGA colour map / depth combination is not valid"));
    }
    if !matches!(depth, 8 | 15 | 16 | 24 | 32) {
        return Err(Error::Malformed("unsupported TGA pixel depth"));
    }
    let mut pos = 18 + id_len;
    let mut map = Vec::new();
    if cmap_type == 1 {
        let entry = (cmap_bits as usize).div_ceil(8);
        if !matches!(cmap_bits, 15 | 16 | 24 | 32) {
            return Err(Error::Malformed("unsupported TGA colour map entry size"));
        }
        let raw = bytes.get(pos..pos + cmap_len * entry).ok_or(Error::Truncated("TGA colour map"))?;
        for e in raw.chunks_exact(entry) {
            map.push(colour(e, cmap_bits, alpha_bits > 0)?);
        }
        pos += cmap_len * entry;
    }
    let px_bytes = (depth as usize).div_ceil(8);
    let n = w * hgt;
    // Decode packets (or a plain run) into stored pixel values, in file order.
    let mut stored = Vec::with_capacity(n * px_bytes);
    if kind >= 9 {
        while stored.len() < n * px_bytes {
            let head = *bytes.get(pos).ok_or(Error::Truncated("TGA RLE packet"))?;
            pos += 1;
            let count = (head & 0x7F) as usize + 1;
            if head & 0x80 != 0 {
                let p = bytes.get(pos..pos + px_bytes).ok_or(Error::Truncated("TGA RLE run"))?;
                for _ in 0..count {
                    stored.extend_from_slice(p);
                }
                pos += px_bytes;
            } else {
                let p = bytes.get(pos..pos + count * px_bytes).ok_or(Error::Truncated("TGA RLE raw packet"))?;
                stored.extend_from_slice(p);
                pos += count * px_bytes;
            }
        }
        if stored.len() != n * px_bytes {
            return Err(Error::Malformed("TGA RLE packet crosses the end of the image"));
        }
    } else {
        stored.extend_from_slice(bytes.get(pos..pos + n * px_bytes).ok_or(Error::Truncated("TGA pixels"))?);
    }
    let mut rgba = vec![0u8; n * 4];
    for (i, p) in stored.chunks_exact(px_bytes).enumerate() {
        let c = if mapped {
            let index = (p[0] as usize).checked_sub(cmap_first).filter(|i| *i < map.len());
            *index.map(|i| &map[i]).ok_or(Error::Malformed("TGA colour index outside the colour map"))?
        } else {
            colour(p, depth, alpha_bits > 0)?
        };
        let (x, y) = (i % w, i / w);
        let x = if right_to_left { w - 1 - x } else { x };
        let y = if top_to_bottom { y } else { hgt - 1 - y };
        rgba[(y * w + x) * 4..][..4].copy_from_slice(&c);
    }
    let stem = name.rsplit('/').next().unwrap_or(name);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    let mut tex = crate::style::texture(stem.to_string(), w, hgt, rgba);
    tex.raster.platform = crate::raster::NativePlatform::Targa;
    Ok(TextureCollection { kind: "Truevision TGA", rw_version: 0, textures: vec![tex], problems: vec![] })
}
