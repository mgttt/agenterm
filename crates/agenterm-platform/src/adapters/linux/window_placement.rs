//! Linux X11 placement preflight.
//!
//! X11 owns numeric size hints and window-manager allowed actions. A stable
//! XID-to-AT-SPI top-level role join is not yet available in the platform
//! crate, so role remains explicitly `Unknown`; callers must not actuate it as
//! an ordinary frame. Pure Wayland is unsupported rather than routed through
//! an X11 or synthetic accessibility fallback.

use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _};

use crate::CapabilityStatus;
use crate::contract::window_placement::{
    PlacementRole, PlacementWindowInfo, SizeConstraints, Support, WindowPlacementError, WindowSize,
};

const P_MIN_SIZE: u32 = 1 << 4;
const P_MAX_SIZE: u32 = 1 << 5;
const P_RESIZE_INC: u32 = 1 << 6;

pub(crate) fn capability_status() -> CapabilityStatus {
    if pure_wayland() {
        CapabilityStatus::Unsupported {
            reason: "window placement inspection requires X11; Wayland is unsupported".into(),
        }
    } else if std::env::var_os("DISPLAY").is_some() {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: "window placement inspection requires DISPLAY".into(),
        }
    }
}

pub(crate) fn inspect(
    handle: isize,
    expected_pid: u32,
) -> Result<PlacementWindowInfo, WindowPlacementError> {
    if pure_wayland() {
        return Err(WindowPlacementError::Unsupported {
            reason: "window placement inspection requires X11; Wayland is unsupported".into(),
        });
    }
    if handle == 0 || expected_pid == 0 {
        return Err(failed(
            "window_identity_invalid",
            "XID and expected process id must be nonzero",
        ));
    }
    let window = u32::try_from(handle).map_err(|_| {
        failed(
            "window_identity_invalid",
            "window handle is not a valid XID",
        )
    })?;
    let (connection, _) = x11rb::connect(None).map_err(|error| {
        failed(
            "window_inspect_failed",
            format!("X11 display could not be opened: {error}"),
        )
    })?;

    // GetGeometry is the liveness check. A recycled or destroyed XID must fail
    // before any metadata is trusted.
    connection
        .get_geometry(window)
        .map_err(|error| failed("window_stale", format!("GetGeometry send failed: {error}")))?
        .reply()
        .map_err(|error| failed("window_stale", format!("GetGeometry failed: {error}")))?;

    let pid_atom = intern(&connection, b"_NET_WM_PID")?;
    let actual_pid = property_u32s(&connection, window, pid_atom, AtomEnum::CARDINAL.into(), 1)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            failed(
                "window_identity_unknown",
                "X11 window has no _NET_WM_PID identity",
            )
        })?;
    if actual_pid != expected_pid {
        return Err(failed(
            "window_stale",
            format!("XID belongs to process {actual_pid}, expected {expected_pid}"),
        ));
    }

    let (movable, resizable) = allowed_actions(&connection, window)?;
    let constraints = size_constraints(&connection, window)?;
    Ok(PlacementWindowInfo {
        handle,
        process_id: actual_pid,
        // Do not reuse the synthetic frame produced by accessibility-tree's
        // X11 fallback. It is geometry evidence, not a real AT-SPI role.
        role: PlacementRole::Unknown,
        movable,
        resizable,
        constraints,
    })
}

fn pure_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland")
        && std::env::var_os("DISPLAY").is_none()
}

fn intern(
    connection: &x11rb::rust_connection::RustConnection,
    name: &[u8],
) -> Result<Atom, WindowPlacementError> {
    connection
        .intern_atom(false, name)
        .map_err(|error| {
            failed(
                "window_inspect_failed",
                format!("InternAtom send failed: {error}"),
            )
        })?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| {
            failed(
                "window_inspect_failed",
                format!("InternAtom failed: {error}"),
            )
        })
}

fn property_u32s(
    connection: &x11rb::rust_connection::RustConnection,
    window: u32,
    property: Atom,
    property_type: Atom,
    maximum_items: u32,
) -> Result<Vec<u32>, WindowPlacementError> {
    let reply = connection
        .get_property(false, window, property, property_type, 0, maximum_items)
        .map_err(|error| {
            failed(
                "window_inspect_failed",
                format!("GetProperty send failed: {error}"),
            )
        })?
        .reply()
        .map_err(|error| {
            failed(
                "window_inspect_failed",
                format!("GetProperty failed: {error}"),
            )
        })?;
    if reply.format == 0 {
        return Ok(Vec::new());
    }
    if reply.format != 32 {
        return Err(failed(
            "window_metadata_invalid",
            format!("X11 property has format {}, expected 32", reply.format),
        ));
    }
    reply
        .value32()
        .map(|values| values.collect())
        .ok_or_else(|| {
            failed(
                "window_metadata_invalid",
                "X11 property is not a 32-bit array",
            )
        })
}

fn allowed_actions(
    connection: &x11rb::rust_connection::RustConnection,
    window: u32,
) -> Result<(Support, Support), WindowPlacementError> {
    let property = intern(connection, b"_NET_WM_ALLOWED_ACTIONS")?;
    let move_atom = intern(connection, b"_NET_WM_ACTION_MOVE")?;
    let resize_atom = intern(connection, b"_NET_WM_ACTION_RESIZE")?;
    let actions = property_u32s(connection, window, property, AtomEnum::ATOM.into(), 256)?;
    if actions.is_empty() {
        return Ok((Support::Unknown, Support::Unknown));
    }
    Ok((
        if actions.contains(&move_atom) {
            Support::Yes
        } else {
            Support::No
        },
        if actions.contains(&resize_atom) {
            Support::Yes
        } else {
            Support::No
        },
    ))
}

fn size_constraints(
    connection: &x11rb::rust_connection::RustConnection,
    window: u32,
) -> Result<SizeConstraints, WindowPlacementError> {
    let property = intern(connection, b"WM_NORMAL_HINTS")?;
    let property_type = intern(connection, b"WM_SIZE_HINTS")?;
    let values = property_u32s(connection, window, property, property_type, 18)?;
    parse_size_hints(&values)
}

fn parse_size_hints(values: &[u32]) -> Result<SizeConstraints, WindowPlacementError> {
    if values.is_empty() {
        return Ok(SizeConstraints::Explicit {
            min: None,
            max: None,
            increment: None,
        });
    }
    if values.len() < 11 {
        return Err(failed(
            "window_constraints_invalid",
            format!(
                "WM_NORMAL_HINTS has {} fields, expected at least 11",
                values.len()
            ),
        ));
    }
    let flags = values[0];
    let size = |flag, width_index, height_index| {
        (flags & flag != 0).then(|| WindowSize::new(values[width_index], values[height_index]))
    };
    let constraints = SizeConstraints::Explicit {
        min: size(P_MIN_SIZE, 5, 6),
        max: size(P_MAX_SIZE, 7, 8),
        increment: size(P_RESIZE_INC, 9, 10),
    };
    constraints.validate()?;
    Ok(constraints)
}

fn failed(code: &'static str, message: impl ToString) -> WindowPlacementError {
    WindowPlacementError::failed(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_icccm_min_max_and_increment_without_conflating_missing() {
        let mut hints = [0u32; 18];
        hints[0] = P_MIN_SIZE | P_MAX_SIZE | P_RESIZE_INC;
        hints[5] = 320;
        hints[6] = 240;
        hints[7] = 1920;
        hints[8] = 1080;
        hints[9] = 8;
        hints[10] = 16;
        assert_eq!(
            parse_size_hints(&hints),
            Ok(SizeConstraints::Explicit {
                min: Some(WindowSize::new(320, 240)),
                max: Some(WindowSize::new(1920, 1080)),
                increment: Some(WindowSize::new(8, 16)),
            })
        );
        assert_eq!(
            parse_size_hints(&[]),
            Ok(SizeConstraints::Explicit {
                min: None,
                max: None,
                increment: None,
            })
        );
    }

    #[test]
    fn malformed_or_zero_icccm_limits_fail_closed() {
        assert!(parse_size_hints(&[P_MIN_SIZE]).is_err());
        let mut hints = [0u32; 18];
        hints[0] = P_MIN_SIZE;
        assert!(parse_size_hints(&hints).is_err());
    }
}
