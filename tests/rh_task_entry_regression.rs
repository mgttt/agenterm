//! Regression coverage for top-level-only `.rhai` task entries and native `.rh`
//! task qualification.

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn transpile_entry(entry: &str) -> (String, agenterm_rh::CdylibTranspileOutput) {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    let output = agenterm_rh::transpile_cdylib_with_mode(&source).unwrap_or_else(|error| {
        panic!("transpile {entry}: {error}");
    });
    (source, output)
}

fn transpile_project_entry(entry: &str) -> (String, agenterm_rh::CdylibTranspileOutput) {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    let output = agenterm_rh::transpile_cdylib_with_project(&repo(), &source).unwrap_or_else(
        |error| {
            panic!("transpile {entry}: {error}");
        },
    );
    (source, output)
}

#[test]
fn validate_artifact_manifest_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/validate-artifact-manifest.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("pub fn is_artifact_name("), "{}", output.rust);
    assert!(output.rust.contains("pub fn validate("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn stage_artifact_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/stage-artifact.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("pub fn stage("), "{}", output.rust);
    assert!(output.rust.contains("pub fn stage_as("), "{}", output.rust);
    assert!(output.rust.contains("rh_try_copy("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn write_build_metadata_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/write-build-metadata.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("pub fn write("), "{}", output.rust);
    assert!(output.rust.contains("pub fn write_platform("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(
        output.rust.contains("serde_json::Value::Bool("),
        "{}",
        output.rust
    );
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn build_identity_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/build-identity.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("pub fn write("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn timing_summary_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/timing-summary.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn bootstrap_info_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/bootstrap-info.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn prepare_target_clean_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/prepare-target-clean.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn stage_build_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/stage-build.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("pub fn stage("), "{}", output.rust);
    assert!(output.rust.contains("pub fn write("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn mcp_conformance_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/mcp-conformance.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_metadata("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn performance_samples_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/performance-samples.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn performance_summary_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/performance-summary.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn agenterm_net_research_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/agenterm-net-research.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn readme_examples_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/readme-examples.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn manifest_top_level_rhai_tasks_use_compatibility_execution() {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("agenterm.tasks.json")).expect("manifest"),
    )
    .expect("manifest json");
    let mut invalid = Vec::new();
    for task in manifest["tasks"].as_array().expect("tasks") {
        let id = task["id"].as_str().expect("id");
        let entry = task["entry"].as_str().expect("entry");
        if !entry.ends_with(".rhai") {
            continue;
        }
        let (source, output) = transpile_entry(entry);
        if output
            .rust
            .contains("fn rh_entry_internal() -> INT {\n    0\n")
            || (!source.contains("fn entry(")
                && output.execution_mode.as_str() != "compat-delegating")
        {
            invalid.push(format!(
                "{id} ({entry}): mode={}",
                output.execution_mode.as_str()
            ));
        }
    }
    assert!(
        invalid.is_empty(),
        "top-level .rhai tasks must use compatibility execution: {invalid:?}"
    );
}

#[test]
fn validate_artifact_manifest_task_run_fails_with_wrong_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm-rh"))
        .current_dir(repo())
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
