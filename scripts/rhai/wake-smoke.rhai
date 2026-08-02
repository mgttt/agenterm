// Prove coalesced server wake delivery under concurrent IPC and PTY output.
//
// Args:
//   --list-evidence
// or:
//   REPO SERVER_EXE CLI_EXE

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = ["fleet.wake-coalescing", "ux-parity.wake-coalescing"];

if args.len == 1 && args[0] == "--list-evidence" {
    print("fleet.wake-coalescing");
    print("ux-parity.wake-coalescing");
    return;
}

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn command(context, cli, arguments, expected_failure) {
    test_harness::invoke_cli(
        context,
        cli,
        arguments,
        expected_failure,
        []
    )
}

fn command_text(context, cli, arguments) {
    let output = command(context, cli, arguments, false);
    output.stdout + output.stderr
}

fn wait_for_server(context, cli, server) {
    for attempt in 0..400 {
        let probe = test_harness::run_cli(
            context,
            cli,
            ["protocol-info", "--running"],
            false,
            []
        );
        if probe.success {
            return rhai::json::parse(probe.stdout);
        }
        if server.state == "exited" {
            let output = server.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw "wake_smoke_server_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "wake_smoke_server_ready_timeout";
}

fn wait_for_exit(child, code) {
    for attempt in 0..200 {
        if child.state == "exited" {
            return child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

require(args.len == 3, "expected: REPO SERVER_EXE CLI_EXE");
let repo = std::path::absolute(args[0]).display;
let server_exe = std::path::absolute(args[1]).display;
let cli = std::path::absolute(args[2]).display;
require(std::fs::exists(server_exe), "wake_smoke_server_missing");
require(std::fs::exists(cli), "wake_smoke_cli_missing");

let context = test_harness::new_context(repo, "wake");
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP start isolated headless server and terminal");
    let server_command = test_harness::configured_command(
        context,
        server_exe,
        ["--address", context.address]
    );
    let server = server_command.start();
    owned_children.push(server);
    let protocol = wait_for_server(context, cli, server);
    require(protocol.pid == server.id, "wake_smoke_server_identity");

    let name = "wake-" + context.run_id;
    let tab_id = command_text(
        context,
        cli,
        [
            "new-window", "-d", "-P", "-F", "#{window_id}",
            "-n", name,
            "--", "cmd.exe", "/d", "/q"
        ]
    );
    tab_id.trim();
    command_text(
        context,
        cli,
        [
            "wait-ui", "-t", tab_id, "--tab-state", "running",
            "--timeout-ms", "10000"
        ]
    );

    print("STEP concurrent IPC replies and PTY output both make progress");
    let clients = [];
    for index in 0..32 {
        let client_command = test_harness::configured_command(
            context,
            cli,
            ["--address", context.address, "ui-snapshot"]
        );
        client_command.timeout(std::time::Duration::from_secs(20));
        client_command.capture_limit(262144);
        let client = client_command.start();
        clients.push(client);
        owned_children.push(client);
    }

    let token = "WAKE_COMPLETE_" + context.run_id;
    command_text(
        context,
        cli,
        [
            "send-keys", "-t", tab_id, "-l",
            "for /L %i in (1,1,80) do @echo WAKE_" +
                context.run_id + "_%i"
        ]
    );
    command_text(context, cli, ["send-keys", "-t", tab_id, "Enter"]);
    command_text(
        context,
        cli,
        ["send-keys", "-t", tab_id, "-l", "echo " + token]
    );
    command_text(context, cli, ["send-keys", "-t", tab_id, "Enter"]);

    for client in clients {
        let output = client.wait_with_output(
            std::time::Duration::from_secs(20)
        );
        test_harness::append_command_record(
            context,
            ["ui-snapshot"],
            output,
            false,
            []
        );
        require(
            output.success,
            "wake_smoke_concurrent_client:" + output.stderr_text()
        );
        let snapshot = rhai::json::parse(output.stdout_text());
        require(
            snapshot.schema_version >= 1
                && (
                    snapshot.projection == "replaceable_ui_client"
                        || snapshot.projection == "headless_server"
                )
                && snapshot.event_position.epoch != "",
            "wake_smoke_concurrent_projection"
        );
    }
    command_text(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id, "--contains", token,
            "--timeout-ms", "10000"
        ]
    );

    print("STEP expired mutation is a typed no-op through std::net::TcpStream");
    let baseline = "wake-baseline-" + context.run_id;
    command_text(
        context,
        cli,
        ["set-tab-note", "-t", tab_id, baseline]
    );
    let command_args = [
        "set-tab-note", "-t", tab_id,
        "expired-mutation-must-not-commit"
    ];
    let args_json = rhai::json::stringify(command_args);
    let fingerprint = rhai::hash::fnv1a64(
        rhai::bytes::from_text(args_json)
    );
    let request_id = "wake-expired-" + context.run_id;
    let request = #{
        args: command_args,
        control: #{
            schema_version: 1,
            request_id: request_id,
            operation_id: "tabs.set-note",
            payload_fingerprint: fingerprint,
            intent: "mutation",
            deadline_unix_ms: 0
        }
    };
    let stream = std::net::TcpStream::connect_timeout(
        context.address,
        std::time::Duration::from_secs(10)
    );
    stream.set_nodelay(true);
    stream.write_all(rhai::json::stringify(request) + "\n");
    stream.flush();
    let response_text = stream.read_line(262144);
    stream.shutdown();
    let expired = rhai::json::parse(response_text);
    require(
        !expired.ok
            && expired.receipt.outcome == "no_op"
            && expired.receipt.error.code == "request_deadline_expired"
            && expired.receipt.error.category == "timeout"
            && expired.receipt.request_id == request_id,
        "wake_smoke_expired_receipt:" + response_text
    );
    let after_note = command_text(
        context,
        cli,
        ["show-tab-note", "-t", tab_id]
    );
    after_note.trim();
    require(after_note == baseline, "wake_smoke_expired_mutated_state");

    test_harness::emit_evidence(
        context,
        declared_evidence,
        "fleet.wake-coalescing"
    );
    command_text(context, cli, ["shutdown"]);
    let server_output = wait_for_exit(
        server,
        "wake_smoke_server_shutdown_timeout"
    );
    require(server_output.success, "wake_smoke_server_exit_failure");
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux-parity.wake-coalescing"
    );
    print(
        "PASS: coalesced wake delivery preserved IPC, PTY, and mutation correctness"
    );
    succeeded = true;
} catch (error) {
    failure = error.to_string();
}

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
if failure != "" {
    throw failure;
}
if completion_failure != "" {
    throw completion_failure;
}
