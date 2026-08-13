//! libagenterm — thin C ABI export shell (milestone 1: compilable skeleton).
//!
//! This is the **mechanism** boundary between embedding consumers (agenterm,
//! agenterm-con, agenterm-cu) and the OS. It contains no product concepts
//! (no tab / workspace / Fleet / lease / instance). Every symbol is prefixed
//! `agt_`.
//!
//! Milestone 1 implements only the four capability/version/error exports and
//! the shared type definitions. PTY / window / screenshot mechanisms arrive in
//! later milestones.
//!
//! Every export is wrapped in `catch_unwind`; a panic never crosses the FFI
//! boundary and is reported as `AGT_FAILED { code = "panic" }`. `catch_unwind`
//! only works under `panic = "unwind"`, but the workspace default profiles
//! abort, so this crate MUST be built with the dedicated unwind profiles
//! (`--profile abi-release` / `--profile abi-dev`). The `compile_error!` gate
//! below makes any abort-profile build fail instead of silently shipping a
//! fence-less library.

use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

// §3.8 panic fence: building this crate under an abort profile would neuter
// every `catch_unwind` below, so it is a hard compile error with an actionable
// hint instead of a silent footgun.
#[cfg(panic = "abort")]
compile_error!(
    "libagenterm 必须以 panic=unwind 构建：请用 --profile abi-release（或 abi-dev）。\
     工作区默认 profile（dev/release）为 panic=abort，会静默产出无 catch_unwind 围栏的库。"
);

/// Stable error state carried in thread-local storage.
#[derive(Clone, Copy)]
struct PendingError {
    operation: &'static CStr,
    code: &'static CStr,
    message: &'static str,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<PendingError>> = const { RefCell::new(None) };
    /// Thread-local buffer backing `agt_error::message`. Valid only until the
    /// next libagenterm call on the same thread.
    static MSG_BUF: RefCell<[u8; 512]> = const { RefCell::new([0u8; 512]) };
}

/// Point a `const char*` at a static NUL-terminated C string literal.
/// `c"..."` literals are `&'static CStr` and include the trailing NUL, so the
/// returned pointer is a valid C string (an `&str` would not be).
const fn cstr_static(s: &'static CStr) -> *const c_char {
    s.as_ptr()
}

/// Copy `s` into the thread-local message buffer and return its pointer.
/// `s` is truncated to fit (leaving room for the trailing NUL).
fn copy_to_tls(s: &str) -> *const c_char {
    MSG_BUF.with(|b| {
        let mut guard = b.borrow_mut();
        guard.fill(0);
        let bytes = s.as_bytes();
        let n = bytes.len().min(guard.len() - 1);
        guard[..n].copy_from_slice(&bytes[..n]);
        guard.as_ptr() as *const c_char
    })
}

/// Record a pending error for the current thread (reported by `agt_last_error`).
fn record_error(operation: &'static CStr, code: &'static CStr, message: &'static str) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = Some(PendingError {
            operation,
            code,
            message,
        });
    });
}

/// C-compatible status.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum agt_status {
    AGT_OK = 0,
    AGT_UNSUPPORTED = 1,
    AGT_FAILED = 2,
}

/// C-compatible error record.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct agt_error {
    /// Static, permanently valid (NUL-terminated C string).
    pub operation: *const c_char,
    /// Static, permanently valid (NUL-terminated C string).
    pub code: *const c_char,
    /// Thread-local, valid until the next call on this thread.
    pub message: *const c_char,
}

/// C-compatible capability enumeration (discovery/metadata only, never a
/// permission grant — see repository policy).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum agt_capability {
    AGT_CAP_PTY = 1,
    AGT_CAP_PROCESS_SPAWN,
    AGT_CAP_PROCESS_OBSERVE,
    AGT_CAP_WINDOW_HOST,
    AGT_CAP_WINDOW_ENUMERATE,
    AGT_CAP_WINDOW_OP,
    AGT_CAP_SCREENSHOT,
    AGT_CAP_CLIPBOARD,
    AGT_CAP_IME,
    AGT_CAP_INPUT_INJECT,
    AGT_CAP_IPC,
    AGT_CAP_FONT_RASTER,
    AGT_CAP_FILESYSTEM_PUBLISH,
    AGT_CAP_SHARED_MEMORY,
    AGT_CAP_PARENT_CONSOLE,
}

const ABI_MAJOR: u16 = 1;
const ABI_MINOR: u16 = 0;

/// First milestone ABI version: `(major << 16) | minor = 0x00010000`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_abi_version() -> u32 {
    catch_unwind(|| ((ABI_MAJOR as u32) << 16) | (ABI_MINOR as u32)).unwrap_or(0)
}

/// Human-readable build identity. Static, permanently valid.
#[unsafe(no_mangle)]
pub extern "C" fn agt_build_id() -> *const c_char {
    catch_unwind(|| cstr_static(c"0.1.16+abi.1")).unwrap_or(std::ptr::null())
}

/// Fill `out` with the last error recorded on this thread, or a "no error"
/// record when nothing has failed. `AGT_UNSUPPORTED` is *not* an error and is
/// never reported here.
//
// `out` is a C ABI boundary contract (see include/agenterm.h): pointer validity
// is the caller's responsibility, so the `not_unsafe_ptr_arg_deref` lint does
// not apply to this exported symbol (it cannot be marked `unsafe fn` without
// breaking the `pub extern "C" fn` export shape).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_last_error(out: *mut agt_error) -> agt_status {
    fn inner(out: *mut agt_error) -> agt_status {
        if out.is_null() {
            record_error(c"agt_last_error", c"bad_pointer", "out pointer is null");
            return agt_status::AGT_FAILED;
        }
        let pending = LAST_ERROR.with(|e| *e.borrow());
        let (operation, code, message) = pending
            .map(|p| (p.operation, p.code, p.message))
            .unwrap_or((c"none", c"ok", "no error"));
        let message_ptr = copy_to_tls(message);
        unsafe {
            *out = agt_error {
                operation: cstr_static(operation),
                code: cstr_static(code),
                message: message_ptr,
            };
        }
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(out))) {
        Ok(s) => s,
        Err(_) => {
            if !out.is_null() {
                unsafe {
                    *out = agt_error {
                        operation: cstr_static(c"agt_last_error"),
                        code: cstr_static(c"panic"),
                        message: copy_to_tls("panic"),
                    };
                }
            }
            agt_status::AGT_FAILED
        }
    }
}

/// Capability negotiation. Milestone 1 exposes no real mechanism, so every
/// capability reports `AGT_UNSUPPORTED`. Real mechanisms arrive in later
/// milestones.
#[unsafe(no_mangle)]
pub extern "C" fn agt_capability_query(cap: agt_capability) -> agt_status {
    let _ = cap;
    catch_unwind(|| agt_status::AGT_UNSUPPORTED).unwrap_or(agt_status::AGT_FAILED)
}
