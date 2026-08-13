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

/// Write `text` through the host accessibility text interface (Linux:
/// AT-SPI `EditableText` `SetTextContents` / `InsertText`, or AT-SPI `Text`
/// plus the toolkit set-value when EditableText is absent: Chrome renderer
/// AX or the WebKitGTK eval helper). Never injects
/// keystrokes. A node without a writeable text interface fails typed.
pub fn set_node_text(
    window_handle: Option<isize>,
    node_id: &str,
    text: &str,
) -> Result<(), AccessibilityTreeError> {
    crate::selected::accessibility_tree::set_node_text(window_handle, node_id, text)
}

/// Read the node's independent accessible text (Linux: AT-SPI `Text.GetText`).
/// This is not the resolve-time snapshot `text` field and is not the
/// `send-text` reply's `matched.text`. A node with no Text interface
/// fails typed.
pub fn get_node_text(
    window_handle: Option<isize>,
    node_id: &str,
) -> Result<String, AccessibilityTreeError> {
    crate::selected::accessibility_tree::get_node_text(window_handle, node_id)
}

/// Route of the last successful `set_node_text` on this thread.
/// Linux: `"editable-text"` or `"text"`. Other hosts: `"editable-text"`.
pub fn last_text_write_via() -> &'static str {
    crate::selected::accessibility_tree::last_text_write_via()
}

/// Deliver `keys` through the host accessibility Device/key interface
/// (Linux: AT-SPI `DeviceEventListener` `NotifyEvent`). Never injects
/// XTest. A node without that interface fails typed.
pub fn send_node_keys(
    window_handle: Option<isize>,
    node_id: &str,
    keys: &str,
) -> Result<(), AccessibilityTreeError> {
    crate::selected::accessibility_tree::send_node_keys(window_handle, node_id, keys)
}

pub fn drain_bus() {
    crate::selected::accessibility_tree::drain_bus()
}
