# Eyefinder

**A viewer and extractor for the assets of Grand Theft Auto III, Vice City and San Andreas.**

Eyefinder opens a game folder or a disc image, works out which game, platform and version it is,
and lets you browse, preview and export its contents. It currently handles **audio** (every radio
station, cutscene, mission line, sound effect and the music of the intro movies) and **textures**
(every texture dictionary, loose or inside IMG archives). Models and other data will follow.

![Eyefinder showing textures in dark mode](docs/screenshots/textures-dark.png)

## Features

* **Add games your way:**
  * a PC install folder, an extracted disc, or a PS2 `.iso` read directly;
  * or let **Find installed games** look through your Steam libraries and the Rockstar Games folders and add one or all of the copies it finds.
* **Choose the game, platform and version:** Eyefinder detects them and shows its evidence, and you can change them.
  * Executable version on PC, disc serial on PS2.
  * Only choices the files actually support are offered.
* **Sidebar sections per asset type** (Audio by category, Textures by folder or archive), or switch to the **file tree**. IMG archives open like folders, and any file can be inspected and copied byte for byte.
* **Textures:**
  * A thumbnail grid or a list, and a large preview with zoom, mip levels and a transparency checkerboard.
  * Supported natives: Direct3D 8/9 (PC), Xbox and PS2, including palettes, DXT1/3/5 and swizzled PS2/Xbox data.
  * Export to PNG, optionally every mip level, with DXT data also kept untouched as `.dds`.
* **Audio:**
  * Play anything instantly, with a waveform, sample-exact seeking and volume control. Hour-long radio stations are streamed, never loaded whole.
  * Export to WAV with nothing lost on the way:
    * PCM is copied bit for bit.
    * ADPCM is decoded to its exact 16-bit output.
    * MP3/MP2/Vorbis are decoded at full 32-bit float precision.
    * Nothing is resampled.
* **Metadata with every export:** `manifest.json` and `manifest.csv` (plus `tracklist.txt` for audio). Audio exports also include:
  * cue markers (as per-marker track files, `.cue` sheets and Audacity labels),
  * SFX loop points,
  * San Andreas beat data,
  * the untouched movie audio bitstream.
* **Honest about what it knows:** every item shows a support level. Nothing is guessed.
* **Dark and light themes.**

![Choosing the game version](docs/screenshots/import.png)

## Supported games

| | PC | PS2 | Xbox / mobile / Mac |
|---|---|---|---|
| GTA III audio | ✅ Verified | 🧪 From spec (streams) | Recognised only |
| Vice City audio | 🧪 From spec | 🧪 From spec (streams) | Recognised only |
| San Andreas audio | 🧪 From spec (streams + SFX) | Recognised only | Recognised only |
| Textures (all three games) | 🧪 From spec | 🧪 From spec | 🧪 From spec (Xbox natives) |

**Verified** means checked against real game files. **From spec** means implemented from
documented sources (reverse-engineered game code, librw, official specifications) and tested
against synthetic files and independent reference decoders, but not yet confirmed on a real copy.
The full matrix, with sources, is in [docs/SUPPORT.md](docs/SUPPORT.md) and the byte layouts are
in [docs/FORMATS.md](docs/FORMATS.md).

**Have one of these games?** A [format report](CONTRIBUTING.md#reporting-a-format-result) is the
fastest way to move a format to Verified. The Definitive Edition is not supported: it uses entirely
different formats.

![Eyefinder in light mode](docs/screenshots/textures-light.png)

## Download

Builds for Windows, macOS and Linux are attached to each
[release](https://github.com/galaxygas/Eyefinder/releases). Unzip and run `eyefinder` (or
`eyefinder.exe`). The macOS build is not signed: right-click the app and choose *Open* the first
time.

The command-line tool runs without the GUI:

```sh
eyefinder-cli scan                                                  # find installed copies
eyefinder-cli info     "C:\Games\GTA Vice City"
eyefinder-cli versions "C:\Games\GTA San Andreas"                   # the choices it accepts
eyefinder-cli export   "C:\Games\GTA Vice City" "D:\VC audio"       # all audio
eyefinder-cli textures "C:\Games\GTA San Andreas" "D:\SA textures" --mipmaps
eyefinder-cli export   gta3_ps2.iso out --only "Radio Stations" --pcm16
eyefinder-cli info     sa.iso --game sa --platform ps2 --version ps2-SLUS-20946
```

## Building from source

Rust 1.95 or newer. On Linux, first install
`libasound2-dev libgtk-3-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev pkg-config`.

```sh
cargo run --release -p eyefinder            # the app
cargo run --release -p eyefinder-cli -- ... # the command line tool
cargo test --workspace                      # all tests
```

## How correctness is checked

Every change runs on Windows, macOS and Linux against synthetic fixtures. There is no game data in
this repository. Outputs are compared with independent references:
* **GTA III PC audio:** the original GTA III ripper, which was verified on real files (`tools/legacy-c`).
* **PS2 audio:** vgmstream.
* **Vice City and San Andreas audio:** the de-obfuscated source streams.
* **Textures** (`tools/txd-ref`):
  * **librw**, for the stored texture data and PS2 unswizzling;
  * an exact **specification decoder**, for every pixel;
  * **Pillow**, as a cross-check.
* **Seeking:** checked to be sample-exact for every decoder.

See [docs/SUPPORT.md](docs/SUPPORT.md#how-the-implementations-were-checked).

## Contributing

Contributions from people and AI agents are welcome through pull requests. Read
[CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md) first. The main rule: cite a source
for every format detail, or leave it unsupported.

## Legal

Eyefinder is licensed under the **GNU General Public License v3.0 or later** ([LICENSE](LICENSE)).
It contains no game data and only works with copies you own. It is not affiliated with or endorsed
by Rockstar Games or Take-Two Interactive. Grand Theft Auto is a trademark of Take-Two Interactive.

Third-party components:
* [Symphonia](https://github.com/pdeljanov/Symphonia) (MPL-2.0)
* [egui/eframe](https://github.com/emilk/egui) (MIT / Apache-2.0)
* [rodio](https://github.com/RustAudio/rodio) (MIT / Apache-2.0)
* [png](https://github.com/image-rs/image-png) (MIT / Apache-2.0)
* [winreg](https://github.com/gentoo90/winreg-rs) (MIT, Windows only)
* [Inter](https://github.com/rsms/inter) (OFL-1.1, see `apps/eyefinder/assets/fonts`)
* [Phosphor Icons](https://github.com/phosphor-icons) (MIT)
* [minimp3](https://github.com/lieff/minimp3) (CC0, in the legacy C tool)
* [librw](https://github.com/aap/librw) (MIT; used only by the test reference tool, fetched at build time)

Format knowledge comes from the re3/reVC, gta-reversed, librw and vgmstream projects, the gtamods
wiki, plugin-sdk, PCSX2's game database and the official Khronos, Vulkan and Microsoft
specifications.
