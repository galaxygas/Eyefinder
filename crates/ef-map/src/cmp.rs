//! GTA1 / London 1969 / London 1961 compressed city maps (`.CMP`, version 331).
//!
//! Source: DMA Design, CityScape Data Structure 12.10, sections 2.1-2.9
//! (https://misc.daniel-marschall.de/spiele/gta1/cds.pdf). The file is a header, then `base`
//! (256x256 byte offsets into `column`), `column`, `block`, `object_pos`, `route`, `location_data`
//! and `nav_data`, each sized by the header.
//!
//! One conflict: CityScape 2.4 says `blockd[0]` of a column is the ground-level block, but OpenGTA
//! (madebr/OpenGTA @ 1ae34ae3, read_cmp.cpp `getBlockAtNew`), which draws the retail maps, reads
//! `blockd[N-1]` as the lowest block. Eyefinder follows OpenGTA; [`CityMap::columns`] are stored
//! ground first. The maintainer's real-file check settles it (docs/FORMATS.md).

use serde::Serialize;

use crate::{Error, slice, u16_at, u32_at};

/// The only version CityScape 12.10 documents ("currently 331").
pub const VERSION: u32 = 331;
const HEADER: usize = 4 + 1 + 1 + 2 + 5 * 4;

/// `block_info` (CityScape 2.1): 8 bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Block {
    pub type_map: u16,
    pub type_map_ext: u8,
    pub left: u8,
    pub right: u8,
    pub top: u8,
    pub bottom: u8,
    /// Lid graphic number; 0 = no lid.
    pub lid: u8,
}

impl Block {
    /// Bits 0-3: up, down, left, right "OK" directions.
    pub fn directions(&self) -> u8 {
        (self.type_map & 0xF) as u8
    }
    /// Bits 4-6: 0 air, 1 water, 2 road, 3 pavement, 4 field, 5 building.
    pub fn block_type(&self) -> u8 {
        ((self.type_map >> 4) & 7) as u8
    }
    /// Bit 7: drawn transparently.
    pub fn flat(&self) -> bool {
        self.type_map & 0x80 != 0
    }
    /// Bits 8-13: slope type (0 none, 1-44 per CityScape 2.1).
    pub fn slope(&self) -> u8 {
        ((self.type_map >> 8) & 0x3F) as u8
    }
    /// Bits 14-15: lid rotation in quarter turns.
    pub fn rotation(&self) -> u8 {
        (self.type_map >> 14) as u8
    }
    /// `type_map_ext` bits 3-4: lid remap 0-3.
    pub fn remap(&self) -> u8 {
        (self.type_map_ext >> 3) & 3
    }
    pub fn traffic_lights(&self) -> u8 {
        self.type_map_ext & 7
    }
    pub fn flip_top_bottom(&self) -> bool {
        self.type_map_ext & 0x20 != 0
    }
    pub fn flip_left_right(&self) -> bool {
        self.type_map_ext & 0x40 != 0
    }
    pub fn railway(&self) -> bool {
        self.type_map_ext & 0x80 != 0
    }
}

/// `object_pos_struct` (CityScape 2.5): 14 bytes; a car when `remap >= 128` (2.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObjectPos {
    pub x: u16,
    pub y: u16,
    pub z: u16,
    pub kind: u8,
    pub remap: u8,
    pub rotation: u16,
    pub pitch: u16,
    pub roll: u16,
}

impl ObjectPos {
    pub fn is_car(&self) -> bool {
        self.remap >= 128
    }
}

/// A police route (type 255) or roadblock route (type = route number), CityScape 2.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Route {
    pub route_type: u8,
    pub vertices: Vec<[u8; 3]>,
}

/// `location_data_struct` (CityScape 2.7): six (x, y, z) bases per service; (0, 0, 0) = unused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Locations {
    pub police_station: Vec<[u8; 3]>,
    pub hospital: Vec<[u8; 3]>,
    pub unused_1: Vec<[u8; 3]>,
    pub unused_2: Vec<[u8; 3]>,
    pub fire_station: Vec<[u8; 3]>,
    pub unused_3: Vec<[u8; 3]>,
}

/// `nav_data_struct` (CityScape 2.8): a named rectangle of blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NavZone {
    pub x: u8,
    pub y: u8,
    pub w: u8,
    pub h: u8,
    pub sample: u8,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct CityMap {
    /// Style number `n`: the map's style is `STYLEnnn` (CityScape 2.9; .GRY / .G24 per 3.7 / 3.8).
    pub style: u8,
    pub sample: u8,
    /// Block numbers of each column, ground first, indexed `y * 256 + x`.
    pub columns: Vec<Vec<u16>>,
    pub blocks: Vec<Block>,
    pub objects: Vec<ObjectPos>,
    pub routes: Vec<Route>,
    pub locations: Locations,
    pub nav: Vec<NavZone>,
}

impl CityMap {
    /// The blocks of column (x, y), ground first.
    pub fn column(&self, x: usize, y: usize) -> impl Iterator<Item = &Block> {
        self.columns[y * 256 + x].iter().map(|&b| &self.blocks[b as usize])
    }
}

pub fn looks_like_cmp(b: &[u8]) -> bool {
    u32_at(b, 0).is_ok_and(|v| v == VERSION)
}

pub fn parse(b: &[u8]) -> Result<CityMap, Error> {
    let version = u32_at(b, 0)?;
    if version != VERSION {
        return Err(Error::Unsupported(format!("CMP version {version} is not documented (CityScape 12.10: 331)")));
    }
    let style = *b.get(4).ok_or(Error::Malformed("truncated"))?;
    let sample = *b.get(5).ok_or(Error::Malformed("truncated"))?;
    let sizes: Vec<usize> = (0..5).map(|i| u32_at(b, 8 + i * 4).map(|v| v as usize)).collect::<Result<_, _>>()?;
    let (route_size, object_size, column_size, block_size, nav_size) =
        (sizes[0], sizes[1], sizes[2], sizes[3], sizes[4]);
    let mut p = HEADER;
    let base = slice(b, p, 65536 * 4)?;
    p += 65536 * 4;
    let column = slice(b, p, column_size)?;
    p += column_size;
    let block_bytes = slice(b, p, block_size)?;
    p += block_size;
    let objects = slice(b, p, object_size)?;
    p += object_size;
    let route = slice(b, p, route_size)?;
    p += route_size;
    let location = slice(b, p, 108)?;
    p += 108;
    let nav = slice(b, p, nav_size)?;
    p += nav_size;
    if p != b.len() {
        return Err(Error::Malformed("CMP sizes do not add up to the file size"));
    }
    if !block_size.is_multiple_of(8) || !object_size.is_multiple_of(14) || !nav_size.is_multiple_of(35) {
        return Err(Error::Malformed("CMP section size is not a whole number of records"));
    }
    let blocks: Vec<Block> = block_bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|r| Block {
            type_map: u16::from_le_bytes([r[0], r[1]]),
            type_map_ext: r[2],
            left: r[3],
            right: r[4],
            top: r[5],
            bottom: r[6],
            lid: r[7],
        })
        .collect();
    let mut columns = Vec::with_capacity(65536);
    for i in 0..65536 {
        let off = u32_at(base, i * 4)? as usize;
        if !off.is_multiple_of(2) {
            return Err(Error::Malformed("CMP column offset is not word aligned"));
        }
        let height = u16_at(column, off)? as usize;
        if height > 6 {
            return Err(Error::Malformed("CMP column height above 6"));
        }
        let n = 6 - height;
        let mut col = Vec::with_capacity(n);
        for k in (0..n).rev() {
            let num = u16_at(column, off + 2 + k * 2)?;
            if num as usize >= blocks.len() {
                return Err(Error::Malformed("CMP column names a block that does not exist"));
            }
            col.push(num);
        }
        columns.push(col);
    }
    let objects = objects
        .as_chunks::<14>()
        .0
        .iter()
        .map(|r| ObjectPos {
            x: u16::from_le_bytes([r[0], r[1]]),
            y: u16::from_le_bytes([r[2], r[3]]),
            z: u16::from_le_bytes([r[4], r[5]]),
            kind: r[6],
            remap: r[7],
            rotation: u16::from_le_bytes([r[8], r[9]]),
            pitch: u16::from_le_bytes([r[10], r[11]]),
            roll: u16::from_le_bytes([r[12], r[13]]),
        })
        .collect();
    let mut routes = Vec::new();
    let mut q = 0;
    while q < route.len() {
        let n = *route.get(q).ok_or(Error::Malformed("truncated route"))? as usize;
        let route_type = *route.get(q + 1).ok_or(Error::Malformed("truncated route"))?;
        let v = slice(route, q + 2, n * 3)?;
        routes.push(Route { route_type, vertices: v.as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect() });
        q += 2 + n * 3;
    }
    let group =
        |i: usize| location[i * 18..i * 18 + 18].as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect();
    let locations = Locations {
        police_station: group(0),
        hospital: group(1),
        unused_1: group(2),
        unused_2: group(3),
        fire_station: group(4),
        unused_3: group(5),
    };
    let nav = nav
        .as_chunks::<35>()
        .0
        .iter()
        .map(|r| {
            let name = &r[5..35];
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            NavZone {
                x: r[0],
                y: r[1],
                w: r[2],
                h: r[3],
                sample: r[4],
                name: name[..end].iter().map(|&c| c as char).collect(),
            }
        })
        .collect();
    Ok(CityMap { style, sample, columns, blocks, objects, routes, locations, nav })
}
