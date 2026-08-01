// Prove the Linux Control Center against one caller-selected native IPC
// authority. The live renderer owns both its structured frame facts and the
// exact PNG bytes. X11 additionally proves compositor-observed focus behavior;
// Wayland keeps the portable no-activate and same-owner focus-request proof.
//
// Args:
//   --list-evidence
// or:
//   REPO CONTROL_CENTER_EXE SERVER_EXE CLI_EXE [--backend auto|x11|wayland]
//
// Headless hosts and explicitly requested unavailable display backends return a
// typed Unsupported skip before creating a test run or any screenshot.

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "control-center.linux-caller-endpoint",
    "control-center.linux-renderer-evidence",
    "control-center.linux-native-cockpit-input",
    "control-center.linux-no-activate-focus-reuse",
    "control-center.linux-server-recovery",
    "control-center.linux-renderer-replacement",
    "control-center.linux-process-isolation"
];

if args.contains("--list-evidence") {
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

fn environment_or_empty(name) {
    if std::env::has(name) {
        std::env::get(name)
    } else {
        ""
    }
}

fn unsupported(code, message, backend) {
    print(
        "SKIP " + rhai::json::stringify(#{
            schema_version: 1,
            suite: "control-center-linux-smoke",
            result_class: "skipped",
            failure: #{
                kind: "Unsupported",
                code: code,
                message: message
            },
            display_backend: backend,
            evidence_emitted: []
        })
    );
}

fn native_command(context, program, registry, endpoint, backend, arguments) {
    let command =
        test_harness::configured_command(context, program, arguments);
    command.env_remove("AGENTERM_IPC_ADDRESS");
    command.env_remove("AGENTERM_IPC_ENDPOINT");
    command.env_remove("AGENTERM_INSTANCE");
    command.env("AGENTERM_CC_REGISTRY_PATH", registry);
    if backend == "x11" {
        command.env_remove("WAYLAND_DISPLAY");
    } else if backend == "wayland" {
        command.env_remove("DISPLAY");
    }
    command.timeout(std::time::Duration::from_secs(15));
    command
}

fn invoke(
    context,
    program,
    registry,
    endpoint,
    backend,
    arguments,
    expected_failure
) {
    let output = native_command(
        context, program, registry, endpoint, backend, arguments
    ).output();
    test_harness::append_command_record(
        context,
        arguments,
        output,
        expected_failure,
        [endpoint]
    );
    if expected_failure {
        require(!output.success, "expected command failure");
    } else {
        require(
            output.success,
            "command_failed:" + output.stdout_text() + output.stderr_text()
        );
    }
    output
}

fn json_command(
    context, program, registry, endpoint, backend, arguments
) {
    let output = invoke(
        context,
        program,
        registry,
        endpoint,
        backend,
        arguments,
        false
    );
    rhai::json::parse(output.stdout_text() + output.stderr_text())
}

fn wait_active_tab(
    context,
    control_center,
    registry,
    endpoint,
    backend,
    child,
    target_tab_id,
    expected_server_pid,
    expected_server_epoch,
    code
) {
    let last = "";
    for attempt in 0..160 {
        let arguments = ["snapshot", "--json", "--endpoint", endpoint];
        let output = native_command(
            context,
            control_center,
            registry,
            endpoint,
            backend,
            arguments
        ).output();
        last = output.stdout_text() + output.stderr_text();
        if output.success {
            let snapshot = rhai::json::parse(last);
            if snapshot.server_state == "connected"
                    && snapshot.connected_server.pid == expected_server_pid
                    && snapshot.connected_server.epoch == expected_server_epoch
                    && snapshot.connected_server.endpoint == endpoint
                    && snapshot.connected_server.active_tab_id == target_tab_id {
                test_harness::append_command_record(
                    context, arguments, output, false, [endpoint]
                );
                return snapshot;
            }
        }
        if child.state == "exited" {
            let exited = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_renderer_exited:" + exited.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code + "_timeout:" + last;
}

fn invoke_activate(
    context, program, registry, endpoint, backend, arguments
) {
    let command = native_command(
        context, program, registry, endpoint, backend, arguments
    );
    command.env_remove("AGENTERM_NO_ACTIVATE");
    let output = command.output();
    test_harness::append_command_record(
        context, arguments, output, false, [endpoint]
    );
    require(
        output.success,
        "activate_command_failed:" +
            output.stdout_text() + output.stderr_text()
    );
}

fn wait_protocol(
    context, cli, registry, endpoint, backend, child, code
) {
    for attempt in 0..240 {
        let arguments = [
            "--endpoint", endpoint, "protocol-info", "--running"
        ];
        let output = native_command(
            context, cli, registry, endpoint, backend, arguments
        ).output();
        if output.success {
            test_harness::append_command_record(
                context, arguments, output, false, [endpoint]
            );
            return rhai::json::parse(
                output.stdout_text() + output.stderr_text()
            );
        }
        if child.state == "exited" {
            let exited = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_early_exit:" + exited.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code + "_timeout";
}

fn wait_registry(path, expected_pid, child, code) {
    for attempt in 0..240 {
        if std::fs::exists(path) {
            try {
                let record = rhai::json::parse_file(path);
                if record.schema_version == 2
                        && record.pid == expected_pid
                        && record.process_start_identity != "" {
                    return record;
                }
            } catch (error) {
                // Atomic publication can be between create and parse.
            }
        }
        if child.state == "exited" {
            let exited = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_early_exit:" + exited.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw code + "_timeout";
}

fn wait_native_window(registry, expected_pid, child, code) {
    require(
        registry.ends_with(".json"),
        "control_center_linux_registry_extension"
    );
    let path = registry.sub_string(0, registry.len - 4) + "window";
    for attempt in 0..240 {
        if std::fs::exists(path) {
            let identity = std::fs::read_to_string(path);
            identity.trim();
            if identity != "" {
                return identity;
            }
        }
        if child.state == "exited" {
            let exited = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_early_exit:" + exited.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw code + "_timeout:" + expected_pid;
}

fn wait_exit(child, code) {
    for attempt in 0..240 {
        if child.state == "exited" {
            return child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
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
    for attempt in 0..240 {
        if !process_present(pid) {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw code;
}

fn wait_capture(
    context,
    control_center,
    registry,
    endpoint,
    backend,
    child,
    output,
    required_state,
    code
) {
    for attempt in 0..40 {
        let arguments = ["screenshot", "--output", output, "--json"];
        let capture = native_command(
            context,
            control_center,
            registry,
            endpoint,
            backend,
            arguments
        ).output();
        if capture.success {
            test_harness::append_command_record(
                context, arguments, capture, false, [endpoint]
            );
            let document = rhai::json::parse(
                capture.stdout_text() + capture.stderr_text()
            );
            if document.rendered_snapshot.server_state == required_state {
                return document;
            }
        }
        if child.state == "exited" {
            let exited = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_early_exit:" + exited.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code + "_timeout";
}

fn prove_pty(
    context, cli, registry, endpoint, backend, label
) {
    let marker = "AGENTERM_CC_LINUX_" + label + "_" + context.run_id;
    let created = invoke(
        context,
        cli,
        registry,
        endpoint,
        backend,
        [
            "--endpoint", endpoint,
            "new-window", "-d", "-P", "-F", "#{window_id}",
            "-n", "cc-" + label + "-" + context.run_id,
            "--", "/bin/echo", marker
        ],
        false
    );
    let tab_id = created.stdout_text();
    tab_id.trim();
    require(tab_id.starts_with("@"), "control_center_linux_stable_tab_id");
    invoke(
        context,
        cli,
        registry,
        endpoint,
        backend,
        [
            "--endpoint", endpoint,
            "wait-pane", "-t", tab_id,
            "--contains", marker,
            "--timeout-ms", "5000"
        ],
        false
    );
    let captured = invoke(
        context,
        cli,
        registry,
        endpoint,
        backend,
        [
            "--endpoint", endpoint,
            "capture-pane", "-p", "-t", tab_id,
            "--max-bytes", "16384"
        ],
        false
    );
    require(
        captured.stdout_text().contains(marker),
        "control_center_linux_pty_capture"
    );
    tab_id
}

fn tool_available(name) {
    let command = std::process::command("/bin/sh");
    command.args(["-c", "command -v -- " + name + " >/dev/null 2>&1"]);
    command.timeout(std::time::Duration::from_secs(2));
    command.output().success
}

fn xdotool(context, arguments, expected_failure) {
    let command = std::process::command("xdotool");
    command.args(arguments);
    command.timeout(std::time::Duration::from_secs(3));
    let output = command.output();
    test_harness::append_command_record(
        context, arguments, output, expected_failure, []
    );
    if !expected_failure {
        require(
            output.success,
            "control_center_linux_xdotool_failed:" + output.stderr_text()
        );
    }
    output
}

fn wait_x11_window(context, title, child, code) {
    let last = "";
    for attempt in 0..120 {
        let output = xdotool(
            context, ["search", "--onlyvisible", "--name", title], true
        );
        last = output.stdout_text() + output.stderr_text();
        if output.success {
            let lines = output.stdout_text().split("\n");
            for line in lines {
                line.trim();
                if line != "" {
                    let name = xdotool(
                        context, ["getwindowname", line], true
                    );
                    let name_text = name.stdout_text();
                    name_text.trim();
                    if name.success && name_text == title {
                        return line;
                    }
                }
            }
        }
        if child.state == "exited" {
            let exited = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_early_exit:" + exited.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code + ":" + last;
}

fn active_x11_window(context) {
    let output = xdotool(context, ["getactivewindow"], false);
    let value = output.stdout_text();
    value.trim();
    require(value != "", "control_center_linux_x11_active_window_empty");
    value
}

fn activate_x11_window(context, identity) {
    xdotool(
        context, ["windowactivate", "--sync", identity], false
    );
}

fn wait_x11_active(context, identity, code) {
    for attempt in 0..120 {
        if active_x11_window(context) == identity {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn start_x11_focus_witness(title) {
    let command = std::process::command("xmessage");
    command.args([
        "-name", "agenterm-focus-witness",
        "-title", title,
        "-center",
        "-buttons", "OK:0",
        title
    ]);
    command.timeout(std::time::Duration::from_secs(30));
    command.start()
}

fn remove_native_runtime(runtime) {
    require(
        runtime.starts_with("/tmp/agt-cc-linux-"),
        "control_center_linux_runtime_not_owned"
    );
    if std::fs::exists(runtime) {
        std::fs::remove_dir_all(runtime);
    }
}

fn prepare_private_runtime(runtime) {
    require(
        runtime.starts_with("/tmp/agt-cc-linux-"),
        "control_center_linux_runtime_not_owned"
    );
    std::fs::create_dir_all(runtime);
    let metadata = std::fs::symlink_metadata(runtime);
    require(
        metadata.is_dir
            && !metadata.is_symlink
            && !metadata.is_reparse_point,
        "control_center_linux_runtime_indirect"
    );

    // create_dir_all observes the runner umask, which is commonly 0022 on
    // Linux CI. The production Unix IPC contract intentionally rejects that
    // resulting 0755 directory, so make the smoke-owned authority explicitly
    // private before any server or client can observe it.
    let chmod = std::process::command("chmod");
    chmod.args(["0700", "--", runtime]);
    chmod.timeout(std::time::Duration::from_secs(2));
    let chmod_output = chmod.output();
    require(
        chmod_output.success,
        "control_center_linux_runtime_chmod_failed:" +
            chmod_output.stderr_text()
    );

    let id = std::process::command("id");
    id.args(["-u"]);
    id.timeout(std::time::Duration::from_secs(2));
    let id_output = id.output();
    require(
        id_output.success,
        "control_center_linux_effective_uid_failed:" +
            id_output.stderr_text()
    );
    let effective_uid = id_output.stdout_text();
    effective_uid.trim();
    require(
        effective_uid != "",
        "control_center_linux_effective_uid_empty"
    );

    let stat = std::process::command("stat");
    stat.args(["-c", "%u:%a", "--", runtime]);
    stat.timeout(std::time::Duration::from_secs(2));
    let stat_output = stat.output();
    require(
        stat_output.success,
        "control_center_linux_runtime_stat_failed:" +
            stat_output.stderr_text()
    );
    let ownership = stat_output.stdout_text();
    ownership.trim();
    require(
        ownership == effective_uid + ":700",
        "control_center_linux_runtime_not_private_to_effective_uid:" +
            ownership
    );
}

require(
    args.len >= 4,
    "expected: REPO CONTROL_CENTER_EXE SERVER_EXE CLI_EXE [--backend auto|x11|wayland]"
);
let repo = std::path::absolute(args[0]).display;
let control_center_value = args[1];
let server_executable_value = args[2];
let cli_executable_value = args[3];
let requested_backend = "auto";
let option = 4;
while option < args.len {
    require(option + 1 < args.len, "control_center_linux_option_value");
    let value = args[option + 1];
    if args[option] == "--backend" {
        require(
            value == "auto" || value == "x11" || value == "wayland",
            "control_center_linux_backend_invalid:" + value
        );
        requested_backend = value;
    } else if args[option] == "--control-center" {
        control_center_value = value;
    } else if args[option] == "--server" {
        server_executable_value = value;
    } else if args[option] == "--cli" {
        cli_executable_value = value;
    } else {
        throw "control_center_linux_unknown_option:" + args[option];
    }
    option += 2;
}

let x11_available = environment_or_empty("DISPLAY") != "";
let wayland_available = environment_or_empty("WAYLAND_DISPLAY") != "";
let backend = if requested_backend == "x11" {
    if !x11_available {
        unsupported(
            "control_center_linux_x11_display_unavailable",
            "--backend x11 requires DISPLAY",
            "x11"
        );
        return;
    }
    "x11"
} else if requested_backend == "wayland" {
    if !wayland_available {
        unsupported(
            "control_center_linux_wayland_display_unavailable",
            "--backend wayland requires WAYLAND_DISPLAY",
            "wayland"
        );
        return;
    }
    "wayland"
} else if wayland_available {
    "wayland"
} else if x11_available {
    "x11"
} else {
    unsupported(
        "control_center_linux_display_unavailable",
        "native Control Center evidence requires DISPLAY or WAYLAND_DISPLAY",
        "none"
    );
    return;
};

if backend == "x11" && (!tool_available("xdotool") || !tool_available("xmessage")) {
    unsupported(
        "control_center_linux_x11_focus_probe_unavailable",
        "the X11 journey requires xdotool and xmessage for black-box focus evidence",
        backend
    );
    return;
}

let control_center = std::path::absolute(control_center_value).display;
let server_executable = std::path::absolute(server_executable_value).display;
let cli_executable = std::path::absolute(cli_executable_value).display;
for executable in [control_center, server_executable, cli_executable] {
    require(
        std::fs::exists(executable),
        "control_center_linux_executable_missing:" + executable
    );
}

let context = test_harness::new_context(repo, "control-center-linux");
let runtime =
    "/tmp/agt-cc-linux-" + std::process::id() + "-" +
    std::time::SystemTime::now().unix_millis;
prepare_private_runtime(runtime);
let endpoint = "unix:" + std::path::join(runtime, "authority.sock").display;
let registry = std::path::join(
    context.run_directory, "control-center-registry.json"
).display;
let cockpit_png = std::path::join(
    std::path::parent(context.evidence_path).display,
    "control-center-live-cockpit-linux.png"
).display;
let refresh_png = std::path::join(
    std::path::parent(context.evidence_path).display,
    "control-center-refresh-linux.png"
).display;
let replacement_png = std::path::join(
    std::path::parent(context.evidence_path).display,
    "control-center-replacement-linux.png"
).display;
let durable_directory = std::path::join(repo, "dist/evidence").display;
let durable_png = std::path::join(
    durable_directory, "control-center-live-cockpit-linux.png"
).display;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP start caller-selected native Unix IPC authority and PTY");
    let server_command = native_command(
        context,
        server_executable,
        registry,
        endpoint,
        backend,
        ["--endpoint", endpoint]
    );
    let server = server_command.start();
    owned_children.push(server);
    let protocol = wait_protocol(
        context,
        cli_executable,
        registry,
        endpoint,
        backend,
        server,
        "control_center_linux_server"
    );
    require(
        protocol.pid == server.id
            && protocol.endpoint == endpoint
            && protocol.endpoint.starts_with("unix:"),
        "control_center_linux_native_authority"
    );
    let baseline_tab = prove_pty(
        context, cli_executable, registry, endpoint, backend, "baseline"
    );
    let native_input_tab = prove_pty(
        context,
        cli_executable,
        registry,
        endpoint,
        backend,
        "native-input"
    );

    let selected = json_command(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["snapshot", "--json", "--endpoint", endpoint]
    );
    require(
        selected.server_state == "connected"
            && selected.connected_server.pid == protocol.pid
            && selected.connected_server.endpoint == endpoint,
        "control_center_linux_caller_endpoint"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.linux-caller-endpoint"
    );

    let witness = ();
    let witness_window = "";
    if backend == "x11" {
        print("STEP establish an independently focused X11 witness");
        let witness_title = "AgenTerm focus witness " + context.run_id;
        witness = start_x11_focus_witness(witness_title);
        owned_children.push(witness);
        witness_window = wait_x11_window(
            context,
            witness_title,
            witness,
            "control_center_linux_x11_witness_window_timeout"
        );
        activate_x11_window(context, witness_window);
        wait_x11_active(
            context,
            witness_window,
            "control_center_linux_x11_witness_focus_timeout"
        );
    }

    print("STEP no-activate opens one renderer and publishes exact frame evidence");
    let cc_command = native_command(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["open", "--no-activate", "--endpoint", endpoint]
    );
    let cc = cc_command.start();
    owned_children.push(cc);
    wait_registry(registry, cc.id, cc, "control_center_linux_registry");
    let native_window = wait_native_window(
        registry, cc.id, cc, "control_center_linux_native_window"
    );
    let capture = wait_capture(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        cc,
        cockpit_png,
        "connected",
        "control_center_linux_first_frame"
    );
    let rendered = rhai::image::inspect_png(cockpit_png);
    let expected_tabs = "" + selected.connected_server.tabs.len + " tabs";
    require(
        capture.owner_pid == cc.id
            && capture.renderer == "native"
            && capture.output == cockpit_png
            && capture.width == rendered.width
            && capture.height == rendered.height
            && capture.width == capture.rendered_snapshot.physical_width
            && capture.height == capture.rendered_snapshot.physical_height
            && capture.rendered_snapshot.scale_factor >= 1.0
            && capture.rendered_snapshot.selected_view == "cockpit"
            && capture.rendered_snapshot.server_endpoint == endpoint
            && capture.rendered_snapshot.window_title.contains(expected_tabs)
            && capture.sha256 == rhai::crypto::sha256_file(cockpit_png)
            && capture.bytes >= 1000
            && rendered.samples > 0
            && rendered.luminance > 1.0
            && rendered.luminance < 254.9,
        "control_center_linux_renderer_evidence"
    );
    if backend == "x11" {
        require(
            active_x11_window(context) == witness_window,
            "control_center_linux_no_activate_stole_focus"
        );
    }
    std::fs::create_dir_all(durable_directory);
    std::fs::copy(cockpit_png, durable_png);
    require(
        rhai::crypto::sha256_file(durable_png) == capture.sha256,
        "control_center_linux_durable_png_hash"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.linux-renderer-evidence"
    );

    print("STEP native keyboard and pointer obey the Linux display contract");
    let baseline_index = ();
    let native_input_index = ();
    for index in 0..selected.connected_server.tabs.len {
        let tab = selected.connected_server.tabs[index];
        if tab.id == baseline_tab {
            baseline_index = index;
        }
        if tab.id == native_input_tab {
            native_input_index = index;
        }
    }
    require(
        baseline_index != ()
            && baseline_index < 3
            && native_input_index != ()
            && native_input_index < 3
            && baseline_tab != native_input_tab,
        "control_center_linux_native_input_targets_not_visible"
    );
    if backend == "x11" {
        cc.window_key("Home");
        for step in 0..native_input_index {
            cc.window_key("Down");
        }
        cc.window_key("Enter");
        let keyboard_selected = wait_active_tab(
            context,
            control_center,
            registry,
            endpoint,
            backend,
            cc,
            native_input_tab,
            server.id,
            selected.connected_server.epoch,
            "control_center_linux_native_keyboard_navigation"
        );
        require(
            keyboard_selected.connected_server.active_tab_id
                    == native_input_tab
                && keyboard_selected.connected_server.pid == server.id
                && keyboard_selected.connected_server.epoch
                    == selected.connected_server.epoch
                && keyboard_selected.connected_server.endpoint == endpoint
                && cc.state == "running"
                && rhai::json::parse_file(registry).pid == cc.id
                && active_x11_window(context) == witness_window,
            "control_center_linux_native_keyboard_navigation_truth"
        );

        // Product lines occupy 0..10, the Tabs help line is 11, and the Unix
        // adapter maps each following 26-pixel row to one Cockpit tab.
        let baseline_pointer_y = 76 + (11 + baseline_index) * 26 + 13;
        cc.window_pointer("click", 120, baseline_pointer_y);
        let pointer_selected = wait_active_tab(
            context,
            control_center,
            registry,
            endpoint,
            backend,
            cc,
            baseline_tab,
            server.id,
            selected.connected_server.epoch,
            "control_center_linux_native_pointer_navigation"
        );
        require(
            pointer_selected.connected_server.active_tab_id == baseline_tab
                && pointer_selected.connected_server.pid == server.id
                && pointer_selected.connected_server.epoch
                    == selected.connected_server.epoch
                && pointer_selected.connected_server.endpoint == endpoint
                && cc.state == "running"
                && rhai::json::parse_file(registry).pid == cc.id
                && active_x11_window(context) == witness_window,
            "control_center_linux_native_pointer_navigation_truth"
        );
        test_harness::emit_evidence(
            context,
            declared_evidence,
            "control-center.linux-native-cockpit-input"
        );
    } else {
        let input_failure = "";
        try {
            cc.window_key("Home");
        } catch (error) {
            input_failure = error.to_string();
        }
        require(
            input_failure.contains("process_window_unsupported")
                && input_failure.contains("unsupported"),
            "control_center_linux_wayland_input_not_typed_unsupported:" +
                input_failure
        );
        let unchanged = json_command(
            context,
            control_center,
            registry,
            endpoint,
            backend,
            ["snapshot", "--json", "--endpoint", endpoint]
        );
        require(
            unchanged.connected_server.active_tab_id
                    == selected.connected_server.active_tab_id
                && unchanged.connected_server.pid == server.id
                && unchanged.connected_server.epoch
                    == selected.connected_server.epoch
                && unchanged.connected_server.endpoint == endpoint
                && cc.state == "running"
                && rhai::json::parse_file(registry).pid == cc.id,
            "control_center_linux_wayland_unsupported_changed_state"
        );
    }

    print("STEP repeated no-activate and focus requests reuse the same owner");
    invoke(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["open", "--no-activate", "--endpoint", endpoint],
        false
    );
    let reused = json_command(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["status", "--json"]
    );
    require(
        reused.state == "running"
            && reused.pid == cc.id
            && reused.context.endpoint == endpoint,
        "control_center_linux_pid_reuse"
    );
    if backend == "x11" {
        require(
            active_x11_window(context) == witness_window,
            "control_center_linux_reused_no_activate_stole_focus"
        );
    }
    invoke_activate(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["open", "--endpoint", endpoint]
    );
    let focused = wait_capture(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        cc,
        refresh_png,
        "connected",
        "control_center_linux_focus_refresh"
    );
    require(
        focused.owner_pid == cc.id
            && wait_native_window(
                registry,
                cc.id,
                cc,
                "control_center_linux_reused_native_window"
            ) == native_window,
        "control_center_linux_focus_owner_changed"
    );
    if backend == "x11" {
        wait_x11_active(
            context,
            native_window,
            "control_center_linux_focus_request_timeout"
        );
        activate_x11_window(context, witness_window);
    }
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.linux-no-activate-focus-reuse"
    );

    print("STEP server loss is explicit and replacement has a new epoch");
    let prior_server_pid = server.id;
    let prior_epoch = selected.connected_server.epoch;
    server.kill();
    wait_exit(server, "control_center_linux_server_kill_timeout");
    wait_process_gone(
        prior_server_pid, "control_center_linux_server_kill_orphan"
    );
    invoke(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["open", "--no-activate", "--endpoint", endpoint],
        false
    );
    let lost = wait_capture(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        cc,
        refresh_png,
        "unavailable",
        "control_center_linux_server_loss"
    );
    require(
        lost.owner_pid == cc.id
            && lost.rendered_snapshot.server_endpoint == ()
            && process_present(cc.id),
        "control_center_linux_server_loss_truth"
    );

    let replacement_server_command = native_command(
        context,
        server_executable,
        registry,
        endpoint,
        backend,
        ["--endpoint", endpoint]
    );
    let replacement_server = replacement_server_command.start();
    owned_children.push(replacement_server);
    let replacement_protocol = wait_protocol(
        context,
        cli_executable,
        registry,
        endpoint,
        backend,
        replacement_server,
        "control_center_linux_replacement_server"
    );
    invoke(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["open", "--no-activate", "--endpoint", endpoint],
        false
    );
    let refreshed = wait_capture(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        cc,
        refresh_png,
        "connected",
        "control_center_linux_server_recovery"
    );
    let replacement_selected = json_command(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["snapshot", "--json", "--endpoint", endpoint]
    );
    require(
        replacement_protocol.pid != prior_server_pid
            && replacement_protocol.endpoint == endpoint
            && refreshed.owner_pid == cc.id
            && refreshed.rendered_snapshot.server_endpoint == endpoint
            && replacement_selected.connected_server.pid
                == replacement_protocol.pid
            && replacement_selected.connected_server.epoch != prior_epoch
            && process_present(cc.id),
        "control_center_linux_recovery_identity"
    );
    prove_pty(
        context,
        cli_executable,
        registry,
        endpoint,
        backend,
        "after-restart"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.linux-server-recovery"
    );

    print("STEP killed renderer is replaced without owning server or PTY");
    let killed_pid = cc.id;
    cc.kill();
    wait_exit(cc, "control_center_linux_renderer_kill_timeout");
    wait_process_gone(
        killed_pid, "control_center_linux_renderer_kill_orphan"
    );
    require(
        process_present(replacement_server.id),
        "control_center_linux_renderer_kill_affected_server"
    );
    prove_pty(
        context,
        cli_executable,
        registry,
        endpoint,
        backend,
        "after-renderer-kill"
    );

    let recovered_command = native_command(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["open", "--no-activate", "--endpoint", endpoint]
    );
    let recovered = recovered_command.start();
    owned_children.push(recovered);
    wait_registry(
        registry,
        recovered.id,
        recovered,
        "control_center_linux_recovered_registry"
    );
    require(
        recovered.id != killed_pid,
        "control_center_linux_renderer_not_replaced"
    );
    let replacement_capture = wait_capture(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        recovered,
        replacement_png,
        "connected",
        "control_center_linux_replacement_frame"
    );
    require(
        replacement_capture.owner_pid == recovered.id
            && replacement_capture.owner_pid != killed_pid
            && replacement_capture.sha256
                == rhai::crypto::sha256_file(replacement_png)
            && replacement_capture.rendered_snapshot.server_endpoint
                == endpoint
            && process_present(replacement_server.id),
        "control_center_linux_replacement_truth"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.linux-renderer-replacement"
    );

    let closed = json_command(
        context,
        control_center,
        registry,
        endpoint,
        backend,
        ["close", "--json"]
    );
    require(
        (closed.state == "closed" || closed.state == "close_requested")
            && closed.pid == recovered.id,
        "control_center_linux_typed_close"
    );
    let recovered_output = wait_exit(
        recovered, "control_center_linux_recovered_close_timeout"
    );
    require(
        recovered_output.success
            && process_present(replacement_server.id),
        "control_center_linux_close_affected_server"
    );
    prove_pty(
        context,
        cli_executable,
        registry,
        endpoint,
        backend,
        "after-close"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.linux-process-isolation"
    );

    invoke(
        context,
        cli_executable,
        registry,
        endpoint,
        backend,
        ["--endpoint", endpoint, "shutdown"],
        false
    );
    let server_output = wait_exit(
        replacement_server,
        "control_center_linux_final_server_shutdown"
    );
    require(
        server_output.success,
        "control_center_linux_final_server_exit"
    );
    wait_process_gone(
        replacement_server.id,
        "control_center_linux_final_server_orphan"
    );
    print(
        "PASS: Linux caller-selected Unix IPC, renderer-owned PNG/digest, " +
            "native-input display contract, no-activate/focus reuse, new " +
            "epoch recovery, renderer replacement, and orphan-free " +
            "isolation (" + backend + ")"
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
try {
    remove_native_runtime(runtime);
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
