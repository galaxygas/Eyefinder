//! GTA2 map files (`.gmp`, "GBMP" version 500).
//!
//! Source: DMA Design, GBH Map Format 8.2, 18 August 1999 (transcribed at
//! https://gtamp.com/gta2/gta2-gmp-map-file-format/): a "GBMP" header and named chunks in any order.
//! Read here: UMAP / CMAP / DMAP (the map, uncompressed or compressed), ZONE, MOBJ, LGHT and ANIM.
//! PSXM and RGEN are kept as bytes; unknown chunks are listed and kept ("Any program which uses the
//! map file should load in the chunks it needs and ignore the others").

use serde::Serialize;

use crate::{Error, slice, u16_at, u32_at};

pub const VERSION: u16 = 500;

/// `block_info` (GBH Map 3): 12 bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Block {
    pub left: u16,
    pub right: u16,
    pub top: u16,
    pub bottom: u16,
    pub lid: u16,
    pub arrows: u8,
    pub slope_type: u8,
}

impl Block {
    /// `slope_type` bits 0-1: 0 air, 1 road, 2 pavement, 3 field.
    pub fn ground_type(&self) -> u8 {
        self.slope_type & 3
    }
    /// `slope_type` bits 2-7: 0 none, 1-44 gradients, 45-48 diagonals, 49-52 diagonal slopes,
    /// 53-61 partial blocks, 63 "slope in block above".
    pub fn slope(&self) -> u8 {
        self.slope_type >> 2
    }
}

/// A face word (GBH Map 3): tile, wall bits / lighting, flat, flip, rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Face(pub u16);

impl Face {
    /// Bits 0-9: tile graphic number, 0 = no face.
    pub fn tile(self) -> u16 {
        self.0 & 0x3FF
    }
    /// Lid bits 10-11: 0 normal, 1-3 darker (the amount is set by the game at run time).
    pub fn lighting(self) -> u8 {
        ((self.0 >> 10) & 3) as u8
    }
    /// Bit 12: drawn transparently.
    pub fn flat(self) -> bool {
        self.0 & 0x1000 != 0
    }
    /// Bit 13: flipped left to right.
    pub fn flip(self) -> bool {
        self.0 & 0x2000 != 0
    }
    /// Bits 14-15: rotation in quarter turns.
    pub fn rotation(self) -> u8 {
        (self.0 >> 14) as u8
    }
}

/// `map_zone` (GBH Map 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Zone {
    pub zone_type: u8,
    pub x: u8,
    pub y: u8,
    pub w: u8,
    pub h: u8,
    pub name: String,
}

impl Zone {
    /// The type name GBH Map 5 gives.
    pub fn type_name(&self) -> &'static str {
        match self.zone_type {
            0 => "general purpose",
            1 => "navigation",
            2 => "traffic light",
            5 => "arrow blocker",
            6 => "railway station (platform)",
            7 => "bus stop (pavement)",
            8 => "general trigger",
            10 => "information",
            11 => "railway station entry point",
            12 => "railway station exit point",
            13 => "railway stop point",
            14 => "gang",
            15 => "local navigation",
            16 => "restart",
            20 => "arrest restart",
            _ => "unused",
        }
    }
}

/// `map_object` (GBH Map 6): (1.8.7) fixed-point block co-ordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MapObject {
    pub x: u16,
    pub y: u16,
    /// Ang8: 256 steps per turn.
    pub rotation: u8,
    pub object_type: u8,
}

/// A (1.8.7) fixed-point value in blocks: sign, 8 bits of block, 7 bits within it.
pub fn fix16(v: u16) -> f32 {
    v as i16 as f32 / 128.0
}

/// `map_light` (GBH Map 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Light {
    pub argb: u32,
    pub x: u16,
    pub y: u16,
    pub z: u16,
    pub radius: u16,
    pub intensity: u8,
    pub shape: u8,
    pub on_time: u8,
    pub off_time: u8,
}

/// `tile_animation` (GBH Map 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TileAnimation {
    pub base: u16,
    pub frame_rate: u8,
    pub repeat: u8,
    pub tiles: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct GbhMap {
    /// The map chunk read: `UMAP`, `DMAP` or `CMAP`.
    pub map_chunk: &'static str,
    /// Block numbers of each column by height z (0-7; 0 = air), indexed `y * 256 + x`.
    pub columns: Vec<[u32; 8]>,
    pub blocks: Vec<Block>,
    pub zones: Vec<Zone>,
    pub objects: Vec<MapObject>,
    pub lights: Vec<Light>,
    pub animations: Vec<TileAnimation>,
    /// Every chunk of the file: name and size.
    pub chunks: Vec<(String, usize)>,
}

impl GbhMap {
    pub fn block(&self, x: usize, y: usize, z: usize) -> &Block {
        &self.blocks[self.columns[y * 256 + x][z] as usize]
    }
}

pub fn looks_like_gmp(b: &[u8]) -> bool {
    b.get(..4) == Some(b"GBMP")
}

fn block_at(r: &[u8; 12]) -> Block {
    let w = |i: usize| u16::from_le_bytes([r[i * 2], r[i * 2 + 1]]);
    Block { left: w(0), right: w(1), top: w(2), bottom: w(3), lid: w(4), arrows: r[10], slope_type: r[11] }
}

/// The compressed map (GBH Map 4). `wide` = DMAP (32-bit), else CMAP (16-bit).
fn compressed(c: &[u8], wide: bool) -> Result<(Vec<[u32; 8]>, Vec<Block>), Error> {
    let unit = if wide { 4 } else { 2 };
    let read = |p: usize| -> Result<u32, Error> { if wide { u32_at(c, p) } else { u16_at(c, p).map(u32::from) } };
    let base = slice(c, 0, 65536 * unit)?;
    let mut p = 65536 * unit;
    let words = read(p)? as usize;
    p += unit;
    let column = slice(c, p, words.checked_mul(unit).ok_or(Error::Malformed("size overflow"))?)?;
    p += words * unit;
    let num_blocks = read(p)? as usize;
    p += unit;
    let block_bytes = slice(c, p, num_blocks.checked_mul(12).ok_or(Error::Malformed("size overflow"))?)?;
    if p + num_blocks * 12 != c.len() {
        return Err(Error::Malformed("compressed map chunk size does not match its contents"));
    }
    let blocks: Vec<Block> = block_bytes.as_chunks::<12>().0.iter().map(block_at).collect();
    let mut columns = Vec::with_capacity(65536);
    for i in 0..65536 {
        let start = if wide { u32_at(base, i * 4)? } else { u32::from(u16_at(base, i * 2)?) } as usize * unit;
        let head = slice(column, start, 2)?;
        let (height, offset) = (head[0] as usize, head[1] as usize);
        if height > 8 || offset > height {
            return Err(Error::Malformed("column height or offset out of range"));
        }
        // CMAP: UInt8 height, UInt8 offset, UInt16 blockd[]; DMAP: the same with a UInt16 pad.
        let first = start + if wide { 4 } else { 2 };
        let mut col = [0u32; 8];
        for (k, z) in (offset..height).enumerate() {
            let num = if wide { u32_at(column, first + k * 4)? } else { u32::from(u16_at(column, first + k * 2)?) };
            if num as usize >= blocks.len() {
                return Err(Error::Malformed("column names a block that does not exist"));
            }
            col[z] = num;
        }
        columns.push(col);
    }
    Ok((columns, blocks))
}

/// The uncompressed map: `block_info city_scape[8][256][256]` ([z][y][x]).
fn uncompressed(c: &[u8]) -> Result<(Vec<[u32; 8]>, Vec<Block>), Error> {
    if c.len() != 8 * 65536 * 12 {
        return Err(Error::Malformed("UMAP is not 256x256x8 blocks"));
    }
    let mut blocks = vec![Block::default()];
    let mut index = std::collections::HashMap::new();
    let mut columns = vec![[0u32; 8]; 65536];
    for z in 0..8 {
        for i in 0..65536 {
            let b = block_at(c[(z * 65536 + i) * 12..][..12].try_into().unwrap());
            if b == Block::default() {
                continue;
            }
            let n = *index.entry(b).or_insert_with(|| {
                blocks.push(b);
                blocks.len() - 1
            });
            columns[i][z] = n as u32;
        }
    }
    Ok((columns, blocks))
}

impl std::hash::Hash for Block {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (self.left, self.right, self.top, self.bottom, self.lid, self.arrows, self.slope_type).hash(h);
    }
}

pub fn parse(b: &[u8]) -> Result<GbhMap, Error> {
    if !looks_like_gmp(b) {
        return Err(Error::Malformed("no GBMP header"));
    }
    let version = u16_at(b, 4)?;
    if version != VERSION {
        return Err(Error::Unsupported(format!("GMP version {version} is not documented (GBH Map 8.2: 500)")));
    }
    let mut chunks = Vec::new();
    let mut p = 6;
    while p < b.len() {
        let name = slice(b, p, 4)?;
        let size = u32_at(b, p + 4)? as usize;
        let data = slice(b, p + 8, size)?;
        chunks.push((String::from_utf8_lossy(name).into_owned(), data));
        p += 8 + size;
    }
    let get = |n: &str| chunks.iter().find(|(c, _)| c == n).map(|(_, d)| *d);
    // The compressed map is what the game loads; UMAP is the editor's copy (GBH Map 4).
    let (map_chunk, (columns, blocks)) = if let Some(d) = get("DMAP") {
        ("DMAP", compressed(d, true)?)
    } else if let Some(c) = get("CMAP") {
        ("CMAP", compressed(c, false)?)
    } else if let Some(u) = get("UMAP") {
        ("UMAP", uncompressed(u)?)
    } else {
        return Err(Error::Malformed("no UMAP, CMAP or DMAP chunk"));
    };
    let mut zones = Vec::new();
    if let Some(z) = get("ZONE") {
        let mut q = 0;
        while q < z.len() {
            let h = slice(z, q, 6)?;
            let name = slice(z, q + 6, h[5] as usize)?;
            zones.push(Zone {
                zone_type: h[0],
                x: h[1],
                y: h[2],
                w: h[3],
                h: h[4],
                name: name.iter().map(|&c| c as char).collect(),
            });
            q += 6 + h[5] as usize;
        }
    }
    let objects = match get("MOBJ") {
        Some(m) if !m.len().is_multiple_of(6) => return Err(Error::Malformed("MOBJ is not whole 6-byte records")),
        Some(m) => m
            .as_chunks::<6>()
            .0
            .iter()
            .map(|r| MapObject {
                x: u16::from_le_bytes([r[0], r[1]]),
                y: u16::from_le_bytes([r[2], r[3]]),
                rotation: r[4],
                object_type: r[5],
            })
            .collect(),
        None => Vec::new(),
    };
    let lights = match get("LGHT") {
        Some(l) if !l.len().is_multiple_of(16) => return Err(Error::Malformed("LGHT is not whole 16-byte records")),
        Some(l) => l
            .as_chunks::<16>()
            .0
            .iter()
            .map(|r| {
                let w = |i: usize| u16::from_le_bytes([r[i], r[i + 1]]);
                Light {
                    argb: u32::from_le_bytes([r[0], r[1], r[2], r[3]]),
                    x: w(4),
                    y: w(6),
                    z: w(8),
                    radius: w(10),
                    intensity: r[12],
                    shape: r[13],
                    on_time: r[14],
                    off_time: r[15],
                }
            })
            .collect(),
        None => Vec::new(),
    };
    let mut animations = Vec::new();
    if let Some(a) = get("ANIM") {
        let mut q = 0;
        while q < a.len() {
            let h = slice(a, q, 6)?;
            let n = h[4] as usize;
            let tiles = slice(a, q + 6, n * 2)?;
            animations.push(TileAnimation {
                base: u16::from_le_bytes([h[0], h[1]]),
                frame_rate: h[2],
                repeat: h[3],
                tiles: tiles.as_chunks::<2>().0.iter().map(|t| u16::from_le_bytes([t[0], t[1]])).collect(),
            });
            q += 6 + n * 2;
        }
    }
    let chunks = chunks.iter().map(|(n, d)| (n.clone(), d.len())).collect();
    Ok(GbhMap { map_chunk, columns, blocks, zones, objects, lights, animations, chunks })
}
