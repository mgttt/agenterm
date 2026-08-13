//! Product-local terminal cell paint policy.
//!
//! This layer translates a `vt100` screen into raster operations. It owns
//! terminal attributes, grid semantics, and cursor overlays, but not frame
//! allocation, native presentation, IME composition, or surrounding chrome.

use agenterm_ui_core::terminal_selection::{TerminalPoint, normalize_endpoints};

use crate::font;
use crate::palette::{self, Rgb};
use crate::raster_surface::{CellRect, Surface};

const ITALIC_SHEAR: f32 = 0.21;
const CURSOR_THICKNESS: u32 = 2;

#[derive(Clone, Copy)]
pub(super) struct CursorPaintSpec {
    pub cell_w: u32,
    pub cell_h: u32,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    pub font_size_px: u16,
    pub left: u32,
    pub top: u32,
    pub scroll_offset: usize,
    pub preedit_cells: u32,
    pub blink_visible: bool,
}

pub(super) fn cursor_visible(
    screen: &vt100::Screen,
    scroll_offset: usize,
    blink_visible: bool,
) -> bool {
    !screen.hide_cursor() && scroll_offset == 0 && (!screen.cursor_blinking() || blink_visible)
}

/// Paints a screen at the surface origin. Kept as a small test-facing wrapper
/// so geometry tests can exercise the same production policy without a host.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_cells(
    surface: &mut Surface<'_>,
    screen: &vt100::Screen,
    selection: Option<(TerminalPoint, TerminalPoint)>,
    cell_w: u32,
    cell_h: u32,
    default_fg: Rgb,
    default_bg: Rgb,
    font_size_px: u16,
) {
    paint_cells_at(
        surface,
        screen,
        selection,
        cell_w,
        cell_h,
        default_fg,
        default_bg,
        font_size_px,
        0,
        0,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_cells_at(
    surface: &mut Surface<'_>,
    screen: &vt100::Screen,
    selection: Option<(TerminalPoint, TerminalPoint)>,
    cell_w: u32,
    cell_h: u32,
    default_fg: Rgb,
    default_bg: Rgb,
    font_size_px: u16,
    left: u32,
    top: u32,
) {
    let (rows, cols) = screen.size();
    let selection = selection.map(|(start, end)| normalize_endpoints(start, end));
    for row in 0..rows {
        let y0 = top.saturating_add(u32::from(row).saturating_mul(cell_h));
        if y0 >= surface.height {
            break;
        }
        if !surface.intersects_rect(left, y0, surface.width.saturating_sub(left), cell_h) {
            continue;
        }
        for col in 0..cols {
            let x0 = left.saturating_add(u32::from(col).saturating_mul(cell_w));
            if x0 >= surface.width {
                break;
            }
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let span_w = if cell.is_wide() {
                cell_w.saturating_mul(2)
            } else {
                cell_w
            };
            if !surface.intersects_rect(x0, y0, span_w, cell_h) {
                continue;
            }

            let mut fg = palette::resolve(cell.fgcolor(), default_fg, cell.bold());
            let mut bg = palette::resolve(cell.bgcolor(), default_bg, false);

            if let Some((lo, hi)) = selection
                && row >= lo.row
                && row <= hi.row
            {
                let col_start = if row == lo.row { lo.col } else { 0 };
                let col_end = if row == hi.row { hi.col } else { u16::MAX };
                if col >= col_start && col <= col_end {
                    std::mem::swap(&mut fg, &mut bg);
                }
            }

            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.dim() {
                fg = palette::blend(fg, bg, 0.55);
            }
            if bg != default_bg {
                surface.fill_rect(x0, y0, span_w, cell_h, bg.to_xrgb());
            }

            if let Some(glyph) = cell
                .has_contents()
                .then(|| font::raster(first_grapheme(cell.contents()), font_size_px))
                .flatten()
            {
                let shear = if cell.italic() { ITALIC_SHEAR } else { 0.0 };
                surface.blit_glyph(
                    &glyph,
                    CellRect {
                        x: x0,
                        y: y0,
                        w: span_w,
                        h: cell_h,
                    },
                    fg,
                    shear,
                );
            }

            if cell.underline() {
                let y = y0.saturating_add(cell_h.saturating_sub(2));
                surface.fill_rect(x0, y, span_w, 1, fg.to_xrgb());
            }
        }
    }
}

pub(super) fn paint_cursor(
    surface: &mut Surface<'_>,
    screen: &vt100::Screen,
    spec: CursorPaintSpec,
) {
    if !cursor_visible(screen, spec.scroll_offset, spec.blink_visible) {
        return;
    }

    let cursor = screen.cursor_position();
    let cursor_col = u32::from(cursor.1).saturating_add(spec.preedit_cells);
    let x = spec
        .left
        .saturating_add(cursor_col.saturating_mul(spec.cell_w));
    let y = spec
        .top
        .saturating_add(u32::from(cursor.0).saturating_mul(spec.cell_h));
    if x >= surface.width || y >= surface.height {
        return;
    }

    let under = (spec.preedit_cells == 0)
        .then(|| screen.cell(cursor.0, cursor.1))
        .flatten();
    let span = match under {
        Some(cell) if cell.is_wide() => spec.cell_w.saturating_mul(2),
        _ => spec.cell_w,
    };

    match screen.cursor_shape() {
        vt100::CursorShape::Block => {
            surface.fill_rect(x, y, span, spec.cell_h, spec.default_fg.to_xrgb());
            let glyph = under
                .filter(|cell| cell.has_contents())
                .and_then(|cell| font::raster(first_grapheme(cell.contents()), spec.font_size_px));
            if let Some(glyph) = glyph {
                surface.blit_glyph(
                    &glyph,
                    CellRect {
                        x,
                        y,
                        w: span,
                        h: spec.cell_h,
                    },
                    spec.default_bg,
                    0.0,
                );
            }
        }
        vt100::CursorShape::Underline => {
            let y = y.saturating_add(spec.cell_h.saturating_sub(CURSOR_THICKNESS));
            surface.fill_rect(x, y, span, CURSOR_THICKNESS, spec.default_fg.to_xrgb());
        }
        vt100::CursorShape::Bar => {
            surface.fill_rect(
                x,
                y,
                CURSOR_THICKNESS,
                spec.cell_h,
                spec.default_fg.to_xrgb(),
            );
        }
    }
}

fn first_grapheme(contents: &str) -> char {
    contents.chars().next().unwrap_or(' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    const FG: Rgb = Rgb(0xEE, 0xEE, 0xEE);
    const BG: Rgb = Rgb(0, 0, 0);

    fn parser() -> vt100::Parser {
        vt100::Parser::new(1, 4, 0)
    }

    fn spec() -> CursorPaintSpec {
        CursorPaintSpec {
            cell_w: 8,
            cell_h: 12,
            default_fg: FG,
            default_bg: BG,
            font_size_px: 10,
            left: 0,
            top: 0,
            scroll_offset: 0,
            preedit_cells: 0,
            blink_visible: true,
        }
    }

    fn painted(shape: &[u8]) -> Vec<u32> {
        let mut parser = parser();
        parser.process(shape);
        let mut pixels = vec![BG.to_xrgb(); 32 * 12];
        paint_cursor(
            &mut Surface::new(&mut pixels, 32, 12),
            parser.screen(),
            spec(),
        );
        pixels
    }

    #[test]
    fn visibility_matches_hidden_scrollback_and_blink_gates() {
        let mut parser = parser();
        assert!(cursor_visible(parser.screen(), 0, true));
        assert!(!cursor_visible(parser.screen(), 0, false));
        assert!(!cursor_visible(parser.screen(), 1, true));

        parser.process(b"\x1b[?25l");
        assert!(!cursor_visible(parser.screen(), 0, true));
        parser.process(b"\x1b[?25h\x1b[2 q");
        assert!(cursor_visible(parser.screen(), 0, false));
    }

    #[test]
    fn block_underline_and_bar_have_distinct_pixel_footprints() {
        let block = painted(b"\x1b[2 q");
        let underline = painted(b"\x1b[4 q");
        let bar = painted(b"\x1b[6 q");
        let fg = FG.to_xrgb();
        let bg = BG.to_xrgb();

        assert_eq!(block[0], fg);
        assert_eq!(block[11 * 32 + 7], fg);
        assert_eq!(underline[0], bg);
        assert_eq!(underline[10 * 32 + 7], fg);
        assert_eq!(bar[0], fg);
        assert_eq!(bar[11 * 32 + 1], fg);
        assert_eq!(bar[11 * 32 + 2], bg);
    }

    #[test]
    fn block_cursor_covers_both_cells_of_a_wide_glyph() {
        let mut parser = parser();
        parser.process("中\r\x1b[2 q".as_bytes());
        let mut pixels = vec![BG.to_xrgb(); 32 * 12];
        paint_cursor(
            &mut Surface::new(&mut pixels, 32, 12),
            parser.screen(),
            spec(),
        );
        let fg = FG.to_xrgb();
        assert!(pixels[..16].contains(&fg));
        assert!(pixels[8..16].contains(&fg));
        for row in pixels.chunks_exact(32) {
            assert!(row[16..].iter().all(|pixel| *pixel == BG.to_xrgb()));
        }
    }

    #[test]
    fn wide_cell_span_saturates_at_extreme_cell_width() {
        let mut parser = parser();
        parser.process("中".as_bytes());
        let mut pixels = vec![BG.to_xrgb(); 1];
        paint_cells(
            &mut Surface::new(&mut pixels, 1, 1),
            parser.screen(),
            None,
            u32::MAX,
            1,
            FG,
            BG,
            1,
        );
    }
}
