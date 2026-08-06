use ab_glyph::{Font, FontRef, ScaleFont};

use crate::contract::font::{
    FontDiscovery, FontError, FontFileCandidate, FontMetrics, FontRequest, OpaqueWindowHandle,
};

/// Font files probed in order, by absolute path.
///
/// TODO(linux): query fontconfig (`fc-match monospace`) before falling back to
/// this list. The paths below follow the Debian/Ubuntu layout; on Arch, Fedora,
/// NixOS, Alpine and slim containers none of them may exist, and the frontend
/// then silently drops to its built-in 8x8 bitmap face
/// (`src/platform/adapters/unix/frontend/font.rs` `resolved_name`), which looks
/// broken rather than merely unstyled. macOS does not share this risk: its
/// candidates live at stable system paths.
pub(crate) fn candidates() -> Vec<FontFileCandidate> {
    vec![
        FontFileCandidate {
            name: "DejaVu Sans Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "dejavu",
                "DejaVuSansMono.ttf",
            ],
        },
        FontFileCandidate {
            name: "Liberation Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "liberation",
                "LiberationMono-Regular.ttf",
            ],
        },
        FontFileCandidate {
            name: "Liberation Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "liberation2",
                "LiberationMono-Regular.ttf",
            ],
        },
        FontFileCandidate {
            name: "Noto Sans Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "noto",
                "NotoSansMono-Regular.ttf",
            ],
        },
        FontFileCandidate {
            name: "Noto Sans Mono CJK",
            components: &[
                "usr",
                "share",
                "fonts",
                "opentype",
                "noto",
                "NotoSansCJK-Regular.ttc",
            ],
        },
    ]
}

pub(crate) fn probe() -> FontDiscovery {
    let mut available_families = Vec::new();
    for candidate in candidates() {
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
        .into_iter()
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

pub(crate) fn create_terminal_font(
    _window: OpaqueWindowHandle,
    _request: FontRequest<'_>,
) -> Result<(isize, FontMetrics), FontError> {
    Err(FontError::Unsupported)
}

pub(crate) fn destroy_terminal_font(_raw: isize) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_never_claims_a_missing_candidate() {
        let facts = probe();
        assert_eq!(
            facts.primary_family,
            facts.available_families.first().copied()
        );
        assert!(facts.available_families.iter().all(|family| {
            candidates()
                .into_iter()
                .any(|candidate| candidate.name == *family && candidate.exists())
        }));
    }
}
