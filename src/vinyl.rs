//! Vinyl Record Widget rendering a retro pixel-art animated spinning vinyl disc.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

/// Pixel-art Vinyl Record widget
pub struct VinylRecord {
    pub is_playing: bool,
    pub frame: usize,
}

impl VinylRecord {
    pub fn new(is_playing: bool, frame: usize) -> Self {
        Self { is_playing, frame }
    }
}

impl Widget for VinylRecord {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 14 || area.height < 7 {
            return;
        }

        // Center vinyl disc in the allocated area
        let disc_w: u16 = 16.min(area.width);
        let disc_h: u16 = 8.min(area.height);

        let start_x = area.x + (area.width.saturating_sub(disc_w)) / 2;
        let start_y = area.y + (area.height.saturating_sub(disc_h)) / 2;

        // 4 rotational reflection phases for spinning animation
        let rot = if self.is_playing { (self.frame / 2) % 4 } else { 0 };

        // Vinyl pattern representation (8 rows x 16 cols)
        // Legend:
        // ' ' = transparent/background
        // 'G' = vinyl groove (dark teal / dark gray)
        // 'R' = specular shine / reflection (bright cyan)
        // 'O' = center orange label
        // 'H' = center spindle hole (dark)
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

                // Check center label region (rows 3..5, cols 6..10)
                let is_center_label = (row_idx == 3 || row_idx == 4) && (col_idx >= 6 && col_idx <= 9);
                let is_center_hole = (row_idx == 3 || row_idx == 4) && (col_idx == 7 || col_idx == 8);

                if let Some(cell) = buf.cell_mut((x, y)) {
                    if is_center_hole {
                        cell.set_char('█')
                            .set_style(Style::default().fg(Color::Rgb(20, 25, 35)));
                    } else if is_center_label {
                        cell.set_char('█')
                            .set_style(Style::default().fg(Color::Rgb(240, 140, 40)).add_modifier(Modifier::BOLD));
                    } else {
                        match ch {
                            '█' => {
                                cell.set_char('█')
                                    .set_style(Style::default().fg(Color::Rgb(25, 75, 95)));
                            }
                            '▒' => {
                                cell.set_char('█')
                                    .set_style(Style::default().fg(Color::Rgb(40, 190, 220)));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
