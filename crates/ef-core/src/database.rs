//! The build database: every documented build, version, patch, edition, port, demo, prototype and
//! leak of every Grand Theft Auto game, with the sources for each (`data/database/*.toml`, imported
//! from `docs/research/GTA_Builds_Versions_Database.md` by `tools/db/import_research_md.py`).
//!
//! It is plain TOML so that anyone can read and correct it, and it is layered so that it can grow
//! without a new Eyefinder build:
//! 1. the files built into Eyefinder;
//! 2. a **database pack**: a complete, newer database imported from a `.zip` or downloaded from a
//!    `db-<version>` GitHub release. It replaces the built-in files when its version is newer and
//!    its schema is one this build understands;
//! 3. the **user's files** (`user/*.toml`): entries with a known `id` replace that entry, `remove =
//!    true` hides it, and new ids add entries. The app's editor writes `user/edits.toml`.
//!
//! A broken file never stops Eyefinder: its problems are listed in [`Database::problems`].
//! The format is described in docs/DATABASE.md.
//!
//! The release index ([`crate::index`]) is a separate, generated layer of dump records (hashes,
//! serials) used to identify copies; [`Database::dumps_of`] links an entry to it by serial.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::index::{self, IndexGame, Release};

/// The newest schema this build reads. A pack with a higher schema is ignored.
pub const SCHEMA: u32 = 1;

/// The repository whose `db-<version>` releases carry database packs.
pub const UPDATE_REPO: &str = "galaxygas/Eyefinder";

const EMBEDDED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/database_files.rs"));

fn is_empty(s: &str) -> bool {
    s.is_empty()
}

/// Where an item came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Layer {
    #[default]
    BuiltIn,
    Pack,
    /// A user file, by file name.
    User(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    /// Unique and stable: user files replace or hide an entry by its id.
    pub id: String,
    /// `md:<line>` for entries imported from the research file.
    #[serde(skip_serializing_if = "is_empty")]
    pub origin: String,
    /// The [`Section`] id the entry belongs to (may be empty for user entries).
    #[serde(skip_serializing_if = "is_empty")]
    pub section: String,
    /// One of [`Database::kinds`].
    #[serde(skip_serializing_if = "is_empty")]
    pub kind: String,
    /// Game ids ([`Database::games`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub games: Vec<String>,
    #[serde(skip_serializing_if = "is_empty")]
    pub title: String,
    /// Platform ids ([`Database::platforms`]); empty when the source names none or all.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    /// The platform text as the source gives it.
    #[serde(skip_serializing_if = "is_empty")]
    pub platform_text: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub region: String,
    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, only when the source gives one date.
    #[serde(skip_serializing_if = "is_empty")]
    pub date: String,
    /// The date text as the source gives it (conflicts, ranges, comments).
    #[serde(skip_serializing_if = "is_empty")]
    pub date_text: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub version: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub serials: Vec<String>,
    /// Serials, IDs, executable versions, sizes and hashes as the source gives them.
    #[serde(skip_serializing_if = "is_empty")]
    pub id_text: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub notes: String,
    /// `confirmed`, `single-source`, `unverified`, `mixed` or empty.
    #[serde(skip_serializing_if = "is_empty")]
    pub confidence: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub confidence_text: String,
    /// Keys of [`Database::sources`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(skip_serializing_if = "is_empty")]
    pub sources_text: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub game_text: String,
    /// Every other column of the source table, verbatim.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub fields: IndexMap<String, String>,
    /// In a user file: hide the entry with this id.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub remove: bool,
    #[serde(skip)]
    pub layer: Layer,
}

impl Entry {
    /// The date to sort and show: the normalised date, else the source's text.
    pub fn when(&self) -> &str {
        if self.date.is_empty() { &self.date_text } else { &self.date }
    }
    /// Whether any text of the entry contains `needle` (lower-case).
    pub fn matches(&self, needle: &str) -> bool {
        let texts = [
            &self.id,
            &self.title,
            &self.platform_text,
            &self.region,
            &self.date,
            &self.date_text,
            &self.version,
            &self.id_text,
            &self.notes,
            &self.game_text,
        ];
        texts.iter().any(|t| t.to_lowercase().contains(needle))
            || self.serials.iter().any(|s| s.to_lowercase().contains(needle))
            || self.fields.values().any(|v| v.to_lowercase().contains(needle))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Section {
    pub id: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub origin: String,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub games: Vec<String>,
    /// The section's prose (Markdown) as the source wrote it.
    #[serde(skip_serializing_if = "is_empty")]
    pub text: String,
    #[serde(skip)]
    pub layer: Layer,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Source {
    pub text: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub url: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub origin: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GameInfo {
    pub id: String,
    pub title: String,
    /// The [`crate::Game::cli_id`] when Eyefinder reads this game.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eyefinder: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Header {
    /// `YYYY.MM.DD`, optionally with `.N` for several releases on one day.
    pub version: String,
    pub schema: u32,
}

/// One database file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DbFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<Header>,
    #[serde(rename = "game", skip_serializing_if = "Vec::is_empty")]
    pub games: Vec<GameInfo>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub platforms: IndexMap<String, String>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub kinds: IndexMap<String, String>,
    #[serde(rename = "section", skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<Section>,
    #[serde(rename = "entry", skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<Entry>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub sources: IndexMap<String, Source>,
}

impl DbFile {
    pub fn parse(text: &str) -> Result<DbFile, DbError> {
        toml::from_str(text).map_err(|e| DbError::Toml(e.to_string()))
    }
    pub fn to_toml(&self) -> Result<String, DbError> {
        toml::to_string_pretty(self).map_err(|e| DbError::Toml(e.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("TOML: {0}")]
    Toml(String),
    #[error("archive: {0}")]
    Zip(String),
    #[error("download: {0}")]
    Net(String),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub file: String,
    pub message: String,
}

/// Where the pack and the user files live.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    pub pack: Option<PathBuf>,
    pub user: Option<PathBuf>,
}

impl Paths {
    /// `<data dir>/eyefinder/database/{pack,user}`, e.g. `%APPDATA%\eyefinder\database` on Windows.
    pub fn standard() -> Paths {
        let root = dirs::data_dir().map(|d| d.join("eyefinder").join("database"));
        Paths { pack: root.as_ref().map(|r| r.join("pack")), user: root.map(|r| r.join("user")) }
    }
    /// The built-in files only.
    pub fn none() -> Paths {
        Paths::default()
    }
    pub fn root(&self) -> Option<PathBuf> {
        self.user.as_ref().and_then(|u| u.parent()).map(Path::to_path_buf)
    }
    fn edits(&self) -> Result<PathBuf, DbError> {
        self.user.as_ref().map(|u| u.join("edits.toml")).ok_or_else(|| DbError::Invalid("no user folder".into()))
    }
}

/// Which complete database the user layer applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    BuiltIn,
    Pack,
}

#[derive(Debug, Clone)]
pub struct Database {
    pub version: String,
    pub schema: u32,
    pub base: Base,
    /// The version of the built-in files, whatever the base.
    pub built_in_version: String,
    pub games: Vec<GameInfo>,
    pub platforms: IndexMap<String, String>,
    pub kinds: IndexMap<String, String>,
    pub sections: Vec<Section>,
    pub entries: Vec<Entry>,
    pub sources: IndexMap<String, Source>,
    /// User files applied, by name.
    pub user_files: Vec<String>,
    /// Ids replaced or hidden by user files.
    pub edited: Vec<String>,
    pub hidden: Vec<String>,
    pub problems: Vec<Problem>,
    /// Things worth knowing that are not errors (an ignored pack, ...).
    pub notices: Vec<String>,
}

/// Compares `YYYY.MM.DD[.N]` versions numerically.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| s.split(['.', '-']).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parts(a).cmp(&parts(b))
}

fn parse_set<'a>(
    files: impl IntoIterator<Item = (String, &'a str)>,
    problems: &mut Vec<Problem>,
) -> Vec<(String, DbFile)> {
    let mut out = Vec::new();
    for (name, text) in files {
        match DbFile::parse(text) {
            Ok(f) => out.push((name, f)),
            Err(e) => problems.push(Problem { file: name, message: e.to_string() }),
        }
    }
    out
}

fn read_dir_toml(dir: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let p = e.ok()?.path();
            if p.extension()? != "toml" {
                return None;
            }
            Some((p.file_name()?.to_string_lossy().into_owned(), std::fs::read_to_string(&p).ok()?))
        })
        .collect();
    v.sort();
    v
}

fn header_of(files: &[(String, DbFile)]) -> Option<Header> {
    files.iter().find(|(n, _)| n == "database.toml").and_then(|(_, f)| f.database.clone())
}

impl Database {
    /// The built-in files, then a newer pack, then the user files.
    pub fn load(paths: &Paths) -> Database {
        let mut problems = Vec::new();
        let mut notices = Vec::new();
        let built_in = parse_set(EMBEDDED.iter().map(|(n, t)| (n.to_string(), *t)), &mut problems);
        let built_in_header = header_of(&built_in).unwrap_or(Header { version: "0".into(), schema: SCHEMA });
        let mut base_files = built_in;
        let mut base = Base::BuiltIn;
        let mut header = built_in_header.clone();
        if let Some(dir) = paths.pack.as_ref().filter(|d| d.join("database.toml").is_file()) {
            let texts = read_dir_toml(dir);
            let mut pack_problems = Vec::new();
            let pack = parse_set(texts.iter().map(|(n, t)| (n.clone(), t.as_str())), &mut pack_problems);
            match header_of(&pack) {
                _ if !pack_problems.is_empty() => {
                    notices.push("The installed database pack has errors and is not used.".into());
                    problems.extend(pack_problems.into_iter().map(|p| Problem { file: format!("pack/{}", p.file), ..p }));
                }
                None => notices.push("The installed database pack has no [database] header and is not used.".into()),
                Some(h) if h.schema > SCHEMA => notices.push(format!(
                    "The installed database pack {} needs a newer Eyefinder (schema {}, this build reads {SCHEMA}).",
                    h.version, h.schema
                )),
                Some(h) if compare_versions(&h.version, &built_in_header.version) != Ordering::Greater => {
                    notices.push(format!(
                        "The installed database pack {} is not newer than the built-in database {}; the built-in one is used.",
                        h.version, built_in_header.version
                    ))
                }
                Some(h) => {
                    base_files = pack;
                    base = Base::Pack;
                    header = h;
                }
            }
        }
        let layer = if base == Base::Pack { Layer::Pack } else { Layer::BuiltIn };
        let mut db = Database {
            version: header.version,
            schema: header.schema,
            base,
            built_in_version: built_in_header.version,
            games: Vec::new(),
            platforms: IndexMap::new(),
            kinds: IndexMap::new(),
            sections: Vec::new(),
            entries: Vec::new(),
            sources: IndexMap::new(),
            user_files: Vec::new(),
            edited: Vec::new(),
            hidden: Vec::new(),
            problems,
            notices,
        };
        // database.toml first, so the games, platforms and kinds are known.
        base_files.sort_by_key(|(n, _)| (n != "database.toml", n.clone()));
        for (name, f) in base_files {
            db.apply(&name, f, layer.clone(), false);
        }
        if let Some(dir) = &paths.user {
            for (name, text) in read_dir_toml(dir) {
                match DbFile::parse(&text) {
                    Ok(f) => {
                        db.apply(&name, f, Layer::User(name.clone()), true);
                        db.user_files.push(name);
                    }
                    Err(e) => db.problems.push(Problem { file: format!("user/{name}"), message: e.to_string() }),
                }
            }
        }
        db.validate();
        db
    }

    /// The `.toml` files of `dir` as one complete database, whatever its version (for checking a
    /// database folder before it is packed or contributed).
    pub fn load_complete(dir: &Path) -> Database {
        let mut problems = Vec::new();
        let texts = read_dir_toml(dir);
        let mut files = parse_set(texts.iter().map(|(n, t)| (n.clone(), t.as_str())), &mut problems);
        let header = header_of(&files);
        if header.is_none() {
            problems.push(Problem { file: "database.toml".into(), message: "missing, or no [database] header".into() });
        }
        let header = header.unwrap_or(Header { version: "0".into(), schema: SCHEMA });
        let mut db = Database {
            version: header.version.clone(),
            schema: header.schema,
            base: Base::Pack,
            built_in_version: header.version,
            games: Vec::new(),
            platforms: IndexMap::new(),
            kinds: IndexMap::new(),
            sections: Vec::new(),
            entries: Vec::new(),
            sources: IndexMap::new(),
            user_files: Vec::new(),
            edited: Vec::new(),
            hidden: Vec::new(),
            problems,
            notices: Vec::new(),
        };
        files.sort_by_key(|(n, _)| (n != "database.toml", n.clone()));
        for (name, f) in files {
            db.apply(&name, f, Layer::Pack, false);
        }
        db.validate();
        db
    }

    /// Adds a file's items. `user`: same ids replace (or hide) instead of being duplicates.
    fn apply(&mut self, name: &str, f: DbFile, layer: Layer, user: bool) {
        let file = match &layer {
            Layer::User(n) => format!("user/{n}"),
            Layer::Pack => format!("pack/{name}"),
            Layer::BuiltIn => name.to_string(),
        };
        for g in f.games {
            match self.games.iter_mut().find(|x| x.id == g.id) {
                Some(x) => *x = g,
                None => self.games.push(g),
            }
        }
        self.platforms.extend(f.platforms);
        self.kinds.extend(f.kinds);
        for mut s in f.sections {
            s.layer = layer.clone();
            match self.sections.iter_mut().find(|x| x.id == s.id) {
                Some(x) if user => *x = s,
                Some(_) => {
                    self.problems.push(Problem { file: file.clone(), message: format!("section {} twice", s.id) })
                }
                None => self.sections.push(s),
            }
        }
        for (k, s) in f.sources {
            if !user && self.sources.contains_key(&k) && self.sources[&k] != s {
                self.problems
                    .push(Problem { file: file.clone(), message: format!("source {k} differs between files") });
            }
            self.sources.insert(k, s);
        }
        for mut e in f.entries {
            e.layer = layer.clone();
            if e.id.is_empty() {
                self.problems
                    .push(Problem { file: file.clone(), message: format!("an entry has no id: {:?}", e.title) });
                continue;
            }
            let at = self.entries.iter().position(|x| x.id == e.id);
            match at {
                Some(i) if user && e.remove => {
                    self.entries.remove(i);
                    self.hidden.push(e.id);
                }
                Some(i) if user => {
                    self.edited.push(e.id.clone());
                    self.entries[i] = e;
                }
                Some(_) => self.problems.push(Problem { file: file.clone(), message: format!("entry {} twice", e.id) }),
                None if e.remove => {
                    self.problems.push(Problem { file: file.clone(), message: format!("hides unknown entry {}", e.id) })
                }
                None => self.entries.push(e),
            }
        }
    }

    fn validate(&mut self) {
        let mut problems = Vec::new();
        let file_of = |l: &Layer| match l {
            Layer::User(n) => format!("user/{n}"),
            Layer::Pack => "pack".into(),
            Layer::BuiltIn => "built-in".into(),
        };
        for e in &self.entries {
            let mut bad =
                |m: String| problems.push(Problem { file: file_of(&e.layer), message: format!("{}: {m}", e.id) });
            if e.games.is_empty() {
                bad("no game".into());
            }
            for g in &e.games {
                if !self.games.iter().any(|x| &x.id == g) {
                    bad(format!("unknown game {g}"));
                }
            }
            for p in &e.platforms {
                if !self.platforms.contains_key(p) {
                    bad(format!("unknown platform {p}"));
                }
            }
            if !self.kinds.contains_key(&e.kind) {
                bad(format!("unknown kind {:?}", e.kind));
            }
            for s in &e.sources {
                if !self.sources.contains_key(s) {
                    bad(format!("unknown source {s}"));
                }
            }
            if !e.date.is_empty() && !valid_date(&e.date) {
                bad(format!("date {:?} is not YYYY, YYYY-MM or YYYY-MM-DD", e.date));
            }
            if !e.section.is_empty() && !self.sections.iter().any(|s| s.id == e.section) {
                bad(format!("unknown section {}", e.section));
            }
        }
        self.problems.extend(problems);
    }

    pub fn game(&self, id: &str) -> Option<&GameInfo> {
        self.games.iter().find(|g| g.id == id)
    }
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
    pub fn entries_of<'a>(&'a self, game: &'a str) -> impl Iterator<Item = &'a Entry> + 'a {
        self.entries.iter().filter(move |e| e.games.iter().any(|g| g == game))
    }
    pub fn sections_of<'a>(&'a self, game: &'a str) -> impl Iterator<Item = &'a Section> + 'a {
        self.sections.iter().filter(move |s| s.games.iter().any(|g| g == game))
    }
    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }
    pub fn platform_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.platforms.get(id).map_or_else(|| index::index().platform_name(id), |s| s.as_str())
    }
    pub fn kind_label<'a>(&'a self, kind: &'a str) -> &'a str {
        self.kinds.get(kind).map_or(kind, |s| s.as_str())
    }

    /// Release-index dump records whose serial is one of the entry's serials (same platform when
    /// the entry names platforms).
    pub fn dumps_of(&self, e: &Entry) -> Vec<(&'static IndexGame, &'static Release)> {
        if e.serials.is_empty() {
            return Vec::new();
        }
        index::index()
            .releases()
            .filter(|(_, r)| e.platforms.is_empty() || e.platforms.contains(&r.platform))
            .filter(|(_, r)| {
                r.serial.split(',').map(str::trim).any(|s| {
                    e.serials.iter().any(|x| {
                        s.eq_ignore_ascii_case(x)
                            || (x.len() == 10
                                && s.get(..10).is_some_and(|p| p.eq_ignore_ascii_case(x))
                                && s.as_bytes().get(10).is_some_and(|b| !b.is_ascii_digit()))
                    })
                })
            })
            .collect()
    }

    /// Every user file merged into one, for sending additions back.
    pub fn export_user(paths: &Paths, dest: &Path) -> Result<usize, DbError> {
        let dir = paths.user.as_ref().ok_or_else(|| DbError::Invalid("no user folder".into()))?;
        let mut all = DbFile::default();
        for (name, text) in read_dir_toml(dir) {
            let f = DbFile::parse(&text).map_err(|e| DbError::Invalid(format!("{name}: {e}")))?;
            all.games.extend(f.games);
            all.platforms.extend(f.platforms);
            all.kinds.extend(f.kinds);
            all.sections.extend(f.sections);
            all.entries.extend(f.entries);
            all.sources.extend(f.sources);
        }
        let n = all.entries.len();
        write_atomic(dest, all.to_toml()?.as_bytes())?;
        Ok(n)
    }
}

fn valid_date(d: &str) -> bool {
    let b = d.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    match b.len() {
        4 => digits(0..4),
        7 => digits(0..4) && b[4] == b'-' && digits(5..7),
        10 => digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10),
        _ => false,
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DbError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("partial");
    std::fs::File::create(&tmp)?.write_all(bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// --- The loaded database ---------------------------------------------------------------------------

static PATHS: OnceLock<Paths> = OnceLock::new();
static DB: RwLock<Option<Arc<Database>>> = RwLock::new(None);

/// Sets where the pack and user files are read from (before the first [`database`] call);
/// the default is [`Paths::standard`].
pub fn configure(paths: Paths) {
    let _ = PATHS.set(paths);
}

pub fn paths() -> &'static Paths {
    PATHS.get_or_init(Paths::standard)
}

/// The database, loaded on first use.
pub fn database() -> Arc<Database> {
    if let Some(db) = DB.read().unwrap().as_ref() {
        return db.clone();
    }
    reload()
}

/// The database if it has been loaded, without loading it (for a UI that loads it in the background).
pub fn loaded() -> Option<Arc<Database>> {
    DB.read().unwrap().clone()
}

/// Reads every layer again (after an import or an edit).
pub fn reload() -> Arc<Database> {
    let db = Arc::new(Database::load(paths()));
    *DB.write().unwrap() = Some(db.clone());
    db
}

// --- Editing the user layer ------------------------------------------------------------------------

fn edit_user<T>(paths: &Paths, f: impl FnOnce(&mut DbFile) -> T) -> Result<T, DbError> {
    let path = paths.edits()?;
    let mut file = match std::fs::read_to_string(&path) {
        Ok(text) => DbFile::parse(&text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DbFile::default(),
        Err(e) => return Err(e.into()),
    };
    let out = f(&mut file);
    write_atomic(&path, file.to_toml()?.as_bytes())?;
    Ok(out)
}

/// Adds or replaces `entry` (by id) in `user/edits.toml`, with any new sources.
pub fn save_user_entry(paths: &Paths, entry: Entry, new_sources: IndexMap<String, Source>) -> Result<(), DbError> {
    edit_user(paths, |f| {
        let mut entry = entry;
        entry.layer = Layer::default();
        match f.entries.iter_mut().find(|e| e.id == entry.id) {
            Some(e) => *e = entry,
            None => f.entries.push(entry),
        }
        f.sources.extend(new_sources);
    })
}

/// Hides the entry `id` through `user/edits.toml`.
pub fn hide_entry(paths: &Paths, id: &str) -> Result<(), DbError> {
    edit_user(paths, |f| {
        f.entries.retain(|e| e.id != id);
        f.entries.push(Entry { id: id.into(), remove: true, ..Entry::default() });
    })
}

/// Removes `id` from `user/edits.toml`: an edited entry returns to its built-in (or pack) form,
/// a hidden one is shown again, an added one disappears.
pub fn revert_entry(paths: &Paths, id: &str) -> Result<(), DbError> {
    edit_user(paths, |f| f.entries.retain(|e| e.id != id))
}

/// What is wrong with an entry before it is saved: it needs a title, a known kind, known games and
/// platforms, a well-formed date and at least one source (from `db` or `new_sources`).
pub fn check_entry(db: &Database, e: &Entry, new_sources: &IndexMap<String, Source>) -> Result<(), String> {
    if e.id.trim().is_empty() {
        return Err("The entry needs an id.".into());
    }
    if e.title.trim().is_empty() {
        return Err("Give the build or version a title.".into());
    }
    if !db.kinds.contains_key(&e.kind) {
        return Err(format!("Unknown kind {:?}.", e.kind));
    }
    if e.games.is_empty() {
        return Err("Choose at least one game.".into());
    }
    if let Some(g) = e.games.iter().find(|g| db.game(g).is_none()) {
        return Err(format!("Unknown game {g:?}."));
    }
    if let Some(pl) = e.platforms.iter().find(|pl| !db.platforms.contains_key(*pl)) {
        return Err(format!("Unknown platform {pl:?}."));
    }
    if !e.date.is_empty() && !valid_date(&e.date) {
        return Err("The date must be YYYY, YYYY-MM or YYYY-MM-DD; put anything else in the date text.".into());
    }
    if e.sources.is_empty() {
        return Err("Cite at least one source: never add a build without one.".into());
    }
    if let Some(s) = e.sources.iter().find(|s| !db.sources.contains_key(*s) && !new_sources.contains_key(*s)) {
        return Err(format!("Unknown source {s:?}."));
    }
    if let Some((k, _)) = new_sources.iter().find(|(_, s)| s.text.trim().is_empty()) {
        return Err(format!("Source {k} needs a description."));
    }
    Ok(())
}

/// A new source key for the user layer: `user-<n>`.
pub fn new_source_key(db: &Database) -> String {
    (1..).map(|n| format!("user-{n}")).find(|k| !db.sources.contains_key(k)).unwrap()
}

/// A new id for a user entry: `user-<game>-<n>`.
pub fn new_entry_id(db: &Database, game: &str) -> String {
    (1..).map(|n| format!("user-{game}-{n}")).find(|id| db.entry(id).is_none()).unwrap()
}

// --- Importing and packs ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Imported {
    /// A `.toml` copied into the user folder under this name.
    UserFile(String),
    /// A complete database installed as the pack.
    Pack { version: String },
}

/// Checks that a set of files is a complete database this build can read.
fn check_pack(files: &[(String, String)]) -> Result<Header, DbError> {
    let mut problems = Vec::new();
    let parsed = parse_set(files.iter().map(|(n, t)| (n.clone(), t.as_str())), &mut problems);
    if let Some(p) = problems.first() {
        return Err(DbError::Invalid(format!("{}: {}", p.file, p.message)));
    }
    let h = header_of(&parsed).ok_or_else(|| DbError::Invalid("no database.toml with a [database] header".into()))?;
    if h.schema > SCHEMA {
        return Err(DbError::Invalid(format!(
            "database {} needs a newer Eyefinder (schema {}, this build reads {SCHEMA})",
            h.version, h.schema
        )));
    }
    // It must load cleanly on its own.
    static CHECKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ef-dbcheck-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    for (n, t) in files {
        std::fs::write(dir.join(n), t)?;
    }
    let db = Database::load(&Paths { pack: Some(dir.clone()), user: None });
    let _ = std::fs::remove_dir_all(&dir);
    if db.base != Base::Pack {
        return Err(DbError::Invalid(db.notices.join(" ")));
    }
    if let Some(p) = db.problems.first() {
        return Err(DbError::Invalid(format!("{}: {}", p.file, p.message)));
    }
    Ok(h)
}

/// The `.toml` files of a database pack archive (flat names only).
pub fn read_pack_zip(bytes: &[u8]) -> Result<Vec<(String, String)>, DbError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| DbError::Zip(e.to_string()))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| DbError::Zip(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let full = f.name().map_err(|e| DbError::Zip(e.to_string()))?.into_owned();
        let name = full.rsplit('/').next().unwrap_or_default().to_string();
        if !name.ends_with(".toml") || name.contains(['\\', ':']) || name.starts_with('.') {
            continue;
        }
        let mut text = String::new();
        f.read_to_string(&mut text).map_err(|e| DbError::Zip(format!("{name}: {e}")))?;
        out.push((name, text));
    }
    Ok(out)
}

/// Writes `dir`'s `.toml` files as a pack archive; returns its SHA-256 (hex).
pub fn write_pack_zip(dir: &Path, out: &Path) -> Result<String, DbError> {
    use sha2::Digest;
    let files = read_dir_toml(dir);
    check_pack(&files).or_else(|e| match e {
        // Packing the built-in folder itself: it is the built-in version, not newer.
        DbError::Invalid(m) if m.contains("not newer") => Ok(Header::default()),
        e => Err(e),
    })?;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in &files {
            zip.start_file(name.as_str(), opts).map_err(|e| DbError::Zip(e.to_string()))?;
            zip.write_all(text.as_bytes())?;
        }
        zip.finish().map_err(|e| DbError::Zip(e.to_string()))?;
    }
    let bytes = buf.into_inner();
    write_atomic(out, &bytes)?;
    Ok(sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Installs a complete database (the files of a pack) as the pack.
pub fn install_pack(paths: &Paths, files: &[(String, String)]) -> Result<String, DbError> {
    let h = check_pack(files)?;
    let dir = paths.pack.as_ref().ok_or_else(|| DbError::Invalid("no pack folder".into()))?;
    let tmp = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    for (n, t) in files {
        std::fs::write(tmp.join(n), t)?;
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&tmp, dir)?;
    Ok(h.version)
}

/// Imports a `.toml` (into the user folder) or a pack `.zip` (as the pack).
pub fn import(paths: &Paths, file: &Path) -> Result<Imported, DbError> {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "zip" => {
            let files = read_pack_zip(&std::fs::read(file)?)?;
            Ok(Imported::Pack { version: install_pack(paths, &files)? })
        }
        "toml" => {
            let text = std::fs::read_to_string(file)?;
            let parsed = DbFile::parse(&text)?;
            if parsed.database.is_some() {
                return Err(DbError::Invalid(
                    "this file has a [database] header: it is part of a complete database; import the pack .zip instead"
                        .into(),
                ));
            }
            let dir = paths.user.as_ref().ok_or_else(|| DbError::Invalid("no user folder".into()))?;
            std::fs::create_dir_all(dir)?;
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("imported");
            let name = (0..)
                .map(|n| if n == 0 { format!("{stem}.toml") } else { format!("{stem}-{n}.toml") })
                .find(|n| n != "edits.toml" && !dir.join(n).exists())
                .unwrap();
            // It must not break the database it joins.
            let mut check = Database::load(&Paths { pack: paths.pack.clone(), user: None });
            let before = check.problems.len();
            check.apply(&name, parsed, Layer::User(name.clone()), true);
            check.validate();
            if let Some(p) = check.problems.iter().skip(before).find(|p| p.file.contains(&name)) {
                return Err(DbError::Invalid(p.message.clone()));
            }
            write_atomic(&dir.join(&name), text.as_bytes())?;
            Ok(Imported::UserFile(name))
        }
        _ => Err(DbError::Invalid("choose a database .toml file or a pack .zip".into())),
    }
}

// --- Updates from GitHub releases ------------------------------------------------------------------

/// A newer database pack on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub version: String,
    pub tag: String,
    pub zip_url: String,
    pub sha256_url: Option<String>,
}

fn http_get(url: &str) -> Result<Vec<u8>, DbError> {
    let mut resp = ureq::get(url)
        .header("User-Agent", "Eyefinder (https://github.com/galaxygas/Eyefinder)")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| DbError::Net(e.to_string()))?;
    resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| DbError::Net(e.to_string()))
}

/// The newest `db-<version>` release of [`UPDATE_REPO`], if it is newer than `current`.
pub fn check_update(current: &str) -> Result<Option<Available>, DbError> {
    let body = http_get(&format!("https://api.github.com/repos/{UPDATE_REPO}/releases?per_page=100"))?;
    let releases: serde_json::Value = serde_json::from_slice(&body).map_err(|e| DbError::Net(e.to_string()))?;
    Ok(newest_pack(&releases).filter(|a| compare_versions(&a.version, current) == Ordering::Greater))
}

/// The newest database pack in a GitHub releases listing.
pub fn newest_pack(releases: &serde_json::Value) -> Option<Available> {
    let mut best: Option<Available> = None;
    for r in releases.as_array()?.iter() {
        let Some(tag) = r["tag_name"].as_str() else { continue };
        let Some(version) = tag.strip_prefix("db-") else { continue };
        if r["draft"].as_bool() == Some(true) {
            continue;
        }
        let assets = r["assets"].as_array().cloned().unwrap_or_default();
        let url = |suffix: &str| {
            assets
                .iter()
                .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(suffix)))
                .and_then(|a| a["browser_download_url"].as_str().map(String::from))
        };
        let Some(zip_url) = url(".zip") else { continue };
        let a = Available { version: version.into(), tag: tag.into(), zip_url, sha256_url: url(".zip.sha256") };
        if best.as_ref().is_none_or(|b| compare_versions(&a.version, &b.version) == Ordering::Greater) {
            best = Some(a);
        }
    }
    best
}

/// Downloads, checks (SHA-256 when published) and installs a pack.
pub fn download_and_install(paths: &Paths, a: &Available) -> Result<String, DbError> {
    use sha2::Digest;
    let bytes = http_get(&a.zip_url)?;
    if let Some(u) = &a.sha256_url {
        let want = String::from_utf8_lossy(&http_get(u)?).split_whitespace().next().unwrap_or_default().to_lowercase();
        let got: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if want != got {
            return Err(DbError::Invalid(format!("the download's SHA-256 is {got}, the release says {want}")));
        }
    }
    install_pack(paths, &read_pack_zip(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ef-db-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_built_in_database_is_valid() {
        let db = Database::load(&Paths::none());
        assert!(db.problems.is_empty(), "{:#?}", &db.problems[..db.problems.len().min(20)]);
        assert_eq!(db.schema, SCHEMA);
        assert!(db.entries.len() >= 1954, "{}", db.entries.len());
        let mut ids = std::collections::HashSet::new();
        for e in &db.entries {
            assert!(ids.insert(&e.id), "duplicate id {}", e.id);
        }
        // Every Eyefinder game and every release-index game is in the database.
        for g in crate::Game::ALL {
            assert!(db.games.iter().any(|x| x.eyefinder.as_deref() == Some(g.cli_id())), "{g:?}");
        }
        for g in &index::index().games {
            assert!(db.game(&g.id).is_some(), "{}", g.id);
        }
        for p in index::index().platforms.keys() {
            assert!(db.platforms.contains_key(p), "{p}");
        }
    }

    /// Every heading, table row and numbered source of the research file is in the database.
    #[test]
    fn research_md_is_fully_covered() {
        let md = include_str!("../../../docs/research/GTA_Builds_Versions_Database.md");
        let db = Database::load(&Paths::none());
        let origins: std::collections::HashSet<&str> = db.entries.iter().map(|e| e.origin.as_str()).collect();
        let sections: std::collections::HashSet<&str> = db.sections.iter().map(|s| s.origin.as_str()).collect();
        let mut in_table = false;
        let mut sources_of: Option<String> = None;
        let (mut rows, mut heads, mut srcs) = (0, 0, 0);
        for (i, line) in md.lines().enumerate() {
            let origin = format!("md:{}", i + 1);
            if let Some(h) = line.strip_prefix("## ").or(line.strip_prefix("### ")).or(line.strip_prefix("#### ")) {
                in_table = false;
                sources_of = h
                    .strip_prefix("Sources (Section ")
                    .map(|r| r.trim_end_matches(')').to_string())
                    .or(h.starts_with("Sources (Appendix A)").then(|| "A".to_string()));
                if sources_of.is_none() {
                    heads += 1;
                    assert!(sections.contains(origin.as_str()), "heading at line {}: {h}", i + 1);
                }
                continue;
            }
            if let Some(top) = &sources_of {
                let t = line.trim_start();
                let num = t
                    .split_once(". ")
                    .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                    .map(|(n, _)| n.to_string())
                    .or_else(|| {
                        t.strip_prefix("- ")
                            .and_then(|r| r.split_once(" — "))
                            .map(|(n, _)| n.to_string())
                            .filter(|n| n.starts_with('S') && n[1..].starts_with(|c: char| c.is_ascii_digit()))
                    })
                    .or_else(|| t.strip_prefix("- [").and_then(|r| r.split_once("] ")).map(|(n, _)| n.to_string()));
                if let Some(n) = num {
                    srcs += 1;
                    assert!(db.sources.contains_key(&format!("{top}-{n}")), "source {top}-{n} at line {}", i + 1);
                }
                continue;
            }
            if line.starts_with('|') {
                let sep = line
                    .trim()
                    .trim_matches('|')
                    .split('|')
                    .all(|c| c.trim().trim_matches(':').chars().all(|ch| ch == '-'));
                if !in_table {
                    in_table = true; // the header row
                } else if !sep {
                    rows += 1;
                    assert!(origins.contains(origin.as_str()), "table row at line {} is missing", i + 1);
                }
            } else {
                in_table = false;
            }
        }
        assert_eq!((heads, rows), (194, 1954));
        assert!(srcs >= 930, "{srcs}");
    }

    #[test]
    fn user_files_add_replace_and_hide() {
        let root = temp("layers");
        let paths = Paths { pack: Some(root.join("pack")), user: Some(root.join("user")) };
        let base = Database::load(&paths);
        let first = base.entries_of("gta3").next().unwrap().clone();
        let mut changed = first.clone();
        changed.notes = "Corrected by hand.".into();
        save_user_entry(&paths, changed, IndexMap::new()).unwrap();
        let id = new_entry_id(&base, "gta3");
        let mut src = IndexMap::new();
        src.insert(
            "user-1".to_string(),
            Source { text: "A test source".into(), url: "https://example.org".into(), ..Default::default() },
        );
        let added = Entry {
            id: id.clone(),
            kind: "prototype".into(),
            games: vec!["gta3".into()],
            title: "A build found later".into(),
            platforms: vec!["ps2".into()],
            sources: vec!["user-1".into()],
            ..Entry::default()
        };
        save_user_entry(&paths, added, src).unwrap();
        let hidden = base.entries_of("vc").next().unwrap().id.clone();
        hide_entry(&paths, &hidden).unwrap();
        let db = Database::load(&paths);
        assert!(db.problems.is_empty(), "{:?}", db.problems);
        assert_eq!(db.entry(&first.id).unwrap().notes, "Corrected by hand.");
        assert_eq!(db.entry(&first.id).unwrap().layer, Layer::User("edits.toml".into()));
        assert!(db.entry(&id).is_some() && db.entry(&hidden).is_none());
        assert_eq!(db.entries.len(), base.entries.len());
        assert_eq!((db.edited.clone(), db.hidden.clone()), (vec![first.id.clone()], vec![hidden.clone()]));
        // Reverting restores the built-in entry; exporting gives one file another copy can import.
        revert_entry(&paths, &first.id).unwrap();
        assert_eq!(Database::load(&paths).entry(&first.id).unwrap().notes, first.notes);
        let out = root.join("mine.toml");
        assert_eq!(Database::export_user(&paths, &out).unwrap(), 2);
        let other = Paths { pack: None, user: Some(root.join("other-user")) };
        assert_eq!(import(&other, &out).unwrap(), Imported::UserFile("mine.toml".into()));
        let db = Database::load(&other);
        assert!(db.entry(&id).is_some() && db.entry(&hidden).is_none(), "{:?}", db.problems);
        // A broken user file is a problem, not a failure.
        std::fs::write(root.join("user/broken.toml"), "[[entry]\nid = ").unwrap();
        let db = Database::load(&paths);
        assert!(db.problems.iter().any(|p| p.file == "user/broken.toml"));
        assert!(db.entries.len() >= base.entries.len());
        // An import that refers to an unknown game is refused.
        std::fs::write(root.join("bad.toml"), "[[entry]]\nid = \"x\"\nkind = \"retail\"\ngames = [\"nope\"]\n")
            .unwrap();
        assert!(import(&paths, &root.join("bad.toml")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packs_replace_the_built_in_files_only_when_newer_and_readable() {
        let root = temp("packs");
        let built_in_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/database");
        let paths = Paths { pack: Some(root.join("pack")), user: Some(root.join("user")) };
        let mut files = read_dir_toml(&built_in_dir);
        let set_header = |files: &mut Vec<(String, String)>, version: &str, schema: u32| {
            let db = files.iter_mut().find(|(n, _)| n == "database.toml").unwrap();
            let mut f = DbFile::parse(&db.1).unwrap();
            f.database = Some(Header { version: version.into(), schema });
            db.1 = f.to_toml().unwrap();
        };
        // The built-in version itself is not newer.
        assert!(install_pack(&paths, &files).is_err());
        set_header(&mut files, "2999.01.01", SCHEMA);
        let zip = root.join("pack.zip");
        {
            let src = root.join("src");
            std::fs::create_dir_all(&src).unwrap();
            for (n, t) in &files {
                std::fs::write(src.join(n), t).unwrap();
            }
            let sha = write_pack_zip(&src, &zip).unwrap();
            assert_eq!(sha.len(), 64);
        }
        assert_eq!(import(&paths, &zip).unwrap(), Imported::Pack { version: "2999.01.01".into() });
        let db = Database::load(&paths);
        assert_eq!((db.base.clone(), db.version.as_str()), (Base::Pack, "2999.01.01"));
        assert!(db.problems.is_empty(), "{:?}", db.problems);
        // A newer schema is refused on import and ignored when already installed.
        set_header(&mut files, "3000.01.01", SCHEMA + 1);
        assert!(install_pack(&paths, &files).unwrap_err().to_string().contains("newer Eyefinder"));
        for (n, t) in &files {
            std::fs::write(root.join("pack").join(n), t).unwrap();
        }
        let db = Database::load(&paths);
        assert_eq!(db.base, Base::BuiltIn);
        assert!(db.notices.iter().any(|n| n.contains("newer Eyefinder")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_newest_pack_release_is_found() {
        let releases = serde_json::json!([
            {"tag_name": "v0.3.0", "assets": [{"name": "Eyefinder-v0.3.0-windows-x64.zip", "browser_download_url": "a"}]},
            {"tag_name": "db-2026.10.24", "assets": [
                {"name": "eyefinder-database-2026.10.24.zip", "browser_download_url": "z1"},
                {"name": "eyefinder-database-2026.10.24.zip.sha256", "browser_download_url": "s1"}]},
            {"tag_name": "db-2026.11.2", "assets": [{"name": "eyefinder-database-2026.11.2.zip", "browser_download_url": "z2"}]},
            {"tag_name": "db-2027.01.01", "draft": true, "assets": [{"name": "x.zip", "browser_download_url": "z3"}]}
        ]);
        let a = newest_pack(&releases).unwrap();
        assert_eq!((a.version.as_str(), a.zip_url.as_str(), a.sha256_url), ("2026.11.2", "z2", None));
        assert_eq!(compare_versions("2026.10.24", "2026.9.30"), Ordering::Greater);
        assert_eq!(compare_versions("2026.10.24", "2026.10.24.1"), Ordering::Less);
    }

    #[test]
    fn entries_link_to_dump_records_by_serial() {
        let db = Database::load(&Paths::none());
        let e = db
            .entries
            .iter()
            .find(|e| e.serials.contains(&"SLUS-20062".to_string()) && e.platforms == ["ps2"])
            .unwrap();
        let dumps = db.dumps_of(e);
        assert!(!dumps.is_empty() && dumps.iter().all(|(g, r)| g.id == "gta3" && r.platform == "ps2"), "{dumps:?}");
    }
}
