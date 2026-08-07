//! Hypothesis-driven audit for top-level-only `.rhai` task entries that transpile to
//! native cdylib packs with `rh_entry_internal() -> 0` (silent no-op at task run).
//!
//! Enable transpile instrumentation:
//!   AGENTERM_RH_TRANSPILE_DEBUG=1 cargo test --test rh_top_level_noop_audit -- --nocapture
//! Logs append to `/opt/cursor/logs/debug.log` (NDJSON).

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[derive(Debug, Clone)]
struct AuditRow {
    id: String,
    entry: String,
    has_entry_fn: bool,
    mode: String,
    noop_stub: bool,
    compat: bool,
}

fn audit_entry(id: &str, entry: &str) -> AuditRow {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    let has_entry_fn = source.contains("fn entry(");
    let output = agenterm_rh::transpile_cdylib_with_mode(&source).unwrap_or_else(|error| {
        panic!("transpile {entry}: {error}");
    });
    AuditRow {
        id: id.to_owned(),
        entry: entry.to_owned(),
        has_entry_fn,
        mode: output.execution_mode.as_str().to_owned(),
        noop_stub: output
            .rust
            .contains("fn rh_entry_internal() -> INT {\n    0\n"),
        compat: output.rust.contains("compat delegating"),
    }
}

fn affected(row: &AuditRow) -> bool {
    row.noop_stub || (!row.has_entry_fn && row.mode != "compat-delegating")
}

#[test]
fn validate_artifact_manifest_transpiles_to_native_noop_stub() {
    let row = audit_entry(
        "validate-artifact-manifest",
        "scripts/rhai/validate-artifact-manifest.rhai",
    );
    eprintln!("{row:?}");
    assert!(!row.has_entry_fn);
    assert_eq!(row.mode, "native");
    assert!(
        row.noop_stub,
        "expected literal-zero rh_entry_internal stub"
    );
    assert!(!row.compat);
}

#[test]
fn manifest_lists_twenty_two_top_level_rhai_tasks_with_noop_or_non_compat_native() {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("agenterm.tasks.json")).expect("manifest"),
    )
    .expect("manifest json");
    let mut rows = Vec::new();
    for task in manifest["tasks"].as_array().expect("tasks") {
        let id = task["id"].as_str().expect("id");
        let entry = task["entry"].as_str().expect("entry");
        if !entry.ends_with(".rhai") {
            continue;
        }
        let row = audit_entry(id, entry);
        if affected(&row) {
            eprintln!(
                "AFFECTED {id}: mode={} has_entry_fn={} noop_stub={} compat={}",
                row.mode, row.has_entry_fn, row.noop_stub, row.compat
            );
            rows.push(row);
        }
    }
    eprintln!("TOTAL_AFFECTED={}", rows.len());
    assert_eq!(
        rows.len(),
        22,
        "expected 22 affected top-level .rhai tasks; rerun with AGENTERM_RH_TRANSPILE_DEBUG=1"
    );
}

#[test]
fn validate_artifact_manifest_task_run_is_silent_noop_with_wrong_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm-rh"))
        .current_dir(repo())
        .env("AGENTERM_RH_TRANSPILE_DEBUG", "1")
        .args([
            "task",
            "run",
            "validate-artifact-manifest",
            "--manifest",
            "agenterm.tasks.json",
            "--json",
            "--",
            "--wrong-arg-count",
        ])
        .output()
        .expect("run task");
    assert!(
        output.status.success(),
        "status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("task json envelope");
    eprintln!(
        "envelope={}",
        serde_json::to_string_pretty(&envelope).unwrap_or_default()
    );
    assert_eq!(
        envelope["ok"], true,
        "bug: wrong args should fail, not succeed"
    );
    assert_eq!(envelope["value"], 0);
    assert_eq!(envelope["stdout"].as_str(), Some(""));
}
