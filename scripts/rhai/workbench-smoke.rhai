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
    let last = ();
    for attempt in 0..200 {
        let snapshot = json_cli(context, cli, ["ui-snapshot"]);
        last = snapshot;
        let matches = snapshot.tab_editor != ()
            && snapshot.tab_editor.target == target;
        let target_present = false;
        for tab in snapshot.tabs {
            if tab.id == target {
                target_present = true;
            }
        }
        if snapshot.projection == "replaceable_ui_client"
                && target_present
                && matches == open {
            return snapshot;
        }
        rhai::task::sleep(std::time::Duration::from_millis(10));
    }
    throw "workbench_editor_timeout:" + target + ":" + open +
        ":" + rhai::json::stringify(last);
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
         "--timeout-ms", "10000"]
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
         "--timeout-ms", "10000"]
    );
    json_cli(
        context,
        cli,
        ["wait-ui", "-t", root, "--tab-state", "running",
         "--timeout-ms", "10000"]
    );

    print(
        "STEP mouse Edit opens independent controls and mouse Save restores Edit"
    );
    let snapshot = json_cli(context, cli, ["ui-snapshot"]);
    let root_tab = find_tab(snapshot, root);
    require(
        root_tab.actions.edit.label == "Edit",
        "workbench_edit_action_missing:" +
            rhai::json::stringify(root_tab)
    );
    click_bounds(gui_child, root_tab.actions.edit.bounds);
    let snapshot = wait_for_editor(context, cli, root, true);
    let composer_before = text_cli(
        context,
        cli,
        ["show-composer", "-t", root]
    );
    let saved_name = "工作台-" + std::process::id();
    let saved_note = "inline note";
    text_cli(
        context,
        cli,
        ["set-composer", "-t", root, saved_name + "\n" + saved_note]
    );
    wait_for_editor_draft(
        context,
        cli,
        root,
        saved_name.len,
        saved_note.len
    );
    let root_tab = find_tab(snapshot, root);
    click_bounds(gui_child, root_tab.actions.save.bounds);
    let snapshot = wait_for_editor(context, cli, root, false);
    let root_tab = find_tab(snapshot, root);
    let composer_after = text_cli(
        context,
        cli,
        ["show-composer", "-t", root]
    );
    require(
        root_tab.name == saved_name
            && root_tab.note == saved_note
            && root_tab.actions.edit.label == "Edit"
            && composer_after == composer_before,
        "workbench_save_contract:" +
            rhai::json::stringify(root_tab)
    );

    print("STEP mouse Cancel discards both independent drafts");
    click_bounds(gui_child, root_tab.actions.edit.bounds);
    let snapshot = wait_for_editor(context, cli, root, true);
    text_cli(
        context,
        cli,
        ["set-composer", "-t", root,
         "discarded name\ndiscarded note"]
    );
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
    test_harness::emit_evidence(
        context,
        declared_evidence,
        "ux.workbench-inline-edit"
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
        text_cli(
            context,
            cli,
            ["set-composer", "-t", child.id,
             child_name + "\n" + child_note]
        );
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
            tab.actions.edit.bounds,
            "edit:" + width
        );
        assert_inside(
            tab.bounds,
            tab.actions.close.bounds,
            "close:" + width
        );
        let density = if width < 300 { "compact" } else { "full" };
        require(
            tab.actions.density == density
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
