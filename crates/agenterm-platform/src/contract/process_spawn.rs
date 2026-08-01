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
}
