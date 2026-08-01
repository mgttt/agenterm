//! OS-neutral process facade service.
//!
//! This module owns the stable product-facing verbs while `selected` resolves
//! the one native adapter for the compilation target.

use std::process::Command;

use crate::{
    contract::process::{ProcessError, ProcessInfo, ProcessObservation},
    selected::process as adapter,
};

pub use crate::contract::process::ProcessErrorKind;
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
    adapter::kill(pid)
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
