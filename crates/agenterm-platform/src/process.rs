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
                ControlKind::IdOutOfRange => ProcessErrorKind::IdOutOfRange,
                ControlKind::Open => ProcessErrorKind::KillOpen,
                ControlKind::Terminate => ProcessErrorKind::Kill,
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

/// Configure a child that must outlive a caller-owned process tree.
///
/// The caller retains ownership of executable discovery, arguments, and stdio.
pub fn configure_detached_command(command: &mut Command) -> Result<(), String> {
    adapter::configure_detached_command(command)
}

/// Reports whether a background child escaped the caller's process job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DetachedSpawnMode {
    /// The child was created outside the caller's owned process job.
    Independent,
    /// The host denied job breakaway, so the child was launched in the caller's job.
    CallerJobFallback,
}

/// Spawns a background child, retrying inside the caller's job only when the
/// host explicitly rejects independent job breakaway.
///
/// The fallback is surfaced to callers rather than silently claiming full
/// detachment. Executable discovery, arguments and stdio remain caller policy.
pub fn spawn_detached_command(command: &mut Command) -> std::io::Result<DetachedSpawnMode> {
    configure_detached_command(command).map_err(std::io::Error::other)?;
    match command.spawn() {
        Ok(_) => Ok(DetachedSpawnMode::Independent),
        Err(error) if adapter::is_breakaway_denied(&error) => {
            adapter::configure_caller_job_fallback(command).map_err(std::io::Error::other)?;
            command.spawn()?;
            Ok(DetachedSpawnMode::CallerJobFallback)
        }
        Err(error) => Err(error),
    }
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
