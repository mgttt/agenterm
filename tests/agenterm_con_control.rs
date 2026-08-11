#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

static UNIQUE: AtomicU64 = AtomicU64::new(1);

struct OwnedGui {
    child: Child,
    screenshot: PathBuf,
}

impl Drop for OwnedGui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.screenshot);
    }
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must follow Unix epoch")
        .as_nanos();
    format!(
        "{}-{nanos}-{}",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    )
}

fn invoke(exe: &Path, endpoint: &str, arguments: &[&str]) -> Output {
    let mut command = Command::new(exe);
    command.args(["cli", "--control", endpoint]);
    command.args(arguments);
    command.output().expect("agenterm-con CLI must start")
}

fn output_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn error_text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn cli_json(exe: &Path, endpoint: &str, arguments: &[&str]) -> Value {
    let output = invoke(exe, endpoint, arguments);
    assert!(
        output.status.success(),
        "CLI failed: {}",
        error_text(&output)
    );
    serde_json::from_str(&output_text(&output)).expect("successful CLI output must be JSON")
}

fn cli_text(exe: &Path, endpoint: &str, arguments: &[&str]) -> String {
    let output = invoke(exe, endpoint, arguments);
    assert!(
        output.status.success(),
        "CLI failed: {}",
        error_text(&output)
    );
    output_text(&output)
}

fn wait_until_ready(exe: &Path, endpoint: &str, timeout: Duration) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let output = invoke(exe, endpoint, &["list-tabs"]);
        if output.status.success() {
            return serde_json::from_str(&output_text(&output))
                .expect("list-tabs output must be JSON");
        }
        assert!(
            Instant::now() < deadline,
            "control endpoint did not become ready: {}",
            error_text(&output)
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn tab_id(value: &Value) -> &str {
    value.as_str().expect("tab ID must be a string")
}

#[test]
fn gui_control_surface_isolated_multitab_black_box() {
    let exe = Path::new(env!("CARGO_BIN_EXE_agenterm-con"));
    let suffix = unique_suffix();
    let endpoint = format!(r"pipe:\\.\pipe\agenterm-con-test-{suffix}");
    let screenshot = std::env::temp_dir().join(format!("agenterm-con-{suffix}.png"));
    let child = Command::new(exe)
        .args([
            "--no-activate",
            "--control",
            &endpoint,
            "-e",
            "cmd.exe",
            "/Q",
            "/K",
            "echo ROOT_READY",
        ])
        .spawn()
        .expect("agenterm-con GUI must start");
    let mut gui = OwnedGui { child, screenshot };

    let listed = wait_until_ready(exe, &endpoint, Duration::from_secs(15));
    let root = tab_id(&listed["tabs"][0]["id"]).to_owned();
    assert_eq!(listed["tabs"][0]["active"], true);
    cli_json(exe, &endpoint, &["reset-perf-stats"]);

    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &root,
            "--timeout-ms",
            "10000",
            "ROOT_READY",
        ],
    );
    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &root, "echo ROOT_ONLY\r"],
    );
    cli_json(
        exe,
        &endpoint,
        &["wait-text", "--target", &root, "ROOT_ONLY"],
    );

    let created = cli_json(exe, &endpoint, &["new-tab", "--parent", &root]);
    let child_id = tab_id(&created["id"]).to_owned();
    assert_eq!(created["parent"], root);
    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &child_id, "echo KEY_EVENT"],
    );
    cli_json(
        exe,
        &endpoint,
        &["send-keys", "--target", &child_id, "Enter"],
    );
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &child_id,
            "--timeout-ms",
            "10000",
            "KEY_EVENT",
        ],
    );
    cli_json(
        exe,
        &endpoint,
        &[
            "send-text",
            "--target",
            &child_id,
            "for /L %i in (1,1,2000) do @echo LOAD_%i & echo LOAD_DONE\r",
        ],
    );
    cli_json(exe, &endpoint, &["select-tab", "--target", &root]);
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &child_id,
            "--timeout-ms",
            "15000",
            "LOAD_DONE",
        ],
    );
    let perf = cli_json(exe, &endpoint, &["perf-stats"]);
    assert!(perf["frames"].as_u64().is_some_and(|frames| frames > 0));
    assert!(
        perf["pty_drained_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
    );

    let root_text = cli_text(exe, &endpoint, &["capture-pane", "--target", &root]);
    let child_text = cli_text(exe, &endpoint, &["capture-pane", "--target", &child_id]);
    assert!(root_text.contains("ROOT_ONLY"));
    assert!(!root_text.contains("LOAD_DONE"));
    assert!(child_text.contains("LOAD_DONE"));

    cli_json(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "move",
            "--button",
            "none",
            "--column",
            "1",
            "--row",
            "1",
        ],
    );
    cli_json(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "click",
            "--button",
            "left",
            "--column",
            "1",
            "--row",
            "1",
        ],
    );
    cli_json(
        exe,
        &endpoint,
        &[
            "send-wheel",
            "--target",
            &child_id,
            "--column",
            "1",
            "--row",
            "1",
            "--notches",
            "1",
        ],
    );

    let screenshot_text = gui.screenshot.to_string_lossy().into_owned();
    cli_json(
        exe,
        &endpoint,
        &[
            "screenshot-pane",
            "--target",
            &child_id,
            "--output",
            &screenshot_text,
        ],
    );
    let png = fs::read(&gui.screenshot).expect("screenshot must exist after successful reply");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

    let timed_out = invoke(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &child_id,
            "--timeout-ms",
            "25",
            "IMPOSSIBLE_TEST_MARKER",
        ],
    );
    assert!(!timed_out.status.success(), "missing text must time out");
    let invalid = invoke(exe, &endpoint, &["capture-pane", "--target", "@999999"]);
    assert!(!invalid.status.success(), "unknown tab must fail");
    cli_json(exe, &endpoint, &["list-tabs"]);

    cli_json(exe, &endpoint, &["close-tab", "--target", &root]);
    let after_close = cli_json(exe, &endpoint, &["list-tabs"]);
    assert_eq!(after_close["tabs"].as_array().map(Vec::len), Some(1));
    assert_eq!(after_close["tabs"][0]["id"], child_id);
    assert!(after_close["tabs"][0]["parent"].is_null());

    gui.child
        .kill()
        .expect("GUI must remain alive through the journey");
    gui.child.wait().expect("GUI process must be reapable");
}
