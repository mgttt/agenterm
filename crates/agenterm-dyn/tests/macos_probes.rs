//! Darwin-only live `dlcall` probes for facts absent from Linux host rows.

#![cfg(target_os = "macos")]

use std::ffi::{CStr, CString, c_void};

use agenterm_dyn::{Dyn, SystemProbeStatus, Value, live_cell};

const LIB: &str = "libSystem.B.dylib";

fn eval_native(env: &mut Dyn, source: &str) -> Result<Value, agenterm_dyn::DynError> {
    // SAFETY: each probe documents its C ABI and owns every writable buffer.
    unsafe { env.eval_native(source) }
}

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
    let got = eval_native(
        &mut env,
        &format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" name "ptr" value "ptr" len "ptr" 0 "u64" 0)"#
        ),
    )
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
    let first = eval_native(&mut env, &script).expect("first mach_absolute_time dlcall");
    let second = eval_native(&mut env, &script).expect("second mach_absolute_time dlcall");
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
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
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
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "i32")"#))
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
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" path "ptr" len)"#),
    )
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
    let got = eval_native(
        &mut env,
        &format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "i32" {} "ptr" path "u32" {len})"#,
            pid
        ),
    )
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
        let value = eval_native(&mut env, &script)
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
    let first = eval_native(&mut env, &script).expect("first clock_gettime_nsec_np dlcall");
    let second = eval_native(&mut env, &script).expect("second clock_gettime_nsec_np dlcall");
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
    let got = eval_native(&mut env, &format!(
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
fn dlcall_sysctlnametomib_writes_caller_owned_mib() {
    unsafe extern "C" {
        fn sysctlnametomib(
            name: *const libc::c_char,
            mibp: *mut libc::c_int,
            sizep: *mut usize,
        ) -> libc::c_int;
    }

    let symbol = live_symbol("sysctlnametomib");
    let name = CString::new("hw.ncpu").expect("literal has no NUL");
    let mut mib = [0 as libc::c_int; 8];
    let mut len = mib.len();
    let mut env = Dyn::new();
    env.bind("name", name.as_ptr().cast_mut().cast::<c_void>())
        .expect("bind sysctl name");
    env.bind("mib", mib.as_mut_ptr().cast())
        .expect("bind MIB output");
    env.bind("len", (&mut len as *mut usize).cast())
        .expect("bind MIB output length");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" name "ptr" mib "ptr" len)"#),
    )
    .expect("sysctlnametomib dlcall");
    assert_eq!(got, Value::Int(0));
    assert!((1..=mib.len()).contains(&len), "MIB length must fit output");

    let mut direct = [0 as libc::c_int; 8];
    let mut direct_len = direct.len();
    let direct_status =
        unsafe { sysctlnametomib(name.as_ptr(), direct.as_mut_ptr(), &mut direct_len) };
    assert_eq!(direct_status, 0, "direct sysctlnametomib must succeed");
    assert_eq!(len, direct_len);
    assert_eq!(&mib[..len], &direct[..direct_len]);
}

#[test]
fn dlcall_pthread_equal_recognizes_current_thread() {
    let symbol = live_symbol("pthread_equal");
    let first = unsafe { libc::pthread_self() } as u64;
    let second = unsafe { libc::pthread_self() } as u64;
    let mut env = Dyn::new();
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "u64" {first} "u64" {second})"#),
    )
    .expect("pthread_equal dlcall")
    .as_int()
    .expect("pthread_equal integer result");
    assert_ne!(got, 0, "dlcall must recognize the current thread");
    let direct =
        unsafe { libc::pthread_equal(first as libc::pthread_t, second as libc::pthread_t) };
    assert_ne!(direct, 0, "direct C call must recognize the current thread");
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
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" ratio)"#),
    )
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
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "i32")"#))
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
        let status = eval_native(
            &mut env,
            &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" name "u64" {len})"#),
        )
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
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" 0 "ptr" tid)"#),
    )
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
fn dlcall_pthread_getname_np_matches_libc_current_thread() {
    let symbol = live_symbol("pthread_getname_np");
    let thread = unsafe { libc::pthread_self() };
    let mut name = [0_i8; 64];
    let mut env = Dyn::new();
    env.bind("name", name.as_mut_ptr().cast())
        .expect("bind thread-name buffer");
    let got = eval_native(
        &mut env,
        &format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "u64" {thread} "ptr" name "u64" {})"#,
            name.len()
        ),
    )
    .expect("pthread_getname_np dlcall")
    .as_int()
    .expect("pthread_getname_np integer status") as i32;

    let mut direct = [0_i8; 64];
    let direct_status = unsafe {
        libc::pthread_getname_np(libc::pthread_self(), direct.as_mut_ptr(), direct.len())
    };
    assert_eq!(got, direct_status, "dlcall and direct statuses must agree");
    if got == 0 {
        let got = unsafe { CStr::from_ptr(name.as_ptr()) };
        let direct = unsafe { CStr::from_ptr(direct.as_ptr()) };
        assert_eq!(got.to_bytes(), direct.to_bytes());
    }
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
    let got = eval_native(&mut env, &format!(
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
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
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
fn dlcall_nsget_argv_matches_libc_borrowed_pointer() {
    let symbol = live_symbol("nsget_argv");
    let mut env = Dyn::new();
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
        .expect("_NSGetArgv dlcall")
        .as_ptr()
        .expect("_NSGetArgv pointer") as *mut *mut *mut libc::c_char;
    assert!(
        !got.is_null(),
        "_NSGetArgv must return a non-null outer pointer"
    );
    let direct = unsafe { libc::_NSGetArgv() };
    assert_eq!(got, direct);
    let argv = unsafe { *got };
    assert!(!argv.is_null(), "_NSGetArgv must expose argv storage");
    assert_eq!(argv, unsafe { *direct });
    assert!(!unsafe { *argv }.is_null(), "argv[0] must name the process");
}

#[test]
fn dlcall_nsget_environ_matches_libc_borrowed_pointer() {
    let symbol = live_symbol("nsget_environ");
    let mut env = Dyn::new();
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
        .expect("_NSGetEnviron dlcall")
        .as_ptr()
        .expect("_NSGetEnviron pointer") as *mut *mut *mut libc::c_char;
    assert!(
        !got.is_null(),
        "_NSGetEnviron must return a non-null outer pointer"
    );
    let direct = unsafe { libc::_NSGetEnviron() };
    assert_eq!(got, direct);
    let environ = unsafe { *got };
    assert!(
        !environ.is_null(),
        "_NSGetEnviron must expose environ storage"
    );
    assert_eq!(environ, unsafe { *direct });
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
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "i32" {pid} "i32" {flavor} "ptr" ri)"#),
    )
    .expect("proc_pid_rusage dlcall");
    assert_eq!(got, Value::Int(0));

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
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "u32")"#))
        .expect("_dyld_image_count dlcall")
        .as_int()
        .expect("_dyld_image_count integer");
    assert!(got >= 1, "loaded image count must be at least 1");
    let direct = unsafe { _dyld_image_count() };
    assert!(direct >= 1, "direct image count must be at least 1");
}

#[test]
fn dlcall_getentropy_fills_caller_owned_buffer() {
    const BYTES: usize = 16;

    let symbol = live_symbol("getentropy");
    let mut bytes = [0_u8; BYTES];
    let mut env = Dyn::new();
    env.bind("bytes", bytes.as_mut_ptr().cast())
        .expect("bind entropy output");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" bytes "u64" {BYTES})"#),
    )
    .expect("getentropy dlcall");
    assert_eq!(got, Value::Int(0));

    let mut direct = [0_u8; BYTES];
    let direct_status = unsafe { libc::getentropy(direct.as_mut_ptr().cast(), BYTES) };
    assert_eq!(direct_status, 0, "direct getentropy must succeed");
}

#[test]
fn dlcall_proc_name_matches_direct_c_current_process_name() {
    let symbol = live_symbol("proc_name");
    let pid = unsafe { libc::getpid() };
    let mut name = [0_i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let mut env = Dyn::new();
    env.bind("name", name.as_mut_ptr().cast())
        .expect("bind process-name buffer");
    let got = eval_native(
        &mut env,
        &format!(
            r#"(dlcall "{LIB}" "{symbol}" "i32" "i32" {pid} "ptr" name "u32" {})"#,
            name.len()
        ),
    )
    .expect("proc_name dlcall")
    .as_int()
    .expect("proc_name byte count") as i32;
    assert!(got > 0, "proc_name must write a current-process name");
    let got = unsafe { CStr::from_ptr(name.as_ptr()) };

    let mut direct_name = [0_i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let direct_status = unsafe {
        libc::proc_name(
            pid,
            direct_name.as_mut_ptr().cast(),
            direct_name.len() as u32,
        )
    };
    assert!(
        direct_status > 0,
        "direct proc_name must write a process name"
    );
    let direct = unsafe { CStr::from_ptr(direct_name.as_ptr()) };
    assert_eq!(got.to_bytes(), direct.to_bytes());
}

#[test]
fn dlcall_pthread_get_stackaddr_np_matches_libc_current_thread() {
    let symbol = live_symbol("pthread_get_stackaddr_np");
    let thread = unsafe { libc::pthread_self() } as u64;
    let mut env = Dyn::new();
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr" "u64" {thread})"#),
    )
    .expect("pthread_get_stackaddr_np dlcall")
    .as_ptr()
    .expect("pthread_get_stackaddr_np pointer") as *mut c_void;
    let direct = unsafe { libc::pthread_get_stackaddr_np(libc::pthread_self()) };
    assert!(
        !got.is_null(),
        "current thread stack address must be non-null"
    );
    assert!(
        !direct.is_null(),
        "direct thread stack address must be non-null"
    );
    assert_eq!(got, direct);
}

#[test]
fn dlcall_pthread_get_stacksize_np_matches_libc_current_thread() {
    let symbol = live_symbol("pthread_get_stacksize_np");
    let thread = unsafe { libc::pthread_self() } as u64;
    let mut env = Dyn::new();
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "u64" "u64" {thread})"#),
    )
    .expect("pthread_get_stacksize_np dlcall")
    .as_int()
    .expect("pthread_get_stacksize_np size") as u64;
    let direct = unsafe { libc::pthread_get_stacksize_np(libc::pthread_self()) } as u64;
    assert!(got > 0, "current thread stack size must be positive");
    assert!(direct > 0, "direct thread stack size must be positive");
    assert_eq!(got, direct);
}

#[test]
fn dlcall_pthread_self_matches_libc_current_thread() {
    let symbol = live_symbol("pthread_self");
    let mut env = Dyn::new();
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "u64")"#))
        .expect("pthread_self dlcall")
        .as_int()
        .expect("pthread_self thread handle") as u64;
    let direct = unsafe { libc::pthread_self() } as u64;
    assert_ne!(got, 0, "current pthread handle must be non-zero");
    assert_eq!(got, direct);
}

#[test]
fn dlcall_pthread_cpu_number_np_writes_current_cpu() {
    unsafe extern "C" {
        fn pthread_cpu_number_np(cpu: *mut u32) -> libc::c_int;
    }

    let symbol = live_symbol("pthread_cpu_number_np");
    // The `dlcall` ABI exposes this caller-owned output slot as `u64`; zeroing
    // its upper bytes preserves the C function's `u32` write exactly.
    let mut cpu = 0_u64;
    let mut env = Dyn::new();
    env.bind("cpu", (&mut cpu as *mut u64).cast())
        .expect("bind current CPU output");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" cpu)"#),
    )
    .expect("pthread_cpu_number_np dlcall");
    assert_eq!(got, Value::Int(0));
    let ncpu = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    assert!(ncpu > 0, "online CPU count must be positive");
    assert!(cpu < ncpu as u64, "current CPU must be online");

    let mut direct = 0_u64;
    let direct_status = unsafe { pthread_cpu_number_np((&mut direct as *mut u64).cast()) };
    assert_eq!(
        direct_status, 0,
        "direct pthread_cpu_number_np must succeed"
    );
    assert!(direct < ncpu as u64, "direct current CPU must be online");
}

#[test]
fn dlcall_malloc_good_size_matches_direct_c_for_requests() {
    unsafe extern "C" {
        fn malloc_good_size(size: usize) -> usize;
    }

    let symbol = live_symbol("malloc_good_size");
    for request in [1_u64, 4097] {
        let mut env = Dyn::new();
        let got = eval_native(
            &mut env,
            &format!(r#"(dlcall "{LIB}" "{symbol}" "u64" "u64" {request})"#),
        )
        .expect("malloc_good_size dlcall")
        .as_int()
        .expect("malloc_good_size allocation size") as u64;
        let direct = unsafe { malloc_good_size(request as usize) } as u64;
        assert!(got >= request, "good allocation size must cover request");
        assert_eq!(got, direct);
    }
}

#[test]
fn dlcall_nsget_progname_matches_libc_outer_pointer_and_c_string() {
    let symbol = live_symbol("nsget_progname");
    let mut env = Dyn::new();
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
        .expect("_NSGetProgname dlcall")
        .as_ptr()
        .expect("_NSGetProgname outer pointer") as *mut *mut libc::c_char;
    let direct = unsafe { libc::_NSGetProgname() };
    assert!(
        !got.is_null(),
        "_NSGetProgname must return an outer pointer"
    );
    assert_eq!(got, direct);
    let name = unsafe { *got };
    let direct_name = unsafe { libc::getprogname() };
    assert!(!name.is_null(), "_NSGetProgname must expose a program name");
    assert!(
        !direct_name.is_null(),
        "getprogname must return a program name"
    );
    assert_eq!(
        unsafe { CStr::from_ptr(name) }.to_bytes(),
        unsafe { CStr::from_ptr(direct_name) }.to_bytes()
    );
}

#[test]
fn dlcall_proc_libversion_writes_caller_owned_version() {
    let symbol = live_symbol("proc_libversion");
    let mut major = 0_i32;
    let mut minor = 0_i32;
    let mut env = Dyn::new();
    env.bind("major", (&mut major as *mut i32).cast())
        .expect("bind proc_libversion major output");
    env.bind("minor", (&mut minor as *mut i32).cast())
        .expect("bind proc_libversion minor output");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" major "ptr" minor)"#),
    )
    .expect("proc_libversion dlcall");
    assert_eq!(got, Value::Int(0));
    assert!(major >= 1, "libproc major version must be positive");

    let mut direct_major = 0_i32;
    let mut direct_minor = 0_i32;
    let direct_status = unsafe { libc::proc_libversion(&mut direct_major, &mut direct_minor) };
    assert_eq!(direct_status, 0, "direct proc_libversion must succeed");
    assert_eq!(major, direct_major);
    assert_eq!(minor, direct_minor);
}

#[test]
fn dlcall_pthread_jit_write_protect_supported_np_matches_libc_boolean() {
    let symbol = live_symbol("pthread_jit_write_protect_supported_np");
    let mut env = Dyn::new();
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "i32")"#))
        .expect("pthread_jit_write_protect_supported_np dlcall")
        .as_int()
        .expect("pthread_jit_write_protect_supported_np integer");
    let direct = unsafe { libc::pthread_jit_write_protect_supported_np() };
    assert!(
        matches!(got, 0 | 1),
        "JIT-write-protect support must be boolean"
    );
    assert_eq!(got, i64::from(direct));
}

#[test]
fn dlcall_gethostname_writes_caller_buffer() {
    let symbol = live_symbol("gethostname");
    let mut buffer = [0_u8; 256];
    let len = buffer.len();
    let mut env = Dyn::new();
    env.bind("buf", buffer.as_mut_ptr().cast())
        .expect("bind hostname buffer");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" buf "u64" {len})"#),
    )
    .expect("gethostname dlcall");
    assert_eq!(got, Value::Int(0));
    let name = CStr::from_bytes_until_nul(&buffer)
        .expect("gethostname must NUL-terminate on success")
        .to_bytes();
    assert!(!name.is_empty(), "hostname must be non-empty");

    let mut direct = [0_u8; 256];
    let direct_status = unsafe { libc::gethostname(direct.as_mut_ptr().cast(), direct.len()) };
    assert_eq!(direct_status, 0, "direct gethostname must succeed");
    let direct_name = CStr::from_bytes_until_nul(&direct)
        .expect("direct gethostname must NUL-terminate on success")
        .to_bytes();
    assert_eq!(name, direct_name);
}

#[test]
fn dlcall_confstr_writes_cs_path() {
    let symbol = live_symbol("confstr");
    let name = libc::_CS_PATH;
    let mut buffer = [0_u8; 4096];
    let len = buffer.len();
    let mut env = Dyn::new();
    env.bind("buf", buffer.as_mut_ptr().cast())
        .expect("bind confstr buffer");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "u64" "i32" {name} "ptr" buf "u64" {len})"#),
    )
    .expect("confstr dlcall")
    .as_int()
    .expect("confstr size");
    assert!(got > 1, "confstr(_CS_PATH) must write a non-empty path");

    let mut direct = [0_u8; 4096];
    let direct_len = unsafe { libc::confstr(name, direct.as_mut_ptr().cast(), direct.len()) };
    assert_eq!(got, direct_len as i64);
    assert_eq!(
        CStr::from_bytes_until_nul(&buffer)
            .expect("confstr must NUL-terminate successful output")
            .to_bytes(),
        CStr::from_bytes_until_nul(&direct)
            .expect("direct confstr must NUL-terminate successful output")
            .to_bytes()
    );
}

#[test]
fn dlcall_clock_getres_writes_monotonic_timespec() {
    let symbol = live_symbol("clock_getres");
    let clock = libc::CLOCK_MONOTONIC;
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let mut env = Dyn::new();
    env.bind("ts", (&mut ts as *mut libc::timespec).cast())
        .expect("bind clock_getres timespec");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "i32" {clock} "ptr" ts)"#),
    )
    .expect("clock_getres dlcall");
    assert_eq!(got, Value::Int(0));
    assert!(
        (0..1_000_000_000).contains(&ts.tv_nsec),
        "timespec nsec must be in 0..1e9"
    );

    let mut direct = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let direct_status = unsafe { libc::clock_getres(clock, &mut direct) };
    assert_eq!(direct_status, 0, "direct clock_getres must succeed");
    assert_eq!(ts.tv_sec, direct.tv_sec);
    assert_eq!(ts.tv_nsec, direct.tv_nsec);
}

#[test]
fn dlcall_pthread_is_threaded_np_matches_direct_c() {
    unsafe extern "C" {
        fn pthread_is_threaded_np() -> libc::c_int;
    }

    let symbol = live_symbol("pthread_is_threaded_np");
    let mut env = Dyn::new();
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "i32")"#))
        .expect("pthread_is_threaded_np dlcall")
        .as_int()
        .expect("pthread_is_threaded_np integer");
    assert!(
        matches!(got, 0 | 1),
        "pthread_is_threaded_np must be 0 or 1"
    );
    let direct = unsafe { pthread_is_threaded_np() };
    assert_eq!(got, i64::from(direct));
}

#[test]
fn dlcall_nsget_mach_execute_header_matches_direct_c() {
    unsafe extern "C" {
        fn _NSGetMachExecuteHeader() -> *mut c_void;
    }

    let symbol = live_symbol("nsget_mach_execute_header");
    let mut env = Dyn::new();
    let got = eval_native(&mut env, &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr")"#))
        .expect("_NSGetMachExecuteHeader dlcall")
        .as_ptr()
        .expect("_NSGetMachExecuteHeader pointer") as *mut c_void;
    assert!(
        !got.is_null(),
        "_NSGetMachExecuteHeader must return a non-null header"
    );
    let direct = unsafe { _NSGetMachExecuteHeader() };
    assert_eq!(got, direct);
}

#[test]
fn dlcall_dyld_get_image_name_matches_image_zero() {
    unsafe extern "C" {
        fn _dyld_get_image_name(image_index: u32) -> *const libc::c_char;
    }

    let symbol = live_symbol("dyld_get_image_name");
    let mut env = Dyn::new();
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr" "u32" 0)"#),
    )
    .expect("_dyld_get_image_name dlcall")
    .as_ptr()
    .expect("_dyld_get_image_name pointer") as *const libc::c_char;
    assert!(
        !got.is_null(),
        "_dyld_get_image_name(0) must return a C string"
    );
    let direct = unsafe { _dyld_get_image_name(0) };
    assert!(!direct.is_null(), "direct image-zero name must be non-null");
    assert_eq!(
        unsafe { CStr::from_ptr(got) }.to_bytes(),
        unsafe { CStr::from_ptr(direct) }.to_bytes()
    );
}

#[test]
fn dlcall_dyld_get_image_vmaddr_slide_matches_image_zero() {
    unsafe extern "C" {
        fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
    }

    let symbol = live_symbol("dyld_get_image_vmaddr_slide");
    let mut env = Dyn::new();
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr" "u32" 0)"#),
    )
    .expect("_dyld_get_image_vmaddr_slide dlcall")
    .as_ptr()
    .expect("_dyld_get_image_vmaddr_slide pointer") as *mut c_void;
    let direct = unsafe { _dyld_get_image_vmaddr_slide(0) } as *mut c_void;
    assert_eq!(got, direct);
}

#[test]
fn dlcall_dladdr_writes_caller_owned_info() {
    let symbol = live_symbol("dladdr");
    let addr = libc::getpid as *mut c_void;
    let mut info = unsafe { std::mem::zeroed::<libc::Dl_info>() };
    let mut env = Dyn::new();
    env.bind("addr", addr).expect("bind live function address");
    env.bind("info", (&raw mut info).cast())
        .expect("bind Dl_info output");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" addr "ptr" info)"#),
    )
    .expect("dladdr dlcall")
    .as_int()
    .expect("dladdr integer status");
    assert_ne!(got, 0, "dladdr must resolve a live function address");

    let mut direct = unsafe { std::mem::zeroed::<libc::Dl_info>() };
    let direct_status = unsafe { libc::dladdr(addr, &mut direct) };
    assert_ne!(direct_status, 0, "direct dladdr must succeed");
    assert_eq!(info.dli_saddr, direct.dli_saddr);
    assert_eq!(info.dli_fname.is_null(), direct.dli_fname.is_null());
    if !info.dli_fname.is_null() {
        assert_eq!(
            unsafe { CStr::from_ptr(info.dli_fname) }.to_bytes(),
            unsafe { CStr::from_ptr(direct.dli_fname) }.to_bytes()
        );
    }
}

#[test]
fn dlcall_gethostuuid_writes_caller_owned_uuid() {
    unsafe extern "C" {
        fn gethostuuid(id: *mut u8, wait: *const libc::timespec) -> libc::c_int;
    }

    let symbol = live_symbol("gethostuuid");
    let mut id = [0_u8; 16];
    let wait = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let mut env = Dyn::new();
    env.bind("id", id.as_mut_ptr().cast())
        .expect("bind host uuid output");
    env.bind("wait", (&raw const wait).cast_mut().cast())
        .expect("bind gethostuuid wait timespec");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" id "ptr" wait)"#),
    )
    .expect("gethostuuid dlcall");
    assert_eq!(got, Value::Int(0));

    let mut direct = [0_u8; 16];
    let direct_status = unsafe { gethostuuid(direct.as_mut_ptr(), &wait) };
    assert_eq!(direct_status, 0, "direct gethostuuid must succeed");
    assert_eq!(id, direct);
}

#[test]
fn dlcall_dyld_get_image_header_matches_image_zero() {
    unsafe extern "C" {
        fn _dyld_get_image_header(image_index: u32) -> *const c_void;
    }

    let symbol = live_symbol("dyld_get_image_header");
    let mut env = Dyn::new();
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "ptr" "u32" 0)"#),
    )
    .expect("_dyld_get_image_header dlcall")
    .as_ptr()
    .expect("_dyld_get_image_header pointer") as *const c_void;
    assert!(
        !got.is_null(),
        "_dyld_get_image_header(0) must return a header"
    );
    let direct = unsafe { _dyld_get_image_header(0) };
    assert!(
        !direct.is_null(),
        "direct image-zero header must be non-null"
    );
    assert_eq!(got, direct);
}

#[test]
fn dlcall_arc4random_uniform_respects_each_upper_bound() {
    let symbol = live_symbol("arc4random_uniform");
    let mut env = Dyn::new();
    for bound in [1_u32, 2, 17, u32::MAX] {
        let got = eval_native(
            &mut env,
            &format!(r#"(dlcall "{LIB}" "{symbol}" "u32" "u32" {bound})"#),
        )
        .expect("arc4random_uniform dlcall")
        .as_int()
        .expect("arc4random_uniform integer result");
        assert!(
            (0..i64::from(bound)).contains(&got),
            "arc4random_uniform({bound}) returned {got}"
        );

        let direct = unsafe { libc::arc4random_uniform(bound) };
        assert!(
            direct < bound,
            "direct arc4random_uniform({bound}) returned {direct}"
        );
    }
}

#[test]
fn dlcall_getdomainname_matches_independent_caller_buffer() {
    const BUFFER_BYTES: usize = 256;
    let symbol = live_symbol("getdomainname");
    let length = libc::c_int::try_from(BUFFER_BYTES).expect("domain buffer fits c_int");
    let mut domain = [0_u8; BUFFER_BYTES];
    let mut env = Dyn::new();
    env.bind("domain", domain.as_mut_ptr().cast())
        .expect("bind domain-name output");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" domain "i32" {length})"#),
    )
    .expect("getdomainname dlcall");
    assert_eq!(got, Value::Int(0));
    let domain = CStr::from_bytes_until_nul(&domain)
        .expect("successful getdomainname must NUL-terminate its bounded output");

    let mut direct = [0_u8; BUFFER_BYTES];
    let direct_status =
        unsafe { libc::getdomainname(direct.as_mut_ptr().cast::<libc::c_char>(), length) };
    assert_eq!(direct_status, 0, "direct getdomainname must succeed");
    let direct = CStr::from_bytes_until_nul(&direct)
        .expect("direct getdomainname must NUL-terminate its bounded output");
    assert_eq!(domain.to_bytes(), direct.to_bytes());
}

#[test]
fn dlcall_statvfs_matches_stable_root_filesystem_fields() {
    let symbol = live_symbol("statvfs");
    let root = CString::new("/").expect("root path literal has no NUL");
    let mut info = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let mut env = Dyn::new();
    env.bind("root", root.as_ptr().cast_mut().cast())
        .expect("bind root path");
    env.bind("info", info.as_mut_ptr().cast())
        .expect("bind statvfs output");
    let got = eval_native(
        &mut env,
        &format!(r#"(dlcall "{LIB}" "{symbol}" "i32" "ptr" root "ptr" info)"#),
    )
    .expect("statvfs dlcall");
    assert_eq!(got, Value::Int(0));
    let info = unsafe { info.assume_init() };
    assert!(info.f_bsize > 0, "filesystem block size must be positive");
    assert!(
        info.f_frsize > 0,
        "filesystem fragment size must be positive"
    );
    assert!(info.f_namemax > 0, "filesystem name limit must be positive");

    let mut direct = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let direct_status = unsafe { libc::statvfs(root.as_ptr(), direct.as_mut_ptr()) };
    assert_eq!(direct_status, 0, "direct statvfs must succeed");
    let direct = unsafe { direct.assume_init() };
    assert_eq!(info.f_bsize, direct.f_bsize);
    assert_eq!(info.f_frsize, direct.f_frsize);
    assert_eq!(info.f_namemax, direct.f_namemax);
}
