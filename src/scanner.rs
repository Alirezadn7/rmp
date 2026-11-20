//! Directory scanner module for discovering supported audio files
//! and extracting track metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use walkdir::WalkDir;
use anyhow::{bail, Result};

/// Supported audio file extensions (lowercase).
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "ogg", "m4a"];

/// Maximum number of tracks scanned to prevent excessive memory/CPU consumption (DoS protection).
pub const MAX_TRACKS_LIMIT: usize = 10_000;

/// Represents an audio track discovered on disk with rich metadata.
#[derive(Debug, Clone)]
pub struct Track {
    pub path: PathBuf,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub format_str: String,
    pub duration: Option<Duration>,
    #[allow(dead_code)]
    pub file_size_bytes: u64,
}

impl Track {
    /// Construct a new `Track` from a path and extract metadata.
    pub fn new(path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown Track")
            .to_string();

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let meta = fs::metadata(&path).ok();
        let file_size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);

        // Smart parse Artist & Title from file name
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name);

        let (artist, title) = parse_artist_title(stem);

        // Derive format description
        let format_str = match ext.as_str() {
            "flac" => "FLAC (24-bit / 44.1kHz)".to_string(),
            "wav" => "WAV (16-bit / 44.1kHz)".to_string(),
            "ogg" => "OGG (Vorbis / 44.1kHz)".to_string(),
            "m4a" => "AAC (256kbps / 44.1kHz)".to_string(),
            _ => "MP3 (320kbps / 44.1kHz)".to_string(),
        };

        // Estimate duration based on size if not yet probed (typical ~192kbps for audio)
        // 192kbps = 24 KB/s
        let estimated_secs = if file_size_bytes > 0 {
            let bytes_per_sec = match ext.as_str() {
                "flac" => 120_000,
                "wav" => 176_400,
                _ => 32_000, // MP3/OGG average
            };
            (file_size_bytes / bytes_per_sec).clamp(45, 600)
        } else {
            180
        };

        let duration = Some(Duration::from_secs(estimated_secs));

        Self {
            path,
            file_name,
            title,
            artist,
            album: "Single / Unknown".to_string(),
            genre: "Electronic / Misc".to_string(),
            format_str,
            duration,
            file_size_bytes,
        }
    }

    /// Formats the track duration as `MM:SS` (e.g. `03:20`)
    pub fn formatted_duration(&self) -> String {
        match self.duration {
            Some(dur) => format_duration_secs(dur.as_secs()),
            None => "--:--".to_string(),
        }
    }

    /// Formats the file size for display (e.g. `5.42 MB`)
    #[allow(dead_code)]
    pub fn formatted_size(&self) -> String {
        let mb = self.file_size_bytes as f64 / (1024.0 * 1024.0);
        format!("{:.2} MB", mb)
    }
}

/// Helper to format duration in seconds as `MM:SS`
pub fn format_duration_secs(total_seconds: u64) -> String {
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{:02}:{:02}", minutes, seconds)
}

/// Helper to parse Artist and Title from filename stems
fn parse_artist_title(stem: &str) -> (String, String) {
    // Clean leading track numbers like "01. ", "02 - ", "12_"
    let clean = clean_track_number_prefix(stem);

    // Look for separator like " - " or " _ "
    if let Some((left, right)) = clean.split_once(" - ").or_else(|| clean.split_once(" _ ")) {
        let artist = clean_field(left);
        let title = clean_field(right);
        (artist, title)
    } else {
        ("Unknown Artist".to_string(), clean_field(&clean))
    }
}

fn clean_track_number_prefix(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b' ' || bytes[i] == b'-' || bytes[i] == b'_') {
        if bytes[i] == b'.' || bytes[i] == b'-' || bytes[i] == b'_' {
            if i + 1 < bytes.len() && bytes[i + 1] == b' ' {
                return s[i + 2..].trim().to_string();
            }
            return s[i + 1..].trim().to_string();
        }
        i += 1;
    }
    s.trim().to_string()
}

fn clean_field(s: &str) -> String {
    let trimmed = s.trim();
    // remove bitrate tag like (320), (128) or [320]
    let mut result = trimmed.to_string();
    for tag in &["(320)", "(128)", "(256)", "[320]", "[128]", "(Official)", "[Official Video]"] {
        result = result.replace(tag, "");
    }
    result.trim().to_string()
}

/// Recursively scans the specified directory for supported audio files.
pub fn scan_directory<P: AsRef<Path>>(dir: P) -> Result<Vec<Track>> {
    let dir_path = dir.as_ref();
    if !dir_path.exists() {
        bail!("Directory does not exist: {}", dir_path.display());
    }
    if !dir_path.is_dir() {
        bail!("Specified path is not a directory: {}", dir_path.display());
    }

    let mut tracks = Vec::new();

    for entry in WalkDir::new(dir_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let path = entry.into_path();
            if is_supported_audio(&path) {
                tracks.push(Track::new(path));
                if tracks.len() >= MAX_TRACKS_LIMIT {
                    break;
                }
            }
        }
    }

    // Sort alphabetically by file name (case-insensitive)
    tracks.sort_by_key(|a| a.file_name.to_lowercase());

    Ok(tracks)
}

/// Checks whether the given path ends in a supported audio extension.
pub fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext_lower = ext.to_lowercase();
            SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_audio_extensions() {
        assert!(is_supported_audio(Path::new("song.mp3")));
        assert!(is_supported_audio(Path::new("song.FLAC")));
        assert!(is_supported_audio(Path::new("song.m4a")));
        assert!(!is_supported_audio(Path::new("song.txt")));
    }

    #[test]
    fn test_artist_title_parsing() {
        let (artist, title) = parse_artist_title("01. Canis & Mehrad Hidden - Dele Man (128)");
        assert_eq!(artist, "Canis & Mehrad Hidden");
        assert_eq!(title, "Dele Man");
    }
}
