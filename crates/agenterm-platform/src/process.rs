//! OS-neutral process facade service.
//!
//! This module owns the stable product-facing verbs while `selected` resolves
//! the one native adapter for the compilation target.

use std::process::{ChildStderr, ChildStdout, Command};

use crate::{
    contract::process::{ProcessError, ProcessInfo},
    selected::process as adapter,
};

pub use crate::contract::process::ProcessErrorKind;
pub use crate::contract::process::{PipeProbeError, PipeProbeToken};
pub use crate::process_observation::{ProcessObservation, observe, start_identity};
pub use crate::process_spawn::{
    DetachedSpawnMode, ProcessExit, classify_exit_status, configure_breakaway_visible_command,
    configure_detached_command, is_breakaway_denied, spawn_breakaway_visible_child,
    spawn_breakaway_visible_command, spawn_detached_child, spawn_detached_command,
};
pub use adapter::ProcessTreeGuard;

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

/// Write a CLI line (e.g. `--version`) to stdout or an attached parent console.
/// GUI-subsystem binaries on Windows attach to the parent terminal when present.
pub fn write_parent_console_stdout(message: &str) -> bool {
    adapter::write_parent_console_stdout(message)
}

/// Duplicate the caller-visible stdin/stdout/stderr handles for explicit
/// child stdio wiring (`[stdin, stdout, stderr]`, `None` where the process
/// holds no valid handle). Attach the parent console first (`ScopedConsole`)
/// so console-backed slots are populated; caller pipe or file redirections
/// are duplicated as-is. This is how a GUI-subsystem binary hands its real
/// streams to a worker child without trusting `Stdio::inherit`.
#[cfg(windows)]
pub fn duplicated_std_handles() -> [Option<std::os::windows::io::OwnedHandle>; 3] {
    crate::selected::console::duplicate_std_handles()
}

/// Opaque guard that keeps the parent console attached.
///
/// On Windows this wraps the shared `ConsoleGuard`; the console is released
/// when the guard is dropped. Spawn this before any `println!` calls in a
/// `windows_subsystem = "windows"` binary.
///
/// On Unix this is a zero-size no-op.
pub struct ScopedConsole {
    #[cfg(windows)]
    _inner: Option<crate::selected::console::ConsoleGuard>,
    #[cfg(not(windows))]
    _unused: (),
}

impl ScopedConsole {
    /// Attach to the parent console. Returns `None` when there is no parent
    /// console (double-click launch on Windows).
    pub fn attach_parent() -> Option<Self> {
        #[cfg(windows)]
        {
            crate::selected::console::ConsoleGuard::attach_parent()
                .ok()
                .map(|inner| Self {
                    _inner: Some(inner),
                })
        }
        #[cfg(not(windows))]
        {
            Some(Self { _unused: () })
        }
    }

    /// Attach to the parent console without suppressing console control
    /// events, so `Ctrl+C` keeps its default terminate behavior. This is
    /// the variant for CLI worker processes; the GUI host uses
    /// `attach_parent`, which must survive child-console control events.
    pub fn attach_parent_with_default_interrupts() -> Option<Self> {
        #[cfg(windows)]
        {
            crate::selected::console::ConsoleGuard::attach_parent_with_default_interrupts()
                .ok()
                .map(|inner| Self {
                    _inner: Some(inner),
                })
        }
        #[cfg(not(windows))]
        {
            Some(Self { _unused: () })
        }
    }
}
