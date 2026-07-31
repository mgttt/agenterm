// Prove theme preview, keyboard rollback, persistence, PTY continuity, and
// visibly distinct native rendering through public AgenTerm interfaces.
//
// Args:
//   --list-evidence
// or:
//   REPO GUI_EXE CLI_EXE [--internal-failure-bundle-probe]

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = ["ux.theme-settings"];

if args.len == 1 && args[0] == "--list-evidence" {
    print("ux.theme-settings");
    return;
}

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn json_cli(context, cli, arguments) {
    rhai::json::parse(
        test_harness::invoke_cli(
            context,
            cli,
            arguments,
            false,
            []
        ).stdout
    )
}

fn text_cli(context, cli, arguments) {
    test_harness::invoke_cli(
        context,
        cli,
        arguments,
        false,
        []
    ).stdout
}

fn wait_for_server(context, cli, gui_child) {
    for attempt in 0..200 {
        let probe = test_harness::run_cli(
            context,
            cli,
            ["ui-snapshot"],
            false,
            []
        );
        if probe.success {
            return;
        }
        if gui_child.state == "exited" {
            let output = gui_child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw "theme_gui_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    throw "theme_server_timeout";
}

fn wait_for_window(gui_child) {
    for attempt in 0..200 {
        let facts = gui_child.platform_facts;
        if facts.top_level_window_present {
            return;
        }
        if gui_child.state == "exited" {
            throw "theme_window_early_exit";
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "theme_window_timeout";
}

fn wait_for_server_stop(context, cli) {
    for attempt in 0..200 {
        let probe = test_harness::run_cli(
            context,
            cli,
            ["ui-snapshot"],
            true,
            []
        );
        let output = probe.stdout + probe.stderr;
        if !probe.success
                && output.contains("AgenTerm server is not running") {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "theme_server_stop_timeout";
}

fn wait_for_exit(child, code) {
    for attempt in 0..100 {
        if child.state == "exited" {
            return child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    throw code;
}

fn start_gui(context, gui, cli) {
    let command = test_harness::configured_command(
        context,
        gui,
        ["--no-activate", "--address", context.address]
    );
    let child = command.start();
    wait_for_server(context, cli, child);
    wait_for_window(child);
    let snapshot = json_cli(
        context,
        cli,
        ["wait-ui", "--window-state", "restored", "--timeout-ms", "10000"]
    );
    require(
        snapshot.client_pid == child.id,
        "theme_ui_lease_wrong_process"
    );
    child
}

fn only_tab(context, cli) {
    let snapshot = json_cli(context, cli, ["ui-snapshot"]);
    require(snapshot.tabs.len == 1, "theme_expected_one_tab");
    snapshot.tabs[0]
}

fn only_pane(context, cli, target) {
    let snapshot = json_cli(
        context,
        cli,
        ["pane-snapshot", "-t", target]
    );
    require(snapshot.windows.len == 1, "theme_expected_one_pane");
    snapshot.windows[0]
}

fn assert_theme(snapshot, saved, draft, open) {
    require(
        snapshot.settings.color_theme == saved,
        "theme_saved_mismatch"
    );
    if open {
        require(
            snapshot.modal != ()
                && snapshot.modal.kind == "settings"
                && snapshot.settings.theme_draft == draft,
            "theme_open_draft_mismatch"
        );
    } else {
        require(
            snapshot.modal == ()
                && snapshot.settings.theme_draft == (),
            "theme_expected_closed"
        );
    }
    let dark = false;
    let light = false;
    for option in snapshot.settings.theme_options {
        if option.id == "dark" && option.label == "Dark" {
            dark = true;
        }
        if option.id == "light" && option.label == "Light" {
            light = true;
        }
    }
    require(
        snapshot.settings.theme_options.len == 2 && dark && light,
        "theme_options_mismatch"
    );
}

fn retain_failure_snapshot(context, cli) {
    std::fs::create_dir_all(context.failure_directory);
    let output = test_harness::run_cli(
        context,
        cli,
        ["ui-snapshot"],
        false,
        []
    );
    rhai::runtime::atomic_write(
        std::path::join(
            context.failure_directory,
            "ui-snapshot.txt"
        ).display,
        "exit_code=" + output.exit_code + "\n" +
            output.stdout + output.stderr
    );
}

require(
    args.len == 3 || (
        args.len == 4
            && args[3] == "--internal-failure-bundle-probe"
    ),
    "expected: REPO GUI_EXE CLI_EXE [--internal-failure-bundle-probe]"
);
let repo = std::path::absolute(args[0]).display;
let gui = std::path::absolute(args[1]).display;
let cli = std::path::absolute(args[2]).display;
require(std::fs::exists(gui), "theme_gui_missing");
require(std::fs::exists(cli), "theme_cli_missing");
let internal_failure_probe = args.len == 4;

let context = test_harness::new_context(repo, "theme");
let dark_png = std::path::join(
    context.run_directory,
    "evidence/theme-dark.png"
).display;
let light_png = std::path::join(
    context.run_directory,
    "evidence/theme-light.png"
).display;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP launch isolated Dark workspace and keep a PTY active");
    let first_gui = start_gui(context, gui, cli);
    owned_children.push(first_gui);
    let tab = only_tab(context, cli);
    json_cli(
        context,
        cli,
        ["wait-ui", "-t", tab.id, "--tab-state", "running",
         "--timeout-ms", "10000"]
    );
    if internal_failure_probe {
        throw "INTERNAL_FAILURE_BUNDLE_PROBE:gui:" + context.run_id;
    }
    let pane_before = only_pane(context, cli, tab.id);
    let token_before =
        "AGENTERM_THEME_BEFORE_" + std::process::id();
    text_cli(
        context,
        cli,
        ["send-keys", "-t", tab.id, "echo " + token_before, "Enter"]
    );
    text_cli(
        context,
        cli,
        ["wait-pane", "-t", tab.id, "--contains", token_before,
         "--submit-complete", "--timeout-ms", "10000"]
    );

    print("STEP native Light preview and Cancel rollback");
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "open-settings"]
    );
    assert_theme(snapshot, "dark", "dark", true);
    text_cli(context, cli, ["screenshot", "-o", dark_png]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "settings-theme-light"]
    );
    assert_theme(snapshot, "dark", "light", true);
    text_cli(context, cli, ["screenshot", "-o", light_png]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "cancel"]
    );
    assert_theme(snapshot, "dark", (), false);

    print("STEP Escape rolls a second Light preview back");
    text_cli(context, cli, ["ui-action", "open-settings"]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "settings-theme-light"]
    );
    assert_theme(snapshot, "dark", "light", true);
    first_gui.window_key("Escape");
    let snapshot = json_cli(
        context,
        cli,
        ["wait-ui", "--window-state", "restored",
         "--modal-kind", "none", "--timeout-ms", "5000"]
    );
    assert_theme(snapshot, "dark", (), false);

    print("STEP native Apply persists Light without interrupting the PTY");
    text_cli(context, cli, ["ui-action", "open-settings"]);
    text_cli(context, cli, ["ui-action", "settings-theme-light"]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "settings-apply"]
    );
    assert_theme(snapshot, "light", (), false);
    require(
        std::fs::exists(context.settings_path)
            && rhai::json::parse_file(
                context.settings_path
            ).color_theme == "light",
        "theme_apply_not_persisted"
    );
    let token_after =
        "AGENTERM_THEME_AFTER_" + std::process::id();
    text_cli(
        context,
        cli,
        ["send-keys", "-t", tab.id, "echo " + token_after, "Enter"]
    );
    text_cli(
        context,
        cli,
        ["wait-pane", "-t", tab.id, "--contains", token_after,
         "--submit-complete", "--timeout-ms", "10000"]
    );
    let pane_after = only_pane(context, cli, tab.id);
    let capture = text_cli(
        context,
        cli,
        ["capture-pane", "-p", "-t", tab.id]
    );
    require(
        pane_after.pid == pane_before.pid
            && !pane_after.dead
            && pane_after.output_bytes >= pane_before.output_bytes
            && capture.contains(token_before)
            && capture.contains(token_after),
        "theme_pty_interrupted"
    );

    print("STEP Dark and Light previews have distinct rendered pixels");
    require(
        std::fs::metadata(dark_png).len >= 1000
            && std::fs::metadata(light_png).len >= 1000,
        "theme_screenshot_too_small"
    );
    let dark = rhai::image::inspect_png(dark_png);
    let light = rhai::image::inspect_png(light_png);
    let luminance_delta = dark.luminance - light.luminance;
    if luminance_delta < 0.0 {
        luminance_delta = -luminance_delta;
    }
    require(
        dark.width == light.width
            && dark.height == light.height
            && luminance_delta >= 20.0,
        "theme_render_not_visibly_distinct"
    );
    print(
        "Dark screenshot: " + dark.width + "x" + dark.height +
        ", luminance " + dark.luminance
    );
    print(
        "Light screenshot: " + light.width + "x" + light.height +
        ", luminance " + light.luminance
    );

    print("STEP saved Light theme survives shutdown and restart");
    text_cli(context, cli, ["ui-action", "close-window"]);
    text_cli(
        context,
        cli,
        ["ui-action", "stop-server-and-exit"]
    );
    wait_for_exit(first_gui, "theme_first_gui_exit_timeout");
    wait_for_server_stop(context, cli);

    let second_gui = start_gui(context, gui, cli);
    owned_children.push(second_gui);
    let restored = only_tab(context, cli);
    json_cli(
        context,
        cli,
        ["wait-ui", "-t", restored.id, "--tab-state", "running",
         "--timeout-ms", "10000"]
    );
    assert_theme(
        json_cli(context, cli, ["ui-snapshot"]),
        "light",
        (),
        false
    );

    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux.theme-settings"
    );
    print(
        "PASS: theme preview, rollback, persistence, PTY continuity, " +
        "and rendering"
    );
    succeeded = true;
} catch (error) {
    failure = error.to_string();
    try {
        retain_failure_snapshot(context, cli);
    } catch (diagnostic_error) {
        rhai::runtime::atomic_write(
            std::path::join(
                context.failure_directory,
                "snapshot-error.txt"
            ).display,
            diagnostic_error.to_string()
        );
    }
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
