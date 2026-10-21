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

Games are recognised by files that the game itself opens (see `MARKERS` in `crates/ef-core/src/lib.rs`). PS2 and Xbox discs are told apart by their boot files (`SYSTEM.CNF`, `default.xbe`).

## How the implementations were checked

These checks run in CI on every change (`crates/ef-audio/tests/fixtures.rs`):

* **GTA III PC:** all 3,047 PCM/ADPCM/SFX outputs from the fixture game are SHA-256 identical to the original C ripper (`tools/legacy-c`). That ripper was itself confirmed on real files. MPEG outputs have identical frame counts and match within 2×10⁻⁷.
* **PS2 `.VB` read from an ISO image:** identical to **vgmstream** (`vgmstream-cli`, commit `f7f7641`). This includes random-data streams that exercise every filter, shift and flag value.
* **Vice City ADF:** decoding the `.ADF` equals decoding the de-obfuscated MP3.
* **San Andreas:** stream tracks read through the XOR key and track header equal the original Ogg files. SFX sounds equal their PCM bytes.
* **Seeking:** for every decoder, seeking to any frame yields exactly the samples of a decode from the start.

## Sources

* re3 / reVC: reverse-engineered GTA III / Vice City source (mirrors of the original repositories). Only facts such as file names, identifiers and record layouts are used; no code is copied.
* gta-reversed: reverse-engineered San Andreas source, `source/game_sa/Audio/Loaders`.
* vgmstream: `src/coding/psx_decoder.c`, `doc/TXTH.md`.
* Microsoft *Multimedia Standards Update* (IMA ADPCM, MS ADPCM WAVE layouts), Vorbis I specification, ISO/IEC 11172 / 13818.
