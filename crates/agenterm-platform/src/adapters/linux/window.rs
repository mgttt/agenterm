use crate::window::{DisplayBackendFacts, NativeTextWindowError, NativeTextWindowHost};

#[cfg(all(feature = "input", feature = "ime"))]
use crate::contract::window_host::{PixelWindowApplication, PixelWindowError, PixelWindowOptions};

#[path = "../unix/window.rs"]
mod unix;
#[cfg(all(feature = "input", feature = "ime"))]
#[path = "../unix/window_host.rs"]
mod window_host;
mod x11_no_activate;
pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    DisplayBackendFacts {
        x11,
        wayland,
        headless: !(x11 || wayland),
    }
}

#[cfg(all(feature = "input", feature = "ime"))]
pub(crate) fn run_pixel_window(
    options: PixelWindowOptions,
    application: Box<dyn PixelWindowApplication>,
) -> Result<(), PixelWindowError> {
    if display_backend_facts().headless {
        return Err(PixelWindowError::Unsupported {
            reason: "headless-display".into(),
        });
    }
    window_host::run_pixel_window(options, application)
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
        x11_no_activate::prepare_window,
        |_builder, _no_activate| {},
    )
}
