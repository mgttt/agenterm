//! Windows input injection (user32 FFI): pointer + Unicode keyboard.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, SendInput, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_F1,
    VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_F11, VK_F12, VK_LEFT,
    VK_LWIN, VK_MENU, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, mouse_event,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;

use crate::CapabilityStatus;
use crate::contract::input_inject::{InputInjectError, PointerButton, PointerPosition};

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn pointer_move(position: PointerPosition) -> Result<(), InputInjectError> {
    unsafe {
        if SetCursorPos(position.x, position.y) == 0 {
            return Err(InputInjectError::failed(
                "set_cursor_failed",
                "SetCursorPos returned 0",
            ));
        }
    }
    Ok(())
}

pub(crate) fn pointer_click(
    position: PointerPosition,
    button: PointerButton,
    clicks: u32,
) -> Result<(), InputInjectError> {
    let flags = match button {
        PointerButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        PointerButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        PointerButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    };
    unsafe {
        if SetCursorPos(position.x, position.y) == 0 {
            return Err(InputInjectError::failed(
                "set_cursor_failed",
                "SetCursorPos returned 0",
            ));
        }
        for _ in 0..clicks.max(1) {
            mouse_event(flags.0, 0, 0, 0, 0);
            mouse_event(flags.1, 0, 0, 0, 0);
        }
    }
    Ok(())
}

pub(crate) fn type_text(text: &str) -> Result<(), InputInjectError> {
    let inputs = type_text_inputs(text);
    send_batch(&inputs)
}

/// Build the keyboard INPUT sequence for `text` as Unicode events
/// (KEYEVENTF_UNICODE via `wScan`). A VK-mode down/up would treat each
/// character code as a *virtual key*: the lowercase letter codes 0x61..0x7A
/// collide with the VK_NUMPAD1..VK_NUMPAD9 / VK_DECIMAL range, so
/// TranslateMessage would map them through the keyboard layout to the wrong
/// WM_CHAR (e.g. 'a' -> '1'). Unicode mode delivers the character verbatim;
/// non-BMP characters are expanded to their UTF-16 surrogate pair, one
/// key-down/key-up pair per code unit.
fn type_text_inputs(text: &str) -> Vec<INPUT> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.chars().count() * 2);
    for ch in text.chars() {
        let mut buf = [0u16; 2];
        let units = ch.encode_utf16(&mut buf);
        for &unit in &*units {
            inputs.push(key_input(unit, KEYEVENTF_UNICODE));
            inputs.push(key_input(unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
    }
    inputs
}

pub(crate) fn send_keys(shortcut: &str) -> Result<(), InputInjectError> {
    // Parse "ctrl+alt+key" style shortcuts; modifiers map to VK, the final key
    // may be a named key or a single character sent as Unicode.
    let parts: Vec<&str> = shortcut.split('+').map(|s| s.trim()).collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return Err(InputInjectError::failed(
            "bad_shortcut",
            format!("cannot parse shortcut '{shortcut}'"),
        ));
    }
    let mut down: Vec<INPUT> = Vec::new();
    let mut up: Vec<INPUT> = Vec::new();

    for part in &parts[..parts.len() - 1] {
        let vk = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => VK_CONTROL,
            "shift" => VK_SHIFT,
            "alt" => VK_MENU,
            "win" => VK_LWIN,
            _ => {
                return Err(InputInjectError::failed(
                    "unknown_modifier",
                    format!("unknown modifier '{part}'"),
                ));
            }
        };
        down.push(key_input(vk, 0));
        up.push(key_input(vk, KEYEVENTF_KEYUP));
    }

    let key = parts[parts.len() - 1];
    let lower = key.to_ascii_lowercase();
    match named_vk(&lower) {
        Some(vk) => {
            down.push(key_input(vk, 0));
            up.push(key_input(vk, KEYEVENTF_KEYUP));
        }
        None => {
            let chars: Vec<char> = key.chars().collect();
            if chars.len() != 1 {
                return Err(InputInjectError::failed(
                    "unknown_key",
                    format!("unknown key '{key}'"),
                ));
            }
            let code = virtual_key_for_character(chars[0]).ok_or_else(|| {
                InputInjectError::failed(
                    "unknown_key",
                    format!(
                        "character '{}' has no layout-independent physical key; use type_text for Unicode text",
                        chars[0]
                    ),
                )
            })?;
            down.push(key_input(code, 0));
            up.push(key_input(code, KEYEVENTF_KEYUP));
        }
    }

    let mut all = down;
    all.extend(up);
    send_batch(&all)
}

fn virtual_key_for_character(character: char) -> Option<u16> {
    if character.is_ascii_alphabetic() {
        Some(character.to_ascii_uppercase() as u16)
    } else if character.is_ascii_digit() {
        Some(character as u16)
    } else {
        None
    }
}

fn named_vk(lower: &str) -> Option<u16> {
    match lower {
        "enter" | "return" => Some(VK_RETURN),
        "tab" => Some(VK_TAB),
        "esc" | "escape" => Some(VK_ESCAPE),
        "space" => Some(VK_SPACE),
        "backspace" => Some(VK_BACK),
        "delete" => Some(VK_DELETE),
        "up" => Some(VK_UP),
        "down" => Some(VK_DOWN),
        "left" => Some(VK_LEFT),
        "right" => Some(VK_RIGHT),
        "f1" => Some(VK_F1),
        "f2" => Some(VK_F2),
        "f3" => Some(VK_F3),
        "f4" => Some(VK_F4),
        "f5" => Some(VK_F5),
        "f6" => Some(VK_F6),
        "f7" => Some(VK_F7),
        "f8" => Some(VK_F8),
        "f9" => Some(VK_F9),
        "f10" => Some(VK_F10),
        "f11" => Some(VK_F11),
        "f12" => Some(VK_F12),
        _ => None,
    }
}

/// Build a keyboard INPUT; `flags` includes KEYEVENTF_KEYUP when releasing.
///
/// Unicode mode (KEYEVENTF_UNICODE) delivers arbitrary characters via `wScan`;
/// VK mode delivers named keys via `wVk`. The two modes are mutually exclusive.
fn key_input(wvk_or_scan: u16, flags: u32) -> INPUT {
    let unicode = flags & KEYEVENTF_UNICODE != 0;
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: if unicode { 0 } else { wvk_or_scan },
        wScan: if unicode { wvk_or_scan } else { 0 },
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };
    input
}

fn send_batch(inputs: &[INPUT]) -> Result<(), InputInjectError> {
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(InputInjectError::failed(
            "send_input_partial",
            format!("SendInput sent {sent}/{} inputs", inputs.len()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, type_text_inputs, virtual_key_for_character};

    #[test]
    fn physical_ascii_keys_map_to_windows_virtual_keys() {
        assert_eq!(virtual_key_for_character('n'), Some(u16::from(b'N')));
        assert_eq!(virtual_key_for_character('N'), Some(u16::from(b'N')));
        assert_eq!(virtual_key_for_character('5'), Some(u16::from(b'5')));
        assert_eq!(virtual_key_for_character('?'), None);
        assert_eq!(virtual_key_for_character('中'), None);
    }

    /// Milestone 72 regression: `type_text` must inject Unicode events
    /// (wVk = 0, wScan = code unit, KEYEVENTF_UNICODE). Sending the raw
    /// character code as wVk maps lowercase letters onto the VK_NUMPAD1..9 /
    /// VK_DECIMAL range and TranslateMessage turns 'a' into '1'.
    #[test]
    fn type_text_injects_unicode_events_never_vk_events() {
        let inputs = type_text_inputs("a");
        assert_eq!(inputs.len(), 2, "one down + one up per character");
        assert_eq!(inputs[0].r#type, super::INPUT_KEYBOARD);
        assert_eq!(inputs[1].r#type, super::INPUT_KEYBOARD);
        let down = unsafe { inputs[0].Anonymous.ki };
        let up = unsafe { inputs[1].Anonymous.ki };
        assert_eq!(down.wVk, 0, "Unicode mode must not carry a virtual key");
        assert_eq!(down.wScan, u16::from(b'a'), "the code unit rides in wScan");
        assert_eq!(down.dwFlags & KEYEVENTF_UNICODE, KEYEVENTF_UNICODE);
        assert_eq!(up.dwFlags & KEYEVENTF_UNICODE, KEYEVENTF_UNICODE);
        assert_eq!(up.dwFlags & KEYEVENTF_KEYUP, KEYEVENTF_KEYUP);
    }

    /// A non-BMP character must be expanded to its UTF-16 surrogate pair,
    /// not truncated by a plain `char as u16` cast.
    #[test]
    fn type_text_expands_surrogate_pairs() {
        let inputs = type_text_inputs("\u{1F600}"); // U+1F600 GRINNING FACE
        assert_eq!(inputs.len(), 4, "two code units x down+up");
        let mut scans: Vec<u16> = inputs
            .iter()
            .filter(|i| {
                let ki = unsafe { i.Anonymous.ki };
                ki.dwFlags & KEYEVENTF_UNICODE != 0 && ki.dwFlags & KEYEVENTF_KEYUP == 0
            })
            .map(|i| unsafe { i.Anonymous.ki.wScan })
            .collect();
        let mut expected = "\u{1F600}".encode_utf16();
        let e1 = expected.next().expect("high surrogate");
        let e2 = expected.next().expect("low surrogate");
        let got = scans.as_mut_slice();
        assert_eq!(
            (got[0], got[1]),
            (e1, e2),
            "surrogate pair must be injected in order"
        );
    }
}
