//! OS-neutral native-font service.

use crate::CapabilityStatus;
pub use crate::contract::font::{
    FontDiscovery, FontError, FontFileCandidate, FontMetrics, FontRequest, OpaqueWindowHandle,
    RasterGlyph,
};
use crate::selected;

impl FontError {
    pub fn to_capability_status(self) -> CapabilityStatus {
        let message = match self {
            Self::Unsupported => "native font creation is unsupported on this platform",
            Self::Unavailable => "no system font candidate is available",
            Self::InvalidRequest => "the font request is invalid",
            Self::DeviceContextUnavailable => "the native font device context is unavailable",
            Self::CreateFailed => "native font creation failed",
            Self::MetricsFailed => "native font metrics could not be measured",
            Self::RasterFailed => "native font glyph rasterization failed",
            Self::GlyphTooLarge => "native font glyph exceeds the allocation bound",
        };
        match self {
            Self::Unsupported => CapabilityStatus::Unsupported {
                reason: "native-font-creation-unsupported".into(),
            },
            _ => CapabilityStatus::Failed {
                code: self.code().into(),
                message: message.to_owned(),
            },
        }
    }
}

pub fn candidates() -> Vec<FontFileCandidate> {
    selected::font::candidates()
}

/// Faces consulted only for glyphs the primary font lacks.
///
/// The primary candidates are fixed-pitch monospace faces for stable cell metrics.
/// Without coverage fallbacks, CJK and emoji can still render as blank cells
/// (cell width reserved, glyphs missing). These are never selected as the
/// primary face.
pub fn fallback_candidates() -> Vec<FontFileCandidate> {
    selected::font::fallback_candidates()
}

pub fn probe() -> FontDiscovery {
    selected::font::probe()
}

pub fn primary_family_name() -> Result<&'static str, FontError> {
    selected::font::primary_family_name()
}

pub fn primary_metrics(size_px: u16) -> Result<FontMetrics, FontError> {
    selected::font::primary_metrics(size_px)
}

/// Name of the face actually selected by the platform rasterizer.
pub fn rasterizer_name() -> Result<String, FontError> {
    selected::font::rasterizer_name()
}

/// Rasterizes one Unicode scalar without exposing font files or native handles.
pub fn rasterize(ch: char, size_px: u16) -> Result<Option<RasterGlyph>, FontError> {
    selected::font::rasterize(ch, size_px)
}

pub fn capability_status() -> CapabilityStatus {
    selected::font::probe_capability()
        .map(|()| CapabilityStatus::Available)
        .unwrap_or_else(FontError::to_capability_status)
}

/// A selected-platform font resource with deterministic native cleanup.
#[derive(Debug)]
pub struct NativeFont {
    raw: isize,
    metrics: FontMetrics,
}

impl NativeFont {
    pub(crate) const fn new(raw: isize, metrics: FontMetrics) -> Self {
        Self { raw, metrics }
    }

    /// Returns an opaque resource identity for product-native renderer glue.
    /// It is deliberately not an SDK-specific `HFONT` or toolkit type.
    pub const fn raw_handle(&self) -> isize {
        self.raw
    }

    pub const fn metrics(&self) -> FontMetrics {
        self.metrics
    }
}

impl Drop for NativeFont {
    fn drop(&mut self) {
        selected::font::destroy_terminal_font(self.raw);
    }
}

pub fn create_terminal_font(
    window: OpaqueWindowHandle,
    request: FontRequest<'_>,
) -> Result<NativeFont, FontError> {
    let (raw, metrics) = selected::font::create_terminal_font(window, request)?;
    Ok(NativeFont::new(raw, metrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_fallbacks_exist_and_are_distinct_from_the_primary_face() {
        // A terminal on a CJK system renders blank cells if this list is
        // empty, so its emptiness is a real regression, not a style nit.
        let fallbacks = super::fallback_candidates();
        assert!(
            !fallbacks.is_empty(),
            "every platform needs coverage fallbacks"
        );

        // The primary face must stay a monospace Latin font: cell metrics come
        // from it, so a proportional CJK face must never lead the list.
        let primary = super::candidates();
        assert!(!primary.is_empty());
        assert_ne!(
            primary.first().map(|c| c.name),
            fallbacks.first().map(|c| c.name),
            "a coverage font must not become the primary face"
        );
    }

    #[test]
    fn failures_map_to_stable_typed_statuses() {
        assert!(matches!(
            FontError::MetricsFailed.to_capability_status(),
            CapabilityStatus::Failed {
                code,
                message
            } if code == "font_metrics_failed" && !message.is_empty()
        ));
        assert!(matches!(
            FontError::Unsupported.to_capability_status(),
            CapabilityStatus::Unsupported { .. }
        ));
    }
}
