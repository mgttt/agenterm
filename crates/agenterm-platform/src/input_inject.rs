//! Input injection facade (portable entry point).

use crate::CapabilityStatus;
pub use crate::contract::input_inject::{InputInjectError, PointerButton, PointerPosition};

pub fn capability_status() -> CapabilityStatus {
    crate::selected::input_inject::capability_status()
}

pub fn pointer_move(position: PointerPosition) -> Result<(), InputInjectError> {
    crate::selected::input_inject::pointer_move(position)
}

pub fn pointer_click(
    position: PointerPosition,
    button: PointerButton,
    clicks: u32,
) -> Result<(), InputInjectError> {
    crate::selected::input_inject::pointer_click(position, button, clicks)
}

/// Types `text` into the focused control using Unicode key events.
pub fn type_text(text: &str) -> Result<(), InputInjectError> {
    crate::selected::input_inject::type_text(text)
}

/// Sends a hotkey such as `ctrl+s`, `alt+f4` or `enter`.
pub fn send_keys(shortcut: &str) -> Result<(), InputInjectError> {
    crate::selected::input_inject::send_keys(shortcut)
}
