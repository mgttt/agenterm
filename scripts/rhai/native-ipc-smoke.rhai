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

fn bounded_diagnostic(value) {
    if value.len <= 512 {
        return value;
    }
    value.sub_string(0, 512) + "<truncated>"
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
        "AGENTERM_INSTANCE",
        "AGENTERM_SETTINGS_PATH"
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
        context.native_runtime_directory
    );
    command.env("LOCALAPPDATA", context.config_directory);
    command.env("XDG_CONFIG_HOME", context.config_directory);
    command.env("HOME", context.home_directory);
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

fn find_by_nonce(rows, nonce, code) {
    for row in rows {
        if row.lease_nonce == nonce {
            return row;
        }
    }
    throw code;
}

fn registration_by_endpoint(directory, endpoint, code) {
    for entry in std::fs::read_dir(directory) {
        if entry.is_file && entry.file_name.ends_with(".json") {
            let record = rhai::json::parse_file(entry.path.display);
            if record.schema_version == 2 && record.endpoint == endpoint {
                return #{ path: entry.path.display, record: record };
            }
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
context.config_directory = std::path::join(
    context.run_directory,
    "config"
).display;
std::fs::create_dir_all(context.config_directory);
context.home_directory = std::path::join(
    context.run_directory,
    "home"
).display;
std::fs::create_dir_all(context.home_directory);
let unix_bootstrap =
    std::env::has("AGENTERM_BOOTSTRAP_PLATFORM")
        && std::env::get("AGENTERM_BOOTSTRAP_PLATFORM") == "unix";
let macos_host =
    (
        std::env::has("AGENTERM_HOST_OS")
            && std::env::get("AGENTERM_HOST_OS") == "macos"
    )
        || std::fs::exists("/System/Library/CoreServices");
let direct_unix_root =
    if std::fs::exists("/private/tmp") {
        "/private/tmp"
    } else if std::fs::exists("/tmp") {
        "/tmp"
    } else {
        ""
    };
let windows_host = !unix_bootstrap
    && (
        (
            std::env::has("AGENTERM_HOST_OS")
                && std::env::get("AGENTERM_HOST_OS") == "windows"
        )
            || (
                std::env::has("OS")
                    && std::env::get("OS") == "Windows_NT"
            )
    );
let unix_host = unix_bootstrap
    || (!windows_host && direct_unix_root != "");
context.native_runtime_directory = if unix_bootstrap {
    let temporary_root =
        if std::env::has("AGENTERM_HOST_OS")
                && std::env::get("AGENTERM_HOST_OS") == "macos" {
            "/private/tmp"
        } else {
            "/tmp"
        };
    temporary_root + "/agt-" + std::process::id() + "-" +
        std::time::SystemTime::now().unix_millis
} else if unix_host {
    direct_unix_root + "/agt-" + std::process::id() + "-" +
        std::time::SystemTime::now().unix_millis
} else {
    std::path::join(context.run_directory, "runtime").display
};
context.native_runtime_external = unix_host;
std::fs::create_dir_all(context.native_runtime_directory);
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

    if unix_host {
        print("STEP overlong Unix endpoint fails typed and bounded");
        let long_endpoint =
            "unix:/this-path-is-intentionally-longer-than-the-portable-" +
            "sockaddr-un-limit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" +
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/server.sock";
        let long_path_started =
            std::time::SystemTime::now().unix_millis;
        let long_path = invoke(
            context,
            cli,
            [
                "--endpoint", long_endpoint,
                "protocol-info", "--running"
            ],
            true,
            "",
            "",
            "",
            native_workspace
        );
        let long_path_text = long_path.stdout + long_path.stderr;
        require(
            long_path_text.contains("InvalidEndpoint")
                && long_path_text.contains("invalid Unix socket path")
                && std::time::SystemTime::now().unix_millis
                    - long_path_started < 2000,
            "native_ipc_unix_long_path_not_typed_or_bounded"
        );

        print("STEP symlink Unix runtime ancestry fails closed");
        let symlink_target =
            context.native_runtime_directory + "-symlink-target";
        let symlink_runtime =
            context.native_runtime_directory + "-symlink";
        std::fs::create_dir_all(symlink_target);
        let link_command = std::process::command("ln");
        link_command.args(["-s", symlink_target, symlink_runtime]);
        link_command.timeout(std::time::Duration::from_secs(2));
        let link_output = link_command.output();
        test_harness::append_command_record(
            context,
            ["-s", symlink_target, symlink_runtime],
            link_output,
            !link_output.success,
            []
        );
        require(
            link_output.success
                && std::fs::symlink_metadata(symlink_runtime).is_symlink,
            "native_ipc_unix_symlink_fixture"
        );
        let symlink_endpoint =
            "unix:" + symlink_runtime + "/server.sock";
        let symlink_server = configured(
            context,
            server_exe,
            [],
            symlink_endpoint,
            "",
            "",
            native_workspace
        ).start();
        owned_children.push(symlink_server);
        let symlink_exit = wait_exit(
            symlink_server,
            "native_ipc_unix_symlink_rejection_timeout"
        );
        require(
            !symlink_exit.success
                && symlink_exit.stderr_text().contains("UnsafeEndpoint")
                && symlink_exit.stderr_text().contains("symlink"),
            "native_ipc_unix_symlink_not_rejected"
        );
        std::fs::remove_file(symlink_runtime);
        std::fs::remove_dir_all(symlink_target);
    }

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
    let settings_root = if native_protocol.transport == "named_pipe" {
        context.config_directory
    } else if macos_host {
        std::path::join(
            std::path::join(context.home_directory, "Library").display,
            "Application Support"
        ).display
    } else {
        context.config_directory
    };
    let settings_base = std::path::join(
        settings_root,
        if native_protocol.transport == "named_pipe" {
            "AgenTerm"
        } else {
            "agenterm"
        }
    ).display;
    let expected_native_settings = if custom_instance == "main" {
        std::path::join(settings_base, "settings.json").display
    } else {
        std::path::join(
            std::path::join(settings_base, "settings").display,
            native_protocol.server_scope_id + ".json"
        ).display
    };
    let expected_native_settings_suffix = std::path::join(
        "settings",
        native_protocol.server_scope_id + ".json"
    ).display;
    require(
        native_protocol.pid == native_server.id
            && native_protocol.instance == custom_instance
            && native_endpoint_shape
            && native_protocol.server_scope_id.starts_with("agt-v1-")
            && native_protocol.server_scope_id.len == 39
            && !native_protocol.endpoint.contains(custom_instance),
        "native_ipc_custom_native_contract"
    );
    let native_settings_match =
        native_protocol.settings_path == expected_native_settings
            || (
                custom_instance != "main"
                    && native_protocol.settings_path.ends_with(
                        expected_native_settings_suffix
                    )
            );
    require(
        native_settings_match,
        "native_ipc_custom_native_settings_path:expected=" +
            bounded_diagnostic(expected_native_settings) +
            ":actual=" +
            bounded_diagnostic(native_protocol.settings_path)
    );
    if native_protocol.transport == "unix" {
        let socket_path = native_protocol.endpoint.sub_string(5);
        let socket_parent = std::path::parent(socket_path).display;
        require(
            unix_mode(context, socket_path) == "600"
                && unix_mode(context, socket_parent) == "700",
            "native_ipc_unix_permissions"
        );
        if context.native_runtime_directory.starts_with("/private/tmp/") {
            print("STEP /tmp and /private/tmp resolve one macOS authority");
            let alias_runtime =
                "/tmp" + context.native_runtime_directory.sub_string(
                    "/private/tmp".len
                );
            let alias_command = configured(
                context,
                cli,
                [
                    "--instance", custom_instance,
                    "protocol-info", "--running"
                ],
                dead_endpoint,
                context.address,
                "main",
                native_workspace
            );
            alias_command.env("XDG_RUNTIME_DIR", alias_runtime);
            alias_command.timeout(std::time::Duration::from_secs(2));
            let alias_output = alias_command.output();
            test_harness::append_command_record(
                context,
                ["--instance", custom_instance, "protocol-info", "--running"],
                alias_output,
                !alias_output.success,
                []
            );
            require(
                alias_output.success,
                "native_ipc_macos_tmp_alias_unreachable:" +
                    alias_output.stderr_text()
            );
            let alias_protocol = rhai::json::parse(
                alias_output.stdout_text() + alias_output.stderr_text()
            );
            require(
                alias_protocol.pid == native_protocol.pid
                    && alias_protocol.endpoint == native_protocol.endpoint
                    && alias_protocol.server_scope_id
                        == native_protocol.server_scope_id,
                "native_ipc_macos_tmp_alias_split_authority"
            );
        }
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
                    != native_protocol.server_scope_id
                && dev_protocol.settings_path == std::path::join(
                    std::path::join(settings_base, "settings").display,
                    dev_protocol.server_scope_id + ".json"
                ).display
                && dev_protocol.settings_path
                    != native_protocol.settings_path,
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

    print("STEP explicit settings path overrides role-scoped default");
    let override_instance =
        "custom:settings-override-" + std::process::id() + "-" +
        std::time::SystemTime::now().unix_millis;
    let override_workspace = std::path::join(
        context.run_directory,
        "workspace/settings-override-workspace.json"
    ).display;
    let override_command = configured(
        context,
        server_exe,
        ["--instance", override_instance],
        dead_endpoint,
        context.address,
        "main",
        override_workspace
    );
    override_command.env("AGENTERM_SETTINGS_PATH", context.settings_path);
    let override_server = override_command.start();
    owned_children.push(override_server);
    let override_protocol = wait_protocol(
        context,
        cli,
        ["--instance", override_instance],
        dead_endpoint,
        context.address,
        "main",
        override_workspace,
        override_server
    );
    require(
        override_protocol.settings_path == context.settings_path,
        "native_ipc_explicit_settings_override_lost"
    );
    invoke(
        context,
        cli,
        ["--instance", override_instance, "shutdown"],
        false,
        dead_endpoint,
        context.address,
        "main",
        override_workspace
    );
    require(
        wait_exit(
            override_server,
            "native_ipc_settings_override_shutdown_timeout"
        ).success,
        "native_ipc_settings_override_shutdown_failed"
    );

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
            && dev_row.classification == "live"
            && dev_row.process_start_identity != ""
            && dev_row.lease_nonce != ""
            && dev_row.server_epoch == dev_row.event_epoch
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
            && native_row.classification == "live"
            && native_row.owner_state == "confirmed_live"
            && native_row.process_start_identity != ""
            && native_row.lease_nonce != ""
            && native_row.server_epoch == native_row.event_epoch
            && native_row.observed_protocol_pid == native_protocol.pid
            && native_row.observed_protocol_endpoint
                == native_protocol.endpoint
            && native_row.observed_protocol_server_scope_id
                == native_protocol.server_scope_id
            && native_row.legacy_client_compatibility
                == "unsupported_no_legacy_listener"
            && tcp_row.schema_version == 2
            && tcp_row.registration_schema_version == 2
            && tcp_row.transport == "tcp"
            && tcp_row.endpoint == tcp_endpoint
            && tcp_row.status == "running"
            && tcp_row.classification == "live"
            && tcp_row.process_start_identity != ""
            && tcp_row.lease_nonce != ""
            && tcp_row.server_epoch == tcp_row.event_epoch
            && tcp_row.legacy_client_compatibility
                == "same_tcp_endpoint"
            && dev_row_ok;

    print("STEP stale test fixture remains visible until confirmed cleanup");
    let stale_fixture_path = std::path::join(
        context.instance_directory,
        "4294967295-stale-fixture.json"
    ).display;
    let stale_fixture_endpoint = "tcp:127.0.0.1:59998";
    let stale_fixture_nonce = "native-ipc-stale-fixture";
    rhai::runtime::atomic_write(
        stale_fixture_path,
        rhai::json::stringify_pretty(#{
            schema_version: 2,
            pid: 4294967295,
            address: "127.0.0.1:59998",
            endpoint: stale_fixture_endpoint,
            logical_instance: "custom:stale-fixture",
            server_scope_id: native_protocol.server_scope_id,
            version: native_protocol.agenterm_version,
            session: "stale-fixture",
            workspace_path: "stale-fixture-workspace.json",
            started_at_unix_ms: 1,
            process_start_identity: "fixture-process-start",
            lease_nonce: stale_fixture_nonce,
            server_epoch: "fixture-server-epoch",
            test_fixture: true
        }) + "\n"
    );
    let stale_rows = invoke_json(
        context,
        cli,
        ["server-list", "--json"],
        "",
        "",
        "",
        native_workspace
    );
    let stale_row = find_by_nonce(
        stale_rows,
        stale_fixture_nonce,
        "native_ipc_stale_fixture_row_missing"
    );
    require(
        stale_row.classification == "stale-test-fixture"
            && stale_row.cleanup_eligible
            && std::fs::exists(stale_fixture_path),
        "native_ipc_stale_fixture_implicitly_removed"
    );
    let pruned_stale_rows = invoke_json(
        context,
        cli,
        ["server-list", "--json", "--prune"],
        "",
        "",
        "",
        native_workspace
    );
    let pruned_stale = find_by_nonce(
        pruned_stale_rows,
        stale_fixture_nonce,
        "native_ipc_stale_fixture_cleanup_receipt_missing"
    );
    require(
        pruned_stale.cleanup_receipt.schema_version == 1
            && pruned_stale.cleanup_receipt.operation
                == "registration.cleanup"
            && pruned_stale.cleanup_receipt.result == "deleted"
            && pruned_stale.cleanup_receipt.expected_lease_nonce
                == stale_fixture_nonce
            && !std::fs::exists(stale_fixture_path),
        "native_ipc_stale_fixture_cleanup_not_confirmed"
    );

    print("STEP legacy live-PID ownership remains unknown and ineligible");
    let unknown_owner_path = std::path::join(
        context.instance_directory,
        native_protocol.pid + "-owner-unknown.json"
    ).display;
    let unknown_owner_endpoint = "tcp:127.0.0.1:59997";
    rhai::runtime::atomic_write(
        unknown_owner_path,
        rhai::json::stringify_pretty(#{
            schema_version: 1,
            pid: native_protocol.pid,
            address: "127.0.0.1:59997",
            version: native_protocol.agenterm_version,
            session: "owner-unknown",
            workspace_path: "owner-unknown-workspace.json",
            started_at_unix_ms: 2
        }) + "\n"
    );
    let unknown_owner_rows = invoke_json(
        context,
        cli,
        ["server-list", "--json", "--prune"],
        "",
        "",
        "",
        native_workspace
    );
    let unknown_owner = find_by_endpoint(
        unknown_owner_rows,
        unknown_owner_endpoint,
        "native_ipc_owner_unknown_row_missing"
    );
    require(
        unknown_owner.classification == "owner-unknown"
            && !unknown_owner.cleanup_eligible
            && unknown_owner.cleanup_receipt.result == "not_eligible"
            && std::fs::exists(unknown_owner_path),
        "native_ipc_owner_unknown_was_removed"
    );
    std::fs::remove_file(unknown_owner_path);

    print("STEP PID-reuse cleanup cannot remove the live generation");
    let native_registration = registration_by_endpoint(
        context.instance_directory,
        native_protocol.endpoint,
        "native_ipc_live_registration_missing"
    );
    let pid_reuse_nonce = "native-ipc-pid-reuse-fixture";
    let pid_reuse_path = std::path::join(
        context.instance_directory,
        native_protocol.pid + "-" + pid_reuse_nonce + ".json"
    ).display;
    let pid_reuse_record = native_registration.record;
    pid_reuse_record.process_start_identity = "foreign-process-start";
    pid_reuse_record.lease_nonce = pid_reuse_nonce;
    pid_reuse_record.test_fixture = false;
    rhai::runtime::atomic_write(
        pid_reuse_path,
        rhai::json::stringify_pretty(pid_reuse_record) + "\n"
    );
    let pid_reuse_rows = invoke_json(
        context,
        cli,
        ["server-list", "--json"],
        "",
        "",
        "",
        native_workspace
    );
    let pid_reuse = find_by_nonce(
        pid_reuse_rows,
        pid_reuse_nonce,
        "native_ipc_pid_reuse_row_missing"
    );
    require(
        pid_reuse.classification == "stale"
            && pid_reuse.owner_state == "pid_reused"
            && std::fs::exists(native_registration.path),
        "native_ipc_pid_reuse_not_classified"
    );
    let pid_reuse_pruned_rows = invoke_json(
        context,
        cli,
        ["server-list", "--json", "--prune"],
        "",
        "",
        "",
        native_workspace
    );
    let pid_reuse_pruned = find_by_nonce(
        pid_reuse_pruned_rows,
        pid_reuse_nonce,
        "native_ipc_pid_reuse_receipt_missing"
    );
    require(
        pid_reuse_pruned.cleanup_receipt.result == "deleted"
            && std::fs::exists(native_registration.path)
            && !std::fs::exists(pid_reuse_path)
            && native_server.state == "running",
        "native_ipc_pid_reuse_cleanup_crossed_generation"
    );

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
    if native_protocol.transport == "named_pipe" {
        print("STEP killed Windows authority recovers its exact stale registration");
        let stale_command = configured(
            context,
            server_exe,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace
        );
        let stale_server = stale_command.start();
        owned_children.push(stale_server);
        let stale_protocol = wait_protocol(
            context,
            cli,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace,
            stale_server
        );
        let stale_registration = registration_by_endpoint(
            context.instance_directory,
            stale_protocol.endpoint,
            "native_ipc_windows_live_registration_missing"
        );
        let stale_nonce = stale_registration.record.lease_nonce;
        let stale_epoch = stale_registration.record.server_epoch;
        require(
            stale_registration.record.pid == stale_protocol.pid
                && stale_nonce != ""
                && stale_epoch != ""
                && std::fs::exists(stale_registration.path),
            "native_ipc_windows_live_registration_identity"
        );

        stale_server.kill();
        let stale_exit = wait_exit(
            stale_server,
            "native_ipc_windows_authority_kill_timeout"
        );
        let dead_rows = invoke_json(
            context,
            cli,
            ["server-list", "--json"],
            "",
            "",
            "",
            native_workspace
        );
        let dead_row = find_by_nonce(
            dead_rows,
            stale_nonce,
            "native_ipc_windows_stale_registration_missing"
        );
        require(
            !stale_exit.success
                && dead_row.classification == "stale"
                && dead_row.cleanup_eligible
                && dead_row.server_epoch == stale_epoch
                && std::fs::exists(stale_registration.path)
                && registration_count(context.instance_directory) == 1,
            "native_ipc_windows_real_stale_registration"
        );

        let replacement_command = configured(
            context,
            server_exe,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace
        );
        let replacement = replacement_command.start();
        owned_children.push(replacement);
        let replacement_protocol = wait_protocol(
            context,
            cli,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace,
            replacement
        );
        let replacement_registration = registration_by_endpoint(
            context.instance_directory,
            replacement_protocol.endpoint,
            "native_ipc_windows_replacement_registration_missing"
        );
        let replacement_rows = invoke_json(
            context,
            cli,
            ["server-list", "--json"],
            "",
            "",
            "",
            native_workspace
        );
        let replacement_row = find_by_nonce(
            replacement_rows,
            replacement_registration.record.lease_nonce,
            "native_ipc_windows_replacement_row_missing"
        );
        require(
            replacement_protocol.endpoint == stale_protocol.endpoint
                && replacement_protocol.server_scope_id
                    == stale_protocol.server_scope_id
                && replacement_protocol.pid != stale_protocol.pid
                && replacement_registration.record.pid
                    == replacement_protocol.pid
                && replacement_registration.record.lease_nonce
                    != stale_nonce
                && replacement_registration.record.server_epoch
                    != stale_epoch
                && replacement_row.classification == "live"
                && replacement_row.server_epoch
                    == replacement_row.event_epoch
                && !std::fs::exists(stale_registration.path)
                && registration_count(context.instance_directory) == 1,
            "native_ipc_windows_stale_authority_not_recovered"
        );
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
        let replacement_exit = wait_exit(
            replacement,
            "native_ipc_windows_replacement_shutdown_timeout"
        );
        let stopped = probe(
            context,
            cli,
            ["--instance", custom_instance, "protocol-info", "--running"],
            dead_endpoint,
            context.address,
            "main",
            native_workspace
        );
        require(
            replacement_exit.success
                && !stopped.success
                && registration_count(context.instance_directory) == 0,
            "native_ipc_windows_stale_recovery_cleanup"
        );
    }
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

        print("STEP killed Unix authority recovers stale endpoint and registration");
        let stale_command = configured(
            context,
            server_exe,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace
        );
        let stale_server = stale_command.start();
        owned_children.push(stale_server);
        let stale_protocol = wait_protocol(
            context,
            cli,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace,
            stale_server
        );
        stale_server.kill();
        let stale_exit = wait_exit(
            stale_server,
            "native_ipc_stale_authority_kill_timeout"
        );
        require(
            !stale_exit.success
                && std::fs::exists(native_socket)
                && std::fs::exists(native_socket + ".lock")
                && registration_count(context.instance_directory) == 1,
            "native_ipc_stale_authority_fixture"
        );

        let replacement_command = configured(
            context,
            server_exe,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace
        );
        let replacement = replacement_command.start();
        owned_children.push(replacement);
        let replacement_protocol = wait_protocol(
            context,
            cli,
            ["--instance", custom_instance],
            dead_endpoint,
            context.address,
            "main",
            native_workspace,
            replacement
        );
        require(
            replacement_protocol.endpoint == stale_protocol.endpoint
                && replacement_protocol.server_scope_id
                    == stale_protocol.server_scope_id
                && replacement_protocol.pid != stale_protocol.pid,
            "native_ipc_stale_authority_not_recovered"
        );
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
        let replacement_exit = wait_exit(
            replacement,
            "native_ipc_stale_replacement_shutdown_timeout"
        );
        require(
            replacement_exit.success
                && registration_count(context.instance_directory) == 0
                && !std::fs::exists(native_socket)
                && !std::fs::exists(native_socket + ".lock"),
            "native_ipc_stale_recovery_cleanup"
        );
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
if context.native_runtime_external
        && std::fs::exists(context.native_runtime_directory) {
    std::fs::remove_dir_all(context.native_runtime_directory);
}
if succeeded {
    require(result.cleanup.orphan_free, "native_ipc_orphan_cleanup");
    test_harness::remove_run(context);
    print("PASS: native IPC selection and migration contract");
} else {
    throw failure;
}
