//! Bounded clipped XRGB raster target.
//!
//! Product paint policy stays in the caller; this leaf owns only pixel bounds,
//! rectangle fill, and glyph-mask blending.

use agenterm_ui_core::PixelRect;

use crate::font;
use crate::palette::Rgb;

#[derive(Clone, Copy)]
pub(super) struct CellRect {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) w: u32,
    pub(super) h: u32,
}

/// The pixel target for one frame: the buffer and its dimensions, which always
/// travel together. Bundling them keeps the drawing calls readable — the free
/// functions this replaced took nine positional arguments, most of them the
/// same three values threaded through every call.
pub(super) struct Surface<'a> {
    pub(super) pixels: &'a mut [u32],
    pub(super) width: u32,
    pub(super) height: u32,
    clip: PixelRect,
}

impl<'a> Surface<'a> {
    #[cfg(test)]
    pub(super) fn new(pixels: &'a mut [u32], width: u32, height: u32) -> Surface<'a> {
        Self::with_clip(pixels, width, height, PixelRect::full_frame(width, height))
    }

    pub(super) fn with_clip(
        pixels: &'a mut [u32],
        width: u32,
        height: u32,
        clip: PixelRect,
    ) -> Surface<'a> {
        Self {
            pixels,
            width,
            height,
            clip: clip.clip(width, height),
        }
    }

    fn clipped_rect(&self, x: u32, y: u32, w: u32, h: u32) -> PixelRect {
        let rect = PixelRect::from_xywh(x, y, w, h).clip(self.width, self.height);
        let left = rect.left.max(self.clip.left);
        let top = rect.top.max(self.clip.top);
        let right = rect.right.min(self.clip.right).max(left);
        let bottom = rect.bottom.min(self.clip.bottom).max(top);
        PixelRect {
            left,
            top,
            right,
            bottom,
        }
    }

    pub(super) fn intersects_rect(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        !self.clipped_rect(x, y, w, h).is_empty()
    }

    pub(super) fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let rect = self.clipped_rect(x, y, w, h);
        if rect.is_empty() {
            return;
        }
        agenterm_ui_core::pixel::fill_xrgb_rect(
            self.pixels,
            self.width,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            color,
        );
    }

    /// Blits a rasterized glyph into a cell, clipped to that cell.
    ///
    /// `shear` slants the glyph for faux italic: a per-row horizontal offset
    /// proportional to height above the baseline. Synthesizing the slant beats
    /// loading a real italic face, which would have a different advance width
    /// and break the fixed cell grid.
    pub(super) fn blit_glyph(
        &mut self,
        glyph: &font::RasterGlyph,
        cell: CellRect,
        fg: Rgb,
        shear: f32,
    ) {
        let clip = self.clipped_rect(cell.x, cell.y, cell.w, cell.h);
        if clip.is_empty() {
            return;
        }
        let start_x = i64::from(cell.x) + i64::from(glyph.offset_x);
        let start_y = i64::from(cell.y) + i64::from(glyph.offset_y);
        let clip_x0 = i64::from(clip.left);
        let clip_y0 = i64::from(clip.top);
        let clip_x1 = i64::from(clip.right);
        let clip_y1 = i64::from(clip.bottom);

        for gy in 0..glyph.height {
            let py = start_y + i64::from(gy);
            if py < clip_y0 || py >= clip_y1 || py < 0 || py >= i64::from(self.height) {
                continue;
            }
            // Rows nearer the top lean further right, pivoting on the bottom
            // of the cell so the glyph stays seated on its baseline.
            let slant = if shear == 0.0 {
                0
            } else {
                agenterm_platform::numeric::round_f32((clip_y1 - py) as f32 * shear) as i64
            };
            let row_start_x = start_x + slant;
            let source_x_start = (clip_x0 - row_start_x).max(0).min(i64::from(u32::MAX)) as u32;
            let source_x_end = glyph
                .width
                .min((clip_x1 - row_start_x).max(0).min(i64::from(u32::MAX)) as u32);
            if source_x_start >= source_x_end {
                continue;
            }
            let destination_x = row_start_x + i64::from(source_x_start);
            if destination_x < 0 || destination_x >= i64::from(self.width) {
                continue;
            }
            let count = usize::try_from(source_x_end - source_x_start).unwrap_or(0);
            let Some(row_start) = usize::try_from(py)
                .ok()
                .and_then(|row| row.checked_mul(self.width as usize))
            else {
                continue;
            };
            let Some(destination_start) = row_start.checked_add(destination_x as usize) else {
                continue;
            };
            let Some(destination_end) = destination_start.checked_add(count) else {
                continue;
            };
            let Some(source_start) = usize::try_from(gy)
                .ok()
                .and_then(|row| row.checked_mul(glyph.width as usize))
                .and_then(|row| row.checked_add(source_x_start as usize))
            else {
                continue;
            };
            let Some(source_end) = source_start.checked_add(count) else {
                continue;
            };
            let Some(destination) = self.pixels.get_mut(destination_start..destination_end) else {
                continue;
            };
            let Some(source) = glyph.alpha.get(source_start..source_end) else {
                continue;
            };
            agenterm_ui_core::pixel::blend_mask_xrgb(destination, source, fg.to_xrgb());
        }
    }
}
