// Prove retained CLI, GUI, and Script failure bundles are bounded, private,
// identity-checked, and orphan-free through public executable journeys.
//
// Args: REPO CLI_EXE SCRIPT_EXE

import "scripts/rhai/lib/test_harness" as test_harness;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn trimmed(value) {
    let text = value.to_string();
    text.trim();
    text
}

fn retained_bundles(bundle_root) {
    let paths = [];
    if !std::fs::exists(bundle_root) {
        return paths;
    }
    for entry in std::fs::read_dir(bundle_root) {
        if entry.is_dir && !entry.is_symlink {
            paths.push(std::path::absolute(entry.path.display).display);
        }
    }
    paths.sort();
    paths
}

fn only_new_bundle(bundle_root, before, suite) {
    let matches = [];
    for path in retained_bundles(bundle_root) {
        let name = std::path::PathBuf::from(path).file_name;
        if !before.contains(path) && name.starts_with(suite + "-") {
            matches.push(path);
        }
    }
    require(
        matches.len == 1,
        "diagnostic_new_bundle_count:" + suite + ":" + matches.len
    );
    matches[0]
}

fn remove_bundle(bundle_root, path) {
    let root = std::path::absolute(bundle_root).display;
    let resolved = std::path::absolute(path).display;
    require(
        std::path::parent(resolved).display.to_lower() ==
            root.to_lower(),
        "diagnostic_bundle_outside_owned_root:" + resolved
    );
    if std::fs::exists(resolved) {
        std::fs::remove_dir_all(resolved);
    }
    require(
        !std::fs::exists(resolved),
        "diagnostic_bundle_cleanup_failed:" + resolved
    );
}

fn assert_bounded_bundle(path, suite, marker_kind) {
    let manifest_path = std::path::join(
        path,
        "manifest.json"
    ).display;
    let cleanup_path = std::path::join(
        path,
        "cleanup.json"
    ).display;
    require(
        std::fs::exists(manifest_path)
            && std::fs::exists(cleanup_path),
        "diagnostic_bundle_required_file:" + suite
    );
    let manifest = rhai::json::parse_file(manifest_path);
    let cleanup = rhai::json::parse_file(cleanup_path);
    let marker =
        "INTERNAL_FAILURE_BUNDLE_PROBE:" + marker_kind + ":" +
        manifest.run_id;
    require(
        manifest.schema_version == 1
            && manifest.suite == suite
            && manifest.failure.split(marker).len - 1 == 1,
        "diagnostic_bundle_identity:" + suite + ":" + manifest.failure
    );
    require(
        manifest.privacy.command_arguments ==
            "known content-bearing arguments redacted"
            && manifest.privacy.output_limit_bytes == 65536
            && manifest.privacy.pane_capture ==
                "disabled for this suite"
            && !manifest.diagnostics.contains("capture-pane.txt"),
        "diagnostic_bundle_privacy:" + suite
    );

    let failure_directory = std::path::join(
        path,
        "failure"
    ).display;
    let names = #{};
    for name in manifest.diagnostics {
        require(
            std::path::PathBuf::from(name).file_name == name
                && !names.contains(name),
            "diagnostic_bundle_local_unique_name:" + suite
        );
        names[name] = true;
        let diagnostic = std::path::join(
            failure_directory,
            name
        ).display;
        require(
            std::fs::exists(diagnostic)
                && std::path::parent(
                    std::path::absolute(diagnostic).display
                ).display.to_lower() ==
                    std::path::absolute(
                        failure_directory
                    ).display.to_lower()
                && std::fs::metadata(diagnostic).len <= 65792,
            "diagnostic_bundle_diagnostic_bound:" + suite + ":" + name
        );
    }
    let command_log = std::path::join(
        path,
        manifest.command_log
    ).display;
    require(
        std::fs::exists(command_log)
            && std::fs::metadata(command_log).len <= 524544,
        "diagnostic_bundle_command_log_bound:" + suite
    );
    require(
        cleanup.orphan_free
            && cleanup.remaining_pids.len == 0
            && cleanup.remaining_windows.len == 0
            && cleanup.remaining_registrations.len == 0,
        "diagnostic_bundle_cleanup_not_orphan_free:" + suite
    );

    if suite == "theme" {
        let snapshot = std::fs::read_to_string(
            std::path::join(
                failure_directory,
                "ui-snapshot.txt"
            ).display
        );
        require(
            snapshot.starts_with("exit_code=0")
                && snapshot.contains(`"protocol_version": 1`)
                && snapshot.contains(`"window"`),
            "diagnostic_theme_snapshot_invalid"
        );
    }
    if suite == "script" {
        let commands_text = std::fs::read_to_string(command_log);
        let commands = rhai::json::parse(commands_text);
        let successful_worker = false;
        let observed_evals = [];
        for command in commands {
            let output = trimmed(command.output);
            if command.arguments.contains("script")
                    && command.arguments.contains("eval") {
                observed_evals.push(#{
                    exit_code: command.exit_code,
                    output: output
                });
                if command.exit_code == 0 && output == "42" {
                    successful_worker = true;
                }
            }
        }
        require(
            successful_worker,
            "diagnostic_script_success_not_retained:" +
                rhai::json::stringify(observed_evals)
        );
        let retained =
            manifest.failure + "\n" + commands_text;
        for name in manifest.diagnostics {
            retained += "\n" + std::fs::read_to_string(
                std::path::join(failure_directory, name).display
            );
        }
        require(
            !retained.contains("AUDIT_ENV_SECRET")
                && !retained.contains("40 + 2"),
            "diagnostic_script_private_input_retained"
        );
    }
}

fn start_task_probe(
    context,
    script,
    repo,
    entry,
    probe_arguments,
    timeout,
    extra_limits
) {
    let arguments = [
        "run", entry,
        "--profile", "local",
        "--project-root", repo,
        "--timeout-ms", timeout,
        "--max-operations", "10000000"
    ];
    arguments.append(extra_limits);
    arguments.push("--");
    arguments.append(probe_arguments);
    arguments.push("--internal-failure-bundle-probe");
    let command = test_harness::configured_command(
        context,
        script,
        arguments
    );
    command.timeout(std::time::Duration::from_secs(45));
    command.capture_limit(262144);
    command.start()
}

fn complete_task_probe(child, suite) {
    let output = child.wait_with_output(
        std::time::Duration::from_secs(45)
    );
    require(
        !output.success,
        "diagnostic_probe_unexpected_success:" + suite
    );
    output
}

require(
    args.len == 4,
    "expected: REPO CLI_EXE SCRIPT_EXE GUI_EXE"
);
let repo = std::path::absolute(args[0]).display;
let cli = std::path::absolute(args[1]).display;
let script = std::path::absolute(args[2]).display;
let gui = std::path::absolute(args[3]).display;
require(
    std::fs::exists(cli)
        && std::fs::exists(script)
        && std::fs::exists(gui),
    "diagnostic_executable_missing"
);
let bundle_root = std::path::join(
    repo,
    "target/smoke/test-runs"
).display;
let initial = retained_bundles(bundle_root);
let outer_context = test_harness::new_context(
    repo,
    "diagnostic-orchestrator"
);
let gui_probe = ();
let script_probe = ();
let failure = "";

try {
    print("STEP CLI failure bundle");
    let before = retained_bundles(bundle_root);
    let cli_context = test_harness::new_context(
        repo,
        "diagnostic-cli-probe"
    );
    let marker =
        "INTERNAL_FAILURE_BUNDLE_PROBE:cli:" + cli_context.run_id;
    let result = test_harness::run_cli(
        cli_context,
        cli,
        ["__diagnostic_bundle_probe_invalid__"],
        true,
        []
    );
    require(!result.success, "diagnostic_cli_probe_succeeded");
    test_harness::complete(
        cli_context,
        [],
        false,
        marker + "\n" + result.stdout + result.stderr
    );
    let cli_bundle = only_new_bundle(
        bundle_root,
        before,
        "diagnostic-cli-probe"
    );
    assert_bounded_bundle(
        cli_bundle,
        "diagnostic-cli-probe",
        "cli"
    );
    remove_bundle(bundle_root, cli_bundle);

    print("STEP GUI and script-worker failure bundles in parallel");
    let gui_before = retained_bundles(bundle_root);
    gui_probe = start_task_probe(
        outer_context,
        script,
        repo,
        std::path::join(
            repo,
            "scripts/rhai/theme-smoke.rhai"
        ).display,
        [repo, gui, cli],
        "60000",
        []
    );
    let script_before = retained_bundles(bundle_root);
    script_probe = start_task_probe(
        outer_context,
        script,
        repo,
        std::path::join(
            repo,
            "scripts/rhai/script-smoke.rhai"
        ).display,
        [repo, cli, script, gui],
        "120000",
        [
            "--max-string-bytes", "8388608",
            "--max-output-bytes", "1048576"
        ]
    );
    complete_task_probe(gui_probe, "theme");
    gui_probe = ();
    let gui_bundle = only_new_bundle(
        bundle_root,
        gui_before,
        "theme"
    );
    assert_bounded_bundle(gui_bundle, "theme", "gui");
    remove_bundle(bundle_root, gui_bundle);

    complete_task_probe(script_probe, "script");
    script_probe = ();
    let script_bundle = only_new_bundle(
        bundle_root,
        script_before,
        "script"
    );
    assert_bounded_bundle(script_bundle, "script", "script");
    remove_bundle(bundle_root, script_bundle);
    print(
        "PASS: CLI, GUI, and script failure bundles are bounded and orphan-free"
    );
} catch (error) {
    failure = error.to_string();
}

for child in [gui_probe, script_probe] {
    if child != () && child.state == "running" {
        child.kill();
        child.wait_with_output(
            std::time::Duration::from_secs(2)
        );
    }
}
for path in retained_bundles(bundle_root) {
    if !initial.contains(path)
            && path != outer_context.run_directory {
        remove_bundle(bundle_root, path);
    }
}
test_harness::remove_run(outer_context);
if failure != "" {
    throw failure;
}
