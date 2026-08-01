//! OS-neutral native-font service.

use crate::CapabilityStatus;
pub use crate::contract::font::{
    FontDiscovery, FontError, FontFileCandidate, FontMetrics, FontRequest, OpaqueWindowHandle,
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

pub fn probe() -> FontDiscovery {
    selected::font::probe()
}

pub fn primary_family_name() -> Result<&'static str, FontError> {
    selected::font::primary_family_name()
}

pub fn primary_metrics(size_px: u16) -> Result<FontMetrics, FontError> {
    selected::font::primary_metrics(size_px)
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
