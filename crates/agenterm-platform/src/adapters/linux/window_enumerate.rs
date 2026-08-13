//! Linux X11 top-level window enumeration.

use std::{collections::HashSet, env};

use x11rb::{
    NONE,
    connection::Connection,
    protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, MapState, Window},
    rust_connection::RustConnection,
};

use crate::CapabilityStatus;
use crate::contract::window_enumerate::{WindowBounds, WindowEnumerateError, WindowInfo};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionKind {
    X11,
    Wayland,
    Unavailable,
}

fn classify_session(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    x11_display: Option<&str>,
) -> SessionKind {
    if session_type == Some("wayland") || wayland_display.is_some_and(|value| !value.is_empty()) {
        SessionKind::Wayland
    } else if session_type == Some("x11") || x11_display.is_some_and(|value| !value.is_empty()) {
        SessionKind::X11
    } else {
        SessionKind::Unavailable
    }
}

fn session_kind() -> SessionKind {
    classify_session(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var("WAYLAND_DISPLAY").ok().as_deref(),
        env::var("DISPLAY").ok().as_deref(),
    )
}

struct Atoms {
    client_list: Atom,
    client_list_stacking: Atom,
    wm_pid: Atom,
    wm_name: Atom,
    utf8_string: Atom,
    active_window: Atom,
}

struct Context {
    connection: RustConnection,
    root: Window,
    atoms: Atoms,
}

fn failed(message: impl ToString) -> WindowEnumerateError {
    WindowEnumerateError::failed("window_enum_failed", message)
}

fn atom(connection: &RustConnection, name: &[u8]) -> Result<Atom, WindowEnumerateError> {
    connection
        .intern_atom(false, name)
        .map_err(|_| failed("an X11 atom request could not be sent"))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|_| failed("an X11 atom request failed"))
}

fn connect() -> Result<Context, WindowEnumerateError> {
    match session_kind() {
        SessionKind::X11 => {}
        SessionKind::Wayland => {
            return Err(WindowEnumerateError::Unsupported {
                reason: "window-enum requires X11; Wayland has no client-list enumeration".into(),
            });
        }
        SessionKind::Unavailable => {
            return Err(WindowEnumerateError::Unsupported {
                reason: "window-enum requires an X11 display".into(),
            });
        }
    }
    let (connection, screen) = x11rb::connect(None)
        .map_err(|error| failed(format!("X11 display could not be opened: {error}")))?;
    let root = connection
        .setup()
        .roots
        .get(screen)
        .ok_or_else(|| failed("configured X11 screen does not exist"))?
        .root;
    let atoms = Atoms {
        client_list: atom(&connection, b"_NET_CLIENT_LIST")?,
        client_list_stacking: atom(&connection, b"_NET_CLIENT_LIST_STACKING")?,
        wm_pid: atom(&connection, b"_NET_WM_PID")?,
        wm_name: atom(&connection, b"_NET_WM_NAME")?,
        utf8_string: atom(&connection, b"UTF8_STRING")?,
        active_window: atom(&connection, b"_NET_ACTIVE_WINDOW")?,
    };
    Ok(Context {
        connection,
        root,
        atoms,
    })
}

fn windows_property(
    context: &Context,
    property: Atom,
) -> Result<Vec<Window>, WindowEnumerateError> {
    let reply = context
        .connection
        .get_property(false, context.root, property, AtomEnum::WINDOW, 0, u32::MAX)
        .map_err(|_| failed("X11 client-list request could not be sent"))?
        .reply()
        .map_err(|_| failed("X11 client-list request failed"))?;
    if reply.format != 32 || reply.type_ != u32::from(AtomEnum::WINDOW) {
        return Ok(Vec::new());
    }
    Ok(reply.value32().into_iter().flatten().collect())
}

fn client_windows(context: &Context) -> Result<Vec<Window>, WindowEnumerateError> {
    let stacking = windows_property(context, context.atoms.client_list_stacking)?;
    let candidates = if stacking.is_empty() {
        windows_property(context, context.atoms.client_list)?
    } else {
        stacking
    };
    let mut seen = HashSet::new();
    Ok(candidates
        .into_iter()
        .filter(|window| seen.insert(*window))
        .collect())
}

fn process_id(context: &Context, window: Window) -> Result<Option<u32>, WindowEnumerateError> {
    let reply = context
        .connection
        .get_property(
            false,
            window,
            context.atoms.wm_pid,
            AtomEnum::CARDINAL,
            0,
            1,
        )
        .map_err(|_| failed("X11 process-owner request could not be sent"))?
        .reply()
        .map_err(|_| failed("X11 process-owner request failed"))?;
    if reply.format != 32 || reply.type_ != u32::from(AtomEnum::CARDINAL) {
        return Ok(None);
    }
    Ok(reply.value32().and_then(|mut values| values.next()))
}

fn map_state(context: &Context, window: Window) -> Result<MapState, WindowEnumerateError> {
    context
        .connection
        .get_window_attributes(window)
        .map_err(|_| failed("X11 window-state request could not be sent"))?
        .reply()
        .map(|reply| reply.map_state)
        .map_err(|_| failed("X11 window-state request failed"))
}

fn title(context: &Context, window: Window) -> Result<String, WindowEnumerateError> {
    let modern = context
        .connection
        .get_property(
            false,
            window,
            context.atoms.wm_name,
            context.atoms.utf8_string,
            0,
            16_384,
        )
        .map_err(|_| failed("X11 window-title request could not be sent"))?
        .reply()
        .map_err(|_| failed("X11 window-title request failed"))?;
    if modern.format == 8 && modern.type_ == context.atoms.utf8_string {
        let title = String::from_utf8_lossy(&modern.value).into_owned();
        if !title.is_empty() {
            return Ok(title);
        }
    }
    let legacy = context
        .connection
        .get_property(
            false,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            0,
            16_384,
        )
        .map_err(|_| failed("legacy X11 window-title request could not be sent"))?
        .reply()
        .map_err(|_| failed("legacy X11 window-title request failed"))?;
    Ok(if legacy.format == 8 {
        String::from_utf8_lossy(&legacy.value).into_owned()
    } else {
        String::new()
    })
}

fn active_window(context: &Context) -> Result<Window, WindowEnumerateError> {
    let reply = context
        .connection
        .get_property(
            false,
            context.root,
            context.atoms.active_window,
            AtomEnum::WINDOW,
            0,
            1,
        )
        .map_err(|_| failed("X11 active-window request could not be sent"))?
        .reply()
        .map_err(|_| failed("X11 active-window request failed"))?;
    Ok(reply
        .value32()
        .and_then(|mut values| values.next())
        .unwrap_or(NONE))
}

fn geometry(
    context: &Context,
    window: Window,
) -> Result<WindowBounds, WindowEnumerateError> {
    let geometry = context
        .connection
        .get_geometry(window)
        .map_err(|_| failed("X11 geometry request could not be sent"))?
        .reply()
        .map_err(|_| failed("X11 geometry request failed"))?;
    let translated = context
        .connection
        .translate_coordinates(window, context.root, 0, 0)
        .map_err(|_| failed("X11 coordinate request could not be sent"))?
        .reply()
        .map_err(|_| failed("X11 coordinate request failed"))?;
    if !translated.same_screen {
        return Err(failed("X11 window is not on the selected root screen"));
    }
    Ok(WindowBounds {
        x: i32::from(translated.dst_x),
        y: i32::from(translated.dst_y),
        width: u32::from(geometry.width),
        height: u32::from(geometry.height),
    })
}

fn process_name(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    let path = format!("/proc/{pid}/comm");
    std::fs::read_to_string(path)
        .map(|name| name.trim().to_owned())
        .unwrap_or_default()
}

pub(crate) fn capability_status() -> CapabilityStatus {
    match session_kind() {
        SessionKind::X11 => CapabilityStatus::Available,
        SessionKind::Wayland => CapabilityStatus::Unsupported {
            reason: "window-enum requires X11".into(),
        },
        SessionKind::Unavailable => CapabilityStatus::Unsupported {
            reason: "window-enum requires DISPLAY".into(),
        },
    }
}

pub(crate) fn enumerate_top_level() -> Result<Vec<WindowInfo>, WindowEnumerateError> {
    let context = connect()?;
    let foreground = active_window(&context)?;
    let mut out = Vec::new();
    for window in client_windows(&context)? {
        let state = map_state(&context, window)?;
        if state != MapState::VIEWABLE {
            continue;
        }
        let title = title(&context, window).unwrap_or_default();
        if title.is_empty() && state != MapState::VIEWABLE {
            continue;
        }
        let pid = process_id(&context, window)?.unwrap_or(0);
        let bounds = geometry(&context, window).unwrap_or(WindowBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
        out.push(WindowInfo {
            handle: window as isize,
            title,
            process_id: pid,
            app_name: process_name(pid),
            bounds,
            focused: window == foreground,
            minimized: false,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_takes_precedence_over_xwayland_display() {
        assert_eq!(
            classify_session(Some("wayland"), Some("wayland-0"), Some(":0")),
            SessionKind::Wayland
        );
        assert_eq!(
            classify_session(Some("x11"), None, Some(":0")),
            SessionKind::X11
        );
    }
}
