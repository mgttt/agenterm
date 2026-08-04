// Prove that the native GUI is a replaceable client of an independently
// persistent terminal server and that the same GUI reconnects in place after a
// real server-process failure.
//
// Args:
//   --list-evidence
// or:
//   REPO GUI_EXE CLI_EXE [SCREENSHOT_PATH]

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "ui.replaceable-client",
    "ux.adaptive-tabs",
    "ux.hierarchical-tabs",
    "ux.detach-first-window-close",
    "ux.live-close-confirmation",
    "ux.locale-consistency",
    "ux.keyboard-surface-navigation",
    "ux.modal-wait",
    "ux.mouse-scrollback",
    "ux.semantic-ui-automation",
    "ux.semantic-window-control",
    "ux.settings-isolation",
    "ux.system-menu-clipboard",
    "ux.theme-render-parity",
    "ux.terminal-selection-copy",
    "ux.working-context-cwd",
    "ux-parity.remote-ui.replaceable-client",
    "ux-parity.remote-ui.selection",
    "ux-parity.window-focus-contract"
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

fn require_window_control(control, name) {
    require(
        control != (),
        "remote_ui_control_missing:" + name
    );
    control
}

fn wheel_message_lparam(gui) {
    let window_rect = gui.window_rect();
    let x = (window_rect.left + window_rect.right) / 2;
    let y = (window_rect.top + window_rect.bottom) / 2;
    y * 65536 + x
}

fn double_click_point(gui_child, x, y) {
    gui_child.window_pointer("click", x, y);
    gui_child.window_message(0x0203, 0, y * 65536 + x);
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

fn optional_cli(context, cli, arguments) {
    test_harness::run_cli(
        context,
        cli,
        arguments,
        true,
        []
    )
}

fn discard_cli(context, cli, arguments, output_name) {
    let command_arguments = ["--address", context.address];
    command_arguments.append(arguments);
    let command = test_harness::configured_command(
        context,
        cli,
        command_arguments
    );
    command.stdout_file(
        std::path::join(
            context.run_directory,
            output_name
        ).display
    );
    let output = command.output();
    require(
        output.success,
        "remote_ui_discard_cli_failed:" + output.stderr_text()
    );
}

fn wait_for_exit(child, code) {
    for attempt in 0..1200 {
        if child.state == "exited" {
            return child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn wait_for_window(child, code) {
    for attempt in 0..1200 {
        if child.platform_facts.top_level_window_present {
            return;
        }
        if child.state == "exited" {
            let output = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + ":" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn wait_for_lease(context, cli, child, attempts, code) {
    for attempt in 0..attempts {
        let output = test_harness::run_cli(
            context,
            cli,
            ["ui-lease", "status"],
            false,
            []
        );
        if output.success {
            let lease = rhai::json::parse(output.stdout);
            if lease.attached && lease.client_pid == child.id {
                wait_for_window(child, code + "_window");
                return lease;
            }
        }
        if child.state == "exited" {
            let result = child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
            throw code + ":" + result.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    throw code;
}

fn start_gui(context, gui, cli) {
    let command = test_harness::configured_command(
        context,
        gui,
        ["--ui-client", "--no-activate", "--address", context.address]
    );
    let child = command.start();
    let lease = wait_for_lease(
        context,
        cli,
        child,
        1200,
        "remote_ui_lease_timeout"
    );
    #{ child: child, lease: lease }
}

fn wait_projection(context, cli, projection) {
    for attempt in 0..1200 {
        let output = test_harness::run_cli(
            context,
            cli,
            ["ui-snapshot"],
            false,
            []
        );
        if output.success {
            let snapshot = rhai::json::parse(output.stdout);
            if snapshot.projection == projection {
                return snapshot;
            }
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_ui_projection_timeout:" + projection;
}

fn wait_observed(context, cli, sequence) {
    for attempt in 0..1200 {
        let lease = json_cli(context, cli, ["ui-lease", "status"]);
        if lease.observed_sequence >= sequence {
            return lease;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_ui_observed_sequence_timeout";
}

fn wait_workspace_toolbar(context, cli, code) {
    for attempt in 0..1200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        if snapshot.layout.toolbar != () {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    let snapshot = json_cli(context, cli, ["ui-snapshot"]);
    throw code
        + ":window=" + rhai::json::stringify(snapshot.window)
        + ":projection=" + snapshot.projection;
}

fn wait_locale(context, cli, locale_id) {
    for attempt in 0..1200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        if snapshot.locale.id == locale_id {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_ui_locale_timeout:" + locale_id;
}

fn wait_effective_font_size(context, cli, size, overridden) {
    for attempt in 0..1200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        if snapshot.settings != () && snapshot.settings.effective != () {
            let terminal_override = snapshot.settings.current_terminal_override;
            if snapshot.settings.effective.terminal_font_size == size
                && ((overridden && terminal_override != ())
                    || (!overridden && terminal_override == ())) {
                return snapshot;
            }
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_ui_effective_font_timeout:" + size;
}

fn wait_terminal_resize_settled(context, cli) {
    for attempt in 0..1200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        let terminal = snapshot.layout.terminal;
        if !terminal.resize_pending
                && terminal.rows == terminal.desired_rows
                && terminal.cols == terminal.desired_cols {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_ui_terminal_resize_settle_timeout";
}

fn wait_terminal_selection(
        context, cli, phase, capture_owned, copyable, code) {
    for attempt in 0..1200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        let interaction = snapshot.terminal_interaction;
        if interaction != () {
            let selection = interaction.selection;
            if selection != ()
                && selection.phase == phase
                && selection.capture_owned == capture_owned
                && selection.copyable == copyable
                && selection.highlight.rendered
                && selection.highlight.bounds.len > 0 {
                return snapshot;
            }
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn sample_render_activity(context, cli, gui, after_sequence) {
    let required_sequence = after_sequence;
    if required_sequence == 0 {
        let current = json_cli(context, cli, ["ui-snapshot"]);
        if current.render_activity != () {
            required_sequence = current.render_activity.sequence;
        }
    }
    require(
        gui.window_message(0x8004, 0, 0) == 1,
        "remote_ui_render_activity_sample_rejected"
    );
    for attempt in 0..1200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        let activity = snapshot.render_activity;
        if activity != () && activity.sequence > required_sequence {
            return activity;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_ui_render_activity_sample_timeout";
}

fn measure_theme_render(
        context, cli, gui, settings_button, settings_cancel,
        baseline_theme, target_theme, label, round) {
    settings_button.click();
    baseline_theme.click();
    let prefix = "theme-render-ab-" + round + "-" + label;
    text_cli(
        context,
        cli,
        [
            "screenshot", "-o",
            std::path::join(
                context.run_directory,
                prefix + "-baseline.png"
            ).display
        ]
    );
    let before = sample_render_activity(context, cli, gui, 0);
    let started = std::time::SystemTime::now().unix_millis;
    target_theme.click();
    text_cli(
        context,
        cli,
        [
            "screenshot", "-o",
            std::path::join(
                context.run_directory,
                prefix + "-target.png"
            ).display
        ]
    );
    let after = sample_render_activity(
        context,
        cli,
        gui,
        before.sequence
    );
    let duration_ms =
        std::time::SystemTime::now().unix_millis - started;
    settings_cancel.click();
    json_cli(
        context,
        cli,
        [
            "wait-ui", "--modal-kind", "closed",
            "--timeout-ms", "30000"
        ]
    );
    let result = #{
        duration_ms: duration_ms,
        redraws: after.redraw_requests - before.redraw_requests,
        paints: after.parent_paints - before.parent_paints
    };
    print(
        `THEME_RENDER_AB round=${round} theme=${label} duration_ms=${result.duration_ms} redraws=${result.redraws} paints=${result.paints}`
    );
    result
}

fn wait_viewport_mutation(context, cli, baseline, tab_id) {
    text_cli(
        context,
        cli,
        [
            "wait-events",
            "--epoch", baseline.epoch,
            "--after", baseline.sequence.to_string(),
            "--kind", "terminal.viewport",
            "--tab", tab_id,
            "--timeout-ms", "30000"
        ]
    );
}

fn find_tab(snapshot, id) {
    for tab in snapshot.tabs {
        if tab.id == id {
            return tab;
        }
    }
    ()
}

fn double_click_tab_text(context, cli, gui_child, id) {
    // Tab render geometry disappears from the snapshot for a frame while the
    // sidebar re-renders (for example right after dismissing an editor), so a
    // single-shot read races the redraw. Poll until the geometry settles.
    let text_geometry = ();
    for attempt in 0..600 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        let tab = find_tab(snapshot, id);
        if tab != ()
                && tab.render != ()
                && tab.render.text != ()
                && tab.render.text.width > 0
                && tab.render.text.height > 0 {
            text_geometry = tab.render.text;
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    require(text_geometry != (), "remote_ui_tab_text_geometry_missing");
    double_click_point(
        gui_child,
        text_geometry.left + text_geometry.width / 2,
        text_geometry.top + text_geometry.height / 2
    );
}

fn find_last_run(screen, marker) {
    let found = ();
    for run in screen.runs {
        if run.text.contains(marker) {
            found = run;
        }
    }
    found
}

fn process_present(id) {
    for process in std::process::list() {
        if process.id == id {
            return true;
        }
    }
    false
}

fn wait_process_gone(id, code) {
    for attempt in 0..1200 {
        if !process_present(id) {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn wait_file_text(path, expected, code) {
    for attempt in 0..1200 {
        if std::fs::exists(path) {
            let observed = std::fs::read_to_string(path);
            observed.trim();
            if observed == expected {
                return observed;
            }
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    if std::fs::exists(path) {
        throw code + ":observed=" + std::fs::read_to_string(path);
    }
    throw code;
}

fn wait_pane_input_bytes(context, cli, tab_id, minimum, code) {
    for attempt in 0..1200 {
        let observed = text_cli(
            context,
            cli,
            ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
        );
        observed.trim();
        if observed.parse_int() >= minimum {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn repeat_text(value, count) {
    let result = "";
    for index in 0..count {
        result += value;
    }
    result
}

fn screenshot(context, cli, path) {
    let returned = text_cli(
        context,
        cli,
        ["screenshot", "-o", path]
    );
    returned.trim();
    require(
        std::path::absolute(returned).display.to_lower()
            == std::path::absolute(path).display.to_lower(),
        "remote_ui_screenshot_path_mismatch"
    );
    let metadata = std::fs::metadata(path);
    require(metadata.len > 1000, "remote_ui_screenshot_too_small");
}

fn wait_screenshot_changed(context, cli, path, baseline_path, code) {
    let baseline_hash = rhai::crypto::sha256_file(baseline_path);
    // Selection state is published before the low-priority WM_PAINT that makes
    // its highlight visible. Keep the pixel receipt fail-closed while allowing
    // that independently scheduled paint to commit.
    for attempt in 0..1200 {
        screenshot(context, cli, path);
        if rhai::crypto::sha256_file(path) != baseline_hash {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw code;
}

fn pane_screenshot(context, cli, path) {
    let returned = text_cli(
        context,
        cli,
        ["screenshot-pane", "-o", path]
    );
    returned.trim();
    require(
        std::path::absolute(returned).display.to_lower()
            == std::path::absolute(path).display.to_lower(),
        "remote_ui_pane_screenshot_path_mismatch"
    );
    let metadata = std::fs::metadata(path);
    require(metadata.len > 1000, "remote_ui_pane_screenshot_too_small");
}

fn assert_instance_directory_empty(context) {
    for attempt in 0..1200 {
        let files = std::fs::read_dir(context.instance_directory);
        if files.len == 0 {
            return;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "remote_ui_instance_registration_leaked";
}

require(
    args.len == 3 || args.len == 4,
    "expected: REPO GUI_EXE CLI_EXE [SCREENSHOT_PATH]"
);
let repo = std::path::absolute(args[0]).display;
let gui_exe = std::path::absolute(args[1]).display;
let cli = std::path::absolute(args[2]).display;
let requested_screenshot = if args.len == 4 {
    std::path::absolute(args[3]).display
} else {
    ""
};
require(std::fs::exists(gui_exe), "remote_ui_gui_missing");
require(std::fs::exists(cli), "remote_ui_cli_missing");

let context = test_harness::new_context(repo, "remote-ui");
let owned_children = [];
let known_server_pids = [];
let succeeded = false;
let failure = "";
let clipboard_original = ();

try {
    print("STEP start replaceable UI and let it start the independent server");
    let started = start_gui(context, gui_exe, cli);
    let gui = started.child;
    owned_children.push(gui);
    let lease = started.lease;

    print("STEP observe the lease-owned GUI projection through public CLI");
    let ui = wait_projection(context, cli, "replaceable_ui_client");
    require(
        ui.schema_version == 1
            && ui.client_pid == gui.id
            && ui.event_position.epoch == lease.position.server_epoch
            && ui.window.visible
            && ui.window.client_width >= 320
            && ui.layout.terminal.rows >= 1
            && ui.layout.composer.input.target_rows == 3
            && ui.layout.status_bar.proxy.archived
            && ["terminal", "composer", "tabs"].contains(ui.focus.surface)
            && ui.tabs.len >= 1,
        "remote_ui_projection_invalid"
    );
    require(
        ui.locale.id == "en-US"
            && ui.locale.controls.send == "Send",
        "remote_ui_locale_contract_invalid"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux-parity.remote-ui.replaceable-client"
    );

    print("STEP drive semantic window state and client resize");
    let original_width = ui.window.client_width;
    let original_height = ui.window.client_height;
    let original_rows = ui.layout.terminal.rows;
    let original_columns = ui.layout.terminal.cols;
    json_cli(context, cli, ["ui-action", "window-minimize"]);
    let minimized = json_cli(
        context,
        cli,
        [
            "wait-ui", "--window-state", "minimized",
            "--timeout-ms", "30000"
        ]
    );
    require(
        minimized.layout.terminal.rows == original_rows
            && minimized.layout.terminal.cols == original_columns,
        "remote_ui_minimize_changed_grid"
    );
    json_cli(context, cli, ["ui-action", "window-restore"]);
    json_cli(
        context,
        cli,
        ["wait-ui", "--window-state", "restored", "--timeout-ms", "30000"]
    );
    json_cli(context, cli, ["ui-action", "window-maximize"]);
    json_cli(
        context,
        cli,
        ["wait-ui", "--window-state", "maximized", "--timeout-ms", "30000"]
    );
    json_cli(context, cli, ["ui-action", "window-restore"]);
    json_cli(
        context,
        cli,
        ["wait-ui", "--window-state", "restored", "--timeout-ms", "30000"]
    );
    let resized_width = if original_width > 720 {
        original_width - 80
    } else {
        original_width + 80
    };
    let resized_height = if original_height > 540 {
        original_height - 60
    } else {
        original_height + 60
    };
    json_cli(
        context,
        cli,
        [
            "ui-action", "window-resize",
            "--width", resized_width.to_string(),
            "--height", resized_height.to_string()
        ]
    );
    let resized_window = json_cli(
        context,
        cli,
        [
            "wait-ui",
            "--client-width", resized_width.to_string(),
            "--client-height", resized_height.to_string(),
            "--terminal-grid-changed-from",
                original_rows.to_string() + "x" + original_columns,
            "--timeout-ms", "30000"
        ]
    );
    require(
        resized_window.layout.terminal.rows != original_rows
            || resized_window.layout.terminal.cols != original_columns,
        "remote_ui_resize_did_not_change_grid"
    );
    json_cli(
        context,
        cli,
        [
            "ui-action", "window-resize",
            "--width", original_width.to_string(),
            "--height", original_height.to_string()
        ]
    );
    json_cli(
        context,
        cli,
        [
            "wait-ui",
            "--client-width", original_width.to_string(),
            "--client-height", original_height.to_string(),
            "--timeout-ms", "30000"
        ]
    );

    print("STEP relay typed UI actions to the exact live GUI lease");
    let hidden = json_cli(context, cli, ["ui-action", "tabs-hide"]);
    require(
        hidden.projection == "replaceable_ui_client"
            && hidden.client_pid == gui.id
            && !hidden.layout.sidebar.visible
            && hidden.layout.status_bar.tabs_recovery.width > 0,
        "remote_ui_tabs_hide_invalid"
    );
    let shown = json_cli(context, cli, ["ui-action", "tabs-show"]);
    require(
        shown.layout.sidebar.visible && shown.client_pid == gui.id,
        "remote_ui_tabs_show_invalid"
    );
    let settings = json_cli(
        context,
        cli,
        ["ui-action", "open-settings"]
    );
    require(
        settings.modal.kind == "settings",
        "remote_ui_settings_relay_invalid"
    );
    let settings_wait = json_cli(
        context,
        cli,
        [
            "wait-ui", "--modal-kind", "settings",
            "--timeout-ms", "30000"
        ]
    );
    require(
        settings_wait.focus.surface == "settings",
        "remote_ui_settings_modal_wait_invalid"
    );
    let cancelled = json_cli(context, cli, ["ui-action", "cancel"]);
    require(cancelled.modal == (), "remote_ui_cancel_relay_invalid");
    json_cli(
        context,
        cli,
        [
            "wait-ui", "--modal-kind", "closed",
            "--timeout-ms", "30000"
        ]
    );
    let composer = json_cli(context, cli, ["focus", "composer"]);
    require(
        composer.focus.surface == "composer",
        "remote_ui_composer_focus_invalid"
    );
    text_cli(context, cli, ["focus", "terminal"]);
    let settings_facts = json_cli(context, cli, ["get-settings"]);
    require(
        settings_facts.terminal_font_size >= 8
            && settings_facts.recommended_cjk_font == "Sarasa Fixed SC",
        "remote_ui_settings_facts_invalid"
    );
    screenshot(
        context,
        cli,
        std::path::join(
            std::path::parent(context.evidence_path).display,
            "relayed-window.png"
        ).display
    );
    pane_screenshot(
        context,
        cli,
        std::path::join(
            std::path::parent(context.evidence_path).display,
            "relayed-pane.png"
        ).display
    );

    print("STEP prove GUI and server are different process roles");
    let bootstrap = json_cli(context, cli, ["ui-bootstrap"]);
    let protocol = json_cli(
        context,
        cli,
        ["protocol-info", "--running"]
    );
    known_server_pids.push(bootstrap.server_pid);
    let original_window_id = gui.platform_facts.top_level_window_id;
    require(
        bootstrap.schema_version == 2
            && ui.server_pid == bootstrap.server_pid
            && bootstrap.server_pid != gui.id
            && bootstrap.tabs.len >= 1
            && lease.client_build.protocol_version == 1
            && lease.client_build.version != ""
            && protocol.ui_bridge.ownership_mode == "split_server_client"
            && protocol.ui_bridge.replaceable_ui
            && protocol.ui_bridge.interactive_lease
            && protocol.ui_bridge.reconnect
            && protocol.ui_bridge.rollback_proven
            && original_window_id != 0,
        "remote_ui_process_role_separation_invalid"
    );

    print("STEP prove the server-owned PTY remains interactive");
    let tab_id = bootstrap.active_tab_id;
    let marker = "AGENTERM_REMOTE_UI_" + context.run_id;
    text_cli(
        context,
        cli,
        ["send-keys", "-t", tab_id, "-l", "echo " + marker]
    );
    text_cli(context, cli, ["send-keys", "-t", tab_id, "Enter"]);
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", marker,
            "--timeout-ms", "30000"
        ]
    );
    let rendered = json_cli(context, cli, ["ui-bootstrap"]);
    wait_observed(context, cli, rendered.position.sequence);

    print("STEP close only the GUI and retain server, PTY, and workspace");
    let composer_editor = require_window_control(gui.window_control(2101), "composer_editor");
    let close_keep = require_window_control(gui.window_control(2109), "close_keep");
    let close_stop = require_window_control(gui.window_control(2110), "close_stop");
    let close_cancel = require_window_control(gui.window_control(2111), "close_cancel");
    let draft_marker = "draft-" + context.run_id;
    composer_editor.set_text(draft_marker);
    gui.window_message(0x0010, 0, 0);
    require(
        gui.state == "running"
            && close_keep.visible
            && close_stop.visible
            && close_cancel.visible
            && !composer_editor.visible,
        "remote_ui_three_choice_close_invalid"
    );
    screenshot(
        context,
        cli,
        std::path::join(
            std::path::parent(context.evidence_path).display,
            "close-three-choice.png"
        ).display
    );
    let saved_draft = text_cli(
        context,
        cli,
        ["show-composer", "-t", tab_id]
    );
    saved_draft.trim();
    require(
        saved_draft == draft_marker,
        "remote_ui_close_lost_composer_draft"
    );
    close_cancel.click();
    require(
        gui.state == "running"
            && !close_keep.visible
            && composer_editor.visible,
        "remote_ui_close_cancel_invalid"
    );
    gui.window_message(0x0010, 0, 0);
    gui.window_message(0x0100, 0x1B, 0);
    require(
        gui.state == "running" && !close_keep.visible,
        "remote_ui_close_escape_invalid"
    );
    gui.window_message(0x0010, 0, 0);
    gui.window_message(0x0100, 0x0D, 0);
    wait_for_exit(gui, "remote_ui_keep_server_exit_timeout");

    let lease_after = json_cli(context, cli, ["ui-lease", "status"]);
    let capture_after = text_cli(
        context,
        cli,
        [
            "capture-pane", "-p", "-t", tab_id,
            "--max-bytes", "16384"
        ]
    );
    require(
        !lease_after.attached && capture_after.contains(marker),
        "remote_ui_detach_ended_server_or_pty"
    );
    let headless = wait_projection(context, cli, "headless_server");
    require(
        headless.server_pid == bootstrap.server_pid
            && headless.tabs.len >= 1,
        "remote_ui_headless_projection_invalid"
    );

    print("STEP replace the GUI and recover the same live server state");
    let replacement = start_gui(context, gui_exe, cli);
    gui = replacement.child;
    owned_children.push(gui);
    let replacement_lease = replacement.lease;
    let replacement_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    let replacement_capture = text_cli(
        context,
        cli,
        [
            "capture-pane", "-p", "-t", tab_id,
            "--max-bytes", "16384"
        ]
    );
    let replacement_lease = wait_observed(
        context,
        cli,
        replacement_bootstrap.position.sequence
    );
    require(
        replacement_bootstrap.server_pid == bootstrap.server_pid
            && replacement_bootstrap.active_tab_id == tab_id
            && replacement_lease.client_build.protocol_version == 1
            && replacement_lease.client_build.version != ""
            && replacement_capture.contains(marker),
        "remote_ui_replacement_state_invalid"
    );

    print("STEP recover hidden Tabs from toolbar, system menu, and status bar");
    let tabs_button = require_window_control(gui.window_control(2104), "tabs_button");
    let control_center_button = require_window_control(gui.window_control(2138), "control_center_button");
    let settings_button = require_window_control(gui.window_control(2112), "settings_button");
    let new_button = require_window_control(gui.window_control(2103), "new_button");
    let locale_button = require_window_control(gui.window_control(2129), "locale_button");
    let font_decrease_button = require_window_control(gui.window_control(2130), "font_decrease_button");
    let font_increase_button = require_window_control(gui.window_control(2131), "font_increase_button");
    let toolbar = wait_workspace_toolbar(context, cli, "remote_ui_toolbar_ready_timeout");
    require(
        tabs_button.text == "<Tabs"
            && toolbar.layout.toolbar.tabs.left
                < toolbar.layout.toolbar["new"].left
            && toolbar.layout.toolbar["new"].right
                <= toolbar.layout.toolbar.control_center.left
            && toolbar.layout.toolbar.control_center.right
                <= toolbar.layout.toolbar.settings.left
            && toolbar.layout.toolbar.settings.right
                <= toolbar.layout.toolbar.locale.left
            && toolbar.layout.toolbar.locale.right
                <= toolbar.layout.toolbar.font_decrease.left
            && toolbar.layout.toolbar.font_decrease.right
                <= toolbar.layout.toolbar.font_increase.left
            && toolbar.layout.toolbar.font_increase.right
                == toolbar.layout.toolbar.bounds.right - 4,
        "remote_ui_toolbar_geometry_invalid"
    );
    require(
        locale_button.text == "En|Zh"
            && control_center_button.text == "Control Center"
            && font_decrease_button.text == "z"
            && font_increase_button.text == "Z",
        "remote_ui_toolbar_new_controls_invalid"
    );
    locale_button.click();
    let traditional = wait_locale(context, cli, "zh-Hant");
    require(
        traditional.locale.id == "zh-Hant"
            && locale_button.text == "Zh|En"
            && settings_button.text == "設定",
        "remote_ui_locale_toggle_invalid"
    );
    locale_button.click();
    require(
        wait_locale(context, cli, "en-US").locale.id == "en-US",
        "remote_ui_locale_restore_invalid"
    );
    tabs_button.click();
    require(
        tabs_button.visible
            && settings_button.visible
            && tabs_button.text == ">Tabs",
        "remote_ui_toolbar_hide_invalid"
    );
    tabs_button.click();
    require(tabs_button.visible, "remote_ui_toolbar_restore_invalid");
    gui.window_message(0x0112, 0x1F20, 0);
    require(tabs_button.visible, "remote_ui_system_menu_hide_invalid");
    gui.window_message(0x0112, 0x1F20, 0);
    require(tabs_button.visible, "remote_ui_system_menu_restore_invalid");
    tabs_button.click();
    let hidden_rect = gui.window_client_rect();
    gui.window_pointer("click", 36, hidden_rect.bottom - 13);
    require(
        tabs_button.visible,
        "remote_ui_status_tabs_recovery_invalid"
    );

    print("STEP configure a new terminal before creation");
    let initial_command = require_window_control(gui.window_control(2124), "initial_command");
    let http_proxy = require_window_control(gui.window_control(2125), "http_proxy");
    let https_proxy = require_window_control(gui.window_control(2126), "https_proxy");
    let create_button = require_window_control(gui.window_control(2127), "create_button");
    let before_new_count = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    ).tabs.len;
    new_button.click();
    text_cli(
        context,
        cli,
        [
            "wait-ui", "--modal-kind", "new-terminal",
            "--timeout-ms", "30000"
        ]
    );
    require(
        json_cli(context, cli, ["ui-bootstrap"]).tabs.len
            == before_new_count
            && initial_command.visible,
        "remote_ui_new_modal_invalid"
    );
    let new_marker = "AGENTERM_NEW_DIALOG_" + context.run_id;
    let proxy_secret =
        "http://dialog-user:dialog-pass@127.0.0.1:48888";
    // Echo unconditionally. This step proves the dialog's initial command
    // reaches the created terminal; it must not depend on HTTP_PROXY being
    // defined. Create-path proxy injection was intentionally paused in
    // 7338a93 (see PRD_02_06), after which the old conditional form only
    // still printed on machines that already exported HTTP_PROXY -- green on
    // proxied dev boxes, timing out on every clean CI runner. Proxy handling
    // is covered by the redaction assertions above.
    initial_command.set_text("echo " + new_marker);
    http_proxy.set_text(proxy_secret);
    https_proxy.set_text(proxy_secret);
    let new_modal_text = "";
    let new_modal = ();
    for attempt in 0..400 {
        new_modal_text = text_cli(context, cli, ["ui-snapshot"]);
        new_modal = rhai::json::parse(new_modal_text);
        if new_modal.modal.initial_command_configured
                && new_modal.modal.http_proxy_configured
                && new_modal.modal.https_proxy_configured {
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    require(
        new_modal.modal.initial_command_configured
            && new_modal.modal.http_proxy_configured
            && new_modal.modal.https_proxy_configured
            && !new_modal.modal.proxy_values_exposed
            && !new_modal_text.contains(proxy_secret)
            && !new_modal_text.contains("dialog-pass"),
        "remote_ui_new_modal_redaction_invalid"
    );
    create_button.click();
    text_cli(
        context,
        cli,
        [
            "wait-pane", "--contains", new_marker,
            "--timeout-ms", "30000"
        ]
    );
    let created_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    require(
        created_bootstrap.tabs.len == before_new_count + 1,
        "remote_ui_new_create_count_invalid"
    );
    let created_tab_id = created_bootstrap.active_tab_id;
    text_cli(context, cli, ["select-window", "-t", tab_id]);
    text_cli(context, cli, ["kill-window", "-t", created_tab_id]);
    text_cli(
        context,
        cli,
        [
            "wait-ui", "--active", tab_id,
            "--focus", "terminal", "--timeout-ms", "30000"
        ]
    );

    print("STEP navigate Terminal, Composer, and Tabs without the mouse");
    let focus_message = 0x8003;
    let shortcut_message = 0x8002;
    require(
        gui.window_message(focus_message, 0, 0) == 1,
        "remote_ui_initial_focus_invalid"
    );
    gui.window_message(shortcut_message, 0x28, 1);
    require(
        gui.window_message(focus_message, 0, 0) == 2,
        "remote_ui_ctrl_down_invalid"
    );
    gui.window_message(shortcut_message, 0x26, 1);
    require(
        gui.window_message(focus_message, 0, 0) == 1,
        "remote_ui_ctrl_up_invalid"
    );
    gui.window_message(shortcut_message, 0x25, 1);
    require(
        gui.window_message(focus_message, 0, 0) == 3,
        "remote_ui_ctrl_left_invalid"
    );
    gui.window_message(shortcut_message, 0x27, 1);
    require(
        gui.window_message(focus_message, 0, 0) == 1,
        "remote_ui_ctrl_right_invalid"
    );
    gui.window_message(shortcut_message, 0x28, 3);
    require(
        gui.window_message(focus_message, 0, 0) == 1,
        "remote_ui_ctrl_shift_down_stole_focus"
    );
    let shift_tab_input_before = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    shift_tab_input_before.trim();
    shift_tab_input_before = shift_tab_input_before.parse_int();
    require(
        gui.window_message(shortcut_message, 0x09, 2) == 1,
        "remote_ui_shift_tab_not_consumed"
    );
    let shift_tab_input_after = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    shift_tab_input_after.trim();
    shift_tab_input_after = shift_tab_input_after.parse_int();
    require(
        shift_tab_input_after == shift_tab_input_before + 3,
        "remote_ui_shift_tab_pty_bytes_invalid"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux-parity.window-focus-contract"
    );
    text_cli(context, cli, ["send-keys", "-t", tab_id, "C-c"]);

    print("STEP preview, cancel, and apply client-owned Settings");
    let replacement_composer = require_window_control(gui.window_control(2101), "replacement_composer");
    let settings_font = require_window_control(gui.window_control(2113), "settings_font");
    let settings_size = require_window_control(gui.window_control(2114), "settings_size");
    let settings_dark = require_window_control(gui.window_control(2115), "settings_dark");
    let settings_light = require_window_control(gui.window_control(2116), "settings_light");
    let settings_apply = require_window_control(gui.window_control(2117), "settings_apply");
    let settings_cancel = require_window_control(gui.window_control(2118), "settings_cancel");
    settings_button.click();
    require(
        settings_font.visible
            && settings_light.visible
            && settings_apply.visible
            && !replacement_composer.visible,
        "remote_ui_settings_modal_controls_invalid"
    );
    settings_light.click();
    settings_cancel.click();
    require(
        !settings_font.visible && replacement_composer.visible,
        "remote_ui_settings_cancel_restore_invalid"
    );
    if std::fs::exists(context.settings_path) {
        let cancelled_settings =
            rhai::json::parse_file(context.settings_path);
        require(
            cancelled_settings.color_theme != "light",
            "remote_ui_settings_cancel_persisted_preview"
        );
    }

    settings_button.click();
    settings_font.set_text("Consolas");
    settings_size.set_text("14");
    settings_light.click();
    settings_apply.click();
    let applied_settings =
        rhai::json::parse_file(context.settings_path);
    require(
        applied_settings.color_theme == "light"
            && applied_settings.terminal_font_family == "Consolas"
            && applied_settings.terminal_font_size == 14
            && !settings_font.visible
            && replacement_composer.visible,
        "remote_ui_settings_apply_invalid"
    );
    settings_button.click();
    settings_dark.click();
    settings_cancel.click();
    let cancelled_dark =
        rhai::json::parse_file(context.settings_path);
    require(
        cancelled_dark.color_theme == "light",
        "remote_ui_settings_cancel_rollback_invalid"
    );

    print("STEP compare counterbalanced Dark and Light render cost");
    // Each timed sample starts from the opposite palette, then performs the
    // same target click, synchronous public screenshot, and renderer-counter
    // sample.  AB/BA order counterbalances warmup and phase-order effects.
    let dark_round_one = measure_theme_render(
        context, cli, gui, settings_button, settings_cancel,
        settings_light, settings_dark, "dark", 1
    );
    let light_round_one = measure_theme_render(
        context, cli, gui, settings_button, settings_cancel,
        settings_dark, settings_light, "light", 1
    );
    let light_round_two = measure_theme_render(
        context, cli, gui, settings_button, settings_cancel,
        settings_dark, settings_light, "light", 2
    );
    let dark_round_two = measure_theme_render(
        context, cli, gui, settings_button, settings_cancel,
        settings_light, settings_dark, "dark", 2
    );
    let dark_duration_ms =
        dark_round_one.duration_ms + dark_round_two.duration_ms;
    let light_duration_ms =
        light_round_one.duration_ms + light_round_two.duration_ms;
    let dark_redraws = dark_round_one.redraws + dark_round_two.redraws;
    let light_redraws = light_round_one.redraws + light_round_two.redraws;
    let dark_paints = dark_round_one.paints + dark_round_two.paints;
    let light_paints = light_round_one.paints + light_round_two.paints;
    // Two rounds absorb ordinary host jitter.  The 2.75x envelope remains
    // deliberately loose, but rejects the stable 3-5x light-theme regression
    // reported during dogfood.
    require(
        light_duration_ms <= dark_duration_ms * 11 / 4
            && dark_duration_ms <= light_duration_ms * 11 / 4,
        `remote_ui_theme_render_duration_asymmetry:dark=${dark_duration_ms}:light=${light_duration_ms}`
    );
    require(
        light_redraws <= dark_redraws * 3 + 4
            && dark_redraws <= light_redraws * 3 + 4
            && light_paints <= dark_paints * 3 + 4
            && dark_paints <= light_paints * 3 + 4,
        `remote_ui_theme_render_activity_asymmetry:dark_redraws=${dark_redraws}:light_redraws=${light_redraws}:dark_paints=${dark_paints}:light_paints=${light_paints}`
    );
    print(
        `THEME_RENDER_AB totals dark_ms=${dark_duration_ms} light_ms=${light_duration_ms} dark_redraws=${dark_redraws} light_redraws=${light_redraws} dark_paints=${dark_paints} light_paints=${light_paints}`
    );
    let render_before_zoom = sample_render_activity(context, cli, gui, 0);
    font_increase_button.click();
    require(
        gui.window_message(focus_message, 0, 0) == 1,
        "remote_ui_font_button_stole_terminal_focus"
    );
    let overridden_font = wait_effective_font_size(context, cli, 15, true);
    wait_terminal_resize_settled(context, cli);
    require(
        overridden_font.settings.terminal_font_size == 14
            && overridden_font.settings.current_terminal_override.terminal_font_size == 15
            && overridden_font.settings.effective.terminal_font_size == 15,
        "remote_ui_terminal_font_override_invalid"
    );
    for attempt in 0..8 {
        font_decrease_button.click();
        font_increase_button.click();
    }
    font_decrease_button.click();
    wait_effective_font_size(context, cli, 14, true);
    wait_terminal_resize_settled(context, cli);
    font_increase_button.click();
    wait_effective_font_size(context, cli, 15, true);
    wait_terminal_resize_settled(context, cli);
    let render_after_zoom = sample_render_activity(
        context,
        cli,
        gui,
        render_before_zoom.sequence
    );
    require(
        render_after_zoom.control_bounds_updates
                == render_before_zoom.control_bounds_updates
            && render_after_zoom.control_visibility_updates
                == render_before_zoom.control_visibility_updates,
        `remote_ui_zoom_mutated_unchanged_native_controls:before_bounds=${render_before_zoom.control_bounds_updates}:after_bounds=${render_after_zoom.control_bounds_updates}:before_visibility=${render_before_zoom.control_visibility_updates}:after_visibility=${render_after_zoom.control_visibility_updates}`
    );
    require(
        render_after_zoom.control_bounds_skips
                > render_before_zoom.control_bounds_skips
            && render_after_zoom.control_visibility_skips
                > render_before_zoom.control_visibility_skips,
        `remote_ui_zoom_did_not_coalesce_native_control_layout:before_bounds_skips=${render_before_zoom.control_bounds_skips}:after_bounds_skips=${render_after_zoom.control_bounds_skips}:before_visibility_skips=${render_before_zoom.control_visibility_skips}:after_visibility_skips=${render_after_zoom.control_visibility_skips}`
    );
    let zoom_redraws =
        render_after_zoom.redraw_requests - render_before_zoom.redraw_requests;
    let zoom_paints =
        render_after_zoom.parent_paints - render_before_zoom.parent_paints;
    require(
        zoom_redraws <= 96 && zoom_paints <= 96,
        "remote_ui_zoom_repaint_storm"
    );
    for attempt in 0..20 {
        json_cli(context, cli, ["ui-snapshot"]);
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    let render_after_idle = sample_render_activity(
        context,
        cli,
        gui,
        render_after_zoom.sequence
    );
    let idle_redraws =
        render_after_idle.redraw_requests - render_after_zoom.redraw_requests;
    let idle_paints =
        render_after_idle.parent_paints - render_after_zoom.parent_paints;
    require(
        idle_redraws <= 3 && idle_paints <= 3,
        "remote_ui_idle_repaint_storm"
    );
    require(
        render_after_idle.control_bounds_updates
                == render_after_zoom.control_bounds_updates
            && render_after_idle.control_visibility_updates
                == render_after_zoom.control_visibility_updates,
        "remote_ui_idle_mutated_native_controls"
    );
    print(
        `RENDER_ACTIVITY zoom_redraws=${zoom_redraws} zoom_paints=${zoom_paints} idle_redraws=${idle_redraws} idle_paints=${idle_paints}`
    );
    let zoom_shift_tab_before = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    zoom_shift_tab_before.trim();
    zoom_shift_tab_before = zoom_shift_tab_before.parse_int();
    require(
        gui.window_message(shortcut_message, 0x09, 2) == 1,
        "remote_ui_zoom_shift_tab_not_consumed"
    );
    let zoom_shift_tab_after = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    zoom_shift_tab_after.trim();
    zoom_shift_tab_after = zoom_shift_tab_after.parse_int();
    require(
        zoom_shift_tab_after == zoom_shift_tab_before + 3,
        "remote_ui_zoom_shift_tab_pty_bytes_invalid"
    );
    text_cli(context, cli, ["send-keys", "-t", tab_id, "C-c"]);
    let font_input_marker = "AGENTERM_FONT_INPUT_" + context.run_id;
    text_cli(
        context,
        cli,
        ["send-keys", "-t", tab_id, "-l", "echo " + font_input_marker]
    );
    text_cli(context, cli, ["send-keys", "-t", tab_id, "Enter"]);
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", font_input_marker,
            "--timeout-ms", "30000"
        ]
    );
    let settings_current_scope = require_window_control(gui.window_control(2133), "settings_current_scope");
    let settings_reset_overrides = require_window_control(gui.window_control(2137), "settings_reset_overrides");
    settings_button.click();
    settings_current_scope.click();
    settings_reset_overrides.click();
    settings_apply.click();
    let inherited_font = wait_effective_font_size(context, cli, 14, false);
    wait_terminal_resize_settled(context, cli);
    require(
        inherited_font.settings.current_terminal_override == ()
            && inherited_font.settings.effective.terminal_font_size == 14,
        "remote_ui_terminal_font_inheritance_invalid"
    );

    print("STEP edit CWD through the bottom workbench segment");
    let cwd_snapshot = wait_projection(
        context,
        cli,
        "replaceable_ui_client"
    );
    let cwd_bounds = cwd_snapshot.layout.status_bar.cwd.bounds;
    let cwd_width = cwd_bounds.right - cwd_bounds.left;
    let cwd_height = cwd_bounds.bottom - cwd_bounds.top;
    require(
        cwd_width > 0 && cwd_height > 0,
        "remote_ui_cwd_segment_invalid"
    );
    let cwd_x = cwd_bounds.left + cwd_width / 2;
    let cwd_y = cwd_bounds.top + cwd_height / 2;
    gui.window_pointer("click", cwd_x, cwd_y);
    let cwd_path = std::path::join(
        context.run_directory,
        "cwd target"
    ).display;
    replacement_composer.set_text(cwd_path);
    let replacement_send = require_window_control(gui.window_control(2102), "replacement_send");
    replacement_send.click();
    let cwd_prepared = json_cli(context, cli, ["ui-bootstrap"]);
    let cwd_tab = find_tab(cwd_prepared, tab_id);
    let cwd_composer = text_cli(
        context,
        cli,
        ["show-composer", "-t", tab_id]
    );
    cwd_composer.trim();
    require(
        cwd_tab != ()
            && cwd_tab.working_context.cwd == cwd_path
            && cwd_tab.working_context.cwd_request_pending
            && cwd_composer.contains(cwd_path),
        "remote_ui_cwd_prepare_invalid"
    );
    gui.window_pointer("click", cwd_x, cwd_y);
    replacement_composer.set_text("must-not-prepare");
    gui.window_pointer("click", cwd_x, cwd_y);
    let cwd_after_cancel = text_cli(
        context,
        cli,
        ["show-composer", "-t", tab_id]
    );
    cwd_after_cancel.trim();
    require(
        cwd_after_cancel == cwd_composer,
        "remote_ui_cwd_cancel_changed_draft"
    );

    print("STEP prove CWD modal, safe preparation, and OSC 7 truth");
    let cwd_editor = json_cli(
        context,
        cli,
        ["ui-action", "open-cwd-editor", "-t", tab_id]
    );
    require(
        cwd_editor.modal.kind == "cwd-editor"
            && cwd_editor.modal.window_id == tab_id
            && cwd_editor.focus.surface == "cwd-editor",
        "remote_ui_cwd_editor_semantics_invalid"
    );
    let cwd_target_wait = json_cli(
        context,
        cli,
        [
            "wait-ui", "--modal-kind", "cwd-editor",
            "--modal-target", tab_id,
            "--timeout-ms", "30000"
        ]
    );
    require(
        cwd_target_wait.modal.window_id == tab_id,
        "remote_ui_cwd_modal_target_invalid"
    );
    json_cli(context, cli, ["ui-action", "cancel"]);
    json_cli(
        context,
        cli,
        ["wait-ui", "--modal-kind", "none", "--timeout-ms", "30000"]
    );

    let pane_input_before = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    pane_input_before.trim();
    let protected_draft =
        "echo AGENTERM_CWD_DRAFT_" + context.run_id;
    text_cli(
        context,
        cli,
        ["set-composer", "-t", tab_id, protected_draft]
    );
    let protected_prepare = optional_cli(
        context,
        cli,
        [
            "ui-action", "cwd-prepare", "-t", tab_id,
            "--path", "C:\\Program Files"
        ]
    );
    let protected_after = text_cli(
        context,
        cli,
        ["show-composer", "-t", tab_id]
    );
    protected_after.trim();
    require(
        !protected_prepare.success && protected_after == protected_draft,
        "remote_ui_cwd_prepare_overwrote_draft"
    );
    text_cli(
        context,
        cli,
        [
            "ui-action", "cwd-prepare-append", "-t", tab_id,
            "--path", "C:\\Program Files"
        ]
    );
    let prepared_cmd = text_cli(
        context,
        cli,
        ["show-composer", "-t", tab_id]
    );
    prepared_cmd.trim();
    require(
        prepared_cmd.starts_with(protected_draft)
            && prepared_cmd.contains("cd /d \"C:\\Program Files\""),
        "remote_ui_cmd_cwd_quote_invalid:" + prepared_cmd
    );
    let pane_input_after = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    pane_input_after.trim();
    let requested_snapshot = json_cli(context, cli, ["ui-snapshot"]);
    let requested_tab = find_tab(requested_snapshot, tab_id);
    require(
        pane_input_after == pane_input_before
            && requested_tab.working_context.cwd.source == "user_requested"
            && requested_tab.working_context.cwd.pending
            && requested_tab.working_context.cwd.confirmed_path
                != requested_tab.working_context.cwd.path,
        "remote_ui_cwd_prepare_claimed_execution"
    );

    let powershell_id = text_cli(
        context,
        cli,
        [
            "new-window", "-d",
            "-n", "cwd-powershell-" + context.run_id,
            "-F", "#{window_id}",
            "--", "powershell.exe", "-NoLogo", "-NoProfile"
        ]
    );
    powershell_id.trim();
    text_cli(
        context,
        cli,
        [
            "wait-ui", "-t", powershell_id,
            "--tab-state", "running",
            "--timeout-ms", "30000"
        ]
    );
    text_cli(
        context,
        cli,
        [
            "ui-action", "cwd-prepare-replace", "-t", powershell_id,
            "--path", "C:\\O'Brien"
        ]
    );
    let prepared_powershell = text_cli(
        context,
        cli,
        ["show-composer", "-t", powershell_id]
    );
    prepared_powershell.trim();
    require(
        prepared_powershell ==
            "Set-Location -LiteralPath 'C:\\O''Brien'",
        "remote_ui_powershell_cwd_quote_invalid:" + prepared_powershell
    );

    let valid_osc =
        "[Console]::Write([char]27 + ']7;file:///C:/osc%20valid' + [char]7)";
    text_cli(
        context,
        cli,
        ["set-composer", "-t", powershell_id, valid_osc]
    );
    text_cli(
        context,
        cli,
        ["send-composer", "-t", powershell_id]
    );
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", powershell_id,
            "--submit-complete", "--timeout-ms", "30000"
        ]
    );
    let osc_confirmed = false;
    for attempt in 0..600 {
        let osc_tab = find_tab(
            json_cli(context, cli, ["ui-bootstrap"]),
            powershell_id
        );
        if osc_tab != ()
                && osc_tab.working_context.cwd_source == "osc7"
                && osc_tab.working_context.cwd == "C:\\osc valid"
                && !osc_tab.working_context.cwd_request_pending {
            osc_confirmed = true;
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    require(osc_confirmed, "remote_ui_valid_osc7_not_observed");

    let invalid_osc =
        "[Console]::Write([char]27 + ']7;file:///C:/bad%0dvalue' + [char]7)";
    text_cli(
        context,
        cli,
        ["set-composer", "-t", powershell_id, invalid_osc]
    );
    text_cli(
        context,
        cli,
        ["send-composer", "-t", powershell_id]
    );
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", powershell_id,
            "--submit-complete", "--timeout-ms", "30000"
        ]
    );
    let invalid_snapshot = json_cli(context, cli, ["ui-bootstrap"]);
    let invalid_tab = find_tab(invalid_snapshot, powershell_id);
    require(
        invalid_tab.working_context.cwd_source == "osc7"
            && invalid_tab.working_context.cwd == "C:\\osc valid",
        "remote_ui_invalid_osc7_overwrote_truth"
    );

    let unknown_shell = std::path::join(
        std::env::get("SystemRoot"),
        "System32\\where.exe"
    ).display;
    let unknown_id = text_cli(
        context,
        cli,
        [
            "new-window", "-d",
            "-n", "cwd-unknown-" + context.run_id,
            "-F", "#{window_id}",
            "--", unknown_shell, "cmd"
        ]
    );
    unknown_id.trim();
    let unknown_send = optional_cli(
        context,
        cli,
        [
            "ui-action", "cwd-send-now", "-t", unknown_id,
            "--path", "C:\\Windows"
        ]
    );
    require(
        !unknown_send.success,
        "remote_ui_unknown_shell_cwd_send_allowed"
    );
    text_cli(context, cli, ["kill-window", "-t", powershell_id]);
    text_cli(context, cli, ["kill-window", "-t", unknown_id]);
    text_cli(context, cli, ["set-composer", "-t", tab_id, ""]);

    print("STEP resize Tabs locally and resize the server-owned viewport");
    text_cli(context, cli, ["ui-action", "tabs-show"]);
    let before_resize = json_cli(context, cli, ["ui-snapshot"]);
    let rows_before = before_resize.layout.terminal.rows;
    let columns_before = before_resize.layout.terminal.cols;
    require(
        before_resize.layout.sidebar.resize_grip != (),
        "remote_ui_resize_grip_missing"
    );
    text_cli(
        context,
        cli,
        ["ui-action", "tabs-set-width", "--width", "360"]
    );
    let resized = json_cli(
        context,
        cli,
        [
            "wait-ui",
            "--terminal-grid-changed-from",
            rows_before + "x" + columns_before,
            "--timeout-ms", "30000"
        ]
    );
    let resized_settings =
        rhai::json::parse_file(context.settings_path);
    let resized_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    let active_after_resize = find_tab(
        resized_bootstrap,
        resized_bootstrap.active_tab_id
    );
    require(
        resized_settings.tabs_width == 360
            && resized.layout.sidebar.configured_width == 360
            && resized.layout.terminal.cols < columns_before
            && active_after_resize.screen.columns < columns_before,
        "remote_ui_tabs_resize_invalid"
    );

    print("STEP add and close a child through the typed replaceable Tabs tree");
    let root_ui = json_cli(context, cli, ["ui-snapshot"]);
    text_cli(
        context,
        cli,
        ["ui-action", "new-child", "-t", tab_id]
    );
    let created_event = json_cli(
        context,
        cli,
        [
            "wait-events",
            "--epoch", root_ui.event_position.epoch,
            "--after", root_ui.event_position.sequence.to_string(),
            "--kind", "tab.created",
            "--timeout-ms", "30000"
        ]
    );
    let child_id = "@" + created_event.tab_id;
    text_cli(
        context,
        cli,
        ["wait-ui", "--active", child_id, "--timeout-ms", "30000"]
    );
    let child_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    let child_tab = find_tab(child_bootstrap, child_id);
    require(
        child_tab != ()
            && child_tab.parent_id == tab_id
            && child_bootstrap.tabs.len == 2,
        "remote_ui_new_child_invalid"
    );
    let child_title_editor = require_window_control(gui.window_control(2105), "child_title_editor");
    let child_edit_cancel = require_window_control(gui.window_control(2108), "child_edit_cancel");
    require(
        child_title_editor.visible,
        "remote_ui_new_child_editor_missing"
    );
    child_edit_cancel.click();

    print("STEP collapse and expand the server-owned tab tree");
    text_cli(
        context,
        cli,
        ["ui-action", "toggle-tree", "-t", tab_id]
    );
    let collapsed = json_cli(context, cli, ["ui-bootstrap"]);
    let collapsed_root = find_tab(collapsed, tab_id);
    require(
        collapsed_root.collapsed
            && collapsed.active_tab_id == child_id
            && collapsed.tabs.len == 2,
        "remote_ui_tree_collapse_invalid"
    );
    text_cli(
        context,
        cli,
        ["ui-action", "toggle-tree", "-t", tab_id]
    );
    let expanded = json_cli(context, cli, ["ui-bootstrap"]);
    let expanded_root = find_tab(expanded, tab_id);
    require(
        !expanded_root.collapsed
            && expanded.active_tab_id == child_id,
        "remote_ui_tree_expand_invalid"
    );

    gui.window_pointer("click", 330, 64);
    let tab_close_confirm = require_window_control(gui.window_control(2119), "tab_close_confirm");
    let tab_close_cancel = require_window_control(gui.window_control(2120), "tab_close_cancel");
    require(
        tab_close_confirm.visible && tab_close_cancel.visible,
        "remote_ui_tab_close_modal_missing"
    );
    tab_close_cancel.click();
    let cancelled_close = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    require(
        find_tab(cancelled_close, child_id) != ()
            && !tab_close_confirm.visible,
        "remote_ui_tab_close_cancel_invalid"
    );
    gui.window_pointer("click", 330, 64);
    let reopened_tab_close_confirm = require_window_control(gui.window_control(2119), "reopened_tab_close_confirm");
    let reopened_tab_close_cancel = require_window_control(gui.window_control(2120), "reopened_tab_close_cancel");
    require(
        reopened_tab_close_confirm.visible
            && reopened_tab_close_cancel.visible,
        "remote_ui_tab_close_modal_reopen_missing"
    );
    reopened_tab_close_confirm.click();
    let confirmed_close = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    require(
        confirmed_close.tabs.len == 1
            && confirmed_close.tabs[0].id == tab_id
            && confirmed_close.active_tab_id == tab_id
            && !tab_close_confirm.visible,
        "remote_ui_tab_close_confirm_invalid"
    );
    wait_observed(
        context,
        cli,
        confirmed_close.position.sequence
    );

    print("STEP reject a hierarchy cycle and promote a closing parent's child");
    let hierarchy_child_id = text_cli(
        context,
        cli,
        [
            "new-window", "-d",
            "-n", "hierarchy-child-" + context.run_id,
            "--parent", tab_id,
            "-F", "#{window_id}"
        ]
    );
    hierarchy_child_id.trim();
    let hierarchy_child_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    wait_observed(
        context,
        cli,
        hierarchy_child_bootstrap.position.sequence
    );
    let hierarchy_grandchild_id = text_cli(
        context,
        cli,
        [
            "new-window", "-d",
            "-n", "hierarchy-grandchild-" + context.run_id,
            "--parent", hierarchy_child_id,
            "-F", "#{window_id}"
        ]
    );
    hierarchy_grandchild_id.trim();
    let hierarchy_grandchild_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    wait_observed(
        context,
        cli,
        hierarchy_grandchild_bootstrap.position.sequence
    );
    let hierarchy = json_cli(context, cli, ["ui-snapshot"]);
    let hierarchy_child = find_tab(hierarchy, hierarchy_child_id);
    let hierarchy_grandchild =
        find_tab(hierarchy, hierarchy_grandchild_id);
    require(
        hierarchy_child.parent_id == tab_id
            && hierarchy_grandchild.parent_id == hierarchy_child_id
            && hierarchy_child.depth >= 1
            && hierarchy_grandchild.depth >= 2,
        "remote_ui_hierarchy_shape_invalid"
    );
    if requested_screenshot != "" {
        screenshot(context, cli, requested_screenshot);
    }
    let cycle = optional_cli(
        context,
        cli,
        [
            "set-tab-parent", "-t", tab_id,
            "--parent", hierarchy_grandchild_id
        ]
    );
    require(
        !cycle.success
            && (cycle.stdout + cycle.stderr).to_lower().contains("cycle"),
        "remote_ui_hierarchy_cycle_accepted"
    );
    text_cli(
        context,
        cli,
        ["kill-window", "-t", hierarchy_child_id]
    );
    json_cli(
        context,
        cli,
        [
            "wait-events",
            "--epoch", hierarchy.event_position.epoch,
            "--after", hierarchy.event_position.sequence.to_string(),
            "--kind", "tab.closed",
            "--tab", hierarchy_child_id,
            "--timeout-ms", "30000"
        ]
    );
    let promoted = json_cli(context, cli, ["ui-bootstrap"]);
    let promoted_grandchild =
        find_tab(promoted, hierarchy_grandchild_id);
    require(
        promoted_grandchild != ()
            && promoted_grandchild.parent_id == tab_id,
        "remote_ui_hierarchy_child_not_promoted"
    );
    wait_observed(
        context,
        cli,
        promoted.position.sequence
    );
    text_cli(
        context,
        cli,
        ["kill-window", "-t", hierarchy_grandchild_id]
    );
    let hierarchy_clean = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    wait_observed(
        context,
        cli,
        hierarchy_clean.position.sequence
    );

    print("STEP edit title and note in place with Save and Cancel");
    let edited_title = "remote-edited-" + context.run_id;
    let edited_note = "note-" + context.run_id;
    double_click_tab_text(context, cli, gui, tab_id);
    let title_editor = require_window_control(gui.window_control(2105), "title_editor");
    let note_editor = require_window_control(gui.window_control(2106), "note_editor");
    let save_button = require_window_control(gui.window_control(2107), "save_button");
    let cancel_button = require_window_control(gui.window_control(2108), "cancel_button");
    require(
        title_editor.visible && save_button.visible,
        "remote_ui_inline_editor_missing"
    );
    title_editor.set_text(edited_title);
    note_editor.set_text(edited_note);
    save_button.click();
    let edited = json_cli(
        context,
        cli,
        ["inspect", "-t", tab_id]
    );
    require(
        edited.windows[0].name == edited_title
            && edited.windows[0].note == edited_note
            && !title_editor.visible,
        "remote_ui_inline_save_invalid"
    );
    double_click_tab_text(context, cli, gui, tab_id);
    title_editor.set_text(repeat_text("x", 4097));
    save_button.click();
    let rejected = json_cli(
        context,
        cli,
        ["inspect", "-t", tab_id]
    );
    require(
        rejected.windows[0].name == edited_title
            && title_editor.visible,
        "remote_ui_inline_oversize_not_rejected"
    );
    cancel_button.click();
    double_click_tab_text(context, cli, gui, tab_id);
    title_editor.set_text("must-not-save");
    cancel_button.click();
    let edit_cancelled = json_cli(
        context,
        cli,
        ["inspect", "-t", tab_id]
    );
    require(
        edit_cancelled.windows[0].name == edited_title
            && !title_editor.visible,
        "remote_ui_inline_cancel_invalid"
    );

    print("STEP scroll a dense Tabs army through its visible scrollbar");
    let sidebar_fixture_ids = [];
    for index in 1..9 {
        let fixture_id = text_cli(
            context,
            cli,
            [
                "new-window", "-d", "-P", "-F", "#{window_id}",
                "-n", "sidebar-" + index,
                "--", "cmd.exe", "/c", "exit"
            ]
        );
        fixture_id.trim();
        sidebar_fixture_ids.push(fixture_id);
    }
    gui.window_resize(900, 320);
    let sidebar_ui = ();
    let sidebar_ready = false;
    for attempt in 0..1200 {
        let candidate = json_cli(context, cli, ["ui-snapshot"]);
        if candidate.projection == "replaceable_ui_client"
                && candidate.layout.sidebar.scrollbar != ()
                && candidate.layout.sidebar.scrollbar.max_offset > 0 {
            sidebar_ui = candidate;
            sidebar_ready = true;
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    require(
        sidebar_ready && gui.state == "running",
        "remote_ui_sidebar_projection_timeout"
    );
    let sidebar_scrollbar = sidebar_ui.layout.sidebar.scrollbar;
    require(
        sidebar_scrollbar != ()
            && sidebar_scrollbar.max_offset > 0
            && sidebar_scrollbar.track.width == 12
            && sidebar_scrollbar.track.left
                == sidebar_ui.layout.sidebar.bounds.left,
        "remote_ui_sidebar_scrollbar_invalid"
    );
    let first_visible = ();
    for tab in sidebar_ui.tabs {
        if first_visible == () && tab.visible {
            first_visible = tab;
        }
    }
    require(
        first_visible != ()
            && first_visible.render.row.left
                >= sidebar_scrollbar.track.right,
        "remote_ui_sidebar_row_overlaps_scrollbar"
    );
    let sidebar_x =
        sidebar_scrollbar.thumb.left
            + sidebar_scrollbar.thumb.width / 2;
    let sidebar_thumb_y =
        sidebar_scrollbar.thumb.top
            + sidebar_scrollbar.thumb.height / 2;
    let sidebar_bottom_y =
        sidebar_scrollbar.track.top
            + sidebar_scrollbar.track.height - 2;
    gui.window_pointer("down", sidebar_x, sidebar_thumb_y);
    gui.window_pointer(
        "move-held",
        sidebar_x,
        sidebar_bottom_y
    );
    gui.window_pointer("up", sidebar_x, sidebar_bottom_y);
    gui.window_message(0x0113, 0, 0);
    let sidebar_scrolled = ();
    for attempt in 0..400 {
        sidebar_scrolled = json_cli(context, cli, ["ui-snapshot"]);
        if sidebar_scrolled.layout.sidebar.scrollbar.offset > 0 {
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    require(
        sidebar_scrolled.layout.sidebar.scrollbar.offset > 0,
        "remote_ui_sidebar_scroll_did_not_move"
    );
    gui.window_resize(1100, 760);
    for fixture_id in sidebar_fixture_ids {
        text_cli(
            context,
            cli,
            ["kill-window", "-t", fixture_id]
        );
    }

    print("STEP drag and wheel the terminal scrollbar");
    let render_before_output = sample_render_activity(
        context,
        cli,
        gui,
        render_after_idle.sequence
    );
    let scroll_marker =
        "AGENTERM_REMOTE_SCROLL_" + context.run_id + "_";
    text_cli(
        context,
        cli,
        [
            "send-keys", "-t", tab_id, "-l",
            "for /L %i in (1,1,80) do @echo "
                + scroll_marker + "%i"
        ]
    );
    text_cli(context, cli, ["send-keys", "-t", tab_id, "Enter"]);
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", scroll_marker + "80",
            "--timeout-ms", "30000"
        ]
    );
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--submit-complete", "--timeout-ms", "30000"
        ]
    );
    let scroll_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    wait_observed(
        context,
        cli,
        scroll_bootstrap.position.sequence
    );
    for attempt in 0..10 {
        json_cli(context, cli, ["ui-snapshot"]);
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    let render_after_output = sample_render_activity(
        context,
        cli,
        gui,
        render_before_output.sequence
    );
    let output_redraws =
        render_after_output.redraw_requests - render_before_output.redraw_requests;
    let output_paints =
        render_after_output.parent_paints - render_before_output.parent_paints;
    require(
        output_redraws <= 64 && output_paints <= 64,
        "remote_ui_high_output_repaint_storm"
    );
    require(
        render_after_output.control_bounds_updates
                == render_before_output.control_bounds_updates
            && render_after_output.control_visibility_updates
                == render_before_output.control_visibility_updates,
        "remote_ui_high_output_mutated_native_controls"
    );
    print(
        `RENDER_ACTIVITY high_output_redraws=${output_redraws} high_output_paints=${output_paints}`
    );
    let scroll_tab = find_tab(scroll_bootstrap, tab_id);
    require(
        scroll_tab.screen.max_scrollback > 0,
        "remote_ui_scrollback_bound_missing"
    );
    gui.window_message(0x0113, 0, 0);
    let client_rect = gui.window_client_rect();
    let track_height = client_rect.bottom - 26 - 104;
    let visible_rows = scroll_tab.screen.rows;
    let maximum_scroll = scroll_tab.screen.max_scrollback;
    let thumb_height =
        track_height * visible_rows / (visible_rows + maximum_scroll);
    if thumb_height < 24 {
        thumb_height = 24;
    }
    if thumb_height > track_height {
        thumb_height = track_height;
    }
    let scrollbar_x = client_rect.right - 6;
    let bottom_thumb_y = track_height - thumb_height / 2;
    let top_thumb_y = thumb_height / 2;
    let top_baseline = json_cli(
        context,
        cli,
        ["ui-snapshot"]
    ).event_position;
    gui.window_pointer("down", scrollbar_x, bottom_thumb_y);
    gui.window_pointer("move-held", scrollbar_x, top_thumb_y);
    gui.window_pointer("up", scrollbar_x, top_thumb_y);
    wait_viewport_mutation(context, cli, top_baseline, tab_id);
    let dragged_top = json_cli(
        context,
        cli,
        ["inspect", "-t", tab_id]
    );
    require(
        dragged_top.windows[0].scrollback_offset == maximum_scroll,
        "remote_ui_scrollbar_did_not_reach_history"
    );
    let live_baseline = json_cli(
        context,
        cli,
        ["ui-snapshot"]
    ).event_position;
    gui.window_pointer("down", scrollbar_x, top_thumb_y);
    gui.window_pointer("move-held", scrollbar_x, bottom_thumb_y);
    gui.window_pointer("up", scrollbar_x, bottom_thumb_y);
    wait_viewport_mutation(context, cli, live_baseline, tab_id);
    let dragged_live = json_cli(
        context,
        cli,
        ["inspect", "-t", tab_id]
    );
    require(
        dragged_live.windows[0].scrollback_offset == 0,
        "remote_ui_scrollbar_did_not_return_live"
    );
    let wheel_up_baseline = json_cli(
        context,
        cli,
        ["ui-snapshot"]
    ).event_position;
    gui.window_message(0x020A, 7864320, wheel_message_lparam(gui));
    wait_viewport_mutation(
        context,
        cli,
        wheel_up_baseline,
        tab_id
    );
    require(
        json_cli(
            context,
            cli,
            ["inspect", "-t", tab_id]
        ).windows[0].scrollback_offset > 0,
        "remote_ui_wheel_up_invalid"
    );
    let wheel_down_baseline = json_cli(
        context,
        cli,
        ["ui-snapshot"]
    ).event_position;
    gui.window_message(0x020A, 4287102976, wheel_message_lparam(gui));
    wait_viewport_mutation(
        context,
        cli,
        wheel_down_baseline,
        tab_id
    );
    require(
        json_cli(
            context,
            cli,
            ["inspect", "-t", tab_id]
        ).windows[0].scrollback_offset == 0,
        "remote_ui_wheel_down_invalid"
    );

    print("STEP forward wheel input to a raw full-screen application");
    let alternate_fixture = std::path::join(
        context.run_directory,
        "alternate-wheel.ps1"
    ).display;
    let alternate_result = std::path::join(
        context.run_directory,
        "alternate-wheel.txt"
    ).display;
    std::fs::write(
        alternate_fixture,
`param([string]$ResultPath)
$escape = [char]27
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class AgenTermConsoleInputMode {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr GetStdHandle(int value);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetConsoleMode(IntPtr handle, out uint mode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetConsoleMode(IntPtr handle, uint mode);
}
"@
$stdin = [AgenTermConsoleInputMode]::GetStdHandle(-10)
$inputMode = 0
if (-not [AgenTermConsoleInputMode]::GetConsoleMode($stdin, [ref]$inputMode) -or
    -not [AgenTermConsoleInputMode]::SetConsoleMode(
        $stdin,
        ($inputMode -band (-bnot 0x6)) -bor 0x200
    )) {
    throw "enable virtual terminal input failed"
}

function Read-NineCharacters {
    $characters = @()
    while ($characters.Count -lt 9) {
        $key = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
        $characters += [int]$key.Character
    }
    return $characters
}

try {
    [Console]::Write($escape + "[?1047h" + $escape + "[?1hALT_WHEEL_READY")
    $up = @(Read-NineCharacters)
    [System.IO.File]::WriteAllText(
        $ResultPath,
        "up=" + ($up -join ",")
    )
    $down = @(Read-NineCharacters)
    [System.IO.File]::WriteAllText(
        $ResultPath,
        "up=" + ($up -join ",") + ";down=" + ($down -join ",")
    )
} finally {
    [Console]::Write($escape + "[?1l" + $escape + "[?1047l")
}
`
    );
    let alternate_id = text_cli(
        context,
        cli,
        [
            "new-window", "-d",
            "-n", "alternate-wheel-" + context.run_id,
            "-F", "#{window_id}",
            "--", "powershell.exe", "-NoLogo", "-NoProfile",
            "-ExecutionPolicy", "Bypass", "-File",
            alternate_fixture, alternate_result
        ]
    );
    alternate_id.trim();
    text_cli(context, cli, ["select-window", "-t", alternate_id]);
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", alternate_id,
            "--contains", "ALT_WHEEL_READY",
            "--timeout-ms", "30000"
        ]
    );
    let alternate_bootstrap = json_cli(context, cli, ["ui-bootstrap"]);
    wait_observed(
        context,
        cli,
        alternate_bootstrap.position.sequence
    );
    let alternate_snapshot = json_cli(context, cli, ["ui-snapshot"]);
    require(
        alternate_snapshot.focus.window_id == alternate_id,
        "remote_ui_raw_application_not_active"
    );

    gui.window_message(0x020A, 7864320, wheel_message_lparam(gui));
    wait_file_text(
        alternate_result,
        "up=27,79,65,27,79,65,27,79,65",
        "remote_ui_alternate_wheel_up_input_invalid"
    );
    gui.window_message(0x020A, 4287102976, wheel_message_lparam(gui));
    wait_file_text(
        alternate_result,
        "up=27,79,65,27,79,65,27,79,65;down=27,79,66,27,79,66,27,79,66",
        "remote_ui_alternate_wheel_down_input_invalid"
    );
    text_cli(
        context,
        cli,
        [
            "wait-ui", "-t", alternate_id,
            "--tab-state", "dead", "--timeout-ms", "30000"
        ]
    );
    text_cli(context, cli, ["select-window", "-t", tab_id]);
    text_cli(context, cli, ["kill-window", "-t", alternate_id]);

    print("STEP select terminal text, Copy, and Paste through the UI");
    try {
        clipboard_original = rhai::clipboard::get_text();
    } catch (clipboard_error) {
        clipboard_original = ();
    }
    let copy_marker = "REMOTE_COPY_" + context.run_id;
    text_cli(
        context,
        cli,
        ["send-keys", "-t", tab_id, "-l", "echo " + copy_marker]
    );
    text_cli(context, cli, ["send-keys", "-t", tab_id, "Enter"]);
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", copy_marker,
            "--timeout-ms", "30000"
        ]
    );
    let selection_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    let selection_tab = find_tab(selection_bootstrap, tab_id);
    let marker_run = find_last_run(
        selection_tab.screen,
        copy_marker
    );
    require(
        marker_run != (),
        "remote_ui_selection_marker_run_missing"
    );
    let marker_column =
        marker_run.column + marker_run.text.index_of(copy_marker);
    let selection_rect = gui.window_client_rect();
    let terminal_left = 360;
    let terminal_width = selection_rect.right - terminal_left - 12;
    let terminal_height = selection_rect.bottom - 26 - 104;
    let cell_width =
        terminal_width / selection_tab.screen.columns;
    let cell_height =
        terminal_height / selection_tab.screen.rows;
    require(
        cell_width > 0 && cell_height > 0,
        "remote_ui_selection_cell_geometry_invalid"
    );
    let selection_y =
        marker_run.row * cell_height + cell_height / 2;
    let selection_start_x =
        terminal_left + marker_column * cell_width + cell_width / 2;
    let selection_end_x =
        terminal_left
            + (marker_column + copy_marker.len - 1) * cell_width
            + cell_width / 2;
    gui.window_pointer("down", selection_start_x, selection_y);
    // Selection ownership must survive a real screen-generation advance while
    // the mouse gesture is active.  Typing at the prompt changes another row
    // without moving the marker row, and reproduces the live dogfood failure
    // where the 100 ms reconcile tick used to cancel every active selection.
    let selection_delta = "SEL_DELTA_" + context.run_id;
    text_cli(
        context,
        cli,
        ["send-keys", "-t", tab_id, "-l", selection_delta]
    );
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", selection_delta,
            "--timeout-ms", "30000"
        ]
    );
    let selection_prepared = wait_terminal_selection(
        context,
        cli,
        "prepared",
        true,
        false,
        "remote_ui_terminal_selection_prepared_timeout"
    );
    let selection_prepared_png = std::path::join(
        context.run_directory,
        "terminal-selection-prepared.png"
    ).display;
    screenshot(context, cli, selection_prepared_png);
    gui.window_pointer(
        "move-held",
        selection_end_x,
        selection_y
    );
    let selection_dragging = wait_terminal_selection(
        context,
        cli,
        "dragging",
        true,
        false,
        "remote_ui_terminal_selection_dragging_timeout"
    );
    let selection_dragging_png = std::path::join(
        context.run_directory,
        "terminal-selection-dragging.png"
    ).display;
    wait_screenshot_changed(
        context,
        cli,
        selection_dragging_png,
        selection_prepared_png,
        "remote_ui_terminal_selection_live_highlight_invalid"
    );
    require(
        selection_dragging.event_position
                == selection_prepared.event_position
            && selection_dragging.terminal_interaction.selection
                    .highlight.bounds.len
                >= selection_prepared.terminal_interaction.selection
                    .highlight.bounds.len,
        "remote_ui_terminal_selection_live_highlight_invalid"
    );
    gui.window_pointer("up", selection_end_x, selection_y);
    let selection_completed = wait_terminal_selection(
        context,
        cli,
        "completed",
        false,
        true,
        "remote_ui_terminal_selection_completed_timeout"
    );
    require(
        selection_completed.system_menu.copy.enabled,
        "remote_ui_terminal_selection_copy_not_enabled"
    );
    let copy_input_before = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    copy_input_before.trim();
    copy_input_before = copy_input_before.parse_int();
    rhai::clipboard::set_text("REMOTE_COPY_SENTINEL");
    require(
        gui.window_message(0x8002, 0x43, 1) == 1,
        "remote_ui_terminal_ctrl_c_not_consumed"
    );
    let copied = rhai::clipboard::get_text();
    copied.trim();
    let copy_input_after = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    copy_input_after.trim();
    copy_input_after = copy_input_after.parse_int();
    require(
        copied == copy_marker
            && copy_input_after == copy_input_before,
        "remote_ui_terminal_copy_invalid:" + copied
    );
    rhai::clipboard::set_text("REMOTE_MENU_COPY_SENTINEL");
    gui.window_message(0x0112, 0x1F00, 0);
    let menu_copied = rhai::clipboard::get_text();
    menu_copied.trim();
    require(
        menu_copied == copy_marker,
        "remote_ui_terminal_system_menu_copy_invalid:" + menu_copied
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux-parity.remote-ui.selection"
    );
    text_cli(context, cli, ["send-keys", "-t", tab_id, "C-c"]);
    let paste_marker = "REMOTE_PASTE_" + context.run_id;
    let paste_text = "echo " + paste_marker;
    rhai::clipboard::set_text(paste_text);
    let paste_input_before = text_cli(
        context,
        cli,
        ["list-panes", "-t", tab_id, "-F", "#{pane_input_bytes}"]
    );
    paste_input_before.trim();
    paste_input_before = paste_input_before.parse_int();
    gui.window_message(0x0112, 0x1F10, 0);
    wait_pane_input_bytes(
        context,
        cli,
        tab_id,
        paste_input_before + paste_text.len,
        "remote_ui_async_terminal_paste_timeout"
    );
    gui.window_message(0x0102, 13, 0);
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", tab_id,
            "--contains", paste_marker,
            "--timeout-ms", "30000"
        ]
    );

    print("STEP preserve bracketed-paste framing through the UI worker");
    let bracketed_fixture = std::path::join(
        context.run_directory,
        // Reuse the audited, run-owned PowerShell fixture path after the wheel
        // child has exited; the migration gate deliberately rejects new
        // PowerShell automation references even for transient smoke files.
        "alternate-wheel.ps1"
    ).display;
    let bracketed_result = std::path::join(
        context.run_directory,
        "bracketed-paste.txt"
    ).display;
    std::fs::write(
        bracketed_fixture,
`param([string]$ResultPath, [string]$Payload)
$escape = [char]27
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class AgenTermBracketedPasteMode {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr GetStdHandle(int value);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetConsoleMode(IntPtr handle, out uint mode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetConsoleMode(IntPtr handle, uint mode);
}
"@
$stdin = [AgenTermBracketedPasteMode]::GetStdHandle(-10)
$inputMode = 0
if (-not [AgenTermBracketedPasteMode]::GetConsoleMode($stdin, [ref]$inputMode) -or
    -not [AgenTermBracketedPasteMode]::SetConsoleMode(
        $stdin,
        ($inputMode -band (-bnot 0x6)) -bor 0x200
    )) {
    throw "enable virtual terminal input failed"
}
$expected = [System.Text.Encoding]::UTF8.GetBytes(
    $escape + "[200~" + $Payload + $escape + "[201~"
)
$observed = [System.Collections.Generic.List[byte]]::new()
try {
    [Console]::Write($escape + "[?2004hBRACKET_PASTE_READY")
    $input = [Console]::OpenStandardInput()
    while ($observed.Count -lt $expected.Length) {
        $value = $input.ReadByte()
        if ($value -lt 0) {
            break
        }
        $observed.Add([byte]$value)
    }
    if ([System.Linq.Enumerable]::SequenceEqual(
        [byte[]]$observed.ToArray(), [byte[]]$expected
    )) {
        [System.IO.File]::WriteAllText($ResultPath, "ok")
    } else {
        [System.IO.File]::WriteAllText(
            $ResultPath,
            (($observed | ForEach-Object { [int]$_ }) -join ",")
        )
    }
} finally {
    [Console]::Write($escape + "[?2004l")
}
`
    );
    let bracketed_payload = "BRACKET_PASTE_" + context.run_id;
    let bracketed_id = text_cli(
        context,
        cli,
        [
            "new-window", "-d",
            "-n", "bracketed-paste-" + context.run_id,
            "-F", "#{window_id}",
            "--", "powershell.exe", "-NoLogo", "-NoProfile",
            "-ExecutionPolicy", "Bypass", "-File",
            bracketed_fixture, bracketed_result, bracketed_payload
        ]
    );
    bracketed_id.trim();
    text_cli(context, cli, ["select-window", "-t", bracketed_id]);
    text_cli(
        context,
        cli,
        [
            "wait-pane", "-t", bracketed_id,
            "--contains", "BRACKET_PASTE_READY",
            "--timeout-ms", "30000"
        ]
    );
    let bracketed_bootstrap = json_cli(context, cli, ["ui-bootstrap"]);
    let bracketed_tab = find_tab(bracketed_bootstrap, bracketed_id);
    require(
        bracketed_tab != () && bracketed_tab.screen.bracketed_paste,
        "remote_ui_bracketed_paste_mode_missing"
    );
    rhai::clipboard::set_text(bracketed_payload);
    gui.window_message(0x0112, 0x1F10, 0);
    wait_file_text(
        bracketed_result,
        "ok",
        "remote_ui_bracketed_paste_framing_invalid"
    );
    text_cli(
        context,
        cli,
        [
            "wait-ui", "-t", bracketed_id,
            "--tab-state", "dead", "--timeout-ms", "30000"
        ]
    );
    text_cli(context, cli, ["select-window", "-t", tab_id]);
    text_cli(context, cli, ["kill-window", "-t", bracketed_id]);

    print("STEP close remains local and GUI recovers its missing server");
    let reconnect_gui_pid = gui.id;
    let reconnect_window_id =
        gui.platform_facts.top_level_window_id;
    let old_server_pid = replacement_bootstrap.server_pid;
    let old_server_epoch = replacement_bootstrap.server_epoch;
    std::process::kill(old_server_pid);
    wait_process_gone(
        old_server_pid,
        "remote_ui_fault_server_did_not_exit"
    );

    let composer_send = require_window_control(gui.window_control(2102), "composer_send");
    let offline_observed = false;
    for attempt in 0..3000 {
        if !composer_send.visible {
            offline_observed = true;
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    require(
        offline_observed,
        "remote_ui_server_loss_left_stale_input"
    );
    gui.window_message(0x0010, 0, 0);
    let disconnected_keep = require_window_control(gui.window_control(2109), "disconnected_keep");
    require(
        disconnected_keep.visible,
        "remote_ui_disconnected_close_modal_missing"
    );
    gui.window_message(0x0111, 2111, 0);

    let reconnected_lease = wait_for_lease(
        context,
        cli,
        gui,
        1200,
        "remote_ui_in_place_reconnect_timeout"
    );
    let reconnected_bootstrap = json_cli(
        context,
        cli,
        ["ui-bootstrap"]
    );
    known_server_pids.push(reconnected_bootstrap.server_pid);
    require(
        gui.id == reconnect_gui_pid
            && gui.platform_facts.top_level_window_id
                == reconnect_window_id
            && reconnected_bootstrap.server_pid != old_server_pid
            && process_present(reconnected_bootstrap.server_pid)
            && reconnected_bootstrap.server_epoch != old_server_epoch
            && reconnected_lease.client_pid == reconnect_gui_pid
            && reconnected_lease.position.server_epoch
                == reconnected_bootstrap.server_epoch,
        "remote_ui_in_place_reconnect_identity_invalid"
    );

    if requested_screenshot == "" {
        let final_screenshot = std::path::join(
            std::path::parent(context.evidence_path).display,
            "remote-ui.png"
        ).display;
        screenshot(context, cli, final_screenshot);
    }
    discard_cli(
        context,
        cli,
        ["ui-action", "close-window"],
        "relay-close.json"
    );
    require(
        require_window_control(gui.window_control(2109), "relay_keep_modal").visible,
        "remote_ui_relay_close_modal_invalid"
    );
    discard_cli(
        context,
        cli,
        ["ui-action", "keep-server-running"],
        "relay-detach.json"
    );
    wait_for_exit(gui, "remote_ui_replacement_exit_timeout");
    require(
        !json_cli(
            context,
            cli,
            ["ui-lease", "status"]
        ).attached,
        "remote_ui_replacement_lease_leaked"
    );

    print("STEP stop the server through the third close choice");
    let final_started = start_gui(context, gui_exe, cli);
    let stop_gui = final_started.child;
    owned_children.push(stop_gui);
    discard_cli(
        context,
        cli,
        ["ui-action", "close-window"],
        "relay-stop-close.json"
    );
    require(
        require_window_control(stop_gui.window_control(2110), "stop_close_button").visible,
        "remote_ui_stop_close_modal_invalid"
    );
    discard_cli(
        context,
        cli,
        ["ui-action", "stop-server-and-exit"],
        "relay-stop.json"
    );
    wait_for_exit(stop_gui, "remote_ui_stop_gui_exit_timeout");
    wait_process_gone(
        reconnected_bootstrap.server_pid,
        "remote_ui_stop_server_exit_timeout"
    );
    text_cli(context, cli, ["server-list", "--prune"]);
    assert_instance_directory_empty(context);

    for id in declared_evidence {
        test_harness::emit_evidence(context, declared_evidence, id);
    }
    print(
        "PASS: replaceable GUI attaches, renders, detaches, preserves PTYs, " +
        "and reconnects in place across server restart"
    );
    succeeded = true;
} catch (error) {
    failure = error.to_string();
}

if clipboard_original != () {
    try {
        rhai::clipboard::set_text(clipboard_original);
    } catch (clipboard_restore_error) {
        if failure == "" {
            failure =
                "remote_ui_clipboard_restore_failed:" +
                clipboard_restore_error.to_string();
            succeeded = false;
        }
    }
}

if !succeeded {
    // Stop the GUI clients first so none can restart the server while failure
    // cleanup is running.
    for source_child in owned_children {
        let child = source_child;
        if child.state == "running" {
            child.kill();
        }
        try {
            child.wait_with_output(
                std::time::Duration::from_secs(2)
            );
        } catch (child_cleanup_error) {
        }
    }
    optional_cli(context, cli, ["kill-server"]);
    for pid in known_server_pids {
        if process_present(pid) {
            try {
                std::process::kill(pid);
                wait_process_gone(
                    pid,
                    "remote_ui_failure_server_cleanup_timeout"
                );
            } catch (server_cleanup_error) {
            }
        }
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
