//! macOS application activation policy for foreground-safe automation.

#![cfg(target_os = "macos")]

use winit::{event_loop::EventLoopBuilder, platform::macos::EventLoopBuilderExtMacOS};

pub(crate) fn configure_event_loop<T>(builder: &mut EventLoopBuilder<T>, no_activate: bool) {
    builder.with_activate_ignoring_other_apps(!no_activate);
}
