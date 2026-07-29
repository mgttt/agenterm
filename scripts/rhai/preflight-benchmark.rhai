// Benchmark the public preflight task, including worker process startup.
//
// Args:
//   WORKER MANIFEST REPO OUTPUT_PATH ITERATIONS

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn parse_iterations(value) {
    let result = value.parse_int();
    require(
        result >= 5 && result <= 100,
        "preflight_benchmark_iterations_range"
    );
    result
}

fn sorted_durations(values) {
    let sorted = [];
    for value in values {
        let inserted = false;
        for index in 0..sorted.len {
            if value < sorted[index] {
                sorted.insert(index, value);
                inserted = true;
                break;
            }
        }
        if !inserted {
            sorted.push(value);
        }
    }
    sorted
}

require(
    args.len == 5,
    "expected: WORKER MANIFEST REPO OUTPUT_PATH ITERATIONS"
);
let worker = std::path::absolute(args[0]).display;
let manifest = std::path::absolute(args[1]).display;
let repo = std::path::absolute(args[2]).display;
let output_value = std::path::PathBuf::from(args[3]);
let output = if output_value.is_absolute {
    output_value.display
} else {
    std::path::join(repo, args[3]).display
};
let iterations = parse_iterations(args[4]);
require(std::fs::exists(worker), "preflight_benchmark_worker_missing");
require(std::fs::exists(manifest), "preflight_benchmark_manifest_missing");

let output_parent = std::path::parent(output).display;
let run_directory = std::path::join(
    output_parent,
    "runs-" + std::time::SystemTime::now().unix_millis
).display;
std::fs::create_dir_all(run_directory);

let durations = [];
let runs = [];
let all_passed = true;
let failure = "";
try {
    for offset in 0..iterations {
        let iteration = offset + 1;
        let run_report =
            std::path::join(run_directory, iteration + ".json").display;
        let started = std::time::SystemTime::now().unix_millis;
        let command = std::process::command(worker);
        command.args([
            "task", "run", "preflight", "--manifest", manifest,
            "--timeout-ms", "10000", "--max-operations", "10000000",
            "--", repo, run_report, "--quiet"
        ]);
        command.timeout(std::time::Duration::from_secs(10));
        let process = command.output();
        let wall_duration =
            std::time::SystemTime::now().unix_millis - started;
        require(
            std::fs::exists(run_report),
            "preflight_benchmark_report_missing:" + iteration +
                ":" + process.error("preflight-benchmark")
        );
        let preflight =
            rhai::json::parse(std::fs::read_to_string(run_report));
        if !preflight.passed || !process.success {
            all_passed = false;
        }
        durations.push(wall_duration);
        runs.push(#{
            iteration: iteration,
            passed: preflight.passed,
            exit_code: process.exit_code,
            preflight_duration_ms: preflight.duration_ms,
            wall_duration_ms: wall_duration
        });
    }

    let sorted = sorted_durations(durations);
    let p95_index = ((95 * sorted.len + 99) / 100) - 1;
    let p95 = sorted[p95_index];
    let benchmark = #{
        schema_version: 1,
        kind: "agenterm-read-only-preflight-benchmark",
        iterations: iterations,
        p95_wall_duration_ms: p95,
        target_ms: 15000,
        target_met: p95 <= 15000,
        all_preflights_passed: all_passed,
        excludes_network_and_interactive_auth: true,
        runs: runs
    };
    std::fs::create_dir_all(output_parent);
    rhai::runtime::atomic_write(
        output,
        rhai::json::stringify_pretty(benchmark) + "\n"
    );
    print(
        "PREFLIGHT BENCHMARK iterations=" + iterations +
        " p95_ms=" + p95 +
        " target_ms=15000 target_met=" + benchmark.target_met
    );
    print("PREFLIGHT BENCHMARK JSON " + output);
    require(
        benchmark.all_preflights_passed,
        "One or more benchmark preflight runs failed."
    );
    require(
        benchmark.target_met,
        "Preflight p95 " + p95 + " ms exceeds the 15000 ms target."
    );
} catch (error) {
    failure = error.to_string();
}

if std::fs::exists(run_directory) {
    std::fs::remove_dir_all(run_directory);
}
if failure != "" {
    throw failure;
}
