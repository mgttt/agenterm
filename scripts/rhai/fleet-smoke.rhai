// Prove Fleet discovery, event causality, scoped launch context, Mux safety,
// restart truth, and the bounded journal under concurrent typed mutations.
//
// Args:
//   --list-evidence
// or:
//   REPO CLI_EXE MUX_EXE [--skip-event-load|--event-load]

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "fleet.codex-launcher",
    "fleet.event-transition-catalog",
    "fleet.instance-discovery",
    "fleet.mux-frontend",
    "fleet.tab-environment",
    "fleet.upgrade-truth"
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

fn run_program(
    context,
    program,
    arguments,
    expected_failure,
    remove_address,
    workspace
) {
    let command = test_harness::configured_command(
        context,
        program,
        arguments
    );
    if remove_address {
        command.env_remove("AGENTERM_IPC_ADDRESS");
    }
    if workspace != "" {
        command.env("AGENTERM_WORKSPACE_PATH", workspace);
    }
    let output = command.output();
    test_harness::append_command_record(
        context,
        arguments,
        output,
        expected_failure,
        []
    );
    if expected_failure {
        require(
            !output.success,
            "fleet_command_unexpected_success:" +
                program + ":" + rhai::json::stringify(arguments)
        );
    } else {
        require(
            output.success,
            "fleet_command_failed:" + program + ":" +
                rhai::json::stringify(arguments) + ":" +
                output.stderr_text()
        );
    }
    #{
        success: output.success,
        exit_code: output.exit_code,
        stdout: output.stdout_text(),
        stderr: output.stderr_text()
    }
}

fn run_default(context, program, arguments) {
    run_program(
        context,
        program,
        arguments,
        false,
        false,
        ""
    )
}

fn run_failure(context, program, arguments) {
    run_program(
        context,
        program,
        arguments,
        true,
        false,
        ""
    )
}

fn run_without_address(context, program, arguments) {
    run_program(
        context,
        program,
        arguments,
        false,
        true,
        ""
    )
}

fn fail_without_address(context, program, arguments) {
    run_program(
        context,
        program,
        arguments,
        true,
        true,
        ""
    )
}

fn text(context, program, arguments) {
    run_default(context, program, arguments).stdout
}

fn json(context, program, arguments) {
    rhai::json::parse(text(context, program, arguments))
}

fn json_without_address(context, program, arguments) {
    rhai::json::parse(
        run_without_address(context, program, arguments).stdout
    )
}

fn find_by_address(values, address) {
    for value in values {
        if value.address == address {
            return value;
        }
    }
    throw "fleet_address_missing:" + address;
}

fn find_tab_by_name(snapshot, name) {
    for tab in snapshot.tabs {
        if tab.name == name {
            return tab;
        }
    }
    throw "fleet_tab_name_missing:" + name;
}

fn find_tab_by_id(snapshot, id) {
    for tab in snapshot.tabs {
        if tab.id == id {
            return tab;
        }
    }
    throw "fleet_tab_id_missing:" + id;
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
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    throw code + ":" + pid;
}

fn start_waiter(context, cli, epoch, after, kind, timeout_ms) {
    let arguments = [
        "wait-events",
        "--epoch", epoch,
        "--after", after.to_string(),
        "--kind", kind,
        "--timeout-ms", timeout_ms.to_string()
    ];
    let command = test_harness::configured_command(
        context,
        cli,
        arguments
    );
    let process_timeout = if timeout_ms >= 55000 {
        60000
    } else {
        timeout_ms + 5000
    };
    command.timeout(std::time::Duration::from_millis(process_timeout));
    #{ arguments: arguments, child: command.start() }
}

fn complete_waiter(context, waiter) {
    let output = waiter.child.wait_with_output(
        std::time::Duration::from_secs(15)
    );
    test_harness::append_command_record(
        context,
        waiter.arguments,
        output,
        false,
        []
    );
    require(
        output.success,
        "fleet_event_waiter_failed:" + output.stderr_text()
    );
    rhai::json::parse(output.stdout_text())
}

fn reserve_addresses() {
    let first = std::net::TcpListener::bind("127.0.0.1:0");
    let second = std::net::TcpListener::bind("127.0.0.1:0");
    let values = [first.local_addr, second.local_addr];
    first = ();
    second = ();
    require(values[0] != values[1], "fleet_reserved_address_collision");
    values
}

fn cleanup_server(context, cli, address) {
    try {
        run_program(
            context,
            cli,
            ["--address", address, "kill-server"],
            false,
            false,
            ""
        );
    } catch (error) {
        // A failed journey may have already stopped this exact server.
    }
}

require(
    args.len >= 3 && args.len <= 5,
    "expected: REPO CLI_EXE MUX_EXE " +
        "[--skip-event-load|--event-load]"
);
let repo = std::path::absolute(args[0]).display;
let cli = std::path::absolute(args[1]).display;
let mux = std::path::absolute(args[2]).display;
for path in [cli, mux] {
    require(std::fs::exists(path), "fleet_executable_missing:" + path);
}
let run_event_load = true;
for index in 3..args.len {
    require(
        args[index] == "--skip-event-load"
            || args[index] == "--event-load",
        "fleet_unknown_mode:" + args[index]
    );
    if args[index] == "--skip-event-load" {
        run_event_load = false;
    } else {
        run_event_load = true;
    }
}

let context = test_harness::new_context(repo, "fleet");
let addresses = reserve_addresses();
let explicit_address = addresses[0];
let second_address = addresses[1];
let explicit_workspace = std::path::join(
    std::path::parent(context.workspace_path).display,
    "explicit.json"
).display;
let second_workspace = std::path::join(
    std::path::parent(context.workspace_path).display,
    "second.json"
).display;
let environment_name = "env-" + std::process::id();
let agent_name = "codex-" + std::process::id();
let role = "reviewer-" + std::process::id();
let proxy = "http://127.0.0.1:" +
    (30000 + std::process::id() % 1000);
let owned_children = [];
let active_load_children = [];
let succeeded = false;
let failure = "";

try {
    let version_output = text(context, cli, ["--version"]);
    version_output.trim();
    let version_fields = version_output.split(" ");
    let expected_version = version_fields[version_fields.len - 1];

    print("STEP control help and option errors are offline and fail fast");
    require(
        text(context, cli, ["--help"]).contains("AgenTerm CLI"),
        "fleet_top_help_missing"
    );
    let catalog = text(context, cli, ["list-commands"]);
    for source_line in catalog.split("\n") {
        let line = source_line;
        line.trim();
        if line == "" {
            continue;
        }
        let pieces = line.split(" (");
        require(
            pieces.len == 1 || pieces.len == 2,
            "fleet_command_catalog_line:" + line
        );
        let names = [pieces[0]];
        if pieces.len == 2 {
            let alias = pieces[1];
            require(alias.ends_with(")"), "fleet_catalog_alias:" + line);
            alias = alias.sub_string(0, alias.len - 1);
            names.push(alias);
        }
        for name in names {
            require(
                text(context, cli, [name, "--help"]).starts_with(
                    "Usage: agenterm-cli "
                ),
                "fleet_command_help:" + name
            );
        }
    }
    let option_started = std::time::SystemTime::now().unix_millis;
    let option_error = run_failure(
        context,
        cli,
        ["-a", context.address, "list-windows"]
    );
    require(
        (option_error.stdout + option_error.stderr).contains(
            "unknown global option"
        )
            && (option_error.stdout + option_error.stderr).contains(
                "--address HOST:PORT"
            )
            && std::time::SystemTime::now().unix_millis
                - option_started <= 2000,
        "fleet_option_error_not_fast_or_actionable"
    );

    print("STEP zero-instance discovery retains stale rows until confirmed cleanup");
    let dead_record = std::path::join(
        context.instance_directory,
        "4294967295.json"
    ).display;
    rhai::runtime::atomic_write(
        dead_record,
        rhai::json::stringify_pretty(#{
            schema_version: 1,
            pid: 4294967295,
            address: "127.0.0.1:59999",
            version: expected_version,
            session: "stale",
            workspace_path: "stale-workspace.json",
            started_at_unix_ms: 1
        }) + "\n"
    );
    let empty_servers = json_without_address(
        context,
        cli,
        ["server-list", "--json"]
    );
    require(
        empty_servers.len == 1
            && empty_servers[0].classification == "stale"
            && empty_servers[0].cleanup_eligible
            && std::fs::exists(dead_record),
        "fleet_dead_registration_not_retained"
    );
    let cleaned_servers = json_without_address(
        context,
        cli,
        ["server-list", "--json", "--prune"]
    );
    require(
        cleaned_servers.len == 1
            && cleaned_servers[0].cleanup_receipt.schema_version == 1
            && cleaned_servers[0].cleanup_receipt.operation
                == "registration.cleanup"
            && cleaned_servers[0].cleanup_receipt.result == "deleted"
            && !std::fs::exists(dead_record),
        "fleet_dead_registration_cleanup_not_confirmed"
    );
    let empty_error = fail_without_address(
        context,
        cli,
        ["list-windows"]
    );
    let empty_text = empty_error.stdout + empty_error.stderr;
    require(
        empty_text.contains("AgenTerm server is not running")
            && empty_text.contains("IPC transport")
            && empty_text.contains("pipe:")
            && !empty_text.contains("tcp:127.0.0.1"),
        "fleet_zero_instance_error"
    );

    print("STEP mux compatibility is discoverable without a server");
    let compatibility = rhai::json::parse(
        run_without_address(
            context,
            mux,
            ["compatibility", "--json"]
        ).stdout
    );
    let split_unsupported = false;
    for item in compatibility.explicitly_unsupported {
        if item.name == "split-window" {
            split_unsupported = true;
        }
    }
    require(
        compatibility.frontend == "agenterm-mux"
            && compatibility.model.window == "tab"
            && split_unsupported
            && !compatibility.supported.contains("split-window"),
        "fleet_mux_compatibility_model"
    );
    let mux_commands = run_without_address(
        context,
        mux,
        ["list-commands"]
    ).stdout;
    require(
        mux_commands.contains("split-window (unsupported:")
            && !mux_commands.contains("\nsplit-window\n"),
        "fleet_mux_command_registry"
    );
    let compatibility_text = run_without_address(
        context,
        mux,
        ["compatibility"]
    ).stdout;
    require(
        compatibility_text.contains("unsupported: split-window"),
        "fleet_mux_text_compatibility"
    );
    let split_failure = fail_without_address(
        context,
        mux,
        ["split-window"]
    );
    require(
        (split_failure.stdout + split_failure.stderr).contains(
            "unsupported"
        ),
        "fleet_mux_split_not_explicit"
    );
    run_program(
        context,
        mux,
        ["--address", "0.0.0.0:42000", "compatibility"],
        true,
        true,
        ""
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "fleet.mux-frontend"
    );

    print("STEP explicit CLI address autostarts the requested server");
    run_program(
        context,
        cli,
        [
            "--address", explicit_address,
            "new-window", "-d", "-n",
            "explicit-" + std::process::id(),
            "--", "cmd.exe", "/d", "/c", "echo explicit"
        ],
        false,
        true,
        explicit_workspace
    );
    let explicit_instances = json_without_address(
        context,
        cli,
        ["list-instances", "--json"]
    );
    let explicit_instance = find_by_address(
        explicit_instances,
        explicit_address
    );
    require(
        explicit_instance.status == "running"
            && std::path::absolute(
                explicit_instance.workspace_path
            ).display.to_lower()
                == std::path::absolute(
                    explicit_workspace
                ).display.to_lower(),
        "fleet_explicit_autostart_identity"
    );
    print("STEP server-kill aliases the existing destructive operation");
    run_program(
        context,
        cli,
        ["--address", explicit_address, "server-kill"],
        false,
        true,
        explicit_workspace
    );

    print("STEP tab-scoped environment and reserved AgenTerm context");
    text(
        context,
        cli,
        [
            "new-window", "-d", "-n", environment_name,
            "-e", "FLEET_ROLE=" + role,
            "--", "cmd.exe", "/d", "/c",
            "set FLEET_ROLE&set AGENTERM_TAB_ID&set AGENTERM_SESSION"
        ]
    );
    text(
        context,
        cli,
        ["wait-pane", "-t", environment_name, "--dead",
         "--timeout-ms", "10000"]
    );

    print("STEP live servers are discoverable and explicitly targetable");
    let instances = json_without_address(
        context,
        cli,
        ["list-instances", "--json"]
    );
    let servers = json_without_address(
        context,
        cli,
        ["server-list", "--json"]
    );
    let instance = find_by_address(instances, context.address);
    let server = find_by_address(servers, context.address);
    let complete_identity = server.upgrade.status == "same";
    if complete_identity {
        require(
            server.running_identity.git_commit
                == server.staged_identity.git_commit,
            "fleet_complete_identity_commit_mismatch"
        );
    } else {
        require(
            server.upgrade.status == "unknown"
                && server.upgrade.reasons.len == 1
                && server.upgrade.reasons[0].kind == "missing_fields"
                && server.upgrade.reasons[0].running.len == 4
                && server.upgrade.reasons[0].staged.len == 4
                && server.running_identity.version
                    == server.staged_identity.version
                && server.running_identity.protocol_version
                    == server.staged_identity.protocol_version,
            "fleet_incomplete_identity_not_truthful"
        );
        for field in [
            "git_commit", "profile", "cargo_lock_sha256",
            "artifact_manifest_sha256"
        ] {
            require(
                server.upgrade.reasons[0].running.contains(field)
                    && server.upgrade.reasons[0].staged.contains(field),
                "fleet_incomplete_identity_missing_reason:" + field
            );
        }
    }
    require(
        instance.status == "running"
            && server.status == instance.status
            && server.pid == instance.pid
            && server.workspace_path == instance.workspace_path
            && !server.window_visible
            && server.window_detached
            && server.window_state == "detached"
            && server.modal_kind == ()
            && server.event_epoch != ""
            && server.event_sequence != ()
            && instance.version == expected_version
            && std::path::absolute(
                instance.workspace_path
            ).display.to_lower()
                == std::path::absolute(
                    context.workspace_path
                ).display.to_lower(),
        "fleet_instance_server_truth"
    );
    let protocol = json(
        context,
        cli,
        ["protocol-info", "--running"]
    );
    require(
        protocol.features.instance_discovery
            && protocol.identity_scope == "running_host"
            && protocol.pid == server.pid
            && (
                (
                    complete_identity
                        && protocol.build_identity_complete
                        && protocol.build_identity.git_commit
                            == server.running_identity.git_commit
                )
                || (
                    !complete_identity
                        && !protocol.build_identity_complete
                        && protocol.agenterm_version
                            == server.running_identity.version
                )
            ),
        "fleet_running_protocol_identity"
    );
    let event_kinds = #{};
    require(
        protocol.event_catalog.schema_version == 2
            && protocol.features.typed_events
            && protocol.event_catalog.events.len == 29,
        "fleet_event_catalog_shape"
    );
    for event in protocol.event_catalog.events {
        require(
            event.kind != ""
                && event.state_path != ""
                && event.payload != ""
                && (event.scope == "server" || event.scope == "tab")
                && event.since != ""
                && !event_kinds.contains(event.kind),
            "fleet_event_catalog_entry:" + event.kind
        );
        event_kinds[event.kind] = true;
    }
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "fleet.upgrade-truth"
    );

    print("STEP server-scoped event transition matches its post-state");
    let note_baseline = json(context, cli, ["ui-snapshot"]);
    let note_tab = find_tab_by_name(note_baseline, environment_name);
    require(
        note_tab.id.starts_with("@"),
        "fleet_note_stable_id"
    );
    let next_note = "fleet-event-" + std::process::id();
    let numeric_tab_id = rhai::json::parse(
        note_tab.id.sub_string(1)
    );
    text(
        context,
        cli,
        ["set-tab-note", "-t", note_tab.id, next_note]
    );
    let note_after = json(context, cli, ["ui-snapshot"]);
    let committed_tab = find_tab_by_id(note_after, note_tab.id);
    let note_batch = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", note_baseline.event_position.epoch,
            "--after", note_baseline.event_position.sequence.to_string(),
            "--limit", "32"
        ]
    );
    let matching_note = ();
    for event in note_batch.events {
        if event.kind == "tab.note"
                && event.tab_id == numeric_tab_id {
            matching_note = event;
        }
    }
    require(
        matching_note != ()
            && matching_note.payload.note == next_note
            && committed_tab.note == next_note,
        "fleet_note_event_post_state"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "fleet.instance-discovery"
    );

    let capture = text(
        context,
        cli,
        ["capture-pane", "-p", "-t", environment_name]
    );
    let snapshot = json(context, cli, ["ui-snapshot"]);
    let environment_tab = find_tab_by_name(snapshot, environment_name);
    let tab_inspect = json(
        context,
        cli,
        ["inspect", "-t", environment_tab.id]
    );
    require(
        capture.contains("FLEET_ROLE=" + role)
            && capture.contains("AGENTERM_TAB_ID=" + environment_tab.id)
            && capture.contains("AGENTERM_SESSION=agenterm")
            && tab_inspect.windows[0].environment_names.contains(
                "FLEET_ROLE"
            ),
        "fleet_tab_environment"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "fleet.tab-environment"
    );

    print("STEP snapshot-to-follow handoff is ordered for concurrent readers");
    let follow_baseline = json(context, cli, ["ui-snapshot"]);
    let reader_a = start_waiter(
        context,
        cli,
        follow_baseline.event_position.epoch,
        follow_baseline.event_position.sequence,
        "tab.created",
        10000
    );
    let reader_b = start_waiter(
        context,
        cli,
        follow_baseline.event_position.epoch,
        follow_baseline.event_position.sequence,
        "tab.created",
        10000
    );
    owned_children.push(reader_a.child);
    owned_children.push(reader_b.child);
    let follow_name = "follow-" + std::process::id();
    text(
        context,
        cli,
        [
            "new-window", "-d", "-n", follow_name,
            "--", "cmd.exe", "/d", "/c", "echo follow"
        ]
    );
    let follow_a = complete_waiter(context, reader_a);
    let follow_b = complete_waiter(context, reader_b);
    require(
        follow_a.kind == "tab.created"
            && follow_a.sequence
                > follow_baseline.event_position.sequence
            && follow_a.epoch == follow_baseline.event_position.epoch
            && follow_a.sequence == follow_b.sequence
            && follow_a.tab_id == follow_b.tab_id
            && follow_a.payload.index == follow_b.payload.index,
        "fleet_concurrent_waiter_causality"
    );
    let follow_batch = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", follow_baseline.event_position.epoch,
            "--after",
            follow_baseline.event_position.sequence.to_string(),
            "--limit", "1024"
        ]
    );
    let previous = follow_baseline.event_position.sequence;
    let found_follow = false;
    for event in follow_batch.events {
        require(
            event.sequence > previous,
            "fleet_follow_batch_order"
        );
        previous = event.sequence;
        if event.sequence == follow_a.sequence {
            found_follow = true;
        }
    }
    require(found_follow, "fleet_follow_event_omitted");
    let follow_snapshot = json(context, cli, ["ui-snapshot"]);
    let follow_tab = find_tab_by_id(
        follow_snapshot,
        "@" + follow_a.tab_id
    );
    require(
        follow_tab.name == follow_name
            && follow_tab.index == follow_a.payload.index
            && follow_a.tab_id != (),
        "fleet_follow_event_post_state"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "fleet.event-transition-catalog"
    );

    print("STEP timed out and cancelled waiters leave no client or server residue");
    let timeout_baseline = json(context, cli, ["ui-snapshot"]);
    let timeout_failure = run_failure(
        context,
        cli,
        [
            "wait-events",
            "--epoch", timeout_baseline.event_position.epoch,
            "--after",
            timeout_baseline.event_position.sequence.to_string(),
            "--kind", "never.fleet-timeout",
            "--timeout-ms", "25"
        ]
    );
    require(
        (timeout_failure.stdout + timeout_failure.stderr).contains(
            "\"code\":\"event_wait_timeout\""
        ),
        "fleet_wait_timeout_class"
    );
    let server_before_cancel = find_by_address(
        json_without_address(
            context,
            cli,
            ["server-list", "--json"]
        ),
        context.address
    );
    let cancel_waiter = start_waiter(
        context,
        cli,
        timeout_baseline.event_position.epoch,
        timeout_baseline.event_position.sequence,
        "never.fleet-cancel",
        60000
    );
    owned_children.push(cancel_waiter.child);
    require(
        cancel_waiter.child.state == "running",
        "fleet_cancel_waiter_early_exit"
    );
    cancel_waiter.child.kill();
    cancel_waiter.child.wait_with_output(
        std::time::Duration::from_secs(5)
    );
    let server_after_cancel = find_by_address(
        json_without_address(
            context,
            cli,
            ["server-list", "--json"]
        ),
        context.address
    );
    let after_cancel = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", timeout_baseline.event_position.epoch,
            "--after",
            timeout_baseline.event_position.sequence.to_string()
        ]
    );
    require(
        server_after_cancel.pid == server_before_cancel.pid
            && after_cancel.position.epoch
                == timeout_baseline.event_position.epoch,
        "fleet_cancel_disturbed_server"
    );

    print("STEP a legacy TCP registration never hijacks native main");
    let implicit_failure = fail_without_address(
        context,
        cli,
        ["list-windows", "-F", "#{window_name}"]
    );
    let implicit_text =
        implicit_failure.stdout + implicit_failure.stderr;
    require(
        implicit_text.contains("pipe:")
            && !implicit_text.contains(context.address),
        "fleet_legacy_tcp_hijacked_native_main"
    );

    print("STEP multiple legacy TCP servers still require explicit targeting");
    run_program(
        context,
        cli,
        [
            "--address", second_address,
            "new-window", "-d", "-n",
            "second-" + std::process::id()
        ],
        false,
        true,
        second_workspace
    );
    let native_default = fail_without_address(
        context,
        cli,
        ["list-windows"]
    );
    let native_default_text =
        native_default.stdout + native_default.stderr;
    require(
        native_default_text.contains("pipe:")
            && !native_default_text.contains(context.address)
            && !native_default_text.contains(second_address),
        "fleet_legacy_tcp_changed_native_default"
    );
    let explicit_windows = text(
        context,
        cli,
        ["list-windows", "-F", "#{window_name}"]
    );
    require(
        explicit_windows.contains(environment_name)
            && !explicit_windows.contains(
                "second-" + std::process::id()
            ),
        "fleet_environment_address_priority"
    );
    run_program(
        context,
        cli,
        ["--address", second_address, "shutdown"],
        false,
        false,
        second_workspace
    );

    text(
        context,
        cli,
        [
            "new-session", "--", "cmd.exe", "/d", "/c",
            "echo child", "-s", "hijack"
        ]
    );
    require(
        text(context, cli, ["list-sessions"]).starts_with("agenterm:"),
        "fleet_child_delimiter_changed_session"
    );

    print("STEP supported Codex launcher proxy workflow");
    let agent_command =
        "if defined HTTPS_PROXY echo HTTPS_PROXY_PRESENT&" +
        "set NO_PROXY&set AGENTERM_TAB_ID";
    text(
        context,
        cli,
        [
            "new-agent", "-d", "-n", agent_name,
            "--program", "cmd.exe",
            "--proxy", proxy,
            "--no-proxy", "localhost,127.0.0.1",
            "--", "/d", "/c", agent_command
        ]
    );
    text(
        context,
        cli,
        ["wait-pane", "-t", agent_name, "--dead",
         "--timeout-ms", "10000"]
    );
    let agent_capture = text(
        context,
        cli,
        ["capture-pane", "-p", "-t", agent_name]
    );
    let agent_snapshot = json(context, cli, ["ui-snapshot"]);
    let agent_tab = find_tab_by_name(agent_snapshot, agent_name);
    let agent_inspect = json(
        context,
        cli,
        ["inspect", "-t", agent_tab.id]
    );
    require(
        agent_capture.contains("HTTPS_PROXY_PRESENT")
            && agent_capture.contains(
                "NO_PROXY=localhost,127.0.0.1"
            )
            && agent_capture.contains(
                "AGENTERM_TAB_ID=" + agent_tab.id
            )
            && !agent_capture.contains(
                "--dangerously-bypass-approvals-and-sandbox"
            )
            && agent_inspect.windows[0].environment_names.contains(
                "HTTPS_PROXY"
            ),
        "fleet_codex_proxy_context"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "fleet.codex-launcher"
    );

    print("STEP child arguments cannot escape -- and --yolo is explicit");
    let delimiter_name = "delimiter-" + std::process::id();
    text(
        context,
        cli,
        [
            "new-agent", "-d", "-n", delimiter_name,
            "--program", "cmd.exe", "--yolo", "--",
            "/d", "/c", "echo %cmdcmdline%",
            "--parent", "@999999",
            "--program", "missing-agent.exe",
            "--proxy", "http://should-not-be-an-environment"
        ]
    );
    text(
        context,
        cli,
        ["wait-pane", "-t", delimiter_name, "--dead",
         "--timeout-ms", "10000"]
    );
    let delimiter_capture = text(
        context,
        cli,
        ["capture-pane", "-p", "-t", delimiter_name]
    );
    let delimiter_inspect = json(
        context,
        cli,
        ["inspect", "-t", delimiter_name]
    );
    require(
        delimiter_capture.contains(
            "--dangerously-bypass-approvals-and-sandbox"
        )
            && delimiter_inspect.windows[0].parent_id == ()
            && !delimiter_inspect.windows[0].environment_names.contains(
                "HTTPS_PROXY"
            ),
        "fleet_delimiter_or_yolo_mapping"
    );
    text(context, cli, ["save-workspace"]);
    require(
        !std::fs::read_to_string(context.workspace_path).contains(proxy),
        "fleet_proxy_persisted"
    );

    if run_event_load {
        print("STEP bounded event history reports an explicit gap");
        let gap_baseline = json(context, cli, ["ui-snapshot"]);
        let gap_target = "";
        for tab in gap_baseline.tabs {
            if tab.active {
                gap_target = tab.id;
            }
        }
        require(
            gap_target.starts_with("@"),
            "fleet_gap_target_missing"
        );
        // Preserve the original 16 logical workers × 258 real CLI mutations.
        // Each round owns 16 simultaneous clients; the barrier bounds active
        // processes while retaining concurrent IPC pressure and exact volume.
        for iteration in 0..258 {
            active_load_children = [];
            for worker in 0..16 {
                let arguments = [
                    "set-tab-note", "-t", gap_target,
                    "gap-" + worker + "-" + iteration
                ];
                let command = test_harness::configured_command(
                    context,
                    cli,
                    arguments
                );
                command.timeout(std::time::Duration::from_secs(15));
                let child = command.start();
                active_load_children.push(#{
                    worker: worker,
                    iteration: iteration,
                    arguments: arguments,
                    child: child
                });
            }
            for client in active_load_children {
                let output = client.child.wait_with_output(
                    std::time::Duration::from_secs(15)
                );
                require(
                    output.success,
                    "fleet_event_load_client:" + client.worker +
                        ":" + client.iteration +
                        ":" + output.stderr_text()
                );
            }
        }
        active_load_children = [];
        print("LOAD workers=16 events_per_worker=258 status=completed");
        let gap_failure = run_failure(
            context,
            cli,
            [
                "read-events",
                "--epoch", gap_baseline.event_position.epoch,
                "--after",
                gap_baseline.event_position.sequence.to_string()
            ]
        );
        let ui_gap_failure = run_failure(
            context,
            cli,
            [
                "ui-deltas",
                "--epoch", gap_baseline.event_position.epoch,
                "--after",
                gap_baseline.event_position.sequence.to_string()
            ]
        );
        let gap_text = gap_failure.stdout + gap_failure.stderr;
        let ui_gap_text =
            ui_gap_failure.stdout + ui_gap_failure.stderr;
        require(
            gap_text.contains("\"code\":\"journal_gap\"")
                && gap_text.contains("\"earliest_available\"")
                && gap_text.contains("\"current\"")
                && ui_gap_text.contains("\"code\":\"journal_gap\"")
                && ui_gap_text.contains("\"earliest_available\"")
                && ui_gap_text.contains("\"current\""),
            "fleet_journal_gap_not_typed"
        );
    } else {
        print("SKIP bounded event journal concurrent load");
    }

    print("STEP a restarted server rejects positions from the previous epoch");
    let restart_before = json(context, cli, ["ui-snapshot"]);
    let server_before_restart = find_by_address(
        json_without_address(
            context,
            cli,
            ["server-list", "--json"]
        ),
        context.address
    );
    text(context, cli, ["shutdown"]);
    wait_process_gone(
        server_before_restart.pid,
        "fleet_old_server_exit_timeout"
    );
    text(context, cli, ["start-server"]);
    let restart_name = "restart-" + std::process::id();
    text(
        context,
        cli,
        [
            "new-window", "-d", "-n", restart_name,
            "--", "cmd.exe", "/d", "/c", "echo restarted"
        ]
    );
    let restart_after = json(context, cli, ["ui-snapshot"]);
    let server_after_restart = find_by_address(
        json_without_address(
            context,
            cli,
            ["server-list", "--json"]
        ),
        context.address
    );
    let restart_failure = run_failure(
        context,
        cli,
        [
            "read-events",
            "--epoch", restart_before.event_position.epoch,
            "--after",
            restart_before.event_position.sequence.to_string()
        ]
    );
    let ui_restart_failure = run_failure(
        context,
        cli,
        [
            "ui-deltas",
            "--epoch", restart_before.event_position.epoch,
            "--after",
            restart_before.event_position.sequence.to_string()
        ]
    );
    let restart_text =
        restart_failure.stdout + restart_failure.stderr;
    let ui_restart_text =
        ui_restart_failure.stdout + ui_restart_failure.stderr;
    require(
        restart_after.event_position.epoch
            != restart_before.event_position.epoch
            && restart_after.projection == "headless_server"
            && restart_after.window.state == "detached"
            && server_after_restart.pid != server_before_restart.pid
            && restart_text.contains("\"code\":\"server_restart\"")
            && restart_text.contains("\"requested_epoch\"")
            && restart_text.contains("\"current\"")
            && ui_restart_text.contains("\"code\":\"server_restart\"")
            && ui_restart_text.contains("\"requested_epoch\"")
            && ui_restart_text.contains("\"current\""),
        "fleet_restart_epoch_truth"
    );

    print("STEP mux uses shared IPC, address override, and native namespace");
    let mux_windows = run_program(
        context,
        mux,
        [
            "--address", context.address,
            "list-windows", "-F", "#{window_name}"
        ],
        false,
        true,
        ""
    ).stdout;
    require(
        mux_windows.contains(agent_name),
        "fleet_mux_shared_ipc"
    );
    let mux_protocol = rhai::json::parse(
        run_program(
            context,
            mux,
            [
                "--address", context.address,
                "agenterm", "protocol-info", "--running"
            ],
            false,
            true,
            ""
        ).stdout
    );
    require(
        mux_protocol.features.mux_frontend,
        "fleet_mux_protocol_feature"
    );

    print("STEP destructive mux failures preserve the live session and tabs");
    let before_destructive = run_program(
        context,
        mux,
        [
            "--address", context.address,
            "list-windows", "-F", "#{window_id}:#{window_name}"
        ],
        false,
        true,
        ""
    ).stdout;
    for rejected in [
        ["--address", context.address, "--session",
         "wrong-session", "attach"],
        ["--address", context.address, "kill-session",
         "-t", "wrong-session"],
        ["--address", context.address, "kill-window",
         "-t", "@999999"]
    ] {
        run_program(
            context,
            mux,
            rejected,
            true,
            true,
            ""
        );
    }
    let after_failures = run_program(
        context,
        mux,
        [
            "--address", context.address,
            "list-windows", "-F", "#{window_id}:#{window_name}"
        ],
        false,
        true,
        ""
    ).stdout;
    require(
        after_failures == before_destructive,
        "fleet_mux_failure_mutated_state"
    );

    let victim_name = "victim-" + std::process::id();
    run_program(
        context,
        mux,
        [
            "--address", context.address,
            "new-window", "-d", "-n", victim_name
        ],
        false,
        true,
        ""
    );
    run_program(
        context,
        mux,
        [
            "--address", context.address,
            "kill-window", "-t", victim_name
        ],
        false,
        true,
        ""
    );
    let after_kill = run_program(
        context,
        mux,
        [
            "--address", context.address,
            "list-windows", "-F", "#{window_name}"
        ],
        false,
        true,
        ""
    ).stdout;
    require(
        !after_kill.contains(victim_name)
            && after_kill.contains(agent_name),
        "fleet_mux_targeted_kill"
    );
    run_program(
        context,
        mux,
        ["ui-snapshot"],
        true,
        true,
        ""
    );

    print("STEP matching kill-session performs the requested destructive action");
    run_program(
        context,
        mux,
        [
            "--address", context.address,
            "kill-session", "-t", "agenterm"
        ],
        false,
        true,
        ""
    );

    print(
        "PASS: fleet launch, delimiter safety, loopback IPC, " +
        "and destructive mux behavior"
    );
    succeeded = true;
} catch (error) {
    failure = error.to_string();
}

for client in active_load_children {
    if client.child.state == "running" {
        client.child.kill();
    }
    try {
        client.child.wait_with_output(
            std::time::Duration::from_secs(2)
        );
    } catch (cleanup_error) {
    }
}
cleanup_server(context, cli, explicit_address);
cleanup_server(context, cli, second_address);
cleanup_server(context, cli, context.address);

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
    let registrations = std::fs::read_dir(context.instance_directory);
    let registration_count = 0;
    for entry in registrations {
        if entry.is_file && entry.file_name.ends_with(".json") {
            registration_count += 1;
        }
    }
    require(
        registration_count == 0,
        "fleet_cleanup_stale_registrations:" + registration_count
    );
    test_harness::remove_run(context);
}
if failure != "" {
    throw failure;
}
if completion_failure != "" {
    throw completion_failure;
}
