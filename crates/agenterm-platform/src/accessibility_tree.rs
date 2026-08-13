//! Accessibility / control-tree facade.

pub use crate::contract::accessibility_tree::{
    AccessibilityBounds, AccessibilityNode, AccessibilityNodeAction, AccessibilityTree,
    AccessibilityTreeError,
};

pub fn capability_status() -> crate::CapabilityStatus {
    crate::selected::accessibility_tree::capability_status()
}

pub fn tree_for_window(
    window_handle: Option<isize>,
) -> Result<AccessibilityTree, AccessibilityTreeError> {
    crate::selected::accessibility_tree::tree_for_window(window_handle)
}

pub fn perform_node_action(
    window_handle: Option<isize>,
    node_id: &str,
    action: AccessibilityNodeAction,
) -> Result<(), AccessibilityTreeError> {
    crate::selected::accessibility_tree::perform_node_action(window_handle, node_id, action)
}
