//! Milestone 65 regression: `agt_window_enumerate` must RETURN while this
//! process hosts an open window from `agt_window_open`.
//!
//! Windows defect (milestone 64b): the enumerator queried every visible
//! top-level window's caption with `GetWindowTextW`. For a window owned by
//! THIS process (any thread) that call sends `WM_GETTEXT` and blocks until
//! the owning thread pumps it. The ABI pixel-window loop thread parks at the
//! frame rendezvous (`wait_commit_or_close` — a condvar wait with no message
//! pump) while the caller runs enumerate, so the two wait on each other
//! forever: open + enumerate deadlocked, the process never exited and the
//! window stayed on the desktop. Any OTHER hung top-level window on the
//! desktop froze the whole enumeration the same way. Fixed (milestone 65):
//! the caption query has a hard time bound (`SendMessageTimeoutW` +
//! `SMTO_ABORTIFHUNG`, 100 ms) and a timeout is not an error — that window's
//! row is emitted with an empty title and enumeration continues.
//!
//! The test only exercises `agt_window_enumerate` and `agt_window_open` /
//! `agt_window_close` (never `agt_native_window_*` or `agt_input_*`). One
//! extra call, `agt_frame_begin`, deterministically parks the loop thread at
//! the frame rendezvous that used to deadlock: without it the enumerate can
//! race ahead of the loop thread's first render and pass even on the broken
//! code, so the regression would not catch a reintroduction.
//!
//! Watchdog: the enumerate runs on a worker thread and the test waits on a
//! channel with a 30-second bound. A timeout means the deadlock is still
//! present: the test prints a diagnosis and force-exits with
//! `std::process::exit(1)`. The worker is stuck inside the dylib (no join
//! possible) and the loop thread may hold an open window, so exiting the
//! process is the only way to guarantee the CI runner does not hang and no
//! orphan window or process survives (the OS reclaims both at process
//! teardown).
//!
//! Capability guard: when `AGT_CAP_WINDOW_HOST` or
//! `AGT_CAP_WINDOW_ENUMERATE` report `AGT_UNSUPPORTED` (headless CI, macOS —
//! AppKit needs the window loop on the main thread, see `agt_window_open`),
//! the test asserts the corresponding mechanism call answers
//! `AGT_UNSUPPORTED` and prints `SKIP:` — the skip branch is asserted too,
//! never a bare return.

use common::capabilities::{AGT_CAP_WINDOW_ENUMERATE, AGT_CAP_WINDOW_HOST};
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

mod common;

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;
/// Bounds on the enumerate worker; a timeout is judged a deadlock.
const WATCHDOG: Duration = Duration::from_secs(30);

// --- C ABI mirrors (layout must match include/agenterm.h) ----------------

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

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
struct agt_frame_desc {
    pixels: *mut u32,
    width: u32,
    height: u32,
    stride_px: u32,
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

type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type WindowOpen = unsafe extern "C" fn(*const agt_window_spec, *mut *mut std::ffi::c_void) -> i32;
type WindowClose = unsafe extern "C" fn(*mut std::ffi::c_void);
type FrameBegin = unsafe extern "C" fn(*mut std::ffi::c_void, *mut agt_frame_desc, u32) -> i32;
type WindowEnumerate = unsafe extern "C" fn(*mut agt_window_info, usize, *mut usize) -> i32;
type ProcessSelf = unsafe extern "C" fn() -> u32;

/// Locate and leak the built cdylib. The handle is intentionally leaked: the
/// DLL's private threads (window-loop thread) may still be winding down when
/// the test function returns, so dropping it would unload the module out
/// from under them (same convention as `dylib_load.rs`).
fn load() -> &'static Library {
    let path = cdylib_path();
    let lib = unsafe { Library::new(&path) }
        .unwrap_or_else(|e| panic!("dlopen/LoadLibrary({path:?}) failed: {e}"));
    Box::leak(Box::new(lib))
}

fn cdylib_path() -> PathBuf {
    common::toolchain::locate_cdylib()
}

unsafe fn sym<'l, T>(lib: &'l Library, name: &[u8]) -> Symbol<'l, T> {
    unsafe { lib.get(name) }.unwrap_or_else(|e| panic!("symbol {name:?} missing: {e}"))
}

/// Read the thread-local error record as `operation: code: message`.
fn last_error_message(lib: &Library) -> String {
    #[repr(C)]
    #[derive(Clone, Copy)]
    #[allow(non_camel_case_types)]
    struct agt_error {
        operation: *const c_char,
        code: *const c_char,
        message: *const c_char,
    }
    type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;
    let f: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
    let mut e = agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    let st = unsafe { f(&mut e) };
    if st != AGT_OK {
        return format!("agt_last_error failed with {st}");
    }
    let mut parts = Vec::new();
    for (label, p) in [
        ("operation", e.operation),
        ("code", e.code),
        ("message", e.message),
    ] {
        if p.is_null() {
            parts.push(format!("{label}=<null>"));
        } else {
            let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
            parts.push(format!("{label}={s}"));
        }
    }
    parts.join("; ")
}

/// SKIP-branch assertion for a headless host: when `AGT_CAP_WINDOW_HOST` is
/// unavailable, `agt_window_open` itself must answer `AGT_UNSUPPORTED`.
fn assert_open_unsupported(open: &Symbol<WindowOpen>) {
    let title = CString::new("agenterm-m65-skip-probe").expect("no NUL");
    let spec = agt_window_spec {
        title: title.as_ptr(),
        width: 8,
        height: 8,
        no_activate: 1,
        ime_allowed: 0,
    };
    let mut window: *mut std::ffi::c_void = std::ptr::null_mut();
    let st = unsafe { open(&spec, &mut window) };
    assert_eq!(
        st, AGT_UNSUPPORTED,
        "agt_window_open must answer AGT_UNSUPPORTED when AGT_CAP_WINDOW_HOST \
         reports unsupported, got {st}"
    );
}

/// SKIP-branch assertion for a headless host: when
/// `AGT_CAP_WINDOW_ENUMERATE` is unavailable, the cap=0 probe must answer
/// `AGT_UNSUPPORTED` (never `AGT_OK`/`AGT_FAILED`).
fn assert_enumerate_unsupported(list: &Symbol<WindowEnumerate>) {
    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    assert_eq!(
        st, AGT_UNSUPPORTED,
        "agt_window_enumerate cap=0 probe must answer AGT_UNSUPPORTED when \
         AGT_CAP_WINDOW_ENUMERATE reports unsupported, got {st}"
    );
}

/// The full two-stage `agt_window_enumerate` round trip, run on the worker
/// thread. Returns (probe_required, records_written, records).
fn two_stage_enumerate(list: &Symbol<WindowEnumerate>) -> (usize, usize, Vec<agt_window_info>) {
    let mut required = 0usize;
    let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
    // cap=0 is the legal "how big?" probe: AGT_FAILED{buffer_too_small} with
    // required > 0, or AGT_OK with required == 0 on an empty desktop.
    match st {
        AGT_FAILED => assert!(
            required > 0,
            "buffer_too_small probe must report required > 0, got {required}"
        ),
        AGT_OK => assert_eq!(required, 0, "AGT_OK probe must mean 0 windows"),
        other => panic!("cap=0 probe answered {other}"),
    }
    if required == 0 {
        return (0, 0, Vec::new());
    }
    let mut recs = vec![agt_window_info::default(); required];
    let mut got = 0usize;
    let st = unsafe { list(recs.as_mut_ptr(), required, &mut got) };
    assert_eq!(st, AGT_OK, "second (fill) call failed with {st}");
    assert!(
        got <= required,
        "out_count {got} exceeds capacity {required}"
    );
    recs.truncate(got);
    (required, got, recs)
}

#[test]
fn enumerate_returns_while_hosting_a_window() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };
    let open: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
    let close: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
    let begin: Symbol<FrameBegin> = unsafe { sym(lib, b"agt_frame_begin") };
    let list: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
    let self_pid: Symbol<ProcessSelf> = unsafe { sym(lib, b"agt_process_self") };

    // Capability guard: a missing mechanism must answer AGT_UNSUPPORTED when
    // actually invoked, and the test prints SKIP (the skip branch is
    // asserted, never a bare return).
    let host_cap = unsafe { query(AGT_CAP_WINDOW_HOST) };
    let enum_cap = unsafe { query(AGT_CAP_WINDOW_ENUMERATE) };
    if host_cap == AGT_UNSUPPORTED {
        assert_open_unsupported(&open);
        eprintln!(
            "SKIP: AGT_CAP_WINDOW_HOST unsupported here (headless host / macOS); \
             open must answer AGT_UNSUPPORTED — asserted"
        );
        return;
    }
    if enum_cap == AGT_UNSUPPORTED {
        assert_enumerate_unsupported(&list);
        eprintln!(
            "SKIP: AGT_CAP_WINDOW_ENUMERATE unsupported here (headless host); \
             cap=0 probe must answer AGT_UNSUPPORTED — asserted"
        );
        return;
    }
    assert_eq!(
        host_cap, AGT_OK,
        "AGT_CAP_WINDOW_HOST must be AGT_OK, got {host_cap}"
    );
    assert_eq!(
        enum_cap, AGT_OK,
        "AGT_CAP_WINDOW_ENUMERATE must be AGT_OK, got {enum_cap}"
    );

    // Open a window. Its loop thread renders the first frame and parks at
    // the frame rendezvous waiting for the caller — the exact state that
    // deadlocked the enumerator before the fix.
    let title = CString::new("agenterm-m65-enumerate-while-host").expect("no NUL");
    let spec = agt_window_spec {
        title: title.as_ptr(),
        width: 320,
        height: 200,
        no_activate: 1,
        ime_allowed: 0,
    };
    let mut window: *mut std::ffi::c_void = std::ptr::null_mut();
    let st = unsafe { open(&spec, &mut window) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_window_open failed (status {st}); last_error: {}",
        last_error_message(lib)
    );
    assert!(!window.is_null(), "agt_window_open returned a null handle");

    // Deterministically park the loop thread at the frame rendezvous (the
    // wait_commit_or_close condvar): the first frame_begin only returns once
    // the first render has published a frame, so afterwards the loop thread
    // is blocked waiting for a commit that never comes (we never commit).
    // Without this the enumerate could race ahead of the loop thread's first
    // render and pass even on the broken code.
    let mut frame = agt_frame_desc {
        pixels: std::ptr::null_mut(),
        width: 0,
        height: 0,
        stride_px: 0,
    };
    let st = unsafe { begin(window, &mut frame, 10_000) };
    assert_eq!(
        st,
        AGT_OK,
        "agt_frame_begin failed (status {st}); last_error: {}",
        last_error_message(lib)
    );

    // The step that used to hang: full two-stage enumerate, bounded by the
    // watchdog. On timeout the deadlock is judged still present.
    let pid = unsafe { self_pid() };
    assert!(pid > 0, "agt_process_self must report a real pid, got 0");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = two_stage_enumerate(&list);
        let _ = tx.send(outcome);
    });
    let (required, got, recs) = match rx.recv_timeout(WATCHDOG) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            eprintln!(
                "FAIL: agt_window_enumerate did not return within {:?} while this \
                 process hosts an open window — the enumerate-while-hosting \
                 deadlock (milestone 64b) is still present.",
                WATCHDOG
            );
            eprintln!(
                "Forcing std::process::exit(1): the worker thread is stuck inside the \
                 dylib and cannot be joined, and the loop thread holds an open window; \
                 exiting the process terminates every thread so the CI runner cannot \
                 hang and no orphan window or process survives."
            );
            std::process::exit(1);
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            eprintln!("FAIL: enumerate worker panicked (see its panic output above)");
            std::process::exit(1);
        }
    };

    // The set must contain our own window's record. Title may be empty —
    // never assert on title content.
    let mine = recs
        .iter()
        .find(|r| r.process_id == pid)
        .expect("enumerate must contain our own window record (process_id == agt_process_self())");
    eprintln!(
        "PASS: enumerate returned {got} records (probe required {required}); \
         our window pid={pid} hwnd=0x{:x} title_len={} found",
        mine.handle, mine.title_len
    );

    // Close the window exactly once. The frame was never committed; close
    // wakes the rendezvous waiter so the loop thread escapes and exits.
    unsafe { close(window) };
}
