//! Product runtime policy: host availability and test-host capability.

pub(crate) fn hosted_script_worker_available() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
            | agenterm_platform::PlatformKind::Linux
            | agenterm_platform::PlatformKind::Macos
    )
}

#[allow(dead_code)]
pub(crate) fn script_process_test_host_supported() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
            | agenterm_platform::PlatformKind::Linux
            | agenterm_platform::PlatformKind::Macos
    )
}

pub(crate) fn alternate_terminal_shell_id() -> &'static str {
    if is_windows_host() {
        "powershell"
    } else {
        "bash"
    }
}

pub(crate) fn alternate_terminal_shell_program() -> &'static str {
    if is_windows_host() {
        "powershell.exe"
    } else {
        "/bin/bash"
    }
}

pub(crate) fn new_terminal_command_line(shell_id: &str, initial: &str) -> Vec<String> {
    if is_windows_host() {
        windows_new_terminal_command_line(shell_id, initial)
    } else {
        unix_new_terminal_command_line(shell_id, initial)
    }
}

fn is_windows_host() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    )
}

fn windows_new_terminal_command_line(shell_id: &str, initial: &str) -> Vec<String> {
    match shell_id {
        "default" if initial.is_empty() => Vec::new(),
        "powershell" | "bash" => {
            let mut child = vec!["powershell.exe".to_owned(), "-NoLogo".to_owned()];
            if !initial.is_empty() {
                child.extend([
                    "-NoExit".to_owned(),
                    "-Command".to_owned(),
                    initial.to_owned(),
                ]);
            }
            child
        }
        _ => {
            let program = agenterm_platform::runtime::primary_terminal_shell().program;
            let mut child = vec![program.to_owned(), "/d".to_owned()];
            if !initial.is_empty() {
                child.extend(["/k".to_owned(), initial.to_owned()]);
            }
            child
        }
    }
}

fn unix_new_terminal_command_line(shell_id: &str, initial: &str) -> Vec<String> {
    if matches!(shell_id, "bash" | "powershell") {
        let mut child = vec!["/bin/bash".to_owned()];
        if !initial.is_empty() {
            child.extend(["-c".to_owned(), format!("{initial}; exec /bin/bash -i")]);
        }
        return child;
    }
    match shell_id {
        "default" if initial.is_empty() => Vec::new(),
        _ => {
            let program = agenterm_platform::runtime::primary_terminal_shell().program;
            let mut child = vec![program.to_owned()];
            if !initial.is_empty() {
                child.extend(["-c".to_owned(), format!("{initial}; exec {program} -i")]);
            }
            child
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_availability_tracks_supported_hosts() {
        assert_eq!(
            hosted_script_worker_available(),
            matches!(
                agenterm_platform::platform_kind(),
                agenterm_platform::PlatformKind::Windows
                    | agenterm_platform::PlatformKind::Linux
                    | agenterm_platform::PlatformKind::Macos
            )
        );
    }

    #[test]
    fn default_empty_new_terminal_command_line_is_empty() {
        assert!(new_terminal_command_line("default", "").is_empty());
    }

    #[test]
    fn alternate_shell_command_line_starts_with_alternate_program() {
        let line = new_terminal_command_line(alternate_terminal_shell_id(), "echo hi");
        assert_eq!(
            line.first().map(String::as_str),
            Some(alternate_terminal_shell_program())
        );
    }
}
