//! Product Control Center policy: how each host obtains screenshots.
//!
//! The host mechanism lives in agenterm-platform; this table is the
//! AgenTerm-level product decision shared by frontend hosts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScreenshotStrategy {
    DirectNativeWindow,
    RendererRequest,
    Unsupported,
}

pub(crate) fn screenshot_strategy() -> ScreenshotStrategy {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => ScreenshotStrategy::DirectNativeWindow,
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            ScreenshotStrategy::RendererRequest
        }
        _ => ScreenshotStrategy::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_matches_runtime_kind() {
        assert_eq!(
            screenshot_strategy(),
            match agenterm_platform::platform_kind() {
                agenterm_platform::PlatformKind::Windows => ScreenshotStrategy::DirectNativeWindow,
                agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
                    ScreenshotStrategy::RendererRequest
                }
                _ => ScreenshotStrategy::Unsupported,
            }
        );
    }
}
