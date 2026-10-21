//! Checks the texture decoders against tests/fixtures/reference.json, which was produced by
//! tools/txd-ref (librw for the stored level bytes and the PS2 unswizzled texels, an exact
//! specification decoder for the RGBA; see that script's documentation).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ef_core::img::ImgVfs;
use ef_core::vfs::{FolderVfs, Vfs};
use ef_core::{Game, GameInstall, Platform};
use ef_texture::raster::NativePlatform;
use ef_texture::{decode, txd};
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fnv(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

#[test]
fn every_fixture_texture_matches_the_reference() {
    let reference: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("reference.json")).unwrap()).unwrap();
    let mut checked = (0, 0, 0);
    for (file, textures) in reference.as_object().unwrap() {
        if file.starts_with('_') {
            continue;
        }
        let bytes = std::fs::read(fixtures().join(file)).unwrap();
        let t = txd::parse(&bytes).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(t.problems.is_empty(), "{file}: {:?}", t.problems);
        let textures = textures.as_array().unwrap();
        assert_eq!(t.textures.len(), textures.len(), "{file}: texture count");
        for (tex, r) in t.textures.iter().zip(textures) {
            let ctx = format!("{file} {}", tex.name);
            assert_eq!(tex.name, r["name"].as_str().unwrap(), "{ctx}");
            let ras = &tex.raster;
            assert_eq!(ras.width as u64, r["width"].as_u64().unwrap(), "{ctx}: width");
            assert_eq!(ras.height as u64, r["height"].as_u64().unwrap(), "{ctx}: height");
            let supported = r["supported"].as_bool().unwrap();
            assert_eq!(ras.support().0.can_decode(), supported, "{ctx}: support level");
            let levels = r["levels"].as_array().unwrap();
            assert_eq!(ras.levels.len(), levels.len(), "{ctx}: mip levels");
            for (i, (l, rl)) in ras.levels.iter().zip(levels).enumerate() {
                assert_eq!(l.width as u64, rl["w"].as_u64().unwrap(), "{ctx}: level {i} width");
                assert_eq!(l.height as u64, rl["h"].as_u64().unwrap(), "{ctx}: level {i} height");
                if ras.platform == NativePlatform::Ps2 {
                    let texels = decode::ps2_texels(ras, i).unwrap();
                    assert_eq!(fnv(&texels), rl["texels"].as_str().unwrap(), "{ctx}: level {i} texels");
                } else {
                    assert_eq!(fnv(&l.data), rl["fnv"].as_str().unwrap(), "{ctx}: level {i} bytes");
                }
                checked.1 += 1;
            }
            if supported {
                let img = ras.decode(0).unwrap_or_else(|e| panic!("{ctx}: {e}"));
                assert_eq!(fnv(&img.pixels), r["rgba_spec"].as_str().unwrap(), "{ctx}: RGBA");
                // every further level decodes too
                for i in 1..ras.levels.len() {
                    let m = ras.decode(i).unwrap_or_else(|e| panic!("{ctx} level {i}: {e}"));
                    assert_eq!(m.pixels.len(), (m.width * m.height * 4) as usize);
                }
                checked.0 += 1;
            } else {
                assert!(ras.decode(0).is_err(), "{ctx}: unsupported textures must not decode");
                checked.2 += 1;
            }
        }
    }
    assert!(checked.0 >= 90, "only {} textures were compared", checked.0);
    println!("{} textures decoded, {} levels compared, {} correctly refused", checked.0, checked.1, checked.2);
}

#[test]
fn img_archives_expose_their_entries() {
    let vfs: Arc<dyn Vfs> = Arc::new(FolderVfs::new(fixtures()));
    let v1 = ImgVfs::open(&vfs, "gta3_v1.img").unwrap();
    let names: Vec<String> = v1.list("").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["loadsc0.txd", "PS2.TXD", "readme.txt"]);
    let inner = v1.read_all("loadsc0.txd").unwrap();
    let loose = std::fs::read(fixtures().join("d3d8_vc.txd")).unwrap();
    assert_eq!(&inner[..loose.len()], &loose[..]);
    assert!(inner[loose.len()..].iter().all(|&b| b == 0), "sector padding");
    let v2 = ImgVfs::open(&vfs, "gta3_v2.img").unwrap();
    let inner = v2.read_all("VEHICLE.TXD").unwrap();
    let loose = std::fs::read(fixtures().join("d3d9_sa.txd")).unwrap();
    assert_eq!(&inner[..loose.len()], &loose[..]);
}

#[test]
fn catalog_finds_loose_and_archived_dictionaries() {
    let install = GameInstall {
        game: Game::SanAndreas,
        platform: Platform::Pc,
        edition: ef_core::Edition::unknown(),
        vfs: Arc::new(FolderVfs::new(fixtures())),
        evidence: vec![],
        user_choice: false,
    };
    let cat = ef_texture::catalog::build(&install, &|_, _| {}, &AtomicBool::new(false));
    let paths: Vec<&str> = cat.sources.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths.len(), 11, "{paths:?}");
    assert!(paths.contains(&"gta3_v1.img/loadsc0.txd"));
    assert!(paths.contains(&"gta3_v1.img/PS2.TXD"));
    assert!(paths.contains(&"gta3_v2.img/vehicle.txd"));
    // decoding through the catalog gives the same pixels as the loose copy
    let a = cat.assets.iter().position(|a| cat.sources[a.source].path == "gta3_v2.img/vehicle.txd").unwrap();
    let via_archive = cat.load(&install, cat.assets[a].source).unwrap();
    let loose = txd::parse(&std::fs::read(fixtures().join("d3d9_sa.txd")).unwrap()).unwrap();
    assert_eq!(via_archive.textures[0].raster.decode(0).unwrap(), loose.textures[0].raster.decode(0).unwrap());
}

#[test]
fn export_writes_png_dds_and_manifest() {
    let install = GameInstall {
        game: Game::Gta3,
        platform: Platform::Pc,
        edition: ef_core::Edition::unknown(),
        vfs: Arc::new(FolderVfs::new(fixtures())),
        evidence: vec![],
        user_choice: false,
    };
    let cat = ef_texture::catalog::build(&install, &|_, _| {}, &AtomicBool::new(false));
    let pick: Vec<usize> =
        (0..cat.assets.len()).filter(|&i| cat.sources[cat.assets[i].source].path == "d3d8_vc.txd").collect();
    let out = std::env::temp_dir().join(format!("ef-tex-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let opts = ef_texture::export::ExportOptions { mipmaps: true, originals: true };
    let rep =
        ef_texture::export::export(&install, &cat, &pick, &out, opts, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(rep.skipped.len(), 1, "DXT2 is skipped: {:?}", rep.skipped);
    assert_eq!(rep.textures, pick.len() - 1);
    // a PNG decodes back to the texture's RGBA
    let png = std::fs::read(out.join("d3d8_vc/d3d8_vc_c565.png")).unwrap();
    let mut dec = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut buf = vec![0; dec.output_buffer_size().unwrap()];
    dec.next_frame(&mut buf).unwrap();
    let t = txd::parse(&std::fs::read(fixtures().join("d3d8_vc.txd")).unwrap()).unwrap();
    let c565 = t.textures.iter().find(|t| t.name == "d3d8_vc_c565").unwrap();
    assert_eq!(buf, c565.raster.decode(0).unwrap().pixels);
    // mip levels and the untouched DXT data
    assert!(out.join("d3d8_vc/d3d8_vc_c8888_mip.mip6.png").exists());
    let dds = std::fs::read(out.join("d3d8_vc/d3d8_vc_dxt1_mip.dds")).unwrap();
    let dxt = t.textures.iter().find(|t| t.name == "d3d8_vc_dxt1_mip").unwrap();
    let body: Vec<u8> = dxt.raster.levels.iter().flat_map(|l| l.data.clone()).collect();
    assert_eq!(&dds[..4], b"DDS ");
    assert_eq!(&dds[84..88], b"DXT1");
    assert_eq!(&dds[128..], &body[..]);
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["textures"].as_array().unwrap().len(), rep.textures);
    assert_eq!(manifest["edition"], serde_json::to_value(&install.edition).unwrap());
    assert_eq!(manifest["user_choice"], false);
    std::fs::remove_dir_all(&out).ok();
}
