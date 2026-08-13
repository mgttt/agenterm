//! Milestone 70: exercise the SUCCESS path of the `agt_native_window_*`
//! exports against a real native window owned by a CHILD process we spawn —
//! never against the ABI's own window handle from `agt_window_open`.
//!
//! The header is explicit (include/agenterm.h): the native-window operations
//! act on raw OS handles obtained from `agt_window_enumerate`, NEVER on the
//! ABI's window handle (`agt_window_close` owns that one; the two are
//! unrelated). Milestone 64 tried operating on an ABI-hosted window and ran
//! into the enumerate rendezvous deadlock (fixed in 65) — the contract
//! forbids that use anyway.
//!
//! So this test spawns `examples/c/agenterm_plain_window.c` as a child
//! process, which opens a plain top-level Win32 window that THIS test owns
//! and can kill. The child pumps messages, so its window answers `WM_GETTEXT`
//! and `agt_window_enumerate` can read the title; the title embeds the child
//! pid, so the test matches on BOTH the title AND `process_id == child pid`.
//!
//! Safety boundary (violating any of these invalidates the test):
//!   - ONLY windows with `process_id == the spawned child's pid` are touched;
//!     the pid is re-verified before EVERY native call (re-enumerate + match),
//!     never filtered once;
//!   - never falls back to another window when the child's is not found;
//!   - never calls `agt_input_*` (real mouse/keyboard);
//!   - moves are small (+40/+40), never off-screen; `set_topmost` is always
//!     undone; minimize is always restored (`close` is the last step);
//!   - no child process or window survives: the guard kills the child on
//!     every failure path, and the success path ends with
//!     `agt_native_window_close` -> child exits 0.
//!
//! Windows-only this round: Linux/macOS CI runners are headless and the test
//! SKIPs with the reason printed (the skip branch still asserts — here the
//! whole mechanism is a compile-time platform branch, so the skip is a
//! printed decision, not a silent pass).

mod common;

#[cfg(windows)]
mod native_ops {
    use super::common::capabilities::{AGT_CAP_WINDOW_ENUMERATE, AGT_CAP_WINDOW_OP};
    use super::common::toolchain;
    use libloading::{Library, Symbol};
    use std::ffi::{CStr, c_char};
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    const AGT_OK: i32 = 0;
    const AGT_UNSUPPORTED: i32 = 1;
    const AGT_FAILED: i32 = 2;
    /// The child must print its `ready <hwnd>` line within this bound.
    const READY_TIMEOUT: Duration = Duration::from_secs(10);
    /// The child must exit within this bound after `agt_native_window_close`.
    const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
    /// Poll interval while waiting for the child to exit.
    const POLL: Duration = Duration::from_millis(50);

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
    type NativeWindowShow = unsafe extern "C" fn(isize, i32) -> i32;
    type NativeWindowMove = unsafe extern "C" fn(isize, i32, i32, u32, u32) -> i32;
    type NativeWindowRect =
        unsafe extern "C" fn(isize, *mut i32, *mut i32, *mut u32, *mut u32) -> i32;
    type NativeWindowSetTopmost = unsafe extern "C" fn(isize, i32) -> i32;
    type NativeWindowClose = unsafe extern "C" fn(isize) -> i32;
    type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;

    /// Load the cdylib and leak the `Library` handle: the DLL's private
    /// threads may still be winding down when the test returns, so dropping
    /// the handle here would `FreeLibrary` the module out from under them
    /// and crash the process at exit. Leaking keeps the module resident for
    /// the whole test process lifetime (same convention as `dylib_load.rs`).
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
    fn native_window_ops_on_child_window() {
        let lib = load();
        let query: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") };

        // ---- capability guard -------------------------------------------
        // Both mechanisms must exist; when one is missing the cap query must
        // answer AGT_UNSUPPORTED and the test SKIPs (the skip branch asserts
        // the status, never a bare return). A cap query must never answer
        // AGT_FAILED for a known mechanism.
        for cap in [AGT_CAP_WINDOW_ENUMERATE, AGT_CAP_WINDOW_OP] {
            let st = unsafe { query(cap) };
            if st == AGT_UNSUPPORTED {
                eprintln!(
                    "SKIP: capability {cap} reports AGT_UNSUPPORTED on this host \
                     (headless session?); native window ops cannot run"
                );
                return;
            }
            assert_eq!(
                st, AGT_OK,
                "capability {cap} must answer AGT_OK or AGT_UNSUPPORTED, got {st}"
            );
        }
        eprintln!("capabilities AGT_CAP_WINDOW_ENUMERATE + AGT_CAP_WINDOW_OP -> AGT_OK");

        // ---- build the plain-window probe -------------------------------
        let Some(compiler) = toolchain::find_c_compiler("native_window_ops") else {
            eprintln!(
                "SKIP: no C compiler matching target_env={} was found (see the \
                 target_env= decision line above) — cannot build the plain-window probe",
                toolchain::target_env_name()
            );
            return;
        };

        let root = toolchain::repo_root();
        let include = root.join("include");
        let c_file = root.join("examples/c/agenterm_plain_window.c");
        assert!(
            c_file.is_file(),
            "missing {} (expected next to this test)",
            c_file.display()
        );

        let seq = toolchain::DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let scratch = std::env::temp_dir().join(format!(
            "agenterm-native-window-ops-{}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&scratch).expect("create scratch dir");
        let _cleanup = toolchain::Cleanup(scratch.clone());

        let exe_name = "agenterm_plain_window.exe";
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
            // /W4 /WX = warnings are errors. cl re-parses the raw command
            // line (not CommandLineToArgvW rules), so each path-bearing
            // option must be a single argument. CWD = scratch keeps the
            // .obj/.exe out of the repo tree. The probe links nothing: it
            // needs only user32 (requested via #pragma comment(lib)).
            cc.current_dir(&scratch);
            cc.arg("/nologo").arg("/W4").arg("/WX");
            cc.arg(format!("/I{}", include.display()));
            cc.arg(&c_file);
            cc.arg("/Foagenterm_plain_window.obj");
            cc.arg(format!("/Fe{exe_name}"));
        } else {
            cc.arg("-Wall").arg("-Wextra").arg("-Werror");
            cc.arg("-I").arg(&include);
            cc.arg(&c_file);
            cc.arg("-o").arg(&exe);
        }
        toolchain::run_or_panic("plain-window probe compile/link", &mut cc);

        // ---- spawn the child and wait for its `ready` line --------------
        let mut child = Command::new(&exe)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", exe.display()));
        let child_pid = child.id();
        let stdout = child.stdout.take().expect("child stdout is piped");
        let mut guard = ChildGuard {
            child: Some(child),
            label: format!("agenterm_plain_window(pid={child_pid})"),
        };

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            let first = lines.next().map(|r| r.unwrap_or_default());
            let _ = tx.send(first);
        });
        let line = rx
            .recv_timeout(READY_TIMEOUT)
            .unwrap_or_else(|_| {
                panic!(
                    "timed out after {READY_TIMEOUT:?} waiting for the child's `ready` \
                     line (pid {child_pid}); the child was killed by the guard"
                )
            })
            .unwrap_or_else(|| {
                panic!("child {child_pid} closed stdout before printing `ready <hwnd>`")
            });
        let reported_hwnd: isize = line
            .strip_prefix("ready ")
            .unwrap_or_else(|| {
                panic!(
                    "child {child_pid} first stdout line was {line:?}, \
                     expected `ready <hwnd>`"
                )
            })
            .trim()
            .parse()
            .unwrap_or_else(|_| {
                panic!("child {child_pid} first stdout line {line:?} has an unparseable hwnd")
            });
        eprintln!("child {child_pid} reported hwnd={reported_hwnd}");

        // ---- symbols ------------------------------------------------------
        let list: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") };
        let rect_fn: Symbol<NativeWindowRect> = unsafe { sym(lib, b"agt_native_window_rect") };
        let move_fn: Symbol<NativeWindowMove> = unsafe { sym(lib, b"agt_native_window_move") };
        let topmost: Symbol<NativeWindowSetTopmost> =
            unsafe { sym(lib, b"agt_native_window_set_topmost") };
        let show: Symbol<NativeWindowShow> = unsafe { sym(lib, b"agt_native_window_show") };
        let close_fn: Symbol<NativeWindowClose> = unsafe { sym(lib, b"agt_native_window_close") };

        // ---- locate the child's window -----------------------------------
        // Matching on title + process_id, never on anything else. Not found
        // is a hard failure (we KNOW the child opened a window), never a
        // fallback to another window.
        let expected_title = format!("agenterm-plain-window-probe-{child_pid}").into_bytes();
        let first = recheck_child_window(lib, &list, child_pid, &expected_title, "locate");
        let handle = first.handle;
        assert!(handle != 0, "enumerated handle must be non-zero");
        assert_eq!(
            handle, reported_hwnd,
            "enumerated handle must equal the child-reported hwnd"
        );
        assert!(
            first.width > 0 && first.height > 0,
            "initial rect must be non-zero, got {}x{}",
            first.width,
            first.height
        );

        // ---- agt_native_window_rect --------------------------------------
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "pre-rect");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
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
        eprintln!("agt_native_window_rect -> AGT_OK: rect=({x},{y}) {w}x{h}");

        // ---- agt_native_window_move (+40/+40, same size) -----------------
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "pre-move");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
        let (nx, ny) = (x + 40, y + 40);
        let st = unsafe { move_fn(handle, nx, ny, w, h) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_move failed: {}",
            last_error_message(lib)
        );
        // Confirm the position actually changed via a fresh rect read.
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "post-move-rect");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
        let mut x2 = 0i32;
        let mut y2 = 0i32;
        let mut w2 = 0u32;
        let mut h2 = 0u32;
        let st = unsafe { rect_fn(handle, &mut x2, &mut y2, &mut w2, &mut h2) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_rect (after move) failed: {}",
            last_error_message(lib)
        );
        assert_eq!(
            (x2, y2),
            (nx, ny),
            "window must actually move from ({x},{y}) to ({nx},{ny}), still at ({x2},{y2})"
        );
        assert_eq!((w2, h2), (w, h), "move must not resize the window");
        eprintln!(
            "agt_native_window_move({nx},{ny}) -> AGT_OK; post-move rect=({x2},{y2}) {w2}x{h2}"
        );

        // ---- agt_native_window_set_topmost: on, then ALWAYS undone -------
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "pre-topmost(1)");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
        let st = unsafe { topmost(handle, 1) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_set_topmost(1) failed: {}",
            last_error_message(lib)
        );
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "pre-topmost(0)");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
        let st = unsafe { topmost(handle, 0) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_set_topmost(0) failed: {}",
            last_error_message(lib)
        );
        eprintln!("agt_native_window_set_topmost(1) and set_topmost(0) -> AGT_OK (topmost undone)");

        // ---- agt_native_window_show: Minimize, then Restore --------------
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "pre-show(2)");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
        let st = unsafe { show(handle, 2) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_show(2 Minimize) failed: {}",
            last_error_message(lib)
        );
        let rec = recheck_child_window(lib, &list, child_pid, &expected_title, "pre-show(4)");
        assert_eq!(rec.handle, handle, "child window handle must be stable");
        let st = unsafe { show(handle, 4) };
        assert_eq!(
            st,
            AGT_OK,
            "agt_native_window_show(4 Restore) failed: {}",
            last_error_message(lib)
        );
        eprintln!("agt_native_window_show(2 Minimize) and show(4 Restore) -> AGT_OK");

        // ---- agt_native_window_close: the intended terminal state --------
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
        eprintln!("native_window_ops: child-owned window round trip OK");
    }

    /// RAII guard that guarantees the child is killed on EVERY path (panic
    /// unwinding included) and its exit status is reaped, so no zombie
    /// process or orphan window survives a failed run. `finish` takes the
    /// child out of the guard so the success path reaps exactly once.
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
                    // Already exited: reap the handle only.
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

    /// Two-stage `agt_window_enumerate` (same pattern as `dylib_load.rs`):
    /// cap=0 probe, then allocate the required count and retry on growth.
    /// A missing mechanism is impossible here (the capability guard above
    /// already returned on AGT_UNSUPPORTED), so a non-buffer_too_small
    /// failure is a hard test failure.
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
    /// the pid and the title (which embeds the pid). `title` is inline UTF-8
    /// without a NUL terminator; compare `title[..title_len]` only.
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
    /// belongs to the child: fresh two-stage enumeration + match + pid
    /// assertion. Never touches a window this does not prove ownership of.
    /// Not found is a hard failure (the child opened a window and is still
    /// alive unless a previous step failed, which would have panicked).
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
mod native_ops {
    #[test]
    fn native_window_ops_on_child_window() {
        eprintln!(
            "SKIP: native_window_ops requires a Windows desktop session (the \
             child-owned plain-window probe is Windows-only this round); host is {}",
            std::env::consts::OS
        );
    }
}
