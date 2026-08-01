//! Whether the native CLI host exposes the Script worker embedding path.

pub(crate) const fn hosted_worker_available() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    )
}
