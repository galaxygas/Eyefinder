//! Eyefinder textures: RenderWare texture dictionaries (`.txd`) from the game folder and from IMG
//! archives, decoded to RGBA for preview and exported as PNG.

pub mod catalog;
pub mod decode;
pub mod export;
pub mod raster;
pub mod rw;
pub mod txd;

pub use decode::Rgba;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a texture dictionary")]
    NotTxd,
    #[error("truncated data ({0})")]
    Truncated(&'static str),
    #[error("malformed data ({0})")]
    Malformed(&'static str),
    #[error("unknown texture platform {0:#x}")]
    UnknownPlatform(u32),
    #[error("not supported yet: {0}")]
    Unsupported(&'static str),
    #[error("mip level {0} does not exist")]
    NoSuchLevel(usize),
    #[error("PNG encoding failed: {0}")]
    Png(String),
}

pub mod collection;
pub mod style;

pub mod sty;
pub mod tga;

pub mod fon;
