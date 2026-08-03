// Prove the public CLI, typed control, UI bridge, PTY, and lifecycle contract.
//
// Args:
//   --list-evidence
// or:
//   REPO GUI_EXE CLI_EXE

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "cli.backspace-del-one",
    "cli.remain-on-exit",
    "cli.stable-create-id",
    "cli.observable-events",
    "cli.typed-tabs-operations",
    "cli.control-receipts",
    "cli.ui-bridge-contracts",
    "cli.ui-bootstrap",
    "cli.ui-follow"
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

fn invoke(context, cli, arguments, expected_failure) {
    test_harness::invoke_cli(
        context,
        cli,
        arguments,
        expected_failure,
        []
    )
}

fn text(context, cli, arguments) {
    let output = invoke(context, cli, arguments, false);
    let result = output.stdout + output.stderr;
    result.trim();
    result
}

fn failure_text(context, cli, arguments) {
    let output = invoke(context, cli, arguments, true);
    output.stdout + output.stderr
}

fn json(context, cli, arguments) {
    rhai::json::parse(text(context, cli, arguments))
}

fn wait_sidebar_snapshot(context, cli, configured_width) {
    for attempt in 0..200 {
        let snapshot = json(context, cli, ["ui-snapshot"]);
        if snapshot.layout.sidebar != ()
            && snapshot.layout.sidebar.configured_width == configured_width
            && snapshot.layout.sidebar.visible
        {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "cli_smoke_sidebar_width_timeout";
}

fn direct(context, program, arguments, expected_failure) {
    let command = test_harness::configured_command(
        context,
        program,
        arguments
    );
    command.timeout(std::time::Duration::from_secs(15));
    let output = command.output();
    test_harness::append_command_record(
        context,
        arguments,
        output,
        expected_failure,
        []
    );
    if expected_failure {
        require(!output.success, "cli_smoke_direct_unexpected_success");
    } else {
        require(
            output.success,
            "cli_smoke_direct_failure:" + output.stderr_text()
        );
    }
    output.stdout_text() + output.stderr_text()
}

fn tab_by_name(tabs, name) {
    for tab in tabs {
        if tab.name == name || (tab.contains("title") && tab.title == name) {
            return tab;
        }
    }
    ()
}

fn tab_by_id(tabs, id) {
    for tab in tabs {
        if tab.id == id {
            return tab;
        }
    }
    ()
}

fn event_by_kind(events, kind) {
    let result = ();
    for event in events {
        if event.kind == kind {
            result = event;
        }
    }
    result
}

fn event_for_tab(events, kind, tab_id) {
    let result = ();
    for event in events {
        if event.kind == kind && event.tab_id == tab_id {
            result = event;
        }
    }
    result
}

fn operation_by_id(operations, id) {
    for operation in operations {
        if operation.id == id {
            return operation;
        }
    }
    ()
}

fn stable_tab_id(value) {
    value.starts_with("@") && value.len > 1
}

fn parse_integer(value) {
    let text = value.to_string();
    text.trim();
    text.parse_int()
}

fn require_file(path, minimum_bytes) {
    require(std::fs::exists(path), "cli_smoke_file_missing:" + path);
    require(
        std::fs::metadata(path).len >= minimum_bytes,
        "cli_smoke_file_too_small:" + path
    );
}

fn wait_for_gui(context, cli, gui) {
    for attempt in 0..400 {
        let probe = test_harness::run_cli(
            context,
            cli,
            [
                "wait-ui", "--window-state", "restored",
                "--timeout-ms", "100"
            ],
            false,
            []
        );
        let facts = gui.platform_facts;
        if probe.success
                && facts.top_level_window_supported
                && facts.top_level_window_present {
            let snapshot = rhai::json::parse(probe.stdout);
            require(
                snapshot.client_pid == gui.id,
                "cli_smoke_gui_lease_owner"
            );
            return snapshot;
        }
        if gui.state == "exited" {
            let output = gui.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw "cli_smoke_gui_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "cli_smoke_gui_ready_timeout";
}

require(args.len == 3, "expected: REPO GUI_EXE CLI_EXE");
let repo = std::path::absolute(args[0]).display;
let gui_exe = std::path::absolute(args[1]).display;
let cli = std::path::absolute(args[2]).display;
require(std::fs::exists(gui_exe), "cli_smoke_gui_missing");
require(std::fs::exists(cli), "cli_smoke_cli_missing");

let context = test_harness::new_context(repo, "cli");
let name = "agenterm-smoke-" + context.run_id;
let token = "AGENTERM_SMOKE_" + context.run_id;
let window_png = std::path::join(
    context.run_directory,
    name + "-window.png"
).display;
let pane_png = std::path::join(
    context.run_directory,
    name + "-pane.png"
).display;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP create tab and prove request receipt replay");
    text(context, cli, ["new-window", "-d", "-n", name]);

    let request_id = "receipt-" + context.run_id;
    let receipt_args = [
        "--request-id", request_id, "--receipt-json",
        "set-tab-note", "-t", name, "receipt-test"
    ];
    let first_receipt = json(context, cli, receipt_args);
    let replayed_receipt = json(context, cli, receipt_args);
    require(
        first_receipt.outcome == "committed"
            && first_receipt.request_id == request_id
            && first_receipt.resolved.tab_id != ()
            && first_receipt.after_position.sequence ==
                replayed_receipt.after_position.sequence,
        "cli_smoke_receipt_replay"
    );
    let conflict_receipt = rhai::json::parse(
        failure_text(
            context,
            cli,
            [
                "--request-id", request_id, "--receipt-json",
                "set-tab-note", "-t", name, "different-payload"
            ]
        )
    );
    require(
        conflict_receipt.outcome == "no_op"
            && conflict_receipt.error.code == "request_id_conflict"
            && conflict_receipt.error.category == "conflict",
        "cli_smoke_receipt_conflict"
    );

    let retry_name = "receipt-window-" + context.run_id;
    let create_request = "receipt-create-" + context.run_id;
    let create_args = [
        "--request-id", create_request, "--receipt-json",
        "new-window", "-d", "-n", retry_name
    ];
    let created = json(context, cli, create_args);
    let created_replay = json(context, cli, create_args);
    let retry_tab = tab_by_name(
        json(context, cli, ["ui-snapshot"]).tabs,
        retry_name
    );
    require(
        created.outcome == "committed"
            && created.after_position.sequence ==
                created_replay.after_position.sequence
            && retry_tab != ()
            && stable_tab_id(retry_tab.id),
        "cli_smoke_create_replay"
    );
    let kill_request = "receipt-kill-" + context.run_id;
    let kill_args = [
        "--request-id", kill_request, "--receipt-json",
        "kill-window", "-t", retry_tab.id
    ];
    let killed = json(context, cli, kill_args);
    let killed_replay = json(context, cli, kill_args);
    require(
        killed.outcome == "committed"
            && killed.after_position.sequence ==
                killed_replay.after_position.sequence
            && tab_by_id(
                json(context, cli, ["ui-snapshot"]).tabs,
                retry_tab.id
            ) == (),
        "cli_smoke_kill_replay"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.control-receipts"
    );

    print("STEP discover commands and typed operation contracts");
    let commands = text(context, cli, ["list-commands"]);
    for expected in [
        "new-window (neww)", "list-windows (lsw)",
        "send-keys (send)", "scroll-pane", "read-events",
        "wait-events", "new-agent", "list-instances",
        "wait-pane (expect-pane)", "set-composer", "ui-bootstrap",
        "ui-deltas", "ui-hello", "ui-snapshot"
    ] {
        require(
            commands.contains(expected),
            "cli_smoke_command_missing:" + expected
        );
    }
    let offline_unknown = direct(
        context,
        cli,
        [
            "--address", "127.0.0.1:1",
            "ui-action", "tabs-teleport"
        ],
        true
    );
    let offline_width = direct(
        context,
        cli,
        [
            "--address", "127.0.0.1:1",
            "ui-action", "tabs-set-width"
        ],
        true
    );
    require(
        offline_unknown.contains("operation_unknown[tabs-teleport]")
            && !offline_unknown.contains("connect"),
        "cli_smoke_offline_operation_validation"
    );
    require(
        offline_width.contains(
            "operation_invalid_arguments[ui.tabs.set-width]"
        ) && !offline_width.contains("connect"),
        "cli_smoke_offline_width_validation"
    );

    let protocol = json(context, cli, ["protocol-info"]);
    let operation_catalog = protocol.operation_catalog;
    require(
        protocol.ui_bridge.schema_version == 7
            && protocol.ui_bridge.ownership_mode == "split_server_client"
            && protocol.ui_bridge.replaceable_ui
            && protocol.ui_bridge.interactive_lease
            && protocol.ui_bridge.target_server_executable ==
                "agenterm-server.exe"
            && protocol.ui_bridge.bootstrap_snapshot
            && protocol.ui_bridge.ordered_deltas
            && protocol.ui_bridge.reconnect
            && protocol.ui_bridge.rollback_proven
            && protocol.ui_bridge.contract_schemas.hello == 1
            && protocol.ui_bridge.contract_schemas.bootstrap == 2
            && protocol.ui_bridge.contract_schemas.screen == 2
            && protocol.ui_bridge.contract_schemas.delta == 2
            && protocol.ui_bridge.contract_schemas.lease == 2
            && protocol.ui_bridge.contract_schemas.interaction == 1
            && protocol.ui_bridge.hard_limits.bootstrap_bytes == 8388608
            && protocol.ui_bridge.hard_limits.tabs == 1024
            && protocol.ui_bridge.hard_limits.screen_rows == 512
            && protocol.ui_bridge.hard_limits.screen_columns == 512
            && protocol.ui_bridge.hard_limits.screen_runs == 262144
            && protocol.ui_bridge.hard_limits.screen_text_bytes == 4194304
            && protocol.ui_bridge.hard_limits.delta_bytes == 8388608
            && protocol.ui_bridge.hard_limits.delta_events == 64
            && protocol.ui_bridge.hard_limits.input_bytes == 262144
            && operation_catalog.schema_version == 1
            && operation_catalog.classification_only
            && !operation_catalog.authorization_policy,
        "cli_smoke_protocol_contract"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.ui-bridge-contracts"
    );

    print("STEP renderer-neutral bootstrap and causal UI follow");
    let running_protocol =
        json(context, cli, ["protocol-info", "--running"]);
    let bootstrap = json(context, cli, ["ui-bootstrap"]);
    let inspect = json(context, cli, ["inspect"]);
    let ui_snapshot = json(context, cli, ["ui-snapshot"]);
    require(
        bootstrap.schema_version == 2
            && running_protocol.identity_scope == "running_host"
            && bootstrap.server_pid == running_protocol.pid
            && bootstrap.server_epoch == bootstrap.position.server_epoch
            && bootstrap.server_epoch == ui_snapshot.event_position.epoch
            && bootstrap.position.sequence ==
                ui_snapshot.event_position.sequence
            && bootstrap.complete
            && !bootstrap.truncated
            && bootstrap.active_tab_id == inspect.active_window_id
            && bootstrap.tabs.len == inspect.windows.len,
        "cli_smoke_bootstrap_identity"
    );
    for tab in bootstrap.tabs {
        let window = tab_by_id(inspect.windows, tab.id);
        require(
            window != ()
                && tab.parent_id == window.parent_id
                && tab.title == window.name
                && tab.note == window.note
                && tab.process_id == window.pid
                && tab.dead == window.dead
                && tab.exit_code == window.exit_code
                && tab.screen.tab_id == tab.id
                && tab.screen.rows == window.rows
                && tab.screen.columns == window.cols
                && tab.screen.scrollback_offset == window.scrollback_offset
                && tab.screen.complete
                && !tab.screen.truncated
                && tab.screen.runs.len > 0,
            "cli_smoke_bootstrap_tab:" + tab.id
        );
        if tab.composer.sensitive {
            require(
                tab.composer.text == (),
                "cli_smoke_sensitive_composer_disclosed"
            );
        } else {
            require(
                tab.composer.byte_length ==
                    rhai::bytes::from_text(tab.composer.text).len,
                "cli_smoke_composer_byte_identity"
            );
        }
        require(
            !tab.working_context.contains("http_proxy")
                && !tab.working_context.contains("https_proxy"),
            "cli_smoke_proxy_value_disclosed"
        );
    }
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.ui-bootstrap"
    );

    let hello_build = #{
        protocol_version: 1,
        version: "cli-smoke",
        git_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        profile: "black-box",
        cargo_lock_sha256:
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        artifact_manifest_sha256:
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    };
    let hello = json(
        context,
        cli,
        [
            "ui-hello", "--minimum", "1", "--maximum", "1",
            "--client-id", "cli-smoke-" + context.run_id,
            "--client-build-json", rhai::json::stringify(hello_build)
        ]
    );
    require(
        hello.schema_version == 1
            && hello.accepted
            && hello.compatibility == "compatible"
            && hello.client_id == "cli-smoke-" + context.run_id
            && hello.protocol_version == 1
            && hello.server_pid == running_protocol.pid
            && hello.client_build.profile == "black-box"
            && hello.client_build.git_commit ==
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            && hello.server_build.protocol_version == 1
            && hello.server_build.version != ""
            && hello.bootstrap_schema_version == 2
            && hello.delta_schema_version == 2
            && hello.capabilities.contains("ordered_delta_poll"),
        "cli_smoke_hello_contract"
    );
    let too_new = json(
        context,
        cli,
        [
            "ui-hello", "--minimum", "2", "--maximum", "2",
            "--client-id", "future-renderer-" + context.run_id
        ]
    );
    require(
        !too_new.accepted
            && too_new.compatibility == "client_too_new",
        "cli_smoke_future_renderer_contract"
    );
    let follow_tab = tab_by_name(bootstrap.tabs, name);
    require(follow_tab != (), "cli_smoke_follow_tab_missing");
    let follow_note = "ui-follow-" + context.run_id;
    text(
        context,
        cli,
        ["set-tab-note", "-t", follow_tab.id, follow_note]
    );
    let deltas = json(
        context,
        cli,
        [
            "ui-deltas", "--epoch", hello.position.server_epoch,
            "--after", hello.position.sequence.to_string(),
            "--limit", "64"
        ]
    );
    let previous = hello.position.sequence;
    for event in deltas.events {
        require(
            event.sequence > previous,
            "cli_smoke_delta_order"
        );
        previous = event.sequence;
    }
    let follow_event =
        event_for_tab(deltas.events, "tab.note", follow_tab.id);
    let follow_update = tab_by_id(deltas.tab_updates, follow_tab.id);
    let post_bootstrap = json(context, cli, ["ui-bootstrap"]);
    let post_tab = tab_by_id(post_bootstrap.tabs, follow_tab.id);
    require(
        deltas.schema_version == 2
            && deltas.server_epoch == hello.position.server_epoch
            && deltas.after_sequence == hello.position.sequence
            && deltas.through_sequence == deltas.current_sequence
            && deltas.complete
            && !deltas.truncated
            && follow_event != ()
            && follow_update != ()
            && follow_update.note == follow_note
            && follow_update.screen.generation == deltas.current_sequence
            && post_bootstrap.position.sequence == deltas.current_sequence
            && post_tab != ()
            && post_tab.note == follow_update.note
            && post_tab.screen.generation ==
                follow_update.screen.generation,
        "cli_smoke_delta_post_state"
    );
    let restart_error = failure_text(
        context,
        cli,
        [
            "ui-deltas", "--epoch", "definitely-stale-ui-epoch",
            "--after", "0"
        ]
    );
    let future_error = failure_text(
        context,
        cli,
        [
            "ui-deltas", "--epoch", deltas.server_epoch,
            "--after", (deltas.current_sequence + 1).to_string()
        ]
    );
    require(
        restart_error.contains("\"code\":\"server_restart\"")
            && restart_error.contains("\"current\"")
            && future_error.contains("\"code\":\"future_sequence\"")
            && future_error.contains("\"current\""),
        "cli_smoke_delta_recovery"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.ui-follow"
    );

    let expected_operations = #{
        "protocol.info": "observe",
        "ui.hello": "observe",
        "ui.bootstrap": "observe",
        "ui.deltas": "observe",
        "ui.tabs.show": "control",
        "ui.tabs.hide": "control",
        "ui.tabs.toggle": "control",
        "ui.tabs.set-width": "control",
        "tabs.set-note": "control",
        "server.kill": "destructive"
    };
    let seen_operations = #{};
    for operation in operation_catalog.operations {
        require(
            !seen_operations.contains(operation.id),
            "cli_smoke_duplicate_operation:" + operation.id
        );
        seen_operations[operation.id] = true;
        for field in [
            "id", "class", "command", "aliases", "parameters",
            "result_type", "errors", "events", "destructive",
            "available", "since"
        ] {
            require(
                operation.contains(field),
                "cli_smoke_operation_field:" + operation.id + ":" + field
            );
        }
        require(
            operation.available
                && operation.result_type != ""
                && operation.since != ""
                && (
                    (operation.class == "destructive") ==
                        operation.destructive
                ),
            "cli_smoke_operation_metadata:" + operation.id
        );
    }
    for id in expected_operations.keys() {
        let operation =
            operation_by_id(operation_catalog.operations, id);
        require(
            operation != () && operation.class == expected_operations[id],
            "cli_smoke_operation_class:" + id
        );
    }
    let width_operation =
        operation_by_id(operation_catalog.operations, "ui.tabs.set-width");
    let width_parameter = width_operation.parameters[0];
    require(
        width_parameter.name == "width"
            && width_parameter.value_type == "integer"
            && width_parameter.required
            && width_parameter.minimum == 180
            && width_parameter.maximum == 480
            && width_operation.result_type == "ui_snapshot"
            && width_operation.events.contains("layout.tabs.width"),
        "cli_smoke_width_discovery"
    );
    require(
        operation_by_id(
            operation_catalog.operations,
            "ui.tabs.toggle"
        ).aliases.contains("toggle-tabs"),
        "cli_smoke_toggle_alias"
    );
    let note_operation =
        operation_by_id(operation_catalog.operations, "tabs.set-note");
    require(
        note_operation.script_surface == "fleet.tabs.set_note"
            && note_operation.parameters.len == 2
            && note_operation.events.contains("tab.note"),
        "cli_smoke_note_discovery"
    );

    let headless_error = failure_text(
        context,
        cli,
        ["ui-action", "tabs-hide"]
    );
    require(
        headless_error.contains(
            "no interactive GUI client is attached to this server"
        ),
        "cli_smoke_headless_ui_error"
    );
    let gui_command = test_harness::configured_command(
        context,
        gui_exe,
        ["--no-activate", "--address", context.address]
    );
    let gui = gui_command.start();
    owned_children.push(gui);
    wait_for_gui(context, cli, gui);

    print("STEP typed Tabs actions preserve layout and event attribution");
    let tabs_baseline = json(context, cli, ["ui-snapshot"]);
    let configured_width =
        tabs_baseline.layout.sidebar.configured_width;
    let hidden = json(
        context,
        cli,
        ["ui-action", "tabs-hide"]
    );
    require(
        !hidden.layout.sidebar.visible
            && hidden.layout.sidebar.effective_width == 0
            && hidden.layout.sidebar.configured_width == configured_width,
        "cli_smoke_tabs_hide"
    );
    let visibility_events = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", tabs_baseline.event_position.epoch,
            "--after", tabs_baseline.event_position.sequence.to_string()
        ]
    );
    let hide_event =
        event_by_kind(visibility_events.events, "layout.tabs.visibility");
    require(
        hide_event != ()
            && hide_event.payload.operation_id == "ui.tabs.hide",
        "cli_smoke_tabs_hide_event"
    );
    require(
        json(context, cli, ["ui-action", "tabs-show"])
            .layout.sidebar.visible,
        "cli_smoke_tabs_show"
    );
    require(
        !json(context, cli, ["ui-action", "tabs-toggle"])
            .layout.sidebar.visible,
        "cli_smoke_tabs_toggle"
    );
    require(
        json(context, cli, ["ui-action", "toggle-tabs"])
            .layout.sidebar.visible,
        "cli_smoke_tabs_legacy_toggle"
    );
    let width_baseline = json(context, cli, ["ui-snapshot"]);
    for width in [180, 480] {
        let sized = json(
            context,
            cli,
            [
                "ui-action", "tabs-set-width",
                "--width", width.to_string()
            ]
        );
        require(
            sized.layout.sidebar.configured_width == width
                && sized.settings.tabs_width == width,
            "cli_smoke_width_boundary:" + width
        );
    }
    let width_events = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", width_baseline.event_position.epoch,
            "--after", width_baseline.event_position.sequence.to_string()
        ]
    );
    let width_event =
        event_by_kind(width_events.events, "layout.tabs.width");
    require(
        width_event != ()
            && width_event.payload.operation_id == "ui.tabs.set-width"
            && width_event.payload.configured_width == 480,
        "cli_smoke_width_event"
    );
    for width in [179, 481] {
        require(
            failure_text(
                context,
                cli,
                [
                    "ui-action", "tabs-set-width",
                    "--width", width.to_string()
                ]
            ).contains(
                "operation_invalid_arguments[ui.tabs.set-width]"
            ),
            "cli_smoke_invalid_width:" + width
        );
    }
    require(
        failure_text(
            context,
            cli,
            ["ui-action", "tabs-teleport"]
        ).contains("operation_unknown[tabs-teleport]"),
        "cli_smoke_unknown_tabs_action"
    );
    let after_invalid = wait_sidebar_snapshot(context, cli, 480);
    text(
        context,
        cli,
        [
            "ui-action", "tabs-set-width",
            "--width", configured_width.to_string()
        ]
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.typed-tabs-operations"
    );

    print("STEP observable event read and bounded wait");
    let event_baseline = json(context, cli, ["ui-snapshot"]);
    let event_name = "event-" + context.run_id;
    text(context, cli, ["new-window", "-d", "-n", event_name]);
    let event_batch = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", event_baseline.event_position.epoch,
            "--after", event_baseline.event_position.sequence.to_string()
        ]
    );
    let created_event =
        event_by_kind(event_batch.events, "tab.created");
    require(
        created_event != ()
            && created_event.sequence >
                event_baseline.event_position.sequence
            && created_event.payload.index >= 0,
        "cli_smoke_created_event"
    );
    let waited = json(
        context,
        cli,
        [
            "wait-events",
            "--epoch", event_baseline.event_position.epoch,
            "--after", event_baseline.event_position.sequence.to_string(),
            "--kind", "tab.created",
            "--tab", "@" + created_event.tab_id,
            "--timeout-ms", "1000"
        ]
    );
    require(
        waited.sequence == created_event.sequence,
        "cli_smoke_waited_event"
    );
    require(
        failure_text(
            context,
            cli,
            [
                "wait-events",
                "--epoch", event_batch.position.epoch,
                "--after", event_batch.position.sequence.to_string(),
                "--kind", "never.test-event",
                "--timeout-ms", "20"
            ]
        ).contains("event_wait_timeout"),
        "cli_smoke_event_timeout"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.observable-events"
    );

    print("STEP composer, PTY, Backspace, viewport, and screenshots");
    text(
        context,
        cli,
        ["set-composer", "-t", name, "echo " + token]
    );
    let before_submit = json(context, cli, ["inspect", "-t", name]);
    require(
        text(context, cli, ["show-composer", "-t", name]) ==
            "echo " + token,
        "cli_smoke_composer_roundtrip"
    );
    let submit_baseline = json(context, cli, ["ui-snapshot"]);
    let submit_request = "submit-" + context.run_id;
    let submit_args = [
        "--request-id", submit_request, "--receipt-json",
        "send-composer", "-t", name
    ];
    let accepted = json(context, cli, submit_args);
    require(
        accepted.outcome == "accepted"
            && accepted.wait.condition == "submission_complete"
            && accepted.wait.event_kind ==
                "composer.submission-finished",
        "cli_smoke_submit_accepted"
    );
    require(
        failure_text(
            context,
            cli,
            ["send-keys", "-t", name, "SHOULD_NOT_MERGE"]
        ).contains("composer submission is pending"),
        "cli_smoke_pending_input"
    );
    text(
        context,
        cli,
        [
            "wait-pane", "-t", name, "--contains", token,
            "--submit-complete", "--timeout-ms", "10000"
        ]
    );
    let committed = json(context, cli, submit_args);
    require(
        committed.outcome == "committed"
            && committed.after_position.sequence >
                accepted.after_position.sequence,
        "cli_smoke_submit_committed"
    );
    let submit_events = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", submit_baseline.event_position.epoch,
            "--after", submit_baseline.event_position.sequence.to_string()
        ]
    );
    let submit_completion = ();
    let submit_count = 0;
    for event in submit_events.events {
        if event.kind == "composer.submission-finished"
                && event.request_id == submit_request
                && event.operation_id == committed.operation_id {
            submit_completion = event;
            submit_count += 1;
        }
    }
    require(
        submit_count == 1 && submit_completion.payload.enter_written,
        "cli_smoke_submit_event"
    );
    let after_submit = json(context, cli, ["inspect", "-t", name]);
    require(
        after_submit.windows[0].input_writes ==
            before_submit.windows[0].input_writes + 2
            && !after_submit.windows[0].submit_pending,
        "cli_smoke_submit_pty_writes"
    );
    require(
        text(context, cli, ["capture-pane", "-p", "-t", name])
            .contains(token),
        "cli_smoke_capture_token"
    );

    let backspace_prefix =
        "AGENTERM_BACKSPACE_" + context.run_id;
    text(
        context,
        cli,
        [
            "send-keys", "-t", name, "-l",
            "echo " + backspace_prefix + "X"
        ]
    );
    text(context, cli, ["send-keys", "-t", name, "Backspace"]);
    text(context, cli, ["send-keys", "-t", name, "-l", "Y"]);
    text(context, cli, ["send-keys", "-t", name, "Enter"]);
    text(
        context,
        cli,
        [
            "wait-pane", "-t", name,
            "--contains", backspace_prefix + "Y",
            "--timeout-ms", "10000"
        ]
    );
    let backspace_capture =
        text(context, cli, ["capture-pane", "-p", "-t", name]);
    let exact_backspace = false;
    for line in backspace_capture.split("\n") {
        line.trim();
        if line == backspace_prefix + "Y" {
            exact_backspace = true;
        }
    }
    require(exact_backspace, "cli_smoke_backspace_contract");
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.backspace-del-one"
    );

    let scroll_prefix = "AGENTERM_SCROLL_" + context.run_id;
    text(
        context,
        cli,
        [
            "set-composer", "-t", name,
            "for /L %i in (1,1,80) do @echo " +
                scroll_prefix + "_%i"
        ]
    );
    text(context, cli, ["send-composer", "-t", name]);
    text(
        context,
        cli,
        [
            "wait-pane", "-t", name,
            "--contains", scroll_prefix + "_80",
            "--submit-complete", "--timeout-ms", "10000"
        ]
    );
    let top_offset = parse_integer(
        text(context, cli, ["scroll-pane", "-t", name, "top"])
    );
    require(
        top_offset > 0
            && !text(
                context,
                cli,
                ["capture-pane", "-p", "-t", name]
            ).contains(scroll_prefix + "_80"),
        "cli_smoke_scroll_top"
    );
    let lower_offset = parse_integer(
        text(
            context,
            cli,
            ["scroll-pane", "-t", name, "down", "5"]
        )
    );
    require(lower_offset < top_offset, "cli_smoke_scroll_down");
    let bottom_offset = parse_integer(
        text(context, cli, ["scroll-pane", "-t", name, "bottom"])
    );
    require(
        bottom_offset == 0
            && text(
                context,
                cli,
                ["capture-pane", "-p", "-t", name]
            ).contains(scroll_prefix + "_80"),
        "cli_smoke_scroll_bottom"
    );
    text(context, cli, ["screenshot", "-o", window_png]);
    text(
        context,
        cli,
        ["screenshot-pane", "-t", name, "-o", pane_png]
    );
    require_file(window_png, 1000);
    require_file(pane_png, 1000);

    print("STEP stable IDs, remain-on-exit, and explicit close");
    let stable_root_name = "stable-root-" + context.run_id;
    let stable_child_name = "stable-child-" + context.run_id;
    let disposable_name = "disposable-" + context.run_id;
    let stable_root = text(
        context,
        cli,
        [
            "new-window", "-d", "-n", stable_root_name,
            "-F", "#{window_id}"
        ]
    );
    require(stable_tab_id(stable_root), "cli_smoke_stable_root_id");
    let stable_child = text(
        context,
        cli,
        [
            "new-window", "-d", "-n", stable_child_name,
            "--parent", stable_root, "-F", "#{window_id}"
        ]
    );
    let disposable = text(
        context,
        cli,
        [
            "new-window", "-d", "-n", disposable_name,
            "-F", "#{window_id}", "--",
            "cmd.exe", "/d", "/c", "exit", "0"
        ]
    );
    text(
        context,
        cli,
        [
            "wait-ui", "-t", disposable, "--tab-state", "dead",
            "--timeout-ms", "10000"
        ]
    );
    text(context, cli, ["kill-window", "-t", disposable]);
    text(context, cli, ["select-window", "-t", stable_child]);
    text(
        context,
        cli,
        [
            "wait-ui", "--active", stable_child,
            "--timeout-ms", "10000"
        ]
    );
    text(
        context,
        cli,
        [
            "rename-window", "-t", stable_child,
            stable_child_name + "-renamed"
        ]
    );
    require(
        text(
            context,
            cli,
            ["show-tab-parent", "-t", stable_child]
        ) == stable_root,
        "cli_smoke_stable_parent"
    );
    text(context, cli, ["kill-window", "-t", stable_child]);
    text(context, cli, ["kill-window", "-t", stable_root]);
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.stable-create-id"
    );

    text(context, cli, ["send-keys", "-t", name, "exit", "Enter"]);
    text(
        context,
        cli,
        [
            "wait-pane", "-t", name, "--dead",
            "--timeout-ms", "10000"
        ]
    );
    text(
        context,
        cli,
        [
            "wait-pane", "-t", name, "--finalized",
            "--timeout-ms", "10000"
        ]
    );
    let finalized_first =
        json(context, cli, ["inspect", "-t", name]).windows[0];
    let finalized_second =
        json(context, cli, ["inspect", "-t", name]).windows[0];
    require(
        finalized_first.reader_closed
            && finalized_first.parser_drained
            && finalized_first.finalized
            && finalized_first.output_bytes ==
                finalized_second.output_bytes,
        "cli_smoke_finalized_boundary"
    );
    let dead_write = rhai::json::parse(
        failure_text(
            context,
            cli,
            [
                "--receipt-json", "send-keys",
                "-t", name, "-l", "late-input"
            ]
        )
    );
    require(
        dead_write.outcome == "no_op"
            && dead_write.error.code == "terminal_not_writable",
        "cli_smoke_dead_write"
    );
    require(
        text(
            context,
            cli,
            [
                "display-message", "-p", "-t", name,
                "#{window_id}:#{window_name}:#{pane_dead}"
            ]
        ).ends_with(":" + name + ":1"),
        "cli_smoke_remain_on_exit"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "cli.remain-on-exit"
    );

    let close_baseline = json(context, cli, ["ui-snapshot"]);
    let closing_tab = tab_by_name(close_baseline.tabs, name);
    require(closing_tab != (), "cli_smoke_closing_tab_missing");
    text(context, cli, ["kill-window", "-t", name]);
    let close_events = json(
        context,
        cli,
        [
            "read-events",
            "--epoch", close_baseline.event_position.epoch,
            "--after", close_baseline.event_position.sequence.to_string()
        ]
    );
    let closing_numeric = parse_integer(
        closing_tab.id.sub_string(1)
    );
    let closed_event =
        event_for_tab(close_events.events, "tab.closed", closing_numeric);
    require(
        closed_event != ()
            && closed_event.payload.terminal_shutdown_complete,
        "cli_smoke_explicit_close"
    );

    let close_state = json(
        context,
        cli,
        ["ui-action", "close-window"]
    );
    require(
        close_state.modal.kind == "confirm-window-close"
            && close_state.modal.actions.contains("stop-server-and-exit"),
        "cli_smoke_window_close_contract"
    );
    text(
        context,
        cli,
        ["ui-action", "stop-server-and-exit"]
    );
    gui.wait_with_output(std::time::Duration::from_secs(5));

    print(
        "PASS: typed control, UI bridge, PTY, screenshots, " +
            "remain-on-exit, and explicit close"
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
