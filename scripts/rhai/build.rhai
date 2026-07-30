// Build and stage AgenTerm through one Rhai-owned lifecycle.
//
// Args:
//   REPO [release|release-fast]

import "scripts/rhai/lib/build_identity" as build_identity;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn run_command(program, arguments, repo, environment, timeout_ms, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(repo);
    command.timeout(std::time::Duration::from_millis(timeout_ms));
    for name in environment.keys() {
        command.env(name, environment[name]);
    }
    let output = command.output();
    require(output.success, output.error(code));
    let stdout = output.stdout_text();
    if stdout != "" {
        print(stdout);
    }
}

require(args.len >= 1 && args.len <= 2, "expected: REPO [release|release-fast]");
let repo = std::path::absolute(args[0]).display;
let requested = if args.len == 2 { args[1] } else { "" };
require(
    requested == "" || requested == "release" || requested == "release-fast",
    "build_profile_argument:" + requested
);
let profile = if requested == "" { "dev" } else { requested };
require(
    std::env::has("AGENTERM_BOOTSTRAP_WORKER"),
    "build_bootstrap_worker_missing"
);
let worker =
    std::path::absolute(std::env::get("AGENTERM_BOOTSTRAP_WORKER")).display;
let manifest = std::path::join(repo, "agenterm.tasks.json").display;
let dist = std::path::join(repo, "dist").display;

let external_target = std::env::has("CARGO_TARGET_DIR");
let cargo_output = if external_target {
    std::path::absolute(std::env::get("CARGO_TARGET_DIR")).display
} else if profile == "release" {
    std::path::join(repo, "target-release").display
} else {
    std::path::join(repo, "target").display
};
let profile_directory = if profile == "dev" {
    std::path::join(cargo_output, "debug").display
} else if profile == "release-fast" {
    std::path::join(cargo_output, "release-fast").display
} else {
    std::path::join(cargo_output, "release").display
};

let identity = build_identity::facts(repo, profile);
let environment = build_identity::environment(identity);
if !external_target && profile == "release" {
    environment.CARGO_TARGET_DIR = cargo_output;
}

let cargo_args = ["build", "--locked"];
if profile == "release" {
    cargo_args.push("--release");
} else if profile == "release-fast" {
    cargo_args.push("--profile");
    cargo_args.push("release-fast");
}
let unix_bootstrap =
    std::env::has("AGENTERM_BOOTSTRAP_PLATFORM")
        && std::env::get("AGENTERM_BOOTSTRAP_PLATFORM") == "unix";
if unix_bootstrap {
    for binary in [
        "agenterm",
        "agenterm-cli",
        "agenterm-mux",
        "agenterm-script",
        "agenterm-mcp"
    ] {
        cargo_args.push("--bin");
        cargo_args.push(binary);
    }
}
run_command(
    "cargo",
    cargo_args,
    repo,
    environment,
    3600000,
    "build_cargo"
);

if unix_bootstrap {
    print("");
    print("Built native Unix client artifacts [" + profile + "]:");
    for binary in [
        "agenterm",
        "agenterm-cli",
        "agenterm-mux",
        "agenterm-script",
        "agenterm-mcp"
    ] {
        print("  " + std::path::join(profile_directory, binary).display);
    }
    return;
}

run_command(
    worker,
    [
        "task", "run", "stage-build",
        "--manifest", manifest,
        "--timeout-ms", "10000",
        "--max-operations", "10000000",
        "--",
        repo, profile_directory, dist, profile
    ],
    repo,
    environment,
    30000,
    "build_stage"
);

if profile == "release" && !external_target {
    run_command(
        worker,
        [
            "task", "run", "target-report",
            "--manifest", manifest,
            "--timeout-ms", "10000",
            "--max-operations", "10000000",
            "--", cargo_output
        ],
        repo,
        environment,
        30000,
        "build_target_report"
    );
    run_command(
        worker,
        [
            "task", "run", "prepare-target-clean",
            "--manifest", manifest,
            "--", repo, cargo_output
        ],
        repo,
        environment,
        30000,
        "build_target_clean_prepare"
    );
    run_command(
        "cargo",
        ["clean", "--target-dir", cargo_output],
        repo,
        environment,
        3600000,
        "build_target_clean"
    );
}

print("");
print("Built:    " + std::path::join(dist, "agenterm.exe").display +
    " [" + profile + "]");
print("Server:   " + std::path::join(dist, "agenterm-server.exe").display);
print("CLI:      " + std::path::join(dist, "agenterm-cli.exe").display);
print("Mux:      " + std::path::join(dist, "agenterm-mux.exe").display);
print("Script:   " + std::path::join(dist, "agenterm-script.exe").display);
print("MCP:      " + std::path::join(dist, "agenterm-mcp.exe").display);
print("Metadata: " + std::path::join(dist, "agenterm.json").display);
