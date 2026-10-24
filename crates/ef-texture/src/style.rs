//! GTA1 retail GRY/G24. DMA CityScape Data Structure 12.10 §§3.3–3.8,
//! https://misc.daniel-marschall.de/spiele/gta1/cds.pdf; retail sprite page
//! offsets cross-checked against OpenGTA read_gry.cpp/read_g24.cpp at
//! 1ae34ae3409d37bf484364d2a1db2e183a8527bd. No game data or source copied.
use crate::raster::{Level, NativePlatform, Raster, TextureNative};
use crate::{Error, collection::TextureCollection};

const BAD: Error = Error::Malformed("invalid or truncated DMA style");
fn slice(b: &[u8], p: usize, n: usize) -> Result<&[u8], Error> {
    b.get(p..p.checked_add(n).ok_or(BAD)?).ok_or(BAD)
}
fn u16_at(b: &[u8], p: usize) -> Result<usize, Error> {
    Ok(u16::from_le_bytes(slice(b, p, 2)?.try_into().unwrap()) as usize)
}
fn u32_at(b: &[u8], p: usize) -> Result<usize, Error> {
    Ok(u32::from_le_bytes(slice(b, p, 4)?.try_into().unwrap()) as usize)
}
fn take<'a>(b: &'a [u8], p: &mut usize, n: usize) -> Result<&'a [u8], Error> {
    let v = slice(b, *p, n)?;
    *p += n;
    Ok(v)
}
pub fn looks_like_style(b: &[u8]) -> bool {
    matches!(u32_at(b, 0), Ok(325 | 336))
}

pub(crate) fn texture(name: String, w: usize, h: usize, pixels: Vec<u8>) -> TextureNative {
    TextureNative {
        name,
        mask: String::new(),
        filter_addressing: 0,
        rw_version: 0,
        raster: Raster {
            platform: NativePlatform::DmaStyle,
            format: 0,
            width: w as u32,
            height: h as u32,
            depth: 32,
            has_alpha: true,
            dxt: 0,
            palette: vec![],
            levels: vec![Level { width: w as u32, height: h as u32, data: pixels }],
            ps2: None,
            d3d_format: None,
        },
    }
}

pub(crate) struct Palette<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) index: &'a [u8],
    pub(crate) g24: bool,
}
impl Palette<'_> {
    pub(crate) fn rgba(&self, indices: &[u8], virtual_pal: usize, remap: Option<&[u8]>) -> Result<Vec<u8>, Error> {
        let physical = if self.g24 { u16_at(self.index, virtual_pal.checked_mul(2).ok_or(BAD)?)? } else { 0 };
        let mut out = Vec::with_capacity(indices.len() * 4);
        for &index in indices {
            let i = if let Some(r) = remap { *r.get(index as usize).ok_or(BAD)? } else { index } as usize;
            let (r, g, b) = if self.g24 {
                let p = (physical / 64) * 65536 + i * 256 + (physical % 64) * 4;
                let c = slice(self.bytes, p, 4)?;
                (c[2], c[1], c[0])
            } else {
                let c = slice(self.bytes, i * 3, 3)?;
                (c[0], c[1], c[2])
            };
            out.extend_from_slice(&[r, g, b, if i == 0 { 0 } else { 255 }]);
        }
        Ok(out)
    }
}
pub(crate) fn rectangle(data: &[u8], start: usize, w: usize, h: usize) -> Result<Vec<u8>, Error> {
    if w == 0 || h == 0 || start % 256 + w > 256 {
        return Err(BAD);
    }
    let mut result = Vec::with_capacity(w * h);
    for y in 0..h {
        result.extend_from_slice(slice(data, start.checked_add(y * 256).ok_or(BAD)?, w)?);
    }
    Ok(result)
}

pub fn parse(b: &[u8]) -> Result<TextureCollection, Error> {
    let version = u32_at(b, 0)?;
    let g24 = match version {
        336 => true,
        325 => false,
        _ => return Err(Error::Unsupported("undocumented style version")),
    };
    let fields = if g24 { 16 } else { 13 };
    slice(b, 0, fields * 4)?;
    let h: Vec<usize> = (0..fields).map(|i| u32_at(b, i * 4)).collect::<Result<_, _>>()?;
    if h[1..4].iter().any(|n| !n.is_multiple_of(4096)) {
        return Err(BAD);
    }
    let count = h[1] / 4096 + h[2] / 4096 + h[3] / 4096;
    let mut pos = fields * 4;
    let tiles = take(b, &mut pos, count.div_ceil(4).checked_mul(16384).ok_or(BAD)?)?;
    take(b, &mut pos, h[4])?; // Animation metadata, no pixel payload.
    let palette = take(b, &mut pos, if g24 { h[5].div_ceil(65536) * 65536 } else { h[5] })?;
    let mut remaps = &[][..];
    let mut remap_index = &[][..];
    let index = if g24 {
        take(b, &mut pos, h[10])?
    } else {
        if palette.len() != 768 {
            return Err(BAD);
        }
        remaps = take(b, &mut pos, h[6])?;
        remap_index = take(b, &mut pos, h[7])?;
        &[]
    };
    let object_field = if g24 { 11 } else { 8 };
    take(b, &mut pos, h[object_field])?;
    take(b, &mut pos, h[object_field + 1])?;
    let sprite_info = take(b, &mut pos, h[object_field + 2])?;
    let sprites = take(b, &mut pos, h[object_field + 3])?;
    take(b, &mut pos, h[object_field + 4])?;
    if pos != b.len() {
        return Err(Error::Malformed("unaccounted bytes after style sections"));
    }
    let pal = Palette { bytes: palette, index, g24 };
    let mut out = TextureCollection {
        kind: if g24 { "GTA1 G24" } else { "GTA1 GRY" },
        rw_version: 0,
        textures: vec![],
        problems: vec![],
    };
    for tile in 0..count {
        let indices = rectangle(tiles, (tile / 4) * 16384 + (tile % 4) * 64, 64, 64)?;
        let (group, id) = if tile < h[1] / 4096 {
            ("side", tile)
        } else if tile < (h[1] + h[2]) / 4096 {
            ("lid", tile - h[1] / 4096)
        } else {
            ("aux", tile - (h[1] + h[2]) / 4096)
        };
        let variants =
            if g24 || (group == "lid" && remap_index.len() == 1024 && remaps.len() == 65536) { 4 } else { 1 };
        for variant in 0..variants {
            let remap = if !g24 && group == "lid" && variant > 0 {
                let table = *remap_index.get(id * 4 + variant).ok_or(BAD)? as usize;
                Some(slice(remaps, table * 256, 256)?)
            } else {
                None
            };
            out.textures.push(texture(
                format!("{group}_{id:04}_palette_{variant}"),
                64,
                64,
                pal.rgba(&indices, tile * 4 + variant, remap)?,
            ));
        }
    }
    let mut p = 0;
    let mut number = 0;
    while p < sprite_info.len() {
        let header = take(sprite_info, &mut p, if g24 { 12 } else { 10 })?;
        let (w, ht, deltas) = (header[0] as usize, header[1] as usize, header[2] as usize);
        if header[3] != 0 || u16_at(header, 4)? != w * ht || deltas > 32 {
            return Err(BAD);
        }
        let vp = if g24 { h[6] / 1024 + u16_at(header, 6)? } else { 0 };
        let start = u32_at(header, if g24 { 8 } else { 6 })?;
        let indices = rectangle(sprites, start, w, ht)?;
        out.textures.push(texture(format!("sprite_{number:04}"), w, ht, pal.rgba(&indices, vp, None)?));
        for d in 0..deltas {
            let size = u16_at(sprite_info, p)?;
            let offset = u32_at(sprite_info, p + 2)?;
            p += 6;
            let delta = slice(sprites, offset, size)?;
            let mut modified = indices.clone();
            let mut cursor = 0usize;
            let mut dp = 0;
            while dp < delta.len() {
                let skip = u16_at(delta, dp)?;
                let len = *delta.get(dp + 2).ok_or(BAD)? as usize;
                dp += 3;
                cursor = cursor.checked_add(skip).ok_or(BAD)?;
                let run = take(delta, &mut dp, len)?;
                let (x, y) = (cursor % 256, cursor / 256);
                if y >= ht || x + len > w {
                    return Err(Error::Malformed("sprite delta leaves its rectangle"));
                }
                modified[y * w + x..y * w + x + len].copy_from_slice(run);
                cursor += len;
            }
            out.textures.push(texture(
                format!("sprite_{number:04}_delta_{d:02}"),
                w,
                ht,
                pal.rgba(&modified, vp, None)?,
            ));
        }
        number += 1;
    }
    out.problems.push("Style export includes tiles, tile palette variants, base sprites and individual stored deltas; sprite remaps and combined delta states are not enumerated.".into());
    Ok(out)
}
