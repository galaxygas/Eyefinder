# Missing documentation

This file lists every file type, data layout and identity that Eyefinder needs for the 2D games
(GTA1, London 1969, London 1961, GTA2, GTA Advance) but for which **no public documentation was
found**. They could only be supported through original reverse engineering of real game files or
executables. Until then they stay **Not supported yet** (`SupportLevel::DetectedOnly`), as
[AGENTS.md](AGENTS.md) requires.

How this list was made (research on 2026-10-21/22):
* DMA Design's own documents: CityScape Data Structure 12.10, GBH Style Format 4.6, GBH Map Format 8.2, and the GTA2 scripting notes.
* Daniel Marschall's GTA1/London pages and his [gta_lang_converter](https://github.com/danielmarschall/gta_lang_converter).
* gta2_re ([CriminalRETeam/gta2_re](https://github.com/CriminalRETeam/gta2_re) @ `edb484d3`).
* OpenGTA, vgmstream (@ `f7f76415`), psx-spx, GTA-Advance-Studio (@ `7eae5b3e`).
* gtamp.com, redump.org, the No-Intro / redump DATs in libretro-database (@ `12c21cf7`) and the DuckStation game database.
* gtamods.com (the GTA modding wiki), tcrf.net and Sega Retro could not be reached during the research (HTTP 503 / bot walls). Their pages may document some items below and should be checked when they are back.

Documented formats that Eyefinder simply has not implemented yet (Smacker and Bink movies, CMP and
GMP maps, GTA2 Japanese text) are **not** listed here. They are in
[docs/SUPPORT.md](docs/SUPPORT.md).

## How to help

A report from a real copy resolves most of these items, without uploading game files:
* the file list of the disc or install (`eyefinder-cli list` and the Files view show it);
* for a single file, its first bytes (the Files view shows them) and its size;
* the `manifest.json` of an export.

See [CONTRIBUTING.md](CONTRIBUTING.md#reporting-a-format-result).

## PlayStation: GTA1, London 1969, GTA2

The releases are identified: disc serials come from redump, the boot rule from psx-spx (see [docs/FORMATS.md](docs/FORMATS.md#console-releases)). The CD audio tracks of GTA1 and London 1969 are decoded. Nothing else on these discs is documented.

| Data | What is known | What is missing |
|---|---|---|
| Disc file inventory (all three) | Only the boot executable name (the serial) and the track layout. GTA1 PS1 has 1 data + 10 audio tracks (redump). London 1969 has 1 + 15 (redump; PSX Datacenter says 16 audio tracks, so the sources disagree). GTA2 PS1 is a single data track. | Which files exist and what each holds. |
| Graphics (styles, sprites, fonts) | GTA2: DMA's GBH Style 4.6 describes a `PSXT` chunk (256×256 pages of 32×32 tiles, 15-bit BGR palettes). The document calls it a design that "may be scrapped". | Whether the retail discs use `PSXT`, other containers, or TIM images; GTA1/London PS1 graphics: nothing. |
| Maps | GTA2: GBH Map 8.2 describes a `PSXM` chunk and says the PSX version "must use CMAP in the final version". | The retail map files and their versions. |
| Sound effects and speech | Nothing GTA-specific. psx-spx documents the generic Sony formats (VAG, VAB, XA-ADPCM), but there is no evidence that these games use them. | Container, sample format and sample rates. |
| GTA2 music | The disc has no CD audio tracks, so the music is in the data track. | Its container and codec (XA is only a guess, not used). |
| Text | Nothing for the PS1 builds. | Whether FXT/GXT are used unchanged. |
| Video | Nothing GTA-specific (psx-spx documents STR/MDEC generically). | Which files, if any, are movies. |
| GTA2 PS1 serial SLES-02458 | Listed by older DuckStation databases (released 2002-03-08), but missing from redump and from current DuckStation. | A redump entry or a real disc to confirm it. Until then it is not in the table. |

## Dreamcast: GTA2

The disc is identified: the product number is read from IP.BIN, using the makeip field table. The
`.STR` audio streams are decoded, following vgmstream.

| Data | What is known | What is missing |
|---|---|---|
| IP.BIN product numbers of the European discs | redump lists `T-40502D-18` and `T-40502D-61` for Europe and `T-40502D-09` for France. Its France and USA entries are flagged "Has to be confirmed". | What IP.BIN actually contains on each disc. A suffix like `-18` may not be in the 10-byte field. If it is not, the disc is reported as unlisted and the game must be chosen by hand. |
| Which files are STR streams, and what they are | vgmstream: "4-bit: common [GTA2 (DC)]". | The file inventory, which streams are music and which are speech, and whether multi-track or 16-bit streams occur (Eyefinder decodes only single-track 4-bit streams and reports the others). |
| Graphics, maps, text, sound effects, movies | Nothing. No public analysis of Dreamcast STY/GMP variants or PVR textures was found. | Everything. |

## Game Boy Color: GTA1, GTA2

The ROMs are identified by cartridge header (Pan Docs) and by No-Intro SHA-1.

| Data | What is known | What is missing |
|---|---|---|
| Everything in the ROM (tiles, palettes, compression, maps, text, music sequencer, sound effects) | Nothing GTA-specific. Only the generic Game Boy 2bpp tile format is public, and there is no evidence of how these ROMs store or compress their data. | Everything. |

## Game Boy Advance: GTA Advance

The ROMs are identified by cartridge header (GBATEK game codes BGTE / BGTP) and by No-Intro SHA-1.

| Data | What is known | What is missing |
|---|---|---|
| Sprites, city textures, fonts, text, sound samples, music | GTA-Advance-Studio (@ `7eae5b3e`) claims for the **European** ROM: "2,475 sprites (2,303 BIOS-RLE compressed, 172 uncompressed), 4bpp", "640 uncompressed 8-bit city textures", 8×8 4bpp fonts at fixed offsets, Latin-1 strings via pointers, and "282 PCM samples … 9,556 Hz, 8 bits, mono". It also says the ROM does **not** use a sequencer such as MusicPlayer2000. | All of it is found by scanning heuristics and fixed offsets in one ROM: no record boundaries or tables are documented (its notes say the ROM "does not reveal exactly where the logical record ends"). There is nothing for the USA ROM (BGTE). The no-sequencer claim is unconfirmed, and it contradicts the earlier "sequenced audio" note in this repository. |

## PC: GTA1, London 1969, London 1961

| Data | What is known | What is missing |
|---|---|---|
| `.RAT` and loose `.RAW` images (cutscenes, logos, menus), `.ACT` palettes | Marschall's inventory: "RAT: 8-bit graphics that use the F_PAL.RAW palette", RAW are raw 640×480 frames, ACT is the "Photoshop ACT Format". London 1961's `f_logoe*.raw` are 24-bit only. | The dimensions of each file, and which palette goes with which image. Without them the pixels cannot be laid out without guessing. |
| `.REP` replay files | Marschall: "purpose unclear". | Everything. |
| `KANJI.BIT` / `KANJI.IDX` (Japanese GTA1 font) | Marschall: KANJI.BIT holds 5040 glyphs of 16×16 pixels at 1 bit per pixel, and KANJI.IDX is an Int16 → Int16 table. The lookup index "becomes transformed (unknown)". | The index transform. FXT text shows Japanese characters as their Shift-JIS codes, and the glyphs are not drawn. |
| FXT codes `0x00 xx yy` and `0xC3 xx` | Marschall's converter keeps them as "illegal character" markers. His notes say `0x00` "is not found in the game files" and give only a theory for it. | What the game does with them. Eyefinder shows them as marked codes. |
| GRY version 305 | DMA's CityScape document says 325 in §3.6 and 305 in §3.7. OpenGTA accepts 325. | Whether any release uses 305. Files with 305 are refused. |
| Executable builds | The exe paths are known (`gtawin\gtawin.exe`, `gtawin\gta_uk.exe`, `gtawin\gta_61.exe`). | A table of builds (retail 12.0/12.1, 11.5 budget, Rockstar Classics, 3Dfx/8-bit/24-bit DOS). Eyefinder reports the file version stored in each executable and does not name the build. |
| Rockstar Classics music WAV files | Marschall: the Classics and Steam editions "play music from WAV files" and he names `Track10.wav`. | Their folder and the full name list. Eyefinder lists every WAV it finds, by path. |

## PC: GTA2

| Data | What is known | What is missing |
|---|---|---|
| Compiled mission scripts (`.scr`) | DMA's scripting notes cover the source language and the `miss2` compiler. A community decompiler exists, and libmagic has signatures. | The binary layout, as a specification. |
| `.REP` replay files | libmagic signature `v9.6\0\0`. | Everything else. |
| Russian text (`r.gxt`) | gta2_re maps code units 0x80-0xFF through `kExtendedRussianCharMap`. Marschall's chart shows the glyph order (JCUKEN keyboard order). | A reliable source for the map's contents: gta2_re itself notes that related character tables are "very wrong". Eyefinder shows those code units as `{U+XXXX}`. |
| `kanji.dat` (Japanese font) | Marschall: the 16×16 glyphs are "implemented in Kanji.dat". | Its layout. |
| Executable builds | `gta2.exe` (gta2_re targets 9.6f). | A table of builds (9.6 retail, rerelease, Classics, 10.1 Russian, 10.5 Japanese). Eyefinder reports the stored file version only. |

## PC: GTA III, Vice City, San Andreas (relevant to issue #3)

| Data | What is known | What is missing |
|---|---|---|
| Executables packed or encrypted on disk | plugin-sdk and SilentPatch read their signature values from the running, unpacked game. | Whether the current Steam and Rockstar Games Launcher executables are wrapped by DRM on disk. If they are, Eyefinder reports "matches no known build" and still names the store from the install location. A real-file report would settle it. |
| Steam vs Rockstar Games Launcher for GTA III / Vice City | SilentPatch knows one shared build signature for both. | No executable difference is documented, so the store comes from the install location (Steam manifest, launcher registry), not from the executable. |
