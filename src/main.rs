//! rmp - Lightweight, Fast, and Clean Terminal User Interface Music Player in Rust.
//!
//! Featuring two screens:
//! 1. `AppScreen::Menu`: Retro ASCII-art Launcher & Directory Selection.
//! 2. `AppScreen::Player`: High-performance dark-cyan TUI with Playlist, Now Playing,
//!    animated spinning vinyl pixel art, and live spectrum visualizer.

mod config;
mod player;
mod scanner;
mod vinyl;
mod visualizer;

use std::fs;
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use config::{AppConfig, RepeatMode};
use player::Player;
use scanner::{format_duration_secs, Track};
use vinyl::VinylRecord;
use visualizer::{SpectrumState, SpectrumVisualizer};

/// RAII Guard ensuring terminal raw mode and alternate screen are restored on exit or panic.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

/// Active screen view
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppScreen {
    Menu,
    Player,
}

/// Modals for Menu screen
#[derive(Debug, Clone)]
enum MenuModal {
    None,
    Browse(BrowseState),
    AddPath(String),
}

/// State for the directory browser modal
#[derive(Debug, Clone)]
struct BrowseState {
    current_dir: PathBuf,
    entries: Vec<PathBuf>,
    selected: usize,
}

impl BrowseState {
    fn new(start_dir: PathBuf) -> Self {
        let mut state = Self {
            current_dir: start_dir,
            entries: Vec::new(),
            selected: 0,
        };
        state.refresh();
        state
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.selected = 0;

        if let Ok(read_dir) = fs::read_dir(&self.current_dir) {
            let mut dirs = Vec::new();
            for entry in read_dir.flatten() {
                if let Ok(ft) = entry.file_type() {
                    if ft.is_dir() {
                        dirs.push(entry.path());
                    }
                }
            }
            dirs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            self.entries = dirs;
        }
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
            self.refresh();
        }
    }

    fn enter_selected(&mut self) -> Option<PathBuf> {
        if self.selected == 0 {
            // Confirm & Open current folder
            Some(self.current_dir.clone())
        } else if self.selected == 1 {
            self.go_up();
            None
        } else if let Some(target) = self.entries.get(self.selected - 2) {
            self.current_dir = target.clone();
            self.refresh();
            None
        } else {
            None
        }
    }
}

/// Application state
struct App {
    screen: AppScreen,
    config: AppConfig,
    player: Player,

    // Menu screen state
    menu_selected: usize,
    chosen_route: usize,
    menu_modal: MenuModal,

    // Player screen state
    tracks: Vec<Track>,
    playlist_state: ListState,
    current_track_index: Option<usize>,
    shuffle_indices: Vec<usize>,
    shuffle_cursor: usize,
    repeat: RepeatMode,
    shuffle: bool,
    search_query: Option<String>,
    filtered_indices: Vec<usize>,

    // Animation & Physics
    frame_counter: usize,
    spectrum: SpectrumState,
    rng_seed: u64,
    should_quit: bool,
}

impl App {
    fn new(config: AppConfig, player: Player) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xCAFE_BABE);

        let repeat = config.repeat;
        let shuffle = config.shuffle;

        Self {
            screen: AppScreen::Menu,
            config,
            player,
            menu_selected: 0,
            chosen_route: 0,
            menu_modal: MenuModal::None,
            tracks: Vec::new(),
            playlist_state: ListState::default(),
            current_track_index: None,
            shuffle_indices: Vec::new(),
            shuffle_cursor: 0,
            repeat,
            shuffle,
            search_query: None,
            filtered_indices: Vec::new(),
            frame_counter: 0,
            spectrum: SpectrumState::new(),
            rng_seed: seed,
            should_quit: false,
        }
    }

    /// Load tracks from a folder and transition to Player screen
    fn load_directory(&mut self, dir_path: &Path) -> Result<()> {
        let tracks = scanner::scan_directory(dir_path)
            .with_context(|| format!("Failed to scan directory: {}", dir_path.display()))?;

        if tracks.is_empty() {
            // If empty, return error or handle gracefully
            return Err(anyhow::anyhow!(
                "No supported audio files found in {}",
                dir_path.display()
            ));
        }

        self.tracks = tracks;
        self.filtered_indices = (0..self.tracks.len()).collect();
        self.playlist_state.select(Some(0));
        self.current_track_index = None;
        self.rebuild_shuffle_queue();

        // Update config last session
        let path_str = dir_path.display().to_string();
        self.config.last_session = Some(path_str);
        let _ = self.config.save();

        self.screen = AppScreen::Player;
        Ok(())
    }

    fn rebuild_shuffle_queue(&mut self) {
        self.shuffle_indices = (0..self.tracks.len()).collect();
        // Fisher-Yates shuffle
        let n = self.shuffle_indices.len();
        if n > 1 {
            for i in (1..n).rev() {
                let j = (self.next_rand() as usize) % (i + 1);
                self.shuffle_indices.swap(i, j);
            }
        }
        self.shuffle_cursor = 0;
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng_seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_seed = x;
        x
    }

    fn play_track(&mut self, index: usize) {
        if index >= self.tracks.len() {
            return;
        }

        let track = &self.tracks[index];
        match self.player.play(&track.path) {
            Ok(dur) => {
                self.current_track_index = Some(index);
                if let Some(d) = dur {
                    self.tracks[index].duration = Some(d);
                }
            }
            Err(_) => {
                // If playback failed, try next
            }
        }
    }

    fn play_selected(&mut self) {
        if let Some(sel) = self.playlist_state.selected() {
            let actual_idx = if let Some(idx) = self.filtered_indices.get(sel) {
                *idx
            } else {
                sel
            };
            self.play_track(actual_idx);
        }
    }

    fn play_next(&mut self) {
        if self.tracks.is_empty() {
            return;
        }

        if self.shuffle {
            self.shuffle_cursor = (self.shuffle_cursor + 1) % self.tracks.len();
            if self.shuffle_cursor == 0 {
                self.rebuild_shuffle_queue();
            }
            let next_idx = self.shuffle_indices[self.shuffle_cursor];
            self.select_and_play(next_idx);
        } else {
            let next_idx = match self.current_track_index {
                Some(idx) => (idx + 1) % self.tracks.len(),
                None => 0,
            };
            self.select_and_play(next_idx);
        }
    }

    fn play_prev(&mut self) {
        if self.tracks.is_empty() {
            return;
        }

        if self.shuffle {
            if self.shuffle_cursor == 0 {
                self.shuffle_cursor = self.tracks.len().saturating_sub(1);
            } else {
                self.shuffle_cursor -= 1;
            }
            let prev_idx = self.shuffle_indices[self.shuffle_cursor];
            self.select_and_play(prev_idx);
        } else {
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
            self.select_and_play(prev_idx);
        }
    }

    fn select_and_play(&mut self, track_idx: usize) {
        if let Some(pos) = self.filtered_indices.iter().position(|&i| i == track_idx) {
            self.playlist_state.select(Some(pos));
        }
        self.play_track(track_idx);
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.config.shuffle = self.shuffle;
        let _ = self.config.save();
        if self.shuffle {
            self.rebuild_shuffle_queue();
        }
    }

    fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.next();
        self.config.repeat = self.repeat;
        let _ = self.config.save();
    }

    fn update_search_filter(&mut self) {
        if let Some(ref q) = self.search_query {
            let q_lower = q.to_lowercase();
            self.filtered_indices = self
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    t.file_name.to_lowercase().contains(&q_lower)
                        || t.artist.to_lowercase().contains(&q_lower)
                        || t.title.to_lowercase().contains(&q_lower)
                })
                .map(|(i, _)| i)
                .collect();
        } else {
            self.filtered_indices = (0..self.tracks.len()).collect();
        }

        if !self.filtered_indices.is_empty() {
            self.playlist_state.select(Some(0));
        } else {
            self.playlist_state.select(None);
        }
    }

    /// Check if current track ended and handle repeat/shuffle auto-progression
    fn check_track_completion(&mut self) {
        if self.current_track_index.is_some() && !self.player.is_paused() && self.player.is_empty() {
            match self.repeat {
                RepeatMode::One => {
                    if let Some(idx) = self.current_track_index {
                        self.play_track(idx);
                    }
                }
                RepeatMode::All => {
                    self.play_next();
                }
                RepeatMode::Off => {
                    if let Some(idx) = self.current_track_index {
                        if idx + 1 < self.tracks.len() {
                            self.play_next();
                        } else {
                            self.player.stop();
                            self.current_track_index = None;
                        }
                    }
                }
            }
        }
    }
}

fn main() -> Result<()> {
    // 1. Load Configuration
    let config = AppConfig::load();

    // 2. Initialize Player
    let player = Player::new().context("Failed to initialize audio output engine")?;

    // 3. Setup Terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
    let _guard = TerminalGuard;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
        original_hook(panic_info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config.clone(), player);

    // If target directory was passed on CLI arguments, directly open it
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        let target = PathBuf::from(args.join(" "));
        if target.exists() {
            let _ = app.load_directory(&target);
        }
    }

    // Main Event Loop with Dynamic Frame Rate / CPU Optimization
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui(f, &mut app))?;

        let is_playing = app.screen == AppScreen::Player
            && app.current_track_index.is_some()
            && !app.player.is_paused();

        // 16ms (~60 FPS) when playing for fluid FFT visualizer motion; 200ms when idle/menu (<1% CPU)
        let tick_rate = if is_playing {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(200)
        };

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_millis(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key_event(&mut app, key.code)?;
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.frame_counter = app.frame_counter.wrapping_add(1);
            let active_playing = app.current_track_index.is_some() && !app.player.is_paused();
            let shared_audio = app.player.audio_buffer();
            app.spectrum.update(active_playing, &shared_audio);
            app.check_track_completion();
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn handle_key_event(app: &mut App, code: KeyCode) -> Result<()> {
    match app.screen {
        AppScreen::Menu => handle_menu_key(app, code),
        AppScreen::Player => handle_player_key(app, code),
    }
}

// -----------------------------------------------------------------------------
// Screen 1: Launcher Menu Key Handling
// -----------------------------------------------------------------------------
fn handle_menu_key(app: &mut App, code: KeyCode) -> Result<()> {
    // Handle modal active in Menu
    match &mut app.menu_modal {
        MenuModal::Browse(ref mut browse) => match code {
            KeyCode::Esc => {
                app.menu_modal = MenuModal::None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if browse.selected > 0 {
                    browse.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = browse.entries.len() + 1; // 0 = confirm current, 1 = parent, 2.. = entries
                if browse.selected < max {
                    browse.selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(confirmed_dir) = browse.enter_selected() {
                    app.menu_modal = MenuModal::None;
                    let _ = app.load_directory(&confirmed_dir);
                }
            }
            KeyCode::Backspace | KeyCode::Char('u') => {
                browse.go_up();
            }
            KeyCode::Char(' ') | KeyCode::Char('o') => {
                let dir = browse.current_dir.clone();
                app.menu_modal = MenuModal::None;
                let _ = app.load_directory(&dir);
            }
            _ => {}
        },
        MenuModal::AddPath(ref mut input) => match code {
            KeyCode::Esc => {
                app.menu_modal = MenuModal::None;
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) => {
                input.push(c);
            }
            KeyCode::Enter => {
                let trimmed = input.trim().to_string();
                if !trimmed.is_empty() {
                    let resolved = config::resolve_path(&trimmed);
                    if resolved.exists() && resolved.is_dir() {
                        app.config.routes.push(trimmed);
                        let _ = app.config.save();
                    }
                }
                app.menu_modal = MenuModal::None;
            }
            _ => {}
        },
        MenuModal::None => {
            let total_items = 1 + app.config.routes.len() + 2; // [Resume] + routes + [Browse] + [Add]

            match code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    app.should_quit = true;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.menu_selected == 0 {
                        app.menu_selected = total_items.saturating_sub(1);
                    } else {
                        app.menu_selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.menu_selected = (app.menu_selected + 1) % total_items;
                }
                // [Space] = SELECT path (marks active route with ● without opening)
                KeyCode::Char(' ') => {
                    if app.menu_selected <= app.config.routes.len() {
                        app.chosen_route = app.menu_selected;
                    }
                }
                // Direct numeric shortcuts [1] to [9]
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    let route_idx = (c as usize) - ('1' as usize);
                    if let Some(r) = app.config.routes.get(route_idx) {
                        app.chosen_route = route_idx + 1;
                        let path = config::resolve_path(r);
                        let _ = app.load_directory(&path);
                    }
                }
                KeyCode::Char('b') | KeyCode::Char('B') => {
                    let start = config::home_dir().unwrap_or_else(|| PathBuf::from("."));
                    app.menu_modal = MenuModal::Browse(BrowseState::new(start));
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    app.menu_modal = MenuModal::AddPath(String::new());
                }
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    // Delete highlighted custom route
                    if app.menu_selected > 0 && app.menu_selected <= app.config.routes.len() {
                        let idx = app.menu_selected - 1;
                        app.config.routes.remove(idx);
                        let _ = app.config.save();
                        app.menu_selected = app.menu_selected.min(total_items.saturating_sub(2));
                        if app.chosen_route > idx + 1 {
                            app.chosen_route -= 1;
                        } else if app.chosen_route == idx + 1 {
                            app.chosen_route = 0;
                        }
                    }
                }
                // [Enter] = CONFIRM & Open
                KeyCode::Enter => {
                    let routes_len = app.config.routes.len();
                    if app.menu_selected == routes_len + 1 {
                        // [B] Browse File System...
                        let start = config::home_dir().unwrap_or_else(|| PathBuf::from("."));
                        app.menu_modal = MenuModal::Browse(BrowseState::new(start));
                    } else if app.menu_selected == routes_len + 2 {
                        // [A] Add New Custom Path...
                        app.menu_modal = MenuModal::AddPath(String::new());
                    } else {
                        // Confirm selected target
                        let target_idx = if app.menu_selected <= routes_len {
                            app.menu_selected
                        } else {
                            app.chosen_route
                        };
                        app.chosen_route = target_idx;

                        if target_idx == 0 {
                            // [Resume Last Session]
                            if let Some(ref last) = app.config.last_session {
                                let path = config::resolve_path(last);
                                let _ = app.load_directory(&path);
                            }
                        } else if target_idx <= routes_len {
                            let route_idx = target_idx - 1;
                            let r = &app.config.routes[route_idx];
                            let path = config::resolve_path(r);
                            let _ = app.load_directory(&path);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Screen 2: Main Player Key Handling
// -----------------------------------------------------------------------------
fn handle_player_key(app: &mut App, code: KeyCode) -> Result<()> {
    // If search mode is active
    if let Some(ref mut query) = app.search_query {
        match code {
            KeyCode::Esc => {
                app.search_query = None;
                app.update_search_filter();
            }
            KeyCode::Enter => {
                app.play_selected();
                app.search_query = None;
            }
            KeyCode::Backspace => {
                query.pop();
                let q_clone = query.clone();
                app.search_query = Some(q_clone);
                app.update_search_filter();
            }
            KeyCode::Char(c) => {
                query.push(c);
                let q_clone = query.clone();
                app.search_query = Some(q_clone);
                app.update_search_filter();
            }
            KeyCode::Up | KeyCode::Down => {
                // allow navigating playlist while typing search
                if code == KeyCode::Up {
                    select_prev_playlist(app);
                } else {
                    select_next_playlist(app);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    match code {
        // Exit to Menu or Quit
        KeyCode::Esc => {
            app.screen = AppScreen::Menu;
        }
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }

        // Playback controls
        KeyCode::Char(' ') => {
            if app.current_track_index.is_none() {
                app.play_selected();
            } else {
                app.player.toggle_pause();
            }
        }
        KeyCode::Enter => {
            app.play_selected();
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            app.toggle_shuffle();
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.cycle_repeat();
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            app.player.toggle_mute();
        }
        KeyCode::Char('n') => {
            app.play_next();
        }
        KeyCode::Char('p') | KeyCode::Char('b') => {
            app.play_prev();
        }

        // Navigation
        KeyCode::Up | KeyCode::Char('k') => {
            select_prev_playlist(app);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            select_next_playlist(app);
        }

        // Volume
        KeyCode::Char('+') | KeyCode::Char('=') => {
            app.player.volume_up();
            app.config.volume = (app.player.volume() * 100.0).round() as u8;
            let _ = app.config.save();
        }
        KeyCode::Char('-') => {
            app.player.volume_down();
            app.config.volume = (app.player.volume() * 100.0).round() as u8;
            let _ = app.config.save();
        }

        // Seek forward / backward
        KeyCode::Right => {
            app.player.seek_forward(5);
        }
        KeyCode::Left => {
            app.player.seek_backward(5);
        }

        // Search trigger
        KeyCode::Char('/') => {
            app.search_query = Some(String::new());
        }

        _ => {}
    }

    Ok(())
}

fn select_next_playlist(app: &mut App) {
    if app.filtered_indices.is_empty() {
        return;
    }
    let cur = app.playlist_state.selected().unwrap_or(0);
    let next = if cur + 1 >= app.filtered_indices.len() {
        0
    } else {
        cur + 1
    };
    app.playlist_state.select(Some(next));
}

fn select_prev_playlist(app: &mut App) {
    if app.filtered_indices.is_empty() {
        return;
    }
    let cur = app.playlist_state.selected().unwrap_or(0);
    let prev = if cur == 0 {
        app.filtered_indices.len() - 1
    } else {
        cur - 1
    };
    app.playlist_state.select(Some(prev));
}

// -----------------------------------------------------------------------------
// UI Rendering
// -----------------------------------------------------------------------------
fn ui(f: &mut Frame, app: &mut App) {
    match app.screen {
        AppScreen::Menu => render_menu_screen(f, app),
        AppScreen::Player => render_player_screen(f, app),
    }
}

// -----------------------------------------------------------------------------
// SCREEN 1: Menu & Launcher UI (Matches Reference Image 1)
// -----------------------------------------------------------------------------
fn render_menu_screen(f: &mut Frame, app: &App) {
    let size = f.area();

    // Center layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top margin
            Constraint::Length(8), // ASCII Banner
            Constraint::Length(2), // Subtitle
            Constraint::Min(10),   // Launcher Box
            Constraint::Length(3), // Bottom status bar
        ])
        .split(size);

    // ASCII Banner for "rmp"
    let banner = vec![
        Line::from(Span::styled(" ██████╗  ███╗   ███╗ ██████╗ ", Style::default().fg(Color::Cyan).bold())),
        Line::from(Span::styled(" ██╔══██╗ ████╗ ████║ ██╔══██╗", Style::default().fg(Color::Cyan).bold())),
        Line::from(Span::styled(" ██████╔╝ ██╔████╔██║ ██████╔╝", Style::default().fg(Color::Cyan).bold())),
        Line::from(Span::styled(" ██╔══██╗ ██║╚██╔╝██║ ██╔═══╝ ", Style::default().fg(Color::Cyan).bold())),
        Line::from(Span::styled(" ██║  ██║ ██║ ╚═╝ ██║ ██║     ", Style::default().fg(Color::Cyan).bold())),
        Line::from(Span::styled(" ╚═╝  ╚═╝ ╚═╝     ╚═╝ ╚═╝     ", Style::default().fg(Color::Cyan).bold())),
    ];
    let banner_widget = Paragraph::new(banner).alignment(Alignment::Center);
    f.render_widget(banner_widget, chunks[1]);

    // Subtitle
    let subtitle = Paragraph::new(Line::from(vec![
        Span::styled("♫ Rust Music Player v1.4.0 ♫", Style::default().fg(Color::Cyan).italic()),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(subtitle, chunks[2]);

    // Center Launcher Box (Fixed width ~52, centered horizontally)
    let center_h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(chunks[3]);

    let launcher_area = center_h[1];

    let mut items = Vec::new();

    // 0: Resume Last Session
    let last_str = app
        .config
        .last_session
        .as_deref()
        .map(|s| config::display_path(Path::new(s)))
        .unwrap_or_else(|| "~/Music".to_string());
    items.push(format!("[Resume Last Session] ({})", last_str));

    // Routes
    for (i, r) in app.config.routes.iter().enumerate() {
        items.push(format!("[{}] {}", i + 1, r));
    }

    // Browse and Add options
    items.push("[B] Browse File System...".to_string());
    items.push("[A] Add New Custom Path...".to_string());

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, text)| {
            let is_focused = app.menu_selected == idx;
            let is_chosen = app.chosen_route == idx && idx <= app.config.routes.len();

            let radio = if is_chosen { "● " } else { "○ " };
            let prefix = if is_focused { "> " } else { "  " };

            let mark = if idx <= app.config.routes.len() {
                radio
            } else {
                "  "
            };

            let line = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Yellow).bold()),
                Span::styled(
                    mark,
                    if is_chosen {
                        Style::default().fg(Color::Cyan).bold()
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(
                    text,
                    if is_focused {
                        Style::default().fg(Color::White).bold()
                    } else if is_chosen {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::Rgb(180, 210, 230))
                    },
                ),
            ]);

            if is_focused {
                ListItem::new(line).style(Style::default().bg(Color::Rgb(15, 65, 120)))
            } else {
                ListItem::new(line)
            }
        })
        .collect();

    let launcher_box = List::new(list_items)
        .block(
            Block::default()
                .title(" LAUNCHER / SELECT PATH ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(120, 180, 200))),
        );

    f.render_widget(launcher_box, launcher_area);

    // Bottom Bar (Separating [Space] Select and [Enter] Confirm)
    let bottom_text = Line::from(vec![
        Span::styled("[Space]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Select  "),
        Span::styled("[Enter]", Style::default().fg(Color::Green).bold()),
        Span::raw(" Confirm  "),
        Span::styled("[B]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Browse Folder  "),
        Span::styled("[A]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Add Route  "),
        Span::styled("[D]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Delete Route  "),
        Span::styled("[Q]", Style::default().fg(Color::Red).bold()),
        Span::raw(" Quit"),
    ]);

    let bottom_box = Paragraph::new(bottom_text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(bottom_box, chunks[4]);

    // Modals
    match &app.menu_modal {
        MenuModal::Browse(ref browse) => render_browse_modal(f, browse, size),
        MenuModal::AddPath(ref input) => render_add_path_modal(f, input, size),
        MenuModal::None => {}
    }
}

fn render_browse_modal(f: &mut Frame, browse: &BrowseState, area: Rect) {
    let popup_area = centered_rect(65, 70, area);
    f.render_widget(Clear, popup_area);

    let mut items = vec![
        ListItem::new(Line::from(vec![
            Span::styled("  ✓ [Confirm & Open Current Folder]", Style::default().fg(Color::Green).bold()),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  .. (Parent Folder)", Style::default().fg(Color::Yellow).bold()),
        ])),
    ];

    for entry in &browse.entries {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("folder");
        let line = Line::from(vec![
            Span::styled("  📁 ", Style::default().fg(Color::Cyan)),
            Span::raw(name),
        ]);
        items.push(ListItem::new(line));
    }

    let mut list_state = ListState::default();
    list_state.select(Some(browse.selected));

    let block = Block::default()
        .title(format!(" 📁 Browse: {} ([Space] Select | [Enter] Open/Confirm) ", browse.current_dir.display()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::Rgb(20, 80, 140)).fg(Color::White).bold())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, popup_area, &mut list_state);
}

fn render_add_path_modal(f: &mut Frame, input: &str, area: Rect) {
    let popup_area = centered_rect(50, 20, area);
    f.render_widget(Clear, popup_area);

    let text = vec![
        Line::from(Span::styled("Enter Directory Path:", Style::default().fg(Color::White).bold())),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!(" > {}_", input), Style::default().fg(Color::Cyan).bold()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("[Enter] Save & Add  |  [Esc] Cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let block = Block::default()
        .title(" ➕ Add Custom Path ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Paragraph::new(text).block(block), popup_area);
}

// -----------------------------------------------------------------------------
// SCREEN 2: Main Music Player UI (Matches Reference Image 2)
// -----------------------------------------------------------------------------
fn render_player_screen(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Vertical layout: Top Status Bar (1), Main Body (Min), Bottom Help (3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Status Bar
            Constraint::Min(12),   // Main 2-column layout
            Constraint::Length(3), // Bottom Controls & Help
        ])
        .split(size);

    // 1. TOP STATUS BAR (Matches Image 2 exactly)
    render_top_status_bar(f, app, chunks[0]);

    // 2. MAIN BODY (50% Left Playlist | 50% Right Player & Visualizer)
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left: Tracks & Playlist
            Constraint::Percentage(50), // Right: Now Playing & Live Visualizer
        ])
        .split(chunks[1]);

    // Left Column: Tracks & Playlist
    render_playlist_pane(f, app, body_cols[0]);

    // Right Column: Now Playing (Top) + Spectrum Visualizer (Bottom)
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(14), // Now Playing & Vinyl
            Constraint::Min(6),     // Live Spectrum Visualizer
        ])
        .split(body_cols[1]);

    render_now_playing_pane(f, app, right_rows[0]);
    render_spectrum_pane(f, app, right_rows[1]);

    // 3. BOTTOM CONTROLS & HELP BAR (Matches Image 2)
    render_player_bottom_bar(f, app, chunks[2]);
}

fn render_top_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let vol_pct = (app.player.volume() * 100.0).round() as u8;
    let muted_str = if app.player.is_muted() { "YES" } else { "NO" };
    let shuffle_str = if app.shuffle { "ON" } else { "OFF" };
    let repeat_str = app.repeat.as_str();

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Left: [VOL: XX%] [MUTED: NO]
            Constraint::Percentage(50), // Center: ♫ rmp - RUST MUSIC PLAYER v1.4.0 ♫
            Constraint::Percentage(25), // Right: [SHUFFLE: ON] [REPEAT: ALL]
        ])
        .split(area);

    // Left status
    let left_line = Line::from(vec![
        Span::styled(format!("[VOL: {}%] ", vol_pct), Style::default().fg(Color::Cyan).bold()),
        Span::styled(format!("[MUTED: {}]", muted_str), if app.player.is_muted() {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default().fg(Color::Cyan).bold()
        }),
    ]);
    f.render_widget(Paragraph::new(left_line).alignment(Alignment::Left), cols[0]);

    // Center title
    let center_line = Line::from(vec![
        Span::styled("♫ rmp - RUST MUSIC PLAYER v1.4.0 ♫", Style::default().fg(Color::Cyan).bold()),
    ]);
    f.render_widget(Paragraph::new(center_line).alignment(Alignment::Center), cols[1]);

    // Right status
    let right_line = Line::from(vec![
        Span::styled(format!("[SHUFFLE: {}] ", shuffle_str), if app.shuffle {
            Style::default().fg(Color::Cyan).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        }),
        Span::styled(format!("[REPEAT: {}]", repeat_str), match app.repeat {
            RepeatMode::Off => Style::default().fg(Color::DarkGray),
            RepeatMode::One => Style::default().fg(Color::Yellow).bold(),
            RepeatMode::All => Style::default().fg(Color::Cyan).bold(),
        }),
    ]);
    f.render_widget(Paragraph::new(right_line).alignment(Alignment::Right), cols[2]);
}

fn render_playlist_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let search_indicator = if let Some(ref q) = app.search_query {
        format!(" (Filter: \"{}\")", q)
    } else {
        String::new()
    };

    let title = format!(" TRACKS & PLAYLIST{} ", search_indicator);

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let inner_width = area.width.saturating_sub(4) as usize;

    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(display_idx, &track_idx)| {
            let track = &app.tracks[track_idx];
            let is_current = app.current_track_index == Some(track_idx);

            let prefix = if is_current { "▶ " } else { "  " };
            let num = format!("{:02}. ", display_idx + 1);

            let name = if track.artist != "Unknown Artist" {
                format!("{} - {}", track.title, track.artist)
            } else {
                track.file_name.clone()
            };

            let dur_str = track.formatted_duration();

            // Right-align duration
            let left_part_len = prefix.len() + num.len() + name.chars().count();
            let pad = if inner_width > left_part_len + dur_str.len() {
                " ".repeat(inner_width - left_part_len - dur_str.len())
            } else {
                " ".to_string()
            };

            let line = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan).bold()),
                Span::styled(num, Style::default().fg(Color::DarkGray)),
                Span::styled(name, if is_current {
                    Style::default().fg(Color::White).bold()
                } else {
                    Style::default().fg(Color::Rgb(210, 230, 245))
                }),
                Span::raw(pad),
                Span::styled(dur_str, Style::default().fg(Color::DarkGray)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(20, 85, 150))
                .fg(Color::White)
                .bold(),
        );

    f.render_stateful_widget(list, area, &mut app.playlist_state);
}

fn render_now_playing_pane(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" NOW PLAYING ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner horizontally: Left (Details & Progress) | Right (Vinyl Disc)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(24),   // Metadata & Controls
            Constraint::Length(18), // Vinyl Disc Widget
        ])
        .split(inner);

    let (artist, album, title, genre, format_str) = match app.current_track_index {
        Some(idx) => {
            let t = &app.tracks[idx];
            (&t.artist, &t.album, &t.title, &t.genre, &t.format_str)
        }
        None => (
            &"None (Stopped)".to_string(),
            &"-".to_string(),
            &"-".to_string(),
            &"-".to_string(),
            &"-".to_string(),
        ),
    };

    let elapsed = app.player.elapsed();
    let total = app.player.total_duration().unwrap_or_else(|| {
        app.current_track_index
            .and_then(|idx| app.tracks[idx].duration)
            .unwrap_or(Duration::from_secs(180))
    });

    let elapsed_secs = elapsed.as_secs();
    let total_secs = total.as_secs().max(1);
    let pct = (elapsed_secs as f64 / total_secs as f64).clamp(0.0, 1.0);

    // Metadata lines
    let mut lines = vec![
        Line::from(vec![
            Span::styled("ARTIST: ", Style::default().fg(Color::Cyan).bold()),
            Span::styled(artist, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("ALBUM:  ", Style::default().fg(Color::Cyan).bold()),
            Span::styled(album, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("TITLE:  ", Style::default().fg(Color::Cyan).bold()),
            Span::styled(title, Style::default().fg(Color::White).bold()),
        ]),
        Line::from(vec![
            Span::styled("GENRE:  ", Style::default().fg(Color::Cyan).bold()),
            Span::styled(genre, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("FORMAT: ", Style::default().fg(Color::Cyan).bold()),
            Span::styled(format_str, Style::default().fg(Color::Rgb(140, 200, 220))),
        ]),
        Line::from(""),
    ];

    // Transport buttons: |<<  ■  ▶  ||  >>|
    let is_paused = app.player.is_paused();
    let is_playing = app.current_track_index.is_some() && !is_paused;

    let transport_line = Line::from(vec![
        Span::styled(" |<<  ", Style::default().fg(Color::Cyan)),
        Span::styled(" ■ ", if app.current_track_index.is_none() {
            Style::default().fg(Color::Red).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        }),
        Span::styled(" ▶ ", if is_playing {
            Style::default().fg(Color::Green).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        }),
        Span::styled(" || ", if is_paused {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default().fg(Color::DarkGray)
        }),
        Span::styled(" >>|", Style::default().fg(Color::Cyan)),
    ]);
    lines.push(transport_line);

    // Custom Progress Bar: [████████░░░░░░░░] 01:45 / 03:20
    let bar_width = (cols[0].width.saturating_sub(18) as usize).clamp(10, 30);
    let filled = ((pct * bar_width as f64).round() as usize).min(bar_width);
    let unfilled = bar_width.saturating_sub(filled);

    let progress_bar = format!(
        "[{}{}]  {} / {}",
        "█".repeat(filled),
        "░".repeat(unfilled),
        format_duration_secs(elapsed_secs),
        format_duration_secs(total_secs)
    );

    lines.push(Line::from(vec![
        Span::styled(progress_bar, Style::default().fg(Color::Cyan)),
    ]));

    let details_p = Paragraph::new(lines);
    f.render_widget(details_p, cols[0]);

    // Right Column: Spinning Vinyl Record Widget!
    let vinyl_box = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Vinyl Disc height
            Constraint::Length(2), // Time indicator below vinyl
        ])
        .split(cols[1]);

    let vinyl = VinylRecord::new(is_playing, app.frame_counter);
    f.render_widget(vinyl, vinyl_box[0]);

    let vinyl_time = Line::from(vec![
        Span::styled(
            format!("{} / {}", format_duration_secs(elapsed_secs), format_duration_secs(total_secs)),
            Style::default().fg(Color::Rgb(160, 200, 220)).bold(),
        ),
    ]);
    f.render_widget(Paragraph::new(vinyl_time).alignment(Alignment::Center), vinyl_box[1]);
}

fn render_spectrum_pane(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" LIVE SPECTRUM VISUALIZER ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visualizer = SpectrumVisualizer::new(&app.spectrum);
    f.render_widget(visualizer, inner);
}

fn render_player_bottom_bar(f: &mut Frame, _app: &App, area: Rect) {
    let text = Line::from(vec![
        Span::styled("[Space]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Play/Pause | "),
        Span::styled("[n]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Next | "),
        Span::styled("[p]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Prev | "),
        Span::styled("[→/←]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Seek | "),
        Span::styled("[+/-]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Vol | "),
        Span::styled("[s]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Shuffle | "),
        Span::styled("[r]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Repeat | "),
        Span::styled("[m]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Mute | "),
        Span::styled("[/]", Style::default().fg(Color::Cyan).bold()),
        Span::raw(" Search | "),
        Span::styled("[Esc]", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" Back to Menu | "),
        Span::styled("[Q]", Style::default().fg(Color::Red).bold()),
        Span::raw(" Quit"),
    ]);

    let block = Block::default()
        .title(" CONTROLS & HELP ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(Paragraph::new(text).alignment(Alignment::Center).block(block), area);
}

/// Helper function to create a centered popup area
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
