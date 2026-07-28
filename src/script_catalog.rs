use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    operations::{OperationSpec, operation_by_id},
    script_protocol::{
        SCRIPT_API_VERSION, SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION,
        SCRIPT_INVOCATION_MAX_BYTES, ScriptBudgets,
    },
    script_stream::{STREAM_BUFFER_BYTES, STREAM_READ_MAX_BYTES},
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
            "std.fs.metadata",
            "system/filesystem/metadata",
            "std::fs::metadata",
            Some("std::fs::metadata"),
            RustMapping::Adapted,
            "std::fs::metadata(path)",
            (&["filesystem_metadata"], &["fs_metadata"]),
        ),
        shipped_local_entry(
            "std.fs.read-dir",
            "system/filesystem/read-directory",
            "std::fs::read_dir",
            Some("std::fs::read_dir"),
            RustMapping::Adapted,
            "std::fs::read_dir(path)",
            (
                &["filesystem_read", "directory_enumeration"],
                &["fs_read_dir"],
            ),
        ),
        shipped_local_entry(
            "std.fs.dir-entry-path",
            "system/filesystem/dir-entry/path",
            "DirEntry.path",
            Some("std::fs::DirEntry::path"),
            RustMapping::Direct,
            "entry.path",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "std.fs.dir-entry-file-name",
            "system/filesystem/dir-entry/file-name",
            "DirEntry.file_name",
            Some("std::fs::DirEntry::file_name"),
            RustMapping::Adapted,
            "entry.file_name",
            (&["lossy_windows_text"], NO_STRINGS),
        ),
        shipped_local_entry(
            "std.fs.dir-entry-types",
            "system/filesystem/dir-entry/types",
            "DirEntry.is_file/is_dir/is_symlink",
            Some("std::fs::FileType"),
            RustMapping::Adapted,
            "entry.is_file / entry.is_dir / entry.is_symlink",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry(
            "std.fs.dir-entry-metadata",
            "system/filesystem/dir-entry/metadata",
            "DirEntry.metadata",
            Some("std::fs::DirEntry::metadata"),
            RustMapping::Adapted,
            "entry.metadata",
            (&["follows_symlinks"], &["fs_dir_entry_metadata"]),
        ),
        shipped_local_entry(
            "std.fs.metadata-facts",
            "system/filesystem/metadata/facts",
            "Metadata.is_file/is_dir/len/modified",
            Some("std::fs::Metadata"),
            RustMapping::Adapted,
            "metadata.is_file / metadata.is_dir / metadata.len / metadata.modified",
            (
                &["integer_bounded_length"],
                &["filesystem_metadata_overflow"],
            ),
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
            "std.path.absolute",
            "data/path/absolute",
            "std::path::absolute",
            Some("std::path::absolute"),
            RustMapping::Adapted,
            "std::path::absolute(path)",
            (&["current_directory_resolution"], &["path_absolute"]),
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
        shipped_local_entry(
            "std.time.system-time-now",
            "system/time/system-time/now",
            "std::time::SystemTime::now",
            Some("std::time::SystemTime::now"),
            RustMapping::Direct,
            "std::time::SystemTime::now()",
            (&["wall_clock"], NO_STRINGS),
        ),
        shipped_local_entry(
            "std.time.system-time-unix-millis",
            "system/time/system-time/unix-millis",
            "SystemTime.unix_millis",
            Some("std::time::SystemTime::duration_since"),
            RustMapping::Adapted,
            "time.unix_millis",
            (
                &["unix_epoch_milliseconds"],
                &["system_time_before_unix_epoch"],
            ),
        ),
        shipped_local_entry(
            "std.time.system-time-rfc3339",
            "system/time/system-time/rfc3339",
            "SystemTime.rfc3339",
            Some("std::time::SystemTime"),
            RustMapping::Adapted,
            "time.rfc3339",
            (&["utc_rfc3339_millisecond_precision"], NO_STRINGS),
        ),
        shipped_local_entry_with_semantics(
            shipped_local_entry(
            "std.env.get",
            "system/environment/read",
            "std::env::get",
            Some("std::env::var"),
            RustMapping::Adapted,
            "std::env::get(name)",
            (
                &[
                    "var_is_a_rhai_reserved_word",
                    "worker_environment_snapshot",
                    "value_not_audited",
                ],
                &["environment_missing", "environment_not_unicode"],
            ),
            ),
            &["std::env::var is exposed as get because var is Rhai-reserved"],
        ),
        shipped_local_entry(
            "std.env.has",
            "system/environment/has",
            "std::env::has",
            Some("std::env::var_os"),
            RustMapping::Adapted,
            "std::env::has(name)",
            (&["worker_environment_snapshot"], &["environment_name_invalid"]),
        ),
        shipped_local_entry(
            "std.env.names",
            "system/environment/names",
            "std::env::names",
            Some("std::env::vars_os"),
            RustMapping::Adapted,
            "std::env::names()",
            (&["values_are_not_returned", "case_insensitive_deduplication"], NO_STRINGS),
        ),
        shipped_local_entry(
            "std.env.current-dir",
            "system/environment/current-directory",
            "std::env::current_dir",
            Some("std::env::current_dir"),
            RustMapping::Direct,
            "std::env::current_dir()",
            (NO_STRINGS, &["environment_current_dir"]),
        ),
        shipped_local_entry_with_semantics(
            shipped_local_entry(
                "std.process.command",
                "system/process/command",
                "std::process::command",
                Some("std::process::Command::new"),
                RustMapping::Adapted,
                "std::process::command(program)",
                (
                    &["new_is_a_rhai_reserved_word", "no_implicit_shell"],
                    &["process_program_empty"],
                ),
            ),
            &[
                "Command::new cannot be exposed because new is Rhai-reserved",
                "the host never inserts an implicit shell",
                "errors use stable AgenTerm codes rather than Rust io::Error values",
            ],
        ),
        shipped_local_entry(
            "std.process.command-builder",
            "system/process/command/builder",
            "Command.arg/args/current_dir/env/env_remove/env_clear/stdin_text/timeout/capture_limit",
            Some("std::process::Command"),
            RustMapping::Adapted,
            "command.arg(value) / command.args(values) / command.current_dir(path) / command.env(name, value)",
            (&["mutable_builder", "bounded_text_stdin", "invocation_owned"], &["process_argument", "environment_name_invalid"]),
        ),
        shipped_local_entry(
            "std.process.command-output",
            "system/process/command/output",
            "Command.output",
            Some("std::process::Command::output"),
            RustMapping::Adapted,
            "command.output()",
            (&["bounded_capture", "typed_timeout", "job_object_cleanup"], &["process_spawn", "process_timeout"]),
        ),
        shipped_local_entry_with_semantics(
            shipped_local_entry(
                "std.process.command-start",
                "system/process/command/start",
                "Command.start",
                Some("std::process::Command::spawn"),
                RustMapping::Adapted,
                "command.start()",
                (
                    &[
                        "spawn_is_a_rhai_reserved_word",
                        "invocation_owned",
                        "job_object_cleanup",
                    ],
                    &["process_spawn"],
                ),
            ),
            &[
                "Command::spawn is exposed as start because spawn is Rhai-reserved",
                "the Child is owned by one supervised invocation",
                "descendants inherit supervisor process-tree cleanup",
            ],
        ),
        shipped_local_entry(
            "std.process.child",
            "system/process/child",
            "Child.id/state/stdout/stderr/kill/wait_with_output",
            Some("std::process::Child"),
            RustMapping::Adapted,
            "child.id / child.state / child.stdout / child.stderr / child.kill() / child.wait_with_output([timeout])",
            (&["live_bounded_streams", "typed_timeout", "invocation_owned"], &["process_kill", "process_timeout"]),
        ),
        shipped_local_entry(
            "std.process.output",
            "system/process/output",
            "Output.success/exit_code/stdout/stderr/complete/truncated/stdout_text/stderr_text/error",
            Some("std::process::Output"),
            RustMapping::Adapted,
            "output.success / output.exit_code / output.stdout / output.stderr",
            (&["bytes_first_output", "strict_utf8_helpers", "truthful_truncation"], &["process_stdout_not_utf8", "process_stderr_not_utf8"]),
        ),
        shipped_local_entry(
            "rhai.stream.handle",
            "runtime/stream/handle",
            "Stream.id/kind/state/buffered_bytes/truncated/complete/read/collect/close",
            None,
            RustMapping::None,
            "stream.id / stream.kind / stream.state / stream.buffered_bytes / stream.truncated / stream.complete / stream.read(max_bytes[, timeout]) / stream.collect(max_bytes[, timeout]) / stream.close()",
            (
                &[
                    "bounded_queue_backpressure",
                    "bytes_first",
                    "truthful_truncation",
                    "invocation_owned",
                ],
                &[
                    "stream_read_timeout",
                    "stream_read_failed",
                    "stream_collect_limit",
                    "stream_closed",
                ],
            ),
        ),
        shipped_local_entry(
            "std.time.duration-from-millis",
            "system/time/duration/from-millis",
            "std::time::Duration::from_millis",
            Some("std::time::Duration::from_millis"),
            RustMapping::Adapted,
            "std::time::Duration::from_millis(value)",
            (&["maximum_10000_ms"], &["duration_millis"]),
        ),
        shipped_local_entry(
            "std.time.duration-from-secs",
            "system/time/duration/from-secs",
            "std::time::Duration::from_secs",
            Some("std::time::Duration::from_secs"),
            RustMapping::Adapted,
            "std::time::Duration::from_secs(value)",
            (&["maximum_10_seconds"], &["duration_seconds"]),
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
        shipped_local_entry(
            "rhai.task.after",
            "runtime/task/timer/after",
            "rhai::task::after",
            None,
            RustMapping::None,
            "rhai::task::after(duration)",
            (&["background_timer", "invocation_owned"], &["task_state_poisoned"]),
        ),
        shipped_local_entry(
            "rhai.task.sleep",
            "runtime/task/timer/sleep",
            "rhai::task::sleep",
            None,
            RustMapping::None,
            "rhai::task::sleep(duration)",
            (&["blocking_wait", "invocation_owned"], &["task_cancelled"]),
        ),
        shipped_local_entry(
            "rhai.task.wait-all",
            "runtime/task/composition/wait-all",
            "rhai::task::wait_all",
            None,
            RustMapping::None,
            "rhai::task::wait_all(tasks[, timeout])",
            (&["deterministic_input_order", "maximum_64_tasks"], &["task_wait_timeout", "task_cancelled"]),
        ),
        shipped_local_entry(
            "rhai.task.race",
            "runtime/task/composition/race",
            "rhai::task::race",
            None,
            RustMapping::None,
            "rhai::task::race(tasks[, timeout])",
            (&["returns_winning_index", "maximum_64_tasks"], &["task_race_empty", "task_wait_timeout"]),
        ),
        shipped_local_entry(
            "rhai.task.cancel-all",
            "runtime/task/composition/cancel-all",
            "rhai::task::cancel_all",
            None,
            RustMapping::None,
            "rhai::task::cancel_all(tasks)",
            (&["idempotent_cancellation", "maximum_64_tasks"], &["task_collection_type"]),
        ),
        shipped_local_entry(
            "rhai.task.handle",
            "runtime/task/handle",
            "Task.id/state/done/cancelled/wait/cancel",
            None,
            RustMapping::None,
            "task.id / task.state / task.done / task.cancelled / task.wait([timeout]) / task.cancel()",
            (&["typed_host_payload_only", "no_rhai_dynamic_cross_thread"], &["task_wait_timeout", "task_cancelled"]),
        ),
        shipped_local_entry(
            "runtime.project.module-import",
            "code-and-automation/module/import",
            "import \"relative/module\" as module",
            None,
            RustMapping::None,
            "import \"relative/module\" as module",
            (
                &[
                    "project_root_relative",
                    "rhai_extension_implicit",
                    "compiled_self_contained",
                ],
                &[
                    "script_module_missing",
                    "script_module_root_escape",
                    "script_module_cycle",
                ],
            ),
        ),
        shipped_local_entry(
            "runtime.project.named-task",
            "code-and-automation/task-manifest/invoke",
            "script task list/show/run",
            None,
            RustMapping::None,
            "script task list|show|run [TASK] [--manifest PATH]",
            (
                &[
                    "agenterm_tasks_json_schema_v1",
                    "invalid_entries_remain_visible",
                    "environment_names_only",
                ],
                &[
                    "task_manifest_version",
                    "task_degraded",
                    "task_environment_missing",
                ],
            ),
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
        "operations": ["api", "check", "eval", "run", "task-list", "task-show", "task-run"],
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
            "stream_buffer_bytes": STREAM_BUFFER_BYTES,
            "stream_read_max_bytes": STREAM_READ_MAX_BYTES,
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

#[allow(clippy::too_many_arguments)]
fn shipped_local_entry_with_semantics(
    mut entry: ScriptApiEntry,
    semantic_differences: &'static [&'static str],
) -> ScriptApiEntry {
    entry.semantic_differences = semantic_differences;
    entry
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
