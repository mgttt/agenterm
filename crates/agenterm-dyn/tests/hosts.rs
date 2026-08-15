//! Host-table matrix tests — all six ISA×OS cells exist as explicit data.

use std::collections::HashSet;

use agenterm_dyn::{
    ALL_CELLS, HostCell, LINUX_AARCH64, LINUX_X86_64, MACOS_AARCH64, MACOS_X86_64, SecondaryProbe,
    SizeProbe, SystemProbeStatus, WINDOWS_AARCH64, WINDOWS_X86_64, cell, live_cell,
};

#[test]
fn platform_candidates_index_lists_host_table_items() {
    use agenterm_dyn::PLATFORM_CANDIDATES;
    assert!(PLATFORM_CANDIDATES.contains(&"HostCell"));
    assert!(PLATFORM_CANDIDATES.contains(&"SystemProbe"));
    assert!(PLATFORM_CANDIDATES.contains(&"ALL_CELLS"));
    assert!(PLATFORM_CANDIDATES.contains(&"live_cell"));
    assert!(PLATFORM_CANDIDATES.contains(&"LINUX_X86_64"));
    assert!(PLATFORM_CANDIDATES.contains(&"WINDOWS_AARCH64"));
    assert!(PLATFORM_CANDIDATES.contains(&"CU_ADJACENT_PROBE_CATALOG"));
}

#[test]
fn all_six_cells_present() {
    assert_eq!(ALL_CELLS.len(), 6);
    let keys: Vec<_> = ALL_CELLS.iter().map(|c| (c.os, c.arch)).collect();
    assert!(keys.contains(&("linux", "x86_64")));
    assert!(keys.contains(&("linux", "aarch64")));
    assert!(keys.contains(&("macos", "x86_64")));
    assert!(keys.contains(&("macos", "aarch64")));
    assert!(keys.contains(&("windows", "x86_64")));
    assert!(keys.contains(&("windows", "aarch64")));
}

#[test]
fn cell_lookup_by_name() {
    assert_eq!(cell("linux", "x86_64"), Some(&LINUX_X86_64));
    assert_eq!(cell("linux", "aarch64"), Some(&LINUX_AARCH64));
    assert_eq!(cell("macos", "x86_64"), Some(&MACOS_X86_64));
    assert_eq!(cell("macos", "aarch64"), Some(&MACOS_AARCH64));
    assert_eq!(cell("windows", "x86_64"), Some(&WINDOWS_X86_64));
    assert_eq!(cell("windows", "aarch64"), Some(&WINDOWS_AARCH64));
    assert_eq!(cell("freebsd", "x86_64"), None);
}

#[test]
fn linux_cells_share_libc_names() {
    for c in [LINUX_X86_64, LINUX_AARCH64] {
        assert_eq!(c.pid_lib, "libc.so.6");
        assert!(matches!(
            c.size_probe,
            SizeProbe::IoctlTiocgwinsz {
                lib: "libc.so.6",
                ..
            }
        ));
    }
}

#[test]
fn macos_cells_share_libsystem_names() {
    for c in [MACOS_X86_64, MACOS_AARCH64] {
        assert_eq!(c.pid_lib, "libSystem.B.dylib");
        assert!(matches!(
            c.secondary_probe,
            SecondaryProbe::Time {
                lib: "libSystem.B.dylib",
                ..
            }
        ));
    }
}

#[test]
fn windows_cells_share_kernel32_names() {
    for c in [WINDOWS_X86_64, WINDOWS_AARCH64] {
        assert_eq!(c.pid_lib, "kernel32.dll");
        assert!(matches!(
            c.size_probe,
            SizeProbe::GetConsoleScreenBufferInfo {
                lib: "kernel32.dll",
                ..
            }
        ));
    }
}

#[test]
fn system_probe_catalog_names_are_ordered_and_unique_across_all_cells() {
    let canonical = LINUX_X86_64
        .system_probes
        .iter()
        .map(|probe| probe.name)
        .collect::<Vec<_>>();
    let canonical_unique = canonical.iter().copied().collect::<HashSet<_>>();
    assert_eq!(canonical_unique.len(), canonical.len());

    for cell in [
        LINUX_AARCH64,
        MACOS_X86_64,
        MACOS_AARCH64,
        WINDOWS_X86_64,
        WINDOWS_AARCH64,
    ] {
        assert_eq!(cell.system_probes.len(), canonical.len());
        let unique = cell
            .system_probes
            .iter()
            .map(|probe| probe.name)
            .collect::<HashSet<_>>();
        assert_eq!(unique.len(), cell.system_probes.len());
        for (index, (expected, actual)) in canonical
            .iter()
            .zip(cell.system_probes.iter().map(|probe| probe.name))
            .enumerate()
        {
            assert_eq!(
                actual, *expected,
                "{} {} system probe {index} must match Linux x86_64 catalog order",
                cell.os, cell.arch
            );
        }
    }
}

#[test]
fn additional_system_probes_use_explicit_live_and_placeholder_statuses() {
    for c in [LINUX_X86_64, LINUX_AARCH64] {
        let sysctlbyname = system_probe_index(c, "sysctlbyname");
        assert_eq!(
            c.system_probes.map(|probe| probe.name),
            [
                "time",
                "times",
                "getrusage",
                "getrlimit_nofile",
                "clock_gettime",
                "uname",
                "getuid",
                "getgid",
                "getppid",
                "getpgrp",
                "getsid",
                "getpgid",
                "geteuid",
                "getegid",
                "sysconf_pagesize",
                "sysconf_clk_tck",
                "sysconf_nprocessors_onln",
                "getcwd",
                "isatty_stdin",
                "open_dev_null",
                "access_root",
                "access_missing",
                "fcntl_stdin_getfd",
                "dup_stdin",
                "getpriority_process",
                "nice_zero",
                "lseek_stdin_cur",
                "fcntl_stdin_getfl",
                "isatty_stdout",
                "isatty_stderr",
                "sched_yield_void",
                "alarm_zero",
                "umask",
                "getdtablesize",
                "gethostid",
                "getpagesize",
                "sysctlbyname",
                "mach_absolute_time",
                "getprogname",
                "issetugid",
                "nsget_executable_path",
                "proc_pidpath",
                "arc4random",
                "clock_gettime_nsec_np",
                "sysctl",
                "mach_timebase_info",
                "pthread_main_np",
                "getlogin_r",
                "pthread_threadid_np",
                "pthread_getname_np",
                "proc_pidinfo",
                "nsget_argc",
                "nsget_argv",
                "nsget_environ",
                "proc_pid_rusage",
                "dyld_image_count",
                "getentropy",
                "proc_name",
                "pthread_get_stackaddr_np",
                "pthread_get_stacksize_np",
                "pthread_self",
                "pthread_cpu_number_np",
                "malloc_good_size",
                "nsget_progname",
                "proc_libversion",
                "pthread_jit_write_protect_supported_np",
                "sysctlnametomib",
                "pthread_equal",
                "mach_host_self",
            ]
        );
        assert_eq!(
            c.system_probes[..sysctlbyname]
                .iter()
                .filter(|probe| {
                    !matches!(
                        probe.name,
                        "open_dev_null" | "fcntl_stdin_getfd" | "fcntl_stdin_getfl"
                    )
                })
                .map(|probe| match probe.status {
                    SystemProbeStatus::LiveDlcall { lib, symbol } => (probe.name, lib, symbol),
                    SystemProbeStatus::Placeholder => panic!("Linux probe must be live"),
                })
                .collect::<Vec<_>>(),
            [
                ("time", "libc.so.6", "time"),
                ("times", "libc.so.6", "times"),
                ("getrusage", "libc.so.6", "getrusage"),
                ("getrlimit_nofile", "libc.so.6", "getrlimit"),
                ("clock_gettime", "libc.so.6", "clock_gettime"),
                ("uname", "libc.so.6", "uname"),
                ("getuid", "libc.so.6", "getuid"),
                ("getgid", "libc.so.6", "getgid"),
                ("getppid", "libc.so.6", "getppid"),
                ("getpgrp", "libc.so.6", "getpgrp"),
                ("getsid", "libc.so.6", "getsid"),
                ("getpgid", "libc.so.6", "getpgid"),
                ("geteuid", "libc.so.6", "geteuid"),
                ("getegid", "libc.so.6", "getegid"),
                ("sysconf_pagesize", "libc.so.6", "sysconf"),
                ("sysconf_clk_tck", "libc.so.6", "sysconf"),
                ("sysconf_nprocessors_onln", "libc.so.6", "sysconf"),
                ("getcwd", "libc.so.6", "getcwd"),
                ("isatty_stdin", "libc.so.6", "isatty"),
                ("access_root", "libc.so.6", "access"),
                ("access_missing", "libc.so.6", "access"),
                ("dup_stdin", "libc.so.6", "dup"),
                ("getpriority_process", "libc.so.6", "getpriority"),
                ("nice_zero", "libc.so.6", "nice"),
                ("lseek_stdin_cur", "libc.so.6", "lseek"),
                ("isatty_stdout", "libc.so.6", "isatty"),
                ("isatty_stderr", "libc.so.6", "isatty"),
                ("sched_yield_void", "libc.so.6", "sched_yield"),
                ("alarm_zero", "libc.so.6", "alarm"),
                ("umask", "libc.so.6", "umask"),
                ("getdtablesize", "libc.so.6", "getdtablesize"),
                ("gethostid", "libc.so.6", "gethostid"),
                ("getpagesize", "libc.so.6", "getpagesize"),
            ]
        );
        for name in ["open_dev_null", "fcntl_stdin_getfd", "fcntl_stdin_getfl"] {
            assert!(matches!(
                c.system_probes
                    .iter()
                    .find(|probe| probe.name == name)
                    .expect("variadic probe is catalogued")
                    .status,
                SystemProbeStatus::Placeholder
            ));
        }
        assert!(
            c.system_probes[sysctlbyname..]
                .iter()
                .all(|probe| matches!(probe.status, SystemProbeStatus::Placeholder))
        );
    }
    for c in [MACOS_X86_64, MACOS_AARCH64] {
        let sysctlbyname = system_probe_index(c, "sysctlbyname");
        let mach_host_self = system_probe_index(c, "mach_host_self");
        assert!(c.system_probes[..sysctlbyname].iter().all(|probe| {
            matches!(
                probe.name,
                "open_dev_null" | "fcntl_stdin_getfd" | "fcntl_stdin_getfl"
            ) || matches!(
                probe.status,
                SystemProbeStatus::LiveDlcall {
                    lib: "libSystem.B.dylib",
                    ..
                }
            )
        }));
        for name in ["open_dev_null", "fcntl_stdin_getfd", "fcntl_stdin_getfl"] {
            assert!(matches!(
                c.system_probes
                    .iter()
                    .find(|probe| probe.name == name)
                    .expect("variadic probe is catalogued")
                    .status,
                SystemProbeStatus::Placeholder
            ));
        }
        assert_eq!(
            c.system_probes[sysctlbyname..]
                .iter()
                .map(|probe| probe.name)
                .collect::<Vec<_>>(),
            [
                "sysctlbyname",
                "mach_absolute_time",
                "getprogname",
                "issetugid",
                "nsget_executable_path",
                "proc_pidpath",
                "arc4random",
                "clock_gettime_nsec_np",
                "sysctl",
                "mach_timebase_info",
                "pthread_main_np",
                "getlogin_r",
                "pthread_threadid_np",
                "pthread_getname_np",
                "proc_pidinfo",
                "nsget_argc",
                "nsget_argv",
                "nsget_environ",
                "proc_pid_rusage",
                "dyld_image_count",
                "getentropy",
                "proc_name",
                "pthread_get_stackaddr_np",
                "pthread_get_stacksize_np",
                "pthread_self",
                "pthread_cpu_number_np",
                "malloc_good_size",
                "nsget_progname",
                "proc_libversion",
                "pthread_jit_write_protect_supported_np",
                "sysctlnametomib",
                "pthread_equal",
                "mach_host_self",
            ]
        );
        assert_eq!(mach_host_self + 1, c.system_probes.len());
        assert!(
            c.system_probes[sysctlbyname..mach_host_self]
                .iter()
                .all(|probe| matches!(
                    probe.status,
                    SystemProbeStatus::LiveDlcall {
                        lib: "libSystem.B.dylib",
                        ..
                    }
                ))
        );
        assert!(matches!(
            c.system_probes[mach_host_self].status,
            SystemProbeStatus::Placeholder
        ));
    }
    for c in [WINDOWS_X86_64, WINDOWS_AARCH64] {
        assert!(
            c.system_probes
                .iter()
                .all(|probe| matches!(probe.status, SystemProbeStatus::Placeholder))
        );
    }
}

#[test]
fn darwin_system_probe_symbols_preserve_exact_c_spellings() {
    for c in [MACOS_X86_64, MACOS_AARCH64] {
        for (name, symbol) in [
            ("nsget_executable_path", "_NSGetExecutablePath"),
            ("nsget_argc", "_NSGetArgc"),
            ("nsget_argv", "_NSGetArgv"),
            ("nsget_environ", "_NSGetEnviron"),
            ("dyld_image_count", "_dyld_image_count"),
            ("proc_name", "proc_name"),
            ("pthread_get_stackaddr_np", "pthread_get_stackaddr_np"),
            ("pthread_get_stacksize_np", "pthread_get_stacksize_np"),
            ("pthread_self", "pthread_self"),
            ("pthread_cpu_number_np", "pthread_cpu_number_np"),
            ("malloc_good_size", "malloc_good_size"),
            ("nsget_progname", "_NSGetProgname"),
            ("proc_libversion", "proc_libversion"),
            (
                "pthread_jit_write_protect_supported_np",
                "pthread_jit_write_protect_supported_np",
            ),
            ("sysctlnametomib", "sysctlnametomib"),
            ("pthread_equal", "pthread_equal"),
        ] {
            let probe = c
                .system_probes
                .iter()
                .find(|probe| probe.name == name)
                .expect("Darwin probe is catalogued");
            assert!(matches!(
                probe.status,
                SystemProbeStatus::LiveDlcall {
                    lib: "libSystem.B.dylib",
                    symbol: actual,
                } if actual == symbol
            ));
        }
    }
}

#[test]
fn live_cell_matches_compile_target() {
    let live = match live_cell() {
        Some(c) => c,
        None => {
            // Unsupported host — matrix data still compiles; no live row to assert.
            return;
        }
    };
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(live, &LINUX_X86_64);
    }
    if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        assert_eq!(live, &LINUX_AARCH64);
    }
    if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        assert_eq!(live, &MACOS_X86_64);
    }
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(live, &MACOS_AARCH64);
    }
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        assert_eq!(live, &WINDOWS_X86_64);
    }
    if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        assert_eq!(live, &WINDOWS_AARCH64);
    }
}

/// Compile-only: every cell row type-checks when referenced (no implicit stubs).
#[test]
fn every_cell_is_well_formed() {
    for c in ALL_CELLS {
        assert!(!c.os.is_empty());
        assert!(!c.arch.is_empty());
        assert!(!c.pid_lib.is_empty());
        assert!(!c.pid_symbol.is_empty());
        assert!(!c.pid_ret_type.is_empty());
        assert_cell_probe_fields(c);
    }
}

fn assert_cell_probe_fields(c: HostCell) {
    assert!(c.system_probes.iter().all(|probe| !probe.name.is_empty()));
    match c.size_probe {
        SizeProbe::IoctlTiocgwinsz {
            lib,
            symbol,
            request,
        } => {
            assert!(!lib.is_empty());
            assert!(!symbol.is_empty());
            assert!(request > 0);
        }
        SizeProbe::GetConsoleScreenBufferInfo { lib, symbol } => {
            assert!(!lib.is_empty());
            assert!(!symbol.is_empty());
        }
    }
    match c.secondary_probe {
        SecondaryProbe::Native {
            lib,
            symbol,
            ret_type,
        } => {
            assert!(!lib.is_empty());
            assert!(!symbol.is_empty());
            assert!(!ret_type.is_empty());
        }
        SecondaryProbe::Time { lib, symbol } => {
            assert!(!lib.is_empty());
            assert!(!symbol.is_empty());
        }
    }
}

fn system_probe_index(c: HostCell, name: &str) -> usize {
    c.system_probes
        .iter()
        .position(|probe| probe.name == name)
        .expect("system probe must be catalogued")
}
