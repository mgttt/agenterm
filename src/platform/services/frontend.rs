//! Cfg-free product frontend dispatcher over platform-neutral extensions.

use crate::wake_signal::WakeSignal;

#[path = "../adapters/windows/remote_frontend.rs"]
mod remote_frontend;
#[path = "../adapters/unix/frontend/mod.rs"]
mod unix_frontend;
#[path = "../adapters/windows/frontend.rs"]
mod windows_frontend;

pub fn run_gui_entry() -> i32 {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => windows_frontend::run_gui_entry(),
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            unix_frontend::run_gui_entry()
        }
        _ => {
            eprintln!("AgenTerm GUI is unsupported on this platform");
            1
        }
    }
}

pub(crate) fn request_gui_wake(wake_window: isize, wake_signal: &WakeSignal) {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => {
            windows_frontend::request_gui_wake(wake_window, wake_signal);
        }
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            unix_frontend::request_gui_wake(wake_window, wake_signal);
        }
        _ => panic!("AgenTerm GUI wake is unsupported on this platform"),
    }
}
