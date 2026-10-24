//! The models and textures of a placed world, loaded once each for the glTF scene and the top-down
//! render.
//!
//! * A model's geometry is the atomic the game shows at full detail ([`crate::dff::map_atomic`]),
//!   with no frame transform: map atomics are given a new, empty frame (re3 / gta-reversed
//!   `SetRelatedModelInfoCB`). Its DFF is the IMG entry (or loose file) `<model>.dff`.
//! * A texture is looked up in the model's dictionary `<txd>.txd`, then (San Andreas) its `txdp`
//!   parents, then the dictionaries the level files load for everything (`TEXDICTION`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ef_core::vfs::{Vfs, walk_files};
use ef_core::{Game, GameInstall};
use ef_texture::decode::Rgba;

use crate::dff;
use crate::export::read;
use crate::world::World;

#[derive(Debug, Clone)]
pub struct Part {
    pub material: usize,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub colors: Option<Vec<[u8; 4]>>,
    pub parts: Vec<Part>,
    /// Model-space bounds: min, max.
    pub bounds: [[f32; 3]; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMaterial {
    pub color: [u8; 4],
    pub texture: Option<usize>,
    /// Geometry flag MODULATE (librw rwobjects.h): the material colour multiplies the texture.
    pub modulate: bool,
}

#[derive(Debug, Clone)]
pub struct SceneTexture {
    pub txd: String,
    pub name: String,
    pub image: Arc<Rgba>,
    /// Some texel is neither fully opaque nor fully transparent.
    pub translucent: bool,
    /// Some texel is transparent.
    pub cutout: bool,
}

#[derive(Default)]
pub struct Scene {
    /// Model id -> mesh index; `None` when the model could not be loaded (see `problems`).
    pub by_model: HashMap<i32, Option<usize>>,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<SceneMaterial>,
    pub textures: Vec<SceneTexture>,
    pub problems: Vec<String>,
}

/// Where each file name (lower case) of the copy is: (archive, entry) for IMG entries, else the path.
struct Files {
    by_name: HashMap<String, (Option<String>, String)>,
}

impl Files {
    fn index(vfs: &Arc<dyn Vfs>) -> Files {
        let mut by_name = HashMap::new();
        for (path, _) in walk_files(vfs.as_ref(), "") {
            let name = path.rsplit('/').next().unwrap_or(&path).to_ascii_lowercase();
            if ef_core::img::is_img_name(&path)
                && let Ok(img) = ef_core::img::ImgVfs::open(vfs, &path)
            {
                for e in img.entries() {
                    by_name.entry(e.name.to_ascii_lowercase()).or_insert((Some(path.clone()), e.name.clone()));
                }
            }
            by_name.entry(name).or_insert((None, path));
        }
        Files { by_name }
    }
    fn read(&self, vfs: &Arc<dyn Vfs>, name: &str) -> Option<Vec<u8>> {
        let (archive, entry) = self.by_name.get(&name.to_ascii_lowercase())?;
        match archive {
            Some(a) => read(vfs, &format!("{a}/{entry}")).ok(),
            None => vfs.read_all(entry).ok(),
        }
    }
}

struct Dictionaries {
    loaded: HashMap<String, Option<HashMap<String, usize>>>,
}

/// Loads the meshes of the models that `wanted` instances use.
pub fn load(
    install: &GameInstall,
    world: &World,
    wanted: &[usize],
    progress: &dyn Fn(usize, usize),
    cancel: &AtomicBool,
) -> Scene {
    let vfs = &install.vfs;
    let sa = install.game == Game::SanAndreas;
    let files = Files::index(vfs);
    let mut scene = Scene::default();
    let mut dicts = Dictionaries { loaded: HashMap::new() };
    let mut models: Vec<i32> = wanted.iter().map(|&i| world.instances[i].model).collect();
    models.sort_unstable();
    models.dedup();
    let globals: Vec<String> = world
        .global_txds
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p).rsplit_once('.').map_or(p.as_str(), |(s, _)| s).to_ascii_lowercase())
        .collect();
    let mut problems: HashMap<String, usize> = HashMap::new();
    for (k, id) in models.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if k % 64 == 0 {
            progress(k, models.len());
        }
        let Some(def) = world.models.get(id) else {
            scene.by_model.insert(*id, None);
            continue;
        };
        let mesh = (|| -> Result<usize, String> {
            let bytes =
                files.read(vfs, &format!("{}.dff", def.name)).ok_or_else(|| "model file not found".to_string())?;
            let clump = dff::parse(&bytes).map_err(|e| e.to_string())?;
            let atomic = *dff::map_atomic(&clump, sa).map_err(|n| {
                format!(
                    "{n} atomics could be the full-detail one; the game's choice depends on RenderWare's list order"
                )
            })?;
            let g = &clump.geometries[atomic.geometry];
            let mut parts: Vec<Part> = Vec::new();
            let mut material_of = Vec::with_capacity(g.materials.len());
            for m in &g.materials {
                let texture = m.texture.as_ref().and_then(|t| {
                    texture(vfs, &files, &mut dicts, &mut scene.textures, world, &def.txd, &globals, t, &mut problems)
                });
                let sm = SceneMaterial { color: m.color, texture, modulate: g.flags & 0x40 != 0 };
                let idx = scene.materials.iter().position(|x| *x == sm).unwrap_or_else(|| {
                    scene.materials.push(sm.clone());
                    scene.materials.len() - 1
                });
                material_of.push(idx);
            }
            for t in &g.triangles {
                let mat = material_of.get(t[3] as usize).copied().unwrap_or(0);
                match parts.iter_mut().find(|p| p.material == mat) {
                    Some(p) => p.indices.extend([t[0] as u32, t[1] as u32, t[2] as u32]),
                    None => parts.push(Part { material: mat, indices: vec![t[0] as u32, t[1] as u32, t[2] as u32] }),
                }
            }
            let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
            for p in &g.positions {
                for k in 0..3 {
                    bounds[0][k] = bounds[0][k].min(p[k]);
                    bounds[1][k] = bounds[1][k].max(p[k]);
                }
            }
            if g.positions.is_empty() {
                bounds = [[0.0; 3]; 2];
            }
            scene.meshes.push(Mesh {
                positions: g.positions.clone(),
                normals: g.normals.clone(),
                uvs: g.uvs.clone(),
                colors: g.colors.clone(),
                parts,
                bounds,
            });
            Ok(scene.meshes.len() - 1)
        })();
        match mesh {
            Ok(m) => {
                scene.by_model.insert(*id, Some(m));
            }
            Err(e) => {
                *problems.entry(e.clone()).or_default() += 1;
                if problems[&e] <= 3 {
                    scene.problems.push(format!("{}: {e}", def.name));
                }
                scene.by_model.insert(*id, None);
            }
        }
    }
    for (p, n) in problems {
        if n > 3 {
            scene.problems.push(format!("{n} models in all: {p}"));
        }
    }
    progress(models.len(), models.len());
    scene
}

#[allow(clippy::too_many_arguments)]
fn texture(
    vfs: &Arc<dyn Vfs>,
    files: &Files,
    dicts: &mut Dictionaries,
    textures: &mut Vec<SceneTexture>,
    world: &World,
    txd: &str,
    globals: &[String],
    name: &str,
    problems: &mut HashMap<String, usize>,
) -> Option<usize> {
    let mut chain = vec![txd.to_ascii_lowercase()];
    while let Some(parent) = world.txd_parents.get(chain.last().unwrap()) {
        if chain.contains(parent) || chain.len() > 16 {
            break;
        }
        chain.push(parent.clone());
    }
    chain.extend(globals.iter().cloned());
    let want = name.to_ascii_lowercase();
    for d in &chain {
        if !dicts.loaded.contains_key(d) {
            let entry =
                files.read(vfs, &format!("{d}.txd")).and_then(|b| ef_texture::collection::parse(&b).ok()).map(|c| {
                    let mut names = HashMap::new();
                    for t in &c.textures {
                        let Ok(img) = t.raster.decode(0) else { continue };
                        let translucent = img.pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0 && p[3] != 255);
                        let cutout = img.pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0);
                        textures.push(SceneTexture {
                            txd: d.clone(),
                            name: t.name.clone(),
                            image: Arc::new(img),
                            translucent,
                            cutout,
                        });
                        names.insert(t.name.to_ascii_lowercase(), textures.len() - 1);
                    }
                    names
                });
            dicts.loaded.insert(d.clone(), entry);
        }
        if let Some(Some(names)) = dicts.loaded.get(d)
            && let Some(&i) = names.get(&want)
        {
            return Some(i);
        }
    }
    *problems.entry(format!("texture {name} not found in {txd}.txd or its parents")).or_default() += 1;
    None
}
