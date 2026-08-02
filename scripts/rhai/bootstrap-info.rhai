// Report stage-0 inputs without performing a build or changing the repository.
//
// Named task:
//   agenterm-rhai task run bootstrap-info --json

fn checked_output(program, argv, step) {
    let command = std::process::command(program);
    command.args(argv);
    command.timeout(std::time::Duration::from_secs(10));
    let output = command.output();
    if !output.success {
        throw output.error(step);
    }
    let text = output.stdout_text();
    text.trim();
    text
}

let repo = std::env::current_dir().display;
let report = #{
    schema_version: 1,
    task_id: "bootstrap-info",
    repository: repo,
    commit: checked_output("git", ["rev-parse", "HEAD"], "bootstrap-git-head"),
    branch: checked_output(
        "git",
        ["branch", "--show-current"],
        "bootstrap-git-branch"
    ),
    cargo: checked_output("cargo", ["--version"], "bootstrap-cargo"),
    rustc: checked_output("rustc", ["--version"], "bootstrap-rustc"),
    script_api_version: 2,
    task_manifest_schema_version: 3
};

print(rhai::json::stringify_pretty(report));
