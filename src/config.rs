//! Configuration management for rmp.
//!
//! Stores user settings, saved directory routes, volume, shuffle, repeat,
//! and last session path in `~/.config/rmp/config.toml`.

use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
    Once,
}

impl RepeatMode {
    pub fn next(&self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Once,
            RepeatMode::Once => RepeatMode::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RepeatMode::Off => "OFF",
            RepeatMode::All => "ALL",
            RepeatMode::One => "ONE",
            RepeatMode::Once => "ONCE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    CyanDeck,
    AmberCrt,
    MatrixGreen,
    Synthwave,
}

impl ThemeKind {
    pub fn next(&self) -> Self {
        match self {
            ThemeKind::CyanDeck => ThemeKind::AmberCrt,
            ThemeKind::AmberCrt => ThemeKind::MatrixGreen,
            ThemeKind::MatrixGreen => ThemeKind::Synthwave,
            ThemeKind::Synthwave => ThemeKind::CyanDeck,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeKind::CyanDeck => "cyan",
            ThemeKind::AmberCrt => "amber",
            ThemeKind::MatrixGreen => "matrix",
            ThemeKind::Synthwave => "synthwave",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ThemeKind::CyanDeck => "CYAN",
            ThemeKind::AmberCrt => "AMBER",
            ThemeKind::MatrixGreen => "MATRIX",
            ThemeKind::Synthwave => "SYNTH",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "amber" | "amber_crt" => ThemeKind::AmberCrt,
            "matrix" | "matrix_green" | "green" => ThemeKind::MatrixGreen,
            "synthwave" | "synth" | "neon" => ThemeKind::Synthwave,
            _ => ThemeKind::CyanDeck,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub last_session: Option<String>,
    pub routes: Vec<String>,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub theme: ThemeKind,
}

impl Default for AppConfig {
    fn default() -> Self {
        let default_routes = vec![
            "~/Music".to_string(),
            "~/Downloads/audio".to_string(),
            "/media/external/tracks".to_string(),
        ];

        Self {
            last_session: Some("~/Music".to_string()),
            routes: default_routes,
            volume: 85,
            shuffle: false,
            repeat: RepeatMode::All,
            theme: ThemeKind::CyanDeck,
        }
    }
}

impl AppConfig {
    /// Returns the standard config directory: `~/.config/rmp/`
    pub fn config_dir() -> PathBuf {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("rmp")
    }

    /// Path to `config.toml`
    pub fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Load config from disk or create default if not found
    pub fn load() -> Self {
        let path = Self::config_file_path();
        if !path.exists() {
            let cfg = Self::default();
            let _ = cfg.save();
            return cfg;
        }

        match fs::read_to_string(&path) {
            Ok(content) => Self::parse_toml(&content),
            Err(_) => Self::default(),
        }
    }

    /// Save current config to disk
    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }

        let mut toml = String::new();
        toml.push_str("# rmp configuration file\n\n");

        if let Some(ref ls) = self.last_session {
            toml.push_str(&format!("last_session = \"{}\"\n", escape_string(ls)));
        }

        toml.push_str(&format!("volume = {}\n", self.volume));
        toml.push_str(&format!("shuffle = {}\n", self.shuffle));
        toml.push_str(&format!("repeat = \"{}\"\n", self.repeat.as_str()));
        toml.push_str(&format!("theme = \"{}\"\n\n", self.theme.as_str()));

        toml.push_str("routes = [\n");
        for r in &self.routes {
            toml.push_str(&format!("    \"{}\",\n", escape_string(r)));
        }
        toml.push_str("]\n");

        fs::write(Self::config_file_path(), toml)?;
        Ok(())
    }

    fn parse_toml(content: &str) -> Self {
        let mut cfg = Self::default();
        let mut in_routes = false;
        let mut saw_routes = false;
        let mut custom_routes = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if in_routes {
                let (items, closed) = extract_quoted_array_segment(trimmed);
                custom_routes.extend(items);
                if closed {
                    in_routes = false;
                }
                continue;
            }

            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim();
                let val = val.trim();

                match key {
                    "last_session" => {
                        cfg.last_session = extract_quoted_string(val);
                    }
                    "volume" => {
                        if let Ok(v) = val.parse::<u8>() {
                            cfg.volume = v.min(100);
                        }
                    }
                    "shuffle" => {
                        cfg.shuffle = val.starts_with("true");
                    }
                    "repeat" => {
                        if let Some(r_str) = extract_quoted_string(val) {
                            cfg.repeat = match r_str.to_uppercase().as_str() {
                                "ONE" => RepeatMode::One,
                                "ONCE" | "SINGLE" => RepeatMode::Once,
                                "OFF" => RepeatMode::Off,
                                _ => RepeatMode::All,
                            };
                        }
                    }
                    "theme" => {
                        if let Some(t_str) = extract_quoted_string(val) {
                            cfg.theme = ThemeKind::from_str(&t_str);
                        }
                    }
                    "routes" if val.starts_with('[') => {
                        saw_routes = true;
                        let after_open = &val[1..];
                        let (items, closed) = extract_quoted_array_segment(after_open);
                        custom_routes.extend(items);
                        in_routes = !closed;
                    }
                    _ => {}
                }
            }
        }

        if saw_routes {
            cfg.routes = custom_routes;
        }

        cfg
    }
}

/// Helper to get user home directory
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Expands `~` into user's home directory
pub fn resolve_path(path_str: &str) -> PathBuf {
    if let Some(stripped) = path_str.strip_prefix("~/").or_else(|| path_str.strip_prefix("~\\")) {
        if let Some(home) = home_dir() {
            return home.join(stripped);
        }
    } else if path_str == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    PathBuf::from(path_str)
}

/// Formats path replacing home directory with `~` if applicable
pub fn display_path(path: &Path) -> String {
    if let Some(home) = home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display()).replace('\\', "/");
        }
    }
    path.display().to_string().replace('\\', "/")
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse_next_quoted(s: &str) -> Option<(String, usize)> {
    let start = s.find('"')?;
    let mut out = String::new();
    let mut escaped = false;
    let mut iter = s[start + 1..].char_indices();

    for (rel_idx, ch) in iter.by_ref() {
        if escaped {
            match ch {
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some((out, start + 1 + rel_idx + 1));
        } else {
            out.push(ch);
        }
    }
    None
}

fn extract_quoted_string(s: &str) -> Option<String> {
    parse_next_quoted(s).map(|(val, _)| val)
}

fn extract_quoted_array_segment(s: &str) -> (Vec<String>, bool) {
    let mut items = Vec::new();
    let mut rest = s;
    loop {
        // Stop at an unquoted closing bracket or comment before the next quote
        let next_quote = rest.find('"');
        let next_close = rest.find(']');
        match (next_quote, next_close) {
            (Some(q), Some(c)) if c < q => return (items, true),
            (None, Some(_)) => return (items, true),
            (Some(_), _) => {
                if let Some((val, consumed)) = parse_next_quoted(rest) {
                    items.push(val);
                    rest = &rest[consumed..];
                } else {
                    return (items, false);
                }
            }
            (None, None) => return (items, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repeat_mode_cycle_includes_once() {
        let mode = RepeatMode::Off;
        assert_eq!(mode.next(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next(), RepeatMode::Once);
        assert_eq!(RepeatMode::Once.next(), RepeatMode::Off);
        assert_eq!(RepeatMode::Once.as_str(), "ONCE");
    }

    #[test]
    fn test_theme_kind_cycle_and_parse() {
        assert_eq!(ThemeKind::CyanDeck.next(), ThemeKind::AmberCrt);
        assert_eq!(ThemeKind::AmberCrt.next(), ThemeKind::MatrixGreen);
        assert_eq!(ThemeKind::MatrixGreen.next(), ThemeKind::Synthwave);
        assert_eq!(ThemeKind::Synthwave.next(), ThemeKind::CyanDeck);
        assert_eq!(ThemeKind::from_str("amber"), ThemeKind::AmberCrt);
        assert_eq!(ThemeKind::from_str("MATRIX"), ThemeKind::MatrixGreen);
        assert_eq!(ThemeKind::from_str("synthwave"), ThemeKind::Synthwave);
        assert_eq!(ThemeKind::from_str("unknown"), ThemeKind::CyanDeck);
    }

    #[test]
    fn test_parse_toml_preserves_empty_routes_and_settings() {
        let content = r#"
volume = 42
shuffle = true
repeat = "ONCE"
theme = "amber"
routes = []
"#;
        let cfg = AppConfig::parse_toml(content);
        assert_eq!(cfg.volume, 42);
        assert!(cfg.shuffle);
        assert_eq!(cfg.repeat, RepeatMode::Once);
        assert_eq!(cfg.theme, ThemeKind::AmberCrt);
        assert!(cfg.routes.is_empty());
    }

    #[test]
    fn test_parse_toml_escaped_quotes_and_closing_bracket_item() {
        let content = r#"
last_session = "C:\\Music\\My \"Best\" Mix"
routes = [
    "~/Music",
    "~/Audio, Vol. 1" ]
"#;
        let cfg = AppConfig::parse_toml(content);
        assert_eq!(
            cfg.last_session.as_deref(),
            Some("C:\\Music\\My \"Best\" Mix")
        );
        assert_eq!(
            cfg.routes,
            vec!["~/Music".to_string(), "~/Audio, Vol. 1".to_string()]
        );
    }
}
