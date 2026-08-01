use crate::window::{DisplayBackendFacts, NativeTextWindowError, NativeTextWindowHost};
pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    DisplayBackendFacts {
        x11,
        wayland,
        headless: !(x11 || wayland),
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
