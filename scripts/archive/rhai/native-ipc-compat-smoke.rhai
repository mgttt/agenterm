// Prove published v0.1.10/v0.1.11 control clients and servers remain
// compatible with the exact staged HEAD bytes across the TCP-to-native IPC
// migration. Published archives are acquired by exact hash at invocation time;
// no historical executable is stored in the repository.
//
// Args:
//   --list-evidence
// or:
//   REPO [--head-server PATH] [--head-cli PATH] [--catalog PATH]

// Platform split:
// - Windows: v0.1.10 default TCP, v0.1.11 native IPC, and HEAD.
// - Linux/macOS: v0.1.11 native IPC and HEAD. The published v0.1.10 Unix
//   archives have no agenterm-server binary, so they cannot truthfully prove a
//   server/client compatibility journey.

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "control.native-ipc-published-compatibility"
];

if args.len == 1 && args[0] == "--list-evidence" {
    print("control.native-ipc-published-compatibility");
    return;
}

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn command_output(context, program, arguments, expected_failure, seconds) {
    let command = std::process::command(program);
    command.args(arguments);
    command.timeout(std::time::Duration::from_secs(seconds));
    command.capture_limit(262144);
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
            "native_ipc_compat_unexpected_success:" + program
        );
    } else {
        require(
            output.success,
            "native_ipc_compat_command_failed:" + program + ":" +
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

fn host_facts(context) {
    let os_name = if std::env::has("AGENTERM_HOST_OS") {
        std::env::get("AGENTERM_HOST_OS")
    } else if std::fs::exists("/System/Library/CoreServices") {
        "macos"
    } else if std::fs::exists("/proc") {
        "linux"
    } else {
        "windows"
    };
    let arch = if std::env::has("AGENTERM_HOST_ARCH") {
        std::env::get("AGENTERM_HOST_ARCH")
    } else if std::env::has("PROCESSOR_ARCHITECTURE") {
        let value = std::env::get("PROCESSOR_ARCHITECTURE").to_lower();
        if value.contains("arm64") {
            "aarch64"
        } else {
            "x86_64"
        }
    } else {
        let output = command_output(
            context,
            "uname",
            ["-m"],
            false,
            2
        );
        let value = output.stdout;
        value.trim();
        if value == "arm64" || value == "aarch64" {
            "aarch64"
        } else {
            "x86_64"
        }
    };
    require(
        os_name == "windows" || os_name == "linux" || os_name == "macos",
        "native_ipc_compat_host_os:" + os_name
    );
    require(
        arch == "x86_64" || arch == "aarch64",
        "native_ipc_compat_host_arch:" + arch
    );
    #{ os: os_name, arch: arch }
}

fn release_by_version(catalog, version) {
    let matches = [];
    for release in catalog.releases {
        if release.version == version {
            matches.push(release);
        }
    }
    require(
        matches.len == 1,
        "native_ipc_compat_release_match:" + version
    );
    matches[0]
}

fn platform_asset(release, facts) {
    let matches = [];
    for asset in release.assets {
        if asset.os == facts.os && asset.arch == facts.arch {
            matches.push(asset);
        }
    }
    require(
        matches.len == 1,
        "native_ipc_compat_asset_match:" + release.version + ":" +
            facts.os + ":" + facts.arch
    );
    matches[0]
}

fn acquire_release(context, catalog, release, asset, facts) {
    let downloads = std::path::join(
        context.run_directory,
        "published-downloads"
    ).display;
    let extracted = std::path::join(
        context.run_directory,
        "published-" + release.version
    ).display;
    std::fs::create_dir_all(downloads);
    std::fs::create_dir_all(extracted);
    let archive = std::path::join(downloads, asset.name).display;
    let url = "https://github.com/" + catalog.repository +
        "/releases/download/" + release.tag + "/" + asset.name;
    let curl = if facts.os == "windows" { "curl.exe" } else { "curl" };
    command_output(
        context,
        curl,
        [
            "--fail", "--location", "--silent", "--show-error",
            "--max-time", "120",
            "--max-filesize", asset.size.to_string(),
            "--output", archive,
            url
        ],
        false,
        130
    );
    require(
        std::fs::metadata(archive).len == asset.size,
        "native_ipc_compat_archive_size:" + release.version
    );
    require(
        rhai::crypto::sha256_file(archive) == asset.sha256,
        "native_ipc_compat_archive_hash:" + release.version
    );
    let tar = if facts.os == "windows" { "tar.exe" } else { "tar" };
    command_output(
        context,
        tar,
        ["-xf", archive, "-C", extracted],
        false,
        30
    );
    let suffix = if facts.os == "windows" { ".exe" } else { "" };
    // Historical releases ship agenterm-server; HEAD consolidated to agenterm.
    let legacy_server = std::path::join(
        extracted,
        "agenterm-server" + suffix
    ).display;
    let consolidated_server = std::path::join(
        extracted,
        "agenterm" + suffix
    ).display;
    let server = if std::fs::exists(legacy_server) {
        legacy_server
    } else {
        consolidated_server
    };
    let cli = std::path::join(
        extracted,
        "agenterm-cli" + suffix
    ).display;
    require(
        std::fs::exists(server),
        "native_ipc_compat_published_server_missing:" + release.version
    );
    require(
        std::fs::exists(cli),
        "native_ipc_compat_published_cli_missing:" + release.version
    );
    let version_output = command_output(
        context,
        cli,
        ["--version"],
        false,
        5
    );
    require(
        (version_output.stdout + version_output.stderr).contains(
            release.version
        ),
        "native_ipc_compat_published_version:" + release.version
    );
    #{ server: server, cli: cli }
}

fn git_head(context, repo) {
    let command = std::process::command("git");
    command.args(["rev-parse", "HEAD"]);
    command.current_dir(repo);
    command.timeout(std::time::Duration::from_secs(5));
    let output = command.output();
    test_harness::append_command_record(
        context,
        ["rev-parse", "HEAD"],
        output,
        !output.success,
        []
    );
    require(output.success, "native_ipc_compat_git_head");
    let value = output.stdout_text();
    value.trim();
    value
}

fn executable_hash(metadata, path) {
    let name = std::path::PathBuf::from(path).file_name;
    let matches = [];
    for executable in metadata.executables {
        if executable.name == name {
            matches.push(executable);
        }
    }
    require(
        matches.len == 1,
        "native_ipc_compat_head_metadata_entry:" + name
    );
    matches[0].sha256
}

fn verify_head(context, repo, server, cli, metadata_path) {
    require(
        std::fs::exists(server) && std::fs::exists(cli),
        "native_ipc_compat_head_binaries_missing"
    );
    require(
        std::fs::exists(metadata_path),
        "native_ipc_compat_head_metadata_missing"
    );
    let metadata = rhai::json::parse_file(metadata_path);
    require(
        metadata.schema_version == 2
            && metadata.git_commit == git_head(context, repo),
        "native_ipc_compat_head_source_identity"
    );
    require(
        rhai::crypto::sha256_file(server) == executable_hash(metadata, server),
        "native_ipc_compat_head_server_hash"
    );
    require(
        rhai::crypto::sha256_file(cli) == executable_hash(metadata, cli),
        "native_ipc_compat_head_cli_hash"
    );
}

fn authority_argv(program, arguments) {
    let name = std::path::PathBuf::from(program).file_name;
    if name == "agenterm" || name == "agenterm.exe" {
        let server_args = ["server"];
        for argument in arguments {
            server_args.push(argument);
        }
        return server_args;
    }
    arguments
}

fn configured(
    context,
    program,
    arguments,
    runtime_directory,
    config_directory,
    workspace,
    legacy_address
) {
    let command = test_harness::configured_command(
        context,
        program,
        authority_argv(program, arguments)
    );
    for name in [
        "AGENTERM_IPC_ENDPOINT",
        "AGENTERM_IPC_ADDRESS",
        "AGENTERM_INSTANCE"
    ] {
        command.env_remove(name);
    }
    if legacy_address != "" {
        command.env("AGENTERM_IPC_ADDRESS", legacy_address);
    }
    command.env("AGENTERM_WORKSPACE_PATH", workspace);
    command.env("AGENTERM_SETTINGS_PATH", workspace + ".settings.json");
    command.env("AGENTERM_INSTANCE_DIR", context.instance_directory);
    command.env("XDG_RUNTIME_DIR", runtime_directory);
    command.env("LOCALAPPDATA", config_directory);
    command.env("XDG_CONFIG_HOME", config_directory);
    command.env("AGENTERM_NO_ACTIVATE", "1");
    command
}

fn invoke(
    context,
    program,
    arguments,
    expected_failure,
    runtime_directory,
    config_directory,
    workspace,
    legacy_address
) {
    let command = configured(
        context,
        program,
        arguments,
        runtime_directory,
        config_directory,
        workspace,
        legacy_address
    );
    command.timeout(std::time::Duration::from_secs(10));
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
            "native_ipc_compat_cli_unexpected_success:" + program
        );
    } else {
        require(
            output.success,
            "native_ipc_compat_cli_failed:" + program + ":" +
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
    program,
    arguments,
    runtime_directory,
    config_directory,
    workspace,
    legacy_address
) {
    let output = invoke(
        context,
        program,
        arguments,
        false,
        runtime_directory,
        config_directory,
        workspace,
        legacy_address
    );
    rhai::json::parse(output.stdout + output.stderr)
}

fn wait_protocol(
    context,
    cli,
    selector,
    runtime_directory,
    config_directory,
    workspace,
    legacy_address,
    server
) {
    for attempt in 0..200 {
        let arguments = selector;
        arguments.push("protocol-info");
        arguments.push("--running");
        let command = configured(
            context,
            cli,
            arguments,
            runtime_directory,
            config_directory,
            workspace,
            legacy_address
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
        if output.success {
            return rhai::json::parse(
                output.stdout_text() + output.stderr_text()
            );
        }
        if server.state == "exited" {
            let ended = server.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw "native_ipc_compat_server_early_exit:" +
                ended.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "native_ipc_compat_server_ready_timeout";
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

fn shutdown(
    context,
    cli,
    selector,
    runtime_directory,
    config_directory,
    workspace,
    legacy_address,
    server,
    code
) {
    let arguments = selector;
    arguments.push("shutdown");
    invoke(
        context,
        cli,
        arguments,
        false,
        runtime_directory,
        config_directory,
        workspace,
        legacy_address
    );
    require(wait_exit(server, code).success, code + "_failure");
}

require(args.len >= 1, "expected: REPO [OPTIONS]");
let repo = std::path::absolute(args[0]).display;
let catalog_path = std::path::join(
    repo,
    "scripts/native-ipc-compat-assets.json"
).display;
let head_server = "";
let head_cli = "";
let head_metadata = "";
let index = 1;
while index < args.len {
    let option = args[index];
    require(
        option == "--head-server" || option == "--head-cli" ||
            option == "--head-metadata" ||
            option == "--catalog",
        "native_ipc_compat_option:" + option
    );
    require(
        index + 1 < args.len,
        "native_ipc_compat_option_value:" + option
    );
    if option == "--head-server" {
        head_server = args[index + 1];
    } else if option == "--head-cli" {
        head_cli = args[index + 1];
    } else if option == "--head-metadata" {
        head_metadata = args[index + 1];
    } else {
        catalog_path = args[index + 1];
    }
    index += 2;
}

let context = test_harness::new_context(repo, "native-ipc-compat");
let facts = host_facts(context);
let suffix = if facts.os == "windows" { ".exe" } else { "" };
if head_server == "" {
    head_server = std::path::join(
        repo,
        "dist/agenterm" + suffix
    ).display;
} else {
    head_server = std::path::absolute(head_server).display;
}
if head_cli == "" {
    head_cli = std::path::join(
        repo,
        "dist/agenterm-cli" + suffix
    ).display;
} else {
    head_cli = std::path::absolute(head_cli).display;
}
if head_metadata == "" {
    head_metadata = std::path::join(repo, "dist/agenterm.json").display;
} else {
    head_metadata = std::path::absolute(head_metadata).display;
}
let catalog = rhai::json::parse_file(
    std::path::absolute(catalog_path).display
);
require(
    catalog.schema_version == 1 && catalog.repository == "mgttt/agenterm",
    "native_ipc_compat_catalog_identity"
);
let release_110 = release_by_version(catalog, "0.1.10");
let release_111 = release_by_version(catalog, "0.1.11");
let asset_111 = platform_asset(release_111, facts);
let runtime_directory = if facts.os == "windows" {
    std::path::join(context.run_directory, "runtime").display
} else {
    let root = if std::fs::exists("/private/tmp") {
        "/private/tmp"
    } else {
        "/tmp"
    };
    root + "/agt-compat-" + std::process::id() + "-" +
        std::time::SystemTime::now().unix_millis
};
let config_directory = std::path::join(
    context.run_directory,
    "config"
).display;
std::fs::create_dir_all(runtime_directory);
std::fs::create_dir_all(config_directory);
let native_workspace = std::path::join(
    context.run_directory,
    "workspace/native-compat.json"
).display;
let tcp_workspace = std::path::join(
    context.run_directory,
    "workspace/tcp-compat.json"
).display;
let native_instance = "custom:published-compat-" + context.run_id;
let tcp_endpoint = "tcp:" + context.address;
let dead_address = "127.0.0.1:1";
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP verify exact staged HEAD artifact bytes");
    verify_head(context, repo, head_server, head_cli, head_metadata);

    let published_111 = acquire_release(
        context,
        catalog,
        release_111,
        asset_111,
        facts
    );

    if facts.os == "windows" {
        print("STEP published v0.1.10 default TCP accepts the HEAD client");
        let asset_110 = platform_asset(release_110, facts);
        let published_110 = acquire_release(
            context,
            catalog,
            release_110,
            asset_110,
            facts
        );
        let old_command = configured(
            context,
            published_110.server,
            [],
            runtime_directory,
            config_directory,
            tcp_workspace,
            context.address
        );
        let old_server = old_command.start();
        owned_children.push(old_server);
        let old_protocol = wait_protocol(
            context,
            published_110.cli,
            [],
            runtime_directory,
            config_directory,
            tcp_workspace,
            context.address,
            old_server
        );
        let head_to_old = wait_protocol(
            context,
            head_cli,
            ["--endpoint", tcp_endpoint],
            runtime_directory,
            config_directory,
            tcp_workspace,
            "",
            old_server
        );
        require(
            old_protocol.pid == old_server.id
                && head_to_old.pid == old_server.id,
            "native_ipc_compat_110_tcp_identity"
        );
        shutdown(
            context,
            published_110.cli,
            [],
            runtime_directory,
            config_directory,
            tcp_workspace,
            context.address,
            old_server,
            "native_ipc_compat_110_shutdown"
        );

        print("STEP published v0.1.10 client reaches explicit HEAD TCP");
        let head_tcp_command = configured(
            context,
            head_server,
            ["--address", context.address],
            runtime_directory,
            config_directory,
            tcp_workspace,
            ""
        );
        let head_tcp_server = head_tcp_command.start();
        owned_children.push(head_tcp_server);
        let old_to_head = wait_protocol(
            context,
            published_110.cli,
            [],
            runtime_directory,
            config_directory,
            tcp_workspace,
            context.address,
            head_tcp_server
        );
        require(
            old_to_head.pid == head_tcp_server.id,
            "native_ipc_compat_110_to_head_tcp_identity"
        );

        print("STEP legacy client fails safely beside native-only HEAD");
        let head_native_command = configured(
            context,
            head_server,
            ["--instance", native_instance],
            runtime_directory,
            config_directory,
            native_workspace,
            ""
        );
        let head_native_server = head_native_command.start();
        owned_children.push(head_native_server);
        let native_protocol = wait_protocol(
            context,
            head_cli,
            ["--instance", native_instance],
            runtime_directory,
            config_directory,
            native_workspace,
            "",
            head_native_server
        );
        let failed_started = std::time::SystemTime::now().unix_millis;
        invoke(
            context,
            published_110.cli,
            ["protocol-info", "--running"],
            true,
            runtime_directory,
            config_directory,
            native_workspace,
            dead_address
        );
        let native_after_failure = invoke_json(
            context,
            head_cli,
            [
                "--instance", native_instance,
                "protocol-info", "--running"
            ],
            runtime_directory,
            config_directory,
            native_workspace,
            ""
        );
        require(
            std::time::SystemTime::now().unix_millis - failed_started < 10000
                && native_after_failure.pid == native_protocol.pid
                && head_native_server.state == "running",
            "native_ipc_compat_legacy_native_fail_safe"
        );
        shutdown(
            context,
            head_cli,
            ["--instance", native_instance],
            runtime_directory,
            config_directory,
            native_workspace,
            "",
            head_native_server,
            "native_ipc_compat_head_native_shutdown"
        );
        shutdown(
            context,
            head_cli,
            ["--endpoint", tcp_endpoint],
            runtime_directory,
            config_directory,
            tcp_workspace,
            "",
            head_tcp_server,
            "native_ipc_compat_head_tcp_shutdown"
        );
    } else {
        print(
            "SKIP v0.1.10 Unix compatibility: published archive has no " +
            "agenterm-server"
        );
    }

    print("STEP published v0.1.11 native server accepts the HEAD client");
    let old_native_command = configured(
        context,
        published_111.server,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    let old_native_server = old_native_command.start();
    owned_children.push(old_native_server);
    let old_native_protocol = wait_protocol(
        context,
        published_111.cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        old_native_server
    );
    let head_to_old_native = wait_protocol(
        context,
        head_cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        old_native_server
    );
    require(
        old_native_protocol.pid == old_native_server.id
            && head_to_old_native.pid == old_native_server.id,
        "native_ipc_compat_111_to_head_native_identity"
    );
    let snapshot = invoke_json(
        context,
        head_cli,
        ["--instance", native_instance, "ui-snapshot"],
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    let tab_id = snapshot.active_tab_id;
    let baseline_marker = "compat-baseline-" + context.run_id;
    let head_marker = "compat-head-write-" + context.run_id;
    invoke(
        context,
        head_cli,
        [
            "--instance", native_instance,
            "rename-window", "-t", tab_id, baseline_marker
        ],
        false,
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    shutdown(
        context,
        published_111.cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        old_native_server,
        "native_ipc_compat_111_first_shutdown"
    );

    print("STEP upgrade to HEAD preserves state for published v0.1.11 client");
    let head_upgrade_command = configured(
        context,
        head_server,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    let head_upgrade_server = head_upgrade_command.start();
    owned_children.push(head_upgrade_server);
    let old_to_head_native = wait_protocol(
        context,
        published_111.cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        head_upgrade_server
    );
    let upgraded_names = invoke(
        context,
        published_111.cli,
        [
            "--instance", native_instance,
            "list-windows", "-F", "#{window_name}"
        ],
        false,
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    require(
        old_to_head_native.pid == head_upgrade_server.id
            && upgraded_names.stdout.contains(baseline_marker),
        "native_ipc_compat_upgrade_state"
    );

    print("STEP HEAD server writes rollback-visible persistent state");
    invoke(
        context,
        head_cli,
        [
            "--instance", native_instance,
            "rename-window", "-t", tab_id, head_marker
        ],
        false,
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    let head_written_names = invoke(
        context,
        published_111.cli,
        [
            "--instance", native_instance,
            "list-windows", "-F", "#{window_name}"
        ],
        false,
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    require(
        head_written_names.stdout.contains(head_marker)
            && !head_written_names.stdout.contains(baseline_marker),
        "native_ipc_compat_head_write_state"
    );
    shutdown(
        context,
        published_111.cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        head_upgrade_server,
        "native_ipc_compat_head_upgrade_shutdown"
    );

    print("STEP rollback lets both v0.1.11 and HEAD read HEAD-written state");
    let rollback_command = configured(
        context,
        published_111.server,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    let rollback_server = rollback_command.start();
    owned_children.push(rollback_server);
    let rollback_old_protocol = wait_protocol(
        context,
        published_111.cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        rollback_server
    );
    let rollback_head_protocol = wait_protocol(
        context,
        head_cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        rollback_server
    );
    let rollback_old_names = invoke(
        context,
        published_111.cli,
        [
            "--instance", native_instance,
            "list-windows", "-F", "#{window_name}"
        ],
        false,
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    let rollback_head_names = invoke(
        context,
        head_cli,
        [
            "--instance", native_instance,
            "list-windows", "-F", "#{window_name}"
        ],
        false,
        runtime_directory,
        config_directory,
        native_workspace,
        ""
    );
    require(
        rollback_old_protocol.pid == rollback_server.id
            && rollback_head_protocol.pid == rollback_server.id
            && rollback_old_names.stdout.contains(head_marker)
            && rollback_head_names.stdout.contains(head_marker)
            && !rollback_old_names.stdout.contains(baseline_marker)
            && !rollback_head_names.stdout.contains(baseline_marker),
        "native_ipc_compat_rollback_state"
    );
    shutdown(
        context,
        published_111.cli,
        ["--instance", native_instance],
        runtime_directory,
        config_directory,
        native_workspace,
        "",
        rollback_server,
        "native_ipc_compat_111_rollback_shutdown"
    );

    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control.native-ipc-published-compatibility"
    );
    print(
        "PASS: exact published IPC bytes interoperate with staged HEAD " +
        "through TCP migration, native upgrade, and rollback"
    );
    succeeded = true;
} catch (error) {
    failure = error.to_string();
}

let completion_failure = "";
try {
    test_harness::complete(context, owned_children, succeeded, failure);
} catch (error) {
    completion_failure = error.to_string();
}
if facts.os != "windows" && std::fs::exists(runtime_directory) {
    std::fs::remove_dir_all(runtime_directory);
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
