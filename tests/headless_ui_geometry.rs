//! Behavioural coverage for the headless (`headless_server`) projection's
//! synthetic geometry -- decision D-1 in `plan/agent-human-parity-audit.md`.
//!
//! Why this file exists: `ui-input` addresses window chrome in **pixels**, and
//! until now the only source of those pixels was a live GUI window, so the
//! whole perceive->act surface had zero coverage that CI could run. The
//! workspace layout is a pure function of a client rectangle, so a headless
//! server can publish the same bounds from a *nominal* viewport. These tests
//! drive a real server process over the control protocol and check the loop an
//! agent actually runs: read a snapshot, locate a target, act, read again, and
//! see the geometry follow.
//!
//! What they deliberately do **not** claim: that anything was rendered. Every
//! rectangle here is tagged `geometry_source: "synthetic"`, and a live-window
//! smoke stays mandatory for pixel fidelity. These tests are to a rendering
//! smoke what a layout unit test is to a screenshot diff.

use std::{
    ffi::OsStr,
    fs,
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

/// Nominal viewport the tests ask for, so they also prove the override path.
const VIEWPORT: &str = "1400x900";
const VIEWPORT_WIDTH: i64 = 1400;
const VIEWPORT_HEIGHT: i64 = 900;

struct Harness {
    root: PathBuf,
    address: String,
    workspace: PathBuf,
    settings: PathBuf,
    instances: PathBuf,
    server: Option<std::process::Child>,
}

impl Harness {
    fn start(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenterm-headless-geometry-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_millis()
        ));
        fs::create_dir_all(&root).expect("create isolation root");
        let address = TcpListener::bind("127.0.0.1:0")
            .expect("reserve loopback address")
            .local_addr()
            .expect("loopback address")
            .to_string();
        let instances = root.join("instances");
        fs::create_dir_all(&instances).expect("create instance directory");
        let mut harness = Self {
            workspace: root.join("workspace.json"),
            settings: root.join("settings.json"),
            instances,
            address,
            root,
            server: None,
        };
        harness.server = Some(
            harness
                .command(env!("CARGO_BIN_EXE_agenterm"))
                .args(["server", "--address", &harness.address])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start isolated headless server"),
        );
        harness.wait_for(&["protocol-info", "--running"], Duration::from_secs(20));
        harness
    }

    fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env("AGENTERM_IPC_ADDRESS", &self.address)
            .env("AGENTERM_WORKSPACE_PATH", &self.workspace)
            .env("AGENTERM_SETTINGS_PATH", &self.settings)
            .env("AGENTERM_INSTANCE_DIR", &self.instances)
            .env("AGENTERM_NO_ACTIVATE", "1")
            .env("AGENTERM_HEADLESS_VIEWPORT", VIEWPORT);
        command
    }

    fn cli(&self, arguments: &[&str]) -> std::process::Output {
        self.command(env!("CARGO_BIN_EXE_agenterm-cli"))
            .args(["--address", &self.address])
            .args(arguments)
            .output()
            .expect("run isolated CLI")
    }

    fn wait_for(&self, arguments: &[&str], timeout: Duration) -> std::process::Output {
        let deadline = Instant::now() + timeout;
        loop {
            let output = self.cli(arguments);
            if output.status.success() {
                return output;
            }
            assert!(
                Instant::now() < deadline,
                "`{arguments:?}` never succeeded: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn snapshot(&self) -> Value {
        let output = self.cli(&["ui-snapshot"]);
        assert!(
            output.status.success(),
            "ui-snapshot failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("ui-snapshot JSON")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            let _ = self.cli(&["kill-server"]);
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if matches!(server.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Rect {
    fn read(value: &Value, what: &str) -> Self {
        let read = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_i64)
                .unwrap_or_else(|| panic!("{what} has no `{key}`: {value}"))
        };
        let rect = Self {
            left: read("left"),
            top: read("top"),
            right: read("right"),
            bottom: read("bottom"),
        };
        assert!(
            rect.right > rect.left && rect.bottom > rect.top,
            "{what} is degenerate, nothing could be aimed at it: {rect:?}"
        );
        assert!(
            rect.left >= 0
                && rect.top >= 0
                && rect.right <= VIEWPORT_WIDTH
                && rect.bottom <= VIEWPORT_HEIGHT,
            "{what} escapes the nominal viewport: {rect:?}"
        );
        rect
    }

    fn center(self) -> (i64, i64) {
        ((self.left + self.right) / 2, (self.top + self.bottom) / 2)
    }

    fn contains(self, (x, y): (i64, i64)) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }
}

fn tab_by_id<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    snapshot["tabs"]
        .as_array()
        .expect("tabs array")
        .iter()
        .find(|tab| tab["id"] == id)
        .unwrap_or_else(|| panic!("no tab {id} in {}", snapshot["tabs"]))
}

/// perceive: a headless snapshot carries the chrome an agent would click, and
/// says out loud that it was computed rather than rendered.
#[test]
fn headless_snapshot_publishes_labelled_synthetic_chrome_geometry() {
    let harness = Harness::start("perceive");
    let snapshot = harness.snapshot();

    assert_eq!(snapshot["projection"], "headless_server");
    // Honesty is the whole precondition for publishing these numbers.
    assert_eq!(snapshot["geometry_source"], "synthetic");
    assert_eq!(snapshot["layout"]["geometry_source"], "synthetic");
    // A window truly is absent; the geometry never pretends otherwise.
    assert_eq!(snapshot["window"]["visible"], false);
    assert_eq!(snapshot["window"]["detached"], true);
    assert_eq!(snapshot["layout"]["composer"]["visible"], false);

    let viewport = &snapshot["layout"]["viewport"];
    assert_eq!(viewport["width"], VIEWPORT_WIDTH);
    assert_eq!(viewport["height"], VIEWPORT_HEIGHT);
    assert_eq!(
        viewport["source"], "environment",
        "the nominal viewport must say where it came from"
    );

    let toolbar = &snapshot["layout"]["toolbar"];
    let buttons = [
        "tabs",
        "new",
        "control_center",
        "settings",
        "locale",
        "font_decrease",
        "font_increase",
    ]
    .map(|key| (key, Rect::read(&toolbar[key], key)));
    // A pointer aimed at one button must not be inside another, otherwise the
    // published bounds cannot be used to drive anything.
    for (name, button) in buttons {
        let center = button.center();
        for (other_name, other) in buttons {
            assert_eq!(
                other.contains(center),
                other_name == name,
                "the centre of {name} lands in {other_name}"
            );
        }
    }

    let composer_input = Rect::read(
        &snapshot["layout"]["composer"]["input"]["bounds"],
        "composer input",
    );
    let composer = Rect::read(&snapshot["layout"]["composer"]["bounds"], "composer");
    assert!(
        composer.contains(composer_input.center()),
        "the composer input must sit inside the composer"
    );
    let terminal = Rect::read(&snapshot["layout"]["terminal"]["bounds"], "terminal");
    let track = Rect::read(
        &snapshot["layout"]["terminal"]["scrollbar"]["track"],
        "terminal scrollbar track",
    );
    assert_eq!(track.right, terminal.right);
    assert!(!terminal.contains(composer_input.center()));
}

/// perceive -> act -> perceive: read where the active tab's Close button is,
/// change which tab is active, and see the published geometry follow. This is
/// the loop `ui-input` exists to serve; before D-1 nothing in CI could run it.
#[test]
fn selecting_a_tab_moves_the_published_row_actions_to_the_new_active_row() {
    let harness = Harness::start("act");

    // act: grow the tree so there is more than one row to route between.
    let created = harness.cli(&["new-window", "-d", "-P", "-F", "#{window_id}"]);
    assert!(
        created.status.success(),
        "new-window failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let created_id = String::from_utf8(created.stdout)
        .expect("window id UTF-8")
        .trim()
        .to_owned();
    assert!(created_id.starts_with('@'), "{created_id}");

    let before = harness.snapshot();
    let rows = before["tabs"]
        .as_array()
        .expect("tabs array")
        .iter()
        .filter(|tab| tab["visible"] == true)
        .map(|tab| {
            (
                tab["id"].as_str().expect("tab id").to_owned(),
                Rect::read(&tab["bounds"], "tab row"),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        rows.len() >= 2,
        "expected at least two visible rows: {rows:?}"
    );
    // Rows must not overlap: a pointer at the centre of one must reach exactly
    // that one.
    for (index, (id, row)) in rows.iter().enumerate() {
        for (other_id, other) in &rows {
            assert_eq!(
                other.contains(row.center()),
                other_id == id,
                "the centre of {id} lands in {other_id}"
            );
        }
        if let Some((_, next)) = rows.get(index + 1) {
            assert!(row.bottom <= next.top, "rows {row:?} and {next:?} overlap");
        }
    }

    // The active row is the only one that draws Add/Close, and its Close bounds
    // must sit inside the row they belong to -- that is what makes them
    // aimable.
    let active_id = before["active_tab_id"].as_str().expect("active tab id");
    for tab in before["tabs"].as_array().expect("tabs array") {
        let id = tab["id"].as_str().expect("tab id");
        if id == active_id {
            let row = Rect::read(&tab["bounds"], "active row");
            let close = Rect::read(&tab["actions"]["close"]["bounds"], "close action");
            let add = Rect::read(&tab["actions"]["new_child"]["bounds"], "add action");
            assert_eq!(tab["actions"]["close"]["id"], "close-tab");
            assert_eq!(tab["actions"]["new_child"]["id"], "new-child");
            assert!(row.contains(close.center()), "Close escapes its row");
            assert!(row.contains(add.center()), "Add escapes its row");
            assert!(!close.contains(add.center()), "Add and Close overlap");
        } else {
            assert!(
                tab["actions"].is_null(),
                "{id} is not active but advertises actions"
            );
        }
    }

    // act
    let other_id = before["tabs"]
        .as_array()
        .expect("tabs array")
        .iter()
        .map(|tab| tab["id"].as_str().expect("tab id"))
        .find(|id| *id != active_id)
        .expect("a second tab")
        .to_owned();
    let selected = harness.cli(&["select-window", "-t", &other_id]);
    assert!(
        selected.status.success(),
        "select-window failed: {}",
        String::from_utf8_lossy(&selected.stderr)
    );

    // perceive again: the geometry moved with the state, so the loop closes.
    let after = harness.snapshot();
    assert_eq!(after["active_tab_id"], Value::String(other_id.clone()));
    assert!(
        tab_by_id(&after, active_id)["actions"].is_null(),
        "the previously active row still advertises actions"
    );
    let moved = &tab_by_id(&after, &other_id)["actions"]["close"]["bounds"];
    let moved = Rect::read(moved, "close action after select");
    let newly_active_row = Rect::read(&tab_by_id(&after, &other_id)["bounds"], "new active row");
    assert!(newly_active_row.contains(moved.center()));
    let previous_close = Rect::read(
        &tab_by_id(&before, active_id)["actions"]["close"]["bounds"],
        "close action before select",
    );
    assert_ne!(
        moved, previous_close,
        "Close did not move when the active row changed"
    );
    // Rows never move sideways, only down the tree, so the horizontal span is
    // the invariant and the vertical offset is the observable change.
    assert_eq!(moved.left, previous_close.left);
    assert_ne!(moved.top, previous_close.top);
}

/// The boundary, stated honestly: geometry is published, dispatch is not.
///
/// A headless server has no window to synthesize a `PixelWindowEvent` into, and
/// the one hard rule of `ui-input` is that it must never grow a second
/// hit-test. So the refusal has to name the missing thing and stay retryable --
/// attaching a GUI is exactly what fixes it -- rather than claim the command is
/// unimplemented.
#[test]
fn ui_input_refuses_headless_dispatch_by_naming_the_missing_client() {
    let harness = Harness::start("refuse");
    let snapshot = harness.snapshot();
    let active_id = snapshot["active_tab_id"].as_str().expect("active tab id");
    let close = Rect::read(
        &tab_by_id(&snapshot, active_id)["actions"]["close"]["bounds"],
        "close action",
    );
    let (x, y) = close.center();

    let output = harness.cli(&[
        "ui-input",
        "pointer",
        "--x",
        &x.to_string(),
        "--y",
        &y.to_string(),
    ]);
    assert!(
        !output.status.success(),
        "headless ui-input must not silently succeed"
    );
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        message.contains("GUI client"),
        "the refusal must name the missing GUI client: {message}"
    );
    assert!(
        !message.contains("does not implement"),
        "ui-input is implemented; it needs a window, and saying otherwise sends \
         an agent looking for a feature that exists: {message}"
    );
}
