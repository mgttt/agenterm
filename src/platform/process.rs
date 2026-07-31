//! Product-facing process facade compatibility surface.
//!
//! OS-specific process implementation is selected through
//! [`crate::platform::services::process`]. Product and Script modules import
//! this stable module without learning which adapter owns the native handles.

#[allow(unused_imports)] // Compatibility exports consumed by product modules per target.
pub(crate) use crate::platform::contract::process::{
    ProcessError, ProcessErrorKind, ProcessInfo, ProcessObservation,
};
#[allow(unused_imports)] // Compatibility exports consumed by product modules per target.
pub(crate) use crate::platform::services::process::{
    ProcessTreeGuard, autostart_server, configure_command, configure_owned_command, kill, list,
    observe, start_identity,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_has_stable_observation_and_inventory_entry() {
        assert!(matches!(
            observe(std::process::id()),
            ProcessObservation::Live { .. }
        ));
        assert!(
            list()
                .expect("process inventory")
                .iter()
                .any(|entry| entry.id == std::process::id())
        );
    }
}
