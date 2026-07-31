// Build and optionally stage AgenTerm through one manifest-owned lifecycle.
//
// Args:
//   REPO [release|release-fast]
//        [--target TRIPLE] [--driver cargo|cargo-xwin]
//        [--os windows|linux|macos] [--arch x86_64|aarch64]
//        [--clients-only] [--action build|clippy]

import "scripts/rhai/lib/build_identity" as build_identity;
import "scripts/rhai/lib/bootstrap_timing" as bootstrap_timing;

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

fn option_value(values, index, name) {
    require(index + 1 < values.len, "build_option_value_missing:" + name);
    values[index + 1]
}

fn target_facts(target) {
    if target.contains("windows") {
        return #{
            os: "windows",
            arch: if target.starts_with("aarch64") {
                "aarch64"
            } else {
                "x86_64"
            }
        };
    }
    if target.contains("apple") {
        return #{
            os: "macos",
            arch: if target.starts_with("aarch64") {
                "aarch64"
            } else {
                "x86_64"
            }
        };
    }
    if target.contains("linux") {
        return #{
            os: "linux",
            arch: if target.starts_with("aarch64") {
                "aarch64"
            } else {
                "x86_64"
            }
        };
    }
    #{ os: "", arch: "" }
}

fn platform_executables(repo, os_name, arch) {
    let manifest = rhai::json::parse_file(
        std::path::join(repo, "scripts/artifacts.json").display
    );
    let matches = [];
    for platform in manifest.platforms {
        if platform.os == os_name && platform.arch == arch {
            matches.push(platform);
        }
    }
    require(
        matches.len == 1,
        "build_platform_manifest_match:" + os_name + "-" + arch
    );
    matches[0].executables
}

fn cargo_binary(name) {
    if name.ends_with(".exe") {
        return name.sub_string(0, name.len - 4);
    }
    name
}

require(args.len >= 1, "expected: REPO [PROFILE] [OPTIONS]");
let repo = std::path::absolute(args[0]).display;
let bootstrap = bootstrap_timing::facts();
print("AgenTerm bootstrap timing: " + rhai::json::stringify(bootstrap));
let profile = "dev";
let target = "";
let driver = "cargo";
let os_name = "";
let arch = "";
let clients_only = false;
let action = "build";
let index = 1;
while index < args.len {
    let value = args[index];
    if value == "release" || value == "release-fast" {
        require(profile == "dev", "build_profile_duplicate");
        profile = value;
    } else if value == "--target" {
        target = option_value(args, index, value);
        index += 1;
    } else if value == "--driver" {
        driver = option_value(args, index, value);
        index += 1;
    } else if value == "--os" {
        os_name = option_value(args, index, value);
        index += 1;
    } else if value == "--arch" {
        arch = option_value(args, index, value);
        index += 1;
    } else if value == "--action" {
        action = option_value(args, index, value);
        index += 1;
    } else if value == "--clients-only" {
        clients_only = true;
    } else {
        throw "build_unknown_argument:" + value;
    }
    index += 1;
}
require(driver == "cargo" || driver == "cargo-xwin", "build_driver:" + driver);
require(action == "build" || action == "clippy", "build_action:" + action);
require(
    target != "" || driver == "cargo",
    "build_xwin_requires_target"
);
if target != "" {
    let facts = target_facts(target);
    require(facts.os != "", "build_target_unsupported:" + target);
    if os_name == "" {
        os_name = facts.os;
    }
    if arch == "" {
        arch = facts.arch;
    }
    require(
        os_name == facts.os && arch == facts.arch,
        "build_target_platform_mismatch"
    );
}
if os_name == "" && std::env::has("AGENTERM_HOST_OS") {
    os_name = std::env::get("AGENTERM_HOST_OS");
}
if arch == "" && std::env::has("AGENTERM_HOST_ARCH") {
    arch = std::env::get("AGENTERM_HOST_ARCH");
}

require(
    std::env::has("AGENTERM_BOOTSTRAP_WORKER"),
    "build_bootstrap_worker_missing"
);
let worker =
    std::path::absolute(std::env::get("AGENTERM_BOOTSTRAP_WORKER")).display;
let task_manifest = std::path::join(repo, "agenterm.tasks.json").display;
let dist = std::path::join(repo, "dist").display;
let cross_client = clients_only || target != "";
let unix_bootstrap =
    std::env::has("AGENTERM_BOOTSTRAP_PLATFORM")
        && std::env::get("AGENTERM_BOOTSTRAP_PLATFORM") == "unix";
if cross_client || unix_bootstrap {
    require(os_name != "" && arch != "", "build_platform_missing");
}

let external_target = std::env::has("CARGO_TARGET_DIR");
let cargo_output = if external_target {
    std::path::absolute(std::env::get("CARGO_TARGET_DIR")).display
} else if profile == "release" && !cross_client && !unix_bootstrap {
    std::path::join(repo, "target-release").display
} else {
    std::path::join(repo, "target").display
};
let development_target = std::path::join(repo, "target").display;
let profile_name = if profile == "dev" { "debug" } else { profile };
let profile_directory = std::path::join(
    if target == "" {
        cargo_output
    } else {
        std::path::join(cargo_output, target).display
    },
    profile_name
).display;

let identity = build_identity::facts(repo, profile);
let environment = build_identity::environment(identity);
if !external_target && profile == "release"
        && !cross_client && !unix_bootstrap {
    environment.CARGO_TARGET_DIR = cargo_output;
}

let cargo_args = [];
if driver == "cargo-xwin" {
    cargo_args.push("xwin");
}
cargo_args.push(action);
cargo_args.push("--locked");
if profile == "release" {
    cargo_args.push("--release");
} else if profile == "release-fast" {
    cargo_args.push("--profile");
    cargo_args.push("release-fast");
}
if target != "" {
    cargo_args.push("--target");
    cargo_args.push(target);
}
let executable_entries = [];
if cross_client || unix_bootstrap {
    executable_entries = platform_executables(repo, os_name, arch);
    for executable in executable_entries {
        cargo_args.push("--bin");
        cargo_args.push(cargo_binary(executable.name));
    }
}
if action == "clippy" {
    cargo_args.push("--");
    cargo_args.push("-D");
    cargo_args.push("warnings");
}
run_command(
    "cargo",
    cargo_args,
    repo,
    environment,
    3600000,
    "build_cargo"
);

if action == "clippy" {
    print(
        "PASS client Clippy " + os_name + "-" + arch +
            if target == "" { "" } else { " (" + target + ")" }
    );
    return;
}
if cross_client || unix_bootstrap {
    print("");
    print(
        "Built client artifacts [" + profile + ", " +
            os_name + "-" + arch + "]:"
    );
    for executable in executable_entries {
        print(
            "  " +
                std::path::join(
                    profile_directory,
                    executable.name
                ).display
        );
    }
    return;
}

run_command(
    worker,
    [
        "task", "run", "stage-build",
        "--manifest", task_manifest,
        "--", repo, profile_directory, dist, profile
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
            "--manifest", task_manifest,
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
            "--manifest", task_manifest,
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
    // bootstrap.cmd invokes this worker outside Cargo output, so a staged
    // release can also reclaim the development target without a locked EXE.
    run_command(
        worker,
        [
            "task", "run", "target-report",
            "--manifest", task_manifest,
            "--", development_target
        ],
        repo,
        environment,
        30000,
        "build_development_target_report"
    );
    run_command(
        worker,
        [
            "task", "run", "prepare-target-clean",
            "--manifest", task_manifest,
            "--", repo, development_target
        ],
        repo,
        environment,
        30000,
        "build_development_target_clean_prepare"
    );
    run_command(
        "cargo",
        ["clean", "--target-dir", development_target],
        repo,
        environment,
        3600000,
        "build_development_target_clean"
    );
}

print("");
print("Built and staged Windows artifacts [" + profile + "] in " + dist);
