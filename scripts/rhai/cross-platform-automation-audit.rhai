// Reject duplicated Cargo executable lists and packaging rules outside Rhai.
//
// Args: REPO

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 1, "expected: REPO");
let repo = std::path::absolute(args[0]).display;
let tracked = std::process::command("git");
tracked.args([
    "-C", repo, "ls-files", "--",
    ".github/workflows/*.yml", "scripts/*.sh",
    "build.sh", "check.sh", "lint.sh", "release.sh"
]);
tracked.timeout(std::time::Duration::from_secs(10));
let output = tracked.output();
require(output.success, output.error("cross_platform_audit_inventory"));
let forbidden = [
    "--bin agenterm --bin",
    "bins=(",
    "cargo build --locked --release --target",
    "cargo xwin build --locked --target",
    "python3 - ./scripts/artifacts.json",
    "package-client-release.sh",
    "./scripts/build-"
];
let checked = 0;
for source_line in output.stdout_text().split("\n") {
    let relative = source_line;
    relative.trim();
    if relative == "" {
        continue;
    }
    let path = std::path::join(repo, relative).display;
    if !std::fs::exists(path) {
        continue;
    }
    checked += 1;
    let source = std::fs::read_to_string(path);
    source = source.to_lower();
    for token in forbidden {
        require(
            !source.contains(token),
            "cross_platform_rule_duplicate:" + relative + ":" + token
        );
    }
}
require(checked >= 9, "cross_platform_audit_inventory_too_small");
let aliases = #{
    "build.bat": #{
        shell: "build.sh",
        task: "build"
    },
    "check.cmd": #{
        shell: "check.sh",
        task: "check"
    },
    "lint.cmd": #{
        shell: "lint.sh",
        task: "lint"
    },
    "release.cmd": #{
        shell: "release.sh",
        task: "release"
    }
};
let batch_inventory = std::process::command("git");
batch_inventory.args([
    "-C", repo, "ls-files", "--", "*.bat", "*.cmd"
]);
batch_inventory.timeout(std::time::Duration::from_secs(10));
let batch_output = batch_inventory.output();
require(
    batch_output.success,
    batch_output.error("cross_platform_batch_inventory")
);
let expected_batch = [
    "build.bat",
    "check.cmd",
    "lint.cmd",
    "release.cmd",
    "scripts/bootstrap.cmd"
];
let actual_batch = [];
for source_line in batch_output.stdout_text().split("\n") {
    let relative = source_line;
    relative.trim();
    if relative != "" {
        actual_batch.push(relative);
    }
}
require(
    actual_batch.len == expected_batch.len,
    "cross_platform_batch_inventory_count:" + actual_batch.len
);
for relative in expected_batch {
    require(
        actual_batch.contains(relative),
        "cross_platform_batch_inventory_missing:" + relative
    );
}
for windows_alias in aliases.keys() {
    let pair = aliases[windows_alias];
    let windows_source = std::fs::read_to_string(
        std::path::join(repo, windows_alias).display
    );
    let unix_source = std::fs::read_to_string(
        std::path::join(repo, pair.shell).display
    );
    require(
        windows_source.to_lower().contains(
            "agenterm_bootstrap_task=" + pair.task
        ),
        "cross_platform_windows_alias:" + windows_alias
    );
    require(
        unix_source.contains("AGENTERM_BOOTSTRAP_TASK=" + pair.task)
            && unix_source.contains("scripts/bootstrap.sh"),
        "cross_platform_unix_alias:" + pair.shell
    );
}
let windows_bootstrap = std::fs::read_to_string(
    std::path::join(repo, "scripts/bootstrap.cmd").display
);
let unix_bootstrap = std::fs::read_to_string(
    std::path::join(repo, "scripts/bootstrap.sh").display
);
require(
    windows_bootstrap.to_lower().contains(
        `task run "%agenterm_bootstrap_task%"`
    )
        && unix_bootstrap.contains(
            `task run "$AGENTERM_BOOTSTRAP_TASK"`
    ),
    "cross_platform_bootstrap_pair"
);
require(
    unix_bootstrap.contains(`chmod +x "$WORKER"`)
        && !unix_bootstrap.contains(`chmod +x -- "$WORKER"`),
    "cross_platform_macos_bsd_chmod"
);
let check_source = std::fs::read_to_string(
    std::path::join(repo, "scripts/rhai/check.rhai").display
);
require(
    check_source.contains(`"native public catalog clients"`)
        && check_source.contains(`"client-build"`)
        && check_source.contains(`"agenterm-cli"`)
        && check_source.contains(`"agenterm-mux"`),
    "cross_platform_unix_prd_catalog_bootstrap"
);
let manifest = rhai::json::parse_file(
    std::path::join(repo, "agenterm.tasks.json").display
);
let check_tasks = [];
for task in manifest.tasks {
    if task.id == "check" {
        check_tasks.push(task);
    }
}
require(
    check_tasks.len == 1
        && check_tasks[0].platforms.contains("windows")
        && check_tasks[0].platforms.contains("linux")
        && check_tasks[0].platforms.contains("macos"),
    "cross_platform_check_task_platforms"
);
let ci_source = std::fs::read_to_string(
    std::path::join(repo, ".github/workflows/ci.yml").display
);
for evidence in [
    "AGENTERM_BOOTSTRAP_TASK: cross-platform-automation-audit",
    "AGENTERM_BOOTSTRAP_TASK: client-build",
    "AGENTERM_BOOTSTRAP_TASK: client-clippy",
    "AGENTERM_BOOTSTRAP_TASK: client-smoke",
    "--test linux_script_cli"
] {
    require(
        ci_source.contains(evidence),
        "cross_platform_ci_evidence:" + evidence
    );
}
let release_source = std::fs::read_to_string(
    std::path::join(repo, ".github/workflows/candidate.yml").display
);
require(
    release_source.contains(
        "AGENTERM_BOOTSTRAP_TASK: package-client-release"
    ),
    "cross_platform_release_package_task"
);
let client_source = std::fs::read_to_string(
    std::path::join(repo, "src/client/mod.rs").display
);
require(
    client_source.contains("run_script_command_direct(&arguments)")
        && client_source.contains("fn run_script_command_direct(")
        && client_source.contains("return run_script_task_command(arguments)")
        && !client_source.contains(
            "fn run_script_command(_arguments: &[String])"
        ),
    "cross_platform_task_host_unavailable"
);
print(
    rhai::json::stringify_pretty(#{
        schema_version: 1,
        checked_files: checked,
        alias_pairs: aliases.keys().len + 1,
        batch_entry_count: actual_batch.len,
        duplicate_business_rules: 0
    })
);
