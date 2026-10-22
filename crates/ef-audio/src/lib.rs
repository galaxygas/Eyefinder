//! Eyefinder audio: decoding and exporting the audio of GTA III, Vice City and San Andreas.
//!
//! Every format module names the source its layout comes from. Formats that are only partly
//! understood are not decoded; see `docs/SUPPORT.md`.

pub mod catalog;
pub mod codec;
pub mod dcstr;
pub mod export;
pub mod movie;
pub mod open;
pub mod pcm;
pub mod sa;
pub mod sources;
pub mod stream;
pub mod symph;
pub mod tables;
pub mod wav;

pub use catalog::{AssetKind, AudioAsset, Catalog, Category};
pub use open::{DecodeOptions, Opened};
pub use pcm::{PcmFormat, PcmSource, SampleKind, SourceInfo};

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{path}: {source}")]
    IoAt { path: String, source: std::io::Error },
    #[error("invalid data: {0}")]
    Format(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cancelled")]
    Cancelled,
}

impl Error {
    pub fn io_at(path: &Path, source: std::io::Error) -> Self {
        Error::IoAt { path: path.display().to_string(), source }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

mod legacy;
