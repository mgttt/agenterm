//! PTY facade projection; native handles remain adapter-private to selection.

pub use crate::contract::pty::{InvalidProcessId, ProcessId, PtyError, PtyResult, TerminalSize};
pub use crate::selected::pty::{ChildCommand, PtyChild, PtyMaster};
