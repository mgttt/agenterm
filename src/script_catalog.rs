use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    operations::{OPERATION_CATALOG, OperationClass, OperationSpec},
    script_http::{
        DEFAULT_HTTP_BODY_BYTES, DEFAULT_HTTP_REDIRECTS, DEFAULT_HTTP_TIMEOUT, MAX_HTTP_BODY_BYTES,
        MAX_HTTP_HEADER_BYTES, MAX_HTTP_HEADERS, MAX_HTTP_REDIRECTS, MAX_HTTP_REQUEST_BODY_BYTES,
        MAX_HTTP_TIMEOUT, MAX_HTTP_URL_BYTES,
    },
    script_protocol::{
        SCRIPT_API_VERSION, SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION,
        SCRIPT_INVOCATION_MAX_BYTES, ScriptBudgets,
    },
    script_stream::{STREAM_BUFFER_BYTES, STREAM_READ_MAX_BYTES},
    script_task::MAX_ACTIVE_TASKS,
};

pub const SCRIPT_CATALOG_SCHEMA_VERSION: u32 = 3;
pub const SCRIPT_COMPARISON_SCHEMA_VERSION: u32 = 1;
pub const SCRIPT_COMPARISON_REVIEWED_ON: &str = "2026-07-29";

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalogueRelationship {
    Similar,
    AgentermSpecific,
    Deferred,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ScriptApiAnalogue {
    pub relationship: AnalogueRelationship,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<&'static str>,
    pub documentation: &'static str,
    pub reviewed_version: &'static str,
    pub reviewed_on: &'static str,
    pub semantic_note: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ScriptApiComparisons {
    pub nodejs: ScriptApiAnalogue,
    pub bun: ScriptApiAnalogue,
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
    pub comparisons: ScriptApiComparisons,
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
const NO_STRINGS: &[&str] = &[];
const FLEET_ERRORS: &[&str] = &[
    "broker_invalid_arguments",
    "broker_operation_unknown",
    "broker_operation_degraded",
    "broker_host_error",
    "broker_transport",
    "broker_invalid_response",
    "broker_receipt_missing",
    "server_restart",
    "journal_gap",
    "future_sequence",
    "event_wait_timeout",
];
const HTTP_REQUEST_ERRORS: &[&str] = &[
    "http_method_invalid",
    "http_method_unsupported",
    "http_url_invalid",
    "http_option_unknown",
    "http_headers_limit",
    "http_request_body_limit",
    "http_timeout",
    "http_redirect",
    "http_proxy",
    "http_tls",
    "http_transport",
];
const HTTP_START_ERRORS: &[&str] = &[
    "http_method_invalid",
    "http_method_unsupported",
    "http_url_invalid",
    "http_option_unknown",
    "http_headers_limit",
    "http_request_body_limit",
    "http_timeout",
    "http_redirect",
    "http_proxy",
    "http_tls",
    "http_transport",
    "task_limit",
    "task_spawn_failed",
    "task_failed",
    "task_cancelled",
];
const HTTP_RESPONSE_ERRORS: &[&str] = &[
    "http_header_name",
    "stream_read_timeout",
    "stream_read_failed",
    "stream_collect_limit",
    "stream_closed",
];

pub fn entries() -> Vec<ScriptApiEntry> {
    let mut entries = vec![ScriptApiEntry {
        stable_id: "rh.print",
        catalog_path: "runtime/output/print",
        surface_path: "print",
        rust_path: None,
        rust_mapping: RustMapping::None,
        semantic_differences: &["output is captured and bounded by the invocation"],
        comparisons: unreviewed_comparisons(),
        status: ScriptApiStatus::Shipped,
        stability: ScriptApiStability::Stable,
        designed_on: "2026-07-28",
        since: "script-api-v1",
        profiles: SHIPPED_PROFILES,
        signature: "print(value)",
        kind: "rh_builtin",
        authority: "none",
        side_effects: &["captured_output"],
        execution: "sync",
        cancellation: "between_rh_operations",
        errors: &["limit_output_bytes"],
        result: None,
        operation_id: None,
        operation: None,
        availability_reason: None,
    }];

    entries.extend(OPERATION_CATALOG.iter().map(fleet_operation_entry));

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
            "std.fs.exists-case-exact",
            "system/filesystem/exists-case-exact",
            "std::fs::exists_case_exact",
            None,
            RustMapping::None,
            // Native/AOT pack surface; interpret path uses exists(path).
            "exists(path)",
            (&["filesystem_metadata", "case_exact_path_components"], NO_STRINGS),
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
            "std.fs.symlink-metadata",
            "system/filesystem/symlink-metadata",
            "std::fs::symlink_metadata",
            Some("std::fs::symlink_metadata"),
            RustMapping::Direct,
            "std::fs::symlink_metadata(path)",
            (
                &["filesystem_metadata", "does_not_follow_final_symlink"],
                &["fs_symlink_metadata"],
            ),
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
            "std.fs.create-dir",
            "system/filesystem/create-directory",
            "std::fs::create_dir",
            Some("std::fs::create_dir"),
            RustMapping::Adapted,
            "std::fs::create_dir(path)",
            (&["filesystem_write"], &["fs_create_dir"]),
        ),
        shipped_local_entry(
            "std.fs.create-dir-all",
            "system/filesystem/create-directory-tree",
            "std::fs::create_dir_all",
            Some("std::fs::create_dir_all"),
            RustMapping::Adapted,
            "std::fs::create_dir_all(path)",
            (&["filesystem_write"], &["fs_create_dir_all"]),
        ),
        shipped_local_entry(
            "std.fs.copy",
            "system/filesystem/copy",
            "std::fs::copy",
            Some("std::fs::copy"),
            RustMapping::Adapted,
            "std::fs::copy(source, destination)",
            (&["filesystem_read", "filesystem_write"], &["fs_copy"]),
        ),
        shipped_local_entry(
            "std.fs.rename",
            "system/filesystem/rename",
            "std::fs::rename",
            Some("std::fs::rename"),
            RustMapping::Adapted,
            "std::fs::rename(source, destination)",
            (
                &["filesystem_write", "platform_overwrite_semantics"],
                &["fs_rename"],
            ),
        ),
        shipped_local_entry(
            "std.fs.remove-file",
            "system/filesystem/remove-file",
            "std::fs::remove_file",
            Some("std::fs::remove_file"),
            RustMapping::Adapted,
            "std::fs::remove_file(path)",
            (&["filesystem_delete", "arbitrary_explicit_target"], &["fs_remove_file"]),
        ),
        shipped_local_entry(
            "std.fs.remove-dir",
            "system/filesystem/remove-empty-directory",
            "std::fs::remove_dir",
            Some("std::fs::remove_dir"),
            RustMapping::Adapted,
            "std::fs::remove_dir(path)",
            (&["filesystem_delete", "arbitrary_explicit_target"], &["fs_remove_dir"]),
        ),
        shipped_local_entry(
            "std.fs.remove-dir-all",
            "system/filesystem/remove-directory-tree",
            "std::fs::remove_dir_all",
            Some("std::fs::remove_dir_all"),
            RustMapping::Adapted,
            "std::fs::remove_dir_all(path)",
            (
                &["filesystem_recursive_delete", "arbitrary_explicit_target"],
                &["fs_remove_dir_all"],
            ),
        ),
        shipped_local_entry_with_result(
            shipped_local_entry_with_design(
                shipped_local_entry(
                    "std.fs.try-lock-exclusive",
                    "system/filesystem/try-lock-exclusive",
                    "std::fs::try_lock_exclusive",
                    Some("std::fs::File::try_lock"),
                    RustMapping::Adapted,
                    "std::fs::try_lock_exclusive(path)",
                    (
                        &["filesystem_lock", "nonblocking_exclusive_lock"],
                        &["fs_try_lock_exclusive"],
                    ),
                ),
                "2026-08-01",
            ),
            Some("FileLockAttempt"),
        ),
        shipped_runtime_entry(
            "rh.runtime.temp-dir",
            "system/temp/invocation-directory",
            "rh::runtime::temp_dir",
            "rh::runtime::temp_dir()",
            (&["invocation_owned_temp"], &["runtime_temp_unavailable"]),
            Some("PathBuf"),
        ),
        shipped_runtime_entry(
            "rh.runtime.atomic-write",
            "system/filesystem/atomic-write-text",
            "rh::runtime::atomic_write",
            "rh::runtime::atomic_write(path, text)",
            (
                &["filesystem_write", "same_volume_atomic_replace"],
                &[
                    "runtime_atomic_write_invalid_target",
                    "runtime_atomic_write_create",
                    "runtime_atomic_write_data",
                    "runtime_atomic_write_promote",
                    "runtime_atomic_write_sync",
                ],
            ),
            None,
        ),
        shipped_runtime_entry(
            "rh.runtime.atomic-write-bytes",
            "system/filesystem/atomic-write-bytes",
            "rh::runtime::atomic_write_bytes",
            "rh::runtime::atomic_write_bytes(path, bytes)",
            (
                &["filesystem_write", "same_volume_atomic_replace"],
                &[
                    "runtime_atomic_write_invalid_target",
                    "runtime_atomic_write_create",
                    "runtime_atomic_write_data",
                    "runtime_atomic_write_promote",
                    "runtime_atomic_write_sync",
                ],
            ),
            None,
        ),
        shipped_runtime_entry(
            "rh.runtime.append-sync",
            "system/filesystem/durable-append-text",
            "rh::runtime::append_sync",
            "rh::runtime::append_sync(path, text)",
            (
                &[
                    "filesystem_append",
                    "record_sync",
                    "parent_sync_on_create",
                    "maximum_8_mib_record",
                ],
                &[
                    "runtime_append_too_large",
                    "runtime_append_invalid_target",
                    "runtime_append_open",
                    "runtime_append_write",
                    "runtime_append_sync",
                    "runtime_append_parent_sync",
                ],
            ),
            None,
        ),
        shipped_runtime_entry(
            "rh.runtime.append-sync-bytes",
            "system/filesystem/durable-append-bytes",
            "rh::runtime::append_sync_bytes",
            "rh::runtime::append_sync_bytes(path, bytes)",
            (
                &[
                    "filesystem_append",
                    "record_sync",
                    "parent_sync_on_create",
                    "maximum_8_mib_record",
                ],
                &[
                    "runtime_append_too_large",
                    "runtime_append_invalid_target",
                    "runtime_append_open",
                    "runtime_append_write",
                    "runtime_append_sync",
                    "runtime_append_parent_sync",
                ],
            ),
            None,
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
            "Metadata.is_file/is_dir/is_symlink/is_reparse_point/len/modified",
            Some("std::fs::Metadata"),
            RustMapping::Adapted,
            "metadata.is_file / metadata.is_dir / metadata.is_symlink / \
             metadata.is_reparse_point / metadata.len / metadata.modified",
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
            "std.path.parent",
            "data/path/parent",
            "std::path::parent",
            Some("std::path::Path::parent"),
            RustMapping::Adapted,
            "std::path::parent(path)",
            (NO_STRINGS, &["path_parent"]),
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
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.net.tcp-stream-connect",
                "system/network/tcp/connect",
                "std::net::TcpStream::connect",
                Some("std::net::TcpStream::connect"),
                RustMapping::Adapted,
                "std::net::TcpStream::connect(address)",
                (
                    &[
                        "blocking_dns_and_connect_inside_supervised_worker",
                        "connect_phase_deadline_after_resolution",
                        "all_targets_allowed",
                    ],
                    &[
                        "net_address_invalid",
                        "net_resolve",
                        "net_resolve_empty",
                        "net_connect",
                        "net_connect_timeout",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.net.tcp-stream-connect-timeout",
                "system/network/tcp/connect-timeout",
                "std::net::TcpStream::connect_timeout",
                Some("std::net::TcpStream::connect_timeout"),
                RustMapping::Direct,
                "std::net::TcpStream::connect_timeout(address, timeout)",
                (
                    &[
                        "blocking_dns_and_connect_inside_supervised_worker",
                        "connect_phase_deadline_after_resolution",
                        "all_targets_allowed",
                    ],
                    &[
                        "net_address_invalid",
                        "net_timeout_invalid",
                        "net_resolve",
                        "net_resolve_empty",
                        "net_connect",
                        "net_connect_timeout",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.net.tcp-stream",
                "system/network/tcp/stream",
                "TcpStream.peer_addr/local_addr/set_read_timeout/set_write_timeout/set_nodelay/write_all/flush/read/read_line/shutdown",
                Some("std::net::TcpStream"),
                RustMapping::Adapted,
                "stream.peer_addr / stream.local_addr / stream.set_read_timeout(timeout) / stream.set_write_timeout(timeout) / stream.set_nodelay(enabled) / stream.write_all(text_or_bytes) / stream.flush() / stream.read(max_bytes) / stream.read_line(max_bytes) / stream.shutdown()",
                (
                    &[
                        "typed_owned_stream",
                        "bounded_per_call_io",
                        "read_line_strips_crlf",
                        "strict_utf8_line",
                    ],
                    &[
                        "net_io_limit_invalid",
                        "net_read",
                        "net_read_timeout",
                        "net_read_limit",
                        "net_read_not_utf8",
                        "net_write",
                        "net_write_timeout",
                        "net_write_limit",
                        "net_eof",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.net.tcp-listener-bind",
                "system/network/tcp/listener/bind",
                "std::net::TcpListener::bind",
                Some("std::net::TcpListener::bind"),
                RustMapping::Direct,
                "std::net::TcpListener::bind(address)",
                (
                    &["blocking_bind_inside_supervised_worker", "all_bind_targets_allowed"],
                    &["net_address_invalid", "net_bind"],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.net.tcp-listener",
                "system/network/tcp/listener",
                "TcpListener.local_addr/set_nonblocking/accept/accept_timeout",
                Some("std::net::TcpListener"),
                RustMapping::Adapted,
                "listener.local_addr / listener.set_nonblocking(enabled) / listener.accept() / listener.accept_timeout(timeout)",
                (
                    &[
                        "typed_owned_listener",
                        "all_peers_allowed",
                        "accepted_stream_has_complete_tcp_stream_surface",
                        "bounded_accept_wait_available",
                    ],
                    &[
                        "net_accept",
                        "net_accept_timeout",
                        "net_listener_poisoned",
                        "net_nonblocking_config",
                    ],
                ),
            ),
            "2026-07-30",
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
                    "var_is_a_rh_reserved_word",
                    "worker_environment_snapshot",
                    "value_not_audited",
                ],
                &["environment_missing", "environment_not_unicode"],
            ),
            ),
            &["std::env::var is exposed as get because var is Rh language reserved"],
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
                    &["new_is_a_rh_reserved_word", "no_implicit_shell"],
                    &["process_program_empty"],
                ),
            ),
            &[
                "Command::new cannot be exposed because new is Rh language reserved",
                "the host never inserts an implicit shell",
                "errors use stable AgenTerm codes rather than Rust io::Error values",
            ],
        ),
        shipped_local_entry(
            "std.process.command-status",
            "system/process/command-status",
            "std::process::command_status",
            Some("std::process::Command::status"),
            RustMapping::Adapted,
            // Native/AOT pack free-form; interpret path uses Command.output().
            "native pack free-form; command.output()",
            (
                &["no_implicit_shell", "typed_timeout", "job_object_cleanup"],
                &["process_program_empty", "process_spawn", "process_timeout"],
            ),
        ),
        shipped_local_entry(
            "std.process.command-stdout-file",
            "system/process/command-stdout-file",
            "std::process::command_stdout_file",
            None,
            RustMapping::None,
            // Native/AOT pack free-form; interpret path uses Command.stdout_file.
            "native pack free-form; command.stdout_file(path)",
            (
                &["no_implicit_shell", "typed_timeout", "job_object_cleanup", "stdout_to_path"],
                &["process_program_empty", "process_spawn", "process_timeout"],
            ),
        ),
        shipped_local_entry(
            "std.process.id",
            "system/process/id",
            "std::process::id",
            Some("std::process::id"),
            RustMapping::Direct,
            "std::process::id()",
            (&["current_worker_process"], NO_STRINGS),
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.process.list",
                "system/process/list",
                "std::process::list",
                None,
                RustMapping::None,
                "std::process::list() -> Array<ProcessInfo{id,parent_id,executable_name}>",
                (
                    &[
                        "operating_system_process_snapshot",
                        "sorted_by_process_id",
                        "parent_process_identity",
                        "unrestricted_inventory",
                    ],
                    &["process_list_failed", "process_list_too_large"],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.process.kill",
                "system/process/kill",
                "std::process::kill",
                None,
                RustMapping::None,
                "std::process::kill(pid)",
                (
                    &[
                        "arbitrary_operating_system_process",
                        "unrestricted_target_selection",
                        "forceful_termination",
                    ],
                    &[
                        "process_id_invalid",
                        "process_kill_open",
                        "process_kill",
                        "process_kill_unsupported",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry(
            "std.process.command-builder",
            "system/process/command/builder",
            "Command.arg/args/current_dir/env/env_remove/env_clear/stdin_text/stdin_bytes/stdout_file/stderr_file/timeout/capture_limit",
            Some("std::process::Command"),
            RustMapping::Adapted,
            "command.arg(value) / command.args(values) / command.current_dir(path) / command.env(name, value) / command.stdin_text(text) / command.stdin_bytes(bytes) / command.stdout_file(path) / command.stderr_file(path)",
            (&["mutable_builder", "bounded_text_or_binary_stdin", "invocation_owned"], &["process_argument", "environment_name_invalid", "process_stdin_too_large"]),
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
                        "spawn_is_a_rh_reserved_word",
                        "invocation_owned",
                        "job_object_cleanup",
                    ],
                    &["process_spawn"],
                ),
            ),
            &[
                "Command::spawn is exposed as start because spawn is Rh language reserved",
                "the Child is owned by one supervised invocation",
                "descendants inherit supervisor process-tree cleanup",
            ],
        ),
        shipped_local_entry(
            "std.process.child",
            "system/process/child",
            "Child.id/state/platform_facts/stdout/stderr/kill/kill_tree/wait_with_output",
            Some("std::process::Child"),
            RustMapping::Adapted,
            "child.id / child.state / child.platform_facts / child.stdout / child.stderr / child.kill() / child.kill_tree() / child.wait_with_output([timeout])",
            (
                &[
                    "live_bounded_streams",
                    "typed_timeout",
                    "invocation_owned",
                    "id_stable_after_completion",
                    "owned_child_platform_observation",
                    "opaque_window_identity",
                    "top_level_window_title",
                    "foreground_window_identity",
                    "top_level_window_is_foreground",
                    "owned_process_tree_cleanup",
                ],
                &["process_kill", "process_kill_tree", "process_timeout"],
            ),
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "std.process.child-window-input",
                "system/process/child/window-input",
                "Child.window_key/window_pointer/window_pointer_coordinate_scale/window_message/window_rect/window_client_rect/window_resize/window_control; WindowControl.visible/text/set_text/click",
                None,
                RustMapping::None,
                "child.window_key(key) / child.window_pointer(action, x, y) / child.window_pointer_coordinate_scale() / child.window_message(message, wparam, lparam) / child.window_rect() / child.window_client_rect() / child.window_resize(width, height) / child.window_control(id)",
                (
                    &[
                        "invocation_owned_child",
                        "top_level_window_lookup",
                        "native_key_delivery",
                        "native_pointer_delivery",
                        "native_pointer_coordinate_scale",
                        "native_message_delivery",
                        "window_and_client_geometry",
                        "nonactivating_resize",
                        "child_control_lookup",
                        "child_control_visibility",
                        "unicode_control_text",
                        "child_control_click",
                        "control_id_reresolution",
                    ],
                    &[
                        "process_window_not_found",
                        "process_window_input",
                        "process_window_input_unsupported",
                        "process_window_key_invalid",
                        "process_window_pointer_action_invalid",
                        "process_window_coordinate_invalid",
                        "process_window_message_invalid",
                        "process_window_message_parameter_invalid",
                        "process_window_rect",
                        "process_window_size_invalid",
                        "process_window_resize",
                        "process_window_control_id_invalid",
                        "process_window_control_not_found",
                        "process_window_control_text",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry(
            "std.process.output",
            "system/process/output",
            "Output.success/exit_code/stdout/stderr/complete/truncated/stdout_text/stderr_text/combined_text/error/require_success",
            Some("std::process::Output"),
            RustMapping::Adapted,
            "output.success / output.exit_code / output.stdout / output.stderr / output.require_success(code)",
            (
                &[
                    "bytes_first_output",
                    "strict_utf8_helpers",
                    "truthful_truncation",
                    "explicit_nonzero_propagation",
                ],
                &[
                    "process_stdout_not_utf8",
                    "process_stderr_not_utf8",
                    "child_nonzero",
                ],
            ),
        ),
        shipped_local_entry(
            "rh.stream.handle",
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
            (&["maximum_60000_ms"], &["duration_millis"]),
        ),
        shipped_local_entry(
            "std.time.duration-from-secs",
            "system/time/duration/from-secs",
            "std::time::Duration::from_secs",
            Some("std::time::Duration::from_secs"),
            RustMapping::Adapted,
            "std::time::Duration::from_secs(value)",
            (&["maximum_60_seconds"], &["duration_seconds"]),
        ),
        shipped_local_entry(
            "rh.fail",
            "runtime/control/fail",
            "rh::fail",
            None,
            RustMapping::None,
            // Transpile/AOT helper; not an Engine module function.
            "native fail helper",
            (NO_STRINGS, &["script_fail"]),
        ),
        shipped_local_entry(
            "std.string.split",
            "data/string/split",
            "String.split",
            Some("str::split"),
            RustMapping::Adapted,
            "text.split(separator)",
            (&["returns_string_list"], NO_STRINGS),
        ),
        shipped_local_entry(
            "rh.json.parse",
            "data/json/parse",
            "rh::json::parse",
            None,
            RustMapping::None,
            "rh::json::parse(text)",
            (NO_STRINGS, &["json_parse", "json_dynamic"]),
        ),
        shipped_local_entry(
            "rh.json.parse-file",
            "data/json/parse-file",
            "rh::json::parse_file",
            Some("serde_json::from_reader"),
            RustMapping::Adapted,
            "rh::json::parse_file(path)",
            (
                &["typed_file_input", "eight_mebibyte_limit"],
                &["json_parse_file", "json_parse_file_too_large"],
            ),
        ),
        shipped_local_entry(
            "rh.json.stringify",
            "data/json/stringify",
            "rh::json::stringify",
            None,
            RustMapping::None,
            "rh::json::stringify(value)",
            (NO_STRINGS, &["json_value", "json_stringify"]),
        ),
        shipped_local_entry(
            "rh.json.stringify-pretty",
            "data/json/stringify-pretty",
            "rh::json::stringify_pretty",
            None,
            RustMapping::None,
            "rh::json::stringify_pretty(value)",
            (NO_STRINGS, &["json_value", "json_stringify"]),
        ),
        shipped_local_entry(
            "rh.bytes.from-text",
            "data/bytes/from-text",
            "rh::bytes::from_text",
            None,
            RustMapping::None,
            "rh::bytes::from_text(text)",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "rh.bytes.from-array",
                "data/bytes/from-array",
                "rh::bytes::from_array",
                None,
                RustMapping::None,
                "rh::bytes::from_array(values)",
                (
                    &["arbitrary_byte_construction", "unsigned_byte_values"],
                    &[
                        "bytes_value_type",
                        "bytes_value_range",
                        "bytes_length_limit",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry(
            "rh.bytes.length",
            "data/bytes/length",
            "Bytes.len",
            None,
            RustMapping::None,
            "bytes.len",
            (NO_STRINGS, NO_STRINGS),
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "rh.bytes.raw-operations",
                "data/bytes/raw-operations",
                "Bytes.get/slice/append",
                None,
                RustMapping::None,
                "bytes.get(index) / bytes.slice(offset, length) / bytes.append(other)",
                (
                    &["unsigned_byte_values", "owned_slice", "bounded_append"],
                    &[
                        "bytes_index_out_of_bounds",
                        "bytes_slice_out_of_bounds",
                        "bytes_length_limit",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry(
            "rh.bytes.to-text",
            "data/bytes/to-text",
            "Bytes.to_text",
            None,
            RustMapping::None,
            "bytes.to_text()",
            (NO_STRINGS, &["bytes_invalid_utf8"]),
        ),
        shipped_local_entry(
            "rh.crypto.sha256",
            "data/crypto/sha256",
            "rh::crypto::sha256",
            Some("sha2::Sha256"),
            RustMapping::Adapted,
            "rh::crypto::sha256(bytes)",
            (&["lowercase_hex", "sha256"], NO_STRINGS),
        ),
        shipped_local_entry(
            "rh.crypto.sha256-file",
            "data/crypto/sha256-file",
            "rh::crypto::sha256_file",
            Some("sha2::Sha256"),
            RustMapping::Adapted,
            "rh::crypto::sha256_file(path)",
            (
                &["streaming_64_kib_chunks", "lowercase_hex", "sha256"],
                &["crypto_sha256_file"],
            ),
        ),
        shipped_local_entry(
            "rh.crypto.tree-metadata-digest",
            "data/crypto/tree-metadata-digest",
            "rh::crypto::tree_metadata_digest",
            Some("sha2::Sha256"),
            RustMapping::Adapted,
            "rh::crypto::tree_metadata_digest(path) -> #{ ok, identity }",
            (
                &[
                    "sorted_metadata_records",
                    "lowercase_hex",
                    "sha256",
                    "no_file_contents",
                ],
                &["crypto_tree_metadata_digest"],
            ),
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "rh.hash.fnv1a64",
                "data/hash/fnv1a64",
                "rh::hash::fnv1a64",
                None,
                RustMapping::None,
                "rh::hash::fnv1a64(bytes)",
                (&["lowercase_hex", "wrapping_u64", "fnv1a64"], NO_STRINGS),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "rh.image.inspect-png",
                "data/image/png/inspect",
                "rh::image::inspect_png",
                None,
                RustMapping::None,
                "rh::image::inspect_png(path) -> PngInfo",
                (
                    &[
                        "typed_dimensions",
                        "sampled_rgb",
                        "sampled_luminance",
                        "bounded_decode",
                    ],
                    &[
                        "image_png_open",
                        "image_png_header",
                        "image_png_decode",
                        "image_png_dimensions",
                        "image_png_color",
                        "image_png_size",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "rh.clipboard.get-text",
                "system/clipboard/text/get",
                "rh::clipboard::get_text",
                None,
                RustMapping::None,
                "rh::clipboard::get_text() -> String",
                (
                    &[
                        "operating_system_clipboard",
                        "unicode_text",
                        "unrestricted_local_access",
                        "get_text",
                    ],
                    &[
                        "clipboard_open",
                        "clipboard_text_unavailable",
                        "clipboard_read",
                        "clipboard_text_invalid",
                        "clipboard_unsupported",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry_with_design(
            shipped_local_entry(
                "rh.clipboard.set-text",
                "system/clipboard/text/set",
                "rh::clipboard::set_text",
                None,
                RustMapping::None,
                "rh::clipboard::set_text(text)",
                (
                    &[
                        "operating_system_clipboard",
                        "unicode_text",
                        "unrestricted_local_access",
                        "set_text",
                    ],
                    &[
                        "clipboard_open",
                        "clipboard_text_too_large",
                        "clipboard_clear",
                        "clipboard_allocate",
                        "clipboard_write",
                        "clipboard_unsupported",
                    ],
                ),
            ),
            "2026-07-30",
        ),
        shipped_local_entry(
            "rh.task.after",
            "runtime/task/timer/after",
            "rh::task::after",
            None,
            RustMapping::None,
            "rh::task::after(duration)",
            (&["background_timer", "invocation_owned"], &["task_state_poisoned"]),
        ),
        shipped_local_entry(
            "rh.task.sleep",
            "runtime/task/timer/sleep",
            "rh::task::sleep",
            None,
            RustMapping::None,
            "rh::task::sleep(duration)",
            (&["blocking_wait", "invocation_owned"], &["task_cancelled"]),
        ),
        shipped_local_entry(
            "rh.task.wait-all",
            "runtime/task/composition/wait-all",
            "rh::task::wait_all",
            None,
            RustMapping::None,
            "rh::task::wait_all(tasks[, timeout])",
            (&["deterministic_input_order", "maximum_64_tasks"], &["task_wait_timeout", "task_cancelled"]),
        ),
        shipped_local_entry(
            "rh.task.race",
            "runtime/task/composition/race",
            "rh::task::race",
            None,
            RustMapping::None,
            "rh::task::race(tasks[, timeout])",
            (&["returns_winning_index", "maximum_64_tasks"], &["task_race_empty", "task_wait_timeout"]),
        ),
        shipped_local_entry(
            "rh.task.cancel-all",
            "runtime/task/composition/cancel-all",
            "rh::task::cancel_all",
            None,
            RustMapping::None,
            "rh::task::cancel_all(tasks)",
            (&["idempotent_cancellation", "maximum_64_tasks"], &["task_collection_type"]),
        ),
        shipped_local_entry(
            "rh.task.handle",
            "runtime/task/handle",
            "Task.id/kind/state/done/cancelled/wait/cancel",
            None,
            RustMapping::None,
            "task.id / task.kind / task.state / task.done / task.cancelled / task.wait([timeout]) / task.cancel()",
            (&["typed_host_payload_only", "no_rh_dynamic_cross_thread", "failed_terminal_state"], &["task_wait_timeout", "task_failed", "task_cancelled"]),
        ),
        http_entry(
            "rh.http.request",
            "network/http/client/request",
            "rh::http::request",
            "rh::http::request(method, url[, options]) -> HttpResponse",
            "sync",
            "supervisor_deadline_and_transport_timeout",
            Some("HttpResponse"),
            HTTP_REQUEST_ERRORS,
        ),
        http_entry(
            "rh.http.start",
            "network/http/client/start",
            "rh::http::start",
            "rh::http::start(method, url[, options]) -> Task",
            "background_task",
            "task_cancel_immediate_late_completion_ignored_transport_timeout_bounded",
            Some("Task<HttpResponse>"),
            HTTP_START_ERRORS,
        ),
        http_entry(
            "rh.http.response",
            "network/http/client/response",
            "HttpResponse.status/version/headers/body/header",
            "response.status / response.version / response.headers / response.body / response.header(name)",
            "typed_value",
            "body_stream_close",
            Some("status_headers_and_bounded_body_stream"),
            HTTP_RESPONSE_ERRORS,
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
                    "rh_extension_implicit",
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
            "script task list/show/check/run",
            None,
            RustMapping::None,
            "script task list|show|check|run [TASK] [--manifest PATH]",
            (
                &[
                    "agenterm_tasks_json_schema_v2",
                    "api_and_capability_requirements",
                    "invalid_entries_remain_visible",
                    "environment_names_only",
                ],
                &[
                    "task_manifest_version",
                    "task_project_incompatible",
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
    for entry in &mut entries {
        entry.comparisons = comparisons_for(entry.stable_id);
    }
    entries
}

pub fn catalog() -> Value {
    let defaults = ScriptBudgets::default();
    let hard_limits = ScriptBudgets::hard_limits();
    json!({
        "schema_version": SCRIPT_CATALOG_SCHEMA_VERSION,
        "api_version": SCRIPT_API_VERSION,
        "default_profile": "local",
        "execution_model": "unrestricted_local",
        "model": "rh_language + rust_shaped_std_subset + rh_native_extensions + agenterm_fleet",
        "comparison": {
            "schema_version": SCRIPT_COMPARISON_SCHEMA_VERSION,
            "purpose": "research analogues for horizontal discovery; never a compatibility claim",
            "reviewed_on": SCRIPT_COMPARISON_REVIEWED_ON,
            "nodejs": {
                "reviewed_version": "26.5.0",
                "documentation": "https://nodejs.org/docs/latest/api/",
            },
            "bun": {
                "reviewed_version": "1.3.14",
                "documentation": "https://bun.com/docs/runtime/bun-apis",
            },
        },
        "profiles": {
            "pure": {
                "status": "shipped",
                "compatibility_label": true,
                "variables": ["args", "fleet"],
                "ambient_authority": ["ordinary_local_program"],
            },
            "observe": {
                "status": "shipped",
                "compatibility_label": true,
                "variables": ["args", "fleet"],
                "ambient_authority": ["ordinary_local_program"],
            },
            "local": {
                "status": "shipped",
                "compatibility_label": true,
                "variables": ["args", "fleet"],
                "ambient_authority": ["ordinary_local_program"],
            },
        },
        "operations": [
            "api", "check", "eval", "run",
            "task-list", "task-show", "task-check", "task-run"
        ],
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
            "global_concurrency": 8,
        },
        "limits": {
            "defaults": defaults,
            "hard_maximums": hard_limits,
            "invocation_bytes": SCRIPT_INVOCATION_MAX_BYTES,
            "stream_buffer_bytes": STREAM_BUFFER_BYTES,
            "stream_read_max_bytes": STREAM_READ_MAX_BYTES,
            "max_active_tasks": MAX_ACTIVE_TASKS,
            "http": {
                "default_timeout_ms": DEFAULT_HTTP_TIMEOUT.as_millis(),
                "max_timeout_ms": MAX_HTTP_TIMEOUT.as_millis(),
                "default_body_bytes": DEFAULT_HTTP_BODY_BYTES,
                "max_body_bytes": MAX_HTTP_BODY_BYTES,
                "max_request_body_bytes": MAX_HTTP_REQUEST_BODY_BYTES,
                "max_headers": MAX_HTTP_HEADERS,
                "max_header_bytes": MAX_HTTP_HEADER_BYTES,
                "max_url_bytes": MAX_HTTP_URL_BYTES,
                "default_redirects": DEFAULT_HTTP_REDIRECTS,
                "max_redirects": MAX_HTTP_REDIRECTS,
            },
        },
        "entries": entries(),
        "typed_error": {
            "fields": [
                "class",
                "code",
                "operation",
                "safe_message",
                "retryable",
                "target_kind",
                "truncated",
                "cause_class",
            ],
            "catchable_slices": [],
        },
        "failure_categories": [
            "configuration", "limit", "script", "child", "cancelled", "fleet", "protocol", "host"
        ],
        "exit_classes": {
            "success": 0,
            "script": 1,
            "protocol": 1,
            "host": 1,
            "configuration": 2,
            "limit": 3,
            "child": 4,
            "cancelled": 5,
            "fleet": 6,
        },
    })
}

fn fleet_operation_entry(operation: &'static OperationSpec) -> ScriptApiEntry {
    let signature = match operation.id {
        "protocol.info" => "fleet.protocol.info()",
        "ui.snapshot" => "fleet.ui.snapshot()",
        "workspace.info" => "fleet.workspace.info()",
        "tabs.list" => "fleet.tabs.list()",
        "tabs.active" => "fleet.tabs.active()",
        "pane.capture" => "fleet.terminal(tab).capture(max_bytes)",
        "events.read" => "fleet.events.read(epoch, after[, limit])",
        "events.wait" => "fleet.events.wait(epoch, after, kind[, tab], timeout_ms)",
        "ui.tabs.show" => "fleet.ui.tabs.show()",
        "ui.tabs.hide" => "fleet.ui.tabs.hide()",
        "ui.tabs.toggle" => "fleet.ui.tabs.toggle()",
        "ui.tabs.set-width" => "fleet.ui.tabs.set_width(width)",
        "ui.window.activate" => "fleet.ui.window.activate()",
        "terminal.paste" => "fleet.terminal.paste()",
        "server.kill" => "fleet.server.kill([target])",
        "workspace.shutdown" => "fleet.workspace.shutdown()",
        _ => operation.script_surface,
    };
    let authority = match operation.class {
        OperationClass::Observe => "observe",
        OperationClass::Control => "fleet_control",
        OperationClass::Destructive => "fleet_destructive",
    };
    ScriptApiEntry {
        stable_id: operation.id,
        catalog_path: operation.id,
        surface_path: operation.script_surface,
        rust_path: None,
        rust_mapping: RustMapping::None,
        semantic_differences: &[
            "AgenTerm-specific invocation-bound broker object",
            "typed operations are derived from the public operation catalog",
            "mutations return native receipt, correlated events, and verified post-state",
        ],
        comparisons: unreviewed_comparisons(),
        status: if operation.available {
            ScriptApiStatus::Shipped
        } else {
            ScriptApiStatus::Planned
        },
        stability: if operation.available {
            ScriptApiStability::Stable
        } else {
            ScriptApiStability::Reserved
        },
        designed_on: "2026-07-28",
        since: "script-api-v2",
        profiles: if operation.available {
            SHIPPED_PROFILES
        } else {
            NO_STRINGS
        },
        signature,
        kind: "brokered_method",
        authority,
        side_effects: operation.events,
        execution: "sync",
        cancellation: "host_deadline_and_broker_wait",
        errors: FLEET_ERRORS,
        result: Some(operation.result_type),
        operation_id: Some(operation.id),
        operation: Some(operation),
        availability_reason: (!operation.available).then_some("backing operation is unavailable"),
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
        comparisons: unreviewed_comparisons(),
        status: ScriptApiStatus::Planned,
        stability: ScriptApiStability::Reserved,
        designed_on: "2026-07-28",
        since: "planned-v0.1.9",
        profiles: SHIPPED_PROFILES,
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
        comparisons: unreviewed_comparisons(),
        status: ScriptApiStatus::Shipped,
        stability: ScriptApiStability::Stable,
        designed_on: "2026-07-28",
        since: "0.1.9",
        profiles: SHIPPED_PROFILES,
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

fn shipped_runtime_entry(
    stable_id: &'static str,
    catalog_path: &'static str,
    surface_path: &'static str,
    signature: &'static str,
    behavior: (&'static [&'static str], &'static [&'static str]),
    result: Option<&'static str>,
) -> ScriptApiEntry {
    ScriptApiEntry {
        stable_id,
        catalog_path,
        surface_path,
        rust_path: None,
        rust_mapping: RustMapping::None,
        semantic_differences: &[
            "AgenTerm/Rh invocation lifecycle extension with no Rust std surface equivalent",
            "temporary ownership and atomic promotion are enforced by the host runtime",
        ],
        comparisons: unreviewed_comparisons(),
        status: ScriptApiStatus::Shipped,
        stability: ScriptApiStability::Stable,
        designed_on: "2026-07-28",
        since: "0.1.9",
        profiles: SHIPPED_PROFILES,
        signature,
        kind: "native_function",
        authority: "local",
        side_effects: behavior.0,
        execution: "sync",
        cancellation: "between_native_calls",
        errors: behavior.1,
        result,
        operation_id: None,
        operation: None,
        availability_reason: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn http_entry(
    stable_id: &'static str,
    catalog_path: &'static str,
    surface_path: &'static str,
    signature: &'static str,
    execution: &'static str,
    cancellation: &'static str,
    result: Option<&'static str>,
    errors: &'static [&'static str],
) -> ScriptApiEntry {
    ScriptApiEntry {
        stable_id,
        catalog_path,
        surface_path,
        rust_path: None,
        rust_mapping: RustMapping::None,
        semantic_differences: &[
            "AgenTerm-owned high-level client; Rust std has no HTTP client",
            "headers and bodies are bytes-first and bounded",
            "errors expose stable privacy-safe codes without URL, credentials, or body",
        ],
        comparisons: unreviewed_comparisons(),
        status: ScriptApiStatus::Shipped,
        stability: ScriptApiStability::Stable,
        designed_on: "2026-07-28",
        since: "0.1.9",
        profiles: SHIPPED_PROFILES,
        signature,
        kind: "native_http",
        authority: "network",
        side_effects: &["network_request", "environment_proxy_when_not_overridden"],
        execution,
        cancellation,
        errors,
        result,
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

fn shipped_local_entry_with_design(
    mut entry: ScriptApiEntry,
    designed_on: &'static str,
) -> ScriptApiEntry {
    entry.designed_on = designed_on;
    entry
}

fn shipped_local_entry_with_result(
    mut entry: ScriptApiEntry,
    result: Option<&'static str>,
) -> ScriptApiEntry {
    entry.result = result;
    entry
}

const NODE_DOCUMENTATION: &str = "https://nodejs.org/docs/latest/api/";
const BUN_DOCUMENTATION: &str = "https://bun.com/docs/runtime/bun-apis";

const fn analogue(
    relationship: AnalogueRelationship,
    path: Option<&'static str>,
    documentation: &'static str,
    reviewed_version: &'static str,
    semantic_note: &'static str,
) -> ScriptApiAnalogue {
    ScriptApiAnalogue {
        relationship,
        path,
        documentation,
        reviewed_version,
        reviewed_on: SCRIPT_COMPARISON_REVIEWED_ON,
        semantic_note,
    }
}

const fn similar_comparisons(
    node_path: &'static str,
    bun_path: &'static str,
    semantic_note: &'static str,
) -> ScriptApiComparisons {
    ScriptApiComparisons {
        nodejs: analogue(
            AnalogueRelationship::Similar,
            Some(node_path),
            NODE_DOCUMENTATION,
            "26.5.0",
            semantic_note,
        ),
        bun: analogue(
            AnalogueRelationship::Similar,
            Some(bun_path),
            BUN_DOCUMENTATION,
            "1.3.14",
            semantic_note,
        ),
    }
}

const fn agenterm_specific_comparisons(semantic_note: &'static str) -> ScriptApiComparisons {
    ScriptApiComparisons {
        nodejs: analogue(
            AnalogueRelationship::AgentermSpecific,
            None,
            NODE_DOCUMENTATION,
            "26.5.0",
            semantic_note,
        ),
        bun: analogue(
            AnalogueRelationship::AgentermSpecific,
            None,
            BUN_DOCUMENTATION,
            "1.3.14",
            semantic_note,
        ),
    }
}

const fn unreviewed_comparisons() -> ScriptApiComparisons {
    ScriptApiComparisons {
        nodejs: analogue(
            AnalogueRelationship::NotApplicable,
            None,
            NODE_DOCUMENTATION,
            "26.5.0",
            "comparison assignment is completed before the catalog is returned",
        ),
        bun: analogue(
            AnalogueRelationship::NotApplicable,
            None,
            BUN_DOCUMENTATION,
            "1.3.14",
            "comparison assignment is completed before the catalog is returned",
        ),
    }
}

fn comparisons_for(stable_id: &str) -> ScriptApiComparisons {
    if stable_id == "rh.print" {
        return similar_comparisons(
            "console.log",
            "console.log",
            "AgenTerm captures and bounds output instead of writing an ambient console",
        );
    }
    if stable_id == "rh.fail" {
        return agenterm_specific_comparisons(
            "rh::fail is an AgenTerm typed script failure helper with no Node/Bun core analogue",
        );
    }
    if stable_id == "std.string.split" {
        return similar_comparisons(
            "String.prototype.split",
            "String.prototype.split",
            "AgenTerm returns a typed StringList rather than a JavaScript array",
        );
    }
    if stable_id.starts_with("std.fs.") {
        return similar_comparisons(
            "node:fs",
            "Bun.file / Bun.write / node:fs",
            "AgenTerm is synchronous, typed, invocation-bounded, and rejects broad deletion targets",
        );
    }
    if stable_id.starts_with("std.path.") {
        return similar_comparisons(
            "node:path",
            "node:path / Bun.fileURLToPath",
            "AgenTerm uses a typed PathBuf value and host-native path semantics",
        );
    }
    if stable_id.starts_with("std.env.") {
        return similar_comparisons(
            "process.env / process.cwd",
            "Bun.env / process.cwd",
            "AgenTerm exposes a worker snapshot and never audits environment values",
        );
    }
    if stable_id == "std.process.kill" {
        return similar_comparisons(
            "process.kill",
            "process.kill",
            "AgenTerm exposes forceful arbitrary-PID termination with typed host failures and no Agent-policy filtering",
        );
    }
    if stable_id.starts_with("std.process.") {
        return similar_comparisons(
            "node:child_process",
            "Bun.spawn / Bun.spawnSync",
            "AgenTerm requires executable-plus-argv and owns bounded capture, timeout, and cleanup",
        );
    }
    if stable_id.starts_with("std.time.") {
        return similar_comparisons(
            "Date / node:timers / performance",
            "Date / Bun.sleep / Bun.nanoseconds",
            "AgenTerm separates wall-clock SystemTime from monotonic Duration and task deadlines",
        );
    }
    if stable_id.starts_with("std.net.") {
        return similar_comparisons(
            "node:net",
            "node:net / Bun.connect",
            "AgenTerm exposes blocking Rust-shaped TCP with typed deadlines and bounded per-call I/O",
        );
    }
    if stable_id.starts_with("rh.json.") {
        return similar_comparisons(
            "JSON",
            "JSON",
            "AgenTerm conversion is bounded and uses Rh-compatible Dynamic values",
        );
    }
    if stable_id.starts_with("rh.bytes.") {
        return similar_comparisons(
            "Buffer / TextEncoder / TextDecoder",
            "Uint8Array / Buffer / Bun.readableStreamToBytes",
            "AgenTerm Bytes is an owned bounded value with strict UTF-8 conversion",
        );
    }
    if stable_id.starts_with("rh.crypto.") {
        return similar_comparisons(
            "node:crypto",
            "Bun.CryptoHasher",
            "AgenTerm exposes deterministic typed digests without implicit encoding or shell tools",
        );
    }
    if stable_id.starts_with("rh.hash.") {
        return similar_comparisons(
            "non-cryptographic userland hash",
            "non-cryptographic userland hash",
            "AgenTerm exposes an exact deterministic wire-compatible FNV-1a 64-bit digest",
        );
    }
    if stable_id.starts_with("rh.image.") {
        return similar_comparisons(
            "sharp / pngjs / Canvas image data",
            "sharp / Canvas image data",
            "AgenTerm exposes bounded typed PNG facts without a JavaScript image object graph",
        );
    }
    if stable_id.starts_with("rh.clipboard.") {
        return agenterm_specific_comparisons(
            "Node.js and Bun have no equivalent core operating-system clipboard API; AgenTerm exposes native Unicode text directly",
        );
    }
    if stable_id.starts_with("rh.stream.") {
        return similar_comparisons(
            "node:stream",
            "ReadableStream / Bun.readableStreamToBytes",
            "AgenTerm streams have invocation ownership, bounded queues, backpressure, and completeness facts",
        );
    }
    if stable_id.starts_with("rh.task.") {
        return similar_comparisons(
            "Promise / AbortController / node:timers",
            "Promise / AbortController / Bun.sleep",
            "AgenTerm exposes explicit Task identity and state rather than JavaScript promises or async syntax",
        );
    }
    if stable_id.starts_with("rh.http.") {
        return similar_comparisons(
            "fetch",
            "fetch",
            "AgenTerm returns typed bounded streams and privacy-safe errors with explicit task ownership",
        );
    }
    if stable_id == "runtime.project.module-import" {
        return similar_comparisons(
            "ECMAScript modules",
            "ECMAScript modules",
            "AgenTerm resolves only deterministic project-root-relative Rh modules without network lookup",
        );
    }
    if stable_id == "runtime.project.named-task" {
        return similar_comparisons(
            "package.json scripts",
            "bun run / package.json scripts",
            "agenterm.tasks.json is a typed local task manifest, not a package or dependency manifest",
        );
    }
    if stable_id.starts_with("rh.runtime.") {
        return agenterm_specific_comparisons(
            "invocation-owned temporary resources and atomic publication are AgenTerm runtime contracts",
        );
    }
    if stable_id.starts_with("fleet.")
        || stable_id.starts_with("protocol.")
        || stable_id.starts_with("ui.")
        || stable_id.starts_with("workspace.")
        || stable_id.starts_with("tabs.")
        || stable_id.starts_with("pane.")
        || stable_id.starts_with("terminal.")
        || stable_id.starts_with("events.")
        || stable_id.starts_with("server.")
        || stable_id.starts_with("control-center.")
    {
        return agenterm_specific_comparisons(
            "Fleet identity, receipts, causal events, and terminal post-state are AgenTerm-specific",
        );
    }
    unreviewed_comparisons()
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
            assert!(!entry.designed_on.is_empty());
            assert!(!entry.since.is_empty());
            for analogue in [entry.comparisons.nodejs, entry.comparisons.bun] {
                assert_ne!(
                    analogue.relationship,
                    AnalogueRelationship::NotApplicable,
                    "{} has no reviewed analogue classification",
                    entry.stable_id
                );
                assert_eq!(analogue.reviewed_on, SCRIPT_COMPARISON_REVIEWED_ON);
                assert!(!analogue.reviewed_version.is_empty());
                assert!(analogue.documentation.starts_with("https://"));
                assert!(!analogue.semantic_note.is_empty());
                if analogue.relationship == AnalogueRelationship::Similar {
                    assert!(analogue.path.is_some());
                }
            }
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
                crate::operations::operation_by_id(entry.operation_id.unwrap())
                    .is_some_and(|operation| operation.available),
                "{} has no available operation",
                entry.stable_id
            );
        }
    }

    #[test]
    fn callable_frontend_operations_have_method_signatures() {
        let entries = entries();
        for (stable_id, expected) in [
            ("ui.window.activate", "fleet.ui.window.activate()"),
            ("terminal.paste", "fleet.terminal.paste()"),
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry.stable_id == stable_id)
                .unwrap();
            assert_eq!(entry.signature, expected);
        }
    }

    #[test]
    fn tcp_catalog_records_its_later_design_slice() {
        for entry in entries()
            .into_iter()
            .filter(|entry| entry.stable_id.starts_with("std.net."))
        {
            assert_eq!(entry.designed_on, "2026-07-30");
        }
    }

    #[test]
    fn every_shipped_api_is_available_under_every_legacy_profile_label() {
        for entry in entries()
            .into_iter()
            .filter(|entry| entry.status == ScriptApiStatus::Shipped)
        {
            assert_eq!(
                entry.profiles, SHIPPED_PROFILES,
                "{} still has profile-dependent availability",
                entry.stable_id
            );
        }
    }

    #[test]
    fn every_typed_operation_has_exactly_one_fleet_surface() {
        let entries = entries();
        for operation in OPERATION_CATALOG {
            let mapped = entries
                .iter()
                .filter(|entry| entry.operation_id == Some(operation.id))
                .collect::<Vec<_>>();
            assert_eq!(
                mapped.len(),
                1,
                "operation {} must map to exactly one Fleet API",
                operation.id
            );
            assert_eq!(mapped[0].surface_path, operation.script_surface);
            assert_eq!(mapped[0].operation, Some(operation));
        }
    }

    #[test]
    fn public_runtime_spec_starts_with_the_english_dated_object_tree() {
        let specification = include_str!("../docs/agenterm-rh-runtime.md");
        assert!(specification.starts_with("# AgenTerm Script Runtime Specification"));
        assert!(specification.contains("## 1. Complete public object and interface tree"));
        assert!(specification.matches("designed 2026-07-28").count() >= 60);
        assert!(specification.contains("The Script surface is the product contract."));
        assert!(
            !specification
                .chars()
                .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)),
            "the international runtime specification must remain English"
        );
    }

    #[test]
    fn catalog_publishes_one_reviewed_node_and_bun_comparison_per_entry() {
        let catalog = catalog();
        assert_eq!(catalog["schema_version"], SCRIPT_CATALOG_SCHEMA_VERSION);
        assert_eq!(
            catalog["comparison"]["schema_version"],
            SCRIPT_COMPARISON_SCHEMA_VERSION
        );
        assert_eq!(
            catalog["comparison"]["reviewed_on"],
            SCRIPT_COMPARISON_REVIEWED_ON
        );
        for entry in catalog["entries"].as_array().unwrap() {
            for ecosystem in ["nodejs", "bun"] {
                let comparison = &entry["comparisons"][ecosystem];
                assert!(comparison["relationship"].is_string());
                assert!(comparison["reviewed_version"].is_string());
                assert!(comparison["reviewed_on"].is_string());
                assert!(comparison["semantic_note"].is_string());
            }
        }
    }

    const NON_CALLABLE_SURFACE_TRIAGE: &[&str] = &[
        // Type-method/property groups, import and CLI surfaces documented in
        // the catalog but not expressible as one module-native call. Method
        // names inside these groups are still checked for registration below;
        // property getters (engine::register_get) are covered by the
        // black-box runtime suites under tests/.
        "std.fs.dir-entry-path",
        "std.fs.dir-entry-file-name",
        "std.fs.dir-entry-types",
        "std.fs.dir-entry-metadata",
        "std.fs.metadata-facts",
        "std.path.path-buf-join",
        "std.path.path-buf-display",
        "std.path.path-buf-file-name",
        "std.path.path-buf-extension",
        "std.path.path-buf-is-absolute",
        "std.time.system-time-unix-millis",
        "std.time.system-time-rfc3339",
        "std.net.tcp-stream",
        "std.net.tcp-listener",
        "std.process.command-builder",
        "std.process.command-output",
        "std.process.command-start",
        "std.process.command-status",
        "std.process.command-stdout-file",
        "std.process.child",
        "std.process.child-window-input",
        "std.process.output",
        "std.fs.exists-case-exact",
        "std.string.split",
        "rh.fail",
        "rh.stream.handle",
        "rh.bytes.length",
        "rh.bytes.raw-operations",
        "rh.bytes.to-text",
        "rh.task.handle",
        "rh.http.response",
        "runtime.project.module-import",
        "runtime.project.named-task",
    ];

    fn collect_module_surface(
        module: &rhai::Module,
        prefix: &str,
        out: &mut std::collections::HashMap<String, std::collections::HashSet<usize>>,
    ) {
        for signature in module.gen_fn_signatures_with_mapper(std::borrow::Cow::Borrowed) {
            let (name, params) = signature
                .split_once('(')
                .unwrap_or((signature.as_str(), ""));
            let params = params.trim_end_matches(')').trim();
            let arity = if params.is_empty() {
                0
            } else {
                params.split(',').count()
            };
            out.entry(format!("{prefix}{name}"))
                .or_default()
                .insert(arity);
        }
        for (name, sub) in module.iter_sub_modules() {
            collect_module_surface(sub, &format!("{prefix}{name}::"), out);
        }
    }

    fn catalog_arity(signature: &str) -> usize {
        let params = signature
            .split_once('(')
            .map(|(_, rest)| rest.split(')').next().unwrap_or(""))
            .unwrap_or("");
        if params.trim().is_empty() {
            0
        } else {
            params.split(',').count()
        }
    }

    fn documented_callee_names(signature: &str) -> std::collections::HashSet<&str> {
        signature
            .split('/')
            .filter_map(|part| {
                part.split_once('(')
                    .map(|(head, _)| head.trim().rsplit('.').next().unwrap_or("").trim())
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    #[test]
    fn every_shipped_plain_api_resolves_in_registered_surface() {
        let mut engine = rhai::Engine::new();
        let (std_module, host_api_module) = crate::script_stdlib::build_local_modules(&mut engine);
        let mut surface: std::collections::HashMap<String, std::collections::HashSet<usize>> =
            std::collections::HashMap::new();
        collect_module_surface(&std_module, "std::", &mut surface);
        collect_module_surface(&host_api_module, "rh::", &mut surface);
        let mut names: std::collections::HashSet<String> = surface
            .keys()
            .filter_map(|path| path.rsplit("::").next().map(str::to_owned))
            .collect();
        for signature in engine.gen_fn_signatures(true) {
            if let Some(name) = signature.split('(').next() {
                let params = signature
                    .split_once('(')
                    .map(|(_, rest)| rest.split(')').next().unwrap_or(""))
                    .unwrap_or("");
                let arity = if params.trim().is_empty() {
                    0
                } else {
                    params.split(',').count()
                };
                surface.entry(name.to_owned()).or_default().insert(arity);
                names.insert(name.to_owned());
            }
        }

        let mut triaged = Vec::new();
        for entry in entries().into_iter().filter(|entry| {
            entry.status == ScriptApiStatus::Shipped && entry.operation_id.is_none()
        }) {
            if entry
                .signature
                .starts_with(&format!("{}(", entry.surface_path))
            {
                let arity = catalog_arity(entry.signature);
                let registered = surface.get(entry.surface_path);
                assert!(
                    registered.is_some_and(|arities| arities.contains(&arity)),
                    "{}: surface {} with arity {} is not registered",
                    entry.stable_id,
                    entry.surface_path,
                    arity
                );
            } else {
                triaged.push(entry.stable_id);
                for callee in documented_callee_names(entry.signature) {
                    assert!(
                        names.contains(callee),
                        "{}: documented callee {} is not registered anywhere",
                        entry.stable_id,
                        callee
                    );
                }
            }
        }
        for stable_id in NON_CALLABLE_SURFACE_TRIAGE {
            assert!(
                triaged.contains(stable_id),
                "{stable_id} is triaged as non-callable but is directly callable"
            );
        }
        for stable_id in triaged {
            assert!(
                NON_CALLABLE_SURFACE_TRIAGE.contains(&stable_id),
                "{stable_id} is a non-direct surface and must be added to the triage allowlist"
            );
        }
    }
}
