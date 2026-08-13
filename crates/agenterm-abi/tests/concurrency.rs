//! Milestone 13: concurrency + thread-local error isolation gates.
//!
//! Two properties were never verified before:
//!
//! 1. `agt_last_error` is documented as thread-local ("valid until next call
//!    on this thread"), but nothing proved the isolation actually holds. If
//!    the implementation ever regressed to a global static, multi-threaded
//!    callers would read each other's errors and a single-threaded test could
//!    never notice.
//! 2. The library runs private threads (PTY reaper, window-loop thread) and
//!    callers may call exports concurrently, yet there was no concurrency
//!    smoke coverage.
//!
//! Safety boundaries (hard, from the brief): no window, no PTY, no screenshot
//! file, no clipboard mutation. Only the side-effect-free exports listed in
//! test 1 are hammered concurrently; error triggers use the NULL-pointer
//! short-circuit paths (`agt_process_list(out_count=NULL)` and
//! `agt_clipboard_get_text(out_len=NULL)`) which fail before touching any
//! platform mechanism. `agt_process_kill` is never called. No blocking call:
//! every `timeout_ms`-style parameter is 0 / absent.

use libloading::{Library, Symbol};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;
use std::sync::mpsc;

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

// Probe calls never write records, so the process-info record is a
// zero-sized placeholder (same pattern as tests/null_sweep.rs).
#[repr(C)]
#[allow(non_camel_case_types)]
struct agt_process_info;

// --- export fn types -----------------------------------------------------

type AbiVersion = unsafe extern "C" fn() -> u32;
type BuildId = unsafe extern "C" fn() -> *const c_char;
type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type ProcessSelf = unsafe extern "C" fn() -> u32;
type ProcessList = unsafe extern "C" fn(*mut agt_process_info, usize, *mut usize) -> i32;
type ClipboardGetText = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;

// --- dylib loading (same pattern as tests/dylib_load.rs / null_sweep.rs) --

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
///
/// `Symbol` is not `Send` (see dylib_load.rs), so each worker thread loads
/// the library itself; the OS returns the same underlying module and just
/// bumps the reference count.
fn load() -> &'static Library {
    let path = cdylib_path();
    let lib = unsafe { Library::new(&path) }
        .unwrap_or_else(|e| panic!("dlopen/LoadLibrary({path:?}) failed: {e}"));
    Box::leak(Box::new(lib))
}

unsafe fn sym<'l, T>(lib: &'l Library, name: &[u8]) -> Symbol<'l, T> {
    unsafe { lib.get(name) }.unwrap_or_else(|e| panic!("symbol {name:?} missing: {e}"))
}

/// The three C strings of the last error record, owned (safe to compare).
#[derive(Debug, PartialEq, Eq)]
struct ErrorRecord {
    operation: String,
    code: String,
    message: String,
}

/// Read `agt_last_error` into an owned record. Asserts the record is readable
/// (three non-null, CStr-parseable pointers) and returns the status plus the
/// record.
fn read_last_error(f: &Symbol<LastError>) -> (i32, ErrorRecord) {
    let mut e = agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    let st = unsafe { f(&mut e) };
    assert!(
        !e.operation.is_null() && !e.code.is_null() && !e.message.is_null(),
        "agt_last_error returned null field(s); status {st}"
    );
    let to_str = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    (
        st,
        ErrorRecord {
            operation: to_str(e.operation),
            code: to_str(e.code),
            message: to_str(e.message),
        },
    )
}

// --- test 1: concurrent smoke of the stateless exports --------------------

/// 8 threads × 200 iterations over the side-effect-free exports: version,
/// build id, capability queries, process self pid, and the legal
/// `process_list` length probe. All threads must join, the process must not
/// crash, and every result must be self-consistent.
#[test]
fn stateless_exports_concurrent_smoke() {
    const THREADS: usize = 8;
    const ITERATIONS: usize = 200;

    // Reference values captured on the main thread before the storm.
    let warm = load();
    let expected_version = unsafe { (sym::<AbiVersion>(warm, b"agt_abi_version"))() };
    let expected_pid = unsafe { (sym::<ProcessSelf>(warm, b"agt_process_self"))() };
    let build_ptr = unsafe { (sym::<BuildId>(warm, b"agt_build_id"))() };
    assert!(
        !build_ptr.is_null(),
        "agt_build_id must not be null on main thread"
    );
    let build_bytes = unsafe { CStr::from_ptr(build_ptr) }.to_bytes().to_vec();
    assert!(!build_bytes.is_empty(), "agt_build_id must not be empty");

    // Capability pins: the pty/process features are compiled into this build
    // (AGT_OK), an unknown capability is a product gap (AGT_UNSUPPORTED).
    const CAP_PTY: i32 = 1;
    const CAP_PROCESS_OBSERVE: i32 = 3;
    const CAP_UNKNOWN: i32 = 999;

    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let build_bytes = build_bytes.clone();
        handles.push(std::thread::spawn(move || {
            let lib = load();
            for i in 0..ITERATIONS {
                let version = unsafe { (sym::<AbiVersion>(lib, b"agt_abi_version"))() };
                assert_eq!(
                    version, expected_version,
                    "iter {i}: agt_abi_version changed ({version} != {expected_version})"
                );

                let ptr = unsafe { (sym::<BuildId>(lib, b"agt_build_id"))() };
                assert!(!ptr.is_null(), "iter {i}: agt_build_id is null");
                let s = unsafe { CStr::from_ptr(ptr) }.to_bytes();
                assert!(!s.is_empty(), "iter {i}: agt_build_id is empty");
                assert_eq!(
                    s,
                    &build_bytes[..],
                    "iter {i}: agt_build_id is not the stable static string"
                );

                let q_pty =
                    unsafe { (sym::<CapabilityQuery>(lib, b"agt_capability_query"))(CAP_PTY) };
                assert_eq!(q_pty, AGT_OK, "iter {i}: AGT_CAP_PTY must be AGT_OK");
                let q_proc = unsafe {
                    (sym::<CapabilityQuery>(lib, b"agt_capability_query"))(CAP_PROCESS_OBSERVE)
                };
                assert_eq!(
                    q_proc, AGT_OK,
                    "iter {i}: AGT_CAP_PROCESS_OBSERVE must be AGT_OK"
                );
                let q_unk =
                    unsafe { (sym::<CapabilityQuery>(lib, b"agt_capability_query"))(CAP_UNKNOWN) };
                assert_eq!(
                    q_unk, AGT_UNSUPPORTED,
                    "iter {i}: unknown capability must be AGT_UNSUPPORTED"
                );

                let pid = unsafe { (sym::<ProcessSelf>(lib, b"agt_process_self"))() };
                assert_eq!(
                    pid, expected_pid,
                    "iter {i}: agt_process_self differs across threads ({pid} != {expected_pid})"
                );

                // Legal "how big?" probe: buf=NULL, cap=0 → buffer_too_small,
                // *out_count = required. Asks for the length, allocates nothing.
                let mut n: usize = 0;
                let st = unsafe {
                    (sym::<ProcessList>(lib, b"agt_process_list"))(std::ptr::null_mut(), 0, &mut n)
                };
                assert_eq!(
                    st, AGT_FAILED,
                    "iter {i}: process_list probe must report buffer_too_small (AGT_FAILED)"
                );
                assert!(
                    n > 0,
                    "iter {i}: probe must report a nonzero required process count, got {n}"
                );
            }
        }));
    }

    for (idx, h) in handles.into_iter().enumerate() {
        h.join()
            .unwrap_or_else(|_| panic!("worker thread {idx} panicked"));
    }
}

// --- test 2: TLS error isolation (the focus of this milestone) ------------

/// A and B race-free error isolation check, sequenced with channels (no
/// sleeps): A fails → B must still read "no error" → B fails differently →
/// A must still read its own record.
///
/// If the implementation regressed to a global static, B would read A's
/// `bad_pointer` here and the test would fail loudly.
#[test]
fn tls_last_error_is_isolated_per_thread() {
    // A -> B: A triggered its error and confirmed it reads bad_pointer.
    let (tx_a, rx_b) = mpsc::channel::<()>();
    // B -> A: B triggered its own (different) error.
    let (tx_b, rx_a) = mpsc::channel::<()>();

    let a = std::thread::spawn(move || {
        let lib = load();
        let last: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
        let list: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") };

        // A's thread starts clean, so the record below is A's own doing.
        let (st, rec) = read_last_error(&last);
        assert_eq!(st, AGT_OK);
        assert_eq!(rec, error_none(), "thread A must start with a clean record");

        // Trigger: agt_process_list(out_count = NULL) → bad_pointer (fails
        // before enumerating anything — no side effects).
        let st = unsafe { list(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
        assert_eq!(st, AGT_FAILED, "agt_process_list(out_count=NULL) must fail");
        let (st, rec) = read_last_error(&last);
        assert_eq!(st, AGT_OK);
        assert_eq!(rec.operation, "agt_process_list");
        assert_eq!(rec.code, "bad_pointer");
        assert!(!rec.message.is_empty());

        tx_a.send(()).expect("A -> B signal");

        // Wait until B has triggered its own error, then re-read: A's record
        // must be untouched by B.
        rx_a.recv().expect("B -> A signal");
        let (st, rec) = read_last_error(&last);
        assert_eq!(st, AGT_OK);
        assert_eq!(
            rec.operation, "agt_process_list",
            "thread A's record was overwritten by thread B: {rec:?}"
        );
        assert_eq!(
            rec.code, "bad_pointer",
            "thread A's record was overwritten: {rec:?}"
        );
    });

    let b = std::thread::spawn(move || {
        let lib = load();
        let last: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
        let clip: Symbol<ClipboardGetText> = unsafe { sym(lib, b"agt_clipboard_get_text") };

        // Wait until A's error is definitely recorded on A's thread.
        rx_b.recv().expect("A -> B signal");

        // B triggered nothing: must read the "no error" record — never A's
        // bad_pointer. This is the isolation proof.
        let (st, rec) = read_last_error(&last);
        assert_eq!(st, AGT_OK);
        assert_eq!(
            rec,
            error_none(),
            "thread B read A's error record — last_error is NOT thread-local: {rec:?}"
        );

        // Trigger a *different* error: agt_clipboard_get_text(out_len = NULL)
        // → bad_pointer (fails before touching the clipboard).
        let st = unsafe { clip(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
        assert_eq!(
            st, AGT_FAILED,
            "agt_clipboard_get_text(out_len=NULL) must fail"
        );
        let (st, rec) = read_last_error(&last);
        assert_eq!(st, AGT_OK);
        assert_eq!(rec.operation, "agt_clipboard_get_text");
        assert_eq!(rec.code, "bad_pointer");

        tx_b.send(()).expect("B -> A signal");
    });

    a.join().expect("thread A must join without panicking");
    b.join().expect("thread B must join without panicking");
}

/// The "no error" record: `operation="none"`, `code="ok"`,
/// `message="no error"` (see `agt_last_error` in src/lib.rs).
fn error_none() -> ErrorRecord {
    ErrorRecord {
        operation: "none".to_owned(),
        code: "ok".to_owned(),
        message: "no error".to_owned(),
    }
}

// --- test 3: fresh thread starts with a clean record ----------------------

/// A brand-new thread that calls nothing else must read `AGT_OK` plus the
/// "no error" record — never a dangling pointer, never a leftover from
/// another thread.
#[test]
fn fresh_thread_last_error_starts_clear() {
    let lib = load();
    std::thread::spawn(move || {
        let last: Symbol<LastError> = unsafe { sym(lib, b"agt_last_error") };
        let (st, rec) = read_last_error(&last);
        assert_eq!(st, AGT_OK);
        assert_eq!(
            rec,
            error_none(),
            "fresh thread must start clean, got {rec:?}"
        );
    })
    .join()
    .expect("fresh-thread probe must join without panicking");
}
