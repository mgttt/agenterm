//! OS-neutral contracts for Script worker supervision and audit coordination.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SupervisorAuditErrorKind {
    LockOpen,
    LockWait,
    ProcessTree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SupervisorAuditError {
    pub(crate) kind: SupervisorAuditErrorKind,
    pub(crate) message: String,
}

impl SupervisorAuditError {
    pub(crate) fn new(kind: SupervisorAuditErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
