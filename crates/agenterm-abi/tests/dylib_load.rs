//! Milestone-1 acceptance regression: really load the built cdylib and call the
//! four exports through the FFI, proving (a) every returned `const char*` is a
//! NUL-terminated C string (defect 1) and (b) the fence actually ships because
//! this test only builds under an unwind profile (defect 2).
//!
//! If the cdylib cannot be located the test FAILS on purpose — silently
//! skipping would leave both defects unproven.

use libloading::{Library, Symbol};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
struct agt_error {
    operation: *const c_char,
    code: *const c_char,
    message: *const c_char,
}

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;
const AGT_CAP_PTY: i32 = 1; // any capability is fine for the query assertion
const EXPECTED_BUILD_ID: &str = "0.1.16+abi.1";

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
fn capability_query_reports_unsupported() {
    let lib = load();
    let f: Symbol<unsafe extern "C" fn(i32) -> i32> = unsafe { sym(&lib, b"agt_capability_query") };
    assert_eq!(unsafe { f(AGT_CAP_PTY) }, AGT_UNSUPPORTED);
}

#[test]
fn last_error_fields_are_readable_c_strings() {
    let lib = load();
    let f: Symbol<unsafe extern "C" fn(*mut agt_error) -> i32> =
        unsafe { sym(&lib, b"agt_last_error") };
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
    let f: Symbol<unsafe extern "C" fn(*mut agt_error) -> i32> =
        unsafe { sym(&lib, b"agt_last_error") };
    assert_eq!(unsafe { f(std::ptr::null_mut()) }, AGT_FAILED);
}
