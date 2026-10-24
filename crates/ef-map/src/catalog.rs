//! What map data a copy holds, and which exports it supports.
//!
//! * GTA1 / London: every `.CMP` of the game's subtree, paired with the style its header names
//!   (CityScape 2.9: style number n means `stylennn`; the retail game loads `.GRY` (8-bit) or `.G24`
//!   (24-bit), 3.7 / 3.8). London 1961 ships `STY001.G24` next to `UKE.CMP`, which no source pairs,
//!   so such maps list every style file and need a choice.
//! * GTA2: the stages of the one `data\*.seq` file name each map and its style (`GMPFile`, `STYFile`;
//!   gta2_re @ edb484d3 Frontend.cpp `LoadSeqFile`, `GetSeqItem_4B48D0`). Other `.gmp` files
//!   (multiplayer maps) need a style choice.
//! * GTA III, Vice City, San Andreas: the level files (`DATA\DEFAULT.DAT`, then `DATA\GTA3.DAT`,
//!   `DATA\GTA_VC.DAT` or `DATA\GTA.DAT`: re3 Game.cpp / win.cpp, reVC main.cpp `InitialiseGame`,
//!   gta-reversed app.h `GAME_LEVEL_FILE`) and what their `IDE`, `IPL`, `COLFILE`, `TEXDICTION`,
//!   `MODELFILE`, `HIERFILE`, `MAPZONE`, `IMG` and `CDIMAGE` lines name (re3 FileLoader.cpp
//!   `LoadLevel`, gta-reversed FileLoader.cpp `LoadLevel`); the map models (`objs`, `tobj` and, in San
//!   Andreas, `anim` lines of the IDE files: id, model, txd, ...) as `.dff` / `.txd` entries of the IMG
//!   archives; the `.col` entries the game streams from them (Vice City, San Andreas), the binary `.ipl`
//!   and path `.dat` entries (San Andreas: gta-reversed Streaming.cpp `LoadCdDirectory`); and the
//!   radar tile dictionaries.

use std::collections::BTreeSet;
use std::sync::Arc;

use ef_core::vfs::{Vfs, walk_files};
use ef_core::{Game, GameInstall, Platform};

use crate::radar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelKind {
    /// GTA1 / London `.CMP`.
    City,
    /// GTA2 `.gmp`.
    Gbh,
}

/// A style a map can be drawn with, and why it belongs to that map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleChoice {
    pub path: String,
    pub why: String,
}

#[derive(Debug, Clone)]
pub struct Level {
    pub kind: LevelKind,
    pub map: String,
    /// "NYC", "wil", or the stage description.
    pub name: String,
    /// The styles the sources pair with this map; empty when it needs a choice.
    pub styles: Vec<StyleChoice>,
}

/// A file of the copy's map data, as a VFS path; IMG entries as `archive.img/entry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapFile {
    pub path: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct MapCatalog {
    pub levels: Vec<Level>,
    /// Every style file of the copy, for maps that need a choice.
    pub style_files: Vec<String>,
    pub radar: Option<(radar::Grid, radar::RadarFiles)>,
    /// Vice City: MODELS/FRONTEN2.TXD is there.
    pub vc_menu: bool,
    pub files: Vec<MapFile>,
    pub notices: Vec<String>,
}

/// Which exports a game on a platform supports, with the reason when it does not.
#[derive(Debug, Clone)]
pub struct Availability {
    pub raw: Result<(), String>,
    /// 2D cities: top-down renders and map data.
    pub cities: Result<(), String>,
    /// 3D games: the radar and pause-menu maps.
    pub premade: Result<(), String>,
    /// 3D games: the map scene and top-down render.
    pub scene: Result<(), String>,
}

impl Availability {
    pub fn any(&self) -> bool {
        self.raw.is_ok() || self.cities.is_ok() || self.premade.is_ok() || self.scene.is_ok()
    }
}

/// What the documented formats allow, before looking at the files.
pub fn availability(game: Game, platform: Platform) -> Availability {
    let no = |s: &str| Err(s.to_string());
    let scene =
        no("The 3D map scene and the top-down render of GTA III, Vice City and San Andreas come in a later update.");
    match (game, platform) {
        (Game::Gta1 | Game::London1969 | Game::London1961, Platform::Pc)
        | (Game::Gta2, Platform::Pc | Platform::Dreamcast) => Availability {
            raw: Ok(()),
            cities: Ok(()),
            premade: no("The 2D games have no premade map."),
            scene: no("Top-down and data exports cover the 2D cities."),
        },
        (Game::Gta3 | Game::ViceCity | Game::SanAndreas, Platform::Pc | Platform::Ps2 | Platform::Xbox) => {
            Availability { raw: Ok(()), cities: no("Not a 2D game."), premade: Ok(()), scene }
        }
        _ => {
            let why = "The map format of this game on this platform is not documented (see MISSING.md).";
            Availability { raw: no(why), cities: no(why), premade: no(why), scene: no(why) }
        }
    }
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// The subtree of a GTA1-family game (London installs into GTADATA/UK, Marschall's inventory).
fn in_subtree(game: Game, path: &str) -> bool {
    let p = lower(path);
    let uk = p.contains("gtadata/uk/");
    match game {
        Game::Gta1 => !uk,
        Game::London1969 => uk && p.ends_with("/uk.cmp") || uk && !p.ends_with(".cmp"),
        Game::London1961 => uk && p.ends_with("/uke.cmp") || uk && !p.ends_with(".cmp"),
        _ => true,
    }
}

fn file_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn dir_of(p: &str) -> &str {
    p.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Resolves a game path (backslashes, any case) to a VFS path.
fn resolve(vfs: &dyn Vfs, all: &[String], path: &str) -> Option<String> {
    let want = lower(&path.trim().replace('\\', "/")).trim_start_matches("./").trim_start_matches('/').to_string();
    if want.is_empty() {
        return None;
    }
    all.iter().find(|p| lower(p) == want).cloned().or_else(|| vfs.exists(&want).then_some(want))
}

pub fn build(install: &GameInstall) -> MapCatalog {
    let vfs = &install.vfs;
    let all: Vec<String> = walk_files(vfs.as_ref(), "").into_iter().map(|(p, _)| p).collect();
    let mut cat = MapCatalog::default();
    match install.game {
        Game::Gta1 | Game::London1969 | Game::London1961 => city_levels(install.game, vfs.as_ref(), &all, &mut cat),
        Game::Gta2 => gbh_levels(vfs.as_ref(), &all, &mut cat),
        Game::Gta3 | Game::ViceCity | Game::SanAndreas => world_files(install.game, vfs, &all, &mut cat),
        _ => {}
    }
    cat
}

fn city_levels(game: Game, vfs: &dyn Vfs, all: &[String], cat: &mut MapCatalog) {
    cat.style_files = all
        .iter()
        .filter(|p| in_subtree(game, p) && (lower(p).ends_with(".gry") || lower(p).ends_with(".g24")))
        .cloned()
        .collect();
    for p in all.iter().filter(|p| in_subtree(game, p) && lower(p).ends_with(".cmp")) {
        let head = vfs.open(p).ok().and_then(|mut f| {
            use std::io::Read;
            let mut b = [0u8; 6];
            f.read_exact(&mut b).ok().map(|_| b)
        });
        let mut styles = Vec::new();
        match head {
            Some(h) if crate::cmp::looks_like_cmp(&h) => {
                let n = h[4];
                // 24-bit first: the game's highest-quality renderer (CityScape 3.8).
                for ext in ["g24", "gry"] {
                    let want = format!("{}/style{n:03}.{ext}", lower(dir_of(p)));
                    if let Some(s) = all.iter().find(|s| lower(s) == want.trim_start_matches('/')) {
                        styles
                            .push(StyleChoice { path: s.clone(), why: format!("style number {n} in the map header") });
                    }
                }
                if styles.is_empty() {
                    cat.notices.push(format!(
                        "{p}: its header names style {n} (STYLE{n:03}.GRY / .G24), which is not here; choose the style."
                    ));
                }
            }
            _ => {
                cat.notices.push(format!("{p}: not a version 331 CMP map; skipped."));
                continue;
            }
        }
        let name = file_name(p).rsplit_once('.').map_or(file_name(p), |(s, _)| s).to_string();
        cat.levels.push(Level { kind: LevelKind::City, map: p.clone(), name, styles });
        cat.files.push(MapFile { path: p.clone(), kind: "CMP city map" });
    }
    for s in &cat.style_files {
        cat.files.push(MapFile { path: s.clone(), kind: "style (tiles)" });
    }
}

/// `Key = Value` lines of a GTA2 sequence file, whitespace removed (gta2_re `GetSeqItem_4B48D0`
/// skips whitespace and reads to the end of the line).
fn seq_items(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let clean = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            Some((clean(k), clean(v)))
        })
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

fn gbh_levels(vfs: &dyn Vfs, all: &[String], cat: &mut MapCatalog) {
    cat.style_files = all.iter().filter(|p| lower(p).ends_with(".sty")).cloned().collect();
    let seqs: Vec<&String> = all.iter().filter(|p| lower(dir_of(p)) == "data" && lower(p).ends_with(".seq")).collect();
    let mut paired = BTreeSet::new();
    match seqs.as_slice() {
        [seq] => {
            cat.files.push(MapFile { path: (*seq).clone(), kind: "stage sequence" });
            let text = vfs.read_all(seq).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            let items = seq_items(&text);
            let mut i = 0;
            while i < items.len() {
                if items[i].0 != "MainOrBonus" {
                    i += 1;
                    continue;
                }
                let stage = &items[i].1;
                let get = |k: &str| items[i + 1..].iter().take(4).find(|(n, _)| n == k).map(|(_, v)| v.clone());
                let (gmp, sty, desc) = (get("GMPFile"), get("STYFile"), get("Description"));
                if let (Some(g), Some(s)) = (gmp, sty) {
                    match (resolve(vfs, all, &g), resolve(vfs, all, &s)) {
                        (Some(gp), Some(sp)) => {
                            let name = desc.unwrap_or_else(|| file_name(&gp).to_string());
                            if paired.insert(lower(&gp)) {
                                cat.levels.push(Level {
                                    kind: LevelKind::Gbh,
                                    map: gp,
                                    name: format!("{name} ({stage})"),
                                    styles: vec![StyleChoice {
                                        path: sp,
                                        why: format!("STYFile of the {stage} stage in {seq}"),
                                    }],
                                });
                            }
                        }
                        _ => cat.notices.push(format!("{seq}: {g} or {s} is not in this copy.")),
                    }
                }
                i += 1;
            }
        }
        [] => cat.notices.push("No data\\*.seq stage list: choose the style of each map.".into()),
        _ => cat.notices.push("Several data\\*.seq files (the game refuses to start with more than one).".into()),
    }
    for p in all.iter().filter(|p| lower(p).ends_with(".gmp")) {
        if !paired.contains(&lower(p)) {
            let name = file_name(p).to_string();
            cat.levels.push(Level { kind: LevelKind::Gbh, map: p.clone(), name, styles: vec![] });
        }
        cat.files.push(MapFile { path: p.clone(), kind: "GMP map" });
    }
    for p in all.iter().filter(|p| lower(p).ends_with(".mmp")) {
        cat.files.push(MapFile { path: p.clone(), kind: "multiplayer map description" });
    }
    for s in &cat.style_files {
        cat.files.push(MapFile { path: s.clone(), kind: "style (tiles)" });
    }
}

fn world_files(game: Game, vfs: &Arc<dyn Vfs>, all: &[String], cat: &mut MapCatalog) {
    let level = match game {
        Game::Gta3 => "DATA\\GTA3.DAT",
        Game::ViceCity => "DATA\\GTA_VC.DAT",
        _ => "DATA\\GTA.DAT",
    };
    let mut seen = BTreeSet::new();
    let mut push = |cat: &mut MapCatalog, path: String, kind: &'static str| {
        if seen.insert(lower(&path)) {
            cat.files.push(MapFile { path, kind });
        }
    };
    let mut ides = Vec::new();
    for dat in ["DATA\\DEFAULT.DAT", level] {
        let Some(p) = resolve(vfs.as_ref(), all, dat) else {
            cat.notices.push(format!("{dat} is not in this copy."));
            continue;
        };
        push(cat, p.clone(), "level file");
        let text = vfs.read_all(&p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        for line in text.lines() {
            // re3 LoadLine: control characters and commas become spaces, leading spaces are skipped.
            let line: String = line.chars().map(|c| if c < ' ' || c == ',' { ' ' } else { c }).collect();
            let line = line.trim_start();
            if line.starts_with('#') {
                continue;
            }
            let (kind, rest) = match line.split_once(' ') {
                Some((k, r)) => (k, r.trim()),
                None => continue,
            };
            let (kind, target) = match kind {
                "IDE" => ("object definitions (IDE)", rest),
                "IPL" => ("placements (IPL)", rest),
                // COLFILE <level> <path>
                "COLFILE" => ("collision (COL)", rest.split_once(' ').map_or(rest, |(_, p)| p.trim())),
                "TEXDICTION" => ("texture dictionary", rest),
                "MODELFILE" | "HIERFILE" => ("model (DFF)", rest),
                "MAPZONE" => ("map zones", rest),
                "IMG" | "CDIMAGE" => ("IMG archive", rest),
                _ => continue,
            };
            match resolve(vfs.as_ref(), all, target) {
                Some(f) => {
                    if kind.starts_with("object definitions") {
                        ides.push(f.clone());
                    }
                    push(cat, f, kind);
                }
                None => cat.notices.push(format!("{p} names {target}, which is not in this copy.")),
            }
        }
    }
    // Map models: the first two names of IDE object lines.
    let mut models = BTreeSet::new();
    let mut txds = BTreeSet::new();
    for ide in &ides {
        let text = vfs.read_all(ide).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let mut section = String::new();
        for line in text.lines() {
            let line: String = line.chars().map(|c| if c < ' ' || c == ',' { ' ' } else { c }).collect();
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if section.is_empty() {
                section = lower(line.get(..4).unwrap_or(""));
                continue;
            }
            if lower(line).starts_with("end") {
                section.clear();
                continue;
            }
            let map_object = section == "objs" || section == "tobj" || (game == Game::SanAndreas && section == "anim");
            let f: Vec<&str> = line.split_whitespace().collect();
            if map_object && f.len() >= 3 && f[0].parse::<i32>().is_ok() {
                models.insert(lower(f[1]));
                txds.insert(lower(f[2]));
            }
        }
    }
    let streamed_col = game != Game::Gta3;
    for img_path in all.iter().filter(|p| ef_core::img::is_img_name(p)) {
        let Ok(img) = ef_core::img::ImgVfs::open(vfs, img_path) else { continue };
        for e in img.entries() {
            let n = lower(&e.name);
            let (stem, ext) = n.rsplit_once('.').unwrap_or((&n, ""));
            let kind = match ext {
                "dff" if models.contains(stem) => "map model (DFF)",
                "txd" if txds.contains(stem) => "map textures (TXD)",
                "txd" if stem.starts_with("radar") && stem[5..].bytes().all(|b| b.is_ascii_digit()) => "radar tile",
                "col" if streamed_col => "collision (COL)",
                "ipl" if game == Game::SanAndreas => "placements (binary IPL)",
                "dat" if game == Game::SanAndreas && stem.starts_with("nodes") => "path nodes",
                _ => continue,
            };
            push(cat, format!("{img_path}/{}", e.name), kind);
        }
    }
    if let Some(grid) = radar::grid(game) {
        let files = radar::find(vfs, grid);
        if files.found() == 0 {
            cat.notices.push("No radar tile dictionaries (radarNN.txd) in the IMG archives.".into());
        }
        cat.radar = Some((grid, files));
    }
    if game == Game::ViceCity {
        cat.vc_menu = vfs.exists(radar::VC_MENU_TXD);
        if cat.vc_menu {
            push(cat, radar::VC_MENU_TXD.to_string(), "menu map textures");
        }
    }
}
