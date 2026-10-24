//! Writes a placed world as a glTF 2.0 scene (`scene.gltf`, `scene.bin`, `textures/`).
//!
//! glTF 2.0 (Khronos): one mesh per model, one node per placed instance with its matrix
//! (column-major), grouped by area (0 = the outside world) and by full detail / low detail. A root
//! node turns the game's Z-up axes into glTF's Y-up. Prelit vertex colours become `COLOR_0` (they
//! multiply the texture, as in the game); textures are PNG files the scene refers to. Materials are
//! double-sided, unlit-looking viewers aside: the culling the game uses is not recorded in the files.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use crate::Error;
use crate::scene::Scene;
use crate::world::World;

struct Bin {
    w: BufWriter<fs::File>,
    len: usize,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn push(&mut self, bytes: &[u8], target: Option<u32>) -> Result<usize, Error> {
        while !self.len.is_multiple_of(4) {
            self.w.write_all(&[0])?;
            self.len += 1;
        }
        self.w.write_all(bytes)?;
        let mut v = json!({"buffer": 0, "byteOffset": self.len, "byteLength": bytes.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.len += bytes.len();
        self.views.push(v);
        Ok(self.views.len() - 1)
    }
    fn accessor(&mut self, view: usize, component: u32, count: usize, kind: &str, extra: Value) -> usize {
        let mut a = json!({"bufferView": view, "componentType": component, "count": count, "type": kind});
        if let Value::Object(m) = extra {
            for (k, v) in m {
                a[k] = v;
            }
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }
}

const FLOAT: u32 = 5126;
const UBYTE: u32 = 5121;
const UINT: u32 = 5125;
const ARRAY: u32 = 34962;
const ELEMENT: u32 = 34963;

fn f32s<const N: usize>(v: &[[f32; N]]) -> Vec<u8> {
    v.iter().flat_map(|a| a.iter().flat_map(|f| f.to_le_bytes())).collect()
}

/// Writes the scene of `instances` (indices into `world.instances`). Returns the number of nodes.
pub fn write(
    world: &World,
    scene: &Scene,
    instances: &[usize],
    out: &Path,
    progress: &dyn Fn(usize, usize),
    cancel: &AtomicBool,
) -> Result<usize, Error> {
    fs::create_dir_all(out.join("textures"))?;
    let bin_path = out.join("scene.bin");
    let mut bin = Bin { w: BufWriter::new(fs::File::create(&bin_path)?), len: 0, views: vec![], accessors: vec![] };
    // Textures and images.
    let mut images = Vec::new();
    let mut textures = Vec::new();
    for (i, t) in scene.textures.iter().enumerate() {
        let file =
            format!("textures/{}/{}.png", ef_texture::export::sanitize(&t.txd), ef_texture::export::sanitize(&t.name));
        let path = out.join(&file);
        fs::create_dir_all(path.parent().unwrap())?;
        ef_texture::export::write_png(&path, &t.image)?;
        images.push(json!({"uri": file, "name": format!("{}/{}", t.txd, t.name)}));
        textures.push(json!({"source": i, "sampler": 0}));
    }
    let materials: Vec<Value> = scene
        .materials
        .iter()
        .map(|m| {
            let c = m.color.map(|v| v as f32 / 255.0);
            let factor = if m.texture.is_some() && !m.modulate { [1.0, 1.0, 1.0, 1.0] } else { c };
            let mut pbr = json!({"baseColorFactor": factor, "metallicFactor": 0.0, "roughnessFactor": 1.0});
            let mut v = json!({"doubleSided": true});
            if let Some(t) = m.texture {
                pbr["baseColorTexture"] = json!({"index": t});
                let tex = &scene.textures[t];
                if tex.translucent {
                    v["alphaMode"] = json!("BLEND");
                } else if tex.cutout {
                    v["alphaMode"] = json!("MASK");
                }
            }
            v["pbrMetallicRoughness"] = pbr;
            v
        })
        .collect();
    // Meshes.
    let mut meshes = Vec::new();
    for (k, m) in scene.meshes.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(0);
        }
        if k % 256 == 0 {
            progress(k, scene.meshes.len() + instances.len());
        }
        let n = m.positions.len();
        let pv = bin.push(&f32s(&m.positions), Some(ARRAY))?;
        let pos = bin.accessor(pv, FLOAT, n, "VEC3", json!({"min": m.bounds[0], "max": m.bounds[1]}));
        let mut attrs = json!({"POSITION": pos});
        if let Some(nrm) = &m.normals {
            let v = bin.push(&f32s(nrm), Some(ARRAY))?;
            attrs["NORMAL"] = json!(bin.accessor(v, FLOAT, n, "VEC3", json!({})));
        }
        if let Some(uv) = &m.uvs {
            let v = bin.push(&f32s(uv), Some(ARRAY))?;
            attrs["TEXCOORD_0"] = json!(bin.accessor(v, FLOAT, n, "VEC2", json!({})));
        }
        if let Some(c) = &m.colors {
            let bytes: Vec<u8> = c.iter().flatten().copied().collect();
            let v = bin.push(&bytes, Some(ARRAY))?;
            attrs["COLOR_0"] = json!(bin.accessor(v, UBYTE, n, "VEC4", json!({"normalized": true})));
        }
        let mut prims = Vec::new();
        for p in &m.parts {
            let bytes: Vec<u8> = p.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let v = bin.push(&bytes, Some(ELEMENT))?;
            let idx = bin.accessor(v, UINT, p.indices.len(), "SCALAR", json!({}));
            prims.push(json!({"attributes": attrs.clone(), "indices": idx, "material": p.material}));
        }
        meshes.push(json!({"primitives": prims}));
    }
    // Nodes: root -> group -> instance.
    let mut nodes: Vec<Value> = Vec::new();
    let mut groups: BTreeMap<(u32, bool), Vec<usize>> = BTreeMap::new();
    for (k, &i) in instances.iter().enumerate() {
        if k % 4096 == 0 {
            progress(scene.meshes.len() + k, scene.meshes.len() + instances.len());
        }
        let inst = &world.instances[i];
        let Some(Some(mesh)) = scene.by_model.get(&inst.model) else { continue };
        let [r, f, u, p] = inst.matrix;
        let matrix = [r[0], r[1], r[2], 0.0, f[0], f[1], f[2], 0.0, u[0], u[1], u[2], 0.0, p[0], p[1], p[2], 1.0];
        nodes.push(json!({"name": format!("{} ({})", inst.name, inst.model), "mesh": mesh, "matrix": matrix}));
        groups.entry((inst.area, inst.is_lod)).or_default().push(nodes.len() - 1);
    }
    let placed = nodes.len();
    let mut group_nodes = Vec::new();
    for ((area, lod), children) in groups {
        let name = format!(
            "{} — {}",
            if area == 0 { "World".to_string() } else { format!("Area {area}") },
            if lod { "low detail (LOD)" } else { "full detail" }
        );
        nodes.push(json!({"name": name, "children": children}));
        group_nodes.push(nodes.len() - 1);
    }
    let h = std::f32::consts::FRAC_1_SQRT_2;
    nodes.push(json!({"name": "Game world (Z up)", "rotation": [-h, 0.0, 0.0, h], "children": group_nodes}));
    let root = nodes.len() - 1;
    bin.w.flush()?;
    drop(bin.w);
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": format!("Eyefinder {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"nodes": [root]}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "textures": textures,
        "images": images,
        "samplers": [{"wrapS": 10497, "wrapT": 10497}],
        "buffers": [{"uri": "scene.bin", "byteLength": bin.len}],
        "bufferViews": bin.views,
        "accessors": bin.accessors,
    });
    // glTF: top-level arrays, when present, hold at least one item.
    if let Value::Object(m) = &mut doc {
        m.retain(|_, v| !matches!(v, Value::Array(a) if a.is_empty()));
        if bin.len == 0 {
            m.remove("buffers");
        }
    }
    fs::write(out.join("scene.gltf"), serde_json::to_vec(&doc).map_err(|e| Error::Io(std::io::Error::other(e)))?)?;
    Ok(placed)
}
