//! Windows console line-editor adapter.
//!
//! Input switches to per-key mode via `SetConsoleMode` (keeping
//! `ENABLE_PROCESSED_INPUT` so Ctrl-C still reaches the console handler chain)
//! and keys are read as `INPUT_RECORD` key events. Stderr VT processing is
//! enabled best-effort so callers can redraw with ANSI sequences on modern
//! conhost/Windows Terminal; legacy consoles keep working without redraws.

use windows_sys::Win32::{
    Foundation::{GetLastError, HANDLE},
    System::Console::{
        ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle,
        PeekConsoleInputW, ReadConsoleInputW, SetConsoleMode, STD_ERROR_HANDLE,
        STD_INPUT_HANDLE, INPUT_RECORD, KEY_EVENT,
    },
    UI::Input::KeyboardAndMouse::{
        VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT,
        VK_UP,
    },
};

use crate::contract::console_line_editor::{ConsoleKey, ConsoleLineEditorError};

const CONTROL_Z: u16 = 0x001A;

fn native_failure(code: &'static str, message: String) -> ConsoleLineEditorError {
    ConsoleLineEditorError::failed(code, Some(i64::from(unsafe { GetLastError() })), message)
}

/// Normalize one Windows key record to a portable key. Pure so the mapping is
/// unit-testable without a console.
fn map_key(virtual_key: u16, unicode: u16) -> ConsoleKey {
    if unicode == CONTROL_Z {
        return ConsoleKey::Eof;
    }
    if unicode != 0 && !(0x00..=0x1F).contains(&unicode) {
        return char::from_u32(u32::from(unicode))
            .map(ConsoleKey::Char)
            .unwrap_or(ConsoleKey::Unknown);
    }
    match virtual_key {
        VK_RETURN => ConsoleKey::Enter,
        VK_BACK => ConsoleKey::Backspace,
        VK_DELETE => ConsoleKey::Delete,
        VK_LEFT => ConsoleKey::Left,
        VK_RIGHT => ConsoleKey::Right,
        VK_UP => ConsoleKey::Up,
        VK_DOWN => ConsoleKey::Down,
        VK_HOME => ConsoleKey::Home,
        VK_END => ConsoleKey::End,
        _ => ConsoleKey::Unknown,
    }
}

pub(crate) struct Editor {
    input_handle: HANDLE,
    original_input_mode: u32,
    error_handle: HANDLE,
    original_error_mode: u32,
}

impl Editor {
    pub(crate) fn enter() -> Result<Self, ConsoleLineEditorError> {
        let input_handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let error_handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        let mut input_mode = 0_u32;
        let mut error_mode = 0_u32;
        if unsafe { GetConsoleMode(input_handle, &mut input_mode) } == 0 {
            return Err(ConsoleLineEditorError::unsupported(
                "stdin-is-not-a-console",
            ));
        }
        let console_has_error_mode = unsafe { GetConsoleMode(error_handle, &mut error_mode) } != 0;
        let editor_mode =
            input_mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT) | ENABLE_PROCESSED_INPUT;
        if unsafe { SetConsoleMode(input_handle, editor_mode) } == 0 {
            return Err(native_failure("set-input-mode", "SetConsoleMode failed".to_owned()));
        }
        let editor = Self {
            input_handle,
            original_input_mode: input_mode,
            error_handle,
            original_error_mode: error_mode,
        };
        if console_has_error_mode {
            // Best effort: redraws use ANSI; legacy consoles simply keep the
            // previous behavior and the editor still works without redraws.
            let _ = unsafe {
                SetConsoleMode(
                    error_handle,
                    error_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
                )
            };
        }
        Ok(editor)
    }

    pub(crate) fn read_key(&mut self) -> Result<Option<ConsoleKey>, ConsoleLineEditorError> {
        loop {
            let mut record = unsafe { std::mem::zeroed::<INPUT_RECORD>() };
            let mut pending = 0_u32;
            if unsafe { PeekConsoleInputW(self.input_handle, &mut record, 1, &mut pending) } == 0 {
                return Err(native_failure(
                    "peek-input",
                    "PeekConsoleInputW failed".to_owned(),
                ));
            }
            if pending == 0 {
                // PeekConsoleInputW does not block; pace the poll loop so the
                // caller's command channel stays responsive without spinning
                // the CPU while idle.
                std::thread::sleep(std::time::Duration::from_millis(10));
                return Ok(None);
            }
            let mut consumed = 0_u32;
            if unsafe { ReadConsoleInputW(self.input_handle, &mut record, 1, &mut consumed) } == 0 {
                return Err(native_failure(
                    "read-input",
                    "ReadConsoleInputW failed".to_owned(),
                ));
            }
            if u32::from(record.EventType) != KEY_EVENT {
                continue;
            }
            let event = unsafe { record.Event.KeyEvent };
            if event.bKeyDown == 0 {
                continue;
            }
            let unicode = unsafe { event.uChar.UnicodeChar };
            return Ok(Some(map_key(event.wVirtualKeyCode, unicode)));
        }
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        let _ = unsafe { SetConsoleMode(self.input_handle, self.original_input_mode) };
        if self.original_error_mode != 0 {
            let _ = unsafe { SetConsoleMode(self.error_handle, self.original_error_mode) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_A, VK_TAB,
    };

    #[test]
    fn maps_printable_unicode_and_control_keys() {
        assert_eq!(map_key(VK_A, b'a'.into()), ConsoleKey::Char('a'));
        assert_eq!(map_key(VK_RETURN, 0), ConsoleKey::Enter);
        assert_eq!(map_key(VK_BACK, 0), ConsoleKey::Backspace);
        assert_eq!(map_key(VK_DELETE, 0), ConsoleKey::Delete);
        assert_eq!(map_key(VK_LEFT, 0), ConsoleKey::Left);
        assert_eq!(map_key(VK_RIGHT, 0), ConsoleKey::Right);
        assert_eq!(map_key(VK_UP, 0), ConsoleKey::Up);
        assert_eq!(map_key(VK_DOWN, 0), ConsoleKey::Down);
        assert_eq!(map_key(VK_HOME, 0), ConsoleKey::Home);
        assert_eq!(map_key(VK_END, 0), ConsoleKey::End);
        assert_eq!(map_key(VK_TAB, 0), ConsoleKey::Unknown);
    }

    #[test]
    fn maps_control_z_to_eof_even_with_virtual_key() {
        assert_eq!(map_key(0, CONTROL_Z), ConsoleKey::Eof);
        assert_eq!(map_key(VK_A, CONTROL_Z), ConsoleKey::Eof);
    }
}
