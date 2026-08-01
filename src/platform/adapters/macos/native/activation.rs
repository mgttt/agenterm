//! macOS application activation policy for foreground-safe automation.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "macos")]

use winit::event_loop::EventLoopBuilder;

pub(crate) fn configure_event_loop<T>(builder: &mut EventLoopBuilder<T>, no_activate: bool) {
    agenterm_platform::activation::configure_event_loop(builder, no_activate);
}
