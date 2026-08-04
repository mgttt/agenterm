// Prove physical inline tab editing, archived Proxy chrome, and compact deep
// tree geometry through public AgenTerm interfaces.
//
// Args:
//   --list-evidence
// or:
//   REPO GUI_EXE CLI_EXE

import "scripts/rhai/lib/test_harness" as test_harness;

let declared_evidence = [
    "ux.workbench-inline-edit",
    "ux.workbench-compact-tree",
    "ux.workbench-proxy-archived"
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

fn find_tab(snapshot, target) {
    for tab in snapshot.tabs {
        if tab.id == target {
            return tab;
        }
    }
    throw "workbench_tab_missing:" + target;
}

// Tab render geometry drops out of the snapshot for a frame while the
// sidebar re-renders (tab creation, tabs-show, selection changes), so any
// read that immediately consumes render bounds must settle instead of
// racing the redraw.
fn wait_tab_render(context, cli, target) {
    for attempt in 0..600 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        let tab = find_tab(snapshot, target);
        if tab != ()
                && tab.render != ()
                && tab.render.text != ()
                && tab.render.text.width > 0 {
            return tab;
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    throw "workbench_tab_render_timeout:" + target;
}

fn active_tab(snapshot) {
    for tab in snapshot.tabs {
        if tab.active {
            return tab;
        }
    }
    throw "workbench_active_tab_missing";
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
            throw "workbench_gui_early_exit:" + output.stderr_text();
        }
        rhai::task::sleep(std::time::Duration::from_millis(50));
    }
    throw "workbench_server_timeout";
}

fn wait_for_window(gui_child) {
    for attempt in 0..200 {
        if gui_child.platform_facts.top_level_window_present {
            return;
        }
        if gui_child.state == "exited" {
            throw "workbench_window_early_exit";
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    throw "workbench_window_timeout";
}

fn wait_for_editor(context, cli, target, open) {
    json_cli(
        context,
        cli,
        [
            "wait-ui",
            "-t", target,
            "--tab-editor-state", if open { "open" } else { "closed" },
            "--timeout-ms", "30000"
        ]
    )
}

fn wait_for_editor_draft(context, cli, target, name_len, note_len) {
    let last = ();
    for attempt in 0..200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        last = snapshot;
        if snapshot.tab_editor != ()
                && snapshot.tab_editor.target == target
                && snapshot.tab_editor.name_length == name_len
                && snapshot.tab_editor.note_length == note_len {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    throw "workbench_editor_draft_timeout:" + target +
        ":" + rhai::json::stringify(last);
}

fn click_bounds(gui_child, bounds) {
    let x_offset = bounds.width / 2;
    let y_offset = bounds.height / 2;
    if x_offset < 1 {
        x_offset = 1;
    }
    if y_offset < 1 {
        y_offset = 1;
    }
    gui_child.window_pointer(
        "click",
        bounds.left + x_offset,
        bounds.top + y_offset
    );
}

fn double_click_text(gui_child, bounds) {
    let x = bounds.left + bounds.width / 2;
    let y = bounds.top + bounds.height / 2;
    gui_child.window_pointer("click", x, y);
    gui_child.window_message(0x0203, 0, y * 65536 + x);
}

fn double_click_message(gui_child, bounds) {
    let x = bounds.left + bounds.width / 2;
    let y = bounds.top + bounds.height / 2;
    gui_child.window_message(0x0203, 0, y * 65536 + x);
}

fn double_click_point(gui_child, x, y) {
    gui_child.window_message(0x0203, 0, y * 65536 + x);
}

fn set_tab_editor_text(gui_child, name, note) {
    let title_editor = gui_child.window_control(2105);
    let note_editor = gui_child.window_control(2106);
    require(
        title_editor.visible && note_editor.visible,
        "workbench_tab_editor_controls_hidden"
    );
    title_editor.set_text(name);
    note_editor.set_text(note);
}

fn assert_editor_closed(snapshot, target, label) {
    let tab = find_tab(snapshot, target);
    let actions_closed = tab.actions == ()
        || (tab.actions.mode == "normal"
            && tab.actions.edit == ()
            && tab.actions.save == ()
            && tab.actions.cancel == ());
    let render_closed = tab.render == () || tab.render.mode == "normal";
    require(
        snapshot.tab_editor == ()
            && render_closed
            && actions_closed,
        "workbench_editor_should_be_closed:" + label +
            ":" + rhai::json::stringify(tab)
    );
}

fn assert_editor_open(snapshot, target, label) {
    let tab = find_tab(snapshot, target);
    require(
        snapshot.tab_editor != ()
            && snapshot.tab_editor.target == target
            && tab.render.mode == "editing"
            && tab.render.editors != ()
            && tab.actions.mode == "editing"
            && tab.actions.edit == ()
            && tab.actions.save.id == "tab-editor-save"
            && tab.actions.cancel.id == "tab-editor-cancel"
            && tab.actions.save.bounds.width > 0
            && tab.actions.cancel.bounds.width > 0,
        "workbench_editor_should_be_open:" + label +
            ":" + rhai::json::stringify(tab)
    );
}

fn retain_png_evidence(context, cli, name) {
    let path = std::path::join(
        context.run_directory,
        "evidence/" + name + ".png"
    ).display;
    text_cli(context, cli, ["screenshot", "-o", path]);
    require(
        std::fs::metadata(path).len >= 1000,
        "workbench_screenshot_too_small:" + name
    );
    path
}

fn assert_inside(outer, inner, label) {
    require(
        inner.left >= outer.left
            && inner.top >= outer.top
            && inner.right <= outer.right
            && inner.bottom <= outer.bottom
            && inner.width >= 0
            && inner.height >= 0,
        "workbench_geometry_escape:" + label +
            ":" + rhai::json::stringify(inner)
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

require(args.len == 3, "expected: REPO GUI_EXE CLI_EXE");
let repo = std::path::absolute(args[0]).display;
let gui = std::path::absolute(args[1]).display;
let cli = std::path::absolute(args[2]).display;
require(std::fs::exists(gui), "workbench_gui_missing");
require(std::fs::exists(cli), "workbench_cli_missing");

let context = test_harness::new_context(repo, "workbench");
let owned_children = [];
let succeeded = false;
let failure = "";

try {
    let command = test_harness::configured_command(
        context,
        gui,
        ["--no-activate", "--address", context.address]
    );
    for name in [
        "HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"
    ] {
        command.env_remove(name);
    }
    let gui_child = command.start();
    owned_children.push(gui_child);
    wait_for_server(context, cli, gui_child);
    wait_for_window(gui_child);
    json_cli(
        context,
        cli,
        ["wait-ui", "--window-state", "restored",
         "--timeout-ms", "30000"]
    );

    let root_name = "workbench-" + std::process::id();
    let root = text_cli(
        context,
        cli,
        ["new-window", "-d", "-n", root_name,
         "-F", "#{window_id}"]
    );
    root.trim();
    text_cli(context, cli, ["select-window", "-t", root]);
    json_cli(
        context,
        cli,
        ["wait-ui", "--active", root, "--window-state", "restored",
         "--timeout-ms", "30000"]
    );
    json_cli(
        context,
        cli,
        ["wait-ui", "-t", root, "--tab-state", "running",
         "--timeout-ms", "30000"]
    );

    let peer_name = "workbench-peer-" + std::process::id();
    let peer = text_cli(
        context,
        cli,
        ["new-window", "-d", "-n", peer_name,
         "-F", "#{window_id}"]
    );
    peer.trim();
    text_cli(context, cli, ["select-window", "-t", root]);
    let snapshot = json_cli(
        context,
        cli,
        ["wait-ui", "--active", root, "--window-state", "restored",
         "--timeout-ms", "30000"]
    );
    let root_tab = find_tab(snapshot, root);
    let peer_tab = find_tab(snapshot, peer);
    assert_editor_closed(snapshot, root, "initial-normal");

    print("STEP a single text click selects without entering edit mode");
    let peer_tab = wait_tab_render(context, cli, peer);
    click_bounds(gui_child, peer_tab.render.text);
    let snapshot = json_cli(
        context,
        cli,
        ["wait-ui", "--active", peer, "--focus", "tabs",
         "--timeout-ms", "30000"]
    );
    assert_editor_closed(snapshot, peer, "single-click-peer");
    let root_tab = wait_tab_render(context, cli, root);
    click_bounds(gui_child, root_tab.render.text);
    let snapshot = json_cli(
        context,
        cli,
        ["wait-ui", "--active", root, "--focus", "tabs",
         "--timeout-ms", "30000"]
    );
    assert_editor_closed(snapshot, root, "single-click-root");

    print("STEP non-text double clicks never enter tab editing");
    let root_tab = wait_tab_render(context, cli, root);
    for zone in [
        root_tab.render.disclosure_hit,
        root_tab.render.status,
        root_tab.actions.new_child.bounds,
        root_tab.actions.close.bounds
    ] {
        double_click_message(gui_child, zone);
        assert_editor_closed(
            json_cli(context, cli, ["ui-snapshot"]),
            root,
            "non-text-zone"
        );
    }
    double_click_point(
        gui_child,
        root_tab.render.row.left + 1,
        root_tab.render.row.top + root_tab.render.row.height / 2
    );
    assert_editor_closed(
        json_cli(context, cli, ["ui-snapshot"]),
        root,
        "tree-indent"
    );

    print(
        "STEP double-clicking tab text opens independent controls and Save restores normal mode"
    );
    let root_tab = wait_tab_render(context, cli, root);
    double_click_text(gui_child, root_tab.render.text);
    let snapshot = wait_for_editor(context, cli, root, true);
    assert_editor_open(snapshot, root, "mouse-double-click");
    let composer_before = text_cli(
        context,
        cli,
        ["show-composer", "-t", root]
    );
    let saved_name = "工作台-" + std::process::id();
    let saved_note = "inline note";
    set_tab_editor_text(gui_child, saved_name, saved_note);
    wait_for_editor_draft(
        context,
        cli,
        root,
        saved_name.len,
        saved_note.len
    );
    retain_png_evidence(context, cli, "tab-editor-open");
    let root_tab = find_tab(snapshot, root);
    click_bounds(gui_child, root_tab.actions.save.bounds);
    let snapshot = wait_for_editor(context, cli, root, false);
    let root_tab = find_tab(snapshot, root);
    assert_editor_closed(snapshot, root, "mouse-save");
    retain_png_evidence(context, cli, "tab-editor-normal");
    let composer_after = text_cli(
        context,
        cli,
        ["show-composer", "-t", root]
    );
    require(
        root_tab.name == saved_name
            && root_tab.note == saved_note
            && root_tab.actions.edit == ()
            && composer_after == composer_before,
        "workbench_save_contract:" +
            rhai::json::stringify(root_tab)
    );

    print("STEP mouse Cancel discards both independent drafts");
    double_click_text(gui_child, root_tab.render.text);
    let snapshot = wait_for_editor(context, cli, root, true);
    set_tab_editor_text(gui_child, "discarded name", "discarded note");
    wait_for_editor_draft(
        context,
        cli,
        root,
        "discarded name".len,
        "discarded note".len
    );
    let root_tab = find_tab(snapshot, root);
    click_bounds(gui_child, root_tab.actions.cancel.bounds);
    let snapshot = wait_for_editor(context, cli, root, false);
    let root_tab = find_tab(snapshot, root);
    require(
        root_tab.name == saved_name && root_tab.note == saved_note,
        "workbench_cancel_persisted_draft"
    );

    print("STEP F2 and public edit-tab expose the same inline editor contract");
    json_cli(context, cli, ["focus", "tabs", "-t", root]);
    gui_child.window_key("F2");
    let f2_snapshot = wait_for_editor(context, cli, root, true);
    assert_editor_open(f2_snapshot, root, "f2");
    gui_child.window_key("Escape");
    assert_editor_closed(
        wait_for_editor(context, cli, root, false),
        root,
        "f2-escape"
    );
    json_cli(context, cli, ["ui-action", "edit-tab", "-t", root]);
    let public_snapshot = wait_for_editor(context, cli, root, true);
    assert_editor_open(public_snapshot, root, "public-edit-tab");
    let f2_actions = find_tab(f2_snapshot, root).actions;
    let public_actions = find_tab(public_snapshot, root).actions;
    require(
        f2_actions.mode == public_actions.mode
            && f2_actions.density == public_actions.density
            && f2_actions.save.id == public_actions.save.id
            && f2_actions.cancel.id == public_actions.cancel.id,
        "workbench_f2_public_editor_mismatch"
    );
    gui_child.window_key("Escape");
    wait_for_editor(context, cli, root, false);

    print("STEP tab switch and Tabs hide cancel unsaved inline drafts");
    json_cli(context, cli, ["ui-action", "edit-tab", "-t", root]);
    wait_for_editor(context, cli, root, true);
    set_tab_editor_text(gui_child, "switch draft", "switch note");
    wait_for_editor_draft(
        context,
        cli,
        root,
        "switch draft".len,
        "switch note".len
    );
    json_cli(context, cli, ["ui-action", "select-tab", "-t", peer]);
    assert_editor_closed(
        wait_for_editor(context, cli, root, false),
        root,
        "tab-switch"
    );
    let root_tab = find_tab(
        json_cli(context, cli, ["ui-snapshot"]),
        root
    );
    require(
        root_tab.name == saved_name && root_tab.note == saved_note,
        "workbench_tab_switch_persisted_draft"
    );
    json_cli(context, cli, ["ui-action", "select-tab", "-t", root]);
    json_cli(context, cli, ["ui-action", "edit-tab", "-t", root]);
    wait_for_editor(context, cli, root, true);
    set_tab_editor_text(gui_child, "hide draft", "hide note");
    wait_for_editor_draft(
        context,
        cli,
        root,
        "hide draft".len,
        "hide note".len
    );
    json_cli(context, cli, ["ui-action", "tabs-hide"]);
    assert_editor_closed(
        wait_for_editor(context, cli, root, false),
        root,
        "tabs-hide"
    );
    json_cli(context, cli, ["ui-action", "tabs-show"]);
    let root_tab = wait_tab_render(context, cli, root);
    require(
        root_tab.name == saved_name && root_tab.note == saved_note,
        "workbench_tabs_hide_persisted_draft"
    );

    print("STEP layout changes invalidate an unfinished double-click candidate");
    let root_tab = wait_tab_render(context, cli, root);
    click_bounds(gui_child, root_tab.render.text);
    json_cli(context, cli, ["ui-action", "select-tab", "-t", peer]);
    double_click_message(gui_child, root_tab.render.text);
    assert_editor_closed(
        json_cli(context, cli, ["ui-snapshot"]),
        root,
        "candidate-tab-switch"
    );
    json_cli(context, cli, ["ui-action", "select-tab", "-t", root]);
    let snapshot = json_cli(context, cli, ["ui-snapshot"]);
    let root_tab = find_tab(snapshot, root);
    click_bounds(gui_child, root_tab.render.text);
    json_cli(context, cli, ["ui-action", "tabs-hide"]);
    json_cli(context, cli, ["ui-action", "tabs-show"]);
    let snapshot = json_cli(context, cli, ["ui-snapshot"]);
    let root_tab = find_tab(snapshot, root);
    double_click_message(gui_child, root_tab.render.text);
    assert_editor_closed(
        json_cli(context, cli, ["ui-snapshot"]),
        root,
        "candidate-tabs-hide"
    );
    let pre_resize_text = root_tab.render.text;
    click_bounds(gui_child, pre_resize_text);
    gui_child.window_resize(1000, 680);
    double_click_message(gui_child, pre_resize_text);
    assert_editor_closed(
        json_cli(context, cli, ["ui-snapshot"]),
        root,
        "candidate-window-resize"
    );

    print("STEP archived Proxy status surface releases its layout and actions");
    let proxy = snapshot.layout.status_bar.proxy;
    require(
        proxy.archived
            && !proxy.available
            && proxy.bounds.width == 0
            && proxy.action == ()
            && proxy.eye_action == (),
        "workbench_proxy_surface_not_archived"
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux.workbench-proxy-archived"
    );

    print("STEP compact and full geometry stay bounded for a deep CJK row");
    let target = root;
    for depth in 1..5 {
        let snapshot = json_cli(
            context,
            cli,
            ["ui-action", "new-child", "-t", target]
        );
        let child = active_tab(snapshot);
        let child_name = "层级-" + depth + "-工作";
        let child_note = "备注-" + depth;
        set_tab_editor_text(gui_child, child_name, child_note);
        wait_for_editor_draft(
            context,
            cli,
            child.id,
            child_name.len,
            child_note.len
        );
        click_bounds(gui_child, child.actions.save.bounds);
        wait_for_editor(context, cli, child.id, false);
        target = child.id;
    }

    print("STEP sidebar scrolling clears a pending text double-click candidate");
    for index in 1..7 {
        text_cli(
            context,
            cli,
            [
                "new-window", "-d", "-P", "-n",
                "candidate-scroll-" + index,
                "--", "cmd.exe", "/c", "exit"
            ]
        );
    }
    gui_child.window_resize(900, 320);
    let scroll_snapshot = ();
    for attempt in 0..40 {
        let candidate = json_cli(context, cli, ["ui-snapshot"]);
        if candidate.projection == "replaceable_ui_client"
                && candidate.layout.sidebar != ()
                && candidate.layout.sidebar.scrollbar != ()
                && candidate.layout.sidebar.scrollbar.max_offset > 0 {
            scroll_snapshot = candidate;
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    require(
        scroll_snapshot != (),
        "workbench_sidebar_scrollbar_timeout"
    );
    let first_visible = ();
    for tab in scroll_snapshot.tabs {
        if first_visible == ()
                && tab.visible
                && tab.render.text.width > 0 {
            first_visible = tab;
        }
    }
    require(first_visible != (), "workbench_visible_scroll_tab_missing");
    click_bounds(gui_child, first_visible.render.text);
    let scrollbar = scroll_snapshot.layout.sidebar.scrollbar;
    let thumb_x = scrollbar.thumb.left + scrollbar.thumb.width / 2;
    let thumb_y = scrollbar.thumb.top + scrollbar.thumb.height / 2;
    let bottom_y = scrollbar.track.top + scrollbar.track.height - 2;
    gui_child.window_pointer("down", thumb_x, thumb_y);
    gui_child.window_pointer("move-held", thumb_x, bottom_y);
    gui_child.window_pointer("up", thumb_x, bottom_y);
    let scrolled = ();
    for attempt in 0..40 {
        let candidate = json_cli(context, cli, ["ui-snapshot"]);
        if candidate.projection == "replaceable_ui_client"
                && candidate.layout.sidebar != ()
                && candidate.layout.sidebar.scrollbar != ()
                && candidate.layout.sidebar.scrollbar.offset > 0 {
            scrolled = candidate;
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(25));
    }
    require(
        scrolled != (),
        "workbench_sidebar_scroll_did_not_move"
    );
    double_click_message(gui_child, first_visible.render.text);
    require(
        wait_for_editor(context, cli, first_visible.id, false).tab_editor == (),
        "workbench_scroll_retained_double_click_candidate"
    );
    gui_child.window_resize(1000, 680);
    let scrollbar = scrolled.layout.sidebar.scrollbar;
    let thumb_x = scrollbar.thumb.left + scrollbar.thumb.width / 2;
    let thumb_y = scrollbar.thumb.top + scrollbar.thumb.height / 2;
    gui_child.window_pointer("down", thumb_x, thumb_y);
    gui_child.window_pointer("move-held", thumb_x, scrollbar.track.top + 2);
    gui_child.window_pointer("up", thumb_x, scrollbar.track.top + 2);
    json_cli(context, cli, ["ui-action", "select-tab", "-t", target]);
    json_cli(
        context,
        cli,
        ["wait-ui", "--active", target, "--focus", "terminal",
         "--timeout-ms", "30000"]
    );
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux.workbench-inline-edit"
    );

    for width in [180, 250, 480] {
        json_cli(
            context,
            cli,
            ["ui-action", "tabs-set-width", "--width", width.to_string()]
        );
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        let tab = find_tab(snapshot, target);
        assert_inside(
            tab.bounds,
            tab.render.name,
            "name:" + width
        );
        assert_inside(
            tab.bounds,
            tab.render.note,
            "note:" + width
        );
        assert_inside(
            tab.bounds,
            tab.actions.close.bounds,
            "close:" + width
        );
        let density = if width < 300 { "compact" } else { "full" };
        require(
            tab.actions.density == density
                && tab.actions.edit == ()
                && tab.render.node.x >= tab.bounds.left
                && tab.render.name.left < tab.actions.new_child.bounds.left,
            "workbench_tree_geometry:" + width +
                ":" + rhai::json::stringify(tab)
        );
    }
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux.workbench-compact-tree"
    );
    print("PASS: inline Tabs editing and compact tree geometry");
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
