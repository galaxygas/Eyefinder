# Working on Eyefinder (guide for AI agents and contributors)

Eyefinder is a desktop viewer and extractor for the assets of GTA III, Vice City and San Andreas
(the original games and their ports, not the Definitive Edition). It is written in Rust and
licensed under GPL-3.0-or-later.

## Ground rules

1. **Never push to `main`.** Work on a branch named `agent/<short-topic>` (people: any branch name),
   and open a pull request. The maintainer reviews and merges every change.
2. **Never guess a file format.** Every layout, constant or table must cite a source in the code
   comment and in [docs/FORMATS.md](docs/FORMATS.md):
   * reverse-engineered game source (re3, reVC, gta-reversed),
   * vgmstream,
   * an official specification,
   * or a real-file observation recorded in [docs/SUPPORT.md](docs/SUPPORT.md).

   If a detail is not documented, leave the data at `SupportLevel::DetectedOnly` and explain why.
3. **Support levels are earned.**
   * New code starts at `SupportLevel::FromSpec`.
   * Only the maintainer raises something to `Verified`, after a real-file report.
   * Never lower the bar to make something look supported.
4. **No game data in the repository.** Fixtures are synthetic and produced by
   `tools/fixtures/make_fixtures.py`. Do not copy code from re3 / reVC / gta-reversed (they have no
   license). Take facts (names, layouts) only, and generate tables with `tools/gen_tables.py`.
5. **Prove correctness with tests.**
   * A new decoder needs fixture coverage in `crates/ef-audio/tests/` and a comparison with an
     independent reference (ffmpeg, vgmstream, or the original C tool).
   * It must also pass the seek-exactness check (`assert_seek_exact`).
6. **Keep the UI responsive.** Decoding, loading and exporting run on worker threads, never on the UI thread.

## Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass. CI runs them on Windows, macOS and Linux. On Linux the GUI needs:
`libasound2-dev libgtk-3-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev pkg-config`.

## Layout

| Path | What it is |
|---|---|
| `crates/ef-core` | Game detection, virtual file systems (folders and ISO 9660 images), GXT text, support levels |
| `crates/ef-audio` | Audio codecs, the asset catalog, decoding (`open`), export with manifests |
| `crates/ef-audio/src/tables` | Generated stream / sample tables (do not edit by hand) |
| `apps/eyefinder` | The egui desktop application |
| `apps/eyefinder-cli` | Headless `info` / `list` / `export` |
| `tools/legacy-c` | The original, real-file-verified GTA III ripper; reference implementation for tests |
| `tools/fixtures` | Fixture generator (needs ffmpeg and genisoimage) |
| `docs/` | Support matrix and format specifications |

## Adding a new asset type (textures, models, ...)

Create a new crate next to `ef-audio` (for example `ef-texture`) with the same shape:
* a catalog that turns a `GameInstall` into a list of assets, each carrying a `SupportLevel`,
* an `open` function for previews,
* an exporter that writes standard files plus a manifest.

Then add a browser section to the GUI. Keep format knowledge out of the GUI crate.

## Commit and PR style

* Small, focused commits with a clear message (what and why).
* PR description: what changed, the sources used for any format detail, and how it was tested.
