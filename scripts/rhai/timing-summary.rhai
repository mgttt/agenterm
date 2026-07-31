// Validate one AgenTerm quality-timing report and render a bounded Markdown
// summary. In GitHub Actions, preserve earlier step summaries by appending to
// the GITHUB_STEP_SUMMARY content; elsewhere write the Markdown to stdout.
//
// Args:
//   TIMING_JSON

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn valid_status(value) {
    ["passed", "failed", "skipped", "not_run"].contains(value)
}

require(args.len == 1, "expected: TIMING_JSON");
let path = std::path::absolute(args[0]).display;
require(
    std::fs::exists(path) && std::fs::metadata(path).is_file,
    "quality_timing_summary_missing"
);
let report = rhai::json::parse_file(path);
require(
    report.schema_version == 1
        && report.kind == "agenterm-quality-timing",
    "quality_timing_summary_schema"
);
require(
    ["quick", "ordinary", "candidate"].contains(report.lane)
        && report.profile != ""
        && ["passed", "failed"].contains(report.status)
        && report.total_wall_ms >= 0,
    "quality_timing_summary_run"
);
require(
    report.source.commit.len == 40
        && report.source.cargo_lock_sha256.len == 64
        && report.gate_manifest.sha256.len == 64
        && report.workload.fingerprint != "",
    "quality_timing_summary_identity"
);
require(report.gates.len > 0, "quality_timing_summary_gates");

let seen = #{};
let counts = #{passed:0,failed:0,skipped:0,not_run:0};
let rows = "";
let max_existing_summary_bytes = 1048576;
for gate in report.gates {
    require(
        gate.id != ""
            && !seen.contains(gate.id)
            && valid_status(gate.status)
            && gate.duration_ms >= 0,
        "quality_timing_summary_gate"
    );
    seen[gate.id] = true;
    counts[gate.status] += 1;
    rows += "| `" + gate.id + "` | " + gate.status + " | " +
        gate.duration_ms + " |\n";
}
if report.status == "passed" {
    require(
        counts.failed == 0 && report.first_failure == (),
        "quality_timing_summary_passed_failure"
    );
} else {
    require(
        report.first_failure != ()
            && report.first_failure.elapsed_ms >= 0
            && (
                (
                    report.first_failure.kind == "gate"
                        && counts.failed == 1
                        && seen.contains(report.first_failure.gate_id)
                )
                || (
                    report.first_failure.kind == "stage"
                        && counts.failed == 0
                        && report.first_failure.stage_id != ""
                )
            ),
        "quality_timing_summary_failed_first_failure"
    );
}

let markdown =
    "## AgenTerm quality timing\n\n" +
    "- Lane: `" + report.lane + "` / `" + report.profile + "`\n" +
    "- Status: **" + report.status + "**\n" +
    "- Total wall time: **" + report.total_wall_ms + " ms**\n" +
    "- Source: `" + report.source.commit + "`\n" +
    "- Gates: " + counts.passed + " passed, " + counts.failed +
        " failed, " + counts.skipped + " skipped, " +
        counts.not_run + " not run\n" +
    if report.first_failure == () {
        ""
    } else {
        "- First failure: `" +
            if report.first_failure.kind == "gate" {
                report.first_failure.gate_id
            } else {
                report.first_failure.stage_id
            } +
            "` after " + report.first_failure.elapsed_ms + " ms\n"
    } +
    "\n| Gate | Status | Duration (ms) |\n" +
    "|---|---:|---:|\n" +
    rows;

if std::env::has("GITHUB_STEP_SUMMARY")
        && std::env::get("GITHUB_STEP_SUMMARY") != "" {
    let summary_path =
        std::path::absolute(std::env::get("GITHUB_STEP_SUMMARY")).display;
    let previous =
        if std::fs::exists(summary_path) {
            require(
                std::fs::metadata(summary_path).len <=
                    max_existing_summary_bytes,
                "quality_timing_summary_previous_too_large"
            );
            std::fs::read_to_string(summary_path)
        } else {
            ""
        };
    std::fs::write(summary_path, previous + markdown + "\n");
} else {
    print(markdown);
}
