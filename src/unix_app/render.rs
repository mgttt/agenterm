use crate::theme::{Rgb, ThemeId, ThemePalette};

use super::font::GLYPH_WIDTH;

pub(super) const CELL_WIDTH: u32 = 10;
pub(super) const CELL_HEIGHT: u32 = 16;
pub(super) const CELL_PADDING_X: u32 = 1;
pub(super) const CELL_PADDING_Y: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalCell {
    pub(super) ch: char,
    pub(super) fg: u8,
    pub(super) bg: u8,
}

impl TerminalCell {
    pub(super) const fn blank() -> Self {
        Self {
            ch: ' ',
            fg: 7,
            bg: 0,
        }
    }

    pub(super) fn with_defaults(ch: char, _palette: &ThemePalette) -> Self {
        Self {
            ch,
            fg: 7,
            bg: 0,
        }
    }
}

pub(super) struct TerminalGrid {
    pub(super) cols: u16,
    pub(super) rows: u16,
    cells: Vec<TerminalCell>,
    palette: &'static ThemePalette,
}

impl TerminalGrid {
    pub(super) fn new(cols: u16, rows: u16, palette: &'static ThemePalette) -> Self {
        let mut grid = Self {
            cols,
            rows,
            cells: vec![TerminalCell::blank(); usize::from(cols) * usize::from(rows)],
            palette,
        };
        grid.fill_stub_message();
        grid
    }

    pub(super) fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.cells.resize(
            usize::from(cols) * usize::from(rows),
            TerminalCell::blank(),
        );
        self.fill_stub_message();
    }

    pub(super) fn cell(&self, col: u16, row: u16) -> TerminalCell {
        self.cells[self.index(col, row)]
    }

    pub(super) fn set_cell(&mut self, col: u16, row: u16, cell: TerminalCell) {
        if col < self.cols && row < self.rows {
            let index = usize::from(row) * usize::from(self.cols) + usize::from(col);
            self.cells[index] = cell;
        }
    }

    pub(super) fn put_text(&mut self, col: u16, row: u16, text: &str) {
        let mut column = col;
        for ch in text.chars() {
            if column >= self.cols {
                break;
            }
            self.set_cell(
                column,
                row,
                TerminalCell::with_defaults(ch, self.palette),
            );
            column += 1;
        }
    }

    fn index(&self, col: u16, row: u16) -> usize {
        usize::from(row) * usize::from(self.cols) + usize::from(col)
    }

    fn fill_stub_message(&mut self) {
        for cell in &mut self.cells {
            *cell = TerminalCell::blank();
        }
        self.put_text(0, 0, "AgenTerm Unix GUI — waiting for PTY");
        self.put_text(0, 2, "Type to queue PTY input (stub echo).");
    }

    /// Local echo for the stub backend until `terminal_runtime` is wired on Unix.
    pub(super) fn apply_local_echo(&mut self, bytes: &[u8]) {
        let mut col = 0_u16;
        let mut row = 4_u16;
        for &byte in bytes {
            match byte {
                b'\r' | b'\n' => {
                    col = 0;
                    row = row.saturating_add(1);
                }
                0x7F | 0x08 => {
                    if col > 0 {
                        col -= 1;
                        self.set_cell(col, row, TerminalCell::blank());
                    }
                }
                b if b.is_ascii_graphic() || b == b' ' => {
                    if col < self.cols {
                        self.set_cell(
                            col,
                            row,
                            TerminalCell::with_defaults(b as char, self.palette),
                        );
                        col += 1;
                    }
                }
                _ => {}
            }
        }
    }
}

pub(super) fn grid_dimensions_for_pixels(width: u32, height: u32) -> (u16, u16) {
    let cols = (width / CELL_WIDTH).max(1) as u16;
    let rows = (height / CELL_HEIGHT).max(1) as u16;
    (cols, rows)
}

pub(super) fn render_grid(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    grid: &TerminalGrid,
    palette: &ThemePalette,
) {
    let background = rgb_to_pixel(palette.terminal_background);
    for pixel in buffer.iter_mut().take((stride * height) as usize) {
        *pixel = background;
    }

    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = grid.cell(col, row);
            let fg = ansi_color(palette, cell.fg);
            let bg = ansi_color(palette, cell.bg);
            draw_cell(buffer, stride, width, height, col, row, cell.ch, fg, bg);
        }
    }
}

fn draw_cell(
    buffer: &mut [u32],
    stride: u32,
    width: u32,
    height: u32,
    col: u16,
    row: u16,
    ch: char,
    fg: Rgb,
    bg: Rgb,
) {
    let origin_x = u32::from(col) * CELL_WIDTH;
    let origin_y = u32::from(row) * CELL_HEIGHT;
    if origin_x >= width || origin_y >= height {
        return;
    }

    let cell_w = CELL_WIDTH.min(width - origin_x);
    let cell_h = CELL_HEIGHT.min(height - origin_y);
    let bg_pixel = rgb_to_pixel(bg);
    fill_rect(
        buffer,
        stride,
        origin_x,
        origin_y,
        cell_w,
        cell_h,
        bg_pixel,
    );

    let Some(glyph) = super::font::glyph_rows(ch) else {
        return;
    };

    let glyph_x = origin_x + CELL_PADDING_X;
    let glyph_y = origin_y + CELL_PADDING_Y;
    let fg_pixel = rgb_to_pixel(fg);
    for (row_index, row_bits) in glyph.iter().enumerate() {
        let y = glyph_y + row_index as u32;
        if y >= origin_y + cell_h {
            break;
        }
        for bit in 0..GLYPH_WIDTH {
            if row_bits & (0x80 >> bit) == 0 {
                continue;
            }
            let x = glyph_x + bit;
            if x < width {
                put_pixel(buffer, stride, x, y, fg_pixel);
            }
        }
    }
}

fn fill_rect(
    buffer: &mut [u32],
    stride: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: u32,
) {
    for row in y..y + height {
        let start = (row * stride + x) as usize;
        let end = start + width as usize;
        if end <= buffer.len() {
            buffer[start..end].fill(color);
        }
    }
}

fn put_pixel(buffer: &mut [u32], stride: u32, x: u32, y: u32, color: u32) {
    let index = (y * stride + x) as usize;
    if let Some(pixel) = buffer.get_mut(index) {
        *pixel = color;
    }
}

fn ansi_color(palette: &ThemePalette, index: u8) -> Rgb {
    palette.ansi[(index & 0x0F) as usize]
}

fn rgb_to_pixel(rgb: Rgb) -> u32 {
    0xFF00_0000
        | (u32::from(rgb.red) << 16)
        | (u32::from(rgb.green) << 8)
        | u32::from(rgb.blue)
}

pub(super) fn theme_palette() -> &'static ThemePalette {
    ThemeId::Dark.palette()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_dimensions_use_cell_metrics() {
        assert_eq!(grid_dimensions_for_pixels(800, 480), (80, 30));
    }
}
