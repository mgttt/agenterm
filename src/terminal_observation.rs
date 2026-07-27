use std::{error::Error, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TerminalProcessState {
    Running,
    Exited { code: u32 },
    Error { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalObservation {
    pub(crate) process_id: Option<u32>,
    pub(crate) exit_code: Option<u32>,
    pub(crate) error: Option<String>,
    pub(crate) input_bytes: usize,
    pub(crate) input_writes: usize,
    pub(crate) output_bytes: usize,
    pub(crate) submit_pending: bool,
}

impl TerminalObservation {
    pub(crate) fn process_state(&self) -> TerminalProcessState {
        if let Some(message) = &self.error {
            TerminalProcessState::Error {
                message: message.clone(),
            }
        } else if let Some(code) = self.exit_code {
            TerminalProcessState::Exited { code }
        } else {
            TerminalProcessState::Running
        }
    }

    pub(crate) fn delta_to(
        &self,
        after: &Self,
    ) -> Result<TerminalObservationDelta, TerminalObservationInvariantError> {
        if after.output_bytes < self.output_bytes {
            return Err(TerminalObservationInvariantError::OutputCounterRegressed {
                before: self.output_bytes,
                after: after.output_bytes,
            });
        }
        Ok(TerminalObservationDelta {
            output_advanced_by: after.output_bytes - self.output_bytes,
            process_state_changed: self.process_state() != after.process_state(),
            submission_finished: self.submit_pending && !after.submit_pending,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TerminalObservationDelta {
    pub(crate) output_advanced_by: usize,
    pub(crate) process_state_changed: bool,
    pub(crate) submission_finished: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalObservationInvariantError {
    OutputCounterRegressed { before: usize, after: usize },
}

impl fmt::Display for TerminalObservationInvariantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputCounterRegressed { before, after } => write!(
                formatter,
                "terminal output counter regressed from {before} to {after}"
            ),
        }
    }
}

impl Error for TerminalObservationInvariantError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation() -> TerminalObservation {
        TerminalObservation {
            process_id: Some(42),
            exit_code: None,
            error: None,
            input_bytes: 10,
            input_writes: 2,
            output_bytes: 100,
            submit_pending: false,
        }
    }

    #[test]
    fn identical_observations_have_an_empty_delta() {
        let before = observation();
        let delta = before.delta_to(&before).unwrap();

        assert_eq!(delta, TerminalObservationDelta::default());
    }

    #[test]
    fn output_advance_reports_the_exact_byte_delta() {
        let before = observation();
        let mut after = before.clone();
        after.output_bytes = 140;

        assert_eq!(
            before.delta_to(&after).unwrap(),
            TerminalObservationDelta {
                output_advanced_by: 40,
                ..TerminalObservationDelta::default()
            }
        );
    }

    #[test]
    fn running_to_exited_preserves_the_exit_code() {
        let before = observation();
        let mut after = before.clone();
        after.exit_code = Some(0xfeed_beef);

        assert_eq!(
            after.process_state(),
            TerminalProcessState::Exited { code: 0xfeed_beef }
        );
        assert!(before.delta_to(&after).unwrap().process_state_changed);
    }

    #[test]
    fn error_state_wins_over_running_or_exited_state() {
        let running = observation();
        let mut error = running.clone();
        error.error = Some("reader failed".to_owned());
        assert_eq!(
            error.process_state(),
            TerminalProcessState::Error {
                message: "reader failed".to_owned()
            }
        );
        assert!(running.delta_to(&error).unwrap().process_state_changed);

        let mut exited = running;
        exited.exit_code = Some(7);
        let mut exited_with_error = exited.clone();
        exited_with_error.error = Some("wait failed".to_owned());
        assert_eq!(
            exited_with_error.process_state(),
            TerminalProcessState::Error {
                message: "wait failed".to_owned()
            }
        );
        assert!(
            exited
                .delta_to(&exited_with_error)
                .unwrap()
                .process_state_changed
        );
    }

    #[test]
    fn only_pending_to_idle_marks_submission_finished() {
        let mut pending = observation();
        pending.submit_pending = true;
        let idle = observation();
        assert!(pending.delta_to(&idle).unwrap().submission_finished);

        assert!(!idle.delta_to(&pending).unwrap().submission_finished);
        assert!(!idle.delta_to(&idle).unwrap().submission_finished);
        assert!(!pending.delta_to(&pending).unwrap().submission_finished);
    }

    #[test]
    fn output_counter_regression_is_a_typed_invariant_error() {
        let before = observation();
        let mut after = before.clone();
        after.output_bytes = 99;

        assert_eq!(
            before.delta_to(&after),
            Err(TerminalObservationInvariantError::OutputCounterRegressed {
                before: 100,
                after: 99,
            })
        );
    }
}
