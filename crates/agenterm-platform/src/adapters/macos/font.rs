use ab_glyph::{Font, FontRef, ScaleFont};

use crate::contract::font::{
    FontDiscovery, FontError, FontFileCandidate, FontMetrics, FontRequest, OpaqueWindowHandle,
    RasterGlyph,
};

pub(crate) fn candidates() -> &'static [FontFileCandidate] {
    &[
        FontFileCandidate {
            name: "SF Mono",
            components: &["System", "Library", "Fonts", "SFNSMono.ttf"],
        },
        FontFileCandidate {
            name: "Hiragino Sans GB",
            components: &["System", "Library", "Fonts", "Hiragino Sans GB.ttc"],
        },
        FontFileCandidate {
            name: "Apple Symbols",
            components: &["System", "Library", "Fonts", "Apple Symbols.ttf"],
        },
    ]
}

/// Fonts consulted only for glyphs the primary face does not have, so CJK and
/// emoji do not render as blank cells. Never chosen as the primary face.
pub(crate) fn fallback_candidates() -> &'static [FontFileCandidate] {
    &[
        FontFileCandidate {
            name: "PingFang",
            components: &["System", "Library", "Fonts", "PingFang.ttc"],
        },
        FontFileCandidate {
            name: "Hiragino Sans GB",
            components: &["System", "Library", "Fonts", "Hiragino Sans GB.ttc"],
        },
        FontFileCandidate {
            name: "Apple Color Emoji",
            components: &["System", "Library", "Fonts", "Apple Color Emoji.ttc"],
        },
    ]
}

pub(crate) fn probe() -> FontDiscovery {
    let mut available_families = Vec::new();
    for &candidate in candidates() {
        if candidate.exists() && !available_families.contains(&candidate.name) {
            available_families.push(candidate.name);
        }
    }
    FontDiscovery {
        primary_family: available_families.first().copied(),
        available_families,
    }
}

pub(crate) fn primary_family_name() -> Result<&'static str, FontError> {
    probe().primary_family.ok_or(FontError::Unavailable)
}

pub(crate) fn primary_metrics(size_px: u16) -> Result<FontMetrics, FontError> {
    let candidate = candidates()
        .iter()
        .copied()
        .find(|candidate| candidate.exists())
        .ok_or(FontError::Unavailable)?;
    let data = std::fs::read(candidate.absolute_path()).map_err(|_| FontError::MetricsFailed)?;
    let font = FontRef::try_from_slice(&data).map_err(|_| FontError::MetricsFailed)?;
    let size_px = size_px.clamp(8, 72);
    let scaled = font.as_scaled(f32::from(size_px));
    let ascent = scaled.ascent();
    let cell_width = scaled.h_advance(scaled.glyph_id('M'));
    let cell_height = scaled.height();
    if ![ascent, cell_width, cell_height]
        .into_iter()
        .all(|metric| metric.is_finite() && metric > 0.0)
    {
        return Err(FontError::MetricsFailed);
    }
    Ok(FontMetrics {
        family: Some(candidate.name),
        size_px,
        cell_width,
        cell_height,
        ascent,
    })
}

pub(crate) fn probe_capability() -> Result<(), FontError> {
    primary_metrics(14).map(|_| ())
}

pub(crate) fn rasterizer_name() -> Result<String, FontError> {
    crate::selected::portable_font_raster::rasterizer_name(candidates, fallback_candidates)
}

pub(crate) fn rasterize(ch: char, size_px: u16) -> Result<Option<RasterGlyph>, FontError> {
    crate::selected::portable_font_raster::rasterize(candidates, fallback_candidates, ch, size_px)
}

pub(crate) fn create_terminal_font(
    _window: OpaqueWindowHandle,
    _request: FontRequest<'_>,
) -> Result<(isize, FontMetrics), FontError> {
    Err(FontError::Unsupported)
}

pub(crate) fn destroy_terminal_font(_raw: isize) {}
