// Public black-box Script Runtime regression and self-hosting journey.
//
// Args:
//   --list-evidence
// or:
//   REPO CLI_EXE SCRIPT_EXE GUI_EXE [--internal-failure-bundle-probe]

import "scripts/rhai/lib/test_harness" as test_harness;
import "scripts/rhai/lib/script_smoke_helpers" as helper;

let declared_evidence = [
    "script.rhai-runtime",
    "script.rhai-fleet",
    "script.api-tree",
    "script.fleet-v2",
    "script.fleet-tabs-set-note",
    "script.direct-entry",
    "script.north-star",
    "script.rhai-robustness-budget",
    "script.rhai-framed",
    "script.exit-classes",
    "script.typed-errors",
    "script.modules-tasks",
    "script.stream",
    "script.http",
    "script.fs-lifecycle",
    "script.runtime-lifecycle",
    "script.supervisor",
    "script.audit"
];

if args.len == 1 && args[0] == "--list-evidence" {
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

fn matching(entries, field, value) {
    let result = [];
    for entry in entries {
        if entry[field] == value {
            result.push(entry);
        }
    }
    result
}

fn emit(context, declared, id) {
    test_harness::emit_evidence(context, declared, id);
}

fn json_literal(value) {
    rhai::json::stringify(value)
}

fn trimmed(value) {
    let text = value.to_string();
    text.trim();
    text
}

fn script_eval(context, cli, source, audit_path) {
    helper::cli_text(
        context,
        cli,
        ["script", "eval", source, "--profile", "local"],
        audit_path
    )
}

fn script_eval_json(context, cli, source, audit_path) {
    helper::cli_json(
        context,
        cli,
        [
            "script", "eval", source,
            "--profile", "local", "--json"
        ],
        audit_path
    )
}

fn assert_api_contract(context, cli, script, runtime_dir, audit_path) {
    print("STEP offline scripting API discovery");
    let api = helper::cli_json(
        context,
        cli,
        ["script", "api", "--json"],
        audit_path
    );
    require(api.ok, "script_api_envelope_failed");
    let catalog = api.value;
    require(
        catalog.api_version == 2
            && catalog.schema_version == 3
            && catalog.default_profile == "local"
            && catalog.execution_model == "unrestricted_local",
        "script_api_identity_mismatch"
    );
    require(
        catalog.profiles.pure.ambient_authority
            .contains("ordinary_local_program")
            && catalog.profiles.observe.ambient_authority
                .contains("ordinary_local_program")
            && catalog.profiles.local.status == "shipped",
        "script_api_profiles_are_not_unrestricted"
    );
    require(
        catalog.limits.defaults.wall_time_ms == 2000
            && catalog.limits.hard_maximums.wall_time_ms == 3600000
            && catalog.limits.invocation_bytes == 2097152
            && catalog.framing.version == 1
            && catalog.framing.max_frame_bytes == 2097152
            && catalog.supervisor.job_object == "kill_on_close"
            && catalog.supervisor.global_concurrency == 8,
        "script_api_runtime_contract_mismatch"
    );
    require(
        catalog.exit_classes.limit == 3
            && catalog.exit_classes.child == 4
            && catalog.exit_classes.cancelled == 5
            && catalog.exit_classes.fleet == 6
            && catalog.typed_error.fields.len == 8
            && catalog.typed_error.catchable_slices
                .contains("std.process.Output.require_success"),
        "script_api_failure_contract_mismatch"
    );
    require(
        matching(
            catalog.entries,
            "stable_id",
            "rhai.bytes.from-array"
        ).len == 1
            && matching(
                catalog.entries,
                "stable_id",
                "std.process.command-builder"
            )[0].surface_path.contains("stdin_bytes")
            && matching(
                catalog.entries,
                "stable_id",
                "fleet.tabs.new"
            )[0].status == "planned",
        "script_api_required_entries_missing"
    );
    let operation_ids = #{};
    let operation_entries = 0;
    for entry in catalog.entries {
        if entry.operation_id != () {
            operation_entries += 1;
            operation_ids[entry.operation_id] = true;
            require(
                entry.surface_path == entry.operation.script_surface
                    && entry.operation.id == entry.operation_id,
                "script_api_operation_mapping_mismatch"
            );
        }
        require(
            entry.comparisons.nodejs.reviewed_on == "2026-07-29"
                && entry.comparisons.bun.reviewed_on == "2026-07-29"
                && entry.comparisons.nodejs.semantic_note != ""
                && entry.comparisons.bun.semantic_note != "",
            "script_api_comparison_unreviewed"
        );
    }
    require(
        operation_entries == 18 && operation_ids.keys().len == 18,
        "script_api_operation_mapping_not_bijective"
    );

    // A downstream consumer closing stdout early must remain a normal exit.
    let pipe_command = helper::configured_program(
        context,
        script,
        ["api", "--json"],
        "",
        #{}
    );
    let pipe_child = pipe_command.start();
    let prefix = pipe_child.stdout.read(
        4096,
        std::time::Duration::from_secs(2)
    ).to_text();
    pipe_child.stdout.close();
    let pipe_output = pipe_child.wait_with_output(
        std::time::Duration::from_secs(5)
    );
    require(
        pipe_output.success && prefix != "",
        "script_api_closed_pipe_failed"
    );

    print("STEP filtered scripting API object tree");
    let tree = helper::direct_text(
        context,
        script,
        ["api", "std::fs", "--status", "shipped", "--tree"],
        audit_path
    );
    require(
        tree.contains("AgenTerm Script API v2")
            && tree.contains("module=std.fs")
            && tree.contains("read-to-string  [shipped]")
            && tree.contains("std::fs::read_to_string")
            && tree.contains("Node.js~node:fs")
            && !tree.contains("[planned]"),
        "script_api_filtered_tree_mismatch"
    );
    let planned = helper::cli_json(
        context,
        cli,
        [
            "script", "api", "fleet",
            "--status", "planned", "--json"
        ],
        audit_path
    );
    require(
        planned.ok
            && planned.value.view["module"] == "fleet"
            && planned.value.view.status == "planned"
            && planned.value.entries.len > 0,
        "script_api_planned_fleet_view_mismatch"
    );
    for entry in planned.value.entries {
        require(
            entry.status == "planned"
                && entry.stable_id.starts_with("fleet."),
            "script_api_planned_view_leaked_other_status"
        );
    }
    let invalid = helper::failure_text(
        context,
        cli,
        ["script", "api", "--status", "experimental"],
        2,
        audit_path
    );
    require(
        invalid.contains("script_api_status_invalid"),
        "script_api_invalid_status_not_typed"
    );

    print("STEP Rhai repository dogfood contract check");
    let catalog_path = std::path::join(
        runtime_dir,
        "script-catalog.json"
    ).display;
    std::fs::write(
        catalog_path,
        rhai::json::stringify_pretty(catalog)
    );
    let spec = std::path::join(
        context.owned_root,
        "../../../docs/agenterm-script-runtime.md"
    ).display;
    // Resolve from the repository supplied to the context rather than trusting
    // the current directory of a nested child.
    let repo = std::path::absolute(".").display;
    let verified = helper::cli_text(
        context,
        cli,
        [
            "script", "run",
            std::path::join(
                repo,
                "scripts/rhai/verify-script-contract.rhai"
            ).display,
            "--profile", "local", "--",
            std::path::join(
                repo,
                "docs/agenterm-script-runtime.md"
            ).display,
            catalog_path
        ],
        audit_path
    );
    require(
        verified.starts_with("PASS: Rhai verified the Script API contract"),
        "script_api_dogfood_contract_failed:" + spec
    );
    catalog
}

fn assert_process_and_streams(
    context,
    cli,
    audit_path,
    runtime_dir
) {
    print("STEP local environment, process, duration, and child lifecycle");
    let process_source = `
let command = std::process::command("cmd.exe");
command.args([
    "/d", "/v:on", "/s", "/c",
    "set /p LINE=&echo out:!LINE!:%AGENTERM_PROCESS_TEST%&echo cwd:%CD% 1>&2&exit /b 7"
]);
command.current_dir(args[0]);
command.env("AGENTERM_PROCESS_TEST", "argv-safe");
command.stdin_text("hello\n");
command.timeout(std::time::Duration::from_secs(2));
let output = command.output();
#{
    success: output.success,
    exit_code: output.exit_code,
    stdout: output.stdout_text(),
    stderr: output.stderr_text(),
    complete: output.complete,
    truncated: output.truncated,
    cwd: std::env::current_dir().display,
    has_path: std::env::has("PATH")
}`;
    let process_file = std::path::join(
        runtime_dir,
        "process.rhai"
    ).display;
    std::fs::write(process_file, process_source);
    let process_result = rhai::json::parse(
        helper::cli_text(
            context,
            cli,
            [
                "script", "run", process_file,
                "--profile", "local",
                "--timeout-ms", "10000",
                "--max-operations", "1000000",
                "--", runtime_dir
            ],
            audit_path
        )
    );
    require(
        !process_result.success
            && process_result.exit_code == 7
            && trimmed(process_result.stdout) == "out:hello:argv-safe"
            && process_result.stderr.contains("cwd:")
            && process_result.stderr.contains("script-runtime")
            && process_result.complete
            && !process_result.truncated
            && process_result.has_path,
        "script_process_context_mismatch"
    );
    let timeout = helper::cli_text(
        context,
        cli,
        [
            "script", "eval",
            `let c=std::process::command("cmd.exe");
             c.args(["/d","/s","/c","ping -n 6 127.0.0.1 >nul"]);
             c.timeout(std::time::Duration::from_millis(10));
             try { c.output(); "missing" } catch(e) { print(e); }`,
            "--profile", "local", "--timeout-ms", "10000"
        ],
        audit_path
    );
    require(timeout.contains("process_timeout"), "script_process_timeout_untyped");
    require(
        script_eval(context, cli, "6 * 7", audit_path) == "42",
        "script_process_timeout_recovery_failed"
    );
    let spawn_failure = helper::failure_text(
        context,
        cli,
        [
            "script", "eval",
            `std::process::command("agenterm-definitely-missing.exe").output()`,
            "--profile", "local"
        ],
        4,
        audit_path
    );
    require(
        spawn_failure.contains(`"code":"process_spawn"`)
            && spawn_failure.contains(`"exit_class":"child"`),
        "script_process_spawn_failure_untyped"
    );
    let nonzero_source = `
let command = std::process::command("cmd.exe");
command.args(["/d", "/s", "/c", "exit /b 7"]);
command.output().require_success("test-child");`;
    let nonzero = helper::failure_text(
        context,
        cli,
        [
            "script", "eval", nonzero_source,
            "--profile", "local"
        ],
        4,
        audit_path
    );
    require(
        nonzero.contains(`"code":"child_nonzero"`)
            && nonzero.contains("test-child"),
        "script_process_nonzero_failure_untyped"
    );
    let caught = script_eval_json(
        context,
        cli,
        `
let caught = ();
try {
    let command = std::process::command("cmd.exe");
    command.args(["/d", "/s", "/c", "exit /b 7"]);
    command.output().require_success("test-child");
} catch (error) { caught = error; }
caught`,
        audit_path
    );
    require(
        caught.ok
            && caught.value.class == "child"
            && caught.value.code == "child_nonzero"
            && caught.value.operation ==
                "std.process.Output.require_success"
            && caught.value.target_kind == "child_process"
            && caught.value.cause_class == "exit_status",
        "script_typed_child_error_mismatch"
    );
    let child = rhai::json::parse(
        script_eval(
            context,
            cli,
            `
let command = std::process::command("cmd.exe");
command.args(["/d","/s","/c","ping -n 6 127.0.0.1 >nul"]);
let child = command.start();
let pid = child.id;
child.kill();
let output = child.wait_with_output();
#{pid: pid, state: child.state, complete: output.complete}`,
            audit_path
        )
    );
    require(
        child.pid > 0 && child.state == "exited" && child.complete,
        "script_child_lifecycle_mismatch"
    );

    print("STEP bounded child streams, backpressure, and truthful truncation");
    let expected = trimmed(helper::run_program(
        context,
        cli,
        ["--version"],
        0,
        "",
        #{},
        []
    ).stdout);
    let cli_literal = json_literal(cli);
    let stream = rhai::json::parse(
        script_eval(
            context,
            cli,
            `
let command=std::process::command(${cli_literal});
command.arg("--version");
let child=command.start();
let stream=child.stdout;
let first=stream.read(2,std::time::Duration::from_secs(1)).to_text();
let rest=stream.collect(64,std::time::Duration::from_secs(1)).to_text();
let output=child.wait_with_output();
let empty=stream.read(1);
#{
 id:stream.id,kind:stream.kind,state:stream.state,
 buffered:stream.buffered_bytes,truncated:stream.truncated,
 complete:stream.complete,first:first,rest:rest,empty:empty.len,
 final:output.stdout_text(),output_complete:output.complete
}`,
            audit_path
        )
    );
    require(
        stream.id > 0
            && stream.kind == "bytes"
            && stream.state == "closed"
            && stream.buffered == 0
            && !stream.truncated
            && stream.complete
            && stream.first == "ag"
            && trimmed(stream.rest) == expected.sub_string(2)
            && stream.empty == 0
            && trimmed(stream.final) == expected
            && stream.output_complete,
        "script_stream_live_final_mismatch"
    );
    let truncated = rhai::json::parse(
        script_eval(
            context,
            cli,
            `
let command=std::process::command(${cli_literal});
command.arg("--version"); command.capture_limit(4);
let child=command.start(); let stream=child.stdout;
let delivered=stream.collect(64).to_text();
let output=child.wait_with_output();
#{
 stdout:output.stdout_text(),truncated:output.truncated,
 complete:output.complete,delivered:delivered,
 stream_truncated:stream.truncated,stream_complete:stream.complete
}`,
            audit_path
        )
    );
    require(
        truncated.stdout == "agen"
            && truncated.truncated
            && !truncated.complete
            && trimmed(truncated.delivered) == expected
            && !truncated.stream_truncated
            && truncated.stream_complete,
        "script_stream_capture_conflation"
    );
    let cancelled = rhai::json::parse(
        script_eval(
            context,
            cli,
            `
let command=std::process::command("cmd.exe");
command.args(["/d","/s","/c","ping -n 6 127.0.0.1 >nul"]);
let child=command.start(); let stream=child.stdout; let timed_out=false;
try { stream.read(1,std::time::Duration::from_millis(10)); }
catch(error) { timed_out=error.contains("stream_read_timeout"); }
let closed=stream.close(); child.kill(); let output=child.wait_with_output();
#{
 timed_out:timed_out,closed:closed,state:stream.state,
 truncated:stream.truncated,complete:stream.complete,
 output_truncated:output.truncated,output_complete:output.complete
}`,
            audit_path
        )
    );
    require(
        cancelled.timed_out
            && cancelled.closed
            && cancelled.state == "cancelled"
            && cancelled.truncated
            && !cancelled.complete
            && cancelled.output_truncated
            && !cancelled.output_complete,
        "script_stream_cancel_mismatch"
    );
}

fn assert_tasks_and_http(
    context,
    repo,
    cli,
    script,
    audit_path,
    runtime_dir
) {
    print("STEP task timers, deterministic race, cancellation, and wait timeout");
    let task_result = rhai::json::parse(
        script_eval(
            context,
            cli,
            `
let slow=rhai::task::after(std::time::Duration::from_millis(100));
let fast=rhai::task::after(std::time::Duration::from_millis(1));
let winner=rhai::task::race([slow,fast],std::time::Duration::from_secs(1));
fast.wait(); let changed=slow.cancel();
#{
 winner:winner,fast_state:fast.state,fast_done:fast.done,
 slow_state:slow.state,slow_cancelled:slow.cancelled,
 cancel_changed_state:changed
}`,
            audit_path
        )
    );
    require(
        task_result.winner == 1
            && task_result.fast_state == "completed"
            && task_result.fast_done
            && task_result.slow_state == "cancelled"
            && task_result.slow_cancelled
            && task_result.cancel_changed_state,
        "script_task_race_mismatch"
    );
    let task_timeout = helper::cli_text(
        context,
        cli,
        [
            "script", "eval",
            `let task=rhai::task::after(std::time::Duration::from_secs(1));
             try { task.wait(std::time::Duration::from_millis(1)); }
             catch(error) { print(error); } task.cancel();`,
            "--profile", "local", "--timeout-ms", "10000"
        ],
        audit_path
    );
    require(
        task_timeout.contains("task_wait_timeout"),
        "script_task_timeout_untyped"
    );

    print("STEP bounded loopback HTTP, async task, cancellation, and privacy");
    let fixture_path = std::path::join(
        repo,
        "scripts/rhai/script-http-fixture.rhai"
    ).display;
    let ready_path = std::path::join(
        runtime_dir,
        "http-ready.json"
    ).display;
    let log_path = std::path::join(
        runtime_dir,
        "http-requests.jsonl"
    ).display;
    let stop_path = std::path::join(runtime_dir, "http-stop").display;
    let fixture = helper::start_http_fixture(
        context,
        script,
        fixture_path,
        repo,
        ready_path,
        log_path,
        stop_path
    );
    let http_error = "";
    try {
        let ready = helper::wait_http_ready(fixture, ready_path, 3000);
        require(
            ready.schema_version == 1
                && ready.pid > 0
                && ready.url.starts_with("http://127.0.0.1:")
                && ready.tls_url.starts_with("https://127.0.0.1:"),
            "script_http_ready_mismatch"
        );
        let http_file = std::path::join(
            runtime_dir,
            "http.rhai"
        ).display;
        std::fs::write(
            http_file,
            `
let base=args[0]; let tls_base=args[1];
let options=#{proxy:false,timeout:std::time::Duration::from_secs(1)};
let protocol_options=#{
 proxy:false,timeout:std::time::Duration::from_millis(500)
};
let status=rhai::http::request("GET",base+"/status",options);
let values=status.header("x-test"); let body=status.body;
let status_text=body.collect(16).to_text();
let echo=rhai::http::request("POST",base+"/echo",#{
 proxy:false,timeout:std::time::Duration::from_secs(2),
 headers:#{"content-type":"application/octet-stream"},
 body:rhai::bytes::from_text("payload")
});
let echo_text=echo.body.collect(16).to_text();
let large=rhai::http::request("GET",base+"/large",#{
 proxy:false,timeout:std::time::Duration::from_secs(2),max_body_bytes:4
});
let large_body=large.body; let large_text=large_body.collect(16).to_text();
let async_task=rhai::http::start("GET",base+"/async",options);
let async_kind=async_task.kind;
let async_response=async_task.wait(std::time::Duration::from_secs(2));
let async_text=async_response.body.collect(16).to_text();
let cancel_task=rhai::http::start("GET",base+"/cancel",options);
rhai::task::sleep(std::time::Duration::from_millis(50));
let cancel_changed=cancel_task.cancel(); let cancel_error="";
try { cancel_task.wait(std::time::Duration::from_secs(1)); }
catch(error) { cancel_error=error; }
rhai::task::sleep(std::time::Duration::from_millis(600));
let timeout_error="";
try { rhai::http::request("GET",base+"/slow",#{
 proxy:false,timeout:std::time::Duration::from_millis(50)
}); } catch(error) { timeout_error=error; }
let malformed_error="";
try { rhai::http::request("GET",base+"/malformed",protocol_options); }
catch(error) { malformed_error=error; }
let disconnect_error="";
try { rhai::http::request("GET",base+"/disconnect",protocol_options); }
catch(error) { disconnect_error=error; }
let tls_error="";
try { rhai::http::request("GET",tls_base+"/tls",#{
 proxy:false,timeout:std::time::Duration::from_millis(250)
}); } catch(error) { tls_error=error; }
let proxy_error="";
try {
 rhai::http::request(
  "GET","http://HTTP_CREDENTIAL_SECRET.invalid/PRIVATE_PATH_SECRET",
  #{proxy:"http://user:PROXY_CREDENTIAL_SECRET@127.0.0.1:1",
    timeout:std::time::Duration::from_millis(50)}
 );
} catch(error) { proxy_error=error; }
#{
 status:status.status,version:status.version,
 first_header:values[0].to_text(),second_header:values[1].to_text(),
 status_text:status_text,status_kind:body.kind,status_complete:body.complete,
 echo_text:echo_text,large_text:large_text,
 large_truncated:large_body.truncated,large_complete:large_body.complete,
 async_kind:async_kind,async_text:async_text,async_state:async_task.state,
 cancel_changed:cancel_changed,cancel_error:cancel_error,
 cancel_state:cancel_task.state,timeout_error:timeout_error,
 malformed_error:malformed_error,disconnect_error:disconnect_error,
 tls_error:tls_error,proxy_error:proxy_error
}`
        );
        let result = rhai::json::parse(
            helper::cli_text(
                context,
                cli,
                [
                    "script", "run", http_file,
                    "--profile", "local",
                    "--timeout-ms", "10000",
                    "--max-operations", "1000000",
                    "--", ready.url, ready.tls_url
                ],
                audit_path
            )
        );
        require(
            result.status == 201
                && result.version == "HTTP/1.1"
                && result.first_header == "one"
                && result.second_header == "two"
                && result.status_text == "hello"
                && result.status_kind == "bytes"
                && result.status_complete
                && result.echo_text == "payload"
                && result.large_text == "abcd"
                && result.large_truncated
                && !result.large_complete
                && result.async_kind == "http"
                && result.async_text == "async-ok"
                && result.async_state == "completed"
                && result.cancel_changed
                && result.cancel_error.contains("task_cancelled")
                && result.cancel_state == "cancelled"
                && result.timeout_error.contains("http_timeout")
                && result.malformed_error.contains("http_")
                && result.disconnect_error.contains("http_transport")
                && result.tls_error.contains("http_tls")
                && result.proxy_error.contains("http_"),
            "script_http_result_mismatch"
        );
        let safe_result = rhai::json::stringify(result);
        for secret in [
            "HTTP_CREDENTIAL_SECRET",
            "PRIVATE_PATH_SECRET",
            "PROXY_CREDENTIAL_SECRET"
        ] {
            require(
                !safe_result.contains(secret),
                "script_http_diagnostic_secret:" + secret
            );
        }
        std::fs::write(stop_path, "stop");
        let fixture_output = fixture.wait_with_output(
            std::time::Duration::from_secs(3)
        );
        require(
            fixture_output.success,
            "script_http_fixture_exit:" + fixture_output.stderr_text()
        );
        test_harness::append_command_record(
            context,
            ["[script-http-fixture]"],
            fixture_output,
            false,
            []
        );
        let requests = [];
        for line in std::fs::read_to_string(log_path).split("\n") {
            if trimmed(line) != "" {
                requests.push(rhai::json::parse(line));
            }
        }
        let echo_count = 0;
        let tls_count = 0;
        let cancel_count = 0;
        for request in requests {
            if request.path == "/echo" {
                echo_count += 1;
                require(
                    request.method == "POST"
                        && request.body_bytes == 7,
                    "script_http_echo_observation_mismatch"
                );
            }
            if request.tls == true {
                tls_count += 1;
            }
            if request.path == "/cancel" {
                cancel_count += 1;
            }
        }
        require(
            (requests.len == 8 || requests.len == 9)
                && echo_count == 1
                && tls_count == 1
                && cancel_count <= 1,
            "script_http_request_matrix_mismatch"
        );
    } catch (error) {
        http_error = error.to_string();
    }
    if fixture.state == "running" {
        fixture.kill();
        fixture.wait_with_output(std::time::Duration::from_secs(2));
    }
    require(http_error == "", http_error);
    require(
        script_eval(context, cli, "6 * 7", audit_path) == "42",
        "script_http_recovery_failed"
    );

    print("STEP project modules and versioned named-task discovery");
    let project = std::path::join(
        repo,
        "tests/fixtures/script-project"
    ).display;
    let manifest = std::path::join(
        project,
        "agenterm.tasks.json"
    ).display;
    let catalog = helper::cli_json(
        context,
        cli,
        [
            "script", "task", "list",
            "--manifest", manifest, "--json"
        ],
        audit_path
    );
    let ready = matching(catalog.tasks, "id", "daily-check");
    let missing = matching(catalog.tasks, "id", "missing-entry");
    let unknown = matching(catalog.tasks, "id", "unknown-field");
    require(
        catalog.schema_version == 2
            && catalog.script_api_version == 2
            && catalog.script_catalog_schema_version == 3
            && catalog.project_id == "script-smoke"
            && catalog.origin.id == "agenterm"
            && catalog.compatible
            && ready.len == 1
            && ready[0].status == "ready"
            && missing.len == 1
            && missing[0].status == "degraded"
            && unknown.len == 1
            && unknown[0].status == "degraded",
        "script_task_catalog_mismatch"
    );
    let shown = helper::cli_json(
        context,
        cli,
        [
            "script", "task", "show", "daily-check",
            "--manifest", manifest, "--json"
        ],
        audit_path
    );
    require(
        shown.project_version == "1.0.0"
            && shown.tasks.len == 1
            && shown.tasks[0].entry == "main.rhai",
        "script_task_show_mismatch"
    );
    require(
        helper::cli_text(
            context,
            cli,
            [
                "script", "task", "check", "daily-check",
                "--manifest", manifest
            ],
            audit_path
        ) == "OK",
        "script_task_check_failed"
    );
    let bad_version = helper::failure_text(
        context,
        cli,
        [
            "script", "task", "list", "--manifest",
            std::path::join(
                project,
                "bad-version.tasks.json"
            ).display
        ],
        2,
        audit_path
    );
    require(
        bad_version.contains("task_manifest_version"),
        "script_task_bad_version_untyped"
    );
    let incompatible = std::path::join(
        project,
        "incompatible.tasks.json"
    ).display;
    let incompatible_list = helper::cli_json(
        context,
        cli,
        [
            "script", "task", "list",
            "--manifest", incompatible, "--json"
        ],
        audit_path
    );
    require(
        !incompatible_list.compatible
            && incompatible_list.compatibility_reason ==
                "capability_unknown: future.capability"
            && incompatible_list.tasks[0].status == "ready",
        "script_task_incompatible_not_inspectable"
    );
    for operation in ["check", "run"] {
        let rejected = helper::failure_text(
            context,
            cli,
            [
                "script", "task", operation, "must-not-run",
                "--manifest", incompatible
            ],
            2,
            audit_path
        );
        require(
            rejected.contains(
                "task_project_incompatible: capability_unknown: future.capability"
            ),
            "script_task_incompatible_executed"
        );
    }
    let named = rhai::json::parse(
        helper::run_cli(
            context,
            cli,
            [
                "script", "task", "run", "daily-check",
                "--manifest", manifest, "--json", "--", "cli-extra"
            ],
            0,
            audit_path,
            #{"AGENTERM_TASK_FIXTURE":"fixture-ok"},
            []
        ).stdout
    );
    require(
        named.ok
            && named.value.answer == 42
            && named.value.environment == "fixture-ok"
            && named.value.args == ["manifest-default", "cli-extra"],
        "script_named_task_context_mismatch"
    );
    let missing_environment = helper::run_cli(
        context,
        cli,
        [
            "script", "task", "run", "daily-check",
            "--manifest", manifest
        ],
        2,
        audit_path,
        #{"AGENTERM_TASK_FIXTURE": ()},
        []
    );
    require(
        (
            missing_environment.stdout +
            missing_environment.stderr
        ).contains("task_environment_missing"),
        "script_named_task_missing_environment_untyped"
    );
    let module_ok = helper::cli_text(
        context,
        cli,
        [
            "script", "check",
            std::path::join(project, "main.rhai").display,
            "--profile", "local", "--project-root", project
        ],
        audit_path
    );
    require(module_ok == "OK", "script_module_check_failed");
    for fixture in [
        ["missing-module.rhai", "script_module_missing"],
        ["escape-module.rhai", "script_module_root_escape"],
        ["cycle-module.rhai", "script_module_cycle"],
        ["bad-module-api.rhai", "script_api_unknown"]
    ] {
        let failure = helper::failure_text(
            context,
            cli,
            [
                "script", "check",
                std::path::join(project, fixture[0]).display,
                "--profile", "local", "--project-root", project
            ],
            1,
            audit_path
        );
        require(
            failure.contains(fixture[1]),
            "script_module_failure_untyped:" + fixture[1]
        );
    }
}

fn assert_runtime_and_framing(
    context,
    repo,
    cli,
    script,
    audit_path,
    runtime_dir,
    source_file,
    internal_failure_probe
) {
    print("STEP Rhai Cargo target inventory migration");
    let target_root = std::path::join(runtime_dir, "target").display;
    let target_debug = std::path::join(target_root, "debug").display;
    let target_deps = std::path::join(target_debug, "deps").display;
    std::fs::create_dir_all(target_deps);
    std::fs::write(std::path::join(target_root, "root.bin").display, "x");
    std::fs::write(std::path::join(target_debug, "debug.bin").display, "abc");
    std::fs::write(std::path::join(target_deps, "dependency.bin").display, "12345");
    let target_envelope = helper::cli_json(
        context,
        cli,
        [
            "script", "run",
            std::path::join(
                repo,
                "scripts/rhai/target-report.rhai"
            ).display,
            "--profile", "local",
            "--timeout-ms", "10000",
            "--max-operations", "10000000",
            "--json", "--", runtime_dir, "target", "--json"
        ],
        audit_path
    );
    let target = rhai::json::parse(target_envelope.stdout);
    let root_profile = matching(target.profiles, "name", "(root)");
    let debug_profile = matching(target.profiles, "name", "debug");
    require(
        target_envelope.ok
            && target.schema_version == 1
            && target.exists
            && target.repo_local
            && target.cleanup_allowed
            && target.files == 3
            && target.bytes == 9
            && root_profile.len == 1
            && root_profile[0].files == 1
            && debug_profile.len == 1
            && debug_profile[0].files == 2,
        "script_target_report_mismatch"
    );

    print("STEP pure eval, file run, arguments, and check");
    require(
        helper::cli_text(
            context,
            cli,
            ["script", "eval", "40 + 2"],
            audit_path
        ) == "42",
        "script_pure_eval_mismatch"
    );
    if internal_failure_probe {
        throw "INTERNAL_FAILURE_BUNDLE_PROBE:script:" + context.run_id;
    }
    std::fs::write(
        source_file,
        `print("hello"); args[0] + args[1]`
    );
    require(
        helper::cli_text(
            context,
            cli,
            [
                "script", "run", source_file,
                "--", "safe", "-script"
            ],
            audit_path
        ) == "hello\nsafe-script",
        "script_file_run_mismatch"
    );
    require(
        helper::cli_text(
            context,
            cli,
            ["script", "check", source_file],
            audit_path
        ) == "OK",
        "script_file_check_failed"
    );
    require(
        helper::cli_text(
            context,
            cli,
            [
                "script", "eval", "args[0] + args[1]",
                "--profile", "local", "--", "local-", "profile"
            ],
            audit_path
        ) == "local-profile",
        "script_local_argv_mismatch"
    );
    let data_file = std::path::join(
        runtime_dir,
        "local-data.json"
    ).display;
    let data_literal = json_literal(data_file);
    let data_text_literal = json_literal(`{"answer":42}`);
    require(
        script_eval(
            context,
            cli,
            "std::fs::write(" + data_literal + ", " +
                data_text_literal + "); " +
                "rhai::json::parse(std::fs::read_to_string(" +
                data_literal + ")).answer",
            audit_path
        ) == "42",
        "script_local_fs_json_mismatch"
    );
    let fs_tree = std::path::join(
        runtime_dir,
        "fs-lifecycle"
    ).display;
    let fs_nested = std::path::join(fs_tree, "nested").display;
    let fs_source = std::path::join(fs_nested, "source.txt").display;
    let fs_copy = std::path::join(fs_nested, "copy.txt").display;
    let fs_renamed = std::path::join(fs_tree, "renamed.txt").display;
    script_eval(
        context,
        cli,
        `std::fs::create_dir_all(${json_literal(fs_nested)})`,
        audit_path
    );
    script_eval(
        context,
        cli,
        `std::fs::write(${json_literal(fs_source)}, "Unicode lifecycle: 目录")`,
        audit_path
    );
    require(
        script_eval(
            context,
            cli,
            `std::fs::copy(${json_literal(fs_source)}, ${json_literal(fs_copy)})`,
            audit_path
        ).parse_int() > 0,
        "script_fs_copy_count_invalid"
    );
    script_eval(
        context,
        cli,
        `std::fs::rename(${json_literal(fs_copy)}, ${json_literal(fs_renamed)})`,
        audit_path
    );
    require(
        script_eval(
            context,
            cli,
            `std::fs::read_to_string(${json_literal(fs_renamed)})`,
            audit_path
        ) == "Unicode lifecycle: 目录",
        "script_fs_unicode_corrupted"
    );
    script_eval(
        context,
        cli,
        `std::fs::remove_file(${json_literal(fs_renamed)})`,
        audit_path
    );
    script_eval(
        context,
        cli,
        `std::fs::remove_dir_all(${json_literal(fs_tree)})`,
        audit_path
    );
    require(!std::fs::exists(fs_tree), "script_fs_tree_survived");
    let temp_root = script_eval(
        context,
        cli,
        "rhai::runtime::temp_dir().display",
        audit_path
    );
    require(
        temp_root != "" && !std::fs::exists(temp_root),
        "script_invocation_temp_survived"
    );
    let atomic_path = std::path::join(
        runtime_dir,
        "atomic-result-目录.json"
    ).display;
    std::fs::write(atomic_path, `{"state":"old"}`);
    script_eval(
        context,
        cli,
        "rhai::runtime::atomic_write(" +
            json_literal(atomic_path) + ", " +
            json_literal(`{"state":"新"}`) + ")",
        audit_path
    );
    require(
        std::fs::read_to_string(atomic_path) == `{"state":"新"}`,
        "script_atomic_write_mismatch"
    );
    let staging_count = 0;
    for entry in std::fs::read_dir(runtime_dir) {
        if entry.file_name.starts_with(".atomic-result-")
                && entry.file_name.contains(".agenterm-atomic-") {
            staging_count += 1;
        }
    }
    require(staging_count == 0, "script_atomic_staging_survived");
    require(
        script_eval(
            context,
            cli,
            `std::path::PathBuf::from(${data_literal}).extension`,
            audit_path
        ) == "json",
        "script_typed_path_mismatch"
    );
    let arbitrary_bytes = script_eval_json(
            context,
            cli,
            `let b=rhai::bytes::from_array([0,65,255]);
             #{len:b.len,first:b.get(0),last:b.get(2)}`,
            audit_path
        );
    require(
        arbitrary_bytes.ok
            && arbitrary_bytes.value.len == 3
            && arbitrary_bytes.value.first == 0
            && arbitrary_bytes.value.last == 255,
        "script_arbitrary_bytes_mismatch"
    );
    require(
        helper::cli_text(
            context,
            cli,
            [
                "script", "eval", `std::fs::exists(".")`,
                "--profile", "pure"
            ],
            audit_path
        ) == "true",
        "script_legacy_pure_restricted"
    );
    for fixture in [
        ["let = ;", "script_parse"],
        ["new_tab()", "script_api_unavailable"],
        ["made_up_api()", "script_api_unknown"],
        ["agent.workspace()", "script_api_migrated"]
    ] {
        std::fs::write(source_file, fixture[0]);
        let failure = helper::failure_text(
            context,
            cli,
            [
                "script", "check", source_file,
                "--profile", "observe"
            ],
            1,
            audit_path
        );
        require(
            failure.contains(fixture[1]),
            "script_static_failure_untyped:" + fixture[1]
        );
        if fixture[1] == "script_api_migrated" {
            require(
                failure.contains("fleet.workspace.info()"),
                "script_static_migration_target_missing"
            );
        }
    }

    print("STEP unrestricted legacy spellings and operation budget");
    require(
        helper::cli_text(
            context,
            cli,
            [
                "script", "eval", "std::process::id()",
                "--profile", "pure"
            ],
            audit_path
        ).parse_int() > 0,
        "script_legacy_pure_process_restricted"
    );
    let operation_limit = helper::failure_text(
        context,
        cli,
        [
            "script", "eval", "loop {}",
            "--max-operations", "1000"
        ],
        3,
        audit_path
    );
    require(
        operation_limit.contains(`"code":"limit_operations"`),
        "script_operation_limit_untyped"
    );
    let wall_limit = helper::failure_text(
        context,
        cli,
        [
            "script", "eval", "loop {}",
            "--timeout-ms", "1",
            "--max-operations", "10000000"
        ],
        3,
        audit_path
    );
    require(
        wall_limit.contains(`"code":"limit_wall_time"`)
            && wall_limit.contains(`"exit_class":"limit"`),
        "script_wall_limit_untyped"
    );
    let huge_source = "";
    huge_source.pad(262145, " ");
    std::fs::write(source_file, huge_source);
    let source_limit = helper::failure_text(
        context,
        cli,
        ["script", "check", source_file],
        3,
        audit_path
    );
    require(
        source_limit.contains("262144 byte limit"),
        "script_source_limit_untyped"
    );
    let malformed_command = helper::configured_program(
        context,
        script,
        ["--worker"],
        "",
        #{}
    );
    malformed_command.stdin_text("{");
    let malformed = malformed_command.output();
    require(
        malformed.exit_code == 1
            && malformed.stdout_text().contains(
                `"code":"protocol_invalid_invocation"`
            ),
        "script_worker_malformed_untyped"
    );
    let oversized_text = "";
    oversized_text.pad(2097153, "x");
    let oversized_command = helper::configured_program(
        context,
        script,
        ["--worker"],
        "",
        #{}
    );
    oversized_command.stdin_text(oversized_text);
    let oversized = oversized_command.output();
    require(
        oversized.exit_code == 1
            && oversized.stdout_text().contains(
                `"code":"protocol_invocation_too_large"`
            ),
        "script_worker_oversized_untyped"
    );
    require(
        script_eval(context, cli, "6 * 7", audit_path) == "42",
        "script_worker_protocol_recovery_failed"
    );

    print("STEP framed worker sequencing, isolation, rejection, and recovery");
    let framed = rhai::bytes::from_array([]);
    framed.append(helper::payload_frame(rhai::bytes::from_text("{")));
    let oversized_payload = "";
    oversized_payload.pad(2097153, "x");
    framed.append(
        helper::payload_frame(
            rhai::bytes::from_text(oversized_payload)
        )
    );
    framed.append(
        helper::json_frame(
            helper::framed_invocation(
                "recovery",
                "invoke-one",
                `print("framed-output"); 40 + 2`,
                1
            )
        )
    );
    framed.append(
        helper::json_frame(
            helper::framed_invocation(
                "frame-two",
                "invoke-one",
                "1",
                1
            )
        )
    );
    framed.append(
        helper::json_frame(
            helper::framed_invocation(
                "bad-version",
                "never-run",
                "1",
                2
            )
        )
    );
    let frames = helper::invoke_framed_worker(
        context,
        script,
        framed
    );
    let codes = [];
    let recovered = ();
    for frame in frames {
        if frame.frame_id == "recovery" {
            recovered = frame;
        }
        if frame.payload.failure != () {
            codes.push(frame.payload.failure.code);
        }
    }
    require(
        frames.len == 5
            && codes.contains("protocol_malformed_frame")
            && codes.contains("protocol_frame_too_large")
            && codes.contains("protocol_duplicate_invocation")
            && codes.contains("protocol_unsupported_frame_version")
            && recovered != ()
            && recovered.payload.ok
            && recovered.payload.stdout == "framed-output\n"
            && recovered.payload.value == 42,
        "script_framed_recovery_mismatch"
    );
    let cancel_invoke = helper::framed_invocation(
        "cancel-invoke",
        "cancel-running",
        "loop {}",
        1
    );
    cancel_invoke.payload.budgets.wall_time_ms = 10000;
    cancel_invoke.payload.budgets.operations = 10000000;
    let cancel_bytes = helper::json_frame(cancel_invoke);
    cancel_bytes.append(
        helper::json_frame(#{
            frame_version: 1,
            frame_id: "cancel-request",
            kind: "cancel",
            payload: #{invocation_id:"cancel-running"}
        })
    );
    let cancel_frames = helper::invoke_framed_worker(
        context,
        script,
        cancel_bytes
    );
    require(
        cancel_frames.len == 1
            && cancel_frames[0].frame_id == "cancel-invoke"
            && cancel_frames[0].payload.failure.code ==
                "limit_cancelled"
            && cancel_frames[0].payload.exit_class == "cancelled",
        "script_framed_cancel_mismatch"
    );
}

fn assert_supervisor(
    context,
    cli,
    audit_path,
    runtime_dir
) {
    print("STEP supervisor timeout, crash, parent exit, concurrency, and recovery");
    let blocking_source =
        `std::net::TcpListener::bind("127.0.0.1:0").accept()`;

    let hard_timeout = helper::start_script_client(
        context,
        cli,
        [
            "script", "eval", blocking_source,
            "--timeout-ms", "500",
            "--max-operations", "10000000"
        ],
        audit_path
    );
    let hard_output = hard_timeout.wait_with_output(
        std::time::Duration::from_secs(5)
    );
    require(
        hard_output.exit_code == 3
            && hard_output.stderr_text().contains(
                `"code":"host_hard_timeout"`
            ),
        "script_supervisor_hard_timeout_untyped"
    );

    let existing = helper::process_ids("agenterm-script.exe");
    let crash_client = helper::start_script_client(
        context,
        cli,
        [
            "script", "eval", blocking_source,
            "--timeout-ms", "10000",
            "--max-operations", "10000000"
        ],
        audit_path
    );
    let crash_worker = helper::wait_new_process(
        "agenterm-script.exe",
        existing,
        3000
    );
    std::process::kill(crash_worker);
    let crash_output = crash_client.wait_with_output(
        std::time::Duration::from_secs(5)
    );
    require(
        crash_output.exit_code == 1
            && crash_output.stderr_text().contains(
                `"code":"host_worker_crash"`
            ),
        "script_supervisor_crash_untyped"
    );

    let pid_path = std::path::join(
        runtime_dir,
        "interrupted-child.pid"
    ).display;
    let interrupted_source =
        `let command=std::process::command("cmd.exe");
         command.args(["/d","/s","/c","ping -n 30 127.0.0.1 >nul"]);
         let child=command.start();
         std::fs::write(${json_literal(pid_path)}, child.id.to_string());
         std::net::TcpListener::bind("127.0.0.1:0").accept();`;
    let interrupted = helper::start_script_client(
        context,
        cli,
        [
            "script", "eval", interrupted_source,
            "--timeout-ms", "10000",
            "--max-operations", "10000000"
        ],
        audit_path
    );
    let nested_pid = 0;
    for attempt in 0..600 {
        if std::fs::exists(pid_path) {
            nested_pid = trimmed(
                std::fs::read_to_string(pid_path)
            ).parse_int();
            break;
        }
        require(
            interrupted.state == "running",
            "script_supervisor_parent_fixture_early_exit"
        );
        rhai::task::sleep(std::time::Duration::from_millis(5));
    }
    require(nested_pid > 0, "script_supervisor_nested_pid_missing");
    interrupted.kill();
    interrupted.wait_with_output(std::time::Duration::from_secs(2));
    helper::wait_process_gone(nested_pid, 3000);

    // Fill the published eight-slot global pool until the first typed denial.
    // The number already occupied by release/check/task ancestors varies with
    // the caller, so a fixed holder count would make this evidence flaky.
    let holders = [];
    let denied_output = ();
    for index in 0..8 {
        let holder = helper::start_script_client(
            context,
            cli,
            [
                "script", "eval", blocking_source,
                "--timeout-ms", "10000",
                "--max-operations", "10000000"
            ],
            audit_path
        );
        rhai::task::sleep(std::time::Duration::from_millis(100));
        if holder.state == "running" {
            holders.push(holder);
        } else {
            denied_output = holder.wait_with_output(
                std::time::Duration::from_secs(3)
            );
            break;
        }
    }
    require(
        holders.len > 0,
        "script_supervisor_no_holder_acquired"
    );
    require(
        denied_output != (),
        "script_supervisor_global_limit_not_enforced"
    );
    require(
        denied_output.exit_code == 2
            && denied_output.stderr_text().contains(
                `"code":"host_concurrency_limit"`
            ),
        "script_supervisor_concurrency_untyped"
    );
    holders[0].kill();
    holders[0].wait_with_output(std::time::Duration::from_secs(2));
    let replacement = helper::start_script_client(
        context,
        cli,
        [
            "script", "eval", blocking_source,
            "--timeout-ms", "10000",
            "--max-operations", "10000000"
        ],
        audit_path
    );
    rhai::task::sleep(std::time::Duration::from_millis(150));
    require(
        replacement.state == "running",
        "script_supervisor_slot_not_recovered"
    );
    for index in 1..holders.len {
        if holders[index].state == "running" {
            holders[index].kill();
        }
        holders[index].wait_with_output(
            std::time::Duration::from_secs(2)
        );
    }
    replacement.kill();
    replacement.wait_with_output(std::time::Duration::from_secs(2));
    require(
        script_eval(context, cli, "6 * 7", audit_path) == "42",
        "script_supervisor_recovery_failed"
    );
}

fn wait_script_gui(context, cli, gui_child) {
    for attempt in 0..400 {
        let probe_command = helper::configured_program(
            context,
            cli,
            [
                "--address", context.address,
                "wait-ui", "--window-state", "restored",
                "--timeout-ms", "100"
            ],
            "",
            #{}
        );
        let probe = probe_command.output();
        if probe.success
                && gui_child.platform_facts.top_level_window_present {
            let snapshot = rhai::json::parse(probe.stdout_text());
            require(
                snapshot.client_pid == gui_child.id,
                "script_fleet_gui_lease_owner"
            );
            return snapshot;
        }
        require(
            gui_child.state == "running",
            "script_fleet_gui_early_exit"
        );
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "script_fleet_gui_ready_timeout";
}

fn assert_fleet_and_north_star(
    context,
    repo,
    cli,
    script,
    gui,
    audit_path,
    runtime_dir
) {
    print("STEP typed brokered observation");
    let server_started = false;
    let gui_child = ();
    let http_child = ();
    let journey_error = "";
    try {
        helper::cli_text(
            context,
            cli,
            [
                "new-window", "-d", "-n",
                "script-observe-" + context.run_id
            ],
            audit_path
        );
        server_started = true;
        let gui_command = helper::configured_program(
            context,
            gui,
            ["--no-activate", "--address", context.address],
            "",
            #{}
        );
        gui_child = gui_command.start();
        let snapshot = wait_script_gui(context, cli, gui_child);
        let public_capture = helper::cli_json(
            context,
            cli,
            [
                "capture-pane", "-p", "-t", "@1",
                "--max-bytes", "5", "--json"
            ],
            audit_path
        );
        require(
            public_capture.max_bytes == 5
                && public_capture.bytes <= 5,
            "script_fleet_public_capture_mismatch"
        );
        let invalid_capture = helper::failure_text(
            context,
            cli,
            [
                "capture-pane", "-p", "-t", "@1",
                "--max-bytes", "0"
            ],
            1,
            audit_path
        );
        require(
            invalid_capture.contains("must be from 1 to 1048576"),
            "script_fleet_invalid_capture_accepted"
        );
        let observed = helper::cli_text(
            context,
            cli,
            [
                "script", "eval",
                "fleet.ui.snapshot().event_position.sequence",
                "--profile", "observe"
            ],
            audit_path
        ).parse_int();
        require(
            observed >= snapshot.event_position.sequence,
            "script_fleet_snapshot_position_regressed"
        );
        let workspace = helper::cli_json(
            context,
            cli,
            [
                "script", "eval", "fleet.workspace.info()",
                "--profile", "observe", "--json"
            ],
            audit_path
        );
        let tabs = helper::cli_json(
            context,
            cli,
            [
                "script", "eval", "fleet.tabs.list()",
                "--profile", "observe", "--json"
            ],
            audit_path
        );
        let active = helper::cli_json(
            context,
            cli,
            [
                "script", "eval", "fleet.tabs.active()",
                "--profile", "observe", "--json"
            ],
            audit_path
        );
        let capture = helper::cli_json(
            context,
            cli,
            [
                "script", "eval",
                "fleet.terminal(`@1`).capture(32)",
                "--profile", "observe", "--json"
            ],
            audit_path
        );
        require(
            workspace.ok
                && workspace.value.event_position.epoch != ""
                && tabs.ok
                && tabs.value.len >= 1
                && active.ok
                && active.value.active
                && capture.ok
                && capture.value.bytes <= 32
                && capture.value.max_bytes == 32,
            "script_fleet_observation_mismatch"
        );
        let epoch = json_literal(snapshot.event_position.epoch);
        let batch = helper::cli_json(
            context,
            cli,
            [
                "script", "eval",
                "fleet.events.read(" + epoch + ", " +
                    snapshot.event_position.sequence + ", 16)",
                "--profile", "observe", "--json"
            ],
            audit_path
        );
        require(
            batch.ok && batch.value.position != (),
            "script_fleet_event_read_mismatch"
        );
        let restart = helper::failure_text(
            context,
            cli,
            [
                "script", "eval",
                "fleet.events.read(`wrong-epoch`, 0, 1)",
                "--profile", "observe"
            ],
            6,
            audit_path
        );
        require(
            restart.contains(`"code":"server_restart"`)
                && restart.contains(`"exit_class":"fleet"`),
            "script_fleet_restart_untyped"
        );
        let wait_started = std::time::SystemTime::now().unix_millis;
        let wait_failure = helper::failure_text(
            context,
            cli,
            [
                "script", "eval",
                "fleet.events.wait(" + epoch + ", " +
                    snapshot.event_position.sequence +
                    ", `never.matches`, 50)",
                "--profile", "observe", "--timeout-ms", "200"
            ],
            6,
            audit_path
        );
        let wait_elapsed =
            std::time::SystemTime::now().unix_millis - wait_started;
        require(
            wait_failure.contains("event_wait_timeout")
                && wait_elapsed < 500,
            "script_fleet_wait_deadline_mismatch"
        );
        require(
            helper::cli_text(
                context,
                cli,
                [
                    "script", "eval",
                    "fleet.ui.tabs.toggle().post_state.verified",
                    "--profile", "observe"
                ],
                audit_path
            ) == "true"
                && helper::cli_text(
                    context,
                    cli,
                    [
                        "script", "eval",
                        "fleet.ui.tabs.toggle().post_state.verified",
                        "--profile", "observe"
                    ],
                    audit_path
                ) == "true",
            "script_fleet_observe_spelling_restricted"
        );

        print("STEP Script API v2 Fleet mutation receipt, event, and post-state");
        let baseline_visible = snapshot.layout.sidebar.visible;
        let mutation = script_eval_json(
            context,
            cli,
            `
let receipt=fleet.ui.tabs.toggle();
#{
 request_id:receipt.request_id,operation_id:receipt.operation_id,
 outcome:receipt.outcome,event_count:receipt.events.len(),
 event_kind:if receipt.events.is_empty(){""}else{receipt.events[0].kind},
 verified:receipt.post_state.verified,reason:receipt.post_state.reason
}`,
            audit_path
        );
        let mutated_snapshot = helper::cli_json(
            context,
            cli,
            ["ui-snapshot"],
            audit_path
        );
        require(
            mutation.ok
                && mutation.value.request_id != ""
                && mutation.value.operation_id == "ui.tabs.toggle"
                && (
                    mutation.value.outcome == "committed"
                        || mutation.value.outcome == "no_op"
                )
                && mutation.value.event_count >= 1
                && mutation.value.event_kind ==
                    "layout.tabs.visibility"
                && mutation.value.verified
                && mutated_snapshot.layout.sidebar.visible !=
                    baseline_visible,
            "script_fleet_mutation_receipt_mismatch"
        );
        require(
            script_eval(
                context,
                cli,
                "fleet.ui.tabs.toggle().post_state.verified",
                audit_path
            ) == "true",
            "script_fleet_visibility_restore_failed"
        );
        let restored = helper::cli_json(
            context,
            cli,
            ["ui-snapshot"],
            audit_path
        );
        require(
            restored.layout.sidebar.visible == baseline_visible,
            "script_fleet_visibility_not_restored"
        );
        let active_tab = ();
        for tab in restored.tabs {
            if tab.active {
                active_tab = tab;
            }
        }
        require(active_tab != (), "script_fleet_active_tab_missing");
        let note = "Script API v2 目录 " + std::process::id();
        let note_mutation = script_eval_json(
            context,
            cli,
            `
let receipt=fleet.tabs.set_note(
 ${json_literal(active_tab.id)},${json_literal(note)}
);
#{
 operation_id:receipt.operation_id,outcome:receipt.outcome,
 event_count:receipt.events.len(),
 event_kind:if receipt.events.is_empty(){""}else{receipt.events[0].kind},
 verified:receipt.post_state.verified,
 tab:receipt.post_state.value.id,note:receipt.post_state.value.note
}`,
            audit_path
        );
        require(
            note_mutation.ok
                && note_mutation.value.operation_id == "tabs.set-note"
                && note_mutation.value.event_count >= 1
                && note_mutation.value.event_kind == "tab.note"
                && note_mutation.value.verified
                && note_mutation.value.tab == active_tab.id
                && note_mutation.value.note == note,
            "script_fleet_note_receipt_mismatch"
        );
        require(
            script_eval(
                context,
                cli,
                "fleet.tabs.set_note(" +
                    json_literal(active_tab.id) + ", " +
                    json_literal(active_tab.note) +
                    ").post_state.verified",
                audit_path
            ) == "true",
            "script_fleet_note_restore_failed"
        );

        print("STEP direct agenterm-script north-star named task");
        let source_project = std::path::join(
            repo,
            "examples/script-daily-check"
        ).display;
        let project = std::path::join(
            runtime_dir,
            "north-star-project"
        ).display;
        std::fs::create_dir_all(project);
        std::fs::copy(
            std::path::join(
                source_project,
                "agenterm.tasks.json"
            ).display,
            std::path::join(project, "agenterm.tasks.json").display
        );
        std::fs::copy(
            std::path::join(
                source_project,
                "daily-check.rhai"
            ).display,
            std::path::join(project, "daily-check.rhai").display
        );
        let manifest = std::path::join(
            project,
            "agenterm.tasks.json"
        ).display;
        let entry = std::path::join(
            project,
            "daily-check.rhai"
        ).display;
        let result_path = std::path::join(
            runtime_dir,
            "daily-check-result.json"
        ).display;
        let ready_path = std::path::join(
            runtime_dir,
            "north-star-http-ready.json"
        ).display;
        let log_path = std::path::join(
            runtime_dir,
            "north-star-http-log.jsonl"
        ).display;
        let stop_path = std::path::join(
            runtime_dir,
            "north-star-http-stop"
        ).display;
        http_child = helper::start_http_fixture(
            context,
            script,
            std::path::join(
                repo,
                "scripts/rhai/script-http-fixture.rhai"
            ).display,
            repo,
            ready_path,
            log_path,
            stop_path
        );
        let ready = helper::wait_http_ready(
            http_child,
            ready_path,
            3000
        );
        let latest = helper::cli_json(
            context,
            cli,
            ["ui-snapshot"],
            audit_path
        );
        let north_tab = ();
        for tab in latest.tabs {
            if tab.active {
                north_tab = tab;
            }
        }
        require(north_tab != (), "script_north_star_tab_missing");
        std::fs::write(
            std::path::join(project, "config.json").display,
            rhai::json::stringify_pretty(#{
                schema_version: 1,
                message: "AgenTerm daily check：目录与终端",
                child_program: cli,
                child_a_args: ["--version"],
                child_b_args: ["list-commands"],
                loopback_url: ready.url + "/status",
                tab_id: north_tab.id,
                result_path: result_path
            })
        );
        let direct_help = helper::direct_text(
            context,
            script,
            ["--help"],
            audit_path
        );
        require(
            direct_help.contains("AgenTerm Script Runtime")
                && direct_help.contains("agenterm-script task run"),
            "script_direct_help_mismatch"
        );
        require(
            helper::direct_text(
                context,
                script,
                [
                    "check", entry, "--profile", "local",
                    "--project-root", project
                ],
                audit_path
            ) == "OK",
            "script_north_star_check_failed"
        );
        let direct_catalog = helper::direct_json(
            context,
            script,
            [
                "task", "list",
                "--manifest", manifest, "--json"
            ],
            audit_path
        );
        let direct_show = helper::direct_json(
            context,
            script,
            [
                "task", "show", "daily-check",
                "--manifest", manifest, "--json"
            ],
            audit_path
        );
        require(
            direct_catalog.project_id ==
                "agenterm-script-daily-check"
                && direct_catalog.schema_version == 3
                && direct_catalog.compatible
                && direct_catalog.tasks.len == 1
                && direct_catalog.tasks[0].status == "ready"
                && direct_catalog.tasks[0].contract.inputs.len == 3
                && direct_catalog.tasks[0].contract.evidence ==
                    ["task.daily-check"]
                && direct_show.compatible
                && direct_show.tasks[0].entry == "daily-check.rhai",
            "script_north_star_catalog_mismatch"
        );
        require(
            helper::direct_text(
                context,
                script,
                [
                    "task", "check", "daily-check",
                    "--manifest", manifest
                ],
                audit_path
            ) == "OK",
            "script_north_star_task_check_failed"
        );
        let north_envelope = helper::direct_json(
            context,
            script,
            [
                "task", "run", "daily-check",
                "--manifest", manifest,
                "--timeout-ms", "10000",
                "--max-operations", "1000000",
                "--json", "--", "smoke-target"
            ],
            audit_path
        );
        let north = north_envelope.value;
        require(
            north_envelope.ok
                && north.schema_version == 1
                && north.target == "smoke-target"
                && north.message ==
                    "AgenTerm daily check：目录与终端"
                && north.child_a.pid > 0
                && north.child_b.pid > 0
                && north.child_a.pid != north.child_b.pid
                && north.child_a.exit_code == 0
                && north.child_b.exit_code == 0
                && north.child_a.complete
                && north.child_b.complete
                && north.http.status == 201
                && north.http.body == "hello"
                && north.http.task_state == "completed"
                && north.fleet.operation_id == "tabs.set-note"
                && north.fleet.event_kind == "tab.note"
                && north.fleet.verified,
            "script_north_star_result_mismatch"
        );
        require(
            !std::fs::exists(north.temp_root)
                && std::fs::exists(result_path),
            "script_north_star_lifecycle_mismatch"
        );
        let persisted = rhai::json::parse_file(result_path);
        require(
            persisted.target == north.target
                && persisted.fleet.note == north.fleet.note,
            "script_north_star_persisted_result_mismatch"
        );
        require(
            !helper::process_exists(north.child_a.pid)
                && !helper::process_exists(north.child_b.pid),
            "script_north_star_child_survived"
        );
        require(
            script_eval(
                context,
                cli,
                "fleet.tabs.set_note(" +
                    json_literal(north_tab.id) + ", " +
                    json_literal(north_tab.note) +
                    ").post_state.verified",
                audit_path
            ) == "true",
            "script_north_star_note_restore_failed"
        );
        std::fs::write(stop_path, "stop");
        let http_output = http_child.wait_with_output(
            std::time::Duration::from_secs(3)
        );
        require(
            http_output.success,
            "script_north_star_http_exit"
        );
        http_child = ();

        let close_state = helper::cli_json(
            context,
            cli,
            ["ui-action", "close-window"],
            audit_path
        );
        require(
            close_state.modal.actions.contains(
                "stop-server-and-exit"
            ),
            "script_fleet_close_contract_missing"
        );
        helper::cli_text(
            context,
            cli,
            ["ui-action", "stop-server-and-exit"],
            audit_path
        );
        gui_child.wait_with_output(
            std::time::Duration::from_secs(5)
        );
        gui_child = ();
        server_started = false;
    } catch (error) {
        journey_error = error.to_string();
    }
    if http_child != () && http_child.state == "running" {
        http_child.kill();
        http_child.wait_with_output(
            std::time::Duration::from_secs(2)
        );
    }
    if server_started {
        try {
            helper::run_cli(
                context,
                cli,
                ["kill-server"],
                0,
                "",
                #{},
                []
            );
        } catch (cleanup_error) {
            // The typed close path may already have removed the server.
        }
    }
    if gui_child != () && gui_child.state == "running" {
        gui_child.kill();
        gui_child.wait_with_output(
            std::time::Duration::from_secs(2)
        );
    }
    require(journey_error == "", journey_error);
}

fn assert_audit(
    context,
    cli,
    audit_path,
    source_file,
    runtime_dir
) {
    print("STEP privacy-bounded reusable script audit");
    std::fs::write(
        source_file,
        `print("AUDIT_STDOUT_SECRET");
         args[0] + "AUDIT_SOURCE_SECRET"`
    );
    let secret_result = helper::cli_text(
        context,
        cli,
        [
            "script", "run", source_file,
            "--", "AUDIT_ARG_SECRET"
        ],
        audit_path
    );
    require(
        secret_result.contains("AUDIT_STDOUT_SECRET")
            && secret_result.contains(
                "AUDIT_ARG_SECRETAUDIT_SOURCE_SECRET"
            ),
        "script_audit_secret_fixture_inactive"
    );
    let audit_text = std::fs::read_to_string(audit_path);
    for secret in [
        "AUDIT_STDOUT_SECRET",
        "AUDIT_ARG_SECRET",
        "AUDIT_SOURCE_SECRET",
        "AUDIT_ENV_SECRET",
        "HTTP_CREDENTIAL_SECRET",
        "PRIVATE_PATH_SECRET",
        "PROXY_CREDENTIAL_SECRET"
    ] {
        require(
            !audit_text.contains(secret),
            "script_audit_secret_leak:" + secret
        );
    }
    for field in [
        `"source":`,
        `"arguments":`,
        `"argv":`,
        `"stdout":`,
        `"pane":`,
        `"environment":`,
        `"clipboard":`,
        `"credentials":`
    ] {
        require(
            !audit_text.contains(field),
            "script_audit_forbidden_field:" + field
        );
    }
    let records = [];
    for line in audit_text.split("\n") {
        if trimmed(line) != "" {
            records.push(rhai::json::parse(line));
        }
    }
    require(records.len >= 10, "script_audit_record_count");
    let failure_codes = #{};
    let timeout_record = false;
    let crash_record = false;
    let concurrency_record = false;
    let observe_record = false;
    let pure_record = false;
    for record in records {
        if record.failure_code != () {
            failure_codes[record.failure_code] = true;
        }
        if record.cancelled == true && record.timed_out == true {
            timeout_record = true;
        }
        if record.crashed == true {
            crash_record = true;
        }
        if record.failure_code == "host_concurrency_limit"
                && record.denied == true {
            concurrency_record = true;
        }
        if record.requested_profile == "observe"
                && record.effective_profile == "unrestricted"
                && record.effective_capabilities
                    .contains("unrestricted_local")
                && record.broker_operation_ids
                    .contains("ui.snapshot") {
            observe_record = true;
        }
        if record.requested_profile == "pure"
                && record.effective_profile == "unrestricted"
                && record.effective_capabilities
                    .contains("unrestricted_local") {
            pure_record = true;
        }
        require(
            record.source_fingerprint.starts_with("fnv1a128:")
                && record.source_fingerprint.len == 41,
            "script_audit_fingerprint_mismatch"
        );
    }
    for required_code in [
        "script_api_unavailable",
        "limit_wall_time",
        "host_hard_timeout",
        "host_worker_crash",
        "host_concurrency_limit"
    ] {
        require(
            failure_codes[required_code] == true,
            "script_audit_failure_code_missing:" + required_code
        );
    }
    require(
        timeout_record
            && crash_record
            && concurrency_record
            && observe_record
            && pure_record,
        "script_audit_result_metadata_mismatch"
    );
    let audit_failure = helper::run_cli(
        context,
        cli,
        ["script", "eval", "1"],
        1,
        runtime_dir,
        #{},
        []
    );
    require(
        (
            audit_failure.stdout +
            audit_failure.stderr
        ).contains(`"code":"host_audit_write"`),
        "script_audit_write_failure_not_fail_closed"
    );
}

require(
    args.len == 4 || (
        args.len == 5
            && args[4] == "--internal-failure-bundle-probe"
    ),
    "expected: REPO CLI_EXE SCRIPT_EXE GUI_EXE [--internal-failure-bundle-probe]"
);
let repo = std::path::absolute(args[0]).display;
let cli = std::path::absolute(args[1]).display;
let script = std::path::absolute(args[2]).display;
let gui = std::path::absolute(args[3]).display;
let internal_failure_probe = args.len == 5;
for executable in [cli, script, gui] {
    require(std::fs::exists(executable), "script_smoke_executable_missing");
}

let context = test_harness::new_context(repo, "script");
let runtime_dir = std::path::join(
    context.run_directory,
    "script-runtime"
).display;
std::fs::create_dir_all(runtime_dir);
let source_file = std::path::join(runtime_dir, "source.rhai").display;
let audit_file = std::path::join(runtime_dir, "audit.jsonl").display;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    let catalog = assert_api_contract(
        context,
        cli,
        script,
        runtime_dir,
        audit_file
    );
    emit(context, declared_evidence, "script.api-tree");
    assert_process_and_streams(
        context,
        cli,
        audit_file,
        runtime_dir
    );
    emit(context, declared_evidence, "script.typed-errors");
    emit(context, declared_evidence, "script.stream");
    assert_tasks_and_http(
        context,
        repo,
        cli,
        script,
        audit_file,
        runtime_dir
    );
    emit(context, declared_evidence, "script.http");
    emit(context, declared_evidence, "script.modules-tasks");
    assert_runtime_and_framing(
        context,
        repo,
        cli,
        script,
        audit_file,
        runtime_dir,
        source_file,
        internal_failure_probe
    );
    emit(context, declared_evidence, "script.fs-lifecycle");
    emit(context, declared_evidence, "script.rhai-runtime");
    emit(
        context,
        declared_evidence,
        "script.rhai-robustness-budget"
    );
    emit(context, declared_evidence, "script.rhai-framed");
    assert_supervisor(
        context,
        cli,
        audit_file,
        runtime_dir
    );
    emit(context, declared_evidence, "script.runtime-lifecycle");
    emit(context, declared_evidence, "script.supervisor");
    assert_fleet_and_north_star(
        context,
        repo,
        cli,
        script,
        gui,
        audit_file,
        runtime_dir
    );
    emit(context, declared_evidence, "script.exit-classes");
    emit(context, declared_evidence, "script.rhai-fleet");
    emit(context, declared_evidence, "script.fleet-tabs-set-note");
    emit(context, declared_evidence, "script.fleet-v2");
    emit(context, declared_evidence, "script.direct-entry");
    emit(context, declared_evidence, "script.north-star");
    assert_audit(
        context,
        cli,
        audit_file,
        source_file,
        runtime_dir
    );
    emit(context, declared_evidence, "script.audit");
    require(catalog.api_version == 2, "script_smoke_catalog_lost");
    succeeded = true;
    print(
        "PASS: unrestricted scripting API, supervision, audit privacy, and budgets"
    );
} catch (error) {
    let safe_error = test_harness::redact_text(
        error.to_string(),
        [
            "AUDIT_STDOUT_SECRET",
            "AUDIT_ARG_SECRET",
            "AUDIT_SOURCE_SECRET",
            "AUDIT_ENV_SECRET",
            "HTTP_CREDENTIAL_SECRET",
            "PRIVATE_PATH_SECRET",
            "PROXY_CREDENTIAL_SECRET"
        ]
    );
    let probe_marker =
        "INTERNAL_FAILURE_BUNDLE_PROBE:script:" + context.run_id;
    failure = if internal_failure_probe
            && safe_error.contains(probe_marker) {
        probe_marker + "\nintentional script failure-bundle probe"
    } else {
        safe_error
    };
}

if !succeeded && std::fs::exists(audit_file) {
    std::fs::create_dir_all(context.failure_directory);
    std::fs::copy(
        audit_file,
        std::path::join(
            context.failure_directory,
            "script-audit.jsonl"
        ).display
    );
}
test_harness::scrub_retained_text(
    context,
    [
        "AUDIT_STDOUT_SECRET",
        "AUDIT_ARG_SECRET",
        "AUDIT_SOURCE_SECRET",
        "AUDIT_ENV_SECRET",
        "HTTP_CREDENTIAL_SECRET",
        "PRIVATE_PATH_SECRET",
        "PROXY_CREDENTIAL_SECRET"
    ]
);
let completion_failure = "";
try {
    test_harness::complete(
        context,
        owned_children,
        succeeded,
        failure
    );
} catch (error) {
    completion_failure = error.to_string();
}
if succeeded && completion_failure == "" {
    test_harness::remove_run(context);
}
if !succeeded {
    throw failure;
}
if completion_failure != "" {
    throw completion_failure;
}
