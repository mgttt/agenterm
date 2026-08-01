//! OS-neutral clipboard service for native frontend projections.

use crate::platform::contract::ui_clipboard::UiClipboardError;

pub(crate) fn set_text(text: &str) -> Result<(), UiClipboardError> {
    agenterm_platform::clipboard::set_text(text)
}

/// Read Unicode text with a bound supplied by the consuming surface.
///
/// Clipboard mechanics must not inherit terminal paste policy: each caller
/// chooses the largest payload it can safely retain and process.
pub(crate) fn get_text_bounded(max_read_bytes: usize) -> Result<String, UiClipboardError> {
    agenterm_platform::clipboard::get_text(max_read_bytes)
}

/// Compatibility projection for callers that have no product-specific read
/// budget. New callers must use [`get_text_bounded`].
pub(crate) fn get_text() -> Result<String, UiClipboardError> {
    get_text_bounded(usize::MAX)
}

pub(crate) fn has_unicode_text() -> bool {
    agenterm_platform::clipboard::has_unicode_text()
}
