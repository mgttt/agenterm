//! Machine-readable audit records for authorized actuation (PRD_02_31).

use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{auth::Grant, command::Command, reply::CuError, target::TargetRef};

#[derive(Serialize)]
struct AuditRecord<'a> {
    ts_ms: u128,
    target: &'a str,
    verb: &'a str,
    grant: &'a str,
    outcome: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn open() -> Result<Self, CuError> {
        let path = std::env::var("AGENTERM_CU_AUDIT_PATH")
            .map(PathBuf::from)
            .or_else(|_| default_audit_path())
            .map_err(|error| CuError::new("audit_unavailable", error))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                CuError::new(
                    "audit_unavailable",
                    format!(
                        "could not create audit directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
        Ok(Self { path })
    }

    pub fn record_actuation(
        &self,
        target: TargetRef,
        command: &Command,
        grant: Grant,
        outcome: &str,
        detail: Option<serde_json::Value>,
    ) -> Result<(), CuError> {
        let record = AuditRecord {
            ts_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0),
            target: target.as_str(),
            verb: &command.verb(),
            grant: match grant {
                Grant::Observe => "observe",
                Grant::Actuate => "actuate",
            },
            outcome,
            detail,
        };
        let line = serde_json::to_string(&record).map_err(|error| {
            CuError::new(
                "audit_unavailable",
                format!("audit serialization failed: {error}"),
            )
        })?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| {
                CuError::new(
                    "audit_unavailable",
                    format!("could not open audit log {}: {error}", self.path.display()),
                )
            })?;
        writeln!(file, "{line}").map_err(|error| {
            CuError::new(
                "audit_unavailable",
                format!(
                    "could not append audit log {}: {error}",
                    self.path.display()
                ),
            )
        })?;
        file.flush().map_err(|error| {
            CuError::new(
                "audit_unavailable",
                format!("could not flush audit log {}: {error}", self.path.display()),
            )
        })
    }
}

fn default_audit_path() -> Result<PathBuf, String> {
    // Resolution order: AGENTERM_CU_AUDIT_PATH is handled by AuditLog::open();
    // here we fall back HOME -> USERPROFILE (the latter covers Windows, which
    // does not set HOME by default).
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| {
            if cfg!(windows) {
                "neither HOME nor USERPROFILE is set".to_owned()
            } else {
                "HOME is not set".to_owned()
            }
        })?;
    if cfg!(windows) {
        Ok(PathBuf::from(home)
            .join("AppData")
            .join("Local")
            .join("agenterm")
            .join("cu-audit.jsonl"))
    } else {
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("agenterm")
            .join("cu-audit.jsonl"))
    }
}

#[cfg(test)]
mod tests {
    use super::default_audit_path;

    #[test]
    fn default_audit_path_resolves_on_current_platform() {
        let path =
            default_audit_path().expect("default audit path must resolve on the current platform");
        assert!(!path.as_os_str().is_empty(), "path must not be empty");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("cu-audit.jsonl")
        );
    }
}
