//! RenderWare clumps (`.dff`) with platform-independent geometry: the models of GTA III, Vice City
//! and San Andreas on PC.
//!
//! Source: librw (aap/librw @ 18532c2e, MIT): `Clump::streamRead`, `FrameList_::streamRead`
//! (`FrameStreamData`), `Atomic::streamReadClump`, `Geometry::streamRead` (`GeoStreamData`, flags in
//! rwobjects.h), `MaterialList::streamRead`, `Material::streamRead` (`MatStreamData`),
//! `Texture::streamRead`, `findChunk` (skips chunks of other types). Frame names come from the
//! Rockstar node-name extension `0x0253F2FE` (re3 src/rw/NodeName.cpp: the name's bytes).
//!
//! Geometry stored natively for a console (`Geometry::NATIVE`, the PS2 and Xbox DFFs) is refused:
//! its vertex data is in the platform's own format.

use ef_texture::rw::{self, Chunk, chunk_at};

use crate::Error;

const ID_TEXTURE: u32 = 0x06;
const ID_MATERIAL: u32 = 0x07;
const ID_MATLIST: u32 = 0x08;
const ID_FRAMELIST: u32 = 0x0E;
const ID_GEOMETRY: u32 = 0x0F;
pub const ID_CLUMP: u32 = 0x10;
const ID_LIGHT: u32 = 0x12;
const ID_ATOMIC: u32 = 0x14;
const ID_GEOMETRYLIST: u32 = 0x1A;
const ID_NODENAME: u32 = 0x0253_F2FE;

pub const GEO_PRELIT: u32 = 0x08;
pub const GEO_NORMALS: u32 = 0x10;
pub const GEO_TEXTURED: u32 = 0x04;
pub const GEO_TEXTURED2: u32 = 0x80;
pub const GEO_NATIVE: u32 = 0x0100_0000;

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// right, up, at: the rows of the RenderWare matrix.
    pub rotation: [[f32; 3]; 3],
    pub position: [f32; 3],
    pub parent: i32,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub color: [u8; 4],
    pub texture: Option<String>,
    pub mask: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub flags: u32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    /// The first texture co-ordinate set.
    pub uvs: Option<Vec<[f32; 2]>>,
    /// Prelit vertex colours.
    pub colors: Option<Vec<[u8; 4]>>,
    /// v0, v1, v2, material index.
    pub triangles: Vec<[u16; 4]>,
    pub materials: Vec<Material>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Atomic {
    pub frame: usize,
    pub geometry: usize,
    pub flags: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Clump {
    pub frames: Vec<Frame>,
    pub geometries: Vec<Geometry>,
    pub atomics: Vec<Atomic>,
}

impl Clump {
    pub fn frame_name(&self, atomic: &Atomic) -> Option<&str> {
        self.frames.get(atomic.frame)?.name.as_deref()
    }
}

const BAD: Error = Error::Malformed("RenderWare stream");

fn f32_at(b: &[u8], o: usize) -> Result<f32, Error> {
    Ok(f32::from_le_bytes(b.get(o..o + 4).ok_or(BAD)?.try_into().unwrap()))
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(b.get(o..o + 4).ok_or(BAD)?.try_into().unwrap()))
}

/// The children of `parent` (or of the whole buffer).
struct Children<'a> {
    b: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Children<'a> {
    fn of(b: &'a [u8], c: &Chunk) -> Self {
        Children { b, pos: c.body, end: c.end() }
    }
    /// librw `findChunk`: the next chunk of `id`, skipping others.
    fn find(&mut self, id: u32) -> Result<Chunk, Error> {
        while self.pos < self.end {
            let c = chunk_at(self.b, self.pos, self.end).map_err(|_| BAD)?;
            self.pos = c.end();
            if c.id == id {
                return Ok(c);
            }
        }
        Err(Error::Malformed("a RenderWare chunk is missing"))
    }
    /// The next chunk, if any.
    fn next(&mut self) -> Option<Chunk> {
        if self.pos >= self.end {
            return None;
        }
        let c = chunk_at(self.b, self.pos, self.end).ok()?;
        self.pos = c.end();
        Some(c)
    }
}

fn body<'a>(b: &'a [u8], c: &Chunk) -> &'a [u8] {
    &b[c.body..c.end()]
}

fn string(b: &[u8], c: &Chunk) -> String {
    let s = body(b, c);
    let end = s.iter().position(|&x| x == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

/// Parses the first clump of a DFF.
pub fn parse(b: &[u8]) -> Result<Clump, Error> {
    let mut top = Children { b, pos: 0, end: b.len() };
    let clump = top.find(ID_CLUMP)?;
    let mut ch = Children::of(b, &clump);
    let st = ch.find(rw::STRUCT)?;
    let s = body(b, &st);
    let num_atomics = u32_at(s, 0)? as usize;
    let num_lights = if st.version() > 0x33000 { u32_at(s, 4)? as usize } else { 0 };

    // Frame list: a struct of frames, then one extension per frame.
    let fl = ch.find(ID_FRAMELIST)?;
    let mut fc = Children::of(b, &fl);
    let fst = fc.find(rw::STRUCT)?;
    let fs = body(b, &fst);
    let n = u32_at(fs, 0)? as usize;
    let mut frames = Vec::with_capacity(n.min(4096));
    for i in 0..n {
        let o = 4 + i * 56;
        let v = |k: usize| -> Result<[f32; 3], Error> {
            Ok([f32_at(fs, o + k * 12)?, f32_at(fs, o + k * 12 + 4)?, f32_at(fs, o + k * 12 + 8)?])
        };
        frames.push(Frame {
            rotation: [v(0)?, v(1)?, v(2)?],
            position: v(3)?,
            parent: u32_at(fs, o + 48)? as i32,
            name: None,
        });
    }
    for f in frames.iter_mut() {
        let Some(ext) = fc.next() else { break };
        if ext.id != rw::EXTENSION {
            continue;
        }
        let mut ec = Children::of(b, &ext);
        while let Some(p) = ec.next() {
            if p.id == ID_NODENAME {
                f.name = Some(String::from_utf8_lossy(body(b, &p)).trim_end_matches('\0').to_string());
            }
        }
    }

    // Geometry list (version 3.4 and later; older files keep geometry inside each atomic).
    let mut geometries = Vec::new();
    let inline_geometry = st.version() < 0x30400;
    if !inline_geometry {
        let gl = ch.find(ID_GEOMETRYLIST)?;
        let mut gc = Children::of(b, &gl);
        let gs = gc.find(rw::STRUCT)?;
        let count = u32_at(body(b, &gs), 0)? as usize;
        for _ in 0..count {
            let g = gc.find(ID_GEOMETRY)?;
            geometries.push(geometry(b, &g)?);
        }
    }
    let mut atomics = Vec::with_capacity(num_atomics);
    for _ in 0..num_atomics {
        let a = ch.find(ID_ATOMIC)?;
        let mut ac = Children::of(b, &a);
        let ast = ac.find(rw::STRUCT)?;
        let s = body(b, &ast);
        let frame = u32_at(s, 0)? as usize;
        let geo = if inline_geometry {
            geometries.push(geometry(b, &ac.find(ID_GEOMETRY)?)?);
            geometries.len() - 1
        } else {
            u32_at(s, 4)? as usize
        };
        let flags = u32_at(s, 8)?;
        if frame >= frames.len() || geo >= geometries.len() {
            return Err(Error::Malformed("an atomic names a frame or geometry that does not exist"));
        }
        atomics.push(Atomic { frame, geometry: geo, flags });
    }
    let _ = (num_lights, ID_LIGHT);
    Ok(Clump { frames, geometries, atomics })
}

fn geometry(b: &[u8], g: &Chunk) -> Result<Geometry, Error> {
    let mut gc = Children::of(b, g);
    let st = gc.find(rw::STRUCT)?;
    let s = body(b, &st);
    let flags = u32_at(s, 0)?;
    let num_tris = u32_at(s, 4)? as usize;
    let num_verts = u32_at(s, 8)? as usize;
    let num_morph = u32_at(s, 12)? as usize;
    if flags & GEO_NATIVE != 0 {
        return Err(Error::Unsupported(
            "geometry stored natively for a console (PS2 / Xbox): its vertex format is not read yet".into(),
        ));
    }
    let mut p = 16;
    if st.version() < 0x34000 {
        p += 12; // surface properties
    }
    let mut sets = ((flags >> 16) & 0xFF) as usize;
    if sets == 0 {
        sets = if flags & GEO_TEXTURED != 0 {
            1
        } else if flags & GEO_TEXTURED2 != 0 {
            2
        } else {
            0
        };
    }
    let take = |p: &mut usize, n: usize| -> Result<usize, Error> {
        let at = *p;
        *p = p.checked_add(n).ok_or(BAD)?;
        if *p > s.len() {
            return Err(Error::Malformed("geometry data is truncated"));
        }
        Ok(at)
    };
    let colors = if flags & GEO_PRELIT != 0 {
        let at = take(&mut p, num_verts * 4)?;
        Some(s[at..at + num_verts * 4].as_chunks::<4>().0.to_vec())
    } else {
        None
    };
    let mut uvs = None;
    for k in 0..sets {
        let at = take(&mut p, num_verts * 8)?;
        if k == 0 {
            uvs = Some(
                (0..num_verts)
                    .map(|i| Ok([f32_at(s, at + i * 8)?, f32_at(s, at + i * 8 + 4)?]))
                    .collect::<Result<Vec<_>, Error>>()?,
            );
        }
    }
    let at = take(&mut p, num_tris * 8)?;
    let mut triangles = Vec::with_capacity(num_tris);
    for i in 0..num_tris {
        let a = u32_at(s, at + i * 8)?;
        let c = u32_at(s, at + i * 8 + 4)?;
        let t = [(a >> 16) as u16, a as u16, (c >> 16) as u16, c as u16];
        if t[..3].iter().any(|&v| v as usize >= num_verts) {
            return Err(Error::Malformed("a triangle names a vertex that does not exist"));
        }
        triangles.push(t);
    }
    // Morph target 0: bounding sphere, hasVertices, hasNormals, vertices, normals.
    let mut positions = Vec::new();
    let mut normals = None;
    for m in 0..num_morph {
        take(&mut p, 16)?;
        let has_v = u32_at(s, take(&mut p, 4)?)? != 0;
        let has_n = u32_at(s, take(&mut p, 4)?)? != 0;
        let read3 = |at: usize| -> Result<Vec<[f32; 3]>, Error> {
            (0..num_verts)
                .map(|i| Ok([f32_at(s, at + i * 12)?, f32_at(s, at + i * 12 + 4)?, f32_at(s, at + i * 12 + 8)?]))
                .collect()
        };
        if has_v {
            let at = take(&mut p, num_verts * 12)?;
            if m == 0 {
                positions = read3(at)?;
            }
        }
        if has_n {
            let at = take(&mut p, num_verts * 12)?;
            if m == 0 {
                normals = Some(read3(at)?);
            }
        }
    }
    if positions.len() != num_verts {
        return Err(Error::Malformed("geometry has no vertex positions"));
    }
    let ml = gc.find(ID_MATLIST)?;
    let materials = material_list(b, &ml)?;
    if triangles.iter().any(|t| t[3] as usize >= materials.len().max(1)) && !triangles.is_empty() {
        return Err(Error::Malformed("a triangle names a material that does not exist"));
    }
    Ok(Geometry { flags, positions, normals, uvs, colors, triangles, materials })
}

fn material_list(b: &[u8], ml: &Chunk) -> Result<Vec<Material>, Error> {
    let mut mc = Children::of(b, ml);
    let st = mc.find(rw::STRUCT)?;
    let s = body(b, &st);
    let n = u32_at(s, 0)? as usize;
    let mut out: Vec<Material> = Vec::with_capacity(n.min(1024));
    for i in 0..n {
        let index = u32_at(s, 4 + i * 4)? as i32;
        if index >= 0 {
            // A reference to an earlier material of this list.
            let m = out.get(index as usize).cloned().ok_or(Error::Malformed("material list reference"))?;
            out.push(m);
            continue;
        }
        let m = mc.find(ID_MATERIAL)?;
        let mut c = Children::of(b, &m);
        let mst = c.find(rw::STRUCT)?;
        let ms = body(b, &mst);
        let color = ms.get(4..8).ok_or(BAD)?.try_into().unwrap();
        let textured = u32_at(ms, 12)? != 0;
        let (mut texture, mut mask) = (None, None);
        if textured {
            let t = c.find(ID_TEXTURE)?;
            let mut tc = Children::of(b, &t);
            tc.find(rw::STRUCT)?;
            texture = Some(string(b, &tc.find(rw::STRING)?));
            mask = Some(string(b, &tc.find(rw::STRING)?)).filter(|m| !m.is_empty());
        }
        out.push(Material { color, texture, mask });
    }
    Ok(out)
}

/// The LOD number of a GTA III / Vice City node name (re3 FileLoader.cpp `GetNameAndLOD`): the
/// digits after the last `_l` / `_L`, 0 without one; and the name before it.
pub fn name_and_lod(node: &str) -> (&str, u32) {
    let bytes = node.as_bytes();
    let mut cut = None;
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == b'_' && (bytes[i + 1] == b'l' || bytes[i + 1] == b'L') {
            cut = Some(i);
        }
    }
    match cut {
        Some(i) => {
            let digits: String = node[i + 2..].chars().take_while(|c| c.is_ascii_digit()).collect();
            (&node[..i], digits.parse().unwrap_or(0))
        }
        None => (node, 0),
    }
}

/// Whether a San Andreas node name marks the damaged atomic (gta-reversed RwHelper.cpp
/// `GetNameAndDamage`: ends with `_dam`).
pub fn is_damaged(node: &str) -> bool {
    node.ends_with("_dam")
}

/// The atomic a map model shows when whole and at full detail:
/// * GTA III / Vice City: the atomic whose node name has LOD number 0 (re3 `SetRelatedModelInfoCB`
///   puts atomic n of `_Ln` in slot n; slot 0 is the nearest);
/// * San Andreas: the atomic not named `_dam` (gta-reversed `SetRelatedModelInfoCB`).
///
/// With several candidates the game keeps the last one RenderWare iterates, which depends on its
/// list order, so `Err` with the count is returned instead of a choice.
pub fn map_atomic(c: &Clump, san_andreas: bool) -> Result<&Atomic, usize> {
    let candidates: Vec<&Atomic> = c
        .atomics
        .iter()
        .filter(|a| {
            let name = c.frame_name(a).unwrap_or("");
            if san_andreas { !is_damaged(name) } else { name_and_lod(name).1 == 0 }
        })
        .collect();
    match candidates.as_slice() {
        [one] => Ok(one),
        many => Err(many.len()),
    }
}
