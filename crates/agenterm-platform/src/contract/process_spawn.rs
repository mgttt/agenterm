//! Product-neutral detached child-process launch facts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DetachedSpawnMode {
    /// The child was created in a new session or outside the caller's job.
    Independent,
    /// Windows denied job breakaway, so the child remains in the caller's job.
    CallerJobFallback,
}

impl DetachedSpawnMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::CallerJobFallback => "caller-job-fallback",
        }
    }
}

/// Native child termination facts without product-specific fallback policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessExit {
    /// The process returned an exit code.
    Code(i32),
    /// Unix terminated the process with a signal.
    Signal(u32),
    /// The host status exposed neither a code nor a supported signal identity.
    Unavailable,
}

impl ProcessExit {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Code(_) => "code",
            Self::Signal(_) => "signal",
            Self::Unavailable => "unavailable",
        }
    }

    /// Return the conventional process code when one can be represented.
    ///
    /// Exit codes preserve their native bit pattern when converted to `u32`;
    /// Unix signals use the shell convention `128 + signal`.
    #[must_use]
    pub const fn conventional_code(self) -> Option<u32> {
        match self {
            Self::Code(code) => Some(code as u32),
            Self::Signal(signal) => 128_u32.checked_add(signal),
            Self::Unavailable => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_spawn_modes_have_stable_names() {
        assert_eq!(DetachedSpawnMode::Independent.as_str(), "independent");
        assert_eq!(
            DetachedSpawnMode::CallerJobFallback.as_str(),
            "caller-job-fallback"
        );
    }

    #[test]
    fn process_exit_preserves_code_signal_and_unavailable_states() {
        assert_eq!(ProcessExit::Code(37).as_str(), "code");
        assert_eq!(ProcessExit::Code(37).conventional_code(), Some(37));
        assert_eq!(ProcessExit::Code(-1).conventional_code(), Some(u32::MAX));
        assert_eq!(ProcessExit::Signal(15).as_str(), "signal");
        assert_eq!(ProcessExit::Signal(15).conventional_code(), Some(143));
        assert_eq!(ProcessExit::Signal(u32::MAX).conventional_code(), None);
        assert_eq!(ProcessExit::Unavailable.as_str(), "unavailable");
        assert_eq!(ProcessExit::Unavailable.conventional_code(), None);
    }
}
