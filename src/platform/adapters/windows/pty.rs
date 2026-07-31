//! Windows ConPTY adapter projection.

#[allow(unused_imports)] // Staged until the Windows PTY wrapper owns neutral TerminalSize conversion.
pub(crate) use rmux_pty::{ChildCommand, PtyChild, PtyMaster};
