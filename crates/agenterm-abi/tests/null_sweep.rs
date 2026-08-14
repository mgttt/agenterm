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
//! Reviewed and kept as designed (milestone 13): `agt_runtime_env_present(NULL, len)`
//! returns `0` — numerically equal to `AGT_OK` — because it is an `i32`
//! environment *query* (NULL name = "not present", documented and asserted by
//! `tests/dylib_load.rs`), not an `agt_status` return. This is the intended
//! `int32_t` query semantics, not a defect. Those cases live in the
//! `#[ignore]`d test below so the strict sweep table never conflates the two
//! semantics.
//!
//! Milestone 63 closes the coverage gap: the sweep previously claimed to be a
//! "strict sweep" of the C boundary while silently skipping 18 exports (the
//! milestone 43/45 computer-use group and a few no-arg queries) and nothing
//! checked that. Now:
//!
//! - `sweep_covers_every_export_in_exports_txt` gates completeness: every
//!   export listed in `exports.txt` must be covered by the sweep table (the
//!   covered names are read out of the sweep table's own labels — never a
//!   parallel hand-written list) or appear in `EXEMPT_EXPORTS` with a
//!   signature-verified reason.
//! - The 11 genuinely missing pointer/handle-taking exports are swept with
//!   necessarily-failing input only (NULL / handle 0 / invalid button) —
//!   never a call that could succeed (see the safety-boundary notes on
//!   `null_group`).
//! - `computer_use_sweep_capability_guards` mirrors the `dylib_load.rs`
//!   capability-guard pattern: on headless hosts where a mechanism reports
//!   `AGT_UNSUPPORTED`, the guard asserts the real mechanism-absent behavior
//!   instead of letting the sweep pass vacuously.

use libloading::{Library, Symbol};
use std::collections::HashSet;
use std::ffi::{CStr, c_char};
use std::path::PathBuf;

/// Test-side AGT_CAP_* discriminants (the single hand-written test copy,
/// gated against the header and the Rust enum by `capability_enum_gate.rs` —
/// never re-typed here).
mod common;
use common::capabilities::{
    AGT_CAP_ACCESSIBILITY_TREE, AGT_CAP_INPUT_INJECT, AGT_CAP_WINDOW_ENUMERATE, AGT_CAP_WINDOW_OP,
};

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
#[repr(C)]
#[allow(non_camel_case_types)]
struct agt_window_info;
#[repr(C)]
#[allow(non_camel_case_types)]
struct agt_screen_info;

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
type A11yNodeSetText = unsafe extern "C" fn(isize, *const c_char, *const u8, usize) -> i32;
type A11yNodeGetText =
    unsafe extern "C" fn(isize, *const c_char, *mut u8, usize, *mut usize) -> i32;
type A11yNodeSendKeys = unsafe extern "C" fn(isize, *const c_char, *const u8, usize) -> i32;
type A11yNodeScroll = unsafe extern "C" fn(isize, *const c_char) -> i32;
type A11yNodeGetExtents =
    unsafe extern "C" fn(isize, *const c_char, *mut i32, *mut i32, *mut i32, *mut i32) -> i32;
type ClipboardSetText = unsafe extern "C" fn(*const u8, usize) -> i32;
type ClipboardGetText = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeUserConfigDir = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeDefaultShell = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type RuntimeEnvPresent = unsafe extern "C" fn(*const u8, usize) -> i32;
type ParentConsoleWrite = unsafe extern "C" fn(*const u8, usize) -> i32;
type RuntimeArgCount = unsafe extern "C" fn(*mut usize) -> i32;
type RuntimeArg = unsafe extern "C" fn(usize, *mut u8, usize, *mut usize) -> i32;
type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type WindowEnumerate = unsafe extern "C" fn(*mut agt_window_info, usize, *mut usize) -> i32;
type ScreenList = unsafe extern "C" fn(*mut agt_screen_info, usize, *mut usize) -> i32;
type A11yLastTextWriteVia = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type NativeWindowShow = unsafe extern "C" fn(isize, i32) -> i32;
type NativeWindowMove = unsafe extern "C" fn(isize, i32, i32, u32, u32) -> i32;
type NativeWindowRect = unsafe extern "C" fn(isize, *mut i32, *mut i32, *mut u32, *mut u32) -> i32;
type NativeWindowSetTopmost = unsafe extern "C" fn(isize, i32) -> i32;
type NativeWindowClose = unsafe extern "C" fn(isize) -> i32;
type InputPointerMove = unsafe extern "C" fn(i32, i32) -> i32;
type InputPointerClick = unsafe extern "C" fn(i32, i32, i32, u32) -> i32;
type InputText = unsafe extern "C" fn(*const u8, usize) -> i32;

// --- dylib loading (same pattern as tests/dylib_load.rs) -----------------

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

// --- milestone 43/45 computer-use exports (milestone 63 sweep additions) ---
//
// Safety boundary (hard, from the milestone 63 brief): every case below
// passes ONLY necessarily-failing input — NULL, handle 0, or an invalid
// button — so no call can ever succeed and really move the pointer / press
// keys / move-close-topmost a real window. No real window handle is ever
// enumerated and re-injected (that would be a success-path test, out of
// scope). The same helpers are reused by
// `computer_use_sweep_capability_guards`.

fn window_enumerate_bad_args(lib: &Library) -> i32 {
    let f: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
    unsafe { f(std::ptr::null_mut(), 1, std::ptr::null_mut()) }
}

fn window_enumerate_probe(lib: &Library) -> i32 {
    let f: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
    let mut n = 0usize;
    unsafe { f(std::ptr::null_mut(), 0, &mut n) }
}

fn window_enumerate_cap1(lib: &Library) -> i32 {
    let f: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
    let mut n = 0usize;
    unsafe { f(std::ptr::null_mut(), 1, &mut n) }
}

fn screen_list_bad_args(lib: &Library) -> i32 {
    let f: Symbol<ScreenList> = unsafe { sym(lib, b"agt_screen_list") };
    unsafe { f(std::ptr::null_mut(), 1, std::ptr::null_mut()) }
}

fn screen_list_probe(lib: &Library) -> i32 {
    let f: Symbol<ScreenList> = unsafe { sym(lib, b"agt_screen_list") };
    let mut n = 0usize;
    unsafe { f(std::ptr::null_mut(), 0, &mut n) }
}

fn screen_list_cap1(lib: &Library) -> i32 {
    let f: Symbol<ScreenList> = unsafe { sym(lib, b"agt_screen_list") };
    let mut n = 0usize;
    unsafe { f(std::ptr::null_mut(), 1, &mut n) }
}

fn a11y_last_text_write_via_bad_args(lib: &Library) -> i32 {
    let f: Symbol<A11yLastTextWriteVia> = unsafe { sym(lib, b"agt_a11y_last_text_write_via") };
    unsafe { f(std::ptr::null_mut(), 1, std::ptr::null_mut()) }
}

fn a11y_last_text_write_via_probe(lib: &Library) -> i32 {
    let f: Symbol<A11yLastTextWriteVia> = unsafe { sym(lib, b"agt_a11y_last_text_write_via") };
    let mut n = 0usize;
    unsafe { f(std::ptr::null_mut(), 0, &mut n) }
}

fn a11y_last_text_write_via_cap1(lib: &Library) -> i32 {
    let f: Symbol<A11yLastTextWriteVia> = unsafe { sym(lib, b"agt_a11y_last_text_write_via") };
    let mut n = 0usize;
    unsafe { f(std::ptr::null_mut(), 1, &mut n) }
}

fn native_window_show_handle0(lib: &Library) -> i32 {
    let f: Symbol<NativeWindowShow> = unsafe { sym(lib, b"agt_native_window_show") };
    unsafe { f(0, 0) }
}

fn native_window_move_handle0(lib: &Library) -> i32 {
    let f: Symbol<NativeWindowMove> = unsafe { sym(lib, b"agt_native_window_move") };
    unsafe { f(0, 0, 0, 0, 0) }
}

fn native_window_rect_handle0_null_outs(lib: &Library) -> i32 {
    let f: Symbol<NativeWindowRect> = unsafe { sym(lib, b"agt_native_window_rect") };
    unsafe {
        f(
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }
}

fn native_window_set_topmost_handle0(lib: &Library) -> i32 {
    let f: Symbol<NativeWindowSetTopmost> = unsafe { sym(lib, b"agt_native_window_set_topmost") };
    unsafe { f(0, 0) }
}

fn native_window_close_handle0(lib: &Library) -> i32 {
    let f: Symbol<NativeWindowClose> = unsafe { sym(lib, b"agt_native_window_close") };
    unsafe { f(0) }
}

/// Invalid `button` value: `agt_input_pointer_click` validates the button
/// *before any platform call* (documented in `include/agenterm.h`), so an
/// invalid button can never click.
fn input_pointer_click_bad_button(lib: &Library) -> i32 {
    let f: Symbol<InputPointerClick> = unsafe { sym(lib, b"agt_input_pointer_click") };
    unsafe { f(0, 0, 99, 1) }
}

fn input_type_text_null(lib: &Library) -> i32 {
    let f: Symbol<InputText> = unsafe { sym(lib, b"agt_input_type_text") };
    unsafe { f(std::ptr::null(), 1) }
}

fn input_send_keys_null(lib: &Library) -> i32 {
    let f: Symbol<InputText> = unsafe { sym(lib, b"agt_input_send_keys") };
    unsafe { f(std::ptr::null(), 1) }
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
            label: "agt_a11y_node_set_text[window_handle=0,node_id=NULL,text=NULL,len=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeSetText> = unsafe { sym(lib, b"agt_a11y_node_set_text") };
                unsafe { CallResult::Status(f(0, std::ptr::null(), std::ptr::null(), 1)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_get_text[window_handle=0,node_id=NULL,buf=NULL,cap=0,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeGetText> = unsafe { sym(lib, b"agt_a11y_node_get_text") };
                unsafe {
                    CallResult::Status(f(
                        0,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                    ))
                }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_send_keys[window_handle=0,node_id=NULL,keys=NULL,len=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeSendKeys> = unsafe { sym(lib, b"agt_a11y_node_send_keys") };
                unsafe { CallResult::Status(f(0, std::ptr::null(), std::ptr::null(), 1)) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_scroll[window_handle=0,node_id=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeScroll> = unsafe { sym(lib, b"agt_a11y_node_scroll") };
                unsafe { CallResult::Status(f(0, std::ptr::null())) }
            }),
        },
        SweepCase {
            label: "agt_a11y_node_get_extents[window_handle=0,node_id=NULL,outs=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeGetExtents> =
                    unsafe { sym(lib, b"agt_a11y_node_get_extents") };
                unsafe {
                    CallResult::Status(f(
                        0,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ))
                }
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
        // --- milestone 43/45 computer-use exports (milestone 63 additions) ---
        // Every case uses necessarily-failing input only (NULL / handle 0 /
        // invalid button); see the safety-boundary comment above the helper
        // fns. Argument validation precedes the mechanism check in all of
        // these except `agt_a11y_last_text_write_via` (mechanism check
        // first), so on headless hosts they still return `AGT_FAILED` — the
        // sweep exercises the real validation path, not a vacuous pass.
        SweepCase {
            label: "agt_a11y_last_text_write_via[buf=NULL,cap=1,out_len=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(a11y_last_text_write_via_bad_args(lib))),
        },
        SweepCase {
            label: "agt_window_enumerate[buf=NULL,cap=1,out_count=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(window_enumerate_bad_args(lib))),
        },
        SweepCase {
            label: "agt_screen_list[buf=NULL,cap=1,out_count=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(screen_list_bad_args(lib))),
        },
        SweepCase {
            label: "agt_native_window_show[handle=0,state=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(native_window_show_handle0(lib))),
        },
        SweepCase {
            label: "agt_native_window_move[handle=0,x=0,y=0,w=0,h=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(native_window_move_handle0(lib))),
        },
        SweepCase {
            label: "agt_native_window_rect[handle=0,x=NULL,y=NULL,w=NULL,h=NULL]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(native_window_rect_handle0_null_outs(lib))),
        },
        SweepCase {
            label: "agt_native_window_set_topmost[handle=0,topmost=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(native_window_set_topmost_handle0(lib))),
        },
        SweepCase {
            label: "agt_native_window_close[handle=0]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(native_window_close_handle0(lib))),
        },
        SweepCase {
            label: "agt_input_pointer_click[x=0,y=0,button=99,clicks=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(input_pointer_click_bad_button(lib))),
        },
        SweepCase {
            label: "agt_input_type_text[text=NULL,len=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(input_type_text_null(lib))),
        },
        SweepCase {
            label: "agt_input_send_keys[shortcut=NULL,len=1]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(input_send_keys_null(lib))),
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
            label: "agt_a11y_node_get_text[window_handle=0,node_id=/0,buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeGetText> = unsafe { sym(lib, b"agt_a11y_node_get_text") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, c"/0".as_ptr(), std::ptr::null_mut(), 0, &mut n)) }
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
        // Milestone 63 additions: the legal "how big?" probe of the milestone
        // 43/45 two-stage exports. On a headless host the probe reaches the
        // mechanism check after passing validation and answers
        // AGT_UNSUPPORTED; `computer_use_sweep_capability_guards` asserts
        // that explicitly. Zero side effects in both cases.
        SweepCase {
            label: "agt_window_enumerate[buf=NULL,cap=0,out_count=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| CallResult::Status(window_enumerate_probe(lib))),
        },
        SweepCase {
            label: "agt_screen_list[buf=NULL,cap=0,out_count=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| CallResult::Status(screen_list_probe(lib))),
        },
        SweepCase {
            label: "agt_a11y_last_text_write_via[buf=NULL,cap=0,out_len=&n]",
            kind: Kind::Probe,
            call: Box::new(|lib| CallResult::Status(a11y_last_text_write_via_probe(lib))),
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
            label: "agt_a11y_node_get_text[window_handle=0,node_id=/0,buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| {
                let f: Symbol<A11yNodeGetText> = unsafe { sym(lib, b"agt_a11y_node_get_text") };
                let mut n = 0usize;
                unsafe { CallResult::Status(f(0, c"/0".as_ptr(), std::ptr::null_mut(), 1, &mut n)) }
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
        // Milestone 63 additions: illegal `buf == NULL, cap > 0` for the
        // two-stage milestone 43/45 exports. `buf == NULL, cap == 1` fails
        // argument validation (bad_pointer) on every host except
        // `agt_a11y_last_text_write_via`, whose mechanism check runs first
        // (headless → AGT_UNSUPPORTED); both are covered by `Kind::MustFail`.
        SweepCase {
            label: "agt_window_enumerate[buf=NULL,cap=1,out_count=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(window_enumerate_cap1(lib))),
        },
        SweepCase {
            label: "agt_screen_list[buf=NULL,cap=1,out_count=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(screen_list_cap1(lib))),
        },
        SweepCase {
            label: "agt_a11y_last_text_write_via[buf=NULL,cap=1,out_len=&n]",
            kind: Kind::MustFail,
            call: Box::new(|lib| CallResult::Status(a11y_last_text_write_via_cap1(lib))),
        },
    ]
}

/// Milestone 12+63 sweep entry point: 44 pointer/handle-taking exports (the
/// original 33 + the 11 milestone 43/45 computer-use exports swept since
/// milestone 63) + the `agt_process_kill(pid=0)` safety boundary, 79
/// combinations in total.
#[test]
fn null_sweep_every_pointer_export() {
    let lib = load();
    run_sweep(lib, &null_group(), "null");
    run_sweep(lib, &probe_group(), "probe(cap=0)");
    run_sweep(lib, &cap_group(), "cap>0");
}

// --- milestone 63: coverage gate ----------------------------------------

/// Exports with no pointer parameter and no handle to pass invalid input to:
/// there is nothing to sweep with NULL. Every entry states the REAL signature
/// from `include/agenterm.h` (verified one by one, not guessed from the name)
/// that makes it exempt, and `sweep_covers_every_export_in_exports_txt`
/// re-checks the stated signature — so a pointer-taking export can never hide
/// behind a "no pointer args" claim (that check is what makes "exempt
/// everything" impossible). The only second channel is `KnownDesignQuirk`,
/// which requires an explicit reason naming the covering test.
#[derive(Clone, Copy)]
enum ExemptReason {
    /// The signature takes no pointer parameter and no handle: there is
    /// nothing to pass NULL / an illegal handle to.
    NoPointerArgs,
    /// Reviewed design exception (milestone 13): the export DOES take a
    /// pointer, but NULL has legal query semantics the strict sweep's "must
    /// fail" assert would reject. Explicitly listed here — never hidden —
    /// with the covering test named; the existing `#[ignore]`d
    /// `agt_runtime_env_present` test is kept as designed.
    KnownDesignQuirk(&'static str),
}

const EXEMPT_EXPORTS: &[(&str, &str, ExemptReason)] = &[
    (
        "agt_abi_version",
        "uint32_t agt_abi_version(void)",
        ExemptReason::NoPointerArgs,
    ),
    (
        "agt_build_id",
        "const char* agt_build_id(void)",
        ExemptReason::NoPointerArgs,
    ),
    (
        "agt_process_self",
        "uint32_t agt_process_self(void)",
        ExemptReason::NoPointerArgs,
    ),
    (
        "agt_capability_query",
        "agt_status agt_capability_query(agt_capability cap)",
        ExemptReason::NoPointerArgs,
    ),
    (
        "agt_clipboard_has_text",
        "int32_t agt_clipboard_has_text(void)",
        ExemptReason::NoPointerArgs,
    ),
    (
        "agt_a11y_drain_bus",
        "agt_status agt_a11y_drain_bus(void)",
        ExemptReason::NoPointerArgs,
    ),
    // Special case, not a signature loophole: `agt_input_pointer_move` takes
    // only coordinates — there is no NULL/illegal-handle equivalent that
    // necessarily fails, and on a host with the input-injection mechanism ANY
    // call really moves the pointer. The milestone 63 safety boundary forbids
    // constructing a call that could succeed, so the success path is out of
    // scope (a future round with a self-owned window). Its mechanism-absent
    // path is asserted by `computer_use_sweep_capability_guards`.
    (
        "agt_input_pointer_move",
        "agt_status agt_input_pointer_move(int32_t x, int32_t y)",
        ExemptReason::NoPointerArgs,
    ),
    // Known design quirk (milestone 13, reviewed and kept as designed): NULL
    // `name` answers "not present" (0, numerically equal to AGT_OK) — an
    // `int32_t` environment query, not an `agt_status`. The strict sweep
    // must-fail assert cannot apply; the behavior is asserted by the
    // `#[ignore]`d `runtime_env_present_null_returns_zero_design_quirk` test.
    (
        "agt_runtime_env_present",
        "int32_t agt_runtime_env_present(const uint8_t* name, size_t len)",
        ExemptReason::KnownDesignQuirk(
            "runtime_env_present_null_returns_zero_design_quirk (milestone 13 review)",
        ),
    ),
];

/// Extract the export symbol from a sweep-case label (`"<symbol>[<combo>]"`).
/// The coverage gate reads the covered set out of the sweep table's own
/// labels — never a parallel hand-written list, which is exactly the drift
/// this gate exists to catch.
fn label_symbol(label: &str) -> &str {
    let symbol = label.split('[').next().unwrap_or(label);
    assert!(
        symbol.starts_with("agt_"),
        "sweep label {label:?} must be '<symbol>[<combination>]'"
    );
    symbol
}

/// Milestone 63 coverage gate: every export in `exports.txt` must be covered
/// by the sweep table or appear in `EXEMPT_EXPORTS` with a signature-verified
/// reason. Fails by listing the unswept exports; also fails if an exempt
/// entry's parameter list contains a pointer (the "exempt everything"
/// backdoor) or if an export is both swept and exempted.
#[test]
fn sweep_covers_every_export_in_exports_txt() {
    let exports_txt = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("exports.txt");
    let text = std::fs::read_to_string(&exports_txt)
        .unwrap_or_else(|e| panic!("read {}: {e}", exports_txt.display()));
    let exports: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert!(
        exports.len() >= 55,
        "exports.txt should list the full export set (>= 55); found {}",
        exports.len()
    );

    // Covered set comes from the sweep table itself. One export legitimately
    // appears in several combination cases (e.g. `agt_pty_open` has three);
    // the gate collects the distinct symbol set.
    let mut swept: HashSet<&str> = HashSet::new();
    for case in null_group()
        .into_iter()
        .chain(probe_group())
        .chain(cap_group())
    {
        swept.insert(label_symbol(case.label));
    }

    // Exempt set: each entry must state a signature. `NoPointerArgs` entries
    // must have NO pointer in the parameter list (return-type pointers such
    // as agt_build_id's `const char*` are fine — there is still nothing to
    // pass NULL to); that check is the "exempt everything" backdoor guard.
    // `KnownDesignQuirk` entries must name the test covering the design
    // exception.
    for &(symbol, signature, reason) in EXEMPT_EXPORTS {
        assert!(
            !signature.is_empty(),
            "exempt export {symbol} must state its real signature"
        );
        let params = signature
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .map(|(params, _)| params)
            .unwrap_or("");
        match reason {
            ExemptReason::NoPointerArgs => {
                assert!(
                    !params.contains('*'),
                    "exempt export {symbol} declares signature `{signature}` whose \
                     parameter list contains a pointer; pointer-taking exports must \
                     be swept, never exempted"
                );
            }
            ExemptReason::KnownDesignQuirk(covering) => {
                assert!(
                    !covering.is_empty(),
                    "KnownDesignQuirk exemption for {symbol} must name its covering test"
                );
                assert!(
                    params.contains('*'),
                    "KnownDesignQuirk exemption for {symbol} declares signature \
                     `{signature}` with no pointer parameter — it belongs in \
                     NoPointerArgs, not the design-quirk channel"
                );
            }
        }
    }
    let exempt: HashSet<&str> = EXEMPT_EXPORTS.iter().map(|(s, _, _)| *s).collect();

    let duplicated: Vec<&str> = swept
        .iter()
        .filter(|s| exempt.contains(*s))
        .copied()
        .collect();
    assert!(
        duplicated.is_empty(),
        "exports both swept and exempted: {duplicated:?}"
    );

    let missing: Vec<&str> = exports
        .iter()
        .filter(|e| !swept.contains(*e) && !exempt.contains(*e))
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "exports in exports.txt neither swept nor exempted ({}): {missing:?}",
        missing.len(),
    );

    assert_eq!(
        swept.len() + exempt.len(),
        exports.len(),
        "swept + exempt must cover exports.txt exactly"
    );
    println!(
        "sweep coverage gate: swept {} + exempt {} = {} exports (exports.txt lists {})",
        swept.len(),
        exempt.len(),
        swept.len() + exempt.len(),
        exports.len(),
    );
}

// --- milestone 63: capability guards (dylib_load.rs pattern) -------------

/// Milestone 63 capability guards for the swept computer-use exports,
/// mirroring `dylib_load.rs` (query the capability first; when the mechanism
/// is absent, assert the real mechanism-absent behavior instead of skipping
/// with a bare `return`).
///
/// Check order is verified against `src/lib.rs`, not guessed:
/// - `agt_window_enumerate` / `agt_screen_list`: the probe form
///   (`buf == NULL, cap == 0, out_count != NULL`) passes argument validation
///   and reaches the mechanism check, so a headless host answers
///   `AGT_UNSUPPORTED`. With the mechanism present the probe is the legal
///   "how big?" path, already covered by `probe_group` (zero side effects).
/// - `agt_a11y_last_text_write_via`: the mechanism check runs FIRST in the
///   implementation, so NULL input answers `AGT_UNSUPPORTED` when the a11y
///   stack is absent and `AGT_FAILED` (bad_pointer) when present.
/// - `agt_native_window_*` (handle 0), `agt_input_pointer_click` (invalid
///   button) and `agt_input_type_text` / `agt_input_send_keys` (NULL text):
///   argument validation PRECEDES the mechanism check, so they answer
///   `AGT_FAILED` on every host — the null-sweep cases exercise the real
///   validation path even headless, never a vacuous pass.
/// - `agt_input_pointer_move`: coordinates only; with the mechanism absent
///   the call cannot move anything and must answer `AGT_UNSUPPORTED`; with it
///   present ANY call moves the pointer, which the safety boundary forbids —
///   so only the absent path is asserted here.
#[test]
fn computer_use_sweep_capability_guards() {
    let lib = load();
    let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };

    // 1. Enumerate group: probe must report AGT_UNSUPPORTED when absent.
    let enumerate_cap = unsafe { query(AGT_CAP_WINDOW_ENUMERATE) };
    assert!(
        enumerate_cap == AGT_OK || enumerate_cap == AGT_UNSUPPORTED,
        "AGT_CAP_WINDOW_ENUMERATE must be AGT_OK or AGT_UNSUPPORTED, got {enumerate_cap}"
    );
    for (name, probe) in [
        (
            "agt_window_enumerate",
            window_enumerate_probe as fn(&Library) -> i32,
        ),
        ("agt_screen_list", screen_list_probe as fn(&Library) -> i32),
    ] {
        if enumerate_cap == AGT_UNSUPPORTED {
            let st = probe(lib);
            assert_eq!(
                st, AGT_UNSUPPORTED,
                "{name}: probe must report AGT_UNSUPPORTED when the mechanism is absent, got {st}"
            );
        } else {
            eprintln!("SKIP: {name} mechanism available; probe behavior covered by probe_group");
        }
    }

    // 2. Native-window group: handle 0 fails argument validation BEFORE the
    //    mechanism check, so it must be AGT_FAILED on every host — no real
    //    window handle is ever enumerated or touched.
    let op_cap = unsafe { query(AGT_CAP_WINDOW_OP) };
    assert!(
        op_cap == AGT_OK || op_cap == AGT_UNSUPPORTED,
        "AGT_CAP_WINDOW_OP must be AGT_OK or AGT_UNSUPPORTED, got {op_cap}"
    );
    for (name, call) in [
        (
            "agt_native_window_show",
            native_window_show_handle0 as fn(&Library) -> i32,
        ),
        (
            "agt_native_window_move",
            native_window_move_handle0 as fn(&Library) -> i32,
        ),
        (
            "agt_native_window_rect",
            native_window_rect_handle0_null_outs as fn(&Library) -> i32,
        ),
        (
            "agt_native_window_set_topmost",
            native_window_set_topmost_handle0 as fn(&Library) -> i32,
        ),
        (
            "agt_native_window_close",
            native_window_close_handle0 as fn(&Library) -> i32,
        ),
    ] {
        let st = call(lib);
        assert_eq!(
            st, AGT_FAILED,
            "{name}(handle=0) must return AGT_FAILED (bad_handle precedes the \
             mechanism check), got {st}"
        );
    }

    // 3. Input-injection group.
    let input_cap = unsafe { query(AGT_CAP_INPUT_INJECT) };
    assert!(
        input_cap == AGT_OK || input_cap == AGT_UNSUPPORTED,
        "AGT_CAP_INPUT_INJECT must be AGT_OK or AGT_UNSUPPORTED, got {input_cap}"
    );
    if input_cap == AGT_UNSUPPORTED {
        // The mechanism is absent, so the call cannot move anything — safe.
        let f: Symbol<InputPointerMove> = unsafe { sym(lib, b"agt_input_pointer_move") };
        let st = unsafe { f(0, 0) };
        assert_eq!(
            st, AGT_UNSUPPORTED,
            "agt_input_pointer_move: mechanism absent must answer AGT_UNSUPPORTED, got {st}"
        );
    } else {
        eprintln!(
            "SKIP: input injection available; agt_input_pointer_move has no \
             necessarily-failing argument (any call moves the pointer) — success \
             path is out of scope this round"
        );
    }
    for (name, call) in [
        (
            "agt_input_pointer_click",
            input_pointer_click_bad_button as fn(&Library) -> i32,
        ),
        (
            "agt_input_type_text",
            input_type_text_null as fn(&Library) -> i32,
        ),
        (
            "agt_input_send_keys",
            input_send_keys_null as fn(&Library) -> i32,
        ),
    ] {
        let st = call(lib);
        assert_eq!(
            st, AGT_FAILED,
            "{name}: bad argument must return AGT_FAILED (validation precedes \
             the mechanism check), got {st}"
        );
    }

    // 4. a11y diagnostic string: mechanism check runs first.
    let a11y_cap = unsafe { query(AGT_CAP_ACCESSIBILITY_TREE) };
    assert!(
        a11y_cap == AGT_OK || a11y_cap == AGT_UNSUPPORTED,
        "AGT_CAP_ACCESSIBILITY_TREE must be AGT_OK or AGT_UNSUPPORTED, got {a11y_cap}"
    );
    let st = a11y_last_text_write_via_bad_args(lib);
    if a11y_cap == AGT_UNSUPPORTED {
        assert_eq!(
            st, AGT_UNSUPPORTED,
            "agt_a11y_last_text_write_via: mechanism absent must answer \
             AGT_UNSUPPORTED, got {st}"
        );
    } else {
        assert_eq!(
            st, AGT_FAILED,
            "agt_a11y_last_text_write_via: NULL out_len must answer AGT_FAILED \
             (bad_pointer), got {st}"
        );
    }
}

/// Reviewed and kept as designed: `agt_runtime_env_present` returns `0` for
/// NULL input — numerically equal to `AGT_OK` — because it is an `i32`
/// environment *query* (NULL name = "not present"), not an `agt_status`.
/// `tests/dylib_load.rs::runtime_env_present_probes_real_environment` already
/// asserts this exact behavior. `#[ignore]`d so the strict sweep above stays
/// unambiguous — the `int32_t` query semantics is intentional, not a pending
/// decision.
#[test]
#[ignore = "design quirk: agt_runtime_env_present(NULL) returns 0 == AGT_OK numeric value; reviewed and kept as designed (milestone 13)"]
fn runtime_env_present_null_returns_zero_design_quirk() {
    let lib = load();
    let present: Symbol<RuntimeEnvPresent> = unsafe { sym(lib, b"agt_runtime_env_present") };
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
    check_last_error_readable(lib, "agt_runtime_env_present(NULL)");
}
