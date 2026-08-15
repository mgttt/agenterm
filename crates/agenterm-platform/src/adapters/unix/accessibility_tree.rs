//! Unix placeholder for hosts without a selected a11y adapter.
//!
//! Linux uses AT-SPI2 and macOS uses AX under `cfg` selection in
//! `selected.rs`. This stub remains only for other Unix targets.

use crate::CapabilityStatus;
use crate::contract::accessibility_tree::{
    AccessibilityBounds, AccessibilityNodeAction, AccessibilitySelection, AccessibilityTree,
    AccessibilityTreeError,
};

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    }
}

pub(crate) fn tree_for_window(
    _window_handle: Option<isize>,
) -> Result<AccessibilityTree, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn drain_bus() {}

pub(crate) fn perform_node_action(
    _window_handle: Option<isize>,
    _node_id: &str,
    _action: AccessibilityNodeAction,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn set_node_text(
    _window_handle: Option<isize>,
    _node_id: &str,
    _text: &str,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn get_node_text(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<String, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn last_text_write_via() -> &'static str {
    "editable-text"
}

pub(crate) fn send_node_keys(
    _window_handle: Option<isize>,
    _node_id: &str,
    _keys: &str,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn scroll_node(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn get_node_extents(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<AccessibilityBounds, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn set_node_selection(
    _window_handle: Option<isize>,
    _node_id: &str,
    _start: i32,
    _end: i32,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn get_node_selection(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<AccessibilitySelection, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn set_node_caret_offset(
    _window_handle: Option<isize>,
    _node_id: &str,
    _offset: i32,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}

pub(crate) fn get_node_caret_offset(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<i32, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "accessibility-tree not wired on this unix host".into(),
    })
}
