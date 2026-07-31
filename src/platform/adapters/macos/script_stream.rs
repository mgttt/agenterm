use std::process::{ChildStderr, ChildStdout};

pub(crate) fn stdout_probe_token(_reader: &ChildStdout) -> Option<usize> {
    None
}
pub(crate) fn stderr_probe_token(_reader: &ChildStderr) -> Option<usize> {
    None
}
pub(crate) fn pipe_available(_token: usize) -> Result<usize, bool> {
    Err(false)
}
