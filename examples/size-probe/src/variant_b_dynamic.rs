//! Size probe — variant B (dynamic).
//!
//! Milestone 15 + 23 + 34: the same probes as variant A, but the mechanism
//! code is NOT statically linked. This binary depends only on `libloading`
//! (plus std); every capability comes from the `libagenterm` cdylib's C
//! exports, loaded at run time. If the mechanism code were statically linked
//! too, variant B's artifact would be much larger than it is.
//!
//! Milestone 23 adds the two biggest mechanisms: `agt_window_open` and
//! `agt_pty_open`/`wait`/`close` (spawn `cmd.exe /c exit` / `/bin/sh -c exit`
//! and reap it immediately). Both may report AGT_UNSUPPORTED or AGT_FAILED on
//! headless hosts; the point is that the calls route through the dylib and
//! nothing statically enters this artifact.
//! Milestone 34 adds the remaining exports: `agt_screenshot_write_png` /
//! `agt_screenshot_capture_window`, `agt_a11y_tree_snapshot`, and
//! `agt_window_event_text` (served from the opened window, after
//! `agt_window_open` succeeds).

use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
use std::process::ExitCode;

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;

/// C-compatible process record (mirror of `agt_process_info` in
/// include/agenterm.h). Only the layout matters; the fields are never read
/// by this probe.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct agt_process_info {
    id: u32,
    parent_id: u32,
    name: [u8; 64],
    name_len: u32,
    name_truncated: u32,
}

impl Default for agt_process_info {
    fn default() -> Self {
        agt_process_info {
            id: 0,
            parent_id: 0,
            name: [0u8; 64],
            name_len: 0,
            name_truncated: 0,
        }
    }
}

/// C-compatible window creation parameters (mirror of `agt_window_spec`).
#[repr(C)]
#[derive(Clone, Copy)]
struct agt_window_spec {
    title: *const c_char,
    width: u32,
    height: u32,
    no_activate: i32,
    ime_allowed: i32,
}

/// C-compatible PTY spawn parameters (mirror of `agt_pty_spawn`).
#[repr(C)]
#[derive(Clone, Copy)]
struct agt_pty_spawn {
    program: *const c_char,
    argv: *const *const c_char,
    argc: usize,
    cwd: *const c_char,
    envp: *const *const c_char,
    envc: usize,
    cols: u16,
    rows: u16,
}

type AbiVersion = unsafe extern "C" fn() -> u32;
/// Both two-stage UTF-8 exports (`agt_runtime_user_config_dir` and
/// `agt_runtime_default_shell`) share this exact FFI signature.
type RuntimeTwoStageUtf8 = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type ClipboardHasText = unsafe extern "C" fn() -> i32;
type ProcessList = unsafe extern "C" fn(*mut agt_process_info, usize, *mut usize) -> i32;
type ParentConsoleWriteStdout = unsafe extern "C" fn(*const u8, usize) -> i32;
type WindowOpen = unsafe extern "C" fn(*const agt_window_spec, *mut *mut u8) -> i32;
type WindowClose = unsafe extern "C" fn(*mut u8) -> i32;
type PtyOpen = unsafe extern "C" fn(*const agt_pty_spawn, *mut *mut u8) -> i32;
type PtyWait = unsafe extern "C" fn(*mut u8, u32, *mut i32) -> i32;
type PtyClose = unsafe extern "C" fn(*mut u8) -> i32;
type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;
/// `agt_screenshot_write_png(path, pixels, pixel_count, width, height)`.
type ScreenshotWritePng = unsafe extern "C" fn(*const c_char, *const u32, usize, u32, u32) -> i32;
/// `agt_screenshot_capture_window(native_window, path, area_kind, l, t, w, h)`.
type ScreenshotCaptureWindow =
    unsafe extern "C" fn(isize, *const c_char, i32, i32, i32, i32, i32) -> i32;
/// `agt_a11y_tree_snapshot(window_handle, *out_node_count)`.
type A11yTreeSnapshot = unsafe extern "C" fn(isize, *mut usize) -> i32;
/// `agt_window_event_text(window, buf, cap, *out_len)` — two-stage UTF-8.
type WindowEventText = unsafe extern "C" fn(*mut u8, *mut u8, usize, *mut usize) -> i32;

/// C-compatible last-error record (mirror of `agt_error`); only `code` is
/// read by this probe, for diagnostics.
#[repr(C)]
#[derive(Clone, Copy)]
struct agt_error {
    operation: *const c_char,
    code: *const c_char,
    message: *const c_char,
}

/// Last recorded failure code, or `None` when `agt_last_error` itself fails.
fn last_error_code(lib: &Library) -> Option<String> {
    let f: Symbol<LastError> = sym(lib, b"agt_last_error").ok()?;
    let mut e = agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    if unsafe { f(&mut e) } != AGT_OK || e.code.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(e.code) }
            .to_string_lossy()
            .into_owned(),
    )
}

const CDYLIB_NAMES: [&str; 3] = [
    "agenterm.dll",      // Windows
    "libagenterm.so",    // Linux
    "libagenterm.dylib", // macOS
];

/// Candidate locations relative to each ancestor of the probe executable.
/// `abi-release` / `abi-dev` are the libagenterm unwind profiles; `release`
/// / `debug` cover a dylib staged into the same profile directory.
const REL_CANDIDATES: [&str; 5] = ["", "abi-release/", "abi-dev/", "release/", "debug/"];

fn main() -> ExitCode {
    println!("size-probe variant B (dynamic: libagenterm cdylib via libloading)");
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("size-probe variant B: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let path = find_cdylib().ok_or(
        "libagenterm cdylib not found. Build it first, e.g. \
         `cargo build -p agenterm-abi --profile abi-release`, or set \
         AGENTERM_ABI_LIB to its path",
    )?;
    let lib = unsafe { Library::new(&path) }
        .map_err(|e| format!("LoadLibrary/dlopen({path:?}) failed: {e}"))?;

    // 1. user config dir length (two-stage; never print its content)
    let config_dir_len = two_stage_utf8_len(&lib, b"agt_runtime_user_config_dir")?;

    // 2. default shell length (two-stage; never print its content)
    let shell_len = two_stage_utf8_len(&lib, b"agt_runtime_default_shell")?;

    // 3. clipboard: does it hold Unicode text? (bool only)
    let has_text: Symbol<ClipboardHasText> = sym(&lib, b"agt_clipboard_has_text")?;
    let clipboard_has_text = unsafe { has_text() } != 0;

    // 4. process list entry count (two-stage; no big speculative allocation)
    let process_count = process_count(&lib)?;

    // 5. write one short line to the parent console
    let write: Symbol<ParentConsoleWriteStdout> = sym(&lib, b"agt_parent_console_write_stdout")?;
    let line = b"size-probe[variant B] parent-console write ok";
    let parent_console = match unsafe { write(line.as_ptr(), line.len()) } {
        AGT_OK => "ok",
        AGT_UNSUPPORTED => "unsupported",
        _ => "failed",
    };

    // 6. window: open-then-close probe (milestone 23). AGT_UNSUPPORTED or
    //    AGT_FAILED is acceptable on headless hosts; on a real desktop the
    //    window opens without activation and is closed immediately.
    //    Milestone 34: when the window opens, the input event-text path
    //    (`agt_window_event_text`) is probed on the live handle first.
    let (window_open, event_text_len) = window_probe(&lib)?;

    // 7. pty: spawn the shortest-lived child, wait for it, close (milestone
    //    23). Failure is acceptable too; the exports must merely be resolved
    //    and routed through the dylib.
    let pty_open = pty_probe(&lib)?;

    // 8. screenshot (milestone 34): encode a 1x1 PNG into `std::env::temp_dir()`
    //    and delete it (never write into the repository tree), plus the
    //    window-capture export with `native_window == 0` (parameter
    //    validation only, no file produced).
    let screenshot = screenshot_probe(&lib)?;
    let screenshot_capture = screenshot_capture_probe(&lib)?;

    // 9. a11y (milestone 34): capture the accessibility tree for all roots.
    //    AGT_UNSUPPORTED is perfectly acceptable on hosts without a stack.
    let a11y = a11y_probe(&lib)?;

    // 10. ABI version from the dylib export
    let abi_version: Symbol<AbiVersion> = sym(&lib, b"agt_abi_version")?;
    let abi_version = unsafe { abi_version() };

    println!("user_config_dir_len={config_dir_len}");
    println!("default_shell_len={shell_len}");
    println!("clipboard_has_text={clipboard_has_text}");
    println!("process_count={process_count}");
    println!("parent_console_write_stdout={parent_console}");
    println!("window_open={window_open}");
    println!("pty_open={pty_open}");
    println!("screenshot={screenshot}");
    println!("screenshot_capture={screenshot_capture}");
    println!("a11y={a11y}");
    println!("event_text_len={event_text_len}");
    println!("abi_version={abi_version}");
    Ok(())
}

/// `agt_window_open` with a no-activate title, then `agt_window_event_text`
/// on the live handle (milestone 34), then `agt_window_close`. Returns a
/// short status string and the event-text length (never the text itself).
fn window_probe(lib: &Library) -> Result<(String, String), String> {
    let window_open: Symbol<WindowOpen> = sym(lib, b"agt_window_open")?;
    let title: &CStr = c"size-probe";
    let spec = agt_window_spec {
        title: title.as_ptr(),
        width: 320,
        height: 200,
        no_activate: 1,
        ime_allowed: 0,
    };
    let mut window: *mut u8 = std::ptr::null_mut();
    match unsafe { window_open(&spec, &mut window) } {
        AGT_OK => {
            let event_text_len = window_event_text_len(lib, window);
            let window_close: Symbol<WindowClose> = sym(lib, b"agt_window_close")?;
            unsafe { window_close(window) };
            Ok(("ok".to_string(), event_text_len?))
        }
        AGT_UNSUPPORTED => Ok(("unsupported".to_string(), "unavailable".to_string())),
        other => Ok((format!("failed(status={other})"), "unavailable".to_string())),
    }
}

/// Two-stage `agt_window_event_text` on a live window handle. `cap == 0` is
/// the canonical "how big?" probe (returns AGT_FAILED + required length);
/// the second stage reads the text and reports only its byte length.
fn window_event_text_len(lib: &Library, window: *mut u8) -> Result<String, String> {
    let f: Symbol<WindowEventText> = sym(lib, b"agt_window_event_text")?;
    let mut required = 0usize;
    let s = unsafe { f(window, std::ptr::null_mut(), 0, &mut required) };
    match s {
        AGT_OK => Ok(format!("len={required}")),
        AGT_UNSUPPORTED => Ok("unsupported".to_string()),
        AGT_FAILED => {
            if required == 0 {
                // First-stage probe succeeded: the window has no staged IME
                // text, so the required length is zero. Report it directly
                // (a second call with cap == 0 would re-trigger
                // buffer_too_small by design).
                return Ok("len=0".to_string());
            }
            let mut buf = vec![0u8; required];
            let mut written = 0usize;
            let s2 = unsafe { f(window, buf.as_mut_ptr(), buf.len(), &mut written) };
            if s2 == AGT_OK {
                Ok(format!("len={written}"))
            } else {
                Ok(format!("failed(read_status={s2}, required={required})"))
            }
        }
        other => Ok(format!("failed(status={other})")),
    }
}

/// `agt_screenshot_write_png` encoding a 1x1 framebuffer into
/// `std::env::temp_dir()` and deleting it immediately. A real PNG encode
/// routes through the dylib; only the status is returned.
fn screenshot_probe(lib: &Library) -> Result<String, String> {
    let f: Symbol<ScreenshotWritePng> = sym(lib, b"agt_screenshot_write_png")?;
    let path =
        std::env::temp_dir().join(format!("size-probe-variant-b-{}.png", std::process::id()));
    let path_c = CString::new(path.to_str().ok_or("temp path is not UTF-8")?)
        .map_err(|e| format!("temp path contains NUL: {e}"))?;
    let pixels: [u32; 1] = [0xFF0000];
    let status = unsafe { f(path_c.as_ptr(), pixels.as_ptr(), 1, 1, 1) };
    let _ = std::fs::remove_file(&path);
    match status {
        AGT_OK => Ok("ok(pixels=1)".to_string()),
        AGT_UNSUPPORTED => Ok("unsupported".to_string()),
        other => Ok(format!("failed(status={other})")),
    }
}

/// `agt_screenshot_capture_window` with `native_window == 0` — the export
/// returns AGT_FAILED{bad_handle} without touching the filesystem. The point
/// is that the symbol resolves and routes through the dylib.
fn screenshot_capture_probe(lib: &Library) -> Result<String, String> {
    let f: Symbol<ScreenshotCaptureWindow> = sym(lib, b"agt_screenshot_capture_window")?;
    let status = unsafe { f(0, std::ptr::null(), 0, 0, 0, 0, 0) };
    match status {
        AGT_UNSUPPORTED => Ok("unsupported".to_string()),
        AGT_OK => Ok("ok".to_string()),
        other => Ok(format!("failed(status={other})")),
    }
}

/// `agt_a11y_tree_snapshot` over all application roots. AGT_UNSUPPORTED is
/// acceptable on hosts without an accessibility stack.
fn a11y_probe(lib: &Library) -> Result<String, String> {
    let f: Symbol<A11yTreeSnapshot> = sym(lib, b"agt_a11y_tree_snapshot")?;
    let mut count = 0usize;
    match unsafe { f(0, &mut count) } {
        AGT_OK => Ok(format!("ok(nodes={count})")),
        AGT_UNSUPPORTED => Ok("unsupported".to_string()),
        other => Ok(format!("failed(status={other})")),
    }
}

/// `agt_pty_open` (`cmd.exe /c exit` on Windows, `/bin/sh -c exit` on Unix),
/// then `agt_pty_wait` and `agt_pty_close`. Returns a short status string
/// with the wait outcome when the spawn succeeded.
fn pty_probe(lib: &Library) -> Result<String, String> {
    let pty_open: Symbol<PtyOpen> = sym(lib, b"agt_pty_open")?;
    let program: &CStr = if cfg!(windows) {
        c"cmd.exe"
    } else {
        c"/bin/sh"
    };
    let arg0: &CStr = if cfg!(windows) { c"/c" } else { c"-c" };
    let arg1: &CStr = c"exit";
    // ABI convention: argv[0] is the program name and is not re-passed as an
    // argument; the library uses argv[1..argc] (same as the dylib acceptance
    // tests). argc == 3 → args = ["/c", "exit"].
    let argv: [*const c_char; 3] = [program.as_ptr(), arg0.as_ptr(), arg1.as_ptr()];
    let spawn = agt_pty_spawn {
        program: program.as_ptr(),
        argv: argv.as_ptr(),
        argc: 3,
        cwd: std::ptr::null(),
        envp: std::ptr::null(),
        envc: 0,
        cols: 80,
        rows: 24,
    };
    let mut pty: *mut u8 = std::ptr::null_mut();
    match unsafe { pty_open(&spawn, &mut pty) } {
        AGT_OK => {
            let pty_wait: Symbol<PtyWait> = sym(lib, b"agt_pty_wait")?;
            let mut exit_code: i32 = -1;
            let wait_status = unsafe { pty_wait(pty, 5000, &mut exit_code) };
            let wait_note = if wait_status == AGT_OK {
                format!("exit_code={exit_code}")
            } else {
                match last_error_code(lib) {
                    Some(code) => format!("wait_status={wait_status}, error_code={code}"),
                    None => format!("wait_status={wait_status}"),
                }
            };
            let pty_close: Symbol<PtyClose> = sym(lib, b"agt_pty_close")?;
            unsafe { pty_close(pty) };
            Ok(format!("ok({wait_note})"))
        }
        AGT_UNSUPPORTED => Ok("unsupported".to_string()),
        other => Ok(format!("failed(status={other})")),
    }
}

/// Fetch a symbol by name.
fn sym<'l, T>(lib: &'l Library, name: &[u8]) -> Result<Symbol<'l, T>, String> {
    unsafe { lib.get(name) }
        .map_err(|e| format!("symbol {} missing: {e}", String::from_utf8_lossy(name)))
}

/// Two-stage UTF-8 probe (spec 3.4): call with `cap == 0` to learn the
/// required length, then allocate exactly that and call again. Returns the
/// number of bytes written. Never returns the content itself.
fn two_stage_utf8_len(lib: &Library, name: &[u8]) -> Result<usize, String> {
    let f: Symbol<RuntimeTwoStageUtf8> = sym(lib, name)?;
    let mut required = 0usize;
    // cap == 0 is the canonical "how big?" probe; buffer_too_small is expected.
    let s = unsafe { f(std::ptr::null_mut(), 0, &mut required) };
    if s != AGT_OK && s != AGT_FAILED {
        return Err(format!("{name:?} probe: unexpected status {s}"));
    }
    if s == AGT_OK {
        return Ok(required); // empty result
    }
    let mut buf = vec![0u8; required];
    let mut written = 0usize;
    let s = unsafe { f(buf.as_mut_ptr(), buf.len(), &mut written) };
    if s != AGT_OK {
        return Err(format!("{name:?} read: status {s}"));
    }
    Ok(written)
}

/// Two-stage process-list probe: `cap == 0` learns the required record
/// count, then the caller allocates exactly that many records and calls
/// again. Returns the written record count.
fn process_count(lib: &Library) -> Result<usize, String> {
    let f: Symbol<ProcessList> = sym(lib, b"agt_process_list")?;
    let mut required = 0usize;
    let s = unsafe { f(std::ptr::null_mut(), 0, &mut required) };
    if s != AGT_OK && s != AGT_FAILED {
        return Err(format!("agt_process_list probe: unexpected status {s}"));
    }
    if s == AGT_OK {
        return Ok(required); // empty result
    }
    let mut buf = vec![agt_process_info::default(); required];
    let mut count = 0usize;
    let s = unsafe { f(buf.as_mut_ptr(), buf.len(), &mut count) };
    if s != AGT_OK {
        return Err(format!("agt_process_list read: status {s}"));
    }
    Ok(count)
}

/// Locate the libagenterm cdylib: `AGENTERM_ABI_LIB` wins, then walk up
/// from the probe executable looking in candidate profile directories under
/// each ancestor. This mirrors how the dylib acceptance test finds the
/// cdylib, but also covers profiles in different directories.
fn find_cdylib() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AGENTERM_ABI_LIB") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut dir: PathBuf = exe.parent()?.to_path_buf();
    loop {
        for rel in REL_CANDIDATES {
            let base = if rel.is_empty() {
                dir.clone()
            } else {
                dir.join(rel)
            };
            for name in CDYLIB_NAMES {
                let candidate = base.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}
