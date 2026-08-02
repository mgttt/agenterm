//! Product-level GUI host selection shared by the frontend ingress.
//!
//! This layer decides which adapter family runs based on the native platform
//! identity exposed by `agenterm-platform`; adapters stay in `src/platform`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum FrontendHost {
    Windows,
    Unix,
    Unsupported,
}

pub(crate) fn frontend_host() -> FrontendHost {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => FrontendHost::Windows,
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            FrontendHost::Unix
        }
        _ => FrontendHost::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::{FrontendHost, frontend_host};

    #[test]
    fn host_selection_matches_runtime_kind() {
        let expected = match agenterm_platform::platform_kind() {
            agenterm_platform::PlatformKind::Windows => FrontendHost::Windows,
            agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
                FrontendHost::Unix
            }
            _ => FrontendHost::Unsupported,
        };
        assert_eq!(frontend_host(), expected);
    }
}
