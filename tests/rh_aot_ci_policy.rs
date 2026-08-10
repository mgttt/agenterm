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
static SCRIPT_SMOKE: &str = include_str!("../scripts/rh/script-smoke.rh");
static DIAGNOSTIC_BUNDLE_SELFTEST: &str =
    include_str!("../scripts/rh/diagnostic-bundle-selftest.rh");
static PLATFORM_UX_PARITY_SMOKE: &str = include_str!("../scripts/rh/platform-ux-parity-smoke.rh");
static RH_AOT_SMOKE: &str = include_str!("../scripts/rh/rh-aot-smoke.rh");
static UNIX_BOOTSTRAP: &str = include_str!("../scripts/bootstrap.sh");
static WINDOWS_BOOTSTRAP: &str = include_str!("../scripts/bootstrap.cmd");
static UNIX_RH_CHECK: &str = include_str!("../scripts/rh-check.sh");
static WINDOWS_RH_CHECK: &str = include_str!("../scripts/rh-check.cmd");
static ARTIFACT_MANIFEST: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../scripts/artifacts.json"))
        .expect("scripts/artifacts.json must be valid JSON")
});
static TASK_MANIFEST: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../agenterm.tasks.json"))
        .expect("agenterm.tasks.json must be valid JSON")
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
fn dist_task_worker_uses_staged_agenterm_rh_without_retired_binary_fallback() {
    let main = CHECK
        .find("\"dist/agenterm.exe\"")
        .expect("staged main binary candidates");
    let selection = CHECK
        .find("let worker = resolve_worker(dist_agenterm, task_rh_cli)")
        .expect("rh worker selection");
    assert!(main < selection);
    assert!(CHECK.contains("[\n        \"rh\", \"task\", \"run\", task_id,"));
    assert!(CHECK.contains("check_dist_task_worker_missing"));
    assert!(!CHECK.contains("dist/agenterm-rh.exe"));
    assert!(!CHECK.contains("target/debug/agenterm-rh.exe"));
    assert!(!CHECK.contains("COMPATIBILITY FALLBACK"));
}

#[test]
fn unix_frontend_native_journeys_have_explicit_cleanup_budget() {
    for task in ["unix-frontend-linux-smoke", "unix-frontend-macos-smoke"] {
        let budget = &TASK_MANIFEST["contracts"][task]["budget"];
        assert_eq!(budget["timeout_ms"], 300_000, "{task}");
        assert_eq!(budget["max_operations"], 100_000_000, "{task}");
        assert_eq!(budget["max_output_bytes"], 1_048_576, "{task}");
    }
}

#[test]
fn every_manifest_task_has_an_execution_contract() {
    let tasks = TASK_MANIFEST["tasks"].as_array().expect("manifest tasks");
    let contracts = TASK_MANIFEST["contracts"]
        .as_object()
        .expect("manifest contracts");
    for task in tasks {
        let id = task["id"].as_str().expect("task id");
        assert!(contracts.contains_key(id), "task contract missing: {id}");
    }
}

#[test]
fn linux_clipboard_smoke_feeds_xclip_through_stdin() {
    let source = include_str!("../scripts/rh/unix-frontend-smoke.rh");
    assert!(source.contains("clipboard_command.stdin_text(clipboard_text)"));
    assert!(source.contains("command.stdin_text(text)"));
    assert!(!source.contains("\"-silent\", payload_path"));
}

#[test]
fn active_quality_smokes_use_the_unified_agenterm_rh_front_door() {
    let manifest = serde_json::to_string(&*TASK_MANIFEST).expect("serialize task manifest");
    for (name, source) in [
        ("task manifest", manifest.as_str()),
        ("check", CHECK.as_str()),
        ("script smoke", SCRIPT_SMOKE),
        ("diagnostic bundle", DIAGNOSTIC_BUNDLE_SELFTEST),
        ("platform UX parity", PLATFORM_UX_PARITY_SMOKE),
        ("rh AOT smoke", RH_AOT_SMOKE),
    ] {
        assert!(!source.contains("dist/agenterm-rh"), "{name}");
        assert!(!source.contains("target/debug/agenterm-rh"), "{name}");
    }
    assert!(SCRIPT_SMOKE.contains("[\\\"rh\\\", \\\"--framed-worker\\\"]"));
    assert!(DIAGNOSTIC_BUNDLE_SELFTEST.contains("\"rh\", \"run\", entry_s"));
    assert!(PLATFORM_UX_PARITY_SMOKE.contains("\"rh\", \"task\", \"run\""));
    assert!(RH_AOT_SMOKE.contains("[\"rh\", \"pack\", \"build\""));
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
    assert!(CLIENT_SMOKE.contains("rh_dev_cli_count == 0"));
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

    let unix_root_build = "cargo build --quiet --locked --bin agenterm";
    let windows_root_build = "cargo build --locked --bin agenterm";
    let retired_build = "--bin agenterm-rh";
    assert!(UNIX_BOOTSTRAP.contains(unix_root_build));
    assert!(WINDOWS_BOOTSTRAP.contains(windows_root_build));
    assert!(!UNIX_BOOTSTRAP.contains(retired_build));
    assert!(!WINDOWS_BOOTSTRAP.contains(retired_build));
    assert!(UNIX_RH_CHECK.contains("cargo build --locked --bin agenterm"));
    assert!(WINDOWS_RH_CHECK.contains("cargo build --locked --bin agenterm"));
    assert!(!UNIX_RH_CHECK.contains(retired_build));
    assert!(!WINDOWS_RH_CHECK.contains(retired_build));
}

#[test]
fn expensive_task_entry_packs_have_one_dedicated_ci_owner() {
    assert!(CHECK.contains("\"--skip\", \"uses_bundled_pack\""));
    assert!(CHECK.contains("\"--skip\", \"uses_native_bundled_pack\""));
    assert!(CHECK.contains("\"--skip\", \"native_pack_\""));
    assert!(CHECK.contains("\"--skip\", \"source_cache_is_stable_for_same_source\""));
    assert!(CHECK.contains("\"--skip\", \"script_engine_exec_parity_\""));
    assert!(
        CHECK.contains("\"--skip\", \"native_for_fixtures_qualify_with_expected_entry_values\"")
    );
    assert!(CHECK.contains("\"--skip\", \"_executes_natively_without_interpreter\""));
    assert!(CHECK.contains("\"--skip\", \"native_pack_executes_without_interpreter\""));
    assert!(UNIX_RH_CHECK.contains("--test script_engine_exec_parity"));
    assert!(WINDOWS_RH_CHECK.contains("--test script_engine_exec_parity"));
    assert!(CHECK.contains("\"--skip\", \"pack_builds\""));
    assert!(UNIX_RH_CHECK.contains("cargo test --locked --test rh_task_entry_regression"));
    assert!(WINDOWS_RH_CHECK.contains("cargo test --locked --test rh_task_entry_regression"));
}

#[test]
fn macos_control_center_lifecycle_has_bounded_full_journey_budget() {
    let budget = &TASK_MANIFEST["contracts"]["control-center-macos-smoke"]["budget"];
    assert_eq!(budget["timeout_ms"], 300_000);
    assert_eq!(budget["max_operations"], 10_000_000);
    assert_eq!(budget["max_output_bytes"], 1_048_576);
}
