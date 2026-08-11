//! OS-neutral parent-console output service.
//!
//! This deliberately does not imply process inventory, control, observation,
//! spawning, or containment. GUI-subsystem launchers can report diagnostics
//! without enabling the aggregate `process` capability.

use crate::selected::parent_console as adapter;

/// Write one diagnostic line to stderr or an already-existing parent console.
///
/// This never allocates a new console and reports best-effort success.
pub fn write_stderr(message: &str) -> bool {
    adapter::write_stderr(message)
}

/// Write one CLI line to stdout or an already-existing parent console.
pub fn write_stdout(message: &str) -> bool {
    adapter::write_stdout(message)
}
