# Contributing to Eyefinder

Thank you for helping. Eyefinder aims to be exact, so contributions are judged first on evidence.

## Reporting a format result

Results from real game copies are the most valuable contribution: they move formats from
"Implemented from spec" to "Verified".

1. Add your game folder or disc image in Eyefinder and export (everything, or one category).
2. Open an issue with the **Format report** template and attach the export's `manifest.json`. It contains no audio, only names, sizes, formats and durations.
3. Say which version you have (retail, Steam, Rockstar Launcher, PS2 region and serial from `SYSTEM.CNF`, ...).
4. Describe anything that sounds wrong: noise, wrong speed, cut-off endings, wrong names.

Never upload game files.

## Code contributions

See [AGENTS.md](AGENTS.md) for the rules (they apply to people too):
* branch and pull request,
* cited sources,
* tests against an independent reference,
* `cargo fmt`, `clippy -D warnings` and `cargo test` all passing.

To rebuild the generated tables, you need local checkouts of the re3 (`master`) and reVC (`miami`) sources:

```sh
python3 tools/gen_tables.py <re3>/src <reVC>/src crates/ef-audio/src/tables
```

To rebuild the test fixtures (needs ffmpeg with libmp3lame/libvorbis, and genisoimage):

```sh
python3 tools/fixtures/make_fixtures.py crates/ef-audio/tests/fixtures
```

To rebuild the texture fixtures and their reference (needs git, cmake, a C++ compiler, Python 3 and
Pillow; librw is fetched at the pinned commit):

```sh
tools/txd-ref/build.sh /tmp/txdref crates/ef-texture/tests/fixtures
```

The texture fixtures are deterministic: CI regenerates them and checks they are unchanged.

Regenerating the audio fixtures changes their contents, so `reference.json` must then be recreated with
the reference tools named in its `_provenance` section.

## License

By contributing you agree that your contribution is licensed under GPL-3.0-or-later.
