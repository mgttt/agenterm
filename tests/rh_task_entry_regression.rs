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

fn assert_bundled_pack_builds(entry: &str) {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    let dir = agenterm::script_rh_cache::compile_source_to_cache_with_project(
        &source,
        Some(&repo()),
    )
    .unwrap_or_else(|error| panic!("compile cache {entry}: {error}"));
    assert!(dir.join("manifest.json").is_file(), "{entry}");
}

fn assert_native_bundled_transpile(entry: &str, rust_needles: &[&str]) {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    assert!(source.contains("fn entry("), "{entry}");
    let bundled = agenterm_rh::bundle_project_source(&repo(), &source)
        .unwrap_or_else(|error| panic!("bundle {entry}: {error}"));
    let output = agenterm_rh::transpile_cdylib_with_mode(&bundled).unwrap_or_else(|error| {
        panic!("transpile bundled {entry}: {error}");
    });
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
    assert!(
        !output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"),
        "{entry}: {}",
        output.rust
    );
    assert!(
        !output.rust.contains("compat delegating"),
        "{entry}: {}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{entry} host-eval count: {}",
        output.rust
    );
}

fn assert_native_bundled_pack(entry: &str, rust_needles: &[&str]) {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    assert!(source.contains("fn entry("), "{entry}");
    let bundled = agenterm_rh::bundle_project_source(&repo(), &source)
        .unwrap_or_else(|error| panic!("bundle {entry}: {error}"));
    let output = agenterm_rh::transpile_cdylib_with_mode(&bundled).unwrap_or_else(|error| {
        panic!("transpile bundled {entry}: {error}");
    });
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

    let pack_dir = tempfile::tempdir().expect("pack dir");
    let pack = agenterm_rh::build_pack_dir(&bundled, pack_dir.path())
        .unwrap_or_else(|error| panic!("build pack {entry}: {error}"));
    assert!(pack.native_path.exists(), "{entry}");
    assert!(pack.manifest_path.exists(), "{entry}");
}

#[test]
fn check_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/check.rh");
}

#[test]
fn workbench_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/workbench-smoke.rh");
}

#[test]
fn control_center_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/control-center-smoke.rh");
}

#[test]
fn control_center_macos_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/control-center-macos-smoke.rh");
}

#[test]
fn control_center_linux_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/control-center-linux-smoke.rh");
}

#[test]
fn unix_frontend_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/unix-frontend-smoke.rh");
}

#[test]
fn fleet_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/fleet-smoke.rh");
}

#[test]
fn server_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/server-smoke.rh");
}

#[test]
fn native_ipc_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/native-ipc-smoke.rh");
}

#[test]
fn wake_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/wake-smoke.rh");
}

#[test]
fn script_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/script-smoke.rh");
}

#[test]
fn remote_ui_smoke_uses_bundled_pack() {
    assert_bundled_pack_builds("scripts/rh/remote-ui-smoke.rh");
}

#[test]
fn fresh_clone_rehearsal_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/fresh-clone-rehearsal.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn validate_artifact_manifest_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/validate-artifact-manifest.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("is_artifact_name("), "{}", output.rust);
    assert!(output.rust.contains("__validate("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn stage_artifact_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/stage-artifact.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("__stage("), "{}", output.rust);
    assert!(output.rust.contains("__stage_as("), "{}", output.rust);
    assert!(output.rust.contains("rh_try_copy("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn write_build_metadata_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/write-build-metadata.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert!(output.rust.contains("__write("), "{}", output.rust);
    assert!(output.rust.contains("__write_platform("), "{}", output.rust);
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
    assert!(output.rust.contains("__write("), "{}", output.rust);
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
    assert!(output.rust.contains("__stage("), "{}", output.rust);
    assert!(output.rust.contains("__write("), "{}", output.rust);
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
    assert_native_bundled_transpile(
        "scripts/rh/supply-chain.rh",
        &[
            "workspace_ids.insert(",
            "rh_json_array_push(",
            "rh_process_stdout_file(",
            "rh_json_parse(",
            "rh_atomic_write(",
        ],
    );
}

#[test]
fn diagnostic_bundle_selftest_uses_native_bundled_execution() {
    assert_native_bundled_transpile(
        "scripts/rh/diagnostic-bundle-selftest.rh",
        &[
            "rh_json_set_path_key(",
            "rh_json_array_get(&matches, ",
            "rh_json_set_path_index(",
            "diagnostic_new_bundle_count",
            "rh_process_stdout_file(",
        ],
    );
}

#[test]
fn task_manifest_has_no_live_rhai_entry_paths() {
    let repo = repo();
    let manifest_text =
        std::fs::read_to_string(repo.join("agenterm.tasks.json")).expect("task manifest");
    assert!(
        !manifest_text.contains(".rhai\""),
        "agenterm.tasks.json must not reference .rhai task entries"
    );
    assert!(
        !manifest_text.contains("scripts/rhai/"),
        "agenterm.tasks.json must not reference live scripts/rhai/ paths"
    );

    let entries = agenterm_rh::extract_task_entries(&repo.join("agenterm.tasks.json"))
        .expect("task entries");
    assert!(
        entries.iter().all(|entry| entry.ends_with(".rh") || entry.ends_with(".lua")),
        "unexpected non-.rh/.lua task entry: {entries:?}"
    );
    assert!(
        !entries.iter().any(|entry| entry.contains("scripts/rhai/")),
        "live scripts/rhai/ task entry still present: {entries:?}"
    );
    assert!(
        !entries.iter().any(|entry| entry.ends_with(".rhai")),
        ".rhai task entry still present: {entries:?}"
    );
}

fn assert_host_eval_or_native_bundled_pack(entry: &str) {
    let source = std::fs::read_to_string(repo().join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    let bundled = agenterm_rh::bundle_project_source(&repo(), &source)
        .unwrap_or_else(|error| panic!("bundle {entry}: {error}"));
    let output = agenterm_rh::transpile_cdylib_with_mode(&bundled).unwrap_or_else(|error| {
        panic!("transpile bundled {entry}: {error}");
    });
    assert!(
        matches!(
            output.execution_mode,
            agenterm_rh::CdylibExecutionMode::HostEval
                | agenterm_rh::CdylibExecutionMode::Native
        ),
        "{entry}: expected host-eval/native after Phase B cutover, got {:?}",
        output.execution_mode
    );
    assert!(
        !output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"),
        "{entry}: must not whole-script compat-delegate"
    );
    assert!(
        !output.rust.contains("compat delegating"),
        "{entry}: must not emit compat-delegating marker"
    );
}

#[test]
fn phase_b_smoke_entries_use_host_eval_or_native_pack() {
    for entry in [
        "scripts/rh/platform-ux-parity-smoke.rh",
        "scripts/rh/script-smoke.rh",
        "scripts/rh/remote-ui-smoke.rh",
        "scripts/rh/workbench-smoke.rh",
        "scripts/rh/unix-frontend-smoke.rh",
        "scripts/rh/fresh-clone-rehearsal.rh",
    ] {
        assert_host_eval_or_native_bundled_pack(entry);
    }
}


#[test]
fn script_smoke_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/script-smoke.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn workbench_smoke_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/workbench-smoke.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}


#[test]
fn unix_frontend_smoke_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/unix-frontend-smoke.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn remote_ui_smoke_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/remote-ui-smoke.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

#[test]
fn working_context_smoke_uses_native_bundled_execution() {
    let (source, output) = transpile_project_entry("scripts/rh/working-context-smoke.rh");
    assert!(source.contains("fn entry("));
    assert_eq!(output.execution_mode.as_str(), "native");
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
}

fn assert_bundled_aot_pack_builds(entry: &str) {
    // Lock AOT compile success (not bare qualify entry_value): HostEval packs still
    // need a rich embedding host to return 0, but Native smokes must compile clean.
    let root = repo();
    let source = std::fs::read_to_string(root.join(entry)).unwrap_or_else(|error| {
        panic!("read {entry}: {error}");
    });
    let bundled = agenterm_rh::bundle_project_source(&root, &source)
        .unwrap_or_else(|error| panic!("bundle {entry}: {error}"));
    let output = agenterm_rh::transpile_cdylib_with_mode(&bundled).unwrap_or_else(|error| {
        panic!("transpile bundled {entry}: {error}");
    });
    assert!(
        matches!(
            output.execution_mode,
            agenterm_rh::CdylibExecutionMode::HostEval
                | agenterm_rh::CdylibExecutionMode::Native
        ),
        "{entry}: {:?}",
        output.execution_mode
    );
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert!(!output.rust.contains("compat delegating"));
    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-aot-pack-{}-{}",
        entry.replace('/', "_").replace('.', "_"),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let built = agenterm_rh::build_pack_dir(&bundled, &dir)
        .unwrap_or_else(|error| panic!("build_pack_dir {entry}: {error}"));
    assert!(
        built.native_path.is_file(),
        "{entry}: missing {}",
        built.native_path.display()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fresh_clone_rehearsal_pack_builds() {
    assert_bundled_aot_pack_builds("scripts/rh/fresh-clone-rehearsal.rh");
}

#[test]
fn workbench_smoke_pack_builds() {
    assert_bundled_aot_pack_builds("scripts/rh/workbench-smoke.rh");
}

#[test]
fn unix_frontend_smoke_pack_builds() {
    assert_bundled_aot_pack_builds("scripts/rh/unix-frontend-smoke.rh");
}

#[test]
fn working_context_smoke_pack_builds() {
    assert_bundled_aot_pack_builds("scripts/rh/working-context-smoke.rh");
}

#[test]
fn script_smoke_pack_builds() {
    assert_bundled_aot_pack_builds("scripts/rh/script-smoke.rh");
}

#[test]
fn remote_ui_smoke_pack_builds() {
    assert_bundled_aot_pack_builds("scripts/rh/remote-ui-smoke.rh");
}

#[test]
fn working_context_smoke_is_past_compat_delegating() {
    assert_host_eval_or_native_bundled_pack("scripts/rh/working-context-smoke.rh");
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
            "rh_host_json_call(\"process.platform_facts\"",
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
            "rh_host_json_call(\"image.inspect_png\"",
        ],
    );
}

#[test]
fn native_ipc_compat_smoke_uses_bundled_pack() {
    // Still host-eval (closure HE>1); pack build is the Phase B gate.
    assert_bundled_pack_builds("scripts/rh/native-ipc-compat-smoke.rh");
}
#[test]
fn server_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/server-smoke.rh",
        &[
            "rh_process_status(",
            "server_smoke_workspace_missing",
            "rh_host_json_call(\"process.platform_facts\"",
        ],
    );
}

#[test]
fn wake_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/wake-smoke.rh",
        &[
            "rh_process_status(",
            "wake_smoke",
            "rh_host_json_call(\"process.platform_facts\"",
        ],
    );
}

#[test]
fn fleet_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/fleet-smoke.rh",
        &[
            "rh_process_status(",
            "fleet_executable_missing",
            "rh_host_json_call(\"process.platform_facts\"",
        ],
    );
}

#[test]
fn native_ipc_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/native-ipc-smoke.rh",
        &[
            "rh_process_status(",
            "native_ipc_server_missing",
            "rh_host_json_call(\"process.platform_facts\"",
        ],
    );
}

#[test]
fn check_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/check.rh",
        &[
            "rh_json_parse(",
            "rh_json_stringify(",
            "pub fn wrap_timing_new(",
        ],
    );
}

#[test]
fn control_center_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/control-center-smoke.rh",
        &[
            "rh_process_stdout_file(",
            "rh_host_json_call(\"process.list\"",
        ],
    );
}

#[test]
fn control_center_macos_smoke_uses_native_bundled_pack() {
    assert_native_bundled_pack(
        "scripts/rh/control-center-macos-smoke.rh",
        &[
            "rh_process_stdout_file(",
            "rh_host_json_call(\"process.list\"",
        ],
    );
}
