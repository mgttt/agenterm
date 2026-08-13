//! Milestone acceptance regression: really load the built cdylib and call the
//! exports through the FFI, proving (a) every returned `const char*` is a
//! NUL-terminated C string (defect 1), (b) the fence actually ships because
//! this test only builds under an unwind profile (defect 2), and (c) the PTY
//! mechanism performs a real end-to-end round trip (milestone 2).
//!
//! If the cdylib cannot be located the test FAILS on purpose — silently
//! skipping would leave the defects unproven.

use agenterm::{ABI_MAJOR, ABI_MINOR};
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
struct agt_error {
    operation: *const c_char,
    code: *const c_char,
    message: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
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

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
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

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
struct agt_window_info {
    handle: isize,
    process_id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    focused: i32,
    minimized: i32,
    title: [u8; 128],
    title_len: u32,
    title_truncated: u32,
    app_name: [u8; 64],
    app_name_len: u32,
    app_name_truncated: u32,
}

impl Default for agt_window_info {
    fn default() -> Self {
        agt_window_info {
            handle: 0,
            process_id: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            focused: 0,
            minimized: 0,
            title: [0u8; 128],
            title_len: 0,
            title_truncated: 0,
            app_name: [0u8; 64],
            app_name_len: 0,
            app_name_truncated: 0,
        }
    }
}

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;
const AGT_CAP_PTY: i32 = 1;
const AGT_CAP_PROCESS_SPAWN: i32 = 2;
const AGT_CAP_PROCESS_OBSERVE: i32 = 3;
const AGT_CAP_WINDOW_HOST: i32 = 4;
const AGT_CAP_WINDOW_ENUMERATE: i32 = 5;
const AGT_CAP_WINDOW_OP: i32 = 6;
const AGT_CAP_SCREENSHOT: i32 = 7;
const AGT_CAP_CLIPBOARD: i32 = 8;
const AGT_CAP_INPUT_INJECT: i32 = 10;
const AGT_CAP_PARENT_CONSOLE: i32 = 15;
const AGT_EV_NONE: u32 = 0;
const AGT_EV_RENDER_DUE: u32 = 4;
const AGT_CAP_ACCESSIBILITY_TREE: i32 = 16;
const PROBE: &[u8] = b"agenterm-abi-probe";

/// PNG signature: `89 50 4E 47 0D 0A 1A 0A`.
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

type PtyOpen = unsafe extern "C" fn(*const agt_pty_spawn, *mut *mut std::ffi::c_void) -> i32;
type PtyRead = unsafe extern "C" fn(*mut std::ffi::c_void, *mut u8, usize, *mut usize) -> i32;
type PtyWait = unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut i32) -> i32;
type PtyClose = unsafe extern "C" fn(*mut std::ffi::c_void);
type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;
type ScreenshotWritePng = unsafe extern "C" fn(*const c_char, *const u32, usize, u32, u32) -> i32;
type ScreenshotCaptureWindow =
    unsafe extern "C" fn(isize, *const c_char, i32, i32, i32, i32, i32) -> i32;
type ProcessList = unsafe extern "C" fn(*mut agt_process_info, usize, *mut usize) -> i32;
type ProcessKill = unsafe extern "C" fn(u32) -> i32;
type ProcessSelf = unsafe extern "C" fn() -> u32;
type WindowEnumerate = unsafe extern "C" fn(*mut agt_window_info, usize, *mut usize) -> i32;
type NativeWindowShow = unsafe extern "C" fn(isize, i32) -> i32;
type InputPointerClick = unsafe extern "C" fn(i32, i32, i32, u32) -> i32;
type InputTypeText = unsafe extern "C" fn(*const u8, usize) -> i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(non_camel_case_types)]
struct agt_screen_info {
    frame_x: i32,
    frame_y: i32,
    frame_width: u32,
    frame_height: u32,
    visible_x: i32,
    visible_y: i32,
    visible_width: u32,
    visible_height: u32,
    primary: i32,
}

type ScreenList = unsafe extern "C" fn(*mut agt_screen_info, usize, *mut usize) -> i32;
type A11yDrainBus = unsafe extern "C" fn() -> i32;
type A11yLastTextWriteVia = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct agt_a11y_node {
    bounds_x: i32,
    bounds_y: i32,
    bounds_width: i32,
    bounds_height: i32,
    id: [u8; 64],
    id_len: u32,
    id_truncated: u32,
    parent_id: [u8; 64],
    parent_id_len: u32,
    parent_id_truncated: u32,
    has_parent: u8,
    actions_count: u32,
}

impl Default for agt_a11y_node {
    fn default() -> Self {
        agt_a11y_node {
            bounds_x: 0,
            bounds_y: 0,
            bounds_width: 0,
            bounds_height: 0,
            id: [0u8; 64],
            id_len: 0,
            id_truncated: 0,
            parent_id: [0u8; 64],
            parent_id_len: 0,
            parent_id_truncated: 0,
            has_parent: 0,
            actions_count: 0,
        }
    }
}

type A11yTreeSnapshot = unsafe extern "C" fn(isize, *mut usize) -> i32;
type A11yTreeMetaString = unsafe extern "C" fn(i32, *mut u8, usize, *mut usize) -> i32;
type A11yTreeNode = unsafe extern "C" fn(usize, *mut agt_a11y_node) -> i32;
type A11yNodeString = unsafe extern "C" fn(usize, i32, *mut u8, usize, *mut usize) -> i32;
type A11yNodePerform = unsafe extern "C" fn(isize, *const c_char, i32) -> i32;
type A11yNodeSetText = unsafe extern "C" fn(isize, *const c_char, *const u8, usize) -> i32;
type A11yNodeGetText =
    unsafe extern "C" fn(isize, *const c_char, *mut u8, usize, *mut usize) -> i32;
type A11yNodeSendKeys = unsafe extern "C" fn(isize, *const c_char, *const u8, usize) -> i32;
type ClipboardSetText = unsafe extern "C" fn(*const u8, usize) -> i32;
type ClipboardGetText = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type ClipboardHasText = unsafe extern "C" fn() -> i32;
type ParentConsoleWrite = unsafe extern "C" fn(*const u8, usize) -> i32;
type RuntimeUserConfigDir = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeDefaultShell = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeEnvPresent = unsafe extern "C" fn(*const u8, usize) -> i32;
type RuntimeArgCount = unsafe extern "C" fn(*mut usize) -> i32;
type RuntimeArg = unsafe extern "C" fn(usize, *mut u8, usize, *mut usize) -> i32;

/// Locate the cdylib built under the active profile. The test binary lives in
/// `target/<profile>/deps/`, the cdylib in `target/<profile>/`.
fn cdylib_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe()");
    let deps = exe.parent().expect("test binary has a parent dir");
    let profile_dir = deps.parent().expect("deps dir has a parent dir");
    const CANDIDATES: [&str; 3] = [
        "agenterm.dll",      // Windows
        "libagenterm.so",    // Linux
        "libagenterm.dylib", // macOS
    ];
    for dir in [profile_dir, deps] {
        for name in CANDIDATES {
            let p = dir.join(name);
            if p.exists() {
                return p;
            }
        }
    }
    panic!(
        "agenterm-abi cdylib not found under {} (candidates: {CANDIDATES:?}). \
         Build it with an unwind profile first, e.g. \
         `cargo build -p agenterm-abi --profile abi-dev`",
        profile_dir.display()
    );
}

/// Load the cdylib and leak the `Library` handle: the DLL's private threads
/// (PTY reaper, window-loop thread) may still be winding down when a test
/// function returns, so dropping the handle here would `FreeLibrary` the
/// module out from under them and crash the process at exit
/// (0xc000041d). Leaking keeps the module resident for the whole test
/// process lifetime — the OS reclaims it at process teardown.
fn load() -> &'static Library {
    let path = cdylib_path();
    let lib = unsafe { Library::new(&path) }
        .unwrap_or_else(|e| panic!("dlopen/LoadLibrary({path:?}) failed: {e}"));
    Box::leak(Box::new(lib))
}

unsafe fn sym<'l, T>(lib: &'l Library, name: &[u8]) -> Symbol<'l, T> {
    unsafe { lib.get(name) }.unwrap_or_else(|e| panic!("symbol {name:?} missing: {e}"))
}

/// Read the thread-local error record as `operation: code: message`.
fn last_error_message(lib: &Library) -> String {
    let f: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
    let mut e = agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    if unsafe { f(&mut e) } != AGT_OK {
        return "<agt_last_error failed>".to_owned();
    }
    let op = unsafe { CStr::from_ptr(e.operation) }.to_string_lossy();
    let code = unsafe { CStr::from_ptr(e.code) }.to_string_lossy();
    let msg = unsafe { CStr::from_ptr(e.message) }.to_string_lossy();
    format!("{op}: {code}: {msg}")
}

/// (program, args) that prints PROBE and exits 0. argv[0] is the program name.
fn pty_echo_probe_program() -> (&'static str, Vec<&'static str>) {
    #[cfg(windows)]
    {
        ("cmd.exe", vec!["/c", "echo agenterm-abi-probe"])
    }
    #[cfg(not(windows))]
    {
        ("/bin/sh", vec!["-c", "echo agenterm-abi-probe"])
    }
}

/// (program, args) that runs for ~30 s (long enough to outlive any wait).
fn pty_long_running_program() -> (&'static str, Vec<&'static str>) {
    #[cfg(windows)]
    {
        ("cmd.exe", vec!["/c", "ping -n 30 127.0.0.1 > nul"])
    }
    #[cfg(not(windows))]
    {
        ("/bin/sh", vec!["-c", "sleep 30"])
    }
}

/// Spawn a PTY for `(program, args)` and return the opaque handle. Panics on
/// any failure (the test must fail, never skip).
fn open_pty(
    lib: &Library,
    open: &Symbol<PtyOpen>,
    program: &str,
    args: &[&str],
) -> *mut std::ffi::c_void {
    let program_c = CString::new(program).expect("program has no NUL");
    let arg_c: Vec<CString> = args
        .iter()
        .map(|a| CString::new(*a).expect("arg has no NUL"))
        .collect();
    let mut argv: Vec<*const c_char> = Vec::with_capacity(1 + arg_c.len());
    argv.push(program_c.as_ptr());
    argv.extend(arg_c.iter().map(|a| a.as_ptr()));
    let spawn = agt_pty_spawn {
        program: program_c.as_ptr(),
        argv: argv.as_ptr(),
        argc: argv.len(),
        cwd: std::ptr::null(),
        envp: std::ptr::null(),
        envc: 0,
        cols: 80,
        rows: 24,
    };
    let mut pty: *mut std::ffi::c_void = std::ptr::null_mut();
    let st = unsafe { open(&spawn, &mut pty) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_pty_open failed: {}",
        last_error_message(lib)
    );
    assert!(!pty.is_null(), "agt_pty_open returned a null handle");
    pty
}

#[test]
fn abi_version_encodes_major_and_minor() {
    let lib = load();
    let f: Symbol<unsafe extern "C" fn() -> u32> = unsafe { sym(lib, b"agt_abi_version") };
    let v = unsafe { f() };
    // Anti-drift gate: the encoded value must match the crate's own ABI
    // constants, so a bump in `abi_version!` cannot leave a stale literal
    // behind in the test.
    assert_eq!(
        v >> 16,
        ABI_MAJOR as u32,
        "high 16 bits must equal ABI_MAJOR ({ABI_MAJOR})"
    );
    assert_eq!(
        v & 0xffff,
        ABI_MINOR as u32,
        "low 16 bits must equal ABI_MINOR ({ABI_MINOR})"
    );
}

#[test]
fn build_id_is_a_valid_nul_terminated_utf8_c_string() {
    let lib = load();
    let f: Symbol<unsafe extern "C" fn() -> *const c_char> = unsafe { sym(lib, b"agt_build_id") };
    let p = unsafe { f() };
    assert!(!p.is_null(), "agt_build_id returned NULL");
    // Defect-1 regression gate: the pointer must be readable as a C string
    // (CStr::from_ptr proves the trailing NUL), and must be valid UTF-8.
    let s = unsafe { CStr::from_ptr(p) };
    assert!(
        std::str::from_utf8(s.to_bytes()).is_ok(),
        "agt_build_id must be valid UTF-8, got bytes {:?}",
        s.to_bytes()
    );
    // Anti-drift gate 1: the id must start with the crate version (derived
    // from CARGO_PKG_VERSION at compile time, so a version bump never leaves
    // a stale hand-written literal behind).
    let text = s.to_str().expect("build id is UTF-8");
    assert!(
        text.starts_with(env!("CARGO_PKG_VERSION")),
        "agt_build_id must start with CARGO_PKG_VERSION ({}), got: {text:?}",
        env!("CARGO_PKG_VERSION")
    );
    // Anti-drift gate 2: the `+abi.<major>.<minor>` suffix must match the
    // exported ABI constants.
    let expected_suffix = format!("+abi.{ABI_MAJOR}.{ABI_MINOR}");
    let suffix = text
        .strip_prefix(env!("CARGO_PKG_VERSION"))
        .unwrap_or_default();
    assert_eq!(
        suffix, expected_suffix,
        "agt_build_id suffix must match ABI_MAJOR/ABI_MINOR, got: {text:?}"
    );
}

#[test]
fn capability_query_reports_pty_ok_others_unsupported() {
    let lib = load();
    let f: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    // Milestone 2 ships the PTY mechanism → AGT_OK.
    assert_eq!(unsafe { f(AGT_CAP_PTY) }, AGT_OK);
    // Milestone 3a ships the window host mechanism → AGT_OK on Windows/Linux.
    // macOS is the deliberate exception: the window host is AGT_UNSUPPORTED
    // because AppKit requires the main thread, which an FFI entry point called
    // from an arbitrary thread cannot guarantee — milestone 22 implemented that
    // as an explicit platform contract. Assert exactly per platform so the
    // Windows/Linux AGT_OK requirement is never silently dropped.
    let window_host_status = unsafe { f(AGT_CAP_WINDOW_HOST) };
    if cfg!(target_os = "macos") {
        assert_eq!(
            window_host_status, AGT_UNSUPPORTED,
            "AGT_CAP_WINDOW_HOST must be AGT_UNSUPPORTED on macOS (AppKit main-thread requirement, milestone 22); got {window_host_status}"
        );
    } else {
        assert_eq!(
            window_host_status,
            AGT_OK,
            "AGT_CAP_WINDOW_HOST must be AGT_OK on {} (milestone 3a); got {window_host_status}",
            std::env::consts::OS
        );
    }
    // Milestone 4 ships the screenshot mechanism → AGT_OK.
    assert_eq!(unsafe { f(AGT_CAP_SCREENSHOT) }, AGT_OK);
    // Milestone 5 ships process observation → AGT_OK.
    assert_eq!(unsafe { f(AGT_CAP_PROCESS_OBSERVE) }, AGT_OK);
    // Milestone 8 ships the clipboard mechanism → AGT_OK.
    assert_eq!(unsafe { f(AGT_CAP_CLIPBOARD) }, AGT_OK);
    // Milestone 9 ships the parent-console write mechanism → AGT_OK.
    assert_eq!(unsafe { f(AGT_CAP_PARENT_CONSOLE) }, AGT_OK);
    // Mechanisms not yet shipped stay AGT_UNSUPPORTED (never AGT_FAILED).
    assert_eq!(unsafe { f(AGT_CAP_PROCESS_SPAWN) }, AGT_UNSUPPORTED);
}

#[test]
fn last_error_fields_are_readable_c_strings() {
    let lib = load();
    let f: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
    let mut e = agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    let st = unsafe { f(&mut e) };
    assert_eq!(st, AGT_OK);
    assert!(!e.operation.is_null());
    assert!(!e.code.is_null());
    assert!(!e.message.is_null());
    let op = unsafe { CStr::from_ptr(e.operation) }.to_bytes();
    let code = unsafe { CStr::from_ptr(e.code) }.to_bytes();
    let msg = unsafe { CStr::from_ptr(e.message) }.to_bytes();
    // Fresh thread, nothing failed yet: the "no error" record must round-trip.
    assert_eq!(op, b"none");
    assert_eq!(code, b"ok");
    assert_eq!(msg, b"no error");
}

#[test]
fn last_error_accepts_null_out_without_crashing() {
    let lib = load();
    let f: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
    assert_eq!(unsafe { f(std::ptr::null_mut()) }, AGT_FAILED);
}

/// Real PTY round trip (milestone 2 evidence): spawn `cmd.exe /c echo probe`
/// (or `/bin/sh -c` on Unix), read until the probe bytes arrive, wait for exit
/// code 0, close cleanly.
#[test]
fn pty_roundtrip_echo_probe() {
    let lib = load();
    let open: Symbol<PtyOpen> = unsafe { sym(lib, b"agt_pty_open") };
    let read: Symbol<PtyRead> = unsafe { sym(lib, b"agt_pty_read") };
    let wait: Symbol<PtyWait> = unsafe { sym(lib, b"agt_pty_wait") };
    let close: Symbol<PtyClose> = unsafe { sym(lib, b"agt_pty_close") };

    let (program, args) = pty_echo_probe_program();
    let pty = open_pty(lib, &open, program, &args);

    // Blocking read loop until the probe is seen or EOF (15 s cap).
    let mut collected = Vec::new();
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for probe; collected so far: {:?}",
            String::from_utf8_lossy(&collected)
        );
        let mut n = 0usize;
        let st = unsafe { read(pty, buf.as_mut_ptr(), buf.len(), &mut n) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_pty_read failed: {}",
            last_error_message(lib)
        );
        if n == 0 {
            break; // EOF
        }
        collected.extend_from_slice(&buf[..n]);
        if collected.windows(PROBE.len()).any(|w| w == PROBE) {
            break;
        }
    }
    assert!(
        collected.windows(PROBE.len()).any(|w| w == PROBE),
        "probe not found in PTY output: {:?}",
        String::from_utf8_lossy(&collected)
    );

    let mut code: i32 = -999;
    let st = unsafe { wait(pty, 10_000, &mut code) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_pty_wait failed: {}",
        last_error_message(lib)
    );
    assert_eq!(code, 0, "expected exit code 0, got {code}");

    unsafe { close(pty) };
}

/// `agt_pty_wait` with a small timeout against a long-running process must
/// return AGT_FAILED with code "timeout" (never AGT_UNSUPPORTED, never hang).
/// Closing must then terminate the long-running child cleanly.
#[test]
fn pty_wait_times_out_for_a_long_running_process() {
    let lib = load();
    let open: Symbol<PtyOpen> = unsafe { sym(lib, b"agt_pty_open") };
    let wait: Symbol<PtyWait> = unsafe { sym(lib, b"agt_pty_wait") };
    let close: Symbol<PtyClose> = unsafe { sym(lib, b"agt_pty_close") };

    let (program, args) = pty_long_running_program();
    let pty = open_pty(lib, &open, program, &args);

    let started = Instant::now();
    let mut code: i32 = -999;
    let st = unsafe { wait(pty, 50, &mut code) };
    let elapsed = started.elapsed();
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("timeout"),
        "expected code \"timeout\" in error, got: {msg}"
    );
    // 50 ms timeout must return in roughly that window, not block for 30 s.
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout wait took {elapsed:?} — the wait did not honor timeout_ms"
    );

    // close must cleanly tear down the still-running child (terminate).
    unsafe { close(pty) };
}

/// Cross-thread close (§3.3): a thread blocked in agt_pty_read must be
/// unblocked when another thread calls agt_pty_close.
#[test]
fn pty_close_unblocks_a_reader_on_another_thread() {
    let lib = load();
    let open: Symbol<PtyOpen> = unsafe { sym(lib, b"agt_pty_open") };
    let close: Symbol<PtyClose> = unsafe { sym(lib, b"agt_pty_close") };

    let (program, args) = pty_long_running_program();
    let pty = open_pty(lib, &open, program, &args);

    let (tx, rx) = mpsc::channel::<(i32, usize)>();
    // `*mut c_void` is not `Send`; carry the opaque handle as `usize` (Send)
    // and cast it back inside the thread. Sound because the library contract
    // is that agt_pty_t is cross-thread safe (§3.3).
    let reader_pty = pty as usize;
    let reader = std::thread::spawn(move || {
        let pty = reader_pty as *mut std::ffi::c_void;
        // Symbol is not Send, so the reader loads the library itself.
        let lib = load();
        let read: Symbol<PtyRead> = unsafe { sym(lib, b"agt_pty_read") };
        let mut buf = [0u8; 256];
        let mut n = 0usize;
        let st = unsafe { read(pty, buf.as_mut_ptr(), buf.len(), &mut n) };
        let _ = tx.send((st, n));
    });

    // Give the reader time to enter the blocking read, then close from here.
    std::thread::sleep(Duration::from_millis(300));
    unsafe { close(pty) };

    let (st, n) = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("reader was NOT unblocked within 5 s of agt_pty_close");
    reader.join().expect("reader thread panicked");
    // The contract (§3.3) is only that close never leaves the reader hanging —
    // the recv_timeout above already proved that. Legal unblock outcomes are
    // clean EOF (AGT_OK, n == 0), buffered data the child emitted before close
    // took effect (AGT_OK, n > 0), or an io failure (AGT_FAILED). Anything
    // else is a real violation.
    match st {
        AGT_OK => {
            // Legal either way: clean EOF (n == 0) or pre-close buffered data
            // (n > 0). The child may well have emitted its prompt/banner
            // before close terminated it, so n must not be pinned to 0.
        }
        AGT_FAILED => { /* io_read_failed is an acceptable unblock path */ }
        other => panic!("unexpected status {other}, n={n}"),
    }
}

// --- milestone 3a: window lifecycle + frame rendezvous ------------------

/// C-compatible mirror of include/agenterm.h `agt_window_spec`.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
struct agt_window_spec {
    title: *const c_char,
    width: u32,
    height: u32,
    no_activate: i32,
    ime_allowed: i32,
}

/// C-compatible mirror of include/agenterm.h `agt_frame_desc`.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
struct agt_frame_desc {
    pixels: *mut u32,
    width: u32,
    height: u32,
    stride_px: u32,
}

/// C-compatible mirror of include/agenterm.h `agt_event` (milestone 3b field
/// set — must match the Rust `#[repr(C)]` layout exactly, otherwise the
/// library writes past the caller's buffer).
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
struct agt_event {
    kind: u32,
    generation: u64,
    width: u32,
    height: u32,
    scale: f64,
    focused: i32,
    modifiers: u32,
    key_state: u8,
    key_repeat: u8,
    key_named: u8,
    key_physical: u8,
    key_physical_value: u32,
    text: [u8; 16],
    text_len: u8,
    text_truncated: u8,
    pointer_x: f64,
    pointer_y: f64,
    pointer_button: u8,
    pointer_state: u8,
    has_position: u8,
    wheel_x: f64,
    wheel_y: f64,
    wheel_unit: u8,
    ime_kind: u8,
    has_ime_cursor: u8,
    ime_cursor_begin: usize,
    ime_cursor_end: usize,
    ime_text_len: usize,
}

impl agt_event {
    fn empty() -> Self {
        Self {
            kind: 0,
            generation: 0,
            width: 0,
            height: 0,
            scale: 0.0,
            focused: 0,
            modifiers: 0,
            key_state: 0,
            key_repeat: 0,
            key_named: 0,
            key_physical: 0,
            key_physical_value: 0,
            text: [0u8; 16],
            text_len: 0,
            text_truncated: 0,
            pointer_x: 0.0,
            pointer_y: 0.0,
            pointer_button: 0,
            pointer_state: 0,
            has_position: 0,
            wheel_x: 0.0,
            wheel_y: 0.0,
            wheel_unit: 0,
            ime_kind: 0,
            has_ime_cursor: 0,
            ime_cursor_begin: 0,
            ime_cursor_end: 0,
            ime_text_len: 0,
        }
    }
}

type WindowOpen = unsafe extern "C" fn(*const agt_window_spec, *mut *mut std::ffi::c_void) -> i32;
type WindowPoll = unsafe extern "C" fn(*mut std::ffi::c_void, *mut agt_event, u32) -> i32;
type WindowRedraw = unsafe extern "C" fn(*mut std::ffi::c_void) -> i32;
type FrameBegin = unsafe extern "C" fn(*mut std::ffi::c_void, *mut agt_frame_desc, u32) -> i32;
type FrameCommit = unsafe extern "C" fn(*mut std::ffi::c_void) -> i32;
type WindowMetrics =
    unsafe extern "C" fn(*mut std::ffi::c_void, *mut u32, *mut u32, *mut f64) -> i32;
type WindowClose = unsafe extern "C" fn(*mut std::ffi::c_void);

/// Open a probe window (320x200, no_activate, ime off). On a headless host
/// where the window host is unavailable the library returns AGT_UNSUPPORTED:
/// that is an explicit allowed skip (printed) — every other failure must go
/// red.
fn try_open_window(lib: &Library, open: &Symbol<WindowOpen>) -> Option<*mut std::ffi::c_void> {
    let title = CString::new("agenterm-abi-window-probe").expect("title has no NUL");
    let spec = agt_window_spec {
        title: title.as_ptr(),
        width: 320,
        height: 200,
        no_activate: 1,
        ime_allowed: 0,
    };
    let mut window: *mut std::ffi::c_void = std::ptr::null_mut();
    let st = unsafe { open(&spec, &mut window) };
    match st {
        AGT_OK => {
            assert!(!window.is_null(), "agt_window_open returned a null handle");
            Some(window)
        }
        AGT_UNSUPPORTED => {
            eprintln!(
                "SKIP (headless host): agt_window_open unsupported: {}",
                last_error_message(lib)
            );
            None
        }
        other => panic!(
            "agt_window_open failed with {other}: {}",
            last_error_message(lib)
        ),
    }
}

/// Evidence 1: open → frame_begin returns a non-null pixel buffer with
/// width*height > 0 → write a known color → frame_commit → close cleanly.
#[test]
fn window_frame_roundtrip_begin_write_commit_close() {
    let lib = load();
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let begin: Symbol<FrameBegin> = unsafe { sym(lib, b"agt_frame_begin") };
    let commit: Symbol<FrameCommit> = unsafe { sym(lib, b"agt_frame_commit") };
    let metrics: Symbol<WindowMetrics> = unsafe { sym(lib, b"agt_window_metrics") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let Some(window) = try_open_window(lib, &open) else {
        return;
    };

    let mut desc = agt_frame_desc {
        pixels: std::ptr::null_mut(),
        width: 0,
        height: 0,
        stride_px: 0,
    };
    let st = unsafe { begin(window, &mut desc, 10_000) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_frame_begin failed: {}",
        last_error_message(lib)
    );
    assert!(!desc.pixels.is_null(), "frame pixel pointer is null");
    assert!(
        desc.width > 0 && desc.height > 0,
        "expected non-empty frame, got {}x{}",
        desc.width,
        desc.height
    );

    // Write a known color into the whole visible area (row-major by stride).
    const COLOR: u32 = 0x0012_3456;
    for row in 0..desc.height {
        let base = (row as usize) * (desc.stride_px as usize);
        for col in 0..desc.width {
            unsafe {
                *desc.pixels.add(base + col as usize) = COLOR;
            }
        }
    }
    let st = unsafe { commit(window) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_frame_commit failed: {}",
        last_error_message(lib)
    );

    // After open the loop thread has recorded geometry: metrics must work.
    let mut w = 0u32;
    let mut h = 0u32;
    let mut scale = 0.0f64;
    let st = unsafe { metrics(window, &mut w, &mut h, &mut scale) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_window_metrics failed: {}",
        last_error_message(lib)
    );
    assert!(
        w > 0 && h > 0 && scale > 0.0,
        "bad metrics {w}x{h} scale={scale}"
    );

    unsafe { close(window) };
}

/// Evidence 2: committing with no pending frame returns
/// AGT_FAILED { code = "no_frame" }.
#[test]
fn frame_commit_without_pending_frame_returns_no_frame() {
    let lib = load();
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let commit: Symbol<FrameCommit> = unsafe { sym(lib, b"agt_frame_commit") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let Some(window) = try_open_window(lib, &open) else {
        return;
    };

    // Never called agt_frame_begin → no pending (held) frame.
    let st = unsafe { commit(window) };
    assert_eq!(
        st, AGT_FAILED,
        "expected AGT_FAILED for commit without a pending frame, got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("no_frame"),
        "expected code \"no_frame\" in error, got: {msg}"
    );

    unsafe { close(window) };
}

/// Evidence 3: `agt_frame_begin` with a tiny timeout and no frame returns
/// AGT_FAILED { code = "timeout" } without hanging. The window's first frame
/// may already be published (opened() schedules a redraw), so consume up to a
/// few such frames first; with no further redraw requests and stable geometry
/// the platform stops rendering and begin must time out.
#[test]
fn frame_begin_times_out_when_no_frame_is_available() {
    let lib = load();
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let begin: Symbol<FrameBegin> = unsafe { sym(lib, b"agt_frame_begin") };
    let commit: Symbol<FrameCommit> = unsafe { sym(lib, b"agt_frame_commit") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let Some(window) = try_open_window(lib, &open) else {
        return;
    };

    for _ in 0..3 {
        let mut desc = agt_frame_desc {
            pixels: std::ptr::null_mut(),
            width: 0,
            height: 0,
            stride_px: 0,
        };
        match unsafe { begin(window, &mut desc, 0) } {
            AGT_OK => {
                // A frame was available (opening render / geometry render):
                // release it and try again — the next begin must time out.
                assert_eq!(
                    unsafe { commit(window) },
                    AGT_OK,
                    "agt_frame_commit failed: {}",
                    last_error_message(lib)
                );
            }
            st => {
                assert_eq!(
                    st, AGT_FAILED,
                    "expected AGT_FAILED from a frameless begin, got {st}"
                );
                let msg = last_error_message(lib);
                assert!(
                    msg.contains("timeout"),
                    "expected code \"timeout\" in error, got: {msg}"
                );
                unsafe { close(window) };
                return;
            }
        }
    }
    panic!(
        "platform kept rendering without redraw requests; cannot construct \
         the no-frame timeout scenario"
    );
}

/// Evidence 4 (design rule 6 regression): taking a frame and never committing
/// it must still allow `agt_window_close` to finish cleanly — the loop thread
/// escapes its rendezvous wait and the process does not hang.
#[test]
fn close_without_committing_taken_frame_does_not_hang() {
    let lib = load();
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let begin: Symbol<FrameBegin> = unsafe { sym(lib, b"agt_frame_begin") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let Some(window) = try_open_window(lib, &open) else {
        return;
    };

    // Take a frame (waiting long enough for the first published frame) and
    // deliberately never commit it — the caller breaches the protocol.
    let mut desc = agt_frame_desc {
        pixels: std::ptr::null_mut(),
        width: 0,
        height: 0,
        stride_px: 0,
    };
    let st = unsafe { begin(window, &mut desc, 10_000) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_frame_begin failed: {}",
        last_error_message(lib)
    );
    assert!(!desc.pixels.is_null());

    // Close must still be able to finish: wake the rendezvous, let the loop
    // thread exit. If close blocked, this assertion would trip.
    let started = Instant::now();
    unsafe { close(window) };
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "agt_window_close blocked on an un-committed frame"
    );
    // Give the detached loop thread a moment to actually tear down.
    std::thread::sleep(Duration::from_millis(200));
}

/// Design rule 6 (close must wake a caller blocked in agt_frame_begin): a
/// waiter on another thread is unblocked with AGT_FAILED { code = "closed" }
/// when agt_window_close runs. Uses a second window state (first frame
/// consumed, no further redraws) so the waiter genuinely blocks.
#[test]
fn close_wakes_a_caller_blocked_in_frame_begin() {
    let lib = load();
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let begin: Symbol<FrameBegin> = unsafe { sym(lib, b"agt_frame_begin") };
    let commit: Symbol<FrameCommit> = unsafe { sym(lib, b"agt_frame_commit") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let Some(window) = try_open_window(lib, &open) else {
        return;
    };

    // Consume the first published frame and commit it. No redraw request
    // follows, so the platform will not render again: the waiter below blocks.
    let mut desc = agt_frame_desc {
        pixels: std::ptr::null_mut(),
        width: 0,
        height: 0,
        stride_px: 0,
    };
    let st = unsafe { begin(window, &mut desc, 10_000) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_frame_begin failed: {}",
        last_error_message(lib)
    );
    let st = unsafe { commit(window) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_frame_commit failed: {}",
        last_error_message(lib)
    );

    // Waiter thread blocks in agt_frame_begin(10 s). `*mut c_void` is not
    // Send, so carry the opaque handle as usize and cast it back inside.
    let (tx, rx) = mpsc::channel::<(i32, String)>();
    let waiter_window = window as usize;
    let waiter = std::thread::spawn(move || {
        let window = waiter_window as *mut std::ffi::c_void;
        let lib = load();
        let begin: Symbol<FrameBegin> = unsafe { sym(lib, b"agt_frame_begin") };
        let mut desc = agt_frame_desc {
            pixels: std::ptr::null_mut(),
            width: 0,
            height: 0,
            stride_px: 0,
        };
        let st = unsafe { begin(window, &mut desc, 10_000) };
        let msg = last_error_message(lib);
        let _ = tx.send((st, msg));
    });

    // Give the waiter time to enter the blocking begin, then close from here.
    std::thread::sleep(Duration::from_millis(300));
    unsafe { close(window) };

    let (st, msg) = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("frame_begin waiter was NOT woken by agt_window_close");
    waiter.join().expect("waiter thread panicked");
    assert_eq!(
        st, AGT_FAILED,
        "expected the blocked agt_frame_begin to fail, got {st}"
    );
    assert!(
        msg.contains("closed"),
        "expected code \"closed\" in error, got: {msg}"
    );
}

/// `agt_window_request_redraw` + `agt_window_poll_event` round trip: after a
/// redraw request the loop publishes a frame and a RENDER_DUE event arrives
/// (opened() already scheduled the first redraw, so the first RENDER_DUE is
/// already in the queue; the explicit request is belt-and-braces).
#[test]
fn request_redraw_produces_a_render_due_event() {
    let lib = load();
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let redraw: Symbol<WindowRedraw> = unsafe { sym(lib, b"agt_window_request_redraw") };
    let poll: Symbol<WindowPoll> = unsafe { sym(lib, b"agt_window_poll_event") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let Some(window) = try_open_window(lib, &open) else {
        return;
    };

    let st = unsafe { redraw(window) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_window_request_redraw failed: {}",
        last_error_message(lib)
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no RENDER_DUE event within 10 s");
        let mut ev = agt_event::empty();
        let st = unsafe { poll(window, &mut ev, 5000) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_window_poll_event failed: {}",
            last_error_message(lib)
        );
        if ev.kind == AGT_EV_RENDER_DUE {
            break;
        }
    }
    unsafe { close(window) };
}

// --- milestone 3b: agt_window_event_text --------------------------------

type WindowEventText =
    unsafe extern "C" fn(*mut std::ffi::c_void, *mut u8, usize, *mut usize) -> i32;

/// Evidence 1/2: `agt_window_event_text` two-stage contract on a live window.
/// With no pending text a normal read returns AGT_OK and *out_len == 0; a
/// cap == 0 probe returns AGT_FAILED { code = "buffer_too_small" } with the
/// required byte count (0 here) written into *out_len. Skips on headless
/// hosts (same policy as the other window tests).
#[test]
fn window_event_text_two_stage_contract() {
    let lib = load();
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let poll: Symbol<WindowPoll> = unsafe { sym(lib, b"agt_window_poll_event") };
    let text_fn: Symbol<WindowEventText> = unsafe { sym(lib, b"agt_window_event_text") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let Some(window) = try_open_window(lib, &open) else {
        return;
    };

    // 1) Before any poll: no pending text → AGT_OK with *out_len == 0.
    let mut buf = [0u8; 32];
    let mut len = usize::MAX;
    let st = unsafe { text_fn(window, buf.as_mut_ptr(), buf.len(), &mut len) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_window_event_text (no text) failed: {}",
        last_error_message(lib)
    );
    assert_eq!(len, 0, "expected no pending text, got len={len}");

    // 2) Poll one event (open schedules the first render → RENDER_DUE etc.;
    //    none of these carry text, so the staged buffer stays empty). This
    //    also proves the poll path resets the text buffer.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ev = agt_event::empty();
    loop {
        assert!(Instant::now() < deadline, "no event within 10 s");
        let st = unsafe { poll(window, &mut ev, 5000) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_window_poll_event failed: {}",
            last_error_message(lib)
        );
        if ev.kind != AGT_EV_NONE {
            break;
        }
    }

    // 3) cap == 0 probe (null buf is legitimate for the first stage) →
    //    AGT_FAILED { code = "buffer_too_small" }, required byte count
    //    written into *out_len (0 while no text is staged).
    let mut probe = usize::MAX;
    let st = unsafe { text_fn(window, std::ptr::null_mut(), 0, &mut probe) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED for cap == 0, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert_eq!(probe, 0, "expected required length 0, got {probe}");

    // 4) cap == 0 with a non-null buf behaves identically.
    let mut probe2 = usize::MAX;
    let st = unsafe { text_fn(window, buf.as_mut_ptr(), 0, &mut probe2) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED for cap == 0, got {st}");
    assert_eq!(probe2, 0, "expected required length 0, got {probe2}");

    unsafe { close(window) };
}

/// A unique PNG path under the system temp dir (never the repo tree).
fn temp_png_path(tag: &str) -> (std::path::PathBuf, CString) {
    let path = std::env::temp_dir().join(format!("agenterm-abi-{tag}-{}.png", std::process::id()));
    let path_c =
        CString::new(path.to_string_lossy().as_bytes()).expect("temp path must be NUL-free");
    (path, path_c)
}

/// Milestone 4 evidence 1 (real round trip): encode a known 4x4 XRGB buffer
/// to a temp-dir PNG, assert AGT_OK plus a non-empty file with the PNG magic,
/// then delete the file.
#[test]
fn screenshot_write_png_roundtrip() {
    let lib = load();
    let f: Symbol<ScreenshotWritePng> = unsafe { sym(lib, b"agt_screenshot_write_png") };
    let pixels: Vec<u32> = vec![0x00FF0000u32; 16]; // 4x4 red (XRGB little-endian)
    let (path, path_c) = temp_png_path("roundtrip");
    let st = unsafe { f(path_c.as_ptr(), pixels.as_ptr(), pixels.len(), 4, 4) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_screenshot_write_png failed: {}",
        last_error_message(lib)
    );
    let data = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("PNG not readable at {}: {e}", path.display()));
    assert!(!data.is_empty(), "PNG file must be non-empty");
    assert_eq!(
        &data[..PNG_MAGIC.len()],
        &PNG_MAGIC,
        "PNG magic missing in first 8 bytes"
    );
    let _ = std::fs::remove_file(&path);
    assert!(!path.exists(), "temp PNG must be cleaned up");
}

/// Milestone 4 evidence 2: `pixel_count` != width*height must fail with code
/// "bad_dimensions" (validated before any platform call, so no file is
/// written).
#[test]
fn screenshot_write_png_rejects_mismatched_pixel_count() {
    let lib = load();
    let f: Symbol<ScreenshotWritePng> = unsafe { sym(lib, b"agt_screenshot_write_png") };
    let pixels = [0u32; 4];
    let (_, path_c) = temp_png_path("dim-mismatch");
    let st = unsafe { f(path_c.as_ptr(), pixels.as_ptr(), 3, 2, 2) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_dimensions"),
        "expected code \"bad_dimensions\" in error, got: {msg}"
    );
}

/// Milestone 4 evidence 3: NULL `path` → "bad_path"; NULL `pixels` →
/// "bad_pointer".
#[test]
fn screenshot_write_png_rejects_null_pointers() {
    let lib = load();
    let f: Symbol<ScreenshotWritePng> = unsafe { sym(lib, b"agt_screenshot_write_png") };
    let pixels = [0x00FFFFFFu32; 4];
    let (_, path_c) = temp_png_path("null-pointers");

    // NULL path (pixels valid, dimensions valid) → bad_path.
    let st = unsafe { f(std::ptr::null(), pixels.as_ptr(), pixels.len(), 2, 2) };
    assert_eq!(
        st, AGT_FAILED,
        "expected AGT_FAILED for NULL path, got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_path"),
        "expected code \"bad_path\" in error, got: {msg}"
    );

    // NULL pixels (path valid, dimensions valid) → bad_pointer.
    let st = unsafe { f(path_c.as_ptr(), std::ptr::null(), pixels.len(), 2, 2) };
    assert_eq!(
        st, AGT_FAILED,
        "expected AGT_FAILED for NULL pixels, got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 4 evidence 4: `agt_screenshot_capture_window` with
/// `native_window == 0` → code "bad_handle".
#[test]
fn screenshot_capture_window_rejects_zero_handle() {
    let lib = load();
    let f: Symbol<ScreenshotCaptureWindow> = unsafe { sym(lib, b"agt_screenshot_capture_window") };
    let (_, path_c) = temp_png_path("zero-handle");
    let st = unsafe { f(0, path_c.as_ptr(), 0, 0, 0, 0, 0) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_handle"),
        "expected code \"bad_handle\" in error, got: {msg}"
    );
}

/// Milestone 5 evidence 1: real two-stage round trip. Probe with
/// cap=0/buf=NULL to learn the required count (assert > 0), allocate that
/// many records, call again and expect AGT_OK. The returned set must contain
/// one record with `id == agt_process_self()` and a non-empty name — this
/// proves both the two-stage contract and the data's truthfulness.
#[test]
fn process_list_roundtrip_contains_self() {
    let lib = load();
    let list: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") };
    let self_pid: Symbol<ProcessSelf> = unsafe { sym(lib, b"agt_process_self") };
    let pid = unsafe { self_pid() };
    assert!(pid > 0, "agt_process_self must return a real pid, got 0");

    // First half: cap == 0 with buf == NULL is the legal "how big?" probe.
    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    assert_eq!(
        st, AGT_FAILED,
        "cap=0 probe must return AGT_FAILED (buffer_too_small), got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(required > 0, "required count must be > 0, got {required}");

    // Second half: allocate and fill. The process table can change between
    // the two calls on a concurrent system, so if the fresh call reports a
    // larger required count, re-allocate with that count and retry.
    let mut capacity = required + 64;
    let self_rec = loop {
        assert!(
            capacity < 1_000_000,
            "process count exploded far beyond the probe result"
        );
        let mut recs = vec![agt_process_info::default(); capacity];
        let mut got = 0usize;
        let st = unsafe { list(recs.as_mut_ptr(), capacity, &mut got) };
        if st == AGT_OK {
            assert!(
                got <= capacity,
                "out_count {got} exceeds capacity {capacity}"
            );
            break recs;
        }
        assert_eq!(
            st,
            AGT_FAILED,
            "agt_process_list failed: {}",
            last_error_message(lib)
        );
        let msg = last_error_message(lib);
        assert!(
            msg.contains("buffer_too_small"),
            "expected code \"buffer_too_small\" in error, got: {msg}"
        );
        assert!(
            got > capacity,
            "out_count must report a required count > capacity, got {got} <= {capacity}"
        );
        capacity = got + 64;
    };
    let self_rec = self_rec
        .iter()
        .find(|r| r.id == pid)
        .unwrap_or_else(|| panic!("process list must contain the calling process (pid {pid})"));
    assert!(
        self_rec.name_len > 0,
        "self record must have a non-empty name"
    );
}

/// Milestone 5 evidence 2: a deliberately too-small cap returns
/// AGT_FAILED{code="buffer_too_small"} and *out_count reports the required
/// (larger) count.
#[test]
fn process_list_small_cap_reports_required_count() {
    let lib = load();
    let list: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") };
    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    assert_eq!(st, AGT_FAILED);
    assert!(
        required > 1,
        "the test machine must have more than one process, got {required}"
    );

    let mut one = [agt_process_info::default(); 1];
    let mut got = 0usize;
    let st = unsafe { list(one.as_mut_ptr(), 1, &mut got) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        got > 1,
        "out_count must report the required count > 1, got {got}"
    );
}

/// Milestone 5 evidence 3: NULL out_count → AGT_FAILED{code="bad_pointer"}.
#[test]
fn process_list_rejects_null_out_count() {
    let lib = load();
    let list: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") };
    let mut one = [agt_process_info::default(); 1];
    let st = unsafe { list(one.as_mut_ptr(), 1, std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 5 evidence 4: `agt_process_kill(0)` → AGT_FAILED{code="bad_pid"}.
/// Only the pid-0 rejection path is exercised — no real process is ever
/// killed by these tests.
#[test]
fn process_kill_rejects_pid_zero() {
    let lib = load();
    let kill: Symbol<ProcessKill> = unsafe { sym(lib, b"agt_process_kill") };
    let st = unsafe { kill(0) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pid"),
        "expected code \"bad_pid\" in error, got: {msg}"
    );
}

/// Milestone 6 evidence 1: accessibility-tree capability is queryable.
#[test]
fn a11y_capability_query_is_ok_or_unsupported() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let st = unsafe { query(AGT_CAP_ACCESSIBILITY_TREE) };
    assert!(
        st == AGT_OK || st == AGT_UNSUPPORTED,
        "capability query must not return AGT_FAILED, got {st}"
    );
}

/// Milestone 43: the native-window / input-injection capabilities report the
/// host's real capability status — AGT_OK or AGT_UNSUPPORTED, never a blanket
/// AGT_OK (they must not lie on hosts that lack the mechanism).
#[test]
fn native_window_and_input_capabilities_query_ok_or_unsupported() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    for cap in [
        AGT_CAP_WINDOW_ENUMERATE,
        AGT_CAP_WINDOW_OP,
        AGT_CAP_INPUT_INJECT,
    ] {
        let st = unsafe { query(cap) };
        assert!(
            st == AGT_OK || st == AGT_UNSUPPORTED,
            "capability query for {cap} must return AGT_OK or AGT_UNSUPPORTED, got {st}"
        );
    }
}

/// Milestone 6 evidence 2: reading a node without a snapshot fails typed.
#[test]
fn a11y_tree_node_without_snapshot_fails() {
    let lib = load();
    let node_fn: Symbol<A11yTreeNode> = unsafe { sym(lib, b"agt_a11y_tree_node") };
    let mut record = agt_a11y_node::default();
    let st = unsafe { node_fn(0, &mut record) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("no_snapshot"),
        "expected code \"no_snapshot\" in error, got: {msg}"
    );
}

/// Milestone 6 evidence 3: NULL node_id → AGT_FAILED{code="bad_pointer"}
/// on hosts with a wired a11y stack; hosts without it return AGT_UNSUPPORTED.
#[test]
fn a11y_node_perform_rejects_null_node_id() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let cap = unsafe { query(AGT_CAP_ACCESSIBILITY_TREE) };
    let perform: Symbol<A11yNodePerform> = unsafe { sym(lib, b"agt_a11y_node_perform") };
    let st = unsafe { perform(0, std::ptr::null(), 0) };
    if cap == AGT_UNSUPPORTED {
        eprintln!("SKIP (no a11y stack): AGT_CAP_ACCESSIBILITY_TREE unsupported");
        assert_eq!(st, AGT_UNSUPPORTED, "expected AGT_UNSUPPORTED, got {st}");
        return;
    }
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Named AT-SPI text write: NULL node_id → AGT_FAILED{code="bad_pointer"}
/// when the a11y stack is wired; otherwise AGT_UNSUPPORTED.
#[test]
fn a11y_node_set_text_rejects_null_node_id() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let cap = unsafe { query(AGT_CAP_ACCESSIBILITY_TREE) };
    let set_text: Symbol<A11yNodeSetText> = unsafe { sym(lib, b"agt_a11y_node_set_text") };
    let st = unsafe { set_text(0, std::ptr::null(), b"x".as_ptr(), 1) };
    if cap == AGT_UNSUPPORTED {
        eprintln!("SKIP (no a11y stack): AGT_CAP_ACCESSIBILITY_TREE unsupported");
        assert_eq!(st, AGT_UNSUPPORTED, "expected AGT_UNSUPPORTED, got {st}");
        return;
    }
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Independent AT-SPI GetText: NULL node_id → AGT_FAILED{code="bad_pointer"}
/// when the a11y stack is wired; otherwise AGT_UNSUPPORTED.
#[test]
fn a11y_node_get_text_rejects_null_node_id() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let cap = unsafe { query(AGT_CAP_ACCESSIBILITY_TREE) };
    let get_text: Symbol<A11yNodeGetText> = unsafe { sym(lib, b"agt_a11y_node_get_text") };
    let mut required = 0usize;
    let st = unsafe { get_text(0, std::ptr::null(), std::ptr::null_mut(), 0, &mut required) };
    if cap == AGT_UNSUPPORTED {
        eprintln!("SKIP (no a11y stack): AGT_CAP_ACCESSIBILITY_TREE unsupported");
        assert_eq!(st, AGT_UNSUPPORTED, "expected AGT_UNSUPPORTED, got {st}");
        return;
    }
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Named AT-SPI Device/key: NULL node_id → AGT_FAILED{code="bad_pointer"}
/// when the a11y stack is wired; otherwise AGT_UNSUPPORTED.
#[test]
fn a11y_node_send_keys_rejects_null_node_id() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let cap = unsafe { query(AGT_CAP_ACCESSIBILITY_TREE) };
    let send_keys: Symbol<A11yNodeSendKeys> = unsafe { sym(lib, b"agt_a11y_node_send_keys") };
    let st = unsafe { send_keys(0, std::ptr::null(), b"k".as_ptr(), 1) };
    if cap == AGT_UNSUPPORTED {
        eprintln!("SKIP (no a11y stack): AGT_CAP_ACCESSIBILITY_TREE unsupported");
        assert_eq!(st, AGT_UNSUPPORTED, "expected AGT_UNSUPPORTED, got {st}");
        return;
    }
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 6 evidence 4: on hosts with a wired accessibility stack, snapshot
/// returns a tree and metadata strings round-trip.
#[test]
fn a11y_tree_snapshot_roundtrip_when_available() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let st = unsafe { query(AGT_CAP_ACCESSIBILITY_TREE) };
    if st == AGT_UNSUPPORTED {
        eprintln!("SKIP (no a11y stack): AGT_CAP_ACCESSIBILITY_TREE unsupported");
        return;
    }
    let snapshot: Symbol<A11yTreeSnapshot> = unsafe { sym(lib, b"agt_a11y_tree_snapshot") };
    let meta: Symbol<A11yTreeMetaString> = unsafe { sym(lib, b"agt_a11y_tree_meta_string") };
    let node_fn: Symbol<A11yTreeNode> = unsafe { sym(lib, b"agt_a11y_tree_node") };
    let node_str: Symbol<A11yNodeString> = unsafe { sym(lib, b"agt_a11y_node_string") };
    let mut count = 0usize;
    let snap_st = unsafe { snapshot(0, &mut count) };
    if snap_st == AGT_UNSUPPORTED {
        eprintln!("SKIP (runtime a11y unavailable): snapshot unsupported");
        return;
    }
    assert_eq!(
        snap_st,
        AGT_OK,
        "snapshot failed: {}",
        last_error_message(lib)
    );
    let backend = read_two_stage_bytes(lib, |buf, cap, out_len| unsafe {
        meta(0, buf, cap, out_len)
    });
    assert!(
        !backend.is_empty(),
        "backend metadata must be non-empty on success"
    );
    if count > 0 {
        let mut record = agt_a11y_node::default();
        let node_st = unsafe { node_fn(0, &mut record) };
        assert_eq!(
            node_st,
            AGT_OK,
            "node read failed: {}",
            last_error_message(lib)
        );
        let role = read_two_stage_bytes(lib, |buf, cap, out_len| unsafe {
            node_str(0, 0, buf, cap, out_len)
        });
        assert!(!role.is_empty(), "first node role should be non-empty");
    }
}

fn read_two_stage_bytes(
    lib: &Library,
    mut read: impl FnMut(*mut u8, usize, *mut usize) -> i32,
) -> Vec<u8> {
    let mut required = 0usize;
    let probe = read(std::ptr::null_mut(), 0, &mut required);
    assert_eq!(probe, AGT_FAILED, "probe should request allocation");
    let mut buf = vec![0u8; required];
    let st = read(buf.as_mut_ptr(), required, &mut required);
    assert_eq!(st, AGT_OK, "read failed: {}", last_error_message(lib));
    buf.truncate(required);
    buf
}

// --- clipboard (milestone 8) --------------------------------------------

/// Process-wide lock serializing every test that touches the real clipboard.
/// The OS clipboard is a global singleton outside this process while cargo
/// runs tests on many threads by default: two clipboard tests racing each
/// other interleave (one test's restore clobbers the other's probe), which
/// made the full suite fail while each test passed alone. Poison is
/// tolerated — a panic in one clipboard test must not cascade into the rest.
static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

/// Take the clipboard serial lock. Every clipboard test calls this first so
/// at most one test touches the OS clipboard at a time.
fn clipboard_lock() -> std::sync::MutexGuard<'static, ()> {
    CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// RAII restore for the user's real clipboard. `original` holds the content
/// that was on the clipboard when the guard was created (empty = no text).
/// `restore()` writes it back; `Drop` calls `restore()`, so a panicking
/// assertion still attempts to restore the user's clipboard.
struct ClipboardGuard {
    set: Symbol<'static, ClipboardSetText>,
    original: Vec<u8>,
    dirty: bool,
    restored: bool,
}

impl ClipboardGuard {
    /// Restore the original content. Returns `true` when the restore
    /// succeeded (or was a no-op); `false` when non-empty original content
    /// could not be written back — the user's data is genuinely at risk and
    /// the caller must surface that as a test failure. Restoring an *empty*
    /// original that fails only prints a warning: there was no user data to
    /// lose.
    fn restore(&mut self) -> bool {
        if self.restored {
            return true;
        }
        self.restored = true;
        // Only touch the clipboard when we actually changed it, or when the
        // original was non-empty (a failed probe write may still have emptied
        // it mid-way; restore the user's text in that case too). An empty
        // original with no probe write means "no text, untouched" — writing
        // then would be an unnecessary mutation.
        if !self.dirty && self.original.is_empty() {
            return true;
        }
        let status = unsafe { (self.set)(self.original.as_ptr(), self.original.len()) };
        if status != AGT_OK {
            if self.original.is_empty() {
                eprintln!("WARNING: failed to clear the user's clipboard (status {status})");
            } else {
                eprintln!(
                    "ERROR: failed to restore the user's clipboard (status {status}); \
                     {} bytes of original content may be lost",
                    self.original.len()
                );
                return false;
            }
        }
        true
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // A panic inside `Drop` while another panic is unwinding would abort
        // the whole test process and hide every other failure, so the Drop
        // path only reports. Every test that mutates the clipboard also calls
        // `restore()` explicitly and asserts its result, which is what makes
        // a lost non-empty original fail the test.
        self.restore();
    }
}

/// Read the full clipboard text through the ABI. Returns `None` when the
/// clipboard is unavailable in this session (no GUI / window station) or a
/// platform failure occurred — the caller must not touch the clipboard then.
fn clipboard_read_all(get: Symbol<'static, ClipboardGetText>) -> Option<Vec<u8>> {
    let mut required = 0usize;
    let st = unsafe { get(std::ptr::null_mut(), 0, &mut required) };
    if st == AGT_OK {
        // No Unicode text: an empty original is the true state.
        return Some(Vec::new());
    }
    if st != AGT_FAILED || required == 0 {
        eprintln!("clipboard probe returned unexpected status {st}");
        return None;
    }
    let mut buf = vec![0u8; required];
    let mut got = 0usize;
    let st = unsafe { get(buf.as_mut_ptr(), required, &mut got) };
    if st != AGT_OK {
        eprintln!("clipboard read failed (status {st}); treating clipboard as unavailable");
        return None;
    }
    buf.truncate(got);
    Some(buf)
}

/// Save the original clipboard content, then install a unique UTF-8 probe
/// (multi-byte characters included) on the clipboard. The probe is padded to
/// at least `min_len` bytes so callers that need a known-long payload (e.g.
/// the too-small-cap test) never depend on what the clipboard happened to
/// hold. Returns the guard plus the probe bytes, or `None` when the clipboard
/// is unavailable in this session (the reason is printed; nothing was
/// modified then). "Unavailable" is judged solely by the probe write failing,
/// so within one process all clipboard tests reach the same verdict.
fn write_clipboard_probe(
    lib: &'static Library,
    min_len: usize,
) -> Option<(ClipboardGuard, Vec<u8>)> {
    let get: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };
    let set: Symbol<ClipboardSetText> = unsafe { sym(lib, b"agt_clipboard_set_text") };
    let original = clipboard_read_all(get)?;
    let mut guard = ClipboardGuard {
        set,
        original,
        dirty: false,
        restored: false,
    };
    let mut probe = format!(
        "agenterm-m8-clipboard-探针-{}-{:?}",
        std::process::id(),
        std::time::Instant::now()
    );
    while probe.len() < min_len {
        probe.push('x');
    }
    let status = unsafe { (guard.set)(probe.as_bytes().as_ptr(), probe.len()) };
    if status != AGT_OK {
        eprintln!(
            "clipboard unavailable in this session: probe write failed \
             ({})",
            last_error_message(lib)
        );
        return None; // guard drops: the original is restored.
    }
    guard.dirty = true;
    Some((guard, probe.into_bytes()))
}

/// Milestone 8 evidence 1: full round trip with the user's real clipboard
/// protected. Save the original content, write a probe, read it back and
/// assert equality, then restore the original and verify the restore took
/// effect.
#[test]
fn clipboard_roundtrip_preserves_user_clipboard() {
    let lib = load();
    let _serial = clipboard_lock();
    let Some((mut guard, probe)) = write_clipboard_probe(lib, 0) else {
        eprintln!("SKIP: clipboard unavailable in this session; round trip cannot run");
        return;
    };
    eprintln!(
        "round trip ran for real: {}-byte probe written to the clipboard",
        probe.len()
    );
    let get: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };
    let read_back = clipboard_read_all(get.clone())
        .expect("probe text must be readable immediately after a successful write");
    assert_eq!(read_back, probe, "clipboard round trip mismatch");
    // The has-text export must report 1 while the probe is installed.
    let has: Symbol<ClipboardHasText> = unsafe { sym(lib, b"agt_clipboard_has_text") };
    assert_eq!(
        unsafe { has() },
        1,
        "has_text must report 1 while probe text is present"
    );
    // Restore the original and verify the restore actually took effect. A
    // failed restore of non-empty content means the user's data is gone: that
    // must fail the test, not just print a warning.
    assert!(
        guard.restore(),
        "user clipboard restore FAILED: {} bytes of original content may be lost",
        guard.original.len()
    );
    let restored =
        clipboard_read_all(get.clone()).expect("clipboard must be readable after restore");
    assert_eq!(restored, guard.original, "user clipboard was not restored");
}

/// Milestone 8 evidence 2: a deliberately too-small cap (1) returns
/// AGT_FAILED{code="buffer_too_small"} and *out_len reports the required
/// (larger) byte count. The test first installs its own known probe padded to
/// at least 64 bytes, so it never depends on what the clipboard happened to
/// hold. Requires the clipboard to be writable; skips with a printed reason
/// when it is not.
#[test]
fn clipboard_get_text_small_cap_reports_required_bytes() {
    let lib = load();
    let _serial = clipboard_lock();
    let Some((mut guard, probe)) = write_clipboard_probe(lib, 64) else {
        eprintln!("SKIP: clipboard unavailable in this session; too-small-cap cannot run");
        return;
    };
    eprintln!(
        "too-small-cap ran for real: {}-byte probe installed",
        probe.len()
    );
    let get: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };
    let mut out_len = 0usize;
    let mut one = [0u8; 1];
    let st = unsafe { get(one.as_mut_ptr(), 1, &mut out_len) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        out_len > 1,
        "out_len must report required bytes > 1, got {out_len}"
    );
    assert_eq!(
        out_len,
        probe.len(),
        "required bytes must equal the installed probe length"
    );
    assert!(
        probe.len() >= 64,
        "probe must be padded to at least 64 bytes, got {}",
        probe.len()
    );
    assert!(
        guard.restore(),
        "user clipboard restore FAILED: {} bytes of original content may be lost",
        guard.original.len()
    );
}

/// Milestone 8 evidence 3: NULL out_len -> AGT_FAILED{code="bad_pointer"}.
/// Does not depend on the real clipboard, so it always runs (the lock still
/// serializes it with the other clipboard tests for verdict consistency).
#[test]
fn clipboard_get_text_rejects_null_out_len() {
    let lib = load();
    let _serial = clipboard_lock();
    let get: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };
    let mut buf = [0u8; 16];
    let st = unsafe { get(buf.as_mut_ptr(), buf.len(), std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 8 evidence 4: `agt_clipboard_set_text(NULL, 5)` ->
/// AGT_FAILED{code="bad_text"}. Does not depend on the real clipboard (the
/// NULL check and the UTF-8 validation both happen before any clipboard
/// access), so it always runs (the lock still serializes it with the other
/// clipboard tests for verdict consistency).
#[test]
fn clipboard_set_text_rejects_null_and_non_utf8() {
    let lib = load();
    let _serial = clipboard_lock();
    let set: Symbol<ClipboardSetText> = unsafe { sym(lib, b"agt_clipboard_set_text") };
    // NULL text with a nonzero length -> bad_text.
    let st = unsafe { set(std::ptr::null(), 5) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_text"),
        "expected code \"bad_text\" in error, got: {msg}"
    );
    // NULL text with length 0 is also rejected (NULL is always invalid).
    let st = unsafe { set(std::ptr::null(), 0) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_text"),
        "expected code \"bad_text\" in error, got: {msg}"
    );
    // Non-UTF-8 bytes -> bad_text (validated before any clipboard access).
    let invalid = [0xffu8, 0xfe];
    let st = unsafe { set(invalid.as_ptr(), invalid.len()) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_text"),
        "expected code \"bad_text\" in error, got: {msg}"
    );
}

/// Milestone 9 evidence 1: a normal UTF-8 line written to the parent
/// console's stdout must be either AGT_OK (this process has a writable
/// parent console) or AGT_UNSUPPORTED (it has none) — never AGT_FAILED —
/// and must not crash. Which status it is depends on the test process, so
/// the actual value is printed for manual cross-checking.
#[test]
fn parent_console_write_stdout_valid_text_never_fails() {
    let lib = load();
    let f: Symbol<ParentConsoleWrite> = unsafe { sym(lib, b"agt_parent_console_write_stdout") };
    let text = b"agenterm-abi-probe: milestone-9 stdout write\n";
    let st = unsafe { f(text.as_ptr(), text.len()) };
    assert!(
        st == AGT_OK || st == AGT_UNSUPPORTED,
        "valid UTF-8 stdout write must be AGT_OK or AGT_UNSUPPORTED, got {st}"
    );
    eprintln!(
        "parent-console stdout write returned {} on this host",
        if st == AGT_OK {
            "AGT_OK"
        } else {
            "AGT_UNSUPPORTED"
        }
    );
}

/// Milestone 9 evidence 1 (stderr twin): same contract, same assertion, same
/// printed verdict for manual cross-checking.
#[test]
fn parent_console_write_stderr_valid_text_never_fails() {
    let lib = load();
    let f: Symbol<ParentConsoleWrite> = unsafe { sym(lib, b"agt_parent_console_write_stderr") };
    let text = b"agenterm-abi-probe: milestone-9 stderr write\n";
    let st = unsafe { f(text.as_ptr(), text.len()) };
    assert!(
        st == AGT_OK || st == AGT_UNSUPPORTED,
        "valid UTF-8 stderr write must be AGT_OK or AGT_UNSUPPORTED, got {st}"
    );
    eprintln!(
        "parent-console stderr write returned {} on this host",
        if st == AGT_OK {
            "AGT_OK"
        } else {
            "AGT_UNSUPPORTED"
        }
    );
}

/// Milestone 9 evidence 2 + 3: NULL text (with len > 0) and non-UTF-8 bytes
/// both -> AGT_FAILED{code="bad_text"}.
#[test]
fn parent_console_write_rejects_null_and_non_utf8() {
    let lib = load();
    let f: Symbol<ParentConsoleWrite> = unsafe { sym(lib, b"agt_parent_console_write_stdout") };
    // NULL text with a nonzero length -> bad_text.
    let st = unsafe { f(std::ptr::null(), 5) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_text"),
        "expected code \"bad_text\" in error, got: {msg}"
    );
    // Non-UTF-8 bytes -> bad_text.
    let invalid = [0xffu8, 0xfe];
    let st = unsafe { f(invalid.as_ptr(), invalid.len()) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_text"),
        "expected code \"bad_text\" in error, got: {msg}"
    );
    // Same bad-input contract on the stderr export.
    let g: Symbol<ParentConsoleWrite> = unsafe { sym(lib, b"agt_parent_console_write_stderr") };
    let st = unsafe { g(std::ptr::null(), 5) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_text"),
        "expected code \"bad_text\" in error, got: {msg}"
    );
}

/// Milestone 9 evidence 4: `len == 0` is legal input (an empty line is
/// written), so it must never return AGT_FAILED{code="bad_text"} — with
/// either a non-NULL empty pointer or NULL (NULL is only rejected when
/// len > 0).
#[test]
fn parent_console_write_accepts_empty_text() {
    let lib = load();
    let f: Symbol<ParentConsoleWrite> = unsafe { sym(lib, b"agt_parent_console_write_stdout") };
    let empty: &[u8] = b"";
    for text in [empty.as_ptr(), std::ptr::null()] {
        let st = unsafe { f(text, 0) };
        assert_ne!(
            st, AGT_FAILED,
            "len == 0 must never be AGT_FAILED {{bad_text}}, got {st}"
        );
    }
    // Same contract on the stderr export.
    let g: Symbol<ParentConsoleWrite> = unsafe { sym(lib, b"agt_parent_console_write_stderr") };
    let st = unsafe { g(std::ptr::null(), 0) };
    assert_ne!(
        st, AGT_FAILED,
        "len == 0 must never be AGT_FAILED, got {st}"
    );
}

/// Milestone 10 evidence 1: `agt_runtime_user_config_dir` two-stage contract.
/// PRIVACY RULE: the value is a real user-home path, so the test asserts only
/// length properties (probed length > 0, written == probed, valid UTF-8) and
/// never prints the path bytes or embeds them in a panic message.
#[test]
fn runtime_user_config_dir_two_stage_contract() {
    let lib = load();
    let dir: Symbol<RuntimeUserConfigDir> = unsafe { sym(lib, b"agt_runtime_user_config_dir") };

    // Stage 1: the legal "how big?" probe (cap == 0, buf == NULL).
    let mut needed = 0usize;
    let st = unsafe { dir(std::ptr::null_mut(), 0, &mut needed) };
    assert_eq!(
        st, AGT_FAILED,
        "cap=0 probe must fail with buffer_too_small, got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        needed > 0,
        "user config dir must be non-empty, got length {needed}"
    );

    // Stage 2: allocate exactly the probed size and fetch.
    let mut buf = vec![0u8; needed];
    let mut got = 0usize;
    let st = unsafe { dir(buf.as_mut_ptr(), needed, &mut got) };
    assert_eq!(st, AGT_OK, "fetch failed: {}", last_error_message(lib));
    assert_eq!(
        got, needed,
        "written length {got} must equal the probed length {needed}"
    );
    assert!(
        std::str::from_utf8(&buf).is_ok(),
        "user config dir must be valid UTF-8"
    );
}

/// Milestone 10 evidence 2: `agt_runtime_default_shell` two-stage contract.
/// Same length-only assertions as the config-dir test; the shell path is
/// never printed.
#[test]
fn runtime_default_shell_two_stage_contract() {
    let lib = load();
    let shell: Symbol<RuntimeDefaultShell> = unsafe { sym(lib, b"agt_runtime_default_shell") };

    let mut needed = 0usize;
    let st = unsafe { shell(std::ptr::null_mut(), 0, &mut needed) };
    assert_eq!(
        st, AGT_FAILED,
        "cap=0 probe must fail with buffer_too_small, got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        needed > 0,
        "default shell must be non-empty, got length {needed}"
    );

    let mut buf = vec![0u8; needed];
    let mut got = 0usize;
    let st = unsafe { shell(buf.as_mut_ptr(), needed, &mut got) };
    assert_eq!(st, AGT_OK, "fetch failed: {}", last_error_message(lib));
    assert_eq!(
        got, needed,
        "written length {got} must equal the probed length {needed}"
    );
    assert!(
        std::str::from_utf8(&buf).is_ok(),
        "default shell must be valid UTF-8"
    );
}

/// Milestone 10 evidence 4: `agt_runtime_env_present` probes the real
/// environment. `PATH` always exists in the test environment; the sentinel
/// never does. NULL / empty / non-UTF-8 names are queries that report 0.
#[test]
fn runtime_env_present_probes_real_environment() {
    let lib = load();
    let present: Symbol<RuntimeEnvPresent> = unsafe { sym(lib, b"agt_runtime_env_present") };

    let path = b"PATH";
    assert_eq!(
        unsafe { present(path.as_ptr(), path.len()) },
        1,
        "PATH must exist in the test environment"
    );
    let absent = b"AGENTERM_ABI_NO_SUCH_VAR_XYZ";
    assert_eq!(
        unsafe { present(absent.as_ptr(), absent.len()) },
        0,
        "the sentinel variable must not exist"
    );

    assert_eq!(unsafe { present(std::ptr::null(), 0) }, 0, "NULL name is 0");
    assert_eq!(unsafe { present(absent.as_ptr(), 0) }, 0, "empty name is 0");
    let bad_utf8 = [0xffu8, 0xfe, 0x80];
    assert_eq!(
        unsafe { present(bad_utf8.as_ptr(), bad_utf8.len()) },
        0,
        "non-UTF-8 name is 0"
    );
}

/// Child-process probe for `agt_runtime_arg*`. This test only asserts inside
/// a subprocess spawned by `runtime_arg_count_reports_real_arguments`, whose
/// extra arguments guarantee a non-empty argument list — the export is
/// exercised against real data. When the test binary runs directly under
/// `cargo test` the argument list is empty by design, so the probe returns
/// without asserting. It panics (failing the parent) if any assertion fails.
/// Argument bytes are known harness input, not user data, but they are still
/// never printed.
#[test]
fn runtime_arg_child_probe() {
    // Guard: only the child process (spawned with the marker argument) runs
    // the assertions; a bare `cargo test` run must not fail here.
    if !std::env::args().any(|a| a == "--agenterm-abi-child-arg") {
        return;
    }
    let lib = load();
    let count: Symbol<RuntimeArgCount> = unsafe { sym(lib, b"agt_runtime_arg_count") };
    let arg: Symbol<RuntimeArg> = unsafe { sym(lib, b"agt_runtime_arg") };

    let mut n = 0usize;
    let st = unsafe { count(&mut n) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_runtime_arg_count failed: {}",
        last_error_message(lib)
    );
    assert!(
        n >= 1,
        "child process must carry at least one argument, got {n}"
    );

    // Two-stage fetch of argument 0.
    let mut needed = 0usize;
    let st = unsafe { arg(0, std::ptr::null_mut(), 0, &mut needed) };
    assert_eq!(
        st, AGT_FAILED,
        "cap=0 probe must fail with buffer_too_small, got {st}"
    );
    assert!(
        needed > 0,
        "argument 0 must be non-empty, got length {needed}"
    );
    let mut buf = vec![0u8; needed];
    let mut got = 0usize;
    let st = unsafe { arg(0, buf.as_mut_ptr(), needed, &mut got) };
    assert_eq!(st, AGT_OK, "fetch failed: {}", last_error_message(lib));
    assert_eq!(
        got, needed,
        "written length {got} must equal the probed length {needed}"
    );
    assert!(
        std::str::from_utf8(&buf).is_ok(),
        "argument 0 must be valid UTF-8"
    );
}

/// Milestone 10 evidence 3: `cargo test` runs the test binary with no extra
/// arguments (only the image name), so the argument list would be empty. This
/// parent test spawns the same binary as a child with known extra arguments,
/// running only `runtime_arg_child_probe` (libtest filter); the child's
/// assertions prove `arg_count >= 1` and a real two-stage `arg(0)` fetch.
#[test]
fn runtime_arg_count_reports_real_arguments() {
    let exe = std::env::current_exe().expect("current_exe()");
    let out = std::process::Command::new(&exe)
        .args(["runtime_arg_child_probe", "--", "--agenterm-abi-child-arg"])
        .output()
        .expect("spawn the child probe");
    assert!(
        out.status.success(),
        "child probe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Milestone 10 evidence 3/5: out-of-range index is `bad_index`; invalid out
/// pointers are `bad_pointer` — the pointer check runs before the index
/// range check, so this is verifiable even from an argument-less process.
#[test]
fn runtime_arg_bad_index_and_bad_pointer() {
    let lib = load();
    let count: Symbol<RuntimeArgCount> = unsafe { sym(lib, b"agt_runtime_arg_count") };
    let arg: Symbol<RuntimeArg> = unsafe { sym(lib, b"agt_runtime_arg") };

    let mut scratch = [0u8; 64];
    let mut out = 0usize;

    // Valid pointers but out-of-range index -> bad_index.
    let st = unsafe { arg(9999, scratch.as_mut_ptr(), 64, &mut out) };
    assert_eq!(st, AGT_FAILED, "out-of-range index must fail, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_index"),
        "expected code \"bad_index\" in error, got: {msg}"
    );

    // NULL out_count -> bad_pointer.
    let st = unsafe { count(std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "NULL out_count must fail, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );

    // NULL out_len -> bad_pointer (checked before the index range).
    let st = unsafe { arg(9999, scratch.as_mut_ptr(), 64, std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "NULL out_len must fail, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 10 evidence 5: every two-stage export reports
/// `AGT_FAILED{code="bad_pointer"}` for a NULL `out_len` (and, for
/// `agt_runtime_arg_count`, a NULL `out_count`).
#[test]
fn runtime_two_stage_rejects_null_out_len() {
    let lib = load();
    let dir: Symbol<RuntimeUserConfigDir> = unsafe { sym(lib, b"agt_runtime_user_config_dir") };
    let shell: Symbol<RuntimeDefaultShell> = unsafe { sym(lib, b"agt_runtime_default_shell") };
    let arg: Symbol<RuntimeArg> = unsafe { sym(lib, b"agt_runtime_arg") };

    let mut scratch = [0u8; 64];

    let st = unsafe { dir(scratch.as_mut_ptr(), 64, std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "dir: NULL out_len must fail, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "dir: expected code \"bad_pointer\" in error, got: {msg}"
    );

    let st = unsafe { shell(scratch.as_mut_ptr(), 64, std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "shell: NULL out_len must fail, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "shell: expected code \"bad_pointer\" in error, got: {msg}"
    );

    let st = unsafe { arg(0, scratch.as_mut_ptr(), 64, std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "arg: NULL out_len must fail, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "arg: expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 43/45 guard shared by the window/screen tests below:
/// `agt_window_enumerate`, `agt_screen_list` (which reuses the same
/// mechanism) and the enumeration step of `agt_native_window_show` all depend
/// on the WINDOW_ENUMERATE capability. On a headless host the mechanism is
/// absent and the cap=0 probe must answer `AGT_UNSUPPORTED` (never
/// `AGT_FAILED`, never `AGT_OK`) — assert that contract, print the skip, and
/// tell the caller to return. Returns false when the capability is available,
/// in which case the caller keeps its existing `AGT_FAILED` assertions.
fn window_enumerate_unsupported_probe(lib: &Library, probe_st: i32, what: &str) -> bool {
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let cap = unsafe { query(AGT_CAP_WINDOW_ENUMERATE) };
    if cap == AGT_UNSUPPORTED {
        eprintln!("SKIP (headless): AGT_CAP_WINDOW_ENUMERATE unsupported for {what}");
        assert_eq!(
            probe_st, AGT_UNSUPPORTED,
            "{what}: cap=0 probe must return AGT_UNSUPPORTED when the mechanism is absent, got {probe_st}"
        );
        true
    } else {
        false
    }
}

/// Milestone 50: capability present != list non-empty. On a headless host the
/// mechanism may be available while zero top-level windows (or screens) exist;
/// the cap=0 probe must then answer `AGT_OK` with `*out_count == 0` — cap=0 is
/// enough for an empty list, and that is the correct two-stage (§3.4) contract,
/// never `AGT_FAILED`. Returns true and prints the skip when the list is empty
/// (still asserting the probe status — never a bare `return`); false keeps the
/// caller on the non-empty `AGT_FAILED` (buffer_too_small) path.
fn empty_list_probe_ok(probe_st: i32, required: usize, what: &str) -> bool {
    if required == 0 {
        assert_eq!(
            probe_st, AGT_OK,
            "{what}: empty list with cap=0 must be AGT_OK, got {probe_st}"
        );
        eprintln!("SKIP (no {what} in this environment): enumeration returned 0");
        true
    } else {
        false
    }
}

/// Milestone 43: two-stage `agt_window_enumerate` round trip. Probes with
/// cap=0/buf=NULL (the legal "how big?" probe), allocates the required count,
/// calls again and asserts AGT_OK with at least one record carrying a
/// non-empty title. Window titles are never printed — they may contain user
/// privacy. On a headless host the mechanism is absent: the probe must return
/// AGT_UNSUPPORTED and the test skips (see `window_enumerate_unsupported_probe`).
#[test]
fn window_enumerate_roundtrip_returns_window_with_title() {
    let lib = load();
    let list: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };

    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    if window_enumerate_unsupported_probe(lib, st, "agt_window_enumerate roundtrip") {
        return;
    }
    if empty_list_probe_ok(st, required, "windows") {
        return;
    }
    assert_eq!(
        st, AGT_FAILED,
        "cap=0 probe must return AGT_FAILED (buffer_too_small), got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        required > 0,
        "the desktop must expose at least one top-level window, got {required}"
    );

    // Allocate and fill. The window set can change between the two calls, so
    // on a larger fresh count re-allocate and retry (same pattern as
    // agt_process_list).
    let mut capacity = required + 32;
    let windows = loop {
        assert!(
            capacity < 1_000_000,
            "window count exploded far beyond the probe result"
        );
        let mut recs = vec![agt_window_info::default(); capacity];
        let mut got = 0usize;
        let st = unsafe { list(recs.as_mut_ptr(), capacity, &mut got) };
        if st == AGT_OK {
            assert!(
                got <= capacity,
                "out_count {got} exceeds capacity {capacity}"
            );
            recs.truncate(got);
            break recs;
        }
        assert_eq!(
            st,
            AGT_FAILED,
            "agt_window_enumerate failed: {}",
            last_error_message(lib)
        );
        let msg = last_error_message(lib);
        assert!(
            msg.contains("buffer_too_small"),
            "expected code \"buffer_too_small\" in error, got: {msg}"
        );
        assert!(
            got > capacity,
            "out_count must report a required count > capacity, got {got} <= {capacity}"
        );
        capacity = got + 32;
    };
    assert!(
        windows.iter().any(|w| w.title_len > 0),
        "at least one enumerated window must carry a non-empty title"
    );
}

/// Milestone 43: a deliberately too-small cap returns
/// AGT_FAILED{code="buffer_too_small"} and *out_count reports the required
/// (larger) count. On a headless host the mechanism is absent: the probe must
/// return AGT_UNSUPPORTED and the test skips.
#[test]
fn window_enumerate_small_cap_reports_required_count() {
    let lib = load();
    let list: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    if window_enumerate_unsupported_probe(lib, st, "agt_window_enumerate small_cap") {
        return;
    }
    if empty_list_probe_ok(st, required, "windows") {
        return;
    }
    assert_eq!(st, AGT_FAILED);
    if required == 1 {
        // A single-window host cannot demonstrate the small-cap path with
        // cap=1 (cap=1 is enough for one window and returns AGT_OK); the
        // cap=0 probe above already covered the buffer_too_small contract.
        eprintln!("SKIP (only 1 window in this environment): small_cap needs n >= 2");
        return;
    }
    assert!(
        required > 1,
        "the desktop must expose more than one top-level window, got {required}"
    );

    let mut one = [agt_window_info::default(); 1];
    let mut got = 0usize;
    let st = unsafe { list(one.as_mut_ptr(), 1, &mut got) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        got > 1,
        "out_count must report the required count > 1, got {got}"
    );
}

/// Milestone 43: NULL out_count -> AGT_FAILED{code="bad_pointer"}.
#[test]
fn window_enumerate_rejects_null_out_count() {
    let lib = load();
    let list: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
    let mut one = [agt_window_info::default(); 1];
    let st = unsafe { list(one.as_mut_ptr(), 1, std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 45: two-stage `agt_screen_list` round trip. Probes with
/// cap=0/buf=NULL (the legal "how big?" probe), allocates the required count,
/// calls again and asserts AGT_OK with at least one screen carrying a
/// non-empty frame and exactly one primary screen (platform contract).
/// `agt_screen_list` reuses the WINDOW_ENUMERATE mechanism: on a headless host
/// the probe must return AGT_UNSUPPORTED and the test skips.
#[test]
fn screen_list_roundtrip_reports_valid_screens() {
    let lib = load();
    let list: Symbol<ScreenList> = unsafe { sym(lib, b"agt_screen_list") };

    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    if window_enumerate_unsupported_probe(lib, st, "agt_screen_list roundtrip") {
        return;
    }
    if empty_list_probe_ok(st, required, "screens") {
        return;
    }
    assert_eq!(
        st, AGT_FAILED,
        "cap=0 probe must return AGT_FAILED (buffer_too_small), got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        required >= 1,
        "the host must expose at least one screen, got {required}"
    );

    let mut screens = vec![agt_screen_info::default(); required];
    let mut got = 0usize;
    let st = unsafe { list(screens.as_mut_ptr(), required, &mut got) };
    assert_eq!(
        st,
        AGT_OK,
        "second-stage screen list failed: {}",
        last_error_message(lib)
    );
    assert!(
        got <= required,
        "out_count {got} exceeds capacity {required}"
    );
    screens.truncate(got);
    assert!(
        screens
            .iter()
            .any(|s| s.frame_width > 0 && s.frame_height > 0),
        "at least one screen must have a non-empty frame"
    );
    assert_eq!(
        screens.iter().filter(|s| s.primary == 1).count(),
        1,
        "exactly one screen must be primary (got {} screens)",
        screens.len()
    );
}

/// Milestone 45: a deliberately too-small cap returns
/// AGT_FAILED{code="buffer_too_small"} and *out_count reports the required
/// (larger) count. On a headless host the mechanism is absent: the probe must
/// return AGT_UNSUPPORTED and the test skips.
#[test]
fn screen_list_small_cap_reports_required_count() {
    let lib = load();
    let list: Symbol<ScreenList> = unsafe { sym(lib, b"agt_screen_list") };
    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    if window_enumerate_unsupported_probe(lib, st, "agt_screen_list small_cap") {
        return;
    }
    if empty_list_probe_ok(st, required, "screens") {
        return;
    }
    assert_eq!(st, AGT_FAILED, "cap=0 probe must fail, got {st}");
    assert!(
        required >= 1,
        "the host must expose at least one screen, got {required}"
    );
    if required == 1 {
        // A single-screen host cannot demonstrate the small-cap path with
        // cap=1; the cap=0 probe above already covers the buffer_too_small
        // contract.
        eprintln!("SKIP (only 1 screen in this environment): small_cap needs n >= 2");
        return;
    }
    let mut one = [agt_screen_info::default(); 1];
    let mut got = 0usize;
    let st = unsafe { list(one.as_mut_ptr(), 1, &mut got) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        got > 1,
        "out_count must report the required count > 1, got {got}"
    );
}

/// Milestone 45: NULL out_count -> AGT_FAILED{code="bad_pointer"}.
#[test]
fn screen_list_rejects_null_out_count() {
    let lib = load();
    let list: Symbol<ScreenList> = unsafe { sym(lib, b"agt_screen_list") };
    let mut one = [agt_screen_info::default(); 1];
    let st = unsafe { list(one.as_mut_ptr(), 1, std::ptr::null_mut()) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_pointer"),
        "expected code \"bad_pointer\" in error, got: {msg}"
    );
}

/// Milestone 45: `agt_a11y_drain_bus()` returns AGT_OK (mechanism present)
/// or AGT_UNSUPPORTED (mechanism absent on this build/host) — never
/// AGT_FAILED (the export has no failure path other than a caught panic).
#[test]
fn a11y_drain_bus_ok_or_unsupported_never_failed() {
    let lib = load();
    let drain: Symbol<A11yDrainBus> = unsafe { sym(lib, b"agt_a11y_drain_bus") };
    let st = unsafe { drain() };
    assert!(
        st == AGT_OK || st == AGT_UNSUPPORTED,
        "agt_a11y_drain_bus must be AGT_OK or AGT_UNSUPPORTED, got {st}"
    );
}

/// Milestone 45: two-stage `agt_a11y_last_text_write_via` round trip. Probes
/// with cap=0 for the required byte count, allocates, reads back and asserts
/// the payload is valid UTF-8. The exact content is a diagnostic string that
/// may change, so it is never asserted. When the mechanism is absent the
/// first probe returns AGT_UNSUPPORTED and the test accepts that.
#[test]
fn a11y_last_text_write_via_two_stage_utf8() {
    let lib = load();
    let via: Symbol<A11yLastTextWriteVia> = unsafe { sym(lib, b"agt_a11y_last_text_write_via") };
    let mut required = 0usize;
    let st = unsafe { via(std::ptr::null_mut(), 0, &mut required) };
    if st == AGT_UNSUPPORTED {
        return;
    }
    assert_eq!(
        st, AGT_FAILED,
        "cap=0 probe must return AGT_FAILED (buffer_too_small), got {st}"
    );
    let msg = last_error_message(lib);
    assert!(
        msg.contains("buffer_too_small"),
        "expected code \"buffer_too_small\" in error, got: {msg}"
    );
    assert!(
        required > 0,
        "the diagnostic string must be non-empty, got {required}"
    );
    let mut buf = vec![0u8; required];
    let mut got = 0usize;
    let st = unsafe { via(buf.as_mut_ptr(), required, &mut got) };
    assert_eq!(
        st,
        AGT_OK,
        "second-stage read failed: {}",
        last_error_message(lib)
    );
    assert_eq!(got, required, "out_len {got} != required {required}");
    assert!(
        std::str::from_utf8(&buf).is_ok(),
        "the diagnostic string must be valid UTF-8"
    );
}

/// Milestone 43: `agt_native_window_show(0, 1)` -> AGT_FAILED{code="bad_handle"}.
/// Only the handle-0 rejection path is exercised — no real window is touched.
#[test]
fn native_window_show_rejects_zero_handle() {
    let lib = load();
    let show: Symbol<NativeWindowShow> = unsafe { sym(lib, b"agt_native_window_show") };
    let st = unsafe { show(0, 1) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_handle"),
        "expected code \"bad_handle\" in error, got: {msg}"
    );
}

/// Milestone 43: `agt_native_window_show(<valid handle>, 99)` ->
/// AGT_FAILED{code="bad_state"}. The handle comes from a real enumeration,
/// but the state is invalid, so the state validation rejects the call before
/// any platform call — the window is never actually moved. On a headless host
/// the enumeration step itself is unavailable: the probe must return
/// AGT_UNSUPPORTED and the test skips (the pure parameter rejection is
/// covered by `native_window_show_rejects_zero_handle`, which validates
/// before the capability check).
#[test]
fn native_window_show_rejects_bad_state() {
    let lib = load();
    let list: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
    let show: Symbol<NativeWindowShow> = unsafe { sym(lib, b"agt_native_window_show") };

    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    if window_enumerate_unsupported_probe(lib, st, "agt_native_window_show bad_state") {
        return;
    }
    if empty_list_probe_ok(st, required, "windows") {
        return;
    }
    assert_eq!(st, AGT_FAILED, "cap=0 probe must fail, got {st}");

    // The window set can change between the two calls, so re-allocate and
    // retry on a larger fresh count (same pattern as agt_process_list).
    let mut capacity = required + 32;
    let first_handle = loop {
        assert!(
            capacity < 1_000_000,
            "window count exploded far beyond the probe result"
        );
        let mut recs = vec![agt_window_info::default(); capacity];
        let mut got = 0usize;
        let st = unsafe { list(recs.as_mut_ptr(), capacity, &mut got) };
        if st == AGT_OK {
            assert!(got > 0, "second-stage enumerate returned no records");
            let handle = recs[0].handle;
            assert!(handle != 0, "enumerated handle must be non-zero");
            break handle;
        }
        assert_eq!(
            st,
            AGT_FAILED,
            "second-stage enumerate failed: {}",
            last_error_message(lib)
        );
        let msg = last_error_message(lib);
        assert!(
            msg.contains("buffer_too_small"),
            "expected code \"buffer_too_small\" in error, got: {msg}"
        );
        assert!(
            got > capacity,
            "out_count must report a required count > capacity, got {got} <= {capacity}"
        );
        capacity = got + 32;
    };

    let st = unsafe { show(first_handle, 99) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_state"),
        "expected code \"bad_state\" in error, got: {msg}"
    );
}

/// Milestone 43: `agt_input_pointer_click(0, 0, 99, 1)` ->
/// AGT_FAILED{code="bad_button"}. The invalid button is rejected before any
/// platform call, so nothing is ever clicked on the user's desktop.
#[test]
fn input_pointer_click_rejects_bad_button() {
    let lib = load();
    let click: Symbol<InputPointerClick> = unsafe { sym(lib, b"agt_input_pointer_click") };
    let st = unsafe { click(0, 0, 99, 1) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_button"),
        "expected code \"bad_button\" in error, got: {msg}"
    );
}

/// Milestone 43: `agt_input_type_text(NULL, 5)` ->
/// AGT_FAILED{code="bad_text"} (pointer validated before any platform call —
/// nothing is typed).
#[test]
fn input_type_text_rejects_null_text() {
    let lib = load();
    let type_text: Symbol<InputTypeText> = unsafe { sym(lib, b"agt_input_type_text") };
    let st = unsafe { type_text(std::ptr::null(), 5) };
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(lib);
    assert!(
        msg.contains("bad_text"),
        "expected code \"bad_text\" in error, got: {msg}"
    );
}
