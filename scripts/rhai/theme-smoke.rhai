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
        ["wait-ui", "--window-state", "restored", "--timeout-ms", "30000"]
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

fn derived_color_theme(preset_id) {
    if preset_id.ends_with("-day") {
        "light"
    } else {
        "dark"
    }
}

fn assert_theme_options(snapshot) {
    let expected = [
        "classic-day",
        "classic-night",
        "fancy-day",
        "fancy-night",
    ];
    require(
        snapshot.settings.theme_options.len == 4,
        "theme_options_count_mismatch"
    );
    for id in expected {
        let found = false;
        for option in snapshot.settings.theme_options {
            if option.id == id && option.label != "" && option.description != "" {
                found = true;
            }
        }
        require(found, "theme_option_missing:" + id);
    }
}

fn assert_theme(snapshot, saved_preset, draft, open) {
    require(
        snapshot.settings.appearance_preset == saved_preset,
        "appearance_preset_saved_mismatch"
    );
    require(
        snapshot.settings.color_theme == derived_color_theme(saved_preset),
        "color_theme_saved_mismatch"
    );
    assert_theme_options(snapshot);
    if open {
        require(
            snapshot.modal != ()
                && snapshot.modal.kind == "settings"
                && snapshot.settings.theme_draft == draft
                && snapshot.settings.appearance_preset_draft == draft,
            "theme_open_draft_mismatch"
        );
    } else {
        require(
            snapshot.modal == ()
                && snapshot.settings.theme_draft == ()
                && snapshot.settings.appearance_preset_draft == (),
            "theme_expected_closed"
        );
    }
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
let night_png = std::path::join(
    context.run_directory,
    "evidence/theme-classic-night.png"
).display;
let day_png = std::path::join(
    context.run_directory,
    "evidence/theme-classic-day.png"
).display;
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    print("STEP legacy color_theme migrates to classic-day on restart");
    std::fs::create_dir_all(
        std::path::parent(context.settings_path).display
    );
    rhai::runtime::atomic_write(
        context.settings_path,
        "{\"color_theme\":\"light\",\"terminal_font_size\":14}\n"
    );
    let migration_gui = start_gui(context, gui, cli);
    owned_children.push(migration_gui);
    assert_theme(
        json_cli(context, cli, ["ui-snapshot"]),
        "classic-day",
        (),
        false
    );
    text_cli(context, cli, ["ui-action", "stop-server-and-exit"]);
    wait_for_exit(migration_gui, "theme_migration_gui_exit_timeout");
    wait_for_server_stop(context, cli);
    let migrated = rhai::json::parse_file(context.settings_path);
    require(
        migrated.appearance_preset == "classic-day",
        "legacy_light_migration_failed"
    );

    print("STEP launch isolated classic-night workspace and keep a PTY active");
    rhai::runtime::atomic_write(
        context.settings_path,
        "{\"appearance_preset\":\"classic-night\"}\n"
    );
    let first_gui = start_gui(context, gui, cli);
    owned_children.push(first_gui);
    let tab = only_tab(context, cli);
    json_cli(
        context,
        cli,
        ["wait-ui", "-t", tab.id, "--tab-state", "running",
         "--timeout-ms", "30000"]
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
         "--submit-complete", "--timeout-ms", "30000"]
    );

    print("STEP preview all four presets through public ui-actions");
    text_cli(context, cli, ["ui-action", "open-settings"]);
    assert_theme(
        json_cli(context, cli, ["ui-snapshot"]),
        "classic-night",
        "classic-night",
        true
    );
    for action in [
        "settings-preset-classic-day",
        "settings-preset-classic-night",
        "settings-preset-fancy-day",
        "settings-preset-fancy-night",
        "settings-theme-light",
        "settings-theme-dark",
    ] {
        text_cli(context, cli, ["ui-action", action]);
    }
    let fancy_snapshot = json_cli(context, cli, ["ui-snapshot"]);
    assert_theme(
        fancy_snapshot,
        "classic-night",
        "classic-night",
        true
    );

    print("STEP native classic-day preview and Cancel rollback");
    text_cli(context, cli, ["ui-action", "open-settings"]);
    assert_theme(
        json_cli(context, cli, ["ui-snapshot"]),
        "classic-night",
        "classic-night",
        true
    );
    text_cli(context, cli, ["screenshot", "-o", night_png]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "settings-preset-classic-day"]
    );
    assert_theme(snapshot, "classic-night", "classic-day", true);
    text_cli(context, cli, ["screenshot", "-o", day_png]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "cancel"]
    );
    assert_theme(snapshot, "classic-night", (), false);

    print("STEP Escape rolls a second classic-day preview back");
    text_cli(context, cli, ["ui-action", "open-settings"]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "settings-preset-classic-day"]
    );
    assert_theme(snapshot, "classic-night", "classic-day", true);
    first_gui.window_key("Escape");
    let snapshot = json_cli(
        context,
        cli,
        ["wait-ui", "--window-state", "restored",
         "--modal-kind", "none", "--timeout-ms", "30000"]
    );
    assert_theme(snapshot, "classic-night", (), false);

    print("STEP native Apply persists classic-day without interrupting the PTY");
    text_cli(context, cli, ["ui-action", "open-settings"]);
    text_cli(context, cli, ["ui-action", "settings-preset-classic-day"]);
    let snapshot = json_cli(
        context,
        cli,
        ["ui-action", "settings-apply"]
    );
    assert_theme(snapshot, "classic-day", (), false);
    require(
        persisted.appearance_preset == "classic-day",
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
         "--submit-complete", "--timeout-ms", "30000"]
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

    print("STEP classic-day and classic-night previews have distinct rendered pixels");
    require(
        std::fs::metadata(night_png).len >= 1000
            && std::fs::metadata(day_png).len >= 1000,
        "theme_screenshot_too_small"
    );
    let dark = rhai::image::inspect_png(night_png);
    let light = rhai::image::inspect_png(day_png);
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

    print("STEP saved classic-day survives shutdown and restart");
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
         "--timeout-ms", "30000"]
    );
    assert_theme(
        json_cli(context, cli, ["ui-snapshot"]),
        "classic-day",
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
        "migration, and rendering"
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
