//! Windows adapter declaration for the Unix frontend clipboard service.

use crate::platform::contract::ui_clipboard::UiClipboardError;

const UNSUPPORTED: &str = "unix frontend clipboard service is unavailable on Windows";

pub(crate) fn set_text(_: &str) -> Result<(), UiClipboardError> {
    Err(UiClipboardError::Unsupported {
        reason: UNSUPPORTED,
    })
}

pub(crate) fn get_text() -> Result<String, UiClipboardError> {
    Err(UiClipboardError::Unsupported {
        reason: UNSUPPORTED,
    })
}

pub(crate) const fn has_unicode_text() -> bool {
    false
}
