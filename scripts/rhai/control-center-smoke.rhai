// Prove the isolated Control Center's offline contract, live Cockpit
// projection, process reuse, context refresh, no-activate behavior,
// missing-binary failure, stale-owner recovery, server loss/new-epoch refresh,
// renderer-owned PNG evidence, and server/PTY isolation.
//
// Args:
//   --list-evidence
// or:
//   REPO CONTROL_CENTER_EXE
//
// The renderer-owned screenshot command targets the exact registry owner. The
// successful Cockpit PNG is copied out of scratch into dist/evidence so the
// visual proof survives successful harness cleanup.

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "control-center.offline-contract",
    "control-center.live-cockpit",
    "control-center.process-reuse",
    "control-center.context-refresh",
    "control-center.launch-failure",
    "control-center.server-recovery",
    "control-center.no-activate",
    "control-center.typed-close",
    "control-center.process-isolation"
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

fn configured(context, executable, registry, arguments) {
    let command =
        test_harness::configured_command(context, executable, arguments);
    command.env("AGENTERM_CC_REGISTRY_PATH", registry);
    command.timeout(std::time::Duration::from_secs(15));
    command
}

fn offline(context, executable, registry, arguments, code) {
    let output =
        configured(context, executable, registry, arguments).output();
    require(
        output.success,
        code + ":" + output.stdout_text() + output.stderr_text()
    );
    output.stdout_text() + output.stderr_text()
}

fn cli(context, executable, arguments, expected_failure) {
    test_harness::invoke_cli(
        context,
        executable,
        arguments,
        expected_failure,
        []
    )
}

fn cli_text(context, executable, arguments) {
    let output = cli(context, executable, arguments, false);
    output.stdout + output.stderr
}

fn cli_json(context, executable, arguments) {
    let output = cli(context, executable, arguments, false);
    rhai::json::parse(output.stdout + output.stderr)
}

fn wait_for_server(context, cli_executable, server) {
    for attempt in 0..400 {
        let probe = test_harness::run_cli(
            context,
            cli_executable,
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
            throw "control_center_server_early_exit:" +
                output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "control_center_server_ready_timeout";
}

fn wait_for_registry(path, expected_pid, child, code) {
    for attempt in 0..400 {
        if std::fs::exists(path) {
            try {
                let record = rhai::json::parse_file(path);
                if record.schema_version == 2
                        && record.pid == expected_pid
                        && record.process_start_identity != "" {
                    return record;
                }
            } catch (error) {
                // The owner publishes through create-new and may still be
                // writing the bounded record.
            }
        }
        if child.state == "exited" {
            let output = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    throw code + "_registry_timeout";
}

fn wait_for_window_title(child, required, code) {
    for attempt in 0..400 {
        let facts = child.platform_facts;
        let matches = true;
        for token in required {
            if !facts.top_level_window_title.contains(token) {
                matches = false;
            }
        }
        if facts.top_level_window_present && matches {
            return facts;
        }
        if child.state == "exited" {
            throw code + "_early_exit";
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    throw code + "_title_timeout:" +
        child.platform_facts.top_level_window_title;
}

fn wait_for_exit(child, code) {
    for attempt in 0..200 {
        if child.state == "exited" {
            return child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    throw code;
}

fn process_present(pid) {
    for process in std::process::list() {
        if process.id == pid {
            return true;
        }
    }
    false
}

fn wait_process_gone(pid, code) {
    for attempt in 0..200 {
        if !process_present(pid) {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn tab_facts_match(expected, actual) {
    if expected.len != actual.len {
        return false;
    }
    for index in 0..expected.len {
        if expected[index].id != actual[index].id
                || expected[index].index != actual[index].index
                || expected[index].title != actual[index].title
                || expected[index].note != actual[index].note
                || expected[index].dead != actual[index].dead {
            return false;
        }
    }
    true
}

fn prove_pty(context, cli_executable, label) {
    let marker = "AGENTERM_CC_" + label + "_" + context.run_id;
    let tab_id = cli_text(
        context,
        cli_executable,
        [
            "new-window", "-d", "-P", "-F", "#{window_id}",
            "-n", "cc-" + label + "-" + context.run_id,
            "--", "cmd.exe", "/d", "/c", "echo " + marker
        ]
    );
    tab_id.trim();
    require(
        tab_id.starts_with("@"),
        "control_center_" + label + "_stable_tab_id"
    );
    cli_text(
        context,
        cli_executable,
        [
            "wait-pane", "-t", tab_id,
            "--contains", marker,
            "--timeout-ms", "5000"
        ]
    );
    let capture = cli_text(
        context,
        cli_executable,
        ["capture-pane", "-p", "-t", tab_id, "--max-bytes", "16384"]
    );
    require(
        capture.contains(marker),
        "control_center_" + label + "_pty_capture"
    );
    tab_id
}

fn remove_owned_registry(path) {
    let source = path;
    require(
        source.ends_with(".json"),
        "control_center_registry_suffix"
    );
    let stem = source.sub_string(0, source.len - 5);
    for owned_path in [
        path,
        stem + ".window",
        stem + ".focus",
        stem + ".context",
        stem + ".close"
    ] {
        if std::fs::exists(owned_path) {
            std::fs::remove_file(owned_path);
        }
    }
}

require(
    args.len == 2,
    "expected: REPO CONTROL_CENTER_EXE"
);
let repo = std::path::absolute(args[0]).display;
let control_center = std::path::absolute(args[1]).display;
let executable_directory =
    std::path::parent(control_center).display;
let server_executable = std::path::join(
    executable_directory,
    "agenterm-server.exe"
).display;
let cli_executable = std::path::join(
    executable_directory,
    "agenterm-cli.exe"
).display;
require(
    std::fs::exists(control_center),
    "control_center_smoke_executable_missing"
);
require(
    std::fs::exists(server_executable),
    "control_center_smoke_server_missing"
);
require(
    std::fs::exists(cli_executable),
    "control_center_smoke_cli_missing"
);

let context = test_harness::new_context(repo, "control-center");
let registry = std::path::join(
    context.run_directory,
    "control-center-registry.json"
).display;
let cockpit_png = std::path::join(
    std::path::parent(context.evidence_path).display,
    "control-center-live-cockpit.png"
).display;
let durable_evidence_directory = std::path::join(
    repo,
    "dist/evidence"
).display;
let durable_cockpit_png = std::path::join(
    durable_evidence_directory,
    "control-center-live-cockpit.png"
).display;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP a launcher with no sibling Control Center fails closed");
    let missing_cc_directory = std::path::join(
        context.run_directory,
        "missing-control-center"
    ).display;
    std::fs::create_dir_all(missing_cc_directory);
    let isolated_cli = std::path::join(
        missing_cc_directory,
        "agenterm-cli.exe"
    ).display;
    std::fs::copy(cli_executable, isolated_cli);
    let missing_cc = cli(
        context,
        isolated_cli,
        ["control-center", "open", "--no-activate"],
        true
    );
    require(
        missing_cc.exit_code != 0
            && (missing_cc.stderr + missing_cc.stdout)
                .contains("control_center_unavailable")
            && !std::fs::exists(registry),
        "control_center_missing_binary_failure"
    );

    print("STEP informational commands are offline and side-effect free");
    let help = offline(
        context, control_center, registry, ["--help"],
        "control_center_help"
    );
    let version = offline(
        context, control_center, registry, ["--version"],
        "control_center_version"
    );
    version.trim();
    let capabilities = rhai::json::parse(offline(
        context, control_center, registry,
        ["capabilities", "--json"],
        "control_center_capabilities"
    ));
    let snapshot = rhai::json::parse(offline(
        context, control_center, registry,
        ["snapshot", "--json"],
        "control_center_snapshot"
    ));
    require(
        help.contains("AgenTerm Control Center")
            && help.contains("agenterm-cc snapshot")
            && version.starts_with("agenterm-cc ")
            && capabilities.schema_version == 1
            && capabilities.executable == "agenterm-cc"
            && capabilities.role == "isolated_projection"
            && capabilities.public_ui_action == "open-control-center"
            && capabilities.typed_operation == "control-center.open"
            && !capabilities.owns_terminal_authority
            && capabilities.process_reuse
            && capabilities.no_activate
            && capabilities.views.len == 4
            && snapshot.schema_version == 1
            && snapshot.executable == "agenterm-cc"
            && snapshot.process_role == "isolated_projection"
            && snapshot.connected_server == ()
            && snapshot.views.len == 4
            && !std::fs::exists(registry),
        "control_center_offline_contract"
    );
    for view in snapshot.views {
        require(
            view.state == "unavailable" && view.reason != "",
            "control_center_snapshot_truth"
        );
    }
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.offline-contract"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.launch-failure"
    );

    print("STEP start an isolated explicit-loopback server and real PTY");
    let server_command = test_harness::configured_command(
        context,
        server_executable,
        ["--address", context.address]
    );
    let server = server_command.start();
    owned_children.push(server);
    let protocol = wait_for_server(
        context,
        cli_executable,
        server
    );
    require(
        protocol.pid == server.id,
        "control_center_server_pid"
    );
    prove_pty(context, cli_executable, "baseline");
    let bootstrap = cli_json(
        context,
        cli_executable,
        ["ui-bootstrap"]
    );

    print("STEP explicit semantic snapshot has the server's exact facts");
    let live_snapshot = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        [
            "snapshot", "--json",
            "--server-endpoint", context.address,
            "--logical-instance", "main"
        ],
        "control_center_live_snapshot"
    ));
    let live_server = live_snapshot.connected_server;
    let typed_endpoint = "tcp:" + context.address;
    require(
        live_snapshot.server_state == "connected"
            && live_snapshot.server_reason == ()
            && live_server.endpoint == typed_endpoint
            && live_server.logical_instance == "main"
            && live_server.pid == server.id
            && live_server.pid == bootstrap.server_pid
            && live_server.epoch == bootstrap.server_epoch
            && live_server.sequence == bootstrap.position.sequence
            && live_server.active_tab_id == bootstrap.active_tab_id
            && tab_facts_match(bootstrap.tabs, live_server.tabs)
            && live_snapshot.views[0].id == "cockpit"
            && live_snapshot.views[0].state == "available"
            && live_snapshot.views[0].data.pid == live_server.pid
            && live_snapshot.views[0].data.epoch == live_server.epoch
            && live_snapshot.views[0].data.tabs.len
                == live_server.tabs.len
            && live_snapshot.views[1].state == "unavailable"
            && live_snapshot.views[2].state == "unavailable"
            && live_snapshot.views[3].state == "unavailable",
        "control_center_live_snapshot_truth"
    );

    print("STEP live open is nonactivating and renders the same endpoint/tabs");
    let first_command = configured(
        context,
        control_center,
        registry,
        [
            "open", "--no-activate",
            "--server-endpoint", context.address,
            "--logical-instance", "main"
        ]
    );
    let first = first_command.start();
    owned_children.push(first);
    wait_for_registry(
        registry,
        first.id,
        first,
        "control_center_first"
    );
    let first_window = wait_for_window_title(
        first,
        [context.address, "" + live_server.tabs.len + " tabs"],
        "control_center_live"
    );
    require(
        first_window.foreground_window_id == 0
            || !first_window.top_level_window_is_foreground,
        "control_center_no_activate_stole_foreground"
    );
    let running = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["status", "--json"],
        "control_center_live_status"
    ));
    require(
        running.state == "running"
            && running.pid == first.id
            && running.context.endpoint == typed_endpoint
            && running.context.logical_instance == "main",
        "control_center_live_window_context"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.live-cockpit"
    );

    print("STEP capture the exact live native Cockpit without activation");
    let capture = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["screenshot", "--output", cockpit_png, "--json"],
        "control_center_cockpit_screenshot"
    ));
    let rendered = rhai::image::inspect_png(cockpit_png);
    require(
        capture.schema_version == 1
            && capture.executable == "agenterm-cc"
            && capture.state == "captured"
            && capture.renderer == "native"
            && capture.owner_pid == first.id
            && capture.output == cockpit_png
            && capture.width == rendered.width
            && capture.height == rendered.height
            && capture.width >= 500
            && capture.height >= 300
            && capture.bytes >= 1000
            && capture.sha256.len == 64
            && rendered.samples > 0
            && rendered.luminance > 1.0
            && rendered.luminance < 254.9
            && (
                first.platform_facts.foreground_window_id == 0
                    || !first.platform_facts.top_level_window_is_foreground
            ),
        "control_center_cockpit_png_truth:" +
            rhai::json::stringify(capture)
    );
    std::fs::create_dir_all(durable_evidence_directory);
    std::fs::copy(cockpit_png, durable_cockpit_png);
    require(
        std::fs::metadata(durable_cockpit_png).len == capture.bytes,
        "control_center_durable_png_copy"
    );
    print(
        "Control Center screenshot: " + capture.width + "x" +
            capture.height + ", " + capture.bytes + " bytes, " +
            durable_cockpit_png
    );

    print("STEP reuse keeps PID and refreshes away from and back to live context");
    let reuse_started =
        std::time::SystemTime::now().unix_millis;
    offline(
        context,
        control_center,
        registry,
        [
            "open", "--no-activate",
            "--server-endpoint", "127.0.0.1:1",
            "--logical-instance", "dev"
        ],
        "control_center_reuse_unavailable"
    );
    let reuse_elapsed =
        std::time::SystemTime::now().unix_millis - reuse_started;
    let reused_record = rhai::json::parse_file(registry);
    require(
        reused_record.pid == first.id
            && first.state == "running"
            && reuse_elapsed < 2000,
        "control_center_process_reuse"
    );
    let unavailable_window = wait_for_window_title(
        first,
        ["server_unreachable"],
        "control_center_unavailable_refresh"
    );
    require(
        unavailable_window.foreground_window_id == 0
            || !unavailable_window.top_level_window_is_foreground,
        "control_center_reuse_stole_foreground"
    );
    let unavailable = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["snapshot", "--json"],
        "control_center_unavailable_snapshot"
    ));
    require(
        unavailable.server_state == "unavailable"
            && unavailable.server_reason == "server_unreachable"
            && unavailable.connected_server == (),
        "control_center_unavailable_context_truth"
    );

    offline(
        context,
        control_center,
        registry,
        [
            "open", "--no-activate",
            "--server-endpoint", context.address,
            "--logical-instance", "main"
        ],
        "control_center_reuse_live"
    );
    let refreshed_window = wait_for_window_title(
        first,
        [context.address, "" + live_server.tabs.len + " tabs"],
        "control_center_live_refresh"
    );
    let refreshed = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["snapshot", "--json"],
        "control_center_refreshed_snapshot"
    ));
    require(
        (
            refreshed_window.foreground_window_id == 0
                || !refreshed_window.top_level_window_is_foreground
        )
            && refreshed.server_state == "connected"
            && refreshed.connected_server.pid == live_server.pid
            && refreshed.connected_server.epoch == live_server.epoch
            && tab_facts_match(
                live_server.tabs,
                refreshed.connected_server.tabs
            )
            && first.state == "running",
        "control_center_context_refresh"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.process-reuse"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.no-activate"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.context-refresh"
    );

    print("STEP server loss is explicit and a new epoch refreshes in place");
    cli_text(context, cli_executable, ["save-workspace"]);
    let prior_server_pid = server.id;
    let prior_server_epoch = live_server.epoch;
    cli_text(context, cli_executable, ["shutdown"]);
    let prior_server_output = wait_for_exit(
        server,
        "control_center_prior_server_shutdown_timeout"
    );
    require(
        prior_server_output.success,
        "control_center_prior_server_exit_failure:" +
            prior_server_output.stderr_text()
    );
    wait_process_gone(
        prior_server_pid,
        "control_center_prior_server_process_orphan"
    );
    offline(
        context,
        control_center,
        registry,
        [
            "open", "--no-activate",
            "--server-endpoint", context.address,
            "--logical-instance", "main"
        ],
        "control_center_server_loss_refresh"
    );
    let loss_window = wait_for_window_title(
        first,
        ["server_unreachable"],
        "control_center_server_loss"
    );
    let lost = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["snapshot", "--json"],
        "control_center_server_loss_snapshot"
    ));
    require(
        first.state == "running"
            && (
                loss_window.foreground_window_id == 0
                    || !loss_window.top_level_window_is_foreground
            )
            && lost.server_state == "unavailable"
            && lost.server_reason == "server_unreachable"
            && lost.connected_server == (),
        "control_center_server_loss_truth"
    );

    let restarted_server_command = test_harness::configured_command(
        context,
        server_executable,
        ["--address", context.address]
    );
    let restarted_server = restarted_server_command.start();
    owned_children.push(restarted_server);
    let restarted_protocol = wait_for_server(
        context,
        cli_executable,
        restarted_server
    );
    offline(
        context,
        control_center,
        registry,
        [
            "open", "--no-activate",
            "--server-endpoint", context.address,
            "--logical-instance", "main"
        ],
        "control_center_server_restart_refresh"
    );
    let restart_window = wait_for_window_title(
        first,
        [context.address],
        "control_center_server_restart"
    );
    let restarted = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["snapshot", "--json"],
        "control_center_server_restart_snapshot"
    ));
    require(
        first.state == "running"
            && (
                restart_window.foreground_window_id == 0
                    || !restart_window.top_level_window_is_foreground
            )
            && restarted.server_state == "connected"
            && restarted.connected_server.pid == restarted_server.id
            && restarted_protocol.pid == restarted_server.id
            && restarted.connected_server.epoch
                != prior_server_epoch,
        "control_center_server_restart_identity"
    );
    prove_pty(context, cli_executable, "after-restart");
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.server-recovery"
    );

    print("STEP typed close targets only the exact reused projection owner");
    let first_pid = first.id;
    let close = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["close", "--json"],
        "control_center_close"
    ));
    require(
        (close.state == "closed" || close.state == "close_requested")
            && close.pid == first_pid,
        "control_center_typed_close"
    );
    let first_output =
        wait_for_exit(first, "control_center_first_close_timeout");
    require(
        first_output.success,
        "control_center_typed_close_failed:" + first_output.stderr_text()
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.typed-close"
    );
    require(
        process_present(restarted_server.id)
            && cli_json(
                context,
                cli_executable,
                ["protocol-info", "--running"]
            ).pid == restarted_server.id,
        "control_center_close_affected_server"
    );
    prove_pty(context, cli_executable, "after-close");

    print("STEP a killed projection leaves server/PTY alive and is replaceable");
    let replacement_command = configured(
        context,
        control_center,
        registry,
        [
            "open", "--no-activate",
            "--server-endpoint", context.address,
            "--logical-instance", "main"
        ]
    );
    let replacement = replacement_command.start();
    owned_children.push(replacement);
    wait_for_registry(
        registry,
        replacement.id,
        replacement,
        "control_center_replacement"
    );
    let replacement_window = wait_for_window_title(
        replacement,
        [context.address],
        "control_center_replacement"
    );
    require(
        replacement.id != first_pid
            && (
                replacement_window.foreground_window_id == 0
                    || !replacement_window.top_level_window_is_foreground
            ),
        "control_center_stale_owner_recovery"
    );
    let killed_pid = replacement.id;
    replacement.kill();
    wait_for_exit(
        replacement,
        "control_center_replacement_kill_timeout"
    );
    wait_process_gone(
        killed_pid,
        "control_center_replacement_process_orphan"
    );
    require(
        process_present(restarted_server.id)
            && cli_json(
                context,
                cli_executable,
                ["protocol-info", "--running"]
            ).pid == restarted_server.id,
        "control_center_kill_affected_server"
    );
    prove_pty(context, cli_executable, "after-kill");
    let recovered_command = configured(
        context,
        control_center,
        registry,
        [
            "open", "--no-activate",
            "--server-endpoint", context.address,
            "--logical-instance", "main"
        ]
    );
    let recovered = recovered_command.start();
    owned_children.push(recovered);
    wait_for_registry(
        registry,
        recovered.id,
        recovered,
        "control_center_recovered"
    );
    require(
        recovered.id != killed_pid,
        "control_center_killed_owner_not_replaced"
    );
    let recovered_close = rhai::json::parse(offline(
        context,
        control_center,
        registry,
        ["close", "--json"],
        "control_center_recovered_close"
    ));
    require(
        recovered_close.pid == recovered.id,
        "control_center_recovered_close_owner"
    );
    let recovered_output = wait_for_exit(
        recovered,
        "control_center_recovered_close_timeout"
    );
    require(
        recovered_output.success,
        "control_center_recovered_close_failed"
    );
    wait_process_gone(
        recovered.id,
        "control_center_recovered_process_orphan"
    );
    remove_owned_registry(registry);
    require(
        !std::fs::exists(registry),
        "control_center_owned_registry_orphan"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.process-isolation"
    );

    print("STEP stop the isolated server through its public CLI");
    cli_text(context, cli_executable, ["save-workspace"]);
    require(
        std::fs::exists(context.workspace_path),
        "control_center_workspace_missing"
    );
    cli_text(context, cli_executable, ["shutdown"]);
    let server_output = wait_for_exit(
        restarted_server,
        "control_center_server_shutdown_timeout"
    );
    require(
        server_output.success,
        "control_center_server_exit_failure:" +
            server_output.stderr_text()
    );
    wait_process_gone(
        restarted_server.id,
        "control_center_server_process_orphan"
    );
    print(
        "PASS: Control Center launch failure, live Cockpit facts, " +
            "no-activate reuse, server loss/new-epoch recovery, " +
            "failure isolation, and orphan-free cleanup"
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
try {
    remove_owned_registry(registry);
} catch (error) {
    if completion_failure == "" {
        completion_failure = error.to_string();
    }
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
