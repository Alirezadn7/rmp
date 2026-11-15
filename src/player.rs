//! Audio playback engine wrapping `rodio`.
//!
//! Provides a safe abstraction over `OutputStream`, `OutputStreamHandle`, and `Sink`.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

/// Audio player handling decoding and playback controls.
pub struct Player {
    /// Preserved audio stream. When `_stream` is dropped, the cpal backend audio
    /// output thread terminates, immediately stopping all playback.
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Sink,
    volume: f32,
    current_path: Option<PathBuf>,
}

impl Player {
    /// Initialize the audio device output stream and playback sink.
    pub fn new() -> Result<Self> {
        let (_stream, stream_handle) = OutputStream::try_default()
            .context("Failed to initialize default audio output device")?;

        let sink = Sink::try_new(&stream_handle)
            .context("Failed to create audio playback sink")?;

        let default_volume = 0.8; // 80% default volume
        sink.set_volume(default_volume);

        Ok(Self {
            _stream,
            stream_handle,
            sink,
            volume: default_volume,
            current_path: None,
        })
    }

    /// Load and play an audio track from the given file path.
    ///
    /// Stops any active track, recreates a fresh `Sink` to clear previous state,
    /// decodes the audio source, and begins playback on rodio's background thread.
    pub fn play<P: AsRef<Path>>(&mut self, track_path: P) -> Result<()> {
        let path = track_path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("Failed to open audio file: {}", path.display()))?;

        let reader = BufReader::new(file);
        let source = Decoder::new(reader)
            .with_context(|| format!("Failed to decode audio file (unsupported format or corrupt): {}", path.display()))?;

        // Recreate the sink to cleanly flush any stopped or residual state from the previous track.
        self.sink.stop();
        self.sink = Sink::try_new(&self.stream_handle)
            .context("Failed to create audio playback sink for new track")?;
        self.sink.set_volume(self.volume);

        self.sink.append(source);
        self.sink.play();
        self.current_path = Some(path.to_path_buf());

        Ok(())
    }

    /// Pause the current playback.
    pub fn pause(&self) {
        self.sink.pause();
    }

    /// Resume the current playback if paused.
    pub fn resume(&self) {
        self.sink.play();
    }

    /// Toggle between pause and resume. Returns `true` if now paused, `false` if playing.
    pub fn toggle_pause(&self) -> bool {
        if self.is_paused() {
            self.resume();
            false
        } else {
            self.pause();
            true
        }
    }

    /// Stop playback immediately and empty the queue.
    pub fn stop(&mut self) {
        self.sink.stop();
        self.current_path = None;
    }

    /// Set the playback volume (clamped between `0.0` [0%] and `1.0` [100%]).
    pub fn set_volume(&mut self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.volume = clamped;
        self.sink.set_volume(clamped);
    }

    /// Increase volume by 5%.
    pub fn volume_up(&mut self) {
        self.set_volume(self.volume + 0.05);
    }

    /// Decrease volume by 5%.
    pub fn volume_down(&mut self) {
        self.set_volume(self.volume - 0.05);
    }

    /// Get the current volume as a normalized float (0.0 to 1.0).
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Returns `true` if the player is currently paused.
    pub fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    /// Returns `true` if the queue is empty (no sound is currently queued or playing).
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.sink.empty()
    }

    /// Returns the path of the track currently loaded, if any.
    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }
}
