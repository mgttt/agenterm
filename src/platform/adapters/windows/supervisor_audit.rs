//! Windows AgenTerm audit-path policy.

use std::path::PathBuf;

use crate::platform::contract::supervisor_audit::{SupervisorAuditError, SupervisorAuditErrorKind};

pub(crate) fn process_tree_error(message: String) -> SupervisorAuditError {
    SupervisorAuditError::new(SupervisorAuditErrorKind::ProcessTree, message)
}

pub(crate) fn default_audit_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("AgenTerm")
        .join("script-audit.jsonl")
}
