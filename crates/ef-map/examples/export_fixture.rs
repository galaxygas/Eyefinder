//! Exports a fixture copy: `cargo run -p ef-map --example export_fixture -- <dir> <game> <out>`.
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = ef_core::Game::from_cli_id(&a[2]).expect("game");
    let inst = ef_core::GameInstall {
        game,
        platform: ef_core::Platform::Pc,
        edition: ef_core::edition::Edition::unknown(),
        vfs: Arc::new(ef_core::vfs::FolderVfs::new(&a[1])),
        evidence: vec![],
        user_choice: true,
        store: ef_core::store::Store::Unknown,
    };
    let cat = ef_map::catalog::build(&inst);
    let opts = ef_map::export::ExportOptions {
        raw: true,
        individual: true,
        topdown: true,
        tiles: true,
        premade: true,
        ..Default::default()
    };
    let r =
        ef_map::export::export(&inst, &cat, &opts, std::path::Path::new(&a[3]), &|_, _, _| {}, &AtomicBool::new(false))
            .unwrap();
    println!("{} written, failed {:?}", r.written, r.failed);
}
