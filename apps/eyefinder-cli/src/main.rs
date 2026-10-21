//! Command-line front end: find and detect games, list their audio and textures, export them.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use ef_audio::export::{ExportOptions, export};
use ef_audio::{Category, DecodeOptions, catalog};
use ef_core::{Game, GameInstall, ImportChoice, Platform};

const USAGE: &str = "Eyefinder command line

USAGE:
    eyefinder-cli scan
    eyefinder-cli info     <game folder, asset file or disc image> [choice]
    eyefinder-cli versions <game folder, asset file or disc image>
    eyefinder-cli list     <game folder, asset file or disc image> [choice]
    eyefinder-cli export   <game folder, asset file or disc image> <output folder> [choice] [audio options]
    eyefinder-cli textures <game folder, asset file or disc image> <output folder> [choice] [texture options]

scan lists the copies found in Steam libraries and the usual Rockstar Games folders.

CHOICE (default: detected; the files must fit the choice):
    --game <gta1|london1969|london1961|gta2|gta3|vc|sa>         --platform <pc|ps2|xbox>      --version <id from versions>

AUDIO OPTIONS:
    --pcm16             decode MPEG audio / Vorbis to 16-bit PCM instead of 32-bit float
    --no-sfx            skip the sound effect bank
    --no-split          do not write per-marker track files
    --no-bitstream      do not save the original movie audio bitstream
    --only <category>   export one category (e.g. \"Radio Stations\")

TEXTURE OPTIONS:
    --mipmaps           also write every mip level
    --no-dds            do not save DXT-compressed textures untouched as .dds
    --only <dictionary> export one texture dictionary (path as listed, e.g. models/gta3.img/hud.txd)";

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn parse_game(s: &str) -> Option<Game> {
    match s.to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "gta1" | "1" => Some(Game::Gta1),
        "gta2" | "2" => Some(Game::Gta2),
        "london1969" => Some(Game::London1969),
        "london1961" => Some(Game::London1961),
        "gta3" | "iii" | "3" => Some(Game::Gta3),
        "vc" | "vicecity" => Some(Game::ViceCity),
        "sa" | "sanandreas" => Some(Game::SanAndreas),
        _ => None,
    }
}

fn parse_platform(s: &str) -> Option<Platform> {
    match s.to_ascii_lowercase().as_str() {
        "pc" => Some(Platform::Pc),
        "ps2" => Some(Platform::Ps2),
        "xbox" => Some(Platform::Xbox),
        _ => None,
    }
}

/// Opens the game, applying --game / --platform / --version if given.
fn open(path: &str, args: &[String]) -> Result<GameInstall, String> {
    let path = PathBuf::from(path);
    let (g, p, v) = (value(args, "--game"), value(args, "--platform"), value(args, "--version"));
    if g.is_none() && p.is_none() && v.is_none() {
        return ef_core::open_and_detect(&path).map_err(|e| e.to_string());
    }
    let vfs = ef_core::import::open_path(&path).map_err(|e| e.to_string())?;
    let detected = ef_core::detect(vfs.clone());
    let game = match g {
        Some(s) => parse_game(s).ok_or(format!("unknown game {s}"))?,
        None => detected.as_ref().map_err(|e| e.to_string())?.game,
    };
    let platform = match p {
        Some(s) => parse_platform(s).ok_or(format!("unknown platform {s}"))?,
        None => {
            let platforms: Vec<_> =
                ef_core::possible_choices(vfs.as_ref()).into_iter().filter(|c| c.0 == game).map(|c| c.1).collect();
            if platforms.len() == 1 { platforms[0] } else { detected.as_ref().map_err(|e| e.to_string())?.platform }
        }
    };
    let edition = v.map(str::to_owned).unwrap_or_else(|| {
        detected
            .as_ref()
            .ok()
            .filter(|d| (d.game, d.platform) == (game, platform))
            .map_or_else(|| "unknown".into(), |d| d.edition.id.clone())
    });
    ef_core::open_with(&path, Some(&ImportChoice { game, platform, edition })).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if cmd == "scan" {
        let found = ef_core::locate::scan();
        if found.is_empty() {
            println!("No copies were found in the Steam libraries or Rockstar Games folders of this computer.");
        }
        for f in found {
            match f.result {
                Ok(i) => println!(
                    "{}\t{}\t{}\t{}\t{}",
                    i.game.name(),
                    i.platform.name(),
                    i.edition.label,
                    f.path.display(),
                    f.source
                ),
                Err(e) => println!("not recognised\t{}\t{}\t{e}", f.path.display(), f.source),
            }
        }
        return ExitCode::SUCCESS;
    }
    let Some(path) = args.get(1) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let install = match open(path, &args) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    match cmd.as_str() {
        "versions" => {
            let vfs = install.vfs.as_ref();
            for (g, p) in ef_core::possible_choices(vfs) {
                println!("--game {} --platform {}", g.short(), p.short().to_lowercase());
                for e in ef_core::edition::known(g, p) {
                    println!("    --version {:<22} {}", e.id, e.label);
                }
            }
            return ExitCode::SUCCESS;
        }
        "textures" => return textures(&install, &args),
        _ => {}
    }
    let cat = match catalog::build(&install) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    match cmd.as_str() {
        "info" => {
            println!("{} ({}), version: {}", install.game.name(), install.platform.name(), install.edition.label);
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
            let tex = ef_texture::catalog::build(&install, &|_, _| {}, &AtomicBool::new(false));
            println!("{:<26} {} in {} dictionaries", "Textures", tex.assets.len(), tex.sources.len());
            ExitCode::SUCCESS
        }
        "list" => {
            for a in &cat.assets {
                println!("{}\t{}\t{}\t{}\t{}", a.category.label(), a.title, a.game_id, a.source, a.support.label());
            }
            let tex = ef_texture::catalog::build(&install, &|_, _| {}, &AtomicBool::new(false));
            for a in &tex.assets {
                println!(
                    "Texture\t{}\t{}x{} {}\t{}\t{}",
                    a.name,
                    a.width,
                    a.height,
                    a.format,
                    tex.sources[a.source].path,
                    a.support.label()
                );
            }
            ExitCode::SUCCESS
        }
        "export" => {
            let Some(out) = args.get(2) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let flag = |f: &str| args.iter().any(|a| a == f);
            let only = value(&args, "--only");
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
                .filter(|a| only.is_none_or(|o| a.category.label().eq_ignore_ascii_case(o)))
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

fn textures(install: &GameInstall, args: &[String]) -> ExitCode {
    let Some(out) = args.get(2).filter(|a| !a.starts_with("--")) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let cancel = AtomicBool::new(false);
    let cat = ef_texture::catalog::build(install, &|_, _| {}, &cancel);
    for n in &cat.notices {
        eprintln!("note: {n}");
    }
    let only = value(args, "--only");
    let pick: Vec<usize> = (0..cat.assets.len())
        .filter(|&i| only.is_none_or(|o| cat.sources[cat.assets[i].source].path.eq_ignore_ascii_case(o)))
        .collect();
    let opts = ef_texture::export::ExportOptions {
        mipmaps: args.iter().any(|a| a == "--mipmaps"),
        originals: !args.iter().any(|a| a == "--no-dds"),
    };
    let progress = |d: usize, t: usize| {
        if d.is_multiple_of(200) || d == t {
            eprintln!("[{d}/{t}]");
        }
    };
    match ef_texture::export::export(install, &cat, &pick, &PathBuf::from(out), opts, &progress, &cancel) {
        Ok(r) => {
            for s in &r.skipped {
                eprintln!("skipped {s}");
            }
            println!("{} textures, {} files written, {} skipped -> {out}", r.textures, r.written, r.skipped.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
