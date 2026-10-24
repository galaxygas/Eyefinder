//! Exercise the public importer rather than constructing GameInstall by hand.
use std::path::Path;
use std::process::Command;

#[test]
fn older_assets_import_and_export_through_cli() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!("ef-cli-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("GTADATA/AUDIO")).unwrap();
    for bank in ["LEVEL000", "LEVEL001"] {
        for ext in ["SDT", "RAW"] {
            let relative = format!("GTADATA/AUDIO/{bank}.{ext}");
            std::fs::copy(repo.join("crates/ef-audio/tests/fixtures/gta1_pc").join(&relative), root.join(relative))
                .unwrap();
        }
    }
    std::fs::copy(repo.join("crates/ef-texture/tests/fixtures/style.g24"), root.join("GTADATA/style.g24")).unwrap();
    for path in
        [root.clone(), root.join("GTADATA"), root.join("GTADATA/AUDIO/LEVEL000.RAW"), root.join("GTADATA/style.g24")]
    {
        let output = Command::new(env!("CARGO_BIN_EXE_eyefinder-cli")).arg("list").arg(path).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("LEVEL000_0000"));
    }
    for (cmd, out) in [("export", "audio-out"), ("textures", "texture-out")] {
        let output = Command::new(env!("CARGO_BIN_EXE_eyefinder-cli"))
            .arg(cmd)
            .arg(root.join("GTADATA"))
            .arg(root.join(out))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(root.join(out).join("manifest.json").is_file());
    }
    let loose = root.with_file_name(format!("ef-cli-loose-{}", std::process::id()));
    std::fs::create_dir_all(&loose).unwrap();
    std::fs::copy(root.join("GTADATA/style.g24"), loose.join("style.g24")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_eyefinder-cli"))
        .arg("textures")
        .arg(loose.join("style.g24"))
        .arg(root.join("loose-out"))
        .args(["--game", "london1961"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        root.join("loose-out/style.g24/side_0000_palette_0.png").is_file()
            || root.join("loose-out/GTADATA/style.g24/side_0000_palette_0.png").is_file()
    );
    // Mixed loose selections need both catalogs, even without an AUDIO subfolder.
    for ext in ["SDT", "RAW"] {
        std::fs::copy(root.join(format!("GTADATA/AUDIO/LEVEL001.{ext}")), loose.join(format!("LEVEL001.{ext}")))
            .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_eyefinder-cli"))
        .arg("list")
        .arg(loose.join("LEVEL001.SDT"))
        .args(["--game", "gta1"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("LEVEL001_0000"));
    std::fs::remove_dir_all(loose).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

// Minimal ECMA-119 fixture with an installation nested under GAME/GTADATA.
// Only synthetic assets from the committed generators are used.
fn nested_disc(repo: &Path) -> Vec<u8> {
    fn record(name: &[u8], sector: u32, size: u32, directory: bool) -> Vec<u8> {
        let mut r = vec![0; 33 + name.len() + (1 - name.len() % 2)];
        r[0] = r.len() as u8;
        r[2..6].copy_from_slice(&sector.to_le_bytes());
        r[6..10].copy_from_slice(&sector.to_be_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[14..18].copy_from_slice(&size.to_be_bytes());
        r[25] = if directory { 2 } else { 0 };
        r[28..30].copy_from_slice(&1u16.to_le_bytes());
        r[30..32].copy_from_slice(&1u16.to_be_bytes());
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    }
    let mut disc = vec![0; 22 * 2048];
    let mut directory: Vec<Vec<u8>> = (18..22)
        .map(|n| {
            let mut d = record(b"\0", n, 2048, true);
            d.extend(record(b"\x01", if n == 18 { 18 } else { n - 1 }, 2048, true));
            d
        })
        .collect();
    directory[0].extend(record(b"GAME", 19, 2048, true));
    directory[1].extend(record(b"GTADATA", 20, 2048, true));
    directory[2].extend(record(b"AUDIO", 21, 2048, true));
    let mut files = vec![(2, "STYLE.G24".to_string(), repo.join("crates/ef-texture/tests/fixtures/style.g24"))];
    for bank in ["LEVEL000", "LEVEL001"] {
        for ext in ["SDT", "RAW"] {
            files.push((
                3,
                format!("{bank}.{ext}"),
                repo.join(format!("crates/ef-audio/tests/fixtures/gta1_pc/GTADATA/AUDIO/{bank}.{ext}")),
            ));
        }
    }
    for (dir, name, path) in files {
        let bytes = std::fs::read(path).unwrap();
        let start = disc.len();
        directory[dir].extend(record(format!("{name};1").as_bytes(), (start / 2048) as u32, bytes.len() as u32, false));
        disc.extend(bytes);
        disc.resize(disc.len().div_ceil(2048) * 2048, 0);
    }
    for (i, bytes) in directory.into_iter().enumerate() {
        let start = (18 + i) * 2048;
        disc[start..start + bytes.len()].copy_from_slice(&bytes);
    }
    let sectors = (disc.len() / 2048) as u32;
    let pvd = &mut disc[16 * 2048..17 * 2048];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    pvd[80..84].copy_from_slice(&sectors.to_le_bytes());
    pvd[84..88].copy_from_slice(&sectors.to_be_bytes());
    pvd[128..130].copy_from_slice(&2048u16.to_le_bytes());
    pvd[130..132].copy_from_slice(&2048u16.to_be_bytes());
    let root = record(b"\0", 18, 2048, true);
    pvd[156..156 + root.len()].copy_from_slice(&root);
    disc[17 * 2048] = 255;
    disc[17 * 2048 + 1..17 * 2048 + 6].copy_from_slice(b"CD001");
    disc[17 * 2048 + 6] = 1;
    disc
}

#[test]
fn iso_bin_and_cue_import_nested_game_audio_and_textures() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!("ef-cli-disc-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let plain = nested_disc(&repo);
    std::fs::write(root.join("disc.iso"), &plain).unwrap();
    let mut raw = Vec::new();
    for sector in plain.as_chunks::<2048>().0 {
        let mut block = [0; 2352];
        block[1..11].fill(255);
        block[15] = 1;
        block[16..2064].copy_from_slice(sector);
        raw.extend(block);
    }
    std::fs::write(root.join("disc.bin"), raw).unwrap();
    std::fs::write(root.join("disc.cue"), "FILE \"disc.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:00\n")
        .unwrap();
    for name in ["disc.iso", "disc.bin", "disc.cue", ""] {
        let output =
            Command::new(env!("CARGO_BIN_EXE_eyefinder-cli")).arg("list").arg(root.join(name)).output().unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("LEVEL000_0000"));
    }
    for cmd in ["export", "textures"] {
        let output = Command::new(env!("CARGO_BIN_EXE_eyefinder-cli"))
            .arg(cmd)
            .arg(root.join("disc.cue"))
            .arg(root.join(cmd))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(root.join(cmd).join("manifest.json").is_file());
    }
    assert!(root.join("textures/GTADATA/STYLE.G24/side_0000_palette_0.png").is_file());
    std::fs::remove_dir_all(root).unwrap();
}
