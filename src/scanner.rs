//! Directory scanner module for discovering supported audio files.

use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use anyhow::{bail, Result};

/// Supported audio file extensions (lowercase).
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "ogg"];

/// Basic metadata extracted from an audio file.
#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub file_size_bytes: u64,
    pub extension: String,
}

/// Represents an audio track discovered on disk.
#[derive(Debug, Clone)]
pub struct Track {
    pub path: PathBuf,
    pub file_name: String,
    pub metadata: Option<TrackMetadata>,
}

impl Track {
    /// Construct a new `Track` from a path, gathering basic metadata if possible.
    pub fn new(path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown Track")
            .to_string();

        let metadata = fs::metadata(&path).ok().map(|meta| {
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            TrackMetadata {
                file_size_bytes: meta.len(),
                extension,
            }
        });

        Self {
            path,
            file_name,
            metadata,
        }
    }

    /// Format file details (extension and size) for display (e.g., "MP3, 5.42 MB").
    pub fn format_details(&self) -> String {
        let size = self.formatted_size();
        match &self.metadata {
            Some(meta) => {
                let ext_upper = meta.extension.to_uppercase();
                format!("{}, {}", ext_upper, size)
            }
            None => size,
        }
    }

    /// Format file size for user-friendly display (e.g., "5.42 MB").
    pub fn formatted_size(&self) -> String {
        match &self.metadata {
            Some(meta) => {
                let mb = meta.file_size_bytes as f64 / (1024.0 * 1024.0);
                format!("{:.2} MB", mb)
            }
            None => "Unknown size".to_string(),
        }
    }
}

/// Recursively scans the specified directory for supported audio files.
///
/// Returns a sorted vector of `Track`s matching supported audio extensions.
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
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let path = entry.into_path();
            if is_supported_audio(&path) {
                tracks.push(Track::new(path));
            }
        }
    }

    // Sort alphabetically by file name (case-insensitive) for clean presentation
    tracks.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));

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
        assert!(is_supported_audio(Path::new("dir/sub/song.wav")));
        assert!(is_supported_audio(Path::new("dir/sub/song.ogg")));

        assert!(!is_supported_audio(Path::new("song.txt")));
        assert!(!is_supported_audio(Path::new("song.mp4")));
        assert!(!is_supported_audio(Path::new("no_extension")));
    }

    #[test]
    fn test_track_creation() {
        let track = Track::new(PathBuf::from("test_audio.mp3"));
        assert_eq!(track.file_name, "test_audio.mp3");
    }
}
