// Cross-platform UX parity smoke orchestrator.
// Canonical runtime for local execution is `agenterm-rhai task run`.
// It chains the existing platform-specific smoke suites with a shared evidence map
// and keeps a single launch contract in one place.
//
// Usage:
//   REPO GUI_EXE SCRIPT_EXE CLI_EXE PLATFORM
// platform: windows|linux|macos
// SCRIPT_EXE is optional and defaults to dist/{agenterm-rhai(.exe)|agenterm-rhai}.

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "ux-parity.startup",
    "ux-parity.startup-title",
    "ux-parity.wake-coalescing",
    "ux-parity.linux.unix-frontend.workbench",
    "ux-parity.linux.unix-frontend.clipboard",
    "ux-parity.macos.unix-frontend.workbench",
    "ux-parity.macos.unix-frontend.clipboard",
    "ux-parity.remote-ui.replaceable-client",
    "ux-parity.remote-ui.selection",
    "ux-parity.window-focus-contract"
];

let evidence_profiles = [
    #{
        id: "ux-parity.startup",
        branch: "startup",
        scenario: "first-window-startup",
        owner: "platform",
        windows_supported: true,
        linux_supported: true,
        macos_supported: true,
        unsupported_root_cause: ""
    },
    #{
        id: "ux-parity.startup-title",
        branch: "startup",
        scenario: "startup-title",
        owner: "platform",
        windows_supported: true,
        linux_supported: true,
        macos_supported: true,
        unsupported_root_cause: ""
    },
    #{
        id: "ux-parity.wake-coalescing",
        branch: "ux-startup",
        scenario: "gui-wake-contract",
        owner: "platform",
        windows_supported: true,
        linux_supported: true,
        macos_supported: true,
        unsupported_root_cause: ""
    },
    #{
        id: "ux-parity.linux.unix-frontend.workbench",
        branch: "frontend-lx",
        scenario: "linux-workbench",
        owner: "ui",
        windows_supported: false,
        linux_supported: true,
        macos_supported: false,
        unsupported_root_cause: "platform-gap"
    },
    #{
        id: "ux-parity.linux.unix-frontend.clipboard",
        branch: "frontend-lx",
        scenario: "linux-clipboard",
        owner: "platform",
        windows_supported: false,
        linux_supported: true,
        macos_supported: false,
        unsupported_root_cause: "platform-gap"
    },
    #{
        id: "ux-parity.macos.unix-frontend.workbench",
        branch: "frontend-mx",
        scenario: "macos-workbench",
        owner: "ui",
        windows_supported: false,
        linux_supported: false,
        macos_supported: true,
        unsupported_root_cause: "platform-gap"
    },
    #{
        id: "ux-parity.macos.unix-frontend.clipboard",
        branch: "frontend-mx",
        scenario: "macos-clipboard",
        owner: "platform",
        windows_supported: false,
        linux_supported: false,
        macos_supported: true,
        unsupported_root_cause: "platform-gap"
    },
    #{
        id: "ux-parity.remote-ui.replaceable-client",
        branch: "remote-ui",
        scenario: "replaceable-client",
        owner: "ui",
        windows_supported: true,
        linux_supported: false,
        macos_supported: false,
        unsupported_root_cause: "windows-only-contract"
    },
    #{
        id: "ux-parity.remote-ui.selection",
        branch: "remote-ui",
        scenario: "selection",
        owner: "ui",
        windows_supported: true,
        linux_supported: false,
        macos_supported: false,
        unsupported_root_cause: "windows-only-contract"
    },
    #{
        id: "ux-parity.window-focus-contract",
        branch: "ux-startup",
        scenario: "window-focus-contract",
        owner: "ui",
        windows_supported: true,
        linux_supported: true,
        macos_supported: true,
        unsupported_root_cause: ""
    }
];

fn has_emitted_evidence(emitted, evidence_id) {
    let hit = false;
    for entry in emitted {
        if entry == evidence_id {
            hit = true;
        }
    }
    hit
}

fn classify_failure_root(failure) {
    if failure == "" {
        return "";
    }

    if failure.contains("startup_gui_early_exit") {
        return "infra/startup-gui-early-exit";
    }
    if failure.contains("startup_window_discovery_timeout") {
        return "infra/startup-no-window";
    }
    if failure.contains("startup_stream_text_timeout") {
        return "infra/startup-stream-timeout";
    }
    if failure.contains("startup_first_window_budget") {
        return "ux/startup-latency-budget";
    }
    if failure.contains("wake_smoke") || failure.contains("ux-parity wake contract") {
        return "ux/wake-contract";
    }
    if failure.contains("runtime_error") || failure.contains("script_runtime") {
        return "infra/rhai-runtime";
    }
    if failure.contains("script_not_found") || failure.contains("task_not_found") {
        return "infra/script-task-mapping";
    }
    "bug/unknown"
}

fn platform_state_for(profile, run_platform, emitted, failure) {
    let pending = "not-executed-yet";
    let emitted_for_profile = has_emitted_evidence(emitted, profile.id);
    let windows_state = if !profile.windows_supported {
        "Unsupported"
    } else if run_platform == "windows" && failure == "" && emitted_for_profile {
        "Supported"
    } else if run_platform == "windows" && failure != "" {
        "Failed"
    } else {
        pending
    };
    let linux_state = if !profile.linux_supported {
        "Unsupported"
    } else if run_platform == "linux" && failure == "" && emitted_for_profile {
        "Supported"
    } else if run_platform == "linux" && failure != "" {
        "Failed"
    } else {
        pending
    };
    let macos_state = if !profile.macos_supported {
        "Unsupported"
    } else if run_platform == "macos" && failure == "" && emitted_for_profile {
        "Supported"
    } else if run_platform == "macos" && failure != "" {
        "Failed"
    } else {
        pending
    };

    let windows_root = if windows_state == "Unsupported" {
        profile.unsupported_root_cause
    } else if windows_state == "Failed" {
        classify_failure_root(failure)
    } else {
        ""
    };
    let linux_root = if linux_state == "Unsupported" {
        profile.unsupported_root_cause
    } else if linux_state == "Failed" {
        classify_failure_root(failure)
    } else {
        ""
    };
    let macos_root = if macos_state == "Unsupported" {
        profile.unsupported_root_cause
    } else if macos_state == "Failed" {
        classify_failure_root(failure)
    } else {
        ""
    };

    #{
        windows: windows_state,
        linux: linux_state,
        macos: macos_state,
        windows_root: windows_root,
        linux_root: linux_root,
        macos_root: macos_root
    }
}

fn write_matrix_reports(context, platform, emitted, failure, emit_matrix, profiles) {
    if !emit_matrix {
        return;
    }

    let suite = "platform-ux-parity-smoke";
    let timestamp = std::time::SystemTime::now().rfc3339;
    let report_path_json = std::path::join(
        context.run_directory,
        suite + "-matrix.json"
    ).display;
    let report_path_csv = std::path::join(
        context.run_directory,
        suite + "-matrix.csv"
    ).display;

    let matrix_result = [];
    for profile in profiles {
        let per_platform = platform_state_for(profile, platform, emitted, failure);
        let row_state = if platform == "windows" {
            per_platform.windows
        } else if platform == "linux" {
            per_platform.linux
        } else {
            per_platform.macos
        };
        let row_root = if platform == "windows" {
            per_platform.windows_root
        } else if platform == "linux" {
            per_platform.linux_root
        } else {
            per_platform.macos_root
        };
        let record = #{
            branch: profile.branch,
            scenario: profile.scenario,
            evidence_id: profile.id,
            windows: per_platform.windows,
            linux: per_platform.linux,
            macos: per_platform.macos,
            state: row_state,
            root_cause: row_root,
            owner: profile.owner
        };
        matrix_result.push(record);
    }

    if emit_matrix {
        let json_payload = #{
            run_id: context.run_id,
            timestamp_utc: timestamp,
            suite: suite,
            environment: #{
                branch: "",
                commit: "",
                runner: ""
            },
            result: matrix_result
        };
        rhai::runtime::atomic_write(
            report_path_json,
            rhai::json::stringify_pretty(json_payload) + "\n"
        );

        let csv_rows = [
            "run_id,timestamp_utc,suite,branch,scenario,evidence_id,platform,state,root_cause,owner"
        ];
        for profile in profiles {
            let per_platform = platform_state_for(profile, platform, emitted, failure);
            let row_state = if platform == "windows" {
                per_platform.windows
            } else if platform == "linux" {
                per_platform.linux
            } else {
                per_platform.macos
            };
            let row_root = if platform == "windows" {
                per_platform.windows_root
            } else if platform == "linux" {
                per_platform.linux_root
            } else {
                per_platform.macos_root
            };
            let row = context.run_id + "," + timestamp + "," + suite + "," +
                profile.branch + "," + profile.scenario + "," + profile.id + "," +
                platform + "," + row_state + "," + row_root + "," + profile.owner;
            csv_rows.push(row);
        }
        let csv_body = "";
        for row in csv_rows {
            if csv_body != "" {
                csv_body += "\n";
            }
            csv_body += row;
        }
        rhai::runtime::atomic_write(report_path_csv, csv_body + "\n");
    }
}

let emit_matrix = args.contains("--emit-matrix");
let positional_args = [];
for argument in args {
    if argument == "--list-evidence" {
        continue;
    }
    if argument == "--emit-matrix" {
        continue;
    }
    positional_args.push(argument);
}

if args.contains("--list-evidence") {
    for id in declared_evidence {
        print(id);
    }
    return;
}

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn absolute(value) {
    std::path::absolute(value).display
}

fn resolve_script_binary(script_exe, repository) {
    let candidates = [
        script_exe,
        std::path::join(repository, "dist/agenterm-rhai.exe").display,
        std::path::join(repository, "dist/agenterm-rhai").display
    ];

    for candidate in candidates {
        if std::fs::exists(candidate) {
            return candidate;
        }
    }
    throw "no_available_script_runtime";
}

fn run_script_task(context, script_bin, repository, task_file, task_args, task_label) {
    let command = std::process::command(script_bin);
    command.current_dir(repository);
    let invoked = ["task", "run", task_file, repository];
    command.args(invoked);
    command.args(task_args);
    command.timeout(std::time::Duration::from_secs(300));
    command.capture_limit(262144);
    command.env("AGENTERM_IPC_ADDRESS", context.address);
    command.env("AGENTERM_WORKSPACE_PATH", context.workspace_path);
    command.env("AGENTERM_SETTINGS_PATH", context.settings_path);
    command.env("AGENTERM_INSTANCE_DIR", context.instance_directory);
    command.env("AGENTERM_NO_ACTIVATE", "1");

    let output = command.output();
    let record_arguments = [];
    for argument in invoked {
        record_arguments.push(argument);
    }
    for argument in task_args {
        record_arguments.push(argument);
    }
    test_harness::append_command_record(context, record_arguments, output, false, []);
    if !output.success {
        throw task_label + "_failed:" + output.stderr_text();
    }
    print("TASK " + task_label + "_PASS");
    return output;
}

fn emit(context, declared, id, observed_evidence) {
    test_harness::emit_evidence(context, declared, id);
    let observed = observed_evidence;
    observed.push(id);
    observed
}

fn parse_platform_args(positional_args) {
    let args_len = positional_args.len;
    require(args_len >= 4, "expected: REPO GUI_EXE [SCRIPT_EXE] CLI_EXE PLATFORM");

    let platform = positional_args[args_len - 1].to_lower();

    let attempt_repo = absolute(positional_args[args_len - 4]);
    let attempt_gui = absolute(positional_args[args_len - 3]);
    let attempt_cli = absolute(positional_args[args_len - 2]);
    let short = #{
        repository: attempt_repo,
        gui_exe: attempt_gui,
        script_exe: "",
        cli_exe: attempt_cli,
        platform: platform
    };
    if args_len >= 5 {
        let maybe_repo = absolute(positional_args[args_len - 5]);
        let maybe_gui = absolute(positional_args[args_len - 4]);
        let maybe_script = absolute(positional_args[args_len - 3]);
        let maybe_cli = absolute(positional_args[args_len - 2]);
        if maybe_script.to_lower().contains("agenterm-rhai") &&
                maybe_gui.ends_with("agenterm.exe") &&
                maybe_cli.ends_with("agenterm-cli.exe") {
            short.repository = maybe_repo;
            short.gui_exe = maybe_gui;
            short.script_exe = maybe_script;
            short.cli_exe = maybe_cli;
        }
    }
    short
}

let parsed = parse_platform_args(positional_args);
let repository = parsed.repository;
let gui_exe = parsed.gui_exe;
let script_exe = parsed.script_exe;
let cli_exe = parsed.cli_exe;
let platform = parsed.platform;
require(
    platform == "windows" || platform == "linux" || platform == "macos",
    "unsupported platform (windows|linux|macos)"
);
require(std::fs::exists(gui_exe), "platform_gui_missing");
require(std::fs::exists(cli_exe), "platform_cli_missing");
let runtime_script = resolve_script_binary(script_exe, repository);
require(std::fs::exists(runtime_script), "platform_script_missing");

let context = test_harness::new_context(repository, "platform-ux-parity");
let owned_children = [];
let failure = "";
let emitted = [];

let succeeded = false;

try {
    print("STEP startup contract and first-window behavior");
    run_script_task(
        context,
        runtime_script,
        repository,
        "startup-smoke",
        [gui_exe, cli_exe, "1000"],
        "startup"
    );
    emitted = emit(context, declared_evidence, "ux-parity.startup", emitted);
    emitted = emit(context, declared_evidence, "ux-parity.startup-title", emitted);
    run_script_task(
        context,
        runtime_script,
        repository,
        "wake-smoke",
        [gui_exe, cli_exe],
        "wake-contract"
    );
    emitted = emit(context, declared_evidence, "ux-parity.wake-coalescing", emitted);

    if platform == "windows" {
        print("STEP windows remote-ui behavior (replaceable client + selection + clipboard)");
        run_script_task(
            context,
            runtime_script,
            repository,
            "remote-ui-smoke",
            [gui_exe, cli_exe],
            "remote-ui"
        );
        emitted = emit(
            context,
            declared_evidence,
            "ux-parity.remote-ui.replaceable-client",
            emitted
        );
        emitted = emit(
            context,
            declared_evidence,
            "ux-parity.remote-ui.selection",
            emitted
        );
        emitted = emit(context, declared_evidence, "ux-parity.window-focus-contract", emitted);
    } else {
        let workbench_evidence = "ux-parity." + platform + ".unix-frontend.workbench";
        let clipboard_evidence = "ux-parity." + platform + ".unix-frontend.clipboard";
        print("STEP unix frontend behavior by platform (workbench + focus contract)");
        run_script_task(
            context,
            runtime_script,
            repository,
            "unix-frontend-" + platform + "-smoke",
            [gui_exe, cli_exe, "--platform", platform],
            "unix-frontend"
        );
        emitted = emit(context, declared_evidence, workbench_evidence, emitted);
        emitted = emit(context, declared_evidence, clipboard_evidence, emitted);
        emitted = emit(context, declared_evidence, "ux-parity.window-focus-contract", emitted);
    }

    succeeded = true;
} catch (error) {
    failure = error.to_string();
};
write_matrix_reports(
    context,
    platform,
    emitted,
    failure,
    emit_matrix,
    evidence_profiles
);
test_harness::complete(context, owned_children, succeeded, failure);
