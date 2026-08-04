// Repository quality, qualification, and receipt orchestration.
//
// Args:
//   REPO WORKER [--quick] [--release] [--skip-smoke] [--include-stress]
//     [--internal-qualification-dry-run] [--receipt PATH] [--target TARGET]

import "scripts/rhai/lib/qualification" as qualification;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn merged(left, right) {
    let result = #{};
    for key in left.keys() {
        result[key] = left[key];
    }
    for key in right.keys() {
        result[key] = right[key];
    }
    result
}

fn joined_text(values, separator) {
    let result = "";
    for index in 0..values.len {
        if index > 0 {
            result += separator;
        }
        result += values[index].to_string();
    }
    result
}

fn spec(program, arguments, timeout_ms, environment) {
    #{
        program: program,
        arguments: arguments,
        timeout_ms: timeout_ms,
        environment: environment,
        allow_powershell: false
    }
}

fn powershell_descendants(root_id) {
    let parents = #{};
    let names = #{};
    for process in std::process::list() {
        let key = process.id.to_string();
        parents[key] = process.parent_id;
        names[key] = process.executable_name;
    }
    let matches = [];
    for key in names.keys() {
        let name = names[key].to_lower();
        if name != "powershell.exe" && name != "pwsh.exe" {
            continue;
        }
        let current = key;
        let depth = 0;
        while depth < 64 && parents.contains(current) {
            if current == root_id.to_string() {
                matches.push(name + ":" + key);
                break;
            }
            let parent = parents[current];
            if parent == 0 {
                break;
            }
            current = parent.to_string();
            depth += 1;
        }
    }
    matches
}

fn execute(command_spec, repo, scratch, sequence) {
    let stdout_path = std::path::join(
        scratch,
        "command-" + sequence + ".stdout"
    ).display;
    let stderr_path = std::path::join(
        scratch,
        "command-" + sequence + ".stderr"
    ).display;
    let command = std::process::command(command_spec.program);
    command.args(command_spec.arguments);
    command.current_dir(repo);
    for key in command_spec.environment.keys() {
        command.env(key, command_spec.environment[key]);
    }
    command.timeout(
        std::time::Duration::from_millis(
            command_spec.timeout_ms + 10000
        )
    );
    command.stdout_file(stdout_path);
    command.stderr_file(stderr_path);
    let started = std::time::SystemTime::now().unix_millis;
    let child = command.start();
    let observed_powershell = [];
    let last_process_scan = 0;
    let process_samples = 0;
    while child.state == "running"
            && std::time::SystemTime::now().unix_millis - started
                < command_spec.timeout_ms {
        let now = std::time::SystemTime::now().unix_millis;
        if last_process_scan == 0 || now - last_process_scan >= 1000 {
            for process in powershell_descendants(child.id) {
                if !observed_powershell.contains(process) {
                    observed_powershell.push(process);
                }
            }
            process_samples += 1;
            last_process_scan = now;
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    if child.state == "running" {
        child.kill();
    }
    let result = child.wait_with_output(
        std::time::Duration::from_secs(5)
    );
    let stdout =
        if std::fs::exists(stdout_path) {
            std::fs::read_to_string(stdout_path)
        } else {
            ""
        };
    let stderr =
        if std::fs::exists(stderr_path) {
            std::fs::read_to_string(stderr_path)
        } else {
            ""
        };
    let duration =
        std::time::SystemTime::now().unix_millis - started;
    let timed_out = duration > command_spec.timeout_ms;
    let powershell_violation =
        observed_powershell.len > 0 && !command_spec.allow_powershell;
    #{
        success:
            result.success
                && !powershell_violation
                && !timed_out,
        exit_code: result.exit_code,
        duration_ms: duration,
        stdout: stdout,
        stderr:
            stderr +
            if powershell_violation {
                "\nrepository_automation_spawned_powershell:" +
                    joined_text(observed_powershell, ",")
            } else {
                ""
            } +
            if timed_out {
                "\ngate_deadline_exceeded:" +
                    command_spec.timeout_ms
            } else {
                ""
            },
        observed_powershell: observed_powershell,
        process_samples: process_samples
    }
}

fn output_lines(text) {
    let lines = [];
    for source_line in text.split("\n") {
        let line = source_line;
        line.trim();
        if line != "" {
            lines.push(line);
        }
    }
    lines
}

fn tail(text, maximum) {
    if text.len <= maximum {
        return text;
    }
    text.sub_string(text.len - maximum, maximum)
}

/// Pull the innermost script diagnostic out of a nested gate failure.
///
/// A failing inner task reports a JSON envelope whose `message` embeds
/// another envelope, so the actionable code (for example
/// `supply_chain_source_missing:softbuffer:0.4.8`) ends up several quoted
/// layers deep. Surfacing it first keeps the CI summary readable without
/// discarding the full payload printed below it.
fn innermost_diagnostic(text) {
    let flat = text;
    flat.replace(`\\"`, `"`);
    flat.replace(`\\n`, "\n");
    let marker = `"message":"Runtime error: `;
    let rest = flat;
    let found = "";
    while true {
        let at = rest.index_of(marker);
        if at < 0 {
            break;
        }
        rest = rest.sub_string(at + marker.len);
        let stop = rest.index_of("\n");
        let quote = rest.index_of(`"`);
        if quote >= 0 && (stop < 0 || quote < stop) {
            stop = quote;
        }
        let candidate = if stop < 0 { rest } else { rest.sub_string(0, stop) };
        candidate.trim();
        if candidate != "" {
            found = candidate;
        }
    }
    found
}

fn run_gate(context, id, label, specs, repo, scratch, sequence) {
    print("\n==> " + label);
    let started = std::time::SystemTime::now().unix_millis;
    let all_output = [];
    for command_spec in specs {
        let result = execute(command_spec, repo, scratch, sequence);
        sequence += 1;
        context = qualification::add_process_observation(
            context,
            result.process_samples,
            result.observed_powershell,
            command_spec.allow_powershell
        );
        if result.stdout != "" {
            print(result.stdout);
        }
        if result.stderr != "" {
            print(result.stderr);
        }
        if result.observed_powershell.len > 0
                && command_spec.allow_powershell {
            print(
                "OBSERVED explicit terminal compatibility payload: " +
                    joined_text(result.observed_powershell, ",")
            );
        }
        all_output.append(output_lines(result.stdout));
        all_output.append(output_lines(result.stderr));
        if !result.success {
            let duration =
                std::time::SystemTime::now().unix_millis - started;
            context.timing = qualification::timing_set_gate(
                context.timing,
                id,
                "failed",
                duration,
                result.exit_code,
                result.stderr.contains("gate_deadline_exceeded:")
            );
            context.timing =
                qualification::timing_finish(context.timing, "failed");
            let payload = result.stdout + result.stderr;
            let diagnostic = innermost_diagnostic(payload);
            let headline = if diagnostic == "" {
                ""
            } else {
                "\n  cause: " + diagnostic
            };
            throw label + " failed with exit code " + result.exit_code +
                headline + "\n" + tail(payload, 16384);
        }
    }
    let duration =
        std::time::SystemTime::now().unix_millis - started;
    context = qualification::add_result(
        context,
        id,
        "passed",
        duration,
        all_output
    );
    context.timing = qualification::timing_set_gate(
        context.timing,
        id,
        "passed",
        duration,
        0,
        false
    );
    print("PASS: " + label + " (" + duration + " ms)");
    #{context:context,sequence:sequence}
}

fn run_quick_gate(timing, id, label, specs, repo, scratch, sequence) {
    print("\n==> " + label);
    let started = std::time::SystemTime::now().unix_millis;
    for command_spec in specs {
        let result = execute(command_spec, repo, scratch, sequence);
        sequence += 1;
        if result.stdout != "" {
            print(result.stdout);
        }
        if result.stderr != "" {
            print(result.stderr);
        }
        if !result.success {
            let duration =
                std::time::SystemTime::now().unix_millis - started;
            timing = qualification::timing_set_gate(
                timing,
                id,
                "failed",
                duration,
                result.exit_code,
                result.stderr.contains("gate_deadline_exceeded:")
            );
            timing = qualification::timing_finish(timing, "failed");
            throw label + " failed with exit code " + result.exit_code +
                "\n" + tail(result.stdout + result.stderr, 16384);
        }
    }
    let duration =
        std::time::SystemTime::now().unix_millis - started;
    timing = qualification::timing_set_gate(
        timing,
        id,
        "passed",
        duration,
        0,
        false
    );
    print("PASS: " + label + " (" + duration + " ms)");
    #{timing:timing,sequence:sequence}
}

fn run_step(label, specs, repo, scratch, sequence) {
    print("\n==> " + label);
    let started = std::time::SystemTime::now().unix_millis;
    for command_spec in specs {
        let result = execute(command_spec, repo, scratch, sequence);
        sequence += 1;
        if result.stdout != "" {
            print(result.stdout);
        }
        if result.stderr != "" {
            print(result.stderr);
        }
        require(
            result.success,
            label + " failed with exit code " + result.exit_code +
                "\n" + tail(result.stdout + result.stderr, 16384)
        );
    }
    print(
        "PASS: " + label + " (" +
            (
                std::time::SystemTime::now().unix_millis - started
            ) +
            " ms)"
    );
    sequence
}

fn task(worker, manifest, id, timeout_ms, extra) {
    let arguments = [
        "task", "run", id,
        "--manifest", manifest
    ];
    if extra.len > 0 {
        arguments.push("--");
        arguments.append(extra);
    }
    spec(worker, arguments, timeout_ms + 10000, #{})
}

// Task caller arguments append after manifest defaults. Use a direct script
// invocation when a platform gate must replace platform-specific defaults.
fn direct_task(worker, repo, id, timeout_ms, arguments) {
    let command_arguments = [
        "run",
        std::path::join(repo, "scripts/rhai/" + id + ".rhai").display,
        "--profile", "local",
        "--project-root", repo,
        "--timeout-ms", timeout_ms.to_string(),
        "--max-operations", "10000000",
        "--max-output-bytes", "1048576",
        "--"
    ];
    command_arguments.append(arguments);
    spec(worker, command_arguments, timeout_ms + 10000, #{})
}

fn sorted(values) {
    let result = [];
    for value in values {
        let inserted = false;
        for index in 0..result.len {
            if value < result[index] {
                result.insert(index, value);
                inserted = true;
                break;
            }
        }
        if !inserted {
            result.push(value);
        }
    }
    result
}

fn exact(left, right) {
    let first = sorted(left);
    let second = sorted(right);
    if first.len != second.len {
        return false;
    }
    for index in 0..first.len {
        if first[index] != second[index] {
            return false;
        }
    }
    true
}

fn assert_declarations(context, worker, repo, scratch, sequence) {
    let manifest =
        std::path::join(repo, "agenterm.tasks.json").display;
    for gate in context.manifest.required_gates {
        if gate.evidence.len == 0 {
            continue;
        }
        let path = std::path::join(
            repo,
            "scripts/rhai/" + gate.id + ".rhai"
        ).display;
        require(
            std::fs::exists(path),
            "check_evidence_suite_missing:" + gate.id
        );
        let result = execute(
            spec(
                worker,
                [
                    "run", path,
                    "--profile", "local",
                    "--project-root", repo,
                    "--timeout-ms", "10000",
                    "--", "--list-evidence"
                ],
                15000,
                #{}
            ),
            repo,
            scratch,
            sequence
        );
        sequence += 1;
        require(
            result.success,
            "check_evidence_declaration_failed:" + gate.id +
                ":" + result.stderr
        );
        require(
            exact(gate.evidence, output_lines(result.stdout)),
            "check_evidence_declaration_stale:" + gate.id
        );
    }
    sequence
}

fn freeze_identity(worker, manifest, repo, scratch, sequence) {
    let path = std::path::join(scratch, "build-identity.cmd").display;
    let result = execute(
        task(
            worker,
            manifest,
            "build-identity",
            10000,
            [repo, "dev", path]
        ),
        repo,
        scratch,
        sequence
    );
    require(
        result.success,
        "check_build_identity:" + result.stdout + result.stderr
    );
    let environment = #{};
    for source_line in std::fs::read_to_string(path).split("\n") {
        let line = source_line;
        line.trim();
        if line == "" {
            continue;
        }
        require(
            line.starts_with("set \"") && line.ends_with("\""),
            "check_build_identity_line:" + line
        );
        let body = line.sub_string(5, line.len - 6);
        let parts = body.split("=");
        require(parts.len == 2, "check_build_identity_value:" + line);
        environment[parts[0]] = parts[1];
    }
    require(
        environment.keys().len == 6,
        "check_build_identity_fields"
    );
    #{environment:environment,sequence:sequence + 1}
}

fn slowest(results) {
    let ordered = [];
    for key in results.keys() {
        let value = results[key];
        let inserted = false;
        for index in 0..ordered.len {
            if value.duration_ms > ordered[index].duration_ms {
                ordered.insert(index, value);
                inserted = true;
                break;
            }
        }
        if !inserted {
            ordered.push(value);
        }
    }
    ordered
}

require(args.len >= 1, "expected: REPO [OPTIONS]");
let repo = std::path::absolute(args[0]).display;
require(
    std::env::has("AGENTERM_BOOTSTRAP_WORKER"),
    "check_bootstrap_worker_missing"
);
let bootstrap_worker =
    std::path::absolute(std::env::get("AGENTERM_BOOTSTRAP_WORKER")).display;
let quick = false;
let release = false;
let skip_smoke = false;
let include_stress = false;
let dry_run = false;
let quick_target = "";
let receipt_path =
    std::path::join(repo, "target/qualification/receipt.json").display;
let timing_path =
    std::path::join(repo, "target/qualification/timing.json").display;
let index = 1;
while index < args.len {
    let value = args[index];
    if value == "--quick" {
        quick = true;
    } else if value == "--release" {
        release = true;
    } else if value == "--skip-smoke" {
        skip_smoke = true;
    } else if value == "--include-stress" {
        include_stress = true;
    } else if value == "--internal-qualification-dry-run" {
        dry_run = true;
    } else if value == "--receipt" {
        require(index + 1 < args.len, "check_receipt_value_missing");
        receipt_path = std::path::absolute(args[index + 1]).display;
        index += 1;
    } else if value == "--target" {
        require(index + 1 < args.len, "check_target_value_missing");
        quick_target = args[index + 1];
        index += 1;
    } else if value == "--timing" {
        require(index + 1 < args.len, "check_timing_value_missing");
        timing_path = std::path::absolute(args[index + 1]).display;
        index += 1;
    } else {
        throw "check_unknown_argument:" + value;
    }
    index += 1;
}
require(
    quick_target.is_empty() || quick,
    "check_target_only_for_quick"
);
require(
    !quick || !(release || skip_smoke || include_stress || dry_run),
    "check_quick_option_conflict"
);
let unix_bootstrap =
    std::env::has("AGENTERM_BOOTSTRAP_PLATFORM")
        && std::env::get("AGENTERM_BOOTSTRAP_PLATFORM") == "unix";
if unix_bootstrap && !quick
        && !release && !skip_smoke && !include_stress && !dry_run {
    quick = true;
}
require(
    !unix_bootstrap || quick,
    "check_full_qualification_requires_windows"
);

let scratch = rhai::runtime::temp_dir().display;
let manifest = std::path::join(repo, "agenterm.tasks.json").display;
let sequence = 1;
if dry_run {
    sequence = run_step(
        "qualification fail-closed dry run",
        [task(
            bootstrap_worker,
            manifest,
            "qualification-selftest",
            20000,
            []
        )],
        repo,
        scratch,
        sequence
    );
    return;
}

if std::fs::exists(timing_path) {
    std::fs::remove_file(timing_path);
}

if quick {
    let quick_started = std::time::SystemTime::now().unix_millis;
    let quick_native_bin_root = if std::env::has("CARGO_TARGET_DIR") {
        std::path::absolute(std::env::get("CARGO_TARGET_DIR")).display
    } else {
        std::path::join(repo, "target").display
    };
    if quick_target != "" {
        quick_native_bin_root =
            std::path::join(quick_native_bin_root, quick_target).display;
    }
    quick_native_bin_root =
        std::path::join(quick_native_bin_root, "debug").display;
    let clippy_arguments = [
        "clippy", "--quiet", "--locked", "--all-targets", "--", "-D", "warnings"
    ];
    let unit_arguments = ["test", "--quiet", "--locked", "--lib"];
    let quick_client_build_args = [];
    if quick_target != "" {
        clippy_arguments.push("--target");
        clippy_arguments.push(quick_target);
        unit_arguments.push("--target");
        unit_arguments.push(quick_target);
        quick_client_build_args.push("--target");
        quick_client_build_args.push(quick_target);
    }
    let quick_gate_ids = [
        "repo-lint",
        "rustfmt",
        "prd-alignment",
        "clippy",
        "unit-tests"
    ];
    if unix_bootstrap {
        quick_gate_ids.insert(2, "native-public-catalog-clients");
    }
    let timing = qualification::new_timing(
        repo,
        std::path::join(
            repo,
            "scripts/qualification-gates.json"
        ).display,
        timing_path,
        "quick",
        "quick",
        #{unix_bootstrap:unix_bootstrap},
        quick_gate_ids
    );
    let prd_spec = task(
        bootstrap_worker,
        manifest,
        "prd-alignment",
        10000,
        []
    );
    if unix_bootstrap {
        let quick_native_bin = std::path::join(
            quick_native_bin_root,
            "agenterm-cli"
        ).display;
        prd_spec = direct_task(
            bootstrap_worker,
            repo,
            "prd-alignment",
            10000,
            [
                repo,
                quick_native_bin,
                std::path::join(quick_native_bin_root, "agenterm-mux").display
            ]
        );
    }
    let quick_gate = run_quick_gate(
        timing,
        "repo-lint",
        "repository static lint",
        [task(
            bootstrap_worker,
            manifest,
            "lint",
            120000,
            [bootstrap_worker, "static"]
        )],
        repo,
        scratch,
        sequence
    );
    timing = quick_gate.timing;
    sequence = quick_gate.sequence;
    let quick_gate = run_quick_gate(
        timing,
        "rustfmt",
        "rustfmt",
        [spec(
            "cargo",
            ["fmt", "--all", "--", "--check"],
            60000,
            #{}
        )],
        repo,
        scratch,
        sequence
    );
    timing = quick_gate.timing;
    sequence = quick_gate.sequence;
    if unix_bootstrap {
        let quick_gate = run_quick_gate(
            timing,
            "native-public-catalog-clients",
            "native public catalog clients",
            [task(
                bootstrap_worker,
                manifest,
                "client-build",
                600000,
                quick_client_build_args
            )],
            repo,
            scratch,
            sequence
        );
        timing = quick_gate.timing;
        sequence = quick_gate.sequence;
    }
    let quick_gate = run_quick_gate(
        timing,
        "prd-alignment",
        "PRD capability alignment",
        [prd_spec],
        repo,
        scratch,
        sequence
    );
    timing = quick_gate.timing;
    sequence = quick_gate.sequence;
    let identity = ();
    try {
        identity = freeze_identity(
            bootstrap_worker,
            manifest,
            repo,
            scratch,
            sequence
        );
    } catch (error) {
        timing = qualification::timing_fail_stage(
            timing,
            "build-identity"
        );
        throw error;
    }
    sequence = identity.sequence;
    let quick_gate = run_quick_gate(
        timing,
        "clippy",
        "all-target Clippy",
        [spec("cargo", clippy_arguments, 60000, identity.environment)],
        repo,
        scratch,
        sequence
    );
    timing = quick_gate.timing;
    sequence = quick_gate.sequence;
    let quick_gate = run_quick_gate(
        timing,
        "unit-tests",
        "library unit tests",
        [spec("cargo", unit_arguments, 60000, identity.environment)],
        repo,
        scratch,
        sequence
    );
    timing = quick_gate.timing;
    sequence = quick_gate.sequence;
    timing = qualification::timing_finish(timing, "passed");
    print(
        "\nPASS: AgenTerm quick development gate (" +
            (
                std::time::SystemTime::now().unix_millis -
                    quick_started
            ) +
            " ms)"
    );
    return;
}

if std::fs::exists(receipt_path) {
    std::fs::remove_file(receipt_path);
}
let context = qualification::new_context(
    std::path::join(
        repo,
        "scripts/qualification-gates.json"
    ).display,
    release,
    include_stress
);
let timing_gate_ids = [];
for declared in context.manifest.required_gates {
    timing_gate_ids.push(declared.id);
}
let timing_profile =
    if release && include_stress {
        "release-stress"
    } else if skip_smoke {
        "skip-smoke"
    } else {
        "full"
    };
context.timing = qualification::new_timing(
    repo,
    context.manifest_path,
    timing_path,
    if release { "candidate" } else { "ordinary" },
    timing_profile,
    #{
        release: release,
        stress_included: include_stress,
        smoke_included: !skip_smoke
    },
    timing_gate_ids
);
if !release {
    for id in [
        "release-preflight",
        "agenterm-net-research",
        "preflight-benchmark"
    ] {
        context.timing = qualification::timing_set_gate(
            context.timing, id, "skipped", 0, 0, false
        );
    }
}
if skip_smoke {
    for id in [
        "startup-smoke", "wake-smoke", "cli-smoke", "server-smoke",
        "native-ipc-smoke", "native-ipc-compat-smoke", "remote-ui-smoke",
        "remote-ui-upgrade-smoke", "fleet-smoke", "script-smoke",
        "theme-smoke", "working-context-smoke", "workbench-smoke",
        "control-center-smoke"
    ] {
        context.timing = qualification::timing_set_gate(
            context.timing, id, "skipped", 0, 0, false
        );
    }
}
let full_started = std::time::SystemTime::now().unix_millis;
let gate = run_gate(
    context,
    "preflight-selftest",
    "read-only preflight self-test",
    [spec(
        "cargo",
        [
            "test", "--quiet", "--locked",
            "--test", "rhai_migration",
            "preflight_task_is_fail_closed_and_writes_reports_for_real_git_fixtures",
            "--", "--nocapture"
        ],
        60000,
        #{}
    )],
    repo,
    scratch,
    sequence
);
context = gate.context;
sequence = gate.sequence;

if release {
    let release_preflight = run_gate(
        context,
        "release-preflight",
        "clean internal candidate preflight",
        [task(
            bootstrap_worker,
            manifest,
            "preflight",
            10000,
            [repo, std::path::join(
                repo,
                "target/preflight/preflight.json"
            ).display]
        )],
        repo,
        scratch,
        sequence
    );
    context = release_preflight.context;
    sequence = release_preflight.sequence;
    let net_research = run_gate(
        context,
        "agenterm-net-research",
        "isolated agenterm-net research",
        [task(
            bootstrap_worker,
            manifest,
            "agenterm-net-research",
            600000,
            []
        )],
        repo,
        scratch,
        sequence
    );
    context = net_research.context;
    sequence = net_research.sequence;
    let benchmark = run_gate(
        context,
        "preflight-benchmark",
        "local preflight p95 benchmark",
        [task(
            bootstrap_worker,
            manifest,
            "preflight-benchmark",
            60000,
            [
                bootstrap_worker,
                manifest,
                repo,
                std::path::join(
                    repo,
                    "target/preflight/benchmark.json"
                ).display,
                "5"
            ]
        )],
        repo,
        scratch,
        sequence
    );
    context = benchmark.context;
    sequence = benchmark.sequence;
}

let lint_gate = run_gate(
    context,
    "repo-lint",
    "repository static lint",
    [
        task(
            bootstrap_worker,
            manifest,
            "lint",
            120000,
            [bootstrap_worker, "--self-test"]
        ),
        task(
            bootstrap_worker,
            manifest,
            "lint",
            120000,
            [bootstrap_worker, "static"]
        )
    ],
    repo,
    scratch,
    sequence
);
context = lint_gate.context;
sequence = lint_gate.sequence;
try {
    sequence = assert_declarations(
        context,
        bootstrap_worker,
        repo,
        scratch,
        sequence
    );
} catch (error) {
    context.timing = qualification::timing_fail_stage(
        context.timing,
        "evidence-declarations"
    );
    throw error;
}
let rustfmt_gate = run_gate(
    context,
    "rustfmt",
    "rustfmt",
    [spec(
        "cargo",
        ["fmt", "--", "--check"],
        60000,
        #{}
    )],
    repo,
    scratch,
    sequence
);
context = rustfmt_gate.context;
sequence = rustfmt_gate.sequence;

let identity = ();
try {
    identity = freeze_identity(
        bootstrap_worker,
        manifest,
        repo,
        scratch,
        sequence
    );
} catch (error) {
    context.timing = qualification::timing_fail_stage(
        context.timing,
        "build-identity"
    );
    throw error;
}
let build_environment = identity.environment;
sequence = identity.sequence;
let clippy_gate = run_gate(
    context,
    "clippy",
    "Clippy",
    [spec(
        "cargo",
        [
            "clippy", "--quiet", "--locked", "--all-targets",
            "--all-features", "--", "-D", "warnings"
        ],
        60000,
        build_environment
    )],
    repo,
    scratch,
    sequence
);
context = clippy_gate.context;
sequence = clippy_gate.sequence;
let unit_specs = [spec(
    "cargo",
    [
        "test", "--quiet", "--locked", "--all-features", "--",
        "--skip", "preflight_task_is_fail_closed_and_writes_reports_for_real_git_fixtures",
        "--skip", "preflight_benchmark_task_measures_clean_public_worker_runs",
        "--skip", "prd_alignment_task_matches_public_catalogs_and_fails_closed",
        "--skip", "supply_chain_task_is_deterministic_and_covers_the_resolved_lock_graph",
        "--skip", "quality_timing_is_complete_on_success_and_failure_and_renders_markdown",
        "--skip", "rhai_working_context_smoke_is_private_ephemeral_and_orphan_free",
        "--skip", "rhai_server_smoke_preserves_headless_authority_and_cleanup",
        "--skip", "rhai_wake_smoke_preserves_concurrent_ipc_pty_and_expired_mutation",
        "--skip", "rhai_startup_smoke_preserves_first_window_and_async_terminal_contract",
        "--skip", "rhai_cli_smoke_preserves_public_control_ui_bridge_and_pty_contract",
        "--skip", "rhai_fleet_smoke_preserves_discovery_event_launch_and_mux_contract",
        "--skip", "rhai_remote_ui_smoke_preserves_replaceable_client_and_reconnect_contract",
        "--skip", "rhai_script_smoke_preserves_unrestricted_runtime_and_supervisor_contract",
        "--skip", "rhai_theme_smoke_preserves_native_rendering_pty_and_restart_contract",
        "--skip", "rhai_workbench_smoke_preserves_physical_editing_and_compact_tree_contract",
        "--skip", "rhai_diagnostic_bundles_are_bounded_private_and_orphan_free",
        "--skip", "rhai_harness_cleanup_owns_only_registered_children",
        "--skip", "migration_audit_rejects_operational_references_to_deleted_scripts",
        "--skip", "rhai_qualification_contract_fails_closed_and_cleans_owned_scratch",
        "--skip", "rhai_qualified_package_accepts_only_the_exact_receipt_bytes"
    ],
    300000,
    build_environment
)];
unit_specs.push(spec(
    "cargo",
    [
        "test", "--quiet", "--locked", "--all-features",
        "--test", "rhai_migration",
        "quality_timing_is_complete_on_success_and_failure_and_renders_markdown",
        "--", "--nocapture"
    ],
    120000,
    build_environment
));
let unit_gate = run_gate(
    context,
    "unit-tests",
    "unit tests",
    unit_specs,
    repo,
    scratch,
    sequence
);
context = unit_gate.context;
sequence = unit_gate.sequence;

let upgrade_fixture =
    std::path::join(scratch, "prior-agenterm.exe").display;
// The release-fast fixture must be captured before the release build runs:
// a staged release reclaims the development target with `cargo clean`, which
// would otherwise remove target/release-fast/agenterm.exe before the copy.
if !skip_smoke {
    let fixture_gate = run_gate(
        context,
        "artifact-build-fast",
        "alternate GUI fixture build",
        [spec(
            "cmd.exe",
            [
                "/d", "/c",
                std::path::join(repo, "build.bat").display,
                "release-fast"
            ],
            300000,
            build_environment
        )],
        repo,
        scratch,
        sequence
    );
    context = fixture_gate.context;
    sequence = fixture_gate.sequence;
    try {
        std::fs::copy(
            std::path::join(
                repo,
                "target/release-fast/agenterm.exe"
            ).display,
            upgrade_fixture
        );
    } catch (error) {
        context.timing = qualification::timing_fail_stage(
            context.timing,
            "upgrade-fixture"
        );
        throw error;
    }
}
let final_build_arguments = [
    "/d", "/c",
    std::path::join(repo, "build.bat").display
];
if release {
    final_build_arguments.push("release");
}
let build_gate = run_gate(
    context,
    "artifact-build",
    "current artifact",
    [spec(
        "cmd.exe",
        final_build_arguments,
        if release { 600000 } else { 60000 },
        build_environment
    )],
    repo,
    scratch,
    sequence
);
context = build_gate.context;
sequence = build_gate.sequence;
let worker =
    std::path::join(repo, "dist/agenterm-rhai.exe").display;

let ordinary_gates = [
    #{
        id:"prd-alignment",
        label:"PRD capability alignment",
        specs:[task(worker, manifest, "prd-alignment", 10000, [])]
    },
    #{
        id:"rhai-lint",
        label:"production Rhai source lint",
        specs:[task(
            worker, manifest, "lint", 120000, [worker, "rhai"]
        )]
    },
    #{
        id:"task-catalog",
        label:"repository Rhai task catalog",
        specs:[spec(
            worker,
            ["task", "check", "--manifest", manifest],
            30000,
            #{}
        )]
    },
    #{
        id:"mcp-conformance",
        label:"public MCP conformance and isolation",
        specs:[task(
            worker, manifest, "mcp-conformance", 600000, []
        )]
    },
    #{
        id:"migration-audit",
        label:"PowerShell migration ledger and no-new-PS1 gate",
        specs:[task(
            worker, manifest, "migration-audit", 10000, []
        )]
    },
    #{
        id:"artifact-verification",
        label:"binary roles and metadata",
        specs:[task(
            worker,
            manifest,
            "artifact-verification",
            30000,
            [if release { "release" } else { "development" }]
        )]
    },
    #{
        id:"readme-examples",
        label:"manifest-driven README examples",
        specs:[task(worker, manifest, "readme-examples", 10000, [])]
    },
    #{
        id:"docs-site",
        label:"case-exact static docs assets",
        specs:[task(worker, manifest, "verify-docs-site", 10000, [])]
    },
    #{
        id:"supply-chain",
        label:"locked dependency licenses and SPDX inventory",
        specs:[task(
            worker,
            manifest,
            "supply-chain",
            60000,
            [
                repo,
                std::path::join(
                    repo,
                    "dist/agenterm-sbom.spdx.json"
                ).display
            ]
        )]
    },
    #{
        id:"target-report",
        label:"Cargo target inventory",
        specs:[task(worker, manifest, "target-report", 10000, [])]
    },
    #{
        id:"internal-version-policy",
        label:"internal version publication policy",
        specs:[task(
            worker, manifest, "internal-version-policy", 10000, []
        )]
    },
    #{
        id:"qualification-selftest",
        label:"qualification fail-closed self-test",
        specs:[task(
            worker, manifest, "qualification-selftest", 20000, []
        )]
    },
    #{
        id:"package-boundary-selftest",
        label:"qualified package boundary self-test",
        specs:[task(
            worker,
            manifest,
            "package-qualified-selftest",
            60000,
            []
        )]
    },
    #{
        id:"harness-cleanup-selftest",
        label:"owned-resource cleanup self-test",
        specs:[task(
            worker, manifest, "harness-cleanup-selftest", 10000, []
        )]
    }
];
for definition in ordinary_gates {
    let completed = run_gate(
        context,
        definition.id,
        definition.label,
        definition.specs,
        repo,
        scratch,
        sequence
    );
    context = completed.context;
    sequence = completed.sequence;
}

if !skip_smoke {
    let no_activate = #{"AGENTERM_NO_ACTIVATE":"1"};
    let smoke_ids = [
        "startup-smoke", "wake-smoke", "cli-smoke", "server-smoke",
        "native-ipc-smoke", "native-ipc-compat-smoke",
        "remote-ui-smoke", "remote-ui-upgrade-smoke", "fleet-smoke",
        "script-smoke", "theme-smoke", "working-context-smoke",
        "workbench-smoke", "control-center-smoke"
    ];
    for id in smoke_ids {
        let extra = [];
        if id == "remote-ui-upgrade-smoke" {
            extra.push(upgrade_fixture);
        }
        if id == "fleet-smoke" && include_stress {
            extra.push("--event-load");
        }
        let smoke_spec = task(
            worker,
            manifest,
            id,
            if id == "remote-ui-smoke"
                    || id == "cli-smoke"
                    || id == "fleet-smoke"
                    || id == "script-smoke" {
                120000
            } else {
                60000
            },
            extra
        );
        smoke_spec.allow_powershell = id == "remote-ui-smoke";
        smoke_spec.environment = no_activate;
        let completed = run_gate(
            context,
            id,
            id,
            [smoke_spec],
            repo,
            scratch,
            sequence
        );
        context = completed.context;
        sequence = completed.sequence;
    }
}

let diagnostic = task(
    worker,
    manifest,
    "diagnostic-bundle-selftest",
    60000,
    []
);
diagnostic.environment = #{"AGENTERM_NO_ACTIVATE":"1"};
if skip_smoke {
    context.timing = qualification::timing_set_gate(
        context.timing,
        "diagnostic-bundle-selftest",
        "skipped",
        0,
        0,
        false
    );
} else {
    let diagnostic_gate = run_gate(
        context,
        "diagnostic-bundle-selftest",
        "CLI GUI and script diagnostic bundle self-test",
        [diagnostic],
        repo,
        scratch,
        sequence
    );
    context = diagnostic_gate.context;
    sequence = diagnostic_gate.sequence;
}
if skip_smoke {
    print("\nQUALIFICATION RECEIPT NOT WRITTEN: smoke gates were skipped.");
} else if !include_stress {
    print(
        "\nQUALIFICATION RECEIPT NOT WRITTEN: explicit stress gate was not included."
    );
} else {
    try {
        let written =
            qualification::write_receipt(context, repo, receipt_path);
        print("\nQUALIFICATION RECEIPT " + written);
    } catch (error) {
        context.timing = qualification::timing_fail_stage(
            context.timing,
            "qualification-receipt"
        );
        throw error;
    }
}
context.timing = qualification::timing_finish(
    context.timing,
    "passed"
);
print("\nSlowest completed gates:");
let ordered = slowest(context.results);
let count = if ordered.len < 8 { ordered.len } else { 8 };
for index in 0..count {
    print(
        "  " + ordered[index].id + ": " +
            ordered[index].duration_ms + " ms"
    );
}
print(
    "Total quality gate: " +
        (
            std::time::SystemTime::now().unix_millis - full_started
        ) +
        " ms"
);
print("\nPASS: AgenTerm quality gate");
