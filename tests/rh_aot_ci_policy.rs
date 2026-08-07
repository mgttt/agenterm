use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> =
    LazyLock::new(|| include_str!("../.github/workflows/ci.yml").replace("\r\n", "\n"));
static ARTIFACT_VERIFICATION: LazyLock<String> = LazyLock::new(|| {
    include_str!("../scripts/rhai/artifact-verification.rhai").replace("\r\n", "\n")
});
static CLIENT_SMOKE: LazyLock<String> =
    LazyLock::new(|| include_str!("../scripts/rhai/client-smoke.rhai").replace("\r\n", "\n"));
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
