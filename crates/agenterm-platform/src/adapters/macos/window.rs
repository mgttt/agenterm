use crate::window::{DisplayBackendFacts, NativeTextWindowError, NativeTextWindowHost};

#[path = "../unix/window.rs"]
mod unix;
pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    DisplayBackendFacts {
        x11: false,
        wayland: false,
        headless: false,
    }
}

pub(crate) fn run_native_text_window(
    host: Box<dyn NativeTextWindowHost>,
    no_activate: bool,
) -> Result<(), NativeTextWindowError> {
    use winit::platform::macos::EventLoopBuilderExtMacOS as _;
    unix::run_native_text_window(
        host,
        no_activate,
        "macos",
        |attributes, no_activate| attributes.with_active(!no_activate),
        |builder, no_activate| {
            builder.with_activate_ignoring_other_apps(!no_activate);
        },
    )
}
