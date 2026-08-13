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
//! milestone 3a added the window lifecycle / pixel-frame rendezvous
//! (`agt_window_open/poll_event/request_redraw/metrics/close` plus
//! `agt_frame_begin/commit`); milestone 3b wires the events 3a deliberately
//! dropped — keyboard / pointer / wheel / IME — into the queue, and adds
//! `agt_window_event_text` for out-of-band IME text (preedit/commit can be
//! long, so it is never truncated into the fixed-size POD record).
//! Milestone 4 ships the screenshot mechanism: `agt_screenshot_write_png`
//! encodes a caller-owned XRGB framebuffer to PNG, and
//! `agt_screenshot_capture_window` captures a native window (or its strict
//! client-area rectangle) to PNG. Cropping is not offered this round (the
//! platform `XrgbClip` type is not nameable from this crate).
//! Milestone 5 closes the Phase 0 process group: `agt_process_list` enumerates
//! live processes (two-stage, §3.4), `agt_process_kill` terminates by pid, and
//! `agt_process_self` reports the calling process pid. `AGT_CAP_PROCESS_OBSERVE`
//! now reports `AGT_OK`; spawn stays `AGT_UNSUPPORTED` (not built this round).
//! Milestone 6 ships structured accessibility-tree observe and node actuation:
//! `agt_a11y_tree_snapshot` captures a flattened tree, variable-length node fields
//! are fetched with `agt_a11y_node_string` / `agt_a11y_node_action_name`, and
//! `agt_a11y_node_perform` invokes click/focus by child-index path,
//! `agt_a11y_node_set_text` writes through the host text interface (Linux:
//! AT-SPI EditableText), and `agt_a11y_node_send_keys` delivers Device/key
//! events (Linux: AT-SPI DeviceEventListener). Backends are
//! the host accessibility stack (Windows UIA / macOS AX / Linux AT-SPI2) behind
//! `agenterm-platform`; the C header names mechanisms only.
//! Milestone 8 ships the clipboard mechanism: `agt_clipboard_set_text`
//! publishes UTF-8 text, `agt_clipboard_get_text` reads it back two-stage, and
//! `agt_clipboard_has_text` probes for Unicode text. `AGT_CAP_CLIPBOARD` now
//! reports `AGT_OK`.
//! Milestone 9 ships the parent-console write mechanism:
//! `agt_parent_console_write_stdout` / `agt_parent_console_write_stderr`
//! forward one UTF-8 line to the parent console. "No writable parent console"
//! maps to `AGT_UNSUPPORTED` (the environment lacks the mechanism), never
//! `AGT_FAILED`; `AGT_CAP_PARENT_CONSOLE` now reports `AGT_OK`.
//! Milestone 10 ships the runtime-environment group: user config directory
//! and default terminal shell (both two-stage, §3.4), an ASCII environment
//! variable probe, and the process argument list (`agt_runtime_arg_count` /
//! `agt_runtime_arg`). Runtime exports have no capability entry — they are
//! always available on a built library, so the capability enum is untouched.
//! Milestone 43 ships the native-window and input-injection mechanisms that
//! `agenterm-cu` consumes: `agt_window_enumerate` (two-stage, §3.4, with the
//! same inline fixed-size string truncation as `agt_process_list`),
//! `agt_native_window_show/move/rect/set_topmost/close` (native OS handles
//! only — deliberately distinct from `agt_window_close`, which owns the ABI's
//! own window), and `agt_input_pointer_move/pointer_click/type_text/send_keys`.
//! `AGT_CAP_WINDOW_ENUMERATE` / `AGT_CAP_WINDOW_OP` / `AGT_CAP_INPUT_INJECT`
//! now report the platform's `capability_status()` truthfully instead of a
//! blanket `AGT_OK`: these mechanisms are not guaranteed on every host.
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

use agenterm_platform::CapabilityStatus;
use agenterm_platform::accessibility_tree::{
    AccessibilityNodeAction, AccessibilityTreeError, perform_node_action, send_node_keys,
    set_node_text, tree_for_window,
};
use agenterm_platform::clipboard::{get_text, has_unicode_text, set_text};
use agenterm_platform::ime::ImeEvent;
use agenterm_platform::input::{
    KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent, PhysicalKeyCode,
};
use agenterm_platform::input_inject::{
    PointerButton as InjectPointerButton, PointerPosition, pointer_click, pointer_move, send_keys,
    type_text,
};
use agenterm_platform::parent_console::{write_stderr, write_stdout};
use agenterm_platform::process::{kill, list};
use agenterm_platform::pty::{
    ChildCommand, PtyChild, PtyMaster, TerminalSize, initialize_shutdown_reaper,
    shutdown_session_detached,
};
use agenterm_platform::runtime::{
    application_arguments, ascii_environment_variable_present, default_terminal_shell,
    user_config_directory,
};
use agenterm_platform::screenshot::{
    MAX_FRAME_PIXELS, MAX_FRAME_SIDE, NativeCaptureArea, ScreenshotWindowHandle, XrgbFrame,
    capture_native_window_png, write_xrgb_png,
};
use agenterm_platform::threading::spawn_named_detached;
use agenterm_platform::window_enumerate::{WindowInfo, enumerate_top_level};
use agenterm_platform::window_host::{
    LogicalPoint, LogicalSize, PixelFrameWrite, PixelWindow, PixelWindowApplication,
    PixelWindowDirective, PixelWindowError, PixelWindowEvent, PixelWindowMetrics,
    PixelWindowOptions, PointerButton, PointerButtonState, WheelDelta, WindowWaker, XrgbPixelFrame,
    run_pixel_window,
};
use agenterm_platform::window_op::{
    WindowShowState, close, move_window, set_topmost, show, window_rect,
};

// §3.8 panic fence: building this crate under an abort profile would neuter
// every `catch_unwind` below, so it is a hard compile error with an actionable
// hint instead of a silent footgun. Rust-native rlib consumers without a C
// boundary may opt out via the `allow-abort-profile` feature (panic then
// aborts instead of unwinding, so there is no fence).
#[cfg(all(panic = "abort", not(feature = "allow-abort-profile")))]
compile_error!(
    "libagenterm 必须以 panic=unwind 构建：请用 --profile abi-release（或 abi-dev）。\
     工作区默认 profile（dev/release）为 panic=abort，会静默产出无 catch_unwind 围栏的库。\
     Rust 原生 rlib 消费者（无 C 边界）可开 allow-abort-profile 绕过，但那将放弃 panic 围栏。"
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
    AGT_CAP_ACCESSIBILITY_TREE,
}

/// ABI versioning: the numeric constants and the build-identity string are
/// all derived from the two literals in this single invocation, so they can
/// never drift apart.
///
/// - **major**: bump only on breaking changes (signature change, symbol
///   removal, or semantic change). Consumers must reject a mismatched major.
/// - **minor**: bump on every additive export addition (a new mechanism).
///   Old consumers are unaffected.
macro_rules! abi_version {
    ($major:literal, $minor:literal) => {
        /// ABI major: breaking changes only (see `abi_version!` docs).
        pub const ABI_MAJOR: u16 = $major;
        /// ABI minor: grows with every additive export addition.
        pub const ABI_MINOR: u16 = $minor;
        /// Build identity: `<crate version>+abi.<major>.<minor>`, NUL-terminated.
        const ABI_BUILD_ID: &str = concat!(
            env!("CARGO_PKG_VERSION"),
            "+abi.",
            stringify!($major),
            ".",
            stringify!($minor),
            "\0",
        );
    };
}
abi_version!(1, 4);

/// ABI version: `(major << 16) | minor`. `minor` grows with every additive
/// export; `major` only moves on breaking changes (consumers must reject a
/// mismatched major).
#[unsafe(no_mangle)]
pub extern "C" fn agt_abi_version() -> u32 {
    catch_unwind(|| ((ABI_MAJOR as u32) << 16) | (ABI_MINOR as u32)).unwrap_or(0)
}

/// Human-readable build identity: `<crate version>+abi.<major>.<minor>`
/// (e.g. `0.1.16+abi.1.1`), assembled at compile time from
/// `CARGO_PKG_VERSION` and the `ABI_MAJOR` / `ABI_MINOR` constants — never a
/// hand-written literal. NUL-terminated, static, permanently valid.
#[unsafe(no_mangle)]
pub extern "C" fn agt_build_id() -> *const c_char {
    catch_unwind(|| {
        cstr_static(
            CStr::from_bytes_with_nul(ABI_BUILD_ID.as_bytes())
                .expect("ABI_BUILD_ID is NUL-terminated by construction"),
        )
    })
    .unwrap_or(std::ptr::null())
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
/// capability query. The `pty`, `native-pixel-window`, `screenshot`,
/// `process`, `clipboard` and `parent-console` features are compiled into
/// this build, so `AGT_CAP_PTY`, `AGT_CAP_WINDOW_HOST`,
/// `AGT_CAP_SCREENSHOT`, `AGT_CAP_PROCESS_OBSERVE`, `AGT_CAP_CLIPBOARD` and
/// `AGT_CAP_PARENT_CONSOLE` report `AGT_OK`;
/// `AGT_CAP_ACCESSIBILITY_TREE` reports `AGT_OK` when the host accessibility
/// stack is wired in this build. One platform exception: `AGT_CAP_WINDOW_HOST`
/// reports `AGT_UNSUPPORTED` on macOS, because AppKit requires the window
/// event loop on the main thread while this ABI hosts it on a library-private
/// thread (so `agt_window_open` can never work there). Milestone 43:
/// `AGT_CAP_WINDOW_ENUMERATE` / `AGT_CAP_WINDOW_OP` / `AGT_CAP_INPUT_INJECT`
/// report the platform `capability_status()` truthfully — `AGT_OK` only when
/// the mechanism is actually available on this host (the window-enum /
/// window-op / input-inject features are compiled in, but the host adapter
/// may still be absent, e.g. a headless build). Mechanisms that have not
/// shipped yet report `AGT_UNSUPPORTED` (a product gap, never a permission
/// statement).
#[unsafe(no_mangle)]
pub extern "C" fn agt_capability_query(cap: agt_capability) -> agt_status {
    fn capability_ok(status: CapabilityStatus) -> agt_status {
        if matches!(status, CapabilityStatus::Available) {
            agt_status::AGT_OK
        } else {
            agt_status::AGT_UNSUPPORTED
        }
    }
    match cap {
        agt_capability::AGT_CAP_PTY
        | agt_capability::AGT_CAP_SCREENSHOT
        | agt_capability::AGT_CAP_PROCESS_OBSERVE
        | agt_capability::AGT_CAP_CLIPBOARD
        | agt_capability::AGT_CAP_PARENT_CONSOLE => agt_status::AGT_OK,
        // AppKit requires the window event loop on the main thread; this ABI
        // hosts it on a library-private thread, so the window host mechanism
        // does not exist on macOS (`agt_window_open` returns AGT_UNSUPPORTED
        // there, and a retry can never succeed).
        agt_capability::AGT_CAP_WINDOW_HOST => {
            if cfg!(target_os = "macos") {
                agt_status::AGT_UNSUPPORTED
            } else {
                agt_status::AGT_OK
            }
        }
        agt_capability::AGT_CAP_ACCESSIBILITY_TREE => {
            if a11y_mechanism_available() {
                agt_status::AGT_OK
            } else {
                agt_status::AGT_UNSUPPORTED
            }
        }
        // Milestone 43: report the host's real capability status for the
        // native-window and input-injection mechanisms (never a blanket
        // AGT_OK — Linux/macOS hosts may not implement them).
        agt_capability::AGT_CAP_WINDOW_ENUMERATE => {
            capability_ok(agenterm_platform::window_enumerate::capability_status())
        }
        agt_capability::AGT_CAP_WINDOW_OP => {
            capability_ok(agenterm_platform::window_op::capability_status())
        }
        agt_capability::AGT_CAP_INPUT_INJECT => {
            capability_ok(agenterm_platform::input_inject::capability_status())
        }
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

/// Event kinds carried by `agt_event`. Milestone 3a translated the first five;
/// milestone 3b adds KEY / POINTER / WHEEL / IME. Unknown platform event
/// variants are dropped (no queue entry, no error) per the ABI contract.
pub const AGT_EV_NONE: u32 = 0;
pub const AGT_EV_CLOSE_REQUEST: u32 = 1;
pub const AGT_EV_GEOMETRY: u32 = 2;
pub const AGT_EV_FOCUS: u32 = 3;
pub const AGT_EV_RENDER_DUE: u32 = 4;
pub const AGT_EV_KEY: u32 = 5;
pub const AGT_EV_POINTER: u32 = 6;
pub const AGT_EV_WHEEL: u32 = 7;
pub const AGT_EV_IME: u32 = 8;

/// `modifiers` bitmask (shared by KEY / POINTER; 0 when not applicable).
pub const AGT_MOD_CONTROL: u32 = 1;
pub const AGT_MOD_SHIFT: u32 = 2;
pub const AGT_MOD_ALT: u32 = 4;
pub const AGT_MOD_META: u32 = 8;

/// `key_named` codes — ABI-owned numbering, **not** the platform enum order.
/// 0 = unnamed (Character / Unidentified); 255 = unrecognized named key
/// (the `_` fallback arm for new platform variants).
const AGT_KEY_NAMED_OTHER: u8 = 0;
const AGT_KEY_NAMED_UNKNOWN: u8 = 255;

/// C-compatible window event record. All fields are POD; the IME text itself
/// never rides in this struct — it is fetched with `agt_window_event_text`
/// (preedit/commit can be long and must not be truncated).
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
    /// `AGT_MOD_*` bitmask; valid for KEY / POINTER, 0 otherwise.
    pub modifiers: u32,
    /// KEY: 0 = released, 1 = pressed.
    pub key_state: u8,
    /// KEY: 0/1.
    pub key_repeat: u8,
    /// KEY: `key_named` code table (0 = unnamed, 255 = unrecognized).
    pub key_named: u8,
    /// KEY: 0 = other, 1 = letter, 2 = digit, 3 = backspace, 4 = enter,
    /// 5 = space, 6 = tab.
    pub key_physical: u8,
    /// KEY: letter's Unicode codepoint / digit's value / 0 otherwise.
    pub key_physical_value: u32,
    /// KEY: `NormalizedKeyEvent::text`, UTF-8, `text_len` bytes used.
    pub text: [u8; 16],
    pub text_len: u8,
    /// KEY: 1 when `text` was truncated to fit `text[16]`.
    pub text_truncated: u8,
    /// POINTER / WHEEL: logical position; valid only when `has_position != 0`.
    pub pointer_x: f64,
    pub pointer_y: f64,
    /// POINTER: 0 = none/move, 1 = left, 2 = right, 3 = middle, 4 = other.
    pub pointer_button: u8,
    /// POINTER: 0 = released, 1 = pressed, 2 = moved, 3 = left, 4 = capture_lost.
    pub pointer_state: u8,
    /// POINTER / WHEEL: 0/1 — whether `pointer_x`/`pointer_y` are valid.
    pub has_position: u8,
    /// WHEEL: scroll delta.
    pub wheel_x: f64,
    pub wheel_y: f64,
    /// WHEEL: 0 = lines, 1 = logical_pixels.
    pub wheel_unit: u8,
    /// IME: 0 = enabled, 1 = preedit, 2 = commit, 3 = disabled.
    pub ime_kind: u8,
    /// IME: 0/1 — whether `ime_cursor_begin`/`ime_cursor_end` are valid.
    pub has_ime_cursor: u8,
    /// IME: valid when `has_ime_cursor != 0`.
    pub ime_cursor_begin: usize,
    pub ime_cursor_end: usize,
    /// IME: text bytes; fetch the text with `agt_window_event_text`.
    pub ime_text_len: usize,
}

impl Default for agt_event {
    fn default() -> Self {
        Self {
            kind: AGT_EV_NONE,
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

/// One queued event: the C-visible POD record plus an optional out-of-band
/// text buffer (IME preedit/commit, which must never be truncated). On poll
/// the text is moved into the window's text buffer and served by
/// `agt_window_event_text`; the next poll replaces it.
struct EventRecord {
    ev: agt_event,
    event_text: Option<Vec<u8>>,
}

impl EventRecord {
    fn new(ev: agt_event) -> Self {
        Self {
            ev,
            event_text: None,
        }
    }

    fn with_text(ev: agt_event, text: Vec<u8>) -> Self {
        Self {
            ev,
            event_text: Some(text),
        }
    }
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
    /// Text of the most recently polled event (IME preedit/commit), served by
    /// `agt_window_event_text`; replaced by the next poll.
    event_text: Vec<u8>,
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
                event_text: Vec::new(),
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
        guard.events.push_back(EventRecord::new(agt_event {
            kind: AGT_EV_RENDER_DUE,
            generation,
            ..agt_event::default()
        }));
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

// --- milestone 3b: input event translation ------------------------------
//
// All `#[non_exhaustive]` enums must keep a `_` fallback arm; unknown variants
// are dropped (never panic, never error). The mapping functions are pure and
// unit-tested below.

/// `NamedKey` → ABI code table (ABI-owned numbering, not the platform enum
/// order). `_` = 255 (unrecognized named key).
fn named_key_code(key: NamedKey) -> u8 {
    match key {
        NamedKey::ArrowDown => 1,
        NamedKey::ArrowLeft => 2,
        NamedKey::ArrowRight => 3,
        NamedKey::ArrowUp => 4,
        NamedKey::Backspace => 5,
        NamedKey::Delete => 6,
        NamedKey::End => 7,
        NamedKey::Enter => 8,
        NamedKey::Escape => 9,
        NamedKey::F1 => 10,
        NamedKey::F2 => 11,
        NamedKey::F3 => 12,
        NamedKey::F4 => 13,
        NamedKey::F5 => 14,
        NamedKey::F6 => 15,
        NamedKey::F7 => 16,
        NamedKey::F8 => 17,
        NamedKey::F9 => 18,
        NamedKey::F10 => 19,
        NamedKey::F11 => 20,
        NamedKey::F12 => 21,
        NamedKey::Home => 22,
        NamedKey::Insert => 23,
        NamedKey::PageDown => 24,
        NamedKey::PageUp => 25,
        NamedKey::Space => 26,
        NamedKey::Tab => 27,
        _ => AGT_KEY_NAMED_UNKNOWN,
    }
}

/// `PhysicalKeyCode` → (code, value): 0=other, 1=letter, 2=digit, 3=backspace,
/// 4=enter, 5=space, 6=tab; value = letter codepoint / digit value / 0.
fn physical_key_code(code: PhysicalKeyCode) -> (u8, u32) {
    match code {
        PhysicalKeyCode::Letter(c) => (1, c as u32),
        PhysicalKeyCode::Digit(d) => (2, d as u32),
        PhysicalKeyCode::Backspace => (3, 0),
        PhysicalKeyCode::Enter => (4, 0),
        PhysicalKeyCode::Space => (5, 0),
        PhysicalKeyCode::Tab => (6, 0),
        _ => (0, 0),
    }
}

/// `ModifierState` → `AGT_MOD_*` bitmask.
fn modifier_bits(m: ModifierState) -> u32 {
    let mut bits = 0u32;
    if m.control {
        bits |= AGT_MOD_CONTROL;
    }
    if m.shift {
        bits |= AGT_MOD_SHIFT;
    }
    if m.alt {
        bits |= AGT_MOD_ALT;
    }
    if m.meta {
        bits |= AGT_MOD_META;
    }
    bits
}

/// `PointerButton` → ABI code: 0=none/move, 1=left, 2=right, 3=middle, 4=other.
fn pointer_button_code(button: PointerButton) -> u8 {
    match button {
        PointerButton::Left => 1,
        PointerButton::Right => 2,
        PointerButton::Middle => 3,
        PointerButton::Other(_) => 4,
        _ => 4,
    }
}

/// `WheelDelta` → (x, y, unit): unit 0 = lines, 1 = logical_pixels.
fn wheel_delta(delta: WheelDelta) -> (f64, f64, u8) {
    match delta {
        WheelDelta::Lines { x, y } => (x as f64, y as f64, 0),
        WheelDelta::LogicalPixels { x, y } => (x, y, 1),
        _ => (0.0, 0.0, 0),
    }
}

/// Translate a normalized keyboard event into a queue record.
fn key_event_to_record(event: NormalizedKeyEvent, generation: u64) -> EventRecord {
    let mut ev = agt_event {
        kind: AGT_EV_KEY,
        generation,
        ..agt_event::default()
    };
    ev.modifiers = modifier_bits(event.modifiers);
    ev.key_state = match event.state {
        KeyPressState::Pressed => 1,
        KeyPressState::Released => 0,
        _ => 0,
    };
    ev.key_repeat = u8::from(event.repeat);
    match &event.logical {
        // Character text rides in the inline `text` buffer; Named/Unidentified
        // keys carry their `key_named` code and `NormalizedKeyEvent::text`.
        LogicalKey::Named(k) => ev.key_named = named_key_code(*k),
        LogicalKey::Character(_) | LogicalKey::Unidentified => ev.key_named = AGT_KEY_NAMED_OTHER,
        _ => ev.key_named = AGT_KEY_NAMED_UNKNOWN,
    }
    let (physical, physical_value) = physical_key_code(event.physical);
    ev.key_physical = physical;
    ev.key_physical_value = physical_value;
    if let Some(t) = event.text.as_deref() {
        let bytes = t.as_bytes();
        let n = bytes.len().min(ev.text.len());
        ev.text[..n].copy_from_slice(&bytes[..n]);
        ev.text_len = n as u8;
        ev.text_truncated = u8::from(bytes.len() > ev.text.len());
    }
    EventRecord::new(ev)
}

/// Translate a pointer move into a queue record (button = none/move).
fn pointer_moved_to_record(
    position: LogicalPoint,
    modifiers: ModifierState,
    generation: u64,
) -> EventRecord {
    let mut ev = agt_event {
        kind: AGT_EV_POINTER,
        generation,
        ..agt_event::default()
    };
    ev.modifiers = modifier_bits(modifiers);
    ev.pointer_x = position.x;
    ev.pointer_y = position.y;
    ev.pointer_state = 2; // moved
    ev.has_position = 1;
    EventRecord::new(ev)
}

/// Translate `PointerLeft` / `PointerCaptureLost` into a queue record.
fn pointer_exit_to_record(capture_lost: bool, generation: u64) -> EventRecord {
    let mut ev = agt_event {
        kind: AGT_EV_POINTER,
        generation,
        ..agt_event::default()
    };
    ev.pointer_state = if capture_lost { 4 } else { 3 }; // capture_lost / left
    EventRecord::new(ev)
}

/// Translate a pointer button press/release into a queue record.
fn pointer_button_to_record(
    button: PointerButton,
    state: PointerButtonState,
    position: Option<LogicalPoint>,
    modifiers: ModifierState,
    generation: u64,
) -> EventRecord {
    let mut ev = agt_event {
        kind: AGT_EV_POINTER,
        generation,
        ..agt_event::default()
    };
    ev.modifiers = modifier_bits(modifiers);
    ev.pointer_button = pointer_button_code(button);
    ev.pointer_state = match state {
        PointerButtonState::Pressed => 1,
        PointerButtonState::Released => 0,
        _ => 0,
    };
    if let Some(p) = position {
        ev.pointer_x = p.x;
        ev.pointer_y = p.y;
        ev.has_position = 1;
    }
    EventRecord::new(ev)
}

/// Translate a mouse wheel event into a queue record. Position rides in the
/// shared `pointer_x`/`pointer_y`/`has_position` fields.
fn wheel_to_record(
    delta: WheelDelta,
    position: Option<LogicalPoint>,
    modifiers: ModifierState,
    generation: u64,
) -> EventRecord {
    let mut ev = agt_event {
        kind: AGT_EV_WHEEL,
        generation,
        ..agt_event::default()
    };
    ev.modifiers = modifier_bits(modifiers);
    let (x, y, unit) = wheel_delta(delta);
    ev.wheel_x = x;
    ev.wheel_y = y;
    ev.wheel_unit = unit;
    if let Some(p) = position {
        ev.pointer_x = p.x;
        ev.pointer_y = p.y;
        ev.has_position = 1;
    }
    EventRecord::new(ev)
}

/// Translate an IME event into a queue record. Preedit/commit text goes
/// out-of-band (`event_text`) and is served by `agt_window_event_text`.
fn ime_event_to_record(event: ImeEvent, generation: u64) -> EventRecord {
    let mut ev = agt_event {
        kind: AGT_EV_IME,
        generation,
        ..agt_event::default()
    };
    let mut text: Option<Vec<u8>> = None;
    match event {
        ImeEvent::Enabled => ev.ime_kind = 0,
        ImeEvent::Preedit { text: t, cursor } => {
            ev.ime_kind = 1;
            if let Some((begin, end)) = cursor {
                ev.has_ime_cursor = 1;
                ev.ime_cursor_begin = begin;
                ev.ime_cursor_end = end;
            }
            ev.ime_text_len = t.len();
            text = Some(t.into_bytes());
        }
        ImeEvent::Commit(t) => {
            ev.ime_kind = 2;
            ev.ime_text_len = t.len();
            text = Some(t.into_bytes());
        }
        ImeEvent::Disabled => ev.ime_kind = 3,
        _ => {}
    }
    match text {
        Some(t) => EventRecord::with_text(ev, t),
        None => EventRecord::new(ev),
    }
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
            // Milestone 3a events: close / geometry / focus. Milestone 3b
            // adds keyboard / IME / pointer / wheel below; every other
            // variant is deliberately dropped (no queue entry, no error).
            PixelWindowEvent::CloseRequested => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(EventRecord::new(agt_event {
                    kind: AGT_EV_CLOSE_REQUEST,
                    generation,
                    ..agt_event::default()
                }));
                // Do not auto-exit: the caller decides (via agt_window_close).
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::GeometryChanged { change: _, metrics } => {
                let (width, height, scale) = geometry_of(window, metrics);
                self.shared.set_geometry(width, height, scale);
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(EventRecord::new(agt_event {
                    kind: AGT_EV_GEOMETRY,
                    generation,
                    width,
                    height,
                    scale,
                    ..agt_event::default()
                }));
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::FocusChanged(focused) => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(EventRecord::new(agt_event {
                    kind: AGT_EV_FOCUS,
                    generation,
                    focused: focused as i32,
                    ..agt_event::default()
                }));
                Ok(PixelWindowDirective::Continue)
            }
            // --- milestone 3b: keyboard / pointer / wheel / IME ----------
            PixelWindowEvent::Keyboard(event) => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(key_event_to_record(event, generation));
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::Ime(event) => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(ime_event_to_record(event, generation));
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::PointerMoved {
                position,
                modifiers,
            } => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared
                    .enqueue(pointer_moved_to_record(position, modifiers, generation));
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::PointerLeft => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared
                    .enqueue(pointer_exit_to_record(false, generation));
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::PointerCaptureLost => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared
                    .enqueue(pointer_exit_to_record(true, generation));
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::PointerButton {
                button,
                state,
                position,
                modifiers,
            } => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared.enqueue(pointer_button_to_record(
                    button, state, position, modifiers, generation,
                ));
                Ok(PixelWindowDirective::Continue)
            }
            PixelWindowEvent::MouseWheel {
                delta,
                position,
                modifiers,
            } => {
                let generation = lock(&self.shared.state).next_generation;
                self.shared
                    .enqueue(wheel_to_record(delta, position, modifiers, generation));
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
/// `AGT_UNSUPPORTED` here; every other failure is `AGT_FAILED`. macOS is one
/// such host by contract: AppKit requires the event loop on the main thread,
/// this ABI hosts it on a library-private thread, so `AGT_UNSUPPORTED`
/// (`code = "unsupported_platform"`) is returned without starting any thread
/// or touching the window stack (a retry can never succeed and would hit
/// poisoned winit global state).
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
        // macOS has no window host in this ABI: AppKit requires the event
        // loop on the main thread, while this ABI runs it on a
        // library-private thread. Report the platform contract as
        // AGT_UNSUPPORTED up front — never start the loop thread, never call
        // run_pixel_window (winit would panic and poison global state for the
        // whole process). AGT_FAILED would be wrong here: retries cannot
        // succeed.
        if cfg!(target_os = "macos") {
            record_error(
                c"agt_window_open",
                c"unsupported_platform",
                "macOS requires the event loop on the main thread; this ABI hosts it on a library-private thread",
            );
            return agt_status::AGT_UNSUPPORTED;
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
            if let Some(record) = guard.events.pop_front() {
                // The out-of-band text (IME preedit/commit) belongs to this
                // polled event: stage it for agt_window_event_text. The next
                // poll replaces it.
                guard.event_text = record.event_text.unwrap_or_default();
                unsafe {
                    *out = record.ev;
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

/// Fetch the text carried by the most recently polled event (IME preedit /
/// commit may be long, so it is never truncated into the fixed POD record).
/// Two-stage contract (§3.4): the caller allocates a buffer.
///
/// - No pending text: returns `AGT_OK` with `*out_len == 0`.
/// - `cap` too small (including `cap == 0`): returns
///   `AGT_FAILED { code = "buffer_too_small" }` and writes the required byte
///   count into `*out_len`.
/// - Otherwise (`cap >= required`): copies the text bytes and sets `*out_len`
///   to the number copied (equal to the text length).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_window_event_text(
    window: agt_window_t,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(window: agt_window_t, buf: *mut u8, cap: usize, out_len: *mut usize) -> agt_status {
        if window.is_null() {
            record_error(c"agt_window_event_text", c"bad_pointer", "window is null");
            return agt_status::AGT_FAILED;
        }
        if out_len.is_null() {
            record_error(c"agt_window_event_text", c"bad_pointer", "out_len is null");
            return agt_status::AGT_FAILED;
        }
        unsafe { *out_len = 0 };
        let shared = unsafe { &*(window as *const WindowHandle) }.shared.clone();
        let guard = lock(&shared.state);
        let text = guard.event_text.as_slice();
        let required = text.len();
        if cap == 0 {
            // Probing with cap == 0 is the canonical first half of the
            // two-stage call; buf may legitimately be null here.
            unsafe { *out_len = required };
            record_error(
                c"agt_window_event_text",
                c"buffer_too_small",
                "cap is 0; allocate the required byte count and call again",
            );
            return agt_status::AGT_FAILED;
        }
        if buf.is_null() {
            record_error(c"agt_window_event_text", c"bad_pointer", "buf is null");
            return agt_status::AGT_FAILED;
        }
        if cap < required {
            unsafe { *out_len = required };
            record_error(
                c"agt_window_event_text",
                c"buffer_too_small",
                "cap is smaller than the text; allocate the required byte count and call again",
            );
            return agt_status::AGT_FAILED;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), buf, required);
            *out_len = required;
        }
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window, buf, cap, out_len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_window_event_text",
                c"panic",
                "panic in agt_window_event_text",
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

// --- screenshot (milestone 4) ----------------------------------------

/// Encode a caller-owned little-endian `0x00RRGGBB` framebuffer as a PNG at
/// `path`. `pixel_count` must equal `width * height` (both >= 1). Validated
/// before any platform call so bad arguments produce a precise `code` instead
/// of a bare platform error. Cropping is not supported this round (the
/// platform `XrgbClip` type is not nameable from this crate), so the full
/// buffer is always encoded.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_screenshot_write_png(
    path: *const c_char,
    pixels: *const u32,
    pixel_count: usize,
    width: u32,
    height: u32,
) -> agt_status {
    fn inner(
        path: *const c_char,
        pixels: *const u32,
        pixel_count: usize,
        width: u32,
        height: u32,
    ) -> agt_status {
        if path.is_null() {
            record_error(c"agt_screenshot_write_png", c"bad_path", "path is null");
            return agt_status::AGT_FAILED;
        }
        let path = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => std::path::Path::new(s),
            Err(_) => {
                record_error(
                    c"agt_screenshot_write_png",
                    c"bad_path",
                    "path is not valid UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        if pixels.is_null() {
            record_error(
                c"agt_screenshot_write_png",
                c"bad_pointer",
                "pixels is null",
            );
            return agt_status::AGT_FAILED;
        }
        // u64 arithmetic: width/height are caller-supplied u32, so the product
        // can never overflow even on 32-bit hosts.
        let expected = width as u64 * height as u64;
        if width == 0 || height == 0 || pixel_count as u64 != expected {
            record_error(
                c"agt_screenshot_write_png",
                c"bad_dimensions",
                "pixel_count must equal width * height and both must be >= 1",
            );
            return agt_status::AGT_FAILED;
        }
        if width > MAX_FRAME_SIDE || height > MAX_FRAME_SIDE || expected > MAX_FRAME_PIXELS as u64 {
            record_error(
                c"agt_screenshot_write_png",
                c"frame_too_large",
                "frame side exceeds 16384 or pixel count exceeds 64 Mi",
            );
            return agt_status::AGT_FAILED;
        }
        // All validation passed: only now may the raw pointer be turned into
        // a slice (brief rule 6).
        let pixels = unsafe { std::slice::from_raw_parts(pixels, pixel_count) };
        let frame = XrgbFrame::new(path, width, height, pixels);
        match write_xrgb_png(frame) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_screenshot_write_png",
                    c"screenshot_failed",
                    format!("{e}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| {
        inner(path, pixels, pixel_count, width, height)
    })) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_screenshot_write_png",
                c"panic",
                "panic in agt_screenshot_write_png",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Capture a native window (or its strict client-area rectangle) to a PNG at
/// `path`. `native_window` is the platform window handle as `intptr_t`;
/// `area_kind` 0 = whole window, 1 = client rectangle given by
/// `left/top/width/height`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_screenshot_capture_window(
    native_window: isize,
    path: *const c_char,
    area_kind: i32,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
) -> agt_status {
    fn inner(
        native_window: isize,
        path: *const c_char,
        area_kind: i32,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    ) -> agt_status {
        if native_window == 0 {
            record_error(
                c"agt_screenshot_capture_window",
                c"bad_handle",
                "native_window is 0",
            );
            return agt_status::AGT_FAILED;
        }
        let window = match unsafe { ScreenshotWindowHandle::from_raw(native_window) } {
            Some(w) => w,
            None => {
                record_error(
                    c"agt_screenshot_capture_window",
                    c"bad_handle",
                    "native_window is not a valid handle",
                );
                return agt_status::AGT_FAILED;
            }
        };
        if path.is_null() {
            record_error(
                c"agt_screenshot_capture_window",
                c"bad_path",
                "path is null",
            );
            return agt_status::AGT_FAILED;
        }
        let path = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => std::path::Path::new(s),
            Err(_) => {
                record_error(
                    c"agt_screenshot_capture_window",
                    c"bad_path",
                    "path is not valid UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        let area = match area_kind {
            0 => NativeCaptureArea::Window,
            1 => NativeCaptureArea::Client {
                left,
                top,
                width,
                height,
            },
            _ => {
                record_error(
                    c"agt_screenshot_capture_window",
                    c"bad_area",
                    "area_kind must be 0 (whole window) or 1 (client rectangle)",
                );
                return agt_status::AGT_FAILED;
            }
        };
        match capture_native_window_png(window, path, area) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_screenshot_capture_window",
                    c"screenshot_failed",
                    format!("{e}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| {
        inner(native_window, path, area_kind, left, top, width, height)
    })) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_screenshot_capture_window",
                c"panic",
                "panic in agt_screenshot_capture_window",
            );
            agt_status::AGT_FAILED
        }
    }
}

// --- process (milestone 5) -------------------------------------------

/// C-compatible single process record. `name` is UTF-8 and is **not**
/// NUL-terminated by the library; use `name_len` for its length. When the
/// original executable name exceeds 64 bytes it is truncated at a UTF-8
/// character boundary (a multi-byte character is never split) and
/// `name_truncated` is set to 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub struct agt_process_info {
    pub id: u32,
    pub parent_id: u32,
    pub name: [u8; 64],
    /// Bytes actually written into `name` (<= 64).
    pub name_len: u32,
    /// 1 when the original name exceeded 64 bytes.
    pub name_truncated: u32,
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

/// Truncate `s` at a UTF-8 character boundary so at most `max` bytes are
/// kept. Returns `(kept_len, truncated)`; `truncated` is 1 when the original
/// string exceeded `max` bytes (even if the character-boundary cut landed
/// below `max`).
fn truncate_name(s: &str, max: usize) -> (usize, bool) {
    if s.len() <= max {
        (s.len(), false)
    } else {
        (s.floor_char_boundary(max), true)
    }
}

/// Translate one platform process record into the C-compatible record,
/// truncating `executable_name` at a UTF-8 character boundary. The platform
/// `ProcessInfo` type is not nameable from this crate, so it is passed by
/// field (all public).
fn process_info_to_record(id: u32, parent_id: u32, executable_name: &str) -> agt_process_info {
    let (kept, truncated) = truncate_name(executable_name, 64);
    let mut name = [0u8; 64];
    name[..kept].copy_from_slice(&executable_name.as_bytes()[..kept]);
    agt_process_info {
        id,
        parent_id,
        name,
        name_len: kept as u32,
        name_truncated: u32::from(truncated),
    }
}

/// Enumerate live processes into a caller-allocated array (two-stage, §3.4).
///
/// - `cap` sufficient → `AGT_OK`, `*out_count` = records actually written.
/// - `cap` insufficient (including `cap == 0` with `buf == NULL`, the legal
///   "how big?" probe) → `AGT_FAILED { code = "buffer_too_small" }`,
///   `*out_count` = required count.
/// - NULL `out_count` (or NULL `buf` with `cap > 0`) →
///   `AGT_FAILED { code = "bad_pointer" }`.
/// - Platform failure → `AGT_FAILED { code = "process_failed" }`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_process_list(
    buf: *mut agt_process_info,
    cap: usize,
    out_count: *mut usize,
) -> agt_status {
    fn inner(buf: *mut agt_process_info, cap: usize, out_count: *mut usize) -> agt_status {
        if out_count.is_null() {
            record_error(c"agt_process_list", c"bad_pointer", "out_count is null");
            return agt_status::AGT_FAILED;
        }
        if cap > 0 && buf.is_null() {
            record_error(c"agt_process_list", c"bad_pointer", "buf is null");
            return agt_status::AGT_FAILED;
        }
        let processes = match list() {
            Ok(v) => v,
            Err(e) => {
                record_error(c"agt_process_list", c"process_failed", format!("{e}"));
                return agt_status::AGT_FAILED;
            }
        };
        let required = processes.len();
        if cap < required {
            unsafe { *out_count = required };
            record_error(
                c"agt_process_list",
                c"buffer_too_small",
                "cap is smaller than the process count; allocate the required count and call again",
            );
            return agt_status::AGT_FAILED;
        }
        unsafe { *out_count = required };
        for (i, p) in processes.iter().enumerate() {
            unsafe { *buf.add(i) = process_info_to_record(p.id, p.parent_id, &p.executable_name) };
        }
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(buf, cap, out_count))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_process_list", c"panic", "panic in agt_process_list");
            agt_status::AGT_FAILED
        }
    }
}

/// Terminate the given process by pid. `pid == 0` →
/// `AGT_FAILED { code = "bad_pid" }`; a platform failure →
/// `AGT_FAILED { code = "process_failed" }`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_process_kill(pid: u32) -> agt_status {
    fn inner(pid: u32) -> agt_status {
        if pid == 0 {
            record_error(
                c"agt_process_kill",
                c"bad_pid",
                "pid 0 is not a killable process",
            );
            return agt_status::AGT_FAILED;
        }
        match kill(pid) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(c"agt_process_kill", c"process_failed", format!("{e}"));
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(pid))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_process_kill", c"panic", "panic in agt_process_kill");
            agt_status::AGT_FAILED
        }
    }
}

/// pid of the current process. Never fails; a panic inside the fence (not
/// expected) is contained and reported as 0.
#[unsafe(no_mangle)]
pub extern "C" fn agt_process_self() -> u32 {
    catch_unwind(std::process::id).unwrap_or(0)
}

// --- accessibility tree (milestone 6) --------------------------------

const AGT_A11Y_META_BACKEND: i32 = 0;
const AGT_A11Y_META_ROOT_ID: i32 = 1;

const AGT_A11Y_STR_ROLE: i32 = 0;
const AGT_A11Y_STR_NAME: i32 = 1;
const AGT_A11Y_STR_TEXT: i32 = 2;
const AGT_A11Y_STR_STATES: i32 = 3;

const AGT_A11Y_ACTION_CLICK: i32 = 0;
const AGT_A11Y_ACTION_FOCUS: i32 = 1;

/// Fixed-size node record mirroring `include/agenterm.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct agt_a11y_node {
    pub bounds_x: i32,
    pub bounds_y: i32,
    pub bounds_width: i32,
    pub bounds_height: i32,
    pub id: [u8; 64],
    pub id_len: u32,
    pub id_truncated: u32,
    pub parent_id: [u8; 64],
    pub parent_id_len: u32,
    pub parent_id_truncated: u32,
    pub has_parent: u8,
    pub actions_count: u32,
}

struct A11ySnapshot {
    backend: String,
    root_id: String,
    nodes: Vec<agenterm_platform::accessibility_tree::AccessibilityNode>,
}

thread_local! {
    static A11Y_SNAPSHOT: RefCell<Option<A11ySnapshot>> = const { RefCell::new(None) };
}

fn a11y_mechanism_available() -> bool {
    matches!(
        agenterm_platform::accessibility_tree::capability_status(),
        CapabilityStatus::Available
    )
}

fn path_to_fixed(path: &str) -> ([u8; 64], u32, u32) {
    let (kept, truncated) = truncate_name(path, 64);
    let mut bytes = [0u8; 64];
    bytes[..kept].copy_from_slice(&path.as_bytes()[..kept]);
    (bytes, kept as u32, u32::from(truncated))
}

fn node_to_record(
    node: &agenterm_platform::accessibility_tree::AccessibilityNode,
) -> agt_a11y_node {
    let (id, id_len, id_truncated) = path_to_fixed(&node.id);
    let (parent_id, parent_id_len, parent_id_truncated) = match node.parent_id.as_deref() {
        Some(p) => {
            let (bytes, len, trunc) = path_to_fixed(p);
            (bytes, len, trunc)
        }
        None => ([0u8; 64], 0, 0),
    };
    agt_a11y_node {
        bounds_x: node.bounds.x,
        bounds_y: node.bounds.y,
        bounds_width: node.bounds.width,
        bounds_height: node.bounds.height,
        id,
        id_len,
        id_truncated,
        parent_id,
        parent_id_len,
        parent_id_truncated,
        has_parent: u8::from(node.parent_id.is_some()),
        actions_count: node.actions.len() as u32,
    }
}

fn map_a11y_error(operation: &'static CStr, error: AccessibilityTreeError) -> agt_status {
    match error {
        AccessibilityTreeError::Unsupported { .. } => agt_status::AGT_UNSUPPORTED,
        AccessibilityTreeError::Failed { code, message } => {
            let code_cstr: &'static CStr = match code.as_ref() {
                "a11y_connect_failed" => c"a11y_connect_failed",
                "a11y_tree_empty" => c"a11y_tree_empty",
                "a11y_node_not_found" => c"a11y_node_not_found",
                "a11y_action_unavailable" => c"a11y_action_unavailable",
                "a11y_text_unavailable" => c"a11y_text_unavailable",
                "a11y_text_timeout" => c"a11y_text_timeout",
                "a11y_key_unavailable" => c"a11y_key_unavailable",
                "a11y_key_timeout" => c"a11y_key_timeout",
                "invalid_input" => c"invalid_input",
                "a11y_backend_failed" => c"a11y_backend_failed",
                "a11y_invalid_node_id" => c"a11y_invalid_node_id",
                other => {
                    record_error(
                        operation,
                        c"a11y_backend_failed",
                        format!("{other}: {message}"),
                    );
                    return agt_status::AGT_FAILED;
                }
            };
            record_error(operation, code_cstr, message);
            agt_status::AGT_FAILED
        }
        _ => {
            record_error(
                operation,
                c"a11y_backend_failed",
                "unknown accessibility-tree error",
            );
            agt_status::AGT_FAILED
        }
    }
}

fn copy_bytes_two_stage(
    operation: &'static CStr,
    data: &[u8],
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    if out_len.is_null() {
        record_error(operation, c"bad_pointer", "out_len is null");
        return agt_status::AGT_FAILED;
    }
    let required = data.len();
    if cap == 0 {
        unsafe { *out_len = required };
        record_error(
            operation,
            c"buffer_too_small",
            "cap is 0; allocate the required byte count and call again",
        );
        return agt_status::AGT_FAILED;
    }
    if buf.is_null() {
        record_error(operation, c"bad_pointer", "buf is null");
        return agt_status::AGT_FAILED;
    }
    if cap < required {
        unsafe { *out_len = required };
        record_error(
            operation,
            c"buffer_too_small",
            "cap is smaller than the string; allocate the required byte count and call again",
        );
        return agt_status::AGT_FAILED;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), buf, required);
        *out_len = required;
    }
    agt_status::AGT_OK
}

fn with_snapshot_node(
    index: usize,
    operation: &'static CStr,
    f: impl FnOnce(&agenterm_platform::accessibility_tree::AccessibilityNode) -> agt_status,
) -> agt_status {
    A11Y_SNAPSHOT.with(|cell| {
        let guard = cell.borrow();
        let snap = match guard.as_ref() {
            Some(s) => s,
            None => {
                record_error(
                    operation,
                    c"no_snapshot",
                    "call agt_a11y_tree_snapshot before reading nodes",
                );
                return agt_status::AGT_FAILED;
            }
        };
        let node = match snap.nodes.get(index) {
            Some(n) => n,
            None => {
                record_error(operation, c"bad_index", "node index is out of range");
                return agt_status::AGT_FAILED;
            }
        };
        f(node)
    })
}

/// Capture a flattened accessibility tree for the host OS stack.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_tree_snapshot(
    window_handle: isize,
    out_node_count: *mut usize,
) -> agt_status {
    fn inner(window_handle: isize, out_node_count: *mut usize) -> agt_status {
        if out_node_count.is_null() {
            record_error(
                c"agt_a11y_tree_snapshot",
                c"bad_pointer",
                "out_node_count is null",
            );
            return agt_status::AGT_FAILED;
        }
        if !a11y_mechanism_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        let filter = if window_handle == 0 {
            None
        } else {
            Some(window_handle)
        };
        let tree = match tree_for_window(filter) {
            Ok(t) => t,
            Err(e) => return map_a11y_error(c"agt_a11y_tree_snapshot", e),
        };
        let count = tree.nodes.len();
        A11Y_SNAPSHOT.with(|cell| {
            *cell.borrow_mut() = Some(A11ySnapshot {
                backend: tree.backend.to_string(),
                root_id: tree.root_id.clone(),
                nodes: tree.nodes,
            });
        });
        unsafe { *out_node_count = count };
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window_handle, out_node_count))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_tree_snapshot",
                c"panic",
                "panic in agt_a11y_tree_snapshot",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Fetch snapshot metadata (backend or root id).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_tree_meta_string(
    field: i32,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(field: i32, buf: *mut u8, cap: usize, out_len: *mut usize) -> agt_status {
        A11Y_SNAPSHOT.with(|cell| {
            let guard = cell.borrow();
            let snap = match guard.as_ref() {
                Some(s) => s,
                None => {
                    record_error(
                        c"agt_a11y_tree_meta_string",
                        c"no_snapshot",
                        "call agt_a11y_tree_snapshot before reading metadata",
                    );
                    return agt_status::AGT_FAILED;
                }
            };
            let data = match field {
                AGT_A11Y_META_BACKEND => snap.backend.as_bytes(),
                AGT_A11Y_META_ROOT_ID => snap.root_id.as_bytes(),
                _ => {
                    record_error(
                        c"agt_a11y_tree_meta_string",
                        c"bad_field",
                        "unknown meta field",
                    );
                    return agt_status::AGT_FAILED;
                }
            };
            copy_bytes_two_stage(c"agt_a11y_tree_meta_string", data, buf, cap, out_len)
        })
    }
    match catch_unwind(AssertUnwindSafe(|| inner(field, buf, cap, out_len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_tree_meta_string",
                c"panic",
                "panic in agt_a11y_tree_meta_string",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Copy one node from the thread-local snapshot.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_tree_node(index: usize, out: *mut agt_a11y_node) -> agt_status {
    fn inner(index: usize, out: *mut agt_a11y_node) -> agt_status {
        if out.is_null() {
            record_error(c"agt_a11y_tree_node", c"bad_pointer", "out is null");
            return agt_status::AGT_FAILED;
        }
        with_snapshot_node(index, c"agt_a11y_tree_node", |node| {
            unsafe { *out = node_to_record(node) };
            agt_status::AGT_OK
        })
    }
    match catch_unwind(AssertUnwindSafe(|| inner(index, out))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_tree_node",
                c"panic",
                "panic in agt_a11y_tree_node",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Fetch a variable-length string for a snapshot node.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_node_string(
    node_index: usize,
    kind: i32,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(
        node_index: usize,
        kind: i32,
        buf: *mut u8,
        cap: usize,
        out_len: *mut usize,
    ) -> agt_status {
        with_snapshot_node(node_index, c"agt_a11y_node_string", |node| {
            let data: Vec<u8> = match kind {
                AGT_A11Y_STR_ROLE => node.role.as_bytes().to_vec(),
                AGT_A11Y_STR_NAME => node.name.as_bytes().to_vec(),
                AGT_A11Y_STR_TEXT => node.text.as_deref().unwrap_or("").as_bytes().to_vec(),
                AGT_A11Y_STR_STATES => {
                    if node.states.is_empty() {
                        Vec::new()
                    } else {
                        node.states.join(",").into_bytes()
                    }
                }
                _ => {
                    record_error(c"agt_a11y_node_string", c"bad_field", "unknown string kind");
                    return agt_status::AGT_FAILED;
                }
            };
            copy_bytes_two_stage(c"agt_a11y_node_string", &data, buf, cap, out_len)
        })
    }
    match catch_unwind(AssertUnwindSafe(|| {
        inner(node_index, kind, buf, cap, out_len)
    })) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_node_string",
                c"panic",
                "panic in agt_a11y_node_string",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Fetch an action name for a snapshot node.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_node_action_name(
    node_index: usize,
    action_index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(
        node_index: usize,
        action_index: usize,
        buf: *mut u8,
        cap: usize,
        out_len: *mut usize,
    ) -> agt_status {
        with_snapshot_node(node_index, c"agt_a11y_node_action_name", |node| {
            let action = match node.actions.get(action_index) {
                Some(a) => a,
                None => {
                    record_error(
                        c"agt_a11y_node_action_name",
                        c"bad_index",
                        "action index is out of range",
                    );
                    return agt_status::AGT_FAILED;
                }
            };
            copy_bytes_two_stage(
                c"agt_a11y_node_action_name",
                action.as_bytes(),
                buf,
                cap,
                out_len,
            )
        })
    }
    match catch_unwind(AssertUnwindSafe(|| {
        inner(node_index, action_index, buf, cap, out_len)
    })) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_node_action_name",
                c"panic",
                "panic in agt_a11y_node_action_name",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Perform click or focus on a child-index path without a prior snapshot.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_node_perform(
    window_handle: isize,
    node_id: *const c_char,
    action: i32,
) -> agt_status {
    fn inner(window_handle: isize, node_id: *const c_char, action: i32) -> agt_status {
        if !a11y_mechanism_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        if node_id.is_null() {
            record_error(c"agt_a11y_node_perform", c"bad_pointer", "node_id is null");
            return agt_status::AGT_FAILED;
        }
        let node_id = match unsafe { CStr::from_ptr(node_id) }.to_str() {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_a11y_node_perform",
                    c"bad_encoding",
                    "node_id is not UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        let platform_action = match action {
            AGT_A11Y_ACTION_CLICK => AccessibilityNodeAction::Click,
            AGT_A11Y_ACTION_FOCUS => AccessibilityNodeAction::Focus,
            _ => {
                record_error(
                    c"agt_a11y_node_perform",
                    c"bad_action",
                    "unknown action kind",
                );
                return agt_status::AGT_FAILED;
            }
        };
        let filter = if window_handle == 0 {
            None
        } else {
            Some(window_handle)
        };
        match perform_node_action(filter, node_id, platform_action) {
            Ok(()) => agt_status::AGT_OK,
            Err(e) => map_a11y_error(c"agt_a11y_node_perform", e),
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(window_handle, node_id, action))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_node_perform",
                c"panic",
                "panic in agt_a11y_node_perform",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Write UTF-8 text through the host accessibility text interface
/// (Linux: AT-SPI `EditableText` `SetTextContents` / `InsertText`, or
/// AT-SPI `Text` plus toolkit set-value when EditableText is absent).
/// `node_id` is a NUL-terminated UTF-8 child-index path. `text == NULL`
/// with `len > 0`, or a slice that is not valid UTF-8, →
/// `AGT_FAILED{code="bad_pointer"}` / `bad_encoding`. A node that does
/// not expose a writeable text interface → `a11y_text_unavailable`.
/// Never injects keystrokes.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_node_set_text(
    window_handle: isize,
    node_id: *const c_char,
    text: *const u8,
    len: usize,
) -> agt_status {
    fn inner(
        window_handle: isize,
        node_id: *const c_char,
        text: *const u8,
        len: usize,
    ) -> agt_status {
        if !a11y_mechanism_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        if node_id.is_null() {
            record_error(c"agt_a11y_node_set_text", c"bad_pointer", "node_id is null");
            return agt_status::AGT_FAILED;
        }
        let node_id = match unsafe { CStr::from_ptr(node_id) }.to_str() {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_a11y_node_set_text",
                    c"bad_encoding",
                    "node_id is not UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        if text.is_null() && len > 0 {
            record_error(c"agt_a11y_node_set_text", c"bad_pointer", "text is null");
            return agt_status::AGT_FAILED;
        }
        let bytes = if text.is_null() {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(text, len) }
        };
        let text = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_a11y_node_set_text",
                    c"bad_encoding",
                    "text is not UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        let filter = if window_handle == 0 {
            None
        } else {
            Some(window_handle)
        };
        match set_node_text(filter, node_id, text) {
            Ok(()) => agt_status::AGT_OK,
            Err(e) => map_a11y_error(c"agt_a11y_node_set_text", e),
        }
    }
    match catch_unwind(AssertUnwindSafe(|| {
        inner(window_handle, node_id, text, len)
    })) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_node_set_text",
                c"panic",
                "panic in agt_a11y_node_set_text",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Deliver a chord through the host accessibility Device/key interface
/// (Linux: AT-SPI `DeviceEventListener` `NotifyEvent`). `node_id` is a
/// NUL-terminated UTF-8 child-index path. `keys == NULL` with `len > 0`,
/// or a slice that is not valid UTF-8, → `AGT_FAILED{code="bad_pointer"}`
/// / `bad_encoding`. A node that does not expose a Device/key interface
/// → `a11y_key_unavailable`. Never injects XTest.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_a11y_node_send_keys(
    window_handle: isize,
    node_id: *const c_char,
    keys: *const u8,
    len: usize,
) -> agt_status {
    fn inner(
        window_handle: isize,
        node_id: *const c_char,
        keys: *const u8,
        len: usize,
    ) -> agt_status {
        if !a11y_mechanism_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        if node_id.is_null() {
            record_error(
                c"agt_a11y_node_send_keys",
                c"bad_pointer",
                "node_id is null",
            );
            return agt_status::AGT_FAILED;
        }
        let node_id = match unsafe { CStr::from_ptr(node_id) }.to_str() {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_a11y_node_send_keys",
                    c"bad_encoding",
                    "node_id is not UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        if keys.is_null() && len > 0 {
            record_error(c"agt_a11y_node_send_keys", c"bad_pointer", "keys is null");
            return agt_status::AGT_FAILED;
        }
        let bytes = if keys.is_null() {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(keys, len) }
        };
        let keys = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_a11y_node_send_keys",
                    c"bad_encoding",
                    "keys is not UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        let filter = if window_handle == 0 {
            None
        } else {
            Some(window_handle)
        };
        match send_node_keys(filter, node_id, keys) {
            Ok(()) => agt_status::AGT_OK,
            Err(e) => map_a11y_error(c"agt_a11y_node_send_keys", e),
        }
    }
    match catch_unwind(AssertUnwindSafe(|| {
        inner(window_handle, node_id, keys, len)
    })) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_a11y_node_send_keys",
                c"panic",
                "panic in agt_a11y_node_send_keys",
            );
            agt_status::AGT_FAILED
        }
    }
}

// --- clipboard (milestone 8) -------------------------------------------

/// Internal read ceiling for `agt_clipboard_get_text`: the library never
/// retains more than this many UTF-8 bytes of clipboard payload. The platform
/// reader treats the bound as a hard limit and *fails* (never splits) beyond
/// it, so a payload that exceeds the ceiling is reported as
/// `clipboard_failed` — the caller never receives a torn multi-byte code
/// point (a UTF-8 string is either delivered whole or not at all).
const MAX_CLIPBOARD_READ_BYTES: usize = 1024 * 1024;

/// Publish UTF-8 text. `text == NULL`, or a slice that is not valid UTF-8 →
/// `AGT_FAILED{code="bad_text"}`; a platform failure (e.g. no clipboard in
/// this session) → `AGT_FAILED{code="clipboard_failed"}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_clipboard_set_text(text: *const u8, len: usize) -> agt_status {
    fn inner(text: *const u8, len: usize) -> agt_status {
        if text.is_null() {
            record_error(
                c"agt_clipboard_set_text",
                c"bad_text",
                "text pointer is null",
            );
            return agt_status::AGT_FAILED;
        }
        // SAFETY: the pointer/length pair is a C ABI contract (see
        // include/agenterm.h); the caller guarantees `len` readable bytes.
        let slice = unsafe { std::slice::from_raw_parts(text, len) };
        let text = match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_clipboard_set_text",
                    c"bad_text",
                    "text is not valid UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        match set_text(text) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_clipboard_set_text",
                    c"clipboard_failed",
                    format!("{e}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(text, len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_clipboard_set_text",
                c"panic",
                "panic in agt_clipboard_set_text",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Read UTF-8 clipboard text (two-stage, §3.4):
/// - `cap` sufficient → `AGT_OK`, `*out_len` = bytes written.
/// - `cap` insufficient → `AGT_FAILED{code="buffer_too_small"}`, `*out_len`
///   = required bytes.
/// - no Unicode text on the clipboard → `AGT_OK` with `*out_len` = 0.
///
/// `out_len == NULL` (or `buf == NULL` with `cap > 0`) →
/// `AGT_FAILED{code="bad_pointer"}`; a platform failure →
/// `AGT_FAILED{code="clipboard_failed"}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_clipboard_get_text(
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(buf: *mut u8, cap: usize, out_len: *mut usize) -> agt_status {
        if out_len.is_null() {
            record_error(
                c"agt_clipboard_get_text",
                c"bad_pointer",
                "out_len pointer is null",
            );
            return agt_status::AGT_FAILED;
        }
        if cap > 0 && buf.is_null() {
            record_error(c"agt_clipboard_get_text", c"bad_pointer", "buf is null");
            return agt_status::AGT_FAILED;
        }
        // No Unicode text at all is not a failure: report an empty read.
        if !has_unicode_text() {
            unsafe { *out_len = 0 };
            return agt_status::AGT_OK;
        }
        let text = match get_text(MAX_CLIPBOARD_READ_BYTES) {
            Ok(s) => s,
            Err(e) => {
                // The probe said text was available but the read failed: the
                // payload may exceed the read ceiling, another process may
                // have emptied the clipboard in between, or the platform
                // adapter failed. Re-probe so "no text" stays a clean empty
                // read instead of a spurious failure.
                if !has_unicode_text() {
                    unsafe { *out_len = 0 };
                    return agt_status::AGT_OK;
                }
                record_error(
                    c"agt_clipboard_get_text",
                    c"clipboard_failed",
                    format!("{e}"),
                );
                return agt_status::AGT_FAILED;
            }
        };
        let required = text.len();
        if cap < required {
            unsafe { *out_len = required };
            record_error(
                c"agt_clipboard_get_text",
                c"buffer_too_small",
                "cap is smaller than the clipboard text; allocate the required size and call again",
            );
            return agt_status::AGT_FAILED;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), buf, required);
            *out_len = required;
        }
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(buf, cap, out_len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_clipboard_get_text",
                c"panic",
                "panic in agt_clipboard_get_text",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// 1 when the clipboard currently holds Unicode text, 0 otherwise. Never
/// fails; a panic inside the fence (not expected) is contained and reported
/// as 0.
#[unsafe(no_mangle)]
pub extern "C" fn agt_clipboard_has_text() -> i32 {
    catch_unwind(|| if has_unicode_text() { 1 } else { 0 }).unwrap_or(0)
}

// --- parent console (milestone 9) -------------------------------------

/// Write UTF-8 text to the parent console's stdout. `text == NULL` (with
/// `len > 0`), or a slice that is not valid UTF-8 →
/// `AGT_FAILED{code="bad_text"}`; "no writable parent console" →
/// `AGT_UNSUPPORTED` (the environment lacks the mechanism, which is *not* a
/// call failure — spec 3.1); success → `AGT_OK`. `len == 0` is legal input:
/// an empty line is written and the platform result is mapped as above.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_parent_console_write_stdout(text: *const u8, len: usize) -> agt_status {
    fn inner(text: *const u8, len: usize) -> agt_status {
        if text.is_null() && len > 0 {
            record_error(
                c"agt_parent_console_write_stdout",
                c"bad_text",
                "text pointer is null",
            );
            return agt_status::AGT_FAILED;
        }
        let message = if len == 0 {
            ""
        } else {
            // SAFETY: the pointer/length pair is a C ABI contract (see
            // include/agenterm.h); the caller guarantees `len` readable bytes.
            let slice = unsafe { std::slice::from_raw_parts(text, len) };
            match std::str::from_utf8(slice) {
                Ok(s) => s,
                Err(_) => {
                    record_error(
                        c"agt_parent_console_write_stdout",
                        c"bad_text",
                        "text is not valid UTF-8",
                    );
                    return agt_status::AGT_FAILED;
                }
            }
        };
        if write_stdout(message) {
            agt_status::AGT_OK
        } else {
            agt_status::AGT_UNSUPPORTED
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(text, len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_parent_console_write_stdout",
                c"panic",
                "panic in agt_parent_console_write_stdout",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Write UTF-8 text to the parent console's stderr. Same contract as
/// `agt_parent_console_write_stdout`; "no writable parent console" maps to
/// `AGT_UNSUPPORTED`, never `AGT_FAILED`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_parent_console_write_stderr(text: *const u8, len: usize) -> agt_status {
    fn inner(text: *const u8, len: usize) -> agt_status {
        if text.is_null() && len > 0 {
            record_error(
                c"agt_parent_console_write_stderr",
                c"bad_text",
                "text pointer is null",
            );
            return agt_status::AGT_FAILED;
        }
        let message = if len == 0 {
            ""
        } else {
            // SAFETY: the pointer/length pair is a C ABI contract (see
            // include/agenterm.h); the caller guarantees `len` readable bytes.
            let slice = unsafe { std::slice::from_raw_parts(text, len) };
            match std::str::from_utf8(slice) {
                Ok(s) => s,
                Err(_) => {
                    record_error(
                        c"agt_parent_console_write_stderr",
                        c"bad_text",
                        "text is not valid UTF-8",
                    );
                    return agt_status::AGT_FAILED;
                }
            }
        };
        if write_stderr(message) {
            agt_status::AGT_OK
        } else {
            agt_status::AGT_UNSUPPORTED
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(text, len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_parent_console_write_stderr",
                c"panic",
                "panic in agt_parent_console_write_stderr",
            );
            agt_status::AGT_FAILED
        }
    }
}

// --- runtime environment (milestone 10) -------------------------------

/// Validate the two-stage pointer contract: `out_len` must be non-NULL and
/// `buf` non-NULL whenever `cap > 0`. Returns `None` when valid, otherwise
/// records the error and returns the status to surface.
fn two_stage_pointer_error(
    operation: &'static CStr,
    buf: *const u8,
    cap: usize,
    out_len: *const usize,
) -> Option<agt_status> {
    if out_len.is_null() {
        record_error(operation, c"bad_pointer", "out_len pointer is null");
        return Some(agt_status::AGT_FAILED);
    }
    if cap > 0 && buf.is_null() {
        record_error(operation, c"bad_pointer", "buf is null");
        return Some(agt_status::AGT_FAILED);
    }
    None
}

/// Shared two-stage UTF-8 write (§3.4): copy `bytes` into `buf` when `cap`
/// is sufficient; otherwise report `AGT_FAILED{code="buffer_too_small"}` with
/// the required length in `*out_len`. `out_len == NULL` (or `buf == NULL`
/// with `cap > 0`) is `AGT_FAILED{code="bad_pointer"}`. On success `*out_len`
/// is the number of bytes written.
fn two_stage_utf8_write(
    operation: &'static CStr,
    bytes: &[u8],
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    if let Some(status) = two_stage_pointer_error(operation, buf, cap, out_len) {
        return status;
    }
    let required = bytes.len();
    if cap < required {
        unsafe { *out_len = required };
        record_error(
            operation,
            c"buffer_too_small",
            "cap is smaller than the required size; allocate the required size and call again",
        );
        return agt_status::AGT_FAILED;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, required);
        *out_len = required;
    }
    agt_status::AGT_OK
}

/// User config directory (UTF-8), two-stage (§3.4). A platform failure →
/// `AGT_FAILED{code="runtime_failed"}`; a path that is not valid UTF-8 →
/// `AGT_FAILED{code="bad_encoding"}` (never lossy-replaced — callers need
/// determinism).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_runtime_user_config_dir(
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(buf: *mut u8, cap: usize, out_len: *mut usize) -> agt_status {
        let path = match user_config_directory() {
            Ok(p) => p,
            Err(e) => {
                record_error(
                    c"agt_runtime_user_config_dir",
                    c"runtime_failed",
                    format!("{e}"),
                );
                return agt_status::AGT_FAILED;
            }
        };
        let bytes = match path.into_os_string().into_string() {
            Ok(s) => s.into_bytes(),
            Err(_) => {
                record_error(
                    c"agt_runtime_user_config_dir",
                    c"bad_encoding",
                    "user config directory is not valid UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        two_stage_utf8_write(c"agt_runtime_user_config_dir", &bytes, buf, cap, out_len)
    }
    match catch_unwind(AssertUnwindSafe(|| inner(buf, cap, out_len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_runtime_user_config_dir",
                c"panic",
                "panic in agt_runtime_user_config_dir",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Default terminal shell (UTF-8), two-stage (§3.4). Never fails on a built
/// library: the platform adapter always has a fallback shell, so an empty
/// result is impossible and the two-stage probe always yields a length > 0.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_runtime_default_shell(
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(buf: *mut u8, cap: usize, out_len: *mut usize) -> agt_status {
        let shell = default_terminal_shell();
        two_stage_utf8_write(
            c"agt_runtime_default_shell",
            shell.as_bytes(),
            buf,
            cap,
            out_len,
        )
    }
    match catch_unwind(AssertUnwindSafe(|| inner(buf, cap, out_len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_runtime_default_shell",
                c"panic",
                "panic in agt_runtime_default_shell",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// 1 when the process environment contains the ASCII variable `name`, 0
/// otherwise. `name == NULL` or a non-UTF-8 slice returns 0 — this is a
/// query, not a fallible operation, so it never sets the error record.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_runtime_env_present(name: *const u8, len: usize) -> i32 {
    fn inner(name: *const u8, len: usize) -> i32 {
        if name.is_null() || len == 0 {
            return 0;
        }
        // SAFETY: the pointer/length pair is a C ABI contract (see
        // include/agenterm.h); the caller guarantees `len` readable bytes.
        let slice = unsafe { std::slice::from_raw_parts(name, len) };
        match std::str::from_utf8(slice) {
            Ok(s) => ascii_environment_variable_present(s) as i32,
            Err(_) => 0,
        }
    }
    catch_unwind(AssertUnwindSafe(|| inner(name, len))).unwrap_or(0)
}

/// Number of command-line arguments (excluding the image name) of this
/// process. `out_count == NULL` → `AGT_FAILED{code="bad_pointer"}`; a
/// platform failure → `AGT_FAILED{code="runtime_failed"}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_runtime_arg_count(out_count: *mut usize) -> agt_status {
    fn inner(out_count: *mut usize) -> agt_status {
        if out_count.is_null() {
            record_error(
                c"agt_runtime_arg_count",
                c"bad_pointer",
                "out_count is null",
            );
            return agt_status::AGT_FAILED;
        }
        match application_arguments() {
            Ok(args) => {
                unsafe { *out_count = args.len() };
                agt_status::AGT_OK
            }
            Err(e) => {
                record_error(c"agt_runtime_arg_count", c"runtime_failed", format!("{e}"));
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(out_count))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_runtime_arg_count",
                c"panic",
                "panic in agt_runtime_arg_count",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Command-line argument `index` (UTF-8, excluding the image name),
/// two-stage (§3.4). `index` out of range →
/// `AGT_FAILED{code="bad_index"}`; a platform failure →
/// `AGT_FAILED{code="runtime_failed"}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_runtime_arg(
    index: usize,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> agt_status {
    fn inner(index: usize, buf: *mut u8, cap: usize, out_len: *mut usize) -> agt_status {
        // Pointer validity is checked before the index range (same ordering
        // as `agt_process_list` / `agt_clipboard_get_text`): an invalid
        // out pointer is `bad_pointer` even when the index is out of range.
        if let Some(status) = two_stage_pointer_error(c"agt_runtime_arg", buf, cap, out_len) {
            return status;
        }
        let args = match application_arguments() {
            Ok(a) => a,
            Err(e) => {
                record_error(c"agt_runtime_arg", c"runtime_failed", format!("{e}"));
                return agt_status::AGT_FAILED;
            }
        };
        let Some(arg) = args.get(index) else {
            record_error(c"agt_runtime_arg", c"bad_index", "index is out of range");
            return agt_status::AGT_FAILED;
        };
        two_stage_utf8_write(c"agt_runtime_arg", arg.as_bytes(), buf, cap, out_len)
    }
    match catch_unwind(AssertUnwindSafe(|| inner(index, buf, cap, out_len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(c"agt_runtime_arg", c"panic", "panic in agt_runtime_arg");
            agt_status::AGT_FAILED
        }
    }
}

// --- native window & input injection (milestone 43) -------------------

/// Native-window handle guard: 0 is never a legal native handle (the ABI's
/// own windows are a separate `agt_window_t` identity; native handles are
/// raw `isize` values from `agt_window_enumerate`).
fn native_handle_error(operation: &'static CStr, handle: isize) -> bool {
    if handle == 0 {
        record_error(operation, c"bad_handle", "native window handle is 0");
        true
    } else {
        false
    }
}

fn window_enumerate_available() -> bool {
    matches!(
        agenterm_platform::window_enumerate::capability_status(),
        CapabilityStatus::Available
    )
}

fn window_op_available() -> bool {
    matches!(
        agenterm_platform::window_op::capability_status(),
        CapabilityStatus::Available
    )
}

fn input_inject_available() -> bool {
    matches!(
        agenterm_platform::input_inject::capability_status(),
        CapabilityStatus::Available
    )
}

/// Truncate `s` at a UTF-8 character boundary and copy it into a fixed-size
/// inline array (the same two-stage semantics and the same `truncate_name`
/// helper as `agt_process_list` — one helper, no second copy). Returns
/// `(bytes, len, truncated)`.
fn string_to_fixed<const N: usize>(s: &str) -> ([u8; N], u32, u32) {
    let (kept, truncated) = truncate_name(s, N);
    let mut bytes = [0u8; N];
    bytes[..kept].copy_from_slice(&s.as_bytes()[..kept]);
    (bytes, kept as u32, u32::from(truncated))
}

/// C-compatible single native-window record mirroring `include/agenterm.h`.
/// `handle` is a raw native window handle valid only for the observation
/// instant; `title` / `app_name` are inline UTF-8 (not NUL-terminated by the
/// library — use the `*_len` fields) truncated at a UTF-8 character boundary
/// with the matching `*_truncated` flag set when the original exceeded the
/// fixed size.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub struct agt_window_info {
    pub handle: isize,
    pub process_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// 0/1.
    pub focused: i32,
    /// 0/1.
    pub minimized: i32,
    pub title: [u8; 128],
    /// Bytes actually written into `title` (<= 128).
    pub title_len: u32,
    /// 1 when the original title exceeded 128 bytes.
    pub title_truncated: u32,
    pub app_name: [u8; 64],
    /// Bytes actually written into `app_name` (<= 64).
    pub app_name_len: u32,
    /// 1 when the original app name exceeded 64 bytes.
    pub app_name_truncated: u32,
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

/// Translate one platform window record into the C-compatible record,
/// truncating `title` (128) and `app_name` (64) at UTF-8 character
/// boundaries with the shared `truncate_name` helper.
fn window_info_to_record(info: &WindowInfo) -> agt_window_info {
    let (title, title_len, title_truncated) = string_to_fixed::<128>(&info.title);
    let (app_name, app_name_len, app_name_truncated) = string_to_fixed::<64>(&info.app_name);
    agt_window_info {
        handle: info.handle,
        process_id: info.process_id,
        x: info.bounds.x,
        y: info.bounds.y,
        width: info.bounds.width,
        height: info.bounds.height,
        focused: i32::from(info.focused),
        minimized: i32::from(info.minimized),
        title,
        title_len,
        title_truncated,
        app_name,
        app_name_len,
        app_name_truncated,
    }
}

/// Enumerate visible top-level windows into a caller-allocated array
/// (two-stage, §3.4, identical semantics to `agt_process_list`):
/// - `cap` sufficient → `AGT_OK`, `*out_count` = records actually written.
/// - `cap` insufficient (including `cap == 0` with `buf == NULL`, the legal
///   "how big?" probe) → `AGT_FAILED { code = "buffer_too_small" }`,
///   `*out_count` = required count.
/// - NULL `out_count` (or NULL `buf` with `cap > 0`) →
///   `AGT_FAILED { code = "bad_pointer" }`.
/// - mechanism absent on this host → `AGT_UNSUPPORTED`.
/// - platform failure → `AGT_FAILED { code = "window_failed" }`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_window_enumerate(
    buf: *mut agt_window_info,
    cap: usize,
    out_count: *mut usize,
) -> agt_status {
    fn inner(buf: *mut agt_window_info, cap: usize, out_count: *mut usize) -> agt_status {
        if out_count.is_null() {
            record_error(c"agt_window_enumerate", c"bad_pointer", "out_count is null");
            return agt_status::AGT_FAILED;
        }
        if cap > 0 && buf.is_null() {
            record_error(c"agt_window_enumerate", c"bad_pointer", "buf is null");
            return agt_status::AGT_FAILED;
        }
        if !window_enumerate_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        let windows = match enumerate_top_level() {
            Ok(v) => v,
            Err(e) => {
                // `WindowEnumerateError` has no `Display` impl in
                // `agenterm-platform`; the facade convention is to forward
                // the message verbatim, so `Debug` stands in (no variant
                // matching, no type annotation — the crate is not modified).
                record_error(c"agt_window_enumerate", c"window_failed", format!("{e:?}"));
                return agt_status::AGT_FAILED;
            }
        };
        let required = windows.len();
        if cap < required {
            unsafe { *out_count = required };
            record_error(
                c"agt_window_enumerate",
                c"buffer_too_small",
                "cap is smaller than the window count; allocate the required count and call again",
            );
            return agt_status::AGT_FAILED;
        }
        unsafe { *out_count = required };
        for (i, w) in windows.iter().enumerate() {
            unsafe { *buf.add(i) = window_info_to_record(w) };
        }
        agt_status::AGT_OK
    }
    match catch_unwind(AssertUnwindSafe(|| inner(buf, cap, out_count))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_window_enumerate",
                c"panic",
                "panic in agt_window_enumerate",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// `state` codes for `agt_native_window_show`.
const AGT_NATIVE_WINDOW_HIDE: i32 = 0;
const AGT_NATIVE_WINDOW_SHOW: i32 = 1;
const AGT_NATIVE_WINDOW_MINIMIZE: i32 = 2;
const AGT_NATIVE_WINDOW_MAXIMIZE: i32 = 3;
const AGT_NATIVE_WINDOW_RESTORE: i32 = 4;

fn show_state_from_i32(state: i32) -> Option<WindowShowState> {
    match state {
        AGT_NATIVE_WINDOW_HIDE => Some(WindowShowState::Hide),
        AGT_NATIVE_WINDOW_SHOW => Some(WindowShowState::Show),
        AGT_NATIVE_WINDOW_MINIMIZE => Some(WindowShowState::Minimize),
        AGT_NATIVE_WINDOW_MAXIMIZE => Some(WindowShowState::Maximize),
        AGT_NATIVE_WINDOW_RESTORE => Some(WindowShowState::Restore),
        _ => None,
    }
}

/// Show/hide/minimize/maximize/restore a native window handle.
/// `handle == 0` → `AGT_FAILED{code="bad_handle"}`; an invalid `state` →
/// `AGT_FAILED{code="bad_state"}` (validated before any platform call, so an
/// invalid state never touches the window); mechanism absent →
/// `AGT_UNSUPPORTED`; platform failure →
/// `AGT_FAILED{code="window_op_failed"}`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_native_window_show(handle: isize, state: i32) -> agt_status {
    fn inner(handle: isize, state: i32) -> agt_status {
        if native_handle_error(c"agt_native_window_show", handle) {
            return agt_status::AGT_FAILED;
        }
        let Some(state) = show_state_from_i32(state) else {
            record_error(
                c"agt_native_window_show",
                c"bad_state",
                "state is not 0 (Hide)..=4 (Restore)",
            );
            return agt_status::AGT_FAILED;
        };
        if !window_op_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match show(handle, state) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_native_window_show",
                    c"window_op_failed",
                    format!("{e:?}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(handle, state))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_native_window_show",
                c"panic",
                "panic in agt_native_window_show",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Move/resize a native window handle. `handle == 0` → `bad_handle`;
/// mechanism absent → `AGT_UNSUPPORTED`; platform failure →
/// `AGT_FAILED{code="window_op_failed"}`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_native_window_move(
    handle: isize,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) -> agt_status {
    fn inner(handle: isize, x: i32, y: i32, w: u32, h: u32) -> agt_status {
        if native_handle_error(c"agt_native_window_move", handle) {
            return agt_status::AGT_FAILED;
        }
        if !window_op_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match move_window(handle, x, y, w, h) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_native_window_move",
                    c"window_op_failed",
                    format!("{e:?}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(handle, x, y, w, h))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_native_window_move",
                c"panic",
                "panic in agt_native_window_move",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Read a native window handle's rectangle (physical pixels, top-origin).
/// `handle == 0` → `bad_handle`; a NULL output pointer → `bad_pointer`;
/// mechanism absent → `AGT_UNSUPPORTED`; platform failure →
/// `AGT_FAILED{code="window_op_failed"}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_native_window_rect(
    handle: isize,
    x: *mut i32,
    y: *mut i32,
    w: *mut u32,
    h: *mut u32,
) -> agt_status {
    fn inner(handle: isize, x: *mut i32, y: *mut i32, w: *mut u32, h: *mut u32) -> agt_status {
        if native_handle_error(c"agt_native_window_rect", handle) {
            return agt_status::AGT_FAILED;
        }
        if x.is_null() || y.is_null() || w.is_null() || h.is_null() {
            record_error(
                c"agt_native_window_rect",
                c"bad_pointer",
                "one of the x/y/w/h out pointers is null",
            );
            return agt_status::AGT_FAILED;
        }
        if !window_op_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match window_rect(handle) {
            Ok(b) => {
                unsafe {
                    *x = b.x;
                    *y = b.y;
                    *w = b.width;
                    *h = b.height;
                }
                agt_status::AGT_OK
            }
            Err(e) => {
                record_error(
                    c"agt_native_window_rect",
                    c"window_op_failed",
                    format!("{e:?}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(handle, x, y, w, h))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_native_window_rect",
                c"panic",
                "panic in agt_native_window_rect",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Pin/unpin a native window handle above other windows. `topmost` is any
/// non-zero value for true, 0 for false. `handle == 0` → `bad_handle`;
/// mechanism absent → `AGT_UNSUPPORTED`; platform failure →
/// `AGT_FAILED{code="window_op_failed"}`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_native_window_set_topmost(handle: isize, topmost: i32) -> agt_status {
    fn inner(handle: isize, topmost: i32) -> agt_status {
        if native_handle_error(c"agt_native_window_set_topmost", handle) {
            return agt_status::AGT_FAILED;
        }
        if !window_op_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match set_topmost(handle, topmost != 0) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_native_window_set_topmost",
                    c"window_op_failed",
                    format!("{e:?}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(handle, topmost))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_native_window_set_topmost",
                c"panic",
                "panic in agt_native_window_set_topmost",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Close a **native** window handle. Deliberately distinct from
/// `agt_window_close`, which releases the ABI's *own* window handle
/// (`agt_window_open`'s `agt_window_t`); `agt_native_window_close` operates
/// on a raw OS handle from `agt_window_enumerate` — the two must never be
/// confused. `handle == 0` → `bad_handle`; mechanism absent →
/// `AGT_UNSUPPORTED`; platform failure →
/// `AGT_FAILED{code="window_op_failed"}`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_native_window_close(handle: isize) -> agt_status {
    fn inner(handle: isize) -> agt_status {
        if native_handle_error(c"agt_native_window_close", handle) {
            return agt_status::AGT_FAILED;
        }
        if !window_op_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match close(handle) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_native_window_close",
                    c"window_op_failed",
                    format!("{e:?}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(handle))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_native_window_close",
                c"panic",
                "panic in agt_native_window_close",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Move the pointer to absolute screen coordinates. Mechanism absent →
/// `AGT_UNSUPPORTED`; platform failure →
/// `AGT_FAILED{code="input_failed"}`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_input_pointer_move(x: i32, y: i32) -> agt_status {
    fn inner(x: i32, y: i32) -> agt_status {
        if !input_inject_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match pointer_move(PointerPosition { x, y }) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(c"agt_input_pointer_move", c"input_failed", format!("{e:?}"));
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(x, y))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_input_pointer_move",
                c"panic",
                "panic in agt_input_pointer_move",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// `button` codes for `agt_input_pointer_click`.
const AGT_INPUT_BUTTON_LEFT: i32 = 0;
const AGT_INPUT_BUTTON_RIGHT: i32 = 1;
const AGT_INPUT_BUTTON_MIDDLE: i32 = 2;

fn pointer_button_from_i32(button: i32) -> Option<InjectPointerButton> {
    match button {
        AGT_INPUT_BUTTON_LEFT => Some(InjectPointerButton::Left),
        AGT_INPUT_BUTTON_RIGHT => Some(InjectPointerButton::Right),
        AGT_INPUT_BUTTON_MIDDLE => Some(InjectPointerButton::Middle),
        _ => None,
    }
}

/// Click a pointer button at absolute screen coordinates. An invalid
/// `button` → `AGT_FAILED{code="bad_button"}` (validated before any platform
/// call, so an invalid button never clicks); mechanism absent →
/// `AGT_UNSUPPORTED`; platform failure →
/// `AGT_FAILED{code="input_failed"}`.
#[unsafe(no_mangle)]
pub extern "C" fn agt_input_pointer_click(x: i32, y: i32, button: i32, clicks: u32) -> agt_status {
    fn inner(x: i32, y: i32, button: i32, clicks: u32) -> agt_status {
        let Some(button) = pointer_button_from_i32(button) else {
            record_error(
                c"agt_input_pointer_click",
                c"bad_button",
                "button is not 0 (Left)..=2 (Middle)",
            );
            return agt_status::AGT_FAILED;
        };
        if !input_inject_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match pointer_click(PointerPosition { x, y }, button, clicks) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(
                    c"agt_input_pointer_click",
                    c"input_failed",
                    format!("{e:?}"),
                );
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(x, y, button, clicks))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_input_pointer_click",
                c"panic",
                "panic in agt_input_pointer_click",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Type UTF-8 text into the focused control via Unicode key events.
/// `text == NULL`, or a slice that is not valid UTF-8 →
/// `AGT_FAILED{code="bad_text"}`; mechanism absent → `AGT_UNSUPPORTED`;
/// platform failure → `AGT_FAILED{code="input_failed"}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_input_type_text(text: *const u8, len: usize) -> agt_status {
    fn inner(text: *const u8, len: usize) -> agt_status {
        if text.is_null() {
            record_error(c"agt_input_type_text", c"bad_text", "text pointer is null");
            return agt_status::AGT_FAILED;
        }
        // SAFETY: the pointer/length pair is a C ABI contract (see
        // include/agenterm.h); the caller guarantees `len` readable bytes.
        let slice = unsafe { std::slice::from_raw_parts(text, len) };
        let text = match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_input_type_text",
                    c"bad_text",
                    "text is not valid UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        if !input_inject_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match type_text(text) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(c"agt_input_type_text", c"input_failed", format!("{e:?}"));
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(text, len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_input_type_text",
                c"panic",
                "panic in agt_input_type_text",
            );
            agt_status::AGT_FAILED
        }
    }
}

/// Send a hotkey chord such as `ctrl+s`, `alt+f4` or `enter`. `shortcut ==
/// NULL`, or a slice that is not valid UTF-8 →
/// `AGT_FAILED{code="bad_text"}`; mechanism absent → `AGT_UNSUPPORTED`;
/// platform failure → `AGT_FAILED{code="input_failed"}`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn agt_input_send_keys(shortcut: *const u8, len: usize) -> agt_status {
    fn inner(shortcut: *const u8, len: usize) -> agt_status {
        if shortcut.is_null() {
            record_error(
                c"agt_input_send_keys",
                c"bad_text",
                "shortcut pointer is null",
            );
            return agt_status::AGT_FAILED;
        }
        // SAFETY: the pointer/length pair is a C ABI contract (see
        // include/agenterm.h); the caller guarantees `len` readable bytes.
        let slice = unsafe { std::slice::from_raw_parts(shortcut, len) };
        let shortcut = match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => {
                record_error(
                    c"agt_input_send_keys",
                    c"bad_text",
                    "shortcut is not valid UTF-8",
                );
                return agt_status::AGT_FAILED;
            }
        };
        if !input_inject_available() {
            return agt_status::AGT_UNSUPPORTED;
        }
        match send_keys(shortcut) {
            Ok(_) => agt_status::AGT_OK,
            Err(e) => {
                record_error(c"agt_input_send_keys", c"input_failed", format!("{e:?}"));
                agt_status::AGT_FAILED
            }
        }
    }
    match catch_unwind(AssertUnwindSafe(|| inner(shortcut, len))) {
        Ok(s) => s,
        Err(_) => {
            record_error(
                c"agt_input_send_keys",
                c"panic",
                "panic in agt_input_send_keys",
            );
            agt_status::AGT_FAILED
        }
    }
}

// --- milestone 3b unit tests (pure translation functions) ---------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Evidence 3: every one of the 27 `NamedKey` variants maps to its ABI
    /// code; the table is complete, unique, and never collides with the `_`
    /// fallback (255). The fallback arm itself is exercised by the
    /// `#[non_exhaustive]` requirement — any future platform variant hits it.
    #[test]
    fn named_key_codes_cover_all_27_variants() {
        let cases = [
            (NamedKey::ArrowDown, 1u8),
            (NamedKey::ArrowLeft, 2),
            (NamedKey::ArrowRight, 3),
            (NamedKey::ArrowUp, 4),
            (NamedKey::Backspace, 5),
            (NamedKey::Delete, 6),
            (NamedKey::End, 7),
            (NamedKey::Enter, 8),
            (NamedKey::Escape, 9),
            (NamedKey::F1, 10),
            (NamedKey::F2, 11),
            (NamedKey::F3, 12),
            (NamedKey::F4, 13),
            (NamedKey::F5, 14),
            (NamedKey::F6, 15),
            (NamedKey::F7, 16),
            (NamedKey::F8, 17),
            (NamedKey::F9, 18),
            (NamedKey::F10, 19),
            (NamedKey::F11, 20),
            (NamedKey::F12, 21),
            (NamedKey::Home, 22),
            (NamedKey::Insert, 23),
            (NamedKey::PageDown, 24),
            (NamedKey::PageUp, 25),
            (NamedKey::Space, 26),
            (NamedKey::Tab, 27),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for (key, expected) in cases {
            let code = named_key_code(key);
            assert_eq!(code, expected, "unexpected code {code}");
            assert!(seen.insert(code), "duplicate code {code}");
        }
        assert_eq!(seen.len(), 27, "table must cover exactly 27 variants");
        assert_eq!(*seen.first().unwrap(), 1, "codes must start at 1");
        assert_eq!(*seen.last().unwrap(), 27, "codes must end at 27");
        // The `_` fallback is reserved for future platform variants.
        assert_eq!(AGT_KEY_NAMED_UNKNOWN, 255);
        assert!(
            !seen.contains(&AGT_KEY_NAMED_UNKNOWN),
            "no known variant may map to the unknown fallback"
        );
    }

    #[test]
    fn physical_key_codes_match_the_table() {
        assert_eq!(
            physical_key_code(PhysicalKeyCode::Letter('A')),
            (1, 'A' as u32)
        );
        assert_eq!(
            physical_key_code(PhysicalKeyCode::Letter('中')),
            (1, '中' as u32)
        );
        assert_eq!(physical_key_code(PhysicalKeyCode::Digit(7)), (2, 7));
        assert_eq!(physical_key_code(PhysicalKeyCode::Backspace), (3, 0));
        assert_eq!(physical_key_code(PhysicalKeyCode::Enter), (4, 0));
        assert_eq!(physical_key_code(PhysicalKeyCode::Space), (5, 0));
        assert_eq!(physical_key_code(PhysicalKeyCode::Tab), (6, 0));
        assert_eq!(physical_key_code(PhysicalKeyCode::Other), (0, 0));
    }

    #[test]
    fn modifier_bits_map_each_modifier() {
        let control_alt = ModifierState {
            control: true,
            shift: false,
            alt: true,
            meta: false,
        };
        assert_eq!(modifier_bits(control_alt), AGT_MOD_CONTROL | AGT_MOD_ALT);
        let all = ModifierState {
            control: true,
            shift: true,
            alt: true,
            meta: true,
        };
        assert_eq!(
            modifier_bits(all),
            AGT_MOD_CONTROL | AGT_MOD_SHIFT | AGT_MOD_ALT | AGT_MOD_META
        );
        let none = ModifierState {
            control: false,
            shift: false,
            alt: false,
            meta: false,
        };
        assert_eq!(modifier_bits(none), 0);
    }

    #[test]
    fn pointer_button_codes_match_the_table() {
        assert_eq!(pointer_button_code(PointerButton::Left), 1);
        assert_eq!(pointer_button_code(PointerButton::Right), 2);
        assert_eq!(pointer_button_code(PointerButton::Middle), 3);
        assert_eq!(pointer_button_code(PointerButton::Other(9)), 4);
    }

    #[test]
    fn wheel_deltas_carry_unit_and_value() {
        assert_eq!(
            wheel_delta(WheelDelta::Lines { x: 1.0, y: -2.0 }),
            (1.0, -2.0, 0)
        );
        assert_eq!(
            wheel_delta(WheelDelta::LogicalPixels { x: 3.5, y: 4.5 }),
            (3.5, 4.5, 1)
        );
    }

    fn no_mods() -> ModifierState {
        ModifierState {
            control: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    #[test]
    fn key_event_character_text_and_named_codes() {
        let rec = key_event_to_record(
            NormalizedKeyEvent {
                logical: LogicalKey::Character("你".into()),
                physical: PhysicalKeyCode::Letter('你'),
                text: Some("你".into()),
                state: KeyPressState::Pressed,
                repeat: false,
                modifiers: ModifierState {
                    control: true,
                    shift: false,
                    alt: false,
                    meta: false,
                },
            },
            7,
        );
        assert_eq!(rec.ev.kind, AGT_EV_KEY);
        assert_eq!(rec.ev.generation, 7);
        assert_eq!(rec.ev.key_state, 1);
        assert_eq!(rec.ev.key_repeat, 0);
        assert_eq!(rec.ev.key_named, AGT_KEY_NAMED_OTHER);
        assert_eq!(rec.ev.key_physical, 1);
        assert_eq!(rec.ev.key_physical_value, '你' as u32);
        assert_eq!(rec.ev.modifiers, AGT_MOD_CONTROL);
        // "你" is exactly 3 UTF-8 bytes.
        assert_eq!(rec.ev.text_len, 3);
        assert_eq!(&rec.ev.text[..3], "你".as_bytes());
        assert_eq!(rec.ev.text_truncated, 0);
        assert!(rec.event_text.is_none(), "KEY text rides in the POD");
    }

    #[test]
    fn key_text_is_truncated_at_16_bytes() {
        let long = "a".repeat(32);
        let rec = key_event_to_record(
            NormalizedKeyEvent {
                logical: LogicalKey::Character(long.clone()),
                physical: PhysicalKeyCode::Other,
                text: Some(long),
                state: KeyPressState::Released,
                repeat: true,
                modifiers: no_mods(),
            },
            0,
        );
        assert_eq!(rec.ev.text_len, 16);
        assert_eq!(rec.ev.text_truncated, 1);
        assert_eq!(rec.ev.key_state, 0);
        assert_eq!(rec.ev.key_repeat, 1);
    }

    #[test]
    fn key_event_named_keys_report_code_and_no_text() {
        let rec = key_event_to_record(
            NormalizedKeyEvent {
                logical: LogicalKey::Named(NamedKey::ArrowUp),
                physical: PhysicalKeyCode::Other,
                text: None,
                state: KeyPressState::Pressed,
                repeat: false,
                modifiers: no_mods(),
            },
            0,
        );
        assert_eq!(rec.ev.key_named, 4);
        assert_eq!(rec.ev.text_len, 0);
        assert_eq!(rec.ev.key_physical, 0);
    }

    #[test]
    fn pointer_events_map_state_button_and_position() {
        let moved = pointer_moved_to_record(LogicalPoint { x: 10.0, y: 20.0 }, no_mods(), 1);
        assert_eq!(moved.ev.kind, AGT_EV_POINTER);
        assert_eq!(moved.ev.pointer_state, 2);
        assert_eq!(moved.ev.pointer_button, 0);
        assert_eq!(moved.ev.has_position, 1);
        assert_eq!(moved.ev.pointer_x, 10.0);
        assert_eq!(moved.ev.pointer_y, 20.0);

        let left = pointer_exit_to_record(false, 2);
        assert_eq!(left.ev.pointer_state, 3);
        assert_eq!(left.ev.has_position, 0);

        let lost = pointer_exit_to_record(true, 3);
        assert_eq!(lost.ev.pointer_state, 4);

        let pressed = pointer_button_to_record(
            PointerButton::Right,
            PointerButtonState::Pressed,
            Some(LogicalPoint { x: 1.0, y: 2.0 }),
            ModifierState {
                control: false,
                shift: true,
                alt: false,
                meta: false,
            },
            4,
        );
        assert_eq!(pressed.ev.pointer_button, 2);
        assert_eq!(pressed.ev.pointer_state, 1);
        assert_eq!(pressed.ev.has_position, 1);
        assert_eq!(pressed.ev.modifiers, AGT_MOD_SHIFT);

        let released_none = pointer_button_to_record(
            PointerButton::Middle,
            PointerButtonState::Released,
            None,
            no_mods(),
            5,
        );
        assert_eq!(released_none.ev.pointer_button, 3);
        assert_eq!(released_none.ev.pointer_state, 0);
        assert_eq!(released_none.ev.has_position, 0);
    }

    #[test]
    fn wheel_events_map_delta_and_position() {
        let rec = wheel_to_record(
            WheelDelta::Lines { x: 0.0, y: 3.0 },
            Some(LogicalPoint { x: 5.0, y: 6.0 }),
            no_mods(),
            6,
        );
        assert_eq!(rec.ev.kind, AGT_EV_WHEEL);
        assert_eq!(rec.ev.wheel_x, 0.0);
        assert_eq!(rec.ev.wheel_y, 3.0);
        assert_eq!(rec.ev.wheel_unit, 0);
        assert_eq!(rec.ev.has_position, 1);
        assert_eq!(rec.ev.pointer_x, 5.0);
        assert_eq!(rec.ev.pointer_y, 6.0);

        let px = wheel_to_record(
            WheelDelta::LogicalPixels { x: 1.5, y: -2.5 },
            None,
            no_mods(),
            7,
        );
        assert_eq!(px.ev.wheel_unit, 1);
        assert_eq!(px.ev.has_position, 0);
    }

    #[test]
    fn ime_events_carry_kind_cursor_and_out_of_band_text() {
        let preedit = ime_event_to_record(
            ImeEvent::Preedit {
                text: "你好".into(),
                cursor: Some((1, 3)),
            },
            8,
        );
        assert_eq!(preedit.ev.kind, AGT_EV_IME);
        assert_eq!(preedit.ev.ime_kind, 1);
        assert_eq!(preedit.ev.has_ime_cursor, 1);
        assert_eq!(preedit.ev.ime_cursor_begin, 1);
        assert_eq!(preedit.ev.ime_cursor_end, 3);
        assert_eq!(preedit.ev.ime_text_len, "你好".len());
        assert_eq!(preedit.event_text.as_deref(), Some("你好".as_bytes()));

        let commit = ime_event_to_record(ImeEvent::Commit("commit".into()), 9);
        assert_eq!(commit.ev.ime_kind, 2);
        assert_eq!(commit.ev.has_ime_cursor, 0);
        assert_eq!(commit.ev.ime_text_len, 6);
        assert_eq!(commit.event_text.as_deref(), Some(b"commit".as_slice()));

        let enabled = ime_event_to_record(ImeEvent::Enabled, 10);
        assert_eq!(enabled.ev.ime_kind, 0);
        assert!(enabled.event_text.is_none());

        let disabled = ime_event_to_record(ImeEvent::Disabled, 11);
        assert_eq!(disabled.ev.ime_kind, 3);
        assert!(disabled.event_text.is_none());
    }

    #[test]
    fn ime_preedit_without_cursor_reports_no_cursor() {
        let rec = ime_event_to_record(
            ImeEvent::Preedit {
                text: "x".into(),
                cursor: None,
            },
            12,
        );
        assert_eq!(rec.ev.ime_kind, 1);
        assert_eq!(rec.ev.has_ime_cursor, 0);
        assert_eq!(rec.ev.ime_cursor_begin, 0);
        assert_eq!(rec.event_text.as_deref(), Some(b"x".as_slice()));
    }

    #[test]
    fn process_name_truncation_respects_utf8_boundaries() {
        // Short ASCII: unchanged, not truncated.
        let (n, t) = truncate_name("agenterm.exe", 64);
        assert_eq!(n, 12);
        assert!(!t);
        // Exactly 64 ASCII bytes: no truncation.
        let exact = "a".repeat(64);
        let (n, t) = truncate_name(&exact, 64);
        assert_eq!(n, 64);
        assert!(!t);
        // 65 ASCII bytes: truncated to 64, flagged.
        let long = "a".repeat(65);
        let (n, t) = truncate_name(&long, 64);
        assert_eq!(n, 64);
        assert!(t);
        // A 3-byte char crossing the 64-byte cut must be dropped whole:
        // 62 ASCII + "€" = 65 bytes; the cut at 64 splits "€".
        let s = format!("{}€", "a".repeat(62));
        let (n, t) = truncate_name(&s, 64);
        assert_eq!(n, 62);
        assert!(t);
        assert_eq!(&s[..n], "a".repeat(62));
        // A 4-byte emoji likewise never splits: 63 ASCII + "😀" = 67 bytes.
        let s2 = format!("{}😀", "a".repeat(63));
        let (n, t) = truncate_name(&s2, 64);
        assert_eq!(n, 63);
        assert!(t);
        // The record translation wires the flag through.
        let rec = process_info_to_record(7, 1, &s2);
        assert_eq!(rec.id, 7);
        assert_eq!(rec.parent_id, 1);
        assert_eq!(rec.name_len, 63);
        assert_eq!(rec.name_truncated, 1);
        assert_eq!(&rec.name[..63], &s2.as_bytes()[..63]);
        // Un-truncated names keep their full bytes and flag 0.
        let rec = process_info_to_record(8, 2, "agenterm.exe");
        assert_eq!(rec.name_len, 12);
        assert_eq!(rec.name_truncated, 0);
        assert_eq!(&rec.name[..12], b"agenterm.exe");
    }
}
