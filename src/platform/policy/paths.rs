//! Product path semantics and host directory naming policy.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum AtomicPathSemantics {
    VerbatimLongPath,
    CanonicalSafe,
}

#[allow(dead_code)]
pub(crate) fn atomic_path_semantics() -> AtomicPathSemantics {
    if matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    ) {
        AtomicPathSemantics::VerbatimLongPath
    } else {
        AtomicPathSemantics::CanonicalSafe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_semantics_matches_windows_host() {
        assert_eq!(
            atomic_path_semantics(),
            if matches!(
                agenterm_platform::platform_kind(),
                agenterm_platform::PlatformKind::Windows
            ) {
                AtomicPathSemantics::VerbatimLongPath
            } else {
                AtomicPathSemantics::CanonicalSafe
            }
        );
    }
}
