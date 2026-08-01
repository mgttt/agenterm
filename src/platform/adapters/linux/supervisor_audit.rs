//! Linux AgenTerm audit-path policy.

use std::path::PathBuf;

use crate::platform::contract::supervisor_audit::{SupervisorAuditError, SupervisorAuditErrorKind};

pub(crate) fn process_tree_error(message: String) -> SupervisorAuditError {
    SupervisorAuditError::new(SupervisorAuditErrorKind::ProcessTree, message)
}

pub(crate) fn default_audit_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(path)
            .join("agenterm")
            .join("script-audit.jsonl")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("agenterm")
            .join("script-audit.jsonl")
    } else {
        std::env::temp_dir().join("agenterm-script-audit.jsonl")
    }
}
