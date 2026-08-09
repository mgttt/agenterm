use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/performance-experiment.yml").replace("\r\n", "\n")
});
static TASK_MANIFEST: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../agenterm.tasks.json")).expect("task manifest")
});
static SAMPLES: LazyLock<String> =
    LazyLock::new(|| include_str!("../scripts/rh/performance-samples.rh").replace("\r\n", "\n"));

#[test]
fn experiment_is_manual_read_only_and_exact_source_bound() {
    assert!(WORKFLOW.contains("workflow_dispatch:"));
    assert!(!WORKFLOW.contains("\n  push:"));
    assert!(!WORKFLOW.contains("\n  pull_request:"));
    assert!(WORKFLOW.contains("permissions:\n  contents: read"));
    assert!(!WORKFLOW.contains("contents: write"));
    assert!(!WORKFLOW.contains("secrets."));
    assert!(WORKFLOW.contains("ref: ${{ inputs.source_sha }}"));
    assert!(WORKFLOW.contains("[[ \"$(git rev-parse HEAD)\" == \"$SOURCE_SHA\" ]]"));
}

#[test]
fn experiment_uses_three_equal_samples_and_one_configured_trial_switch() {
    assert!(SAMPLES.contains("for sample in 1..4"));
    assert!(!WORKFLOW.contains("matrix:\n        sample:"));
    assert!(WORKFLOW.contains("vars.AGENTERM_WINDOWS_EXPERIMENT_RUNNER"));
    assert!(WORKFLOW.contains("test -n \"$TRIAL_RUNNER\""));
    assert!(WORKFLOW.contains("'windows-latest'"));
    assert!(!WORKFLOW.contains("runs-on: ${{ inputs."));
    assert!(!WORKFLOW.contains("continue-on-error: ${{"));
    assert!(WORKFLOW.contains("Build isolated Rh experiment driver"));
    assert!(WORKFLOW.contains("agenterm-rh.exe"));
    assert!(WORKFLOW.contains("task run performance-samples --manifest agenterm.tasks.json"));
    assert!(WORKFLOW.contains("task run performance-summary"));
    let compatibility_cli = ["agenterm", "rhai"].join("-");
    assert!(!WORKFLOW.contains(&compatibility_cli));
    assert!(!WORKFLOW.contains("shell: pwsh"));

    let tasks = TASK_MANIFEST["tasks"].as_array().expect("tasks");
    for id in ["performance-samples", "performance-summary"] {
        assert!(
            tasks.iter().any(|task| task["id"] == id),
            "missing manifest task {id}"
        );
        assert!(
            TASK_MANIFEST["contracts"].get(id).is_some(),
            "missing manifest contract {id}"
        );
    }
}

#[test]
fn cache_strategies_are_isolated_fail_safe_and_observable() {
    assert!(
        WORKFLOW.contains("options:\n          - target\n          - sccache\n          - none")
    );
    assert!(
        WORKFLOW
            .contains("mozilla-actions/sccache-action@fc920bf0ec8de6ee65d409111f7ec508035751ba")
    );
    assert!(WORKFLOW.contains("CARGO_INCREMENTAL:"));
    assert!(WORKFLOW.contains("RUSTC_WRAPPER:"));
    assert!(SAMPLES.contains("sample == 1 || cache != \"target\""));
    assert!(
        SAMPLES
            .contains("command_status(\n                \"cargo\",\n                [\"clean\"]")
    );
    assert!(SAMPLES.contains("[\"--zero-stats\"]"));
    assert!(SAMPLES.contains("[\"--show-stats\", \"--stats-format\", \"json\"]"));
    assert!(
        WORKFLOW
            .contains("SCCACHE_GHA_VERSION: perf-${{ github.run_id }}-${{ github.run_attempt }}")
    );
    assert!(!WORKFLOW.contains("uses: actions/cache/"));
    assert!(WORKFLOW.contains("task run performance-summary"));
    assert!(WORKFLOW.contains("performance-summary.json"));
    assert!(SAMPLES.contains("\"performance-\" + sample_tag + \".json\""));
    assert!(SAMPLES.contains("\"sccache-\" + sample_tag + \".json\""));
    assert!(!SAMPLES.contains("target\\qualification\\performance-"));
}

#[test]
fn experiment_runs_quick_only_and_cannot_publish_or_claim_qualification() {
    assert!(SAMPLES.contains("check.cmd --quick --timing"));
    for forbidden in [
        "--release",
        "--include-stress",
        "gh release",
        "git tag",
        "candidate-aggregate",
        "candidate-verify",
        "package-client-release",
    ] {
        assert!(
            !WORKFLOW.contains(forbidden),
            "forbidden experiment behavior: {forbidden}"
        );
    }
    assert!(WORKFLOW.contains("Aggregate typed experiment evidence"));
    assert!(WORKFLOW.contains("retention-days: 14"));
    assert!(WORKFLOW.contains("if-no-files-found: warn"));
}
