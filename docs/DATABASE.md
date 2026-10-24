# The build database

Eyefinder's **Index** page lists every documented build, version, patch, title update, edition,
port, demo, prototype and leak of every Grand Theft Auto game, with the sources for each. That list
is the **build database**: plain TOML files in
[`crates/ef-core/data/database/`](../crates/ef-core/data/database/) that anyone can read and correct.

The first version was imported from the research file
[docs/research/GTA_Builds_Versions_Database.md](research/GTA_Builds_Versions_Database.md). Its 1,954
table rows, 194 sections and 935 sources are all in the database. A test (`research_md_is_fully_covered`)
fails if one goes missing. From now on, **the TOML files are the source of truth**. The research
file is kept as provenance and is not re-imported.

Next to it, the Index page shows the **dump records** of the release index
(`crates/ef-core/data/release_index.json`, generated from redump, No-Intro and titledb). Eyefinder
uses them to identify the copy you add. An entry whose serial matches a dump record links to it.

## The rules

1. **Every entry cites a source.** The source may be a publisher or developer page, patch notes, a
   dump database (redump, No-Intro), a platform database, the press, or a preservation archive such as
   Hidden Palace. Community wikis count as well, marked with lower confidence.
2. **Nothing is estimated.** When a source gives no value, leave the field empty or write
   `not documented`. When sources disagree, give both values in the text field and set `confidence`
   to `single-source` or `mixed`.
3. **Normalised fields only when unambiguous.** `date` holds one date that a source states. Write
   ranges, conflicts and comments in `date_text`. `serials` and `version` work the same way next to
   `id_text`.
4. **Ids never change.** Other files (and users' own edits) refer to an entry by its `id`.

## Files

| File | Holds |
|---|---|
| `database.toml` | `[database]` (version, schema), the games (`[[game]]`), platform names (`[platforms]`), entry kinds (`[kinds]`) |
| `gta1.toml`, `gta3.toml`, `gta5.toml`, ... | One game each: its `[[section]]`s, `[[entry]]`s and `[sources]` |
| `collections.toml`, `cancelled.toml`, `media.toml`, `regional.toml`, `series.toml`, `audit.toml`, `dumps.toml` | Compilations; cancelled projects; related media; country-specific releases and ratings; series-wide notes; the research audit; preserved-dump listings |

An entry's file only matters for keeping things tidy. The `games` field decides which game cards the
entry appears on.

## An entry

```toml
[[entry]]
id = "3.3-4"                   # unique and stable; new entries: any unique id, e.g. "gta3-ps2-v2"
origin = "md:475"              # imported entries: the line of the research file
section = "3.3"                # the [[section]] it belongs to (optional)
kind = "retail"                # a key of [kinds] in database.toml
games = ["gta3"]               # keys of [[game]]
title = "Revised disc v1.60"
platforms = ["ps2"]            # keys of [platforms]; empty = all platforms or none named
platform_text = "PS2"          # as the source writes it
region = "PAL (Europe, Australia)"
                               # date = "YYYY-MM-DD" (or YYYY-MM, YYYY) only when a source gives one
                               # release date; here none does, so only the text is kept:
date_text = "not documented (EXE date 2001-10-07)"
version = "1.60"
serials = ["SLES-50330"]
id_text = "SLES-50330 and variants ...; disc VER 1.60; CRC 1582E336"
notes = "Redump editions: Original, Platinum, Double Pack ..."
confidence = "mixed"           # confirmed | single-source | unverified | mixed
confidence_text = "✅ (version/editions) / ⚠️ (change list)"
sources = ["3-16", "3-17", "3-47"]   # keys of [sources]
sources_text = "[16][17][47]"

[entry.fields]                 # any other column, verbatim
"Detection signature (address → value)" = "0x5C1E70 → 0x53E58955"
```

A source:

```toml
[sources]
"3-47" = { text = "redump.org, GTA III PS2 v1.60 disc: http://redump.org/disc/...", url = "http://redump.org/disc/..." }
```

Kinds are the groups the Index page shows, in this order:
* retail, reissue, patch, title-update, exe-build, digital, mobile-version;
* edition, compilation, dlc, app;
* demo, prototype, leak, milestone, cancelled;
* regional, rating, languages;
* dump, third-party, media, note, gap.

## Adding or correcting entries

**In the app:** on the Index page, use **Add entry** on a game, or open an entry and click **Edit**
or **Hide**. Your changes go to your own file, `user/edits.toml`, in Eyefinder's database folder
(**Database → Open the database folder**). They survive Eyefinder and database updates. **Undo my
change** puts an entry back to its built-in form.

**By hand:** put any `.toml` file with `[[entry]]` items (and their `[sources]`) in the `user` folder.
Then use **Database → Reload**, or restart:
* an `id` the database already has **replaces** that entry;
* `remove = true` with an existing `id` **hides** it;
* a new `id` **adds** an entry.

User files can also add `[[game]]`, `[platforms]`, `[kinds]` and `[[section]]` items.

**Sending them back:** **Database → Export my additions** saves everything you changed as one `.toml`.
Open a pull request that moves the entries into the right game file, or attach the file to an issue.

Problems, such as an unknown game or a source key that does not exist, are listed at the top of the
Index page. They never stop Eyefinder, and the rest of the database still loads.

## Updating without a new Eyefinder build

| How | What happens |
|---|---|
| **Database → Check for a database update** | Looks for the newest `db-<version>` release on GitHub (only when you click it), checks the SHA-256 and installs it as the **pack** |
| **Database → Import database file** | A pack `.zip` (from any release) becomes the pack; a `.toml` is added to your own files |
| `eyefinder-cli db update` / `db import <file>` | The same, from the command line |

A pack replaces the built-in files only when its version is newer and its schema is one this build
reads (`schema = 1`). An older or unreadable pack is ignored, with a notice on the Index page. Your own
files apply on top of whichever database is in use.

The folders are `pack` and `user`, inside:
* Windows: `%APPDATA%\eyefinder\database\`
* Linux: `~/.local/share/eyefinder/database/`
* macOS: `~/Library/Application Support/eyefinder/database/`

## Publishing a new database (maintainer)

1. Edit the files in `crates/ef-core/data/database/` and raise `version` in `database.toml`
   (`YYYY.MM.DD`, add `.N` for a second release on one day).
2. `cargo run -p eyefinder-cli -- db validate crates/ef-core/data/database` must report 0 problems,
   and `cargo test -p ef-core database` must pass.
3. Merge to `main`. The **Database** workflow (`.github/workflows/database.yml`) validates the folder,
   packs it (`eyefinder-cli db pack`), and publishes release `db-<version>` with the `.zip` and its
   `.sha256`. It does not mark that release as Latest.

Every app release also carries the database pack of its build. A change to `schema` (fields that
older builds cannot read) needs a new Eyefinder release first.

## Command line

```sh
eyefinder-cli db info                                 # version, layers, entries per game, problems
eyefinder-cli db validate [folder]                    # 0 problems or exit code 1
eyefinder-cli db export my-additions.toml
eyefinder-cli db import my-additions.toml             # or a pack .zip
eyefinder-cli db pack crates/ef-core/data/database eyefinder-database-2026.10.24.zip
eyefinder-cli db update
```
