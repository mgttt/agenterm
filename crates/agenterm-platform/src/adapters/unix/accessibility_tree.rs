//! Unix placeholder: accessibility tree is not wired on macOS yet (AX pending).

use crate::CapabilityStatus;
use crate::contract::accessibility_tree::{
    AccessibilityNodeAction, AccessibilityTree, AccessibilityTreeError,
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
