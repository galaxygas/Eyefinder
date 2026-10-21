# GTA III Music Ripper

An ASI plugin for the PC version of Grand Theft Auto III (the original 2002 PC game and its Steam / Rockstar Launcher re-releases, **not** the Definitive Edition). The first time the game starts with it installed, it extracts all of the game's audio to standard WAV files in a new folder, `<game folder>\Extracted Audio`. That covers every radio station, every other music source and every other streamed sound. It runs on a background thread, so the game starts as normal while it works.

## Install

1. Install an ASI loader. [Ultimate ASI Loader](https://github.com/ThirteenAG/Ultimate-ASI-Loader) works: put its Win32 `dinput8.dll` in the game folder. If you already use mods such as the widescreen fix or modloader, you already have one.
2. Copy `gta3_music_ripper.asi` and `gta3_music_ripper.ini` into the game folder, or into its `scripts` folder.
3. Start the game. Progress goes to `Extracted Audio\extraction_log.txt`. The run is finished when `Extracted Audio\_extraction_complete.txt` appears.

After that the plugin does nothing. To extract again, delete `_extraction_complete.txt` or set `Force=1` in the ini.

You can also skip the game entirely. Run `gta3_music_ripper.exe` from the game folder, or run `gta3_music_ripper.exe "C:\path\to\GTA III"`.

Requirements: Windows 10/11. Windows 7/8.1 also work if the Universal C Runtime is installed (update KB2999226). The output needs disk space: decoded ADPCM is 4 times the size of the source WAV files.

## What gets extracted

| Folder | Source files | Notes |
|---|---|---|
| `Radio Stations` | `AUDIO\HEAD, CLASS, KJAH, RISE, LIPS, GAME, MSX, FLASH, CHAT .wav` | Full station streams. Station names are read from the game's own `TEXT\AMERICAN.GXT` (keys `FEA_FM0`–`FEA_FM8`). |
| `Police Radio & Ambience` | `POLICE.WAV, CITY.WAV, WATER.WAV` | |
| `Announcements & Intros` | `COMOPEN.WAV, SUBOPEN.WAV, JB.MP3, BET.MP3` | |
| `Cutscenes` | 76 cutscene streams (`L1_LG.MP3` … `MT_PH4.MP3`, `A1_SSO.WAV` …) | Cutscene music and dialogue |
| `Music Cues` | `MISCOM.WAV` (mission complete), `END.MP3` (game complete) | |
| `Mission Audio` | 101 mission streams (`lib_a1.WAV` … `cat1.WAV`) | |
| `Intro Movies` | Audio track of every `movies\*.mpg` (`Logo.mpg`, `GTAtitles.mpg`, …) | Demuxed from the MPEG program stream. Also saved as the untouched `.original.mp2` bitstream. |
| `SFX Bank` | All 3032 samples in `AUDIO\SFX.RAW` | Named from the PC sample list (`SFX_ICE_CREAM_TUNE`, `SFX_PIANO_BAR_1`, …) |
| `Other Audio Files` | Any other `.wav`/`.mp3` in `AUDIO` | So nothing is missed in releases with extra files |

The list of streamed files, their in-game identifiers and the SFX sample names come from the reverse-engineered game source (re3: `sampman.h` `StreamedNameTable`, `audio_enums.h`, `AudioSamples.h`). Files are matched case-insensitively. A file your copy doesn't have is logged and skipped.

### Track splitting

The game does not mark where songs start inside a station file. In the game code (re3 `cMusicManager::GetTrackStartPos`), a station is one continuous looping stream, and the play position is only "where the stream would be by now". The plugin still reads any `cue` / `LIST adtl` (`labl`, `note`, `ltxt`) / `smpl` chunks inside each source WAV. If a file has markers, the plugin also writes:

* `<station> - Tracks\NNN - <label>.wav`: one file per marked segment. Joined together they are byte-identical to the full stream.
* `<station>.wav.cue`: a CUE sheet.
* `<station>.wav.labels.txt`: an Audacity label track.
* The markers themselves, embedded unchanged in the full-length WAV.

If a station has no markers, only the full stream is written, and the manifest shows an empty marker list.

## Quality

The source format is detected from each file's contents, not its extension:

| Source | Output |
|---|---|
| PCM WAV (any bit depth), float, WAVE_FORMAT_EXTENSIBLE | The sample data and `fmt` chunk are copied **bit-for-bit** |
| IMA ADPCM (`0x0011`), Xbox IMA (`0x0069`), MS ADPCM (`0x0002`) | Decoded to 16-bit PCM, which is the exact output of the codec, at the original sample rate. Trimmed to the `fact` chunk length. |
| MP3 / MP2 / MP1 (raw, in RIFF, or in .mpg) | Decoded with minimp3 to **32-bit float** (full decoder precision) at the original rate. Set `MpegFloat32=0` for 16-bit. |
| SFX.RAW | 16-bit mono PCM copied bit-for-bit, using the per-sample rate from SFX.SDT |

Nothing is resampled, normalized or dithered. ADPCM and MP3 are lossy formats, so the WAVs cannot hold more detail than the game files. They are an exact, lossless capture of what those files decode to.

## Metadata

* `manifest.json` covers every file: category, title, game id, source file, size, codec, rate, channels, average bitrate, LAME encoder delay/padding when present, output codec, sample-frame count, exact duration, markers with frame offsets and timestamps, SFX loop points (bytes and frames), and movie audio start times (first PTS).
* `manifest.csv` is the same data as a spreadsheet.
* `tracklist.txt` is a readable list with durations.
* Each WAV contains a `LIST/INFO` chunk with its title, product and source.

## Settings (`gta3_music_ripper.ini`)

`Enabled`, `Force`, `OutputFolder`, `MpegFloat32`, `ExtractMissionAudio`, `ExtractMovieAudio`, `KeepMovieAudioBitstream`, `ExtractSfxBank`, `ExtractOtherAudioFiles`. Each is described in the file.

## Building

`./build.sh` needs `pip install ziglang`. It cross-compiles the 32-bit Windows `.asi` and `.exe` with zig's bundled MinGW, and builds a native Linux binary for testing.

## How it was verified

There are no real game files in the build environment. Tests used generated files in every format the code handles:

* IMA ADPCM: bit-identical to an independent implementation of the IMA reference algorithm. The block/channel layout is bit-identical to FFmpeg 6.1.1 when that version's nibble formula is used. FFmpeg 6.1.1 uses a multiply approximation, while current FFmpeg master and re3 use the reference shift-add form, which this plugin implements.
* MS ADPCM: bit-identical to FFmpeg.
* PCM 8/16-bit: bit-identical copies.
* MP3: within 4.3e-7 of FFmpeg's float decoder. MP2 from MPEG-1/MPEG-2 program streams: within 3.2e-5, less than one 16-bit step. The demuxed bitstream is byte-identical to `ffmpeg -c copy`.
* Cue-marker splitting: the joined tracks equal the full stream.
* Windows build, run under Wine from a stand-in game exe: loads from `scripts\`, survives `FreeLibrary`, handles a non-ASCII game path, and produces output byte-identical to the native build.

## Credits

* MP1/2/3 decoding: [minimp3](https://github.com/lieff/minimp3) (CC0, see `src/minimp3_LICENSE.txt`).
* File and sample name tables: generated from the re3 project's reverse-engineered GTA III source. No game data is included.
