<div align="center">

# rmp

**A lightweight, retro-styled Terminal User Interface (TUI) music player written in Rust.**

[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![TUI](https://img.shields.io/badge/TUI-Ratatui%200.29-00d7ff)](https://ratatui.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build](https://img.shields.io/badge/Build-Cargo-brightgreen)](#installation--quick-start)

<br />

![Demo](assets/demo.gif)

</div>

```text
[VOL: 85%] [MUTED: NO]             ♫ rmp - RUST MUSIC PLAYER v1.4.0 ♫          [SHUFFLE: ON] [REPEAT: ALL]
╭────────────── TRACKS & PLAYLIST ───────────────╮╭──────────────────── NOW PLAYING ─────────────────────╮
│  01. Midnight City - M83                 04:03 ││ ARTIST: Canis & Mehrad Hidden        ████████        │
│▶ 02. Dele Man - Canis & Mehrad Hidden    03:20 ││ ALBUM:  Single / Unknown           ██▒▒▒▒▒▒▒▒██      │
│  03. Nightcall - Kavinsky                04:18 ││ TITLE:  Dele Man                  ██▒▒  ██  ▒▒██     │
│  04. Resonance - HOME                    03:32 ││ GENRE:  Electronic / Misc        ██▒▒  ████  ▒▒██    │
│  05. Voyager - Daft Punk                 03:47 ││ FORMAT: MP3 (320kbps / 44.1kHz)  ██▒▒  ████  ▒▒██    │
│  06. Sunset - The Midnight               04:34 ││                                   ██▒▒  ██  ▒▒██     │
│  07. Tech Noir - GUNSHIP                 04:57 ││  |<<   ■   ▶   ||   >>|            ██▒▒▒▒▒▒▒▒██      │
│  08. Running in the Night - FM-84        04:30 ││ [██████████░░░░░░░] 01:45 / 03:20    ████████        │
│  09. Turbo Killer - Carpenter Brut       03:28 ││                                     01:45 / 03:20    │
│  10. Days of Thunder - The Midnight      04:53 │╰──────────────────────────────────────────────────────╯
│  11. Pacific Coast Highway - Kavinsky    06:22 │╭────────────── LIVE SPECTRUM VISUALIZER ──────────────╮
│  12. Odd Look - Kavinsky                 04:49 ││  ▀   ▀     ▀       ▀         ▀                       │
│  13. Veridis Quo - Daft Punk             05:45 ││  █ ▀ █ ▀   █ ▀     █ ▀   ▀   █ ▀     ▀               │
│  14. Sunset Lover - Petit Biscuit        03:57 ││  █ █ █ █   █ █ ▀   █ █ ▀ █   █ █ ▀   █ ▀             │
│  15. A Real Hero - College               04:27 ││  █ █ █ █ ▄ █ █ █ ▅ █ █ █ █ ▄ █ █ █ ▆ █ █ ▃           │
│  16. Odyssey - HOME                      05:12 ││  █ █ █ █ █ █ █ █ █ █ █ █ █ █ █ █ █ █ █ █ █ ▅ ▂       │
╰────────────────────────────────────────────────╯╰──────────────────────────────────────────────────────╯
╭─────────────────────────────────────────── CONTROLS & HELP ────────────────────────────────────────────╮
│  [Space] Play/Pause | [n] Next | [p] Prev | [→/←] Seek | [+/-] Vol | [s] Shuf | [r] Rep | [/] Search   │
╰────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

---

## Features

- **🎵 Audio Playback Engine (`rodio`)**: Smooth playback with 5-second forward/backward seeking, volume control, instant mute toggling, and non-blocking PCM tapping (`AudioTap` using `try_lock()`) so UI rendering never stalls audio. Supports `.mp3`, `.flac`, `.wav`, and `.ogg` out of the box (`.m4a` files are also discovered during scanning).
- **📊 Real-Time FFT Spectrum Visualizer (`rustfft`)**: Zero-allocation 1024-point FFT pipeline with a pre-computed Hann window, 32 logarithmic frequency bands (`20 Hz – 16 kHz`), normalized dB scaling, gravity decay physics, floating peak caps, and Unicode eighth-block sub-cell rendering.
- **💿 Animated Spinning Vinyl Widget**: 4-phase pixel-art vinyl disc with specular groove reflections that rotates while a track is playing and pauses cleanly on stop/pause.
- **📂 Two-Screen TUI Workflow (`ratatui` + `crossterm`)**:
  - **Launcher Menu**: Resume your last directory, launch saved routes with `[1]`–`[9]`, add/delete custom paths, or pick a folder with the built-in interactive filesystem browser.
  - **Main Player**: Split-pane playlist, now-playing details, transport indicator, spinning vinyl, and live spectrum analyzer.
- **🔀 Playback Modes & Live Search**: Fisher-Yates shuffle queue (`s`), `ALL` / `ONE` / `OFF` repeat cycling (`r`), and instant case-insensitive filtering by filename, artist, or title (`/`).
- **⚙️ TOML Config & Terminal Safety**: Automatically persists routes, last session, volume, shuffle, and repeat preferences to `~/.config/rmp/config.toml`, with an RAII `TerminalGuard` and panic hook that restore raw mode and leave the alternate screen cleanly on exit or crash.

---

## Installation & Quick Start

### Prerequisites

- **Rust & Cargo** (2021 edition or newer) — install via [rustup.rs](https://rustup.rs/)
- **OS Audio Dependencies**:
  - **Linux**: ALSA development headers and `pkg-config` are required to compile `cpal`/`alsa-sys`:
    ```bash
    # Debian / Ubuntu
    sudo apt install libasound2-dev pkg-config

    # Fedora
    sudo dnf install alsa-lib-devel pkg-config

    # Arch Linux
    sudo pacman -S alsa-lib pkgconf
    ```
  - **Windows**: No extra dependencies (uses built-in WASAPI).
  - **macOS**: No extra dependencies (uses built-in CoreAudio).

### Build & Run

**Install directly from Git:**

```bash
cargo install --git https://github.com/Alirezadn7/rmp
```

**Or clone and build locally:**

```bash
git clone https://github.com/Alirezadn7/rmp.git
cd rmp
cargo run --release
```

You can also skip the launcher menu and open a music folder directly from the CLI:

```bash
cargo run --release -- "/path/to/your/music"
```

---

## Keybindings & Controls

### Launcher Menu

| Key | Action |
| :--- | :--- |
| `↑` / `↓` or `k` / `j` | Navigate launcher entries |
| `Space` | Select / mark active route (`●`) without opening |
| `Enter` | Confirm & open highlighted route (or modal action) |
| `1` – `9` | Instant jump to & open numbered route |
| `b` / `B` | Open interactive folder browser modal |
| `a` / `A` | Open add custom route modal |
| `d` / `D` | Delete highlighted custom route |
| `q` / `Esc` | Quit `rmp` |

<details>
<summary><strong>Modal Controls (Folder Browser & Add Custom Path)</strong></summary>

**Folder Browser (`[B]`):**
| Key | Action |
| :--- | :--- |
| `↑` / `↓` or `k` / `j` | Move cursor through folders |
| `Enter` | Enter highlighted subfolder / confirm entry |
| `Backspace` / `u` | Go up to parent directory |
| `Space` / `o` | Confirm & load current directory immediately |
| `Esc` | Close browser modal |

**Add Custom Path (`[A]`):**
| Key | Action |
| :--- | :--- |
| `Enter` | Validate directory and add route to config |
| `Backspace` | Delete last character |
| `Esc` | Cancel and close modal |

</details>

### Music Player

| Key | Action |
| :--- | :--- |
| `↑` / `↓` or `k` / `j` | Navigate playlist |
| `Enter` | Play highlighted track |
| `Space` | Toggle Play / Pause (or start selected track if stopped) |
| `n` | Next track |
| `p` / `b` | Previous track |
| `→` / `←` | Seek forward / backward 5 seconds |
| `+` (or `=`) / `-` | Volume up / down (5% steps, persisted to config) |
| `m` / `M` | Toggle mute |
| `s` / `S` | Toggle Shuffle (`ON` / `OFF`) |
| `r` / `R` | Cycle Repeat mode (`ALL` → `ONE` → `OFF`) |
| `/` | Open live playlist search / filter |
| `Esc` | Return to Launcher Menu |
| `q` / `Q` | Quit `rmp` |

**Search Mode (`/`):** Type to filter by filename, artist, or title, use `↑` / `↓` to navigate matches, press `Enter` to play the selected match, or press `Esc` to clear the filter.

---

## Configuration

Settings and saved library routes are stored at `~/.config/rmp/config.toml` (`%USERPROFILE%\.config\rmp\config.toml` on Windows). The file is created automatically on first run.

```toml
# rmp configuration file

last_session = "~/Music"
volume = 85
shuffle = false
repeat = "ALL" # Options: "ALL", "ONE", "OFF"

routes = [
    "~/Music",
    "~/Downloads/audio",
    "/media/external/tracks",
]
```

- Paths starting with `~/` (or `~\` on Windows) automatically expand to your home directory.
- Updating volume, shuffle, repeat, routes, or opening a directory automatically writes changes back to `config.toml`.

---

## Roadmap

- [x] `rodio` playback engine with seek, volume, mute, shuffle, and repeat (`ALL` / `ONE` / `OFF`)
- [x] Non-blocking `AudioTap` + real-time 1024-point FFT spectrum analyzer (`rustfft`)
- [x] 4-phase pixel-art spinning vinyl disc widget
- [x] Launcher menu with saved routes, CLI path support, and interactive folder browser
- [x] Live playlist search/filter and persistent TOML configuration
- [x] RAII terminal guard and panic hook cleanup
- [ ] Embedded tag parsing (`ID3` / `FLAC` / `Vorbis` metadata & exact track durations)
- [ ] Enable `symphonia` AAC/MP4 features for full `.m4a` playback & graceful skip-on-error handling
- [ ] Channel-aware mono downmix and dynamic sample-rate calibration in the FFT visualizer
- [ ] Status/error toast notifications, `Stop` keybinding, and persistent play-next queue
- [ ] Mouse interaction (click-to-play, click-to-seek, scroll wheel)
- [ ] Multiple visualizer modes (Oscilloscope / Stereo VU-Meter) and customizable themes

---

## License

Distributed under the [MIT License](LICENSE).
