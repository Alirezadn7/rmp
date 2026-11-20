//! rmp - Lightweight, Fast, and Clean Terminal User Interface Music Player in Rust.
//!
//! Featuring two screens:
//! 1. `AppScreen::Menu`: Retro ASCII-art Launcher & Directory Selection.
//! 2. `AppScreen::Player`: High-performance retro TUI with Playlist, Now Playing,
//!    animated spinning vinyl + tonearm pixel art, and live spectrum visualizer.

mod config;
mod player;
mod playlist;
mod scanner;
mod theme;
mod vinyl;
mod visualizer;

use std::fs;
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use config::{AppConfig, RepeatMode};
use player::Player;
use playlist::{CompletionAction, PlaylistController};
use scanner::{format_duration_secs, Track};
use theme::ThemePalette;
use vinyl::VinylRecord;
use visualizer::{SpectrumState, SpectrumVisualizer};

const STATUS_DURATION: Duration = Duration::from_secs(4);

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
    list_state: ListState,
}

impl BrowseState {
    fn new(start_dir: PathBuf) -> Self {
        let mut state = Self {
            current_dir: start_dir,
            entries: Vec::new(),
            selected: 0,
            list_state: ListState::default(),
        };
        state.refresh();
        state
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.selected = 0;
        self.list_state = ListState::default();
        self.list_state.select(Some(0));

        if let Ok(read_dir) = fs::read_dir(&self.current_dir) {
            let mut dirs = Vec::new();
            for entry in read_dir.flatten() {
                if let Ok(ft) = entry.file_type() {
                    if ft.is_dir() {
                        dirs.push(entry.path());
                    }
                }
            }
            dirs.sort_by_key(|a| a.file_name().map(|n| n.to_string_lossy().to_lowercase()));
            self.entries = dirs;
        }
    }

    fn sync_selection(&mut self) {
        self.list_state.select(Some(self.selected));
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
            self.refresh();
        }
    }

    fn enter_selected(&mut self) -> Option<PathBuf> {
        if self.selected == 0 {
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

/// Ephemeral status/error message shown in the TUI status strip
#[derive(Debug, Clone)]
struct StatusBanner {
    text: String,
    is_error: bool,
    expires_at: Instant,
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
    playlist: PlaylistController,
    playlist_state: ListState,
    current_dir_display: String,
    consecutive_errors: usize,

    // Overlay & Feedback
    show_help: bool,
    status_banner: Option<StatusBanner>,

    // Animation & Physics
    frame_counter: usize,
    spectrum: SpectrumState,
    should_quit: bool,
}

impl App {
    fn new(config: AppConfig, player: Player) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xCAFE_BABE);

        let playlist = PlaylistController::new(config.repeat, config.shuffle, seed);

        Self {
            screen: AppScreen::Menu,
            config,
            player,
            menu_selected: 0,
            chosen_route: 0,
            menu_modal: MenuModal::None,
            tracks: Vec::new(),
            playlist,
            playlist_state: ListState::default(),
            current_dir_display: String::new(),
            consecutive_errors: 0,
            show_help: false,
            status_banner: None,
            frame_counter: 0,
            spectrum: SpectrumState::new(),
            should_quit: false,
        }
    }

    fn set_status<S: Into<String>>(&mut self, text: S, is_error: bool) {
        self.status_banner = Some(StatusBanner {
            text: text.into(),
            is_error,
            expires_at: Instant::now() + STATUS_DURATION,
        });
    }

    fn active_status(&self) -> Option<&StatusBanner> {
        self.status_banner
            .as_ref()
            .filter(|b| Instant::now() < b.expires_at)
    }

    fn open_directory(&mut self, dir_path: &Path) {
        if let Err(err) = self.load_directory(dir_path) {
            self.set_status(err.to_string(), true);
        }
    }

    /// Load tracks from a folder and transition to Player screen
    fn load_directory(&mut self, dir_path: &Path) -> Result<()> {
        let tracks = scanner::scan_directory(dir_path)
            .with_context(|| format!("Failed to scan directory: {}", dir_path.display()))?;

        if tracks.is_empty() {
            return Err(anyhow::anyhow!(
                "No supported audio files found in {}",
                dir_path.display()
            ));
        }

        // Cleanly stop any track from a previous directory before switching
        self.player.stop();
        self.tracks = tracks;
        self.playlist.load_tracks(self.tracks.len());
        self.playlist_state = ListState::default();
        self.playlist_state.select(Some(0));
        self.consecutive_errors = 0;
        self.show_help = false;
        self.current_dir_display = config::display_path(dir_path);

        let path_str = dir_path.display().to_string();
        self.config.last_session = Some(path_str);
        let _ = self.config.save();

        self.set_status(
            format!("Loaded {} tracks from {}", self.tracks.len(), self.current_dir_display),
            false,
        );
        self.screen = AppScreen::Player;
        Ok(())
    }

    fn stop_playback(&mut self) {
        self.player.stop();
        self.playlist.current_track = None;
        self.consecutive_errors = 0;
    }

    fn play_track(&mut self, index: usize) -> bool {
        if index >= self.tracks.len() {
            return false;
        }

        let track_path = self.tracks[index].path.clone();
        let track_name = self.tracks[index].file_name.clone();

        match self.player.play(&track_path) {
            Ok(info) => {
                self.playlist.current_track = Some(index);
                self.playlist.sync_shuffle_cursor(index);
                self.consecutive_errors = 0;
                if let Some(d) = info.duration {
                    self.tracks[index].duration = Some(d);
                }
                self.spectrum.set_sample_rate(info.sample_rate);
                true
            }
            Err(err) => {
                self.consecutive_errors += 1;
                self.set_status(format!("Cannot play '{}': {}", track_name, err), true);
                false
            }
        }
    }

    fn select_and_play(&mut self, track_idx: usize) -> bool {
        if let Some(pos) = self.playlist.filtered_indices.iter().position(|&i| i == track_idx) {
            self.playlist_state.select(Some(pos));
        }
        self.play_track(track_idx)
    }

    fn play_selected(&mut self) {
        if let Some(sel) = self.playlist_state.selected() {
            let actual_idx = self
                .playlist
                .filtered_indices
                .get(sel)
                .copied()
                .unwrap_or(sel);
            self.play_track(actual_idx);
        }
    }

    fn confirm_search_and_play(&mut self) {
        let chosen_idx = self
            .playlist_state
            .selected()
            .and_then(|sel| self.playlist.filtered_indices.get(sel).copied());

        self.playlist.clear_search();

        if let Some(idx) = chosen_idx {
            self.select_and_play(idx);
        } else if let Some(cur) = self.playlist.current_track {
            self.playlist_state.select(Some(cur));
        } else if !self.tracks.is_empty() {
            self.playlist_state.select(Some(0));
        }
    }

    fn advance_track(&mut self, forward: bool) {
        if self.tracks.is_empty() {
            return;
        }

        let max_attempts = self.tracks.len();
        for _ in 0..max_attempts {
            let candidate = if forward {
                self.playlist.next_manual()
            } else {
                self.playlist.prev_manual()
            };

            let Some(idx) = candidate else {
                break;
            };

            if self.select_and_play(idx) {
                return;
            }
            // Advance pointer so next attempt skips past the broken track
            self.playlist.current_track = Some(idx);
        }

        self.stop_playback();
        self.set_status("No playable tracks found in playlist", true);
    }

    fn play_next(&mut self) {
        self.advance_track(true);
    }

    fn play_prev(&mut self) {
        self.advance_track(false);
    }

    fn toggle_shuffle(&mut self) {
        self.playlist.toggle_shuffle();
        self.config.shuffle = self.playlist.shuffle;
        let _ = self.config.save();
    }

    fn cycle_repeat(&mut self) {
        self.playlist.repeat = self.playlist.repeat.next();
        self.config.repeat = self.playlist.repeat;
        let _ = self.config.save();
    }

    fn cycle_theme(&mut self) {
        self.config.theme = self.config.theme.next();
        let _ = self.config.save();
        self.set_status(format!("Theme: {}", self.config.theme.label()), false);
    }

    fn update_search_filter(&mut self) {
        self.playlist.apply_search_filter(&self.tracks);
        if !self.playlist.filtered_indices.is_empty() {
            self.playlist_state.select(Some(0));
        } else {
            self.playlist_state.select(None);
        }
    }

    /// Check if current track ended and handle repeat/once/shuffle auto-progression
    fn check_track_completion(&mut self) {
        if self.playlist.current_track.is_some()
            && !self.player.is_paused()
            && self.player.is_empty()
        {
            let mut action = self.playlist.next_on_completion();
            let mut attempts = 0;
            while let CompletionAction::Play(idx) = action {
                if self.select_and_play(idx) {
                    return;
                }
                attempts += 1;
                if attempts >= self.tracks.len()
                    || self.consecutive_errors >= self.tracks.len()
                {
                    self.stop_playback();
                    self.set_status("No playable tracks found in playlist", true);
                    return;
                }
                action = self.playlist.handle_completion_failure(idx);
            }
            self.stop_playback();
        }
    }
}

fn main() -> Result<()> {
    // 1. Load Configuration
    let config = AppConfig::load();

    // 2. Initialize Player with saved volume
    let player = Player::with_volume(config.volume as f32 / 100.0)
        .context("Failed to initialize audio output engine")?;

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

    let mut app = App::new(config, player);

    // If target directory was passed on CLI arguments, directly open it
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        let target = PathBuf::from(args.join(" "));
        app.open_directory(&target);
    }

    // Main Event Loop with Dynamic Frame Rate / CPU Optimization
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui(f, &mut app))?;

        let is_playing = app.screen == AppScreen::Player
            && app.playlist.current_track.is_some()
            && !app.player.is_paused();

        // 16ms (~60 FPS) when playing for fluid FFT visualizer motion; 200ms when idle/menu
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
                handle_key_event(&mut app, key)?;
            }
            let mut drained = 0;
            while !app.should_quit && drained < 16 && event::poll(Duration::ZERO)? {
                if let Event::Key(key) = event::read()? {
                    handle_key_event(&mut app, key)?;
                }
                drained += 1;
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.frame_counter = app.frame_counter.wrapping_add(1);
            let active_playing =
                app.playlist.current_track.is_some() && !app.player.is_paused();
            let shared_audio = app.player.audio_buffer();
            app.spectrum.update(active_playing, &shared_audio);
            app.check_track_completion();
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn is_plain_char(modifiers: KeyModifiers) -> bool {
    !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}

fn handle_key_event(app: &mut App, key: KeyEvent) -> Result<()> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return Ok(());
    }

    // Global Ctrl+C quit in raw mode
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c' | 'C'))
    {
        app.should_quit = true;
        return Ok(());
    }

    // Help overlay intercepts keys cleanly when open
    if app.show_help {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?' | 'q' | 'Q') => {
                app.show_help = false;
            }
            _ => {}
        }
        return Ok(());
    }

    match app.screen {
        AppScreen::Menu => handle_menu_key(app, key),
        AppScreen::Player => handle_player_key(app, key),
    }
}

// -----------------------------------------------------------------------------
// Screen 1: Launcher Menu Key Handling
// -----------------------------------------------------------------------------
fn handle_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let code = key.code;

    match &mut app.menu_modal {
        MenuModal::Browse(ref mut browse) => match code {
            KeyCode::Esc => {
                app.menu_modal = MenuModal::None;
            }
            KeyCode::Up | KeyCode::Char('k' | 'K') => {
                if browse.selected > 0 {
                    browse.selected -= 1;
                    browse.sync_selection();
                }
            }
            KeyCode::Down | KeyCode::Char('j' | 'J') => {
                let max = browse.entries.len() + 1;
                if browse.selected < max {
                    browse.selected += 1;
                    browse.sync_selection();
                }
            }
            KeyCode::Enter => {
                if let Some(confirmed_dir) = browse.enter_selected() {
                    app.menu_modal = MenuModal::None;
                    app.open_directory(&confirmed_dir);
                }
            }
            KeyCode::Backspace | KeyCode::Char('u' | 'U') => {
                browse.go_up();
            }
            KeyCode::Char(' ' | 'o' | 'O') => {
                let dir = browse.current_dir.clone();
                app.menu_modal = MenuModal::None;
                app.open_directory(&dir);
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
            KeyCode::Char(c) if is_plain_char(key.modifiers) => {
                input.push(c);
            }
            KeyCode::Enter => {
                let trimmed = input.trim().to_string();
                app.menu_modal = MenuModal::None;
                if !trimmed.is_empty() {
                    let resolved = config::resolve_path(&trimmed);
                    if resolved.exists() && resolved.is_dir() {
                        if !app.config.routes.contains(&trimmed) {
                            app.config.routes.push(trimmed.clone());
                            let _ = app.config.save();
                            app.set_status(format!("Added route: {}", trimmed), false);
                        } else {
                            app.set_status("Route already exists in list", true);
                        }
                    } else {
                        app.set_status(format!("Directory not found: {}", trimmed), true);
                    }
                }
            }
            _ => {}
        },
        MenuModal::None => {
            let total_items = 1 + app.config.routes.len() + 2;

            match code {
                KeyCode::Char('q' | 'Q') | KeyCode::Esc => {
                    app.should_quit = true;
                }
                KeyCode::Char('?') => {
                    app.show_help = true;
                }
                KeyCode::Char('t' | 'T') => {
                    app.cycle_theme();
                }
                KeyCode::Up | KeyCode::Char('k' | 'K') => {
                    if app.menu_selected == 0 {
                        app.menu_selected = total_items.saturating_sub(1);
                    } else {
                        app.menu_selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j' | 'J') => {
                    app.menu_selected = (app.menu_selected + 1) % total_items;
                }
                KeyCode::Char(' ') => {
                    if app.menu_selected <= app.config.routes.len() {
                        app.chosen_route = app.menu_selected;
                    }
                }
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    let route_idx = (c as usize) - ('1' as usize);
                    if let Some(r) = app.config.routes.get(route_idx).cloned() {
                        app.chosen_route = route_idx + 1;
                        let path = config::resolve_path(&r);
                        app.open_directory(&path);
                    }
                }
                KeyCode::Char('b' | 'B') => {
                    let start = config::home_dir().unwrap_or_else(|| PathBuf::from("."));
                    app.menu_modal = MenuModal::Browse(BrowseState::new(start));
                }
                KeyCode::Char('a' | 'A') => {
                    app.menu_modal = MenuModal::AddPath(String::new());
                }
                KeyCode::Char('d' | 'D') => {
                    if app.menu_selected > 0 && app.menu_selected <= app.config.routes.len() {
                        let idx = app.menu_selected - 1;
                        let removed = app.config.routes.remove(idx);
                        let _ = app.config.save();
                        app.menu_selected = app.menu_selected.min(total_items.saturating_sub(2));
                        if app.chosen_route > idx + 1 {
                            app.chosen_route -= 1;
                        } else if app.chosen_route == idx + 1 {
                            app.chosen_route = 0;
                        }
                        app.set_status(format!("Removed route: {}", removed), false);
                    }
                }
                KeyCode::Enter => {
                    let routes_len = app.config.routes.len();
                    if app.menu_selected == routes_len + 1 {
                        let start = config::home_dir().unwrap_or_else(|| PathBuf::from("."));
                        app.menu_modal = MenuModal::Browse(BrowseState::new(start));
                    } else if app.menu_selected == routes_len + 2 {
                        app.menu_modal = MenuModal::AddPath(String::new());
                    } else {
                        let target_idx = app.menu_selected;
                        app.chosen_route = target_idx;

                        if target_idx == 0 {
                            if let Some(last) = app.config.last_session.clone() {
                                let path = config::resolve_path(&last);
                                app.open_directory(&path);
                            } else {
                                app.set_status("No last session recorded", true);
                            }
                        } else if target_idx <= routes_len {
                            let route_idx = target_idx - 1;
                            let r = app.config.routes[route_idx].clone();
                            let path = config::resolve_path(&r);
                            app.open_directory(&path);
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
fn handle_player_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let code = key.code;

    // Dedicated Search Mode handling
    if let Some(ref mut query) = app.playlist.search_query {
        match code {
            KeyCode::Esc => {
                app.playlist.clear_search();
                if let Some(cur) = app.playlist.current_track {
                    app.playlist_state.select(Some(cur));
                } else if !app.tracks.is_empty() {
                    app.playlist_state.select(Some(0));
                }
            }
            KeyCode::Enter => {
                app.confirm_search_and_play();
            }
            KeyCode::Backspace => {
                query.pop();
                app.update_search_filter();
            }
            KeyCode::Char(c) if is_plain_char(key.modifiers) => {
                query.push(c);
                app.update_search_filter();
            }
            KeyCode::Up => {
                select_prev_playlist(app);
            }
            KeyCode::Down => {
                select_next_playlist(app);
            }
            _ => {}
        }
        return Ok(());
    }

    match code {
        KeyCode::Esc => {
            app.screen = AppScreen::Menu;
        }
        KeyCode::Char('q' | 'Q') => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.show_help = true;
        }
        KeyCode::Char('t' | 'T') => {
            app.cycle_theme();
        }

        // Playback controls
        KeyCode::Char(' ') => {
            if app.playlist.current_track.is_none() {
                app.play_selected();
            } else {
                app.player.toggle_pause();
            }
        }
        KeyCode::Enter => {
            app.play_selected();
        }
        KeyCode::Char('x' | 'X') => {
            app.stop_playback();
            app.set_status("Playback stopped", false);
        }
        KeyCode::Char('s' | 'S') => {
            app.toggle_shuffle();
        }
        KeyCode::Char('r' | 'R') => {
            app.cycle_repeat();
        }
        KeyCode::Char('m' | 'M') => {
            app.player.toggle_mute();
        }
        KeyCode::Char('n' | 'N') => {
            app.play_next();
        }
        KeyCode::Char('p' | 'P' | 'b' | 'B') => {
            app.play_prev();
        }

        // Navigation
        KeyCode::Up | KeyCode::Char('k' | 'K') => {
            select_prev_playlist(app);
        }
        KeyCode::Down | KeyCode::Char('j' | 'J') => {
            select_next_playlist(app);
        }

        // Volume
        KeyCode::Char('+' | '=') => {
            app.player.volume_up();
            app.config.volume = (app.player.volume() * 100.0).round() as u8;
            let _ = app.config.save();
        }
        KeyCode::Char('-' | '_') => {
            app.player.volume_down();
            app.config.volume = (app.player.volume() * 100.0).round() as u8;
            let _ = app.config.save();
        }

        // Seek forward / backward
        KeyCode::Right => {
            if let Err(err) = app.player.seek_forward(5) {
                app.set_status(err.to_string(), true);
            }
        }
        KeyCode::Left => {
            if let Err(err) = app.player.seek_backward(5) {
                app.set_status(err.to_string(), true);
            }
        }

        // Search trigger
        KeyCode::Char('/') => {
            app.playlist.search_query = Some(String::new());
            app.update_search_filter();
        }

        _ => {}
    }

    Ok(())
}

fn select_next_playlist(app: &mut App) {
    if app.playlist.filtered_indices.is_empty() {
        return;
    }
    let cur = app.playlist_state.selected().unwrap_or(0);
    let next = if cur + 1 >= app.playlist.filtered_indices.len() {
        0
    } else {
        cur + 1
    };
    app.playlist_state.select(Some(next));
}

fn select_prev_playlist(app: &mut App) {
    if app.playlist.filtered_indices.is_empty() {
        return;
    }
    let cur = app.playlist_state.selected().unwrap_or(0);
    let prev = if cur == 0 {
        app.playlist.filtered_indices.len() - 1
    } else {
        cur - 1
    };
    app.playlist_state.select(Some(prev));
}

// -----------------------------------------------------------------------------
// UI Rendering
// -----------------------------------------------------------------------------
fn ui(f: &mut Frame, app: &mut App) {
    let palette = ThemePalette::for_kind(app.config.theme);

    match app.screen {
        AppScreen::Menu => render_menu_screen(f, app, &palette),
        AppScreen::Player => render_player_screen(f, app, &palette),
    }

    if app.show_help {
        render_help_modal(f, app.screen, &palette);
    }
}

// -----------------------------------------------------------------------------
// SCREEN 1: Menu & Launcher UI
// -----------------------------------------------------------------------------
fn render_menu_screen(f: &mut Frame, app: &mut App, palette: &ThemePalette) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top margin
            Constraint::Length(6), // ASCII Banner
            Constraint::Length(2), // Subtitle
            Constraint::Min(8),    // Launcher Box
            Constraint::Length(1), // Status / Toast line
            Constraint::Length(3), // Bottom controls bar
        ])
        .split(size);

    let banner = vec![
        Line::from(Span::styled(
            " ██████╗  ███╗   ███╗ ██████╗ ",
            Style::default().fg(palette.accent_bright).bold(),
        )),
        Line::from(Span::styled(
            " ██╔══██╗ ████╗ ████║ ██╔══██╗",
            Style::default().fg(palette.accent_bright).bold(),
        )),
        Line::from(Span::styled(
            " ██████╔╝ ██╔████╔██║ ██████╔╝",
            Style::default().fg(palette.accent).bold(),
        )),
        Line::from(Span::styled(
            " ██╔══██╗ ██║╚██╔╝██║ ██╔═══╝ ",
            Style::default().fg(palette.accent).bold(),
        )),
        Line::from(Span::styled(
            " ██║  ██║ ██║ ╚═╝ ██║ ██║     ",
            Style::default().fg(palette.border).bold(),
        )),
        Line::from(Span::styled(
            " ╚═╝  ╚═╝ ╚═╝     ╚═╝ ╚═╝     ",
            Style::default().fg(palette.border).bold(),
        )),
    ];
    f.render_widget(Paragraph::new(banner).alignment(Alignment::Center), chunks[1]);

    let subtitle = Paragraph::new(Line::from(vec![
        Span::styled(
            "♫ Rust Music Player  •  ",
            Style::default().fg(palette.accent).italic(),
        ),
        Span::styled(
            format!("[THEME: {}]", app.config.theme.label()),
            Style::default().fg(palette.badge_special).bold(),
        ),
        Span::styled(" ♫", Style::default().fg(palette.accent).italic()),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(subtitle, chunks[2]);

    let box_width = size.width.saturating_sub(4).clamp(36, 68);
    let center_h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(box_width),
            Constraint::Min(0),
        ])
        .split(chunks[3]);

    let launcher_area = center_h[1];

    struct MenuEntry {
        label: String,
        exists: Option<bool>,
    }

    let mut entries = Vec::new();
    let last_raw = app
        .config
        .last_session
        .as_deref()
        .unwrap_or("~/Music");
    let last_resolved = config::resolve_path(last_raw);
    entries.push(MenuEntry {
        label: format!("[Resume Last Session] ({})", config::display_path(Path::new(last_raw))),
        exists: Some(last_resolved.is_dir()),
    });

    for (i, r) in app.config.routes.iter().enumerate() {
        let resolved = config::resolve_path(r);
        entries.push(MenuEntry {
            label: format!("[{}] {}", i + 1, r),
            exists: Some(resolved.is_dir()),
        });
    }

    entries.push(MenuEntry {
        label: "[B] Browse File System...".to_string(),
        exists: None,
    });
    entries.push(MenuEntry {
        label: "[A] Add New Custom Path...".to_string(),
        exists: None,
    });

    let inner_w = launcher_area.width.saturating_sub(4) as usize;
    let list_items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let is_focused = app.menu_selected == idx;
            let is_chosen = app.chosen_route == idx && idx <= app.config.routes.len();

            let prefix = if is_focused { "▶ " } else { "  " };
            let mark = if idx <= app.config.routes.len() {
                if is_chosen { "● " } else { "○ " }
            } else {
                "  "
            };
            let status_tag = match entry.exists {
                Some(true) => " ✓",
                Some(false) => " ✗",
                None => "",
            };
            let max_label_w = inner_w.saturating_sub(4 + status_tag.chars().count());
            let label = truncate_chars(&entry.label, max_label_w);

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(palette.badge_warn).bold()),
                Span::styled(
                    mark,
                    if is_chosen {
                        Style::default().fg(palette.accent).bold()
                    } else {
                        Style::default().fg(palette.text_muted)
                    },
                ),
                Span::styled(
                    label,
                    if is_focused {
                        Style::default().fg(palette.text_primary).bold()
                    } else if is_chosen {
                        Style::default().fg(palette.accent)
                    } else {
                        Style::default().fg(palette.text_secondary)
                    },
                ),
            ];

            if let Some(ok) = entry.exists {
                spans.push(Span::styled(
                    status_tag,
                    Style::default().fg(if ok {
                        palette.badge_ok
                    } else {
                        palette.badge_err
                    }),
                ));
            }

            let line = Line::from(spans);
            if is_focused {
                ListItem::new(line).style(Style::default().bg(palette.highlight_bg))
            } else {
                ListItem::new(line)
            }
        })
        .collect();

    let launcher_box = List::new(list_items).block(
        Block::default()
            .title(" ◈ LAUNCHER / SELECT PATH ◈ ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(palette.border)),
    );
    f.render_widget(launcher_box, launcher_area);

    render_status_strip(f, app, chunks[4], palette, "Select a music folder or press [1-9]");

    let bottom_text = menu_bottom_bar_line(chunks[5].width.saturating_sub(2), palette);

    let bottom_box = Paragraph::new(bottom_text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette.border_dim)),
        );
    f.render_widget(bottom_box, chunks[5]);

    match &mut app.menu_modal {
        MenuModal::Browse(ref mut browse) => render_browse_modal(f, browse, size, palette),
        MenuModal::AddPath(ref input) => render_add_path_modal(f, input, size, palette),
        MenuModal::None => {}
    }
}

fn render_browse_modal(
    f: &mut Frame,
    browse: &mut BrowseState,
    area: Rect,
    palette: &ThemePalette,
) {
    let popup_area = centered_rect(70, 75, area);
    f.render_widget(Clear, popup_area);

    let mut items = vec![
        ListItem::new(Line::from(vec![Span::styled(
            "  ✓ [Confirm & Open Current Folder]",
            Style::default().fg(palette.badge_ok).bold(),
        )])),
        ListItem::new(Line::from(vec![Span::styled(
            "  .. (Parent Folder)",
            Style::default().fg(palette.badge_warn).bold(),
        )])),
    ];

    for entry in &browse.entries {
        let name = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder");
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  ▸ ", Style::default().fg(palette.accent)),
            Span::styled(name, Style::default().fg(palette.text_secondary)),
        ])));
    }

    let title_path = truncate_chars(
        &browse.current_dir.display().to_string(),
        popup_area.width.saturating_sub(36) as usize,
    );
    let block = Block::default()
        .title(format!(
            " Browse: {} ([Space] Open | [Enter] Step) ",
            title_path
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(palette.border));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(palette.highlight_bg)
                .fg(palette.text_primary)
                .bold(),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, popup_area, &mut browse.list_state);
}

fn render_add_path_modal(f: &mut Frame, input: &str, area: Rect, palette: &ThemePalette) {
    let popup_area = centered_fixed_rect(area.width.saturating_sub(6).clamp(32, 64), 7, area);
    f.render_widget(Clear, popup_area);

    let text = vec![
        Line::from(Span::styled(
            "Enter Directory Path (supports ~):",
            Style::default().fg(palette.text_primary).bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" > ", Style::default().fg(palette.badge_warn).bold()),
            Span::styled(input, Style::default().fg(palette.accent_bright).bold()),
            Span::styled("█", Style::default().fg(palette.badge_warn)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "[Enter] Save & Add  |  [Esc] Cancel",
            Style::default().fg(palette.text_muted),
        )),
    ];

    let block = Block::default()
        .title(" + Add Custom Path ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(palette.badge_warn));

    f.render_widget(Paragraph::new(text).block(block), popup_area);
}

// -----------------------------------------------------------------------------
// SCREEN 2: Main Music Player UI
// -----------------------------------------------------------------------------
fn render_player_screen(f: &mut Frame, app: &mut App, palette: &ThemePalette) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Status Bar
            Constraint::Min(12),   // Main 2-column layout
            Constraint::Length(1), // Status / Ticker Strip
            Constraint::Length(3), // Bottom Controls & Help
        ])
        .split(size);

    render_top_status_bar(f, app, chunks[0], palette);

    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[1]);

    render_playlist_pane(f, app, body_cols[0], palette);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(now_playing_height(body_cols[1].height)),
            Constraint::Min(6),
        ])
        .split(body_cols[1]);

    render_now_playing_pane(f, app, right_rows[0], palette);
    render_spectrum_pane(f, app, right_rows[1], palette);

    let fallback_info = if app.current_dir_display.is_empty() {
        format!("{} tracks loaded", app.tracks.len())
    } else {
        format!("{}  •  {} tracks", app.current_dir_display, app.tracks.len())
    };
    render_status_strip(f, app, chunks[2], palette, &fallback_info);
    render_player_bottom_bar(f, chunks[3], palette);
}

fn render_status_strip(
    f: &mut Frame,
    app: &App,
    area: Rect,
    palette: &ThemePalette,
    fallback: &str,
) {
    let line = if let Some(banner) = app.active_status() {
        let (tag, color) = if banner.is_error {
            (" [ERR] ", palette.badge_err)
        } else {
            (" [INFO] ", palette.badge_ok)
        };
        Line::from(vec![
            Span::styled(tag, Style::default().fg(color).bold()),
            Span::styled(&banner.text, Style::default().fg(color)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ◈ ", Style::default().fg(palette.text_muted)),
            Span::styled(fallback, Style::default().fg(palette.text_muted)),
        ])
    };
    f.render_widget(Paragraph::new(line), area);
}

fn render_top_status_bar(f: &mut Frame, app: &App, area: Rect, palette: &ThemePalette) {
    let vol_pct = (app.player.volume() * 100.0).round() as u8;
    let muted = app.player.is_muted();
    let muted_str = if muted { "YES" } else { "NO" };
    let shuffle_str = if app.playlist.shuffle { "ON" } else { "OFF" };
    let repeat_str = app.playlist.repeat.as_str();

    let center_line = Line::from(vec![
        Span::styled("♫ rmp DECK ", Style::default().fg(palette.accent_bright).bold()),
        Span::styled(
            format!("[{}]", app.config.theme.label()),
            Style::default().fg(palette.badge_special).bold(),
        ),
        Span::styled(" ♫", Style::default().fg(palette.accent_bright).bold()),
    ]);

    let compact = area.width < 76;
    let left_line = if compact {
        let mut spans = vec![Span::styled(
            format!("[V:{}%]", vol_pct),
            Style::default().fg(palette.accent).bold(),
        )];
        if muted {
            spans.push(Span::styled(" [M]", Style::default().fg(palette.badge_warn).bold()));
        }
        Line::from(spans)
    } else {
        Line::from(vec![
            Span::styled(
                format!("[VOL: {}%] ", vol_pct),
                Style::default().fg(palette.accent).bold(),
            ),
            Span::styled(
                format!("[MUTED: {}]", muted_str),
                if muted {
                    Style::default().fg(palette.badge_warn).bold()
                } else {
                    Style::default().fg(palette.text_muted)
                },
            ),
        ])
    };

    let repeat_style = match app.playlist.repeat {
        RepeatMode::Off => Style::default().fg(palette.text_muted),
        RepeatMode::One => Style::default().fg(palette.badge_warn).bold(),
        RepeatMode::Once => Style::default().fg(palette.badge_special).bold(),
        RepeatMode::All => Style::default().fg(palette.accent).bold(),
    };
    let shuffle_style = if app.playlist.shuffle {
        Style::default().fg(palette.accent).bold()
    } else {
        Style::default().fg(palette.text_muted)
    };

    let right_line = if compact {
        Line::from(vec![
            Span::styled(format!("[SHF:{}] ", shuffle_str), shuffle_style),
            Span::styled(format!("[REP:{}]", repeat_str), repeat_style),
        ])
    } else {
        Line::from(vec![
            Span::styled(format!("[SHUFFLE: {}] ", shuffle_str), shuffle_style),
            Span::styled(format!("[REPEAT: {}]", repeat_str), repeat_style),
        ])
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(top_bar_constraints(
            area.width,
            left_line.width() as u16,
            right_line.width() as u16,
        ))
        .split(area);

    f.render_widget(Paragraph::new(left_line).alignment(Alignment::Left), cols[0]);
    f.render_widget(Paragraph::new(center_line).alignment(Alignment::Center), cols[1]);
    f.render_widget(Paragraph::new(right_line).alignment(Alignment::Right), cols[2]);
}

fn render_playlist_pane(f: &mut Frame, app: &mut App, area: Rect, palette: &ThemePalette) {
    let search_active = app.playlist.search_query.is_some();
    let (list_area, search_area) = if search_active {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);
        (split[0], Some(split[1]))
    } else {
        (area, None)
    };

    let title = if search_active {
        format!(
            " TRACKS & PLAYLIST ({} matches) ",
            app.playlist.filtered_indices.len()
        )
    } else {
        format!(" TRACKS & PLAYLIST ({}) ", app.tracks.len())
    };

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(if search_active {
            palette.border_dim
        } else {
            palette.border
        }));

    let inner_width = list_area.width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = app
        .playlist
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(display_idx, &track_idx)| {
            let track = &app.tracks[track_idx];
            let is_current = app.playlist.current_track == Some(track_idx);

            let prefix = if is_current { "▶ " } else { "  " };
            let prefix_cols = 2usize;
            let num = format!("{:02}. ", display_idx + 1);
            let num_cols = num.chars().count();

            let raw_name = if track.artist != "Unknown Artist" {
                format!("{} - {}", track.title, track.artist)
            } else {
                track.file_name.clone()
            };

            let dur_str = track.formatted_duration();
            let dur_cols = dur_str.chars().count();

            let max_name_cols = inner_width.saturating_sub(prefix_cols + num_cols + dur_cols + 1);
            let name = truncate_chars(&raw_name, max_name_cols);
            let name_cols = name.chars().count();

            let used = prefix_cols + num_cols + name_cols + dur_cols;
            let pad = " ".repeat(inner_width.saturating_sub(used).max(1));

            let line = Line::from(vec![
                Span::styled(prefix, Style::default().fg(palette.accent_bright).bold()),
                Span::styled(num, Style::default().fg(palette.text_muted)),
                Span::styled(
                    name,
                    if is_current {
                        Style::default().fg(palette.text_primary).bold()
                    } else {
                        Style::default().fg(palette.text_secondary)
                    },
                ),
                Span::raw(pad),
                Span::styled(
                    dur_str,
                    Style::default().fg(if is_current {
                        palette.accent
                    } else {
                        palette.text_muted
                    }),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(palette.highlight_bg)
            .fg(palette.text_primary)
            .bold(),
    );

    f.render_stateful_widget(list, list_area, &mut app.playlist_state);

    if let (Some(s_area), Some(ref query)) = (search_area, &app.playlist.search_query) {
        let search_line = Line::from(vec![
            Span::styled(" SEARCH: ", Style::default().fg(palette.badge_warn).bold()),
            Span::styled(query.as_str(), Style::default().fg(palette.text_primary).bold()),
            Span::styled("█", Style::default().fg(palette.badge_warn)),
        ]);
        let search_block = Block::default()
            .title(" FILTER MODE ([Enter] Play & Clear | [Esc] Cancel) ")
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(palette.badge_warn));
        f.render_widget(Paragraph::new(search_line).block(search_block), s_area);
    }
}

fn render_now_playing_pane(f: &mut Frame, app: &App, area: Rect, palette: &ThemePalette) {
    let block = Block::default()
        .title(" NOW PLAYING ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(palette.border));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let vinyl_col_w = if inner.width >= 48 { 21 } else { 17 };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(22), Constraint::Length(vinyl_col_w)])
        .split(inner);

    let (artist, album, title, genre, format_str) = match app.playlist.current_track {
        Some(idx) => {
            let t = &app.tracks[idx];
            (
                t.artist.as_str(),
                t.album.as_str(),
                t.title.as_str(),
                t.genre.as_str(),
                t.format_str.as_str(),
            )
        }
        None => ("None (Stopped)", "-", "-", "-", "-"),
    };

    let meta_max = cols[0].width.saturating_sub(9) as usize;

    let has_track = app.playlist.current_track.is_some();
    let elapsed_secs = if has_track {
        app.player.elapsed().as_secs()
    } else {
        0
    };
    let total_dur = if has_track {
        app.player.total_duration().or_else(|| {
            app.playlist
                .current_track
                .and_then(|idx| app.tracks[idx].duration)
        })
    } else {
        None
    };

    let (pct, time_display) = match total_dur {
        Some(tot) if tot.as_secs() > 0 => {
            let t_secs = tot.as_secs();
            let p = (elapsed_secs as f64 / t_secs as f64).clamp(0.0, 1.0);
            (
                p,
                format!(
                    "{} / {}",
                    format_duration_secs(elapsed_secs),
                    format_duration_secs(t_secs)
                ),
            )
        }
        _ if has_track => (0.0, format!("{} / --:--", format_duration_secs(elapsed_secs))),
        _ => (0.0, "--:-- / --:--".to_string()),
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("ARTIST: ", Style::default().fg(palette.accent).bold()),
            Span::styled(
                truncate_chars(artist, meta_max),
                Style::default().fg(palette.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("ALBUM:  ", Style::default().fg(palette.accent).bold()),
            Span::styled(
                truncate_chars(album, meta_max),
                Style::default().fg(palette.text_secondary),
            ),
        ]),
        Line::from(vec![
            Span::styled("TITLE:  ", Style::default().fg(palette.accent).bold()),
            Span::styled(
                truncate_chars(title, meta_max),
                Style::default().fg(palette.accent_bright).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("GENRE:  ", Style::default().fg(palette.accent).bold()),
            Span::styled(
                truncate_chars(genre, meta_max),
                Style::default().fg(palette.text_secondary),
            ),
        ]),
        Line::from(vec![
            Span::styled("FORMAT: ", Style::default().fg(palette.accent).bold()),
            Span::styled(
                truncate_chars(format_str, meta_max),
                Style::default().fg(palette.text_muted),
            ),
        ]),
        Line::from(""),
    ];

    // Retro Hi-Fi Transport Bar
    let is_paused = app.player.is_paused();
    let is_playing = has_track && !is_paused;

    let transport_line = Line::from(vec![
        Span::styled(" |<< ", Style::default().fg(palette.accent)),
        Span::raw(" "),
        Span::styled(
            " ■ ",
            if !has_track {
                Style::default()
                    .bg(palette.badge_err)
                    .fg(palette.text_primary)
                    .bold()
            } else {
                Style::default().fg(palette.text_muted)
            },
        ),
        Span::raw(" "),
        Span::styled(
            " ▶ ",
            if is_playing {
                Style::default()
                    .bg(palette.badge_ok)
                    .fg(palette.vinyl_hole)
                    .bold()
            } else {
                Style::default().fg(palette.text_muted)
            },
        ),
        Span::raw(" "),
        Span::styled(
            " || ",
            if has_track && is_paused {
                Style::default()
                    .bg(palette.badge_warn)
                    .fg(palette.vinyl_hole)
                    .bold()
            } else {
                Style::default().fg(palette.text_muted)
            },
        ),
        Span::raw(" "),
        Span::styled(" >>| ", Style::default().fg(palette.accent)),
    ]);
    lines.push(transport_line);

    // Multi-tone Retro Progress Bar with Playhead Marker ◆
    let bar_width = progress_bar_width(cols[0].width, time_display.chars().count());
    let filled = ((pct * bar_width as f64).round() as usize).min(bar_width);
    let head = if has_track && bar_width > 0 { 1 } else { 0 };
    let played_len = filled.saturating_sub(head);
    let remain_len = bar_width.saturating_sub(played_len + head);

    let mut bar_spans = vec![
        Span::styled("[", Style::default().fg(palette.border_dim)),
        Span::styled("━".repeat(played_len), Style::default().fg(palette.accent)),
    ];
    if head > 0 {
        bar_spans.push(Span::styled(
            "◆",
            Style::default().fg(palette.accent_bright).bold(),
        ));
    }
    bar_spans.push(Span::styled(
        "─".repeat(remain_len),
        Style::default().fg(palette.border_dim),
    ));
    bar_spans.push(Span::styled("] ", Style::default().fg(palette.border_dim)));
    bar_spans.push(Span::styled(
        time_display.clone(),
        Style::default().fg(palette.text_secondary),
    ));
    lines.push(Line::from(bar_spans));

    f.render_widget(Paragraph::new(lines), cols[0]);

    let vinyl_box = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Length(2)])
        .split(cols[1]);

    let vinyl = VinylRecord::new(is_playing, app.frame_counter, palette);
    f.render_widget(vinyl, vinyl_box[0]);

    let vinyl_time = Line::from(vec![Span::styled(
        time_display,
        Style::default().fg(palette.accent).bold(),
    )]);
    f.render_widget(
        Paragraph::new(vinyl_time).alignment(Alignment::Center),
        vinyl_box[1],
    );
}

fn render_spectrum_pane(f: &mut Frame, app: &App, area: Rect, palette: &ThemePalette) {
    let block = Block::default()
        .title(" LIVE SPECTRUM VISUALIZER ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(palette.border));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visualizer = SpectrumVisualizer::new(&app.spectrum, palette);
    f.render_widget(visualizer, inner);
}

fn render_player_bottom_bar(f: &mut Frame, area: Rect, palette: &ThemePalette) {
    let text = player_bottom_bar_line(area.width.saturating_sub(2), palette);

    let block = Block::default()
        .title(" CONTROLS ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.border_dim));

    f.render_widget(
        Paragraph::new(text).alignment(Alignment::Center).block(block),
        area,
    );
}

fn now_playing_height(body_height: u16) -> u16 {
    if body_height < 20 { 12 } else { 14 }
}

fn progress_bar_width(col_width: u16, time_display_chars: usize) -> usize {
    (col_width as usize)
        .saturating_sub(3 + time_display_chars)
        .min(30)
}

fn top_bar_constraints(area_width: u16, left_chars: u16, right_chars: u16) -> [Constraint; 3] {
    let side = left_chars.max(right_chars);
    if area_width >= side.saturating_mul(2).saturating_add(22) {
        [
            Constraint::Length(side),
            Constraint::Min(0),
            Constraint::Length(side),
        ]
    } else {
        [
            Constraint::Length(left_chars),
            Constraint::Min(0),
            Constraint::Length(right_chars),
        ]
    }
}

type HotkeyItem = (&'static str, &'static str, Color);
type HotkeyTier<'a> = (&'a [HotkeyItem], &'static str);

fn build_hotkey_line(items: &[HotkeyItem], sep: &'static str) -> Line<'static> {
    let mut spans = Vec::with_capacity(items.len() * 3);
    for (idx, &(key, label, color)) in items.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw(sep));
        }
        spans.push(Span::styled(key, Style::default().fg(color).bold()));
        if !label.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::raw(label));
        }
    }
    Line::from(spans)
}

fn pick_fitting_hotkey_line(tiers: &[HotkeyTier<'_>], max_width: u16) -> Line<'static> {
    for &(items, sep) in tiers {
        let line = build_hotkey_line(items, sep);
        if line.width() <= max_width as usize {
            return line;
        }
    }
    let &(items, sep) = tiers.last().expect("at least one hotkey tier");
    build_hotkey_line(items, sep)
}

fn menu_bottom_bar_line(inner_width: u16, palette: &ThemePalette) -> Line<'static> {
    let full: [(&'static str, &'static str, Color); 8] = [
        ("[Space]", "Mark", palette.accent),
        ("[Enter]", "Open", palette.badge_ok),
        ("[B]", "Browse", palette.accent),
        ("[A]", "Add", palette.accent),
        ("[D]", "Del", palette.accent),
        ("[T]", "Theme", palette.badge_special),
        ("[?]", "Help", palette.badge_warn),
        ("[Q]", "Quit", palette.badge_err),
    ];
    let compact: [(&'static str, &'static str, Color); 8] = [
        ("[Space]", "Mark", palette.accent),
        ("[Enter]", "Open", palette.badge_ok),
        ("[B]", "Browse", palette.accent),
        ("[A]", "Add", palette.accent),
        ("[D]", "Del", palette.accent),
        ("[T]", "Theme", palette.badge_special),
        ("[?]", "Help", palette.badge_warn),
        ("[Q]", "", palette.badge_err),
    ];
    let minimal: [(&'static str, &'static str, Color); 5] = [
        ("[Enter]", "Open", palette.badge_ok),
        ("[B]", "Browse", palette.accent),
        ("[T]", "Theme", palette.badge_special),
        ("[?]", "Help", palette.badge_warn),
        ("[Q]", "Quit", palette.badge_err),
    ];
    pick_fitting_hotkey_line(
        &[(&full, "  "), (&compact, " "), (&minimal, " ")],
        inner_width,
    )
}

fn player_bottom_bar_line(inner_width: u16, palette: &ThemePalette) -> Line<'static> {
    let full: [(&'static str, &'static str, Color); 11] = [
        ("[Space]", "Play", palette.accent),
        ("[n/p]", "Track", palette.accent),
        ("[x]", "Stop", palette.accent),
        ("[←/→]", "Seek", palette.accent),
        ("[+/-]", "Vol", palette.accent),
        ("[s]", "Shuf", palette.accent),
        ("[r]", "Rep", palette.accent),
        ("[t]", "Theme", palette.badge_special),
        ("[/]", "Find", palette.badge_warn),
        ("[?]", "Help", palette.badge_warn),
        ("[Q]", "Quit", palette.badge_err),
    ];
    let medium: [(&'static str, &'static str, Color); 9] = [
        ("[Space]", "Play", palette.accent),
        ("[n/p]", "Track", palette.accent),
        ("[x]", "Stop", palette.accent),
        ("[←/→]", "Seek", palette.accent),
        ("[+/-]", "Vol", palette.accent),
        ("[t]", "Theme", palette.badge_special),
        ("[/]", "Find", palette.badge_warn),
        ("[?]", "Help", palette.badge_warn),
        ("[Q]", "Quit", palette.badge_err),
    ];
    let compact: [(&'static str, &'static str, Color); 8] = [
        ("[Space]", "Play", palette.accent),
        ("[n/p]", "Track", palette.accent),
        ("[←/→]", "Seek", palette.accent),
        ("[+/-]", "Vol", palette.accent),
        ("[t]", "Theme", palette.badge_special),
        ("[/]", "Find", palette.badge_warn),
        ("[?]", "Help", palette.badge_warn),
        ("[Q]", "", palette.badge_err),
    ];
    let minimal: [(&'static str, &'static str, Color); 5] = [
        ("[Space]", "Play", palette.accent),
        ("[n/p]", "Track", palette.accent),
        ("[/]", "Find", palette.badge_warn),
        ("[?]", "Help", palette.badge_warn),
        ("[Q]", "Quit", palette.badge_err),
    ];
    pick_fitting_hotkey_line(
        &[(&full, "  "), (&medium, " "), (&compact, " "), (&minimal, " ")],
        inner_width,
    )
}

fn render_help_modal(f: &mut Frame, screen: AppScreen, palette: &ThemePalette) {
    let area = f.area();
    let w = area.width.saturating_sub(4).clamp(36, 66);
    let h = area.height.saturating_sub(4).clamp(12, 20);
    let popup_area = centered_fixed_rect(w, h, area);
    f.render_widget(Clear, popup_area);

    let key_style = Style::default().fg(palette.accent_bright).bold();
    let desc_style = Style::default().fg(palette.text_secondary);

    let mut lines = match screen {
        AppScreen::Menu => vec![
            Line::from(Span::styled("LAUNCHER MENU", Style::default().fg(palette.badge_warn).bold())),
            Line::from(vec![Span::styled("  ↑/↓ or j/k   ", key_style), Span::styled("Move cursor", desc_style)]),
            Line::from(vec![Span::styled("  Space / Enter", key_style), Span::styled("Mark route / Open directory", desc_style)]),
            Line::from(vec![Span::styled("  1..9         ", key_style), Span::styled("Quick launch saved route", desc_style)]),
            Line::from(vec![Span::styled("  b / a / d    ", key_style), Span::styled("Browse / Add / Delete route", desc_style)]),
            Line::from(vec![Span::styled("  t            ", key_style), Span::styled("Cycle retro color theme", desc_style)]),
            Line::from(vec![Span::styled("  q / Esc      ", key_style), Span::styled("Quit application", desc_style)]),
        ],
        AppScreen::Player => vec![
            Line::from(Span::styled("MUSIC PLAYER", Style::default().fg(palette.badge_warn).bold())),
            Line::from(vec![Span::styled("  Space / Enter", key_style), Span::styled("Play/Pause or Play selected track", desc_style)]),
            Line::from(vec![Span::styled("  x            ", key_style), Span::styled("Stop playback", desc_style)]),
            Line::from(vec![Span::styled("  n / p (or b) ", key_style), Span::styled("Next / Previous track", desc_style)]),
            Line::from(vec![Span::styled("  ↑/↓ or j/k   ", key_style), Span::styled("Navigate playlist", desc_style)]),
            Line::from(vec![Span::styled("  ← / →        ", key_style), Span::styled("Seek -5s / +5s", desc_style)]),
            Line::from(vec![Span::styled("  + / - / m    ", key_style), Span::styled("Volume Up/Down / Mute", desc_style)]),
            Line::from(vec![Span::styled("  s / r        ", key_style), Span::styled("Toggle Shuffle / Cycle Repeat (ALL/ONE/ONCE/OFF)", desc_style)]),
            Line::from(vec![Span::styled("  t / /        ", key_style), Span::styled("Cycle Theme / Search playlist", desc_style)]),
            Line::from(vec![Span::styled("  Esc / q      ", key_style), Span::styled("Back to Menu / Quit (Ctrl+C always quits)", desc_style)]),
        ],
    };
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press [?], [Enter], or [Esc] to close",
        Style::default().fg(palette.text_muted),
    )));

    let block = Block::default()
        .title(" ◈ KEYBINDINGS HELP ◈ ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(palette.border));

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        popup_area,
    );
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

/// Helper function to create a centered popup area by percentage
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

/// Helper function to create a centered popup area with fixed dimensions clamped to container
fn centered_fixed_rect(width: u16, height: u16, r: Rect) -> Rect {
    let w = width.min(r.width);
    let h = height.min(r.height);
    let x = r.x + r.width.saturating_sub(w) / 2;
    let y = r.y + r.height.saturating_sub(h) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_chars_preserves_short_and_adds_ellipsis_for_long() {
        assert_eq!(truncate_chars("Hello", 10), "Hello");
        assert_eq!(truncate_chars("Hello World", 5), "Hell…");
        assert_eq!(truncate_chars("سلام دنیا", 4), "سلا…");
        assert_eq!(truncate_chars("Test", 0), "");
    }

    #[test]
    fn test_centered_fixed_rect_clamps_to_area() {
        let area = Rect::new(0, 0, 40, 10);
        let popup = centered_fixed_rect(60, 15, area);
        assert_eq!(popup.width, 40);
        assert_eq!(popup.height, 10);
    }

    #[test]
    fn test_is_plain_char_rejects_ctrl_and_alt_modifiers() {
        assert!(is_plain_char(KeyModifiers::NONE));
        assert!(is_plain_char(KeyModifiers::SHIFT));
        assert!(!is_plain_char(KeyModifiers::CONTROL));
        assert!(!is_plain_char(KeyModifiers::ALT));
    }

    #[test]
    fn test_layout_bars_fit_within_80_and_120_columns() {
        let palette = ThemePalette::for_kind(config::ThemeKind::CyanDeck);

        // 80-col terminal -> inner width inside bordered box is 78
        let player_line_80 = player_bottom_bar_line(78, &palette);
        assert!(
            player_line_80.width() <= 78,
            "player bottom bar width {} exceeded 78",
            player_line_80.width()
        );

        let menu_line_80 = menu_bottom_bar_line(78, &palette);
        assert!(
            menu_line_80.width() <= 78,
            "menu bottom bar width {} exceeded 78",
            menu_line_80.width()
        );

        // 120-col terminal -> full bar fits inside 118
        let player_line_120 = player_bottom_bar_line(118, &palette);
        assert!(player_line_120.width() <= 118);
        assert!(player_line_120.to_string().contains("[s] Shuf"));
    }

    #[test]
    fn test_progress_bar_width_never_overflows_narrow_column() {
        // col_width = 22, time = "01:45 / 03:20" (13 chars), brackets+space = 3 chars -> max bar = 6
        let w = progress_bar_width(22, 13);
        assert!(w + 3 + 13 <= 22, "bar {} + 16 overflowed 22", w);
        assert_eq!(w, 6);
        assert_eq!(progress_bar_width(60, 13), 30);
    }

    #[test]
    fn test_now_playing_height_compacts_for_80x24_terminal() {
        assert_eq!(now_playing_height(19), 12);
        assert_eq!(now_playing_height(18), 12);
        assert_eq!(now_playing_height(24), 14);
    }

    #[test]
    fn test_top_bar_constraints_give_right_side_enough_room_at_80_cols() {
        let rects = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(top_bar_constraints(80, 24, 29))
            .split(Rect::new(0, 0, 80, 1));
        assert!(rects[0].width >= 24);
        assert!(rects[1].width >= 21);
        assert!(rects[2].width >= 29);
    }
}
