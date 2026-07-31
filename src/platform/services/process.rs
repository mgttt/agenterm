//! OS-neutral process facade service.
//!
//! This module owns the stable product-facing verbs while `selected` resolves
//! the one native adapter for the compilation target.

use std::process::Command;

use crate::platform::{
    contract::process::{ProcessError, ProcessInfo, ProcessObservation},
    selected::process as adapter,
};

pub(crate) use adapter::ProcessTreeGuard;

pub(crate) fn autostart_server(
    parameter_name: &str,
    parameter_value: &str,
) -> std::io::Result<bool> {
    adapter::autostart_server(parameter_name, parameter_value)
}

pub(crate) fn observe(pid: u32) -> ProcessObservation {
    adapter::observe(pid)
}

pub(crate) fn start_identity(pid: u32) -> Result<String, String> {
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

pub(crate) fn list() -> Result<Vec<ProcessInfo>, ProcessError> {
    adapter::list()
}

pub(crate) fn kill(pid: u32) -> Result<(), ProcessError> {
    adapter::kill(pid)
}

pub(crate) fn configure_owned_command(command: &mut Command) -> Result<(), String> {
    adapter::configure_owned_command(command)
}

/// Backwards-compatible product-neutral verb used by Script Runtime.
pub(crate) fn configure_command(command: &mut Command) -> Result<(), String> {
    configure_owned_command(command)
}
