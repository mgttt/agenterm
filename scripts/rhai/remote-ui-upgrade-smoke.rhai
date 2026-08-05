// Prove real GUI byte upgrade and rollback against one stable server and PTY.
//
// Args:
//   --list-evidence
// or:
//   REPO PRIOR_GUI_EXE NEXT_GUI_EXE CLI_EXE SERVER_EXE [NEXT_GUI_OVERRIDE]

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = ["ui.same-server-upgrade-rollback"];

if args.len == 1 && args[0] == "--list-evidence" {
    print("ui.same-server-upgrade-rollback");
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

fn command_json(context, cli, arguments) {
    let output = command(context, cli, arguments, false);
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
            throw "remote_upgrade_server_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_upgrade_server_ready_timeout";
}

fn wait_for_exit(child, timeout_attempts, code) {
    for attempt in 0..timeout_attempts {
        if child.state == "exited" {
            return child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn start_upgrade_ui(context, cli, executable, label) {
    let gui_command = test_harness::configured_command(
        context,
        executable,
        ["--ui-client", "--no-activate", "--address", context.address]
    );
    let gui = gui_command.start();
    for attempt in 0..480 {
        let lease_probe = test_harness::run_cli(
            context,
            cli,
            ["ui-lease", "status"],
            false,
            []
        );
        let facts = gui.platform_facts;
        if lease_probe.success
                && facts.top_level_window_supported
                && facts.top_level_window_present
                && facts.top_level_window_id != 0 {
            let lease = rhai::json::parse(lease_probe.stdout);
            if lease.attached && lease.client_pid == gui.id {
                return #{
                    child: gui,
                    lease: lease,
                    window_id: facts.top_level_window_id
                };
            }
        }
        if gui.state == "exited" {
            let output = gui.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw label + "_gui_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw label + "_gui_lease_timeout";
}

fn stop_upgrade_ui_keeping_server(context, cli, ui) {
    let modal = command_json(
        context,
        cli,
        ["ui-action", "close-window"]
    );
    require(
        modal.modal.kind == "confirm-window-close"
            && modal.modal.default_action == "keep-server-running"
            && modal.modal.actions.contains("keep-server-running")
            && modal.modal.actions.contains("stop-server-and-exit")
            && modal.modal.actions.contains("cancel"),
        "remote_upgrade_close_contract"
    );
    let detached = command_json(
        context,
        cli,
        ["ui-action", "keep-server-running"]
    );
    require(
        !detached.window.visible
            && detached.window.detached
            && !detached.layout.composer.visible,
        "remote_upgrade_detached_ui_facts"
    );
    let output = wait_for_exit(
        ui.child,
        200,
        "remote_upgrade_gui_exit_timeout"
    );
    require(output.success, "remote_upgrade_gui_exit_failure");
    for attempt in 0..200 {
        let lease = command_json(
            context,
            cli,
            ["ui-lease", "status"]
        );
        if !lease.attached {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_upgrade_lease_detach_timeout";
}

fn get_upgrade_state(context, cli, tab_id) {
    let bootstrap = command_json(context, cli, ["ui-bootstrap"]);
    #{
        bootstrap: bootstrap,
        tab: find_by_id(
            bootstrap.tabs,
            tab_id,
            "remote_upgrade_stable_tab_missing"
        )
    }
}

require(
    args.len == 5 || args.len == 6,
    "expected: REPO PRIOR_GUI_EXE NEXT_GUI_EXE CLI_EXE SERVER_EXE [NEXT_GUI_OVERRIDE]"
);
let repo = std::path::absolute(args[0]).display;
let prior_gui_exe = std::path::absolute(args[1]).display;
let next_gui_exe = std::path::absolute(
    if args.len == 6 { args[5] } else { args[2] }
).display;
let cli = std::path::absolute(args[3]).display;
let server_exe = std::path::absolute(args[4]).display;
for path in [prior_gui_exe, next_gui_exe, cli, server_exe] {
    require(
        std::fs::exists(path),
        "remote_upgrade_input_missing:" + path
    );
}
require(
    rhai::crypto::sha256_file(prior_gui_exe)
        != rhai::crypto::sha256_file(next_gui_exe),
    "remote_upgrade_requires_distinct_gui_bytes"
);

let context = test_harness::new_context(repo, "remote-ui-upgrade");
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP start one stable headless server");
    let server_command = test_harness::configured_command(
        context,
        server_exe,
        ["server", "--address", context.address]
    );
    let server = server_command.start();
    owned_children.push(server);
    wait_for_server(context, cli, server);

    let baseline = command_json(context, cli, ["ui-bootstrap"]);
    let tab_id = baseline.active_tab_id;
    let baseline_tab = find_by_id(
        baseline.tabs,
        tab_id,
        "remote_upgrade_baseline_tab_missing"
    );
    require(
        baseline.server_pid == server.id
            && baseline_tab.process_id != (),
        "remote_upgrade_baseline_identity"
    );
    let server_pid = baseline.server_pid;
    let server_epoch = baseline.server_epoch;
    let pty_pid = baseline_tab.process_id;
    let draft = "upgrade-draft-" + context.run_id;
    command_text(
        context,
        cli,
        ["set-composer", "-t", tab_id, draft]
    );

    print("STEP connect the prior GUI and reject a competing startup");
    let prior = start_upgrade_ui(
        context,
        cli,
        prior_gui_exe,
        "remote_upgrade_prior"
    );
    owned_children.push(prior.child);
    let prior_identity = rhai::json::stringify(prior.lease.client_build);
    require(
        prior_identity != ""
            && prior.lease.client_build.protocol_version == 1,
        "remote_upgrade_prior_build_identity"
    );
    let prior_pid = prior.child.id;
    let prior_window_id = prior.window_id;

    let conflict_command = test_harness::configured_command(
        context,
        next_gui_exe,
        ["--ui-client", "--no-activate", "--address", context.address]
    );
    let conflict = conflict_command.start();
    owned_children.push(conflict);
    let conflict_output = wait_for_exit(
        conflict,
        200,
        "remote_upgrade_competing_gui_timeout"
    );
    require(
        !conflict_output.success,
        "remote_upgrade_competing_gui_succeeded"
    );
    let conflict_lease = command_json(
        context,
        cli,
        ["ui-lease", "status"]
    );
    let healthy_after_conflict = get_upgrade_state(
        context,
        cli,
        tab_id
    );
    require(
        conflict_lease.attached
            && conflict_lease.client_pid == prior_pid
            && healthy_after_conflict.bootstrap.server_pid == server_pid
            && healthy_after_conflict.tab.process_id == pty_pid,
        "remote_upgrade_competing_gui_disturbed_state"
    );

    print("STEP stream output while replacing the prior GUI");
    let stream_prefix = "UPGRADE_STREAM_" + context.run_id;
    let stream_command =
        "for /L %i in (1,1,12) do @(echo " + stream_prefix +
        "_%i & ping -n 2 127.0.0.1 >nul)";
    command_text(
        context,
        cli,
        ["send-keys", "-t", tab_id, "-l", stream_command]
    );
    command_text(
        context,
        cli,
        ["send-keys", "-t", tab_id, "Enter"]
    );
    command_text(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", stream_prefix + "_2",
            "--timeout-ms", "30000"
        ]
    );
    stop_upgrade_ui_keeping_server(context, cli, prior);

    print("STEP connect the next GUI to the unchanged server");
    let next = start_upgrade_ui(
        context,
        cli,
        next_gui_exe,
        "remote_upgrade_next"
    );
    owned_children.push(next.child);
    let next_identity = rhai::json::stringify(next.lease.client_build);
    require(
        next_identity != prior_identity
            && next.child.id != prior_pid
            && next.window_id != prior_window_id,
        "remote_upgrade_next_identity_unchanged"
    );
    command_text(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", stream_prefix + "_8",
            "--timeout-ms", "30000"
        ]
    );
    let next_state = get_upgrade_state(context, cli, tab_id);
    require(
        next_state.bootstrap.server_pid == server_pid
            && next_state.bootstrap.server_epoch == server_epoch
            && next_state.bootstrap.active_tab_id == tab_id
            && next_state.tab.process_id == pty_pid
            && next_state.tab.composer.text == draft
            && next_state.tab.working_context.cwd
                == baseline_tab.working_context.cwd
            && next_state.tab.working_context.proxy_configured
                == baseline_tab.working_context.proxy_configured,
        "remote_upgrade_next_lost_state"
    );

    print("STEP reject an incompatible hello without ending live state");
    let incompatible = command_json(
        context,
        cli,
        [
            "ui-hello", "--minimum", "2", "--maximum", "2",
            "--client-id", "incompatible-" + context.run_id
        ]
    );
    require(
        !incompatible.accepted
            && incompatible.compatibility == "client_too_new"
            && server.state == "running",
        "remote_upgrade_incompatible_hello"
    );

    print("STEP roll back to the prior GUI and finish the output stream");
    stop_upgrade_ui_keeping_server(context, cli, next);
    let rollback = start_upgrade_ui(
        context,
        cli,
        prior_gui_exe,
        "remote_upgrade_rollback"
    );
    owned_children.push(rollback.child);
    let rollback_identity =
        rhai::json::stringify(rollback.lease.client_build);
    require(
        rollback_identity == prior_identity,
        "remote_upgrade_rollback_identity"
    );
    command_text(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", stream_prefix + "_12",
            "--submit-complete",
            "--timeout-ms", "30000"
        ]
    );
    let final_state = get_upgrade_state(context, cli, tab_id);
    let capture = command_text(
        context,
        cli,
        [
            "capture-pane", "-p", "-t", tab_id,
            "--max-bytes", "65536"
        ]
    );
    require(
        final_state.bootstrap.server_pid == server_pid
            && final_state.bootstrap.server_epoch == server_epoch
            && final_state.bootstrap.active_tab_id == tab_id
            && final_state.tab.process_id == pty_pid
            && final_state.tab.composer.text == draft
            && final_state.tab.screen.max_scrollback
                >= next_state.tab.screen.max_scrollback
            && capture.contains(stream_prefix + "_2")
            && capture.contains(stream_prefix + "_8")
            && capture.contains(stream_prefix + "_12"),
        "remote_upgrade_rollback_lost_state"
    );

    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ui.same-server-upgrade-rollback"
    );
    stop_upgrade_ui_keeping_server(context, cli, rollback);
    command_text(context, cli, ["shutdown"]);
    let server_output = wait_for_exit(
        server,
        400,
        "remote_upgrade_server_shutdown_timeout"
    );
    require(
        server_output.success,
        "remote_upgrade_server_shutdown_failure"
    );
    print(
        "PASS: real GUI bytes upgrade and roll back on one stable server and PTY"
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
