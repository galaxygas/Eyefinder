//! The map files of the games Eyefinder reads: what a copy holds ([`catalog`]), the GTA1 / London
//! CMP ([`cmp`]) and GTA2 GMP ([`gmp`]) city maps, the premade maps of GTA III, Vice City and San
//! Andreas ([`radar`]: radar tiles and pause-menu map), top-down renders of the 2D cities
//! ([`topdown`]), and the exporter ([`export`]) that writes raw files, individual files and full maps
//! with a manifest.
//!
//! Every layout cites its source in its module and in docs/FORMATS.md. Nothing here is verified on
//! real files yet: every output is `SupportLevel::FromSpec`.

pub mod catalog;
pub mod cmp;
pub mod dff;
pub mod export;
pub mod gltf;
pub mod gmp;
pub mod lid;
pub mod radar;
pub mod render3d;
pub mod scene;
pub mod topdown;
pub mod world;

pub use catalog::{Availability, MapCatalog};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("malformed map data: {0}")]
    Malformed(&'static str),
    #[error("{0}")]
    Unsupported(String),
    #[error("texture: {0}")]
    Texture(String),
    #[error("PNG: {0}")]
    Png(String),
}

impl From<ef_texture::Error> for Error {
    fn from(e: ef_texture::Error) -> Self {
        Error::Texture(e.to_string())
    }
}

pub(crate) fn u16_at(b: &[u8], p: usize) -> Result<u16, Error> {
    b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or(Error::Malformed("truncated"))
}

pub(crate) fn u32_at(b: &[u8], p: usize) -> Result<u32, Error> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or(Error::Malformed("truncated"))
}

pub(crate) fn slice(b: &[u8], p: usize, n: usize) -> Result<&[u8], Error> {
    b.get(p..p.checked_add(n).ok_or(Error::Malformed("size overflow"))?).ok_or(Error::Malformed("truncated"))
}
