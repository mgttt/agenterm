//! Native process-pipe observation used by the Script Runtime stream pump.

use std::process::{ChildStderr, ChildStdout};

use crate::platform::selected;

pub(crate) fn stdout_probe_token(reader: &ChildStdout) -> Option<usize> {
    selected::script_stream::stdout_probe_token(reader)
}

pub(crate) fn stderr_probe_token(reader: &ChildStderr) -> Option<usize> {
    selected::script_stream::stderr_probe_token(reader)
}

/// `Err(true)` means the native pipe has closed; `Err(false)` is a typed
/// native observation failure consumed by the existing stream pump.
pub(crate) fn pipe_available(token: usize) -> Result<usize, bool> {
    selected::script_stream::pipe_available(token)
}
