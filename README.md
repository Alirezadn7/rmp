# rmp

A lightweight, fast, and minimalist Terminal User Interface (TUI) music player written in Rust.

---

## Features

- **Interactive TUI**: Built with [Ratatui](https://github.com/ratatui-org/ratatui) and [Crossterm](https://github.com/crossterm-rs/crossterm).
- **Audio Engine**: Powered by [Rodio](https://github.com/RustAudio/rodio) with support for `.mp3`, `.flac`, `.wav`, and `.ogg`.
- **Recursive Scanner**: Automatically scans folders and subfolders for audio files.
- **Volume & Playback Controls**: Intuitive volume gauge and playback management.
- **Safe Terminal Handling**: RAII guard & panic hook ensure your terminal is cleanly restored upon exit or unexpected crash.

---

## Getting Started

### Prerequisites

- [Rust & Cargo](https://rustup.rs/) (edition 2021 or newer)

### Installation & Run

1. **Clone the repository:**
   ```bash
   git clone https://github.com/<your-username>/rmp.git
   cd rmp
   ```

2. **Run pointing to your music directory:**
   ```bash
   cargo run -- "C:\path\to\your\music"
   ```
   *(Or simply run `cargo run` if you are already inside a folder with music files).*

3. **Build optimized release binary:**
   ```bash
   cargo build --release
   ```
   The compiled executable will be located at `target/release/rmp`.

---

## Keyboard Controls

| Key | Action |
| :--- | :--- |
| `Enter` | Play selected track |
| `Space` | Toggle Pause / Resume |
| `↑` / `↓` or `k` / `j` | Navigate track list |
| `n` | Next track |
| `b` | Previous track |
| `+` / `-` or `→` / `←` | Increase / Decrease volume |
| `s` | Stop playback |
| `q` or `Esc` | Quit application |

---

## License

This project is licensed under the [MIT License](LICENSE).
