//! Linux X11 ConfigureWindow for foreign top-level windows.

use x11rb::{
    connection::Connection,
    protocol::xproto::{ConfigureWindowAux, ConnectionExt as _},
};

use crate::CapabilityStatus;
use crate::contract::window_enumerate::WindowBounds;
use crate::contract::window_op::WindowOpError;

fn failed(message: impl ToString) -> WindowOpError {
    WindowOpError::failed("window_op_failed", message)
}

fn connect() -> Result<x11rb::rust_connection::RustConnection, WindowOpError> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland")
        && std::env::var_os("DISPLAY").is_none()
    {
        return Err(WindowOpError::Unsupported {
            reason: "window-op requires X11; Wayland is unsupported".into(),
        });
    }
    x11rb::connect(None)
        .map(|(conn, _)| conn)
        .map_err(|error| failed(format!("X11 display could not be opened: {error}")))
}

pub(crate) fn capability_status() -> CapabilityStatus {
    if std::env::var_os("DISPLAY").is_some() {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: "window-op requires DISPLAY".into(),
        }
    }
}

pub(crate) fn show(
    _handle: isize,
    _state: crate::contract::window_op::WindowShowState,
) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window show-state is not wired on Linux yet".into(),
    })
}

pub(crate) fn move_window(
    handle: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), WindowOpError> {
    let conn = connect()?;
    let window = u32::try_from(handle).map_err(|_| failed("window handle is not a valid XID"))?;
    let aux = ConfigureWindowAux::new()
        .x(x)
        .y(y)
        .width(width.max(1))
        .height(height.max(1));
    conn.configure_window(window, &aux)
        .map_err(|error| failed(format!("ConfigureWindow send failed: {error}")))?;
    conn.flush()
        .map_err(|error| failed(format!("ConfigureWindow flush failed: {error}")))?;
    Ok(())
}

pub(crate) fn window_rect(handle: isize) -> Result<WindowBounds, WindowOpError> {
    let conn = connect()?;
    let window = u32::try_from(handle).map_err(|_| failed("window handle is not a valid XID"))?;
    let geom = conn
        .get_geometry(window)
        .map_err(|error| failed(format!("GetGeometry send failed: {error}")))?
        .reply()
        .map_err(|error| failed(format!("GetGeometry failed: {error}")))?;
    Ok(WindowBounds {
        x: i32::from(geom.x),
        y: i32::from(geom.y),
        width: u32::from(geom.width),
        height: u32::from(geom.height),
    })
}

pub(crate) fn set_topmost(_handle: isize, _topmost: bool) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window topmost is not wired on Linux yet".into(),
    })
}

pub(crate) fn close(_handle: isize) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window close is not wired on Linux yet".into(),
    })
}
