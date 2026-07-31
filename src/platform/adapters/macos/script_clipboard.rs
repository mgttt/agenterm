use crate::platform::contract::script_clipboard::ScriptClipboardError;

const UNSUPPORTED: ScriptClipboardError = ScriptClipboardError::new(
    "clipboard_unsupported",
    "native clipboard text is not implemented on this platform",
    Some("unsupported"),
);

pub(crate) fn get_text() -> Result<String, ScriptClipboardError> {
    Err(UNSUPPORTED)
}

pub(crate) fn set_text(_text: &str) -> Result<(), ScriptClipboardError> {
    Err(UNSUPPORTED)
}
