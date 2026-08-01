//! OS-neutral font discovery, measurement, and native-face contracts.

use std::{fmt, path::PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontFileCandidate {
    pub name: &'static str,
    pub components: &'static [&'static str],
}

impl FontFileCandidate {
    /// Resolves the candidate below the host filesystem root.
    pub fn absolute_path(self) -> PathBuf {
        self.components.iter().fold(
            PathBuf::from(std::path::MAIN_SEPARATOR_STR),
            |path, component| path.join(component),
        )
    }

    pub fn exists(self) -> bool {
        self.absolute_path().is_file()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontDiscovery {
    pub available_families: Vec<&'static str>,
    pub primary_family: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// Discovered family when the adapter can identify it without leaking a
    /// native SDK type. Windows GDI requests retain the family at the caller.
    pub family: Option<&'static str>,
    pub size_px: u16,
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontRequest<'a> {
    pub family: &'a str,
    pub point_size: u16,
}

/// A caller-supplied native window identity without exposing an OS SDK type.
///
/// This is needed only by adapters, such as GDI, which measure a font against
/// a window's device context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct OpaqueWindowHandle(isize);

impl OpaqueWindowHandle {
    /// Wraps a live platform window handle.
    ///
    /// # Safety
    ///
    /// `raw` must identify a live window on the selected target for the full
    /// duration of the operation receiving this token.
    pub const unsafe fn from_raw(raw: isize) -> Self {
        Self(raw)
    }

    #[allow(dead_code)] // Read by the selected Windows adapter.
    pub(crate) const fn get(self) -> isize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FontError {
    Unsupported,
    Unavailable,
    InvalidRequest,
    DeviceContextUnavailable,
    CreateFailed,
    MetricsFailed,
}

impl FontError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unsupported => "font_unsupported",
            Self::Unavailable => "font_unavailable",
            Self::InvalidRequest => "font_invalid_request",
            Self::DeviceContextUnavailable => "font_device_context_unavailable",
            Self::CreateFailed => "font_create_failed",
            Self::MetricsFailed => "font_metrics_failed",
        }
    }
}

impl fmt::Display for FontError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for FontError {}
