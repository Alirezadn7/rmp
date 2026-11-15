# rmp

A complete, high-performance, retro Terminal User Interface (TUI) music player written in Rust.

---

## Features

- **Two-Screen Architecture**:
  - **Screen 1: Launcher Menu (`AppScreen::Menu`)**: Centered ASCII art banner, recent session resume, customizable numbered route shortcuts (`[1]`..`[9]`), interactive directory browser (`[B]`), and custom route manager (`[A]`).
  - **Screen 2: Music Player (`AppScreen::Player`)**: Dark theme with cyan/blue accents matching retro audio gear.
- **Audio Playback Engine**: Powered by `rodio` with support for `.mp3`, `.flac`, `.wav`, `.ogg`, and `.m4a`.
- **Playback Modes**:
  - **Shuffle Mode**: Randomized track progression using Fisher-Yates shuffle queue (`[s]`).
  - **Repeat Mode**: Toggle between `ALL`, `ONE`, and `OFF` (`[r]`).
  - **Seeking**: Fast forward (`[→]`) and rewind (`[←]`) by 5 seconds.
  - **Mute Control**: Instant audio muting with volume preservation (`[m]`).
- **Interactive Visuals**:
  - **Pixel-Art Spinning Vinyl Disc**: Animated 4-phase rotating vinyl record with groove reflections and center spindle label.
  - **Live Spectrum Visualizer**: Multi-band equalizer with dynamic frequency synthesis and floating peak hold caps.
- **Playlist Search & Filter**: Real-time interactive playlist searching (`[/]`).
- **Config Persistence**: Automatically persists volume, shuffle, repeat, last played directory, and saved routes in `~/.config/rmp/config.toml`.
- **Terminal Crash Protection**: Custom panic hook and RAII guard to safely restore raw mode and alternate screens.

---

## Getting Started

### Prerequisites

- [Rust & Cargo](https://rustup.rs/) (edition 2021 or newer)

### Installation & Run

1. **Run directly:**
   ```bash
   cargo run
   ```
   *(Or pass a music path directly: `cargo run -- "C:\path\to\music"`)*

2. **Run optimized release executable:**
   ```powershell
   .\target\release\rmp.exe
   ```

---

## Keyboard Controls

### Launcher Menu Screen
| Key | Action |
| :--- | :--- |
| `↑` / `↓` or `k` / `j` | Navigate launcher options |
| `Space` | **Select** path (marks active route with `●` radio circle) |
| `Enter` | **Confirm** & Open selected route |
| `1` – `9` | Instant jump & open corresponding numbered route |
| `b` or `B` | Open interactive file system folder browser |
| `a` or `A` | Add new custom music directory path |
| `d` or `D` | Delete highlighted custom route |
| `q` or `Esc` | Quit application |

### Music Player Screen
| Key | Action |
| :--- | :--- |
| `Space` | Toggle Play / Pause |
| `Enter` | Play highlighted track in playlist |
| `n` | Next track (respects Shuffle / Repeat) |
| `p` or `b` | Previous track |
| `s` | Toggle **Shuffle** (ON / OFF) |
| `r` | Cycle **Repeat** (`ALL` → `ONE` → `OFF`) |
| `m` | Toggle **Mute** (YES / NO) |
| `→` / `←` | Seek forward / backward 5 seconds |
| `+` / `-` | Volume Up / Down (or `=` / `-`) |
| `/` | Live Playlist Search & Filter |
| `Esc` | Return to Launcher Menu |
| `q` or `Q` | Quit application |

---

## Configuration

Settings are saved in `~/.config/rmp/config.toml`:

```toml
# rmp configuration file
last_session = "C:\\Users\\Asus\\Music"
volume = 85
shuffle = true
repeat = "ALL"

routes = [
    "~/Music",
    "~/Downloads/audio",
    "/media/external/tracks",
]
```

---

## License

This project is licensed under the [MIT License](LICENSE).
