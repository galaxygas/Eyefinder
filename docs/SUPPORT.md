# Support matrix

Every asset in Eyefinder carries one of three support levels, shown in the app next to each item:

| Level | Meaning |
|---|---|
| **Verified** | Decoded output has been checked against **real game files**. |
| **Implemented from spec** | Implemented from documented reverse-engineering sources and tested against synthetic files built from that documentation. It has **not yet been confirmed on real game files**. |
| **Not supported yet** | Eyefinder recognises the data but has no trustworthy specification for it, so it does not decode it. Nothing is guessed. |

A format moves to **Verified** only when someone runs Eyefinder on a real copy and the result is confirmed (see [CONTRIBUTING.md](../CONTRIBUTING.md#reporting-a-format-result)). Each such confirmation is recorded below.

The **Documentation** page of the app shows, per game, every file type with its status
(verified, implemented but unverified, documented but not implemented, not supported, undocumented),
its sources and its real-file confirmations. Its data is `crates/ef-core/data/formats.json`, from
which [MISSING.md](../MISSING.md) is generated. The **Index** page lists every documented build and
version of every GTA game ([the build database](DATABASE.md)), next to the dump records Eyefinder
identifies copies by (`crates/ef-core/data/release_index.json`). Its support badges are per platform:
a level applies only to the platform it was earned on.

## The 2D games (new, unverified implementations)

Every release of GTA1, London 1969, London 1961, GTA2 and GTA Advance is **identified**. The
**decoded** data is listed per release below. Everything without a public specification is listed in
[MISSING.md](../MISSING.md).

| Release | Identified by | Implemented from specification | Not supported yet |
|---|---|---|---|
| GTA1 PC (DOS/Windows retail, Rockstar Classics) | Retail inventory files (maps, text, styles, banks); retail discs by redump track sizes and data-track SHA-1; executables by file version or real-file SHA-1 | Six SDT/RAW bank types, cutscene speech and music WAVs, CD audio tracks (BIN/CUE), GRY 325 / G24 336 tiles, variants, sprites and stored deltas, FON images, FXT text | Sprite remaps and combined delta states; RAT/RAW/ACT images, REP (MISSING.md); CMP maps; Smacker movie |
| London 1969 PC | `GTADATA/UK` map, text, mission list | As GTA1 (UK subtree), CD audio tracks | As GTA1 |
| London 1961 PC | `GTADATA/UK` `uke.cmp`, `enguke.fxt`, `missuke.ini` | As GTA1 (it plays London 1969's banks) | As GTA1 |
| GTA2 PC (retail, freeware 9.6) | `gta2.exe`, `data/fstyle.sty`, `data/e.gxt`, `data/nyc.gci`; the 2004 re-release `gta2.exe` by SHA-1; discs by redump tracks | 24-byte SDT/RAW banks (signed 16-bit; police radio slots unsigned 8-bit), voice clips, GTAUDIO music WAVs, STY 700 tiles, sprites, stored deltas, frontend TGA images, GXT text (E/F/G/I/S) | Remaps, combined deltas; GMP maps; compiled SCR, REP, Russian/Japanese text, kanji.dat (MISSING.md); Bink movies |
| GTA1 / London 1969 / GTA2 PlayStation (BIN/CUE, ISO, CDI, POPStarter VCD) | `SYSTEM.CNF` serial (redump) | CD audio tracks (GTA1, London) | Everything on the data track (MISSING.md) |
| GTA2 Dreamcast (GDI, CDI) | IP.BIN product number, version and date (redump disc pages) | Single-track 4-bit STR streams (AICA ADPCM); STY styles and GXT text in the PC formats | Other STR variants; SDT/RAW banks (4-bit codec undocumented), PVR textures, per-level DSG/SPX/… files, maps (GMP: documented, not rendered), SFD movie (MISSING.md) |
| GTA1 / GTA2 Game Boy Color | Cartridge header, No-Intro SHA-1 | — | Everything (MISSING.md) |
| GTA Advance (Game Boy Advance) | Cartridge header (BGTE/BGTP), No-Intro SHA-1 | — | Everything (MISSING.md) |

These additions have synthetic reference tests, **not real-file confirmation**. Sources and exact
limits are recorded in [FORMATS.md](FORMATS.md).
* A folder holding GTA1 together with the London packs offers each game, and each becomes its own library entry.
* A loose GTA1/London file needs a choice, because its format is shared.
* A console disc or ROM missing from the table can be added only by explicit choice.

## Audio

| Game | Platform | Data | Level | Specification source | Real-file confirmation |
|---|---|---|---|---|---|
| GTA III | PC | Radio, police, ambience, announcements, cutscenes, mission-complete/game-complete music, mission audio (`AUDIO\*.WAV`, `*.MP3`) | **Verified** | re3 `src/audio/sampman.h` (`StreamedNameTable`), `audio_enums.h` | The original GTA III Music Ripper `.asi` (now `tools/legacy-c`) worked on the project owner's PC copy, 2026-10. The Rust implementation is checked against that tool on every CI run |
| GTA III | PC | SFX bank (`SFX.SDT` + `SFX.RAW`) | **Verified** | re3 `sampman.h` `tSample`, `AudioSamples.h` | as above |
| GTA III | PC | Intro movie audio (`movies\*.mpg`, MPEG-1/2 program streams) | **Verified** | ISO/IEC 11172-1 / 13818-1 | as above |
| GTA III | PS2 | Streams (`AUDIO\MUSIC\*.VB`, cutscene `.VB`) | Implemented from spec | re3 `PS2StreamedNameTable`, `oal/stream.cpp` `CVbFile`, `IsThisTrackAt16KHz`; PS-ADPCM per vgmstream `psx_decoder.c` | pending |
| GTA III | PS2 | SFX bank, mission audio, movies | Not supported yet | PS2 layouts not yet confirmed | — |
| Vice City | PC | Radio (`AUDIO\*.ADF`, MP3 XOR 0x22) and all other streams (`*.MP3`, `*.WAV`) | Implemented from spec | reVC `sampman.h`, `sampman_miles.cpp` (`vfs_read_callback`) | pending |
| Vice City | PC | SFX bank (`SFX.SDT` + `SFX.RAW`) | Implemented from spec | reVC `sampman.h` `tSample`, `AudioSamples.h` | pending |
| Vice City | PS2 | Streams (`AUDIO\MUSIC\*.VB`, `AUDIO\CUTSCENE\*\*.VB`) | Implemented from spec | reVC PS2 table, `IsThisTrackAt16KHz` | pending |
| San Andreas | PC | Stream packs (`AUDIO\STREAMS\*`, per-track Ogg Vorbis + beat data) | Implemented from spec | gta-reversed `Audio/Loaders/AEMP3TrackLoader`, `AETrackLoader.h`, `AEStreamTransformer`, `AEDataStream` | pending |
| San Andreas | PC | SFX banks (`AUDIO\SFX\*`) | Implemented from spec | gta-reversed `Audio/Loaders/AEBankLoader.h`, `AEMP3BankLoader.cpp` | pending |
| San Andreas | PS2 | All audio | Not supported yet | Only partial notes exist (vgmstream `doc/TXTH.md`) | — |
| GTA1, London 1969 | PC, PlayStation | CD audio tracks of a BIN/CUE image | Implemented from spec | libcdio bincue (INDEX), Red Book PCM, bchunk byte order | Maintainer's GTA1 disc, 2026-10-23: all 10 tracks equal ffmpeg's `s16le` decode (below) |
| GTA1, London | PC | SDT/RAW banks, WAV speech and music | Implemented from spec | Marschall SDT analysis, OpenGTA | pending |
| GTA2 | PC | SDT/RAW banks (slots 69-136 unsigned 8-bit), voice and GTAUDIO music WAVs | Implemented from spec | gta2_re `cSampleManager`, `sound_obj.cpp`, `file.cpp`; Miles `mss.h` | Maintainer's 2004 release, 2026-10-23: all 960 level-bank slots decode; the radio slots equal ffmpeg's `u8` reading (below). Awaiting the maintainer's listening check |
| GTA2 | Dreamcast | Single-track 4-bit `.STR` streams | Implemented from spec | vgmstream `str_sega.c`, `yamaha_decoder.c` | Maintainer's disc, 2026-10-23: all 16 streams bit-identical to vgmstream-cli (below). Awaiting the maintainer's listening check |
| GTA1, London, GTA2 | PlayStation, Dreamcast (other data), GBC, GBA | All other audio | Not supported yet | No specification (MISSING.md) | — |
| GTA1, GTA2 | PC | Smacker / Bink movies | Not supported yet | Documented (FFmpeg decoders) but not implemented | — |
| All | Xbox, mobile, Mac | All audio | Not supported yet | No documented specification | — |

## Textures

| Game | Platform | Data | Level | Specification source | Real-file confirmation |
|---|---|---|---|---|---|
| All three | PC | Texture dictionaries (`.txd`) in the game folder and in IMG archives (v1 `.img`+`.dir`, v2 `VER2`); Direct3D 8 / 9 natives: ARGB1555, RGB565, ARGB4444, L8, ARGB8888, RGB888, RGB555, 8- and 4-bit palettes, DXT1/3/5, all mip levels | Implemented from spec | librw `d3d8.cpp`, `d3d9.cpp`, `d3d.cpp`; re3 / gta-reversed `Directory.h`, `CdStream.h`; Khronos S3TC | pending |
| All three | PC | DXT2 / DXT4 (premultiplied alpha), Direct3D 9 cube textures | Not supported yet | librw does not convert DXT2/DXT4 to an image (`d3d.cpp` `rasterToImage`) and refuses cube textures (`d3d9.cpp`), so there is no reference to check against | — |
| All three | PS2 | PS2 natives: 8- and 4-bit palettes of 32-bit colours, 32-bit and 16-bit colour, swizzled or not, all documented mip levels | Implemented from spec | librw `ps2raster.cpp` | pending |
| All three | PS2 | PS2 natives with 16-bit palettes, 24-bit colour | Not supported yet | librw does not convert them, so the layout is unconfirmed | — |
| All three | Xbox | Xbox natives: ARGB1555, RGB555, ARGB8888, palettes, DXT1/3/5 | Implemented from spec | librw `xbox.cpp` | pending |
| All three | Xbox | Other Xbox pixel formats; LZO-compressed IMG entries | Not supported yet | no reference decoder / only wiki notes | — |

| GTA1, London | PC | GRY 325 / G24 336 styles, FON images | Implemented from spec | DMA CityScape 12.10, OpenGTA, Marschall FON | pending |
| GTA2 | PC, Dreamcast | STY 700 styles; frontend TGA images (PC) | Implemented from spec | DMA GBH Style 4.6, gta2_re; Truevision TGA 2.0 | The Dreamcast styles use the PC chunks (below) |
| 2D games | PlayStation, GBC, GBA; Dreamcast PVR and per-level files | All other graphics | Not supported yet | No specification (MISSING.md) | — |

Textures carry the support level of their own native format, whatever the game's platform.

## Text

| Game | Platform | Data | Level | Specification source |
|---|---|---|---|---|
| GTA1, London | PC | `.FXT` (Windows-1252; Japanese shown as Shift-JIS codes) | Implemented from spec | Marschall FXT specification and converter |
| GTA2 | PC, Dreamcast | `.GXT` (E, F, G, I, S); the string prefix codes 0x2153-0x217A are shown as marked codes | Implemented from spec | gta2_re `text_0x14`, Marschall GXT notes |
| GTA2 | PC | `.GXT` Russian / Japanese characters above 0x7F | Not supported yet | Mapping not reliably documented (MISSING.md) |

Text is shown in the file view and printed by `eyefinder-cli text`.

## Maps

"Export Map files" (the game header) and `eyefinder-cli maps`. Layouts and sources: [FORMATS.md](FORMATS.md#maps-ef_map).

| Game | Platform | Export | Level | Specification source |
|---|---|---|---|---|
| GTA1, London 1969, London 1961 | PC | Raw `.CMP` and styles; `map.json` (blocks, columns, objects, cars, routes, bases, navigation zones); `overlay.svg`; full top-down map per style (64 px per block) and tiles | Implemented from spec | DMA CityScape 12.10 §2; OpenGTA (column order, lid rotation) |
| GTA2 | PC, Dreamcast | Raw `.gmp`, styles and stage list; `map.json` (blocks, columns, zones, objects, lights, animations); `overlay.svg`; full top-down map (64 px per block) and tiles | Implemented from spec | DMA GBH Map Format 8.2; gta2_re (lid orientation and shapes, stage list) |
| GTA III, Vice City, San Andreas | PC, PS2, Xbox | Raw map files (level files, IDE, IPL, COL, map models and textures, radar tiles); radar map and its tiles; pause-menu map (Vice City, San Andreas) | Implemented from spec | re3 / reVC, gta-reversed (Radar, Frontend, FileLoader, Streaming); textures as above |
| GTA III, Vice City, San Andreas | PC | 3D map scene (glTF 2.0: every placed model with its textures, by area and detail level); top-down render of the placed world (0.5–4 px per unit); `world.json` (models, instances, matrices) | Implemented from spec | re3 / reVC / gta-reversed FileLoader, IplStore, Matrix; librw (DFF) |
| GTA III, Vice City, San Andreas | PS2, Xbox | 3D map scene and top-down render | Not supported yet | Native (console) geometry is not read yet; `world.json` and the raw files are written |
| GTA1, London 1969, GTA2 | PlayStation; GBC; GTA Advance (GBA) | Everything | Not supported yet | No specification (MISSING.md) |

## Game versions

| Platform | Identified by | Source |
|---|---|---|
| PC (III, VC, SA) | One value in each game executable (1.0, 1.1 / 1.01, 3.0, the Steam updates, the Rockstar Games Launcher build); every executable present is reported, with its file version | plugin-sdk `GameVersion.cpp`, SilentPatch `MemoryMgr.GTA.h` |
| PC (GTA1, London, GTA2) | Executable file version; executables from real-file reports by size and SHA-1 | Microsoft `VS_FIXEDFILEINFO`; real-file reports below |
| PC discs (BIN/CUE) | Track sizes and data-track SHA-1 against redump's IBM PC DAT | redump.org PC DAT 2026-06-15 (release index) |
| PC store | Steam app manifest, Rockstar Games Launcher uninstall registration, retail setup registry | Galaxy-Plugin-Rockstar, Steam |
| PS2 | The disc serial in `SYSTEM.CNF` | PCSX2 `GameIndex.yaml` |
| PlayStation | The disc serial in `SYSTEM.CNF` (`BOOT`) | psx-spx, redump |
| Dreamcast | IP.BIN product number, version and date | makeip; redump disc pages 19717, 61592, 22119 |
| Game Boy Color / Advance | Cartridge header and SHA-1 | Pan Docs, GBATEK, No-Intro |
| Xbox, Mac | Not identified | — |

The GTA III and Vice City Steam and Rockstar Games Launcher executables carry the same build value,
so for them the store comes from the install location (issue #3).

Users can choose the game, platform and version when adding a copy. The version is recorded in every export manifest and never changes how data is decoded.

Games are recognised by files that the game itself opens (see `MARKERS` in `crates/ef-core/src/lib.rs`). PS2 and Xbox discs are told apart by their boot files (`SYSTEM.CNF`, `default.xbe`).

## How the implementations were checked

These checks run in CI on every change (`crates/ef-audio/tests/fixtures.rs`, `crates/ef-texture/tests/fixtures.rs`):

* **GTA III PC:** all 3,047 PCM/ADPCM/SFX outputs from the fixture game are SHA-256 identical to the original C ripper (`tools/legacy-c`). That ripper was itself confirmed on real files. MPEG outputs have identical frame counts and match within 2×10⁻⁷.
* **PS2 `.VB` read from an ISO image:** identical to **vgmstream** (`vgmstream-cli`, commit `f7f7641`). This includes random-data streams that exercise every filter, shift and flag value.
* **Vice City ADF:** decoding the `.ADF` equals decoding the de-obfuscated MP3.
* **San Andreas:** stream tracks read through the XOR key and track header equal the original Ogg files. SFX sounds equal their PCM bytes.
* **Seeking:** for every decoder, seeking to any frame yields exactly the samples of a decode from the start.
* **Textures:** 121 synthetic textures cover every format, palette and mip combination for Direct3D 8 and 9, Xbox and PS2 (RenderWare 3.1, 3.3, 3.4, 3.5 and 3.6 streams). The checks are:
  * Every level's bytes, and every PS2 level's unswizzled texels, equal what **librw** reads. The PS2 files themselves are written by librw's own PS2 raster code.
  * The RGBA output equals a separate **specification decoder** in exact arithmetic (`tools/txd-ref/make_reference.py`).
  * librw and **Pillow** stay within the documented rounding tolerance.
  * CI rebuilds librw at the pinned commit and regenerates everything to check it still matches.
* **Windows:** the Windows build's texture export is byte-identical to the Linux build's (checked under Wine).
* **CD audio tracks:** equal ffmpeg's `s16le` decode of each track of a mixed-mode PlayStation BIN/CUE fixture.
* **Dreamcast STR:** bit-identical to **vgmstream-cli** (commit `f7f7641`) on random-data mono and stereo streams in a GD-ROM fixture.
* **TGA:** equal to **Pillow 12.3.0** for the layouts Pillow writes, and to a specification reference for 16-bit images.
* **FXT / GXT:** synthetic tables encoded independently from the specifications; the FXT header mask reproduces file(1)'s FXT signature.
* **Release identification:** synthetic layouts of each 2D release (GTA1 with both London packs, London 1969, lower-case GTA2, PlayStation BIN/CUE, Dreamcast GDI, GBA ROM) through the command line (`apps/eyefinder-cli/tests/legacy_releases.rs`); every San Andreas build value; a Steam folder holding a downgraded `gta_sa.exe` next to `gta-sa.exe`.

## Real-file observations (2026-10-23)

The maintainer shared real copies on 2026-10-23 for testing. They were downloaded into a temporary
work folder, never committed, and deleted afterwards. Nothing below contains game data: only sizes,
hashes, header fields and counts.

**What the maintainer confirmed in the app** (so these are marked **Verified** on the
Documentation page): the GTA2 PC "Rockstar Classics" folder (2004 release) loads, and the GTA1 PC
disc registers as GTA1. The maintainer also reported that GTA2 audio sometimes played at the wrong
speed. The cause was the player, not the decoders (rodio kept converting every track at the first track's rate; `apps/eyefinder/src/player.rs`), and it is fixed, so
the audio decoders stay "Implemented from spec" until the maintainer has listened again.

**GTA2 PC, 2004 re-release** (`readme.txt` "(c) 2004 Rockstar Games"):
* `gta2.exe`: 1,667,072 bytes, SHA-1 `646e1d479fa2aea12b3ff448fa23f6362f9e012f`, file version 9.6.0.0.
* `data/Audio/WIL.SDT`, `STE.SDT`, `bil.sdt`: 7680 bytes, 320 records each. Slots 69-136 are the only 5500 Hz records; 34, 34 and 36 of them have odd byte sizes; their bytes average 127.6 (the midpoint of unsigned 8-bit samples). Every other slot has an even size. Each RAW file is exactly covered by its records. All 960 slots now decode (before: none, because of the odd sizes), and the 68 radio slots of `WIL` equal ffmpeg's `u8` reading at the SDT rate.
* Export report of Eyefinder 0.2.0 (before these fixes): 137 files (18 FSTYLE sounds, 97 voice clips, 22 music tracks), no failures.
* `e.gxt`, `s.gxt`: every code unit of 0x100 or above is 0x2153-0x217A, at the start of a string.

**GTA2 PlayStation, POPStarter VCD** (`SLES_014.04.Grand Theft Auto 2.VCD`, 547,516,960 bytes):
* Header bytes as cue2pops writes them (`41 00 A0 … 20 … A1 … A2`), track entry `41 00 01 00 00 00 00 00 02 00`, sector count 232,342 at 1032 and 1036, version tag `kHn"` at 1024. Payload: 232,342 raw sectors.
* `SYSTEM.CNF` boots SLES-01404: redump "Grand Theft Auto 2 (Europe) (En,Fr,De,Es,It)". Checked with the command line, not yet in the app.

**GTA2 Dreamcast, DiscJuggler CDI** (`Grand Theft Auto 2.cdi`, 439,554,332 bytes):
* Trailer `0x80000006` (V3.5), header offset 700; two sessions; data track at LBA 11700, Mode 2/2336, at file offset 1,408,800; 296 files.
* IP.BIN: product `T40502D 93`, version `V1.003`, date `20000702`, title "GRAND THEFT AUTO 2". These are the fields redump disc 19717 shows (its "Internal Serial : T40502D 93"; Europe, T-40502D-18 / T-40502D-61). The area (`JUE` instead of ` E`) and company (`PARADISO` instead of `SEGA LC-T-405`) differ from redump's dump: the image was rebuilt.
* Compared with the PC 2004 release: `DATA/BIL.GMP` is byte-identical; `DATA/BIL.STY` has the same GBST 700 chunk set, with TILE, DELS, DELX, CARI, OBJI, SPEC, RECY and FONB identical and the palettes (PPAL holds every PC colour value: 34,375 shared, 7 more) and sprites (SPRG/SPRX) different. The SDT files have the same 320 slots, but each sample is about a quarter of the PC size (4 bits per sample; codec undocumented). `.PVR` files start with `GBIX` and contain `PVRT`.
* All 16 `.STR` streams decode bit-identically to vgmstream-cli (commit `f7f7641`).

**GTA1 PC disc, BIN/CUE** (one 754,825,008-byte BIN, 11 tracks, a `PREGAP` before track 2):
* Data track: 99,099,168 bytes, SHA-1 `e0232c49cb60bbfc634bb284bad002e0c8fdf962`, which is redump's track 1 of "Grand Theft Auto (USA) (En,Fr,De,It)", "(Europe) (En,Fr,De,It)", "(Europe) (En,Fr,De,It) (Alt)" and "(Australia)". The audio track sizes (adding the generated pregap to track 2) match the first three, not Australia. The audio tracks' SHA-1 differ from redump's (a different read offset when the disc was ripped).
* Files: `GTAWIN/GRAND THEFT AUTO.EXE` (909,824 bytes, SHA-1 `038561860ce80eda794a491935d64878f925376e`), `GTADOS/GTA24.EXE`, `GTA8.EXE`, `GTAFX.EXE`; no `GTAWIN.EXE`, which is why the disc was reported as missing its executable.
* All 10 CD audio tracks exported by Eyefinder equal ffmpeg's `s16le` decode of the same byte ranges.

**GTA2 Game Boy Color** (`Grand Theft Auto 2 (USA).gbc`): SHA-1 `6fd4b31d410ca98452ada0a1f3c0e2590e7df099`, the No-Intro dump "Grand Theft Auto 2 (USA)".

**GTA III / Vice City / San Andreas executables** (issue #3): `gta3.exe` (3,418,544 bytes) is identified as 1.1, `gta-vc.exe` (4,302,264 bytes) as the Steam / Rockstar Games Launcher build, and `gta_sa.exe` and `gta-sa.exe` (both 14,383,616 bytes, different SHA-1) as 1.0 US (HOODLUM executable). These are the builds whose signature each file carries; the store of an install still comes from its location.

## Sources

### Vice City PS2 manifest report (2026-10-21)

The maintainer supplied Eyefinder 0.2.0 audio and texture export manifests for a disc
identified as `SLUS-20552`. The source filename describes v1.40; that revision was
not independently identified from the disc. Personal source paths are not reproduced here.

* Audio: 99 entries, 99 files written, no failures, not cancelled. Categories:
  9 radio stations, 15 police/ambience, 2 announcements, 72 cutscenes, 1 music cue.
  96 entries use 32000 Hz and 3 use 16000 Hz; all are stereo. Every frame count
  equals `floor(source_bytes / 16384) * 14336`, and every duration equals frames/rate.
* Textures: 12,326 entries from 1,382 dictionaries, no reported skips. Formats:
  10,513 PAL4 ARGB8888, 1,789 PAL8 ARGB8888, 23 ARGB8888, 1 PAL8 RGB888.
* Both CSV files have the same entry counts as their JSON counterparts.

These are **export-completion observations**, not a byte comparison with a reference
decoder or an inventory of every asset on the disc. They do not establish support
for omitted sound banks, mission audio or movies. No support level is promoted.

Report fingerprints (SHA-256, metadata only; the uploads are not committed):

| Report | SHA-256 |
|---|---|
| Audio JSON | `2c5aaade72ffe73159cf50adf7ea5836a147ef7ec4db230bec064eca0138b3c5` |
| Audio CSV | `140f78c95e145b85e4104029373c4276a42817aa1b92fb11040e34395924b3a6` |
| Texture JSON | `cd204ce5a17b0a0f45ae77eb799865c245c4ff6421a8d0d2f325c295756bed08` |
| Texture CSV | `8faca0ee0d6d9c857055fde874f76be4481f0c4960219487d994ef7ee9fc39b2` |

### Format references

* re3 / reVC: reverse-engineered GTA III / Vice City source (mirrors of the original repositories). Only facts such as file names, identifiers and record layouts are used; no code is copied.
* gta-reversed: reverse-engineered San Andreas source, `source/game_sa/Audio/Loaders`.
* vgmstream: `src/coding/psx_decoder.c`, `doc/TXTH.md`.
* librw (MIT): `src/texture.cpp`, `src/d3d/*.cpp`, `src/ps2/ps2raster.cpp`, commit `18532c2e`.
* Khronos Data Format Specification 1.3 (S3TC), Vulkan specification (normalized conversions), Microsoft `D3DFORMAT`, `PALETTEENTRY`, DDS and PE format documentation.
* plugin-sdk `shared/GameVersion.cpp`; SilentPatch `MemoryMgr.GTA.h` (MIT); PCSX2 `GameIndex.yaml` and `CDVD.cpp`; Steam app info; steamlocate, Lutris and Playnite (Steam library locations); Galaxy-Plugin-Rockstar (Rockstar Games Launcher registry).
* redump and No-Intro DATs (libretro-database), DuckStation game database, psx-spx, Pan Docs, GBATEK, KallistiOS makeip, gditools3, libcdio, bchunk.
* Daniel Marschall's GTA1/London pages and gta_lang_converter (Apache-2.0); gta2_re; vgmstream `str_sega.c`; Truevision TGA 2.0.
* Microsoft *Multimedia Standards Update* (IMA ADPCM, MS ADPCM WAVE layouts), Vorbis I specification, ISO/IEC 11172 / 13818.
