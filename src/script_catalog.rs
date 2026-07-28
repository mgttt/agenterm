use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    operations::{OperationSpec, operation_by_id},
    script_protocol::{
        SCRIPT_API_VERSION, SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION,
        SCRIPT_INVOCATION_MAX_BYTES, ScriptBudgets,
    },
};

pub const SCRIPT_CATALOG_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptApiStatus {
    Shipped,
    Planned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptApiStability {
    Stable,
    Reserved,
    Legacy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustMapping {
    Direct,
    Adapted,
    Inspired,
    None,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScriptApiEntry {
    pub stable_id: &'static str,
    pub catalog_path: &'static str,
    pub surface_path: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_path: Option<&'static str>,
    pub rust_mapping: RustMapping,
    pub semantic_differences: &'static [&'static str],
    pub status: ScriptApiStatus,
    pub stability: ScriptApiStability,
    pub designed_on: &'static str,
    pub since: &'static str,
    pub profiles: &'static [&'static str],
    pub signature: &'static str,
    pub kind: &'static str,
    pub authority: &'static str,
    pub side_effects: &'static [&'static str],
    pub execution: &'static str,
    pub cancellation: &'static str,
    pub errors: &'static [&'static str],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<&'static OperationSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_reason: Option<&'static str>,
}

const SHIPPED_PROFILES: &[&str] = &["pure", "observe", "local"];
const OBSERVE_PROFILE: &[&str] = &["observe"];
const LOCAL_PROFILE: &[&str] = &["local"];
const NO_STRINGS: &[&str] = &[];
const BROKER_ERRORS: &[&str] = &["broker_host_error", "broker_transport"];

pub fn entries() -> Vec<ScriptApiEntry> {
    let mut entries = vec![ScriptApiEntry {
        stable_id: "rhai.print",
        catalog_path: "runtime/output/print",
        surface_path: "print",
        rust_path: None,
        rust_mapping: RustMapping::None,
        semantic_differences: &["output is captured and bounded by the invocation"],
        status: ScriptApiStatus::Shipped,
        stability: ScriptApiStability::Stable,
        designed_on: "2026-07-28",
        since: "script-api-v1",
        profiles: SHIPPED_PROFILES,
        signature: "print(value)",
        kind: "rhai_builtin",
        authority: "none",
        side_effects: &["captured_output"],
        execution: "sync",
        cancellation: "between_rhai_operations",
        errors: &["limit_output_bytes"],
        result: None,
        operation_id: None,
        operation: None,
        availability_reason: None,
    }];

    entries.extend([
        broker_entry(
            "fleet.workspace",
            "fleet/workspace/get",
            "agent.workspace",
            "agent.workspace()",
            "workspace.info",
            "workspace_metadata_with_event_position",
            BROKER_ERRORS,
        ),
        broker_entry(
            "fleet.tabs",
            "fleet/tabs/list",
            "agent.tabs",
            "agent.tabs()",
            "tabs.list",
            "tab_list",
            BROKER_ERRORS,
        ),
        broker_entry(
            "fleet.active-tab",
            "fleet/tabs/active",
            "agent.active_tab",
            "agent.active_tab()",
            "tabs.active",
            "tab_or_null",
            BROKER_ERRORS,
        ),
        broker_entry(
            "fleet.ui-snapshot",
            "fleet/ui/snapshot",
            "agent.ui_snapshot",
            "agent.ui_snapshot()",
            "ui.snapshot",
            "ui_snapshot",
            &[
                "broker_host_error",
                "broker_transport",
                "broker_return_too_large",
            ],
        ),
        broker_entry(
            "fleet.pane-capture",
            "fleet/pane/capture",
            "agent.capture",
            "agent.capture(tab, max_bytes)",
            "pane.capture",
            "bounded_capture",
            &[
                "broker_invalid_arguments",
                "broker_host_error",
                "broker_return_too_large",
            ],
        ),
        broker_entry(
            "fleet.events-read",
            "fleet/events/read",
            "agent.events_read",
            "agent.events_read(epoch, after, limit)",
            "events.read",
            "event_batch",
            &[
                "server_restart",
                "journal_gap",
                "future_sequence",
                "broker_invalid_arguments",
            ],
        ),
        broker_entry(
            "fleet.events-wait",
            "fleet/events/wait",
            "agent.events_wait",
            "agent.events_wait(epoch, after, kind, timeout_ms)",
            "events.wait",
            "event",
            &[
                "server_restart",
                "journal_gap",
                "future_sequence",
                "event_wait_timeout",
            ],
        ),
    ]);

    entries.extend([
        shipped_local_entry(
            "std.fs.read-to-string",
            "system/filesystem/read-text",
            "std::fs::read_to_string",
            Some("std::fs::read_to_string"),
            RustMapping::Adapted,
            "std::fs::read_to_string(path)",
            (&["filesystem_read"], &["fs_read_to_string"]),
        ),
        shipped_local_entry(
            "std.fs.read",
            "system/filesystem/read-bytes",
            "std::fs::read",
            Some("std::fs::read"),
            RustMapping::Adapted,
            "std::fs::read(path)",
            (&["filesystem_read"], &["fs_read"]),
        ),
        shipped_local_entry(
            "std.fs.write",
            "system/filesystem/write-text",
            "std::fs::write",
            Some("std::fs::write"),
            RustMapping::Adapted,
            "std::fs::write(path, text)",
            (&["filesystem_write"], &["fs_write"]),
        ),
        shipped_local_entry(
            "std.fs.write-bytes",
            "system/filesystem/write-bytes",
            "std::fs::write_bytes",
            Some("std::fs::write"),
            RustMapping::Adapted,
            "std::fs::write_bytes(path, bytes)",
            (&["filesystem_write"], &["fs_write"]),
        ),
        shipped_local_entry(
            "std.fs.exists",
            "system/filesystem/exists",
            "std::fs::exists",
            Some("std::path::Path::exists"),
            RustMapping::Adapted,
            "std::fs::exists(path)",
            (&["filesystem_metadata"], NO_STRINGS),
        ),
        shipped_local_entry(
            "std.path.path-buf",
            "data/path/path-buf",
            "std::path::PathBuf::from",
            Some("std::path::PathBuf::from"),
            RustMapping::Direct,
            "std::path::PathBuf::from(value)",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "std.path.join",
            "data/path/join",
            "std::path::join",
            Some("std::path::Path::join"),
            RustMapping::Adapted,
            "std::path::join(parent, child)",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "std.path.path-buf-join",
            "data/path/path-buf/join",
            "PathBuf.join",
            Some("std::path::PathBuf::push"),
            RustMapping::Adapted,
            "path.join(child)",
            (&["receiver_mutation"], NO_STRINGS),
        ),
        shipped_local_entry(
            "std.path.path-buf-display",
            "data/path/path-buf/display",
            "PathBuf.display",
            Some("std::path::Path::display"),
            RustMapping::Adapted,
            "path.display",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "std.path.path-buf-file-name",
            "data/path/path-buf/file-name",
            "PathBuf.file_name",
            Some("std::path::Path::file_name"),
            RustMapping::Adapted,
            "path.file_name",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "std.path.path-buf-extension",
            "data/path/path-buf/extension",
            "PathBuf.extension",
            Some("std::path::Path::extension"),
            RustMapping::Adapted,
            "path.extension",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "std.path.path-buf-is-absolute",
            "data/path/path-buf/is-absolute",
            "PathBuf.is_absolute",
            Some("std::path::Path::is_absolute"),
            RustMapping::Adapted,
            "path.is_absolute",
            (NO_STRINGS, NO_STRINGS),
        ),
        planned_entry(
            "std.process.command",
            "system/process/command",
            "std::process::command",
            Some("std::process::Command::new"),
            RustMapping::Adapted,
            "std::process::command(program)",
            "process API is not shipped yet",
        ),
        shipped_local_entry(
            "rhai.json.parse",
            "data/json/parse",
            "rhai::json::parse",
            None,
            RustMapping::None,
            "rhai::json::parse(text)",
            (NO_STRINGS, &["json_parse", "json_dynamic"]),
        ),
        shipped_local_entry(
            "rhai.json.stringify",
            "data/json/stringify",
            "rhai::json::stringify",
            None,
            RustMapping::None,
            "rhai::json::stringify(value)",
            (NO_STRINGS, &["json_value", "json_stringify"]),
        ),
        shipped_local_entry(
            "rhai.json.stringify-pretty",
            "data/json/stringify-pretty",
            "rhai::json::stringify_pretty",
            None,
            RustMapping::None,
            "rhai::json::stringify_pretty(value)",
            (NO_STRINGS, &["json_value", "json_stringify"]),
        ),
        shipped_local_entry(
            "rhai.bytes.from-text",
            "data/bytes/from-text",
            "rhai::bytes::from_text",
            None,
            RustMapping::None,
            "rhai::bytes::from_text(text)",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "rhai.bytes.length",
            "data/bytes/length",
            "Bytes.len",
            None,
            RustMapping::None,
            "bytes.len",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "rhai.bytes.to-text",
            "data/bytes/to-text",
            "Bytes.to_text",
            None,
            RustMapping::None,
            "bytes.to_text()",
            (NO_STRINGS, &["bytes_invalid_utf8"]),
        ),
        planned_entry(
            "rhai.task.start",
            "runtime/task/start",
            "rhai::task::start",
            None,
            RustMapping::None,
            "rhai::task::start(fn)",
            "task runtime is not shipped yet",
        ),
        planned_entry(
            "fleet.tabs.new",
            "fleet/tabs/new",
            "fleet.tabs.new",
            None,
            RustMapping::None,
            "fleet.tabs.new(options)",
            "Fleet control API is not shipped yet",
        ),
    ]);
    entries
}

pub fn catalog() -> Value {
    let defaults = ScriptBudgets::default();
    let hard_limits = ScriptBudgets::hard_limits();
    json!({
        "schema_version": SCRIPT_CATALOG_SCHEMA_VERSION,
        "api_version": SCRIPT_API_VERSION,
        "default_profile": "local",
        "model": "rhai_language + rust_shaped_std_subset + rhai_native_extensions + agenterm_fleet",
        "profiles": {
            "pure": {
                "status": "shipped",
                "variables": ["args"],
                "ambient_authority": [],
            },
            "observe": {
                "status": "shipped",
                "variables": ["args", "agent"],
                "ambient_authority": [],
            },
            "local": {
                "status": "shipped",
                "variables": ["args"],
                "ambient_authority": ["ordinary_local_program"],
                "availability": "first_std_slice",
            },
        },
        "operations": ["api", "check", "eval", "run"],
        "framing": {
            "version": SCRIPT_FRAME_VERSION,
            "max_frame_bytes": SCRIPT_FRAME_MAX_BYTES,
            "mode": "--framed-worker",
            "input_kinds": {
                "invoke": "available",
                "cancel": "available",
                "result": "worker_output_only",
                "broker_request": "available_worker_to_host",
                "broker_response": "available_host_to_worker",
            },
        },
        "supervisor": {
            "transport": "inherited_length_bounded_frames",
            "job_object": "kill_on_close",
            "cancel_grace_ms": 150,
            "per_process_concurrency": 2,
            "global_concurrency": 4,
        },
        "limits": {
            "defaults": defaults,
            "hard_maximums": hard_limits,
            "invocation_bytes": SCRIPT_INVOCATION_MAX_BYTES,
        },
        "entries": entries(),
        "failure_categories": ["configuration", "limit", "script", "protocol", "host"],
        "exit_classes": {
            "success": 0,
            "script": 1,
            "protocol": 1,
            "host": 1,
            "configuration": 2,
            "limit": 3,
        },
    })
}

fn broker_entry(
    stable_id: &'static str,
    catalog_path: &'static str,
    surface_path: &'static str,
    signature: &'static str,
    operation_id: &'static str,
    result: &'static str,
    errors: &'static [&'static str],
) -> ScriptApiEntry {
    let operation = operation_by_id(operation_id);
    let available = operation.is_some_and(|operation| operation.available);
    ScriptApiEntry {
        stable_id,
        catalog_path,
        surface_path,
        rust_path: None,
        rust_mapping: RustMapping::None,
        semantic_differences: &["AgenTerm-specific brokered observation"],
        status: if available {
            ScriptApiStatus::Shipped
        } else {
            ScriptApiStatus::Planned
        },
        stability: ScriptApiStability::Legacy,
        designed_on: "2026-07-28",
        since: "script-api-v1",
        profiles: if available {
            OBSERVE_PROFILE
        } else {
            NO_STRINGS
        },
        signature,
        kind: "brokered_method",
        authority: "observe",
        side_effects: NO_STRINGS,
        execution: "sync",
        cancellation: "host_deadline_and_broker_wait",
        errors,
        result: Some(result),
        operation_id: Some(operation_id),
        operation,
        availability_reason: (!available).then_some("backing operation is unavailable"),
    }
}

fn planned_entry(
    stable_id: &'static str,
    catalog_path: &'static str,
    surface_path: &'static str,
    rust_path: Option<&'static str>,
    rust_mapping: RustMapping,
    signature: &'static str,
    reason: &'static str,
) -> ScriptApiEntry {
    ScriptApiEntry {
        stable_id,
        catalog_path,
        surface_path,
        rust_path,
        rust_mapping,
        semantic_differences: &["planned surface; runtime semantics are not frozen"],
        status: ScriptApiStatus::Planned,
        stability: ScriptApiStability::Reserved,
        designed_on: "2026-07-28",
        since: "planned-v0.1.9",
        profiles: LOCAL_PROFILE,
        signature,
        kind: "planned",
        authority: "local",
        side_effects: NO_STRINGS,
        execution: "sync",
        cancellation: "not_shipped",
        errors: NO_STRINGS,
        result: None,
        operation_id: None,
        operation: None,
        availability_reason: Some(reason),
    }
}

fn shipped_local_entry(
    stable_id: &'static str,
    catalog_path: &'static str,
    surface_path: &'static str,
    rust_path: Option<&'static str>,
    rust_mapping: RustMapping,
    signature: &'static str,
    behavior: (&'static [&'static str], &'static [&'static str]),
) -> ScriptApiEntry {
    ScriptApiEntry {
        stable_id,
        catalog_path,
        surface_path,
        rust_path,
        rust_mapping,
        semantic_differences: &[
            "blocking call inside one supervised worker invocation",
            "errors use stable AgenTerm codes rather than Rust io::Error values",
        ],
        status: ScriptApiStatus::Shipped,
        stability: ScriptApiStability::Stable,
        designed_on: "2026-07-28",
        since: "0.1.9",
        profiles: LOCAL_PROFILE,
        signature,
        kind: "native_function",
        authority: "local",
        side_effects: behavior.0,
        execution: "sync",
        cancellation: "between_native_calls",
        errors: behavior.1,
        result: None,
        operation_id: None,
        operation: None,
        availability_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn catalog_entries_have_unique_identity_and_paths() {
        let entries = entries();
        let mut ids = HashSet::new();
        let mut surfaces = HashSet::new();
        for entry in &entries {
            assert!(ids.insert(entry.stable_id), "duplicate {}", entry.stable_id);
            assert!(
                surfaces.insert(entry.surface_path),
                "duplicate {}",
                entry.surface_path
            );
            assert!(!entry.catalog_path.is_empty());
            assert!(!entry.signature.is_empty());
            assert!(!entry.semantic_differences.is_empty());
            assert_eq!(entry.designed_on, "2026-07-28");
            assert!(!entry.since.is_empty());
            if entry.status == ScriptApiStatus::Planned {
                assert!(entry.availability_reason.is_some());
                assert_eq!(entry.stability, ScriptApiStability::Reserved);
            }
        }
    }

    #[test]
    fn shipped_broker_entries_resolve_to_available_operations() {
        for entry in entries().into_iter().filter(|entry| {
            entry.status == ScriptApiStatus::Shipped && entry.operation_id.is_some()
        }) {
            assert!(
                operation_by_id(entry.operation_id.unwrap())
                    .is_some_and(|operation| operation.available),
                "{} has no available operation",
                entry.stable_id
            );
        }
    }

    #[test]
    fn public_runtime_spec_starts_with_the_english_dated_object_tree() {
        let specification = include_str!("../docs/agenterm-script-runtime.md");
        assert!(specification.starts_with("# AgenTerm Script Runtime Specification"));
        assert!(specification.contains("## 1. Complete public object and interface tree"));
        assert!(specification.matches("designed 2026-07-28").count() >= 60);
        assert!(specification.contains("The Rhai surface is the product contract."));
        assert!(
            !specification
                .chars()
                .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)),
            "the international runtime specification must remain English"
        );
    }
}
