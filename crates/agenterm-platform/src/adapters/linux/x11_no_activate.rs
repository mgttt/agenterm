//! X11 initial-map focus suppression for native product windows.

use winit::{
    event_loop::ActiveEventLoop,
    platform::x11::ActiveEventLoopExtX11 as _,
    raw_window_handle::{HasWindowHandle as _, RawWindowHandle},
    window::{Window, WindowAttributes},
};
use x11rb::{
    connection::Connection as _,
    protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode},
    wrapper::ConnectionExt as _,
};

pub(super) fn prepare_window(attributes: WindowAttributes, no_activate: bool) -> WindowAttributes {
    // Winit documents WindowAttributes::active as unsupported on X11 and
    // Wayland. Keep an X11 no-activate window unmapped until its EWMH user-time
    // marker is installed; otherwise the window manager may focus it on map.
    attributes
        .with_active(!no_activate)
        .with_visible(!no_activate)
}

pub(super) fn reveal_window(
    event_loop: &ActiveEventLoop,
    window: &Window,
    no_activate: bool,
) -> Result<(), String> {
    if !no_activate {
        return Ok(());
    }
    if event_loop.is_x11() {
        mark_x11_window_as_not_user_initiated(window)?;
    }
    window.set_visible(true);
    Ok(())
}

fn mark_x11_window_as_not_user_initiated(window: &Window) -> Result<(), String> {
    let handle = window
        .window_handle()
        .map_err(|error| format!("native X11 window handle unavailable: {error}"))?;
    let window_id = match handle.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window)
            .map_err(|_| "native Xlib window identity exceeds u32".to_owned())?,
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        _ => return Err("active Linux display is X11 but window handle is not X11".to_owned()),
    };
    let (connection, _) = x11rb::connect(None)
        .map_err(|error| format!("X11 no-activate connection failed: {error}"))?;
    let user_time = connection
        .intern_atom(false, b"_NET_WM_USER_TIME")
        .map_err(|error| format!("X11 user-time atom request failed: {error}"))?
        .reply()
        .map_err(|error| format!("X11 user-time atom reply failed: {error}"))?
        .atom;
    connection
        .change_property32(
            PropMode::REPLACE,
            window_id,
            user_time,
            AtomEnum::CARDINAL,
            &[0],
        )
        .map_err(|error| format!("X11 user-time update failed: {error}"))?
        .check()
        .map_err(|error| format!("X11 user-time update was rejected: {error}"))?;
    connection
        .flush()
        .map_err(|error| format!("X11 user-time flush failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_activate_window_stays_hidden_until_x11_metadata_is_installed() {
        let attributes = prepare_window(WindowAttributes::default(), true);
        assert!(!attributes.visible);
        assert!(!attributes.active);
    }

    #[test]
    fn ordinary_window_keeps_normal_initial_visibility_and_activation() {
        let attributes = prepare_window(WindowAttributes::default(), false);
        assert!(attributes.visible);
        assert!(attributes.active);
    }
}
