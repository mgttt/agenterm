/// Bounded Unicode clipboard write through the active platform adapter.
pub(super) fn set_clipboard_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::clipboard::set_text(text).map_err(|error| error.message())
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::clipboard::set_text(text).map_err(|error| error.message())
    }
}

/// Bounded Unicode clipboard read through the active platform adapter.
pub(super) fn get_clipboard_text() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::clipboard::get_text().map_err(|error| error.message())
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::clipboard::get_text().map_err(|error| error.message())
    }
}

/// Fast probe for Unicode clipboard text without reading the full payload when possible.
pub(super) fn clipboard_has_unicode_text() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::clipboard::has_unicode_text()
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::clipboard::has_unicode_text()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_clipboard_delegates_to_platform_capability_boundary() {
        use crate::platform::{CapabilityKind, CapabilityStatus};
        let status = crate::platform::linux::capability_status(CapabilityKind::Clipboard);
        assert!(matches!(
            status,
            CapabilityStatus::Available
                | CapabilityStatus::Failed {
                    code: "clipboard_unavailable",
                    ..
                }
                | CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
        ));
    }
}
