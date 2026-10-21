//! Raster levels to 8-bit RGBA.
//!
//! One rule converts every stored channel to 8 bits: the channel's value is `stored / max` (as the
//! formats define it), and that value is rounded to the nearest 8-bit step (Vulkan specification,
//! "Conversion From Floating-Point to Normalized Fixed-Point"; ties round up). Integer arithmetic
//! below computes exactly that. Per-format sources are in docs/FORMATS.md.

use crate::Error;
use crate::raster::{NativePlatform, Raster, fmt, ps2_min_size};

/// A decoded image: `width * height` RGBA texels, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// `num / den` (in 0..=1) to 8 bits, rounded to nearest.
#[inline]
fn q(num: u32, den: u32) -> u8 {
    ((2 * num * 255 + den) / (2 * den)) as u8
}

#[inline]
fn u16le(b: &[u8], i: usize) -> u32 {
    b[i] as u32 | (b[i + 1] as u32) << 8
}

/// Uncompressed Direct3D texel (Microsoft `D3DFORMAT`: A1R5G5B5, R5G6B5, A4R4G4B4, L8, A8R8G8B8,
/// X8R8G8B8, X1R5G5B5; little-endian with the alpha / red field in the high bits).
fn d3d_texel(format: u32, b: &[u8], i: usize) -> [u8; 4] {
    match format & fmt::PIXEL_MASK {
        fmt::C1555 => {
            let v = u16le(b, i);
            [q(v >> 10 & 31, 31), q(v >> 5 & 31, 31), q(v & 31, 31), if v >> 15 != 0 { 255 } else { 0 }]
        }
        fmt::C555 => {
            let v = u16le(b, i);
            [q(v >> 10 & 31, 31), q(v >> 5 & 31, 31), q(v & 31, 31), 255]
        }
        fmt::C565 => {
            let v = u16le(b, i);
            [q(v >> 11 & 31, 31), q(v >> 5 & 63, 63), q(v & 31, 31), 255]
        }
        fmt::C4444 => {
            let v = u16le(b, i);
            [q(v >> 8 & 15, 15), q(v >> 4 & 15, 15), q(v & 15, 15), q(v >> 12 & 15, 15)]
        }
        fmt::LUM8 => [b[i], b[i], b[i], 255],
        fmt::C8888 => [b[i + 2], b[i + 1], b[i], b[i + 3]],
        _ => [b[i + 2], b[i + 1], b[i], 255], // C888: X8R8G8B8
    }
}

/// Xbox texel order: the bits of the texel index interleave x and y, starting with x, while both
/// still have bits (librw `src/d3d/xbox.cpp` `unswizzle`).
fn xbox_unswizzle(w: u32, h: u32) -> Vec<u32> {
    let (mut mask_u, mut mask_v, mut i, mut j) = (0u32, 0u32, 1u32, 1u32);
    loop {
        let mut c = 0;
        if i < w {
            mask_u |= j;
            j <<= 1;
            c = j;
        }
        if i < h {
            mask_v |= j;
            j <<= 1;
            c = j;
        }
        i <<= 1;
        if c == 0 {
            break;
        }
    }
    let mut out = Vec::with_capacity((w * h) as usize);
    let mut v = 0u32;
    for _ in 0..h {
        let mut u = 0u32;
        for _ in 0..w {
            out.push(u | v);
            u = u.wrapping_sub(mask_u) & mask_u;
        }
        v = v.wrapping_sub(mask_v) & mask_v;
    }
    out
}

// ---- S3TC (Khronos Data Format Specification 1.3, "S3TC Compressed Texture Image Formats") -----

fn dxt_decode(dxt: u8, data: &[u8], w: u32, h: u32, opaque_dxt1: bool) -> Result<Vec<u8>, Error> {
    let bs = if dxt == 1 { 8 } else { 16 };
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    if data.len() < bw * bh * bs {
        return Err(Error::Truncated("compressed level"));
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) * bs..][..bs];
            let texels = dxt_block(dxt, blk, opaque_dxt1);
            for (t, c) in texels.iter().enumerate() {
                let (x, y) = (bx * 4 + t % 4, by * 4 + t / 4);
                if x < w as usize && y < h as usize {
                    out[(y * w as usize + x) * 4..][..4].copy_from_slice(c);
                }
            }
        }
    }
    Ok(out)
}

fn dxt_block(dxt: u8, blk: &[u8], opaque_dxt1: bool) -> [[u8; 4]; 16] {
    let cb = if dxt == 1 { blk } else { &blk[8..] };
    let (c0, c1) = (u16le(cb, 0), u16le(cb, 2));
    let ch = |c: u32| [(c >> 11 & 31, 31u32), (c >> 5 & 63, 63), (c & 31, 31)];
    let (e0, e1) = (ch(c0), ch(c1));
    // BC2/BC3 colour blocks always use the four-colour encoding
    let four = c0 > c1 || dxt != 1;
    let mut pal = [[0u8; 4]; 4];
    for k in 0..3 {
        let ((a, m), (b, _)) = (e0[k], e1[k]);
        pal[0][k] = q(a, m);
        pal[1][k] = q(b, m);
        if four {
            pal[2][k] = q(2 * a + b, 3 * m);
            pal[3][k] = q(a + 2 * b, 3 * m);
        } else {
            pal[2][k] = q(a + b, 2 * m);
            pal[3][k] = 0;
        }
    }
    pal[0][3] = 255;
    pal[1][3] = 255;
    pal[2][3] = 255;
    pal[3][3] = if four || opaque_dxt1 { 255 } else { 0 };
    let bits = u32::from_le_bytes(cb[4..8].try_into().unwrap());
    let mut out = [[0u8; 4]; 16];
    let alpha3 = u64::from_le_bytes(blk[0..8].try_into().unwrap());
    let (a0, a1) = (blk[0] as u32, blk[1] as u32);
    let mut abits = 0u64;
    for k in 0..6 {
        abits |= (blk[2 + k] as u64) << (8 * k);
    }
    for (t, o) in out.iter_mut().enumerate() {
        let mut c = pal[(bits >> (2 * t) & 3) as usize];
        match dxt {
            3 => c[3] = q((alpha3 >> (4 * t) & 15) as u32, 15),
            5 => {
                let code = (abits >> (3 * t) & 7) as u32;
                c[3] = match code {
                    0 => a0 as u8,
                    1 => a1 as u8,
                    k if a0 > a1 => q((8 - k) * a0 + (k - 1) * a1, 7 * 255),
                    6 => 0,
                    7 => 255,
                    k => q((6 - k) * a0 + (k - 1) * a1, 5 * 255),
                };
            }
            _ => {}
        }
        *o = c;
    }
    out
}

// ---- PS2 ----------------------------------------------------------------------------------------

/// GS texel address of (x, y) in an 8-bit swizzled level (librw ps2raster.cpp `swizzle`).
fn ps2_swizzle(x: u32, y: u32, logw: u32) -> u32 {
    let bit = |v: u32, n: u32| v >> n & 1;
    let x = x ^ ((bit(y, 1) ^ bit(y, 2)) << 2);
    let nx = (x & 7) | ((x >> 1) & !7);
    let ny = (y & 1) | ((y >> 1) & !1);
    let n = bit(y, 1) | bit(x, 3) << 1;
    n | nx << 2 | ny << (logw - 1 + 2)
}

/// The texels of a PS2 level in raster order: palette indices for palettised rasters, otherwise
/// the colour bytes. Follows librw ps2raster.cpp `unswizzleRaster` and `rasterToImage`.
pub fn ps2_texels(r: &Raster, level: usize) -> Result<Vec<u8>, Error> {
    let info = r.ps2.as_ref().ok_or(Error::Malformed("not a PS2 raster"))?;
    let lv = r.levels.get(level).ok_or(Error::NoSuchLevel(level))?;
    let (w, h) = (lv.width, lv.height);
    let (minw, minh) = ps2_min_size(r.format, r.depth, info);
    let (tw, th) = (w.max(minw), h.max(minh));
    let px = &lv.data;
    let pal4 = r.format & fmt::PAL4 != 0;
    let pal8 = r.format & fmt::PAL8 != 0;
    if pal4 || pal8 {
        let mut logw = 0;
        while (1u32 << logw) < tw {
            logw += 1;
        }
        let pitch = 1usize << logw;
        let needed = if pal4 { pitch * th as usize / 2 } else { pitch * th as usize };
        if px.len() < needed {
            return Err(Error::Truncated("PS2 texels"));
        }
        let mut idx = vec![0u8; (w * h) as usize];
        let swz = (pal4 && info.swizzled4) || (pal8 && info.swizzled8);
        let mask = (1u32 << (logw + 2)) - 1;
        for y in 0..h {
            for x in 0..w {
                let v = if swz {
                    // the 4-row band containing y, addressed through the swizzle
                    let band = (y & !3) as usize;
                    let s = (ps2_swizzle(x, y, logw) & mask) as usize;
                    if pal4 {
                        let b = px[(band << logw) / 2 + (s >> 1)];
                        if s & 1 != 0 { b >> 4 } else { b & 0xF }
                    } else {
                        px[(band << logw) + s]
                    }
                } else if pal4 {
                    let b = px[(y * tw + x) as usize / 2];
                    if x & 1 != 0 { b >> 4 } else { b & 0xF }
                } else {
                    px[(y * tw + x) as usize]
                };
                idx[(y * w + x) as usize] = v;
            }
        }
        Ok(idx)
    } else {
        let bpp = (r.depth / 8) as usize;
        let mut out = Vec::with_capacity((w * h) as usize * bpp);
        for y in 0..h as usize {
            let row = px.get(y * tw as usize * bpp..(y * tw as usize + w as usize) * bpp);
            out.extend_from_slice(row.ok_or(Error::Truncated("PS2 texels"))?);
        }
        Ok(out)
    }
}

/// PS2 colours: 32-bit RGBA with alpha 0x80 = opaque (librw ps2raster.cpp `rasterToImage`,
/// `a*255/128`; values above 0x80 are treated as opaque), 16-bit A1B5G5R5 (red in the low bits).
fn ps2_colour(format: u32, b: &[u8], i: usize) -> [u8; 4] {
    if format & fmt::PIXEL_MASK == fmt::C8888 {
        [b[i], b[i + 1], b[i + 2], q((b[i + 3] as u32).min(128), 128)]
    } else {
        let v = u16le(b, i);
        [q(v & 31, 31), q(v >> 5 & 31, 31), q(v >> 10 & 31, 31), if v >> 15 != 0 { 255 } else { 0 }]
    }
}

/// CSM1 CLUT order: within every 32 entries, entries 8..15 and 16..23 are exchanged
/// (librw ps2raster.cpp `convertCSM1_32`).
fn csm1(i: usize) -> usize {
    if i & 0x18 == 0x10 || i & 0x18 == 0x08 { i ^ 0x18 } else { i }
}

impl Raster {
    /// Decodes mip level `level` to RGBA.
    pub fn decode(&self, level: usize) -> Result<Rgba, Error> {
        let (support, why) = self.support();
        if !support.can_decode() {
            return Err(Error::Unsupported(why.unwrap_or("unsupported texture")));
        }
        let lv = self.levels.get(level).ok_or(Error::NoSuchLevel(level))?;
        let (w, h) = (lv.width, lv.height);
        let n = (w * h) as usize;
        let pal = self.format & (fmt::PAL8 | fmt::PAL4) != 0;
        let entries = if self.format & fmt::PAL4 != 0 { 16 } else { 256 };
        let pixels = match self.platform {
            NativePlatform::DmaStyle => {
                if lv.data.len() != (lv.width as usize) * (lv.height as usize) * 4 {
                    return Err(Error::Malformed("invalid normalized style raster"));
                }
                lv.data.clone()
            }
            NativePlatform::D3d8 | NativePlatform::D3d9 | NativePlatform::Xbox => {
                let xbox = self.platform == NativePlatform::Xbox;
                if self.dxt != 0 {
                    // DXT1 opacity: librw treats DXT1 in a C565 raster (PC) or without the
                    // hasAlpha flag (Xbox) as opaque
                    let opaque = if xbox { !self.has_alpha } else { self.format & fmt::PIXEL_MASK == fmt::C565 };
                    dxt_decode(self.dxt, &lv.data, w, h, opaque)?
                } else {
                    let order: Vec<u32> = if xbox { xbox_unswizzle(w, h) } else { (0..n as u32).collect() };
                    let bpp = crate::raster::d3d_bytes_per_texel(self.format).unwrap_or(1) as usize;
                    if lv.data.len() < n * bpp {
                        return Err(Error::Truncated("level texels"));
                    }
                    let mut out = Vec::with_capacity(n * 4);
                    for &i in &order {
                        let i = i as usize;
                        if pal {
                            let e = lv.data[i] as usize;
                            if e >= entries {
                                return Err(Error::Malformed("palette index beyond the palette"));
                            }
                            let p = &self.palette[e * 4..e * 4 + 4];
                            // Direct3D PALETTEENTRY is R, G, B, flags (alpha); Xbox stores B, G, R, A
                            out.extend_from_slice(&if xbox {
                                [p[2], p[1], p[0], p[3]]
                            } else {
                                [p[0], p[1], p[2], p[3]]
                            });
                        } else {
                            out.extend_from_slice(&d3d_texel(self.format, &lv.data, i * bpp));
                        }
                    }
                    out
                }
            }
            NativePlatform::Ps2 => {
                let t = ps2_texels(self, level)?;
                let mut out = Vec::with_capacity(n * 4);
                if pal {
                    for &e in &t {
                        let e = if self.format & fmt::PAL8 != 0 { csm1(e as usize) } else { e as usize };
                        out.extend_from_slice(&ps2_colour(self.format, &self.palette, e * 4));
                    }
                } else {
                    let bpp = (self.depth / 8) as usize;
                    for i in 0..n {
                        out.extend_from_slice(&ps2_colour(self.format, &t, i * bpp));
                    }
                }
                out
            }
        };
        Ok(Rgba { width: w, height: h, pixels })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_matches_exact_values() {
        // 3/31 * 255 = 24.68 -> 25; 15/15 -> 255; 64/128 * 255 = 127.5 -> 128 (ties up)
        assert_eq!(q(3, 31), 25);
        assert_eq!(q(15, 15), 255);
        assert_eq!(q(64, 128), 128);
        assert_eq!(q(0, 63), 0);
        for v in 0..=15 {
            assert_eq!(q(v, 15), (v * 17) as u8);
        }
    }

    #[test]
    fn xbox_order_interleaves_x_first() {
        // 4x2: index bits are u0 v0 u1
        assert_eq!(xbox_unswizzle(4, 2), [0, 1, 4, 5, 2, 3, 6, 7]);
    }
}
