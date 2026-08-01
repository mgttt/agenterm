use crate::window::{DisplayBackendFacts, NativeTextWindowError, NativeTextWindowHost};

#[path = "../unix/window.rs"]
mod unix;
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
    host: Box<dyn NativeTextWindowHost>,
    no_activate: bool,
) -> Result<(), NativeTextWindowError> {
    if display_backend_facts().headless {
        return Err(NativeTextWindowError::Unsupported {
            reason: "headless-display".into(),
        });
    }
    unix::run_native_text_window(
        host,
        no_activate,
        "linux",
        |attributes, no_activate| attributes.with_active(!no_activate),
        |_builder, _no_activate| {},
    )
}
