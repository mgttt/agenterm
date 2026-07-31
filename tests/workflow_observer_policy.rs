use std::sync::LazyLock;

static WORKFLOW: LazyLock<String> = LazyLock::new(|| {
    include_str!("../.github/workflows/workflow-observer.yml").replace("\r\n", "\n")
});

#[test]
fn observer_is_read_only_and_never_executes_observed_code() {
    assert!(WORKFLOW.contains("permissions:\n  actions: read\n  contents: read"));
    for forbidden in [
        "actions: write",
        "contents: write",
        "id-token: write",
        "pull-requests: write",
        "actions/checkout",
        "github.event.workflow_run.head_sha }}",
        "git checkout",
        "git switch",
        "cargo ",
        "./scripts/",
    ] {
        assert!(
            !WORKFLOW.contains(forbidden),
            "observer contains forbidden authority or candidate execution: {forbidden}"
        );
    }
}

#[test]
fn observer_covers_all_completed_delivery_workflows() {
    for workflow in ["CI", "Release Candidate", "Release"] {
        assert!(
            WORKFLOW.contains(&format!("      - {workflow}\n")),
            "missing observed workflow {workflow}"
        );
    }
    assert!(WORKFLOW.contains("types:\n      - completed"));
    assert!(!WORKFLOW.contains("types:\n      - requested"));
    assert!(!WORKFLOW.contains("if: success()"));
    assert!(!WORKFLOW.contains("if: failure()"));
}

#[test]
fn observer_rest_pagination_and_output_are_bounded() {
    for boundary in [
        "PER_PAGE = 100",
        "MAX_PAGES = 10",
        "MAX_JOBS = 200",
        "MAX_STEPS_PER_JOB = 100",
        "MAX_TEXT_CHARS = 256",
        "MAX_JSON_BYTES = 1_048_576",
        "REQUEST_TIMEOUT_SECONDS = 30",
        "MAX_REQUEST_ATTEMPTS = 3",
        "?filter=all&per_page={PER_PAGE}&page={page}",
        "for page in range(1, MAX_PAGES + 1)",
        "raw_jobs.extend(page_jobs[:remaining])",
        "for step in raw_steps[:MAX_STEPS_PER_JOB]",
        "while len(encoded) > MAX_JSON_BYTES and payload[\"jobs\"]",
    ] {
        assert!(
            WORKFLOW.contains(boundary),
            "missing observer boundary: {boundary}"
        );
    }
    assert!(WORKFLOW.contains("actions/runs/{run_id}/jobs"));
    assert!(WORKFLOW.contains("\"jobs_truncated\""));
    assert!(WORKFLOW.contains("\"steps_truncated\""));
}

#[test]
fn observer_schema_contains_only_metadata_durations_and_slos() {
    for field in [
        "\"schema_version\"",
        "\"generated_at\"",
        "\"run\"",
        "\"stage_duration_ms\"",
        "\"id\"",
        "\"attempt\"",
        "\"workflow_id\"",
        "\"workflow_name\"",
        "\"event\"",
        "\"head_sha\"",
        "\"status\"",
        "\"conclusion\"",
        "\"queue_duration_ms\"",
        "\"run_duration_ms\"",
        "\"duration_ms\"",
        "\"jobs\"",
        "\"steps\"",
        "\"pagination\"",
        "\"slo\"",
    ] {
        assert!(WORKFLOW.contains(field), "missing schema field {field}");
    }
    for forbidden in [
        "\"environment\"",
        "\"env\"",
        "\"command\"",
        "\"output\"",
        "\"logs\"",
        "/logs",
        "os.environ.items",
        "dict(os.environ",
    ] {
        assert!(
            !WORKFLOW.contains(forbidden),
            "observer may expose forbidden execution data: {forbidden}"
        );
    }
    assert!(WORKFLOW.contains("\"Release Candidate\": 8 * 60 * 1000"));
    assert!(WORKFLOW.contains("\"Release\": 3 * 60 * 1000"));
    assert!(WORKFLOW.contains("\"candidate\""));
    assert!(WORKFLOW.contains("\"promotion\""));
    assert!(WORKFLOW.contains("checkout_ms"));
    assert!(WORKFLOW.contains("toolchain_ms"));
    assert!(WORKFLOW.contains("compile_ms"));
    assert!(WORKFLOW.contains("test_ms"));
    assert!(WORKFLOW.contains("package_ms"));
    assert!(WORKFLOW.contains("artifact_transfer_ms"));
    assert!(WORKFLOW.contains("promotion_ms"));
    assert!(WORKFLOW.contains("aggregate_ms"));
}

#[test]
fn observer_artifact_is_sha_pinned_and_summary_is_small() {
    assert!(
        WORKFLOW
            .contains("actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2")
    );
    assert!(WORKFLOW.contains("path: workflow-observation.json"));
    assert!(WORKFLOW.contains("if-no-files-found: error"));
    assert!(WORKFLOW.contains("retention-days: 30"));
    assert!(WORKFLOW.contains("cat workflow-observation-summary.md >> \"$GITHUB_STEP_SUMMARY\""));
}
