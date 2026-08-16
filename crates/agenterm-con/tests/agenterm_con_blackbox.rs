//! Black-box integration tests for `agenterm-con`, run against the real
//! compiled binary — not the `#[cfg(test)]` unit tests inside the binary
//! itself, which exercise pure functions in isolation and cannot prove the
//! *wiring* between a real window/PTY session and those functions is
//! correct. That distinction mattered concretely this session: the
//! `fill_rect` bug (background fills, underline, and the cursor all painting
//! at column 0) passed every unit test that called `paint_cells` directly
//! with hand-built inputs, and was only caught by a test that rendered into
//! an actual pixel buffer and checked actual pixel colors. This file is the
//! same idea one layer up — spawn the real process, drive it, check what it
//! actually produced.
//!
//! The public control CLI drives the real session while `--emit-snapshot`
//! supplies structured observation. Journey JSON exists only in this test
//! harness and is translated into ordinary `agenterm-con cli` invocations;
//! the product binary intentionally has no script runtime.
//!
//! Ordinary CI remains no-activate and cannot prove a desktop IME. The ignored
//! Windows native-IME acceptance test deliberately takes foreground focus and
//! uses physical `SendInput` virtual keys; it never substitutes Unicode input
//! or synthetic `WM_IME_*` messages for the real input-method path.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
type TestHwnd = *mut std::ffi::c_void;
#[cfg(windows)]
#[repr(C)]
struct TestRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[cfg(windows)]
const WM_ENTERSIZEMOVE: u32 = 0x0231;
#[cfg(windows)]
const WM_EXITSIZEMOVE: u32 = 0x0232;
#[cfg(windows)]
const WM_LBUTTONDOWN: u32 = 0x0201;
#[cfg(windows)]
const WM_LBUTTONUP: u32 = 0x0202;
#[cfg(windows)]
const WM_INPUTLANGCHANGEREQUEST: u32 = 0x0050;
#[cfg(windows)]
const WM_IME_CONTROL: u32 = 0x0283;
#[cfg(windows)]
const IMC_SETCONVERSIONMODE: usize = 0x0002;
#[cfg(windows)]
const IMC_SETOPENSTATUS: usize = 0x0006;
#[cfg(windows)]
const IME_CMODE_NATIVE: isize = 0x0001;
#[cfg(windows)]
const KLF_ACTIVATE: u32 = 0x0000_0001;

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(
        callback: Option<unsafe extern "system" fn(TestHwnd, isize) -> i32>,
        lparam: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(hwnd: TestHwnd, process_id: *mut u32) -> u32;
    fn GetWindowRect(hwnd: TestHwnd, rect: *mut TestRect) -> i32;
    fn GetForegroundWindow() -> TestHwnd;
    fn SetForegroundWindow(hwnd: TestHwnd) -> i32;
    fn LoadKeyboardLayoutW(layout_id: *const u16, flags: u32) -> isize;
    fn SendMessageW(hwnd: TestHwnd, message: u32, wparam: usize, lparam: isize) -> isize;
}

#[cfg(windows)]
#[link(name = "imm32")]
unsafe extern "system" {
    fn ImmGetDefaultIMEWnd(hwnd: TestHwnd) -> TestHwnd;
}

const TEST_JOURNEY_ARG: &str = "--test-control-journey";

fn binary() -> &'static str {
    // `agenterm-con` is its own workspace package now, so
    // `CARGO_BIN_EXE_agenterm-con` is no longer defined when compiling this
    // package's tests -- that variable only covers bins declared in the *same*
    // package, and the compile error it produces took linux-x86_64 red at the
    // all-target Clippy gate. Resolve the path at run time instead: an
    // integration test executes from `target/<profile>/deps/`, so the sibling
    // binary is one directory up.
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let mut path = std::env::current_exe().expect("test executable path");
        path.pop();
        path.pop();
        path.push(format!("agenterm-con{}", std::env::consts::EXE_SUFFIX));
        assert!(
            path.is_file(),
            "agenterm-con is missing at {}; build it with \
             `cargo build -p agenterm-con --bin agenterm-con`. It left this \
             package in the lightweight-package split, so a bare \
             `cargo test -p agenterm` no longer builds it as a side effect.",
            path.display()
        );
        path.to_string_lossy().into_owned()
    })
    .as_str()
}

/// Real GUI/PTY-spawning tests in this file get measurably flakier the more
/// of them race at once — observed directly, not hypothetically: adding two
/// more real-TUI tests to this file pushed a previously 100%-green suite
/// (under default `cargo test` parallelism, which spawns every test
/// concurrently) into occasional false failures on window/selection state
/// that pass reliably alone. Rather than pin `--test-threads=1` globally
/// (which would also serialize the fast pure-CLI tests for no reason) or
/// pull in a `serial_test` dependency, every test that spawns a real
/// `ConSession` takes this lock for its whole body. Cheap, dependency-free,
/// and turns "occasionally flaky under load" back into "always correct," at
/// the cost of wall-clock time (these tests now run one at a time instead
/// of racing). `unwrap_or_else` recovers from poisoning rather than letting
/// one test's panic cascade-fail every test queued behind it — a mutex
/// serializing OS resource contention has nothing to do with the poisoned
/// test's own correctness.
static GUI_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gui_test_guard() -> std::sync::MutexGuard<'static, ()> {
    GUI_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn shell_program() -> String {
    if cfg!(windows) {
        return "cmd.exe".to_owned();
    }
    std::env::var("SHELL")
        .ok()
        .filter(|shell| Path::new(shell).is_file())
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

fn command_shell_args(command: &str) -> Vec<String> {
    vec![
        "-e".to_owned(),
        shell_program(),
        if cfg!(windows) { "/c" } else { "-c" }.to_owned(),
        command.to_owned(),
    ]
}

fn interactive_shell_args(journey: &Path) -> Vec<String> {
    let mut args = vec![
        TEST_JOURNEY_ARG.to_owned(),
        journey.to_string_lossy().into_owned(),
        "-e".to_owned(),
    ];
    if cfg!(windows) {
        // Keep Windows journeys on cmd.exe so KEY_EVENT / cooked-line tests
        // match the original ConPTY evidence.
        args.push("cmd.exe".to_owned());
        args.push("/k".to_owned());
    } else {
        // Clean bash without rc/profile: macOS login shells inject MOTD and a
        // multi-segment prompt that shifts rows and confuses geometry-sensitive
        // journeys (selection rows, cursor-column deltas). Windows cmd starts
        // empty; match that posture for UX parity of black-box evidence.
        args.push("/bin/bash".to_owned());
        args.push("--norc".to_owned());
        args.push("--noprofile".to_owned());
    }
    args
}

/// Locates a real `less.exe` (bundled with Git for Windows) if one is
/// installed, for tests that need a genuine raw-mode/curses-style TUI
/// rather than a cooked-mode shell — closing the gap plan-v0.1.16.md §C
/// flagged: "no test against a real TUI exists because no dependency was
/// found that installs reliably on this machine." `less` turns out to
/// already be exactly that dependency: Git for Windows ships it, and Git
/// for Windows is a near-universal dev-machine prerequisite (this repo's
/// own tooling assumes Git). Not on `PATH` for a plain `CreateProcess`
/// spawn the way it is for this Bash tool's shell, so this checks known
/// install locations directly and returns `None` (letting the caller skip)
/// rather than failing outright on a machine that genuinely lacks it —
/// this is a real environment dependency, not a bug to hard-fail on.
fn find_less_exe() -> Option<PathBuf> {
    #[cfg(not(windows))]
    {
        for candidate in ["/usr/bin/less", "/bin/less", "/usr/local/bin/less"] {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
        None
    }

    #[cfg(windows)]
    {
        let mut candidates = vec![
            PathBuf::from(r"C:\Program Files\Git\usr\bin\less.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Git\usr\bin\less.exe"),
        ];
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Ok(base) = std::env::var(var) {
                candidates.push(PathBuf::from(base).join(r"Git\usr\bin\less.exe"));
            }
        }
        candidates.into_iter().find(|path| path.is_file())
    }
}

/// Writes a fixture file of `count` numbered, greppable lines — enough to
/// force any reasonably-sized terminal window into needing to scroll.
fn write_numbered_lines(dir: &Path, prefix: &str, count: usize) -> PathBuf {
    let path = dir.join("lines.txt");
    let mut content = String::new();
    for n in 1..=count {
        content.push_str(&format!("{prefix}{n}\n"));
    }
    std::fs::write(&path, content).expect("write fixture lines");
    path
}

/// Compact FNV-1a so Unix control socket paths stay under `sockaddr_un`
/// limits (macOS sun_path is only ~104 bytes).
fn short_token(label: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in label.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// A unique scratch directory per test, so parallel `cargo test` runs never
/// collide on the same script/snapshot file.
///
/// Unix places scratch under the platform IPC runtime directory (short real
/// path such as `/private/tmp/agenterm-platform-<uid>`). macOS process temps
/// under `/var/folders/...` are too long for Unix control sockets and made
/// OSX journeys look unstable vs Windows named pipes.
fn scratch_dir(label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    #[cfg(unix)]
    let base = {
        let base = agenterm_platform::ipc::native_runtime_directory();
        let _ = std::fs::create_dir_all(&base);
        base
    };
    #[cfg(not(unix))]
    let base = std::env::temp_dir();
    let dir = base.join(format!(
        "cb{:x}-{}-{}",
        short_token(label),
        std::process::id() % 100_000,
        stamp % 1_000_000_000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(unix)]
    {
        std::fs::canonicalize(&dir).unwrap_or(dir)
    }
    #[cfg(not(unix))]
    {
        // Windows canonicalization introduces a `\\?\` verbatim prefix.
        // Native children accept it, but Git for Windows' MSYS `less.exe`
        // treats its backslashes as shell escapes and loses every separator.
        dir
    }
}

fn write_journey(dir: &Path, commands_json: &str) -> PathBuf {
    let path = dir.join("control-journey.json");
    std::fs::write(&path, commands_json).expect("write control journey");
    path
}

fn unique_control_endpoint(dir: &Path) -> String {
    if cfg!(windows) {
        return format!(
            r"pipe:\\.\pipe\agenterm-con-blackbox-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
    }
    let path = dir.join("c.sock");
    let path_len = path.to_string_lossy().len();
    assert!(
        path_len <= 103,
        "Unix control socket path length {path_len} exceeds sockaddr_un limit: {}",
        path.display()
    );
    format!("unix:{}", path.to_string_lossy())
}

fn invoke_control_output(endpoint: &str, args: &[String]) -> Result<String, String> {
    let output = Command::new(binary())
        .arg("cli")
        .arg("--control")
        .arg(endpoint)
        .args(args)
        .output()
        .map_err(|error| format!("launch control command {args:?}: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(format!(
        "control command {args:?} failed ({:?}): {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn invoke_control(endpoint: &str, args: &[String]) -> Result<(), String> {
    invoke_control_output(endpoint, args).map(|_| ())
}

fn replay_control_journey(endpoint: &str, path: &Path, host_pid: u32) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if invoke_control(endpoint, &["list-tabs".to_owned()]).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("control endpoint did not become ready".to_owned());
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let bytes = std::fs::read(path).map_err(|error| format!("read journey: {error}"))?;
    let commands: Vec<serde_json::Value> =
        serde_json::from_slice(&bytes).map_err(|error| format!("parse journey: {error}"))?;
    for command in commands {
        let object = command
            .as_object()
            .ok_or_else(|| "journey command must be an object".to_owned())?;
        if object
            .get("reset_perf")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            invoke_control(endpoint, &["reset-perf-stats".to_owned()])?;
        } else if let Some(phase) = object
            .get("native_resize_phase")
            .and_then(serde_json::Value::as_str)
        {
            send_native_resize_phase(host_pid, phase)?;
        } else if let Some(point) = object.get("native_click") {
            send_native_click(host_pid, journey_u64(point, "x")?, journey_u64(point, "y")?)?;
        } else if object
            .get("native_click_composer")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            let state = invoke_control_output(endpoint, &["ui-snapshot".to_owned()])?;
            let state: serde_json::Value = serde_json::from_str(&state)
                .map_err(|error| format!("parse UI snapshot: {error}"))?;
            let input = &state["composer_input"];
            send_native_click(
                host_pid,
                journey_u64(input, "x")?.saturating_add(journey_u64(input, "width")? / 2),
                journey_u64(input, "y")?.saturating_add(journey_u64(input, "height")? / 2),
            )?;
        } else if let Some(path) = object
            .get("native_screenshot")
            .and_then(serde_json::Value::as_str)
        {
            capture_native_window(host_pid, Path::new(path))?;
        } else if let Some(path) = object.get("perf_stats").and_then(serde_json::Value::as_str) {
            let stats = invoke_control_output(endpoint, &["perf-stats".to_owned()])?;
            std::fs::write(path, stats).map_err(|error| format!("write perf stats: {error}"))?;
        } else if let Some(path) = object
            .get("ui_snapshot")
            .and_then(serde_json::Value::as_str)
        {
            let state = invoke_control_output(endpoint, &["ui-snapshot".to_owned()])?;
            std::fs::write(path, state).map_err(|error| format!("write UI snapshot: {error}"))?;
        } else if let Some(path) = object
            .get("capture_text")
            .and_then(serde_json::Value::as_str)
        {
            let text = invoke_control_output(endpoint, &["capture-pane".to_owned()])?;
            std::fs::write(path, text).map_err(|error| format!("write capture text: {error}"))?;
        } else if object
            .get("close_window")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            invoke_control(endpoint, &["close-window".to_owned()])?;
        } else if let Some(text) = object.get("text").and_then(serde_json::Value::as_str) {
            invoke_control(endpoint, &["send-text".to_owned(), text.to_owned()])?;
        } else if let Some(text) = object.get("paste").and_then(serde_json::Value::as_str) {
            invoke_control(endpoint, &["send-paste".to_owned(), text.to_owned()])?;
        } else if let Some(key) = object.get("key").and_then(serde_json::Value::as_str) {
            let mut spec = String::new();
            for (field, name) in [("ctrl", "Ctrl"), ("alt", "Alt"), ("shift", "Shift")] {
                if object.get(field).and_then(serde_json::Value::as_bool) == Some(true) {
                    spec.push_str(name);
                    spec.push('+');
                }
            }
            spec.push_str(key);
            invoke_control(endpoint, &["send-keys".to_owned(), spec])?;
        } else if let Some(keys) = object.get("ui_keys").and_then(serde_json::Value::as_array) {
            let mut args = vec!["send-ui-keys".to_owned()];
            for key in keys {
                args.push(
                    key.as_str()
                        .ok_or_else(|| "ui_keys entries must be strings".to_owned())?
                        .to_owned(),
                );
            }
            invoke_control(endpoint, &args)?;
        } else if let Some(ms) = object.get("wait_ms").and_then(serde_json::Value::as_u64) {
            std::thread::sleep(Duration::from_millis(ms));
        } else if let Some(text) = object.get("wait_text").and_then(serde_json::Value::as_str) {
            let timeout = object
                .get("timeout_ms")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(10_000);
            invoke_control(
                endpoint,
                &[
                    "wait-text".to_owned(),
                    "--timeout-ms".to_owned(),
                    timeout.to_string(),
                    text.to_owned(),
                ],
            )?;
        } else if let Some(path) = object.get("screenshot").and_then(serde_json::Value::as_str) {
            invoke_control(
                endpoint,
                &[
                    "screenshot-pane".to_owned(),
                    "--output".to_owned(),
                    path.to_owned(),
                ],
            )?;
        } else if let Some(size) = object.get("resize") {
            invoke_control(
                endpoint,
                &[
                    "resize-window".to_owned(),
                    "--width".to_owned(),
                    journey_u64(size, "width")?.to_string(),
                    "--height".to_owned(),
                    journey_u64(size, "height")?.to_string(),
                ],
            )?;
        } else if let Some(point) = object.get("mouse_move") {
            invoke_mouse(endpoint, "move", "none", point)?;
        } else if let Some(point) = object.get("click") {
            invoke_mouse(endpoint, "click", mouse_button(point), point)?;
        } else if let Some(point) = object.get("mouse_down") {
            invoke_mouse(endpoint, "press", mouse_button(point), point)?;
        } else if let Some(point) = object.get("mouse_up") {
            invoke_mouse(endpoint, "release", mouse_button(point), point)?;
        } else if let Some(wheel) = object.get("wheel") {
            let row = journey_u64(wheel, "row")?;
            let column = journey_u64(wheel, "col")?;
            let notches = journey_i64(wheel, "notches")?;
            let mut args = vec![
                "send-wheel".to_owned(),
                "--column".to_owned(),
                column.to_string(),
                "--row".to_owned(),
                row.to_string(),
                "--notches".to_owned(),
                notches.to_string(),
            ];
            if object.get("ctrl").and_then(serde_json::Value::as_bool) == Some(true) {
                args.push("--ctrl".to_owned());
            }
            invoke_control(endpoint, &args)?;
        } else {
            return Err(format!("unknown journey command: {command}"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn find_process_window(host_pid: u32) -> Result<TestHwnd, String> {
    struct Search {
        pid: u32,
        hwnd: TestHwnd,
    }

    unsafe extern "system" fn find_process_window(hwnd: TestHwnd, lparam: isize) -> i32 {
        let search = unsafe { &mut *(lparam as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == search.pid {
            search.hwnd = hwnd;
            0
        } else {
            1
        }
    }

    let mut search = Search {
        pid: host_pid,
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(
            Some(find_process_window),
            (&mut search as *mut Search) as isize,
        )
    };
    if search.hwnd.is_null() {
        return Err(format!("no native window found for con pid {host_pid}"));
    }
    Ok(search.hwnd)
}

#[cfg(windows)]
fn capture_native_window(host_pid: u32, path: &Path) -> Result<(), String> {
    use agenterm_platform::screenshot::{
        NativeCaptureArea, ScreenshotWindowHandle, capture_native_window_png,
    };

    let hwnd = find_process_window(host_pid)?;
    let handle = unsafe { ScreenshotWindowHandle::from_raw(hwnd as isize) }
        .ok_or_else(|| "native window handle was null".to_owned())?;
    capture_native_window_png(
        handle,
        path,
        NativeCaptureArea::Client {
            left: 0,
            top: 0,
            width: 800,
            height: 480,
        },
    )
    .map(|_| ())
    .map_err(|error| format!("capture native con window: {error}"))
}

#[cfg(windows)]
fn send_native_click(host_pid: u32, x: u64, y: u64) -> Result<(), String> {
    let hwnd = find_process_window(host_pid)?;
    let x = u16::try_from(x).map_err(|_| "native click x exceeds u16".to_owned())?;
    let y = u16::try_from(y).map_err(|_| "native click y exceeds u16".to_owned())?;
    let lparam = isize::try_from(u32::from(x) | (u32::from(y) << 16))
        .map_err(|_| "native click coordinates exceed LPARAM".to_owned())?;
    unsafe {
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, lparam);
        SendMessageW(hwnd, WM_LBUTTONUP, 0, lparam);
    }
    Ok(())
}

#[cfg(windows)]
fn activate_native_simplified_chinese_ime(host_pid: u32) -> Result<(), String> {
    use agenterm_platform::input_inject::{PointerButton, PointerPosition, pointer_click};

    let hwnd = find_process_window(host_pid)?;
    if unsafe { SetForegroundWindow(hwnd) } == 0 || unsafe { GetForegroundWindow() } != hwnd {
        return Err("Windows denied foreground activation for native IME acceptance".to_owned());
    }

    let mut rect = TestRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return Err("GetWindowRect failed for native IME acceptance".to_owned());
    }
    pointer_click(
        PointerPosition {
            x: rect.left.saturating_add((rect.right - rect.left) / 2),
            y: rect.top.saturating_add((rect.bottom - rect.top) / 2),
        },
        PointerButton::Left,
        1,
    )
    .map_err(|error| format!("activate native con window: {error:?}"))?;

    let layout_id = "00000804\0".encode_utf16().collect::<Vec<_>>();
    let layout = unsafe { LoadKeyboardLayoutW(layout_id.as_ptr(), KLF_ACTIVATE) };
    if layout == 0 {
        return Err("Simplified Chinese keyboard layout 00000804 is not installed".to_owned());
    }
    unsafe { SendMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, layout) };

    let ime_window = unsafe { ImmGetDefaultIMEWnd(hwnd) };
    if ime_window.is_null() {
        return Err("the native con window has no default IME window".to_owned());
    }
    unsafe {
        SendMessageW(ime_window, WM_IME_CONTROL, IMC_SETOPENSTATUS, 1);
        SendMessageW(
            ime_window,
            WM_IME_CONTROL,
            IMC_SETCONVERSIONMODE,
            IME_CMODE_NATIVE,
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn send_native_click(_host_pid: u32, _x: u64, _y: u64) -> Result<(), String> {
    Err("native window clicks are unavailable on this host".to_owned())
}

#[cfg(not(windows))]
fn capture_native_window(_host_pid: u32, _path: &Path) -> Result<(), String> {
    Err("native window screenshots are unavailable on this host".to_owned())
}

#[cfg(windows)]
fn send_native_resize_phase(host_pid: u32, phase: &str) -> Result<(), String> {
    let hwnd = find_process_window(host_pid)?;
    let message = match phase {
        "begin" => WM_ENTERSIZEMOVE,
        "end" => WM_EXITSIZEMOVE,
        _ => return Err(format!("unknown native resize phase {phase:?}")),
    };
    unsafe { SendMessageW(hwnd, message, 0, 0) };
    Ok(())
}

#[cfg(not(windows))]
fn send_native_resize_phase(_host_pid: u32, phase: &str) -> Result<(), String> {
    Err(format!(
        "native resize phase {phase:?} is unavailable on this host"
    ))
}

fn journey_u64(value: &serde_json::Value, field: &str) -> Result<u64, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("journey field {field} must be an unsigned integer"))
}

fn journey_i64(value: &serde_json::Value, field: &str) -> Result<i64, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| format!("journey field {field} must be an integer"))
}

fn mouse_button(value: &serde_json::Value) -> &str {
    value
        .get("button")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("left")
}

fn invoke_mouse(
    endpoint: &str,
    action: &str,
    button: &str,
    point: &serde_json::Value,
) -> Result<(), String> {
    invoke_control(
        endpoint,
        &[
            "send-mouse".to_owned(),
            "--action".to_owned(),
            action.to_owned(),
            "--button".to_owned(),
            button.to_owned(),
            "--column".to_owned(),
            journey_u64(point, "col")?.to_string(),
            "--row".to_owned(),
            journey_u64(point, "row")?.to_string(),
        ],
    )
}

/// Owns a spawned `agenterm-con` child and guarantees it is killed even if
/// an assertion panics mid-test — otherwise a failing test leaks a live GUI
/// process (and its own child shell) for the rest of the run.
struct ConSession {
    child: Child,
    snapshot_path: PathBuf,
    driver: Option<std::thread::JoinHandle<()>>,
    driver_error: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl ConSession {
    /// Spawns `agenterm-con --no-activate <extra_args before -e>`. `extra_args`
    /// must come before any `-e`, matching this binary's own contract that
    /// `-e` consumes the remainder of the command line verbatim.
    fn spawn<S: AsRef<std::ffi::OsStr>>(dir: &Path, extra_args: &[S]) -> Self {
        Self::spawn_with_activation(dir, extra_args, false)
    }

    fn spawn_with_activation<S: AsRef<std::ffi::OsStr>>(
        dir: &Path,
        extra_args: &[S],
        activate: bool,
    ) -> Self {
        let snapshot_path = dir.join("snapshot.json");
        let mut child_args: Vec<std::ffi::OsString> = extra_args
            .iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect();
        let journey = child_args
            .iter()
            .position(|argument| argument == TEST_JOURNEY_ARG)
            .map(|index| {
                assert!(index + 1 < child_args.len(), "test journey path is missing");
                let path = PathBuf::from(child_args.remove(index + 1));
                child_args.remove(index);
                path
            });
        let endpoint = unique_control_endpoint(dir);
        let mut command = Command::new(binary());
        if activate {
            command.env_remove("AGENTERM_NO_ACTIVATE");
        } else {
            command.arg("--no-activate");
        }
        command.arg("--emit-snapshot").arg(&snapshot_path);
        if journey.is_some() {
            command.arg("--control").arg(&endpoint);
        }
        let child = command
            .args(&child_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn agenterm-con");
        let host_pid = child.id();
        let driver_error = std::sync::Arc::new(std::sync::Mutex::new(None));
        let driver = journey.map(|journey| {
            let endpoint = endpoint.clone();
            let error_slot = std::sync::Arc::clone(&driver_error);
            std::thread::spawn(move || {
                if let Err(error) = replay_control_journey(&endpoint, &journey, host_pid) {
                    *error_slot
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error);
                }
            })
        });
        Self {
            child,
            snapshot_path,
            driver,
            driver_error,
        }
    }

    /// Polls the snapshot file until `predicate` accepts its parsed content
    /// or `timeout` elapses. Retrying rather than sleeping once is what
    /// makes this robust against slow CI machines and PTY scheduling
    /// jitter — a fixed sleep is exactly the kind of flake source a
    /// black-box GUI test needs to avoid, not introduce.
    fn wait_for(
        &self,
        timeout: Duration,
        predicate: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let deadline = Instant::now() + timeout;
        let mut last_seen: Option<serde_json::Value> = None;
        loop {
            if let Some(error) = self
                .driver_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
            {
                panic!("control journey failed: {error}");
            }
            if let Ok(bytes) = std::fs::read(&self.snapshot_path)
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                if predicate(&value) {
                    return value;
                }
                last_seen = Some(value);
            }
            if Instant::now() >= deadline {
                panic!(
                    "timed out waiting for snapshot condition; last seen: {}",
                    last_seen
                        .map(|v| serde_json::to_string_pretty(&v).unwrap_or_default())
                        .unwrap_or_else(|| "<no valid snapshot read yet>".to_owned())
                );
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Joined text of every visible row, for a simple substring assertion
    /// without the caller needing to know which row something landed on.
    fn screen_text(value: &serde_json::Value) -> String {
        value["rows_text"]
            .as_array()
            .expect("rows_text array")
            .iter()
            .map(|row| row.as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(windows)]
#[test]
#[ignore = "requires an interactive Windows desktop and installed Microsoft Pinyin"]
fn native_microsoft_pinyin_preedit_and_commit_reach_the_real_window() {
    use agenterm_platform::input_inject::send_keys;

    let _guard = gui_test_guard();
    let dir = scratch_dir("native-pinyin");
    let args = ["-e", "cmd.exe", "/k"];
    let session = ConSession::spawn_with_activation(&dir, &args, true);
    session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["child_alive"] == true
    });
    activate_native_simplified_chinese_ime(session.child.id())
        .expect("prepare real Simplified Chinese IME");

    for key in ["n", "i", "h", "a", "o"] {
        send_keys(key).unwrap_or_else(|error| panic!("inject physical {key} key: {error:?}"));
    }
    let composing = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["ime_preedit"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    });
    assert!(
        composing["ime_preedit"]
            .as_str()
            .is_some_and(|text| text.to_ascii_lowercase().contains("nihao")),
        "Microsoft Pinyin did not expose the expected native preedit: {composing}"
    );

    send_keys("space").expect("commit the first Microsoft Pinyin candidate");
    let committed = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["ime_preedit"] == "" && ConSession::screen_text(snapshot).contains("你好")
    });
    assert!(
        ConSession::screen_text(&committed).contains("你好"),
        "native IME commit did not reach the terminal: {committed}"
    );

    let screenshot = dir.join("native-pinyin.png");
    capture_native_window(session.child.id(), &screenshot).expect("capture native IME result");
    assert!(
        std::fs::metadata(&screenshot)
            .expect("native IME screenshot metadata")
            .len()
            > 100,
        "native IME screenshot was empty"
    );
}

impl Drop for ConSession {
    fn drop(&mut self) {
        // Best-effort: the process may have already exited on its own (the
        // child-exit tests rely on exactly that). TerminateProcess-style
        // kill does not run this process's own Drop chain for its PTY child,
        // same caveat noted in plan/plan-v0.1.16.md — acceptable for a test
        // teardown, not for a real session.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(driver) = self.driver.take() {
            let _ = driver.join();
        }
    }
}

#[test]
fn version_and_help_are_synchronous_and_never_open_a_window() {
    // Deliberately does *not* take `gui_test_guard()`: the whole point of
    // this test is that it never opens a window or spawns a PTY, so it
    // does not contend with the tests that do and does not need to wait
    // its turn behind them.
    //
    // These exit before any window/PTY is touched (see offline_cli_exit in
    // main()), so a plain synchronous `.output()` is the right tool — no
    // snapshot needed, and if this ever regressed into opening a window
    // first, this test would hang instead of completing, which is itself
    // a meaningful failure mode to catch.
    let version = Command::new(binary())
        .arg("--version")
        .output()
        .expect("run --version");
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("agenterm-con "));

    let help = Command::new(binary())
        .arg("--help")
        .output()
        .expect("run --help");
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(
        !help_text.contains("--script"),
        "con must not expose scripts"
    );
    assert!(
        help_text.contains("send-paste"),
        "help must expose control paste"
    );
    assert!(
        help_text.contains("--emit-snapshot"),
        "help must document --emit-snapshot"
    );
}

#[test]
fn removed_script_flag_fails_fast_without_opening_a_window() {
    // The lightweight host must reject the old script runtime before opening
    // a window; automation belongs to the public control CLI.
    let dir = scratch_dir("bad-script");
    let script_path = write_journey(&dir, "{not valid json");
    let output = Command::new(binary())
        .arg("--no-activate")
        .arg("--script")
        .arg(&script_path)
        .output()
        .expect("run with a broken script");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown argument '--script'"), "{stderr}");
}

#[test]
fn dash_e_passthrough_retains_final_screen_after_child_exit() {
    let _guard = gui_test_guard();
    // Proves -e's argv passthrough end-to-end: not just that the CLI parser
    // builds the right Vec<String> (that's unit-tested), but that a real
    // spawned program actually receives it and its actual output lands on
    // screen. Uses /c (not /k) so the child exits on its own while the host
    // must retain the tab and its final frame.
    let dir = scratch_dir("dash-e");
    let args = command_shell_args("echo DASH_E_PASSTHROUGH_MARKER");
    let mut session = ConSession::spawn(&dir, &args);
    session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("DASH_E_PASSTHROUGH_MARKER")
    });

    let exited = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["child_alive"] == false
    });
    assert_eq!(exited["child_exit_code"], 0, "{exited}");
    assert!(
        ConSession::screen_text(&exited).contains("DASH_E_PASSTHROUGH_MARKER"),
        "final child output was not retained: {exited}"
    );
    assert!(
        session
            .child
            .try_wait()
            .expect("poll retained host")
            .is_none(),
        "host exited when its only child completed"
    );
    let _ = session.child.kill();
}

#[test]
fn nonexistent_program_via_dash_e_exits_cleanly_instead_of_hanging() {
    let _guard = gui_test_guard();
    let _dir = scratch_dir("bad-e");
    let mut child = Command::new(binary())
        .arg("--no-activate")
        .args(["-e", "definitely-not-a-real-program-agenterm-con-test"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn with a bad -e target");

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("agenterm-con hung instead of exiting on a spawn failure");
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    assert_eq!(
        status.code(),
        Some(1),
        "a child spawn failure must map to the ordinary runtime-error exit code"
    );
}

#[test]
fn controlled_text_and_paste_both_reach_the_pty() {
    let _guard = gui_test_guard();
    // Closes a gap this session's own retrospective flagged: paste had unit
    // coverage for its byte-level encoding, but the wiring from
    // ConTerminal::paste_text to a live session was never exercised
    // end-to-end. `paste` in a script goes through that exact function.
    let dir = scratch_dir("text-and-paste");
    let script = write_journey(
        &dir,
        r#"[
            {"text": "echo TYPED_MARKER\r"},
            {"wait_ms": 300},
            {"paste": "PASTED_MARKER\r"},
            {"wait_ms": 300}
        ]"#,
    );
    let args = interactive_shell_args(&script);
    let session = ConSession::spawn(&dir, &args);
    let snapshot = session.wait_for(Duration::from_secs(10), |snapshot| {
        let text = ConSession::screen_text(snapshot);
        text.contains("TYPED_MARKER") && text.contains("PASTED_MARKER")
    });
    let text = ConSession::screen_text(&snapshot);
    // Order matters: paste must not have raced ahead of the typed command.
    let typed_at = text.find("TYPED_MARKER").unwrap();
    let pasted_at = text.find("PASTED_MARKER").unwrap();
    assert!(
        typed_at < pasted_at,
        "script commands ran out of order:\n{text}"
    );
}

#[test]
fn cjk_output_from_a_real_child_process_appears_as_actual_characters() {
    let _guard = gui_test_guard();
    // Complements the pixel-level CJK regression test (agenterm-con.rs's own
    // font fallback fix) with the layer it cannot cover: that real UTF-8
    // bytes from a real child process survive PTY -> vt100 -> snapshot
    // intact. This does not prove the glyphs were *painted* (no pixels
    // here) — that half is `font::raster` returning `Some` for CJK, already
    // unit-tested — but it does prove the text pipeline carries them
    // correctly end-to-end, which is a distinct and previously-unverified
    // integration point.
    let dir = scratch_dir("cjk");
    // Deliberately *not* `type` of a UTF-8 file: cmd.exe's `type` interprets
    // the bytes it reads through the console's active ANSI/OEM codepage
    // rather than passing them through raw, so a UTF-8 file comes out
    // garbled regardless of `chcp` — this test tried that first and learned
    // the hard way. Literal text on the command line is delivered as UTF-16
    // (CommandLineToArgvW) and `echo` re-emits it through the *output*
    // encoding, which `chcp 65001` does control correctly.
    let command = if cfg!(windows) {
        "chcp 65001>nul && echo CJK_MARKER_\u{4e2d}\u{6587}\u{5b57}\u{5f62}"
    } else {
        "printf 'CJK_MARKER_\u{4e2d}\u{6587}\u{5b57}\u{5f62}\\n'"
    };
    let args = command_shell_args(command);
    let mut session = ConSession::spawn(&dir, &args);
    session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("CJK_MARKER_\u{4e2d}\u{6587}\u{5b57}\u{5f62}")
    });
    let exited = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["child_alive"] == false
    });
    assert_eq!(exited["child_exit_code"], 0, "{exited}");
    assert!(
        ConSession::screen_text(&exited).contains("CJK_MARKER_\u{4e2d}\u{6587}\u{5b57}\u{5f62}"),
        "final CJK output was not retained: {exited}"
    );
    let _ = session.child.kill();
}

#[test]
fn snapshot_reports_exit_code_and_retains_tab_until_explicit_close() {
    let _guard = gui_test_guard();
    // child_alive is the field a test (or agent) should poll instead of
    // guessing a fixed delay before asserting a command finished. Verify it
    // actually flips, rather than trusting the field always reads true.
    let dir = scratch_dir("child-alive");
    // `echo` alone exits in the same instant its output becomes visible —
    // observed as a real race, not a hypothetical one: the marker and
    // child_alive:false landed in the same snapshot. The trailing `ping`
    // keeps the child alive for ~1s after the marker prints, giving the
    // poll below a real window to observe true before it flips.
    let command = if cfg!(windows) {
        "echo READY_MARKER && ping -n 2 127.0.0.1 >nul"
    } else {
        "echo READY_MARKER; sleep 1"
    };
    let args = command_shell_args(command);
    let mut session = ConSession::spawn(&dir, &args);
    let snapshot = session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("READY_MARKER")
    });
    assert_eq!(snapshot["child_alive"], true);

    let exited = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["child_alive"] == false
    });
    assert_eq!(exited["child_exit_code"], 0, "{exited}");
    assert!(
        ConSession::screen_text(&exited).contains("READY_MARKER"),
        "final screen was discarded after child exit: {exited}"
    );
    let retain_deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < retain_deadline {
        assert!(
            session
                .child
                .try_wait()
                .expect("poll retained host")
                .is_none(),
            "host exited before the tab was explicitly closed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = session.child.kill();
}

#[test]
fn typed_input_echoes_back_well_under_one_blink_cycle() {
    let _guard = gui_test_guard();
    // Regression canary for a real, user-reported bug: `PixelWindowEvent::
    // Wake` — fired by the PTY reader thread whenever the shell actually
    // sends new output, i.e. exactly when a keystroke's echo has arrived —
    // used to fall through to a wildcard match arm that requested no
    // redraw at all. Nothing else painted a fresh PTY echo either, so the
    // *only* thing that ever eventually repainted it was the unrelated
    // cursor-blink timer (`BLINK_INTERVAL` = 530ms), which fires on its own
    // fixed cadence regardless of when input landed — average ~265ms
    // added latency, worst case ~530ms. That is not a guess: it is exactly
    // what "often takes about half a second to respond" (the reported
    // symptom) means. Fixed by having `Wake` (and `Keyboard`, for purely
    // local effects) call `window.request_redraw()` directly.
    //
    // This can't assert a tight bound with full confidence — real wall-
    // clock timing on a shared, occasionally-loaded machine is inherently
    // noisy, and window/PTY startup cost varies — so this is a canary, not
    // a proof: comfortably passes under the fix, and would have measurably
    // and repeatably approached/exceeded BLINK_INTERVAL under the bug this
    // fixes (verified manually while diagnosing: reverting the
    // `PixelWindowEvent::Wake` arm reproduces multi-hundred-ms echo delay).
    let dir = scratch_dir("typing-latency");
    let script = write_journey(
        &dir,
        r#"[
            {"text": "echo LATENCY_READY\r"},
            {"wait_ms": 200},
            {"text": "echo LATENCY_MARKER\r"}
        ]"#,
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("LATENCY_READY")
    });
    let started = Instant::now();
    session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("LATENCY_MARKER")
    });
    let elapsed = started.elapsed();
    // Measured on this machine: fixed, this consistently lands around
    // 650-700ms (mostly the intentional 400ms scripted wait plus normal
    // window/ConPTY startup); with the `Wake` redraw removed to reproduce
    // the bug, repeated runs measured 2.9-3.3s — not the ~530ms a single
    // blink cycle alone would suggest, evidently compounding somehow, but
    // unambiguously and repeatably much worse. 1500ms sits with comfortable
    // margin below every "fixed" measurement and comfortably above every
    // "bug reproduced" one.
    assert!(
        elapsed < Duration::from_millis(1500),
        "typed output took {elapsed:?} to become visible — the 400ms scripted \
         pace plus normal window/PTY startup should not come close to this; \
         a regression back to blink-driven repainting is the likely cause"
    );
    let _ = session.child.kill();
}

#[test]
fn key_command_moves_the_cursor_through_the_real_forward_key_path() {
    // Proves the complete wiring: a scripted key event reaches
    // ConTerminal::forward_key (the same path a real OS keyboard event
    // takes). On Windows the platform adapter attaches to the child console
    // and writes native KEY_EVENT_RECORD press/release pairs; on Unix the
    // same product path encodes CSI sequences into the PTY. Either host's
    // cooked line editor must move exactly two cells.
    let _guard = gui_test_guard();
    let dir = scratch_dir("key-wiring");
    let script = write_journey(
        &dir,
        r#"[
            {"text": "ABCDE"},
            {"wait_ms": 400},
            {"key": "ArrowLeft"},
            {"key": "ArrowLeft"},
            {"wait_ms": 400}
        ]"#,
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    let first = session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("ABCDE")
            && snapshot["cursor"]["col"].as_u64().unwrap_or(0) >= 5
    });
    let col_after_typing = first["cursor"]["col"].as_u64().expect("cursor.col");

    let second = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["cursor"]["col"].as_u64() == Some(col_after_typing.saturating_sub(2))
    });
    assert_eq!(
        second["cursor"]["col"].as_u64(),
        Some(col_after_typing - 2),
        "two ArrowLeft presses must move the cursor back exactly two columns"
    );
    let _ = session.child.kill();
}

#[test]
fn controlled_click_without_drag_does_not_create_a_selection() {
    let _guard = gui_test_guard();
    // Closes the gap this session's own plan doc flagged in plain writing:
    // the original hidden driver had no mouse commands. cmd.exe never negotiates mouse
    // reporting (DECSET 1000/1002/1003), so a real click here always falls
    // through `handle_pointer_button`'s local path. A press and release at
    // the same cell must not leave a zero-length selection: that would keep
    // the cell inverted and steal the next Ctrl+C/right-click. This test
    // proves the public control endpoint drives that same physical path.
    let dir = scratch_dir("click-selection");
    // `wait_text`, not a guessed `wait_ms`: `drain_pty` clears the selection
    // on any new output ("new output clears stale selection"), so a click
    // that lands before `echo`'s output arrives has its selection wiped
    // before any snapshot can observe it. A fixed duration passes on a fast
    // box and failed on a loaded CI runner (windows lane, run 31400784754);
    // this sequences on the output actually being on screen instead.
    let script = write_journey(
        &dir,
        r#"[
            {"text": "echo CLICK_MARKER\r"},
            {"wait_text": "CLICK_MARKER", "timeout_ms": 15000},
            {"wait_ms": 300},
            {"click": {"row": 3, "col": 5}},
            {"wait_ms": 200}
        ]"#,
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    session
        .driver
        .take()
        .expect("control journey driver")
        .join()
        .expect("control journey thread");
    if let Some(error) = session
        .driver_error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
    {
        panic!("control journey failed: {error}");
    }
    let snapshot = session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("CLICK_MARKER")
            && snapshot["selection"].is_null()
    });
    assert!(snapshot["selection"].is_null(), "{snapshot}");
    let _ = session.child.kill();
}

#[cfg(windows)]
#[test]
fn controlled_terminal_click_keeps_native_pixels_stable_outside_local_feedback() {
    let _guard = gui_test_guard();
    let dir = scratch_dir("click-native-pixels");
    let before_path = dir.join("before.png");
    let after_path = dir.join("after.png");
    let script = write_journey(
        &dir,
        &format!(
            r#"[
                {{"text": "echo CLICK_PIXEL_MARKER\r"}},
                {{"wait_text": "CLICK_PIXEL_MARKER", "timeout_ms": 15000}},
                {{"wait_ms": 300}},
                {{"native_screenshot": {}}},
                {{"native_click": {{"x": 420, "y": 220}}}},
                {{"wait_ms": 100}},
                {{"native_screenshot": {}}}
            ]"#,
            serde_json::to_string(before_path.to_str().unwrap()).unwrap(),
            serde_json::to_string(after_path.to_str().unwrap()).unwrap(),
        ),
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    session
        .driver
        .take()
        .expect("native screenshot journey driver")
        .join()
        .expect("native screenshot journey thread");
    if let Some(error) = session
        .driver_error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
    {
        panic!("native screenshot journey failed: {error}");
    }
    assert!(before_path.is_file() && after_path.is_file());

    fn decode(path: &Path) -> (usize, usize, Vec<u8>, png::ColorType) {
        let decoder = png::Decoder::new(std::fs::File::open(path).expect("open native screenshot"));
        let mut reader = decoder.read_info().expect("read native screenshot info");
        let mut bytes = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut bytes)
            .expect("decode native screenshot");
        bytes.truncate(info.buffer_size());
        (
            info.width as usize,
            info.height as usize,
            bytes,
            info.color_type,
        )
    }

    let (before_width, before_height, before, before_color) = decode(&before_path);
    let (after_width, after_height, after, after_color) = decode(&after_path);
    assert_eq!((after_width, after_height), (before_width, before_height));
    fn visible_pixels(bytes: &[u8], color: png::ColorType) -> usize {
        match color {
            png::ColorType::Rgb => bytes
                .chunks_exact(3)
                .filter(|pixel| pixel.iter().any(|channel| *channel > 0x20))
                .count(),
            png::ColorType::Rgba => bytes
                .chunks_exact(4)
                .filter(|pixel| pixel[..3].iter().any(|channel| *channel > 0x20))
                .count(),
            other => panic!("unexpected native screenshot color type {other:?}"),
        }
    }
    let before_visible = visible_pixels(&before, before_color);
    let after_visible = visible_pixels(&after, after_color);
    assert!(
        after_visible * 10 >= before_visible * 9,
        "terminal click erased visible native pixels: before={before_visible}, after={after_visible}"
    );
    let _ = session.child.kill();
}

#[cfg(windows)]
#[test]
fn native_composer_focus_keeps_editing_keys_local_until_enter() {
    let _guard = gui_test_guard();
    let dir = scratch_dir("composer-focus-routing");
    let before_submit = dir.join("before-submit.txt");
    let ui_state = dir.join("ui-state.json");
    let script = write_journey(
        &dir,
        &format!(
            r#"[
                {{"text":"echo COMPOSER_READY\r"}},
                {{"wait_text":"COMPOSER_READY"}},
                {{"native_click_composer":true}},
                {{"ui_keys":["e","c","h","o","Space","S","T","A","L","E"]}},
                {{"ui_keys":["Ctrl+A","e","c","h","o","Space","C","O","M","P","O","S","E","R","_","O","K"]}},
                {{"ui_keys":["Ctrl+A","Ctrl+C","Ctrl+X","Ctrl+V","Space","F","I","N","A","L"]}},
                {{"ui_snapshot":{}}},
                {{"capture_text":{}}},
                {{"ui_keys":["Enter"]}},
                {{"wait_text":"COMPOSER_OK FINAL"}}
            ]"#,
            serde_json::to_string(ui_state.to_str().unwrap()).unwrap(),
            serde_json::to_string(before_submit.to_str().unwrap()).unwrap(),
        ),
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    let completed = session.wait_for(Duration::from_secs(15), |snapshot| {
        ConSession::screen_text(snapshot).contains("COMPOSER_OK FINAL")
    });
    assert_eq!(completed["child_alive"], true, "{completed}");

    let state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&ui_state).expect("UI snapshot must be written before submit"),
    )
    .expect("UI snapshot must be JSON");
    assert_eq!(state["composer_focused"], true, "{state}");
    assert_eq!(state["composer_text"], "echo COMPOSER_OK FINAL", "{state}");
    let terminal_before = std::fs::read_to_string(&before_submit)
        .expect("terminal capture must be written before submit");
    assert!(
        !terminal_before.contains("COMPOSER_OK") && !terminal_before.contains("STALE"),
        "focused composer keys leaked into the PTY before Enter: {terminal_before:?}"
    );
    let _ = session.child.kill();
}

#[test]
fn controlled_press_drag_release_extends_a_local_selection() {
    let _guard = gui_test_guard();
    // Closes a gap this session's own plan doc flagged: `click` is atomic
    // press+release and cannot express a drag, so drag-selection —
    // `mouse_move` events extending a selection while a button is held via
    // `handle_pointer_moved`'s `self.selecting` branch — had never been
    // driven by anything but a real physical mouse. `mouse_down`/`mouse_up`
    // close that: press, move (still held), release, and confirm the
    // selection followed the drag rather than staying pinned to the press
    // point the way a single `click` always does.
    let dir = scratch_dir("drag-selection");
    // Row 0 is the marker line under a clean interactive shell (cmd / bash
    // --norc). Hard-coding cmd.exe + a Windows-only row offset made OSX look
    // like a selection regression when the host was fine.
    let script = write_journey(
        &dir,
        r#"[
            {"text": "echo DRAG_MARKER\r"},
            {"wait_text": "DRAG_MARKER", "timeout_ms": 15000},
            {"wait_ms": 300},
            {"mouse_down": {"row": 1, "col": 0}},
            {"mouse_move": {"row": 1, "col": 10}},
            {"mouse_up": {"row": 1, "col": 10}},
            {"wait_ms": 200}
        ]"#,
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    let snapshot = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["selection"].is_array()
            && snapshot["selection"][1]["col"] != snapshot["selection"][0]["col"]
    });
    // The anchor (press point) must stay put and the moving endpoint must
    // have followed the drag to the release point — a `click` alone could
    // only ever produce a single-cell selection, so this is real evidence
    // the drag path, not just press/release individually, is wired.
    assert_eq!(snapshot["selection"][0]["row"], 1, "{snapshot}");
    assert_eq!(
        snapshot["selection"][0]["col"], 0,
        "anchor must stay at the press point: {snapshot}"
    );
    assert_eq!(snapshot["selection"][1]["row"], 1, "{snapshot}");
    assert_eq!(
        snapshot["selection"][1]["col"], 10,
        "moving endpoint must follow the drag: {snapshot}"
    );
    let _ = session.child.kill();
}

#[test]
fn controlled_wheel_moves_the_real_scrollback_offset_up_then_down() {
    let _guard = gui_test_guard();
    // Same gap as above, the scroll half: proves a scripted `wheel` reaches
    // `handle_wheel`'s local-scrollback branch in a live session, not just
    // that `scroll_by`'s clamping is correct in isolation
    // (`scrolling_clamps_to_available_scrollback` already covers that).
    // Both directions in one session: scrolling down after scrolling up is
    // what proves `notches`' sign is actually wired through, not just that
    // *a* wheel command moves the offset off zero once.
    let dir = scratch_dir("wheel-scroll");
    let scroll_command = if cfg!(windows) {
        "for /l %i in (1,1,120) do @echo SCROLL_LINE_%i"
    } else {
        "i=1; while [ $i -le 120 ]; do echo SCROLL_LINE_$i; i=$((i+1)); done"
    };
    let script = write_journey(
        &dir,
        &format!(
            r#"[
            {{"text": {} }},
            {{"wait_text": "SCROLL_LINE_120", "timeout_ms": 30000}},
            {{"wait_ms": 300}},
            {{"wheel": {{"row": 0, "col": 0, "notches": 5}}}},
            {{"wait_ms": 200}},
            {{"wheel": {{"row": 0, "col": 0, "notches": -2}}}},
            {{"wait_ms": 200}}
        ]"#,
            serde_json::to_string(&format!("{scroll_command}\r")).unwrap()
        ),
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    // wait_ms in the script paces the wheel commands, not this poll — but the
    // wheel commands only move the offset meaningfully once the loop has
    // actually pushed 200 lines into scrollback, so this confirms that
    // happened before trusting the scroll assertions below.
    session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("SCROLL_LINE_120")
    });
    let scrolled_up = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["scroll_offset"].as_u64().unwrap_or(0) > 0
    });
    let offset_after_up = scrolled_up["scroll_offset"].as_u64().unwrap();
    assert_eq!(
        offset_after_up, 5,
        "5 wheel-up notches must move exactly 5 lines: {scrolled_up}"
    );

    let scrolled_down = session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["scroll_offset"].as_u64() == Some(3)
    });
    assert_eq!(
        scrolled_down["scroll_offset"], 3,
        "wheel-down after wheel-up must move back down, not clamp or ignore the sign"
    );
    let _ = session.child.kill();
}

#[test]
fn repeated_ctrl_wheel_zoom_cycles_survive_without_crashing() {
    let _guard = gui_test_guard();
    // Reproduction attempt for a user-reported bug: "Ctrl+wheel zoom past a
    // certain size and the process exits" (self-terminates, no error
    // dialog). The con delivery profiles now use unwind so native callback
    // boundaries can convert a panic to typed fail-closed state; this journey
    // still guards cumulative resize/raster state that need not panic. An earlier
    // investigation (`3d23dfde`) swept `apply_resize`/`font::raster` as
    // *independent, single-shot* unit-test calls at every font size and
    // found nothing — which cannot rule out a bug that only manifests
    // across a *sequence* of real, cumulative zoom steps against a live
    // ConPTY session (each one resizing the real pseudoconsole via
    // `master.resize`, not a mocked/isolated call). This drives that exact
    // sequence for real: 20 full zoom-in-to-max, zoom-out-to-min cycles
    // (1600 individual, back-to-back `zoom_font` calls — no `wait_ms`
    // between them, deliberately, to match a fast real wheel spin rather
    // than a gentle one) through `wheel` with `ctrl: true` (added alongside
    // this test specifically because the original driver had no way to reach the
    // Ctrl+wheel code path at all before now).
    //
    // Result of this investigation, stated plainly: did not reproduce. An
    // earlier ad hoc run of this same scenario driven from outside the Rust
    // test harness (a raw shell script backgrounding the process and
    // polling `--emit-snapshot` for `child_alive`) did show `child_alive:
    // false` with the process still running — but re-run through this
    // harness's real `Child`/`try_wait` process handle, which is the
    // trustworthy source of truth here (Git Bash's `timeout` + background
    // job control has known quirks signaling a detached Windows GUI
    // process), that result did not reproduce. Left as permanent
    // regression coverage — if this ever does catch a real crash, that is
    // exactly the point of keeping it, not a sign the earlier investigation
    // (or this one) was wrong to look.
    let dir = scratch_dir("ctrl-wheel-zoom-cycles");
    let mut commands = vec![
        r#"{"text": "echo ZOOM_TEST_MARKER\r"}"#.to_owned(),
        r#"{"wait_ms": 300}"#.to_owned(),
    ];
    // 8..=36 is the real clamp range (`zoom_font`); overshoot on each leg so
    // every cycle actually touches both ends rather than assuming the step
    // count lines up exactly.
    for _ in 0..20 {
        commands.push(r#"{"wheel": {"row": 0, "col": 0, "notches": 40}, "ctrl": true}"#.to_owned());
        commands
            .push(r#"{"wheel": {"row": 0, "col": 0, "notches": -40}, "ctrl": true}"#.to_owned());
    }
    commands.push(r#"{"wait_ms": 250}"#.to_owned());
    commands.push(r#"{"text": "echo ZOOM_DONE\r"}"#.to_owned());
    commands.push(r#"{"wait_ms": 300}"#.to_owned());
    let script_json = format!("[{}]", commands.join(","));
    let script = write_journey(&dir, &script_json);

    let args = interactive_shell_args(script.as_path());
    let mut session = ConSession::spawn(&dir, &args);
    let completed = session.wait_for(Duration::from_secs(30), |snapshot| {
        ConSession::screen_text(snapshot).contains("ZOOM_DONE")
    });
    assert_eq!(
        completed["child_alive"], true,
        "shell must remain alive after the final zoom checkpoint: {completed}"
    );
    let snapshot = session.wait_for(Duration::from_secs(15), |snapshot| {
        ConSession::screen_text(snapshot).contains("ZOOM_TEST_MARKER")
    });
    assert_eq!(
        snapshot["child_alive"], true,
        "process must still be alive: {snapshot}"
    );

    // The real assertion: the *process itself* must still be running after
    // every zoom cycle, not just that the last snapshot read looked fine —
    // a crashed process stops writing new snapshots entirely, which the
    // text check above cannot distinguish from "still alive but slow."
    // Polls rather than checking once, since a crash could land anywhere
    // in the last zoom cycle, slightly after this point in real time.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(status)) = session.child.try_wait() {
            panic!(
                "agenterm-con exited on its own during/after Ctrl+wheel zoom cycling \
                 (status: {status:?}) — this is the crash under investigation"
            );
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let _ = session.child.kill();
}

#[test]
fn real_tui_less_scrolls_via_character_and_space_keys() {
    let _guard = gui_test_guard();
    // Every other test in this file drives cmd.exe — a cooked-mode line
    // editor. `less` is a genuinely different animal: a raw/cbreak-mode
    // curses-style TUI that reads keys directly rather than through a line
    // editor, which is exactly the category of program plan-v0.1.16.md §C
    // says has zero black-box coverage. This proves character-key and
    // space-key forwarding (`forward_key` -> `write_pty`) reaches such a
    // program and it responds correctly — real integration evidence, not
    // just the encoder-level/single-process coverage that existed before.
    let Some(less) = find_less_exe() else {
        eprintln!("skipping: no less.exe found (Git for Windows not detected on this machine)");
        return;
    };
    let dir = scratch_dir("less-jk-space");
    let lines_path = write_numbered_lines(&dir, "LESS_LINE_", 300);
    let script = write_journey(
        &dir,
        r#"[
            {"wait_ms": 500},
            {"key": "j"},
            {"key": "j"},
            {"key": "j"},
            {"wait_ms": 300},
            {"key": "space"},
            {"wait_ms": 500}
        ]"#,
    );
    let mut session = ConSession::spawn(
        &dir,
        &[
            TEST_JOURNEY_ARG,
            script.to_str().unwrap(),
            "-e",
            less.to_str().unwrap(),
            lines_path.to_str().unwrap(),
        ],
    );
    // Deliberately does not first wait to observe the initial (unscrolled)
    // frame: `less` reads all pty-buffered input the instant it enters raw
    // mode, so on an occasional slow-scheduled run it can process the
    // scripted j/j/j/space before this process ever captures a
    // pre-scroll snapshot — a real, observed flake (line 1 genuinely never
    // appears in any polled frame that run), not a hypothetical one. The
    // only fact this test needs is where the view ends up, not that it
    // transiently passed through line 1 on the way there.
    let scrolled = session.wait_for(Duration::from_secs(10), |snapshot| {
        // `less` redraws by clearing then repainting, so an in-flight frame
        // can transiently show a blank top row; wait for a *settled*
        // numbered line, not just "no longer line 1".
        let first_row = snapshot["rows_text"][0].as_str().unwrap_or_default();
        first_row.starts_with("LESS_LINE_") && first_row != "LESS_LINE_1"
    });
    let first_row = scrolled["rows_text"][0].as_str().unwrap_or_default();
    assert!(
        first_row.starts_with("LESS_LINE_"),
        "still expected a LESS_LINE_* row after scrolling, got {first_row:?}: {scrolled}"
    );
    let scrolled_to: u64 = first_row
        .trim_start_matches("LESS_LINE_")
        .parse()
        .unwrap_or(0);
    assert!(
        scrolled_to > 1,
        "3x 'j' + space must have advanced past line 1, top row is {first_row:?}"
    );
    let _ = session.child.kill();
}

#[test]
fn real_tui_less_arrow_keys_and_alt_screen_wheel_scroll() {
    let _guard = gui_test_guard();
    // Companion to `real_tui_less_scrolls_via_character_and_space_keys`:
    // that test proves plain character/space keys reach a real raw-mode TUI.
    // This owns the native-arrow half plus the alternate-screen wheel path,
    // which deliberately translates wheel notches through the same native
    // cursor-key injection contract. Both must advance a real `less` session.
    let Some(less) = find_less_exe() else {
        eprintln!("skipping: no less.exe found (Git for Windows not detected on this machine)");
        return;
    };
    let dir = scratch_dir("less-arrows-wheel");
    let lines_path = write_numbered_lines(&dir, "LESS_LINE_", 300);
    let script = write_journey(
        &dir,
        r#"[
            {"wait_ms": 500},
            {"key": "ArrowDown"},
            {"key": "ArrowDown"},
            {"wait_ms": 300},
            {"wheel": {"row": 5, "col": 5, "notches": -3}},
            {"wait_ms": 500}
        ]"#,
    );
    let mut session = ConSession::spawn(
        &dir,
        &[
            TEST_JOURNEY_ARG,
            script.to_str().unwrap(),
            "-e",
            less.to_str().unwrap(),
            lines_path.to_str().unwrap(),
        ],
    );
    session.wait_for(Duration::from_secs(10), |snapshot| {
        snapshot["rows_text"][0].as_str() == Some("LESS_LINE_1")
    });
    let after = session.wait_for(Duration::from_secs(5), |snapshot| {
        snapshot["rows_text"][0].as_str() != Some("LESS_LINE_1")
    });
    assert_ne!(after["rows_text"][0], "LESS_LINE_1");
    let _ = session.child.kill();
}

#[test]
fn controlled_screenshot_produces_a_valid_nonempty_png() {
    let _guard = gui_test_guard();
    // --emit-snapshot proves text; this proves the *feedback* half the
    // product's north star calls out by name — screenshots, not just
    // structured text — actually exists for agenterm-con specifically. Not
    // a pixel-content assertion (paint_cells's own tests own that); this is
    // "the file exists, decodes, and is the right size," which is what a
    // driving agent needs to trust before it looks at the image at all.
    let dir = scratch_dir("screenshot");
    let png_path = dir.join("out.png");
    let script = write_journey(
        &dir,
        &format!(
            r#"[
                {{"text": "echo SHOT_MARKER\r"}},
                {{"wait_ms": 400}},
                {{"screenshot": {}}},
                {{"wait_ms": 200}}
            ]"#,
            serde_json::to_string(png_path.to_str().unwrap()).unwrap()
        ),
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    session.wait_for(Duration::from_secs(10), |snapshot| {
        ConSession::screen_text(snapshot).contains("SHOT_MARKER")
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    while !png_path.exists() {
        assert!(
            Instant::now() < deadline,
            "screenshot PNG was never written"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    let bytes = std::fs::read(&png_path).expect("read screenshot");
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G']),
        "not a valid PNG signature"
    );
    assert!(
        bytes.len() > 1000,
        "suspiciously small PNG ({} bytes)",
        bytes.len()
    );
    let _ = session.child.kill();
}

#[test]
fn controlled_resize_changes_the_native_render_surface() {
    let _guard = gui_test_guard();
    let dir = scratch_dir("resize-window");
    let before_path = dir.join("before.png");
    let after_path = dir.join("after.png");
    let script = write_journey(
        &dir,
        &format!(
            r#"[
                {{"screenshot": {}}},
                {{"resize": {{"width": 640, "height": 420}}}},
                {{"screenshot": {}}}
            ]"#,
            serde_json::to_string(before_path.to_str().unwrap()).unwrap(),
            serde_json::to_string(after_path.to_str().unwrap()).unwrap()
        ),
    );
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);

    let deadline = Instant::now() + Duration::from_secs(10);
    while !(before_path.exists() && after_path.exists()) {
        assert!(
            Instant::now() < deadline,
            "resize screenshots were not written"
        );
        std::thread::sleep(Duration::from_millis(30));
    }

    fn png_size(path: &Path) -> (u32, u32) {
        let bytes = std::fs::read(path).expect("read screenshot");
        assert!(bytes.len() >= 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        (
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        )
    }

    let before = png_size(&before_path);
    let after = png_size(&after_path);
    assert!(
        after.0 < before.0 && after.1 < before.1,
        "native surface did not shrink: before={before:?}, after={after:?}"
    );
    let _ = session.child.kill();
}

#[test]
fn controlled_resize_storm_reports_successful_frames_and_exits_cleanly() {
    let _guard = gui_test_guard();
    let dir = scratch_dir("resize-perf");
    let png_path = dir.join("fence.png");
    let stats_path = dir.join("perf.json");
    let mut commands = vec![r#"{"reset_perf":true}"#.to_owned()];
    if cfg!(windows) {
        commands.push(r#"{"native_resize_phase":"begin"}"#.to_owned());
    }
    for _ in 0..4 {
        for (width, height) in [(720, 460), (1180, 740), (640, 420), (1100, 680)] {
            commands.push(format!(
                r#"{{"resize":{{"width":{width},"height":{height}}}}}"#
            ));
        }
    }
    if cfg!(windows) {
        commands.push(r#"{"native_resize_phase":"end"}"#.to_owned());
    }
    commands.push(format!(
        r#"{{"screenshot":{}}}"#,
        serde_json::to_string(png_path.to_str().unwrap()).unwrap()
    ));
    commands.push(format!(
        r#"{{"perf_stats":{}}}"#,
        serde_json::to_string(stats_path.to_str().unwrap()).unwrap()
    ));
    commands.push(r#"{"close_window":true}"#.to_owned());
    let script = write_journey(&dir, &format!("[{}]", commands.join(",")));
    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);

    let deadline = Instant::now() + Duration::from_secs(15);
    while !stats_path.exists() {
        assert!(
            Instant::now() < deadline,
            "resize perf stats were not written"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    let stats: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&stats_path).expect("read resize perf stats"))
            .expect("parse resize perf stats");
    eprintln!("agenterm-con resize perf: {stats}");
    let frames = stats["frames"].as_u64().expect("frames");
    assert!(frames > 0, "resize journey rendered no frames");
    if cfg!(windows) {
        let full_frames = stats["full_candidate_frames"]
            .as_u64()
            .expect("full candidate frames");
        let partial_frames = stats["partial_candidate_frames"]
            .as_u64()
            .expect("partial candidate frames");
        assert!(
            partial_frames > 0 && full_frames < frames,
            "live resize never reused a partial raster: {stats}"
        );
        assert!(
            stats["dirty_pixels"].as_u64().expect("dirty pixels")
                < stats["frame_pixels"].as_u64().expect("frame pixels"),
            "live resize rasterized every candidate pixel: {stats}"
        );
    } else {
        assert!(
            frames <= 24,
            "native resize invalidated too many frames: {frames}"
        );
    }
    assert_eq!(stats["observed_frames"].as_u64(), Some(frames));
    assert_eq!(stats["present_failure"].as_u64(), Some(0));
    assert!(stats["present_success"].as_u64().unwrap_or(0) >= frames);

    while session
        .child
        .try_wait()
        .expect("poll controlled host")
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "close-window did not exit the host"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn zooming_in_while_the_shell_is_actively_producing_output_survives() {
    let _guard = gui_test_guard();
    // A second angle on the same user-reported Ctrl+wheel-zoom crash
    // (see `repeated_ctrl_wheel_zoom_cycles_survive_without_crashing`),
    // narrowed by the user's own follow-up: it happens while *enlarging*
    // (not shrinking) the font. Every other zoom test here zooms against
    // an otherwise-idle shell; this one grows the font one notch at a time
    // from the default up toward the max clamp while the shell is
    // continuously printing new lines (including CJK/box-drawing glyphs,
    // in case a specific glyph only mis-renders at a larger size) — real
    // concurrent PTY writes racing real resizes, not an idle session
    // between zoom steps.
    let dir = scratch_dir("grow-while-busy");
    let busy_loop = if cfg!(windows) {
        r#"for /l %i in (1,1,500) do @echo LINE_%i █▒░ 中文日本語\r"#
    } else {
        // bash: continuous CJK/box-drawing output while font zoom races PTY
        // writes — same stress as the Windows for-loop, without cmd.exe.
        r#"for i in $(seq 1 500); do echo \"LINE_$i █▒░ 中文日本語\"; done\r"#
    };
    let mut commands = vec![
        format!(r#"{{"text": "{busy_loop}"}}"#),
        r#"{"wait_ms": 200}"#.to_owned(),
    ];
    for _ in 0..30 {
        commands.push(r#"{"wheel": {"row": 0, "col": 0, "notches": 1}, "ctrl": true}"#.to_owned());
        commands.push(r#"{"wait_ms": 15}"#.to_owned());
    }
    commands.push(r#"{"wait_ms": 500}"#.to_owned());
    let script = write_journey(&dir, &format!("[{}]", commands.join(",")));

    let args = interactive_shell_args(&script);
    let mut session = ConSession::spawn(&dir, &args);
    // Doesn't wait to observe LINE_1 specifically: the loop can outrun the
    // first poll (a real, observed race — see the `less` tests' comments
    // on the same class of issue), and the point here is just to confirm
    // the CJK/box-drawing content is actively flowing while zoom happens,
    // not to catch it at any particular line.
    session.wait_for(Duration::from_secs(15), |snapshot| {
        ConSession::screen_text(snapshot).contains("中文日本語")
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(Some(status)) = session.child.try_wait() {
            panic!(
                "agenterm-con exited on its own while zooming in against a busy shell \
                 (status: {status:?}) — this is the crash under investigation"
            );
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let _ = session.child.kill();
}

#[test]
fn rapid_ctrl_wheel_zoom_burst_against_a_repainting_tui_survives() {
    let _guard = gui_test_guard();
    // A third hypothesis on the same user-reported "Ctrl+wheel zoom
    // crashes" report, and the one that led to an actual fix: it might not
    // be agenterm-con itself panicking, but the *hosted program* struggling
    // under a burst of resize notifications. Before this test prompted it,
    // `zoom_font` fired a real ConPTY resize on *every single notch* with
    // zero debouncing — unlike a window drag-resize, which already goes
    // through `RESIZE_DEBOUNCE`. A hosted program that repaints on every
    // resize (a real TUI, unlike an idle `cmd.exe` prompt) getting a dozen
    // resizes within milliseconds is a real, previously-untested stress
    // shape, and if the child crashed, agenterm-con would correctly (by its
    // own child-exit design) close its window right alongside it — which
    // from the user's side would look exactly like "the terminal window
    // just vanishes, no error."
    //
    // Did not reproduce a crash even before the fix. `zoom_font` now
    // coalesces rapid notches through the same debounce `pending_geometry`
    // mechanism a window drag-resize already used, on general principle —
    // sending a real program a dozen-plus resize notifications in
    // milliseconds because a wheel spun fast was never good behavior on
    // its own merits, confirmed crash or not. This test now exercises
    // exactly that coalescing path against `less`, which actively repaints
    // on resize.
    let Some(less) = find_less_exe() else {
        eprintln!("skipping: no less.exe found");
        return;
    };
    let dir = scratch_dir("less-zoom-burst-probe");
    let lines_path = write_numbered_lines(&dir, "LESS_LINE_", 500);
    let mut commands = vec![r#"{"wait_ms": 500}"#.to_owned()];
    for _ in 0..28 {
        commands.push(r#"{"wheel": {"row": 0, "col": 0, "notches": 1}, "ctrl": true}"#.to_owned());
    }
    commands.push(r#"{"wait_ms": 1000}"#.to_owned());
    let script = write_journey(&dir, &format!("[{}]", commands.join(",")));

    let mut session = ConSession::spawn(
        &dir,
        &[
            TEST_JOURNEY_ARG,
            script.to_str().unwrap(),
            "-e",
            less.to_str().unwrap(),
            lines_path.to_str().unwrap(),
        ],
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last_snapshot = None;
    loop {
        if let Ok(Some(status)) = session.child.try_wait() {
            eprintln!("PROCESS SELF-EXITED: status={status:?}, last snapshot: {last_snapshot:?}");
            panic!(
                "CRASH REPRODUCED: agenterm-con exited on its own during rapid zoom burst against less"
            );
        }
        if let Ok(bytes) = std::fs::read(&session.snapshot_path)
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
        {
            last_snapshot = Some(value);
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    eprintln!("survived: last snapshot = {last_snapshot:?}");
    let _ = session.child.kill();
}

#[test]
fn offline_cli_preserves_unicode_diagnostics_through_redirected_stderr() {
    let output = Command::new(binary())
        .arg("--未知选项")
        .output()
        .expect("run invalid Unicode argument");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr remains UTF-8");
    assert!(stderr.contains("--未知选项"), "{stderr}");
}
