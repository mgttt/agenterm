//! Product compatibility wrapper for the reusable clipboard facade.

use windows_sys::Win32::Foundation::HWND;

pub(crate) struct ClipboardError(agenterm_platform::contract::clipboard::ClipboardError);

impl ClipboardError {
    pub(crate) fn to_capability_status(&self) -> crate::platform::CapabilityStatus {
        match &self.0 {
            agenterm_platform::contract::clipboard::ClipboardError::Unsupported { reason } => {
                crate::platform::CapabilityStatus::Unsupported {
                    reason: match reason {
                        std::borrow::Cow::Borrowed(reason) => reason,
                        std::borrow::Cow::Owned(_) => "clipboard-unsupported",
                    },
                }
            }
            agenterm_platform::contract::clipboard::ClipboardError::Failed { code, message } => {
                crate::platform::CapabilityStatus::Failed {
                    code: match code {
                        std::borrow::Cow::Borrowed(code) => code,
                        std::borrow::Cow::Owned(_) => "clipboard_failed",
                    },
                    message: message.clone(),
                }
            }
            _ => crate::platform::CapabilityStatus::Failed {
                code: "clipboard_failed",
                message: self.0.to_string(),
            },
        }
    }
}

pub(crate) fn set_text(_owner: HWND, text: &str) -> Result<(), ClipboardError> {
    agenterm_platform::clipboard::set_text(text).map_err(ClipboardError)
}

pub(crate) fn get_text(max_utf8_bytes: usize) -> Result<String, ClipboardError> {
    agenterm_platform::clipboard::get_text(max_utf8_bytes).map_err(ClipboardError)
}

pub(crate) fn has_unicode_text() -> bool {
    agenterm_platform::clipboard::has_unicode_text()
}
