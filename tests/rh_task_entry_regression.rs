//! Regression coverage for native `.rh` task cutover entries (including pack
//! bundle/build qualification) and remaining Rhai-backed task entries where
//! applicable.

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

fn assert_native_bundled_pack(entry: &str, rust_needles: &[&str]) {
    let (source, output) = transpile_project_entry(entry);
    assert!(source.contains("fn entry("), "{entry}");
    assert_eq!(
        output.execution_mode.as_str(),
        "native",
        "{entry}: {}",
        output.rust
    );
    for needle in rust_needles {
        assert!(
            output.rust.contains(needle),
            "{entry} missing {needle:?}: {}",
            output.rust
        );
    }
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{entry} host-eval count: {}",
        output.rust
    );

    let bundled = agenterm_rh::bundle_project_source(&repo(), &source)
        .unwrap_or_else(|error| panic!("bundle {entry}: {error}"));
    let pack_dir = tempfile::tempdir().expect("pack dir");
    let pack = agenterm_rh::build_pack_dir(&bundled, pack_dir.path())
        .unwrap_or_else(|error| panic!("build pack {entry}: {error}"));
    assert!(pack.native_path.exists(), "{entry}");
    assert!(pack.manifest_path.exists(), "{entry}");
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
fn build_releases_index_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/build-releases-index.rh");
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
fn rh_aot_smoke_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/rh-aot-smoke.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn client_smoke_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/client-smoke.rh");
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
fn preflight_benchmark_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/preflight-benchmark.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn cross_platform_automation_audit_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/cross-platform-automation-audit.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_array_items("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn artifact_verification_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/artifact-verification.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn lint_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/lint.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn supply_chain_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/supply-chain.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_array_push("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
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
	fn migration_audit_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/powershell-migration-audit.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
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

#[test]
fn prd_alignment_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/prd-alignment.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}




#[test]
fn preflight_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/preflight.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn target_report_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/target-report.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_read_dir("), "{}", output.rust);
    assert!(output.rust.contains("rh_path_absolute("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn prune_target_incremental_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/prune-target-incremental.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_process_stdout_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(output.rust.contains("rh_path_join("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}


#[test]
fn package_qualified_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/package-qualified.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn candidate_aggregate_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/candidate-aggregate.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native", "{}", output.rust);
    assert!(output.rust.contains("pub fn build_manifest("), "{}", output.rust);
    assert!(output.rust.contains("pub fn validate_manifest("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse(&rh_std_fs_read_to_string("), "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("serde_json::Value::Bool("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn candidate_verify_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/candidate-verify.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native", "{}", output.rust);
    assert!(output.rust.contains("pub fn verify_platform("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_parse(&rh_std_fs_read_to_string("), "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_path_join("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn package_release_qualified_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/package-release-qualified.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn package_client_release_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/package-client-release.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native", "{}", output.rust);
    assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
    assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
    assert!(output.rust.contains("rh_json_stringify_pretty("), "{}", output.rust);
    assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn package_qualified_selftest_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/package-qualified-selftest.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native", "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn startup_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/startup-smoke.rh",
        &[
            "rh_child_stderr(",
            "rh_stream_read(",
            "rh_bytes_to_text(",
            "rh_host_json_call(\"process.list\"",
        ],
    );
}

#[test]
fn cli_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/cli-smoke.rh",
        &[
            "rh_process_stdout_file(",
            "rh_host_json_call(\"process.list\"",
            "cli_smoke_gui_missing",
        ],
    );
}

#[test]
fn release_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/release.rh",
        &[
            "rh_process_stdout_file(",
            "rh_sha256_file(",
            "build_release_package(",
            "release_branch_not_main",
        ],
    );
}

#[test]
fn promotion_identity_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/promotion-identity.rh",
        &[
            "rh_sha256_file(",
            "rh_atomic_write(",
            "agenterm-promotion-identity",
            "promotion_identity_manifest",
        ],
    );
}

#[test]
fn qualification_selftest_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/qualification-selftest.rh",
        &[
            "rh_process_stdout_file(",
            "qualification_selftest_expected_rejection",
            "rh_json_stringify_pretty(",
        ],
    );
}

#[test]
fn theme_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/theme-smoke.rh",
        &[
            "rh_process_stdout_file(",
            "theme_server_timeout",
            "rh_host_json_call(\"process.list\"",
        ],
    );
}

#[test]
fn native_ipc_compat_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/native-ipc-compat-smoke.rh",
        &[
            "rh_process_status(",
            "native_ipc_compat_command_failed",
            "rh_host_json_call(\"process.list\"",
        ],
    );
}
