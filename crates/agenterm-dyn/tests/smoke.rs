//! Platform native smoke tests — real `dlcall` into the host OS libraries.
//!
//! Each supported OS module uses [`agenterm_dyn::live_cell`] script data and
//! cross-checks results with a second `dlcall` where possible.

use std::ffi::{CString, c_void};

use agenterm_dyn::DynError;
use agenterm_dyn::{
    CU_ADJACENT_PROBE_CATALOG, Dyn, HostArch, HostOs, SecondaryProbe, Value, live_cell,
};

#[test]
fn cu_adjacent_catalog_has_six_cells() {
    assert_eq!(CU_ADJACENT_PROBE_CATALOG.len(), 6);
    assert!(
        CU_ADJACENT_PROBE_CATALOG
            .iter()
            .any(|cell| cell.os == HostOs::Linux && cell.arch == HostArch::X86_64)
    );
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use agenterm_dyn::{HostCell, LINUX_ATSPI_EXISTENCE_LIBS, SizeProbe};

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    fn cell() -> &'static HostCell {
        live_cell().expect("linux cell")
    }

    #[test]
    fn dlcall_getpid_matches_libc_and_second_dlcall() {
        let c = cell();
        let mut env = Dyn::new();
        let script = format!(
            r#"(dlcall "{}" "{}" "{}")"#,
            c.pid_lib, c.pid_symbol, c.pid_ret_type
        );
        let got = env.eval(&script).expect("getpid dlcall");
        let again = env.eval(&script).expect("second getpid dlcall");
        assert_eq!(got, again, "two dlcall getpid should agree");
        let real = unsafe { libc::getpid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getppid_secondary_probe() {
        let c = cell();
        let SecondaryProbe::Native {
            lib,
            symbol,
            ret_type,
        } = c.secondary_probe
        else {
            panic!("linux secondary should be getppid family");
        };
        let mut env = Dyn::new();
        let script = format!(r#"(dlcall "{lib}" "{symbol}" "{ret_type}")"#);
        let got = env.eval(&script).expect("getppid dlcall");
        let real = unsafe { libc::getppid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_ioctl_winsize() {
        let c = cell();
        let SizeProbe::IoctlTiocgwinsz {
            lib,
            symbol,
            request,
        } = c.size_probe
        else {
            panic!("linux size probe should be ioctl");
        };

        let mut ws = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let mut env = Dyn::new();
        env.bind("ws", (&mut ws as *mut Winsize).cast())
            .expect("bind ws");

        let (fd, expect_pty_dims) = open_probe_fd();
        let script =
            format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {fd} "u64" {request} "ptr" ws)"#);
        let ret = env.eval(&script).expect("ioctl dlcall");
        let code = ret.as_int().expect("ioctl return code");
        if expect_pty_dims {
            assert_eq!(code, 0, "ioctl on pty master should succeed");
            assert_eq!(ws.ws_row, 24, "pty rows");
            assert_eq!(ws.ws_col, 80, "pty cols");
        } else {
            assert!(
                code == -1 || code == -25,
                "unexpected ioctl result {code} (expected -1 or -ENOTTY)"
            );
        }
    }

    fn open_probe_fd() -> (i64, bool) {
        unsafe {
            let mut master: libc::c_int = -1;
            let mut slave: libc::c_int = -1;
            if libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &libc::winsize {
                    ws_row: 24,
                    ws_col: 80,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                },
            ) == 0
            {
                libc::close(slave);
                return (i64::from(master), true);
            }
        }
        (0, false)
    }

    #[test]
    fn eval_do_sequence_with_dlcall() {
        let c = cell();
        let mut env = Dyn::new();
        let script = format!(
            r#"
            (do
              (set pid (dlcall "{}" "{}" "{}"))
              pid)
            "#,
            c.pid_lib, c.pid_symbol, c.pid_ret_type
        );
        let v = env.eval(script.trim()).expect("do/dlcall");
        let real = unsafe { libc::getpid() };
        assert_eq!(v, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getenv_display_probe() {
        let c = cell();
        let mut env = Dyn::new();
        let key = CString::new("DISPLAY").expect("DISPLAY key");
        env.bind("env_key", key.as_ptr().cast::<c_void>() as *mut c_void)
            .expect("bind env_key");

        let script = format!(r#"(dlcall "{}" "getenv" "ptr" "ptr" env_key)"#, c.pid_lib);
        env.eval(&script)
            .expect("getenv dlcall should resolve and run");
    }

    #[test]
    fn dlcall_x11_x_open_display_probe() {
        let row = CU_ADJACENT_PROBE_CATALOG
            .iter()
            .find(|c| c.os == HostOs::Linux && c.arch == HostArch::X86_64)
            .expect("linux x86_64 catalog row");
        let lib = row.window_list.lib;
        let sym = row.window_list.symbol;

        let mut env = Dyn::new();
        let script = format!(r#"(dlcall "{lib}" "{sym}" "ptr" "ptr" 0)"#);
        match env.eval(&script) {
            Ok(Value::Ptr(_)) | Ok(Value::Nil) => {}
            Err(DynError::Library(msg)) => {
                assert!(
                    msg.contains(lib),
                    "library load should name {lib}, got {msg}"
                );
            }
            other => panic!("unexpected XOpenDisplay probe outcome: {other:?}"),
        }
    }

    #[test]
    fn atspi_library_existence_probe() {
        let mut attempted = false;
        for name in LINUX_ATSPI_EXISTENCE_LIBS {
            attempted = true;
            // SAFETY: existence probe only; we never invoke resolved symbols.
            let _ = unsafe { libloading::Library::new(name) };
        }
        assert!(attempted, "should try at least one AT-SPI library name");
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use agenterm_dyn::HostCell;

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    fn cell() -> &'static HostCell {
        live_cell().expect("macos cell")
    }

    #[test]
    fn dlcall_getpid_matches_libc_and_second_dlcall() {
        let c = cell();
        let mut env = Dyn::new();
        let script = format!(
            r#"(dlcall "{}" "{}" "{}")"#,
            c.pid_lib, c.pid_symbol, c.pid_ret_type
        );
        let got = env.eval(&script).expect("getpid dlcall");
        let again = env.eval(&script).expect("second getpid dlcall");
        assert_eq!(got, again);
        let real = unsafe { libc::getpid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_time_secondary_probe() {
        let c = cell();
        let SecondaryProbe::Time { lib, symbol } = c.secondary_probe else {
            panic!("macos secondary should be time()");
        };
        let mut env = Dyn::new();
        let script = format!(r#"(dlcall "{lib}" "{symbol}" "i64" "ptr" 0)"#);
        let got = env.eval(&script).expect("time dlcall");
        let t = got.as_int().expect("time return");
        assert!(
            t > 1_600_000_000,
            "time() should be a recent unix timestamp"
        );
        let again = env.eval(&script).expect("second time dlcall");
        let t2 = again.as_int().expect("second time");
        assert!(
            t2 >= t,
            "time() should be monotonic across back-to-back calls"
        );
    }

    #[test]
    fn dlcall_ioctl_winsize_on_tty_when_possible() {
        let c = cell();
        let SizeProbe::IoctlTiocgwinsz {
            lib,
            symbol,
            request,
        } = c.size_probe
        else {
            panic!("macos size probe should be ioctl");
        };

        let mut ws = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let mut env = Dyn::new();
        env.bind("ws", (&mut ws as *mut Winsize).cast())
            .expect("bind ws");

        let fd = unsafe { libc::open(b"/dev/tty\0".as_ptr().cast(), libc::O_RDONLY) };
        if fd < 0 {
            return;
        }
        let script =
            format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {fd} "u64" {request} "ptr" ws)"#);
        let ret = env.eval(&script).expect("ioctl dlcall");
        let code = ret.as_int().expect("ioctl code");
        unsafe {
            libc::close(fd);
        }
        assert!(
            code == 0 || code == -1,
            "ioctl on /dev/tty should succeed or fail with -1, got {code}"
        );
        if code == 0 {
            assert!(
                ws.ws_row > 0 && ws.ws_col > 0,
                "winsize should be populated"
            );
        }
    }

    #[test]
    fn dlcall_getenv_display_probe() {
        let c = cell();
        let mut env = Dyn::new();
        let key = CString::new("DISPLAY").expect("DISPLAY key");
        env.bind("env_key", key.as_ptr().cast::<c_void>() as *mut c_void)
            .expect("bind env_key");
        let script = format!(r#"(dlcall "{}" "getenv" "ptr" "ptr" env_key)"#, c.pid_lib);
        env.eval(&script)
            .expect("getenv dlcall should resolve and run");
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use agenterm_dyn::HostCell;
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};

    fn cell() -> &'static HostCell {
        live_cell().expect("windows cell")
    }

    #[test]
    fn dlcall_get_current_process_id() {
        let c = cell();
        let mut env = Dyn::new();
        let script = format!(
            r#"(dlcall "{}" "{}" "{}")"#,
            c.pid_lib, c.pid_symbol, c.pid_ret_type
        );
        let got = env.eval(&script).expect("GetCurrentProcessId dlcall");
        let again = env.eval(&script).expect("second dlcall");
        assert_eq!(got, again);
        let real = unsafe { GetCurrentProcessId() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_get_current_thread_id_secondary() {
        let c = cell();
        let SecondaryProbe::Native {
            lib,
            symbol,
            ret_type,
        } = c.secondary_probe
        else {
            panic!("windows secondary should be GetCurrentThreadId");
        };
        let mut env = Dyn::new();
        let script = format!(r#"(dlcall "{lib}" "{symbol}" "{ret_type}")"#);
        let got = env.eval(&script).expect("GetCurrentThreadId dlcall");
        let real = unsafe { GetCurrentThreadId() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getenv_display_probe() {
        let mut env = Dyn::new();
        let key = CString::new("DISPLAY").expect("DISPLAY key");
        env.bind("env_key", key.as_ptr().cast::<c_void>() as *mut c_void)
            .expect("bind env_key");
        match env.eval(r#"(dlcall "ucrtbase.dll" "getenv" "ptr" "ptr" env_key)"#) {
            Ok(_) => {}
            Err(DynError::Library(_)) => {
                env.eval(r#"(dlcall "msvcrt.dll" "getenv" "ptr" "ptr" env_key)"#)
                    .expect("getenv via msvcrt when ucrtbase is absent");
            }
            Err(other) => panic!("unexpected getenv probe error: {other:?}"),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported {
    use super::*;

    #[test]
    fn host_table_compiles_without_live_native_smoke() {
        assert!(live_cell().is_none());
        let _ = Dyn::new();
    }
}

#[test]
fn live_cell_present_on_supported_hosts() {
    if cfg!(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    )) {
        assert!(
            live_cell().is_some(),
            "supported OS should resolve a live host cell"
        );
    }
}

#[test]
fn non_live_cells_are_distinct_from_live() {
    use agenterm_dyn::ALL_CELLS;
    if let Some(live) = live_cell() {
        let others: Vec<_> = ALL_CELLS
            .iter()
            .filter(|c| c.os != live.os || c.arch != live.arch)
            .collect();
        assert_eq!(others.len(), 5, "five non-live placeholder rows");
    }
}
