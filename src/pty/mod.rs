//! Shared pseudoterminal backend for `terminal_runtime`.
//!
//! Windows delegates to `rmux-pty`; Unix uses POSIX `openpty` with `libc`.

#[cfg(windows)]
pub use rmux_pty::{ProcessId, TerminalSize};

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

#[cfg(unix)]
impl TerminalSize {
    #[must_use]
    pub const fn new(rows: u16, cols: u16) -> Self {
        Self { rows, cols }
    }
}

#[cfg(unix)]
/// A platform-neutral process identifier for PTY children.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProcessId(u32);

#[cfg(unix)]
impl ProcessId {
    /// Creates a process identifier from the operating-system value.
    pub fn new(raw: u32) -> Result<Self, InvalidProcessId> {
        if raw == 0 {
            return Err(InvalidProcessId(raw));
        }
        Ok(Self(raw))
    }

    /// Returns the raw operating-system process id.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidProcessId(pub u32);

#[cfg(unix)]
impl std::fmt::Display for InvalidProcessId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid process id: {}", self.0)
    }
}

#[cfg(unix)]
impl std::error::Error for InvalidProcessId {}

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;

#[cfg(windows)]
pub use windows::{
    ChildCommand, PtyChild, PtyMaster, SpawnedPty, write_windows_console_mouse_drag,
};
#[cfg(unix)]
pub use unix::{ChildCommand, PtyChild, PtyMaster, SpawnedPty};
