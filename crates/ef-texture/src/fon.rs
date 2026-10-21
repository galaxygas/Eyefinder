//! GTA1/London PC FON: Daniel Marschall's firsthand format_fon.html analysis at
//! https://misc.daniel-marschall.de/spiele/gta1/format_fon.html.
//! Index-zero transparency: OpenGTA 1ae34ae3409d37bf484364d2a1db2e183a8527bd,
//! read_fnt.cpp getCharacterBitmap and read_gry.cpp RGBPalette::apply.
use crate::{Error, collection::TextureCollection, style::texture};

pub fn parse(bytes: &[u8]) -> Result<TextureCollection, Error> {
    let bad = Error::Malformed("invalid or truncated GTA1 FON");
    let header = bytes.get(..2).ok_or(bad)?;
    let (count, height) = (header[0] as usize, header[1] as usize);
    if height == 0 {
        return Err(Error::Malformed("zero FON image height"));
    }
    let mut cursor = 2;
    let mut images = Vec::with_capacity(count);
    for i in 0..count {
        let width = *bytes.get(cursor).ok_or(Error::Truncated("FON image width"))? as usize;
        cursor += 1;
        let end = cursor + width * height;
        let indices = bytes.get(cursor..end).ok_or(Error::Truncated("FON image pixels"))?;
        images.push((i, width, indices));
        cursor = end;
    }
    let palette = bytes.get(cursor..).ok_or(Error::Truncated("FON palette"))?;
    if palette.len() != 768 {
        return Err(Error::Malformed("FON requires a trailing 256-color RGB palette"));
    }
    let mut out = TextureCollection { kind: "GTA1 FON", rw_version: 0, textures: vec![], problems: vec![] };
    for (i, width, indices) in images {
        if width == 0 {
            out.problems.push(format!("image_{i:03}: zero-width image has no pixels to export"));
            continue;
        }
        let mut pixels = Vec::with_capacity(indices.len() * 4);
        for &index in indices {
            let p = index as usize * 3;
            pixels.extend_from_slice(&palette[p..p + 3]);
            pixels.push(if index == 0 { 0 } else { 255 });
        }
        out.textures.push(texture(format!("image_{i:03}"), width, height, pixels));
    }
    Ok(out)
}
