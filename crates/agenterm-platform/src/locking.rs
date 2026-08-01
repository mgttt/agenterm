//! Cross-process locking with caller-owned paths and namespaces.

use std::path::Path;

use crate::selected;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LockErrorKind {
    Open,
    Wait,
    Contended,
    InvalidInput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockError {
    kind: LockErrorKind,
    message: String,
}

impl LockError {
    pub fn new(kind: LockErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> LockErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for LockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "cross-process lock {:?}: {}",
            self.kind, self.message
        )
    }
}

impl std::error::Error for LockError {}

pub struct PathLock {
    _guard: selected::locking::PathLock,
}

impl PathLock {
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        selected::locking::PathLock::acquire(path).map(|guard| Self { _guard: guard })
    }

    pub fn try_acquire(path: &Path) -> Result<Self, LockError> {
        selected::locking::PathLock::try_acquire(path).map(|guard| Self { _guard: guard })
    }
}

pub struct SlotPermit {
    _guard: selected::locking::SlotPermit,
}

impl SlotPermit {
    pub fn try_acquire(directory: &Path, namespace: &str, limit: usize) -> Result<Self, LockError> {
        if namespace.is_empty() || limit == 0 {
            return Err(LockError::new(
                LockErrorKind::InvalidInput,
                "namespace and limit must be non-empty",
            ));
        }
        selected::locking::SlotPermit::try_acquire(directory, namespace, limit)
            .map(|guard| Self { _guard: guard })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_limit_is_process_safe_and_released_on_drop() {
        let directory = std::env::temp_dir().join(format!(
            "agenterm-platform-lock-test-{}",
            std::process::id()
        ));
        let first = SlotPermit::try_acquire(&directory, "unit", 1).expect("first slot");
        let second = match SlotPermit::try_acquire(&directory, "unit", 1) {
            Ok(_) => panic!("slot should be contended"),
            Err(error) => error,
        };
        assert_eq!(second.kind(), LockErrorKind::Contended);
        drop(first);
        SlotPermit::try_acquire(&directory, "unit", 1).expect("released slot");
    }

    #[test]
    fn invalid_slot_configuration_is_typed() {
        let error = match SlotPermit::try_acquire(Path::new("."), "", 0) {
            Ok(_) => panic!("empty slot configuration should fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), LockErrorKind::InvalidInput);
    }

    #[cfg(windows)]
    #[test]
    fn path_aliases_share_one_lock_identity() {
        let directory = std::env::temp_dir().join(format!(
            "agenterm-platform-path-alias-{}",
            std::process::id()
        ));
        let child = directory.join("child");
        std::fs::create_dir_all(&child).expect("create alias fixture");
        let canonical = directory.join("state.lock");
        let alias = child.join("..").join("STATE.LOCK");

        let first = PathLock::acquire(&canonical).expect("acquire canonical path");
        let error = match PathLock::try_acquire(&alias) {
            Ok(_) => panic!("path alias bypassed the existing lock"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), LockErrorKind::Contended);
        drop(first);
        PathLock::try_acquire(&alias).expect("alias is released with the canonical guard");
        std::fs::remove_dir_all(directory).expect("remove alias fixture");
    }
}
