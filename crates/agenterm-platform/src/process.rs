//! OS-neutral process facade service.
//!
//! This module owns the stable product-facing verbs while `selected` resolves
//! the one native adapter for the compilation target.

use std::process::{ChildStderr, ChildStdout, Command};

use crate::{
    contract::process::{ProcessError, ProcessInfo, ProcessObservation},
    selected::process as adapter,
};

pub use crate::contract::process::ProcessErrorKind;
pub use crate::contract::process::{PipeProbeError, PipeProbeToken};
pub use crate::process_spawn::{
    DetachedSpawnMode, configure_detached_command, spawn_detached_child, spawn_detached_command,
};
pub use adapter::ProcessTreeGuard;

pub fn observe(pid: u32) -> ProcessObservation {
    adapter::observe(pid)
}

pub fn start_identity(pid: u32) -> Result<String, String> {
    match observe(pid) {
        ProcessObservation::Live {
            start_identity: Some(identity),
        } => Ok(identity),
        ProcessObservation::Live {
            start_identity: None,
        } => Err("process is live but its start identity is unavailable".to_owned()),
        ProcessObservation::Dead { reason } | ProcessObservation::Unknown { reason } => Err(reason),
    }
}

pub fn list() -> Result<Vec<ProcessInfo>, ProcessError> {
    adapter::list()
}

pub fn kill(pid: u32) -> Result<(), ProcessError> {
    crate::process_control::terminate(pid, crate::process_control::TerminationMode::Forceful)
        .map_err(|error| {
            use crate::process_control::ProcessControlErrorKind as ControlKind;
            let kind = match error.kind() {
                ControlKind::InvalidId => ProcessErrorKind::IdOutOfRange,
                ControlKind::IdOutOfRange => ProcessErrorKind::IdOutOfRange,
                ControlKind::Open => ProcessErrorKind::KillOpen,
                ControlKind::Terminate | ControlKind::Suspend | ControlKind::Resume => {
                    ProcessErrorKind::Kill
                }
                ControlKind::Unsupported => ProcessErrorKind::Unsupported,
            };
            ProcessError::new(kind, error.detail())
        })
}

pub fn configure_owned_command(command: &mut Command) -> Result<(), String> {
    adapter::configure_owned_command(command)
}

/// Backwards-compatible product-neutral verb used by Script Runtime.
pub fn configure_command(command: &mut Command) -> Result<(), String> {
    configure_owned_command(command)
}

pub fn stdout_probe_token(reader: &ChildStdout) -> Option<PipeProbeToken> {
    adapter::stdout_probe_token(reader)
}

pub fn stderr_probe_token(reader: &ChildStderr) -> Option<PipeProbeToken> {
    adapter::stderr_probe_token(reader)
}

pub fn pipe_available(token: PipeProbeToken) -> Result<usize, PipeProbeError> {
    adapter::pipe_available(token)
}

/// Write a launcher diagnostic to stderr or an already-existing parent
/// console. This never allocates a new console and reports best-effort success.
pub fn write_parent_console_stderr(message: &str) -> bool {
    adapter::write_parent_console_stderr(message)
}
