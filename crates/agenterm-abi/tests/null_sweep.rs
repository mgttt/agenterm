//! Milestone 12 null/illegal-input sweep across the whole exported surface.
//!
//! Every export that takes a pointer parameter is called with NULL (plus the
//! degenerate `cap`/`len` combinations) and must:
//!
//! 1. **not crash the process** (a crash fails the test on its own);
//! 2. return `AGT_FAILED` or `AGT_UNSUPPORTED` — never `AGT_OK` (a NULL that
//!    "succeeds" means the export either skipped the check or treats NULL as
//!    legal input);
//! 3. leave a readable thread-local error record: after the call
//!    `agt_last_error` must yield three non-empty, `CStr`-parseable C strings.
//!
//! The legal "how big?" probe (`buf == NULL, cap == 0`) is swept separately
//! (`Kind::Probe`): it may return `AGT_OK` or `AGT_FAILED`, and is never
//! mixed with the strict assertions above.
//!
//! Safety boundaries (hard, from the brief): `agt_process_kill` is only ever
//! called with pid `0`; handle-class parameters (window / PTY / frame /
//! native window) are only ever NULL — no fake handle is constructed;
//! `agt_screenshot_*` never receives a real path; the real clipboard is never
//! modified (`agt_clipboard_set_text` only gets NULL); nothing may block, so
//! every `timeout_ms` is `0`.
//!
//! Known finding (reported, not silently fixed, not asserted away):
//! `agt_runtime_env_present(NULL, len)` returns `0` — numerically equal to
//! `AGT_OK` — because it is an `i32` environment *query* (NULL name = "not
//! present", documented and asserted by `tests/dylib_load.rs`), not an
//! `agt_status` return. Those cases live in the `#[ignore]`d test below so
//! the strict sweep table never conflates the two semantics; the owner
//! decides whether to keep or change the query behavior.

use libloading::{Library, Symbol};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;

// --- C ABI mirrors (layout must match include/agenterm.h) ----------------

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
struct agt_window_spec {
    title: *const c_char,
    width: u32,
    height: u32,
    no_activate: i32,
    ime_allowed: i32,
}

// Pointer-only parameters in the sweep are never constructed — they are only
// ever passed as NULL — so the mirrors below are zero-sized placeholders.
#[repr(C)]
#[allow(non_camel_case_types)]
struct agt_event;
#[repr(C)]
#[allow(non_camel_case_types)]
struct agt_frame_desc;
#[repr(C)]
#[allow(non_camel_case_types)]
struct agt_process_info;
#[repr(C)]
#[allow(non_camel_case_types)]
struct agt_a11y_node;

// --- export fn types -----------------------------------------------------

type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;
type PtyOpen = unsafe extern "C" fn(*const agt_pty_spawn, *mut *mut std::ffi::c_void) -> i32;
type PtyRead = unsafe extern "C" fn(*mut std::ffi::c_void, *mut u8, usize, *mut usize) -> i32;
type PtyWrite = unsafe extern "C" fn(*mut std::ffi::c_void, *const u8, usize, *mut usize) -> i32;
type PtyResize = unsafe extern "C" fn(*mut std::ffi::c_void, u16, u16) -> i32;
type PtyWait = unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut i32) -> i32;
type PtyClose = unsafe extern "C" fn(*mut std::ffi::c_void);
type WindowOpen = unsafe extern "C" fn(*const agt_window_spec, *mut *mut std::ffi::c_void) -> i32;
type WindowPollEvent = unsafe extern "C" fn(*mut std::ffi::c_void, *mut agt_event, u32) -> i32;
type WindowEventText =
    unsafe extern "C" fn(*mut std::ffi::c_void, *mut u8, usize, *mut usize) -> i32;
type WindowRequestRedraw = unsafe extern "C" fn(*mut std::ffi::c_void) -> i32;
type FrameBegin = unsafe extern "C" fn(*mut std::ffi::c_void, *mut agt_frame_desc, u32) -> i32;
type FrameCommit = unsafe extern "C" fn(*mut std::ffi::c_void) -> i32;
type WindowMetrics =
    unsafe extern "C" fn(*mut std::ffi::c_void, *mut u32, *mut u32, *mut f64) -> i32;
type WindowClose = unsafe extern "C" fn(*mut std::ffi::c_void);
type ScreenshotWritePng = unsafe extern "C" fn(*const c_char, *const u32, usize, u32, u32) -> i32;
type ScreenshotCaptureWindow =
    unsafe extern "C" fn(isize, *const c_char, i32, i32, i32, i32, i32) -> i32;
type ProcessList = unsafe extern "C" fn(*mut agt_process_info, usize, *mut usize) -> i32;
type ProcessKill = unsafe extern "C" fn(u32) -> i32;
type A11yTreeSnapshot = unsafe extern "C" fn(isize, *mut usize) -> i32;
type A11yTreeMetaString = unsafe extern "C" fn(i32, *mut u8, usize, *mut usize) -> i32;
type A11yTreeNode = unsafe extern "C" fn(usize, *mut agt_a11y_node) -> i32;
type A11yNodeString = unsafe extern "C" fn(usize, i32, *mut u8, usize, *mut usize) -> i32;
type A11yNodeActionName = unsafe extern "C" fn(usize, usize, *mut u8, usize, *mut usize) -> i32;
type A11yNodePerform = unsafe extern "C" fn(isize, *const c_char, i32) -> i32;
type ClipboardSetText = unsafe extern "C" fn(*const u8, usize) -> i32;
type ClipboardGetText = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeUserConfigDir = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeDefaultShell = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeEnvPresent = unsafe extern "C" fn(*const u8, usize) -> i32;
type ParentConsoleWrite = unsafe extern "C" fn(*const u8, usize) -> i32;
type RuntimeArgCount = unsafe extern "C" fn(*mut usize) -> i32;
type RuntimeArg = unsafe extern "C" fn(usize, *mut u8, usize, *mut usize) -> i32;

// --- dylib loading (same pattern as tests/dylib_load.rs) -----------------

/// Locate the cdylib built under the active profile. The test binary lives in
/// `target/<profile>/deps/`, the cdylib in `target/<profile>/`.
fn cdylib_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe()");
    let deps = exe.parent().expect("test binary has a parent dir");
    let profile_dir = deps.parent().expect("deps dir has a parent dir");
    const CANDIDATES: [&str; 3] = [
        "agenterm_abi.dll",      // Windows
        "libagenterm_abi.so",    // Linux
        "libagenterm_abi.dylib", // macOS
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

/// Load the cdylib and leak the `Library` handle (the DLL's private threads
/// may still be winding down when a test returns; leaking keeps the module
/// resident for the whole test process lifetime).
fn load() -> &'static Library {
    let path = cdylib_path();
    let lib = unsafe { Library::new(&path) }
        .unwrap_or_else(|e| panic!("dlopen/LoadLibrary({path:?}) failed: {e}"));
    Box::leak(Box::new(lib))
}

unsafe fn sym<'l, T>(lib: &'l Library, name: &[u8]) -> Symbol<'l, T> {
    unsafe { lib.get(name) }.unwrap_or_else(|e| panic!("symbol {name:?} missing: {e}"))
}

/// Assert that `agt_last_error` yields three non-empty, CStr-parseable C
/// strings after the sweep call. Panics (failing the test) on any violation.
fn check_last_error_readable(lib: &Library, context: &str) {
    let f: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
    let mut e = agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    let st = unsafe { f(&mut e) };
    assert_eq!(st, AGT_OK, "{context}: agt_last_error itself failed: {st}");
    for (name, ptr) in [
        ("operation", e.operation),
        ("code", e.code),
        ("message", e.message),
    ] {
        assert!(!ptr.is_null(), "{context}: agt_last_error.{name} is null");
        let s = unsafe { CStr::from_ptr(ptr) };
        assert!(
            !s.to_bytes().is_empty(),
            "{context}: agt_last_error.{name} is an empty C string"
        );
    }
}

fn status_name(st: i32) -> String {
    match st {
        AGT_OK => "AGT_OK".to_owned(),
        AGT_UNSUPPORTED => "AGT_UNSUPPORTED".to_owned(),
        AGT_FAILED => "AGT_FAILED".to_owned(),
        other => format!("unknown status {other}"),
    }
}

// --- sweep table ---------------------------------------------------------

#[derive(Clone, Copy)]
enum Kind {
    /// NULL input must fail: `AGT_FAILED` or `AGT_UNSUPPORTED`, never `AGT_OK`.
    MustFail,
    /// void-returning export: only "does not crash" is asserted.
    VoidSafe,
    /// Legal `buf == NULL, cap == 0` probe: `AGT_OK` or `AGT_FAILED` both OK.
    Probe,
}

enum CallResult {
    Status(i32),
    Void,
}

struct SweepCase {
    /// `<symbol>[<combination>]`, e.g. `agt_process_list[buf=NULL,cap=1]`.
    label: &'static str,
    kind: Kind,
    call: Box<dyn Fn(&Library) -> CallResult>,
}

fn run_sweep(lib: &Library, cases: &[SweepCase], group: &str) {
    for case in cases {
        let result = (case.call)(lib);
        match (case.kind, result) {
            (Kind::VoidSafe, CallResult::Void) => {}
            (Kind::VoidSafe, CallResult::Status(st)) => {
                panic!(
                    "{group}: {} returned status {st}; expected a void call",
                    case.label
                )
            }
            (Kind::MustFail, CallResult::Status(st)) => {
                assert!(
                    st == AGT_FAILED || st == AGT_UNSUPPORTED,
                    "{group}: {} returned {}; must be AGT_FAILED/AGT_UNSUPPORTED, never AGT_OK",
                    case.label,
                    status_name(st),
                );
            }
            (Kind::MustFail, CallResult::Void) => {
                panic!("{group}: {} unexpectedly returned void", case.label)
            }
            (Kind::Probe, CallResult::Status(st)) => {
                assert!(
                    st == AGT_OK || st == AGT_FAILED || st == AGT_UNSUPPORTED,
                    "{group}: {} returned unexpected status {}",
                    case.label,
                    status_name(st),
                );
            }
            (Kind::Probe, CallResult::Void) => {
                panic!("{group}: {} unexpectedly returned void", case.label)
            }
        }
        check_last_error_readable(lib, &format!("{group}: {}", case.label));
    }
}

/// NUL-terminated empty C string (legal but useless path for
/// `agt_screenshot_write_png`, which still fails on the NULL pixels pointer
/// before any file is opened — so no real path ever reaches the filesystem).
fn empty_c_string() -> *const c_char {
    static EMPTY: [u8; 1] = [0];
    EMPTY.as_ptr() as *const c_char
}

/// A `agt_pty_spawn` whose `program` is NULL: pointer validation fails before
/// anything could be spawned.
fn pty_spawn_program_null() -> agt_pty_spawn {
    agt_pty_spawn {
        program: std::ptr::null(),
        argv: std::ptr::null(),
        argc: 0,
        cwd: std::ptr::null(),
        envp: std::ptr::null(),
        envc: 0,
        cols: 80,
        rows: 24,
    }
}

/// An `agt_window_spec` whose `title` is NULL: pointer validation fails
/// before any window host is started.
fn window_spec_title_null() -> agt_window_spec {
    agt_window_spec {
        title: std::ptr::null(),
        width: 640,
        height: 480,
        no_activate: 1,
        ime_allowed: 0,
    }
}

/// Group 1 — every pointer-taking export with all pointer parameters NULL.
/// `agt_runtime_env_present` is excluded here by design (see module docs and
/// the `#[ignore]`d test at the bottom).
fn null_group() -> Vec<SweepCase> {
    vec![
        SweepCase {
            label: "agt_last_error[out=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
                unsafe { CallResult::Status(f(std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_pty_open[spawn=NULL,out=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyOpen> = unsafe { sym(lib, b"agt_pty_open") };
                unsafe { CallResult::Status(f(std::ptr::null(), std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_pty_open[spawn=NULL,out=&h]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyOpen> = unsafe { sym(lib, b"agt_pty_open") };
                let mut h: *mut std::ffi::c_void = std::ptr::null_mut();
                unsafe { CallResult::Status(f(std::ptr::null(), &mut h)) }
            }),
        },
        SweepCase {
            label: "agt_pty_open[spawn.valid,program=NULL,out=&h]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyOpen> = unsafe { sym(lib, b"agt_pty_open") };
                let spawn = pty_spawn_program_null();
                let mut h: *mut std::ffi::c_void = std::ptr::null_mut();
                unsafe { CallResult::Status(f(&spawn, &mut h)) }
            }),
        },
        SweepCase {
            label: "agt_pty_read[pty=NULL,buf=NULL,cap=0,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyRead> = unsafe { sym(lib, b"agt_pty_read") };
                unsafe {
                    CallResult::Status(f(
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                    ))
                }
            }),
        },
        SweepCase {
            label: "agt_pty_write[pty=NULL,buf=NULL,len=0,written=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyWrite> = unsafe { sym(lib, b"agt_pty_write") };
                unsafe {
                    CallResult::Status(f(
                        std::ptr::null_mut(),
                        std::ptr::null(),
                        0,
                        std::ptr::null_mut(),
                    ))
                }
            }),
        },
        SweepCase {
            label: "agt_pty_resize[pty=NULL,cols=0,rows=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyResize> = unsafe { sym(lib, b"agt_pty_resize") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, 0)) }
            }),
        },
        SweepCase {
            label: "agt_pty_wait[pty=NULL,timeout_ms=0,exit_code=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyWait> = unsafe { sym(lib, b"agt_pty_wait") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_pty_close[pty=NULL]",
            kind: Kind::VoidSafe,
            call: Box::new(|lib| {
                let f: Symbol<PtyClose> = unsafe { sym(lib, b"agt_pty_close") };
                unsafe {
                    f(std::ptr::null_mut());
                }
                CallResult::Void
            }),
        },
        SweepCase {
            label: "agt_window_open[spec=NULL,out=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
                unsafe { CallResult::Status(f(std::ptr::null(), std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_window_open[spec=NULL,out=&h]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
                let mut h: *mut std::ffi::c_void = std::ptr::null_mut();
                unsafe { CallResult::Status(f(std::ptr::null(), &mut h)) }
            }),
        },
        SweepCase {
            label: "agt_window_open[spec.valid,title=NULL,out=&h]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowOpen> = unsafe { sym(lib, b"agt_window_open") };
                let spec = window_spec_title_null();
                let mut h: *mut std::ffi::c_void = std::ptr::null_mut();
                unsafe { CallResult::Status(f(&spec, &mut h)) }
            }),
        },
        SweepCase {
            label: "agt_window_poll_event[window=NULL,out=NULL,timeout_ms=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowPollEvent> = unsafe { sym(lib, b"agt_window_poll_event") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), std::ptr::null_mut(), 0)) }
            }),
        },
        SweepCase {
            label: "agt_window_event_text[window=NULL,buf=NULL,cap=0,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowEventText> = unsafe { sym(lib, b"agt_window_event_text") };
                unsafe {
                    CallResult::Status(f(
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                    ))
                }
            }),
        },
        SweepCase {
            label: "agt_window_request_redraw[window=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowRequestRedraw> =
                    unsafe { sym(lib, b"agt_window_request_redraw") };
                unsafe { CallResult::Status(f(std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_frame_begin[window=NULL,out=NULL,timeout_ms=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<FrameBegin> = unsafe { sym(lib, b"agt_frame_begin") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), std::ptr::null_mut(), 0)) }
            }),
        },
        SweepCase {
            label: "agt_frame_commit[window=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<FrameCommit> = unsafe { sym(lib, b"agt_frame_commit") };
                unsafe { CallResult::Status(f(std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_window_metrics[window=NULL,width=NULL,height=NULL,scale=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowMetrics> = unsafe { sym(lib, b"agt_window_metrics") };
                unsafe {
                    CallResult::Status(f(
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ))
                }
            }),
        },
        SweepCase {
            label: "agt_window_close[window=NULL]",
            kind: Kind::VoidSafe,
            call: Box::new(|lib| {
                let f: Symbol<WindowClose> = unsafe { sym(lib, b"agt_window_close") };
                unsafe {
                    f(std::ptr::null_mut());
                }
                CallResult::Void
            }),
        },
        SweepCase {
            label: "agt_screenshot_write_png[path=NULL,pixels=NULL,pc=0,w=0,h=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ScreenshotWritePng> =
                    unsafe { sym(lib, b"agt_screenshot_write_png") };
                unsafe { CallResult::Status(f(std::ptr::null(), std::ptr::null(), 0, 0, 0)) }
            }),
        },
        SweepCase {
            label: "agt_screenshot_write_png[path=\"\",pixels=NULL,pc=1,w=1,h=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ScreenshotWritePng> =
                    unsafe { sym(lib, b"agt_screenshot_write_png") };
                unsafe { CallResult::Status(f(empty_c_string(), std::ptr::null(), 1, 1, 1)) }
            }),
        },
        SweepCase {
            label: "agt_screenshot_capture_window[native=0,path=NULL,kind=0,l=0,t=0,w=0,h=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ScreenshotCaptureWindow> =
                    unsafe { sym(lib, b"agt_screenshot_capture_window") };
                unsafe { CallResult::Status(f(0, std::ptr::null(), 0, 0, 0, 0, 0)) }
            }),
        },
        SweepCase {
            label: "agt_process_list[buf=NULL,cap=0,out_count=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_process_kill[pid=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ProcessKill> = unsafe { sym(lib, b"agt_process_kill") };
                unsafe { CallResult::Status(f(0)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_tree_snapshot[window_handle=0,out_node_count=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yTreeSnapshot> = unsafe { sym(lib, b"agt_a11y_tree_snapshot") };
                unsafe { CallResult::Status(f(0, std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_a11y_tree_node[index=0,out=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yTreeNode> = unsafe { sym(lib, b"agt_a11y_tree_node") };
                unsafe { CallResult::Status(f(0, std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_perform[window_handle=0,node_id=NULL,action=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodePerform> = unsafe { sym(lib, b"agt_a11y_node_perform") };
                unsafe { CallResult::Status(f(0, std::ptr::null(), 0)) }
            }),
        },
        SweepCase {
            label: "agt_clipboard_set_text[text=NULL,len=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ClipboardSetText> = unsafe { sym(lib, b"agt_clipboard_set_text") };
                unsafe { CallResult::Status(f(std::ptr::null(), 0)) }
            }),
        },
        SweepCase {
            label: "agt_clipboard_get_text[buf=NULL,cap=0,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_runtime_user_config_dir[buf=NULL,cap=0,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeUserConfigDir> =
                    unsafe { sym(lib, b"agt_runtime_user_config_dir") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_runtime_default_shell[buf=NULL,cap=0,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeDefaultShell> =
                    unsafe { sym(lib, b"agt_runtime_default_shell") };
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_runtime_arg_count[out_count=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeArgCount> = unsafe { sym(lib, b"agt_runtime_arg_count") };
                unsafe { CallResult::Status(f(std::ptr::null_mut())) }
            }),
        },
        SweepCase {
            label: "agt_runtime_arg[index=0,buf=NULL,cap=0,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeArg> = unsafe { sym(lib, b"agt_runtime_arg") };
                unsafe { CallResult::Status(f(0, std::ptr::null_mut(), 0, std::ptr::null_mut())) }
            }),
        },
    ]
}

/// Group 2 — the legal "how big?" probe (`buf == NULL, cap == 0`): may return
/// `AGT_OK` or `AGT_FAILED`; the strict "never AGT_OK" assertion does not
/// apply here.
fn probe_group() -> Vec<SweepCase> {
    vec![
        SweepCase {
            label: "agt_pty_read[pty=NULL,buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<PtyRead> = unsafe { sym(lib, b"agt_pty_read") };
                let mut n = 0usize;
                unsafe {
                    CallResult::Status(f(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n))
                }
            }),
        },
        SweepCase {
            label: "agt_pty_write[pty=NULL,buf=NULL,len=0,written=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<PtyWrite> = unsafe { sym(lib, b"agt_pty_write") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), std::ptr::null(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_window_event_text[window=NULL,buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<WindowEventText> = unsafe { sym(lib, b"agt_window_event_text") };
                let mut n = 0usize;
                unsafe {
                    CallResult::Status(f(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n))
                }
            }),
        },
        SweepCase {
            label: "agt_screenshot_write_png[path=\"\",pixels=NULL,pc=0,w=0,h=0]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<ScreenshotWritePng> =
                    unsafe { sym(lib, b"agt_screenshot_write_png") };
                unsafe { CallResult::Status(f(empty_c_string(), std::ptr::null(), 0, 0, 0)) }
            }),
        },
        SweepCase {
            label: "agt_process_list[buf=NULL,cap=0,out_count=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_clipboard_get_text[buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_tree_meta_string[field=0,buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<A11yTreeMetaString> =
                    unsafe { sym(lib, b"agt_a11y_tree_meta_string") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_string[index=0,kind=0,buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeString> = unsafe { sym(lib, b"agt_a11y_node_string") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, 0, std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_action_name[index=0,action=0,buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeActionName> =
                    unsafe { sym(lib, b"agt_a11y_node_action_name") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, 0, std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_runtime_user_config_dir[buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeUserConfigDir> =
                    unsafe { sym(lib, b"agt_runtime_user_config_dir") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_runtime_default_shell[buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeDefaultShell> =
                    unsafe { sym(lib, b"agt_runtime_default_shell") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_runtime_arg[index=0,buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeArg> = unsafe { sym(lib, b"agt_runtime_arg") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, std::ptr::null_mut(), 0, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_parent_console_write_stdout[text=NULL,len=0]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<ParentConsoleWrite> =
                    unsafe { sym(lib, b"agt_parent_console_write_stdout") };
                unsafe { CallResult::Status(f(std::ptr::null(), 0)) }
            }),
        },
        SweepCase {
            label: "agt_parent_console_write_stderr[text=NULL,len=0]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<ParentConsoleWrite> =
                    unsafe { sym(lib, b"agt_parent_console_write_stderr") };
                unsafe { CallResult::Status(f(std::ptr::null(), 0)) }
            }),
        },
    ]
}

/// Group 3 — illegal `buf == NULL, cap > 0`: must return `AGT_FAILED` (or
/// `AGT_UNSUPPORTED`), never `AGT_OK`.
fn cap_group() -> Vec<SweepCase> {
    vec![
        SweepCase {
            label: "agt_pty_read[pty=NULL,buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyRead> = unsafe { sym(lib, b"agt_pty_read") };
                let mut n = 0usize;
                unsafe {
                    CallResult::Status(f(std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut n))
                }
            }),
        },
        SweepCase {
            label: "agt_pty_write[pty=NULL,buf=NULL,len=1,written=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<PtyWrite> = unsafe { sym(lib, b"agt_pty_write") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), std::ptr::null(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_window_event_text[window=NULL,buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<WindowEventText> = unsafe { sym(lib, b"agt_window_event_text") };
                let mut n = 0usize;
                unsafe {
                    CallResult::Status(f(std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut n))
                }
            }),
        },
        SweepCase {
            label: "agt_screenshot_write_png[path=\"\",pixels=NULL,pc=1,w=1,h=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ScreenshotWritePng> =
                    unsafe { sym(lib, b"agt_screenshot_write_png") };
                unsafe { CallResult::Status(f(empty_c_string(), std::ptr::null(), 1, 1, 1)) }
            }),
        },
        SweepCase {
            label: "agt_process_list[buf=NULL,cap=1,out_count=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_clipboard_get_text[buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_tree_meta_string[field=0,buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yTreeMetaString> =
                    unsafe { sym(lib, b"agt_a11y_tree_meta_string") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_string[index=0,kind=0,buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeString> = unsafe { sym(lib, b"agt_a11y_node_string") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, 0, std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_action_name[index=0,action=0,buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeActionName> =
                    unsafe { sym(lib, b"agt_a11y_node_action_name") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, 0, std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_runtime_user_config_dir[buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeUserConfigDir> =
                    unsafe { sym(lib, b"agt_runtime_user_config_dir") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_runtime_default_shell[buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeDefaultShell> =
                    unsafe { sym(lib, b"agt_runtime_default_shell") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_runtime_arg[index=0,buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<RuntimeArg> = unsafe { sym(lib, b"agt_runtime_arg") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, std::ptr::null_mut(), 1, &mut n)) }
            }),
        },
        SweepCase {
            label: "agt_parent_console_write_stdout[text=NULL,len=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ParentConsoleWrite> =
                    unsafe { sym(lib, b"agt_parent_console_write_stdout") };
                unsafe { CallResult::Status(f(std::ptr::null(), 1)) }
            }),
        },
        SweepCase {
            label: "agt_parent_console_write_stderr[text=NULL,len=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ParentConsoleWrite> =
                    unsafe { sym(lib, b"agt_parent_console_write_stderr") };
                unsafe { CallResult::Status(f(std::ptr::null(), 1)) }
            }),
        },
        SweepCase {
            label: "agt_clipboard_set_text[text=NULL,len=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<ClipboardSetText> = unsafe { sym(lib, b"agt_clipboard_set_text") };
                unsafe { CallResult::Status(f(std::ptr::null(), 1)) }
            }),
        },
    ]
}

/// Milestone 12 sweep entry point: 33 pointer-taking exports + the
/// `agt_process_kill(pid=0)` safety boundary, 62 combinations in total.
#[test]
fn null_sweep_every_pointer_export() {
    let lib = load();
    run_sweep(&lib, &null_group(), "null");
    run_sweep(&lib, &probe_group(), "probe(cap=0)");
    run_sweep(&lib, &cap_group(), "cap>0");
}

/// Reported finding (not silently fixed, not asserted away): `agt_runtime_env_present`
/// returns `0` for NULL input — numerically equal to `AGT_OK` — because it is
/// an `i32` environment *query* (NULL name = "not present"), not an
/// `agt_status`. `tests/dylib_load.rs::runtime_env_present_probes_real_environment`
/// already asserts this exact behavior. `#[ignore]`d so the strict sweep above
/// stays unambiguous; the owner decides whether to keep the query semantics or
/// turn NULL into a failure.
#[test]
#[ignore = "design quirk: agt_runtime_env_present(NULL) returns 0 == AGT_OK numeric value; reported to owner (milestone 12)"]
fn runtime_env_present_null_returns_zero_design_quirk() {
    let lib = load();
    let present: Symbol<RuntimeEnvPresent> = unsafe { sym(&lib, b"agt_runtime_env_present") };
    // NULL + len == 0 and NULL + len > 0 both answer "not present" (0).
    assert_eq!(
        unsafe { present(std::ptr::null(), 0) },
        0,
        "NULL, len=0 must answer 0"
    );
    assert_eq!(
        unsafe { present(std::ptr::null(), 1) },
        0,
        "NULL, len>0 must answer 0"
    );
    // The error record is untouched by this query (it never records errors);
    // it must still be readable as three C strings.
    check_last_error_readable(&lib, "agt_runtime_env_present(NULL)");
}
