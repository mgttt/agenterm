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
    LazyLock::new(|| include_str!("../scripts/rhai/check.rhai").replace("\r\n", "\n"));
static ARTIFACT_VERIFICATION: LazyLock<String> = LazyLock::new(|| {
    include_str!("../scripts/rhai/artifact-verification.rhai").replace("\r\n", "\n")
});
static CLIENT_SMOKE: LazyLock<String> =
    LazyLock::new(|| include_str!("../scripts/rhai/client-smoke.rhai").replace("\r\n", "\n"));
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
        let rh_task_runs = normalized.matches("agenterm-rh task run").count();
        assert!(task_runs > 0, "{name} must retain manifest task coverage");
        assert_eq!(
            rh_task_runs, task_runs,
            "{name} has a task run outside the agenterm-rh front door"
        );
        assert!(
            !normalized.contains("agenterm-rhai task run"),
            "{name} regressed to the compatibility CLI"
        );
    }
}

#[test]
fn candidate_and_release_use_only_rh_scripting_front_door() {
    assert!(CANDIDATE.contains(
        "target/aarch64-apple-darwin/release/agenterm-rh \\\n            run scripts/rh/finalize-macos-provenance.rh"
    ));
    assert!(CANDIDATE.contains(
        "target/x86_64-apple-darwin/release/agenterm-rh \\\n            run scripts/rh/finalize-macos-provenance.rh"
    ));
    assert!(CANDIDATE.contains("chmod +x \"$RUNNER_TEMP/agenterm-candidate-tool/agenterm-rh\""));
    assert!(CANDIDATE.contains(
        "\"$RUNNER_TEMP/agenterm-candidate-tool/agenterm-rh\" \\\n            run scripts/rhai/candidate-aggregate.rhai"
    ));
    assert!(RELEASE.contains("chmod +x \"$RUNNER_TEMP/agenterm-promotion-tool/agenterm-rh\""));
    assert!(RELEASE.contains(
        "\"$RUNNER_TEMP/agenterm-promotion-tool/agenterm-rh\" \\\n            run scripts/rhai/candidate-verify.rhai"
    ));
    assert!(RELEASE.contains("chmod +x \"$RUNNER_TEMP/agenterm-publish-tool/agenterm-rh\""));
    assert!(!CANDIDATE.contains("agenterm-rhai"));
    assert!(!RELEASE.contains("agenterm-rhai"));
}

#[test]
fn performance_experiment_uses_rh_for_build_and_manifest_tasks() {
    let normalized = normalized_task_callers(&PERFORMANCE_EXPERIMENT);
    assert!(normalized.contains("cargo build --quiet --locked --bin agenterm-rh"));
    assert!(normalized.contains("target/debug/agenterm-rh"));
    assert!(normalized.contains("%RUNNER_TEMP%/agenterm-rh task run performance-samples"));
    assert!(normalized.contains("%RUNNER_TEMP%/agenterm-rh task run performance-summary"));
    assert!(!normalized.contains("agenterm-rhai"));
}

#[test]
fn dist_task_worker_prefers_rh_with_explicit_compatibility_fallback() {
    let rh = CHECK
        .find("[\"dist/agenterm-rh.exe\", \"dist/agenterm-rh\"]")
        .expect("dist rh candidates");
    let compatibility = CHECK
        .find("[\"dist/agenterm-rhai.exe\", \"dist/agenterm-rhai\"]")
        .expect("dist compatibility candidates");
    let selection = CHECK
        .find("let worker = if dist_rh_cli != \"\"")
        .expect("rh-first worker selection");
    assert!(rh < compatibility && compatibility < selection);
    assert!(CHECK.contains("COMPATIBILITY FALLBACK: dist task worker uses agenterm-rhai"));
    assert!(CHECK.contains("check_dist_task_worker_missing"));
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
fn rh_binary_is_owned_and_built_by_root_package() {
    let root_bin = "[[bin]]\nname = \"agenterm-rh\"\npath = \"crates/agenterm-rh/src/main.rs\"";
    assert!(ROOT_MANIFEST.contains(root_bin));
    assert!(RH_MANIFEST.contains("autobins = false"));
    assert!(!RH_MANIFEST.contains("[[bin]]"));

    let root_build = "cargo build --quiet --locked --bin agenterm-rh";
    let old_package_build = "-p agenterm-rh --bin agenterm-rh";
    assert!(UNIX_BOOTSTRAP.contains(root_build));
    assert!(WINDOWS_BOOTSTRAP.contains(root_build));
    assert!(!UNIX_BOOTSTRAP.contains(old_package_build));
    assert!(!WINDOWS_BOOTSTRAP.contains(old_package_build));
}

#[test]
fn artifact_manifest_declares_both_rhai_and_rh_offline_version_probes() {
    let executables = ARTIFACT_MANIFEST["executables"]
        .as_array()
        .expect("manifest executables");
    let role = |expected: &str| {
        let matches = executables
            .iter()
            .filter(|entry| entry["role"] == expected)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "expected one {expected} executable");
        matches[0]
    };

    let rhai = role("scripting-cli");
    assert_eq!(rhai["name"], "agenterm-rhai.exe");
    assert_eq!(rhai["offline_probe"], serde_json::json!(["--version"]));

    let rh = role("rh-dev-cli");
    assert_eq!(rh["name"], "agenterm-rh.exe");
    assert_eq!(rh["offline_probe"], serde_json::json!(["version"]));
}

#[test]
fn artifact_verification_probes_manifest_roles_and_rejects_invalid_versions() {
    assert!(ARTIFACT_VERIFICATION.contains("artifact_for_role(manifest, \"scripting-cli\")"));
    assert!(ARTIFACT_VERIFICATION.contains("artifact_for_role(manifest, \"rh-dev-cli\")"));
    assert!(ARTIFACT_VERIFICATION.contains("artifact.offline_probe[0] == probe_argument"));
    assert!(ARTIFACT_VERIFICATION.contains("std::fs::metadata(path).len > 0"));
    assert!(ARTIFACT_VERIFICATION.contains("output(path, artifact.offline_probe, repo, code)"));
    assert!(ARTIFACT_VERIFICATION.contains("== banner + \" \" + version"));
    assert!(ARTIFACT_VERIFICATION.contains("\"artifact_rhai_version\""));
    assert!(ARTIFACT_VERIFICATION.contains("\"artifact_rh_version\""));
    assert!(!ARTIFACT_VERIFICATION.contains("dist, \"agenterm-rh.exe\""));
}

#[test]
fn client_smoke_fail_closes_rh_version_probe_from_platform_manifest() {
    assert!(CLIENT_SMOKE.contains("metadata.is_file && metadata.len > 0"));
    assert!(CLIENT_SMOKE.contains("executable.role == \"scripting-cli\""));
    assert!(CLIENT_SMOKE.contains("executable.role == \"rh-dev-cli\""));
    assert!(CLIENT_SMOKE.contains("executable.offline_probe[0] == \"--version\""));
    assert!(CLIENT_SMOKE.contains("executable.offline_probe[0] == \"version\""));
    assert!(CLIENT_SMOKE.contains("probes.push(executable.offline_probe)"));
    assert!(CLIENT_SMOKE.contains("banner == \"agenterm-rh \" + version"));
    assert!(CLIENT_SMOKE.contains("scripting_cli_count == 1"));
    assert!(CLIENT_SMOKE.contains("rh_dev_cli_count == 1"));
}
