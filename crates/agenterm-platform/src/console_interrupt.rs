//! RAII console interrupt observation and temporary ignore scopes.

pub use crate::contract::console_interrupt::ConsoleInterruptError;

/// A process-wide observer for Ctrl-C/SIGINT.
///
/// Native handlers only perform async-safe notification. Call
/// [`take_pending`](Self::take_pending) from ordinary Rust code to consume one
/// or more coalesced interrupts. Only one observer or ignore guard may be
/// installed through this crate at a time. Unix callers must not independently
/// replace the SIGINT disposition while this value is alive.
#[must_use = "dropping the observer restores the previous interrupt handler"]
pub struct ConsoleInterruptObserver {
    inner: crate::selected::console_interrupt::Observer,
}

impl ConsoleInterruptObserver {
    pub fn install() -> Result<Self, ConsoleInterruptError> {
        crate::selected::console_interrupt::Observer::install().map(|inner| Self { inner })
    }

    /// Return whether at least one interrupt arrived since the previous call.
    pub fn take_pending(&self) -> Result<bool, ConsoleInterruptError> {
        self.inner.take_pending()
    }
}

/// A process-wide RAII scope that ignores only Ctrl-C/SIGINT.
///
/// Dropping the guard restores the handler disposition that was active when
/// the guard was installed. Unix callers must not independently replace the
/// SIGINT disposition while this value is alive.
#[must_use = "dropping the guard restores normal interrupt handling"]
pub struct ConsoleInterruptIgnoreGuard {
    _inner: crate::selected::console_interrupt::IgnoreGuard,
}

impl ConsoleInterruptIgnoreGuard {
    pub fn install() -> Result<Self, ConsoleInterruptError> {
        crate::selected::console_interrupt::IgnoreGuard::install()
            .map(|inner| Self { _inner: inner })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_and_ignore_are_exclusive_and_drop_releases_ownership() {
        assert_eq!(
            crate::capability_status(crate::Capability::ConsoleInterrupt),
            crate::CapabilityStatus::Available
        );
        let observer = ConsoleInterruptObserver::install().unwrap();
        assert!(!observer.take_pending().unwrap());
        assert!(matches!(
            ConsoleInterruptIgnoreGuard::install(),
            Err(ConsoleInterruptError::Failed { code, .. }) if code == "already-installed"
        ));
        drop(observer);
        drop(ConsoleInterruptIgnoreGuard::install().unwrap());
    }
}
