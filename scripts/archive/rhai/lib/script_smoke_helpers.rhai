// Public-process and binary-protocol helpers for the Script Runtime journey.

import "scripts/rhai/lib/test_harness" as test_harness;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn configured_program(
    context,
    program,
    arguments,
    audit_path,
    environment
) {
    let command = test_harness::configured_command(
        context,
        program,
        arguments
    );
    command.timeout(std::time::Duration::from_secs(60));
    command.capture_limit(262144);
    if audit_path == "" {
        command.env_remove("AGENTERM_SCRIPT_AUDIT_PATH");
        command.env_remove("AGENTERM_AUDIT_ENV_SECRET");
    } else {
        command.env("AGENTERM_SCRIPT_AUDIT_PATH", audit_path);
        command.env("AGENTERM_AUDIT_ENV_SECRET", "AUDIT_ENV_SECRET");
    }
    for name in environment.keys() {
        let value = environment[name];
        if value == () {
            command.env_remove(name);
        } else {
            command.env(name, value.to_string());
        }
    }
    command
}

fn safe_command_arguments(program, arguments) {
    let safe = [program];
    safe.append(arguments);
    let script_index = -1;
    for index in 0..safe.len {
        if safe[index] == "script" {
            script_index = index;
            break;
        }
    }
    let operation_index =
        if script_index >= 0 {
            script_index + 1
        } else if safe.len > 1
                && (
                    safe[1] == "eval"
                        || safe[1] == "run"
                        || safe[1] == "check"
                ) {
            1
        } else {
            -1
        };
    if operation_index >= 0
            && operation_index + 1 < safe.len
            && (
                safe[operation_index] == "eval"
                    || safe[operation_index] == "run"
                    || safe[operation_index] == "check"
            ) {
        safe[operation_index + 1] = "<content>";
        let index = operation_index + 2;
        while index < safe.len {
            if safe[index] == "--" {
                let content_index = index + 1;
                while content_index < safe.len {
                    safe[content_index] = "<content>";
                    content_index += 1;
                }
                break;
            }
            index += 1;
        }
    }
    safe
}

fn run_program(
    context,
    program,
    arguments,
    expected_exit,
    audit_path,
    environment,
    redactions
) {
    let command = configured_program(
        context,
        program,
        arguments,
        audit_path,
        environment
    );
    let output = command.output();
    test_harness::append_command_record(
        context,
        safe_command_arguments(program, arguments),
        output,
        expected_exit != 0,
        redactions
    );
    require(
        output.exit_code == expected_exit,
        "script_smoke_process_exit:" + program + ":" +
            output.exit_code + ":expected:" + expected_exit + ":" +
            test_harness::redact_text(
                output.stdout_text() + output.stderr_text(),
                redactions
            )
    );
    #{
        success: output.success,
        exit_code: output.exit_code,
        stdout: output.stdout_text(),
        stderr: output.stderr_text(),
        bytes: output.stdout,
        complete: output.complete,
        truncated: output.truncated
    }
}

fn run_cli(
    context,
    cli,
    arguments,
    expected_exit,
    audit_path,
    environment,
    redactions
) {
    let command_arguments = ["--address", context.address];
    command_arguments.append(arguments);
    run_program(
        context,
        cli,
        command_arguments,
        expected_exit,
        audit_path,
        environment,
        redactions
    )
}

fn cli_text(context, cli, arguments, audit_path) {
    let text = run_cli(
        context,
        cli,
        arguments,
        0,
        audit_path,
        #{},
        []
    ).stdout;
    text.trim();
    text
}

fn cli_json(context, cli, arguments, audit_path) {
    rhai::json::parse(
        run_cli(
            context,
            cli,
            arguments,
            0,
            audit_path,
            #{},
            []
        ).stdout
    )
}

fn direct_text(context, script, arguments, audit_path) {
    let text = run_program(
        context,
        script,
        arguments,
        0,
        audit_path,
        #{},
        []
    ).stdout;
    text.trim();
    text
}

fn direct_json(context, script, arguments, audit_path) {
    rhai::json::parse(
        run_program(
            context,
            script,
            arguments,
            0,
            audit_path,
            #{},
            []
        ).stdout
    )
}

fn failure_text(
    context,
    cli,
    arguments,
    expected_exit,
    audit_path
) {
    let result = run_cli(
        context,
        cli,
        arguments,
        expected_exit,
        audit_path,
        #{},
        []
    );
    result.stdout + result.stderr
}

fn process_ids(executable_name) {
    let ids = [];
    for process in std::process::list() {
        if process.executable_name.to_lower() == executable_name.to_lower() {
            ids.push(process.id);
        }
    }
    ids
}

fn process_exists(pid) {
    for process in std::process::list() {
        if process.id == pid {
            return true;
        }
    }
    false
}

fn wait_new_process(executable_name, existing, timeout_ms) {
    let attempts = timeout_ms / 5;
    for attempt in 0..attempts {
        for pid in process_ids(executable_name) {
            if !existing.contains(pid) {
                return pid;
            }
        }
        rhai::task::sleep(std::time::Duration::from_millis(5));
    }
    throw "script_smoke_new_process_timeout:" + executable_name;
}

fn wait_process_gone(pid, timeout_ms) {
    let attempts = timeout_ms / 10;
    for attempt in 0..attempts {
        if !process_exists(pid) {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    throw "script_smoke_process_remained:" + pid;
}

fn start_script_client(
    context,
    cli,
    arguments,
    audit_path
) {
    let command_arguments = ["--address", context.address];
    command_arguments.append(arguments);
    configured_program(
        context,
        cli,
        command_arguments,
        audit_path,
        #{}
    ).start()
}

fn start_http_fixture(
    context,
    script,
    fixture_path,
    repo,
    ready_path,
    log_path,
    stop_path
) {
    configured_program(
        context,
        script,
        [
            "run",
            fixture_path,
            "--profile",
            "local",
            "--project-root",
            repo,
            "--timeout-ms",
            "20000",
            "--max-operations",
            "10000000",
            "--",
            ready_path,
            log_path,
            stop_path,
            "9",
            "10000"
        ],
        "",
        #{}
    ).start()
}

fn wait_http_ready(child, ready_path, timeout_ms) {
    let attempts = timeout_ms / 5;
    for attempt in 0..attempts {
        if std::fs::exists(ready_path) {
            return rhai::json::parse_file(ready_path);
        }
        require(
            child.state != "exited",
            "script_smoke_http_fixture_early_exit"
        );
        rhai::task::sleep(std::time::Duration::from_millis(5));
    }
    throw "script_smoke_http_fixture_ready_timeout";
}

fn u32_header(length) {
    rhai::bytes::from_array([
        (length / 16777216) % 256,
        (length / 65536) % 256,
        (length / 256) % 256,
        length % 256
    ])
}

fn payload_frame(payload) {
    let stream = u32_header(payload.len);
    stream.append(payload);
    stream
}

fn json_frame(frame) {
    payload_frame(
        rhai::bytes::from_text(rhai::json::stringify(frame))
    )
}

fn framed_invocation(frame_id, invocation_id, source, version) {
    #{
        frame_version: version,
        frame_id: frame_id,
        kind: "invoke",
        payload: #{
            envelope_version: 2,
            invocation_id: invocation_id,
            api_version: 2,
            operation: "eval",
            profile: "pure",
            source_label: "smoke",
            source: source,
            arguments: [],
            budgets: #{
                source_bytes: 262144,
                operations: 1000000,
                call_depth: 64,
                expression_depth: 64,
                collection_items: 10000,
                string_bytes: 262144,
                output_bytes: 65536,
                wall_time_ms: 2000
            }
        }
    }
}

fn parse_frames(bytes) {
    let frames = [];
    let offset = 0;
    while offset < bytes.len {
        require(
            bytes.len - offset >= 4,
            "script_smoke_truncated_frame_length"
        );
        let length =
            bytes.get(offset) * 16777216 +
            bytes.get(offset + 1) * 65536 +
            bytes.get(offset + 2) * 256 +
            bytes.get(offset + 3);
        offset += 4;
        require(
            length <= 2097152 && bytes.len - offset >= length,
            "script_smoke_invalid_frame_payload:" + length
        );
        frames.push(
            rhai::json::parse(bytes.slice(offset, length).to_text())
        );
        offset += length;
    }
    frames
}

fn invoke_framed_worker(
    context,
    script,
    input_bytes
) {
    let command = configured_program(
        context,
        script,
        ["--framed-worker"],
        "",
        #{}
    );
    command.stdin_bytes(input_bytes);
    command.capture_limit(262144);
    let output = command.output();
    let worker_label =
        "[" + std::path::PathBuf::from(script).file_name + "]";
    test_harness::append_command_record(
        context,
        [worker_label, "--framed-worker"],
        output,
        false,
        []
    );
    require(
        output.success,
        "script_smoke_framed_worker_failed:" + output.stderr_text()
    );
    parse_frames(output.stdout)
}
