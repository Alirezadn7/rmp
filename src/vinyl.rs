//! Vinyl Record Widget rendering a retro pixel-art animated spinning vinyl disc
//! with an animated turntable tonearm.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};
use crate::theme::ThemePalette;

/// Pixel-art Vinyl Record widget with retro tonearm
pub struct VinylRecord<'a> {
    pub is_playing: bool,
    pub frame: usize,
    pub palette: &'a ThemePalette,
}

impl<'a> VinylRecord<'a> {
    pub fn new(is_playing: bool, frame: usize, palette: &'a ThemePalette) -> Self {
        Self {
            is_playing,
            frame,
            palette,
        }
    }
}

impl<'a> Widget for VinylRecord<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 14 || area.height < 7 {
            return;
        }

        let show_tonearm = area.width >= 20 && area.height >= 8;
        let total_w: u16 = if show_tonearm { 20 } else { 16.min(area.width) };
        let disc_h: u16 = 8.min(area.height);

        let start_x = area.x + (area.width.saturating_sub(total_w)) / 2;
        let start_y = area.y + (area.height.saturating_sub(disc_h)) / 2;

        // 4 rotational reflection phases for spinning animation
        let rot = if self.is_playing { (self.frame / 2) % 4 } else { 0 };

        let matrix: [&str; 8] = match rot {
            0 => [
                "    ████████    ",
                "  ██▒▒▒▒▒▒▒▒██  ",
                " ██▒▒  ██  ▒▒██ ",
                "██▒▒  ████  ▒▒██",
                "██▒▒  ████  ▒▒██",
                " ██▒▒  ██  ▒▒██ ",
                "  ██▒▒▒▒▒▒▒▒██  ",
                "    ████████    ",
            ],
            1 => [
                "    ████████    ",
                "  ██▒▒▒▒  ▒▒██  ",
                " ██  ▒▒██  ▒▒██ ",
                "██▒▒  ████  ▒▒██",
                "██▒▒  ████  ▒▒██",
                " ██▒▒  ██▒▒  ██ ",
                "  ██▒▒  ▒▒▒▒██  ",
                "    ████████    ",
            ],
            2 => [
                "    ████████    ",
                "  ██  ▒▒▒▒  ██  ",
                " ██▒▒  ██  ▒▒██ ",
                "██  ▒▒████▒▒  ██",
                "██  ▒▒████▒▒  ██",
                " ██▒▒  ██  ▒▒██ ",
                "  ██  ▒▒▒▒  ██  ",
                "    ████████    ",
            ],
            _ => [
                "    ████████    ",
                "  ██▒▒  ▒▒▒▒██  ",
                " ██▒▒  ██▒▒  ██ ",
                "██▒▒  ████  ▒▒██",
                "██▒▒  ████  ▒▒██",
                " ██  ▒▒██  ▒▒██ ",
                "  ██▒▒▒▒  ▒▒██  ",
                "    ████████    ",
            ],
        };

        for (row_idx, row_str) in matrix.iter().enumerate() {
            let y = start_y + row_idx as u16;
            if y >= area.bottom() {
                break;
            }

            for (col_idx, ch) in row_str.chars().enumerate() {
                let x = start_x + col_idx as u16;
                if x >= area.right() {
                    break;
                }

                let is_center_label = (row_idx == 3 || row_idx == 4) && (6..=9).contains(&col_idx);
                let is_center_hole = (row_idx == 3 || row_idx == 4) && (col_idx == 7 || col_idx == 8);

                if let Some(cell) = buf.cell_mut((x, y)) {
                    if is_center_hole {
                        cell.set_char('█')
                            .set_style(Style::default().fg(self.palette.vinyl_hole));
                    } else if is_center_label {
                        cell.set_char('█')
                            .set_style(
                                Style::default()
                                    .fg(self.palette.vinyl_label)
                                    .add_modifier(Modifier::BOLD),
                            );
                    } else {
                        match ch {
                            '█' => {
                                cell.set_char('█')
                                    .set_style(Style::default().fg(self.palette.vinyl_groove));
                            }
                            '▒' => {
                                cell.set_char('█')
                                    .set_style(Style::default().fg(self.palette.vinyl_shine));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Draw turntable tonearm on the right edge when width allows
        if show_tonearm {
            let arm_x = start_x + 16;
            let pivot_style = Style::default()
                .fg(self.palette.accent_bright)
                .add_modifier(Modifier::BOLD);
            let arm_style = Style::default().fg(self.palette.tonearm);
            let stylus_style = Style::default()
                .fg(self.palette.vinyl_label)
                .add_modifier(Modifier::BOLD);

            // Pivot housing (top-right)
            if let Some(c) = buf.cell_mut((arm_x + 1, start_y)) {
                c.set_char('╭').set_style(pivot_style);
            }
            if let Some(c) = buf.cell_mut((arm_x + 2, start_y)) {
                c.set_char('●').set_style(pivot_style);
            }
            if let Some(c) = buf.cell_mut((arm_x + 3, start_y)) {
                c.set_char('╮').set_style(pivot_style);
            }

            if self.is_playing {
                // Engaged tonearm angled onto the vinyl groove
                let points: [(u16, u16, char, Style); 5] = [
                    (arm_x + 2, start_y + 1, '│', arm_style),
                    (arm_x + 1, start_y + 2, '╱', arm_style),
                    (arm_x, start_y + 3, '╱', arm_style),
                    (start_x + 15, start_y + 4, '╱', arm_style),
                    (start_x + 14, start_y + 5, '◈', stylus_style),
                ];
                for (px, py, ch, st) in points {
                    if px < area.right() && py < area.bottom() {
                        if let Some(c) = buf.cell_mut((px, py)) {
                            c.set_char(ch).set_style(st);
                        }
                    }
                }
            } else {
                // Parked tonearm resting vertically beside the platter
                for dy in 1..=4u16 {
                    let py = start_y + dy;
                    if py < area.bottom() {
                        if let Some(c) = buf.cell_mut((arm_x + 2, py)) {
                            c.set_char('│').set_style(arm_style);
                        }
                    }
                }
                if start_y + 5 < area.bottom() {
                    if let Some(c) = buf.cell_mut((arm_x + 2, start_y + 5)) {
                        c.set_char('┴').set_style(pivot_style);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ThemeKind;

    #[test]
    fn test_vinyl_tonearm_differs_between_playing_and_paused() {
        let palette = ThemePalette::for_kind(ThemeKind::CyanDeck);
        let area = Rect::new(0, 0, 20, 8);

        let mut buf_playing = Buffer::empty(area);
        VinylRecord::new(true, 0, &palette).render(area, &mut buf_playing);

        let mut buf_paused = Buffer::empty(area);
        VinylRecord::new(false, 0, &palette).render(area, &mut buf_paused);

        assert_eq!(buf_playing.cell((14, 5)).unwrap().symbol(), "◈");
        assert_eq!(buf_paused.cell((18, 5)).unwrap().symbol(), "┴");
    }
}
