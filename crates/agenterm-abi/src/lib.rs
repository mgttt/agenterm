//! libagenterm — thin C ABI export shell (milestone 3a: window lifecycle +
//! frame rendezvous).
//!
//! This is the **mechanism** boundary between embedding consumers (agenterm,
//! agenterm-con, agenterm-cu) and the OS. It contains no product concepts
//! (no tab / workspace / Fleet / lease / instance). Every symbol is prefixed
//! `agt_`.
//!
//! Milestone 1 shipped the four capability/version/error exports; milestone 2
//! added the PTY mechanism (`agt_pty_open/read/write/resize/wait/close`);
//! milestone 3a adds the window lifecycle / pixel-frame rendezvous
//! (`agt_window_open/poll_event/request_redraw/metrics/close` plus
//! `agt_frame_begin/commit`). Keyboard/pointer/wheel/IME event translation
//! is deliberately out of scope until milestone 3b.
//!
//! Every export is wrapped in `catch_unwind`; a panic never crosses the FFI
//! boundary and is reported as `AGT_FAILED { code = "panic" }`. `catch_unwind`
//! only works under `panic = "unwind"`, but the workspace default profiles
//! abort, so this crate MUST be built with the dedicated unwind profiles
//! (`--profile abi-release` / `--profile abi-dev`). The `compile_error!` gate
//! below makes any abort-profile build fail instead of silently shipping a
//! fence-less library.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use agenterm_platform::pty::{
    ChildCommand, PtyChild, PtyMaster, TerminalSize, initialize_shutdown_reaper,
    shutdown_session_detached,
};
use agenterm_platform::threading::spawn_named_detached;
use agenterm_platform::window_host::{
    LogicalSize, PixelFrameWrite, PixelWindow, PixelWindowApplication, PixelWindowDirective,
    PixelWindowError, PixelWindowEvent, PixelWindowMetrics, PixelWindowOptions, WindowWaker,
    XrgbPixelFrame, run_pixel_window,
};

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
/// capability query. The `pty` and `native-pixel-window` features are compiled
/// into this build, so `AGT_CAP_PTY` and `AGT_CAP_WINDOW_HOST` report
/// `AGT_OK`; mechanisms that have not shipped yet report `AGT_UNSUPPORTED`
/// (a product gap, never a permission statement).
#[unsafe(no_mangle)]
pub extern "C" fn agt_capability_query(cap: agt_capability) -> agt_status {
    match cap {
        agt_capability::AGT_CAP_PTY | agt_capability::AGT_CAP_WINDOW_HOST => agt_status::AGT_OK,
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

// --- window & frame (milestone 3a) ------------------------------------

/// Event kinds carried by `agt_event` (the only four events this milestone
/// translates; everything else from the platform's rich event enum is
/// deliberately dropped — keyboard/pointer/wheel/IME arrive in 3b).
pub const AGT_EV_NONE: u32 = 0;
pub const AGT_EV_CLOSE_REQUEST: u32 = 1;
pub const AGT_EV_GEOMETRY: u32 = 2;
pub const AGT_EV_FOCUS: u32 = 3;
pub const AGT_EV_RENDER_DUE: u32 = 4;

/// C-compatible window event record.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct agt_event {
    pub kind: u32,
    pub generation: u64,
    /// Valid only when `kind == AGT_EV_GEOMETRY`.
    pub width: u32,
    /// Valid only when `kind == AGT_EV_GEOMETRY`.
    pub height: u32,
    /// Valid only when `kind == AGT_EV_GEOMETRY`.
    pub scale: f64,
    /// Valid only when `kind == AGT_EV_FOCUS`.
    pub focused: i32,
}

/// C-compatible window creation parameters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct agt_window_spec {
    /// Required, NUL-terminated, UTF-8 window title.
    pub title: *const c_char,
    /// Initial logical size, each >= 1.
    pub width: u32,
    pub height: u32,
    /// Non-zero: do not take foreground focus when opening.
    pub no_activate: i32,
    /// Non-zero: allow IME input on this window.
    pub ime_allowed: i32,
}

/// C-compatible frame descriptor filled by `agt_frame_begin`. The `pixels`
/// pointer is valid **only** between a successful `agt_frame_begin` and the
/// matching `agt_frame_commit`; it must never be stored past that window.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct agt_frame_desc {
    /// 0xRRGGBB-style XRGB pixel buffer, owned by the library.
    pub pixels: *mut u32,
    pub width: u32,
    pub height: u32,
    /// Row stride in pixels (XRGB buffers are tightly packed: stride == width).
    pub stride_px: u32,
}

/// Opaque window handle sentinel. Owned by the library; released exactly once
/// via `agt_window_close`.
#[repr(C)]
pub struct agt_window {
    _private: [u8; 0],
}

#[allow(non_camel_case_types)]
pub type agt_window_t = *mut agt_window;

/// Bounded event queue: `event()` never blocks and never grows unbounded;
/// when full, the oldest event is dropped.
const EVENT_QUEUE_CAP: usize = 256;
/// Budget for `agt_window_open` to observe `opened()` or a headless
/// Unsupported/Failed exit. Headless hosts fail fast; interactive hosts call
/// `opened()` within milliseconds.
const WINDOW_OPEN_WAIT_MS: u64 = 10_000;

/// Raw frame pointer crossing the loop → caller rendezvous. `*mut u32` is not
/// `Send`; the pointer is produced on the library-private loop thread and
/// consumed by the ABI caller thread between `agt_frame_begin` and
/// `agt_frame_commit`, so a thin Send wrapper is required (same pattern as the
/// PTY waiter's shared state). Validity is bounded by the begin/commit window;
/// the caller must never dereference it after commit or close.
struct FramePtr(*mut u32);
unsafe impl Send for FramePtr {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FramePhase {
    /// Published by `render()`; available to `agt_frame_begin`.
    Waiting,
    /// `agt_frame_begin` handed the pointer to the caller; awaiting commit.
    Held,
    /// `agt_frame_commit` released the rendezvous; the loop thread may present.
    Committed,
}

/// One frame published by `render()` at the rendezvous point.
struct FrameSlot {
    generation: u64,
    ptr: FramePtr,
    width: u32,
    height: u32,
    stride_px: u32,
    phase: FramePhase,
}

struct EventRecord {
    kind: u32,
    generation: u64,
    width: u32,
    height: u32,
    scale: f64,
    focused: i32,
}

/// Why the window loop exited before `opened()` (or after it, on close).
enum OpenOutcome {
    Unsupported(String),
    Failed { code: String, message: String },
    ExitedClean,
}

struct WindowState {
    closed: bool,
    opened: bool,
    open_outcome: Option<OpenOutcome>,
    events: VecDeque<EventRecord>,
    pending_frame: Option<FrameSlot>,
    last_geometry: Option<(u32, u32, f64)>,
    redraw_requested: bool,
    waker: Option<WindowWaker>,
    next_generation: u64,
    /// `render()`'s `frame.commit(Full)` failed after the caller released the
    /// rendezvous; reported on the caller's next `agt_frame_begin`.
    commit_failed: Option<String>,
}

/// All state shared between the ABI caller thread and the library-private
/// `run_pixel_window` loop thread. One mutex + one condvar: event enqueue
/// never blocks, and every waiter (poll, begin, commit, open) is woken by
/// notify_all on the same condvar.
struct WindowShared {
    state: Mutex<WindowState>,
    cond: Condvar,
}

impl WindowShared {
    fn new() -> Self {
        Self {
            state: Mutex::new(WindowState {
                closed: false,
                opened: false,
                open_outcome: None,
                events: VecDeque::with_capacity(EVENT_QUEUE_CAP),
                pending_frame: None,
                last_geometry: None,
                redraw_requested: false,
                waker: None,
                next_generation: 0,
                commit_failed: None,
            }),
            cond: Condvar::new(),
        }
    }

    fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    fn next_generation(&self) -> u64 {
        let mut guard = lock(&self.state);
        guard.next_generation = guard.next_generation.wrapping_add(1);
        guard.next_generation
    }

    /// `event()` entry: enqueue one record, bounded (drop oldest when full),
    /// never blocks.
    fn enqueue(&self, record: EventRecord) {
        let mut guard = lock(&self.state);
        if guard.closed {
            return;
        }
        guard.events.push_back(record);
        if guard.events.len() > EVENT_QUEUE_CAP {
            guard.events.pop_front();
        }
        drop(guard);
        self.cond.notify_all();
    }

    /// `render()` entry: publish the frame at the rendezvous point, enqueue
    /// RENDER_DUE, and wake every `agt_frame_begin` waiter.
    fn publish_frame(&self, slot: FrameSlot) {
        let generation = slot.generation;
        let mut guard = lock(&self.state);
        guard.pending_frame = Some(slot);
        guard.events.push_back(EventRecord {
            kind: AGT_EV_RENDER_DUE,
            generation,
            width: 0,
            height: 0,
            scale: 0.0,
            focused: 0,
        });
        if guard.events.len() > EVENT_QUEUE_CAP {
            guard.events.pop_front();
        }
        drop(guard);
        self.cond.notify_all();
    }

    /// `render()` rendezvous half: block until the caller calls
    /// `agt_frame_commit` (returns true) or the window is closed (returns
    /// false → the loop thread must return Exit).
    fn wait_commit_or_close(&self) -> bool {
        let mut guard = lock(&self.state);
        loop {
            if guard.closed {
                return false;
            }
            if let Some(slot) = guard.pending_frame.as_ref()
                && slot.phase == FramePhase::Committed
            {
                return true;
            }
            guard = self
                .cond
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// `render()` entry after a successful commit: store the platform commit
    /// error so the caller observes it on the next `agt_frame_begin`.
    fn record_commit_failed(&self, message: String) {
        let mut guard = lock(&self.state);
        guard.commit_failed = Some(message);
        drop(guard);
        self.cond.notify_all();
    }

    fn set_geometry(&self, width: u32, height: u32, scale: f64) {
        let mut guard = lock(&self.state);
        guard.last_geometry = Some((width, height, scale));
        drop(guard);
        self.cond.notify_all();
    }

    /// `agt_window_close`: mark closed, wake every waiter (including a caller
    /// blocked in `agt_frame_begin` and the loop thread's rendezvous wait),
    /// and wake the platform loop so it returns Exit.
    fn request_close(&self) {
        let waker = {
            let mut guard = lock(&self.state);
            guard.closed = true;
            guard.redraw_requested = false;
            guard.waker.clone()
        };
        self.cond.notify_all();
        if let Some(waker) = waker {
            let _ = waker.wake();
        }
    }

    /// Loop thread exit (run_pixel_window returned): mark closed, record the
    /// outcome for a still-waiting `agt_window_open`, and wake everyone.
    fn on_loop_exited(&self, result: Result<(), PixelWindowError>) {
        let mut guard = lock(&self.state);
        guard.closed = true;
        guard.open_outcome = Some(match result {
            Ok(()) => OpenOutcome::ExitedClean,
            Err(PixelWindowError::Unsupported { reason }) => {
                OpenOutcome::Unsupported(reason.to_string())
            }
            Err(PixelWindowError::Failed { code, message }) => OpenOutcome::Failed {
                code: code.to_string(),
                message,
            },
            Err(_) => OpenOutcome::Failed {
                code: "pixel_window_unknown".to_string(),
                message: "unknown window host error".to_string(),
            },
        });
        drop(guard);
        self.cond.notify_all();
    }
}

/// Real state behind an opaque `agt_window_t`.
struct WindowHandle {
    shared: Arc<WindowShared>,
}

/// The `PixelWindowApplication` owned by the library-private loop thread.
struct WindowApp {
    shared: Arc<WindowShared>,
}

/// Read width/height/scale for a geometry event. The `metrics` payload has all
/// fields; on an invalid/zero payload fall back to `window.metrics()` /
/// `window.scale_factor()` (the platform never returns a zero size from those).
fn geometry_of(window: &PixelWindow, metrics: PixelWindowMetrics) -> (u32, u32, f64) {
    let mut width = metrics.physical_width;
    let mut height = metrics.physical_height;
    let mut scale = metrics.scale_factor;
    let valid = width > 0 && height > 0 && scale.is_finite() && scale > 0.0;
    if !valid && let Ok(m) = window.metrics() {
        if m.physical_width > 0 {
            width = m.physical_width;
        }
        if m.physical_height > 0 {
            height = m.physical_height;
        }
        if m.scale_factor.is_finite() && m.scale_factor > 0.0 {
            scale = m.scale_factor;
        }
    }
    if width == 0 {
        width = 1;
    }
    if height == 0 {
        height = 1;
    }
    if !(scale.is_finite() && scale > 0.0) {
        scale = 1.0;
    }
    (width, height, scale)
}

impl PixelWindowApplication for WindowApp {
    fn opened(&mut self, window: &PixelWindow) -> Result<PixelWindowDirective, PixelWindowError> {
        // Record the waker (close/redraw need it), try to record geometry so
        // agt_window_metrics works right after open, and signal open's wait.
        let metrics = window.metrics().ok();
        let waker = window.waker();
        {
            let mut guard = lock(&self.shared.state);
            guard.waker = Some(waker);
            if let Some(m) = metrics {
                guard.last_geometry = Some((m.physical_width, m.physical_height, m.scale_factor));
            }
            guard.opened = true;
        }
        self.shared.cond.notify_all();
        // Drive the first frame: the caller's first agt_frame_begin waits for
        // the frame published by the render() that this request schedules.
        window.request_redraw();
        Ok(PixelWindowDirective::Continue)
    }

    fn event(
        &mut self,
        window: &PixelWindow,
        event: PixelWindowEvent,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        match event {
            // Only these three events are translated in 3a; every other
            // variant is deliberately dropped (no queue entry, no error).
            PixelWindowEvent::CloseRequested => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(EventRecord {
                    kind: AGT_EV_CLOSE_REQUEST,
                    generation,
                    width: 0,
                    height: 0,
                    scale: 0.0,
                    focused: 0,
                });
                // Do not auto-exit: the caller decides (via agt_window_close).
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::GeometryChanged { change: _, metrics } => {
                let (width, height, scale) = geometry_of(window, metrics);
                self.shared.set_geometry(width, height, scale);
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(EventRecord {
                    kind: AGT_EV_GEOMETRY,
                    generation,
                    width,
                    height,
                    scale,
                    focused: 0,
                });
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::FocusChanged(focused) => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(EventRecord {
                    kind: AGT_EV_FOCUS,
                    generation,
                    width: 0,
                    height: 0,
                    scale: 0.0,
                    focused: focused as i32,
                });
                Ok(PixelWindowDirective::Continue)
            }
            _ => Ok(PixelWindowDirective::Continue),
        }
    }

    fn render(
        &mut self,
        window: &PixelWindow,
        frame: &mut XrgbPixelFrame<'_>,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        // Keep agt_window_metrics fresh even if no GeometryChanged arrives.
        if let Ok(m) = window.metrics() {
            self.shared
                .set_geometry(m.physical_width, m.physical_height, m.scale_factor);
        }
        // Publish the frame and the raw pointer, then rendezvous: block until
        // agt_frame_commit releases us (or the window is closed).
        let generation = self.shared.next_generation();
        let slot = FrameSlot {
            generation,
            ptr: FramePtr(frame.pixels_mut().as_mut_ptr()),
            width: frame.width(),
            height: frame.height(),
            stride_px: frame.width(),
            phase: FramePhase::Waiting,
        };
        self.shared.publish_frame(slot);
        if !self.shared.wait_commit_or_close() {
            return Ok(PixelWindowDirective::Exit);
        }
        // Released: the caller has finished writing pixels; present the frame.
        match frame.commit(PixelFrameWrite::Full) {
            Ok(_) => {}
            Err(e) => self.shared.record_commit_failed(format!("{e}")),
        }
        if self.shared.is_closed() {
            Ok(PixelWindowDirective::Exit)
        } else {
            Ok(PixelWindowDirective::Continue)
        }
    }

    fn about_to_wait(
        &mut self,
        window: &PixelWindow,
        _now: Instant,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        let (closed, redraw) = {
            let mut guard = lock(&self.shared.state);
            (guard.closed, std::mem::take(&mut guard.redraw_requested))
        };
        if closed {
            return Ok(PixelWindowDirective::Exit);
        }
        if redraw {
            window.request_redraw();
            return Ok(PixelWindowDirective::Continue);
        }
        Ok(PixelWindowDirective::Wait)
    }
}

/// Open a native pixel window. The window loop runs on a library-private
/// thread (the platform contract is a blocking callback loop); the returned
/// handle belongs to the calling thread, and frames/events rendezvous back
/// through `agt_frame_begin` / `agt_window_poll_event`.
///
/// Headless hosts where the window host reports `AGT_UNSUPPORTED` return
/// `AGT_UNSUPPORTED` here; every other failure is `AGT_FAILED`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_window_open(
    spec: *const agt_window_spec,
    out: *mut agt_window_t,
) -> agt_status {
    fn inner(spec: *const agt_window_spec, out: *mut agt_window_t) -> agt_status {
        if spec.is_null() || out.is_null() {
            record_error(c"agt_window_open", c"bad_pointer", "spec or out is null");
            return agt_status::AGT_FAILED;
        }
        let spec = unsafe { &*spec };
        let title = if spec.title.is_null() {
            record_error(c"agt_window_open", c"bad_pointer", "title is null");
            return agt_status::AGT_FAILED;
        } else {
            match unsafe { CStr::from_ptr(spec.title) }.to_str() {
                Ok(s) => s,
                Err(_) => {
                    record_error(
                        c"agt_window_open",
                        c"bad_encoding",
                        "title is not valid UTF-8",
                    );
                    return agt_status::AGT_FAILED;
                }
            }
        };
        if spec.width == 0 || spec.height == 0 {
            record_error(
                c"agt_window_open",
                c"bad_size",
                "width and height must be >= 1",
            );
            return agt_status::AGT_FAILED;
        }

        let shared = Arc::new(WindowShared::new());
        let options = PixelWindowOptions::new(
            title,
            LogicalSize::new(spec.width as f64, spec.height as f64),
        )
        .with_no_activate(spec.no_activate != 0)
        .with_ime_allowed(spec.ime_allowed != 0);
        let app = WindowApp {
            shared: Arc::clone(&shared),
        };
        let loop_shared = Arc::clone(&shared);

        // Library-private loop thread. `run_pixel_window` blocks on this
        // thread (message pump on Windows); events and frames rendezvous back
        // to the caller via shared state. The concrete `WindowApp` (Send: it
        // only owns an Arc) is moved in and boxed as the trait object here so
        // the task closure stays Send.
        let task = Box::new(move || {
            let app: Box<dyn PixelWindowApplication> = Box::new(app);
            let result = run_pixel_window(options, app);
            loop_shared.on_loop_exited(result);
        });
        if let Err(e) = spawn_named_detached("agenterm-abi-window-loop", task) {
            record_error(
                c"agt_window_open",
                c"loop_thread_failed",
                format!("spawn window loop thread: {e}"),
            );
            return agt_status::AGT_FAILED;
        }

        // Wait for opened() or a fast headless failure. Never returns while
        // the loop is still healthy but the window is not yet up.
        let deadline = Instant::now() + Duration::from_millis(WINDOW_OPEN_WAIT_MS);
        let mut guard = lock(&shared.state);
        loop {
            if guard.opened {
                drop(guard);
                let handle = Box::new(WindowHandle { shared });
                unsafe { *out = Box::into_raw(handle) as agt_window_t };
                return agt_status::AGT_OK;
            }
            if let Some(outcome) = guard.open_outcome.as_ref() {
                let status = match outcome {
                    OpenOutcome::Unsupported(reason) => {
                        record_error(
                            c"agt_window_open",
                            c"unsupported",
                            format!("window host unavailable on this platform: {reason}"),
                        );
                        agt_status::AGT_UNSUPPORTED
                    }
                    OpenOutcome::Failed { code, message } => {
                        record_error(
                            c"agt_window_open",
                            c"open_failed",
                            format!("window host failed ({code}): {message}"),
                        );
                        agt_status::AGT_FAILED
                    }
                    OpenOutcome::ExitedClean => {
                        record_error(
                            c"agt_window_open",
                            c"open_failed",
                            "window host exited before opened()",
                        );
                        agt_status::AGT_FAILED
                    }
                };
                drop(guard);
                return status;
            }
            if Instant::now() >= deadline {
                record_error(
                    c"agt_window_open",
                    c"open_timeout",
                    "window host did not report opened() within the budget",
                );
                return agt_status::AGT_FAILED;
            }
            let remaining = deadline - Instant::now();
            let (g, _) = shared
                .cond
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = g;
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(spec, out))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_window_open", c"panic", "panic in agt_window_open");
            agt_status::AGT_FAILED
        }
    }
}

/// Pop the next window event into `*out`, waiting up to `timeout_ms`.
/// Timeout returns `AGT_FAILED { code = "timeout" }`; a closed window with an
/// empty queue returns `AGT_FAILED { code = "closed" }`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_window_poll_event(
    window: agt_window_t,
    out: *mut agt_event,
    timeout_ms: u32,
) -> agt_status {
    fn inner(window: agt_window_t, out: *mut agt_event, timeout_ms: u32) -> agt_status {
        if window.is_null() || out.is_null() {
            record_error(
                c"agt_window_poll_event",
                c"bad_pointer",
                "window or out is null",
            );
            return agt_status::AGT_FAILED;
        }
        let shared = unsafe { &*(window as *const WindowHandle) }.shared.clone();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut guard = lock(&shared.state);
        loop {
            if let Some(ev) = guard.events.pop_front() {
                unsafe {
                    *out = agt_event {
                        kind: ev.kind,
                        generation: ev.generation,
                        width: ev.width,
                        height: ev.height,
                        scale: ev.scale,
                        focused: ev.focused,
                    };
                }
                return agt_status::AGT_OK;
            }
            if guard.closed {
                record_error(c"agt_window_poll_event", c"closed", "window is closed");
                return agt_status::AGT_FAILED;
            }
            if Instant::now() >= deadline {
                record_error(
                    c"agt_window_poll_event",
                    c"timeout",
                    "no event within timeout_ms",
                );
                return agt_status::AGT_FAILED;
            }
            let remaining = deadline - Instant::now();
            let (g, _) = shared
                .cond
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = g;
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window, out, timeout_ms))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_window_poll_event",
                c"panic",
                "panic in agt_window_poll_event",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Ask the loop thread to schedule a redraw (wakes it from its platform
/// wait). The next `render()` rendezvous publishes a fresh frame for
/// `agt_frame_begin`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_window_request_redraw(window: agt_window_t) -> agt_status {
    fn inner(window: agt_window_t) -> agt_status {
        if window.is_null() {
            record_error(
                c"agt_window_request_redraw",
                c"bad_pointer",
                "window is null",
            );
            return agt_status::AGT_FAILED;
        }
        let shared = unsafe { &*(window as *const WindowHandle) }.shared.clone();
        let waker = {
            let mut guard = lock(&shared.state);
            if guard.closed {
                record_error(c"agt_window_request_redraw", c"closed", "window is closed");
                return agt_status::AGT_FAILED;
            }
            guard.redraw_requested = true;
            guard.waker.clone()
        };
        if let Some(waker) = waker {
            let _ = waker.wake();
        }
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_window_request_redraw",
                c"panic",
                "panic in agt_window_request_redraw",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Rendezvous half of the frame protocol: wait (up to `timeout_ms`) for the
/// loop thread's `render()` to publish a frame, then fill `*out` with the
/// frame's pixel pointer / size. The pointer is valid only until the matching
/// `agt_frame_commit`. Timeout returns `AGT_FAILED { code = "timeout" }`;
/// calling again while a previous frame is un-committed returns
/// `AGT_FAILED { code = "frame_pending" }`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_frame_begin(
    window: agt_window_t,
    out: *mut agt_frame_desc,
    timeout_ms: u32,
) -> agt_status {
    fn inner(window: agt_window_t, out: *mut agt_frame_desc, timeout_ms: u32) -> agt_status {
        if window.is_null() || out.is_null() {
            record_error(c"agt_frame_begin", c"bad_pointer", "window or out is null");
            return agt_status::AGT_FAILED;
        }
        let shared = unsafe { &*(window as *const WindowHandle) }.shared.clone();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut guard = lock(&shared.state);
        loop {
            if guard.closed {
                record_error(c"agt_frame_begin", c"closed", "window is closed");
                return agt_status::AGT_FAILED;
            }
            // A commit failure from the previous frame surfaces here: the
            // caller must learn that its pixels never reached the host.
            if let Some(message) = guard.commit_failed.take() {
                record_error(
                    c"agt_frame_begin",
                    c"frame_commit_failed",
                    format!("previous frame.commit failed: {message}"),
                );
                return agt_status::AGT_FAILED;
            }
            if let Some(slot) = guard.pending_frame.as_mut() {
                match slot.phase {
                    FramePhase::Waiting => {
                        slot.phase = FramePhase::Held;
                        unsafe {
                            *out = agt_frame_desc {
                                pixels: slot.ptr.0,
                                width: slot.width,
                                height: slot.height,
                                stride_px: slot.stride_px,
                            };
                        }
                        return agt_status::AGT_OK;
                    }
                    FramePhase::Held => {
                        record_error(
                            c"agt_frame_begin",
                            c"frame_pending",
                            "previous frame was not committed",
                        );
                        return agt_status::AGT_FAILED;
                    }
                    FramePhase::Committed => {
                        // Released frame; wait for the loop thread to publish
                        // the next one.
                    }
                }
            }
            if Instant::now() >= deadline {
                record_error(
                    c"agt_frame_begin",
                    c"timeout",
                    "no frame published within timeout_ms",
                );
                return agt_status::AGT_FAILED;
            }
            let remaining = deadline - Instant::now();
            let (g, _) = shared
                .cond
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = g;
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window, out, timeout_ms))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_frame_begin", c"panic", "panic in agt_frame_begin");
            agt_status::AGT_FAILED
        }
    }
}

/// Release the pending frame: wake the loop thread so it presents the pixels
/// the caller wrote. Exactly once per frame; without a pending (held) frame
/// returns `AGT_FAILED { code = "no_frame" }`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_frame_commit(window: agt_window_t) -> agt_status {
    fn inner(window: agt_window_t) -> agt_status {
        if window.is_null() {
            record_error(c"agt_frame_commit", c"bad_pointer", "window is null");
            return agt_status::AGT_FAILED;
        }
        let shared = unsafe { &*(window as *const WindowHandle) }.shared.clone();
        {
            let mut guard = lock(&shared.state);
            match guard.pending_frame.as_mut() {
                Some(slot) if slot.phase == FramePhase::Held => {
                    slot.phase = FramePhase::Committed;
                }
                _ => {
                    record_error(
                        c"agt_frame_commit",
                        c"no_frame",
                        "no pending frame to commit",
                    );
                    return agt_status::AGT_FAILED;
                }
            }
        }
        shared.cond.notify_all();
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_frame_commit", c"panic", "panic in agt_frame_commit");
            agt_status::AGT_FAILED
        }
    }
}

/// Report the last known window geometry (physical pixels + scale factor).
/// Returns `AGT_FAILED { code = "no_geometry" }` before the first
/// GeometryChanged event / render has been observed.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_window_metrics(
    window: agt_window_t,
    width: *mut u32,
    height: *mut u32,
    scale: *mut f64,
) -> agt_status {
    fn inner(
        window: agt_window_t,
        width: *mut u32,
        height: *mut u32,
        scale: *mut f64,
    ) -> agt_status {
        if window.is_null() || width.is_null() || height.is_null() || scale.is_null() {
            record_error(
                c"agt_window_metrics",
                c"bad_pointer",
                "window, width, height or scale is null",
            );
            return agt_status::AGT_FAILED;
        }
        let shared = unsafe { &*(window as *const WindowHandle) }.shared.clone();
        let guard = lock(&shared.state);
        match guard.last_geometry {
            Some((w, h, s)) => {
                unsafe {
                    *width = w;
                    *height = h;
                    *scale = s;
                }
                agt_status::AGT_OK
            }
            None => {
                record_error(
                    c"agt_window_metrics",
                    c"no_geometry",
                    "no window geometry recorded yet",
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window, width, height, scale))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_window_metrics",
                c"panic",
                "panic in agt_window_metrics",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Close a window and release its handle. Must be called exactly once. Wakes
/// any caller blocked in `agt_frame_begin` / `agt_window_poll_event` on
/// another thread and lets the loop thread escape its rendezvous wait
/// (even if the caller never committed a taken frame), so close never hangs.
#[unsafe(no_mangle)]
pub extern "C" fn agt_window_close(window: agt_window_t) {
    if window.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let handle = unsafe { Box::from_raw(window as *mut WindowHandle) };
        handle.shared.request_close();
        drop(handle);
    }));
}
