//! Platform smoke tests for `agenterm-dyn`.

use agenterm_dyn::{Dyn, Value};

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    const TIOCGWINSZ: i64 = 0x5413;

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    fn libc_path() -> &'static str {
        "libc.so.6"
    }

    #[test]
    fn dlcall_getpid_matches_libc() {
        let mut env = Dyn::new();
        let script = format!(r#"(dlcall "{}" "getpid" "i32")"#, libc_path());
        let got = env.eval(&script).expect("getpid dlcall");
        let real = unsafe { libc::getpid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_ioctl_winsize() {
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
        let script = format!(
            r#"(dlcall "{}" "ioctl" "i32" "i32" {fd} "u64" {TIOCGWINSZ} "ptr" ws)"#,
            libc_path()
        );
        let ret = env.eval(&script).expect("ioctl dlcall");
        let code = ret.as_int().expect("ioctl return code");
        if expect_pty_dims {
            assert_eq!(code, 0, "ioctl on pty master should succeed");
            assert_eq!(ws.ws_row, 24, "pty rows");
            assert_eq!(ws.ws_col, 80, "pty cols");
        } else {
            // No pty: ENOTTY (25) or generic failure (-1) both prove the call happened.
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
        let mut env = Dyn::new();
        let script = format!(
            r#"
            (do
              (set pid (dlcall "{}" "getpid" "i32"))
              pid)
            "#,
            libc_path()
        );
        let v = env.eval(script.trim()).expect("do/dlcall");
        let real = unsafe { libc::getpid() };
        assert_eq!(v, Value::Int(i64::from(real)));
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    #[test]
    fn dlcall_getpid_matches_libc() {
        let mut env = Dyn::new();
        let got = env
            .eval(r#"(dlcall "libSystem.B.dylib" "getpid" "i32")"#)
            .expect("getpid dlcall");
        let real = unsafe { libc::getpid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    #[test]
    fn dlcall_get_current_process_id() {
        let mut env = Dyn::new();
        let got = env
            .eval(r#"(dlcall "kernel32.dll" "GetCurrentProcessId" "u32")"#)
            .expect("GetCurrentProcessId dlcall");
        let real = unsafe { GetCurrentProcessId() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[test]
fn unsupported_os_compile_only() {
    let _ = Dyn::new();
}
