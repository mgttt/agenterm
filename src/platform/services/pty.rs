//! PTY facade projection; native handles remain adapter-private to selection.

pub(crate) use crate::platform::contract::pty::TerminalSize;
pub(crate) use crate::platform::selected::pty::{ChildCommand, PtyChild, PtyMaster};
