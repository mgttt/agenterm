//! Linux frontend clipboard adapter.

use crate::platform::contract::ui_clipboard::UiClipboardError;

pub(crate) fn set_text(text: &str) -> Result<(), UiClipboardError> {
    crate::platform::selected::native::clipboard::set_text(text).map_err(map_error)
}

pub(crate) fn get_text() -> Result<String, UiClipboardError> {
    crate::platform::selected::native::clipboard::get_text().map_err(map_error)
}

pub(crate) fn has_unicode_text() -> bool {
    crate::platform::selected::native::clipboard::has_unicode_text()
}

fn map_error(
    error: crate::platform::selected::native::clipboard::ClipboardError,
) -> UiClipboardError {
    use crate::platform::selected::native::clipboard::ClipboardError;

    let code = match &error {
        ClipboardError::Unavailable { .. } => "clipboard_unavailable",
        ClipboardError::TooLarge { .. } => "clipboard_too_large",
        ClipboardError::Timeout { .. } => "clipboard_timeout",
        ClipboardError::Backend { .. } => "clipboard_backend_error",
    };
    UiClipboardError::Failed {
        code,
        message: error.message(),
    }
}
