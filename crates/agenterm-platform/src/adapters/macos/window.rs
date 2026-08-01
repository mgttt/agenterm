use crate::window::{DisplayBackendFacts, NativeTextWindowError, NativeTextWindowHost};

#[cfg(all(feature = "input", feature = "ime"))]
use crate::contract::window_host::{PixelWindowApplication, PixelWindowError, PixelWindowOptions};

#[path = "../unix/window.rs"]
mod unix;
#[cfg(all(feature = "input", feature = "ime"))]
#[path = "../unix/window_host.rs"]
mod window_host;
pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    DisplayBackendFacts {
        x11: false,
        wayland: false,
        headless: false,
    }
}

#[cfg(all(feature = "input", feature = "ime"))]
pub(crate) fn run_pixel_window(
    options: PixelWindowOptions,
    application: Box<dyn PixelWindowApplication>,
) -> Result<(), PixelWindowError> {
    window_host::run_pixel_window(options, application)
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
