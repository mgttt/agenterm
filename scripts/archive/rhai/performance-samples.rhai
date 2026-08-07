// Run one ordered cold-to-warm Windows Quick experiment. The workflow owns
// runner selection and sccache setup; this Rhai program owns sample semantics.
//
// Args: CACHE_STRATEGY EVIDENCE_DIRECTORY

fn require(condition, code) {
    if !condition {
        throw code;
    }
}

fn run(program, arguments, timeout_seconds, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.timeout(std::time::Duration::from_secs(timeout_seconds));
    let output = command.output();
    if output.stdout_text() != "" {
        print(output.stdout_text());
    }
    if output.stderr_text() != "" {
        print(output.stderr_text());
    }
    require(output.success, code);
    output
}

require(args.len == 2, "performance_samples_arguments");
let cache = args[0];
require(
    ["target", "sccache", "none"].contains(cache),
    "performance_samples_cache"
);
let evidence = std::path::absolute(args[1]).display;
std::fs::create_dir_all(evidence);

for sample in 1..4 {
    if sample == 1 || cache != "target" {
        run("cargo", ["clean"], 300, "performance_samples_clean:" + sample);
    }
    if cache == "sccache" {
        run(
            "sccache",
            ["--zero-stats"],
            30,
            "performance_samples_sccache_reset:" + sample
        );
    }
    let timing = std::path::join(
        evidence,
        "performance-" + sample + ".json"
    ).display;
    run(
        "cmd.exe",
        [
            "/d",
            "/s",
            "/c",
            "check.cmd --quick --timing \"" + timing + "\""
        ],
        900,
        "performance_samples_quick:" + sample
    );
    if cache == "sccache" {
        let stats_path =
            std::path::absolute("sccache-" + sample + ".json").display;
        let stderr_path =
            std::path::absolute("sccache-" + sample + ".stderr").display;
        let command = std::process::command("sccache");
        command.args(["--show-stats", "--stats-format", "json"]);
        command.timeout(std::time::Duration::from_secs(30));
        command.stdout_file(stats_path);
        command.stderr_file(stderr_path);
        let output = command.output();
        require(
            output.success && std::fs::exists(stats_path),
            "performance_samples_sccache_stats:" + sample
        );
        if std::fs::exists(stderr_path) {
            let stderr = std::fs::read_to_string(stderr_path);
            if stderr != "" {
                print(stderr);
            }
            std::fs::remove_file(stderr_path);
        }
    }
}

#{cache_strategy: cache, samples: 3, evidence_directory: evidence}
