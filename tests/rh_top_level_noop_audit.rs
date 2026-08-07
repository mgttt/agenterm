//! Regression coverage for top-level-only `.rhai` task entries and native `.rh`
//! task qualification.
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
    has_entry_fn: bool,
    mode: String,
    noop_stub: bool,
    compat: bool,
}

fn audit_entry(entry: &str) -> AuditRow {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    let has_entry_fn = source.contains("fn entry(");
    let output = agenterm_rh::transpile_cdylib_with_mode(&source).unwrap_or_else(|error| {
        panic!("transpile {entry}: {error}");
    });
    AuditRow {
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
fn validate_artifact_manifest_uses_top_level_compatibility_execution() {
    let row = audit_entry("scripts/rhai/validate-artifact-manifest.rhai");
    eprintln!("{row:?}");
    assert!(!row.has_entry_fn);
    assert_eq!(row.mode, "compat-delegating");
    assert!(!row.noop_stub);
    assert!(row.compat);
}

#[test]
fn manifest_has_no_top_level_rhai_tasks_with_noop_or_non_compat_native_entry() {
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
        let row = audit_entry(entry);
        if affected(&row) {
            eprintln!(
                "AFFECTED {id} ({entry}): mode={} has_entry_fn={} noop_stub={} compat={}",
                row.mode, row.has_entry_fn, row.noop_stub, row.compat
            );
            rows.push(row);
        }
    }
    eprintln!("TOTAL_AFFECTED={}", rows.len());
    assert_eq!(
        rows.len(),
        0,
        "top-level .rhai tasks must use compatibility execution; rerun with AGENTERM_RH_TRANSPILE_DEBUG=1"
    );
}

#[test]
fn validate_artifact_manifest_task_run_fails_with_wrong_args() {
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
    assert_eq!(output.status.code(), Some(2));
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("task json envelope");
    eprintln!(
        "envelope={}",
        serde_json::to_string_pretty(&envelope).unwrap_or_default()
    );
    assert_eq!(envelope["ok"], false);
    assert!(
        envelope["failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("expected: ARTIFACT_MANIFEST")),
        "{envelope}"
    );
    assert_eq!(envelope["stdout"].as_str(), Some(""));
}

#[test]
fn native_rh_task_without_entry_fails_manifest_qualification() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scripts = temp.path().join("scripts");
    std::fs::create_dir_all(&scripts).expect("scripts dir");
    std::fs::write(
        scripts.join("top-level.rh"),
        "if args.len != 1 { throw \"expected one argument\"; }\n0\n",
    )
    .expect("native task source");
    let manifest = temp.path().join("agenterm.tasks.json");
    std::fs::write(&manifest, r#"{"tasks":[{"entry":"scripts/top-level.rh"}]}"#)
        .expect("task manifest");

    let report = agenterm_rh::scan_task_manifest(agenterm_rh::CorpusScanOptions {
        project_root: temp.path().to_owned(),
        tasks_manifest: Some(manifest),
        ..agenterm_rh::CorpusScanOptions::default()
    })
    .expect("qualification report");

    assert_eq!(report.passed, 0);
    assert_eq!(report.failed, 1);
    assert!(
        report.entries[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("native .rh task requires compat-delegating")),
        "{report:?}"
    );
}
