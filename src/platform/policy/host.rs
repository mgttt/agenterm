//! Product host routing policy shared by product modules.
//!
//! Native platform identity comes from agenterm-platform; this table turns
//! that fact into the product-level host labels used by paths, fonts, IPC and
//! Script runtime policy.

pub(crate) fn is_windows_host() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    )
}

pub(crate) fn is_macos_host() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    )
}

pub(crate) fn is_unix_host() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos
    )
}

#[allow(dead_code)]
pub(crate) fn shell_command_for_host<'a>(
    windows_command: &'a str,
    unix_command: &'a str,
) -> &'a str {
    if is_windows_host() {
        windows_command
    } else {
        unix_command
    }
}

#[cfg(test)]
mod tests {
    use super::{is_macos_host, is_unix_host, is_windows_host, shell_command_for_host};

    #[test]
    fn host_predicates_match_runtime_kind() {
        let kind = agenterm_platform::platform_kind();
        assert_eq!(
            is_windows_host(),
            matches!(kind, agenterm_platform::PlatformKind::Windows)
        );
        assert_eq!(
            is_macos_host(),
            matches!(kind, agenterm_platform::PlatformKind::Macos)
        );
        assert_eq!(
            is_unix_host(),
            matches!(
                kind,
                agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos
            )
        );
    }

    #[test]
    fn shell_command_for_host_selects_by_runtime_kind() {
        assert_eq!(
            shell_command_for_host("win", "unix"),
            if is_windows_host() { "win" } else { "unix" }
        );
    }
}
