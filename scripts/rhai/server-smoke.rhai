// Prove the split headless server's lease, PTY, receipt, event, and workspace authority.
//
// Args:
//   --list-evidence
// or:
//   REPO SERVER_EXE CLI_EXE

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = ["server.headless-authority"];

if args.len == 1 && args[0] == "--list-evidence" {
    print("server.headless-authority");
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

fn command_text(context, cli, arguments, expected_failure) {
    let output = command(
        context,
        cli,
        arguments,
        expected_failure
    );
    output.stdout + output.stderr
}

fn command_json(context, cli, arguments, expected_failure) {
    let output = command(
        context,
        cli,
        arguments,
        expected_failure
    );
    rhai::json::parse(output.stdout + output.stderr)
}

fn find_by_id(values, id, code) {
    for value in values {
        if value.id == id {
            return value;
        }
    }
    throw code;
}

fn count_tab_events(events, tab_id, kind) {
    let count = 0;
    for event in events {
        if event.tab_id == tab_id && event.kind == kind {
            count += 1;
        }
    }
    count
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
            throw "server_smoke_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "server_smoke_ready_timeout";
}

fn wait_for_exit(child, code) {
    for attempt in 0..200 {
        if child.state == "exited" {
            return child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    throw code;
}

require(args.len == 3, "expected: REPO SERVER_EXE CLI_EXE");
let repo = std::path::absolute(args[0]).display;
let server_exe = std::path::absolute(args[1]).display;
let cli = std::path::absolute(args[2]).display;
require(std::fs::exists(server_exe), "server_smoke_server_missing");
require(std::fs::exists(cli), "server_smoke_cli_missing");

let context = test_harness::new_context(repo, "server");
let worker_pid = std::process::id();
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP start internal headless server");
    let server_command = test_harness::configured_command(
        context,
        server_exe,
        ["--address", context.address]
    );
    let server = server_command.start();
    owned_children.push(server);
    let protocol = wait_for_server(context, cli, server);
    let server_facts = server.platform_facts;
    require(
        server_facts.top_level_window_supported
            && !server_facts.top_level_window_present
            && protocol.pid == server.id
            && protocol.ui_bridge.ownership_mode == "split_server_client"
            && protocol.ui_bridge.server_executable == "agenterm-server.exe"
            && protocol.ui_bridge.replaceable_ui
            && protocol.ui_bridge.interactive_lease
            && protocol.ui_bridge.reconnect
            && protocol.ui_bridge.rollback_proven,
        "server_smoke_process_ownership_boundary"
    );

    print("STEP acquire, renew, conflict, heartbeat, and detach the UI lease");
    let lease_client_id = "server-smoke-ui-" + context.run_id;
    let attach_args = [
        "ui-lease", "attach",
        "--client-id", lease_client_id,
        "--client-pid", "" + worker_pid
    ];
    let lease = command_json(context, cli, attach_args, false);
    let same_lease = command_json(context, cli, attach_args, false);
    let conflict = command_text(
        context,
        cli,
        [
            "ui-lease", "attach",
            "--client-id", "conflict-" + context.run_id,
            "--client-pid", "" + server.id
        ],
        true
    );
    require(
        lease.lease_id.starts_with("ui-")
            && same_lease.lease_id == lease.lease_id
            && same_lease.expires_unix_ms >= lease.expires_unix_ms
            && conflict.contains("another live UI client"),
        "server_smoke_ui_lease_owner"
    );
    let heartbeat = command_json(
        context,
        cli,
        [
            "ui-lease", "heartbeat",
            "--lease-id", lease.lease_id,
            "--client-pid", "" + worker_pid
        ],
        false
    );
    require(
        heartbeat.lease_id == lease.lease_id
            && heartbeat.client_pid == worker_pid,
        "server_smoke_ui_lease_heartbeat"
    );

    print("STEP require the lease for UI select, resize, and binary input");
    let interaction_tab = command_text(
        context,
        cli,
        [
            "new-window", "-d", "-P", "-F", "#{window_id}",
            "-n", "interaction-" + context.run_id,
            "--", "cmd.exe", "/d", "/q"
        ]
    );
    interaction_tab.trim();
    let unauthorized = command_text(
        context,
        cli,
        [
            "ui-interact", "select",
            "--lease-id", "wrong-lease",
            "--client-pid", "" + worker_pid,
            "-t", interaction_tab
        ],
        true
    );
    let selected = command_json(
        context,
        cli,
        [
            "ui-interact", "select",
            "--lease-id", lease.lease_id,
            "--client-pid", "" + worker_pid,
            "-t", interaction_tab
        ],
        false
    );
    let resized = command_json(
        context,
        cli,
        [
            "ui-interact", "resize",
            "--lease-id", lease.lease_id,
            "--client-pid", "" + worker_pid,
            "-t", interaction_tab,
            "--rows", "24", "--columns", "90"
        ],
        false
    );
    let interaction_marker = "AGENTERM_UI_INPUT";
    let input_text = "echo AGENTERM_UI_INPUT\r";
    let input = command_json(
        context,
        cli,
        [
            "ui-interact", "input",
            "--lease-id", lease.lease_id,
            "--client-pid", "" + worker_pid,
            "-t", interaction_tab,
            "--hex", "6563686f204147454e5445524d5f55495f494e5055540d"
        ],
        false
    );
    command_text(
        context,
        cli,
        [
            "wait-pane", "-t", interaction_tab,
            "--contains", interaction_marker,
            "--timeout-ms", "5000"
        ]
    );
    let interaction_bootstrap = command_json(
        context,
        cli,
        ["ui-bootstrap"],
        false
    );
    let interaction_state = find_by_id(
        interaction_bootstrap.tabs,
        interaction_tab,
        "server_smoke_interaction_tab_missing"
    );
    require(
        unauthorized.contains("does not match the current owner")
            && selected.action == "select"
            && selected.tab_id == interaction_tab
            && resized.rows == 24
            && resized.columns == 90
            && input.input_bytes == input_text.len
            && interaction_bootstrap.active_tab_id == interaction_tab
            && interaction_state.screen.rows == 24
            && interaction_state.screen.columns == 90,
        "server_smoke_lease_gated_interaction"
    );
    let acknowledged = command_json(
        context,
        cli,
        [
            "ui-lease", "acknowledge",
            "--lease-id", lease.lease_id,
            "--client-pid", "" + worker_pid,
            "--sequence", "" + interaction_bootstrap.position.sequence
        ],
        false
    );
    let regression = command_text(
        context,
        cli,
        [
            "ui-lease", "acknowledge",
            "--lease-id", lease.lease_id,
            "--client-pid", "" + worker_pid,
            "--sequence", "" + (interaction_bootstrap.position.sequence - 1)
        ],
        true
    );
    require(
        acknowledged.observed_sequence
                == interaction_bootstrap.position.sequence
            && regression.contains("advance monotonically"),
        "server_smoke_ui_position_regression"
    );
    let detached = command_json(
        context,
        cli,
        [
            "ui-lease", "detach",
            "--lease-id", lease.lease_id,
            "--client-pid", "" + worker_pid
        ],
        false
    );
    let lease_status = command_json(
        context,
        cli,
        ["ui-lease", "status"],
        false
    );
    require(
        detached.detached && !lease_status.attached,
        "server_smoke_ui_lease_detach"
    );

    print("STEP create and observe a real PTY through public IPC");
    let hello = command_json(
        context,
        cli,
        [
            "ui-hello", "--minimum", "1", "--maximum", "1",
            "--client-id", "server-smoke-" + context.run_id
        ],
        false
    );
    for capability in [
        "interactive_lease",
        "lease_gated_interaction",
        "replaceable_ui_client",
        "lease_owned_client_state",
        "lease_owned_client_commands",
        "in_place_reconnect"
    ] {
        require(
            hello.capabilities.contains(capability),
            "server_smoke_capability_missing:" + capability
        );
    }
    let tab_id = command_text(
        context,
        cli,
        [
            "new-window", "-d", "-P", "-F", "#{window_id}",
            "-n", "headless-" + context.run_id,
            "--", "cmd.exe", "/d", "/c", "echo AGENTERM_HEADLESS_OK"
        ]
    );
    tab_id.trim();
    require(tab_id.starts_with("@"), "server_smoke_stable_tab_id");
    command_text(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", "AGENTERM_HEADLESS_OK",
            "--timeout-ms", "5000"
        ]
    );
    let capture = command_text(
        context,
        cli,
        ["capture-pane", "-p", "-t", tab_id, "--max-bytes", "16384"]
    );
    require(
        capture.contains("AGENTERM_HEADLESS_OK"),
        "server_smoke_terminal_capture"
    );

    print("STEP prove server-owned receipt replay and asynchronous completion");
    let receipt_request_id = "server-receipt-" + context.run_id;
    let receipt_args = [
        "--request-id", receipt_request_id, "--receipt-json",
        "set-tab-note", "-t", tab_id, "receipt-note-" + context.run_id
    ];
    let first_receipt = command_json(context, cli, receipt_args, false);
    let replayed_receipt = command_json(context, cli, receipt_args, false);
    let conflict_receipt = command_json(
        context,
        cli,
        [
            "--request-id", receipt_request_id, "--receipt-json",
            "set-tab-note", "-t", tab_id, "different-note"
        ],
        true
    );
    require(
        first_receipt.outcome == "committed"
            && first_receipt.request_id == receipt_request_id
            && first_receipt.after_position.sequence
                == replayed_receipt.after_position.sequence
            && conflict_receipt.outcome == "no_op"
            && conflict_receipt.error.code == "request_id_conflict",
        "server_smoke_receipt_replay"
    );
    let bootstrap = command_json(context, cli, ["ui-bootstrap"], false);
    let interactive_tab = bootstrap.active_tab_id;
    let submission_marker = "AGENTERM_SERVER_RECEIPT_" + context.run_id;
    command_text(
        context,
        cli,
        [
            "set-composer", "-t", interactive_tab,
            "echo " + submission_marker
        ]
    );
    let submission_request_id = "server-submit-" + context.run_id;
    let submission_args = [
        "--request-id", submission_request_id, "--receipt-json",
        "send-composer", "-t", interactive_tab
    ];
    let accepted_receipt = command_json(
        context,
        cli,
        submission_args,
        false
    );
    require(
        accepted_receipt.outcome == "accepted"
            && accepted_receipt.wait.condition == "submission_complete",
        "server_smoke_receipt_acceptance"
    );
    command_text(
        context,
        cli,
        [
            "wait-pane", "-t", interactive_tab,
            "--contains", submission_marker,
            "--submit-complete", "--timeout-ms", "5000"
        ]
    );
    let committed_receipt = command_json(
        context,
        cli,
        submission_args,
        false
    );
    require(
        committed_receipt.outcome == "committed"
            && committed_receipt.wait == ()
            && committed_receipt.after_position.sequence
                > accepted_receipt.after_position.sequence,
        "server_smoke_receipt_completion"
    );

    print("STEP follow server-owned events into terminal post-state");
    let note = "server-note-" + context.run_id;
    command_text(
        context,
        cli,
        ["set-tab-note", "-t", tab_id, note]
    );
    bootstrap = command_json(context, cli, ["ui-bootstrap"], false);
    let deltas = command_json(
        context,
        cli,
        [
            "ui-deltas",
            "--epoch", hello.position.server_epoch,
            "--after", "" + hello.position.sequence,
            "--limit", "64"
        ],
        false
    );
    let tab = find_by_id(
        bootstrap.tabs,
        tab_id,
        "server_smoke_bootstrap_tab_missing"
    );
    let post_state = find_by_id(
        deltas.tab_updates,
        tab_id,
        "server_smoke_delta_tab_missing"
    );
    require(
        hello.server_pid == server.id
            && bootstrap.server_pid == server.id
            && tab.note == note
            && post_state.note == note
            && count_tab_events(
                deltas.events,
                tab_id,
                "terminal.output"
            ) > 0
            && deltas.complete
            && !deltas.truncated,
        "server_smoke_causal_post_state"
    );

    print("STEP persist and stop server without a GUI process");
    command_text(context, cli, ["save-workspace"]);
    require(
        std::fs::exists(context.workspace_path),
        "server_smoke_workspace_missing"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "server.headless-authority"
    );
    command_text(context, cli, ["shutdown"]);
    let server_output = wait_for_exit(
        server,
        "server_smoke_shutdown_timeout"
    );
    require(
        server_output.success,
        "server_smoke_server_exit_failure"
    );
    print(
        "PASS: headless server owns PTY, parser, workspace, events, and no HWND"
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
