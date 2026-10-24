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
    eyefinder-cli text     <game folder, asset file or disc image> [choice]
    eyefinder-cli files    <game folder or disc image>
    eyefinder-cli docs     missing
    eyefinder-cli db       info | validate [folder] | export <file.toml> | import <file.toml|pack.zip>
    eyefinder-cli db       pack <folder> <pack.zip> | update

scan lists the copies found in Steam libraries, the Rockstar Games Launcher and the usual Rockstar
Games folders, with the store each belongs to.

db works on the build database (docs/DATABASE.md): info shows the loaded database, validate checks it
(or a database folder) and fails on any problem, export saves your own additions as one file, import adds
a file or installs a pack, pack writes a pack .zip and its .sha256, update installs a newer pack from
GitHub.

Disc images: ISO, BIN/CUE, GDI and CDI (Dreamcast), VCD (PS2 POPStarter). Cartridge ROMs: GB, GBC, GBA.
A folder holding several games (GTA1 with London 1969/1961 installed into it) needs --game.

CHOICE (default: detected; the files must fit the choice; `versions` lists what fits):
    --game <gta1|london1969|london1961|gta2|advance|gta3|vc|sa>
    --platform <pc|ps1|ps2|xbox|dc|gbc|gba>
    --version <id from versions>

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

/// `eyefinder-cli db ...`
fn db(args: &[String]) -> ExitCode {
    use ef_core::database::{self, Database};
    let paths = database::paths();
    let fail = |e: &dyn std::fmt::Display| {
        eprintln!("error: {e}");
        ExitCode::from(1)
    };
    match (args.first().map(String::as_str), args.get(1), args.get(2)) {
        (Some("info"), None, None) => {
            let db = Database::load(paths);
            println!("version	{}	{:?}	built-in {}", db.version, db.base, db.built_in_version);
            println!("entries	{}	sections	{}	sources	{}", db.entries.len(), db.sections.len(), db.sources.len());
            for g in &db.games {
                println!("{}	{}	{}", g.id, db.entries_of(&g.id).count(), g.title);
            }
            for f in &db.user_files {
                println!("user file	{f}");
            }
            for n in &db.notices {
                println!("notice	{n}");
            }
            for p in &db.problems {
                println!("problem	{}	{}", p.file, p.message);
            }
            ExitCode::SUCCESS
        }
        (Some("validate"), dir, None) => {
            let db = match dir {
                Some(d) => Database::load_complete(&PathBuf::from(d)),
                None => Database::load(paths),
            };
            for p in &db.problems {
                println!("{}	{}", p.file, p.message);
            }
            println!(
                "{} entries, {} sections, {} sources, {} problems",
                db.entries.len(),
                db.sections.len(),
                db.sources.len(),
                db.problems.len()
            );
            if db.problems.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        (Some("export"), Some(out), None) => match Database::export_user(paths, &PathBuf::from(out)) {
            Ok(n) => {
                println!("{n} entries written to {out}");
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        },
        (Some("import"), Some(file), None) => match database::import(paths, &PathBuf::from(file)) {
            Ok(r) => {
                println!("{r:?}");
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        },
        (Some("pack"), Some(dir), Some(out)) => {
            let out = PathBuf::from(out);
            match database::write_pack_zip(&PathBuf::from(dir), &out) {
                Ok(sha) => {
                    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let sum = PathBuf::from(format!("{}.sha256", out.display()));
                    if let Err(e) = std::fs::write(
                        &sum,
                        format!(
                            "{sha}  {name}
"
                        ),
                    ) {
                        return fail(&e);
                    }
                    println!("{sha}  {name}");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&e),
            }
        }
        (Some("update"), None, None) => {
            let db = Database::load(paths);
            match database::check_update(&db.version) {
                Ok(None) => {
                    println!("The database is up to date ({}).", db.version);
                    ExitCode::SUCCESS
                }
                Ok(Some(a)) => match database::download_and_install(paths, &a) {
                    Ok(v) => {
                        println!("Database {v} installed.");
                        ExitCode::SUCCESS
                    }
                    Err(e) => fail(&e),
                },
                Err(e) => fail(&e),
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn parse_game(s: &str) -> Option<Game> {
    Game::from_cli_id(s)
}

fn parse_platform(s: &str) -> Option<Platform> {
    Platform::from_cli_id(s)
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
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    i.game.name(),
                    i.platform.name(),
                    i.edition.label,
                    f.store.label(),
                    f.path.display(),
                    f.source
                ),
                Err(e) => println!("not recognised\t{}\t{}\t{}\t{e}", f.store.label(), f.path.display(), f.source),
            }
        }
        return ExitCode::SUCCESS;
    }
    if cmd == "db" {
        return db(&args[1..]);
    }
    let Some(path) = args.get(1) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if cmd == "docs" && args.get(1).map(String::as_str) == Some("missing") {
        print!("{}", ef_core::docs::missing_markdown());
        return ExitCode::SUCCESS;
    }
    if cmd == "files" {
        // Every file with its size: the file list a format report asks for (CONTRIBUTING.md).
        let vfs = match ef_core::import::open_path(&PathBuf::from(path)) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        };
        for (p, e) in ef_core::vfs::walk_files(vfs.as_ref(), "") {
            println!("{}\t{p}", e.size);
        }
        return ExitCode::SUCCESS;
    }
    if cmd == "versions" {
        let vfs = match ef_core::import::open_path(&PathBuf::from(path)) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        };
        let choices = ef_core::possible_choices(vfs.as_ref());
        if choices.is_empty() {
            eprintln!("error: no supported GTA data was found in {path}");
            return ExitCode::from(2);
        }
        for (g, p) in choices {
            println!("--game {} --platform {}\t{} ({})", g.cli_id(), p.cli_id(), g.name(), p.name());
            for e in ef_core::edition::known(g, p) {
                println!("    --version {:<22} {}", e.id, e.label);
            }
        }
        return ExitCode::SUCCESS;
    }
    let install = match open(path, &args) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if cmd == "textures" {
        return textures(&install, &args);
    }
    if cmd == "text" {
        return text(&install);
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
            if !install.store.is_unknown() {
                println!("store: {}", install.store.label());
            }
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

/// Prints every entry of the installation's GTA1/London FXT and GTA2 GXT text files.
fn text(install: &GameInstall) -> ExitCode {
    let vfs = install.vfs.as_ref();
    let mut any = false;
    for (path, _) in ef_core::vfs::walk_files(vfs, "") {
        let lower = path.to_ascii_lowercase();
        if !(lower.ends_with(".fxt") || lower.ends_with(".gxt")) {
            continue;
        }
        let data = match vfs.read_all(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        match ef_core::text::parse_named(&path, &data) {
            Some(Ok(t)) => {
                any = true;
                for n in &t.notes {
                    eprintln!("note: {path}: {n}");
                }
                for (k, v) in &t.entries {
                    println!("{path}\t{k}\t{}", v.replace('\n', "\\n"));
                }
            }
            Some(Err(e)) => eprintln!("{path}: {e}"),
            None => {
                eprintln!("{path}: not a GTA1/London FXT or GTA2 GXT file (GTA III/VC GXT is used for station names)")
            }
        }
    }
    if !any {
        eprintln!("no GTA1/London/GTA2 text files were found");
    }
    ExitCode::SUCCESS
}
