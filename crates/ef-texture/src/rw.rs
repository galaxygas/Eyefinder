//! RenderWare binary stream chunks.
//!
//! Every chunk starts with a 12-byte header `{u32 type, u32 size, u32 libraryIdStamp}`; `size`
//! counts the payload, which may contain child chunks. Sources: librw `src/base.cpp`
//! (`readChunkHeaderInfo`), gtamods "RenderWare binary stream file".

use crate::Error;

pub const STRUCT: u32 = 0x01;
pub const STRING: u32 = 0x02;
pub const EXTENSION: u32 = 0x03;
pub const TEXTURE_NATIVE: u32 = 0x15;
pub const TEX_DICTIONARY: u32 = 0x16;

#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub id: u32,
    pub size: u32,
    pub stamp: u32,
    /// Offset of the payload in the buffer.
    pub body: usize,
}

impl Chunk {
    pub fn end(&self) -> usize {
        self.body + self.size as usize
    }
    /// RenderWare version as `0x3VJNB`-style integer (3.6.0.3 = 0x36003).
    pub fn version(&self) -> u32 {
        version_of_stamp(self.stamp)
    }
}

/// Reads the chunk header at `pos`; the payload must fit inside `limit`.
pub fn chunk_at(b: &[u8], pos: usize, limit: usize) -> Result<Chunk, Error> {
    if pos + 12 > limit.min(b.len()) {
        return Err(Error::Truncated("chunk header"));
    }
    let u = |o: usize| u32::from_le_bytes(b[pos + o..pos + o + 4].try_into().unwrap());
    let c = Chunk { id: u(0), size: u(4), stamp: u(8), body: pos + 12 };
    if c.end() > limit.min(b.len()) {
        return Err(Error::Truncated("chunk payload"));
    }
    Ok(c)
}

/// Decodes a library ID stamp (librw `src/rwbase.h` `libraryIDUnpack`): new-style stamps carry
/// the version in their high 16 bits; old-style ones (before 3.1.0.1) are the version >> 8.
pub fn version_of_stamp(stamp: u32) -> u32 {
    if stamp & 0xFFFF_0000 != 0 { ((stamp >> 14 & 0x3FF00) + 0x30000) | (stamp >> 16 & 0x3F) } else { stamp << 8 }
}

/// `0x36003` -> `"3.6.0.3"`.
pub fn version_label(v: u32) -> String {
    format!("{}.{}.{}.{}", v >> 16, v >> 12 & 0xF, v >> 8 & 0xF, v & 0xFF)
}

pub(crate) fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
pub(crate) fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
pub(crate) fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_decode_to_the_documented_versions() {
        // gtamods "RenderWare_Version": 3.6.0.3 is stored as 0x1803FFFF, 3.1.0.0 as 0x00000310
        assert_eq!(version_label(version_of_stamp(0x1803FFFF)), "3.6.0.3");
        assert_eq!(version_label(version_of_stamp(0x0C02FFFF)), "3.3.0.2");
        assert_eq!(version_label(version_of_stamp(0x1003FFFF)), "3.4.0.3");
        assert_eq!(version_label(version_of_stamp(0x00000310)), "3.1.0.0");
        assert_eq!(version_of_stamp(0x0401FFFF), 0x31001);
    }
}
