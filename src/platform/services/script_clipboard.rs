//! Script Runtime clipboard projection over the reusable native facade.

use std::time::Duration;

use crate::platform::contract::script_clipboard::ScriptClipboardError;

const SCRIPT_CLIPBOARD_OPEN_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn get_text() -> Result<String, ScriptClipboardError> {
    agenterm_platform::clipboard::get_text_with_timeout(usize::MAX, SCRIPT_CLIPBOARD_OPEN_TIMEOUT)
        .map_err(|error| map_error(error, "clipboard_read"))
}

pub(crate) fn set_text(text: &str) -> Result<(), ScriptClipboardError> {
    agenterm_platform::clipboard::set_text_with_timeout(text, SCRIPT_CLIPBOARD_OPEN_TIMEOUT)
        .map_err(|error| map_error(error, "clipboard_write"))
}

fn map_error(
    error: agenterm_platform::contract::clipboard::ClipboardError,
    fallback_code: &'static str,
) -> ScriptClipboardError {
    match error {
        agenterm_platform::contract::clipboard::ClipboardError::Unsupported { .. } => {
            ScriptClipboardError::new(
                "clipboard_unsupported",
                "native clipboard text is not implemented on this platform",
                Some("unsupported"),
            )
        }
        agenterm_platform::contract::clipboard::ClipboardError::Failed { code, .. } => {
            match code.as_ref() {
                "clipboard_busy" => ScriptClipboardError::new(
                    "clipboard_open",
                    "native clipboard could not be opened within two seconds",
                    Some("busy"),
                ),
                "clipboard_unavailable" => ScriptClipboardError::new(
                    "clipboard_text_unavailable",
                    "clipboard does not contain Unicode text",
                    Some("not_found"),
                ),
                "clipboard_too_large" => ScriptClipboardError::new(
                    "clipboard_text_too_large",
                    "clipboard text exceeds the runtime allocation bound",
                    Some("limit"),
                ),
                "clipboard_invalid_utf16" => ScriptClipboardError::new(
                    "clipboard_text_invalid",
                    "clipboard text is not valid UTF-16",
                    Some("invalid_data"),
                ),
                "clipboard_timeout" => ScriptClipboardError::new(
                    "clipboard_timeout",
                    "clipboard helper exceeded its deadline",
                    Some("timeout"),
                ),
                _ => ScriptClipboardError::new(
                    fallback_code,
                    "native clipboard operation failed",
                    Some("platform_error"),
                ),
            }
        }
        _ => ScriptClipboardError::new(
            fallback_code,
            "native clipboard operation failed",
            Some("platform_error"),
        ),
    }
}
