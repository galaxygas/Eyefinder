//! Command-line front end: detect a game, list its audio, export it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use ef_audio::export::{ExportOptions, export};
use ef_audio::{Category, DecodeOptions, catalog};

const USAGE: &str = "Eyefinder command line

USAGE:
    eyefinder-cli info   <game folder or .iso>
    eyefinder-cli list   <game folder or .iso>
    eyefinder-cli export <game folder or .iso> <output folder> [options]

EXPORT OPTIONS:
    --pcm16             decode MPEG audio / Vorbis to 16-bit PCM instead of 32-bit float
    --no-sfx            skip the sound effect bank
    --no-split          do not write per-marker track files
    --no-bitstream      do not save the original movie audio bitstream
    --only <category>   export one category (e.g. \"Radio Stations\")";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(path)) = (args.first(), args.get(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let install = match ef_core::open_and_detect(&PathBuf::from(path)) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let cat = match catalog::build(&install) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    match cmd.as_str() {
        "info" => {
            println!("{} ({})", install.game.name(), install.platform.name());
            for e in &install.evidence {
                println!("  - {e}");
            }
            for c in Category::ALL {
                let n = cat.assets.iter().filter(|a| a.category == c).count();
                if n > 0 {
                    println!("{:<26} {n}", c.label());
                }
            }
            for n in &cat.notices {
                println!("note: {n}");
            }
            ExitCode::SUCCESS
        }
        "list" => {
            for a in &cat.assets {
                println!("{}\t{}\t{}\t{}\t{}", a.category.label(), a.title, a.game_id, a.source, a.support.label());
            }
            ExitCode::SUCCESS
        }
        "export" => {
            let Some(out) = args.get(2) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let flag = |f: &str| args.iter().any(|a| a == f);
            let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).cloned();
            let opts = ExportOptions {
                out_dir: PathBuf::from(out),
                decode: DecodeOptions { float_output: !flag("--pcm16") },
                split_markers: !flag("--no-split"),
                keep_movie_bitstream: !flag("--no-bitstream"),
            };
            let assets: Vec<_> = cat
                .assets
                .iter()
                .filter(|a| a.support.can_decode())
                .filter(|a| !(flag("--no-sfx") && a.category == Category::SfxBank))
                .filter(|a| only.as_ref().is_none_or(|o| a.category.label().eq_ignore_ascii_case(o)))
                .collect();
            let cancel = AtomicBool::new(false);
            let report = export(
                &install,
                &assets,
                &opts,
                |p| {
                    if !p.current.is_empty() {
                        eprintln!("[{}/{}] {}", p.done + 1, p.total, p.current);
                    }
                },
                &cancel,
            );
            match report {
                Ok(r) => {
                    for (t, e) in &r.failed {
                        eprintln!("FAILED {t}: {e}");
                    }
                    println!("{} files written, {} failed -> {}", r.written, r.failed.len(), r.out_dir.display());
                    if r.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
