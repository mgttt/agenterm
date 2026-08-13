//! Platform-neutral window enumeration contract.

use std::borrow::Cow;

/// Bounds of a top-level window in physical screen pixels (top-origin).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// One display in top-origin coordinates (same space as [`WindowBounds`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScreenInfo {
    pub frame: WindowBounds,
    pub visible: WindowBounds,
    pub primary: bool,
}

/// A snapshot of one visible top-level window.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WindowInfo {
    /// Native window handle (HWND on Windows), valid for the observation instant.
    pub handle: isize,
    pub title: String,
    pub process_id: u32,
    pub app_name: String,
    pub bounds: WindowBounds,
    pub focused: bool,
    pub minimized: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WindowEnumerateError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl WindowEnumerateError {
    pub(crate) fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: code.into(),
            message: message.to_string(),
        }
    }
}
