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
}

impl RepeatMode {
    pub fn next(&self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RepeatMode::Off => "OFF",
            RepeatMode::All => "ALL",
            RepeatMode::One => "ONE",
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
        toml.push_str(&format!("repeat = \"{}\"\n\n", self.repeat.as_str()));

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
        let mut custom_routes = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if in_routes {
                if trimmed.contains(']') {
                    in_routes = false;
                } else if let Some(val) = extract_quoted_string(trimmed) {
                    custom_routes.push(val);
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
                                "OFF" => RepeatMode::Off,
                                _ => RepeatMode::All,
                            };
                        }
                    }
                    "routes" => {
                        if val.starts_with('[') {
                            if val.ends_with(']') {
                                // inline array
                                let inner = &val[1..val.len() - 1];
                                for part in inner.split(',') {
                                    if let Some(s) = extract_quoted_string(part.trim()) {
                                        custom_routes.push(s);
                                    }
                                }
                            } else {
                                in_routes = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if !custom_routes.is_empty() {
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

fn extract_quoted_string(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let end = s[start + 1..].find('"')?;
    Some(s[start + 1..start + 1 + end].replace("\\\\", "\\").replace("\\\"", "\""))
}
