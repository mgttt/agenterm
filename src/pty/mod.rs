//! Shared pseudoterminal backend for `terminal_runtime`.
//!
//! Windows delegates to `rmux-pty`; Unix uses POSIX `openpty` with `libc`.

#[cfg(windows)]
pub use rmux_pty::{ChildCommand, PtyChild, PtyMaster, TerminalSize};

#[cfg(unix)]
pub(crate) use crate::platform::contract::pty::{InvalidProcessId, ProcessId, TerminalSize};

#[cfg(unix)]
#[path = "../platform/adapters/linux/pty.rs"]
mod unix;

#[cfg(unix)]
pub(crate) use unix::{ChildCommand, PtyChild, PtyMaster};
