//! The placed world of GTA III, Vice City and San Andreas: object definitions (IDE), placements
//! (text IPL; San Andreas binary IPL) and each instance's matrix, as the game builds them.
//!
//! * IDE `objs` / `tobj` lines start *id, model, txd*; GTA III / Vice City then give the number of
//!   atomics and that many draw distances (re3 / reVC FileLoader.cpp `LoadObject`, `LoadTimeObject`);
//!   San Andreas `objs`, `tobj` and `anim` lines (gta-reversed FileLoader.cpp `LoadObject`,
//!   `LoadTimeObject`, `LoadAnimatedClumpObject`). San Andreas `txdp` lines name a dictionary's
//!   parent (`LoadTXDParent`).
//! * IPL `inst` lines (`LoadObjectInstance`):
//!   * GTA III: id, model, position, scale, rotation axis x y z, w;
//!   * Vice City: the same with the area (interior) after the model, or without it;
//!   * San Andreas: id, model, interior/flags, position, rotation x y z w, LOD index.
//! * San Andreas binary IPLs (`"bnry"`, gta-reversed tBinaryIplFile.h): a 0x4C-byte header of
//!   counts, offsets and sizes, then `CFileObjectInstance` records (FileObjectInstance.h: position,
//!   quaternion x y z w, model id, area code and flags, LOD index; 40 bytes).
//! * The matrix:
//!   * GTA III / Vice City rotate by -2 acos(w) about (x, y, z) (`RwMatrixRotate`, librw
//!     `Matrix::makeRotation`); the scale is read but not used.
//!   * San Andreas negates the quaternion's vector and uses it when |x| or |y| exceeds 0.05 (or the
//!     instance is "don't stream" with x and y non-zero), else a heading of acos(w) times -2 (or 2
//!     when z < 0) (`LoadObjectInstance`, Matrix.cpp `SetRotate`, `SetRotateZOnly`).
//! * Low-detail models:
//!   * GTA III / Vice City: a model whose first draw distance exceeds `LOD_DISTANCE` (300) is a
//!     "big building" (re3 SimpleModelInfo.cpp `SetupBigBuilding`, Renderer.h).
//!   * San Andreas: an instance that another instance of its IPL group names as its LOD; a group is a
//!     text IPL and the `<name>_stream*` IPLs of the IMG archives (IplStore.cpp `SetupRelatedIpls`,
//!     `LoadIpl`).

use std::collections::{BTreeMap, HashMap};

use ef_core::{Game, GameInstall};

use crate::catalog;
use crate::export::read;

/// re3 Renderer.h `LOD_DISTANCE` (with the default LOD multiplier of 1).
pub const LOD_DISTANCE: f32 = 300.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelDef {
    pub id: i32,
    pub name: String,
    pub txd: String,
    /// The first draw distance.
    pub draw_distance: f32,
    pub flags: u32,
}

/// Rows of the placement matrix in game space (Z up): the images of the x, y and z axes, then the
/// position.
pub type Matrix = [[f32; 3]; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub model: i32,
    pub name: String,
    pub matrix: Matrix,
    /// Interior / area code; 0 is the outside world.
    pub area: u32,
    pub lod_index: i32,
    pub is_lod: bool,
    /// Index into [`World::ipl_files`].
    pub file: usize,
}

#[derive(Debug, Clone, Default)]
pub struct World {
    pub models: BTreeMap<i32, ModelDef>,
    pub txd_parents: HashMap<String, String>,
    pub instances: Vec<Instance>,
    /// Dictionaries the level files load for every model (`TEXDICTION`).
    pub global_txds: Vec<String>,
    pub ipl_files: Vec<String>,
    pub problems: Vec<String>,
}

fn clean(line: &str) -> String {
    // re3 LoadLine: control characters and commas become spaces.
    line.chars().map(|c| if c < ' ' || c == ',' { ' ' } else { c }).collect::<String>().trim().to_string()
}

fn f(t: &[&str], i: usize) -> Option<f32> {
    t.get(i)?.parse().ok()
}

/// The object definitions and dictionary parents of one IDE file.
pub fn parse_ide(text: &str, game: Game, world: &mut World) {
    let mut section = String::new();
    for raw in text.lines() {
        let line = clean(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if section.is_empty() {
            section = line.get(..4).unwrap_or("").to_ascii_lowercase();
            continue;
        }
        if line.to_ascii_lowercase().starts_with("end") {
            section.clear();
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        match section.as_str() {
            "objs" | "tobj" | "anim" if game == Game::SanAndreas || section != "anim" => {
                let Some(id) = t.first().and_then(|v| v.parse::<i32>().ok()) else { continue };
                if t.len() < 4 {
                    continue;
                }
                // III/VC (and SA's long form): id model txd numObjs dist... ; SA: id model txd dist flags;
                // SA anim: id model txd anim dist flags.
                let (draw_distance, flags) = if section == "anim" {
                    (f(&t, 4).unwrap_or(0.0), t.get(5).and_then(|v| v.parse().ok()).unwrap_or(0))
                } else if let Some(n) = t[3].parse::<usize>().ok().filter(|n| (1..=3).contains(n) && t.len() >= 5 + n) {
                    (f(&t, 4).unwrap_or(0.0), t.get(4 + n).and_then(|v| v.parse().ok()).unwrap_or(0))
                } else {
                    (f(&t, 3).unwrap_or(0.0), t.get(4).and_then(|v| v.parse().ok()).unwrap_or(0))
                };
                world
                    .models
                    .insert(id, ModelDef { id, name: t[1].to_string(), txd: t[2].to_string(), draw_distance, flags });
            }
            "txdp" if t.len() >= 2 => {
                world.txd_parents.insert(t[0].to_ascii_lowercase(), t[1].to_ascii_lowercase());
            }
            _ => {}
        }
    }
}

/// GTA III / Vice City: rotation by -2 acos(w) about (x, y, z) (librw `Matrix::makeRotation`).
pub fn matrix_iii_vc(axis: [f32; 3], w: f32, pos: [f32; 3]) -> Matrix {
    let angle = -2.0 * w.clamp(-1.0, 1.0).acos();
    let len = axis.iter().map(|v| v * v).sum::<f32>();
    let inv = if len != 0.0 { 1.0 / len.sqrt() } else { 0.0 };
    let [x, y, z] = axis.map(|v| v * inv);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    [
        [c + x * x * t, x * y * t + z * s, z * x * t - y * s],
        [x * y * t - z * s, c + y * y * t, y * z * t + x * s],
        [z * x * t + y * s, y * z * t - x * s, c + z * z * t],
        pos,
    ]
}

/// San Andreas (gta-reversed `CFileLoader::LoadObjectInstance`).
pub fn matrix_sa(q: [f32; 4], dont_stream: bool, pos: [f32; 3]) -> Matrix {
    let [x, y, z, w] = q;
    if x.abs() > 0.05 || y.abs() > 0.05 || (dont_stream && x != 0.0 && y != 0.0) {
        let (x, y, z) = (-x, -y, -z);
        let (x2, y2, z2) = (2.0 * x, 2.0 * y, 2.0 * z);
        let (x2x, y2x, z2x, y2y, z2y, z2z) = (x2 * x, y2 * x, z2 * x, y2 * y, z2 * y, z2 * z);
        let (x2r, y2r, z2r) = (x2 * w, y2 * w, z2 * w);
        [
            [1.0 - (z2z + y2y), z2r + y2x, z2x - y2r],
            [y2x - z2r, 1.0 - (z2z + x2x), x2r + z2y],
            [y2r + z2x, z2y - x2r, 1.0 - (y2y + x2x)],
            pos,
        ]
    } else {
        let heading = w.clamp(-1.0, 1.0).acos() * if z < 0.0 { 2.0 } else { -2.0 };
        let (s, c) = heading.sin_cos();
        [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0], pos]
    }
}

/// The `inst` lines of a text IPL.
pub fn parse_ipl_text(text: &str, game: Game, file: usize, out: &mut Vec<Instance>) {
    let mut in_inst = false;
    for raw in text.lines() {
        let line = clean(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if !in_inst {
            in_inst = lower.starts_with("inst");
            continue;
        }
        if lower.starts_with("end") {
            in_inst = false;
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        let Some(id) = t.first().and_then(|v| v.parse::<i32>().ok()) else { continue };
        let name = t.get(1).copied().unwrap_or_default().to_string();
        let inst = match game {
            Game::SanAndreas if t.len() >= 11 => {
                let flags: u32 = t[2].parse().unwrap_or(0);
                let v = |i: usize| f(&t, i).unwrap_or(0.0);
                Some(Instance {
                    model: id,
                    name,
                    matrix: matrix_sa([v(6), v(7), v(8), v(9)], flags >> 9 & 1 != 0, [v(3), v(4), v(5)]),
                    area: flags & 0xFF,
                    lod_index: t[10].parse().unwrap_or(-1),
                    is_lod: false,
                    file,
                })
            }
            Game::Gta3 | Game::ViceCity => {
                // Vice City may put the area after the model (13 values).
                let (area, o) =
                    if game == Game::ViceCity && t.len() >= 13 { (f(&t, 2).unwrap_or(0.0) as u32, 3) } else { (0, 2) };
                if t.len() < o + 10 {
                    None
                } else {
                    let v = |i: usize| f(&t, o + i).unwrap_or(0.0);
                    Some(Instance {
                        model: id,
                        name,
                        matrix: matrix_iii_vc([v(6), v(7), v(8)], v(9), [v(0), v(1), v(2)]),
                        area,
                        lod_index: -1,
                        is_lod: false,
                        file,
                    })
                }
            }
            _ => None,
        };
        out.extend(inst);
    }
}

/// The instances of a San Andreas binary IPL.
pub fn parse_ipl_binary(b: &[u8], file: usize, out: &mut Vec<Instance>) -> Result<(), crate::Error> {
    if b.get(..4) != Some(b"bnry") {
        return Err(crate::Error::Malformed("not a binary IPL"));
    }
    let u = |o: usize| crate::u32_at(b, o);
    let count = u(4)? as usize;
    let offset = u(28)? as usize;
    for i in 0..count {
        let r = crate::slice(b, offset + i * 40, 40)?;
        let fl = |k: usize| f32::from_le_bytes(r[k * 4..k * 4 + 4].try_into().unwrap());
        let model = i32::from_le_bytes(r[28..32].try_into().unwrap());
        let flags = u32::from_le_bytes(r[32..36].try_into().unwrap());
        let lod = i32::from_le_bytes(r[36..40].try_into().unwrap());
        out.push(Instance {
            model,
            name: String::new(),
            matrix: matrix_sa([fl(3), fl(4), fl(5), fl(6)], flags >> 9 & 1 != 0, [fl(0), fl(1), fl(2)]),
            area: flags & 0xFF,
            lod_index: lod,
            is_lod: false,
            file,
        });
    }
    Ok(())
}

fn stem(p: &str) -> String {
    let name = p.rsplit('/').next().unwrap_or(p);
    name.rsplit_once('.').map_or(name, |(s, _)| s).to_ascii_lowercase()
}

/// Reads every IDE and IPL the level files name, and the San Andreas IPLs of the IMG archives.
pub fn build(install: &GameInstall) -> World {
    let game = install.game;
    let vfs = &install.vfs;
    let cat = catalog::build(install);
    let mut w = World::default();
    for f in cat.files.iter().filter(|f| f.kind == "object definitions (IDE)") {
        match read(vfs, &f.path) {
            Ok(b) => parse_ide(&String::from_utf8_lossy(&b), game, &mut w),
            Err(e) => w.problems.push(format!("{}: {e}", f.path)),
        }
    }
    w.global_txds = cat.files.iter().filter(|f| f.kind == "texture dictionary").map(|f| f.path.clone()).collect();
    // Text IPLs, then (San Andreas) the binary ones in the IMG archives.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for f in cat.files.iter().filter(|f| f.kind == "placements (IPL)" || f.kind == "placements (binary IPL)") {
        let idx = w.ipl_files.len();
        w.ipl_files.push(f.path.clone());
        let bytes = match read(vfs, &f.path) {
            Ok(b) => b,
            Err(e) => {
                w.problems.push(format!("{}: {e}", f.path));
                continue;
            }
        };
        let s = stem(&f.path);
        let group = s.split_once("_stream").map_or(s.clone(), |(g, _)| g.to_string());
        groups.entry(group).or_default().push(idx);
        if bytes.starts_with(b"bnry") {
            if let Err(e) = parse_ipl_binary(&bytes, idx, &mut w.instances) {
                w.problems.push(format!("{}: {e}", f.path));
            }
        } else {
            parse_ipl_text(&String::from_utf8_lossy(&bytes), game, idx, &mut w.instances);
        }
    }
    // Names and low-detail flags.
    for inst in &mut w.instances {
        if let Some(m) = w.models.get(&inst.model) {
            if inst.name.is_empty() {
                inst.name = m.name.clone();
            }
            if game != Game::SanAndreas && m.draw_distance > LOD_DISTANCE {
                inst.is_lod = true;
            }
        }
    }
    if game == Game::SanAndreas {
        // A LOD index points into the text IPL of the instance's group.
        let mut by_file: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, inst) in w.instances.iter().enumerate() {
            by_file.entry(inst.file).or_default().push(i);
        }
        let mut lods = Vec::new();
        for files in groups.values() {
            let Some(&text) = files.iter().find(|&&f| !stem(&w.ipl_files[f]).contains("_stream")) else { continue };
            let base = by_file.get(&text).cloned().unwrap_or_default();
            for &f in files {
                for &i in by_file.get(&f).map(Vec::as_slice).unwrap_or(&[]) {
                    let l = w.instances[i].lod_index;
                    if l >= 0 {
                        match base.get(l as usize) {
                            Some(&target) => lods.push(target),
                            None => w.problems.push(format!("{}: LOD index {l} is outside its group", w.ipl_files[f])),
                        }
                    }
                }
            }
        }
        for i in lods {
            w.instances[i].is_lod = true;
        }
    }
    let unknown = w.instances.iter().filter(|i| !w.models.contains_key(&i.model)).count();
    if unknown > 0 {
        w.problems.push(format!("{unknown} instances name a model id no IDE defines"));
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|k| m[0][k] * p[0] + m[1][k] * p[1] + m[2][k] * p[2] + m[3][k])
    }

    #[test]
    fn rotations_follow_each_game() {
        // A quarter turn: w = cos(45°), axis z. GTA III turns by -90°: x goes to -y.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let m = matrix_iii_vc([0.0, 0.0, h], h, [0.0; 3]);
        let p = apply(&m, [1.0, 0.0, 0.0]);
        assert!((p[0]).abs() < 1e-6 && (p[1] + 1.0).abs() < 1e-6, "{p:?}");
        // San Andreas' heading path gives the same for the same quaternion.
        let sa = matrix_sa([0.0, 0.0, h, h], false, [0.0; 3]);
        let q = apply(&sa, [1.0, 0.0, 0.0]);
        assert!((q[0] - p[0]).abs() < 1e-6 && (q[1] - p[1]).abs() < 1e-6, "{q:?}");
        // And its full quaternion path agrees with GTA III's axis-angle on a tilted rotation.
        let s = (0.3f32).sin();
        let axis = [s * 0.6, s * 0.8, 0.0];
        let a = matrix_iii_vc(axis, 0.3f32.cos(), [1.0, 2.0, 3.0]);
        let b = matrix_sa([axis[0], axis[1], axis[2], 0.3f32.cos()], false, [1.0, 2.0, 3.0]);
        for r in 0..4 {
            for c in 0..3 {
                assert!((a[r][c] - b[r][c]).abs() < 1e-5, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn ide_and_ipl_lines() {
        let mut w = World::default();
        parse_ide(
            "objs\n100, road, roads, 1, 120, 0\n101, LODroad, roads, 1, 1500, 0\nend\ntxdp\nroads, generic\nend\n",
            Game::Gta3,
            &mut w,
        );
        assert_eq!(w.models[&101].draw_distance, 1500.0);
        assert_eq!(w.txd_parents["roads"], "generic");
        let mut sa = World::default();
        parse_ide(
            "objs\n200, sa_obj, satxd, 299, 4\nend\nanim\n201, sa_anim, satxd, anim1, 100, 0\nend\n",
            Game::SanAndreas,
            &mut sa,
        );
        assert_eq!((sa.models[&200].draw_distance, sa.models[&200].flags), (299.0, 4));
        assert_eq!(sa.models[&201].txd, "satxd");
        let mut out = Vec::new();
        parse_ipl_text("inst\n100, road, 10, 20, 30, 1, 1, 1, 0, 0, 0, 1\nend\n", Game::Gta3, 0, &mut out);
        parse_ipl_text("inst\n100, road, 5, 10, 20, 30, 1, 1, 1, 0, 0, 0, 1\nend\n", Game::ViceCity, 0, &mut out);
        parse_ipl_text("inst\n200, sa_obj, 0, 1, 2, 3, 0, 0, 0, 1, -1\nend\n", Game::SanAndreas, 0, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].matrix[3], [10.0, 20.0, 30.0]);
        assert_eq!((out[1].area, out[1].matrix[3]), (5, [10.0, 20.0, 30.0]));
        assert_eq!(out[2].matrix[3], [1.0, 2.0, 3.0]);
    }
}
