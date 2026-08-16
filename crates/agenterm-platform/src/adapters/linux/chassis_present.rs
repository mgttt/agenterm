//! Small X11 presenter for the validated Chassis-L1 image status.

use x11rb::{
    COPY_DEPTH_FROM_PARENT,
    connection::Connection as _,
    protocol::{
        Event,
        xproto::{
            AtomEnum, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, PropMode,
            Rectangle, WindowClass,
        },
    },
    wrapper::ConnectionExt as _,
};

use crate::contract::chassis_present::{ChassisPresentError, ChassisPresentOptions};

const WIDTH: u16 = 560;
const HEIGHT: u16 = 240;
const BACKGROUND: u32 = 0x0014_1b24;
const LOADED: u32 = 0x0036_b37e;

pub(crate) fn present(options: &ChassisPresentOptions) -> Result<(), ChassisPresentError> {
    if options.title.is_empty() || options.title.as_bytes().contains(&0) {
        return Err(failed(
            "chassis_present_invalid_title",
            "window title must be non-empty and contain no NUL byte",
        ));
    }
    let (connection, screen_index) =
        x11rb::connect(None).map_err(|error| failed("chassis_present_connect_failed", error))?;
    let screen = &connection.setup().roots[screen_index];
    let window = connection
        .generate_id()
        .map_err(|error| failed("chassis_present_window_id_failed", error))?;
    connection
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            screen.root,
            0,
            0,
            WIDTH,
            HEIGHT,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .background_pixel(BACKGROUND)
                .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY),
        )
        .map_err(|error| failed("chassis_present_create_failed", error))?
        .check()
        .map_err(|error| failed("chassis_present_create_rejected", error))?;

    let wm_protocols = intern(&connection, b"WM_PROTOCOLS")?;
    let wm_delete = intern(&connection, b"WM_DELETE_WINDOW")?;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            wm_protocols,
            AtomEnum::ATOM,
            &[wm_delete],
        )
        .map_err(|error| failed("chassis_present_protocol_failed", error))?
        .check()
        .map_err(|error| failed("chassis_present_protocol_rejected", error))?;
    connection
        .change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            options.title.as_bytes(),
        )
        .map_err(|error| failed("chassis_present_title_failed", error))?
        .check()
        .map_err(|error| failed("chassis_present_title_rejected", error))?;

    let gc = connection
        .generate_id()
        .map_err(|error| failed("chassis_present_gc_id_failed", error))?;
    connection
        .create_gc(gc, window, &CreateGCAux::new().foreground(LOADED))
        .map_err(|error| failed("chassis_present_gc_failed", error))?
        .check()
        .map_err(|error| failed("chassis_present_gc_rejected", error))?;
    connection
        .map_window(window)
        .map_err(|error| failed("chassis_present_map_failed", error))?
        .check()
        .map_err(|error| failed("chassis_present_map_rejected", error))?;
    connection
        .flush()
        .map_err(|error| failed("chassis_present_flush_failed", error))?;

    loop {
        match connection
            .wait_for_event()
            .map_err(|error| failed("chassis_present_event_failed", error))?
        {
            Event::Expose(_) => {
                let height = options.loaded_rows.clamp(1, 16).saturating_mul(6);
                connection
                    .poly_fill_rectangle(
                        window,
                        gc,
                        &[Rectangle {
                            x: 0,
                            y: 0,
                            width: WIDTH,
                            height,
                        }],
                    )
                    .map_err(|error| failed("chassis_present_draw_failed", error))?
                    .check()
                    .map_err(|error| failed("chassis_present_draw_rejected", error))?;
                connection
                    .flush()
                    .map_err(|error| failed("chassis_present_flush_failed", error))?;
            }
            Event::ClientMessage(event) if event.data.as_data32()[0] == wm_delete => return Ok(()),
            Event::DestroyNotify(_) => return Ok(()),
            _ => {}
        }
    }
}

fn intern(
    connection: &impl x11rb::connection::Connection,
    name: &[u8],
) -> Result<u32, ChassisPresentError> {
    connection
        .intern_atom(false, name)
        .map_err(|error| failed("chassis_present_atom_failed", error))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| failed("chassis_present_atom_reply_failed", error))
}

fn failed(code: &'static str, error: impl std::fmt::Display) -> ChassisPresentError {
    ChassisPresentError::failed(code, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_title_fails_before_native_connection() {
        let error = present(&ChassisPresentOptions::new("", 6)).expect_err("invalid title");
        assert_eq!(error.code, "chassis_present_invalid_title");
    }
}
