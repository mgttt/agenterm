//! Milestone 72: SUCCESS-path evidence for the four `agt_input_*` exports
//! (`agt_input_pointer_move` / `agt_input_pointer_click` /
//! `agt_input_type_text` / `agt_input_send_keys`) against a real native
//! window owned by a CHILD process we spawn — the receiver the exports
//! inject into.
//!
//! Background: the four exports previously had only failure-path evidence
//! (`null_sweep.rs` swept NULL pointers, `dylib_load.rs` exercised an
//! invalid button 99 and NULL), while `crates/agenterm-cu` has 9 call sites
//! that rely on them. This test is the first place that asserts they
//! actually DO something.
//!
//! Safety contract (the repo's standing rule, unchanged — input injection is
//! GLOBAL and must never run on someone's live desktop by default):
//!   - the FIRST check is `AGENTERM_ALLOW_INPUT_INJECTION == "1"`; anything
//!     else prints the SKIP line and passes. Only the windows CI job sets
//!     the variable (`.github/workflows/ci-libagenterm.yml`).
//!   - injection happens ONLY into this test's own child window: the child
//!     (`examples/c/agenterm_input_target_window.c`) embeds its pid in the
//!     title, and the pid is re-verified by re-enumerating BEFORE every
//!     native call — never filtered once;
//!   - BEFORE EVERY injection the foreground window is verified to BE the
//!     child window; a foreign foreground is a hard failure (red), never a
//!     skip — injecting into someone else's window is an incident;
//!   - "environment can't do it" is separated from "mechanism is broken":
//!     when the child window cannot be foregrounded because there is NO
//!     interactive foreground at all (`GetForegroundWindow() == NULL`, e.g.
//!     a headless/disconnected runner), the test prints a specific SKIP and
//!     passes;
//!   - the cursor is restored after the run, on the failure paths too (RAII
//!     guard, same pattern as `native_window_ops.rs`'s ChildGuard);
//!   - assertions align with the CONTRACT, not this machine's incidental
//!     behavior: no exact pixel coords (DPI/decoration), no exact event
//!     counts (the OS may synthesize extra `WM_MOUSEMOVE`), no timing
//!     asserts — bounded polling over the child's stdout lines.
//!
//! Windows-only this round (milestone 72): Linux/macOS CI runners are
//! headless and the whole injection stack there is another round; the
//! non-Windows branch prints a reasoned SKIP and passes.

mod common;

#[cfg(windows)]
mod inject {
    use super::common::capabilities::{
        AGT_CAP_INPUT_INJECT, AGT_CAP_WINDOW_ENUMERATE, AGT_CAP_WINDOW_OP,
    };
    use super::common::toolchain;
    use libloading::{Library, Symbol};
    use std::ffi::{CStr, c_char};
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    const AGT_OK: i32 = 0;
    const AGT_UNSUPPORTED: i32 = 1;
    const AGT_FAILED: i32 = 2;
    /// The child must print its `ready <hwnd>` line within this bound.
    const READY_TIMEOUT: Duration = Duration::from_secs(10);
    /// The child must exit within this bound after `agt_native_window_close`.
    const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
    /// Poll interval while waiting for the child (exit, foreground, events).
    const POLL: Duration = Duration::from_millis(50);
    /// How long we try to make the child window the foreground window.
    const FOREGROUND_TIMEOUT: Duration = Duration::from_secs(3);
    /// How long an injected event may take to show up in the child's stdout.
    const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

    // --- Win32 FFI (user32; the test crate has no windows-sys dependency,
    // so the five functions the test needs are declared directly). -------

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut c_char;
        fn SetForegroundWindow(hwnd: *mut c_char) -> i32;
        fn GetCursorPos(lp: *mut Point) -> i32;
        fn SetCursorPos(x: i32, y: i32) -> i32;
        fn GetClientRect(hwnd: *mut c_char, rect: *mut Rect) -> i32;
        fn ClientToScreen(hwnd: *mut c_char, pt: *mut Point) -> i32;
        fn GetWindowThreadProcessId(hwnd: *mut c_char, pid: *mut u32) -> u32;
    }

    // --- C ABI mirrors (layout must match include/agenterm.h) -------------

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
    type WindowEnumerate = unsafe extern "C" fn(*mut agt_window_info, usize, *mut usize) -> i32;
    type NativeWindowRect =
        unsafe extern "C" fn(isize, *mut i32, *mut i32, *mut u32, *mut u32) -> i32;
    type NativeWindowClose = unsafe extern "C" fn(isize) -> i32;
    type InputPointerMove = unsafe extern "C" fn(i32, i32) -> i32;
    type InputPointerClick = unsafe extern "C" fn(i32, i32, i32, u32) -> i32;
    type InputTypeText = unsafe extern "C" fn(*const u8, usize) -> i32;
    type InputSendKeys = unsafe extern "C" fn(*const u8, usize) -> i32;
    type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;

    /// Load the cdylib and leak the `Library` handle (same convention as
    /// `native_window_ops.rs` / `dylib_load.rs`).
    fn load() -> &'static Library {
        let path = toolchain::locate_cdylib();
        let lib = unsafe { Library::new(&path) }
            .unwrap_or_else(|e| panic!("LoadLibrary({path:?}) failed: {e}"));
        Box::leak(Box::new(lib))
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

    #[test]
    fn input_injection_success_path() {
        // ---- opt-in gate: NEVER inject by default -----------------------
        let allow = std::env::var("AGENTERM_ALLOW_INPUT_INJECTION").unwrap_or_default();
        if allow != "1" {
            eprintln!("SKIP: input injection is opt-in; set AGENTERM_ALLOW_INPUT_INJECTION=1");
            return;
        }
        eprintln!("INPUT-INJECTION: REAL RUN (AGENTERM_ALLOW_INPUT_INJECTION=1)");

        let lib = load();
        let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };

        // ---- capability guard -------------------------------------------
        for cap in [
            AGT_CAP_INPUT_INJECT,
            AGT_CAP_WINDOW_ENUMERATE,
            AGT_CAP_WINDOW_OP,
        ] {
            let st = unsafe { query(cap) };
            if st == AGT_UNSUPPORTED {
                eprintln!(
                    "SKIP: capability {cap} reports AGT_UNSUPPORTED on this host \
                     (headless session?); input injection cannot run"
                );
                return;
            }
            assert_eq!(
                st, AGT_OK,
                "capability {cap} must answer AGT_OK or AGT_UNSUPPORTED, got {st}"
            );
        }
        eprintln!(
            "capabilities AGT_CAP_INPUT_INJECT + AGT_CAP_WINDOW_ENUMERATE + AGT_CAP_WINDOW_OP -> AGT_OK"
        );

        // ---- build the input-target probe -------------------------------
        let Some(compiler) = toolchain::find_c_compiler("input_inject_success") else {
            eprintln!(
                "SKIP: no C compiler matching target_env={} was found (see the \
                 target_env= decision line above) — cannot build the input-target probe",
                toolchain::target_env_name()
            );
            return;
        };

        let root = toolchain::repo_root();
        let include = root.join("include");
        let c_file = root.join("examples/c/agenterm_input_target_window.c");
        assert!(
            c_file.is_file(),
            "missing {} (expected next to this test)",
            c_file.display()
        );

        let seq = toolchain::DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let scratch = std::env::temp_dir().join(format!(
            "agenterm-input-inject-{}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&scratch).expect("create scratch dir");
        let _cleanup = toolchain::Cleanup(scratch.clone());

        let exe_name = "agenterm_input_target_window.exe";
        let exe = scratch.join(exe_name);

        let is_msvc = compiler
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("cl.exe") || s.eq_ignore_ascii_case("cl"))
            .unwrap_or(false);
        let mut cc = Command::new(&compiler.path);
        for (k, v) in &compiler.env {
            cc.env(k, v);
        }
        if is_msvc {
            cc.current_dir(&scratch);
            cc.arg("/nologo").arg("/W4").arg("/WX");
            cc.arg(format!("/I{}", include.display()));
            cc.arg(&c_file);
            cc.arg("/Foagenterm_input_target_window.obj");
            cc.arg(format!("/Fe{exe_name}"));
        } else {
            cc.arg("-Wall").arg("-Wextra").arg("-Werror");
            cc.arg("-I").arg(&include);
            cc.arg(&c_file);
            cc.arg("-o").arg(&exe);
        }
        toolchain::run_or_panic("input-target probe compile/link", &mut cc);

        // ---- spawn the child; first stdout line is the `ready` handshake,
        // every later line is an injected-input report --------------------
        let mut child = Command::new(&exe)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", exe.display()));
        let child_pid = child.id();
        let stdout = child.stdout.take().expect("child stdout is piped");
        let mut guard = ChildGuard {
            child: Some(child),
            label: format!("agenterm_input_target_window(pid={child_pid})"),
        };

        let log = Arc::new(EventLog::default());
        let (tx_ready, rx_ready) = mpsc::channel();
        {
            let log = log.clone();
            std::thread::spawn(move || {
                let mut lines = BufReader::new(stdout).lines();
                if let Some(first) = lines.next() {
                    let first = first.unwrap_or_default();
                    eprintln!("child> {first}");
                    let _ = tx_ready.send(first);
                }
                for line in lines {
                    let line = line.unwrap_or_default();
                    eprintln!("child> {line}");
                    log.push(line);
                }
            });
        }
        let ready = rx_ready.recv_timeout(READY_TIMEOUT).unwrap_or_else(|_| {
            panic!(
                "timed out after {READY_TIMEOUT:?} waiting for the child's `ready` \
                     line (pid {child_pid}); the child was killed by the guard"
            )
        });
        let reported_hwnd: isize = ready
            .strip_prefix("ready ")
            .unwrap_or_else(|| {
                panic!(
                    "child {child_pid} first stdout line was {ready:?}, \
                     expected `ready <hwnd>`"
                )
            })
            .trim()
            .parse()
            .unwrap_or_else(|_| {
                panic!("child {child_pid} first stdout line {ready:?} has an unparseable hwnd")
            });
        eprintln!("child {child_pid} reported hwnd={reported_hwnd}");

        // ---- symbols ------------------------------------------------------
        let list: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
        let rect_fn: Symbol<NativeWindowRect> = unsafe { sym(lib, b"agt_native_window_rect") };
        let close_fn: Symbol<NativeWindowClose> = unsafe { sym(lib, b"agt_native_window_close") };
        let pmove: Symbol<InputPointerMove> = unsafe { sym(lib, b"agt_input_pointer_move") };
        let pclick: Symbol<InputPointerClick> = unsafe { sym(lib, b"agt_input_pointer_click") };
        let ptype: Symbol<InputTypeText> = unsafe { sym(lib, b"agt_input_type_text") };
        let pkeys: Symbol<InputSendKeys> = unsafe { sym(lib, b"agt_input_send_keys") };

        // ---- locate the child's window (title + pid, both re-checked) ----
        let expected_title = format!("agenterm-input-target-{child_pid}").into_bytes();
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "locate");
        let handle = rec.handle;
        assert!(handle != 0, "enumerated handle must be non-zero");
        assert_eq!(
            handle, reported_hwnd,
            "enumerated handle must equal the child-reported hwnd"
        );
        assert!(
            rec.width > 0 && rec.height > 0,
            "initial rect must be non-zero, got {}x{}",
            rec.width,
            rec.height
        );
        let (win_x, win_y, win_w, win_h) = (rec.x, rec.y, rec.width as i32, rec.height as i32);

        // ---- cursor restore guard (installed before anything moves it) ---
        let mut cursor = CursorRestore::capture();

        // ---- window rect + client-area center (the injection point) ------
        let mut x = 0i32;
        let mut y = 0i32;
        let mut w = 0u32;
        let mut h = 0u32;
        let st = unsafe { rect_fn(handle, &mut x, &mut y, &mut w, &mut h) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_rect failed: {}",
            last_error_message(lib)
        );
        assert!(w > 0 && h > 0, "rect must be non-zero, got {w}x{h}");
        eprintln!("target window hwnd={handle:#x} pid={child_pid} rect=({x},{y}) {w}x{h}");

        let mut client = Rect::default();
        assert!(
            unsafe { GetClientRect(handle as *mut c_char, &mut client) } != 0,
            "GetClientRect failed"
        );
        let (cw, ch) = (client.right, client.bottom);
        assert!(
            cw > 0 && ch > 0,
            "client rect must be non-zero, got {cw}x{ch}"
        );
        let mut center = Point {
            x: cw / 2,
            y: ch / 2,
        };
        assert!(
            unsafe { ClientToScreen(handle as *mut c_char, &mut center) } != 0,
            "ClientToScreen failed"
        );
        eprintln!(
            "client rect {cw}x{ch}; injection point (client center) = ({},{})",
            center.x, center.y
        );

        // ---- agt_input_pointer_move --------------------------------------
        if !ensure_child_foreground_or_skip(handle, child_pid, "before pointer_move") {
            return;
        }
        let before = log.len();
        let st = unsafe { pmove(center.x, center.y) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_input_pointer_move failed: {}",
            last_error_message(lib)
        );
        let mut cp = Point::default();
        assert!(unsafe { GetCursorPos(&mut cp) } != 0, "GetCursorPos failed");
        assert!(
            cp.x >= win_x && cp.x < win_x + win_w && cp.y >= win_y && cp.y < win_y + win_h,
            "cursor must land inside the child window rect ({win_x},{win_y}) {win_w}x{win_h} \
             (DPI/decoration tolerance: only the rectangle is asserted), got ({},{})",
            cp.x,
            cp.y
        );
        eprintln!(
            "agt_input_pointer_move({},{}) -> AGT_OK; cursor now at ({},{}) inside rect",
            center.x, center.y, cp.x, cp.y
        );
        wait_for_event_since(
            &log,
            before,
            EVENT_TIMEOUT,
            "mousemove after pointer_move",
            |snap| snap.iter().any(|l| l.starts_with("mousemove ")),
        );

        // ---- agt_input_pointer_click (Left, 1 click) ----------------------
        if !ensure_child_foreground_or_skip(handle, child_pid, "before pointer_click") {
            return;
        }
        let before = log.len();
        let st = unsafe { pclick(center.x, center.y, 0, 1) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_input_pointer_click failed: {}",
            last_error_message(lib)
        );
        eprintln!(
            "agt_input_pointer_click({},{}, Left, 1) -> AGT_OK",
            center.x, center.y
        );
        // The child must report a WM_LBUTTONDOWN whose client-area coords are
        // inside the client rect. The OS may synthesize extra WM_MOUSEMOVE
        // lines, so only the lbuttondown line is asserted here.
        wait_for_event_since(
            &log,
            before,
            EVENT_TIMEOUT,
            "lbuttondown after pointer_click",
            |snap| {
                snap.iter().any(|l| {
                    let Some(rest) = l.strip_prefix("lbuttondown ") else {
                        return false;
                    };
                    let mut parts = rest.split_whitespace();
                    let ox: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
                    let oy: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
                    ox >= 0 && ox < cw && oy >= 0 && oy < ch
                })
            },
        );

        // ---- agt_input_type_text: ASCII, side-effect-free ---------------
        // "agenterm" is deliberately not enter/tab/alt/ctrl — nothing that
        // could trigger a system shortcut.
        if !ensure_child_foreground_or_skip(handle, child_pid, "before type_text") {
            return;
        }
        let before = log.len();
        let text = b"agenterm";
        let st = unsafe { ptype(text.as_ptr(), text.len()) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_input_type_text failed: {}",
            last_error_message(lib)
        );
        eprintln!(
            "agt_input_type_text(\"{}\", {}) -> AGT_OK",
            String::from_utf8_lossy(text),
            text.len()
        );
        // The child reports one `char <codepoint>` per WM_CHAR; the sequence
        // concatenated must equal the string that was sent.
        wait_for_event_since(
            &log,
            before,
            EVENT_TIMEOUT,
            "WM_CHAR sequence after type_text",
            |snap| {
                let typed: String = snap
                    .iter()
                    .filter_map(|l| l.strip_prefix("char "))
                    .filter_map(|cp| cp.trim().parse::<u32>().ok())
                    .filter_map(char::from_u32)
                    .filter(|c| c.is_ascii())
                    .collect();
                typed == "agenterm"
            },
        );

        // ---- agt_input_send_keys: "a" -> VK_A (0x41 = 65) ----------------
        // A plain letter key has no system-wide side effects; the child must
        // report a WM_KEYDOWN with vk == 65.
        if !ensure_child_foreground_or_skip(handle, child_pid, "before send_keys") {
            return;
        }
        let before = log.len();
        let keys = b"a";
        let st = unsafe { pkeys(keys.as_ptr(), keys.len()) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_input_send_keys failed: {}",
            last_error_message(lib)
        );
        eprintln!("agt_input_send_keys(\"a\", 1) -> AGT_OK");
        wait_for_event_since(
            &log,
            before,
            EVENT_TIMEOUT,
            "keydown 65 after send_keys(\"a\")",
            |snap| snap.iter().any(|l| l == "keydown 65"),
        );

        // ---- close: the intended terminal state --------------------------
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "pre-close");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
        let st = unsafe { close_fn(handle) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_close failed: {}",
            last_error_message(lib)
        );
        eprintln!("agt_native_window_close -> AGT_OK");

        // ---- the child must exit 0 once its own window is closed ---------
        let status = wait_for_exit(
            guard.child.as_mut().expect("child present"),
            "after agt_native_window_close",
        );
        let code = status.code();
        eprintln!("child {child_pid} exit code = {code:?} (full status {status:?})");
        assert_eq!(code, Some(0), "child must exit 0, got {status:?}");
        guard.finish();
        cursor.restore();
        eprintln!(
            "input_inject_success: all four agt_input_* success paths verified against \
             the child-owned window hwnd={handle:#x}"
        );
    }

    /// Verify (and, when needed, (re)establish) that the FOREGROUND window
    /// is the child's window. Called BEFORE EVERY injection.
    ///
    /// Returns `true` when the child owns the foreground (injection may
    /// proceed). Returns `false` after printing a SKIP when there is NO
    /// foreground window at all (headless / disconnected runner — an
    /// environment limitation, the caller must return). Panics when the
    /// foreground belongs to a foreign window — injecting into someone
    /// else's window is an incident and must ring red, never skip.
    fn ensure_child_foreground_or_skip(hwnd: isize, child_pid: u32, step: &str) -> bool {
        let target = hwnd as *mut c_char;
        let deadline = Instant::now() + FOREGROUND_TIMEOUT;
        loop {
            let fg = unsafe { GetForegroundWindow() };
            if fg == target {
                eprintln!("{step}: foreground confirmed = child window hwnd={hwnd:#x}");
                return true;
            }
            unsafe { SetForegroundWindow(target) };
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(POLL);
        }
        let fg = unsafe { GetForegroundWindow() };
        if fg.is_null() {
            eprintln!(
                "SKIP: cannot foreground the child window (GetForegroundWindow() still \
                 returned NULL after {FOREGROUND_TIMEOUT:?} — no interactive foreground \
                 on this session/desktop, e.g. a headless or disconnected runner; child \
                 pid {child_pid} is reaped by the guard)"
            );
            false
        } else {
            let mut fg_pid = 0u32;
            unsafe { GetWindowThreadProcessId(fg, &mut fg_pid) };
            panic!(
                "{step}: foreground window is {fg:?} (pid {fg_pid}, child pid {child_pid}) \
                 — not the child window; refusing to inject into someone else's window"
            );
        }
    }

    /// RAII guard that guarantees the child is killed on EVERY path (panic
    /// unwinding included) and its exit status is reaped. Same as
    /// `native_window_ops.rs`.
    struct ChildGuard {
        child: Option<Child>,
        label: String,
    }

    impl ChildGuard {
        /// Success path: reap the (already exited) child and return its
        /// final status. After this the guard has nothing left to kill.
        fn finish(mut self) -> ExitStatus {
            let mut child = self.child.take().expect("child already finished");
            child.wait().expect("wait for child exit")
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let Some(child) = self.child.as_mut() else {
                return;
            };
            match child.try_wait() {
                Ok(Some(_)) => {
                    let _ = child.wait();
                }
                Ok(None) => {
                    eprintln!("{}: still running at drop -> killing", self.label);
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(e) => {
                    eprintln!("{}: try_wait failed ({e}) -> killing", self.label);
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }

    /// Restore the cursor to its pre-injection position on EVERY path:
    /// explicit `restore()` on success, `Drop` on failure. Same guard
    /// pattern as `ChildGuard` / `Cleanup`.
    struct CursorRestore {
        pos: Point,
        restored: bool,
    }

    impl CursorRestore {
        fn capture() -> Self {
            let mut pos = Point::default();
            let ok = unsafe { GetCursorPos(&mut pos) };
            eprintln!(
                "cursor start = ({}, {}) (GetCursorPos ok={ok})",
                pos.x, pos.y
            );
            CursorRestore {
                pos,
                restored: false,
            }
        }

        fn restore(&mut self) {
            if self.restored {
                return;
            }
            self.restored = true;
            let ok = unsafe { SetCursorPos(self.pos.x, self.pos.y) };
            let mut now = Point::default();
            unsafe { GetCursorPos(&mut now) };
            eprintln!(
                "cursor restored to ({}, {}) -> SetCursorPos ok={ok}, now at ({}, {})",
                self.pos.x, self.pos.y, now.x, now.y
            );
        }
    }

    impl Drop for CursorRestore {
        fn drop(&mut self) {
            if !self.restored {
                let ok = unsafe { SetCursorPos(self.pos.x, self.pos.y) };
                eprintln!(
                    "CursorRestore::drop: restoring cursor to ({}, {}) on the failure \
                     path (SetCursorPos ok={ok})",
                    self.pos.x, self.pos.y
                );
            }
        }
    }

    /// The child's stdout lines after the `ready` handshake, as an event
    /// log the test polls with a bounded deadline.
    #[derive(Default)]
    struct EventLog {
        lines: Mutex<Vec<String>>,
    }

    impl EventLog {
        fn push(&self, line: String) {
            self.lines.lock().expect("event log mutex").push(line);
        }

        fn len(&self) -> usize {
            self.lines.lock().expect("event log mutex").len()
        }

        fn snapshot_since(&self, since: usize) -> Vec<String> {
            let guard = self.lines.lock().expect("event log mutex");
            guard.iter().skip(since).cloned().collect()
        }
    }

    /// Wait (bounded, polling) until `ok` holds over the child lines emitted
    /// since `since`. A timeout is a hard failure that prints everything the
    /// child reported since the injection — the "green but didn't run" anti
    /// pattern must stay visible.
    fn wait_for_event_since(
        log: &EventLog,
        since: usize,
        timeout: Duration,
        desc: &str,
        ok: impl Fn(&[String]) -> bool,
    ) {
        let deadline = Instant::now() + timeout;
        loop {
            let snap = log.snapshot_since(since);
            if ok(&snap) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{desc}: no matching child line within {timeout:?}; child lines since \
                 injection:\n{}",
                if snap.is_empty() {
                    "(none)".to_owned()
                } else {
                    snap.join("\n")
                }
            );
            std::thread::sleep(POLL);
        }
    }

    /// Wait (bounded) for the child to exit; a timeout is a hard failure.
    fn wait_for_exit(child: &mut Child, what: &str) -> ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status,
                Ok(None) => {
                    assert!(
                        Instant::now() < deadline,
                        "{what}: child did not exit within {EXIT_TIMEOUT:?}"
                    );
                    std::thread::sleep(POLL);
                }
                Err(e) => panic!("{what}: try_wait failed: {e}"),
            }
        }
    }

    /// Two-stage `agt_window_enumerate` (same pattern as `native_window_ops.rs`).
    fn enumerate_windows(lib: &Library, list: &Symbol<WindowEnumerate>) -> Vec<agt_window_info> {
        let mut required = 0usize;
        let st = unsafe { list(std::ptr::null_mut(), 0, &mut required) };
        assert_eq!(
            st,
            AGT_FAILED,
            "cap=0 probe must be AGT_FAILED (buffer_too_small), got {st} \
             ({})",
            last_error_message(lib)
        );
        let msg = last_error_message(lib);
        assert!(
            msg.contains("buffer_too_small"),
            "expected code \"buffer_too_small\" in error, got: {msg}"
        );

        let mut capacity = required + 32;
        loop {
            assert!(
                capacity < 1_000_000,
                "window count exploded far beyond the probe result"
            );
            let mut recs = vec![agt_window_info::default(); capacity];
            let mut got = 0usize;
            let st = unsafe { list(recs.as_mut_ptr(), capacity, &mut got) };
            if st == AGT_OK {
                assert!(
                    got <= capacity,
                    "out_count {got} exceeds capacity {capacity}"
                );
                recs.truncate(got);
                return recs;
            }
            assert_eq!(
                st,
                AGT_FAILED,
                "agt_window_enumerate failed: {}",
                last_error_message(lib)
            );
            let msg = last_error_message(lib);
            assert!(
                msg.contains("buffer_too_small"),
                "expected code \"buffer_too_small\" in error, got: {msg}"
            );
            assert!(
                got > capacity,
                "out_count must report a required count > capacity, got {got} <= {capacity}"
            );
            capacity = got + 32;
        }
    }

    /// Find the child's window in one enumeration snapshot by matching BOTH
    /// the pid and the title (which embeds the pid).
    fn find_child_window(
        windows: &[agt_window_info],
        child_pid: u32,
        expected_title: &[u8],
    ) -> Option<agt_window_info> {
        windows.iter().copied().find(|w| {
            w.process_id == child_pid && w.title.get(..w.title_len as usize) == Some(expected_title)
        })
    }

    /// Re-verify BEFORE every native call that the enumerated record still
    /// belongs to the child: fresh enumeration + match + pid assertion.
    fn recheck_child_window(
        lib: &Library,
        list: &Symbol<WindowEnumerate>,
        child_pid: u32,
        expected_title: &[u8],
        step: &str,
    ) -> agt_window_info {
        let windows = enumerate_windows(lib, list);
        let rec = find_child_window(&windows, child_pid, expected_title).unwrap_or_else(|| {
            panic!(
                "{step}: child window (pid {child_pid}) not found in enumeration — \
                 refusing to touch any other window"
            )
        });
        assert_eq!(
            rec.process_id, child_pid,
            "{step}: enumerated process_id {} != child pid {child_pid}",
            rec.process_id
        );
        eprintln!(
            "{step}: handle={} pid={} rect=({},{}) {}x{} minimized={}",
            rec.handle, rec.process_id, rec.x, rec.y, rec.width, rec.height, rec.minimized
        );
        rec
    }
}

#[cfg(not(windows))]
mod inject {
    #[test]
    fn input_injection_success_path() {
        eprintln!(
            "SKIP: input injection success path requires a Windows desktop session \
             (the child-owned input-target probe is Windows-only this round, and the \
             Linux/macOS injection stack is a separate milestone); host is {}",
            std::env::consts::OS
        );
    }
}
