//! Native check locks for newly added fixtures/rh process and bytes probes.

use std::path::PathBuf;

use agenterm_rh::{CdylibExecutionMode, check, transpile_cdylib_with_mode};

const NEW_PUBLIC_PROBES: [(&str, &[&str]); 8] = [
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
        "json-marker-run-probe.rh",
        &[
            "rh_json_string_path(&marker_run, &[\"text\"])",
            "rh_json_get_path(&marker_run, &[\"row\"])",
            "rh_json_get_path(&marker_run, &[\"column\"])",
        ],
    ),
    ("process-kill-probe.rh", &["rh_process_kill(4242)"]),
    ("std-fs-write-probe.rh", &["rh_std_fs_write("]),
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
