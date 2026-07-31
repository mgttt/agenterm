use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/performance-experiment.yml").replace("\r\n", "\n")
});

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
    assert!(WORKFLOW.contains("foreach ($sample in 1..3)"));
    assert!(!WORKFLOW.contains("matrix:\n        sample:"));
    assert!(WORKFLOW.contains("vars.AGENTERM_WINDOWS_EXPERIMENT_RUNNER"));
    assert!(WORKFLOW.contains("test -n \"$TRIAL_RUNNER\""));
    assert!(WORKFLOW.contains("'windows-latest'"));
    assert!(!WORKFLOW.contains("runs-on: ${{ inputs."));
    assert!(!WORKFLOW.contains("continue-on-error: ${{"));
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
    assert!(WORKFLOW.contains("cargo clean"));
    assert!(WORKFLOW.contains("sccache --zero-stats"));
    assert!(WORKFLOW.contains("sccache --show-stats --stats-format json"));
    assert!(
        WORKFLOW
            .contains("SCCACHE_GHA_VERSION: perf-${{ github.run_id }}-${{ github.run_attempt }}")
    );
    assert!(!WORKFLOW.contains("uses: actions/cache/"));
    assert!(WORKFLOW.contains("performance-summary.rhai"));
    assert!(WORKFLOW.contains("performance-summary.json"));
    assert!(WORKFLOW.contains("performance-evidence\\performance-$sample.json"));
    assert!(WORKFLOW.contains("sccache-1.json"));
    assert!(WORKFLOW.contains("sccache-2.json"));
    assert!(WORKFLOW.contains("sccache-3.json"));
    assert!(!WORKFLOW.contains("target\\qualification\\performance-"));
}

#[test]
fn experiment_runs_quick_only_and_cannot_publish_or_claim_qualification() {
    assert!(WORKFLOW.contains("check.cmd --quick"));
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
