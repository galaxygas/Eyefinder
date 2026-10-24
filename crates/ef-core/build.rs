//! Embeds every `data/database/*.toml` file (the build database) into the crate.

use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=data/database");
    let mut names: Vec<String> = fs::read_dir("data/database")
        .expect("data/database exists")
        .filter_map(|e| {
            let p = e.ok()?.path();
            let name = p.file_name()?.to_string_lossy().into_owned();
            (p.extension()? == "toml").then_some(name)
        })
        .collect();
    names.sort();
    let mut out = String::from("&[\n");
    for n in &names {
        out += &format!("    ({n:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/data/database/{n}\"))),\n");
    }
    out += "]\n";
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("database_files.rs"), out).unwrap();
}
