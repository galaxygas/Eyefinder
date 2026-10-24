//! The premade maps of GTA III, Vice City and San Andreas: the radar map and the pause-menu map.
//!
//! **Radar map.** The radar is drawn from square tiles, one texture dictionary each, streamed from the
//! IMG archives: every `.txd` entry of a CD image becomes a dictionary slot of that name (re3 / reVC
//! Streaming.cpp `LoadCdDirectory`). The game draws each slot's first texture (`GetFirstTexture`).
//! * GTA III and Vice City: 8 x 8 tiles `radar00`..`radar63`, tile (x, y) = `radar{x + 8y}`, covering
//!   world X -2000..2000 and Y 2000..-2000 in 500-unit squares (re3 @ 19d2deaa and reVC @ e19f4061,
//!   src/core/Radar.cpp `RADAR_NUM_TILES`, `RADAR_TILE_SIZE`, `gRadarTexNames`, `GetTextureCorners`).
//! * San Andreas: 12 x 12 tiles `radar{y * 12 + x:02}`, X -3000..3000 (gta-reversed @ f6221419,
//!   source/game_sa/Radar.cpp `MAX_RADAR_WIDTH_TILES`, `InitFrontEndMap`, `TransformRealWorldToTexCoordSpace`).
//!
//! In every game texture u runs along +X and v from north to south (`TransformRealWorldToTexCoordSpace`),
//! so the tiles are placed as stored, row 0 at the north edge.
//!
//! **Pause-menu map.**
//! * GTA III (PC) has none: re3's menu map is a re3 addition (config.h `MENU_MAP`, "VC-like menu map.
//!   Won't appear if you don't have our menu.txd").
//! * Vice City draws nine textures, `mapTop01`..`mapBot03` of MODELS/FRONTEN2.TXD, as a 3 x 3 grid of
//!   equal squares, top row first (reVC Frontend.cpp `PrintMap`, `FrontendFilenames`, `LoadAllTextures`).
//! * San Andreas draws the radar tiles over a rectangle of `MENU_MAP_BACKGROUND` = (111, 137, 170)
//!   (gta-reversed MenuManager_Pages.cpp `PrintMap`, MenuManager.h).

use std::collections::BTreeMap;
use std::sync::Arc;

use ef_core::Game;
use ef_core::img::ImgVfs;
use ef_core::vfs::{Vfs, walk_files};
use ef_texture::decode::Rgba;

use crate::Error;

/// The radar tile grid of a game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// Tiles per side.
    pub side: usize,
    /// World co-ordinate of the west and south edges.
    pub min: f32,
    /// World units per tile.
    pub tile: f32,
}

pub fn grid(game: Game) -> Option<Grid> {
    match game {
        Game::Gta3 | Game::ViceCity => Some(Grid { side: 8, min: -2000.0, tile: 500.0 }),
        Game::SanAndreas => Some(Grid { side: 12, min: -3000.0, tile: 500.0 }),
        _ => None,
    }
}

/// San Andreas `MENU_MAP_BACKGROUND`.
pub const SA_MENU_BACKGROUND: [u8; 3] = [111, 137, 170];

/// The Vice City menu map textures, row by row.
pub const VC_MENU_TILES: [&str; 9] =
    ["mapTop01", "mapTop02", "mapTop03", "mapMid01", "mapMid02", "mapMid03", "mapBot01", "mapBot02", "mapBot03"];
pub const VC_MENU_TXD: &str = "MODELS/FRONTEN2.TXD";

/// Where each radar tile's dictionary is: (IMG archive path, entry name).
#[derive(Debug, Clone, Default)]
pub struct RadarFiles {
    pub tiles: Vec<Option<(String, String)>>,
}

impl RadarFiles {
    pub fn found(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }
}

/// The radar dictionaries in the IMG archives of `vfs`.
pub fn find(vfs: &Arc<dyn Vfs>, grid: Grid) -> RadarFiles {
    let mut tiles = vec![None; grid.side * grid.side];
    for (path, _) in walk_files(vfs.as_ref(), "") {
        if !ef_core::img::is_img_name(&path) {
            continue;
        }
        let Ok(img) = ImgVfs::open(vfs, &path) else { continue };
        for e in img.entries() {
            let lower = e.name.to_ascii_lowercase();
            let Some(n) = lower.strip_prefix("radar").and_then(|r| r.strip_suffix(".txd")) else { continue };
            if n.len() < 2 || !n.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            if let Ok(k) = n.parse::<usize>()
                && k < tiles.len()
                && tiles[k].is_none()
            {
                tiles[k] = Some((path.clone(), e.name.clone()));
            }
        }
    }
    RadarFiles { tiles }
}

/// A premade map picture and what went wrong on the way.
pub struct Picture {
    pub image: Rgba,
    /// Pixel size of one tile.
    pub tile: u32,
    pub problems: Vec<String>,
}

fn only_texture(bytes: &[u8], what: &str) -> Result<Rgba, String> {
    let c = ef_texture::collection::parse(bytes).map_err(|e| format!("{what}: {e}"))?;
    match c.textures.as_slice() {
        [t] => t.raster.decode(0).map_err(|e| format!("{what}: {e}")),
        [] => Err(format!("{what}: no texture")),
        // The game draws the first texture of the dictionary's list; with several, which one that is
        // depends on RenderWare's list order, so it is not chosen here.
        many => Err(format!("{what}: {} textures, the game's choice is not documented", many.len())),
    }
}

/// Scales `img` to `size` x `size` by nearest neighbour (tiles of mixed sizes).
fn fit(img: &Rgba, size: u32) -> Rgba {
    if img.width == size && img.height == size {
        return img.clone();
    }
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (sx, sy) = (x * img.width / size, y * img.height / size);
            pixels.extend_from_slice(&img.pixels[((sy * img.width + sx) * 4) as usize..][..4]);
        }
    }
    Rgba { width: size, height: size, pixels }
}

/// Lays `tiles` (row by row, `side` per row) into one picture at the largest tile size.
fn grid_picture(tiles: Vec<Option<Rgba>>, side: usize, mut problems: Vec<String>) -> Picture {
    let size = tiles.iter().flatten().map(|t| t.width.max(t.height)).max().unwrap_or(1);
    if tiles.iter().flatten().any(|t| t.width != size || t.height != size) {
        problems.push(format!("tiles have different sizes; smaller ones are scaled to {size} pixels"));
    }
    let w = size as usize * side;
    let mut pixels = vec![0u8; w * w * 4];
    for (k, t) in tiles.iter().enumerate() {
        let Some(t) = t else { continue };
        let t = fit(t, size);
        let (tx, ty) = (k % side, k / side);
        for y in 0..size as usize {
            let s = y * size as usize * 4;
            let d = ((ty * size as usize + y) * w + tx * size as usize) * 4;
            pixels[d..d + size as usize * 4].copy_from_slice(&t.pixels[s..s + size as usize * 4]);
        }
    }
    Picture { image: Rgba { width: w as u32, height: w as u32, pixels }, tile: size, problems }
}

/// The radar map; `progress(done, total)` per tile.
pub fn radar_map(
    vfs: &Arc<dyn Vfs>,
    grid: Grid,
    files: &RadarFiles,
    progress: &dyn Fn(usize, usize),
) -> Result<Picture, Error> {
    if files.found() == 0 {
        return Err(Error::Unsupported("no radar tile dictionaries (radarNN.txd) in the IMG archives".into()));
    }
    let mut archives: BTreeMap<String, ImgVfs> = BTreeMap::new();
    let mut problems = Vec::new();
    let mut tiles = Vec::with_capacity(files.tiles.len());
    for (k, t) in files.tiles.iter().enumerate() {
        progress(k, files.tiles.len());
        let Some((path, entry)) = t else {
            problems.push(format!("radar{k:02} is missing"));
            tiles.push(None);
            continue;
        };
        if !archives.contains_key(path) {
            archives.insert(path.clone(), ImgVfs::open(vfs, path).map_err(|e| Error::Unsupported(e.to_string()))?);
        }
        let bytes = archives[path].read_all(entry)?;
        match only_texture(&bytes, entry) {
            Ok(img) => tiles.push(Some(img)),
            Err(e) => {
                problems.push(e);
                tiles.push(None);
            }
        }
    }
    Ok(grid_picture(tiles, grid.side, problems))
}

/// Vice City's pause-menu map, from MODELS/FRONTEN2.TXD.
pub fn vc_menu_map(vfs: &dyn Vfs) -> Result<Picture, Error> {
    let bytes = vfs.read_all(VC_MENU_TXD)?;
    let c = ef_texture::collection::parse(&bytes)?;
    let mut problems = Vec::new();
    let tiles = VC_MENU_TILES
        .iter()
        .map(|name| match c.textures.iter().find(|t| t.name.eq_ignore_ascii_case(name)) {
            Some(t) => t.raster.decode(0).map_err(|e| problems.push(format!("{name}: {e}"))).ok(),
            None => {
                problems.push(format!("{name} is not in {VC_MENU_TXD}"));
                None
            }
        })
        .collect();
    Ok(grid_picture(tiles, 3, problems))
}

/// San Andreas' pause-menu map: the radar map over `MENU_MAP_BACKGROUND`.
pub fn sa_menu_map(radar: &Picture) -> Picture {
    let [r, g, b] = SA_MENU_BACKGROUND.map(u32::from);
    let mut pixels = radar.image.pixels.clone();
    for p in pixels.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        for (c, bg) in p[..3].iter_mut().zip([r, g, b]) {
            *c = ((*c as u32 * a + bg * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    Picture {
        image: Rgba { width: radar.image.width, height: radar.image.height, pixels },
        tile: radar.tile,
        problems: radar.problems.clone(),
    }
}
