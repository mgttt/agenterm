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
    use agenterm_dyn::{
        HostCell, LINUX_ATSPI_EXISTENCE_LIBS, SizeProbe, SystemProbe, SystemProbeStatus,
    };

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

    fn getpid_script() -> String {
        let c = cell();
        format!(
            r#"(dlcall "{}" "{}" "{}")"#,
            c.pid_lib, c.pid_symbol, c.pid_ret_type
        )
    }

    #[test]
    fn dlcall_getpid_matches_libc() {
        let mut env = Dyn::new();
        let script = getpid_script();
        let got = env.eval(&script).expect("getpid dlcall");
        let real = unsafe { libc::getpid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getpid_is_stable_across_cached_library_calls() {
        let mut env = Dyn::new();
        let script = getpid_script();
        let first = env.eval(&script).expect("first getpid dlcall");
        let second = env.eval(&script).expect("cached-library getpid dlcall");
        assert_eq!(first, second, "cached libc must not change symbol results");
    }

    #[test]
    fn missing_symbol_does_not_evict_cached_libc() {
        let c = cell();
        let missing_symbol = "agenterm_dyn_missing_before_getpid";
        let missing = format!(r#"(dlcall "{}" "{missing_symbol}" "i32")"#, c.pid_lib);
        let mut env = Dyn::new();
        let err = env.eval(&missing).unwrap_err();
        assert!(matches!(err, DynError::DlCall(_)));
        assert!(err.to_string().contains(missing_symbol));

        let got = env
            .eval(&getpid_script())
            .expect("getpid after missing symbol");
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
        let system_probe = live_system_probe("getppid");
        assert_eq!(
            system_probe.status,
            SystemProbeStatus::LiveDlcall { lib, symbol }
        );
        let mut env = Dyn::new();
        let script = format!(r#"(dlcall "{lib}" "{symbol}" "{ret_type}")"#);
        let got = env.eval(&script).expect("getppid dlcall");
        let real = unsafe { libc::getppid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getpgrp_matches_libc() {
        let probe = live_system_probe("getpgrp");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32")"#))
            .expect("getpgrp dlcall");
        let real = unsafe { libc::getpgrp() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getsid_zero_matches_libc() {
        let probe = live_system_probe("getsid");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0)"#))
            .expect("getsid(0) dlcall");
        let real = unsafe { libc::getsid(0) };
        assert!(real > 0, "current session id should be positive");
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getpgid_zero_matches_libc() {
        let probe = live_system_probe("getpgid");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0)"#))
            .expect("getpgid(0) dlcall");
        let real = unsafe { libc::getpgid(0) };
        assert!(real > 0, "current process group id should be positive");
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_time_matches_libc() {
        let mut env = Dyn::new();
        let got = env
            .eval(r#"(dlcall "libc.so.6" "time" "i64" "ptr" 0)"#)
            .expect("time dlcall")
            .as_int()
            .expect("time return value");
        let real = unsafe { libc::time(std::ptr::null_mut()) };
        assert!(
            (got - real).abs() <= 1,
            "dlcall and libc time should be adjacent"
        );
    }

    #[test]
    fn dlcall_void_return_maps_to_nil() {
        let mut env = Dyn::new();
        let got = env
            .eval(r#"(dlcall "libc.so.6" "free" "void" "ptr" 0)"#)
            .expect("free(NULL) dlcall");
        assert_eq!(got, Value::Nil);
    }

    #[test]
    fn dlcall_clock_gettime_writes_timespec() {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let mut env = Dyn::new();
        env.bind("ts", (&mut ts as *mut libc::timespec).cast())
            .expect("bind timespec");
        let got = env
            .eval(r#"(dlcall "libc.so.6" "clock_gettime" "i32" "i32" 1 "ptr" ts)"#)
            .expect("clock_gettime dlcall");
        assert_eq!(got, Value::Int(0));
        assert!(ts.tv_sec > 0);
        assert!((0..1_000_000_000).contains(&ts.tv_nsec));
    }

    #[test]
    fn dlcall_uname_writes_linux_identity() {
        let mut uts = std::mem::MaybeUninit::<libc::utsname>::zeroed();
        let mut env = Dyn::new();
        env.bind("uts", uts.as_mut_ptr().cast())
            .expect("bind utsname");
        let got = env
            .eval(r#"(dlcall "libc.so.6" "uname" "i32" "ptr" uts)"#)
            .expect("uname dlcall");
        assert_eq!(got, Value::Int(0));
        let uts = unsafe { uts.assume_init() };
        let sysname = unsafe { std::ffi::CStr::from_ptr(uts.sysname.as_ptr()) };
        assert_eq!(sysname.to_bytes(), b"Linux");
    }

    fn live_system_probe(name: &str) -> SystemProbe {
        let probe = cell()
            .system_probes
            .into_iter()
            .find(|probe| probe.name == name)
            .unwrap_or_else(|| panic!("missing {name} system probe"));
        assert!(matches!(probe.status, SystemProbeStatus::LiveDlcall { .. }));
        probe
    }

    #[test]
    fn dlcall_getuid_matches_libc() {
        let probe = live_system_probe("getuid");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "u32")"#))
            .expect("getuid dlcall");
        let real = unsafe { libc::getuid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getgid_matches_libc() {
        let probe = live_system_probe("getgid");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "u32")"#))
            .expect("getgid dlcall");
        let real = unsafe { libc::getgid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_geteuid_matches_libc() {
        let probe = live_system_probe("geteuid");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "u32")"#))
            .expect("geteuid dlcall");
        let real = unsafe { libc::geteuid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_getegid_matches_libc() {
        let probe = live_system_probe("getegid");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "u32")"#))
            .expect("getegid dlcall");
        let real = unsafe { libc::getegid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_sysconf_pagesize_matches_libc() {
        let probe = live_system_probe("sysconf_pagesize");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i64" "i32" {})"#,
                libc::_SC_PAGESIZE
            ))
            .expect("sysconf(_SC_PAGESIZE) dlcall");
        let real = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        assert!(real > 0, "host page size should be positive");
        assert_eq!(got, Value::Int(real));
    }

    #[test]
    fn dlcall_sysconf_clk_tck_matches_libc() {
        let probe = live_system_probe("sysconf_clk_tck");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i64" "i32" {})"#,
                libc::_SC_CLK_TCK
            ))
            .expect("sysconf(_SC_CLK_TCK) dlcall");
        let real = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        assert!(real > 0, "host clock ticks per second should be positive");
        assert_eq!(got, Value::Int(real));
    }

    #[test]
    fn dlcall_sysconf_nprocessors_onln_matches_libc() {
        let probe = live_system_probe("sysconf_nprocessors_onln");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i64" "i32" {})"#,
                libc::_SC_NPROCESSORS_ONLN
            ))
            .expect("sysconf(_SC_NPROCESSORS_ONLN) dlcall");
        let real = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        assert!(real > 0, "online processor count should be positive");
        assert_eq!(got, Value::Int(real));
    }

    #[test]
    fn dlcall_getcwd_writes_current_directory() {
        use std::os::unix::ffi::OsStrExt;

        let probe = live_system_probe("getcwd");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut buffer = [0_u8; 4096];
        let buffer_ptr = buffer.as_mut_ptr();
        let mut env = Dyn::new();
        env.bind("cwd", buffer_ptr.cast()).expect("bind cwd buffer");
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "ptr" "ptr" cwd "u64" {})"#,
                buffer.len()
            ))
            .expect("getcwd dlcall");
        assert_eq!(got, Value::Ptr(buffer_ptr as usize));
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .expect("getcwd result should be NUL terminated");
        let expected = std::env::current_dir().expect("read current directory");
        assert_eq!(&buffer[..end], expected.as_os_str().as_bytes());
    }

    #[test]
    fn dlcall_isatty_stdin_reports_real_host_state() {
        let probe = live_system_probe("isatty_stdin");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0)"#))
            .expect("isatty(0) dlcall");
        let real = unsafe { libc::isatty(0) };
        assert!(matches!(real, 0 | 1));
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_open_dev_null_is_not_tty_and_closes_fd() {
        let probe = live_system_probe("open_dev_null");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let path = CString::new("/dev/null").expect("/dev/null path");
        let mut env = Dyn::new();
        env.bind("dev_null", path.as_ptr().cast_mut().cast())
            .expect("bind /dev/null path");
        let fd = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "ptr" dev_null "i32" {})"#,
                libc::O_RDONLY
            ))
            .expect("open(/dev/null) dlcall")
            .as_int()
            .expect("open return code");
        assert!(fd >= 0, "open(/dev/null) returned {fd}");

        let isatty = env.eval(&format!(r#"(dlcall "{lib}" "isatty" "i32" "i32" {fd})"#));
        let close = env.eval(&format!(r#"(dlcall "{lib}" "close" "i32" "i32" {fd})"#));

        assert_eq!(isatty.expect("isatty(/dev/null) dlcall"), Value::Int(0));
        assert_eq!(close.expect("close(/dev/null) dlcall"), Value::Int(0));
    }

    #[test]
    fn dlcall_access_root_f_ok_succeeds() {
        let probe = live_system_probe("access_root");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let path = CString::new("/").expect("root path");
        let mut env = Dyn::new();
        env.bind("root", path.as_ptr().cast_mut().cast())
            .expect("bind root path");
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "ptr" root "i32" {})"#,
                libc::F_OK
            ))
            .expect("access(\"/\", F_OK) dlcall");
        assert_eq!(got, Value::Int(0));
    }

    #[test]
    fn dlcall_access_missing_path_fails_after_real_call() {
        let probe = live_system_probe("access_missing");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let path = CString::new("/proc/self/agenterm-dyn-missing-access-probe")
            .expect("missing probe path");
        let mut env = Dyn::new();
        env.bind("missing", path.as_ptr().cast_mut().cast())
            .expect("bind missing path");
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "ptr" missing "i32" {})"#,
                libc::F_OK
            ))
            .expect("access(missing, F_OK) dlcall");
        assert_eq!(got, Value::Int(-1));
    }

    #[test]
    fn dlcall_fcntl_stdin_getfd_matches_libc() {
        let probe = live_system_probe("fcntl_stdin_getfd");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0 "i32" {})"#,
                libc::F_GETFD
            ))
            .expect("fcntl(0, F_GETFD) dlcall");
        let real = unsafe { libc::fcntl(0, libc::F_GETFD) };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_dup_stdin_then_close() {
        let probe = live_system_probe("dup_stdin");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let fd = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0)"#))
            .expect("dup(0) dlcall")
            .as_int()
            .expect("dup return code");
        let close =
            (fd >= 0).then(|| env.eval(&format!(r#"(dlcall "{lib}" "close" "i32" "i32" {fd})"#)));

        assert!(fd >= 0, "dup(0) returned {fd}");
        assert_eq!(
            close
                .expect("successful dup should be closed")
                .expect("close duplicated fd dlcall"),
            Value::Int(0)
        );
    }

    #[test]
    fn dlcall_getpriority_process_matches_libc() {
        let probe = live_system_probe("getpriority_process");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "u32" {} "u32" 0)"#,
                libc::PRIO_PROCESS
            ))
            .expect("getpriority(PRIO_PROCESS, 0) dlcall");
        let real = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_nice_zero_matches_libc() {
        let probe = live_system_probe("nice_zero");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0)"#))
            .expect("nice(0) dlcall");
        let real = unsafe { libc::nice(0) };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_lseek_stdin_cur_matches_libc() {
        let probe = live_system_probe("lseek_stdin_cur");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i64" "i32" 0 "i64" 0 "i32" {})"#,
                libc::SEEK_CUR
            ))
            .expect("lseek(0, 0, SEEK_CUR) dlcall");
        let real = unsafe { libc::lseek(0, 0, libc::SEEK_CUR) };
        assert_eq!(got, Value::Int(real));
    }

    #[test]
    fn dlcall_fcntl_stdin_getfl_matches_libc() {
        let probe = live_system_probe("fcntl_stdin_getfl");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0 "i32" {})"#,
                libc::F_GETFL
            ))
            .expect("fcntl(0, F_GETFL) dlcall");
        let real = unsafe { libc::fcntl(0, libc::F_GETFL) };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_isatty_stdout_matches_libc() {
        let probe = live_system_probe("isatty_stdout");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 1)"#))
            .expect("isatty(1) dlcall");
        let real = unsafe { libc::isatty(1) };
        assert!(matches!(real, 0 | 1));
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_isatty_stderr_matches_libc() {
        let probe = live_system_probe("isatty_stderr");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 2)"#))
            .expect("isatty(2) dlcall");
        let real = unsafe { libc::isatty(2) };
        assert!(matches!(real, 0 | 1));
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_sched_yield_as_void_completes() {
        let probe = live_system_probe("sched_yield_void");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "void")"#))
            .expect("sched_yield void dlcall");
        assert_eq!(got, Value::Nil);
    }

    #[test]
    fn dlcall_alarm_zero_returns_integer_and_leaves_none_pending() {
        let probe = live_system_probe("alarm_zero");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let prior = unsafe { libc::alarm(0) };
        assert_eq!(
            prior, 0,
            "test process should start without a pending alarm"
        );

        let mut env = Dyn::new();
        let got = env.eval(&format!(r#"(dlcall "{lib}" "{symbol}" "u32" "u32" 0)"#));
        let remaining = unsafe { libc::alarm(0) };

        assert_eq!(got.expect("alarm(0) dlcall"), Value::Int(0));
        assert_eq!(remaining, 0, "alarm(0) should leave no alarm pending");
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
