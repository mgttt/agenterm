//! Product-local terminal cell paint policy.
//!
//! This layer translates a `vt100` screen into raster operations. It owns
//! terminal attributes and grid semantics, but not frame allocation, native
//! presentation, cursor/IME overlays, or surrounding chrome.

use agenterm_ui_core::terminal_selection::{TerminalPoint, normalize_endpoints};

use crate::font;
use crate::palette::{self, Rgb};
use crate::raster_surface::{CellRect, Surface};

const ITALIC_SHEAR: f32 = 0.21;

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
            let span_w = if cell.is_wide() { cell_w * 2 } else { cell_w };
            if !surface.intersects_rect(x0, y0, span_w, cell_h) {
                continue;
            }

            let mut fg = palette::resolve(cell.fgcolor(), default_fg, cell.bold());
            let mut bg = palette::resolve(cell.bgcolor(), default_bg, false);

            if let Some((sa, sb)) = selection {
                let (lo, hi) = normalize_endpoints(sa, sb);
                if row >= lo.row && row <= hi.row {
                    let col_start = if row == lo.row { lo.col } else { 0 };
                    let col_end = if row == hi.row { hi.col } else { u16::MAX };
                    if col >= col_start && col <= col_end {
                        std::mem::swap(&mut fg, &mut bg);
                    }
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
                let y = y0 + cell_h.saturating_sub(2);
                surface.fill_rect(x0, y, span_w, 1, fg.to_xrgb());
            }
        }
    }
}

fn first_grapheme(contents: &str) -> char {
    contents.chars().next().unwrap_or(' ')
}
