//! Glyph rasterization for the fallback terminal.
//!
//! Loads the platform's primary monospace font via `agenterm_platform::font`
//! candidates and rasterizes glyphs with `ab_glyph`. A self-contained 8×8
//! ASCII bitmap font guarantees readable output even when no system font file
//! can be opened, so the terminal never renders as a blank window.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use ab_glyph::{Font, FontArc, FontRef, GlyphId, PxScale, ScaleFont};

/// Width/height of the embedded fallback bitmap glyphs.
const BITMAP_W: u32 = 8;
const BITMAP_H: u32 = 8;

/// A rasterized glyph: an alpha coverage bitmap plus placement metadata.
#[derive(Clone, Debug)]
pub struct RasterGlyph {
    /// Row-major alpha coverage (`0..=255`), length `width * height`.
    pub alpha: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Pixel offset from the cell's top-left corner to the glyph bitmap's
    /// top-left pixel. `offset_y` is already baseline-resolved, so the caller
    /// does not need a separate ascent term.
    pub offset_x: i32,
    pub offset_y: i32,
}

/// Cell metrics derived from the loaded font for a given pixel size.
#[derive(Clone, Copy, Debug)]
pub struct CellMetrics {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct GlyphKey {
    ch: char,
    size: u16,
}

const MAX_CACHED_GLYPHS: usize = 4096;
const MAX_COLLECTION_FACES: u32 = 32;

#[derive(Default)]
struct GlyphCache {
    values: HashMap<GlyphKey, Option<Arc<RasterGlyph>>>,
    order: VecDeque<GlyphKey>,
}

impl GlyphCache {
    fn get(&self, key: &GlyphKey) -> Option<Option<Arc<RasterGlyph>>> {
        self.values.get(key).cloned()
    }

    fn insert(&mut self, key: GlyphKey, value: Option<Arc<RasterGlyph>>) {
        if let Some(slot) = self.values.get_mut(&key) {
            *slot = value;
            return;
        }
        while self.values.len() >= MAX_CACHED_GLYPHS {
            let Some(evicted) = self.order.pop_front() else {
                break;
            };
            self.values.remove(&evicted);
        }
        self.order.push_back(key);
        self.values.insert(key, value);
    }
}

/// Largest glyph coverage bitmap dimension this renderer will ever allocate,
/// in either axis. Far larger than any real glyph needs at the `[8, 72]`
/// pixel sizes `raster`/`cell_metrics` clamp their callers to — this only
/// ever engages on bogus outline data, never on legitimate output.
///
/// Defensive, not just a size guard: an unusual or malformed glyph outline
/// reporting an enormous bounding box is real font-rasterizer territory
/// (exotic/buggy fonts do sometimes yield nonsensical extents from outline
/// parsing). Without this clamp, `width * height` computed in `u32` before
/// widening to `usize` for the allocation can silently wrap in a release
/// build (no overflow-checks there) instead of panicking the way a debug
/// build would — a wrapped product undersizes the alpha buffer, but the
/// rasterizer's own draw-callback index math still multiplies by the real
/// (unclamped) width, which is an out-of-bounds *write*, not just an
/// oversized allocation. Under this binary's release profile
/// (`panic = "abort"`), any panic anywhere — caller included — is a silent,
/// dialog-free process exit: exactly the symptom a "renders fine at every
/// size on this machine's fonts, but crashes on some machine's font at some
/// size" report would produce, and not reproducible by sweeping this
/// machine's own installed fonts.
const MAX_GLYPH_DIM: u32 = 4096;

/// Converts a glyph outline's raw pixel-space width/height (from
/// `ab_glyph::OutlinedGlyph::px_bounds()`, which is font data, not
/// something this renderer controls) into a safe allocation size. Pulled
/// out of `raster_uncached` as a pure function so the clamp itself — the
/// actual fix — is directly unit-testable without needing a real font file
/// that happens to produce a pathological outline.
fn clamp_glyph_dims(width: f32, height: f32) -> (u32, u32) {
    let width = (width.max(0.0) as u32).min(MAX_GLYPH_DIM);
    let height = (height.max(0.0) as u32).min(MAX_GLYPH_DIM);
    (width, height)
}

struct Face {
    name: &'static str,
    font: FontArc,
}

struct Renderer {
    faces: Vec<Face>,
    cache: std::sync::Mutex<GlyphCache>,
}

impl Renderer {
    fn resolved_name(&self) -> &'static str {
        self.faces.first().map(|f| f.name).unwrap_or("bitmap-8x8")
    }

    fn cell_metrics(&self, size_px: u16) -> CellMetrics {
        if let Some(face) = self.faces.first() {
            let scaled = face.font.as_scaled(f32::from(size_px));
            let ascent = scaled.ascent();
            let descent = scaled.descent();
            let line_height = (ascent - descent).ceil().max(1.0) as u32;
            let advance = scaled.h_advance(scaled.glyph_id('M')).ceil().max(1.0) as u32;
            return CellMetrics {
                width: advance,
                height: line_height,
            };
        }
        // Bitmap fallback: scale the 8×8 glyph to roughly match the request.
        let scale = bitmap_scale(size_px);
        CellMetrics {
            width: BITMAP_W * scale,
            height: BITMAP_H * scale,
        }
    }

    fn raster(&self, ch: char, size_px: u16) -> Option<Arc<RasterGlyph>> {
        let clamped = size_px.clamp(8, 72);
        let key = GlyphKey { ch, size: clamped };
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = cache.get(&key) {
            return cached;
        }
        let produced = self.raster_uncached(ch, clamped);
        cache.insert(key, produced.clone());
        produced
    }

    fn raster_uncached(&self, ch: char, size_px: u16) -> Option<Arc<RasterGlyph>> {
        if let Some(face) = self
            .faces
            .iter()
            .find(|f| f.font.glyph_id(ch) != GlyphId(0))
        {
            let scaled = face.font.as_scaled(f32::from(size_px));
            let glyph_id = scaled.glyph_id(ch);
            let outlined =
                scaled.outline_glyph(glyph_id.with_scale(PxScale::from(f32::from(size_px))))?;
            let bounds = outlined.px_bounds();
            let (width, height) = clamp_glyph_dims(bounds.width(), bounds.height());
            let mut alpha = vec![0u8; width as usize * height as usize];
            outlined.draw(|x, y, coverage| {
                if x < width && y < height {
                    alpha[(y * width + x) as usize] =
                        (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            });
            // Absolute cell-relative placement: the caller adds offset_y to the
            // cell top (no separate baseline term needed).
            let ascent = scaled.ascent();
            return Some(Arc::new(RasterGlyph {
                alpha,
                width,
                height,
                offset_x: bounds.min.x.round() as i32,
                offset_y: (ascent + bounds.min.y).round() as i32,
            }));
        }
        // Bitmap fallback for ASCII; non-ASCII without a system face yields empty.
        bitmap_glyph(ch, size_px).map(Arc::new)
    }
}

/// Loads system font faces discovered by the platform facade. Leaked
/// intentionally: the data must outlive the static renderer for the process
/// lifetime, mirroring the product terminal's own font loader.
fn load_faces() -> Vec<Face> {
    let mut faces = Vec::new();

    // Windows ships SimSun and NSimSun in the same collection. NSimSun is the
    // face Windows exposes as "新宋体"; it has measured 1:2 Latin/CJK advances,
    // so it must be the primary face rather than a fallback squeezed into a
    // grid measured from Consolas or Cascadia.
    // No `#[cfg(windows)]` here: the platform difference already lives in
    // `agenterm_platform::font::fallback_candidates()`, which only offers the
    // SimSun collection on hosts that ship it. Off Windows the lookup below
    // simply finds nothing and this falls through. Branching on the target in a
    // main-crate module is what `production_sources_use_platform_crate_as_the_only_native_boundary`
    // rejects -- the exemption covers the three subsystem entrypoints, not their
    // modules.
    if let Some(face) = load_new_simsun_face() {
        faces.push(face);
        for candidate in agenterm_platform::font::fallback_candidates() {
            if candidate.name != "SimSun / NSimSun" {
                push_faces(&mut faces, candidate);
            }
        }
        return faces;
    }

    // The primary face decides cell metrics, so only the first readable
    // monospace candidate is taken — mixing advance widths would break the
    // grid.
    for candidate in agenterm_platform::font::candidates() {
        if push_faces(&mut faces, candidate) {
            break;
        }
    }

    // Coverage fallbacks are additive: every one that loads joins the chain,
    // because `raster_uncached` walks it until a face actually has the glyph.
    // Stopping at the first (as this function used to, for the primary list)
    // is what made CJK render as blank cells when the chosen primary face had
    // no Han glyph coverage and no fallback was consulted.
    for candidate in agenterm_platform::font::fallback_candidates() {
        push_faces(&mut faces, candidate);
    }

    faces
}

/// Loads the New SimSun face from the stock Windows SimSun collection.
///
/// Windows exposes the collection as two regular families in this order:
/// SimSun (index 0) and NSimSun/New SimSun (index 1). Selecting by collection
/// index here is deliberate: `ab_glyph` exposes glyph outlines but not a
/// family-name selector, and accepting the first face would silently select
/// SimSun instead of the requested terminal face.
fn load_new_simsun_face() -> Option<Face> {
    const NEW_SIMSUN_INDEX: u32 = 1;
    let candidate = agenterm_platform::font::fallback_candidates()
        .into_iter()
        .find(|candidate| candidate.name == "SimSun / NSimSun")?;
    let data = fs::read(candidate.absolute_path()).ok()?;
    let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
    let font = FontRef::try_from_slice_and_index(leaked, NEW_SIMSUN_INDEX).ok()?;
    Some(Face {
        name: "New SimSun",
        font: FontArc::from(font),
    })
}

/// Appends every face in a candidate file. Returns whether anything loaded.
fn push_faces(
    faces: &mut Vec<Face>,
    candidate: agenterm_platform::font::FontFileCandidate,
) -> bool {
    let path: PathBuf = candidate.absolute_path();
    let Ok(data) = fs::read(&path) else {
        return false;
    };
    let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
    let before = faces.len();
    // Collections (.ttc) hold several faces; index until one fails to parse.
    for index in 0..MAX_COLLECTION_FACES {
        match FontRef::try_from_slice_and_index(leaked, index) {
            Ok(font) => faces.push(Face {
                name: candidate.name,
                font: FontArc::from(font),
            }),
            Err(_) => break,
        }
    }
    faces.len() > before
}

fn renderer() -> &'static Renderer {
    static RENDERER: OnceLock<Renderer> = OnceLock::new();
    RENDERER.get_or_init(|| Renderer {
        faces: load_faces(),
        cache: std::sync::Mutex::new(GlyphCache::default()),
    })
}

/// Public entry point: the resolved font family name (or `bitmap-8x8`).
pub fn resolved_name() -> &'static str {
    renderer().resolved_name()
}

/// Public entry point: cell metrics for a pixel size.
pub fn cell_metrics(size_px: u16) -> CellMetrics {
    renderer().cell_metrics(size_px.clamp(8, 72))
}

/// Public entry point: rasterize a single character, cached.
pub fn raster(ch: char, size_px: u16) -> Option<Arc<RasterGlyph>> {
    renderer().raster(ch, size_px)
}

// ---------------------------------------------------------------------------
// 8×8 ASCII bitmap fallback (32..=126). Each glyph row is 8 bits, low bit left.
// ---------------------------------------------------------------------------

const FIRST_CHAR: u8 = 32;
const LAST_CHAR: u8 = 126;

include!("bitmap_glyphs.in.rs");

fn bitmap_scale(size_px: u16) -> u32 {
    // Pick an integer scale that brings the 8px bitmap closest to the request.
    let target = u32::from(size_px).max(BITMAP_W);
    ((target + BITMAP_W / 2) / BITMAP_W).max(1)
}

fn bitmap_glyph(ch: char, size_px: u16) -> Option<RasterGlyph> {
    let code = ch as u32;
    if !(u32::from(FIRST_CHAR)..=u32::from(LAST_CHAR)).contains(&code) {
        return None;
    }
    let rows = &BITMAP_GLYPHS[(code - u32::from(FIRST_CHAR)) as usize];
    let scale = bitmap_scale(size_px);
    let scale = scale as usize;
    let width = BITMAP_W * scale as u32;
    let height = BITMAP_H * scale as u32;
    let mut alpha = vec![0u8; (width * height) as usize];
    for (gy, row) in rows.iter().enumerate() {
        for gx in 0..BITMAP_W {
            if row & (1u8 << gx) != 0 {
                let block = gx as usize * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = block + dx;
                        let py = gy * scale + dy;
                        let idx = py * width as usize + px;
                        if idx < alpha.len() {
                            alpha[idx] = 0xFF;
                        }
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
    fn clamp_glyph_dims_passes_through_normal_sizes() {
        // A real glyph at a real clamped font size — must come out unchanged.
        assert_eq!(clamp_glyph_dims(42.0, 58.0), (42, 58));
    }

    #[test]
    fn clamp_glyph_dims_caps_pathological_outline_bounds() {
        // Simulates the bug this guards against: a font outline reporting
        // an enormous bounding box. Before the fix, `width * height` in
        // `u32` would silently wrap in release (e.g. 100_000 * 100_000 far
        // exceeds `u32::MAX`), and the draw callback's index math would
        // then write past whatever undersized buffer that wrapped product
        // allocated — an out-of-bounds write. Post-fix, both dimensions are
        // capped, so the product can never approach overflow in the first
        // place, in any build profile.
        let (width, height) = clamp_glyph_dims(1_000_000.0, 1_000_000.0);
        assert!(width <= MAX_GLYPH_DIM);
        assert!(height <= MAX_GLYPH_DIM);
        // The product itself must be safely representable and small enough
        // that the allocation this feeds is never a real concern.
        assert!(
            u64::from(width) * u64::from(height)
                <= u64::from(MAX_GLYPH_DIM) * u64::from(MAX_GLYPH_DIM)
        );
    }

    #[test]
    fn clamp_glyph_dims_handles_negative_and_nan_bounds() {
        // `ab_glyph`'s bounds are attacker/font-controlled floats in
        // practice, not just "large" — must not panic or produce a
        // nonsensical (e.g. huge via a bad cast) result for any input.
        assert_eq!(clamp_glyph_dims(-5.0, -5.0), (0, 0));
        assert_eq!(clamp_glyph_dims(f32::NAN, f32::NAN), (0, 0));
        assert_eq!(
            clamp_glyph_dims(f32::INFINITY, f32::INFINITY),
            (MAX_GLYPH_DIM, MAX_GLYPH_DIM)
        );
    }

    #[test]
    fn bitmap_fallback_renders_ascii() {
        let glyph = bitmap_glyph('A', 16).expect("A renders");
        assert_eq!(glyph.width, 16);
        assert_eq!(glyph.height, 16);
        assert!(glyph.alpha.iter().any(|&a| a > 0));
    }

    #[test]
    fn bitmap_fallback_skips_non_ascii() {
        assert!(bitmap_glyph('繁', 16).is_none());
    }

    #[test]
    fn cache_evicts_at_capacity() {
        let mut cache = GlyphCache::default();
        for offset in 0..=MAX_CACHED_GLYPHS {
            let ch = char::from_u32(0x1000 + offset as u32).expect("valid char");
            cache.insert(GlyphKey { ch, size: 14 }, None);
        }
        assert_eq!(cache.values.len(), MAX_CACHED_GLYPHS);
        assert!(
            cache
                .get(&GlyphKey {
                    ch: '\u{1000}',
                    size: 14
                })
                .is_none()
        );
    }

    #[test]
    fn cell_metrics_are_positive() {
        let metrics = cell_metrics(14);
        assert!(metrics.width > 0);
        assert!(metrics.height > 0);
    }
}
