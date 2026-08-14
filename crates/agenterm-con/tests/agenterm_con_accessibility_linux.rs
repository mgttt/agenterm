#![cfg(target_os = "linux")]

use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agenterm_platform::accessibility_tree::{
    AccessibilityNode, AccessibilityNodeAction, get_node_caret_offset, get_node_extents,
    get_node_selection, perform_node_action, scroll_node, set_node_caret_offset,
    set_node_selection, set_node_text, tree_for_window,
};

const DEADLINE: Duration = Duration::from_secs(20);

struct RunningCon {
    child: Child,
    scratch: PathBuf,
}

impl Drop for RunningCon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn scratch() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("agenterm-con-a11y-{}-{nonce}", std::process::id()))
}

fn cli(executable: &str, endpoint: &str, args: &[&str]) -> Output {
    Command::new(executable)
        .arg("cli")
        .arg("--control")
        .arg(endpoint)
        .args(args)
        .output()
        .expect("agenterm-con CLI starts")
}

fn cli_json(executable: &str, endpoint: &str, args: &[&str]) -> serde_json::Value {
    let output = cli(executable, endpoint, args);
    assert!(
        output.status.success(),
        "CLI {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI response is JSON")
}

fn wait_for<T>(
    running: &mut RunningCon,
    label: &str,
    mut probe: impl FnMut(&mut RunningCon) -> Option<T>,
) -> T {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(value) = probe(running) {
            return value;
        }
        if let Some(status) = running.child.try_wait().expect("poll agenterm-con") {
            let mut stderr = String::new();
            if let Some(mut stream) = running.child.stderr.take() {
                let _ = stream.read_to_string(&mut stderr);
            }
            panic!(
                "agenterm-con exited before {label}: {status}\nstderr:\n{}",
                stderr.trim()
            );
        }
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn named<'a>(nodes: &'a [AccessibilityNode], name: &str) -> Option<&'a AccessibilityNode> {
    nodes.iter().find(|node| node.name == name)
}

#[test]
fn real_atspi_tree_edits_command_and_activates_send() {
    let executable = env!("CARGO_BIN_EXE_agenterm-con");
    let scratch = scratch();
    std::fs::create_dir_all(&scratch).expect("create a11y scratch directory");
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700))
        .expect("make a11y scratch directory private");
    let endpoint = format!("unix:{}", scratch.join("control.sock").display());
    let child = Command::new(executable)
        .args(["--no-activate", "--control", &endpoint, "-e", "sh"])
        .env("RUST_BACKTRACE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch agenterm-con under AT-SPI session");
    let mut running = RunningCon { child, scratch };

    wait_for(&mut running, "control endpoint", |_| {
        let output = cli(executable, &endpoint, &["ui-snapshot"]);
        output.status.success().then_some(())
    });

    let tree = wait_for(&mut running, "published AT-SPI chrome", |_| {
        let tree = tree_for_window(None).ok()?;
        ["Tabs", "Session", "Command", "SEND", "OffscreenField"]
            .iter()
            .all(|name| named(&tree.nodes, name).is_some())
            .then_some(tree)
    });
    assert_eq!(tree.backend, "at-spi2");
    let command = named(&tree.nodes, "Command").expect("Command node");
    let send = named(&tree.nodes, "SEND").expect("SEND node");
    let field = named(&tree.nodes, "OffscreenField").expect("OffscreenField node");

    let command_id = command.id.clone();
    let send_id = send.id.clone();
    let field_id = field.id.clone();
    perform_node_action(None, &command_id, AccessibilityNodeAction::Focus)
        .expect("AT-SPI Command focus");
    wait_for(&mut running, "composer focus", |_| {
        let snapshot = cli_json(executable, &endpoint, &["ui-snapshot"]);
        (snapshot["composer_focused"] == true).then_some(())
    });
    set_node_text(None, &command_id, "printf ATSPI_OK").expect("AT-SPI SetTextContents");
    wait_for(&mut running, "composer text", |_| {
        let snapshot = cli_json(executable, &endpoint, &["ui-snapshot"]);
        (snapshot["composer_text"] == "printf ATSPI_OK").then_some(())
    });

    perform_node_action(None, &send_id, AccessibilityNodeAction::Click)
        .expect("AT-SPI SEND action");
    let matched = cli_json(
        executable,
        &endpoint,
        &["wait-text", "--timeout-ms", "10000", "ATSPI_OK"],
    );
    assert_eq!(matched["matched"], true);

    set_node_text(None, &command_id, "HELLO").expect("seed Command text for selection");
    wait_for(&mut running, "HELLO in composer", |_| {
        let snapshot = cli_json(executable, &endpoint, &["ui-snapshot"]);
        (snapshot["composer_text"] == "HELLO").then_some(())
    });
    let before_sel =
        get_node_selection(None, &command_id).expect("GetSelection before SetSelection");
    assert!(
        before_sel.n == 0 || before_sel.start != 0 || before_sel.end != 4,
        "pre-select range must not already be 0..4: {before_sel:?}"
    );
    set_node_selection(None, &command_id, 0, 4).expect("AT-SPI SetSelection");
    let after_sel = get_node_selection(None, &command_id).expect("GetSelection after SetSelection");
    assert_eq!(after_sel.n, 1);
    assert_eq!(after_sel.start, 0);
    assert_eq!(after_sel.end, 4);

    let before_caret =
        get_node_caret_offset(None, &command_id).expect("GetCaretOffset before SetCaretOffset");
    assert_ne!(
        before_caret, 2,
        "pre-set caret must not already be 2: {before_caret}"
    );
    set_node_caret_offset(None, &command_id, 2).expect("AT-SPI SetCaretOffset");
    let after_caret =
        get_node_caret_offset(None, &command_id).expect("GetCaretOffset after SetCaretOffset");
    assert_eq!(after_caret, 2);

    let before = get_node_extents(None, &field_id).expect("GetExtents before ScrollTo");
    assert!(
        before.width > 0 && before.height > 0,
        "OffscreenField extents must be non-empty: {before:?}"
    );
    scroll_node(None, &field_id).expect("AT-SPI Component.ScrollTo");
    let after = get_node_extents(None, &field_id).expect("GetExtents after ScrollTo");
    let delta_y = after.y.abs_diff(before.y);
    assert!(
        delta_y >= 20,
        "ScrollTo must move OffscreenField |Δy|>=20, before={before:?} after={after:?}"
    );

    let closed = cli_json(executable, &endpoint, &["close-window"]);
    assert_eq!(closed["closing"], true);
    wait_for(&mut running, "clean window exit", |running| {
        running.child.try_wait().ok().flatten()
    });
}
