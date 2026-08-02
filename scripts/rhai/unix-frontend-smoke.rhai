// Prove the matching-host Unix AgenTerm workbench through public CLI actions,
// renderer-owned state/screenshots, native focus observation, and the real
// platform clipboard helper.
//
// Args:
//   --list-evidence
// or:
//   REPO GUI_EXE CLI_EXE --platform linux|macos

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "ux.unix-frontend-linux-no-activate",
    "ux.unix-frontend-linux-workbench",
    "ux.unix-frontend-linux-native-clipboard",
    "ux.unix-frontend-linux-stale-paste",
    "ux.unix-frontend-macos-no-activate",
    "ux.unix-frontend-macos-workbench",
    "ux.unix-frontend-macos-native-clipboard",
    "ux.unix-frontend-macos-stale-paste"
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

fn environment_or_empty(name) {
    if std::env::has(name) { std::env::get(name) } else { "" }
}

fn unsupported(platform, code, message) {
    print(
        "SKIP " + rhai::json::stringify(#{
            schema_version: 1,
            suite: "unix-frontend-smoke",
            platform: platform,
            result_class: "skipped",
            failure: #{
                kind: "Unsupported",
                code: code,
                message: message
            },
            evidence_emitted: []
        })
    );
}

fn resolve_tool(name) {
    let command = std::process::command("/bin/sh");
    command.args(["-c", "command -v -- " + name]);
    command.timeout(std::time::Duration::from_secs(2));
    let output = command.output();
    if !output.success {
        return "";
    }
    let path = output.stdout_text();
    path.trim();
    path
}

fn invoke_tool(context, program, arguments, input) {
    let command = std::process::command(program);
    command.args(arguments);
    if input != () {
        command.stdin_text(input);
    }
    command.timeout(std::time::Duration::from_secs(5));
    let output = command.output();
    let recorded_arguments = [
        std::path::PathBuf::from(program).file_name
    ];
    recorded_arguments.append(arguments);
    test_harness::append_command_record(
        context, recorded_arguments,
        output, !output.success, []
    );
    require(
        output.success,
        "unix_frontend_native_tool_failed:" +
            std::path::PathBuf::from(program).file_name + ":" +
            output.stderr_text()
    );
    output
}

fn json_cli(context, cli, arguments) {
    rhai::json::parse(
        test_harness::invoke_cli(
            context, cli, arguments, false, []
        ).stdout
    )
}

fn text_cli(context, cli, arguments) {
    test_harness::invoke_cli(
        context, cli, arguments, false, []
    ).stdout
}

fn wait_for_server(context, cli, gui) {
    for attempt in 0..240 {
        let probe = test_harness::run_cli(
            context, cli, ["ui-snapshot"], false, []
        );
        if probe.success {
            return rhai::json::parse(probe.stdout);
        }
        if gui.state == "exited" {
            let output = gui.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw "unix_frontend_gui_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "unix_frontend_server_timeout";
}

fn wait_for_window(gui) {
    for attempt in 0..240 {
        let facts = gui.platform_facts;
        if facts.top_level_window_supported
                && facts.top_level_window_present
                && facts.top_level_window_id != 0 {
            return facts;
        }
        if gui.state == "exited" {
            throw "unix_frontend_window_early_exit";
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw "unix_frontend_window_timeout";
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
            "unix_frontend_xdotool_failed:" + output.stderr_text()
        );
    }
    output
}

fn wait_x11_window(context, title, child) {
    for attempt in 0..160 {
        let output = xdotool(
            context, ["search", "--onlyvisible", "--name", title], true
        );
        if output.success {
            for raw in output.stdout_text().split("\n") {
                let id = raw;
                id.trim();
                if id != "" {
                    let name = xdotool(
                        context, ["getwindowname", id], true
                    );
                    let actual = name.stdout_text();
                    actual.trim();
                    if name.success && actual == title {
                        return id;
                    }
                }
            }
        }
        if child.state == "exited" {
            throw "unix_frontend_x11_window_owner_exited";
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "unix_frontend_x11_window_timeout:" + title;
}

fn active_x11_window(context) {
    let output = xdotool(context, ["getactivewindow"], false);
    let id = output.stdout_text();
    id.trim();
    require(id != "", "unix_frontend_x11_active_window_empty");
    id
}

fn wait_x11_active(context, id, code) {
    for attempt in 0..160 {
        if active_x11_window(context) == id {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw code;
}

fn start_x11_witness(title) {
    let command = std::process::command("xmessage");
    command.args([
        "-name", "agenterm-unix-frontend-witness",
        "-title", title, "-center", "-buttons", "OK:0", title
    ]);
    command.timeout(std::time::Duration::from_secs(120));
    command.start()
}

fn frontmost_macos(context) {
    let output = invoke_tool(
        context,
        "/usr/bin/osascript",
        [
            "-e",
            "tell application \"System Events\" to get name of first application process whose frontmost is true"
        ],
        ()
    );
    let name = output.stdout_text();
    name.trim();
    require(name != "", "unix_frontend_macos_frontmost_empty");
    name
}

fn activate_finder_witness(context) {
    invoke_tool(
        context,
        "/usr/bin/osascript",
        ["-e", "tell application \"Finder\" to activate"],
        ()
    );
    for attempt in 0..100 {
        if frontmost_macos(context) == "Finder" {
            return "Finder";
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw "unix_frontend_macos_witness_focus_timeout";
}

fn wait_macos_agenterm_active(context) {
    for attempt in 0..160 {
        let name = frontmost_macos(context);
        if name.to_lower().contains("agenterm") {
            return name;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw "unix_frontend_macos_activation_timeout";
}

fn wait_native_focus(context, cli, gui, platform, native_window_id) {
    let last = ();
    for attempt in 0..160 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        last = snapshot;
        let os_focused = if platform == "linux" {
            active_x11_window(context) == native_window_id
        } else {
            frontmost_macos(context).to_lower().contains("agenterm")
        };
        if snapshot.focus.window_focused
                && snapshot.focus.surface == "terminal"
                && gui.platform_facts.top_level_window_is_foreground
                && os_focused {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw "unix_frontend_native_focus_event_timeout:" +
        rhai::json::stringify(last);
}

fn set_native_clipboard(context, platform, helper, text) {
    if platform == "linux" {
        let command = std::process::command(helper);
        command.args([
            "-selection", "clipboard", "-loops", "2", "-silent"
        ]);
        command.stdin_text(text);
        command.timeout(std::time::Duration::from_secs(30));
        return command.start();
    } else {
        invoke_tool(context, helper, [], text);
    }
}

fn wait_linux_clipboard_owner(context, helper, expected, owner) {
    for attempt in 0..100 {
        if owner.state == "exited" {
            let output = owner.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw "unix_frontend_xclip_owner_exited:" +
                output.stderr_text();
        }
        let command = std::process::command(helper);
        command.args(["-selection", "clipboard", "-o"]);
        command.timeout(std::time::Duration::from_secs(2));
        let output = command.output();
        test_harness::append_command_record(
            context,
            ["xclip", "-selection", "clipboard", "-o"],
            output,
            !output.success,
            []
        );
        if output.success && output.stdout_text() == expected {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw "unix_frontend_xclip_owner_readiness_timeout";
}

fn wait_file(path, gui, code) {
    for attempt in 0..240 {
        if std::fs::exists(path) {
            return;
        }
        if gui.state == "exited" {
            throw code + "_gui_exited";
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw code;
}

fn wait_stale_failure(context, cli) {
    let last = ();
    for attempt in 0..240 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        last = snapshot;
        if snapshot.terminal_paste.state == "idle"
                && snapshot.feedback.error != ()
                && snapshot.feedback.error.code == "terminal_paste_failed"
                && snapshot.feedback.error.category == "precondition"
                && snapshot.feedback.error.retryable {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw "unix_frontend_stale_paste_failure_timeout:" +
        rhai::json::stringify(last);
}

fn wait_terminal_paste_success(context, cli) {
    let last = ();
    for attempt in 0..240 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        last = snapshot;
        if snapshot.terminal_paste.state == "idle" {
            if snapshot.feedback.error != () {
                throw "unix_frontend_native_paste_failed:" +
                    rhai::json::stringify(snapshot.feedback.error);
            }
            if snapshot.feedback.message != ()
                    && snapshot.feedback.message.starts_with("Pasted ") {
                return snapshot;
            }
        }
        rhai::task::sleep(std::time::Duration::from_millis(20));
    }
    throw "unix_frontend_native_paste_timeout:" +
        rhai::json::stringify(last);
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

fn retain_failure_snapshot(context, cli) {
    std::fs::create_dir_all(context.failure_directory);
    let output = test_harness::run_cli(
        context, cli, ["ui-snapshot"], true, []
    );
    rhai::runtime::atomic_write(
        std::path::join(
            context.failure_directory, "ui-snapshot.txt"
        ).display,
        output.stdout + output.stderr
    );
}

require(
    args.len == 5 && args[3] == "--platform",
    "expected: REPO GUI_EXE CLI_EXE --platform linux|macos"
);
let platform = args[4];
require(
    platform == "linux" || platform == "macos",
    "unix_frontend_platform_invalid:" + platform
);

if platform == "linux" {
    if environment_or_empty("WAYLAND_DISPLAY") != "" {
        unsupported(
            platform,
            "unix_frontend_wayland_focus_witness_unsupported",
            "the Linux native-focus journey currently requires an X11 compositor witness"
        );
        return;
    }
    if environment_or_empty("DISPLAY") == "" {
        unsupported(
            platform,
            "unix_frontend_x11_display_unavailable",
            "the Linux native workbench journey requires DISPLAY"
        );
        return;
    }
    if resolve_tool("xdotool") == "" || resolve_tool("xmessage") == "" {
        unsupported(
            platform,
            "unix_frontend_x11_witness_unavailable",
            "the Linux journey requires xdotool and xmessage"
        );
        return;
    }
} else if resolve_tool("osascript") == "" {
    unsupported(
        platform,
        "unix_frontend_macos_witness_unavailable",
        "the macOS journey requires osascript"
    );
    return;
}

let clipboard_reader = if platform == "linux" {
    resolve_tool("xclip")
} else {
    resolve_tool("pbpaste")
};
let clipboard_writer = if platform == "linux" {
    clipboard_reader
} else {
    resolve_tool("pbcopy")
};
if clipboard_reader == "" || clipboard_writer == "" {
    unsupported(
        platform,
        "unix_frontend_native_clipboard_unavailable",
        if platform == "linux" {
            "the Linux journey requires the real xclip helper"
        } else {
            "the macOS journey requires pbpaste and pbcopy"
        }
    );
    return;
}

let repo = std::path::absolute(args[0]).display;
let gui_exe = std::path::absolute(args[1]).display;
let cli_exe = std::path::absolute(args[2]).display;
require(std::fs::exists(gui_exe), "unix_frontend_gui_missing");
require(std::fs::exists(cli_exe), "unix_frontend_cli_missing");

let context = test_harness::new_context(
    repo, "unix-frontend-" + platform
);
let owned_children = [];
let wrapper_directory = std::path::join(
    context.run_directory, "clipboard-wrapper"
).display;
std::fs::create_dir_all(wrapper_directory);
let entered_path = std::path::join(
    context.run_directory, "clipboard-entered"
).display;
let release_path = std::path::join(
    context.run_directory, "clipboard-release"
).display;
let mode_path = std::path::join(
    context.run_directory, "clipboard-barrier-enabled"
).display;
let wrapper_name = if platform == "linux" { "xclip" } else { "pbpaste" };
let wrapper_path = std::path::join(
    wrapper_directory, wrapper_name
).display;
let wrapper_source =
    "#!/bin/sh\n" +
    "if [ -f \"$AGENTERM_SMOKE_CLIPBOARD_MODE\" ]; then\n" +
    "  : > \"$AGENTERM_SMOKE_CLIPBOARD_ENTERED\"\n" +
    "  while [ ! -f \"$AGENTERM_SMOKE_CLIPBOARD_RELEASE\" ]; do sleep 0.02; done\n" +
    "  printf '%s' \"$AGENTERM_SMOKE_STALE_MARKER\"\n" +
    "  exit 0\n" +
    "fi\n" +
    "exec \"" + clipboard_reader + "\" \"$@\"\n";
rhai::runtime::atomic_write(wrapper_path, wrapper_source);
invoke_tool(context, "/bin/chmod", ["0700", wrapper_path], ());

let stale_marker = "AGENTERM_STALE_PASTE_" + context.run_id;
let witness_title = "AgenTerm foreground witness " + context.run_id;
let witness_id = "";
let foreground_before = "";
if platform == "linux" {
    let witness = start_x11_witness(witness_title);
    owned_children.push(witness);
    witness_id = wait_x11_window(context, witness_title, witness);
    xdotool(
        context, ["windowactivate", "--sync", witness_id], false
    );
    wait_x11_active(
        context, witness_id, "unix_frontend_witness_activation_timeout"
    );
    foreground_before = witness_id;
} else {
    foreground_before = activate_finder_witness(context);
}

let succeeded = false;
let failure = "";
try {
    print("STEP no-activate launch preserves an independent native foreground witness");
    let gui_command = test_harness::configured_command(context, gui_exe, []);
    gui_command.env(
        "PATH", wrapper_directory + ":" + environment_or_empty("PATH")
    );
    gui_command.env("AGENTERM_SMOKE_CLIPBOARD_MODE", mode_path);
    gui_command.env("AGENTERM_SMOKE_CLIPBOARD_ENTERED", entered_path);
    gui_command.env("AGENTERM_SMOKE_CLIPBOARD_RELEASE", release_path);
    gui_command.env("AGENTERM_SMOKE_STALE_MARKER", stale_marker);
    let gui = gui_command.start();
    owned_children.push(gui);
    let native_window = wait_for_window(gui);
    let initial = wait_for_server(context, cli_exe, gui);
    require(
        initial.projection == "embedded_gui"
            && initial.focus.surface == "terminal"
            && !initial.focus.window_focused,
        "unix_frontend_initial_snapshot_contract:" +
            rhai::json::stringify(initial)
    );
    if platform == "linux" {
        require(
            active_x11_window(context) == foreground_before,
            "unix_frontend_linux_no_activate_stole_focus"
        );
    } else {
        require(
            frontmost_macos(context) == foreground_before,
            "unix_frontend_macos_no_activate_stole_focus"
        );
    }
    test_harness::emit_evidence(
        context, declared_evidence,
        "ux.unix-frontend-" + platform + "-no-activate"
    );

    print("STEP semantic activation waits for native focus and compositor truth");
    json_cli(context, cli_exe, ["ui-action", "window-activate"]);
    let native_window_id = if platform == "linux" {
        native_window.top_level_window_id.to_string()
    } else {
        ""
    };
    let focused = wait_native_focus(
        context, cli_exe, gui, platform, native_window_id
    );
    if platform == "linux" {
        wait_x11_active(
            context, native_window_id,
            "unix_frontend_linux_compositor_focus_timeout"
        );
    } else {
        wait_macos_agenterm_active(context);
    }

    print("STEP renderer-owned snapshot and PNG describe one event position");
    let before_capture = json_cli(context, cli_exe, ["ui-snapshot"]);
    let screenshot_path = std::path::join(
        std::path::parent(context.evidence_path).display,
        "unix-workbench-" + platform + ".png"
    ).display;
    let returned_path = text_cli(
        context, cli_exe, ["screenshot", "-o", screenshot_path]
    );
    returned_path.trim();
    let after_capture = json_cli(context, cli_exe, ["ui-snapshot"]);
    let rendered = rhai::image::inspect_png(screenshot_path);
    require(
        returned_path == screenshot_path
            && before_capture.event_position.epoch
                == after_capture.event_position.epoch
            && before_capture.event_position.sequence
                == after_capture.event_position.sequence
            && after_capture.focus.window_focused
            && after_capture.focus.surface == "terminal"
            && after_capture.window.title != ""
            && after_capture.layout.terminal.rows > 0
            && after_capture.layout.terminal.cols > 0
            && std::fs::metadata(screenshot_path).len > 1000
            && rendered.width > 0
            && rendered.height > 0
            && rendered.samples > 0
            && rendered.luminance > 1.0
            && rendered.luminance < 254.9,
        "unix_frontend_renderer_evidence:" +
            rhai::json::stringify(#{
                before: before_capture.event_position,
                after: after_capture.event_position,
                focus: focused.focus,
                png: screenshot_path,
                width: rendered.width,
                height: rendered.height,
                bytes: std::fs::metadata(screenshot_path).len
            })
    );
    test_harness::emit_evidence(
        context, declared_evidence,
        "ux.unix-frontend-" + platform + "-workbench"
    );

    print("STEP real native clipboard text is asynchronously pasted into the PTY");
    let active_tab = focused.active_window_id;
    let paste_marker = "AGENTERM_NATIVE_PASTE_" + context.run_id;
    let clipboard_text = "echo " + paste_marker;
    let clipboard_owner = set_native_clipboard(
        context, platform, clipboard_writer, clipboard_text
    );
    if platform == "linux" {
        owned_children.push(clipboard_owner);
        wait_linux_clipboard_owner(
            context, clipboard_reader, clipboard_text, clipboard_owner
        );
    }
    let paste_baseline = json_cli(context, cli_exe, ["ui-snapshot"]);
    json_cli(context, cli_exe, ["ui-action", "terminal-paste"]);
    let paste_complete = wait_terminal_paste_success(context, cli_exe);
    require(
        paste_complete.active_window_id == active_tab,
        "unix_frontend_native_paste_target_changed"
    );
    json_cli(
        context,
        cli_exe,
        [
            "wait-events",
            "--epoch", paste_baseline.event_position.epoch,
            "--after", paste_baseline.event_position.sequence.to_string(),
            "--kind", "terminal.pasted",
            "--tab", active_tab,
            "--timeout-ms", "5000"
        ]
    );
    text_cli(
        context, cli_exe, ["send-keys", "-t", active_tab, "Enter"]
    );
    text_cli(
        context,
        cli_exe,
        [
            "wait-pane", "-t", active_tab,
            "--contains", paste_marker, "--timeout-ms", "5000"
        ]
    );
    let pasted = text_cli(
        context,
        cli_exe,
        ["capture-pane", "-p", "-t", active_tab, "--max-bytes", "16384"]
    );
    require(
        pasted.contains(paste_marker),
        "unix_frontend_native_clipboard_marker_missing"
    );
    test_harness::emit_evidence(
        context, declared_evidence,
        "ux.unix-frontend-" + platform + "-native-clipboard"
    );

    print("STEP a delayed clipboard result cannot cross an active-tab boundary");
    let peer = text_cli(
        context,
        cli_exe,
        [
            "new-window", "-d", "-P", "-F", "#{window_id}",
            "-n", "stale-target-" + context.run_id,
            "--", "/bin/sh"
        ]
    );
    peer.trim();
    require(peer.starts_with("@"), "unix_frontend_peer_id_invalid:" + peer);
    json_cli(context, cli_exe, ["ui-action", "select-tab", "-t", active_tab]);
    rhai::runtime::atomic_write(mode_path, "enabled\n");
    json_cli(context, cli_exe, ["ui-action", "terminal-paste"]);
    wait_file(
        entered_path, gui, "unix_frontend_clipboard_barrier_timeout"
    );
    json_cli(context, cli_exe, ["ui-action", "select-tab", "-t", peer]);
    rhai::runtime::atomic_write(release_path, "released\n");
    let stale = wait_stale_failure(context, cli_exe);
    require(
        stale.active_window_id == peer
            && stale.feedback.error.code == "terminal_paste_failed"
            && stale.feedback.error.category == "precondition"
            && stale.feedback.error.retryable,
        "unix_frontend_stale_failure_contract:" +
            rhai::json::stringify(stale.feedback.error)
    );
    for tab in [active_tab, peer] {
        let capture = text_cli(
            context,
            cli_exe,
            ["capture-pane", "-p", "-t", tab, "--max-bytes", "16384"]
        );
        require(
            !capture.contains(stale_marker),
            "unix_frontend_stale_marker_delivered:" + tab
        );
    }
    test_harness::emit_evidence(
        context, declared_evidence,
        "ux.unix-frontend-" + platform + "-stale-paste"
    );

    print("STEP public close choice terminates the owned GUI/server cleanly");
    json_cli(context, cli_exe, ["ui-action", "close-window"]);
    json_cli(context, cli_exe, ["ui-action", "stop-server-and-exit"]);
    let exit = wait_exit(gui, "unix_frontend_shutdown_timeout");
    require(
        exit.success,
        "unix_frontend_shutdown_failed:" + exit.stderr_text()
    );
    print("PASS: " + platform + " native workbench and clipboard journey");
    succeeded = true;
} catch (error) {
    failure = error.to_string();
    try {
        retain_failure_snapshot(context, cli_exe);
    } catch (diagnostic_error) {
        rhai::runtime::atomic_write(
            std::path::join(
                context.failure_directory, "snapshot-error.txt"
            ).display,
            diagnostic_error.to_string()
        );
    }
}

let completion_failure = "";
try {
    test_harness::complete(
        context, owned_children, succeeded, failure
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
