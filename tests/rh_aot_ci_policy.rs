use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci.yml").replace("\r\n", "\n"));
static WINDOWS_FULL_GATE: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/win-full-gate.yml").replace("\r\n", "\n"));
static CANDIDATE: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/candidate.yml").replace("\r\n", "\n"));
static RELEASE: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/release.yml").replace("\r\n", "\n"));
static PERFORMANCE_EXPERIMENT: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/performance-experiment.yml").replace("\r\n", "\n")
});
static CHECK: LazyLock<String> =
    LazyLock::new(|| include_str!("../scripts/rh/check.rh").replace("\r\n", "\n"));
static ARTIFACT_VERIFICATION: LazyLock<String> =
    LazyLock::new(|| include_str!("../scripts/rh/artifact-verification.rh").replace("\r\n", "\n"));
static CLIENT_SMOKE: LazyLock<String> =
    LazyLock::new(|| include_str!("../scripts/rh/client-smoke.rh").replace("\r\n", "\n"));
static ROOT_MANIFEST: &str = include_str!("../Cargo.toml");
static RH_MANIFEST: &str = include_str!("../crates/agenterm-rh/Cargo.toml");
static UNIX_BOOTSTRAP: &str = include_str!("../scripts/bootstrap.sh");
static WINDOWS_BOOTSTRAP: &str = include_str!("../scripts/bootstrap.cmd");
static ARTIFACT_MANIFEST: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../scripts/artifacts.json"))
        .expect("scripts/artifacts.json must be valid JSON")
});

fn job_span(name: &str, next_job: Option<&str>) -> &'static str {
    let marker = format!("  {name}:\n");
    let start = WORKFLOW
        .find(marker.as_str())
        .unwrap_or_else(|| panic!("missing CI job: {name}"));
    let end = next_job
        .and_then(|next| WORKFLOW.find(&format!("\n  {next}:\n")))
        .unwrap_or(WORKFLOW.len());
    assert!(end > start, "CI job span for {name} is empty");
    &WORKFLOW[start..end]
}

fn normalized_task_callers(source: &str) -> String {
    source
        .replace('\\', "/")
        .replace(".exe", "")
        .replace('"', "")
}

#[test]
fn ci_manifest_task_entrypoints_use_rh_front_door() {
    for (name, source) in [
        ("ci", WORKFLOW.as_str()),
        ("win-full-gate", WINDOWS_FULL_GATE.as_str()),
        ("candidate", CANDIDATE.as_str()),
        ("performance-experiment", PERFORMANCE_EXPERIMENT.as_str()),
    ] {
        let normalized = normalized_task_callers(source);
        let task_runs = normalized.matches("task run").count();
        // Post-retirement front door: `agenterm rh task run ...` (the main
        // PE's rh subcommand), never the retired standalone binary.
        let rh_task_runs = normalized.matches(" rh task run").count();
        assert!(task_runs > 0, "{name} must retain manifest task coverage");
        assert_eq!(
            rh_task_runs, task_runs,
            "{name} has a task run outside the `agenterm rh` front door"
        );
        assert!(
            !normalized.contains("agenterm-rh task run"),
            "{name} still calls the retired standalone agenterm-rh binary"
        );
        assert!(
            !normalized.contains("agenterm-rhai task run"),
            "{name} regressed to the compatibility CLI"
        );
    }
}

#[test]
fn candidate_and_release_use_only_rh_scripting_front_door() {
    // Post-retirement (engine exes folded into the main PE as subcommands,
    // 2026-08-09): the scripting front door is `agenterm rh ...`, and no
    // workflow may reference the retired standalone binaries.
    assert!(CANDIDATE.contains(
        "target/aarch64-apple-darwin/release/agenterm rh \\\n            run scripts/rh/finalize-macos-provenance.rh"
    ));
    assert!(CANDIDATE.contains(
        "target/x86_64-apple-darwin/release/agenterm rh \\\n            run scripts/rh/finalize-macos-provenance.rh"
    ));
    assert!(CANDIDATE.contains("chmod +x \"$RUNNER_TEMP/agenterm-candidate-tool/agenterm\""));
    assert!(CANDIDATE.contains(
        "\"$RUNNER_TEMP/agenterm-candidate-tool/agenterm\" rh \\\n            run scripts/rh/candidate-aggregate.rh"
    ));
    assert!(RELEASE.contains("chmod +x \"$RUNNER_TEMP/agenterm-promotion-tool/agenterm\""));
    assert!(RELEASE.contains(
        "\"$RUNNER_TEMP/agenterm-promotion-tool/agenterm\" rh \\\n            run scripts/rh/candidate-verify.rh"
    ));
    assert!(!CANDIDATE.contains("agenterm-rh\""));
    assert!(!RELEASE.contains("agenterm-rh\""));
    assert!(!CANDIDATE.contains("agenterm-rhai"));
    assert!(!RELEASE.contains("agenterm-rhai"));
}

#[test]
fn performance_experiment_uses_rh_for_build_and_manifest_tasks() {
    // Post-retirement: the experiment builds the main PE and drives rh
    // tasks through the `agenterm rh` subcommand front door.
    let normalized = normalized_task_callers(&PERFORMANCE_EXPERIMENT);
    assert!(normalized.contains("cargo build --quiet --locked --bin agenterm"));
    assert!(normalized.contains("%RUNNER_TEMP%/agenterm rh task run performance-samples"));
    assert!(normalized.contains("%RUNNER_TEMP%/agenterm rh task run performance-summary"));
    assert!(!normalized.contains("--bin agenterm-rh"));
    assert!(!normalized.contains("agenterm-rhai"));
}

#[test]
fn dist_task_worker_requires_rh_cli_without_compatibility_fallback() {
    let rh = CHECK
        .find("\"dist/agenterm-rh.exe\"")
        .expect("dist rh candidates");
    let selection = CHECK
        .find("let worker = resolve_worker(dist_rh_cli, task_rh_cli)")
        .expect("rh worker selection");
    assert!(rh < selection);
    assert!(CHECK.contains("check_dist_task_worker_missing"));
    assert!(!CHECK.contains("COMPATIBILITY FALLBACK"));
}

#[test]
fn artifact_manifest_declares_rh_dev_cli_offline_version_probe() {
    let executables = ARTIFACT_MANIFEST["executables"]
        .as_array()
        .expect("manifest executables");
    // Post-retirement: the rh-dev-cli role (standalone agenterm-rh.exe) is
    // gone the same way scripting-cli went before it — rh scripting rides
    // the main PE (`agenterm rh ...`), which needs no separate artifact.
    assert!(
        !executables
            .iter()
            .any(|entry| entry["role"] == "rh-dev-cli"),
        "retired rh-dev-cli role must not reappear"
    );
    assert!(
        !executables.iter().any(|entry| entry["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("agenterm-rh"))),
        "retired agenterm-rh executable must not reappear in the manifest"
    );
    assert!(
        !executables
            .iter()
            .any(|entry| entry["role"] == "scripting-cli"),
        "retired scripting-cli role must not reappear"
    );
}

#[test]
fn artifact_verification_carries_no_retired_engine_cli_probes() {
    // Post-retirement rewrite: artifact verification validates roles via
    // scripts/rh/lib/artifact_manifest and must not re-grow probes for the
    // retired standalone engine CLIs (rh-dev-cli went the way of
    // scripting-cli).
    assert!(ARTIFACT_VERIFICATION.contains("fn entry("));
    assert!(ARTIFACT_VERIFICATION.contains("artifact_manifest::validate("));
    assert!(!ARTIFACT_VERIFICATION.contains("\"rh-dev-cli\""));
    assert!(!ARTIFACT_VERIFICATION.contains("agenterm-rh.exe"));
    assert!(!ARTIFACT_VERIFICATION.contains("\"scripting-cli\""));
}

#[test]
fn client_smoke_fail_closes_rh_version_probe_from_platform_manifest() {
    assert!(CLIENT_SMOKE.contains("fn entry("));
    assert!(CLIENT_SMOKE.contains("metadata.is_file && metadata.len > 0"));
    assert!(CLIENT_SMOKE.contains("executable_role == \"rh-dev-cli\""));
    assert!(CLIENT_SMOKE.contains("probe_arg == \"version\""));
    assert!(CLIENT_SMOKE.contains("std::process::command_status("));
    assert!(CLIENT_SMOKE.contains("std::process::command_stdout_file("));
    assert!(CLIENT_SMOKE.contains("banner == \"agenterm-rh \" + package_version"));
    assert!(CLIENT_SMOKE.contains("rh_dev_cli_count == 1"));
    assert!(!CLIENT_SMOKE.contains("scripting_cli_count"));
    assert!(!CLIENT_SMOKE.contains("executable_role == \"scripting-cli\""));
}

#[test]
fn linux_x86_64_ci_proves_rh_aot_pipeline() {
    let job = job_span("linux-x86_64", Some("linux-aarch64"));
    assert!(job.contains("./rh-check.sh"));
    assert!(job.contains("rh_regression") || job.contains("rh-check"));
}

#[test]
fn windows_ci_proves_rh_aot_pipeline() {
    let job = job_span("windows", Some("linux-x86_64"));
    assert!(job.contains("./rh-check.sh"));
    assert!(job.contains("rh-aot-smoke"));
}

#[test]
fn cross_cells_compile_rh_for_target() {
    let linux = job_span("linux-aarch64", Some("macos"));
    assert!(linux.contains("cargo check -p agenterm-rh --locked"));
    assert!(linux.contains("aarch64-unknown-linux-gnu"));
    assert!(linux.contains("AGENTERM_RH_QUALIFY_TARGET"));

    let windows = job_span("windows-aarch64", None);
    assert!(windows.contains("cargo check -p agenterm-rh --locked"));
    assert!(windows.contains("aarch64-pc-windows-msvc"));
    assert!(windows.contains("AGENTERM_RH_QUALIFY_TARGET"));
}

#[test]
fn macos_ci_cross_compiles_rh_reference_pack() {
    let job = job_span("macos", Some("windows-aarch64"));
    assert!(job.contains("AGENTERM_RH_QUALIFY_TARGET"));
    assert!(job.contains("cargo check -p agenterm-rh --locked"));
    assert!(job.contains("cross_compiles_reference_pack_when_target_env_set"));
}

#[test]
fn rh_rides_the_main_binary_with_no_standalone_bin_target() {
    // Post-retirement: the standalone agenterm-rh [[bin]] is gone — rh's
    // CLI lives in the root lib (script_rh_cli_main) behind the
    // `agenterm rh` subcommand, and bootstrap builds the single main PE.
    let retired_bin = "name = \"agenterm-rh\"";
    assert!(!ROOT_MANIFEST.contains(retired_bin));
    assert!(RH_MANIFEST.contains("autobins = false"));
    assert!(!RH_MANIFEST.contains("[[bin]]"));

    let root_build = "cargo build --quiet --locked --bin agenterm";
    let retired_build = "--bin agenterm-rh";
    assert!(UNIX_BOOTSTRAP.contains(root_build));
    assert!(WINDOWS_BOOTSTRAP.contains(root_build));
    assert!(!UNIX_BOOTSTRAP.contains(retired_build));
    assert!(!WINDOWS_BOOTSTRAP.contains(retired_build));
}
