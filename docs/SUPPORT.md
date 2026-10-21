# Support matrix

Every asset in Eyefinder carries one of three support levels, shown in the app next to each item:

| Level | Meaning |
|---|---|
| **Verified** | Decoded output has been checked against **real game files**. |
| **Implemented from spec** | Implemented from documented reverse-engineering sources and tested against synthetic files built from that documentation. It has **not yet been confirmed on real game files**. |
| **Not supported yet** | Eyefinder recognises the data but has no trustworthy specification for it, so it does not decode it. Nothing is guessed. |

A format moves to **Verified** only when someone runs Eyefinder on a real copy and the result is confirmed (see [CONTRIBUTING.md](../CONTRIBUTING.md#reporting-a-format-result)). Each such confirmation is recorded in the table below.

## Earlier games (new, unverified implementations)

| Game/platform | Implemented from specification | Not yet covered |
|---|---|---|
| GTA1 PC (shared DOS/Windows asset layout) | Six documented SDT/RAW bank types, loose WAVs; GRY 325 and G24 336 tiles, tile palette variants, base sprites, stored deltas and FON images | CD tracks, standalone RAW/RAT graphics, sprite remaps and combined delta states |
| London 1969 / 1961 PC | Explicit expansion choice; UK SDT/RAW and WAV subtree; same documented GRY/G24 and FON readers | Automatic expansion identity, CD tracks, standalone RAW/RAT graphics, sprite remaps and combined delta states |
| GTA2 PC | 24-byte SDT/RAW banks, loose WAVs; STY 700 tiles, default sprites and stored deltas | Sprite remaps, combined delta states, standalone graphics; release-wide asset inventory validation |
| GTA1 / London / GTA2 PlayStation | None added | Platform-specific discovery and full audio/graphics formats |
| GTA1 / GTA2 Game Boy Color | None added | ROM asset boundaries, compression/palette mappings and music sequencer |
| GTA2 Dreamcast | None added | Disc/archive inventory, texture/palette mapping, complete audio catalog |
| GTA Advance Game Boy Advance | None added | Regional ROM mappings, complete graphics inventory and sequenced audio |

These additions have synthetic reference tests, **not real-file confirmation**.
They do not establish every-platform or every-asset support. Sources and exact
limits are recorded in [FORMATS.md](FORMATS.md). London import never guesses between
1969 and 1961 from their shared audio paths. Use the installation root containing
`GTADATA` (GTA1/London) or `DATA` (GTA2).

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

Textures carry the support level of their own native format, whatever the game's platform.

## Game versions

| Platform | Identified by | Source |
|---|---|---|
| PC | One value in the game executable (1.0, 1.1 / 1.01, Steam and the San Andreas variants) | plugin-sdk `GameVersion.cpp` |
| PS2 | The disc serial in `SYSTEM.CNF` | PCSX2 `GameIndex.yaml` |
| Xbox, Mac | Not identified | — |

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
* plugin-sdk `shared/GameVersion.cpp`; PCSX2 `GameIndex.yaml` and `CDVD.cpp`; Steam app info; steamlocate, Lutris and Playnite (Steam library locations).
* Microsoft *Multimedia Standards Update* (IMA ADPCM, MS ADPCM WAVE layouts), Vorbis I specification, ISO/IEC 11172 / 13818.
