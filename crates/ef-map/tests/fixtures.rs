//! Synthetic map fixtures (tools/fixtures/map_fixtures.py) against the reference that script computes
//! independently: image hashes of the expected top-down and radar maps, and the parsed records.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ef_core::vfs::{FolderVfs, Vfs};
use ef_core::{Game, GameInstall, Platform};
use ef_map::{catalog, cmp, export, gmp, radar, topdown};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn reference() -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(fixtures().join("map_reference.json")).unwrap()).unwrap()
}

fn fnv(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn assert_image(name: &str, w: usize, h: usize, pixels: &[u8]) {
    let r = &reference()[name];
    assert_eq!((w as u64, h as u64), (r["width"].as_u64().unwrap(), r["height"].as_u64().unwrap()), "{name}");
    assert_eq!(fnv(pixels), r["fnv"].as_str().unwrap(), "{name} pixels");
}

fn install(dir: &str, game: Game) -> GameInstall {
    GameInstall {
        game,
        platform: Platform::Pc,
        edition: ef_core::edition::Edition::unknown(),
        vfs: Arc::new(FolderVfs::new(fixtures().join(dir))),
        evidence: vec![],
        user_choice: true,
        store: ef_core::store::Store::Unknown,
    }
}

#[test]
fn gta1_city_map_and_top_down() {
    let dir = fixtures().join("gta1/GTADATA");
    let m = cmp::parse(&std::fs::read(dir.join("NYC.CMP")).unwrap()).unwrap();
    let r = &reference()["gta1"];
    assert_eq!(m.objects.iter().filter(|o| !o.is_car()).count() as u64, r["objects"].as_u64().unwrap());
    assert_eq!(m.objects.iter().filter(|o| o.is_car()).count() as u64, r["cars"].as_u64().unwrap());
    assert_eq!(m.routes.len() as u64, r["routes"].as_u64().unwrap());
    let nav: Vec<&str> = m.nav.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(serde_json::json!(nav), r["nav"]);
    assert_eq!(m.locations.police_station.iter().filter(|b| **b != [0, 0, 0]).count(), 1);
    assert_eq!(m.style, 1);
    // Ground first: the flat lid 2 sits on lid 1.
    assert_eq!(m.column(10, 21).map(|b| b.lid).collect::<Vec<_>>(), [1, 2]);

    let style = ef_texture::style::parse(&std::fs::read(dir.join("STYLE001.GRY")).unwrap()).unwrap();
    let tiles = topdown::Tiles::gta1_lids(&style).unwrap();
    let src = topdown::CitySource { map: &m, variants: true };
    let e = topdown::extent(&src).unwrap();
    assert_eq!(e, (10, 20, 14, 22));
    let img = topdown::render_band(&src, &tiles, e.0, e.2, e.1, e.3);
    assert_image("gta1_topdown", (e.2 - e.0) * 64, (e.3 - e.1) * 64, &img);
}

#[test]
fn gta2_map_and_top_down() {
    let dir = fixtures().join("gta2/data");
    let m = gmp::parse(&std::fs::read(dir.join("wil.gmp")).unwrap()).unwrap();
    let r = &reference()["gta2"];
    let zones: Vec<(u8, &str)> = m.zones.iter().map(|z| (z.zone_type, z.name.as_str())).collect();
    assert_eq!(serde_json::json!(zones), r["zones"]);
    assert_eq!(m.objects.len() as u64, r["objects"].as_u64().unwrap());
    assert_eq!(m.lights.len() as u64, r["lights"].as_u64().unwrap());
    assert_eq!(m.animations.len() as u64, r["animations"].as_u64().unwrap());
    assert_eq!(m.animations[0].tiles, [3, 4]);
    let mut chunks: Vec<&str> = m.chunks.iter().map(|(n, _)| n.as_str()).collect();
    chunks.sort();
    assert_eq!(serde_json::json!(chunks), r["chunks"]);
    assert_eq!(m.map_chunk, "DMAP");
    assert_eq!((gmp::fix16(m.objects[0].x), gmp::fix16(m.objects[0].y)), (6.5, 5.25));

    let style = ef_texture::sty::parse(&std::fs::read(dir.join("wil.sty")).unwrap()).unwrap();
    let tiles = topdown::Tiles::gta2_tiles(&style).unwrap();
    let src = topdown::GbhSource { map: &m };
    let e = topdown::extent(&src).unwrap();
    assert_eq!(e, (5, 5, 9, 8));
    let img = topdown::render_band(&src, &tiles, e.0, e.2, e.1, e.3);
    assert_image("gta2_topdown", (e.2 - e.0) * 64, (e.3 - e.1) * 64, &img);
}

#[test]
fn radar_maps() {
    for (dir, game, name) in [("gta3", Game::Gta3, "gta3_radar"), ("sa", Game::SanAndreas, "sa_radar")] {
        let vfs: Arc<dyn Vfs> = Arc::new(FolderVfs::new(fixtures().join(dir)));
        let grid = radar::grid(game).unwrap();
        let files = radar::find(&vfs, grid);
        assert_eq!(files.found(), grid.side * grid.side, "{dir}");
        let pic = radar::radar_map(&vfs, grid, &files, &|_, _| {}).unwrap();
        assert!(pic.problems.is_empty(), "{:?}", pic.problems);
        assert_image(name, pic.image.width as usize, pic.image.height as usize, &pic.image.pixels);
    }
}

#[test]
fn catalogs_pair_maps_and_styles() {
    let g2 = catalog::build(&install("gta2", Game::Gta2));
    assert_eq!(g2.levels.len(), 1);
    assert_eq!(g2.levels[0].name, "Downtown (MAIN)");
    assert_eq!(g2.levels[0].styles[0].path.to_ascii_lowercase(), "data/wil.sty");
    let g1 = catalog::build(&install("gta1", Game::Gta1));
    assert_eq!(g1.levels.len(), 1);
    assert_eq!(g1.levels[0].styles.len(), 1, "{:?}", g1.notices);
    assert!(g1.levels[0].styles[0].why.contains("style number 1"));
    // London 1969 reads only the UK subtree: this copy has none.
    assert!(catalog::build(&install("gta1", Game::London1969)).levels.is_empty());
    let g3 = catalog::build(&install("gta3", Game::Gta3));
    assert_eq!(g3.files.iter().filter(|f| f.kind == "radar tile").count(), 64);
    assert!(!g3.files.iter().any(|f| f.path.ends_with("other.dff")), "not named by an IDE");
    assert!(g3.notices.iter().any(|n| n.contains("DEFAULT.DAT")));
    assert!(catalog::availability(Game::Gta2, Platform::Ps1).raw.is_err());
    assert!(catalog::availability(Game::ViceCity, Platform::Ps2).premade.is_ok());
}

#[test]
fn placed_worlds_scene_and_top_down() {
    use ef_map::{gltf, render3d, scene, world};
    for (dir, game, key) in [("world3", Game::Gta3, "iii_world"), ("world_sa", Game::SanAndreas, "sa_world")] {
        let inst = install(dir, game);
        let w = world::build(&inst);
        let r = &reference()[key];
        assert!(w.problems.is_empty(), "{dir}: {:?}", w.problems);
        assert_eq!(w.instances.len() as u64, r["instances"].as_u64().unwrap(), "{dir}");
        let lods: Vec<bool> = w.instances.iter().map(|i| i.is_lod).collect();
        assert_eq!(serde_json::json!(lods), r["lod"], "{dir}");
        let all: Vec<usize> = (0..w.instances.len()).collect();
        let cancel = AtomicBool::new(false);
        let sc = scene::load(&inst, &w, &all, &|_, _| {}, &cancel);
        assert!(sc.problems.is_empty(), "{dir}: {:?}", sc.problems);
        assert!(sc.by_model.values().all(Option::is_some), "{dir}");
        let detail: Vec<usize> =
            all.iter().copied().filter(|&i| w.instances[i].area == 0 && !w.instances[i].is_lod).collect();
        let out = std::env::temp_dir().join(format!("ef-map-world-{dir}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let png_path = out.join("top.png");
        let view = render3d::View { min: 0.0, size: 64.0, ppu: 1.0 };
        render3d::render(&w, &sc, &detail, &view, &png_path, &|_, _| {}, &cancel).unwrap().unwrap();
        let mut dec =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png_path).unwrap())).read_info().unwrap();
        let mut px = vec![0; dec.output_buffer_size().unwrap()];
        dec.next_frame(&mut px).unwrap();
        let rr = &r["render"];
        assert_eq!(fnv(&px), rr["fnv"].as_str().unwrap(), "{dir} top-down render");
        // The glTF scene: one node per instance, its groups and the root; every accessor in range.
        let n = gltf::write(&w, &sc, &all, &out.join("scene"), &|_, _| {}, &cancel).unwrap();
        assert_eq!(n, w.instances.len());
        let doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("scene/scene.gltf")).unwrap()).unwrap();
        let bin = std::fs::metadata(out.join("scene/scene.bin")).unwrap().len();
        assert_eq!(doc["buffers"][0]["byteLength"].as_u64().unwrap(), bin);
        for v in doc["bufferViews"].as_array().unwrap() {
            assert!(v["byteOffset"].as_u64().unwrap() + v["byteLength"].as_u64().unwrap() <= bin);
        }
        let nodes = doc["nodes"].as_array().unwrap();
        let groups = nodes.iter().filter(|n| n.get("children").is_some()).count();
        assert_eq!(nodes.len(), n + groups);
        assert!(nodes.iter().any(|n| n["name"].as_str().is_some_and(|s| s.contains("low detail"))), "{dir}");
        for t in doc["images"].as_array().unwrap() {
            assert!(out.join("scene").join(t["uri"].as_str().unwrap()).exists());
        }
        std::fs::remove_dir_all(&out).unwrap();
    }
}

#[test]
fn export_writes_every_output_and_a_manifest() {
    let out = std::env::temp_dir().join(format!("ef-map-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    for (dir, game) in [("gta2", Game::Gta2), ("gta3", Game::Gta3)] {
        let inst = install(dir, game);
        let cat = catalog::build(&inst);
        let opts = export::ExportOptions {
            raw: true,
            individual: true,
            topdown: true,
            tiles: true,
            premade: true,
            ..Default::default()
        };
        let dest = out.join(dir);
        let r = export::export(&inst, &cat, &opts, &dest, &|_, _, _| {}, &AtomicBool::new(false)).unwrap();
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dest.join("manifest.json")).unwrap()).unwrap();
        let paths: Vec<&str> =
            manifest["outputs"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap()).collect();
        for p in &paths {
            assert!(dest.join(p.trim_end_matches('/')).exists(), "{p}");
        }
        if game == Game::Gta2 {
            assert!(paths.contains(&"Downtown (MAIN)/topdown-wil-sty.png"), "{paths:?}");
            assert!(paths.contains(&"Downtown (MAIN)/map.json") && paths.contains(&"Downtown (MAIN)/overlay.svg"));
            let png = std::fs::read(dest.join("Downtown (MAIN)/topdown-wil-sty.png")).unwrap();
            let dec = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
            assert_eq!((dec.info().width, dec.info().height), (256, 192));
            assert!(dest.join("Downtown (MAIN)/tiles-wil-sty/x005_y005.png").exists());
        } else {
            assert!(paths.contains(&"radar_map.png") && paths.contains(&"radar_tiles/radar63.png"));
            assert!(dest.join("raw/models/gta3.img/radar00.txd").exists());
        }
    }
    std::fs::remove_dir_all(out).unwrap();
}
