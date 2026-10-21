# Support matrix

Every asset in Eyefinder carries one of three support levels, shown in the app next to each item:

| Level | Meaning |
|---|---|
| **Verified** | Decoded output has been checked against **real game files**. |
| **Implemented from spec** | Implemented from documented reverse-engineering sources and tested against synthetic files built from that documentation. It has **not yet been confirmed on real game files**. |
| **Not supported yet** | Eyefinder recognises the data but has no trustworthy specification for it, so it does not decode it. Nothing is guessed. |

A format moves to **Verified** only when someone runs Eyefinder on a real copy and the result is confirmed (see [CONTRIBUTING.md](../CONTRIBUTING.md#reporting-a-format-result)). Each such confirmation is recorded in the table below.

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

* re3 / reVC: reverse-engineered GTA III / Vice City source (mirrors of the original repositories). Only facts such as file names, identifiers and record layouts are used; no code is copied.
* gta-reversed: reverse-engineered San Andreas source, `source/game_sa/Audio/Loaders`.
* vgmstream: `src/coding/psx_decoder.c`, `doc/TXTH.md`.
* librw (MIT): `src/texture.cpp`, `src/d3d/*.cpp`, `src/ps2/ps2raster.cpp`, commit `18532c2e`.
* Khronos Data Format Specification 1.3 (S3TC), Vulkan specification (normalized conversions), Microsoft `D3DFORMAT`, `PALETTEENTRY`, DDS and PE format documentation.
* plugin-sdk `shared/GameVersion.cpp`; PCSX2 `GameIndex.yaml` and `CDVD.cpp`; Steam app info; steamlocate, Lutris and Playnite (Steam library locations).
* Microsoft *Multimedia Standards Update* (IMA ADPCM, MS ADPCM WAVE layouts), Vorbis I specification, ISO/IEC 11172 / 13818.
