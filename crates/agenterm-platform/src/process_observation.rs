//! Lightweight single-process liveness and start-identity observation.

pub use crate::contract::process_observation::ProcessObservation;

/// Observe one process without claiming ownership or changing its state.
///
/// `Unknown` is fail-closed evidence: callers must not infer that a process is
/// dead from permission errors, parse failures, or incomplete native queries.
pub fn observe(pid: u32) -> ProcessObservation {
    crate::selected::process_observation::observe(pid)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_live_with_a_start_identity() {
        assert!(matches!(
            observe(std::process::id()),
            ProcessObservation::Live {
                start_identity: Some(identity)
            } if !identity.is_empty()
        ));
    }

    #[test]
    fn portable_missing_pid_is_dead() {
        assert!(matches!(
            observe(i32::MAX as u32),
            ProcessObservation::Dead { .. }
        ));
    }
}
