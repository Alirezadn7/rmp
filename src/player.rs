//! Audio playback engine wrapping `rodio` with non-blocking audio tap for real-time FFT.
//!
//! Provides a safe abstraction over `OutputStream`, `OutputStreamHandle`, and `Sink`,
//! with support for real-time PCM sample tapping, position tracking, seeking, mute toggling,
//! and volume control.

use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::{Context, Result};
use rodio::{source::SeekError, Decoder, OutputStream, OutputStreamHandle, Sink, Source};

/// Maximum PCM sample buffer capacity
pub const AUDIO_BUFFER_CAPACITY: usize = 2048;

/// Type alias for shared thread-safe audio sample buffer
pub type SharedAudioBuffer = Arc<Mutex<VecDeque<f32>>>;

pub fn create_shared_audio_buffer() -> SharedAudioBuffer {
    Arc::new(Mutex::new(VecDeque::with_capacity(AUDIO_BUFFER_CAPACITY)))
}

/// Metadata returned after starting a track.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackInfo {
    pub duration: Option<Duration>,
    pub sample_rate: u32,
}

/// Custom non-blocking audio tap forwarding mono-downmixed PCM frames into a shared
/// circular buffer using `try_lock()` so the audio playback thread never stalls.
pub struct AudioTap<S> {
    input: S,
    buffer: SharedAudioBuffer,
    channels: u16,
    chan_idx: u16,
    sample_acc: f32,
}

impl<S> AudioTap<S>
where
    S: Source<Item = f32>,
{
    pub fn new(input: S, buffer: SharedAudioBuffer) -> Self {
        let channels = input.channels().max(1);
        Self {
            input,
            buffer,
            channels,
            chan_idx: 0,
            sample_acc: 0.0,
        }
    }
}

impl<S> Iterator for AudioTap<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;

        self.sample_acc += sample;
        self.chan_idx += 1;

        if self.chan_idx >= self.channels {
            let mono = self.sample_acc / self.channels as f32;
            self.sample_acc = 0.0;
            self.chan_idx = 0;

            // Non-blocking try_lock: playback thread never stutters if UI thread is reading
            if let Ok(mut lock) = self.buffer.try_lock() {
                if lock.len() >= AUDIO_BUFFER_CAPACITY {
                    lock.pop_front();
                }
                lock.push_back(mono);
            }
        }

        Some(sample)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<S> Source for AudioTap<S>
where
    S: Source<Item = f32> + Send + 'static,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.input.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> std::result::Result<(), SeekError> {
        self.input.try_seek(pos)?;
        if let Ok(mut lock) = self.buffer.try_lock() {
            lock.clear();
        }
        Ok(())
    }
}

/// Audio player handling decoding, seeking, PCM tapping, and playback controls.
pub struct Player {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Sink,
    volume: f32,
    saved_volume: f32,
    is_muted: bool,
    current_path: Option<PathBuf>,
    total_duration: Option<Duration>,
    audio_buffer: SharedAudioBuffer,
}

impl Player {
    /// Initialize the audio device output stream and playback sink with initial volume (0.0..=1.0).
    pub fn with_volume(initial_volume: f32) -> Result<Self> {
        let (_stream, stream_handle) = OutputStream::try_default()
            .context("Failed to initialize default audio output device")?;

        let sink = Sink::try_new(&stream_handle)
            .context("Failed to create audio playback sink")?;

        let vol = initial_volume.clamp(0.0, 1.0);
        sink.set_volume(vol);

        Ok(Self {
            _stream,
            stream_handle,
            sink,
            volume: vol,
            saved_volume: vol,
            is_muted: false,
            current_path: None,
            total_duration: None,
            audio_buffer: create_shared_audio_buffer(),
        })
    }

    /// Access the shared PCM sample buffer for real-time FFT spectrum analysis.
    pub fn audio_buffer(&self) -> SharedAudioBuffer {
        self.audio_buffer.clone()
    }

    /// Load and play an audio track from the given file path.
    /// Returns duration and sample rate for visualizer calibration.
    pub fn play<P: AsRef<Path>>(&mut self, track_path: P) -> Result<PlaybackInfo> {
        let path = track_path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("Failed to open audio file: {}", path.display()))?;

        let reader = BufReader::new(file);
        let source = Decoder::new(reader)
            .with_context(|| format!("Failed to decode audio file: {}", path.display()))?;

        let duration = source.total_duration();
        let sample_rate = source.sample_rate().max(8000);

        // Clear buffer for new track
        if let Ok(mut buf) = self.audio_buffer.lock() {
            buf.clear();
        }

        // Recreate the sink to cleanly flush any previous track state
        self.sink.stop();
        self.sink = Sink::try_new(&self.stream_handle)
            .context("Failed to create audio playback sink for new track")?;

        let effective_vol = if self.is_muted { 0.0 } else { self.volume };
        self.sink.set_volume(effective_vol);

        // Tap the audio stream with sample conversion to f32
        let tapped = AudioTap::new(source.convert_samples::<f32>(), self.audio_buffer.clone());

        self.sink.append(tapped);
        self.sink.play();
        self.current_path = Some(path.to_path_buf());
        self.total_duration = duration;

        Ok(PlaybackInfo {
            duration,
            sample_rate,
        })
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
        if let Ok(mut buf) = self.audio_buffer.lock() {
            buf.clear();
        }
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
    pub fn seek_forward(&mut self, secs: u64) -> Result<()> {
        if self.sink.empty() {
            return Ok(());
        }
        let cur = self.elapsed();
        let target = cur + Duration::from_secs(secs);
        let clamped = if let Some(tot) = self.total_duration {
            target.min(tot)
        } else {
            target
        };
        self.sink
            .try_seek(clamped)
            .map_err(|e| anyhow::anyhow!("Seek failed: {e:?}"))
    }

    /// Seek backward by seconds.
    pub fn seek_backward(&mut self, secs: u64) -> Result<()> {
        if self.sink.empty() {
            return Ok(());
        }
        let cur = self.elapsed();
        let step = Duration::from_secs(secs);
        let target = if cur > step {
            cur - step
        } else {
            Duration::ZERO
        };
        self.sink
            .try_seek(target)
            .map_err(|e| anyhow::anyhow!("Seek failed: {e:?}"))
    }

    #[allow(dead_code)]
    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    #[test]
    fn test_audio_tap_downmixes_stereo_to_mono_preserving_output() {
        let buf = create_shared_audio_buffer();
        // 2 stereo frames: (0.2, 0.6) -> avg 0.4, (-0.4, 0.0) -> avg -0.2
        let src = SamplesBuffer::new(2, 44_100, vec![0.2_f32, 0.6, -0.4, 0.0]);
        let tap = AudioTap::new(src, buf.clone());
        let output: Vec<f32> = tap.collect();

        // Output to rodio Sink must stay stereo-interleaved unchanged
        assert_eq!(output, vec![0.2_f32, 0.6, -0.4, 0.0]);

        // Shared FFT buffer must contain 2 mono-averaged frames, not 4 interleaved samples
        let captured: Vec<f32> = buf.lock().unwrap().iter().copied().collect();
        assert_eq!(captured.len(), 2);
        assert!((captured[0] - 0.4).abs() < 1e-6);
        assert!((captured[1] - (-0.2)).abs() < 1e-6);
    }

    #[test]
    fn test_audio_tap_delegates_try_seek_preserving_channel_alignment() {
        let buf = create_shared_audio_buffer();
        // 1 second of stereo audio at 100 Hz (200 samples total), sample i has value i as f32
        let samples: Vec<f32> = (0..200).map(|i| i as f32).collect();
        let src = SamplesBuffer::new(2, 100, samples);
        let mut tap = AudioTap::new(src, buf.clone());

        // Read one full stereo frame (left=0.0, right=1.0)
        assert_eq!(tap.next(), Some(0.0));
        assert_eq!(tap.next(), Some(1.0));

        // Seek to 500ms -> frame 50 -> sample index 100 (left=100.0, right=101.0)
        tap.try_seek(Duration::from_millis(500))
            .expect("AudioTap must forward try_seek to underlying seekable source");

        assert_eq!(tap.next(), Some(100.0));
        assert_eq!(tap.next(), Some(101.0));

        let captured: Vec<f32> = buf.lock().unwrap().iter().copied().collect();
        // Pre-seek frame was cleared on seek; only the post-seek frame remains
        assert_eq!(captured.len(), 1);
        assert!((captured[0] - 100.5).abs() < 1e-6);

        // Also verify mid-frame seek keeps stereo channel alignment intact (rodio contract)
        assert_eq!(tap.next(), Some(102.0)); // left channel only
        tap.try_seek(Duration::from_millis(200))
            .expect("Mid-frame try_seek must succeed");
        // SamplesBuffer seeks to right channel of previous frame (39.0) before next full frame (40.0, 41.0)
        assert_eq!(tap.next(), Some(39.0));
        assert_eq!(tap.next(), Some(40.0));
        assert_eq!(tap.next(), Some(41.0));
        let last_mono = *buf.lock().unwrap().back().unwrap();
        assert!((last_mono - 40.5).abs() < 1e-6);
    }
}
