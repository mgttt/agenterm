//! Darwin-only live `dlcall` probes for facts absent from Linux host rows.

#![cfg(target_os = "macos")]

use std::ffi::{c_void, CStr, CString};

use agenterm_dyn::{live_cell, Dyn, SystemProbeStatus, Value};

const LIB: &str = "libSystem.B.dylib";

fn live_symbol(name: &str) -> &'static str {
    let probe = live_cell()
        .expect("macOS host cell")
        .system_probes
        .iter()
        .find(|probe| probe.name == name)
        .expect("Darwin probe is catalogued");
    match probe.status {
        SystemProbeStatus::LiveDlcall { lib: LIB, symbol } => symbol,
        other => panic!("{name} must be a live libSystem probe, got {other:?}"),
    }
}

#[test]
fn dlcall_sysctlbyname_writes_ncpu_into_caller_buffer() {
    let symbol = live_symbol("sysctlbyname");
    let name = CString::new("hw.ncpu").expect("literal has no NUL");
    let mut ncpu: libc::c_uint = 0;
    let mut len = std::mem::size_of_val(&ncpu);
    let mut env = Dyn::new();
    env.bind("name", name.as_ptr().cast_mut().cast::<c_void>())
        .expect("bind sysctl name");
    env.bind("value", (&mut ncpu as *mut libc::c_uint).cast())
        .expect("bind CPU output");
    env.bind("len", (&mut len as *mut usize).cast())
        .expect("bind CPU output length");
    let got = env
        .eval(&format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" name "ptr" value "ptr" len "ptr" 0 "u64" 0)"#
        ))
        .expect("sysctlbyname dlcall");
    assert_eq!(got, Value::Int(0));
    assert_eq!(len, std::mem::size_of_val(&ncpu));
    assert!(ncpu >= 1, "hw.ncpu must be positive");

    let mut direct: libc::c_uint = 0;
    let mut direct_len = std::mem::size_of_val(&direct);
    let direct_status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut direct as *mut libc::c_uint).cast(),
            &mut direct_len,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_eq!(direct_status, 0, "direct sysctlbyname must succeed");
    assert_eq!(direct_len, std::mem::size_of_val(&direct));
    assert_eq!(ncpu, direct);
    let available = std::thread::available_parallelism()
        .expect("host exposes available parallelism")
        .get();
    assert!(
        ncpu as usize >= available,
        "kernel CPU count must cover process availability"
    );
}

#[test]
fn dlcall_mach_absolute_time_is_monotonic_against_libc() {
    let symbol = live_symbol("mach_absolute_time");
    let mut env = Dyn::new();
    let script = format!(r#"(dlcall "{LIB}" "{symbol}" "i64")"#);
    let first = env.eval(&script).expect("first mach_absolute_time dlcall");
    let second = env.eval(&script).expect("second mach_absolute_time dlcall");
    let first = first.as_int().expect("integer tick result") as u64;
    let second = second.as_int().expect("integer tick result") as u64;
    let direct = unsafe { libc::mach_absolute_time() };
    assert!(second >= first, "later dlcall tick must not precede first");
    assert!(direct >= second, "later libc tick must not precede dlcall");
}

#[test]
fn dlcall_getprogname_matches_libc_c_string() {
    let symbol = live_symbol("getprogname");
    let mut env = Dyn::new();
    let got = env
        .eval(&format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
        .expect("getprogname dlcall")
        .as_ptr()
        .expect("program name pointer") as *const libc::c_char;
    let direct = unsafe { libc::getprogname() };
    assert!(!got.is_null(), "dlcall must return a program-name pointer");
    assert!(!direct.is_null(), "libc must return a program-name pointer");
    let got = unsafe { CStr::from_ptr(got) };
    let direct = unsafe { CStr::from_ptr(direct) };
    assert_eq!(got.to_bytes(), direct.to_bytes());
}

#[test]
fn dlcall_issetugid_matches_libc_boolean() {
    let symbol = live_symbol("issetugid");
    let mut env = Dyn::new();
    let got = env
        .eval(&format!(r#"(dlcall "{LIB}" "{symbol}" "i32")"#))
        .expect("issetugid dlcall")
        .as_int()
        .expect("issetugid integer");
    let direct = unsafe { libc::issetugid() };
    assert!(matches!(got, 0 | 1), "issetugid must be boolean");
    assert_eq!(got, i64::from(direct));
}

#[test]
fn dlcall_nsget_executable_path_writes_a_caller_buffer() {
    let symbol = live_symbol("nsget_executable_path");
    let mut buffer = vec![0_u8; 4096];
    let mut length = u32::try_from(buffer.len()).expect("test buffer fits u32");
    let mut env = Dyn::new();
    env.bind("path", buffer.as_mut_ptr().cast())
        .expect("bind executable-path buffer");
    env.bind("len", (&mut length as *mut u32).cast())
        .expect("bind executable-path length");
    let got = env
        .eval(&format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" path "ptr" len)"#
        ))
        .expect("_NSGetExecutablePath dlcall");
    assert_eq!(got, Value::Int(0));
    let path = CStr::from_bytes_until_nul(&buffer)
        .expect("_NSGetExecutablePath must NUL-terminate on success")
        .to_bytes();
    assert!(!path.is_empty(), "executable path must be non-empty");
    let current = std::env::current_exe().expect("current executable path");
    let current = current.as_os_str().as_encoded_bytes();
    assert!(
        path.starts_with(current) || current.starts_with(path),
        "_NSGetExecutablePath and current_exe must identify the executable"
    );
}

#[test]
fn dlcall_proc_pidpath_writes_a_caller_buffer() {
    let symbol = live_symbol("proc_pidpath");
    let mut buffer = vec![0_u8; 4096];
    let len = u32::try_from(buffer.len()).expect("test buffer fits u32");
    let pid = unsafe { libc::getpid() };
    let mut env = Dyn::new();
    env.bind("path", buffer.as_mut_ptr().cast())
        .expect("bind proc_pidpath buffer");
    let got = env
        .eval(&format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "i32" {} "ptr" path "u32" {len})"#,
            pid
        ))
        .expect("proc_pidpath dlcall")
        .as_int()
        .expect("proc_pidpath integer result");
    assert!(got > 0, "proc_pidpath must write at least one byte");
    let path = CStr::from_bytes_until_nul(&buffer)
        .expect("proc_pidpath must NUL-terminate its successful output")
        .to_bytes();
    assert!(!path.is_empty(), "proc_pidpath path must be non-empty");
}

#[test]
fn dlcall_arc4random_returns_u32_values() {
    let symbol = live_symbol("arc4random");
    let mut env = Dyn::new();
    let script = format!(r#"(dlcall "{LIB}" "{symbol}" "u32")"#);
    for _ in 0..2 {
        let value = env
            .eval(&script)
            .expect("arc4random dlcall")
            .as_int()
            .expect("arc4random integer result");
        assert!((0..=i64::from(u32::MAX)).contains(&value));
    }
}

#[test]
fn dlcall_clock_gettime_nsec_np_is_monotonic_against_libc() {
    let symbol = live_symbol("clock_gettime_nsec_np");
    let clock = i64::from(libc::CLOCK_UPTIME_RAW);
    let mut env = Dyn::new();
    let script = format!(r#"(dlcall "{LIB}" "{symbol}" "u64" "i32" {clock})"#);
    let first = env
        .eval(&script)
        .expect("first clock_gettime_nsec_np dlcall");
    let second = env
        .eval(&script)
        .expect("second clock_gettime_nsec_np dlcall");
    let first = first.as_int().expect("integer nsec result") as u64;
    let second = second.as_int().expect("integer nsec result") as u64;
    // libc 0.2 does not bind clock_gettime_nsec_np; call the same Darwin symbol.
    unsafe extern "C" {
        fn clock_gettime_nsec_np(clock_id: libc::clockid_t) -> u64;
    }
    let direct = unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) };
    assert!(second >= first, "later dlcall tick must not precede first");
    assert!(
        direct >= second,
        "later libc tick must not precede last dlcall"
    );
}

#[test]
fn dlcall_sysctl_writes_ncpu_into_caller_buffer() {
    let symbol = live_symbol("sysctl");
    let mut mib = [libc::CTL_HW, libc::HW_NCPU];
    let mut ncpu: i32 = 0;
    let mut oldlen = std::mem::size_of_val(&ncpu);
    let mut env = Dyn::new();
    env.bind("mib", mib.as_mut_ptr().cast())
        .expect("bind sysctl mib");
    env.bind("oldp", (&mut ncpu as *mut i32).cast())
        .expect("bind ncpu output");
    env.bind("oldlenp", (&mut oldlen as *mut usize).cast())
        .expect("bind ncpu output length");
    let got = env
        .eval(&format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" mib "u32" 2 "ptr" oldp "ptr" oldlenp "ptr" 0 "u64" 0)"#
        ))
        .expect("sysctl dlcall");
    assert_eq!(got, Value::Int(0));
    assert!(ncpu >= 1, "hw.ncpu must be at least 1");

    let mut direct: i32 = 0;
    let mut direct_len = std::mem::size_of_val(&direct);
    let mut direct_mib = [libc::CTL_HW, libc::HW_NCPU];
    let direct_status = unsafe {
        libc::sysctl(
            direct_mib.as_mut_ptr(),
            2,
            (&mut direct as *mut i32).cast(),
            &mut direct_len,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_eq!(direct_status, 0, "direct sysctl must succeed");
    assert_eq!(ncpu, direct);
}

#[test]
fn dlcall_mach_timebase_info_writes_caller_owned_ratio() {
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }

    let symbol = live_symbol("mach_timebase_info");
    let mut ratio = Timebase { numer: 0, denom: 0 };
    let mut env = Dyn::new();
    env.bind("ratio", (&mut ratio as *mut Timebase).cast())
        .expect("bind timebase output");
    let got = env
        .eval(&format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" ratio)"#))
        .expect("mach_timebase_info dlcall");
    assert_eq!(got, Value::Int(0));
    assert!(ratio.numer > 0, "timebase numerator must be positive");
    assert!(ratio.denom > 0, "timebase denominator must be positive");

    let mut direct = Timebase { numer: 0, denom: 0 };
    let direct_status = unsafe { mach_timebase_info(&mut direct) };
    assert_eq!(direct_status, 0, "direct mach_timebase_info must succeed");
    assert_eq!(ratio.numer, direct.numer);
    assert_eq!(ratio.denom, direct.denom);
}

#[test]
fn dlcall_pthread_main_np_matches_libc() {
    let symbol = live_symbol("pthread_main_np");
    let mut env = Dyn::new();
    let got = env
        .eval(&format!(r#"(dlcall "{LIB}" "{symbol}" "i32")"#))
        .expect("pthread_main_np dlcall")
        .as_int()
        .expect("pthread_main_np integer");
    let direct = unsafe { libc::pthread_main_np() };
    assert!(matches!(got, 0 | 1), "pthread_main_np must be boolean");
    assert_eq!(got, i64::from(direct));
}

#[test]
fn dlcall_getlogin_r_matches_direct_c_buffer() {
    unsafe extern "C" {
        fn getlogin_r(name: *mut libc::c_char, name_len: usize) -> libc::c_int;
    }

    let symbol = live_symbol("getlogin_r");
    let mut len = 256_usize;
    let (got_status, got_buffer) = loop {
        let mut buffer = vec![0_u8; len];
        let mut env = Dyn::new();
        env.bind("name", buffer.as_mut_ptr().cast())
            .expect("bind login output");
        let status = env
            .eval(&format!(
                r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" name "u64" {len})"#
            ))
            .expect("getlogin_r dlcall")
            .as_int()
            .expect("getlogin_r integer status");
        if status == i64::from(libc::ERANGE) {
            assert_eq!(len, 256, "only the initial buffer may be too small");
            len = 1024;
            continue;
        }
        break (status, buffer);
    };
    let mut direct_buffer = vec![0_u8; len];
    let direct_status = unsafe { getlogin_r(direct_buffer.as_mut_ptr().cast(), len) };
    assert_eq!(
        got_status,
        i64::from(direct_status),
        "dlcall and direct getlogin_r must return the same status for length {len}"
    );
    if got_status == 0 {
        let got = CStr::from_bytes_until_nul(&got_buffer)
            .expect("getlogin_r must NUL-terminate successful output");
        let direct = CStr::from_bytes_until_nul(&direct_buffer)
            .expect("direct getlogin_r must NUL-terminate successful output");
        assert!(!got.to_bytes().is_empty(), "login name must be non-empty");
        assert_eq!(got.to_bytes(), direct.to_bytes());
    }
}

#[test]
fn dlcall_pthread_threadid_np_matches_libc_current_thread() {
    let symbol = live_symbol("pthread_threadid_np");
    let mut tid: u64 = 0;
    let mut env = Dyn::new();
    env.bind("tid", (&mut tid as *mut u64).cast())
        .expect("bind thread-id output");
    let got = env
        .eval(&format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" 0 "ptr" tid)"#
        ))
        .expect("pthread_threadid_np dlcall");
    assert_eq!(got, Value::Int(0));
    assert_ne!(tid, 0, "current thread id must be non-zero");

    let mut direct: u64 = 0;
    // Darwin pthread_t is usize; a typed null pointer does not coerce.
    let direct_status = unsafe { libc::pthread_threadid_np(0, &mut direct) };
    assert_eq!(direct_status, 0, "direct pthread_threadid_np must succeed");
    assert_eq!(tid, direct);
}

#[test]
fn dlcall_proc_pidinfo_writes_caller_owned_bsdinfo() {
    let symbol = live_symbol("proc_pidinfo");
    let pid = unsafe { libc::getpid() };
    let ppid = unsafe { libc::getppid() };
    let flavor = libc::PROC_PIDTBSDINFO;
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let bufsize =
        i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).expect("struct fits i32");
    let mut env = Dyn::new();
    env.bind("info", (&raw mut info).cast())
        .expect("bind proc_bsdinfo");
    let got = env
        .eval(&format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "i32" {pid} "i32" {flavor} "u64" 0 "ptr" info "i32" {bufsize})"#
        ))
        .expect("proc_pidinfo dlcall")
        .as_int()
        .expect("proc_pidinfo byte count");
    assert_eq!(got, i64::from(bufsize));
    assert_eq!(info.pbi_pid, pid as u32);
    assert_eq!(info.pbi_ppid, ppid as u32);

    let mut direct = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let direct_bytes =
        unsafe { libc::proc_pidinfo(pid, flavor, 0, (&raw mut direct).cast(), bufsize) };
    assert_eq!(
        direct_bytes, bufsize,
        "direct proc_pidinfo must fill struct"
    );
    assert_eq!(info.pbi_pid, direct.pbi_pid);
    assert_eq!(info.pbi_ppid, direct.pbi_ppid);
}

#[test]
fn dlcall_nsget_argc_matches_libc_pointer_and_count() {
    let symbol = live_symbol("nsget_argc");
    let mut env = Dyn::new();
    let got = env
        .eval(&format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
        .expect("_NSGetArgc dlcall")
        .as_ptr()
        .expect("_NSGetArgc pointer") as *mut i32;
    assert!(
        !got.is_null(),
        "_NSGetArgc must return a non-null int pointer"
    );
    let argc = unsafe { *got };
    assert!(argc >= 1, "process argc must be at least 1");
    let direct = unsafe { libc::_NSGetArgc() };
    assert_eq!(got, direct);
    assert_eq!(argc, unsafe { *direct });
}

#[test]
fn dlcall_proc_pid_rusage_writes_caller_owned_v4() {
    let symbol = live_symbol("proc_pid_rusage");
    let pid = unsafe { libc::getpid() };
    let flavor = libc::RUSAGE_INFO_V4;
    let mut ri = unsafe { std::mem::zeroed::<libc::rusage_info_v4>() };
    let mut env = Dyn::new();
    env.bind("ri", (&raw mut ri).cast())
        .expect("bind rusage_info_v4");
    let got = env
        .eval(&format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "i32" {pid} "i32" {flavor} "ptr" ri)"#
        ))
        .expect("proc_pid_rusage dlcall");
    assert_eq!(got, Value::Int(0));
    assert_eq!(
        ri.ri_proc_exit_abstime, 0,
        "live process must not have an exit time"
    );
    assert!(
        ri.ri_wired_size > 0 || ri.ri_resident_size > 0 || ri.ri_phys_footprint > 0,
        "at least one size field must be positive"
    );

    let mut direct = unsafe { std::mem::zeroed::<libc::rusage_info_v4>() };
    let direct_status = unsafe {
        libc::proc_pid_rusage(pid, flavor, (&raw mut direct).cast::<libc::rusage_info_t>())
    };
    assert_eq!(direct_status, 0, "direct proc_pid_rusage must succeed");
    assert_eq!(ri.ri_uuid, direct.ri_uuid);
    assert_eq!(ri.ri_proc_start_abstime, direct.ri_proc_start_abstime);
}

#[test]
fn dlcall_dyld_image_count_matches_direct_c() {
    unsafe extern "C" {
        fn _dyld_image_count() -> u32;
    }

    let symbol = live_symbol("dyld_image_count");
    let mut env = Dyn::new();
    let got = env
        .eval(&format!(r#"(dlcall "{LIB}" "{symbol}" "u32")"#))
        .expect("_dyld_image_count dlcall")
        .as_int()
        .expect("_dyld_image_count integer");
    assert!(got >= 1, "loaded image count must be at least 1");
    let direct = unsafe { _dyld_image_count() };
    assert_eq!(got, i64::from(direct));
}
