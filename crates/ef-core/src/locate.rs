//! Finds installed copies of the games in the usual places.
//!
//! * Steam: the client's root folder (Windows registry `HKCU\Software\Valve\Steam\SteamPath`,
//!   then `HKLM\SOFTWARE\Valve\Steam\InstallPath`; on Linux and macOS the folders steamlocate 2.1.1
//!   and Lutris check), every library in `steamapps/libraryfolders.vdf` (both its current form,
//!   `"0" { "path" "..." }`, and the older `"1" "D:\\Library"` form, as Playnite's Steam library
//!   reads it), and each game's `steamapps/appmanifest_<id>.acf` `installdir`. App IDs and
//!   install folder names from Steam's app info (api.steamcmd.net): 12100 GTA III, 12110 Vice
//!   City, 12120 San Andreas, 12230 GTA III (Mac), 12250 San Andreas (Mac).
//! * Rockstar Games Launcher and retail setups (Windows): the registry entries described in
//!   [`crate::store`], and every folder directly inside `Rockstar Games` under Program Files that
//!   passes detection. Exact folder names are not assumed.
//!
//! Every candidate is confirmed with [`crate::detect`] before it is offered, and labelled with
//! the store it belongs to ([`crate::store::identify`]), which is separate from the executable
//! build ([`crate::edition`]).

use std::path::{Path, PathBuf};

use crate::store::Store;
use crate::{Game, GameInstall, vfs::FolderVfs};

/// Steam app IDs and their `installdir` (api.steamcmd.net app info).
pub const STEAM_APPS: &[(u32, Game, &str)] = &[
    (12100, Game::Gta3, "Grand Theft Auto 3"),
    (12110, Game::ViceCity, "Grand Theft Auto Vice City"),
    (12120, Game::SanAndreas, "Grand Theft Auto San Andreas"),
    (12230, Game::Gta3, "grand theft auto 3"),
    (12250, Game::SanAndreas, "grand theft auto - san andreas"),
];

#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    /// Where it was found, e.g. "Steam library D:\SteamLibrary".
    pub source: String,
    /// The store the copy belongs to.
    pub store: Store,
    /// The detected game, or why the folder was not recognised.
    pub result: Result<GameInstall, String>,
}

// ---- Valve KeyValues (text) -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    Str(String),
    Obj(Vec<(String, Kv)>),
}

impl Kv {
    pub fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::Obj(v) => v.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Kv::Str(_) => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Kv::Str(s) => Some(s),
            Kv::Obj(_) => None,
        }
    }
}

fn kv_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                let mut t = String::new();
                while let Some(c) = it.next() {
                    match c {
                        '\\' => match it.next() {
                            Some('n') => t.push('\n'),
                            Some('t') => t.push('\t'),
                            Some(o) => t.push(o),
                            None => {}
                        },
                        '"' => break,
                        o => t.push(o),
                    }
                }
                out.push(format!("\"{t}"));
            }
            '{' | '}' => out.push(c.to_string()),
            '/' if it.peek() == Some(&'/') => {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Parses a KeyValues text document into its root object.
pub fn parse_kv(s: &str) -> Kv {
    fn obj(t: &[String], i: &mut usize) -> Kv {
        let mut v = Vec::new();
        while *i < t.len() && t[*i] != "}" {
            let Some(key) = t[*i].strip_prefix('"') else {
                *i += 1;
                continue;
            };
            *i += 1;
            match t.get(*i).map(String::as_str) {
                Some("{") => {
                    *i += 1;
                    v.push((key.to_string(), obj(t, i)));
                    *i += 1; // closing brace
                }
                Some(val) if val.starts_with('"') => {
                    v.push((key.to_string(), Kv::Str(val[1..].to_string())));
                    *i += 1;
                }
                _ => {}
            }
        }
        Kv::Obj(v)
    }
    let t = kv_tokens(s);
    obj(&t, &mut 0)
}

/// Library folders listed in `libraryfolders.vdf`.
pub fn steam_libraries(vdf: &str) -> Vec<PathBuf> {
    let root = parse_kv(vdf);
    let Some(Kv::Obj(entries)) = root.get("libraryfolders").or_else(|| root.get("LibraryFolders")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, v) in entries {
        if !k.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let p = match v {
            Kv::Str(s) => Some(s.as_str()),
            Kv::Obj(_) => v.get("path").and_then(Kv::as_str),
        };
        if let Some(p) = p {
            out.push(PathBuf::from(p));
        }
    }
    out
}

/// Steam client folders on this computer.
pub fn steam_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    #[cfg(windows)]
    v.extend(windows::steam_roots());
    if let Some(home) = dirs::home_dir() {
        if cfg!(target_os = "macos") {
            v.push(home.join("Library/Application Support/Steam"));
        } else if cfg!(unix) {
            let data =
                std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
            v.push(data.join("Steam"));
            for p in [
                ".steam/steam",
                ".steam/root",
                ".steam/debian-installation",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
                ".var/app/com.valvesoftware.Steam/.steam/steam",
                ".var/app/com.valvesoftware.Steam/.steam/root",
                "snap/steam/common/.local/share/Steam",
                "snap/steam/common/.steam/steam",
                "snap/steam/common/.steam/root",
            ] {
                v.push(home.join(p));
            }
        }
    }
    v.retain(|p| p.join("steamapps").is_dir());
    dedup(v)
}

fn dedup(v: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for p in v {
        let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if !seen.contains(&key) {
            seen.push(key);
            out.push(p);
        }
    }
    out
}

/// Game folders inside the given Steam roots: `(folder, description)`.
pub fn steam_candidates(roots: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut libs = Vec::new();
    for r in roots {
        libs.push(r.clone());
        if let Ok(s) = std::fs::read_to_string(r.join("steamapps").join("libraryfolders.vdf")) {
            libs.extend(steam_libraries(&s));
        }
    }
    let mut out = Vec::new();
    for lib in dedup(libs) {
        for &(id, _, default_dir) in STEAM_APPS {
            let manifest = lib.join("steamapps").join(format!("appmanifest_{id}.acf"));
            let dir = std::fs::read_to_string(&manifest)
                .ok()
                .and_then(|s| parse_kv(&s).get("AppState")?.get("installdir")?.as_str().map(str::to_string));
            let folder = lib.join("steamapps").join("common").join(dir.as_deref().unwrap_or(default_dir));
            if folder.is_dir() {
                out.push((folder, format!("Steam library {} (app {id})", lib.display())));
            }
        }
    }
    out
}

/// Folders directly inside `base` that hold a recognised game.
pub fn recognised_folders(base: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(base) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !crate::possible_choices(&FolderVfs::new(p)).is_empty())
        .collect();
    out.sort();
    out
}

/// Every candidate on this computer, before detection.
pub fn candidates() -> Vec<(PathBuf, String)> {
    let mut v = steam_candidates(&steam_roots());
    v.extend(
        crate::store::rgl_installs().into_iter().map(|(g, p)| (p, format!("Rockstar Games Launcher ({})", g.short()))),
    );
    v.extend(crate::store::retail_installs());
    #[cfg(windows)]
    v.extend(windows::program_files_candidates());
    v
}

/// Scans the usual places and detects the game in each candidate (duplicates removed).
pub fn scan() -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (path, source) in candidates() {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.contains(&key) || !path.is_dir() {
            continue;
        }
        seen.push(key);
        let result = crate::open_with(&path, None).map_err(|e| e.to_string());
        let store = result.as_ref().map(|i| i.store.clone()).unwrap_or_else(|_| crate::store::identify(&path).0);
        out.push(Found { path, source, store, result });
    }
    out
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    use crate::store::windows_read as read;

    pub fn steam_roots() -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = read(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")
            .into_iter()
            .map(|s| PathBuf::from(s.replace('/', "\\")))
            .collect();
        v.extend(read(HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath").into_iter().map(PathBuf::from));
        v
    }

    pub fn program_files_candidates() -> Vec<(PathBuf, String)> {
        let mut out = Vec::new();
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(pf) = std::env::var_os(var) {
                let base = PathBuf::from(pf).join("Rockstar Games");
                for p in super::recognised_folders(&base) {
                    out.push((p, format!("{}", base.display())));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn reads_both_library_folder_layouts() {
        let new = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "12120" "4651839488" }
	}
	"1" { "path" "D:\\SteamLibrary" }
}"#;
        assert_eq!(
            steam_libraries(new),
            [PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]
        );
        let old = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\"1\"\n\t\"1\"\t\"E:\\\\Games\"\n}\n";
        assert_eq!(steam_libraries(old), [PathBuf::from(r"E:\Games")]);
    }

    #[test]
    fn finds_a_game_in_a_steam_library() {
        let root = std::env::temp_dir().join(format!("ef-steam-{}", std::process::id()));
        let lib = root.join("lib2");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
                lib.display().to_string().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        std::fs::create_dir_all(lib.join("steamapps/common/Custom Dir/audio")).unwrap();
        std::fs::write(
            lib.join("steamapps/appmanifest_12110.acf"),
            "\"AppState\" { \"appid\" \"12110\" \"installdir\" \"Custom Dir\" }",
        )
        .unwrap();
        for f in ["WILD.ADF", "FLASH.ADF", "KCHAT.ADF", "FEVER.ADF", "VROCK.ADF", "VCPR.ADF"] {
            std::fs::write(lib.join("steamapps/common/Custom Dir/audio").join(f), b"x").unwrap();
        }
        let c = steam_candidates(std::slice::from_ref(&root));
        assert_eq!(c.len(), 1, "{c:?}");
        assert!(c[0].0.ends_with("Custom Dir"));
        let g = crate::detect(Arc::new(FolderVfs::new(&c[0].0))).unwrap();
        assert_eq!(g.game, Game::ViceCity);
        std::fs::remove_dir_all(root).ok();
    }
}
