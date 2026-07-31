//! macOS PTY adapter identity using the shared POSIX mechanism.

#[path = "../linux/pty.rs"]
mod unix_mechanism;

pub(crate) use crate::platform::contract::pty::TerminalSize;
pub(crate) use unix_mechanism::{ChildCommand, PtyChild, PtyMaster};
