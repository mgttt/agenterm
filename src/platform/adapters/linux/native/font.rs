//! Linux font discovery / capability bridge for platform migration slice-2
//! Adapter-private native mechanism selected only by platform::selected.
//! (contract revision 1).
//!
//! Owns Linux monospace candidate paths, primary cell metrics, and typed
//! availability. Glyph rasterization and caches remain in `unix_app::font`;
//! this module must not invent shared Font API fields — request a contract
//! revision if needed.
//!
//! Failure modes use [`CapabilityStatus::Failed`] / never claim Available when
//! no candidate file exists or primary metrics cannot be measured.

#![cfg(target_os = "linux")]

use std::path::PathBuf;

use ab_glyph::{Font, FontRef, ScaleFont};

use crate::platform::CapabilityStatus;

/// Linux system font candidate (adapter-local discovery metadata).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinuxFontCandidate {
    pub name: &'static str,
    pub components: &'static [&'static str],
}

impl LinuxFontCandidate {
    pub(crate) fn absolute_path(self) -> PathBuf {
        self.components.iter().fold(
            PathBuf::from(std::path::MAIN_SEPARATOR_STR),
            |path, part| path.join(part),
        )
    }

    pub(crate) fn exists(self) -> bool {
        self.absolute_path().is_file()
    }
}

/// Ordered Linux monospace / CJK fallback candidates.
pub(crate) fn candidates() -> &'static [LinuxFontCandidate] {
    &[
        LinuxFontCandidate {
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
        LinuxFontCandidate {
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
        LinuxFontCandidate {
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
        LinuxFontCandidate {
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
        LinuxFontCandidate {
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

/// Result of probing the Linux font candidate table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinuxFontDiscovery {
    pub available_families: Vec<&'static str>,
    pub primary_family: Option<&'static str>,
}

impl LinuxFontDiscovery {
    pub(crate) fn probe() -> Self {
        let mut available_families = Vec::new();
        for candidate in candidates() {
            if candidate.exists() && !available_families.contains(&candidate.name) {
                available_families.push(candidate.name);
            }
        }
        let primary_family = available_families.first().copied();
        Self {
            available_families,
            primary_family,
        }
    }
}

/// Typed Linux font discovery / metrics failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinuxFontError {
    Unavailable,
    MetricsFailed,
}

impl LinuxFontError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "font_unavailable",
            Self::MetricsFailed => "font_metrics_failed",
        }
    }

    pub(crate) fn to_capability_status(self) -> CapabilityStatus {
        let message = match self {
            Self::Unavailable => "no Linux system monospace font candidate found",
            Self::MetricsFailed => "Linux primary font metrics could not be measured",
        };
        CapabilityStatus::Failed {
            code: self.code(),
            message: message.to_string(),
        }
    }
}

/// Primary face cell metrics at a requested pixel size (adapter-local).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LinuxFontMetrics {
    pub family: &'static str,
    pub size_px: u16,
    pub ascent: f32,
    pub advance_m: f32,
}

/// Font capability: Available only when a candidate exists and metrics load.
pub(crate) fn font_capability_status() -> CapabilityStatus {
    match primary_metrics(14) {
        Ok(_) => CapabilityStatus::Available,
        Err(error) => error.to_capability_status(),
    }
}

/// Primary discovered family name (first existing candidate).
pub(crate) fn primary_family_name() -> Result<&'static str, LinuxFontError> {
    LinuxFontDiscovery::probe()
        .primary_family
        .ok_or(LinuxFontError::Unavailable)
}

/// First existing candidate path, if any.
pub(crate) fn primary_candidate() -> Result<LinuxFontCandidate, LinuxFontError> {
    candidates()
        .iter()
        .copied()
        .find(|candidate| candidate.exists())
        .ok_or(LinuxFontError::Unavailable)
}

/// Measure primary monospace cell metrics at `size_px`.
pub(crate) fn primary_metrics(size_px: u16) -> Result<LinuxFontMetrics, LinuxFontError> {
    let candidate = primary_candidate()?;
    let data =
        std::fs::read(candidate.absolute_path()).map_err(|_| LinuxFontError::MetricsFailed)?;
    let font = FontRef::try_from_slice(&data).map_err(|_| LinuxFontError::MetricsFailed)?;
    let size = size_px.clamp(8, 72);
    let scaled = font.as_scaled(f32::from(size));
    let ascent = scaled.ascent();
    let advance_m = scaled.h_advance(scaled.glyph_id('M'));
    if !(ascent.is_finite() && ascent > 0.0 && advance_m.is_finite() && advance_m > 0.0) {
        return Err(LinuxFontError::MetricsFailed);
    }
    Ok(LinuxFontMetrics {
        family: candidate.name,
        size_px: size,
        ascent,
        advance_m,
    })
}

/// Snapshot facts for evidence / diagnostics (not authorization).
pub(crate) fn font_facts() -> LinuxFontDiscovery {
    LinuxFontDiscovery::probe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_include_dejavu_and_liberation() {
        let names: Vec<_> = candidates().iter().map(|c| c.name).collect();
        assert!(names.contains(&"DejaVu Sans Mono"));
        assert!(names.contains(&"Liberation Mono"));
    }

    #[test]
    fn discovery_reports_primary_when_dejavu_present() {
        let dejavu = candidates()
            .iter()
            .find(|c| c.name == "DejaVu Sans Mono")
            .expect("dejavu candidate");
        if !dejavu.exists() {
            // Environment without fonts: capability must still be typed Failed.
            assert!(matches!(
                font_capability_status(),
                CapabilityStatus::Failed {
                    code: "font_unavailable" | "font_metrics_failed",
                    ..
                }
            ));
            return;
        }
        let facts = font_facts();
        assert_eq!(facts.primary_family, Some("DejaVu Sans Mono"));
        assert_eq!(primary_family_name(), Ok("DejaVu Sans Mono"));
        assert_eq!(font_capability_status(), CapabilityStatus::Available);
        let metrics = primary_metrics(14).expect("primary metrics");
        assert_eq!(metrics.family, "DejaVu Sans Mono");
        assert!(metrics.ascent > 0.0);
        assert!(metrics.advance_m > 0.0);
    }

    #[test]
    fn missing_candidate_path_is_not_silently_available() {
        let missing = LinuxFontCandidate {
            name: "Missing Face",
            components: &["tmp", "agenterm-missing-font.ttf"],
        };
        assert!(!missing.exists());
    }

    #[test]
    fn unavailable_error_maps_to_typed_capability_status() {
        assert!(matches!(
            LinuxFontError::Unavailable.to_capability_status(),
            CapabilityStatus::Failed {
                code: "font_unavailable",
                ..
            }
        ));
        assert!(matches!(
            LinuxFontError::MetricsFailed.to_capability_status(),
            CapabilityStatus::Failed {
                code: "font_metrics_failed",
                ..
            }
        ));
    }
}
