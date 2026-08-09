//! Native check locks for newly added fixtures/rh process and bytes probes.

use std::path::PathBuf;

use agenterm_rh::{CdylibExecutionMode, check, transpile_cdylib_with_mode};

const NEW_PUBLIC_PROBES: [(&str, &[&str]); 28] = [
    (
        "bytes-append-probe.rh",
        &["rh_bytes_append(", "rh_bytes_from_text("],
    ),
    ("child-stdout-probe.rh", &["rh_child_stdout("]),
    (
        "command-arg-probe.rh",
        &["rh_command_arg(", "String::from(\"--probe\")"],
    ),
    (
        "command-args-json-probe.rh",
        &["rh_command_args(", "rh_json_string_argv("],
    ),
    (
        "command-stdin-text-probe.rh",
        &["rh_command_stdin_text(", "String::from(\"hello\\n\")"],
    ),
    (
        "duration-task-sleep-probe.rh",
        &["std::thread::sleep(std::time::Duration::from_millis("],
    ),
    ("env-current-dir-probe.rh", &["rh_env_current_dir()"]),
    (
        "env-parse-int-probe.rh",
        &["rh_env_has(", "rh_env_parse_int("],
    ),
    (
        "json-marker-run-probe.rh",
        &[
            "rh_json_string_path(&marker_run, &[\"text\"])",
            "rh_json_get_path(&marker_run, &[\"row\"])",
            "rh_json_get_path(&marker_run, &[\"column\"])",
        ],
    ),
    ("json-stringify-probe.rh", &["rh_json_stringify("]),
    ("path-join-probe.rh", &["rh_path_join("]),
    ("process-kill-probe.rh", &["rh_process_kill(4242)"]),
    ("std-fs-write-probe.rh", &["rh_std_fs_write("]),
    (
        "string-list-set-probe.rh",
        &["rh_string_list_set(&mut parts,"],
    ),
    (
        "bytes-from-text-probe.rh",
        &["rh_bytes_from_text(&String::from(\"agenterm-rh-probe\"))"],
    ),
    (
        "bytes-to-text-probe.rh",
        &["rh_bytes_from_text(", "rh_bytes_to_text(&payload)"],
    ),
    (
        "system-time-unix-millis-probe.rh",
        &["rh_system_time_now_unix_millis()"],
    ),
    ("path-is-absolute-probe.rh", &["rh_path_is_absolute("]),
    ("string-split-probe.rh", &["rh_string_split("]),
    ("std-fs-exists-probe.rh", &["rh_std_fs_exists("]),
    (
        "json-parse-file-probe.rh",
        &["rh_json_parse(&rh_std_fs_read_to_string("],
    ),
    ("string-to-lower-probe.rh", &[".to_ascii_lowercase()"]),
    ("string-contains-probe.rh", &[".contains("]),
    (
        "direntry-file-name-probe.rh",
        &["for entry in rh_read_dir(", "entry.file_name"],
    ),
    (
        "direntry-is-file-probe.rh",
        &["for entry in rh_read_dir(", "entry.is_file"],
    ),
    (
        "std-fs-read-to-string-probe.rh",
        &["rh_std_fs_read_to_string("],
    ),
    // `"x" + json_number` must transpile through the JSON string helpers,
    // whose prelude now stringifies scalars (numbers/bools) instead of
    // hard-failing — the interpreter/native divergence that broke every
    // gate message concatenating a JSON number.
    ("json-scalar-concat-probe.rh", &["rh_json_string_path("]),
    // json == json must emit a real serde_json::Value equality (null-safe),
    // never the fail-closed string coercion the qualification gate died on.
    ("json-null-eq-probe.rh", &["== null_json())) as INT"]),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn read_public_probe(name: &str) -> String {
    std::fs::read_to_string(repo_root().join("fixtures/rh").join(name))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn assert_native_probe(name: &str, needles: &[&str]) {
    let source = read_public_probe(name);
    check(&source).unwrap_or_else(|error| panic!("check {name}: {error}"));
    let output = transpile_cdylib_with_mode(&source)
        .unwrap_or_else(|error| panic!("transpile {name}: {error}"));
    assert_ne!(
        output.execution_mode,
        CdylibExecutionMode::CompatDelegating,
        "{name} must not compat-delegate: {}",
        output.rust
    );
    assert!(
        !output.rust.contains("compat delegating"),
        "{name}: {}",
        output.rust
    );
    for needle in needles {
        assert!(
            output.rust.contains(needle),
            "{name} missing {needle:?}: {}",
            output.rust
        );
    }
}

#[test]
fn new_public_probes_check_and_emit_native_code() {
    for (name, needles) in NEW_PUBLIC_PROBES {
        assert_native_probe(name, needles);
    }
}
