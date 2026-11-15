//! Live Spectrum Visualizer widget.
//!
//! Generates real-time animated frequency bars with peak hold indicators
//! and cyan-blue gradient coloring matching vintage audio equalizer hardware.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

const BARS_COUNT: usize = 32;

/// Manages spectrum physics (smoothing, decay, peak caps).
pub struct SpectrumState {
    values: [f32; BARS_COUNT],
    peaks: [f32; BARS_COUNT],
    peak_hold: [u8; BARS_COUNT],
}

impl Default for SpectrumState {
    fn default() -> Self {
        Self {
            values: [0.0; BARS_COUNT],
            peaks: [0.0; BARS_COUNT],
            peak_hold: [0; BARS_COUNT],
        }
    }
}

impl SpectrumState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance physics one frame.
    pub fn update(&mut self, is_playing: bool, frame: usize) {
        if is_playing {
            let t = frame as f32 * 0.15;

            for i in 0..BARS_COUNT {
                let fi = i as f32;

                // Wave synthesis: bass rumble + mid melodic movement + treble sparkles
                let bass = (t * 2.2 + fi * 0.3).sin().abs() * (1.0 - (fi / 10.0).min(1.0));
                let mids = ((t * 1.6 + fi * 0.7).cos().abs() * 0.8) * (1.0 - ((fi - 14.0).abs() / 14.0).clamp(0.0, 1.0));
                let highs = ((t * 3.1 + fi * 1.2).sin().abs() * 0.6) * (fi / 28.0).clamp(0.0, 1.0);
                let pulse = (t * 4.0).sin().max(0.0) * 0.25;

                let target = ((bass * 1.1 + mids + highs + pulse).clamp(0.05, 1.0)).powf(1.1);

                // Smooth attack and decay
                if target > self.values[i] {
                    self.values[i] = self.values[i] * 0.4 + target * 0.6;
                } else {
                    self.values[i] = self.values[i] * 0.82 + target * 0.18;
                }

                // Update peaks with hold & falloff
                if self.values[i] >= self.peaks[i] {
                    self.peaks[i] = self.values[i];
                    self.peak_hold[i] = 4; // hold for 4 frames
                } else if self.peak_hold[i] > 0 {
                    self.peak_hold[i] -= 1;
                } else {
                    self.peaks[i] = (self.peaks[i] - 0.05).max(0.0);
                }
            }
        } else {
            // Decay to zero when paused or stopped
            for i in 0..BARS_COUNT {
                self.values[i] = (self.values[i] * 0.7 - 0.02).max(0.0);
                self.peaks[i] = (self.peaks[i] * 0.7 - 0.02).max(0.0);
            }
        }
    }
}

/// Spectrum Visualizer Widget for Ratatui
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
        let max_height = area.height as f32;

        for i in 0..num_bars {
            let col_x = area.x + (i as u16 * bar_width);
            if col_x >= area.right() {
                break;
            }

            let val = self.state.values[i];
            let peak = self.state.peaks[i];

            let bar_h = (val * max_height).round() as u16;
            let peak_y_offset = (peak * max_height).round() as u16;

            // Draw vertical column from bottom upwards
            for y_off in 0..area.height {
                let y = area.bottom().saturating_sub(1 + y_off);
                if y < area.top() {
                    break;
                }

                let color = if y_off < (max_height * 0.4) as u16 {
                    Color::Rgb(20, 80, 140) // Deep blue base
                } else if y_off < (max_height * 0.75) as u16 {
                    Color::Rgb(30, 170, 210) // Vibrant Cyan mid
                } else {
                    Color::Rgb(100, 220, 255) // Bright Cyan high
                };

                let w = (bar_width.saturating_sub(1)).max(1);
                for w_off in 0..w {
                    let x = col_x + w_off;
                    if x < area.right() {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            if y_off < bar_h {
                                cell.set_char('█')
                                    .set_style(Style::default().fg(color));
                            } else if y_off == peak_y_offset && peak > 0.05 {
                                // Orange peak indicator cap
                                cell.set_char('▀')
                                    .set_style(Style::default().fg(Color::Rgb(245, 150, 45)));
                            }
                        }
                    }
                }
            }
        }
    }
}
