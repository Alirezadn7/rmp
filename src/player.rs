//! Audio playback engine wrapping `rodio`.
//!
//! Provides a safe abstraction over `OutputStream`, `OutputStreamHandle`, and `Sink`,
//! with support for real-time position tracking, seeking, mute toggling, and volume control.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;
use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

/// Audio player handling decoding, seeking, and playback controls.
pub struct Player {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Sink,
    volume: f32,
    saved_volume: f32,
    is_muted: bool,
    current_path: Option<PathBuf>,
    total_duration: Option<Duration>,
}

impl Player {
    /// Initialize the audio device output stream and playback sink.
    pub fn new() -> Result<Self> {
        let (_stream, stream_handle) = OutputStream::try_default()
            .context("Failed to initialize default audio output device")?;

        let sink = Sink::try_new(&stream_handle)
            .context("Failed to create audio playback sink")?;

        let default_volume = 0.85; // 85% default matching UI
        sink.set_volume(default_volume);

        Ok(Self {
            _stream,
            stream_handle,
            sink,
            volume: default_volume,
            saved_volume: default_volume,
            is_muted: false,
            current_path: None,
            total_duration: None,
        })
    }

    /// Load and play an audio track from the given file path.
    /// Returns the exact total duration if provided by the decoder.
    pub fn play<P: AsRef<Path>>(&mut self, track_path: P) -> Result<Option<Duration>> {
        let path = track_path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("Failed to open audio file: {}", path.display()))?;

        let reader = BufReader::new(file);
        let source = Decoder::new(reader)
            .with_context(|| format!("Failed to decode audio file: {}", path.display()))?;

        let duration = source.total_duration();

        // Recreate the sink to cleanly flush any previous track state
        self.sink.stop();
        self.sink = Sink::try_new(&self.stream_handle)
            .context("Failed to create audio playback sink for new track")?;

        let effective_vol = if self.is_muted { 0.0 } else { self.volume };
        self.sink.set_volume(effective_vol);

        self.sink.append(source);
        self.sink.play();
        self.current_path = Some(path.to_path_buf());
        self.total_duration = duration;

        Ok(duration)
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
    pub fn toggle_pause(&mut self) -> bool {
        if self.is_paused() {
            self.resume();
            false
        } else {
            self.pause();
            true
        }
    }

    /// Stop playback immediately.
    pub fn stop(&mut self) {
        self.sink.stop();
        self.current_path = None;
        self.total_duration = None;
    }

    /// Set volume (clamped 0.0 to 1.0)
    pub fn set_volume(&mut self, vol: f32) {
        let clamped = vol.clamp(0.0, 1.0);
        self.volume = clamped;
        if self.is_muted {
            self.is_muted = false;
        }
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

    /// Toggle Mute state. Returns new muted boolean.
    pub fn toggle_mute(&mut self) -> bool {
        if self.is_muted {
            self.is_muted = false;
            self.sink.set_volume(self.saved_volume);
        } else {
            self.saved_volume = self.volume;
            self.is_muted = true;
            self.sink.set_volume(0.0);
        }
        self.is_muted
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    /// Returns `true` if no sound is currently queued or playing (track ended).
    pub fn is_empty(&self) -> bool {
        self.sink.empty()
    }

    /// Current playback position elapsed.
    pub fn elapsed(&self) -> Duration {
        self.sink.get_pos()
    }

    /// Total track duration if known.
    pub fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    /// Seek forward by seconds.
    pub fn seek_forward(&mut self, secs: u64) {
        let cur = self.elapsed();
        let target = cur + Duration::from_secs(secs);
        let clamped = if let Some(tot) = self.total_duration {
            target.min(tot)
        } else {
            target
        };
        let _ = self.sink.try_seek(clamped);
    }

    /// Seek backward by seconds.
    pub fn seek_backward(&mut self, secs: u64) {
        let cur = self.elapsed();
        let step = Duration::from_secs(secs);
        let target = if cur > step {
            cur - step
        } else {
            Duration::ZERO
        };
        let _ = self.sink.try_seek(target);
    }

    #[allow(dead_code)]
    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }
}
