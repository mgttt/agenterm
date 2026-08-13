//! Platform-neutral accessibility / control-tree contract.
//!
//! Windows maps to UIA, Linux to AT-SPI2, macOS to AX. Product callers use the
//! same node shape regardless of host backend.

use std::borrow::Cow;

/// Screen-space bounds in physical pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AccessibilityBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// One node in a flattened accessibility tree.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AccessibilityNode {
    /// Stable path id from the application root, e.g. `/0/2/5`.
    pub id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub name: String,
    pub states: Vec<String>,
    pub bounds: AccessibilityBounds,
    /// Action names exposed by the backend (`click`, `focus`, ...).
    pub actions: Vec<String>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub text: Option<String>,
}

/// Flattened control tree for one observation instant.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AccessibilityTree {
    pub backend: &'static str,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub window_handle: Option<isize>,
    pub root_id: String,
    pub nodes: Vec<AccessibilityNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AccessibilityNodeAction {
    Click,
    Focus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AccessibilityTreeError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl AccessibilityTreeError {
    // Only a selected native backend constructs typed mechanism failures; the
    // neutral contract remains available when the selected backend is a stub.
    #[allow(dead_code)]
    pub(crate) fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: code.into(),
            message: message.to_string(),
        }
    }
}
