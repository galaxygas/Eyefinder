//! What Eyefinder knows about each game's files (`data/formats.json`): every file type per game
//! and platform, with its status and sources. The Documentation page shows it, and `MISSING.md`
//! is generated from its rows with status [`Status::Missing`] (`eyefinder-cli docs missing`; a
//! test keeps the file in sync).

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Output checked against real game files.
    Verified,
    /// Implemented from the cited specification; not yet confirmed on real files.
    Spec,
    /// A specification exists, but Eyefinder does not implement it.
    Documented,
    /// Recognised, but no trustworthy specification covers it, so it is not decoded.
    Unsupported,
    /// No public documentation was found (listed in MISSING.md).
    Missing,
}

impl Status {
    pub const ALL: [Status; 5] =
        [Status::Verified, Status::Spec, Status::Documented, Status::Unsupported, Status::Missing];
    pub fn label(self) -> &'static str {
        match self {
            Status::Verified => "Verified",
            Status::Spec => "Implemented, unverified",
            Status::Documented => "Documented, not implemented",
            Status::Unsupported => "Not supported",
            Status::Missing => "Undocumented",
        }
    }
    pub fn explanation(self) -> &'static str {
        match self {
            Status::Verified => "Eyefinder's output was checked against real game files.",
            Status::Spec => {
                "Implemented from the cited specification and tested on synthetic files; not yet confirmed on real game files."
            }
            Status::Documented => "A public specification exists, but Eyefinder does not implement it yet.",
            Status::Unsupported => {
                "Recognised, but no trustworthy specification covers it, so Eyefinder does not decode it."
            }
            Status::Missing => {
                "No public documentation was found. Listed in MISSING.md; a real-file report can settle it."
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub intro: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    /// Release-index game ids; `*` = every game.
    pub games: Vec<String>,
    /// Release-index platform ids.
    pub platforms: Vec<String>,
    /// audio, texture, text, map, video, script, exe, container, identification, other.
    pub kind: String,
    pub data: String,
    pub files: String,
    pub status: Status,
    pub sources: Vec<String>,
    #[serde(default)]
    pub confirmation: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// MISSING.md section of a missing row.
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub known: Option<String>,
    #[serde(default)]
    pub needed: Option<String>,
}

impl Entry {
    pub fn covers(&self, game: &str) -> bool {
        self.games.iter().any(|g| g == game || g == "*")
    }
}

#[derive(Debug, Deserialize)]
pub struct Registry {
    pub about: String,
    pub sections: Vec<Section>,
    pub entries: Vec<Entry>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

pub fn registry() -> &'static Registry {
    REGISTRY
        .get_or_init(|| serde_json::from_str(include_str!("../data/formats.json")).expect("data/formats.json is valid"))
}

impl Registry {
    /// The rows about a release-index game, including the ones about every game.
    pub fn for_game<'a>(&'a self, game: &'a str) -> impl Iterator<Item = &'a Entry> + 'a {
        self.entries.iter().filter(move |e| e.covers(game))
    }
    /// How many rows have each status (in [`Status::ALL`] order).
    pub fn counts<'a>(entries: impl Iterator<Item = &'a Entry>) -> [usize; 5] {
        let mut c = [0; 5];
        for e in entries {
            c[Status::ALL.iter().position(|s| *s == e.status).unwrap()] += 1;
        }
        c
    }
}

const MISSING_HEAD: &str = "# Missing documentation

<!-- Generated from crates/ef-core/data/formats.json by `eyefinder-cli docs missing`. Edit that
file, then regenerate; a test fails when they differ. -->

This file lists every file type, data layout and identity that Eyefinder needs but for which **no
public documentation was found**. They could only be supported through original reverse engineering
of real game files or executables. Until then they stay **Not supported yet**
(`SupportLevel::DetectedOnly`), as [AGENTS.md](AGENTS.md) requires. The Documentation page of the
app shows the same list, next to everything that is documented.

How this list was made (research on 2026-10-21/23):
* DMA Design's own documents: CityScape Data Structure 12.10, GBH Style Format 4.6, GBH Map Format 8.2, and the GTA2 scripting notes.
* Daniel Marschall's GTA1/London pages and his [gta_lang_converter](https://github.com/danielmarschall/gta_lang_converter).
* gta2_re ([CriminalRETeam/gta2_re](https://github.com/CriminalRETeam/gta2_re) @ `edb484d3`).
* OpenGTA, vgmstream (@ `f7f76415`), psx-spx, GTA-Advance-Studio (@ `7eae5b3e`), cue2pops (@ `3f2be612`), Flycast (@ `fa9d39a3`), Redream (@ `ffb73022`).
* gtamp.com, redump.org, the No-Intro / redump DATs in libretro-database (@ `12c21cf7`) and the DuckStation game database.
* Real files from the maintainer (2026-10-23): GTA2 PC (2004 release), GTA2 PlayStation (VCD), GTA2 Dreamcast (CDI), GTA1 PC disc; see [docs/SUPPORT.md](docs/SUPPORT.md#real-file-observations-2026-10-23).
* gtamods.com (the GTA modding wiki), tcrf.net, Sega Retro and PCGamingWiki could not be reached during the research (HTTP 503 / 403 / bot walls). Their pages may document some items below and should be checked when they are back.

Documented formats that Eyefinder simply has not implemented yet (Smacker and Bink movies, CMP and
GMP maps, GTA2 Japanese text) are **not** listed here. They are in
[docs/SUPPORT.md](docs/SUPPORT.md).

## How to help

A report from a real copy resolves most of these items, without uploading game files:
* the file list of the disc or install (`eyefinder-cli files` prints it; the Files view shows it);
* for a single file, its first bytes (the Files view shows them) and its size;
* the `manifest.json` of an export.

See [CONTRIBUTING.md](CONTRIBUTING.md#reporting-a-format-result).
";

/// `MISSING.md`, generated from the registry's missing rows and the release index's gaps.
pub fn missing_markdown() -> String {
    let r = registry();
    let mut out = MISSING_HEAD.to_string();
    for s in &r.sections {
        let rows: Vec<&Entry> =
            r.entries.iter().filter(|e| e.status == Status::Missing && e.section.as_deref() == Some(&s.id)).collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {}\n\n", s.title));
        if !s.intro.is_empty() {
            out.push_str(&format!("{}\n\n", s.intro));
        }
        out.push_str("| Data | What is known | What is missing |\n|---|---|---|\n");
        for e in rows {
            let cell = |t: &str| t.replace('|', "\\|").replace('\n', " ");
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                cell(&e.data),
                cell(e.known.as_deref().unwrap_or("")),
                cell(e.needed.as_deref().unwrap_or(""))
            ));
        }
    }
    out.push_str(
        "\n## Release index gaps\n\nThe release index (Index page, `crates/ef-core/data/release_index.json`) lists only what its \
         pinned sources name. These releases and versions have no reachable, citable source yet:\n\n",
    );
    for g in &crate::index::index().gaps {
        out.push_str(&format!("* **{}**: {}\n", g.platforms, g.text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_complete() {
        let r = registry();
        let index = crate::index::index();
        for e in &r.entries {
            assert!(!e.data.is_empty() && !e.games.is_empty() && !e.platforms.is_empty(), "{e:?}");
            for g in &e.games {
                assert!(g == "*" || index.game(g).is_some(), "unknown game {g}");
            }
            for p in &e.platforms {
                assert!(index.platforms.contains_key(p), "unknown platform {p}");
            }
            if e.status == Status::Missing {
                let s = e.section.as_deref().expect("a missing row names its MISSING.md section");
                assert!(r.sections.iter().any(|x| x.id == s), "{s}");
                assert!(e.known.is_some() && e.needed.is_some(), "{}", e.data);
            } else {
                assert!(e.section.is_none(), "{}", e.data);
            }
            if matches!(e.status, Status::Verified | Status::Spec | Status::Documented) {
                assert!(!e.sources.is_empty(), "{} names no source", e.data);
            }
            if e.status == Status::Verified {
                assert!(e.confirmation.is_some(), "{} is verified without a real-file confirmation", e.data);
            }
        }
    }

    #[test]
    fn missing_md_is_in_sync() {
        // A Windows checkout may turn the file's line endings into CRLF.
        let file = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../MISSING.md"))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(
            file == missing_markdown(),
            "MISSING.md differs from crates/ef-core/data/formats.json: run `cargo run -p eyefinder-cli -- docs missing > MISSING.md`"
        );
    }
}
