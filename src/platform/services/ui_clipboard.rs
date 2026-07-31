//! OS-neutral clipboard service for native frontend projections.

use crate::platform::{contract::ui_clipboard::UiClipboardError, selected};

pub(crate) fn set_text(text: &str) -> Result<(), UiClipboardError> {
    selected::ui_clipboard::set_text(text)
}

pub(crate) fn get_text() -> Result<String, UiClipboardError> {
    selected::ui_clipboard::get_text()
}

pub(crate) fn has_unicode_text() -> bool {
    selected::ui_clipboard::has_unicode_text()
}
