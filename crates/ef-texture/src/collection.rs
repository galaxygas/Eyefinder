//! Container-neutral texture collection shared by preview and export workers.
use crate::{Error, raster::TextureNative, style, txd};

pub struct TextureCollection {
    pub kind: &'static str,
    /// Zero for containers which are not RenderWare.
    pub rw_version: u32,
    pub textures: Vec<TextureNative>,
    pub problems: Vec<String>,
}

pub fn parse(bytes: &[u8]) -> Result<TextureCollection, Error> {
    if bytes.starts_with(b"GBST") {
        return crate::sty::parse(bytes);
    }
    if style::looks_like_style(bytes) {
        return style::parse(bytes);
    }
    let txd = txd::parse(bytes)?;
    Ok(TextureCollection {
        kind: "RenderWare TXD",
        rw_version: txd.rw_version,
        textures: txd.textures,
        problems: txd.problems,
    })
}

/// FON has no magic: only dispatch it using its container filename.
pub fn parse_named(bytes: &[u8], name: &str) -> Result<TextureCollection, Error> {
    if name.to_ascii_lowercase().ends_with(".fon") {
        return crate::fon::parse(bytes);
    }
    parse(bytes)
}
