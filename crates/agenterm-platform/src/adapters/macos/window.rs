use crate::window::{DisplayBackendFacts, NativeTextWindowError, NativeTextWindowHost};
pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    DisplayBackendFacts {
        x11: false,
        wayland: false,
        headless: false,
    }
}

pub(crate) fn run_native_text_window(
    _host: Box<dyn NativeTextWindowHost>,
    _no_activate: bool,
) -> Result<(), NativeTextWindowError> {
    Err(NativeTextWindowError::Unsupported {
        reason: "native-text-window-adapter-pending".into(),
    })
}
