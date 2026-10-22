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
        store: ef_core::Store::Unknown,
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
        store: ef_core::Store::Unknown,
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

#[test]
fn dma_style_pixels_match_specification_reference() {
    let reference: Value =
        serde_json::from_slice(&std::fs::read(fixtures().join("style_reference.json")).unwrap()).unwrap();
    for (file, rows) in reference.as_object().unwrap() {
        let bytes = std::fs::read(fixtures().join(file)).unwrap();
        let collection = ef_texture::collection::parse(&bytes).unwrap();
        assert_eq!(collection.rw_version, 0);
        assert_eq!(collection.textures.len(), rows.as_array().unwrap().len());
        for (texture, want) in collection.textures.iter().zip(rows.as_array().unwrap()) {
            assert_eq!(texture.name, want["name"].as_str().unwrap());
            assert_eq!(texture.raster.support().0, ef_core::SupportLevel::FromSpec);
            let image = texture.raster.decode(0).unwrap();
            assert_eq!(image.width as u64, want["width"].as_u64().unwrap());
            assert_eq!(image.height as u64, want["height"].as_u64().unwrap());
            assert_eq!(fnv(&image.pixels), want["fnv"].as_str().unwrap(), "{file} {}", texture.name);
        }
        for n in [0, 3, 4, 50, 63, 100, bytes.len() - 1] {
            assert!(ef_texture::collection::parse(&bytes[..n]).is_err(), "truncated {file} at {n}");
        }
        let mut bad = bytes.clone();
        bad[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(ef_texture::collection::parse(&bad).is_err());
    }
}

#[test]
fn styles_use_normal_catalog_preview_and_png_export() {
    for game in [Game::Gta1, Game::Gta2] {
        let install = GameInstall {
            game,
            platform: Platform::Pc,
            edition: ef_core::Edition::unknown(),
            vfs: Arc::new(FolderVfs::new(fixtures())),
            evidence: vec![],
            user_choice: false,
            store: ef_core::Store::Unknown,
        };
        let cat = ef_texture::catalog::build(&install, &|_, _| {}, &AtomicBool::new(false));
        let picked: Vec<_> = cat
            .assets
            .iter()
            .enumerate()
            .filter(|(_, a)| a.platform == NativePlatform::DmaStyle)
            .map(|(i, _)| i)
            .collect();
        assert!(!picked.is_empty());
        if game == Game::Gta1 {
            assert!(cat.sources.iter().any(|s| s.container == "GTA1 FON"));
        }
        let root = std::env::temp_dir().join(format!("ef-style-export-{game:?}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let report = ef_texture::export::export(
            &install,
            &cat,
            &picked,
            &root,
            ef_texture::export::ExportOptions::default(),
            &|_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert_eq!(report.textures, picked.len());
        assert_eq!(report.written, picked.len());
        let manifest: Value = serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
        // Each encoded PNG must match the same catalog object used by preview workers.
        for i in picked {
            let asset = &cat.assets[i];
            let src = &cat.sources[asset.source];
            let expected = cat.load(&install, asset.source).unwrap().textures[asset.index].raster.decode(0).unwrap();
            let stem = &src.path;
            let png = std::fs::read(root.join(stem).join(format!("{}.png", asset.name))).unwrap();
            let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut pixels).unwrap();
            assert_eq!(pixels, expected.pixels);
        }
        assert!(!manifest.to_string().contains("Direct3D"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn fon_images_match_reference_and_reject_truncation() {
    let bytes = std::fs::read(fixtures().join("images.fon")).unwrap();
    let rows: Value = serde_json::from_slice(&std::fs::read(fixtures().join("fon_reference.json")).unwrap()).unwrap();
    let collection = ef_texture::collection::parse_named(&bytes, "IMAGES.FON").unwrap();
    assert_eq!(collection.kind, "GTA1 FON");
    assert_eq!(collection.textures.len(), 3);
    assert_eq!(collection.problems.len(), 1); // zero-width spacer
    for (tex, want) in collection.textures.iter().zip(rows.as_array().unwrap()) {
        let image = tex.raster.decode(0).unwrap();
        assert_eq!(tex.name, want["name"].as_str().unwrap());
        assert_eq!(image.width as u64, want["width"].as_u64().unwrap());
        assert_eq!(image.height, 16);
        assert_eq!(fnv(&image.pixels), want["fnv"].as_str().unwrap());
        assert_eq!(tex.raster.support().0, ef_core::SupportLevel::FromSpec);
    }
    for n in 0..bytes.len() {
        assert!(ef_texture::fon::parse(&bytes[..n]).is_err(), "truncated at {n}");
    }
    let mut bad = bytes.clone();
    bad[1] = 0;
    assert!(ef_texture::fon::parse(&bad).is_err());
    bad = bytes.clone();
    bad.push(0);
    assert!(ef_texture::fon::parse(&bad).is_err());
    // No magic: unrelated containers cannot be silently treated as fonts.
    assert!(ef_texture::collection::parse(&bytes).is_err());
}

#[test]
fn tga_images_match_their_reference() {
    use sha2::{Digest, Sha256};
    let refs: Value = serde_json::from_slice(&std::fs::read(fixtures().join("tga_reference.json")).unwrap()).unwrap();
    let refs = refs.as_object().unwrap();
    assert_eq!(refs.len(), 6);
    for (name, want) in refs {
        let bytes = std::fs::read(fixtures().join(name)).unwrap();
        let c = ef_texture::collection::parse_named(&bytes, &format!("data/frontend/{name}")).unwrap();
        assert_eq!((c.kind, c.textures.len()), ("Truevision TGA", 1));
        let t = &c.textures[0];
        assert_eq!(t.raster.platform, NativePlatform::Targa);
        let image = t.raster.decode(0).unwrap();
        assert_eq!(image.width as u64, want["width"].as_u64().unwrap(), "{name}");
        assert_eq!(image.height as u64, want["height"].as_u64().unwrap(), "{name}");
        let got: String = Sha256::digest(&image.pixels).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(got, want["sha256"].as_str().unwrap(), "{name} ({})", want["reference"]);
        // Pillow appends the 26-byte TGA 2.0 footer ("TRUEVISION-XFILE.\0"), which the image
        // does not need; cutting anywhere before it must fail.
        let footer = if bytes.ends_with(b"TRUEVISION-XFILE.\0") { 26 } else { 0 };
        for n in 0..bytes.len() - footer {
            assert!(ef_texture::tga::parse(&bytes[..n], name).is_err(), "{name} truncated at {n}");
        }
    }
    // Right-to-left order (descriptor bit 4), which Pillow does not read: a 2x1 24-bit image.
    let mut rtl = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 24, 0x30];
    rtl.extend([1, 2, 3, 4, 5, 6]); // stored B,G,R: first stored pixel is the rightmost
    let image = ef_texture::tga::parse(&rtl, "x.tga").unwrap().textures[0].raster.decode(0).unwrap();
    assert_eq!(image.pixels, [6, 5, 4, 255, 3, 2, 1, 255]);
}
