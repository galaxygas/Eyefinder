//! Texture natives: the platform-specific raster inside each TXD entry.
//!
//! The first `u32` of a native's struct selects the platform (librw `src/texture.cpp`
//! `Texture::streamReadNative`): `PS2\0` (0x00325350), 8 = Direct3D 8, 9 = Direct3D 9, 5 = Xbox.
//! Layouts follow librw `readNativeTexture` in `src/d3d/d3d8.cpp`, `src/d3d/d3d9.cpp`,
//! `src/d3d/xbox.cpp` and `src/ps2/ps2raster.cpp`; see docs/FORMATS.md for the full tables.

use ef_core::SupportLevel;

use crate::Error;
use crate::rw::{self, Chunk, cstr, u16_at, u32_at};

/// RenderWare raster format bits (librw `src/rwobjects.h`, `Raster::Format`).
pub mod fmt {
    pub const C1555: u32 = 0x100;
    pub const C565: u32 = 0x200;
    pub const C4444: u32 = 0x300;
    pub const LUM8: u32 = 0x400;
    pub const C8888: u32 = 0x500;
    pub const C888: u32 = 0x600;
    pub const C555: u32 = 0xA00;
    pub const AUTOMIPMAP: u32 = 0x1000;
    pub const PAL8: u32 = 0x2000;
    pub const PAL4: u32 = 0x4000;
    pub const MIPMAP: u32 = 0x8000;
    pub const PIXEL_MASK: u32 = 0xF00;
}

pub const FOURCC_PS2: u32 = 0x0032_5350;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum NativePlatform {
    /// Normalized RGBA from a DMA style container, not a RenderWare native.
    DmaStyle,
    D3d8,
    D3d9,
    Xbox,
    Ps2,
}

impl NativePlatform {
    pub fn label(self) -> &'static str {
        match self {
            NativePlatform::DmaStyle => "PC (DMA style)",
            NativePlatform::D3d8 => "PC (Direct3D 8)",
            NativePlatform::D3d9 => "PC (Direct3D 9)",
            NativePlatform::Xbox => "Xbox",
            NativePlatform::Ps2 => "PlayStation 2",
        }
    }
}

/// Facts from the PS2 raster header needed to locate and unswizzle the texels.
#[derive(Debug, Clone)]
pub struct Ps2Info {
    /// Header `version` (high half of the raster format word): 0 = plain texels, 1 = 8-bit
    /// texels swizzled, >= 2 = every level and the palette preceded by a 0x50-byte GIF packet.
    pub version: i16,
    /// RenderWare version of the raster struct (decides 4-bit swizzling and the CLUT height).
    pub rw_version: u32,
    pub swizzled8: bool,
    pub swizzled4: bool,
    pub new_style: bool,
    pub pixel_size: u32,
    pub palette_size: u32,
    pub tex1low: u32,
}

#[derive(Debug, Clone)]
pub struct Level {
    pub width: u32,
    pub height: u32,
    /// The level's bytes as stored (PS2: the GS upload, still swizzled, without its GIF packet).
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Raster {
    pub platform: NativePlatform,
    /// Raster format word (pixel format, palette and mipmap flags).
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub has_alpha: bool,
    /// 1, 2, 3, 4 or 5 for DXT1..DXT5, 0 for uncompressed.
    pub dxt: u8,
    /// Palette bytes as stored (D3D: RGBA entries; Xbox: BGRA entries; PS2: CLUT entries).
    pub palette: Vec<u8>,
    pub levels: Vec<Level>,
    pub ps2: Option<Ps2Info>,
    /// D3D9 `D3DFORMAT` (or FourCC) as stored, for display.
    pub d3d_format: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TextureNative {
    pub name: String,
    pub mask: String,
    pub filter_addressing: u32,
    /// RenderWare version of the native chunk.
    pub rw_version: u32,
    pub raster: Raster,
}

fn levels_halving(w: u32, h: u32, n: usize) -> Vec<(u32, u32)> {
    let (mut w, mut h) = (w, h);
    (0..n)
        .map(|_| {
            let r = (w, h);
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            r
        })
        .collect()
}

/// Bytes per texel of uncompressed D3D / Xbox rasters (librw `src/d3d/d3d.cpp` `formatInfoRW`;
/// palettised rasters store one byte per texel even for PAL4, `rasterSetFormat`).
pub(crate) fn d3d_bytes_per_texel(format: u32) -> Option<u32> {
    if format & (fmt::PAL8 | fmt::PAL4) != 0 {
        return Some(1);
    }
    match format & fmt::PIXEL_MASK {
        fmt::C1555 | fmt::C565 | fmt::C4444 | fmt::C555 => Some(2),
        fmt::LUM8 => Some(1),
        fmt::C8888 | fmt::C888 => Some(4),
        _ => None,
    }
}

/// S3TC level size: ceil(w/4) * ceil(h/4) blocks of 8 (DXT1) or 16 bytes.
pub fn dxt_level_size(dxt: u8, w: u32, h: u32) -> usize {
    (w.div_ceil(4) * h.div_ceil(4)) as usize * if dxt == 1 { 8 } else { 16 }
}

fn palette_len(format: u32) -> usize {
    if format & fmt::PAL4 != 0 {
        4 * 32 // librw reads 32 entries for PAL4 (d3d8.cpp, d3d9.cpp, xbox.cpp); 16 are used
    } else if format & fmt::PAL8 != 0 {
        4 * 256
    } else {
        0
    }
}

fn take<'a>(b: &'a [u8], pos: &mut usize, n: usize, what: &'static str) -> Result<&'a [u8], Error> {
    let s = b.get(*pos..*pos + n).ok_or(Error::Truncated(what))?;
    *pos += n;
    Ok(s)
}

/// Parses one TextureNative chunk (`c` is the 0x15 chunk).
pub fn parse_native(b: &[u8], c: &Chunk) -> Result<TextureNative, Error> {
    let st = rw::chunk_at(b, c.body, c.end())?;
    if st.id != rw::STRUCT || st.size < 4 {
        return Err(Error::Malformed("texture native without a struct"));
    }
    match u32_at(b, st.body) {
        8 | 9 => parse_d3d(b, &st),
        5 => parse_xbox(b, &st),
        FOURCC_PS2 => parse_ps2(b, c, &st),
        other => Err(Error::UnknownPlatform(other)),
    }
}

fn parse_d3d(b: &[u8], st: &Chunk) -> Result<TextureNative, Error> {
    let s = &b[st.body..st.end()];
    if s.len() < 88 {
        return Err(Error::Truncated("Direct3D raster header"));
    }
    let d3d9 = u32_at(s, 0) == 9;
    let format = u32_at(s, 72);
    let word76 = u32_at(s, 76);
    let (width, height) = (u16_at(s, 80) as u32, u16_at(s, 82) as u32);
    let (depth, nlev, flags) = (s[84] as u32, s[85] as usize, s[87]);
    let (dxt, has_alpha, d3d_format) = if d3d9 {
        // flags: bit 0 has alpha, bit 1 cube map, bit 2 auto mipmaps, bit 3 compressed
        if flags & 2 != 0 {
            return Err(Error::Unsupported("Direct3D 9 cube textures"));
        }
        let dxt = if flags & 8 != 0 {
            match word76.to_le_bytes() {
                [b'D', b'X', b'T', n @ b'1'..=b'5'] => n - b'0',
                _ => return Err(Error::Unsupported("compressed Direct3D 9 format other than DXT1-5")),
            }
        } else {
            0
        };
        (dxt, flags & 1 != 0, Some(word76))
    } else {
        if flags > 5 {
            return Err(Error::Malformed("Direct3D 8 compression byte above 5"));
        }
        (flags, word76 != 0, None)
    };
    let mut pos = 88;
    let palette = take(s, &mut pos, palette_len(format), "palette")?.to_vec();
    // librw allocates one level unless the raster has the MIPMAP flag; extra levels are skipped
    let used = if format & fmt::MIPMAP != 0 { nlev } else { nlev.min(1) };
    let mut levels = Vec::with_capacity(used);
    for (i, (w, h)) in levels_halving(width, height, nlev).into_iter().enumerate() {
        let size = u32_at(take(s, &mut pos, 4, "level size")?, 0) as usize;
        let data = take(s, &mut pos, size, "level data")?;
        if i < used {
            levels.push(Level { width: w, height: h, data: data.to_vec() });
        }
    }
    Ok(TextureNative {
        name: cstr(&s[8..40]),
        mask: cstr(&s[40..72]),
        filter_addressing: u32_at(s, 4),
        rw_version: st.version(),
        raster: Raster {
            platform: if d3d9 { NativePlatform::D3d9 } else { NativePlatform::D3d8 },
            format,
            width,
            height,
            depth,
            has_alpha,
            dxt,
            palette,
            levels,
            ps2: None,
            d3d_format,
        },
    })
}

/// Xbox D3DFMT values of the compressed formats (librw `src/d3d/rwxbox.h`).
fn xbox_dxt(code: u8) -> Option<u8> {
    match code {
        0 => Some(0),
        0x0C => Some(1),
        0x0E => Some(3),
        0x0F => Some(5),
        _ => None,
    }
}

fn parse_xbox(b: &[u8], st: &Chunk) -> Result<TextureNative, Error> {
    let s = &b[st.body..st.end()];
    if st.version() < 0x34001 {
        // librw xbox.cpp readNativeTexture rejects older streams
        return Err(Error::Unsupported("Xbox textures older than RenderWare 3.4.0.1"));
    }
    if s.len() < 92 {
        return Err(Error::Truncated("Xbox raster header"));
    }
    let format = u32_at(s, 72);
    let has_alpha = u16_at(s, 76) != 0;
    if u16_at(s, 78) != 0 {
        return Err(Error::Unsupported("Xbox textures with the unknown (cube map?) flag set"));
    }
    let (width, height) = (u16_at(s, 80) as u32, u16_at(s, 82) as u32);
    let (depth, nlev) = (s[84] as u32, s[85] as usize);
    let dxt = xbox_dxt(s[87]).ok_or(Error::Unsupported("Xbox compressed format other than DXT1/3/5"))?;
    let total = u32_at(s, 88) as usize;
    let mut pos = 92;
    let palette = take(s, &mut pos, palette_len(format), "palette")?.to_vec();
    let blob = take(s, &mut pos, total, "texel data")?;
    let used = if format & fmt::MIPMAP != 0 { nlev.max(1) } else { 1 };
    let mut levels = Vec::with_capacity(used);
    let mut off = 0;
    for (w, h) in levels_halving(width, height, used) {
        // librw xbox.cpp calculateTextureSize; levels are stored back to back
        let size = if dxt != 0 {
            dxt_level_size(dxt, w, h)
        } else {
            (w * h * d3d_bytes_per_texel(format).ok_or(Error::Unsupported("Xbox raster format"))?) as usize
        };
        let data = blob.get(off..off + size).ok_or(Error::Truncated("Xbox mip level"))?;
        levels.push(Level { width: w, height: h, data: data.to_vec() });
        off += size;
    }
    Ok(TextureNative {
        name: cstr(&s[8..40]),
        mask: cstr(&s[40..72]),
        filter_addressing: u32_at(s, 4),
        rw_version: st.version(),
        raster: Raster {
            platform: NativePlatform::Xbox,
            format,
            width,
            height,
            depth,
            has_alpha,
            dxt,
            palette,
            levels,
            ps2: None,
            d3d_format: None,
        },
    })
}

/// GS transfer minimum width/height of a pixel storage mode (librw ps2raster.cpp `transferMinSize`).
pub(crate) fn ps2_min_size(format: u32, depth: u32, info: &Ps2Info) -> (u32, u32) {
    if format & fmt::PAL4 != 0 {
        if info.swizzled4 { (32, 4) } else { (8, 1) }
    } else if format & fmt::PAL8 != 0 {
        if info.swizzled8 { (16, 4) } else { (8, 1) }
    } else {
        match depth {
            32 => (2, 1),
            16 => (4, 1),
            _ => (8, 1),
        }
    }
}

fn align16(x: u32) -> u32 {
    (x + 15) & !15
}

fn parse_ps2(b: &[u8], c: &Chunk, st: &Chunk) -> Result<TextureNative, Error> {
    if st.size < 8 {
        return Err(Error::Truncated("PS2 texture struct"));
    }
    let filter_addressing = u32_at(b, st.body + 4);
    let name_c = rw::chunk_at(b, st.end(), c.end())?;
    let mask_c = rw::chunk_at(b, name_c.end(), c.end())?;
    if name_c.id != rw::STRING || mask_c.id != rw::STRING {
        return Err(Error::Malformed("PS2 texture without name strings"));
    }
    let outer = rw::chunk_at(b, mask_c.end(), c.end())?;
    let hdr_c = rw::chunk_at(b, outer.body, outer.end())?;
    if outer.id != rw::STRUCT || hdr_c.id != rw::STRUCT || hdr_c.size < 64 {
        return Err(Error::Malformed("PS2 raster header"));
    }
    let data_c = rw::chunk_at(b, hdr_c.end(), outer.end())?;
    let h = &b[hdr_c.body..hdr_c.body + 64];
    let (width, height, depth) = (u32_at(h, 0), u32_at(h, 4), u32_at(h, 8));
    let format = u16_at(h, 12) as u32;
    let version = u16_at(h, 14) as i16;
    let tex1low = u32_at(h, 28);
    let (pixel_size, palette_size) = (u32_at(h, 48), u32_at(h, 52));
    let rw_version = hdr_c.version();
    let new_style = version >= 2;
    let pal = format & (fmt::PAL4 | fmt::PAL8) != 0;
    // librw ps2raster.cpp rasterCreateTexture / readNativeTexture: new-style rasters swizzle 8-bit
    // texels always and 4-bit texels from RenderWare 3.1.0.1 on; version 1 swizzles 8-bit texels.
    let info = Ps2Info {
        version,
        rw_version,
        swizzled8: format & fmt::PAL8 != 0 && (new_style || version == 1),
        swizzled4: format & fmt::PAL4 != 0 && new_style && rw_version > 0x31000,
        new_style,
        pixel_size,
        palette_size,
        tex1low,
    };
    let data = &b[data_c.body..data_c.end()];
    if (pixel_size as usize) + (palette_size as usize) > data.len() {
        return Err(Error::Truncated("PS2 texel data"));
    }
    let nlev = if format & fmt::MIPMAP != 0 { (tex1low >> 2) as usize + 1 } else { 1 };
    let (minw, minh) = ps2_min_size(format, depth, &info);
    let mut levels = Vec::new();
    if new_style {
        // each level is a GIF packet (0x50 bytes) and its texels, padded to 16 bytes, the level
        // rectangle widened to the transfer minimum (librw rasterCreateTexture pixelSize)
        let mut spans = Vec::new();
        let (mut w, mut hh, mut off) = (width, height, 0usize);
        for _ in 0..nlev {
            if w == 0 || hh == 0 {
                break;
            }
            let size = align16(w.max(minw) * hh.max(minh) * depth / 8) as usize;
            spans.push((w, hh, off + 0x50, size));
            off += 0x50 + size;
            w /= 2;
            hh /= 2;
        }
        // the header's pixelSize must account for exactly these levels; otherwise only the first
        // level's position is certain
        if off != pixel_size as usize {
            spans.truncate(1);
        }
        for (w, hh, start, size) in spans {
            let lv = data.get(start..start + size).ok_or(Error::Truncated("PS2 mip level"))?;
            levels.push(Level { width: w, height: hh, data: lv.to_vec() });
        }
    } else {
        // plain rasters: the texels start the data, the palette follows at pixelSize; only the
        // first level's position is documented for this layout
        let size = (width.max(minw) * height.max(minh) * depth / 8) as usize;
        let lv = data.get(..size).ok_or(Error::Truncated("PS2 texels"))?;
        levels.push(Level { width, height, data: lv.to_vec() });
    }
    let palette = if pal {
        let start = pixel_size as usize + if new_style { 0x50 } else { 0 };
        let entry = if format & fmt::PIXEL_MASK == fmt::C8888 { 4 } else { 2 };
        let entries = if format & fmt::PAL4 != 0 { 16 } else { 256 };
        data.get(start..start + entry * entries).ok_or(Error::Truncated("PS2 palette"))?.to_vec()
    } else {
        Vec::new()
    };
    Ok(TextureNative {
        name: cstr(&b[name_c.body..name_c.end()]),
        mask: cstr(&b[mask_c.body..mask_c.end()]),
        filter_addressing,
        rw_version,
        raster: Raster {
            platform: NativePlatform::Ps2,
            format,
            width,
            height,
            depth,
            has_alpha: false,
            dxt: 0,
            palette,
            levels,
            ps2: Some(info),
            d3d_format: None,
        },
    })
}

impl Raster {
    /// Short description of the pixel format, e.g. "DXT1", "8-bit palette (RGBA8888)".
    pub fn format_label(&self) -> String {
        if self.platform == NativePlatform::DmaStyle {
            return "Indexed DMA style → RGBA8".into();
        }
        let px = match self.format & fmt::PIXEL_MASK {
            fmt::C1555 => "ARGB1555",
            fmt::C565 => "RGB565",
            fmt::C4444 => "ARGB4444",
            fmt::LUM8 => "L8",
            fmt::C8888 => "ARGB8888",
            fmt::C888 => "RGB888",
            fmt::C555 => "RGB555",
            0 => "no pixel format",
            _ => "unknown pixel format",
        };
        if self.dxt != 0 {
            format!("DXT{}", self.dxt)
        } else if self.format & fmt::PAL8 != 0 {
            format!("8-bit palette ({px})")
        } else if self.format & fmt::PAL4 != 0 {
            format!("4-bit palette ({px})")
        } else {
            px.to_string()
        }
    }

    /// Whether Eyefinder decodes this raster, and why not if it does not.
    pub fn support(&self) -> (SupportLevel, Option<&'static str>) {
        let px = self.format & fmt::PIXEL_MASK;
        let pal = self.format & (fmt::PAL8 | fmt::PAL4) != 0;
        let why = match self.platform {
            NativePlatform::DmaStyle => None,
            NativePlatform::D3d8 | NativePlatform::D3d9 => {
                if self.dxt == 2 || self.dxt == 4 {
                    Some("premultiplied-alpha DXT2/DXT4 is not decoded")
                } else if self.dxt == 0 && !pal && d3d_bytes_per_texel(self.format).is_none() {
                    Some("unknown Direct3D pixel format")
                } else {
                    None
                }
            }
            NativePlatform::Xbox => {
                if self.dxt == 0 && !pal && !matches!(px, fmt::C1555 | fmt::C555 | fmt::C8888) {
                    Some("this Xbox pixel format has no reference decoder to check against yet")
                } else if self.dxt == 0 && !(self.width.is_power_of_two() && self.height.is_power_of_two()) {
                    Some("swizzled Xbox texture whose size is not a power of two")
                } else {
                    None
                }
            }
            NativePlatform::Ps2 => {
                if pal && px != fmt::C8888 {
                    Some("PS2 16-bit palettes are not decoded yet (their CLUT layout is unconfirmed)")
                } else if !pal && !matches!(px, fmt::C8888 | fmt::C1555) {
                    Some("this PS2 pixel format is not decoded yet")
                } else if pal && !(self.width.is_power_of_two() && self.height.is_power_of_two()) {
                    Some("swizzled PS2 texture whose size is not a power of two")
                } else {
                    None
                }
            }
        };
        match why {
            Some(w) => (SupportLevel::DetectedOnly, Some(w)),
            None => (SupportLevel::FromSpec, None),
        }
    }
}
