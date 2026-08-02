//! Product runtime policy: host availability and test-host capability.

pub(crate) fn hosted_script_worker_available() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_availability_tracks_windows() {
        assert_eq!(
            hosted_script_worker_available(),
            matches!(
                agenterm_platform::platform_kind(),
                agenterm_platform::PlatformKind::Windows
            )
        );
    }
}
