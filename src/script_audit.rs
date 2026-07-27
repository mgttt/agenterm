use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    ptr,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
    System::Threading::{CreateMutexW, INFINITE, ReleaseMutex, WaitForSingleObject},
};

const AUDIT_SCHEMA_VERSION: u32 = 1;
const MAX_RECORD_BYTES: usize = 16 * 1024;
const MAX_ACTIVE_BYTES: u64 = 4 * 1024 * 1024;
static PROCESS_AUDIT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AuditBudgets {
    pub(crate) source_bytes: usize,
    pub(crate) operations: u64,
    pub(crate) call_depth: usize,
    pub(crate) expression_depth: usize,
    pub(crate) collection_items: usize,
    pub(crate) string_bytes: usize,
    pub(crate) output_bytes: usize,
    pub(crate) wall_time_ms: u64,
    pub(crate) broker_requests: usize,
    pub(crate) broker_return_bytes: usize,
    pub(crate) capture_bytes: usize,
    pub(crate) event_items: usize,
    pub(crate) wait_time_ms: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuditSourceKind {
    Api,
    Eval,
    Stdin,
    File,
}

#[derive(Clone, Debug)]
pub(crate) struct AuditInvocation {
    pub(crate) invocation_id: String,
    pub(crate) source_fingerprint: String,
    pub(crate) source_kind: AuditSourceKind,
    pub(crate) api_version: u32,
    pub(crate) operation: String,
    pub(crate) requested_profile: String,
    pub(crate) effective_profile: String,
    pub(crate) requested_capabilities: Vec<String>,
    pub(crate) effective_capabilities: Vec<String>,
    pub(crate) requested_budgets: AuditBudgets,
    pub(crate) effective_budgets: AuditBudgets,
    pub(crate) broker_operation_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct AuditOutcome {
    pub(crate) duration_ms: u64,
    pub(crate) result_class: String,
    pub(crate) failure_code: Option<String>,
    pub(crate) denied: bool,
    pub(crate) cancelled: bool,
    pub(crate) timed_out: bool,
    pub(crate) crashed: bool,
}

#[derive(Serialize)]
struct AuditRecord<'a> {
    schema_version: u32,
    timestamp_unix_ms: u64,
    invocation_id: &'a str,
    source_fingerprint: &'a str,
    source_kind: AuditSourceKind,
    api_version: u32,
    operation: &'a str,
    requested_profile: &'a str,
    effective_profile: &'a str,
    requested_capabilities: &'a [String],
    effective_capabilities: &'a [String],
    requested_budgets: &'a AuditBudgets,
    effective_budgets: &'a AuditBudgets,
    broker_operation_ids: &'a [String],
    duration_ms: u64,
    result_class: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_code: Option<&'a str>,
    denied: bool,
    cancelled: bool,
    timed_out: bool,
    crashed: bool,
}

pub(crate) struct ScriptAuditSink {
    path: PathBuf,
}

impl ScriptAuditSink {
    pub(crate) fn discover() -> Result<Self, String> {
        let path = match env::var_os("AGENTERM_SCRIPT_AUDIT_PATH") {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => {
                let base = env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(env::temp_dir);
                base.join("AgenTerm").join("script-audit.jsonl")
            }
        };
        let path = if path.is_absolute() {
            path
        } else {
            env::current_dir()
                .map_err(|error| format!("failed to resolve script audit path: {error}"))?
                .join(path)
        };
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create script audit directory: {error}"))?;
        }
        Ok(Self { path })
    }

    pub(crate) fn append(
        &self,
        invocation: &AuditInvocation,
        outcome: &AuditOutcome,
    ) -> Result<(), String> {
        let record = AuditRecord {
            schema_version: AUDIT_SCHEMA_VERSION,
            timestamp_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            invocation_id: &invocation.invocation_id,
            source_fingerprint: &invocation.source_fingerprint,
            source_kind: invocation.source_kind,
            api_version: invocation.api_version,
            operation: &invocation.operation,
            requested_profile: &invocation.requested_profile,
            effective_profile: &invocation.effective_profile,
            requested_capabilities: &invocation.requested_capabilities,
            effective_capabilities: &invocation.effective_capabilities,
            requested_budgets: &invocation.requested_budgets,
            effective_budgets: &invocation.effective_budgets,
            broker_operation_ids: &invocation.broker_operation_ids,
            duration_ms: outcome.duration_ms,
            result_class: &outcome.result_class,
            failure_code: outcome.failure_code.as_deref(),
            denied: outcome.denied,
            cancelled: outcome.cancelled,
            timed_out: outcome.timed_out,
            crashed: outcome.crashed,
        };
        let mut bytes = serde_json::to_vec(&record)
            .map_err(|error| format!("failed to encode script audit record: {error}"))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(format!(
                "script audit record exceeds the {MAX_RECORD_BYTES} byte limit"
            ));
        }
        self.append_bounded(&bytes, MAX_ACTIVE_BYTES)
    }

    fn append_bounded(&self, bytes: &[u8], max_active_bytes: u64) -> Result<(), String> {
        let _process_guard = PROCESS_AUDIT_LOCK
            .lock()
            .map_err(|_| "script audit process lock is poisoned".to_owned())?;
        let _global_guard = NamedAuditLock::acquire(&self.path)?;
        let current_bytes = fs::metadata(&self.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if current_bytes.saturating_add(bytes.len() as u64) > max_active_bytes {
            let backup = backup_path(&self.path);
            match fs::remove_file(&backup) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "failed to remove rotated script audit file: {error}"
                    ));
                }
            }
            if self.path.exists() {
                fs::rename(&self.path, &backup)
                    .map_err(|error| format!("failed to rotate script audit file: {error}"))?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| format!("failed to open script audit file: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("failed to append script audit record: {error}"))?;
        file.flush()
            .map_err(|error| format!("failed to flush script audit record: {error}"))
    }
}

pub(crate) fn source_fingerprint(source: &str) -> String {
    let first = fnv1a64(source.as_bytes(), 0xcbf29ce484222325);
    let second = fnv1a64(source.as_bytes(), 0x84222325cbf29ce4);
    format!("fnv1a128:{first:016x}{second:016x}")
}

fn fnv1a64(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn backup_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".1");
    PathBuf::from(value)
}

struct NamedAuditLock(HANDLE);

impl NamedAuditLock {
    fn acquire(path: &Path) -> Result<Self, String> {
        let identity = source_fingerprint(&path.to_string_lossy().to_ascii_lowercase());
        let mut name: Vec<u16> = format!("Local\\AgenTermScriptAudit-{}", &identity[9..])
            .encode_utf16()
            .collect();
        name.push(0);
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "CreateMutexW for script audit failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            unsafe { CloseHandle(handle) };
            return Err(format!("script audit mutex wait failed: {wait}"));
        }
        Ok(Self(handle))
    }
}

impl Drop for NamedAuditLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation(secret: &str) -> AuditInvocation {
        let budgets = AuditBudgets {
            source_bytes: 1024,
            operations: 100,
            call_depth: 8,
            expression_depth: 8,
            collection_items: 32,
            string_bytes: 1024,
            output_bytes: 1024,
            wall_time_ms: 100,
            broker_requests: 4,
            broker_return_bytes: 4096,
            capture_bytes: 1024,
            event_items: 16,
            wait_time_ms: 100,
        };
        AuditInvocation {
            invocation_id: "audit-unit".to_owned(),
            source_fingerprint: source_fingerprint(secret),
            source_kind: AuditSourceKind::Eval,
            api_version: 1,
            operation: "eval".to_owned(),
            requested_profile: "pure".to_owned(),
            effective_profile: "pure".to_owned(),
            requested_capabilities: Vec::new(),
            effective_capabilities: Vec::new(),
            requested_budgets: budgets.clone(),
            effective_budgets: budgets,
            broker_operation_ids: Vec::new(),
        }
    }

    fn outcome() -> AuditOutcome {
        AuditOutcome {
            duration_ms: 12,
            result_class: "success".to_owned(),
            failure_code: None,
            denied: false,
            cancelled: false,
            timed_out: false,
            crashed: false,
        }
    }

    #[test]
    fn serialized_record_contains_metadata_but_not_source_or_forbidden_fields() {
        let secret = "AUDIT_SOURCE_SECRET";
        let invocation = invocation(secret);
        let record = AuditRecord {
            schema_version: AUDIT_SCHEMA_VERSION,
            timestamp_unix_ms: 1,
            invocation_id: &invocation.invocation_id,
            source_fingerprint: &invocation.source_fingerprint,
            source_kind: invocation.source_kind,
            api_version: invocation.api_version,
            operation: &invocation.operation,
            requested_profile: &invocation.requested_profile,
            effective_profile: &invocation.effective_profile,
            requested_capabilities: &invocation.requested_capabilities,
            effective_capabilities: &invocation.effective_capabilities,
            requested_budgets: &invocation.requested_budgets,
            effective_budgets: &invocation.effective_budgets,
            broker_operation_ids: &invocation.broker_operation_ids,
            duration_ms: 12,
            result_class: "success",
            failure_code: None,
            denied: false,
            cancelled: false,
            timed_out: false,
            crashed: false,
        };
        let json = serde_json::to_string(&record).expect("record JSON");
        assert!(!json.contains(secret));
        for forbidden in [
            "\"source\"",
            "\"arguments\"",
            "\"argv\"",
            "\"stdout\"",
            "\"pane\"",
            "\"environment\"",
            "\"clipboard\"",
            "\"credentials\"",
        ] {
            assert!(!json.contains(forbidden), "{forbidden} leaked");
        }
        assert!(json.contains("\"source_fingerprint\""));
    }

    #[test]
    fn append_rotates_at_the_bound_and_keeps_jsonl_records_whole() {
        let directory = env::temp_dir().join(format!(
            "agenterm-audit-unit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).expect("temp audit directory");
        let path = directory.join("audit.jsonl");
        let sink = ScriptAuditSink { path: path.clone() };
        sink.append_bounded(b"{\"one\":1}\n", 15)
            .expect("first append");
        sink.append_bounded(b"{\"two\":2}\n", 15)
            .expect("rotating append");
        assert_eq!(
            fs::read_to_string(&path).expect("active audit"),
            "{\"two\":2}\n"
        );
        assert_eq!(
            fs::read_to_string(backup_path(&path)).expect("backup audit"),
            "{\"one\":1}\n"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn append_writes_one_complete_record() {
        let directory =
            env::temp_dir().join(format!("agenterm-audit-record-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("temp audit directory");
        let path = directory.join("audit.jsonl");
        let sink = ScriptAuditSink { path: path.clone() };
        sink.append(&invocation("secret"), &outcome())
            .expect("audit append");
        let lines: Vec<_> = fs::read_to_string(&path)
            .expect("audit file")
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(lines.len(), 1);
        assert!(serde_json::from_str::<serde_json::Value>(&lines[0]).is_ok());
        let _ = fs::remove_dir_all(directory);
    }
}
