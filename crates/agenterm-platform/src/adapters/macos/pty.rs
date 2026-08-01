//! macOS PTY adapter identity using the shared POSIX mechanism.

#[path = "../linux/pty.rs"]
mod unix_mechanism;

pub use unix_mechanism::{ChildCommand, PtyChild, PtyMaster, login_shell_argument};
