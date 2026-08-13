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

#[cfg(windows)]
struct ClipboardRestore(Option<String>);

#[cfg(windows)]
impl Drop for ClipboardRestore {
    fn drop(&mut self) {
        let _ = agenterm_platform::clipboard::set_text(self.0.as_deref().unwrap_or_default());
    }
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must follow Unix epoch")
        .as_nanos();
    format!(
        "{}-{}-{}",
        std::process::id() % 100_000,
        nanos % 1_000_000_000,
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    )
}

fn control_endpoint(suffix: &str) -> String {
    if cfg!(windows) {
        format!(r"pipe:\\.\pipe\agenterm-con-test-{suffix}")
    } else {
        let base = agenterm_platform::ipc::native_runtime_directory();
        let _ = fs::create_dir_all(&base);
        let dir = base.join(format!("ct-{suffix}"));
        let _ = fs::create_dir_all(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        }
        let path = dir.join("c.sock");
        assert!(
            path.to_string_lossy().len() <= 103,
            "Unix control socket path too long: {}",
            path.display()
        );
        format!("unix:{}", path.to_string_lossy())
    }
}

fn host_shell_args() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["cmd.exe", "/Q", "/K"]
    } else {
        vec!["/bin/bash", "--norc", "--noprofile"]
    }
}

fn shell_flood_command(prefix: &str) -> String {
    if cfg!(windows) {
        format!("for /L %i in (1,1,1000000) do @echo {prefix}%i\r")
    } else {
        format!("for i in $(seq 1 200000); do echo {prefix}$i; done\r")
    }
}

fn shell_load_done_command() -> &'static str {
    if cfg!(windows) {
        "for /L %i in (1,1,2000) do @echo LOAD_%i & echo LOAD_DONE\r"
    } else {
        "for i in $(seq 1 2000); do echo LOAD_$i; done; echo LOAD_DONE\r"
    }
}

fn shell_vt_noise_command() -> &'static str {
    if cfg!(windows) {
        "for /F \"delims=\" %e in ('echo prompt $E^| cmd') do @set \"ESC=%e\"\r"
    } else {
        // Unix path injects CSI noise in one shot below.
        "true\r"
    }
}

fn shell_vt_noise_body() -> &'static str {
    if cfg!(windows) {
        "for /L %i in (1,1,1200) do @echo %ESC%[999999999999999999;999999999999999999;999999999999999999mVT_NOISE_%i%ESC%[0m\r"
    } else {
        "for i in $(seq 1 1200); do printf '\\033[999mVT_NOISE_%s\\033[0m\\n' \"$i\"; done\r"
    }
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

/// Resolve `agenterm-con` next to the test executable.
///
/// `CARGO_BIN_EXE_agenterm-con` only exists for bins declared in the *same*
/// package, and `agenterm-con` moved to its own workspace package -- the
/// compile error that produces is what took linux-x86_64 red at the all-target
/// Clippy gate. An integration test runs from `target/<profile>/deps/`, so the
/// binary sits one directory up. Same resolution as `tests/agenterm_con_blackbox.rs`.
fn agenterm_con_binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable path");
    path.pop();
    path.pop();
    path.push(format!("agenterm-con{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "agenterm-con is missing at {}; build it with \
         `cargo build -p agenterm-con --bin agenterm-con`. It left this package \
         in the lightweight-package split, so a bare `cargo test -p agenterm` no \
         longer builds it as a side effect.",
        path.display()
    );
    path
}

#[test]
fn gui_control_surface_isolated_multitab_black_box() {
    let exe = agenterm_con_binary();
    let exe = exe.as_path();
    let suffix = unique_suffix();
    let endpoint = control_endpoint(&suffix);
    let screenshot = if cfg!(windows) {
        std::env::temp_dir().join(format!("agenterm-con-{suffix}.png"))
    } else {
        agenterm_platform::ipc::native_runtime_directory().join(format!("shot-{suffix}.png"))
    };
    let mut host = Command::new(exe);
    host.arg("--no-activate")
        .arg("--control")
        .arg(&endpoint)
        .arg("-e");
    for arg in host_shell_args() {
        host.arg(arg);
    }
    // Launch with an interactive shell; inject ROOT_READY after control is up.
    let child = host.spawn().expect("agenterm-con GUI must start");
    let mut gui = OwnedGui { child, screenshot };

    let listed = wait_until_ready(exe, &endpoint, Duration::from_secs(15));
    let root = tab_id(&listed["tabs"][0]["id"]).to_owned();
    assert_eq!(listed["tabs"][0]["active"], true);
    cli_json(exe, &endpoint, &["reset-perf-stats"]);

    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &root, "echo ROOT_READY\r"],
    );
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

    let enabled_ime = cli_json(exe, &endpoint, &["send-ui-ime", "enabled"]);
    assert_eq!(enabled_ime["route"], "terminal");
    assert_eq!(enabled_ime["action"], "enabled");
    cli_json(
        exe,
        &endpoint,
        &["send-ui-ime", "preedit", "nihao", "--cursor", "5"],
    );
    let terminal_preedit = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(terminal_preedit["terminal_ime_preedit"], "nihao");
    assert_eq!(terminal_preedit["composer_preedit"], "");
    let ime_status = &terminal_preedit["ime_status"];
    assert!(ime_status["known"].is_boolean());
    assert!(ime_status["name"].is_string());
    assert!(ime_status["available"].is_boolean());
    assert!(ime_status["open"].is_boolean());
    assert!(ime_status["native_mode"].is_boolean());
    assert!(ime_status["full_shape"].is_boolean());
    assert!(
        ime_status["label"]
            .as_str()
            .is_some_and(|label| label.starts_with("IME:"))
    );
    let terminal_commit = cli_json(
        exe,
        &endpoint,
        &["send-ui-ime", "commit", "echo 你好_IME_OK\r"],
    );
    assert_eq!(terminal_commit["route"], "terminal");
    assert_eq!(terminal_commit["action"], "commit");
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &root,
            "--timeout-ms",
            "10000",
            "你好_IME_OK",
        ],
    );
    assert_eq!(
        cli_json(exe, &endpoint, &["ui-snapshot"])["terminal_ime_preedit"],
        ""
    );
    cli_json(exe, &endpoint, &["send-ui-ime", "disabled"]);

    cli_json(exe, &endpoint, &["send-ui-keys", "Ctrl+Shift+I"]);
    let composer_preedit = cli_json(exe, &endpoint, &["send-ui-ime", "preedit", "zhongwen"]);
    assert_eq!(composer_preedit["route"], "composer");
    let focused_preedit = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(focused_preedit["composer_preedit"], "zhongwen");
    assert_eq!(focused_preedit["terminal_ime_preedit"], "");
    cli_json(exe, &endpoint, &["send-ui-ime", "commit", "中文"]);
    let focused_commit = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(focused_commit["composer_text"], "中文");
    assert_eq!(focused_commit["composer_preedit"], "");
    cli_json(
        exe,
        &endpoint,
        &["send-ui-keys", "Ctrl+A", "Backspace", "Escape"],
    );

    #[cfg(windows)]
    {
        let _restore = ClipboardRestore(agenterm_platform::clipboard::get_text(1024 * 1024).ok());
        agenterm_platform::clipboard::set_text("echo ASYNC_CLIPBOARD_OK\r")
            .expect("test clipboard text must be published");
        cli_json(exe, &endpoint, &["send-ui-keys", "Ctrl+Shift+V"]);
        cli_json(
            exe,
            &endpoint,
            &[
                "wait-text",
                "--target",
                &root,
                "--timeout-ms",
                "10000",
                "ASYNC_CLIPBOARD_OK",
            ],
        );
        let snapshot = cli_json(exe, &endpoint, &["ui-snapshot"]);
        assert_eq!(snapshot["terminal_clipboard_paste"]["state"], "idle");
        assert_eq!(
            snapshot["terminal_clipboard_paste"]["target"],
            serde_json::Value::Null
        );
        assert_eq!(
            snapshot["terminal_clipboard_paste"]["error"],
            serde_json::Value::Null
        );
    }

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
            shell_load_done_command(),
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
    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &child_id, shell_vt_noise_command()],
    );
    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &child_id, shell_vt_noise_body()],
    );
    thread::scope(|scope| {
        let mut requests = Vec::new();
        let endpoint = endpoint.as_str();
        let child_target = child_id.as_str();
        for index in 0..24 {
            requests.push(scope.spawn(move || match index % 3 {
                0 => invoke(exe, endpoint, &["list-tabs"]),
                1 => invoke(exe, endpoint, &["perf-stats"]),
                _ => invoke(
                    exe,
                    endpoint,
                    &[
                        "capture-pane",
                        "--target",
                        child_target,
                        "--max-bytes",
                        "1048576",
                    ],
                ),
            }));
        }
        for request in requests {
            let output = request.join().expect("control request thread must join");
            assert!(
                output.status.success(),
                "concurrent control request failed: {}",
                error_text(&output)
            );
        }
    });
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &child_id,
            "--timeout-ms",
            "15000",
            "VT_NOISE_1200",
        ],
    );
    let perf = cli_json(exe, &endpoint, &["perf-stats"]);
    assert!(perf["frames"].as_u64().is_some_and(|frames| frames > 0));
    assert!(
        perf["pty_drained_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0)
    );
    assert!(
        perf["control_requests"]
            .as_u64()
            .is_some_and(|requests| requests >= 24)
    );
    assert!(perf["control_budget_yields"].as_u64().is_some());

    let root_text = cli_text(exe, &endpoint, &["capture-pane", "--target", &root]);
    let child_text = cli_text(exe, &endpoint, &["capture-pane", "--target", &child_id]);
    assert!(root_text.contains("ROOT_ONLY"));
    assert!(!root_text.contains("LOAD_DONE"));
    assert!(child_text.contains("VT_NOISE_1200"));

    let mouse_move_receipt = cli_json(
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
    assert_eq!(mouse_move_receipt["delivered"], true);
    assert_eq!(mouse_move_receipt["route"], "noop");
    assert_eq!(mouse_move_receipt["changed"], false);
    let mouse_click_receipt = cli_json(
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
    assert_eq!(mouse_click_receipt["delivered"], true);
    assert_eq!(mouse_click_receipt["route"], "selection");
    assert_eq!(mouse_click_receipt["changed"], true);
    let wheel_receipt = cli_json(
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
    assert_eq!(wheel_receipt["route"], "scrollback");
    assert_eq!(wheel_receipt["delivered_notches"], 1);
    assert_eq!(wheel_receipt["changed"], true);

    let screenshot_text = gui.screenshot.to_string_lossy().into_owned();
    let screenshot_receipt = cli_json(
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
    assert!(
        screenshot_receipt["encode_ns"]
            .as_u64()
            .is_some_and(|elapsed| elapsed > 0),
        "screenshot receipt must expose positive encoding time: {screenshot_receipt}"
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
    let still_running = invoke(
        exe,
        &endpoint,
        &["wait-tab-exit", "--target", &root, "--timeout-ms", "25"],
    );
    assert!(
        !still_running.status.success(),
        "wait-tab-exit must time out for a live terminal"
    );

    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &child_id, "echo CHILD_FINAL\r"],
    );
    cli_json(
        exe,
        &endpoint,
        &["wait-text", "--target", &child_id, "CHILD_FINAL"],
    );
    cli_json(
        exe,
        &endpoint,
        &["send-text", "--target", &child_id, "exit 7\r"],
    );
    let exited = cli_json(
        exe,
        &endpoint,
        &[
            "wait-tab-exit",
            "--target",
            &child_id,
            "--timeout-ms",
            "10000",
        ],
    );
    assert_eq!(exited["id"], child_id);
    assert_eq!(exited["child_alive"], false);
    assert_eq!(exited["child_exit_code"], 7);
    let child_final = cli_text(exe, &endpoint, &["capture-pane", "--target", &child_id]);
    assert!(child_final.contains("CHILD_FINAL"));
    let selected_exited = cli_json(exe, &endpoint, &["select-tab", "--target", &child_id]);
    assert_eq!(selected_exited["active"], child_id);
    for arguments in [
        vec!["send-text", "--target", &child_id, "late text"],
        vec!["send-paste", "--target", &child_id, "late paste"],
        vec!["send-keys", "--target", &child_id, "A"],
        vec!["send-ui-keys", "A"],
    ] {
        let rejected = invoke(exe, &endpoint, &arguments);
        assert!(
            !rejected.status.success(),
            "input to an exited tab must fail: {}",
            error_text(&rejected)
        );
        assert!(
            error_text(&rejected).contains("terminal process has exited"),
            "exited-tab failure must be explicit: {}",
            error_text(&rejected)
        );
    }

    cli_json(exe, &endpoint, &["send-ui-ime", "preedit", "retry-ime"]);
    let rejected_ime = invoke(exe, &endpoint, &["send-ui-ime", "commit", "unavailable"]);
    assert!(!rejected_ime.status.success());
    assert!(
        error_text(&rejected_ime).contains("terminal process has exited"),
        "exited terminal IME commit must fail explicitly: {}",
        error_text(&rejected_ime)
    );
    assert_eq!(
        cli_json(exe, &endpoint, &["ui-snapshot"])["terminal_ime_preedit"],
        "retry-ime"
    );
    cli_json(exe, &endpoint, &["send-ui-ime", "disabled"]);

    cli_json(exe, &endpoint, &["send-ui-keys", "Ctrl+Shift+I"]);
    cli_json(
        exe,
        &endpoint,
        &["send-ui-keys", "R", "E", "T", "R", "Y", "Enter"],
    );
    let failed_composer = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(failed_composer["composer_focused"], true);
    assert_eq!(failed_composer["composer_text"], "RETRY");
    assert!(
        failed_composer["composer_submit_error"]
            .as_str()
            .is_some_and(|error| error.contains("terminal process has exited")),
        "failed composer submit must be observable without losing text: {failed_composer}"
    );
    cli_json(exe, &endpoint, &["send-ui-keys", "Escape"]);

    let exited_mouse = cli_json(
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
            "0",
            "--row",
            "0",
        ],
    );
    assert_eq!(exited_mouse["delivered"], true);
    assert_eq!(exited_mouse["route"], "selection");
    assert_eq!(exited_mouse["changed"], true);

    let exited_wheel = cli_json(
        exe,
        &endpoint,
        &[
            "send-wheel",
            "--target",
            &child_id,
            "--column",
            "0",
            "--row",
            "0",
            "--notches",
            "1",
        ],
    );
    assert_eq!(exited_wheel["route"], "scrollback");
    assert_eq!(exited_wheel["delivered_notches"], 1);
    assert_eq!(exited_wheel["changed"], true);

    cli_json(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "press",
            "--button",
            "left",
            "--column",
            "0",
            "--row",
            "0",
        ],
    );
    let cancelled_pointer = cli_json(exe, &endpoint, &["cancel-pointer"]);
    assert_eq!(cancelled_pointer["cancelled_owner"], child_id);
    let cancelled_snapshot = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(
        cancelled_snapshot["control_pointer_owner"],
        serde_json::Value::Null
    );
    let idempotent_cancel = cli_json(exe, &endpoint, &["cancel-pointer"]);
    assert_eq!(
        idempotent_cancel["cancelled_owner"],
        serde_json::Value::Null
    );
    let release_after_cancel = invoke(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "release",
            "--button",
            "left",
            "--column",
            "0",
            "--row",
            "0",
        ],
    );
    assert!(!release_after_cancel.status.success());
    assert!(error_text(&release_after_cancel).contains("no matching control pointer press"));

    let held_pointer = cli_json(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "press",
            "--button",
            "left",
            "--column",
            "0",
            "--row",
            "0",
        ],
    );
    assert_eq!(held_pointer["route"], "selection");
    let held_snapshot = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(held_snapshot["control_pointer_owner"], child_id);
    cli_json(exe, &endpoint, &["select-tab", "--target", &root]);
    let switched_snapshot = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(
        switched_snapshot["control_pointer_owner"],
        serde_json::Value::Null
    );
    let stale_release = invoke(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "release",
            "--button",
            "left",
            "--column",
            "0",
            "--row",
            "0",
        ],
    );
    assert!(!stale_release.status.success());
    assert!(
        error_text(&stale_release).contains("no matching control pointer press"),
        "tab activation must cancel the old control gesture: {}",
        error_text(&stale_release)
    );

    cli_json(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "press",
            "--button",
            "left",
            "--column",
            "0",
            "--row",
            "0",
        ],
    );
    let replacement = cli_json(exe, &endpoint, &["new-tab"]);
    let replacement_id = replacement["id"].as_str().unwrap().to_owned();
    let stale_background_release = invoke(
        exe,
        &endpoint,
        &[
            "send-mouse",
            "--target",
            &child_id,
            "--action",
            "release",
            "--button",
            "left",
            "--column",
            "0",
            "--row",
            "0",
        ],
    );
    assert!(!stale_background_release.status.success());
    assert!(
        error_text(&stale_background_release).contains("no matching control pointer press"),
        "new-tab must cancel a background control gesture: {}",
        error_text(&stale_background_release)
    );
    cli_json(exe, &endpoint, &["close-tab", "--target", &replacement_id]);

    cli_json(
        exe,
        &endpoint,
        &[
            "send-text",
            "--target",
            &root,
            "echo ROOT_AFTER_CHILD_EXIT\r",
        ],
    );
    cli_json(
        exe,
        &endpoint,
        &[
            "wait-text",
            "--target",
            &root,
            "--timeout-ms",
            "10000",
            "ROOT_AFTER_CHILD_EXIT",
        ],
    );
    let listed_after_exit = cli_json(exe, &endpoint, &["list-tabs"]);
    let root_state = listed_after_exit["tabs"]
        .as_array()
        .and_then(|tabs| tabs.iter().find(|tab| tab["id"] == root))
        .expect("root tab remains listed");
    let child_state = listed_after_exit["tabs"]
        .as_array()
        .and_then(|tabs| tabs.iter().find(|tab| tab["id"] == child_id))
        .expect("exited child tab remains listed");
    assert_eq!(root_state["child_alive"], true);
    assert_eq!(child_state["child_alive"], false);
    assert_eq!(child_state["child_exit_code"], 7);

    let mut flood_ids = Vec::new();
    for index in 0..4 {
        let flood = cli_json(exe, &endpoint, &["new-tab", "--parent", &root]);
        let flood_id = tab_id(&flood["id"]).to_owned();
        let ready = format!("QUEUE_CLOSE_START_{index}");
        let ready_command = format!("echo {ready}\r");
        cli_json(
            exe,
            &endpoint,
            &["send-text", "--target", &flood_id, &ready_command],
        );
        cli_json(
            exe,
            &endpoint,
            &[
                "wait-text",
                "--target",
                &flood_id,
                "--timeout-ms",
                "10000",
                &ready,
            ],
        );
        let fill = format!("QUEUE_FILL_{index}_");
        let fill_command = shell_flood_command(&fill);
        cli_json(
            exe,
            &endpoint,
            &["send-text", "--target", &flood_id, &fill_command],
        );
        cli_json(
            exe,
            &endpoint,
            &[
                "wait-text",
                "--target",
                &flood_id,
                "--timeout-ms",
                "10000",
                &fill,
            ],
        );
        flood_ids.push(flood_id);
    }
    let active_before_shots = cli_json(exe, &endpoint, &["ui-snapshot"])["active"].clone();
    let mut screenshot_jobs = Vec::new();
    for (index, flood_id) in flood_ids.iter().enumerate() {
        let path =
            std::env::temp_dir().join(format!("agenterm-con-{suffix}-concurrent-shot-{index}.png"));
        let child = Command::new(exe)
            .args([
                "cli",
                "--control",
                &endpoint,
                "screenshot-pane",
                "--target",
                flood_id,
                "--output",
                path.to_str().expect("screenshot path is Unicode"),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("concurrent screenshot CLI must start");
        screenshot_jobs.push((child, path));
    }
    let screenshot_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut running = false;
        for (child, _) in &mut screenshot_jobs {
            running |= child
                .try_wait()
                .expect("poll concurrent screenshot CLI")
                .is_none();
        }
        if !running {
            break;
        }
        assert!(
            Instant::now() < screenshot_deadline,
            "concurrent screenshot requests did not finish within their bounded deadline"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let mut screenshot_successes = 0;
    let mut screenshot_busy = 0;
    for (child, path) in screenshot_jobs {
        let output = child
            .wait_with_output()
            .expect("concurrent screenshot CLI must be reapable");
        if output.status.success() {
            screenshot_successes += 1;
            let bytes = fs::read(&path).expect("successful screenshot publishes its PNG");
            assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("a screenshot is already pending"),
                "concurrent screenshot failed without typed busy result: {error}"
            );
            screenshot_busy += 1;
        }
        let _ = fs::remove_file(path);
    }
    assert_eq!(screenshot_successes, 1);
    assert_eq!(screenshot_busy, flood_ids.len() - 1);
    let after_concurrent_shots = cli_json(exe, &endpoint, &["ui-snapshot"]);
    assert_eq!(after_concurrent_shots["pending_control_screenshots"], 0);
    assert_eq!(after_concurrent_shots["active"], active_before_shots);

    let raced_path = std::env::temp_dir().join(format!("agenterm-con-{suffix}-raced-shot.png"));
    let raced_path_text = raced_path
        .to_str()
        .expect("raced screenshot path is Unicode")
        .to_owned();
    cli_json(exe, &endpoint, &["reset-perf-stats"]);
    let mut raced_shot = Command::new(exe)
        .args([
            "cli",
            "--control",
            &endpoint,
            "screenshot-pane",
            "--target",
            &flood_ids[0],
            "--output",
            &raced_path_text,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("raced screenshot CLI must start");
    cli_json(exe, &endpoint, &["select-tab", "--target", &root]);
    let raced_deadline = Instant::now() + Duration::from_secs(10);
    while raced_shot
        .try_wait()
        .expect("poll raced screenshot CLI")
        .is_none()
    {
        assert!(
            Instant::now() < raced_deadline,
            "screenshot racing active-tab selection did not complete"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let raced_output = raced_shot
        .wait_with_output()
        .expect("raced screenshot CLI must be reapable");
    assert!(
        raced_output.status.success(),
        "raced screenshot failed: {}",
        error_text(&raced_output)
    );
    let raced_bytes = fs::read(&raced_path).expect("raced screenshot publishes its PNG");
    assert!(raced_bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    let _ = fs::remove_file(raced_path);
    assert_eq!(cli_json(exe, &endpoint, &["ui-snapshot"])["active"], root);
    assert_eq!(
        cli_json(exe, &endpoint, &["perf-stats"])["discarded_capture_frames"],
        1
    );
    for (index, flood_id) in flood_ids.iter().enumerate() {
        let mut close_flood = Command::new(exe)
            .args([
                "cli",
                "--control",
                &endpoint,
                "close-tab",
                "--target",
                flood_id,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("flooded tab close CLI must start");
        let close_deadline = Instant::now() + Duration::from_secs(3);
        while close_flood
            .try_wait()
            .expect("poll flooded tab close")
            .is_none()
        {
            assert!(
                Instant::now() < close_deadline,
                "closing flooded tab {index} exceeded the bounded shutdown deadline"
            );
            thread::sleep(Duration::from_millis(20));
        }
        let close_output = close_flood
            .wait_with_output()
            .expect("flooded tab close CLI must be reapable");
        assert!(
            close_output.status.success(),
            "flooded tab {index} close failed: {}",
            error_text(&close_output)
        );
        let listed = cli_json(exe, &endpoint, &["list-tabs"]);
        assert!(
            listed["tabs"]
                .as_array()
                .is_some_and(|tabs| tabs.iter().all(|tab| tab["id"] != *flood_id)),
            "closed flooded tab remained in tree: {listed}"
        );
        let root_marker = format!("ROOT_AFTER_FLOOD_CLOSE_{index}");
        let root_command = format!("echo {root_marker}\r");
        cli_json(
            exe,
            &endpoint,
            &["send-text", "--target", &root, &root_command],
        );
        cli_json(
            exe,
            &endpoint,
            &[
                "wait-text",
                "--target",
                &root,
                "--timeout-ms",
                "10000",
                &root_marker,
            ],
        );
    }

    cli_json(exe, &endpoint, &["close-tab", "--target", &root]);
    let after_close = cli_json(exe, &endpoint, &["list-tabs"]);
    assert_eq!(after_close["tabs"].as_array().map(Vec::len), Some(1));
    assert_eq!(after_close["tabs"][0]["id"], child_id);
    assert!(after_close["tabs"][0]["parent"].is_null());
    let mut pending_wait = Command::new(exe)
        .args([
            "cli",
            "--control",
            &endpoint,
            "wait-text",
            "--target",
            &child_id,
            "--timeout-ms",
            "10000",
            "NEVER_MATCH_CLOSED_TAB",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("pending wait CLI must start");
    let pending_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = cli_json(exe, &endpoint, &["ui-snapshot"]);
        if state["pending_control_waits"].as_u64() == Some(1) {
            assert_eq!(state["pending_control_screenshots"], 0);
            break;
        }
        assert!(
            Instant::now() < pending_deadline,
            "wait-text was not registered before tab close: {state}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    cli_json(exe, &endpoint, &["close-tab", "--target", &child_id]);
    let cancel_deadline = Instant::now() + Duration::from_secs(3);
    while pending_wait
        .try_wait()
        .expect("poll cancelled wait CLI")
        .is_none()
    {
        assert!(
            Instant::now() < cancel_deadline,
            "closing a tab did not cancel its pending wait"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let cancelled = pending_wait
        .wait_with_output()
        .expect("cancelled wait CLI must be reapable");
    assert!(!cancelled.status.success());
    let cancel_error = String::from_utf8_lossy(&cancelled.stderr);
    assert!(
        cancel_error.contains(&format!("terminal {child_id} closed")),
        "pending wait did not receive a typed close error: {cancel_error}"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = gui.child.try_wait().expect("poll explicitly closed GUI") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "closing the final retained tab did not exit the GUI"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "explicit close failed with {status:?}");
}
