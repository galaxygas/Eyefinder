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
