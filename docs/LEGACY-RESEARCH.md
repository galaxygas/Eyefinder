# Earlier GTA formats: evidence and remaining work

Research for `agent/ps2-legacy-support`, 2026-10-21. This is an implementation
assessment, **not a claim that new game support has shipped**. The current game
importer still supports GTA III, Vice City and San Andreas only.

## Primary sources examined

* DMA Design, **CityScape Data Structure 12.10**, 1998-02-03:
  <https://misc.daniel-marschall.de/spiele/gta1/cds.pdf>.
  Sections 3.6–3.8 describe GTA1 GRY/G24 style files, palettes, tile pages and sprites.
  There is a version inconsistency: section 3.6 says GRY 325, section 3.7 says 305.
  A decoder must resolve that discrepancy rather than silently treating both alike.
* DMA Design, **GBH Style Format 4.6**, 1999-11-16, preserved at
  <https://gtamp.com/gta2/gta2-style-sty-graphics-file-format/>.
  Documents `GBST`, named chunks, palette indirection, tile/sprite pages and deltas.
  Its header section says version 600. Its palette/page counts contain apparent
  inconsistencies; validate actual offsets and versions against an independent implementation.
* Daniel Marschall's firsthand GTA1/London analyses:
  <https://misc.daniel-marschall.de/spiele/gta1/format_sdt.html>,
  <https://misc.daniel-marschall.de/spiele/gta1/format_city.html>,
  <https://misc.daniel-marschall.de/spiele/gta1/files/index.html>.
  The SDT index has three little-endian 32-bit fields. Channel count and bit depth
  are not in it: LEVEL000 uses 16-bit PCM and its first three entries are stereo;
  the remaining entries are mono. Other documented banks use 8-bit mono PCM.
  Therefore the GTA III/VC 20-byte SDT reader cannot be reused for these files.
* OpenGTA at `1ae34ae3409d37bf484364d2a1db2e183a8527bd`:
  <https://github.com/madebr/OpenGTA/tree/1ae34ae3409d37bf484364d2a1db2e183a8527bd>.
  Examined `read_sdt.cpp`, `util/sound_fx_cache.cpp`, and its documentation references.
  This is a useful comparison implementation, not proof of every platform/release.
  Its sound cache distinguishes LEVEL000 bit depth but does not establish the
  three-stereo-entry rule by itself. No source code was copied into Eyefinder.
* vgmstream at `f7f764155b227f683adbab25402fe8f2be0b2ec2`:
  <https://github.com/vgmstream/vgmstream/blob/f7f764155b227f683adbab25402fe8f2be0b2ec2/doc/TXTH.md>.
  The SA PS2 example assumes tracks have **already been extracted** from bigfiles.
  It describes separate type-2/type-4 layouts; it is not a complete pack-index
  specification. Do not apply the PC Ogg/XOR reader to PS2 packs.
  `src/meta/str_sega.c` also explicitly identifies GTA2 Dreamcast as a user of
  a Sega stream variant; this is separate from GTA1/III SDT banks.

## Implementation boundaries

| Requested area | Current result | What is still needed |
|---|---|---|
| III/VC PS2 VB | Existing decoder and reference/seek tests retained; VC manifest observations recorded in SUPPORT.md | Any audible defect needs an identified affected file and reproducible output comparison |
| III/VC PS2 SFX, mission audio, PSS movies | Not implemented | Bank/container boundaries, sample metadata and independent fixtures; a successful VB export does not supply those facts |
| SA PS2 audio | Not implemented | Pack lookup/extraction specification plus separate type-2/type-4 reference tests |
| GTA1 and London PC audio | Research only | Dedicated 12-byte SDT parser, bank-specific PCM metadata, game/expansion identification, WAV and radio-source inventory, seek and independent-reference tests |
| GTA1 and London PC graphics | Research only | GRY/G24 parsers, tile paging, palettes, sprites/deltas/remaps and loose graphics; independent fixtures and preview/export integration |
| GTA2 PC audio and graphics | Research only | Audio bank specification; STY parser and validated version handling; palettes, sprites/deltas/remaps and reference fixtures |
| Earlier console/handheld editions, including Advance | No new support | Platform-specific asset discovery and format specifications; PC layouts must not be assumed to apply |

## Integration requirements

Earlier games need real `Game` identities and evidence-based import choices in
`ef-core`, not aliases for GTA III. Their graphics must not be labelled RenderWare
or exposed as synthetic TXD files: `TexCatalog::load` currently returns `Txd`, and
the preview/export pipeline expects RenderWare rasters. Add a container-aware
abstraction before integrating style files. Keep parsing/decoding on workers.

Generate synthetic data only. For each implemented audio path, compare with an
independent reference and use `assert_seek_exact`; for graphics, extend the
fixture/reference generator and check pixels, palette selection, transparency,
page boundaries, malformed/truncated input, and export/preview consistency.
Keep new implementations at `FromSpec`; unconfirmed layouts remain `DetectedOnly`.

## Branch testing

Issue #1's regression test exercises both asset tables, both drag directions and
stationary pointer frames. It fails with the original flexible-title layout and
passes with the flexible column last. Headings are centered in their cells.

The `Test build` workflow packages the Windows GUI and CLI only after formatting,
workspace Clippy and workspace tests pass. It records the exact commit in
`COMMIT.txt`. It uploads a test artifact and does not create a public release.
