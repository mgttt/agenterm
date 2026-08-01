use std::{fs, io};

use crate::process_security::{ProcessPrincipal, ProcessSecurityFacts};

pub(crate) fn current_process() -> io::Result<ProcessSecurityFacts> {
    process(std::process::id())
}

pub(crate) fn process(process_id: u32) -> io::Result<ProcessSecurityFacts> {
    let status = fs::read_to_string(format!("/proc/{process_id}/status"))?;
    parse_status(&status)
}

fn parse_status(status: &str) -> io::Result<ProcessSecurityFacts> {
    let effective_user_id = effective_id(status, "Uid:")?;
    let effective_group_id = effective_id(status, "Gid:")?;
    Ok(ProcessSecurityFacts::new(
        ProcessPrincipal::Posix {
            effective_user_id,
            effective_group_id,
        },
        None,
    ))
}

fn effective_id(status: &str, label: &str) -> io::Result<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .and_then(|values| values.split_ascii_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("missing {label}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_effective_uid_and_gid_columns() {
        let facts =
            parse_status("Name:\tt\nUid:\t10\t11\t12\t13\nGid:\t20\t21\t22\t23\n").expect("status");
        assert_eq!(
            facts.principal(),
            &ProcessPrincipal::Posix {
                effective_user_id: 11,
                effective_group_id: 21,
            }
        );
    }
}
