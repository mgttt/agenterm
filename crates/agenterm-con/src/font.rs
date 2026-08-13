//! Product-side glyph cache over the platform font raster contract.
//!
//! Native/file font APIs stop in `agenterm-platform`. This module owns only
//! terminal cache policy and the embedded ASCII disaster fallback, keeping the
//! product state machine independent of GDI, CoreText, fontconfig, and parsers.

use std::sync::{Arc, OnceLock};

use agenterm_ui_core::{GlyphCache, GlyphCacheKey};

pub use agenterm_platform::font::RasterGlyph;

const BITMAP_W: u32 = 8;
const BITMAP_H: u32 = 8;
const MAX_CACHED_GLYPHS: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub struct CellMetrics {
    pub width: u32,
    pub height: u32,
}

struct Renderer {
    cache: std::sync::Mutex<GlyphCache<GlyphCacheKey, Option<Arc<RasterGlyph>>>>,
}

impl Renderer {
    fn raster(&self, ch: char, size_px: u16) -> Option<Arc<RasterGlyph>> {
        let size = size_px.clamp(8, 72);
        let key = GlyphCacheKey::new(ch, size, 0);
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = cache.get_cloned(&key) {
            return cached;
        }
        let produced = agenterm_platform::font::rasterize(ch, size)
            .ok()
            .flatten()
            .or_else(|| bitmap_glyph(ch, size))
            .map(Arc::new);
        cache.insert(key, produced.clone());
        produced
    }
}

fn renderer() -> &'static Renderer {
    static RENDERER: OnceLock<Renderer> = OnceLock::new();
    RENDERER.get_or_init(|| Renderer {
        cache: std::sync::Mutex::new(GlyphCache::with_capacity(MAX_CACHED_GLYPHS)),
    })
}

pub fn resolved_name() -> String {
    agenterm_platform::font::rasterizer_name().unwrap_or_else(|_| "bitmap-8x8".to_owned())
}

pub fn cell_metrics(size_px: u16) -> CellMetrics {
    let size = size_px.clamp(8, 72);
    agenterm_platform::font::primary_metrics(size)
        .map(|metrics| CellMetrics {
            width: agenterm_platform::numeric::ceil_f32(metrics.cell_width).max(1.0) as u32,
            height: agenterm_platform::numeric::ceil_f32(metrics.cell_height).max(1.0) as u32,
        })
        .unwrap_or_else(|_| {
            let scale = bitmap_scale(size);
            CellMetrics {
                width: BITMAP_W * scale,
                height: BITMAP_H * scale,
            }
        })
}

pub fn raster(ch: char, size_px: u16) -> Option<Arc<RasterGlyph>> {
    renderer().raster(ch, size_px)
}

const FIRST_CHAR: u8 = 32;
const LAST_CHAR: u8 = 126;

include!("bitmap_glyphs.in.rs");

fn bitmap_scale(size_px: u16) -> u32 {
    let target = u32::from(size_px).max(BITMAP_W);
    ((target + BITMAP_W / 2) / BITMAP_W).max(1)
}

fn bitmap_glyph(ch: char, size_px: u16) -> Option<RasterGlyph> {
    let code = ch as u32;
    if !(u32::from(FIRST_CHAR)..=u32::from(LAST_CHAR)).contains(&code) {
        return None;
    }
    let rows = &BITMAP_GLYPHS[(code - u32::from(FIRST_CHAR)) as usize];
    let scale = bitmap_scale(size_px) as usize;
    let width = BITMAP_W * scale as u32;
    let height = BITMAP_H * scale as u32;
    let mut alpha = vec![0u8; (width * height) as usize];
    for (gy, row) in rows.iter().enumerate() {
        for gx in 0..BITMAP_W {
            if row & (1u8 << gx) != 0 {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = gx as usize * scale + dx;
                        let py = gy * scale + dy;
                        alpha[py * width as usize + px] = 0xff;
                    }
                }
            }
        }
    }
    Some(RasterGlyph {
        alpha,
        width,
        height,
        offset_x: 0,
        offset_y: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_fallback_renders_ascii() {
        let glyph = bitmap_glyph('A', 16).expect("A renders");
        assert_eq!(glyph.width, 16);
        assert_eq!(glyph.height, 16);
        assert!(glyph.alpha.iter().any(|&alpha| alpha > 0));
    }

    #[test]
    fn bitmap_fallback_skips_non_ascii() {
        assert!(bitmap_glyph('繁', 16).is_none());
    }

    #[test]
    fn cache_evicts_at_capacity() {
        let mut cache = GlyphCache::with_capacity(MAX_CACHED_GLYPHS);
        for offset in 0..=MAX_CACHED_GLYPHS {
            let ch = char::from_u32(0x1000 + offset as u32).expect("valid char");
            cache.insert(GlyphCacheKey::new(ch, 14, 0), None::<Arc<RasterGlyph>>);
        }
        assert_eq!(cache.len(), MAX_CACHED_GLYPHS);
        assert!(cache.get(&GlyphCacheKey::new('\u{1000}', 14, 0)).is_none());
    }

    #[test]
    fn cell_metrics_are_positive() {
        let metrics = cell_metrics(14);
        assert!(metrics.width > 0);
        assert!(metrics.height > 0);
    }
}
