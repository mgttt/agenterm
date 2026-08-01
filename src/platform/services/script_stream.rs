//! Native process-pipe observation used by the Script Runtime stream pump.

use std::process::{ChildStderr, ChildStdout};

pub(crate) use agenterm_platform::process::PipeProbeToken;

pub(crate) fn stdout_probe_token(reader: &ChildStdout) -> Option<PipeProbeToken> {
    agenterm_platform::process::stdout_probe_token(reader)
}

pub(crate) fn stderr_probe_token(reader: &ChildStderr) -> Option<PipeProbeToken> {
    agenterm_platform::process::stderr_probe_token(reader)
}

pub(crate) fn pipe_available(token: PipeProbeToken) -> Result<usize, bool> {
    agenterm_platform::process::pipe_available(token)
        .map_err(|error| matches!(error, agenterm_platform::process::PipeProbeError::Closed))
}
