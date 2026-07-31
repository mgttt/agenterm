// Validate three ordered AgenTerm Quick timing samples and emit one typed
// cold-to-warm comparison. This describes the Quick lane only; it must not be
// used alone to claim Candidate release/stress runner speedups.
//
// Args:
//   OUTPUT_JSON RUNNER_CLASS CACHE_STRATEGY SAMPLE_1 SAMPLE_2 SAMPLE_3
//     SCCACHE_1 SCCACHE_2 SCCACHE_3

fn require(condition, code) {
    if !condition {
        throw code;
    }
}

fn sum_numeric(value) {
    let kind = type_of(value);
    if kind == "i64" {
        return value;
    }
    if kind == "array" {
        let total = 0;
        for item in value {
            total += sum_numeric(item);
        }
        return total;
    }
    if kind == "map" {
        let total = 0;
        for key in value.keys() {
            total += sum_numeric(value[key]);
        }
        return total;
    }
    0
}

fn sccache_metric(report, name) {
    if report.contains(name) {
        return sum_numeric(report[name]);
    }
    if report.contains("stats")
            && type_of(report.stats) == "map"
            && report.stats.contains(name) {
        return sum_numeric(report.stats[name]);
    }
    0
}

require(args.len == 9, "performance_summary_arguments");
let output = std::path::absolute(args[0]).display;
let runner_class = args[1];
let cache_strategy = args[2];
require(
    ["standard", "trial"].contains(runner_class),
    "performance_summary_runner"
);
require(
    ["target", "sccache", "none"].contains(cache_strategy),
    "performance_summary_cache"
);

let samples = [];
let totals = [];
let sccache_samples = [];
let source_commit = "";
let workload_fingerprint = "";
for index in 0..3 {
    let path = std::path::absolute(args[index + 3]).display;
    require(
        std::fs::exists(path) && std::fs::metadata(path).is_file,
        "performance_summary_sample_missing:" + (index + 1)
    );
    let report = rhai::json::parse_file(path);
    require(
        report.schema_version == 1
            && report.kind == "agenterm-quality-timing"
            && report.lane == "quick"
            && report.profile == "quick"
            && report.status == "passed"
            && report.total_wall_ms >= 0,
        "performance_summary_sample_schema:" + (index + 1)
    );
    if source_commit == "" {
        source_commit = report.source.commit;
        workload_fingerprint = report.workload.fingerprint;
    } else {
        require(
            source_commit == report.source.commit
                && workload_fingerprint == report.workload.fingerprint,
            "performance_summary_identity:" + (index + 1)
        );
    }
    samples.push(#{
        ordinal: index + 1,
        phase: if index == 0 { "cold" } else { "warm" },
        total_wall_ms: report.total_wall_ms,
        timing_path: std::path::PathBuf::from(path).file_name
    });
    totals.push(report.total_wall_ms);
    let stats_path = std::path::absolute(args[index + 6]).display;
    if cache_strategy == "sccache" {
        require(
            std::fs::exists(stats_path)
                && std::fs::metadata(stats_path).is_file,
            "performance_summary_sccache_missing:" + (index + 1)
        );
        let stats = rhai::json::parse_file(stats_path);
        sccache_samples.push(#{
            ordinal: index + 1,
            compile_requests: sccache_metric(stats, "compile_requests"),
            cache_hits: sccache_metric(stats, "cache_hits"),
            cache_misses: sccache_metric(stats, "cache_misses"),
            cache_errors:
                sccache_metric(stats, "cache_read_errors")
                + sccache_metric(stats, "cache_write_errors"),
            raw_sha256: rhai::crypto::sha256_file(stats_path),
            stats_path: std::path::PathBuf::from(stats_path).file_name
        });
    }
}
require(
    source_commit.len == 40 && workload_fingerprint != "",
    "performance_summary_source"
);

let ordered = totals;
ordered.sort();
let warm_ordered = [totals[1], totals[2]];
warm_ordered.sort();
let warm_median = (warm_ordered[0] + warm_ordered[1]) / 2;
let summary = #{
    schema_version: 2,
    kind: "agenterm-performance-experiment",
    workload_scope: "quick",
    runner_class: runner_class,
    cache_strategy: cache_strategy,
    source_commit: source_commit,
    workload_fingerprint: workload_fingerprint,
    cache_namespace_scope: if cache_strategy == "sccache" {
        "unique_per_workflow_run"
    } else {
        "runner_local"
    },
    sample_model: if cache_strategy == "target" {
        "cold_then_same_runner_incremental_warm"
    } else if cache_strategy == "sccache" {
        "cold_then_same_runner_sccache_warm_with_clean_target"
    } else {
        "three_same_runner_clean_target_samples"
    },
    samples: samples,
    cold_total_wall_ms: totals[0],
    warm_median_total_wall_ms: warm_median,
    warm_min_total_wall_ms: warm_ordered[0],
    warm_max_total_wall_ms: warm_ordered[1],
    all_sample_min_total_wall_ms: ordered[0],
    all_sample_max_total_wall_ms: ordered[2],
    warm_speedup_percent: if totals[0] > 0 {
        ((totals[0] - warm_median) * 100) / totals[0]
    } else {
        0
    },
    sccache: if cache_strategy == "sccache" {
        #{available: true, samples: sccache_samples}
    } else {
        #{available: false, samples: []}
    }
};
rhai::runtime::atomic_write(
    output,
    rhai::json::stringify_pretty(summary) + "\n"
);

let markdown =
    "## AgenTerm CI performance experiment\n\n" +
    "- Scope: **Quick lane only**\n" +
    "- Runner: `" + runner_class + "`\n" +
    "- Cache: `" + cache_strategy + "`\n" +
    "- Cold / warm / warm: `" + totals[0] + " / " + totals[1] +
        " / " + totals[2] + " ms`\n" +
    "- Cold: **" + totals[0] + " ms**\n" +
    "- Warm median/range: **" + warm_median + " ms** (`" +
        warm_ordered[0] + ".." + warm_ordered[1] + "`)\n" +
    "- Sample model: `" + summary.sample_model + "`\n\n" +
    "Runner-purchase decisions also require real Candidate release/stress " +
    "timing evidence.\n";
if std::env::has("GITHUB_STEP_SUMMARY")
        && std::env::get("GITHUB_STEP_SUMMARY") != "" {
    let summary_path =
        std::path::absolute(std::env::get("GITHUB_STEP_SUMMARY")).display;
    let previous = if std::fs::exists(summary_path) {
        std::fs::read_to_string(summary_path)
    } else {
        ""
    };
    require(
        previous.len <= 1048576,
        "performance_summary_previous_too_large"
    );
    std::fs::write(summary_path, previous + markdown);
} else {
    print(markdown);
}
