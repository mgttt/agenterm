// Prove native local IPC selection, schema-v1/v2 discovery, and the explicit
// loopback TCP migration path through public AgenTerm executables only.
//
// Args:
//   --list-evidence
// or:
//   REPO SERVER_EXE CLI_EXE
// Optional task-run overrides appended after the manifest defaults:
//   --server PATH --cli PATH [--ci-main-dev]

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "control.native-local-ipc"
];

if args.len == 1 && args[0] == "--list-evidence" {
    print("control.native-local-ipc");
    return;
}

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn configured(
    context,
    program,
    arguments,
    endpoint,
    address,
    instance,
    workspace
) {
    let command = test_harness::configured_command(
        context,
        program,
        arguments
    );
    for name in [
        "AGENTERM_IPC_ENDPOINT",
        "AGENTERM_IPC_ADDRESS",
        "AGENTERM_INSTANCE"
    ] {
        command.env_remove(name);
    }
    if endpoint != "" {
        command.env("AGENTERM_IPC_ENDPOINT", endpoint);
    }
    if address != "" {
        command.env("AGENTERM_IPC_ADDRESS", address);
    }
    if instance != "" {
        command.env("AGENTERM_INSTANCE", instance);
    }
    if workspace != "" {
        command.env("AGENTERM_WORKSPACE_PATH", workspace);
    }
    command.env(
        "XDG_RUNTIME_DIR",
        std::path::join(context.run_directory, "runtime").display
    );
    command
}

fn invoke(
    context,
    program,
    arguments,
    expected_failure,
    endpoint,
    address,
    instance,
    workspace
) {
    let command = configured(
        context,
        program,
        arguments,
        endpoint,
        address,
        instance,
        workspace
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
        require(
            !output.success,
            "native_ipc_command_unexpected_success:" +
                rhai::json::stringify(arguments)
        );
    } else {
        require(
            output.success,
            "native_ipc_command_failed:" +
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

fn invoke_json(
    context,
    cli,
    arguments,
    endpoint,
    address,
    instance,
    workspace
) {
    let output = invoke(
        context,
        cli,
        arguments,
        false,
        endpoint,
        address,
        instance,
        workspace
    );
    rhai::json::parse(output.stdout + output.stderr)
}

fn probe(
    context,
    program,
    arguments,
    endpoint,
    address,
    instance,
    workspace
) {
    let command = configured(
        context,
        program,
        arguments,
        endpoint,
        address,
        instance,
        workspace
    );
    command.timeout(std::time::Duration::from_secs(2));
    let output = command.output();
    test_harness::append_command_record(
        context,
        arguments,
        output,
        !output.success,
        []
    );
    #{
        success: output.success,
        exit_code: output.exit_code,
        stdout: output.stdout_text(),
        stderr: output.stderr_text()
    }
}

fn wait_protocol(
    context,
    cli,
    selector_arguments,
    endpoint,
    address,
    instance,
    workspace,
    server
) {
    for attempt in 0..200 {
        let arguments = selector_arguments;
        arguments.push("protocol-info");
        arguments.push("--running");
        let output = probe(
            context,
            cli,
            arguments,
            endpoint,
            address,
            instance,
            workspace
        );
        if output.success {
            return rhai::json::parse(output.stdout + output.stderr);
        }
        if server.state == "exited" {
            let ended = server.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw "native_ipc_server_early_exit:" + ended.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "native_ipc_server_ready_timeout";
}

fn wait_exit(child, code) {
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

fn find_by_endpoint(rows, endpoint, code) {
    for row in rows {
        if row.endpoint == endpoint {
            return row;
        }
    }
    throw code;
}

fn registration_schema_counts(directory) {
    let v1 = 0;
    let v2 = 0;
    for entry in std::fs::read_dir(directory) {
        if entry.is_file && entry.file_name.ends_with(".json") {
            let value = rhai::json::parse_file(entry.path.display);
            if value.schema_version == 1 {
                v1 += 1;
            } else if value.schema_version == 2 {
                v2 += 1;
            }
        }
    }
    #{ v1: v1, v2: v2 }
}

fn registration_count(directory) {
    let count = 0;
    for entry in std::fs::read_dir(directory) {
        if entry.is_file && entry.file_name.ends_with(".json") {
            count += 1;
        }
    }
    count
}

fn unix_mode(context, path) {
    let probes = [
        ["-c", "%a", path],
        ["-f", "%Lp", path]
    ];
    for arguments in probes {
        let command = std::process::command("stat");
        command.args(arguments);
        command.timeout(std::time::Duration::from_secs(2));
        let output = command.output();
        test_harness::append_command_record(
            context,
            arguments,
            output,
            !output.success,
            []
        );
        if output.success {
            let text = output.stdout_text();
            text.trim();
            return text;
        }
    }
    throw "native_ipc_unix_stat_unavailable";
}

require(args.len >= 3, "expected: REPO SERVER_EXE CLI_EXE");
let repo = std::path::absolute(args[0]).display;
let server_exe = args[1];
let cli = args[2];
let ci_main_dev = false;
let option_index = 3;
while option_index < args.len {
    let option = args[option_index];
    if option == "--server" || option == "--cli" {
        require(
            option_index + 1 < args.len,
            "native_ipc_option_value_missing:" + option
        );
        if option == "--server" {
            server_exe = args[option_index + 1];
        } else {
            cli = args[option_index + 1];
        }
        option_index += 1;
    } else if option == "--ci-main-dev" {
        ci_main_dev = true;
    } else {
        throw "native_ipc_unknown_option:" + option;
    }
    option_index += 1;
}
server_exe = std::path::absolute(server_exe).display;
cli = std::path::absolute(cli).display;
require(std::fs::exists(server_exe), "native_ipc_server_missing");
require(std::fs::exists(cli), "native_ipc_cli_missing");

let context = test_harness::new_context(repo, "native-ipc");
let native_workspace = std::path::join(
    context.run_directory,
    "workspace/native-workspace.json"
).display;
let tcp_workspace = std::path::join(
    context.run_directory,
    "workspace/tcp-workspace.json"
).display;
let custom_instance = if ci_main_dev {
    "main"
} else {
    "custom:smoke-" + std::process::id() + "-" +
        std::time::SystemTime::now().unix_millis
};
let dead_endpoint = "tcp:127.0.0.1:1";
let tcp_endpoint = "tcp:" + context.address;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP CLI selector conflicts fail fast and typed");
    let conflict_started =
        std::time::SystemTime::now().unix_millis;
    let conflict = invoke(
        context,
        cli,
        [
            "--endpoint", dead_endpoint,
            "--instance", custom_instance,
            "protocol-info", "--running"
        ],
        true,
        "",
        "",
        "",
        native_workspace
    );
    let conflict_text = conflict.stdout + conflict.stderr;
    require(
        conflict.exit_code == 2
            && conflict_text.contains("mutually exclusive")
            && conflict_text.contains("ConflictingCliSelectors")
            && std::time::SystemTime::now().unix_millis
                - conflict_started < 2000,
        "native_ipc_selector_conflict_not_typed_or_bounded"
    );

    print("STEP isolated logical instance derives its native endpoint");
    let native_command = configured(
        context,
        server_exe,
        ["--instance", custom_instance],
        dead_endpoint,
        context.address,
        "main",
        native_workspace
    );
    let native_server = native_command.start();
    owned_children.push(native_server);
    let native_protocol = wait_protocol(
        context,
        cli,
        ["--instance", custom_instance],
        dead_endpoint,
        context.address,
        "main",
        native_workspace,
        native_server
    );
    let native_endpoint_shape = if native_protocol.transport == "named_pipe" {
        native_protocol.endpoint.starts_with(
            `pipe:\\.\pipe\agenterm-agt-v1-`
        )
    } else if native_protocol.transport == "unix" {
        native_protocol.endpoint.starts_with("unix:/")
            && native_protocol.endpoint.ends_with(".sock")
    } else {
        false
    };
    require(
        native_protocol.pid == native_server.id
            && native_protocol.instance == custom_instance
            && native_endpoint_shape
            && native_protocol.server_scope_id.starts_with("agt-v1-")
            && native_protocol.server_scope_id.len == 39
            && !native_protocol.endpoint.contains(custom_instance),
        "native_ipc_custom_native_contract"
    );
    if native_protocol.transport == "unix" {
        let socket_path = native_protocol.endpoint.sub_string(5);
        let socket_parent = std::path::parent(socket_path).display;
        require(
            unix_mode(context, socket_path) == "600"
                && unix_mode(context, socket_parent) == "700",
            "native_ipc_unix_permissions"
        );
    }

    let dev_server = ();
    let dev_protocol = ();
    if ci_main_dev {
        print("STEP main and dev are distinct native authorities");
        let dev_workspace = std::path::join(
            context.run_directory,
            "workspace/dev-workspace.json"
        ).display;
        let dev_command = configured(
            context,
            server_exe,
            ["--instance", "dev"],
            dead_endpoint,
            context.address,
            "main",
            dev_workspace
        );
        dev_server = dev_command.start();
        owned_children.push(dev_server);
        dev_protocol = wait_protocol(
            context,
            cli,
            ["--instance", "dev"],
            dead_endpoint,
            context.address,
            "main",
            dev_workspace,
            dev_server
        );
        require(
            dev_protocol.instance == "dev"
                && dev_protocol.transport == native_protocol.transport
                && dev_protocol.endpoint != native_protocol.endpoint
                && dev_protocol.server_scope_id
                    != native_protocol.server_scope_id,
            "native_ipc_main_dev_authority_separation"
        );

        print("STEP duplicate instance authority fails closed and bounded");
        let duplicate_started =
            std::time::SystemTime::now().unix_millis;
        let duplicate = configured(
            context,
            server_exe,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace
        ).start();
        owned_children.push(duplicate);
        let duplicate_exit = wait_exit(
            duplicate,
            "native_ipc_duplicate_authority_timeout"
        );
        require(
            !duplicate_exit.success
                && std::time::SystemTime::now().unix_millis
                    - duplicate_started < 5000,
            "native_ipc_duplicate_authority_not_rejected"
        );
    }

    print("STEP explicit loopback TCP remains a live compatibility route");
    let tcp_command = configured(
        context,
        server_exe,
        ["--address", context.address],
        dead_endpoint,
        "127.0.0.1:1",
        custom_instance,
        tcp_workspace
    );
    let tcp_server = tcp_command.start();
    owned_children.push(tcp_server);
    let tcp_protocol = wait_protocol(
        context,
        cli,
        ["--endpoint", tcp_endpoint],
        dead_endpoint,
        "127.0.0.1:1",
        custom_instance,
        tcp_workspace,
        tcp_server
    );
    require(
        tcp_protocol.pid == tcp_server.id
            && tcp_protocol.endpoint == tcp_endpoint
            && tcp_protocol.transport == "tcp",
        "native_ipc_explicit_endpoint_precedence"
    );
    let legacy_protocol = invoke_json(
        context,
        cli,
        ["--address", context.address, "protocol-info", "--running"],
        dead_endpoint,
        "127.0.0.1:1",
        custom_instance,
        tcp_workspace
    );
    require(
        legacy_protocol.pid == tcp_server.id
            && legacy_protocol.endpoint == tcp_endpoint
            && legacy_protocol.transport == "tcp",
        "native_ipc_legacy_address_compatibility"
    );

    print("STEP environment endpoint priority never falls through to address");
    let priority = invoke(
        context,
        cli,
        ["protocol-info", "--running"],
        true,
        dead_endpoint,
        context.address,
        custom_instance,
        tcp_workspace
    );
    require(
        !priority.success,
        "native_ipc_environment_endpoint_fell_through"
    );

    print("STEP mixed schema discovery emits typed v2 rows deterministically");
    let schemas = registration_schema_counts(
        context.instance_directory
    );
    require(
        schemas.v1 == 1
            && schemas.v2 == (if ci_main_dev { 3 } else { 2 }),
        "native_ipc_mixed_registration_fixture"
    );
    let rows = invoke_json(
        context,
        cli,
        ["server-list", "--json"],
        "",
        "",
        "",
        native_workspace
    );
    require(
        rows.len == (if ci_main_dev { 3 } else { 2 }),
        "native_ipc_discovery_not_deduplicated"
    );
    let native_row = find_by_endpoint(
        rows,
        native_protocol.endpoint,
        "native_ipc_native_row_missing"
    );
    let tcp_row = find_by_endpoint(
        rows,
        tcp_endpoint,
        "native_ipc_tcp_row_missing"
    );
    let dev_row_ok = if ci_main_dev {
        let dev_row = find_by_endpoint(
            rows,
            dev_protocol.endpoint,
            "native_ipc_dev_row_missing"
        );
        dev_row.schema_version == 2
            && dev_row.registration_schema_version == 2
            && dev_row.instance == "dev"
            && dev_row.transport == dev_protocol.transport
            && dev_row.server_scope_id == dev_protocol.server_scope_id
            && dev_row.status == "running"
    } else {
        true
    };
    let typed_schema_ok =
        native_row.schema_version == 2
            && native_row.registration_schema_version == 2
            && native_row.instance == custom_instance
            && native_row.transport == native_protocol.transport
            && native_row.server_scope_id
                == native_protocol.server_scope_id
            && native_row.status == "running"
            && native_row.legacy_client_compatibility
                == "unsupported_no_legacy_listener"
            && tcp_row.schema_version == 2
            && tcp_row.registration_schema_version == 2
            && tcp_row.transport == "tcp"
            && tcp_row.endpoint == tcp_endpoint
            && tcp_row.status == "running"
            && tcp_row.legacy_client_compatibility
                == "same_tcp_endpoint"
            && dev_row_ok;

    print("STEP each authority shuts down through its selected endpoint");
    invoke(
        context,
        cli,
        ["--instance", custom_instance, "shutdown"],
        false,
        dead_endpoint,
        context.address,
        "main",
        native_workspace
    );
    if ci_main_dev {
        invoke(
            context,
            cli,
            ["--instance", "dev", "shutdown"],
            false,
            dead_endpoint,
            context.address,
            "main",
            native_workspace
        );
    }
    invoke(
        context,
        cli,
        ["--address", context.address, "shutdown"],
        false,
        dead_endpoint,
        "127.0.0.1:1",
        custom_instance,
        tcp_workspace
    );
    let native_exit = wait_exit(
        native_server,
        "native_ipc_native_shutdown_timeout"
    );
    let tcp_exit = wait_exit(
        tcp_server,
        "native_ipc_tcp_shutdown_timeout"
    );
    let dev_exit = if ci_main_dev {
        wait_exit(
            dev_server,
            "native_ipc_dev_shutdown_timeout"
        )
    } else {
        ()
    };
    require(
        native_exit.success
            && tcp_exit.success
            && (!ci_main_dev || dev_exit.success),
        "native_ipc_server_shutdown_failure"
    );
    require(
        registration_count(context.instance_directory) == 0,
        "native_ipc_registration_leak"
    );
    if native_protocol.transport == "unix" {
        let native_socket = native_protocol.endpoint.sub_string(5);
        require(
            !std::fs::exists(native_socket)
                && !std::fs::exists(native_socket + ".lock"),
            "native_ipc_unix_native_endpoint_leak"
        );
        if ci_main_dev {
            let dev_socket = dev_protocol.endpoint.sub_string(5);
            require(
                !std::fs::exists(dev_socket)
                    && !std::fs::exists(dev_socket + ".lock"),
                "native_ipc_unix_dev_endpoint_leak"
            );
        }
    }
    require(
        typed_schema_ok,
        "native_ipc_server_list_typed_schema"
    );

    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control.native-local-ipc"
    );
    succeeded = true;
} catch (error) {
    failure = error;
}

let result = test_harness::complete(
    context,
    owned_children,
    succeeded,
    failure
);
if succeeded {
    require(result.cleanup.orphan_free, "native_ipc_orphan_cleanup");
    test_harness::remove_run(context);
    print("PASS: native IPC selection and migration contract");
} else {
    throw failure;
}
