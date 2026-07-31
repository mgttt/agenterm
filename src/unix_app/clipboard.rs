/// Bounded Unicode clipboard write through the active platform adapter.
pub(super) fn set_clipboard_text(text: &str) -> Result<(), String> {
    crate::platform::services::ui_clipboard::set_text(text).map_err(|error| error.message())
}

/// Bounded Unicode clipboard read through the active platform adapter.
pub(super) fn get_clipboard_text() -> Result<String, String> {
    crate::platform::services::ui_clipboard::get_text().map_err(|error| error.message())
}

/// Fast probe for Unicode clipboard text without reading the full payload when possible.
pub(super) fn clipboard_has_unicode_text() -> bool {
    crate::platform::services::ui_clipboard::has_unicode_text()
}
