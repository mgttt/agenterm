use crate::contract::process_observation::ProcessObservation;

pub(crate) fn observe(pid: u32) -> ProcessObservation {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ProcessObservation::Dead {
                reason: "process_not_found".to_owned(),
            };
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return ProcessObservation::Unknown {
                reason: "process_access_denied".to_owned(),
            };
        }
        Err(error) => {
            return ProcessObservation::Unknown {
                reason: format!("process_identity_read_failed:{error}"),
            };
        }
    };
    parse_stat_observation(&stat)
}

fn parse_stat_observation(stat: &str) -> ProcessObservation {
    let Some(fields) = stat.rsplit_once(") ").map(|(_, fields)| fields) else {
        return ProcessObservation::Unknown {
            reason: "process_identity_parse_failed".to_owned(),
        };
    };
    let mut fields = fields.split_whitespace();
    let Some(state) = fields.next() else {
        return ProcessObservation::Unknown {
            reason: "process_identity_parse_failed".to_owned(),
        };
    };
    if matches!(state, "Z" | "X" | "x") {
        return ProcessObservation::Dead {
            reason: "process_exited_not_reaped".to_owned(),
        };
    }
    let Some(start_ticks) = fields
        .nth(18)
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
    else {
        return ProcessObservation::Unknown {
            reason: "process_identity_parse_failed".to_owned(),
        };
    };
    ProcessObservation::Live {
        start_identity: Some(format!("proc-start-ticks:{start_ticks}")),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_stat_observation;
    use crate::contract::process_observation::ProcessObservation;

    fn stat_with_state(state: &str) -> String {
        format!(
            "42 (worker with spaces) {state} 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 12345"
        )
    }

    #[test]
    fn zombie_and_dead_states_are_not_reported_live() {
        for state in ["Z", "X", "x"] {
            assert!(matches!(
                parse_stat_observation(&stat_with_state(state)),
                ProcessObservation::Dead { ref reason }
                    if reason == "process_exited_not_reaped"
            ));
        }
    }

    #[test]
    fn live_state_preserves_start_identity() {
        assert!(matches!(
            parse_stat_observation(&stat_with_state("S")),
            ProcessObservation::Live { start_identity: Some(ref identity) }
                if identity == "proc-start-ticks:12345"
        ));
    }
}
