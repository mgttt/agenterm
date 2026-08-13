//! libagenterm — thin C ABI export shell (milestone 2: PTY).
//!
//! This is the **mechanism** boundary between embedding consumers (agenterm,
//! agenterm-con, agenterm-cu) and the OS. It contains no product concepts
//! (no tab / workspace / Fleet / lease / instance). Every symbol is prefixed
//! `agt_`.
//!
//! Milestone 1 shipped the four capability/version/error exports; milestone 2
//! adds the PTY mechanism (`agt_pty_open/read/write/resize/wait/close`).
//! Window / screenshot mechanisms arrive in later milestones.
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
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use agenterm_platform::pty::{
    ChildCommand, PtyChild, PtyMaster, TerminalSize, initialize_shutdown_reaper,
    shutdown_session_detached,
};
use agenterm_platform::threading::spawn_named_detached;

// §3.8 panic fence: building this crate under an abort profile would neuter
// every `catch_unwind` below, so it is a hard compile error with an actionable
// hint instead of a silent footgun.
#[cfg(panic = "abort")]
compile_error!(
    "libagenterm 必须以 panic=unwind 构建：请用 --profile abi-release（或 abi-dev）。\
     工作区默认 profile（dev/release）为 panic=abort，会静默产出无 catch_unwind 围栏的库。"
);

/// Stable error state carried in thread-local storage.
#[derive(Clone)]
struct PendingError {
    operation: &'static CStr,
    code: &'static CStr,
    message: String,
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
/// `message` may be dynamic (format!-produced), it is owned by the record.
fn record_error(operation: &'static CStr, code: &'static CStr, message: impl Into<String>) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = Some(PendingError {
            operation,
            code,
            message: message.into(),
        });
    });
}

/// Lock a mutex, recovering from poisoning (a panicked holder) instead of
/// propagating a second panic through the FFI fence.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
        let pending = LAST_ERROR.with(|e| e.borrow().clone());
        let (operation, code, message) = pending
            .map(|p| (p.operation, p.code, p.message))
            .unwrap_or_else(|| (c"none", c"ok", "no error".to_owned()));
        let message_ptr = copy_to_tls(&message);
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

/// Capability negotiation. §3.2/§14.2 rule two: compile-time feature → runtime
/// capability query. The `pty` feature is compiled into this build, so
/// `AGT_CAP_PTY` reports `AGT_OK`; mechanisms that have not shipped yet report
/// `AGT_UNSUPPORTED` (a product gap, never a permission statement).
#[unsafe(no_mangle)]
pub extern "C" fn agt_capability_query(cap: agt_capability) -> agt_status {
    match cap {
        agt_capability::AGT_CAP_PTY => agt_status::AGT_OK,
        _ => agt_status::AGT_UNSUPPORTED,
    }
    // Pure match — no panic surface; the fence is kept for uniformity.
    // (catch_unwind is unnecessary on a non-panicking arm, so no wrapper.)
}

// --- PTY -------------------------------------------------------------

/// Opaque handle sentinel. `agt_pty_t` is a pointer to this incomplete type;
/// the real state lives in `PtyHandle`, which is never exposed to callers.
#[repr(C)]
pub struct agt_pty {
    _private: [u8; 0],
}

/// C-compatible opaque PTY handle (§3.3: cross-thread safe).
#[allow(non_camel_case_types)]
pub type agt_pty_t = *mut agt_pty;

/// C-compatible spawn parameters (§3.7). All pointers are borrowed for the
/// duration of `agt_pty_open` only; the library copies what it needs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct agt_pty_spawn {
    /// Required, NUL-terminated, UTF-8.
    pub program: *const c_char,
    /// `argv[0]` is the program name by POSIX convention and is not re-passed
    /// as an argument; arguments are `argv[1..argc]`. NULL/0 = no arguments.
    pub argv: *const *const c_char,
    pub argc: usize,
    /// Working directory; NULL = inherit the caller's.
    pub cwd: *const c_char,
    /// `"K=V"` entries; NULL or envc==0 = inherit the parent environment.
    pub envp: *const *const c_char,
    pub envc: usize,
    /// Terminal size; each must be >= 1.
    pub cols: u16,
    pub rows: u16,
}

/// Shared wait state between the library-private waiter thread and
/// `agt_pty_wait`. `PtyChild::wait()` blocks with no timeout, so the blocking
/// wait runs on a detached thread (same pattern as `src/bin/agenterm-con.rs`);
/// ABI callers only ever touch this shared state, never the native handle.
struct PtyShared {
    state: Mutex<PtyWaitState>,
    cond: Condvar,
}

struct PtyWaitState {
    exited: bool,
    exit_code: i32,
    closed: bool,
    wait_failed: Option<String>,
}

impl PtyShared {
    fn new() -> Self {
        Self {
            state: Mutex::new(PtyWaitState {
                exited: false,
                exit_code: -1,
                closed: false,
                wait_failed: None,
            }),
            cond: Condvar::new(),
        }
    }

    /// Record a clean process exit (called from the waiter thread).
    fn set_exit(&self, code: i32) {
        let mut s = lock(&self.state);
        s.exited = true;
        s.exit_code = code;
        self.cond.notify_all();
    }

    /// Record that the waiter's `wait()` itself failed (not a process exit).
    fn set_wait_failed(&self, message: String) {
        let mut s = lock(&self.state);
        s.wait_failed = Some(message);
        self.cond.notify_all();
    }

    /// Mark the handle closed (called from `agt_pty_close` to wake waiters).
    fn mark_closed(&self) {
        let mut s = lock(&self.state);
        s.closed = true;
        self.cond.notify_all();
    }
}

/// Real state behind an opaque `agt_pty_t`. Cross-thread safe (§3.3): every
/// access goes through a mutex, and `agt_pty_close` unblocks a reader blocked
/// on another thread by terminating the child and closing the pseudoconsole
/// *before* taking the master lock.
struct PtyHandle {
    master: Mutex<Option<PtyMaster>>,
    child: Mutex<Option<PtyChild>>,
    shared: Arc<PtyShared>,
}

/// Spawn `program` in a new PTY. On success `*out` is an opaque library-owned
/// handle; the caller must release it with `agt_pty_close` exactly once.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_pty_open(spawn: *const agt_pty_spawn, out: *mut agt_pty_t) -> agt_status {
    fn inner(spawn: *const agt_pty_spawn, out: *mut agt_pty_t) -> agt_status {
        if spawn.is_null() || out.is_null() {
            record_error(c"agt_pty_open", c"bad_pointer", "spawn or out is null");
            return agt_status::AGT_FAILED;
        }
        let spawn = unsafe { &*spawn };

        if spawn.program.is_null() {
            record_error(c"agt_pty_open", c"bad_pointer", "program is null");
            return agt_status::AGT_FAILED;
        }
        let program = match unsafe { CStr::from_ptr(spawn.program) }.to_str() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                record_error(c"agt_pty_open", c"bad_program", "program is empty");
                return agt_status::AGT_FAILED;
            }
            Err(_) => {
                record_error(
                    c"agt_pty_open",
                    c"bad_encoding",
                    "program is not valid UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };

        // argv[0] is the program name by convention; arguments are argv[1..].
        let mut args: Vec<String> = Vec::new();
        if !spawn.argv.is_null() {
            for i in 1..spawn.argc {
                let arg_ptr = unsafe { *spawn.argv.add(i) };
                match unsafe { CStr::from_ptr(arg_ptr) }.to_str() {
                    Ok(s) => args.push(s.to_owned()),
                    Err(_) => {
                        record_error(
                            c"agt_pty_open",
                            c"bad_encoding",
                            format!("argv[{i}] is not valid UTF-8"),
                        );
                        return agt_status::AGT_FAILED;
                    }
                }
            }
        }

        let cwd = if spawn.cwd.is_null() {
            None
        } else {
            match unsafe { CStr::from_ptr(spawn.cwd) }.to_str() {
                Ok(s) => Some(std::path::PathBuf::from(s)),
                Err(_) => {
                    record_error(c"agt_pty_open", c"bad_encoding", "cwd is not valid UTF-8");
                    return agt_status::AGT_FAILED;
                }
            }
        };

        // envp entries are "K=V"; NULL/0 inherits the parent environment.
        let mut envs: Vec<(String, String)> = Vec::new();
        if !spawn.envp.is_null() {
            for i in 0..spawn.envc {
                let item_ptr = unsafe { *spawn.envp.add(i) };
                let item = match unsafe { CStr::from_ptr(item_ptr) }.to_str() {
                    Ok(s) => s,
                    Err(_) => {
                        record_error(
                            c"agt_pty_open",
                            c"bad_encoding",
                            format!("envp[{i}] is not valid UTF-8"),
                        );
                        return agt_status::AGT_FAILED;
                    }
                };
                match item.split_once('=') {
                    Some((k, v)) => envs.push((k.to_owned(), v.to_owned())),
                    None => {
                        record_error(
                            c"agt_pty_open",
                            c"bad_env",
                            format!("envp[{i}] has no '=' separator: {item}"),
                        );
                        return agt_status::AGT_FAILED;
                    }
                }
            }
        }

        if spawn.cols == 0 || spawn.rows == 0 {
            record_error(c"agt_pty_open", c"bad_size", "cols and rows must be >= 1");
            return agt_status::AGT_FAILED;
        }

        // Platform contract (not optional): the shutdown reaper must be ready
        // before any native PTY resource is created, so close paths never
        // discover thread-creation failure while already owning a session.
        if let Err(e) = initialize_shutdown_reaper() {
            record_error(
                c"agt_pty_open",
                c"reaper_init_failed",
                format!("initialize_shutdown_reaper: {e}"),
            );
            return agt_status::AGT_FAILED;
        }

        let mut command = ChildCommand::new(program).size(TerminalSize {
            rows: spawn.rows,
            cols: spawn.cols,
        });
        for a in &args {
            command = command.arg(a.clone());
        }
        if let Some(dir) = cwd {
            command = command.current_dir(dir);
        }
        for (k, v) in &envs {
            command = command.env(k.clone(), v.clone());
        }

        let spawned = match command.spawn() {
            Ok(s) => s,
            Err(e) => {
                record_error(
                    c"agt_pty_open",
                    c"spawn_failed",
                    format!("spawn {program}: {e}"),
                );
                return agt_status::AGT_FAILED;
            }
        };
        let (master, child) = spawned.into_parts();

        // Private waiter thread: `PtyChild::wait()` blocks with no timeout but
        // the ABI requires `agt_pty_wait(timeout_ms)`. The blocking wait runs
        // on a library-private detached thread (the pattern adopted in §3.5
        // and implemented in src/bin/agenterm-con.rs:2695-2830); the ABI
        // caller only ever reads the shared state below.
        let mut waiter = match child.try_clone_for_wait() {
            Ok(w) => w,
            Err(e) => {
                record_error(c"agt_pty_open", c"waiter_clone_failed", format!("{e}"));
                let _ = shutdown_session_detached(Some(master), Some(child));
                return agt_status::AGT_FAILED;
            }
        };
        let shared = Arc::new(PtyShared::new());
        let waiter_shared = Arc::clone(&shared);
        if let Err(e) = spawn_named_detached(
            "agenterm-abi-pty-waiter",
            Box::new(move || match waiter.wait() {
                Ok(status) => waiter_shared.set_exit(status.code().unwrap_or(-1)),
                Err(e) => waiter_shared.set_wait_failed(format!("{e}")),
            }),
        ) {
            record_error(c"agt_pty_open", c"waiter_spawn_failed", format!("{e}"));
            let _ = shutdown_session_detached(Some(master), Some(child));
            return agt_status::AGT_FAILED;
        }

        let handle = Box::new(PtyHandle {
            master: Mutex::new(Some(master)),
            child: Mutex::new(Some(child)),
            shared,
        });
        unsafe { *out = Box::into_raw(handle) as agt_pty_t };
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(spawn, out))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_pty_open", c"panic", "panic in agt_pty_open");
            agt_status::AGT_FAILED
        }
    }
}

/// Block until data is available or the PTY is closed (§3.4: caller-allocated
/// buffer; the library never takes memory ownership). EOF is reported as
/// `AGT_OK` with `*out_len == 0`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_pty_read(
    pty: agt_pty_t,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(pty: agt_pty_t, buf: *mut u8, cap: usize, out_len: *mut usize) -> agt_status {
        if pty.is_null() {
            record_error(c"agt_pty_read", c"bad_pointer", "pty is null");
            return agt_status::AGT_FAILED;
        }
        if out_len.is_null() {
            record_error(c"agt_pty_read", c"bad_pointer", "out_len is null");
            return agt_status::AGT_FAILED;
        }
        unsafe { *out_len = 0 };
        if cap == 0 {
            // §3.4: insufficient capacity → FAILED, required length in out_len.
            unsafe { *out_len = 1 };
            record_error(
                c"agt_pty_read",
                c"buffer_too_small",
                "cap is 0; at least 1 byte is required",
            );
            return agt_status::AGT_FAILED;
        }
        if buf.is_null() {
            record_error(c"agt_pty_read", c"bad_pointer", "buf is null");
            return agt_status::AGT_FAILED;
        }
        let handle = unsafe { &*(pty as *const PtyHandle) };
        let guard = lock(&handle.master);
        let master = match guard.as_ref() {
            Some(m) => m,
            None => {
                record_error(c"agt_pty_read", c"closed", "pty handle is closed");
                return agt_status::AGT_FAILED;
            }
        };
        let slice = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
        loop {
            match master.io().read(slice) {
                Ok(0) => {
                    unsafe { *out_len = 0 };
                    return agt_status::AGT_OK;
                }
                Ok(n) => {
                    unsafe { *out_len = n };
                    return agt_status::AGT_OK;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    record_error(c"agt_pty_read", c"io_read_failed", format!("{e}"));
                    return agt_status::AGT_FAILED;
                }
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(pty, buf, cap, out_len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_pty_read", c"panic", "panic in agt_pty_read");
            agt_status::AGT_FAILED
        }
    }
}

/// Write `len` bytes to the PTY master. On success `*written == len`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_pty_write(
    pty: agt_pty_t,
    buf: *const u8,
    len: usize,
    written: *mut usize,
) -> agt_status {
    fn inner(pty: agt_pty_t, buf: *const u8, len: usize, written: *mut usize) -> agt_status {
        if pty.is_null() {
            record_error(c"agt_pty_write", c"bad_pointer", "pty is null");
            return agt_status::AGT_FAILED;
        }
        if written.is_null() {
            record_error(c"agt_pty_write", c"bad_pointer", "written is null");
            return agt_status::AGT_FAILED;
        }
        unsafe { *written = 0 };
        if len > 0 && buf.is_null() {
            record_error(c"agt_pty_write", c"bad_pointer", "buf is null");
            return agt_status::AGT_FAILED;
        }
        if len == 0 {
            return agt_status::AGT_OK;
        }
        let slice = unsafe { std::slice::from_raw_parts(buf, len) };
        let handle = unsafe { &*(pty as *const PtyHandle) };
        let guard = lock(&handle.master);
        let master = match guard.as_ref() {
            Some(m) => m,
            None => {
                record_error(c"agt_pty_write", c"closed", "pty handle is closed");
                return agt_status::AGT_FAILED;
            }
        };
        match master.write_all(slice) {
            Ok(()) => {
                unsafe { *written = len };
                agt_status::AGT_OK
            }
            Err(e) => {
                record_error(c"agt_pty_write", c"io_write_failed", format!("{e}"));
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(pty, buf, len, written))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_pty_write", c"panic", "panic in agt_pty_write");
            agt_status::AGT_FAILED
        }
    }
}

/// Resize the PTY to `cols` x `rows` (each >= 1).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_pty_resize(pty: agt_pty_t, cols: u16, rows: u16) -> agt_status {
    fn inner(pty: agt_pty_t, cols: u16, rows: u16) -> agt_status {
        if pty.is_null() {
            record_error(c"agt_pty_resize", c"bad_pointer", "pty is null");
            return agt_status::AGT_FAILED;
        }
        if cols == 0 || rows == 0 {
            record_error(c"agt_pty_resize", c"bad_size", "cols and rows must be >= 1");
            return agt_status::AGT_FAILED;
        }
        let handle = unsafe { &*(pty as *const PtyHandle) };
        let guard = lock(&handle.master);
        let master = match guard.as_ref() {
            Some(m) => m,
            None => {
                record_error(c"agt_pty_resize", c"closed", "pty handle is closed");
                return agt_status::AGT_FAILED;
            }
        };
        match master.resize(TerminalSize { rows, cols }) {
            Ok(()) => agt_status::AGT_OK,
            Err(e) => {
                record_error(c"agt_pty_resize", c"resize_failed", format!("{e}"));
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(pty, cols, rows))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_pty_resize", c"panic", "panic in agt_pty_resize");
            agt_status::AGT_FAILED
        }
    }
}

/// Wait up to `timeout_ms` for the process to exit. On exit `*exit_code` is
/// filled and `AGT_OK` is returned. On timeout `AGT_FAILED { code = "timeout" }`
/// is returned — never `AGT_UNSUPPORTED`; the two states are distinct and are
/// never merged (§3.1). The blocking native wait runs on a library-private
/// thread; this call only waits on shared state.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_pty_wait(pty: agt_pty_t, timeout_ms: u32, exit_code: *mut i32) -> agt_status {
    fn inner(pty: agt_pty_t, timeout_ms: u32, exit_code: *mut i32) -> agt_status {
        if pty.is_null() {
            record_error(c"agt_pty_wait", c"bad_pointer", "pty is null");
            return agt_status::AGT_FAILED;
        }
        if exit_code.is_null() {
            record_error(c"agt_pty_wait", c"bad_pointer", "exit_code is null");
            return agt_status::AGT_FAILED;
        }
        unsafe { *exit_code = -1 };
        let handle = unsafe { &*(pty as *const PtyHandle) };
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut guard = lock(&handle.shared.state);
        loop {
            if guard.exited {
                unsafe { *exit_code = guard.exit_code };
                return agt_status::AGT_OK;
            }
            if guard.closed {
                record_error(c"agt_pty_wait", c"closed", "pty handle is closed");
                return agt_status::AGT_FAILED;
            }
            if let Some(message) = guard.wait_failed.as_deref() {
                record_error(
                    c"agt_pty_wait",
                    c"wait_failed",
                    format!("waiter failed: {message}"),
                );
                return agt_status::AGT_FAILED;
            }
            if Instant::now() >= deadline {
                record_error(
                    c"agt_pty_wait",
                    c"timeout",
                    "process did not exit within timeout_ms",
                );
                return agt_status::AGT_FAILED;
            }
            let remaining = deadline - Instant::now();
            let (new_guard, _) = handle
                .shared
                .cond
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = new_guard;
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(pty, timeout_ms, exit_code))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_pty_wait", c"panic", "panic in agt_pty_wait");
            agt_status::AGT_FAILED
        }
    }
}

/// Release a PTY handle. Must be called exactly once. Cross-thread safe: a
/// thread blocked inside `agt_pty_read` on another thread is unblocked by
/// terminating the child and closing the pseudoconsole before the master
/// handle is dropped.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_pty_close(pty: agt_pty_t) {
    if pty.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let handle = unsafe { Box::from_raw(pty as *mut PtyHandle) };
        // 1. Terminate the child and close the pseudoconsole. This unblocks a
        //    reader blocked on another thread (ConPTY output pipe EOF).
        if let Some(child) = lock(&handle.child).take() {
            let _ = child.terminate_forcefully();
            child.close_pseudoconsole();
        }
        // 2. Wait for the blocked reader to release the master lock, then drop
        //    the master. Safe: step 1 already unblocked any in-flight read.
        if let Some(master) = lock(&handle.master).take() {
            drop(master);
        }
        // 3. Wake any caller blocked in agt_pty_wait.
        handle.shared.mark_closed();
        // 4. Free the handle itself (waiter thread keeps the shared Arc alive
        //    until its blocked wait() returns).
        drop(handle);
    }));
}
