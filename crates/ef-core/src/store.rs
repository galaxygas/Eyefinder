//! Where an installed PC copy came from: Steam, the Rockstar Games Launcher or a retail setup.
//!
//! The executable build ([`crate::edition`]) cannot always tell stores apart (the GTA III and Vice
//! City Rockstar Games Launcher builds carry the Steam build's signature), so the store is
//! identified from the installation itself:
//!
//! * Steam: the folder is `<library>/steamapps/common/<installdir>` and
//!   `<library>/steamapps/appmanifest_<id>.acf` names that `installdir` for one of the app IDs in
//!   [`crate::locate::STEAM_APPS`].
//! * Rockstar Games Launcher (Windows): the launcher registers each title under
//!   `HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\<GUID>` with its folder
//!   in `InstallLocation`. GUIDs from tylerbrawl Galaxy-Plugin-Rockstar `src/game_cache.py` and
//!   `src/local.py` (commit 5518f066): GTA III `{92B94569-6683-4617-8C54-EB27A1B51B30}`, Vice City
//!   `{4B35F00C-E63D-40DC-9839-DF15A33EAC46}`, San Andreas `{D417C96A-FCC7-4590-A1BB-FAF73F5BC98E}`.
//! * Retail setup (Windows): the install paths the setup programs store
//!   (`Rockstar Games\GTA San Andreas\Installation\ExePath`, `Rockstar Games\GTAIII\Path`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Game;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Store {
    Steam {
        app: u32,
    },
    RockstarLauncher,
    RetailSetup,
    #[default]
    Unknown,
}

impl Store {
    pub fn label(&self) -> String {
        match self {
            Store::Steam { app } => format!("Steam (app {app})"),
            Store::RockstarLauncher => "Rockstar Games Launcher".into(),
            Store::RetailSetup => "Retail setup".into(),
            Store::Unknown => "Unknown store".into(),
        }
    }
    pub fn is_unknown(&self) -> bool {
        *self == Store::Unknown
    }
}

/// Rockstar Games Launcher uninstall GUIDs (see the module documentation).
pub const RGL_TITLES: &[(Game, &str)] = &[
    (Game::Gta3, "{92B94569-6683-4617-8C54-EB27A1B51B30}"),
    (Game::ViceCity, "{4B35F00C-E63D-40DC-9839-DF15A33EAC46}"),
    (Game::SanAndreas, "{D417C96A-FCC7-4590-A1BB-FAF73F5BC98E}"),
];

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (a, b) = (canon(a), canon(b));
    a == b || a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

/// The Steam app whose manifest installs into `folder`, if `folder` is inside a Steam library.
pub fn steam_app_of(folder: &Path) -> Option<u32> {
    let name = folder.file_name()?.to_string_lossy().to_string();
    let common = folder.parent()?;
    let steamapps = common.parent()?;
    if !common.file_name()?.to_string_lossy().eq_ignore_ascii_case("common")
        || !steamapps.file_name()?.to_string_lossy().eq_ignore_ascii_case("steamapps")
    {
        return None;
    }
    crate::locate::STEAM_APPS.iter().find_map(|&(id, _, _)| {
        let acf = std::fs::read_to_string(steamapps.join(format!("appmanifest_{id}.acf"))).ok()?;
        let dir = crate::locate::parse_kv(&acf).get("AppState")?.get("installdir")?.as_str()?.to_string();
        dir.eq_ignore_ascii_case(&name).then_some(id)
    })
}

/// Rockstar Games Launcher installations on this computer: `(game, folder)`.
pub fn rgl_installs() -> Vec<(Game, PathBuf)> {
    #[cfg(windows)]
    {
        windows::rgl_installs()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Retail setup installations on this computer: `(folder, description)`.
pub fn retail_installs() -> Vec<(PathBuf, String)> {
    #[cfg(windows)]
    {
        windows::retail_installs()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Identifies the store of an installation folder. Returns the store and the evidence line.
pub fn identify(folder: &Path) -> (Store, Option<String>) {
    if let Some(app) = steam_app_of(folder) {
        return (
            Store::Steam { app },
            Some(format!("the folder is in a Steam library and appmanifest_{app}.acf installs into it")),
        );
    }
    if let Some((g, _)) = rgl_installs().into_iter().find(|(_, p)| same_dir(p, folder)) {
        return (
            Store::RockstarLauncher,
            Some(format!("the Rockstar Games Launcher registered this folder for {}", g.name())),
        );
    }
    if let Some((_, why)) = retail_installs().into_iter().find(|(p, _)| same_dir(p, folder)) {
        return (Store::RetailSetup, Some(format!("this folder is recorded by the {why}")));
    }
    (Store::Unknown, None)
}

#[cfg(windows)]
pub(crate) use windows::read as windows_read;

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

    use crate::Game;

    pub fn read(root: winreg::HKEY, path: &str, value: &str) -> Vec<String> {
        let mut v = Vec::new();
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            if let Ok(k) = RegKey::predef(root).open_subkey_with_flags(path, KEY_READ | view)
                && let Ok(s) = k.get_value::<String, _>(value)
                && !v.contains(&s)
            {
                v.push(s);
            }
        }
        v
    }

    fn clean(s: &str) -> PathBuf {
        // Galaxy-Plugin-Rockstar strips quotes and a trailing ",0" from these values.
        PathBuf::from(s.trim().trim_matches('"').trim_end_matches(",0").trim_matches('"'))
    }

    pub fn rgl_installs() -> Vec<(Game, PathBuf)> {
        let mut out = Vec::new();
        for &(game, guid) in super::RGL_TITLES {
            // The 32-bit view of SOFTWARE\Microsoft\...\Uninstall is SOFTWARE\WOW6432Node\... .
            for s in read(
                HKEY_LOCAL_MACHINE,
                &format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{guid}"),
                "InstallLocation",
            ) {
                out.push((game, clean(&s)));
            }
        }
        out
    }

    pub fn retail_installs() -> Vec<(PathBuf, String)> {
        let mut out = Vec::new();
        for s in read(HKEY_LOCAL_MACHINE, r"SOFTWARE\Rockstar Games\GTA San Andreas\Installation", "ExePath") {
            if let Some(dir) = clean(&s).parent() {
                out.push((dir.to_path_buf(), "San Andreas setup (registry)".to_string()));
            }
        }
        for s in read(HKEY_LOCAL_MACHINE, r"SOFTWARE\Rockstar Games\GTAIII", "Path") {
            out.push((clean(&s), "GTA III setup (registry)".to_string()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_folders_are_identified_by_their_manifest() {
        let root = std::env::temp_dir().join(format!("ef-store-{}", std::process::id()));
        let game = root.join("steamapps/common/Grand Theft Auto San Andreas");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(
            root.join("steamapps/appmanifest_12120.acf"),
            "\"AppState\" { \"appid\" \"12120\" \"installdir\" \"Grand Theft Auto San Andreas\" }",
        )
        .unwrap();
        assert_eq!(identify(&game).0, Store::Steam { app: 12120 });
        let other = root.join("steamapps/common/Something Else");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(identify(&other).0, Store::Unknown);
        let rgl_like = root.join("Rockstar Games/Grand Theft Auto San Andreas");
        std::fs::create_dir_all(&rgl_like).unwrap();
        assert_eq!(steam_app_of(&rgl_like), None);
        std::fs::remove_dir_all(root).ok();
    }
}
