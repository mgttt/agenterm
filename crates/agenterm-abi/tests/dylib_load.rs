//! Milestone acceptance regression: really load the built cdylib and call the
//! exports through the FFI, proving (a) every returned `const char*` is a
//! NUL-terminated C string (defect 1), (b) the fence actually ships because
//! this test only builds under an unwind profile (defect 2), and (c) the PTY
//! mechanism performs a real end-to-end round trip (milestone 2).
//!
//! If the cdylib cannot be located the test FAILS on purpose — silently
//! skipping would leave the defects unproven.

use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
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

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;
const AGT_CAP_PTY: i32 = 1;
const AGT_CAP_SCREENSHOT: i32 = 7;
const EXPECTED_BUILD_ID: &str = "0.1.16+abi.1";
const PROBE: &[u8] = b"agenterm-abi-probe";

type PtyOpen = unsafe extern "C" fn(*const agt_pty_spawn, *mut *mut std::ffi::c_void) -> i32;
type PtyRead = unsafe extern "C" fn(*mut std::ffi::c_void, *mut u8, usize, *mut usize) -> i32;
type PtyWait = unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut i32) -> i32;
type PtyClose = unsafe extern "C" fn(*mut std::ffi::c_void);
type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;

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

fn load() -> Library {
    let path = cdylib_path();
    unsafe { Library::new(&path) }
        .unwrap_or_else(|e| panic!("dlopen/LoadLibrary({path:?}) failed: {e}"))
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
fn abi_version_is_0x00010000() {
    let lib = load();
    let f: Symbol<unsafe extern "C" fn() -> u32> = unsafe { sym(&lib, b"agt_abi_version") };
    assert_eq!(unsafe { f() }, 0x0001_0000);
}

#[test]
fn build_id_is_a_valid_nul_terminated_c_string() {
    let lib = load();
    let f: Symbol<unsafe extern "C" fn() -> *const c_char> = unsafe { sym(&lib, b"agt_build_id") };
    let p = unsafe { f() };
    assert!(!p.is_null(), "agt_build_id returned NULL");
    // Defect-1 regression gate: the pointer must be readable as a C string.
    let s = unsafe { CStr::from_ptr(p) };
    assert_eq!(s.to_bytes(), EXPECTED_BUILD_ID.as_bytes());
}

#[test]
fn capability_query_reports_pty_ok_others_unsupported() {
    let lib = load();
    let f: Symbol<CapabilityQuery> = unsafe { sym(&lib, b"agt_capability_query") };
    // Milestone 2 ships the PTY mechanism → AGT_OK.
    assert_eq!(unsafe { f(AGT_CAP_PTY) }, AGT_OK);
    // Mechanisms not yet shipped stay AGT_UNSUPPORTED (never AGT_FAILED).
    assert_eq!(unsafe { f(AGT_CAP_SCREENSHOT) }, AGT_UNSUPPORTED);
}

#[test]
fn last_error_fields_are_readable_c_strings() {
    let lib = load();
    let f: Symbol<LastError> = unsafe { sym(&lib, b"agt_last_error") };
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
    let f: Symbol<LastError> = unsafe { sym(&lib, b"agt_last_error") };
    assert_eq!(unsafe { f(std::ptr::null_mut()) }, AGT_FAILED);
}

/// Real PTY round trip (milestone 2 evidence): spawn `cmd.exe /c echo probe`
/// (or `/bin/sh -c` on Unix), read until the probe bytes arrive, wait for exit
/// code 0, close cleanly.
#[test]
fn pty_roundtrip_echo_probe() {
    let lib = load();
    let open: Symbol<PtyOpen> = unsafe { sym(&lib, b"agt_pty_open") };
    let read: Symbol<PtyRead> = unsafe { sym(&lib, b"agt_pty_read") };
    let wait: Symbol<PtyWait> = unsafe { sym(&lib, b"agt_pty_wait") };
    let close: Symbol<PtyClose> = unsafe { sym(&lib, b"agt_pty_close") };

    let (program, args) = pty_echo_probe_program();
    let pty = open_pty(&lib, &open, program, &args);

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
            last_error_message(&lib)
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
        last_error_message(&lib)
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
    let open: Symbol<PtyOpen> = unsafe { sym(&lib, b"agt_pty_open") };
    let wait: Symbol<PtyWait> = unsafe { sym(&lib, b"agt_pty_wait") };
    let close: Symbol<PtyClose> = unsafe { sym(&lib, b"agt_pty_close") };

    let (program, args) = pty_long_running_program();
    let pty = open_pty(&lib, &open, program, &args);

    let started = Instant::now();
    let mut code: i32 = -999;
    let st = unsafe { wait(pty, 50, &mut code) };
    let elapsed = started.elapsed();
    assert_eq!(st, AGT_FAILED, "expected AGT_FAILED, got {st}");
    let msg = last_error_message(&lib);
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
    let open: Symbol<PtyOpen> = unsafe { sym(&lib, b"agt_pty_open") };
    let close: Symbol<PtyClose> = unsafe { sym(&lib, b"agt_pty_close") };

    let (program, args) = pty_long_running_program();
    let pty = open_pty(&lib, &open, program, &args);

    let (tx, rx) = mpsc::channel::<(i32, usize)>();
    // `*mut c_void` is not `Send`; carry the opaque handle as `usize` (Send)
    // and cast it back inside the thread. Sound because the library contract
    // is that agt_pty_t is cross-thread safe (§3.3).
    let reader_pty = pty as usize;
    let reader = std::thread::spawn(move || {
        let pty = reader_pty as *mut std::ffi::c_void;
        // Symbol is not Send, so the reader loads the library itself.
        let lib = load();
        let read: Symbol<PtyRead> = unsafe { sym(&lib, b"agt_pty_read") };
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
    // Unblocked: either clean EOF (AGT_OK, n == 0) or an io failure — the
    // contract is that close never leaves the reader hanging.
    match st {
        AGT_OK => assert_eq!(n, 0, "expected EOF (n==0), got n={n}"),
        AGT_FAILED => { /* io_read_failed is an acceptable unblock path */ }
        other => panic!("unexpected status {other}"),
    }
}
