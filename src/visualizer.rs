//! Live Spectrum Visualizer widget powered by a high-precision, zero-allocation
//! Real-Time FFT (Fast Fourier Transform) DSP engine.
//!
//! Features:
//! - Pre-allocated FFT buffers with zero allocations in the render loop.
//! - Pre-computed Hann Window to eliminate spectral leakage.
//! - Logarithmic frequency bands (20Hz to 16,000Hz) matching human hearing perception.
//! - Normalized Decibel (dB) scale with smooth gravity decay and peak hold caps.
//! - High-resolution sub-cell rendering using Unicode eighth-blocks for 8x vertical precision.

use std::sync::Arc;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use crate::player::SharedAudioBuffer;

pub const FFT_SIZE: usize = 1024;
pub const BARS_COUNT: usize = 32;

const MIN_FREQ: f32 = 20.0;
const MAX_FREQ: f32 = 16000.0;
const SAMPLE_RATE: f32 = 44100.0;

const EIGHTH_BLOCKS: [char; 9] = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Manages FFT processing, windowing, logarithmic binning, and physics state.
pub struct SpectrumState {
    fft: Arc<dyn Fft<f32>>,
    fft_input: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    hann_window: Vec<f32>,
    sample_copy_buf: Vec<f32>,
    band_bins: [(usize, usize); BARS_COUNT],

    // Normalized bar heights (0.0 to 1.0)
    values: [f32; BARS_COUNT],
    // Peak cap positions (0.0 to 1.0)
    peaks: [f32; BARS_COUNT],
    // Peak hold counters in frames
    peak_hold: [u8; BARS_COUNT],
}

impl Default for SpectrumState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumState {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft.get_inplace_scratch_len();

        let fft_input = vec![Complex::new(0.0, 0.0); FFT_SIZE];
        let fft_scratch = vec![Complex::new(0.0, 0.0); scratch_len];
        let sample_copy_buf = vec![0.0; FFT_SIZE];

        // Precompute Hann window: w[n] = 0.5 * (1 - cos(2*pi*n / (N - 1)))
        let mut hann_window = Vec::with_capacity(FFT_SIZE);
        let n_minus_1 = (FFT_SIZE - 1) as f32;
        for n in 0..FFT_SIZE {
            let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / n_minus_1).cos());
            hann_window.push(w);
        }

        // Precalculate logarithmic frequency band boundaries (20 Hz - 16,000 Hz)
        let bin_width = SAMPLE_RATE / FFT_SIZE as f32;
        let mut band_bins = [(0usize, 0usize); BARS_COUNT];

        for i in 0..BARS_COUNT {
            let ratio_low = i as f32 / BARS_COUNT as f32;
            let ratio_high = (i + 1) as f32 / BARS_COUNT as f32;

            let f_low = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf(ratio_low);
            let f_high = MIN_FREQ * (MAX_FREQ / MIN_FREQ).powf(ratio_high);

            let k_start = ((f_low / bin_width).floor() as usize).clamp(1, FFT_SIZE / 2 - 1);
            let mut k_end = ((f_high / bin_width).ceil() as usize).clamp(k_start + 1, FFT_SIZE / 2);
            if k_end <= k_start {
                k_end = k_start + 1;
            }

            band_bins[i] = (k_start, k_end);
        }

        Self {
            fft,
            fft_input,
            fft_scratch,
            hann_window,
            sample_copy_buf,
            band_bins,
            values: [0.0; BARS_COUNT],
            peaks: [0.0; BARS_COUNT],
            peak_hold: [0; BARS_COUNT],
        }
    }

    /// Advance FFT processing and physics state by one frame.
    /// Fully zero-allocation: copies samples, applies window, computes FFT, and applies physics in-place.
    pub fn update(&mut self, is_playing: bool, shared_buffer: &SharedAudioBuffer) {
        if is_playing {
            let mut samples_read = 0;

            // Non-blocking try_lock: if buffer is busy, reuse last samples or pad with 0
            if let Ok(buf) = shared_buffer.try_lock() {
                let avail = buf.len();
                let take = avail.min(FFT_SIZE);
                let start_idx = avail.saturating_sub(take);

                for (idx, sample) in buf.range(start_idx..).enumerate() {
                    self.sample_copy_buf[idx] = *sample;
                }
                samples_read = take;
            }

            // Zero-pad remainder if buffer didn't have full FFT_SIZE
            for i in samples_read..FFT_SIZE {
                self.sample_copy_buf[i] = 0.0;
            }

            // 1. Apply Hann Window to prevent spectral leakage
            for i in 0..FFT_SIZE {
                let windowed = self.sample_copy_buf[i] * self.hann_window[i];
                self.fft_input[i] = Complex::new(windowed, 0.0);
            }

            // 2. Perform in-place FFT
            self.fft.process_with_scratch(&mut self.fft_input, &mut self.fft_scratch);

            // 3. Compute magnitude for each logarithmic band
            for (b, &(k_start, k_end)) in self.band_bins.iter().enumerate() {
                let mut sum = 0.0f32;
                let mut max_val = 0.0f32;

                for k in k_start..k_end {
                    // Normalized complex magnitude
                    let mag = self.fft_input[k].norm() * (2.0 / FFT_SIZE as f32);
                    sum += mag;
                    if mag > max_val {
                        max_val = mag;
                    }
                }

                let count = (k_end - k_start).max(1) as f32;
                let energy = (sum / count) * 0.4 + max_val * 0.6;

                // 4. Convert to normalized Decibel (dB) scale
                // Floor at -60 dB, ceiling at -5 dB
                let db = 20.0 * (energy + 1e-6).log10();
                let mut norm = ((db + 60.0) / 55.0).clamp(0.0, 1.0);

                // Treble equal-loudness compensation curve (boost higher bands)
                let boost = 1.0 + (b as f32 / BARS_COUNT as f32) * 0.85;
                norm = (norm * boost).clamp(0.0, 1.0);

                // 5. Smooth Gravity / Peak Decay
                // Attack: fast rise
                // Decay: smooth gravity fall
                if norm > self.values[b] {
                    self.values[b] = self.values[b] * 0.25 + norm * 0.75;
                } else {
                    self.values[b] = (self.values[b] - 0.045).max(0.0);
                }

                // Peak cap physics
                if self.values[b] >= self.peaks[b] {
                    self.peaks[b] = self.values[b];
                    self.peak_hold[b] = 5; // hold for 5 frames
                } else if self.peak_hold[b] > 0 {
                    self.peak_hold[b] -= 1;
                } else {
                    self.peaks[b] = (self.peaks[b] - 0.02).max(0.0);
                }
            }
        } else {
            // Decay to zero when paused or stopped
            for i in 0..BARS_COUNT {
                self.values[i] = (self.values[i] - 0.05).max(0.0);
                self.peaks[i] = (self.peaks[i] - 0.05).max(0.0);
                self.peak_hold[i] = 0;
            }
        }
    }
}

/// Spectrum Visualizer Widget for Ratatui with 8x sub-cell vertical precision.
pub struct SpectrumVisualizer<'a> {
    pub state: &'a SpectrumState,
}

impl<'a> SpectrumVisualizer<'a> {
    pub fn new(state: &'a SpectrumState) -> Self {
        Self { state }
    }
}

impl<'a> Widget for SpectrumVisualizer<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 3 {
            return;
        }

        let num_bars = BARS_COUNT.min((area.width / 2) as usize);
        if num_bars == 0 {
            return;
        }

        let bar_width: u16 = (area.width / num_bars as u16).max(1);
        let total_sub_levels = area.height as usize * 8;

        for i in 0..num_bars {
            let col_x = area.x + (i as u16 * bar_width);
            if col_x >= area.right() {
                break;
            }

            let val = self.state.values[i];
            let peak = self.state.peaks[i];

            // 8x sub-cell height calculation
            let bar_sub_h = ((val * total_sub_levels as f32).round() as usize).min(total_sub_levels);
            let peak_row = ((peak * area.height as f32).round() as usize).saturating_sub(1).min(area.height as usize - 1);

            // Draw vertical column from bottom upwards
            for y_off in 0..area.height as usize {
                let y = area.bottom().saturating_sub(1 + y_off as u16);
                if y < area.top() {
                    break;
                }

                // Vertical color gradient
                let color = if y_off < (area.height as usize * 35 / 100) {
                    Color::Rgb(20, 80, 140) // Deep blue base
                } else if y_off < (area.height as usize * 70 / 100) {
                    Color::Rgb(30, 180, 220) // Vibrant Cyan mid
                } else {
                    Color::Rgb(120, 230, 255) // Bright Cyan high
                };

                let cell_sub_bottom = y_off * 8;
                let cell_sub_top = cell_sub_bottom + 8;

                let (ch, style) = if bar_sub_h >= cell_sub_top {
                    // Fully solid block cell
                    ('█', Style::default().fg(color))
                } else if bar_sub_h > cell_sub_bottom {
                    // Partial top cell: select precise eighth-block (1..=7)
                    let fraction = (bar_sub_h - cell_sub_bottom).clamp(1, 8);
                    (EIGHTH_BLOCKS[fraction], Style::default().fg(color))
                } else if y_off == peak_row && peak > 0.05 {
                    // Floating amber/orange peak cap
                    ('▀', Style::default().fg(Color::Rgb(245, 150, 45)))
                } else {
                    (' ', Style::default())
                };

                let w = (bar_width.saturating_sub(1)).max(1);
                for w_off in 0..w {
                    let x = col_x + w_off;
                    if x < area.right() {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            if ch != ' ' {
                                cell.set_char(ch).set_style(style);
                            }
                        }
                    }
                }
            }
        }
    }
}
