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

    /// Test-only owner for file descriptors obtained from `openpty`.
    ///
    /// The ioctl assertions intentionally have several early-panic paths, so
    /// the pty master must be tied to Rust scope rather than a trailing close.
    struct ProbeFd(libc::c_int);

    impl ProbeFd {
        fn as_i64(&self) -> i64 {
            i64::from(self.0)
        }
    }

    impl Drop for ProbeFd {
        fn drop(&mut self) {
            if self.0 >= 0 {
                // SAFETY: this owner is created only from an fd returned by
                // openpty (including an unusual partial-failure result) and
                // is the sole closer for that descriptor.
                unsafe {
                    libc::close(self.0);
                }
            }
        }
    }

    fn cell() -> &'static HostCell {
        live_cell().expect("linux cell")
    }

    fn run_isolated_test(child: &str) {
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", child, "--nocapture"])
            .env("AGENTERM_DYN_ISOLATED_CHILD", child)
            .status()
            .expect("spawn isolated smoke child");
        assert!(status.success(), "isolated child {child} failed: {status}");
    }

    #[test]
    fn variadic_system_probes_are_catalogued_but_not_invoked() {
        for name in ["open_dev_null", "fcntl_stdin_getfd", "fcntl_stdin_getfl"] {
            let probe = cell()
                .system_probes
                .into_iter()
                .find(|probe| probe.name == name)
                .unwrap_or_else(|| panic!("missing system probe {name}"));
            assert!(matches!(probe.status, SystemProbeStatus::Placeholder));
        }
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
    fn dlcall_times_writes_caller_owned_tms_and_matches_libc_baseline() {
        let probe = live_system_probe("times");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut dlcall_tms = libc::tms {
            tms_utime: 0,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
        };
        let mut env = Dyn::new();
        env.bind("tms", (&mut dlcall_tms as *mut libc::tms).cast())
            .expect("bind caller-owned tms");

        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i64" "ptr" tms)"#))
            .expect("times dlcall")
            .as_int()
            .expect("times return value");

        let mut libc_tms = libc::tms {
            tms_utime: 0,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
        };
        let baseline = unsafe { libc::times(&mut libc_tms) };

        assert!(got >= 0, "times dlcall should return elapsed clock ticks");
        assert!(
            baseline >= got,
            "later libc baseline must not precede dlcall result"
        );
        assert!(
            libc_tms.tms_utime >= dlcall_tms.tms_utime
                && libc_tms.tms_stime >= dlcall_tms.tms_stime
                && libc_tms.tms_cutime >= dlcall_tms.tms_cutime
                && libc_tms.tms_cstime >= dlcall_tms.tms_cstime,
            "times fields must be initialized by dlcall and monotonic at the later libc baseline"
        );
    }

    #[test]
    fn dlcall_getrusage_writes_caller_owned_rusage_and_matches_libc_baseline() {
        let probe = live_system_probe("getrusage");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut dlcall_usage: libc::rusage = unsafe { std::mem::zeroed() };
        let mut env = Dyn::new();
        env.bind("usage", (&mut dlcall_usage as *mut libc::rusage).cast())
            .expect("bind caller-owned rusage");

        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {} "ptr" usage)"#,
                libc::RUSAGE_SELF
            ))
            .expect("getrusage dlcall");
        assert_eq!(got, Value::Int(0));

        let mut libc_usage: libc::rusage = unsafe { std::mem::zeroed() };
        let baseline = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut libc_usage) };
        assert_eq!(baseline, 0, "direct libc getrusage baseline");
        assert!(
            timeval_at_most(dlcall_usage.ru_utime, libc_usage.ru_utime)
                && timeval_at_most(dlcall_usage.ru_stime, libc_usage.ru_stime),
            "later direct libc baseline must not precede dlcall CPU usage"
        );
    }

    #[test]
    fn dlcall_getrlimit_nofile_writes_caller_owned_rlimit_and_matches_libc_baseline() {
        let probe = live_system_probe("getrlimit_nofile");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut dlcall_limit: libc::rlimit = unsafe { std::mem::zeroed() };
        let mut env = Dyn::new();
        env.bind("limit", (&mut dlcall_limit as *mut libc::rlimit).cast())
            .expect("bind caller-owned rlimit");

        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {} "ptr" limit)"#,
                libc::RLIMIT_NOFILE
            ))
            .expect("getrlimit dlcall");
        assert_eq!(got, Value::Int(0));

        let mut libc_limit: libc::rlimit = unsafe { std::mem::zeroed() };
        let baseline = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut libc_limit) };
        assert_eq!(baseline, 0, "direct libc getrlimit baseline");
        assert_eq!(
            dlcall_limit.rlim_cur, libc_limit.rlim_cur,
            "getrlimit dlcall soft limit must match the direct libc baseline"
        );
        assert_eq!(
            dlcall_limit.rlim_max, libc_limit.rlim_max,
            "getrlimit dlcall hard limit must match the direct libc baseline"
        );
    }

    fn timeval_at_most(left: libc::timeval, right: libc::timeval) -> bool {
        (left.tv_sec, left.tv_usec) <= (right.tv_sec, right.tv_usec)
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
    fn dlcall_sched_yield_matches_libc_status() {
        let probe = live_system_probe("sched_yield");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32")"#))
            .expect("sched_yield i32 dlcall");
        let direct = unsafe { libc::sched_yield() };
        assert_eq!(direct, 0, "sched_yield direct status");
        assert_eq!(got, Value::Int(i64::from(direct)));
    }

    #[test]
    fn dlcall_alarm_zero_returns_integer_and_leaves_none_pending() {
        run_isolated_test("linux::dlcall_alarm_zero_child");
    }

    #[test]
    fn dlcall_alarm_zero_child() {
        if std::env::var("AGENTERM_DYN_ISOLATED_CHILD").ok().as_deref()
            != Some("linux::dlcall_alarm_zero_child")
        {
            return;
        }
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
    fn dlcall_umask_reads_and_immediately_restores_current_mask() {
        run_isolated_test("linux::dlcall_umask_child");
    }

    #[test]
    fn dlcall_umask_child() {
        if std::env::var("AGENTERM_DYN_ISOLATED_CHILD").ok().as_deref()
            != Some("linux::dlcall_umask_child")
        {
            return;
        }
        let probe = live_system_probe("umask");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let previous = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "u32" "u32" 0)"#))
            .expect("umask(0) dlcall")
            .as_int()
            .expect("umask return value");
        let restored = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "u32" "u32" {previous})"#
            ))
            .expect("umask(previous) restore dlcall");

        assert_eq!(
            restored,
            Value::Int(0),
            "restore should replace temporary zero mask"
        );
        assert_eq!(
            previous & !0o777,
            0,
            "Linux umask should contain permission bits only"
        );
    }

    #[test]
    fn dlcall_getdtablesize_matches_libc() {
        let probe = live_system_probe("getdtablesize");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32")"#))
            .expect("getdtablesize dlcall");
        let real = unsafe { libc::getdtablesize() };
        assert!(real > 0, "descriptor table size should be positive");
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn dlcall_gethostid_matches_libc() {
        let probe = live_system_probe("gethostid");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i64")"#))
            .expect("gethostid dlcall");
        let real = unsafe { libc::gethostid() };
        assert_eq!(got, Value::Int(real));
    }

    #[test]
    fn dlcall_getpagesize_matches_libc_and_sysconf() {
        let probe = live_system_probe("getpagesize");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!("live_system_probe validates status")
        };
        let mut env = Dyn::new();
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32")"#))
            .expect("getpagesize dlcall");
        let sysconf = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        assert!(sysconf > 0, "page size should be positive");
        assert_eq!(got, Value::Int(sysconf));
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
        let raw_fd = fd.as_ref().map_or(0, ProbeFd::as_i64);
        let script =
            format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {raw_fd} "u64" {request} "ptr" ws)"#);
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

    fn open_probe_fd() -> (Option<ProbeFd>, bool) {
        unsafe {
            let mut master: libc::c_int = -1;
            let mut slave: libc::c_int = -1;
            let status = libc::openpty(
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
            );
            if status == 0 {
                let master = ProbeFd(master);
                let _slave = ProbeFd(slave);
                return (Some(master), true);
            }

            // Defensive ownership for any libc implementation that leaves a
            // descriptor initialized on a failed openpty call.
            drop(ProbeFd(master));
            drop(ProbeFd(slave));
        }
        (None, false)
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
            Ok(Value::Ptr(display)) if display != 0 => {
                let close = env
                    .eval(&format!(
                        r#"(dlcall "{lib}" "XCloseDisplay" "i32" "ptr" {display})"#
                    ))
                    .expect("XCloseDisplay dlcall");
                assert_eq!(close, Value::Int(0), "XCloseDisplay should succeed");
            }
            Ok(Value::Ptr(0)) | Ok(Value::Nil) => {}
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
    use agenterm_dyn::{HostCell, SizeProbe, SystemProbe, SystemProbeStatus};

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    /// Test-only owner for file descriptors opened by this smoke test.
    ///
    /// `openpty` can initialize either descriptor before reporting a failure,
    /// while the ioctl assertions can panic after a successful call.  Scope
    /// ownership covers both cases without ever closing borrowed stdin.
    struct ProbeFd(libc::c_int);

    impl ProbeFd {
        fn as_i64(&self) -> i64 {
            i64::from(self.0)
        }
    }

    impl Drop for ProbeFd {
        fn drop(&mut self) {
            if self.0 >= 0 {
                // SAFETY: the owner is constructed only from descriptors
                // obtained by openpty or open in this test and closes once.
                unsafe {
                    libc::close(self.0);
                }
            }
        }
    }

    fn cell() -> &'static HostCell {
        live_cell().expect("macos cell")
    }

    fn run_isolated_test(child: &str) {
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", child, "--nocapture"])
            .env("AGENTERM_DYN_ISOLATED_CHILD", child)
            .status()
            .expect("spawn isolated smoke child");
        assert!(status.success(), "isolated child {child} failed: {status}");
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

    fn getpid_script() -> String {
        let c = cell();
        format!(
            r#"(dlcall "{}" "{}" "{}")"#,
            c.pid_lib, c.pid_symbol, c.pid_ret_type
        )
    }

    #[test]
    fn dlcall_getpid_matches_libc_and_second_dlcall() {
        let mut env = Dyn::new();
        let script = getpid_script();
        let got = env.eval(&script).expect("getpid dlcall");
        let again = env.eval(&script).expect("second getpid dlcall");
        assert_eq!(got, again);
        let real = unsafe { libc::getpid() };
        assert_eq!(got, Value::Int(i64::from(real)));
    }

    #[test]
    fn missing_symbol_does_not_evict_cached_libsystem() {
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
    fn dlcall_time_secondary_probe() {
        let c = cell();
        let SecondaryProbe::Time { lib, symbol } = c.secondary_probe else {
            panic!("macos secondary should be time()");
        };
        let mut env = Dyn::new();
        let script = format!(r#"(dlcall "{lib}" "{symbol}" "i64" "ptr" 0)"#);
        let got = env.eval(&script).expect("time dlcall");
        let t = got.as_int().expect("time return");
        let real = unsafe { libc::time(std::ptr::null_mut()) };
        assert!(
            (t - real).abs() <= 1,
            "dlcall and libc time should be adjacent"
        );
    }

    #[test]
    fn dlcall_times_writes_caller_owned_tms_and_matches_libc_baseline() {
        let probe = live_system_probe("times");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!()
        };
        let mut dlcall_tms: libc::tms = unsafe { std::mem::zeroed() };
        let mut env = Dyn::new();
        env.bind("tms", (&mut dlcall_tms as *mut libc::tms).cast())
            .expect("bind caller-owned tms");
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i64" "ptr" tms)"#))
            .expect("times dlcall")
            .as_int()
            .expect("times return value");

        let mut libc_tms: libc::tms = unsafe { std::mem::zeroed() };
        let baseline = unsafe { libc::times(&mut libc_tms) };
        assert!(got >= 0, "times dlcall should return elapsed clock ticks");
        assert!(
            baseline as i64 >= got,
            "later libc baseline must not precede dlcall result"
        );
        assert!(
            libc_tms.tms_utime >= dlcall_tms.tms_utime
                && libc_tms.tms_stime >= dlcall_tms.tms_stime
                && libc_tms.tms_cutime >= dlcall_tms.tms_cutime
                && libc_tms.tms_cstime >= dlcall_tms.tms_cstime,
            "times fields must be initialized by dlcall and monotonic at the later libc baseline"
        );
    }

    #[test]
    fn dlcall_getrusage_writes_caller_owned_rusage_and_matches_libc_baseline() {
        let probe = live_system_probe("getrusage");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!()
        };
        let mut dlcall_usage: libc::rusage = unsafe { std::mem::zeroed() };
        let mut env = Dyn::new();
        env.bind("usage", (&mut dlcall_usage as *mut libc::rusage).cast())
            .expect("bind caller-owned rusage");
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {} "ptr" usage)"#,
                libc::RUSAGE_SELF
            ))
            .expect("getrusage dlcall");
        assert_eq!(got, Value::Int(0));

        let mut libc_usage: libc::rusage = unsafe { std::mem::zeroed() };
        let baseline = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut libc_usage) };
        assert_eq!(baseline, 0, "direct libc getrusage baseline");
        assert!(
            timeval_at_most(dlcall_usage.ru_utime, libc_usage.ru_utime)
                && timeval_at_most(dlcall_usage.ru_stime, libc_usage.ru_stime),
            "later direct libc baseline must not precede dlcall CPU usage"
        );
    }

    #[test]
    fn dlcall_getrlimit_nofile_writes_caller_owned_rlimit_and_matches_libc_baseline() {
        let probe = live_system_probe("getrlimit_nofile");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!()
        };
        let mut dlcall_limit: libc::rlimit = unsafe { std::mem::zeroed() };
        let mut env = Dyn::new();
        env.bind("limit", (&mut dlcall_limit as *mut libc::rlimit).cast())
            .expect("bind caller-owned rlimit");
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {} "ptr" limit)"#,
                libc::RLIMIT_NOFILE
            ))
            .expect("getrlimit dlcall");
        assert_eq!(got, Value::Int(0));

        let mut libc_limit: libc::rlimit = unsafe { std::mem::zeroed() };
        let baseline = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut libc_limit) };
        assert_eq!(baseline, 0, "direct libc getrlimit baseline");
        assert_eq!(
            dlcall_limit.rlim_cur, libc_limit.rlim_cur,
            "getrlimit dlcall soft limit must match the direct libc baseline"
        );
        assert_eq!(
            dlcall_limit.rlim_max, libc_limit.rlim_max,
            "getrlimit dlcall hard limit must match the direct libc baseline"
        );
    }

    fn timeval_at_most(left: libc::timeval, right: libc::timeval) -> bool {
        (left.tv_sec, left.tv_usec) <= (right.tv_sec, right.tv_usec)
    }

    #[test]
    fn dlcall_clock_gettime_writes_timespec() {
        let probe = live_system_probe("clock_gettime");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!()
        };
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let mut env = Dyn::new();
        env.bind("ts", (&mut ts as *mut libc::timespec).cast())
            .expect("bind timespec");
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {} "ptr" ts)"#,
                libc::CLOCK_MONOTONIC
            ))
            .expect("clock_gettime dlcall");
        assert_eq!(got, Value::Int(0));
        assert!(ts.tv_sec > 0);
        assert!((0..1_000_000_000).contains(&ts.tv_nsec));
    }

    #[test]
    fn dlcall_uname_writes_darwin_identity() {
        let probe = live_system_probe("uname");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!()
        };
        let mut uts = std::mem::MaybeUninit::<libc::utsname>::zeroed();
        let mut env = Dyn::new();
        env.bind("uts", uts.as_mut_ptr().cast())
            .expect("bind utsname");
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "ptr" uts)"#))
            .expect("uname dlcall");
        assert_eq!(got, Value::Int(0));
        let uts = unsafe { uts.assume_init() };
        let sysname = unsafe { std::ffi::CStr::from_ptr(uts.sysname.as_ptr()) };
        assert_eq!(sysname.to_bytes(), b"Darwin");
    }

    #[test]
    fn dlcall_ids_match_libc() {
        for (name, ret) in [
            ("getuid", "u32"),
            ("getgid", "u32"),
            ("getppid", "i32"),
            ("getpgrp", "i32"),
            ("geteuid", "u32"),
            ("getegid", "u32"),
        ] {
            let probe = live_system_probe(name);
            let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
                unreachable!()
            };
            let mut env = Dyn::new();
            let got = env
                .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "{ret}")"#))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let real = match name {
                "getuid" => i64::from(unsafe { libc::getuid() }),
                "getgid" => i64::from(unsafe { libc::getgid() }),
                "getppid" => i64::from(unsafe { libc::getppid() }),
                "getpgrp" => i64::from(unsafe { libc::getpgrp() }),
                "geteuid" => i64::from(unsafe { libc::geteuid() }),
                "getegid" => i64::from(unsafe { libc::getegid() }),
                _ => unreachable!(),
            };
            assert_eq!(got, Value::Int(real), "{name}");
        }
    }

    #[test]
    fn dlcall_getsid_and_getpgid_zero_match_libc() {
        for name in ["getsid", "getpgid"] {
            let probe = live_system_probe(name);
            let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
                unreachable!()
            };
            let mut env = Dyn::new();
            let got = env
                .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" 0)"#))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let real = if name == "getsid" {
                unsafe { libc::getsid(0) }
            } else {
                unsafe { libc::getpgid(0) }
            };
            assert!(real > 0, "{name} should be positive");
            assert_eq!(got, Value::Int(i64::from(real)), "{name}");
        }
    }

    #[test]
    fn dlcall_sysconf_matches_libc() {
        for (name, key) in [
            ("sysconf_pagesize", libc::_SC_PAGESIZE),
            ("sysconf_clk_tck", libc::_SC_CLK_TCK),
            ("sysconf_nprocessors_onln", libc::_SC_NPROCESSORS_ONLN),
        ] {
            let probe = live_system_probe(name);
            let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
                unreachable!()
            };
            let mut env = Dyn::new();
            let got = env
                .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i64" "i32" {key})"#))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let real = unsafe { libc::sysconf(key) };
            assert!(real > 0, "{name} should be positive");
            assert_eq!(got, Value::Int(real), "{name}");
        }
    }

    #[test]
    fn dlcall_getcwd_writes_current_directory() {
        use std::os::unix::ffi::OsStrExt;
        let probe = live_system_probe("getcwd");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
            unreachable!()
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
    fn dlcall_isatty_std_streams_match_libc() {
        for (name, fd) in [
            ("isatty_stdin", 0),
            ("isatty_stdout", 1),
            ("isatty_stderr", 2),
        ] {
            let probe = live_system_probe(name);
            let SystemProbeStatus::LiveDlcall { lib, symbol } = probe.status else {
                unreachable!()
            };
            let mut env = Dyn::new();
            let got = env
                .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {fd})"#))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let real = unsafe { libc::isatty(fd) };
            assert!(matches!(real, 0 | 1));
            assert_eq!(got, Value::Int(i64::from(real)), "{name}");
        }
    }

    #[test]
    fn dlcall_access_root_and_missing() {
        let root_probe = live_system_probe("access_root");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = root_probe.status else {
            unreachable!()
        };
        let root = CString::new("/").expect("root path");
        let missing = CString::new("/tmp/agenterm-dyn-missing-access-probe").expect("missing path");
        let mut env = Dyn::new();
        env.bind("root", root.as_ptr().cast_mut().cast())
            .expect("bind root");
        env.bind("missing", missing.as_ptr().cast_mut().cast())
            .expect("bind missing");
        let ok = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "ptr" root "i32" {})"#,
                libc::F_OK
            ))
            .expect("access /");
        let miss = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "ptr" missing "i32" {})"#,
                libc::F_OK
            ))
            .expect("access missing");
        assert_eq!(ok, Value::Int(0));
        assert_eq!(miss, Value::Int(-1));
    }

    #[test]
    fn dlcall_dup_lseek_match_libc() {
        let mut env = Dyn::new();
        let dup = live_system_probe("dup_stdin");
        let SystemProbeStatus::LiveDlcall {
            lib,
            symbol: dup_sym,
        } = dup.status
        else {
            unreachable!()
        };
        let fd = env
            .eval(&format!(r#"(dlcall "{lib}" "{dup_sym}" "i32" "i32" 0)"#))
            .expect("dup")
            .as_int()
            .expect("dup int");
        assert!(fd >= 0, "dup(0) returned {fd}");
        let close = env
            .eval(&format!(r#"(dlcall "{lib}" "close" "i32" "i32" {fd})"#))
            .expect("close");
        assert_eq!(close, Value::Int(0));

        let lseek = live_system_probe("lseek_stdin_cur");
        let SystemProbeStatus::LiveDlcall {
            symbol: lseek_sym, ..
        } = lseek.status
        else {
            unreachable!()
        };
        let got_off = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{lseek_sym}" "i64" "i32" 0 "i64" 0 "i32" {})"#,
                libc::SEEK_CUR
            ))
            .expect("lseek");
        assert_eq!(
            got_off,
            Value::Int(unsafe { libc::lseek(0, 0, libc::SEEK_CUR) })
        );
    }

    #[test]
    fn dlcall_priority_nice_yield_alarm_umask() {
        let mut env = Dyn::new();
        let prio = live_system_probe("getpriority_process");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = prio.status else {
            unreachable!()
        };
        let got = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{symbol}" "i32" "u32" {} "u32" 0)"#,
                libc::PRIO_PROCESS
            ))
            .expect("getpriority");
        assert_eq!(
            got,
            Value::Int(i64::from(unsafe {
                libc::getpriority(libc::PRIO_PROCESS, 0)
            }))
        );

        let nice = live_system_probe("nice_zero");
        let SystemProbeStatus::LiveDlcall {
            symbol: nice_sym, ..
        } = nice.status
        else {
            unreachable!()
        };
        let got_nice = env
            .eval(&format!(r#"(dlcall "{lib}" "{nice_sym}" "i32" "i32" 0)"#))
            .expect("nice");
        assert_eq!(got_nice, Value::Int(i64::from(unsafe { libc::nice(0) })));

        let yld = live_system_probe("sched_yield");
        let SystemProbeStatus::LiveDlcall {
            symbol: yld_sym, ..
        } = yld.status
        else {
            unreachable!()
        };
        let yld_direct = unsafe { libc::sched_yield() };
        assert_eq!(yld_direct, 0, "sched_yield direct status");
        assert_eq!(
            env.eval(&format!(r#"(dlcall "{lib}" "{yld_sym}" "i32")"#))
                .expect("sched_yield"),
            Value::Int(i64::from(yld_direct))
        );

        run_isolated_test("macos::dlcall_alarm_zero_child");
        run_isolated_test("macos::dlcall_umask_child");
    }

    #[test]
    fn dlcall_alarm_zero_child() {
        if std::env::var("AGENTERM_DYN_ISOLATED_CHILD").ok().as_deref()
            != Some("macos::dlcall_alarm_zero_child")
        {
            return;
        }
        let alarm = live_system_probe("alarm_zero");
        let SystemProbeStatus::LiveDlcall {
            lib,
            symbol: alarm_sym,
        } = alarm.status
        else {
            unreachable!()
        };
        let prior = unsafe { libc::alarm(0) };
        assert_eq!(
            prior, 0,
            "isolated test process should start without a pending alarm"
        );
        let mut env = Dyn::new();
        let got_alarm = env
            .eval(&format!(r#"(dlcall "{lib}" "{alarm_sym}" "u32" "u32" 0)"#))
            .expect("alarm");
        let remaining = unsafe { libc::alarm(0) };
        assert_eq!(got_alarm, Value::Int(0));
        assert_eq!(remaining, 0);
    }

    #[test]
    fn dlcall_umask_child() {
        if std::env::var("AGENTERM_DYN_ISOLATED_CHILD").ok().as_deref()
            != Some("macos::dlcall_umask_child")
        {
            return;
        }
        let umask = live_system_probe("umask");
        let SystemProbeStatus::LiveDlcall {
            lib,
            symbol: umask_sym,
        } = umask.status
        else {
            unreachable!()
        };
        let mut env = Dyn::new();
        let previous = env
            .eval(&format!(r#"(dlcall "{lib}" "{umask_sym}" "u32" "u32" 0)"#))
            .expect("umask(0)")
            .as_int()
            .expect("umask int");
        let restored = env
            .eval(&format!(
                r#"(dlcall "{lib}" "{umask_sym}" "u32" "u32" {previous})"#
            ))
            .expect("umask restore");
        assert_eq!(restored, Value::Int(0));
        assert_eq!(previous & !0o777, 0);
    }

    #[test]
    fn dlcall_sizes_and_hostid_match_libc() {
        let mut env = Dyn::new();
        let dt = live_system_probe("getdtablesize");
        let SystemProbeStatus::LiveDlcall { lib, symbol } = dt.status else {
            unreachable!()
        };
        let got = env
            .eval(&format!(r#"(dlcall "{lib}" "{symbol}" "i32")"#))
            .expect("getdtablesize");
        let real = unsafe { libc::getdtablesize() };
        assert!(real > 0);
        assert_eq!(got, Value::Int(i64::from(real)));

        let hid = live_system_probe("gethostid");
        let SystemProbeStatus::LiveDlcall {
            symbol: hid_sym, ..
        } = hid.status
        else {
            unreachable!()
        };
        let got_id = env
            .eval(&format!(r#"(dlcall "{lib}" "{hid_sym}" "i64")"#))
            .expect("gethostid");
        assert_eq!(got_id, Value::Int(unsafe { libc::gethostid() }));

        let ps = live_system_probe("getpagesize");
        let SystemProbeStatus::LiveDlcall { symbol: ps_sym, .. } = ps.status else {
            unreachable!()
        };
        let got_ps = env
            .eval(&format!(r#"(dlcall "{lib}" "{ps_sym}" "i32")"#))
            .expect("getpagesize");
        let sysconf = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        assert!(sysconf > 0);
        assert_eq!(got_ps, Value::Int(sysconf));
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
        let (fd, expect_pty_dims) = open_probe_fd();
        let raw_fd = fd.as_ref().map_or(0, ProbeFd::as_i64);
        let script =
            format!(r#"(dlcall "{lib}" "{symbol}" "i32" "i32" {raw_fd} "u64" {request} "ptr" ws)"#);
        let ret = env.eval(&script).expect("ioctl dlcall");
        let code = ret.as_int().expect("ioctl return code");
        // The signature-gated Darwin path calls the loaded `ioctl` symbol
        // through its variadic ABI. An owned pty is live evidence, so it
        // must round-trip both the successful status and its seeded geometry.
        if expect_pty_dims {
            assert_eq!(code, 0, "ioctl on owned pty slave should succeed");
            assert_eq!(ws.ws_row, 24, "pty rows");
            assert_eq!(ws.ws_col, 80, "pty cols");
        } else {
            // If openpty is unavailable, the fallback is an ambient tty (or
            // stdin) whose geometry is not owned by this test. It may work or
            // report the native non-terminal failure, but proves no 24x80 row.
            assert!(
                code == 0 || code == -1,
                "fallback ioctl should return success or native failure; got {code}"
            );
        }
    }

    fn open_probe_fd() -> (Option<ProbeFd>, bool) {
        unsafe {
            let mut master: libc::c_int = -1;
            let mut slave: libc::c_int = -1;
            let mut win = libc::winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let status = libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut win,
            );
            let master = ProbeFd(master);
            let slave = ProbeFd(slave);
            if status == 0 {
                // Darwin TIOCGWINSZ is on the slave; master often returns -1.
                drop(master);
                return (Some(slave), true);
            }
            // Both owners fall out of scope on a partial openpty failure.
        }
        let fd = unsafe { libc::open(b"/dev/tty\0".as_ptr().cast(), libc::O_RDONLY) };
        if fd >= 0 {
            (Some(ProbeFd(fd)), false)
        } else {
            (None, false)
        }
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
