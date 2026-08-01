//! Platform-neutral screenshot encoding contract.

use std::{borrow::Cow, num::NonZeroIsize, path::Path};

use crate::CapabilityStatus;

/// A strict rectangle in framebuffer pixel coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XrgbClip {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl XrgbClip {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Opaque identity for a native window whose pixels may be captured.
///
/// Construction is unsafe because the caller owns the window lifetime and must
/// keep the handle valid until the capture call returns.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScreenshotWindowHandle(NonZeroIsize);

impl ScreenshotWindowHandle {
    /// # Safety
    ///
    /// `raw` must identify a live native window for the duration of the call.
    pub const unsafe fn from_raw(raw: isize) -> Option<Self> {
        match NonZeroIsize::new(raw) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    #[allow(dead_code)] // Read by the selected Windows adapter.
    pub(crate) const fn raw(self) -> isize {
        self.0.get()
    }
}

/// Native window pixels to capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NativeCaptureArea {
    Window,
    Client {
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    },
}

/// A caller-owned little-endian `0x00RRGGBB` framebuffer and output path.
#[derive(Clone, Copy, Debug)]
pub struct XrgbFrame<'a> {
    pub path: &'a Path,
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u32],
    pub clip: Option<XrgbClip>,
}

impl<'a> XrgbFrame<'a> {
    pub const fn new(path: &'a Path, width: u32, height: u32, pixels: &'a [u32]) -> Self {
        Self {
            path,
            width,
            height,
            pixels,
            clip: None,
        }
    }

    pub const fn with_clip(mut self, clip: XrgbClip) -> Self {
        self.clip = Some(clip);
        self
    }

    pub const fn path(&self) -> &'a Path {
        self.path
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn pixels(&self) -> &'a [u32] {
        self.pixels
    }

    pub const fn clip(&self) -> Option<XrgbClip> {
        self.clip
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ScreenshotWriteResult {
    pub frame_width: u32,
    pub frame_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub output_pixels: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum UiScreenshotError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl UiScreenshotError {
    pub fn code(&self) -> &str {
        match self {
            Self::Unsupported { .. } => "screenshot_unsupported",
            Self::Failed { code, .. } => code.as_ref(),
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unsupported { reason } => format!("screenshot unsupported: {reason}"),
            Self::Failed { message, .. } => message.clone(),
        }
    }

    pub fn to_capability_status(&self) -> CapabilityStatus {
        match self {
            Self::Unsupported { reason } => CapabilityStatus::Unsupported {
                reason: reason.clone(),
            },
            Self::Failed { code, message } => CapabilityStatus::Failed {
                code: code.clone(),
                message: message.clone(),
            },
        }
    }

    pub(crate) fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self::Failed {
            code: Cow::Borrowed(code),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for UiScreenshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for UiScreenshotError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_preserves_caller_owned_inputs() {
        let pixels = [0_u32; 4];
        let frame = XrgbFrame::new(Path::new("capture.png"), 2, 2, &pixels)
            .with_clip(XrgbClip::new(1, 1, 1, 1));
        assert_eq!(frame.path(), Path::new("capture.png"));
        assert_eq!(
            (frame.width(), frame.height(), frame.pixels().len()),
            (2, 2, 4)
        );
        assert_eq!(frame.clip().expect("clip").width, 1);
    }

    #[test]
    fn typed_failure_preserves_code_and_status() {
        let error = UiScreenshotError::failed("screenshot_invalid_clip", "outside frame");
        assert_eq!(error.code(), "screenshot_invalid_clip");
        assert_eq!(error.message(), "outside frame");
        assert_eq!(
            error.to_capability_status(),
            CapabilityStatus::Failed {
                code: Cow::Borrowed("screenshot_invalid_clip"),
                message: "outside frame".to_owned(),
            }
        );
    }

    #[test]
    fn null_native_window_handle_is_rejected() {
        // SAFETY: zero is intentionally supplied to verify construction rejects
        // it before any adapter can observe a native handle.
        assert!(unsafe { ScreenshotWindowHandle::from_raw(0) }.is_none());
    }
}
