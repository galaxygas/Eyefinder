# Missing documentation

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

## PlayStation: GTA1, London 1969, GTA2

The releases are identified: disc serials come from redump, the boot rule from psx-spx (see [docs/FORMATS.md](docs/FORMATS.md#console-releases)). The CD audio tracks of GTA1 and London 1969 are decoded. Nothing else on these discs is documented.

| Data | What is known | What is missing |
|---|---|---|
| Disc file inventory | The boot executable name (the serial) and the track layout. GTA1 PS1 has 1 data + 10 audio tracks (redump). London 1969 has 1 + 15 (redump; PSX Datacenter says 16 audio tracks, so the sources disagree). GTA2 PS1 is a single data track. `eyefinder-cli files` lists a disc's files. | Which files exist and what each holds. |
| Graphics (styles, sprites, fonts) | GTA2: DMA's GBH Style 4.6 describes a `PSXT` chunk (256×256 pages of 32×32 tiles, 15-bit BGR palettes). The document calls it a design that "may be scrapped". | Whether the retail discs use `PSXT`, other containers, or TIM images; GTA1/London PS1 graphics: nothing. |
| Maps | GTA2: GBH Map 8.2 describes a `PSXM` chunk and says the PSX version "must use CMAP in the final version". The PC and Dreamcast GMP files carry a 4096-byte `PSXM` chunk (2026-10-23). | The retail map files and their versions. |
| Sound effects and speech | Nothing GTA-specific. psx-spx documents the generic Sony formats (VAG, VAB, XA-ADPCM), but there is no evidence that these games use them. | Container, sample format and sample rates. |
| GTA2 music | The disc has no CD audio tracks, so the music is in the data track. | Its container and codec (XA is only a guess, not used). |
| Text | Nothing for the PS1 builds. | Whether FXT/GXT are used unchanged. |
| Video | Nothing GTA-specific (psx-spx documents STR/MDEC generically). | Which files, if any, are movies. |
| GTA2 PS1 serial SLES-02458 | Listed by older DuckStation databases (released 2002-03-08), but missing from redump and from current DuckStation. | A redump entry or a real disc to confirm it. Until then it is not in the index. |

## Dreamcast: GTA2

The disc is identified from IP.BIN, matched against redump's disc pages. The `.STR` audio streams are decoded, following vgmstream. The maintainer's disc (redump 19717, rebuilt as a DiscJuggler image) was compared file by file with the GTA2 PC release of 2004 (docs/SUPPORT.md): its maps are byte-identical and its styles use the PC format, so those are read with the PC readers.

| Data | What is known | What is missing |
|---|---|---|
| Which files are STR streams, and what they are | vgmstream: "4-bit: common [GTA2 (DC)]". The maintainer's disc holds A, D, M, S1-S12 and S101 `.STR` at the root (2026-10-23). | Which streams are music and which are speech, and whether multi-track or 16-bit streams occur (Eyefinder decodes only single-track 4-bit streams and reports the others). |
| SDT/RAW sound banks | The SDT files are the same 7680 bytes (320 slots of 24 bytes) as on PC, but every sample is about a quarter of the PC size (WIL slot 0: 59394 → 14852 bytes; radio slot 69: 2063 → 1032), i.e. 4 bits per sample (2026-10-23). | The codec. The Dreamcast's AICA ADPCM is likely but not documented for these banks, so they are not decoded. |
| PowerVR textures (`.PVR`) | The files start with `GBIX` and contain a `PVRT` chunk (2026-10-23). | A cited PowerVR texture specification to decode them against. |
| Per-level `.DSG`, `.SPX`, `.DTL`, `.DPL`, `.PSG`, `.DLX`, `.USG` files | Each level folder (BIL, STE, WIL, FSTYLE) holds them (2026-10-23). | Everything. |
| Movies (`GTA230.SFD`) | One 73,828,352-byte Sofdec-named file at the root (2026-10-23). | A cited specification of its container and codecs. |

## Game Boy Color: GTA1, GTA2

The ROMs are identified by cartridge header (Pan Docs) and by No-Intro SHA-1.

| Data | What is known | What is missing |
|---|---|---|
| Everything in the ROM (tiles, palettes, compression, maps, text, music sequencer, sound effects) | Nothing GTA-specific. Only the generic Game Boy 2bpp tile format is public, and there is no evidence of how these ROMs store or compress their data. | Everything. |

## Game Boy Advance: GTA Advance

The ROMs are identified by cartridge header (GBATEK game codes BGTE / BGTP) and by No-Intro SHA-1.

| Data | What is known | What is missing |
|---|---|---|
| Sprites, city textures, fonts, text, sound samples, music | GTA-Advance-Studio (@ `7eae5b3e`) claims for the **European** ROM: "2,475 sprites (2,303 BIOS-RLE compressed, 172 uncompressed), 4bpp", "640 uncompressed 8-bit city textures", 8×8 4bpp fonts at fixed offsets, Latin-1 strings via pointers, and "282 PCM samples … 9,556 Hz, 8 bits, mono". It also says the ROM does **not** use a sequencer such as MusicPlayer2000. | All of it is found by scanning heuristics and fixed offsets in one ROM: no record boundaries or tables are documented (its notes say the ROM "does not reveal exactly where the logical record ends"). There is nothing for the USA ROM (BGTE). The no-sequencer claim is unconfirmed. |

## PC: GTA1, London 1969, London 1961

| Data | What is known | What is missing |
|---|---|---|
| `.RAT` and loose `.RAW` images (cutscenes, logos, menus), `.ACT` palettes | Marschall's inventory: "RAT: 8-bit graphics that use the F_PAL.RAW palette", RAW are raw 640×480 frames, ACT is the "Photoshop ACT Format". London 1961's `f_logoe*.raw` are 24-bit only. | The dimensions of each file, and which palette goes with which image. Without them the pixels cannot be laid out without guessing. |
| `.REP` replay files | Marschall: "purpose unclear". | Everything. |
| `KANJI.BIT` / `KANJI.IDX` (Japanese GTA1 font) | Marschall: KANJI.BIT holds 5040 glyphs of 16×16 pixels at 1 bit per pixel, and KANJI.IDX is an Int16 → Int16 table. The lookup index "becomes transformed (unknown)". | The index transform. FXT text shows Japanese characters as their Shift-JIS codes, and the glyphs are not drawn. |
| FXT codes `0x00 xx yy` and `0xC3 xx` | Marschall's converter keeps them as "illegal character" markers. His notes say `0x00` "is not found in the game files" and give only a theory for it. | What the game does with them. Eyefinder shows them as marked codes. |
| GRY version 305 | DMA's CityScape document says 325 in §3.6 and 305 in §3.7. OpenGTA accepts 325. | Whether any release uses 305. Files with 305 are refused. |
| Executable builds | The exe paths are known: `gtawin\gtawin.exe` (Classics / Steam), `GTAWIN\GRAND THEFT AUTO.EXE` and `GTADOS\GTA24.EXE`, `GTA8.EXE`, `GTAFX.EXE` (retail discs), `gtawin\gta_uk.exe`, `gtawin\gta_61.exe`. Retail PC discs are identified by their tracks against redump's PC DAT, and one disc executable is known from a real-file report. | A table of builds (retail 12.0/12.1, 11.5 budget, Rockstar Classics, 3Dfx/8-bit/24-bit DOS). Eyefinder reports the file version stored in each executable and does not name other builds. |
| Rockstar Classics music WAV files | Marschall: the Classics and Steam editions "play music from WAV files" and he names `Track10.wav`. | Their folder and the full name list. Eyefinder lists every WAV it finds, by path. |

## PC: GTA2

| Data | What is known | What is missing |
|---|---|---|
| Compiled mission scripts (`.scr`) | DMA's scripting notes cover the source language and the `miss2` compiler. A community decompiler exists, and libmagic has signatures. | The binary layout, as a specification. |
| `.REP` replay files | libmagic signature `v9.6\0\0`. | Everything else. |
| Russian text (`r.gxt`) | gta2_re maps code units 0x80-0xFF through `kExtendedRussianCharMap`. Marschall's chart shows the glyph order (JCUKEN keyboard order). | A reliable source for the map's contents: gta2_re itself notes that related character tables are "very wrong". Eyefinder shows those code units as `{U+XXXX}`. |
| GXT prefix codes 0x2153-0x217A | Real e.gxt and s.gxt (PC 2004 release) and E.GXT (Dreamcast): every code unit of 0x100 or above is one of 0x2153-0x217A, and each starts its string (docs/SUPPORT.md, 2026-10-23). | What the game does with them (they may select a speaker or portrait). Eyefinder shows them as marked codes `{U+21XX}`. |
| `kanji.dat` (Japanese font) | Marschall: the 16×16 glyphs are "implemented in Kanji.dat". | Its layout. |
| Executable builds | `gta2.exe` (gta2_re targets 9.6f). Retail PC discs are identified by their tracks against redump's PC DAT, and the 2004 Rockstar Games re-release executable (9.6.0.0) is known from a real-file report. | A table of the other builds (9.6 retail, rerelease, Classics, 10.1 Russian, 10.5 Japanese). Eyefinder reports the stored file version of an unknown executable. |

## PC: GTA III, Vice City, San Andreas (relevant to issue #3)

| Data | What is known | What is missing |
|---|---|---|
| Executables packed or encrypted on disk | plugin-sdk and SilentPatch read their signature values from the running, unpacked game. | Whether the current Steam and Rockstar Games Launcher executables are wrapped by DRM on disk. If they are, Eyefinder reports "matches no known build" and still names the store from the install location. A real-file report would settle it. |
| Steam vs Rockstar Games Launcher for GTA III / Vice City | SilentPatch knows one shared build signature for both. | No executable difference is documented, so the store comes from the install location (Steam manifest, launcher registry), not from the executable. |

## Disc images

| Data | What is known | What is missing |
|---|---|---|
| CD audio tracks inside a VCD | cue2pops stores them inline and writes their positions into the header. | A mapping from header to stream that holds for every converter option: cue2pops shifts the header times by its gap options and may inject a pregap (cue2pops.c lines 740-960), so Eyefinder reads only the data track of a VCD. Use the BIN/CUE for the music. |

## Release index gaps

The release index (Index page, `crates/ef-core/data/release_index.json`) lists only what its pinned sources name. These releases and versions have no reachable, citable source yet:

* **ps4, ps5, xone, xsx**: Disc and digital releases for PlayStation 4/5 and Xbox One / Series X|S: no redump or No-Intro DAT for these platforms exists in libretro-database or on redump.org, so their regional releases, serials and versions are not listed.
* **ios, android, fireos, winphone**: Mobile releases: the iTunes Search API gives only the current version, and no source of the version history was reachable (PCGamingWiki answered 403); the stores were not used.
* **pc**: PC patches of GTA IV (1.0.0.0 to 1.0.8.0, Complete Edition 1.2.0.x) and GTA V builds: the citable lists (PCGamingWiki, gtamods.com) could not be reached (403 / 503).
* **pc**: Digital PC releases (Steam, Rockstar Games Launcher, Epic, GOG and the old Rockstar Classics downloads): store build numbers carry no version names; only the executable builds Eyefinder identifies are listed.
* **mac**: Mac releases: redump publishes a Mac disc DAT, but it has not been added to the index yet; App Store versions are not listed.
* **psp, ps3, x360**: Digital entries (PSN, Xbox Live) come from No-Intro and list licence or add-on files, not every regional storefront release.
