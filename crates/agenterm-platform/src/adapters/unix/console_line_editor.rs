//! Unix console line-editor adapter (termios raw input).
//!
//! Raw mode keeps `ISIG` enabled so Ctrl-C still raises SIGINT for the
//! interrupt observer instead of becoming a key byte. Keys are read with
//! `poll` slices and decoded by the shared [`EscapeParser`]
//! (`crate::console_line_editor`), which survives split reads.

use std::io;

use crate::console_line_editor::EscapeParser;
use crate::contract::console_line_editor::{ConsoleKey, ConsoleLineEditorError};

const POLL_WINDOW_MS: i32 = 10;
const ESC_PENDING_WINDOW_MS: i32 = 20;

fn native_failure(code: &'static str, error: io::Error) -> ConsoleLineEditorError {
    ConsoleLineEditorError::failed(code, error.raw_os_error().map(i64::from), error.to_string())
}

pub(crate) struct Editor {
    original: libc::termios,
    parser: EscapeParser,
}

impl Editor {
    pub(crate) fn enter() -> Result<Self, ConsoleLineEditorError> {
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
            return Err(ConsoleLineEditorError::unsupported(
                "stdin-is-not-a-terminal",
            ));
        }
        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        // Keep Ctrl-C (and Ctrl-\ / Ctrl-Z) as signals so the existing
        // interrupt observer keeps working while we read per-key input.
        raw.c_lflag |= libc::ISIG;
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            return Err(native_failure("set-raw-mode", io::Error::last_os_error()));
        }
        Ok(Self {
            original,
            parser: EscapeParser::new(),
        })
    }

    pub(crate) fn read_key(&mut self) -> Result<Option<ConsoleKey>, ConsoleLineEditorError> {
        loop {
            let window = if self.parser.has_pending() {
                ESC_PENDING_WINDOW_MS
            } else {
                POLL_WINDOW_MS
            };
            let mut pollfd = libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut pollfd, 1, window) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    return Ok(None);
                }
                return Err(native_failure("poll-stdin", error));
            }
            if ready == 0 {
                if self.parser.has_pending() {
                    return Ok(Some(self.parser.flush()));
                }
                return Ok(None);
            }
            let mut bytes = [0_u8; 64];
            let read =
                unsafe { libc::read(libc::STDIN_FILENO, bytes.as_mut_ptr().cast(), bytes.len()) };
            if read < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    return Ok(None);
                }
                return Err(native_failure("read-stdin", error));
            }
            if read == 0 {
                return Ok(Some(ConsoleKey::Eof));
            }
            let mut keys = self.parser.feed(&bytes[..read as usize]);
            if keys.is_empty() {
                continue;
            }
            return Ok(keys.pop());
        }
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        let _ = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original) };
    }
}
