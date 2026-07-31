//! Script Runtime clipboard service preserving its independent two-second API contract.

use crate::platform::{contract::script_clipboard::ScriptClipboardError, selected};

pub(crate) fn get_text() -> Result<String, ScriptClipboardError> {
    selected::script_clipboard::get_text()
}

pub(crate) fn set_text(text: &str) -> Result<(), ScriptClipboardError> {
    selected::script_clipboard::set_text(text)
}
