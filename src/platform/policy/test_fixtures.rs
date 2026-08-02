//! Long-running process fixtures shared by host test journeys.

#[allow(dead_code)]
pub(crate) fn long_running_process_command_fixture() -> (&'static str, &'static [&'static str]) {
    if matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    ) {
        ("ping.exe", &["-n", "30", "127.0.0.1", ">nul"])
    } else {
        ("sleep", &["30"])
    }
}

#[allow(dead_code)]
pub(crate) fn long_running_process_command_timeout() -> (&'static str, &'static [&'static str]) {
    if matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    ) {
        ("ping", &["-n", "6", "127.0.0.1"])
    } else {
        ("sleep", &["5"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_are_not_empty() {
        assert!(!long_running_process_command_fixture().0.is_empty());
        assert!(!long_running_process_command_timeout().0.is_empty());
    }
}
