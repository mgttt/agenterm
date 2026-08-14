//! Host-table matrix tests — all six ISA×OS cells exist as explicit data.

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
fn additional_system_probes_are_live_only_on_linux() {
    for c in [LINUX_X86_64, LINUX_AARCH64] {
        assert_eq!(
            c.system_probes.map(|probe| probe.name),
            [
                "time",
                "clock_gettime",
                "uname",
                "getuid",
                "getgid",
                "geteuid",
                "getegid",
                "sysconf_pagesize",
                "sysconf_clk_tck",
                "sysconf_nprocessors_onln",
                "getcwd"
            ]
        );
        assert_eq!(
            c.system_probes.map(|probe| match probe.status {
                SystemProbeStatus::LiveDlcall { lib, symbol } => (probe.name, lib, symbol),
                SystemProbeStatus::Placeholder => panic!("Linux probe must be live"),
            }),
            [
                ("time", "libc.so.6", "time"),
                ("clock_gettime", "libc.so.6", "clock_gettime"),
                ("uname", "libc.so.6", "uname"),
                ("getuid", "libc.so.6", "getuid"),
                ("getgid", "libc.so.6", "getgid"),
                ("geteuid", "libc.so.6", "geteuid"),
                ("getegid", "libc.so.6", "getegid"),
                ("sysconf_pagesize", "libc.so.6", "sysconf"),
                ("sysconf_clk_tck", "libc.so.6", "sysconf"),
                ("sysconf_nprocessors_onln", "libc.so.6", "sysconf"),
                ("getcwd", "libc.so.6", "getcwd"),
            ]
        );
    }
    for c in [MACOS_X86_64, MACOS_AARCH64, WINDOWS_X86_64, WINDOWS_AARCH64] {
        assert!(
            c.system_probes
                .iter()
                .all(|probe| matches!(probe.status, SystemProbeStatus::Placeholder))
        );
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
