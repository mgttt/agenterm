// Prove the macOS Control Center against one isolated logical dev authority.
// The renderer itself serves the structured snapshot and exact native frame.
// A general runner may expose either a 1x or HiDPI backing scale; Retina is a
// recorded display fact, not a CPU-architecture prerequisite.
//
// Args:
//   --list-evidence
// or:
//   REPO CONTROL_CENTER_EXE SERVER_EXE CLI_EXE

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "control-center.macos-caller-instance",
    "control-center.macos-renderer-evidence",
    "control-center.macos-native-input-contract",
    "control-center.macos-no-activate-focus",
    "control-center.macos-incompatible-sibling",
    "control-center.macos-server-recovery",
    "control-center.macos-process-isolation"
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

fn native_command(context, program, registry, runtime, arguments) {
    let command =
        test_harness::configured_command(context, program, arguments);
    command.env_remove("AGENTERM_IPC_ADDRESS");
    command.env("AGENTERM_INSTANCE", "dev");
    command.env("XDG_RUNTIME_DIR", runtime);
    command.env("AGENTERM_CC_REGISTRY_PATH", registry);
    command.timeout(std::time::Duration::from_secs(15));
    command
}

fn invoke(context, program, registry, runtime, arguments, expected_failure) {
    let output = native_command(
        context, program, registry, runtime, arguments
    ).output();
    test_harness::append_command_record(
        context,
        arguments,
        output,
        expected_failure,
        []
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

fn json_command(context, program, registry, runtime, arguments) {
    let output = invoke(
        context, program, registry, runtime, arguments, false
    );
    rhai::json::parse(output.stdout_text() + output.stderr_text())
}

fn invoke_activate(context, program, registry, runtime, arguments) {
    let command = native_command(
        context, program, registry, runtime, arguments
    );
    command.env_remove("AGENTERM_NO_ACTIVATE");
    let output = command.output();
    test_harness::append_command_record(
        context,
        arguments,
        output,
        false,
        []
    );
    require(
        output.success,
        "activate_command_failed:" +
            output.stdout_text() + output.stderr_text()
    );
}

fn wait_protocol(context, cli, registry, runtime, child, code) {
    for attempt in 0..200 {
        let output = native_command(
            context,
            cli,
            registry,
            runtime,
            ["--instance", "dev", "protocol-info", "--running"]
        ).output();
        if output.success {
            test_harness::append_command_record(
                context,
                ["--instance", "dev", "protocol-info", "--running"],
                output,
                false,
                []
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
    for attempt in 0..200 {
        if std::fs::exists(path) {
            try {
                let record = rhai::json::parse_file(path);
                if record.schema_version == 2
                        && record.pid == expected_pid
                        && record.process_start_identity != "" {
                    return record;
                }
            } catch (error) {
                // Atomic owner publication can be between create and parse.
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

fn wait_exit(child, code) {
    for attempt in 0..200 {
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
    for attempt in 0..200 {
        if !process_present(pid) {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw code;
}

fn frontmost_application(context) {
    let command = std::process::command("osascript");
    command.args([
        "-e",
        "tell application \"System Events\" to get name of first application process whose frontmost is true"
    ]);
    command.timeout(std::time::Duration::from_secs(3));
    let output = command.output();
    test_harness::append_command_record(
        context,
        ["-e", "frontmost application query"],
        output,
        !output.success,
        []
    );
    let application = output.stdout_text();
    application.trim();
    require(
        output.success && application != "",
        "control_center_macos_frontmost_unavailable"
    );
    application
}

fn wait_frontmost_change(context, previous, code) {
    for attempt in 0..100 {
        let current = frontmost_application(context);
        if current != previous
                && current.to_lower().contains("agenterm") {
            return current;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw code;
}

fn wait_active_tab(
    context,
    control_center,
    registry,
    runtime,
    child,
    target_tab_id,
    expected_server_pid,
    expected_server_epoch,
    code
) {
    let last = "";
    for attempt in 0..160 {
        let arguments = ["snapshot", "--json", "--instance", "dev"];
        let output = native_command(
            context, control_center, registry, runtime, arguments
        ).output();
        last = output.stdout_text() + output.stderr_text();
        if output.success {
            let snapshot = rhai::json::parse(last);
            if snapshot.server_state == "connected"
                    && snapshot.connected_server.pid == expected_server_pid
                    && snapshot.connected_server.epoch == expected_server_epoch
                    && snapshot.connected_server.active_tab_id == target_tab_id {
                test_harness::append_command_record(
                    context, arguments, output, false, []
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
    let diagnostic_path = std::path::join(
        context.run_directory,
        "native-input-timeout.png"
    ).display;
    let diagnostic_output = native_command(
        context,
        control_center,
        registry,
        runtime,
        ["screenshot", "--output", diagnostic_path, "--json"]
    ).output();
    let native_input = ();
    if diagnostic_output.success {
        let diagnostic = rhai::json::parse(
            diagnostic_output.stdout_text() + diagnostic_output.stderr_text()
        );
        if diagnostic.rendered_snapshot.contains("last_native_input") {
            native_input = diagnostic.rendered_snapshot.last_native_input;
        }
    }
    test_harness::append_command_record(
        context,
        ["screenshot", "--output", diagnostic_path, "--json"],
        diagnostic_output,
        !diagnostic_output.success,
        []
    );
    throw code + "_timeout:" + rhai::json::stringify(#{
        server_snapshot: last,
        last_native_input: native_input
    });
}

fn wait_capture(
    context,
    control_center,
    registry,
    runtime,
    child,
    output,
    required_state,
    code
) {
    for attempt in 0..20 {
        let capture = native_command(
            context,
            control_center,
            registry,
            runtime,
            ["screenshot", "--output", output, "--json"]
        ).output();
        if capture.success {
            test_harness::append_command_record(
                context,
                ["screenshot", "--output", output, "--json"],
                capture,
                false,
                []
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
    }
    throw code + "_timeout";
}

fn wait_incompatible_capture(
    context,
    control_center,
    registry,
    runtime,
    child,
    output,
    code
) {
    let last = "no_capture";
    let deadline = std::time::SystemTime::now().unix_millis + 10000;
    for attempt in 0..80 {
        if std::time::SystemTime::now().unix_millis >= deadline {
            break;
        }
        let arguments = ["screenshot", "--output", output, "--json"];
        let capture = native_command(
            context,
            control_center,
            registry,
            runtime,
            arguments
        ).output();
        last = capture.stdout_text() + capture.stderr_text();
        if capture.success {
            test_harness::append_command_record(
                context, arguments, capture, false, []
            );
            try {
                let document = rhai::json::parse(last);
                last = rhai::json::stringify(document);
                if document.owner_pid == child.id
                        && document.rendered_snapshot.server_state
                            == "unavailable"
                        && document.rendered_snapshot.server_reason
                            == "server_incompatible" {
                    return document;
                }
            } catch (error) {
                last = last + ":" + error.to_string();
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
    throw code + "_timeout:last_frame=" + last;
}

fn incompatible_listener(
    context, script_executable, address, ready_path, stop_path, code
) {
    let address_parts = address.split(":");
    require(
        address_parts.len == 2,
        "control_center_macos_incompatible_address"
    );
    for path in [ready_path, stop_path] {
        if std::fs::exists(path) {
            std::fs::remove_file(path);
        }
    }
    let source =
        `let listener = std::net::TcpListener::bind(args[0]);
         std::fs::write(args[1], "ready");
         loop {
             if std::fs::exists(args[2]) { break; }
             try {
                 let stream = listener.accept_timeout(
                     std::time::Duration::from_millis(100)
                 );
                 stream.write_all("not-json\n");
                 stream.flush();
             } catch (error) {
                 if !error.to_string().contains("net_accept_timeout") {
                     throw error;
                 }
             }
         }`;
    let command = test_harness::configured_command(
        context,
        script_executable,
        [
            "eval", source,
            "--timeout-ms", "20000",
            "--max-operations", "10000000",
            "--", address, ready_path, stop_path
        ]
    );
    command.timeout(std::time::Duration::from_secs(22));
    let child = command.start();
    for attempt in 0..400 {
        if std::fs::exists(ready_path) {
            return child;
        }
        if child.state == "exited" {
            let output = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + "_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    child.kill();
    child.wait_with_output(std::time::Duration::from_secs(2));
    throw code + "_ready_timeout";
}

fn prove_pty(context, cli, registry, runtime, label) {
    let marker = "AGENTERM_CC_MACOS_" + label + "_" + context.run_id;
    let created = invoke(
        context,
        cli,
        registry,
        runtime,
        [
            "--instance", "dev",
            "new-window", "-d", "-P", "-F", "#{window_id}",
            "-n", "cc-" + label + "-" + context.run_id,
            "--", "/bin/sh"
        ],
        false
    );
    let tab_id = created.stdout_text();
    tab_id.trim();
    require(
        tab_id.starts_with("@"),
        "control_center_macos_stable_tab_id"
    );
    invoke(
        context,
        cli,
        registry,
        runtime,
        [
            "--instance", "dev", "send-keys", "-t", tab_id,
            "-l", "echo " + marker
        ],
        false
    );
    invoke(
        context,
        cli,
        registry,
        runtime,
        ["--instance", "dev", "send-keys", "-t", tab_id, "Enter"],
        false
    );
    invoke(
        context,
        cli,
        registry,
        runtime,
        [
            "--instance", "dev",
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
        runtime,
        [
            "--instance", "dev",
            "capture-pane", "-p", "-t", tab_id,
            "--max-bytes", "16384"
        ],
        false
    );
    require(
        captured.stdout_text().contains(marker),
        "control_center_macos_pty_capture"
    );
    tab_id
}

fn remove_native_runtime(runtime) {
    require(
        runtime.starts_with("/private/tmp/agt-cc-"),
        "control_center_macos_runtime_not_owned"
    );
    if std::fs::exists(runtime) {
        std::fs::remove_dir_all(runtime);
    }
}

require(args.len >= 4, "expected: REPO CONTROL_CENTER_EXE SERVER_EXE CLI_EXE");
let repo = std::path::absolute(args[0]).display;
let control_center_value = args[1];
let server_executable_value = args[2];
let cli_executable_value = args[3];
let option = 4;
while option < args.len {
    require(option + 1 < args.len, "control_center_macos_option_value");
    let value = args[option + 1];
    if args[option] == "--control-center" {
        control_center_value = value;
    } else if args[option] == "--server" {
        server_executable_value = value;
    } else if args[option] == "--cli" {
        cli_executable_value = value;
    } else {
        throw "control_center_macos_unknown_option:" + args[option];
    }
    option += 2;
}
let control_center = std::path::absolute(control_center_value).display;
let server_executable =
    std::path::absolute(server_executable_value).display;
let cli_executable = std::path::absolute(cli_executable_value).display;
let script_executable = std::path::join(
    std::path::parent(control_center).display,
    "agenterm-script"
).display;
for executable in [
    control_center,
    server_executable,
    cli_executable,
    script_executable
] {
    require(
        std::fs::exists(executable),
        "control_center_macos_executable_missing:" + executable
    );
}
require(
    std::fs::exists("/System/Library/CoreServices"),
    "control_center_macos_host_required"
);

let context = test_harness::new_context(repo, "control-center-macos");
let runtime =
    "/private/tmp/agt-cc-" + std::process::id() + "-" +
    std::time::SystemTime::now().unix_millis;
std::fs::create_dir_all(runtime);
let registry = std::path::join(
    context.run_directory,
    "control-center-registry.json"
).display;
let cockpit_png = std::path::join(
    std::path::parent(context.evidence_path).display,
    "control-center-live-cockpit-macos.png"
).display;
let refresh_png = std::path::join(
    std::path::parent(context.evidence_path).display,
    "control-center-refresh-macos.png"
).display;
let durable_directory = std::path::join(repo, "dist/evidence").display;
let durable_png = std::path::join(
    durable_directory,
    "control-center-live-cockpit-macos.png"
).display;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP start isolated logical dev UDS authority and PTY");
    let server_command = native_command(
        context,
        server_executable,
        registry,
        runtime,
        ["--instance", "dev"]
    );
    let server = server_command.start();
    owned_children.push(server);
    let protocol = wait_protocol(
        context, cli_executable, registry, runtime, server,
        "control_center_macos_server"
    );
    require(
        protocol.pid == server.id
            && protocol.endpoint.starts_with("unix:/private/tmp/")
            && protocol.instance == "dev",
        "control_center_macos_dev_authority"
    );
    let baseline_tab = prove_pty(
        context, cli_executable, registry, runtime, "baseline"
    );
    let native_input_tab = prove_pty(
        context, cli_executable, registry, runtime, "native-input"
    );

    print("STEP caller-selected snapshot resolves the same dev authority");
    let selected = json_command(
        context,
        control_center,
        registry,
        runtime,
        ["snapshot", "--json", "--instance", "dev"]
    );
    require(
        selected.server_state == "connected"
            && selected.connected_server.pid == protocol.pid
            && selected.connected_server.endpoint == protocol.endpoint
            && selected.connected_server.logical_instance == "dev",
        "control_center_macos_caller_instance"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.macos-caller-instance"
    );

    print("STEP no-activate opens one native renderer without stealing focus");
    let foreground_before = frontmost_application(context);
    let cc_command = native_command(
        context,
        control_center,
        registry,
        runtime,
        ["open", "--no-activate", "--instance", "dev"]
    );
    let cc = cc_command.start();
    owned_children.push(cc);
    wait_registry(
        registry, cc.id, cc, "control_center_macos_registry"
    );
    let capture = wait_capture(
        context,
        control_center,
        registry,
        runtime,
        cc,
        cockpit_png,
        "connected",
        "control_center_macos_first_frame"
    );
    let foreground_after = frontmost_application(context);
    let rendered = rhai::image::inspect_png(cockpit_png);
    let expected_tabs =
        "" + selected.connected_server.tabs.len + " tabs";
    require(
        foreground_after == foreground_before,
        "control_center_macos_no_activate_stole_focus:" +
            rhai::json::stringify(#{
                before: foreground_before,
                after: foreground_after,
                owner_pid: capture.owner_pid,
                control_center_pid: cc.id
            })
    );
    let png_sha256 = rhai::crypto::sha256_file(cockpit_png);
    require(
        capture.owner_pid == cc.id
            && capture.width == rendered.width
            && capture.height == rendered.height
            && capture.width
                == capture.rendered_snapshot.physical_width
            && capture.height
                == capture.rendered_snapshot.physical_height
            && capture.rendered_snapshot.scale_factor >= 1.0
            && capture.rendered_snapshot.selected_view == "cockpit"
            && capture.rendered_snapshot.server_endpoint
                == protocol.endpoint
            && capture.rendered_snapshot.logical_instance == "dev"
            && capture.rendered_snapshot.window_title.contains(expected_tabs)
            && capture.sha256 == png_sha256
            && capture.bytes >= 1000
            && rendered.samples > 0
            && rendered.luminance > 1.0
            && rendered.luminance < 254.9,
        "control_center_macos_renderer_evidence:" +
            rhai::json::stringify(#{
                owner_pid: capture.owner_pid,
                control_center_pid: cc.id,
                capture_width: capture.width,
                capture_height: capture.height,
                png_width: rendered.width,
                png_height: rendered.height,
                snapshot_width: capture.rendered_snapshot.physical_width,
                snapshot_height: capture.rendered_snapshot.physical_height,
                scale_factor: capture.rendered_snapshot.scale_factor,
                selected_view: capture.rendered_snapshot.selected_view,
                endpoint: capture.rendered_snapshot.server_endpoint,
                expected_endpoint: protocol.endpoint,
                logical_instance: capture.rendered_snapshot.logical_instance,
                title: capture.rendered_snapshot.window_title,
                expected_tabs: expected_tabs,
                capture_sha256: capture.sha256,
                png_sha256: png_sha256,
                bytes: capture.bytes,
                samples: rendered.samples,
                luminance: rendered.luminance
            })
    );
    std::fs::create_dir_all(durable_directory);
    std::fs::copy(cockpit_png, durable_png);
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.macos-renderer-evidence"
    );

    print("STEP native input either navigates in-place or reports TCC denial");
    let baseline_index = ();
    let native_input_index = ();
    let baseline_live = false;
    let native_input_live = false;
    for index in 0..selected.connected_server.tabs.len {
        let tab = selected.connected_server.tabs[index];
        if tab.id == baseline_tab {
            baseline_index = index;
            baseline_live = !tab.dead;
        }
        if tab.id == native_input_tab {
            native_input_index = index;
            native_input_live = !tab.dead;
        }
    }
    require(
        baseline_index != ()
            && native_input_index != ()
            && baseline_live
            && native_input_live
            && baseline_tab != native_input_tab,
        "control_center_macos_native_input_targets_not_live"
    );
    // Cockpit keeps only three tab rows visible and scrolls that window around
    // the selected keyboard target. Derive a different, currently rendered
    // pointer target from the same bounded-window contract instead of treating
    // its global tab index as a rendered row.
    let visible_start = if native_input_index > 0 {
        native_input_index - 1
    } else {
        0
    };
    let maximum_start = if selected.connected_server.tabs.len > 3 {
        selected.connected_server.tabs.len - 3
    } else {
        0
    };
    if visible_start > maximum_start {
        visible_start = maximum_start;
    }
    let visible_end = if visible_start + 3
            < selected.connected_server.tabs.len {
        visible_start + 3
    } else {
        selected.connected_server.tabs.len
    };
    let pointer_index = ();
    for index in visible_start..visible_end {
        let tab = selected.connected_server.tabs[index];
        if tab.id == baseline_tab && !tab.dead {
            pointer_index = index;
        }
    }
    if pointer_index == () {
        for index in visible_start..visible_end {
            let tab = selected.connected_server.tabs[index];
            if pointer_index == ()
                    && index != native_input_index
                    && !tab.dead {
                pointer_index = index;
            }
        }
    }
    require(
        pointer_index != ()
            && pointer_index < selected.connected_server.tabs.len
            && pointer_index >= visible_start
            && pointer_index < visible_end
            && pointer_index != native_input_index,
        "control_center_macos_native_pointer_target_not_live_visible"
    );
    let pointer_target = selected.connected_server.tabs[pointer_index].id;
    let pointer_row = pointer_index - visible_start;
    let native_bounds = cc.window_rect();
    // The WindowServer outer frame is in display points, while the screenshot
    // is the client framebuffer in renderer pixels. Convert through the
    // renderer's own scale and then into the exact coordinate units consumed
    // by the selected process-window adapter. These scales are intentionally
    // independent on headless macOS runners.
    require(
        native_bounds.width > 0
            && native_bounds.height > 0
            && capture.width > 0
            && capture.height > 0
            && capture.rendered_snapshot.scale_factor > 0.0,
        "control_center_macos_native_pointer_scale_unusable:" +
            rhai::json::stringify(#{
                native_width_points: native_bounds.width,
                framebuffer_width: capture.width,
                renderer_scale_factor: capture.rendered_snapshot.scale_factor
            })
    );
    let renderer_scale = capture.rendered_snapshot.scale_factor;
    let pointer_scale = cc.window_pointer_coordinate_scale();
    require(
        pointer_scale > 0.0,
        "control_center_macos_native_pointer_scale_non_positive"
    );
    let client_width_points = capture.width.to_float() / renderer_scale;
    let client_height_points = capture.height.to_float() / renderer_scale;
    let client_left_points =
        (native_bounds.width.to_float() - client_width_points) / 2.0;
    let client_top_points =
        native_bounds.height.to_float() - client_height_points;
    require(
        client_left_points >= 0.0 && client_top_points >= 0.0,
        "control_center_macos_native_pointer_geometry_unusable:" +
            rhai::json::stringify(#{
                native_width_points: native_bounds.width,
                native_height_points: native_bounds.height,
                framebuffer_width: capture.width,
                framebuffer_height: capture.height,
                pointer_scale: pointer_scale,
                renderer_scale_factor: renderer_scale
            })
    );
    let body_scale = if capture.width >= 640 { 2 } else { 1 };
    let line_height = 7 * body_scale + 12;
    // Product lines are 0..10, the Tabs help line is 11, and the following
    // renderer rows are the three bounded Cockpit tab targets.
    let content_pointer_x = 120;
    let content_pointer_y = 76 + (11 + pointer_row) * line_height
        + line_height / 2;
    let pointer_x = ((client_left_points
        + content_pointer_x.to_float() / renderer_scale) * pointer_scale)
        .round().to_int();
    let pointer_y = ((client_top_points
        + content_pointer_y.to_float() / renderer_scale) * pointer_scale)
        .round().to_int();
    let native_width = (
        native_bounds.width.to_float() * pointer_scale
    ).round().to_int();
    let native_height = (
        native_bounds.height.to_float() * pointer_scale
    ).round().to_int();
    require(
        content_pointer_x >= 0
            && content_pointer_x < capture.width
            && content_pointer_y >= 0
            && content_pointer_y < capture.height
            && pointer_x >= 0
            && pointer_x < native_width
            && pointer_y >= 0
            && pointer_y < native_height,
        "control_center_macos_native_pointer_not_bounded:" +
            rhai::json::stringify(#{
                target: pointer_target,
                pointer_row: pointer_row,
                content_x: content_pointer_x,
                content_y: content_pointer_y,
                client_left_points: client_left_points,
                client_top_points: client_top_points,
                quartz_x: pointer_x,
                quartz_y: pointer_y,
                native_width: native_width,
                native_height: native_height,
                pointer_scale: pointer_scale,
                renderer_scale_factor: renderer_scale
            })
    );
    let input_baseline = json_command(
        context,
        control_center,
        registry,
        runtime,
        ["snapshot", "--json", "--instance", "dev"]
    );
    let input_owner = rhai::json::parse_file(registry).pid;
    let input_frontmost = frontmost_application(context);
    let initial_active_tab = input_baseline.connected_server.active_tab_id;
    require(
        input_owner == cc.id
            && input_frontmost == foreground_before
            && input_baseline.connected_server.pid == server.id
            && input_baseline.connected_server.epoch
                == selected.connected_server.epoch
            && input_baseline.connected_server.endpoint == protocol.endpoint,
        "control_center_macos_native_input_baseline_changed"
    );
    let input_failure = "";
    try {
        cc.window_key("Home");
    } catch (error) {
        input_failure = error.to_string();
    }
    if input_failure == "" {
        for step in 0..native_input_index {
            cc.window_key("Down");
        }
        cc.window_key("Enter");
        let keyboard_selected = wait_active_tab(
            context,
            control_center,
            registry,
            runtime,
            cc,
            native_input_tab,
            server.id,
            selected.connected_server.epoch,
            "control_center_macos_native_keyboard_navigation"
        );
        // The public server snapshot can observe the active tab before the
        // Control Center worker consumes its pending receipt. Send the pointer
        // immediately: the product's single-slot last-input-wins queue must
        // retain this second selection rather than dropping it as busy.
        let pointer_failure = "";
        try {
            cc.window_pointer("click", pointer_x, pointer_y);
        } catch (error) {
            pointer_failure = error.to_string();
        }
        if pointer_failure == "" {
            let pointer_selected = wait_active_tab(
                context,
                control_center,
                registry,
                runtime,
                cc,
                pointer_target,
                server.id,
                selected.connected_server.epoch,
                "control_center_macos_native_pointer_navigation"
            );
            require(
                pointer_selected.connected_server.active_tab_id
                        == pointer_target
                    && pointer_selected.connected_server.pid == server.id
                    && pointer_selected.connected_server.epoch
                        == selected.connected_server.epoch
                    && pointer_selected.connected_server.endpoint
                        == protocol.endpoint,
                "control_center_macos_native_pointer_navigation_truth"
            );
            print(
                "NATIVE_POINTER_POSITIVE: bounded pointer navigation " +
                    "changed only active_tab"
            );
        } else {
            require(
                pointer_failure.contains("process_window_input_unsupported")
                    && pointer_failure.contains("unsupported")
                    && !pointer_failure.contains("platform_error"),
                "control_center_macos_native_pointer_unexpected_failure:" +
                    pointer_failure
            );
            let pointer_unchanged = json_command(
                context,
                control_center,
                registry,
                runtime,
                ["snapshot", "--json", "--instance", "dev"]
            );
            require(
                pointer_unchanged.connected_server.active_tab_id
                        == native_input_tab
                    && pointer_unchanged.connected_server.pid == server.id
                    && pointer_unchanged.connected_server.epoch
                        == selected.connected_server.epoch
                    && pointer_unchanged.connected_server.endpoint
                        == protocol.endpoint,
                "control_center_macos_native_pointer_unsupported_changed_state"
            );
            print(
                "NATIVE_POINTER_UNAVAILABLE: process-targeted background " +
                    "pointer delivery is typed Unsupported"
            );
        }
        require(
            keyboard_selected.connected_server.active_tab_id
                    == native_input_tab
                && keyboard_selected.connected_server.pid == server.id
                && keyboard_selected.connected_server.epoch
                    == selected.connected_server.epoch
                && keyboard_selected.connected_server.endpoint
                    == protocol.endpoint
                && cc.state == "running"
                && rhai::json::parse_file(registry).pid == input_owner
                && frontmost_application(context) == input_frontmost,
            "control_center_macos_native_keyboard_navigation_truth"
        );
        print(
            "NATIVE_INPUT_POSITIVE: keyboard navigation changed only " +
                "active_tab on the same CC/server/epoch"
        );
    } else {
        require(
            input_failure.contains("process_window_input_unsupported")
                && input_failure.contains("permission_denied")
                && !input_failure.contains("platform_error"),
            "control_center_macos_native_input_unexpected_failure:" +
                input_failure
        );
        let unchanged = json_command(
            context,
            control_center,
            registry,
            runtime,
            ["snapshot", "--json", "--instance", "dev"]
        );
        require(
            unchanged.connected_server.active_tab_id == initial_active_tab
                && unchanged.connected_server.pid == server.id
                && unchanged.connected_server.epoch
                    == selected.connected_server.epoch
                && unchanged.connected_server.endpoint == protocol.endpoint
                && cc.state == "running"
                && rhai::json::parse_file(registry).pid == input_owner
                && frontmost_application(context) == input_frontmost,
            "control_center_macos_tcc_denial_changed_state"
        );
        print(
            "NATIVE_INPUT_UNAVAILABLE: TCC denied event posting; " +
                "safe-failure contract passed without positive input claim"
        );
    }
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.macos-native-input-contract"
    );

    print("STEP repeated open reuses PID; activate focuses the same window");
    invoke(
        context,
        control_center,
        registry,
        runtime,
        ["open", "--no-activate", "--instance", "dev"],
        false
    );
    let reused = json_command(
        context,
        control_center,
        registry,
        runtime,
        ["status", "--json"]
    );
    require(
        reused.pid == cc.id
            && reused.context.logical_instance == "dev"
            && reused.context.endpoint == protocol.endpoint,
        "control_center_macos_pid_reuse"
    );
    invoke_activate(
        context,
        control_center,
        registry,
        runtime,
        ["open", "--instance", "dev"]
    );
    wait_frontmost_change(
        context,
        foreground_before,
        "control_center_macos_focus_timeout"
    );
    let restore = std::process::command("open");
    restore.args(["-a", foreground_before]);
    restore.timeout(std::time::Duration::from_secs(3));
    let restored = restore.output();
    test_harness::append_command_record(
        context,
        ["-a", "prior foreground application"],
        restored,
        !restored.success,
        []
    );
    require(restored.success, "control_center_macos_focus_restore");
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.macos-no-activate-focus"
    );

    print("STEP server loss is explicit; replacement has a new epoch");
    let prior_pid = server.id;
    let prior_epoch = selected.connected_server.epoch;
    server.kill();
    wait_exit(server, "control_center_macos_server_kill_timeout");
    wait_process_gone(
        prior_pid, "control_center_macos_server_kill_orphan"
    );
    invoke(
        context,
        control_center,
        registry,
        runtime,
        ["open", "--no-activate", "--instance", "dev"],
        false
    );
    let lost = wait_capture(
        context,
        control_center,
        registry,
        runtime,
        cc,
        refresh_png,
        "unavailable",
        "control_center_macos_server_loss"
    );
    require(
        lost.rendered_snapshot.server_endpoint == ()
            && process_present(cc.id),
        "control_center_macos_server_loss_truth"
    );

    print("STEP incompatible sibling is typed in the same projection PID");
    let incompatible_probe_ready = std::path::join(
        context.run_directory,
        "incompatible-probe.ready"
    ).display;
    let incompatible_probe_stop = std::path::join(
        context.run_directory,
        "incompatible-probe.stop"
    ).display;
    let incompatible_probe = incompatible_listener(
        context,
        script_executable,
        context.address,
        incompatible_probe_ready,
        incompatible_probe_stop,
        "control_center_macos_incompatible_probe"
    );
    owned_children.push(incompatible_probe);
    let incompatible_endpoint = "tcp:" + context.address;
    let incompatible = json_command(
        context,
        control_center,
        registry,
        runtime,
        [
            "snapshot", "--json",
            "--endpoint", incompatible_endpoint
        ]
    );
    require(
        incompatible.server_state == "unavailable"
            && incompatible.server_reason == "server_incompatible",
        "control_center_macos_incompatible_snapshot"
    );
    std::fs::write(incompatible_probe_stop, "stop");
    let incompatible_probe_output = wait_exit(
        incompatible_probe,
        "control_center_macos_incompatible_probe_timeout"
    );
    require(
        incompatible_probe_output.success,
        "control_center_macos_incompatible_probe_exit"
    );
    std::fs::remove_file(incompatible_probe_ready);
    std::fs::remove_file(incompatible_probe_stop);

    let incompatible_renderer_ready = std::path::join(
        context.run_directory,
        "incompatible-renderer.ready"
    ).display;
    let incompatible_renderer_stop = std::path::join(
        context.run_directory,
        "incompatible-renderer.stop"
    ).display;
    let incompatible_renderer = incompatible_listener(
        context,
        script_executable,
        context.address,
        incompatible_renderer_ready,
        incompatible_renderer_stop,
        "control_center_macos_incompatible_renderer"
    );
    owned_children.push(incompatible_renderer);
    invoke(
        context,
        control_center,
        registry,
        runtime,
        [
            "open", "--no-activate",
            "--endpoint", incompatible_endpoint
        ],
        false
    );
    let incompatible_frame = wait_incompatible_capture(
        context,
        control_center,
        registry,
        runtime,
        cc,
        refresh_png,
        "control_center_macos_incompatible_renderer"
    );
    require(
        incompatible_frame.owner_pid == cc.id
            && incompatible_frame.rendered_snapshot.server_reason
                == "server_incompatible"
            && process_present(cc.id),
        "control_center_macos_incompatible_renderer_truth"
    );
    std::fs::write(incompatible_renderer_stop, "stop");
    let incompatible_renderer_output = wait_exit(
        incompatible_renderer,
        "control_center_macos_incompatible_renderer_timeout"
    );
    require(
        incompatible_renderer_output.success,
        "control_center_macos_incompatible_renderer_exit"
    );
    std::fs::remove_file(incompatible_renderer_ready);
    std::fs::remove_file(incompatible_renderer_stop);
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.macos-incompatible-sibling"
    );

    let replacement_server_command = native_command(
        context,
        server_executable,
        registry,
        runtime,
        ["--instance", "dev"]
    );
    let replacement_server = replacement_server_command.start();
    owned_children.push(replacement_server);
    let replacement_protocol = wait_protocol(
        context,
        cli_executable,
        registry,
        runtime,
        replacement_server,
        "control_center_macos_replacement_server"
    );
    require(
        replacement_protocol.pid != prior_pid,
        "control_center_macos_new_epoch"
    );
    invoke(
        context,
        control_center,
        registry,
        runtime,
        ["open", "--no-activate", "--instance", "dev"],
        false
    );
    let refreshed = wait_capture(
        context,
        control_center,
        registry,
        runtime,
        cc,
        refresh_png,
        "connected",
        "control_center_macos_server_recovery"
    );
    let replacement_selected = json_command(
        context,
        control_center,
        registry,
        runtime,
        ["snapshot", "--json", "--instance", "dev"]
    );
    require(
        refreshed.owner_pid == cc.id
            && refreshed.rendered_snapshot.server_endpoint
                == replacement_protocol.endpoint
            && replacement_selected.connected_server.pid
                == replacement_protocol.pid
            && replacement_selected.connected_server.epoch != prior_epoch
            && process_present(cc.id),
        "control_center_macos_recovery_identity"
    );
    prove_pty(
        context, cli_executable, registry, runtime, "after-restart"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.macos-server-recovery"
    );

    print("STEP typed close and renderer kill never own server or PTY");
    let closed = json_command(
        context,
        control_center,
        registry,
        runtime,
        ["close", "--json"]
    );
    require(
        (closed.state == "closed" || closed.state == "close_requested")
            && closed.pid == cc.id,
        "control_center_macos_typed_close"
    );
    let cc_output = wait_exit(
        cc, "control_center_macos_close_timeout"
    );
    require(
        cc_output.success && process_present(replacement_server.id),
        "control_center_macos_close_affected_server"
    );
    prove_pty(
        context, cli_executable, registry, runtime, "after-close"
    );

    let killed_command = native_command(
        context,
        control_center,
        registry,
        runtime,
        ["open", "--no-activate", "--instance", "dev"]
    );
    let killed = killed_command.start();
    owned_children.push(killed);
    wait_registry(
        registry, killed.id, killed,
        "control_center_macos_killed_registry"
    );
    wait_capture(
        context,
        control_center,
        registry,
        runtime,
        killed,
        refresh_png,
        "connected",
        "control_center_macos_killed_first_frame"
    );
    let killed_pid = killed.id;
    killed.kill();
    wait_exit(killed, "control_center_macos_renderer_kill_timeout");
    wait_process_gone(
        killed_pid, "control_center_macos_renderer_kill_orphan"
    );
    require(
        process_present(replacement_server.id),
        "control_center_macos_renderer_kill_affected_server"
    );
    prove_pty(
        context, cli_executable, registry, runtime, "after-renderer-kill"
    );

    let recovered_command = native_command(
        context,
        control_center,
        registry,
        runtime,
        ["open", "--no-activate", "--instance", "dev"]
    );
    let recovered = recovered_command.start();
    owned_children.push(recovered);
    wait_registry(
        registry, recovered.id, recovered,
        "control_center_macos_recovered_registry"
    );
    require(
        recovered.id != killed_pid,
        "control_center_macos_renderer_not_replaced"
    );
    json_command(
        context,
        control_center,
        registry,
        runtime,
        ["close", "--json"]
    );
    let recovered_output = wait_exit(
        recovered, "control_center_macos_recovered_close_timeout"
    );
    require(
        recovered_output.success
            && process_present(replacement_server.id),
        "control_center_macos_recovered_close_affected_server"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "control-center.macos-process-isolation"
    );

    invoke(
        context,
        cli_executable,
        registry,
        runtime,
        ["--instance", "dev", "shutdown"],
        false
    );
    let server_output = wait_exit(
        replacement_server,
        "control_center_macos_final_server_shutdown"
    );
    require(
        server_output.success,
        "control_center_macos_final_server_exit"
    );
    wait_process_gone(
        replacement_server.id,
        "control_center_macos_final_server_orphan"
    );
    print(
        "PASS: macOS caller-selected UDS, renderer-owned Retina evidence, " +
            "no-activate/focus reuse, new epoch recovery, and isolation"
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
