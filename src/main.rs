//! rmp - Lightweight, Fast, and Clean Music Player in Rust
//! Stage 2: Interactive Terminal User Interface (TUI) via Ratatui & Crossterm

mod player;
mod scanner;

use std::io::{self, stdout};
use std::path::PathBuf;
use std::time::Duration;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use player::Player;
use scanner::Track;

/// RAII Guard ensuring terminal raw mode and alternate screen are restored on exit or panic.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

/// Application State
struct App {
    tracks: Vec<Track>,
    list_state: ListState,
    current_track_index: Option<usize>,
    player: Player,
    should_quit: bool,
    status_message: String,
}

impl App {
    fn new(tracks: Vec<Track>, player: Player) -> Self {
        let mut list_state = ListState::default();
        if !tracks.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            tracks,
            list_state,
            current_track_index: None,
            player,
            should_quit: false,
            status_message: "Ready. Select a track on the right and press [Enter] to play.".to_string(),
        }
    }

    fn select_next(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.tracks.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_previous(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tracks.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn play_selected(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            self.play_index(selected);
        }
    }

    fn play_index(&mut self, index: usize) {
        if index >= self.tracks.len() {
            return;
        }

        let track = &self.tracks[index];
        match self.player.play(&track.path) {
            Ok(_) => {
                self.current_track_index = Some(index);
                self.status_message = format!("Playing: {}", track.file_name);
            }
            Err(err) => {
                self.status_message = format!("Error playing {}: {:#}", track.file_name, err);
            }
        }
    }

    fn toggle_pause(&mut self) {
        if self.current_track_index.is_some() {
            let is_paused = self.player.toggle_pause();
            if is_paused {
                self.status_message = "Playback Paused ⏸".to_string();
            } else {
                self.status_message = "Playback Resumed ▶".to_string();
            }
        } else {
            self.status_message = "No track playing. Select a track and press [Enter].".to_string();
        }
    }

    fn play_next_track(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let next_idx = match self.current_track_index {
            Some(idx) => (idx + 1) % self.tracks.len(),
            None => self.list_state.selected().unwrap_or(0),
        };
        self.list_state.select(Some(next_idx));
        self.play_index(next_idx);
    }

    fn play_prev_track(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let prev_idx = match self.current_track_index {
            Some(idx) => {
                if idx == 0 {
                    self.tracks.len() - 1
                } else {
                    idx - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(prev_idx));
        self.play_index(prev_idx);
    }

    fn stop_playback(&mut self) {
        self.player.stop();
        self.current_track_index = None;
        self.status_message = "Playback stopped ⏹".to_string();
    }
}

fn main() -> Result<()> {
    // 1. Determine target directory from CLI argument or default to current directory.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let target_dir = if args.is_empty() {
        PathBuf::from(".")
    } else {
        let direct = PathBuf::from(&args[0]);
        if direct.exists() {
            direct
        } else {
            let joined = PathBuf::from(args.join(" "));
            if joined.exists() {
                joined
            } else {
                joined
            }
        }
    };

    // 2. Scan directory for audio files
    let tracks = scanner::scan_directory(&target_dir)
        .with_context(|| format!("Failed to scan directory: {}", target_dir.display()))?;

    if tracks.is_empty() {
        eprintln!("\n⚠️  No supported audio files (.mp3, .flac, .wav, .ogg) found in: {}", target_dir.display());
        eprintln!("Run rmp pointing to your music directory:");
        eprintln!("  cargo run -- <path_to_music_folder>\n");
        return Ok(());
    }

    // 3. Initialize Audio Engine
    let player = Player::new().context("Failed to initialize audio engine")?;

    // 4. Setup Terminal with RAII Guard
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
    let _guard = TerminalGuard;

    // Set panic hook to ensure terminal restoration on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
        original_hook(panic_info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(tracks, player);

    // 5. Main TUI Event Loop
    while !app.should_quit {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        // Quit
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.should_quit = true;
                        }

                        // Navigation in song list (Right Pane)
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.select_next();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.select_previous();
                        }

                        // Play selected track
                        KeyCode::Enter => {
                            app.play_selected();
                        }

                        // Toggle Pause / Resume
                        KeyCode::Char(' ') => {
                            app.toggle_pause();
                        }

                        // Next / Previous Track
                        KeyCode::Char('n') => {
                            app.play_next_track();
                        }
                        KeyCode::Char('b') => {
                            app.play_prev_track();
                        }

                        // Volume Control (+ / - or Right / Left)
                        KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Right => {
                            app.player.volume_up();
                            app.status_message = format!("Volume: {:.0}%", app.player.volume() * 100.0);
                        }
                        KeyCode::Char('-') | KeyCode::Left => {
                            app.player.volume_down();
                            app.status_message = format!("Volume: {:.0}%", app.player.volume() * 100.0);
                        }

                        // Stop
                        KeyCode::Char('s') => {
                            app.stop_playback();
                        }

                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

/// Render the TUI layout:
/// - Top: Title / App Banner
/// - Center: Split layout (Left: Details & Controls | Right: Song List)
/// - Bottom: Playback Controller & Volume Bar
fn ui(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Vertical layout: Top (3), Main Body (Min), Bottom Controller (7)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(7),
        ])
        .split(size);

    // 1. Header Bar
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" 🎵 RMP ", Style::default().fg(Color::Black).bg(Color::Cyan).bold()),
        Span::raw(" Lightweight Rust Music Player "),
        Span::styled(format!("({} tracks found)", app.tracks.len()), Style::default().fg(Color::DarkGray)),
    ]))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(header, chunks[0]);

    // 2. Middle Body: Horizontal split (Left: Info & Keys | Right: Song List)
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40), // Left: Details & Controls
            Constraint::Percentage(60), // Right: Song List (as requested)
        ])
        .split(chunks[1]);

    render_left_pane(f, app, body_chunks[0]);
    render_right_pane(f, app, body_chunks[1]);

    // 3. Bottom Pane: Playback Controller & Status
    render_bottom_pane(f, app, chunks[2]);
}

/// Left Pane: Now Playing details, Metadata & Keyboard Shortcuts
fn render_left_pane(f: &mut Frame, app: &App, area: Rect) {
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // Track Details
            Constraint::Min(6),    // Keybindings Help
        ])
        .split(area);

    // Track Details Block
    let (track_title, track_format, track_size) = match app.current_track_index {
        Some(idx) => {
            let t = &app.tracks[idx];
            (t.file_name.clone(), t.format_details(), t.path.display().to_string())
        }
        None => ("None (Stopped)".to_string(), "-".to_string(), "-".to_string()),
    };

    let details_text = vec![
        Line::from(vec![
            Span::styled("Track:  ", Style::default().fg(Color::Yellow).bold()),
            Span::styled(track_title, Style::default().fg(Color::White).bold()),
        ]),
        Line::from(vec![
            Span::styled("Format: ", Style::default().fg(Color::Yellow)),
            Span::styled(track_format, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Path:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(track_size, Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let details_widget = Paragraph::new(details_text)
        .block(
            Block::default()
                .title(" ℹ️ Track Info ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(details_widget, inner_chunks[0]);

    // Shortcuts Block
    let help_text = vec![
        Line::from(vec![Span::styled("  [Enter]    ", Style::default().fg(Color::Green).bold()), Span::raw("Play Selected Track")]),
        Line::from(vec![Span::styled("  [Space]    ", Style::default().fg(Color::Cyan).bold()), Span::raw("Toggle Pause / Resume")]),
        Line::from(vec![Span::styled("  [↑ / k]    ", Style::default().fg(Color::Yellow)), Span::raw("Move Up in List")]),
        Line::from(vec![Span::styled("  [↓ / j]    ", Style::default().fg(Color::Yellow)), Span::raw("Move Down in List")]),
        Line::from(vec![Span::styled("  [n / b]    ", Style::default().fg(Color::Magenta)), Span::raw("Next / Prev Track")]),
        Line::from(vec![Span::styled("  [+ / -]    ", Style::default().fg(Color::Blue)), Span::raw("Volume Up / Down")]),
        Line::from(vec![Span::styled("  [s]        ", Style::default().fg(Color::Red)), Span::raw("Stop Playback")]),
        Line::from(vec![Span::styled("  [q / Esc]  ", Style::default().fg(Color::Gray)), Span::raw("Quit Application")]),
    ];

    let help_widget = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" ⌨️ Controls ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Blue)),
        );
    f.render_widget(help_widget, inner_chunks[1]);
}

/// Right Pane: The interactive, scrollable song list
fn render_right_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_current = app.current_track_index == Some(i);

            let (icon, color) = if is_current {
                if app.player.is_paused() {
                    ("⏸ ", Color::Yellow)
                } else {
                    ("▶ ", Color::Green)
                }
            } else {
                ("  ", Color::White)
            };

            let line = Line::from(vec![
                Span::styled(icon, Style::default().fg(color).bold()),
                Span::styled(format!("{:>2}. ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(&track.file_name, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(format!("({})", track.format_details()), Style::default().fg(Color::DarkGray)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list_title = format!(" 📜 Track List ({} Songs) - Right Pane ", app.tracks.len());

    let list = List::new(items)
        .block(
            Block::default()
                .title(list_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 50, 75))
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, area, &mut app.list_state);
}

/// Bottom Pane: Playback controls, status, and volume meter
fn render_bottom_pane(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" 🎛️ Playback Controller (Bottom) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Now playing status line
            Constraint::Length(3), // Volume Gauge & Info
        ])
        .split(inner);

    // Status / Current Playback Line
    let status_icon = if app.current_track_index.is_none() {
        "⏹ Stopped"
    } else if app.player.is_paused() {
        "⏸ Paused"
    } else {
        "▶ Playing"
    };

    let status_style = if app.player.is_paused() {
        Style::default().fg(Color::Yellow).bold()
    } else if app.current_track_index.is_some() {
        Style::default().fg(Color::Green).bold()
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let current_info = Line::from(vec![
        Span::styled(format!(" [{}] ", status_icon), status_style),
        Span::styled(&app.status_message, Style::default().fg(Color::White).bold()),
    ]);
    f.render_widget(Paragraph::new(current_info), rows[0]);

    // Volume Gauge Row (Split horizontally between gauge and tip)
    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Volume Gauge
            Constraint::Percentage(50), // Quick hint
        ])
        .split(rows[1]);

    let vol_pct = (app.player.volume() * 100.0).round() as u16;
    let volume_gauge = Gauge::default()
        .block(Block::default().title("Volume [+/-]").borders(Borders::NONE))
        .gauge_style(Style::default().fg(Color::Magenta).bg(Color::Rgb(30, 30, 40)))
        .percent(vol_pct)
        .label(format!("{}%", vol_pct));

    f.render_widget(volume_gauge, bottom_cols[0]);

    let hint_text = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Controls: ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter]", Style::default().fg(Color::Green)),
            Span::raw(" Play | "),
            Span::styled("[Space]", Style::default().fg(Color::Cyan)),
            Span::raw(" Pause | "),
            Span::styled("[q]", Style::default().fg(Color::Red)),
            Span::raw(" Quit"),
        ]),
    ])
    .alignment(Alignment::Right);

    f.render_widget(hint_text, bottom_cols[1]);
}
