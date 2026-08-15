//! Machine-readable audit records for authorized actuation (PRD_02_31).

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
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
    decision: &'a str,
    authority_scope: &'a str,
    outcome: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

pub struct AuditLog {
    path: PathBuf,
    file: File,
    #[cfg(test)]
    injected_failure: Option<InjectedAuditFailure>,
    #[cfg(test)]
    successful_records: usize,
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum InjectedAuditFailure {
    AppendAfter(usize),
    FlushAfter(usize),
}

impl AuditLog {
    pub fn open() -> Result<Self, CuError> {
        let path = std::env::var("AGENTERM_CU_AUDIT_PATH")
            .map(PathBuf::from)
            .or_else(|_| default_audit_path())
            .map_err(|error| CuError::new("audit_unavailable", error))?;
        Self::open_at(path)
    }

    pub(crate) fn open_at(path: impl AsRef<Path>) -> Result<Self, CuError> {
        let path = path.as_ref().to_path_buf();
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
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                CuError::new(
                    "audit_unavailable",
                    format!("could not open audit log {}: {error}", path.display()),
                )
            })?;
        Ok(Self {
            path,
            file,
            #[cfg(test)]
            injected_failure: None,
            #[cfg(test)]
            successful_records: 0,
        })
    }

    #[cfg(test)]
    pub(crate) fn inject_failure(&mut self, failure: InjectedAuditFailure) {
        self.injected_failure = Some(failure);
    }

    pub fn record_actuation(
        &mut self,
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
            decision: "authorized",
            authority_scope: "process",
            outcome,
            detail,
        };
        let line = serde_json::to_string(&record).map_err(|error| {
            CuError::new(
                "audit_unavailable",
                format!("audit serialization failed: {error}"),
            )
        })?;
        #[cfg(test)]
        if matches!(
            self.injected_failure,
            Some(InjectedAuditFailure::AppendAfter(records))
                if records == self.successful_records
        ) {
            return Err(CuError::new(
                "audit_unavailable",
                format!(
                    "could not append audit log {}: injected failure",
                    self.path.display()
                ),
            ));
        }
        writeln!(self.file, "{line}").map_err(|error| {
            CuError::new(
                "audit_unavailable",
                format!(
                    "could not append audit log {}: {error}",
                    self.path.display()
                ),
            )
        })?;
        #[cfg(test)]
        if matches!(
            self.injected_failure,
            Some(InjectedAuditFailure::FlushAfter(records))
                if records == self.successful_records
        ) {
            return Err(CuError::new(
                "audit_unavailable",
                format!(
                    "could not flush audit log {}: injected failure",
                    self.path.display()
                ),
            ));
        }
        self.file.flush().map_err(|error| {
            CuError::new(
                "audit_unavailable",
                format!("could not flush audit log {}: {error}", self.path.display()),
            )
        })?;
        #[cfg(test)]
        {
            self.successful_records += 1;
        }
        Ok(())
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
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{AuditLog, InjectedAuditFailure, default_audit_path};
    use crate::{auth::Grant, command::Command, target::TargetRef};

    static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

    fn scratch_path(label: &str) -> PathBuf {
        let sequence = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "agenterm-cu-audit-{label}-{}-{sequence}",
                std::process::id()
            ))
            .join("audit.jsonl")
    }

    fn command() -> Command {
        Command::WindowPlace {
            target: TargetRef::Current,
            action: "left-half".into(),
            window: None,
        }
    }

    fn remove_scratch(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }

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

    #[test]
    fn one_open_log_appends_and_flushes_both_records() {
        let path = scratch_path("same-handle");
        let mut audit = AuditLog::open_at(&path).expect("open isolated audit");
        audit
            .record_actuation(
                TargetRef::Current,
                &command(),
                Grant::Actuate,
                "attempt",
                None,
            )
            .expect("attempt record");
        audit
            .record_actuation(
                TargetRef::Current,
                &command(),
                Grant::Actuate,
                "ok",
                Some(serde_json::json!({"result": "placed"})),
            )
            .expect("outcome record");
        let records: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .expect("read audit")
            .lines()
            .map(|line| serde_json::from_str(line).expect("record JSON"))
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["outcome"], "attempt");
        assert_eq!(records[0]["decision"], "authorized");
        assert_eq!(records[0]["authority_scope"], "process");
        assert_eq!(records[1]["outcome"], "ok");
        drop(audit);
        remove_scratch(&path);
    }

    #[test]
    fn append_failure_is_typed() {
        let path = scratch_path("append-failure");
        let mut audit = AuditLog::open_at(&path).expect("open isolated audit");
        audit.inject_failure(InjectedAuditFailure::AppendAfter(0));
        let error = audit
            .record_actuation(
                TargetRef::Current,
                &command(),
                Grant::Actuate,
                "attempt",
                None,
            )
            .expect_err("append failure must fail closed");
        assert_eq!(error.code, "audit_unavailable");
        assert!(error.message.contains("append"));
        drop(audit);
        remove_scratch(&path);
    }

    #[test]
    fn flush_failure_is_typed() {
        let path = scratch_path("flush-failure");
        let mut audit = AuditLog::open_at(&path).expect("open isolated audit");
        audit.inject_failure(InjectedAuditFailure::FlushAfter(0));
        let error = audit
            .record_actuation(
                TargetRef::Current,
                &command(),
                Grant::Actuate,
                "attempt",
                None,
            )
            .expect_err("flush failure must fail closed");
        assert_eq!(error.code, "audit_unavailable");
        assert!(error.message.contains("flush"));
        drop(audit);
        remove_scratch(&path);
    }
}
