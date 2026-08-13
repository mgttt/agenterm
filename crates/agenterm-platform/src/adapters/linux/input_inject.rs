//! Linux X11 input injection through the XTest extension.

use std::env;

use x11rb::{
    CURRENT_TIME,
    connection::{Connection, RequestConnection},
    errors::ConnectionError,
    protocol::xproto::{
        BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, KEY_PRESS_EVENT,
        KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
    },
    protocol::xtest::{ConnectionExt as _, X11_EXTENSION_NAME},
    rust_connection::RustConnection,
};

use crate::CapabilityStatus;
use crate::contract::input_inject::{InputInjectError, PointerButton, PointerPosition};

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

struct Context {
    connection: RustConnection,
    root: u32,
}

fn failed(message: impl ToString) -> InputInjectError {
    InputInjectError::failed("input_inject_failed", message)
}

fn connect() -> Result<Context, InputInjectError> {
    match session_kind() {
        SessionKind::X11 => {}
        SessionKind::Wayland => {
            return Err(InputInjectError::Unsupported {
                reason: "input-inject requires X11; Wayland has no XTest injection".into(),
            });
        }
        SessionKind::Unavailable => {
            return Err(InputInjectError::Unsupported {
                reason: "input-inject requires an X11 display".into(),
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
    Ok(Context { connection, root })
}

fn require_xtest(context: &Context) -> Result<(), InputInjectError> {
    match context.connection.extension_information(X11_EXTENSION_NAME) {
        Ok(Some(_)) => Ok(()),
        Ok(None) | Err(ConnectionError::UnsupportedExtension) => Err(InputInjectError::Unsupported {
            reason: "X11 server does not provide the XTest extension".into(),
        }),
        Err(error) => Err(failed(format!("XTest extension query failed: {error}"))),
    }
}

fn xtest_input(
    context: &Context,
    event_type: u8,
    detail: u8,
    root_x: i16,
    root_y: i16,
) -> Result<(), InputInjectError> {
    require_xtest(context)?;
    context
        .connection
        .xtest_fake_input(
            event_type,
            detail,
            CURRENT_TIME,
            context.root,
            root_x,
            root_y,
            0,
        )
        .map_err(|_| failed("XTest input request could not be sent"))?
        .check()
        .map_err(|_| failed("X11 server rejected XTest input"))?;
    context
        .connection
        .flush()
        .map_err(|_| failed("XTest input request could not be flushed"))
}

fn button_detail(button: PointerButton) -> u8 {
    match button {
        PointerButton::Left => 1,
        PointerButton::Right => 3,
        PointerButton::Middle => 2,
    }
}

fn keysym_for_token(token: &str) -> Option<u32> {
    match token.to_ascii_lowercase().as_str() {
        "backspace" => Some(0xff08),
        "tab" => Some(0xff09),
        "enter" | "return" => Some(0xff0d),
        "escape" | "esc" => Some(0xff1b),
        "space" => Some(0x0020),
        "home" => Some(0xff50),
        "left" => Some(0xff51),
        "up" => Some(0xff52),
        "right" => Some(0xff53),
        "down" => Some(0xff54),
        "delete" | "del" => Some(0xffff),
        "f1" => Some(0xffbe),
        "f2" => Some(0xffbf),
        "f3" => Some(0xffc0),
        "f4" => Some(0xffc1),
        "f5" => Some(0xffc2),
        "f6" => Some(0xffc3),
        "f7" => Some(0xffc4),
        "f8" => Some(0xffc5),
        "f9" => Some(0xffc6),
        "f10" => Some(0xffc7),
        "f11" => Some(0xffc8),
        "f12" => Some(0xffc9),
        _ => token.chars().next().map(|ch| u32::from(ch as u8)),
    }
}

fn modifier_keysym(token: &str) -> Option<u32> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(0xffe3),
        "shift" => Some(0xffe1),
        "alt" => Some(0xffe9),
        "meta" | "super" | "win" => Some(0xffeb),
        _ => None,
    }
}

fn keycode(context: &Context, requested: u32) -> Result<u8, InputInjectError> {
    let setup = context.connection.setup();
    let first = setup.min_keycode;
    let count = setup
        .max_keycode
        .checked_sub(first)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| failed("X11 keyboard mapping is invalid"))?;
    let reply = context
        .connection
        .get_keyboard_mapping(first, count)
        .map_err(|_| failed("X11 keyboard-map request could not be sent"))?
        .reply()
        .map_err(|_| failed("X11 keyboard-map request failed"))?;
    let per_keycode = usize::from(reply.keysyms_per_keycode);
    if per_keycode == 0 {
        return Err(failed("X11 keyboard mapping is empty"));
    }
    reply
        .keysyms
        .chunks(per_keycode)
        .position(|symbols| symbols.contains(&requested))
        .and_then(|offset| u8::try_from(offset).ok())
        .and_then(|offset| first.checked_add(offset))
        .ok_or_else(|| failed(format!("requested key has no X11 keycode for keysym {requested:#x}")))
}

fn press_key(context: &Context, keycode: u8) -> Result<(), InputInjectError> {
    xtest_input(context, KEY_PRESS_EVENT, keycode, 0, 0)
}

fn release_key(context: &Context, keycode: u8) -> Result<(), InputInjectError> {
    xtest_input(context, KEY_RELEASE_EVENT, keycode, 0, 0)
}

pub(crate) fn capability_status() -> CapabilityStatus {
    match session_kind() {
        SessionKind::X11 => CapabilityStatus::Available,
        SessionKind::Wayland => CapabilityStatus::Unsupported {
            reason: "input-inject requires X11".into(),
        },
        SessionKind::Unavailable => CapabilityStatus::Unsupported {
            reason: "input-inject requires DISPLAY".into(),
        },
    }
}

pub(crate) fn pointer_move(position: PointerPosition) -> Result<(), InputInjectError> {
    let context = connect()?;
    let x = i16::try_from(position.x)
        .map_err(|_| failed("pointer x coordinate is outside the X11 range"))?;
    let y = i16::try_from(position.y)
        .map_err(|_| failed("pointer y coordinate is outside the X11 range"))?;
    xtest_input(&context, MOTION_NOTIFY_EVENT, 0, x, y)
}

pub(crate) fn pointer_click(
    position: PointerPosition,
    button: PointerButton,
    clicks: u32,
) -> Result<(), InputInjectError> {
    let context = connect()?;
    let x = i16::try_from(position.x)
        .map_err(|_| failed("pointer x coordinate is outside the X11 range"))?;
    let y = i16::try_from(position.y)
        .map_err(|_| failed("pointer y coordinate is outside the X11 range"))?;
    let detail = button_detail(button);
    for _ in 0..clicks.max(1) {
        xtest_input(&context, MOTION_NOTIFY_EVENT, 0, x, y)?;
        xtest_input(&context, BUTTON_PRESS_EVENT, detail, 0, 0)?;
        xtest_input(&context, BUTTON_RELEASE_EVENT, detail, 0, 0)?;
    }
    Ok(())
}

pub(crate) fn type_text(text: &str) -> Result<(), InputInjectError> {
    let context = connect()?;
    for ch in text.chars() {
        let keysym = u32::from(ch as u8);
        let keycode = keycode(&context, keysym)?;
        press_key(&context, keycode)?;
        release_key(&context, keycode)?;
    }
    Ok(())
}

pub(crate) fn send_keys(shortcut: &str) -> Result<(), InputInjectError> {
    let context = connect()?;
    let parts: Vec<&str> = shortcut.split('+').map(str::trim).collect();
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(failed(format!("cannot parse shortcut '{shortcut}'")));
    }
    let mut modifier_codes = Vec::new();
    for part in &parts[..parts.len() - 1] {
        let keysym = modifier_keysym(part)
            .ok_or_else(|| failed(format!("unknown modifier '{part}' in shortcut '{shortcut}'")))?;
        modifier_codes.push(keycode(&context, keysym)?);
    }
    let final_token = parts[parts.len() - 1];
    let final_keysym = keysym_for_token(final_token)
        .ok_or_else(|| failed(format!("unknown key '{final_token}' in shortcut '{shortcut}'")))?;
    let final_code = keycode(&context, final_keysym)?;
    for code in &modifier_codes {
        press_key(&context, *code)?;
    }
    press_key(&context, final_code)?;
    release_key(&context, final_code)?;
    for code in modifier_codes.iter().rev() {
        release_key(&context, *code)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_tokens_map_to_expected_keysyms() {
        assert_eq!(keysym_for_token("enter"), Some(0xff0d));
        assert_eq!(modifier_keysym("ctrl"), Some(0xffe3));
        assert_eq!(keysym_for_token("a"), Some(b'a'.into()));
    }
}
