//! Texture dictionaries (`.txd`): RenderWare chunk 0x16 holding a struct `{u16 count, u16 device}`
//! and then one TextureNative (0x15) per texture (librw `src/texture.cpp`
//! `TexDictionary::streamRead`; gtamods "Texture Dictionary").

use crate::Error;
use crate::raster::{TextureNative, parse_native};
use crate::rw;

#[derive(Debug, Clone)]
pub struct Txd {
    pub rw_version: u32,
    pub device: u16,
    pub textures: Vec<TextureNative>,
    /// Natives that could not be read, with the reason (the rest of the dictionary still loads).
    pub problems: Vec<String>,
}

/// Whether the bytes start like a texture dictionary.
pub fn looks_like_txd(b: &[u8]) -> bool {
    b.len() >= 12 && rw::u32_at(b, 0) == rw::TEX_DICTIONARY
}

pub fn parse(b: &[u8]) -> Result<Txd, Error> {
    let top = rw::chunk_at(b, 0, b.len())?;
    if top.id != rw::TEX_DICTIONARY {
        return Err(Error::NotTxd);
    }
    let st = rw::chunk_at(b, top.body, top.end())?;
    if st.id != rw::STRUCT || st.size < 4 {
        return Err(Error::Malformed("texture dictionary without a struct"));
    }
    let count = rw::u16_at(b, st.body) as usize;
    let device = rw::u16_at(b, st.body + 2);
    let mut textures = Vec::with_capacity(count);
    let mut problems = Vec::new();
    let mut pos = st.end();
    while textures.len() + problems.len() < count && pos < top.end() {
        let c = rw::chunk_at(b, pos, top.end())?;
        pos = c.end();
        if c.id != rw::TEXTURE_NATIVE {
            continue;
        }
        match parse_native(b, &c) {
            Ok(t) => textures.push(t),
            Err(e) => problems.push(format!("texture {}: {e}", textures.len() + problems.len() + 1)),
        }
    }
    Ok(Txd { rw_version: top.version(), device, textures, problems })
}
