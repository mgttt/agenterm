//! Host script data for ISA×2 / OS×3 cells.
//!
//! Library paths, symbol names, and planned native probes are **script data**
//! consumed by `dlcall` — not a verb API. Every cell is written explicitly so
//! the full matrix compiles on any host; `live_cell()` selects the row that
//! matches `cfg(target_os)` × `cfg(target_arch)`.

/// One OS×ISA cell: default libraries and probe symbols for native smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostCell {
    pub os: &'static str,
    pub arch: &'static str,
    /// Primary dynamic library for PID-family calls.
    pub pid_lib: &'static str,
    pub pid_symbol: &'static str,
    pub pid_ret_type: &'static str,
    /// Window / console dimension probe (may be ioctl or Win32 console API).
    pub size_probe: SizeProbe,
    /// Cheap second native call to prove `dlcall` is not a one-off stub.
    pub secondary_probe: SecondaryProbe,
}

/// Planned terminal / console size probe for a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeProbe {
    /// `ioctl(fd, request, &winsize)` — Linux and macOS tty paths.
    IoctlTiocgwinsz {
        lib: &'static str,
        symbol: &'static str,
        /// Platform `TIOCGWINSZ` request code (script literal for eval).
        request: i64,
    },
    /// `GetConsoleScreenBufferInfo` — Windows console geometry.
    GetConsoleScreenBufferInfo {
        lib: &'static str,
        symbol: &'static str,
    },
}

/// Second real native call for cross-check smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondaryProbe {
    /// Another PID-family symbol (e.g. `getppid`, `GetCurrentThreadId`).
    Native {
        lib: &'static str,
        symbol: &'static str,
        ret_type: &'static str,
    },
    /// `time(NULL)` on Unix — no extra library beyond `libSystem` / libc.
    Time {
        lib: &'static str,
        symbol: &'static str,
    },
}

// --- Linux × {x86_64, aarch64} ------------------------------------------------

pub const LINUX_X86_64: HostCell = HostCell {
    os: "linux",
    arch: "x86_64",
    pid_lib: "libc.so.6",
    pid_symbol: "getpid",
    pid_ret_type: "i32",
    size_probe: SizeProbe::IoctlTiocgwinsz {
        lib: "libc.so.6",
        symbol: "ioctl",
        request: 0x5413, // TIOCGWINSZ
    },
    secondary_probe: SecondaryProbe::Native {
        lib: "libc.so.6",
        symbol: "getppid",
        ret_type: "i32",
    },
};

pub const LINUX_AARCH64: HostCell = HostCell {
    os: "linux",
    arch: "aarch64",
    pid_lib: "libc.so.6",
    pid_symbol: "getpid",
    pid_ret_type: "i32",
    size_probe: SizeProbe::IoctlTiocgwinsz {
        lib: "libc.so.6",
        symbol: "ioctl",
        request: 0x5413,
    },
    secondary_probe: SecondaryProbe::Native {
        lib: "libc.so.6",
        symbol: "getppid",
        ret_type: "i32",
    },
};

// --- macOS × {x86_64, aarch64} ------------------------------------------------

pub const MACOS_X86_64: HostCell = HostCell {
    os: "macos",
    arch: "x86_64",
    pid_lib: "libSystem.B.dylib",
    pid_symbol: "getpid",
    pid_ret_type: "i32",
    size_probe: SizeProbe::IoctlTiocgwinsz {
        lib: "libSystem.B.dylib",
        symbol: "ioctl",
        request: 0x4008_7468, // macOS TIOCGWINSZ
    },
    secondary_probe: SecondaryProbe::Time {
        lib: "libSystem.B.dylib",
        symbol: "time",
    },
};

pub const MACOS_AARCH64: HostCell = HostCell {
    os: "macos",
    arch: "aarch64",
    pid_lib: "libSystem.B.dylib",
    pid_symbol: "getpid",
    pid_ret_type: "i32",
    size_probe: SizeProbe::IoctlTiocgwinsz {
        lib: "libSystem.B.dylib",
        symbol: "ioctl",
        request: 0x4008_7468,
    },
    secondary_probe: SecondaryProbe::Time {
        lib: "libSystem.B.dylib",
        symbol: "time",
    },
};

// --- Windows × {x86_64, aarch64} ----------------------------------------------

pub const WINDOWS_X86_64: HostCell = HostCell {
    os: "windows",
    arch: "x86_64",
    pid_lib: "kernel32.dll",
    pid_symbol: "GetCurrentProcessId",
    pid_ret_type: "u32",
    size_probe: SizeProbe::GetConsoleScreenBufferInfo {
        lib: "kernel32.dll",
        symbol: "GetConsoleScreenBufferInfo",
    },
    secondary_probe: SecondaryProbe::Native {
        lib: "kernel32.dll",
        symbol: "GetCurrentThreadId",
        ret_type: "u32",
    },
};

pub const WINDOWS_AARCH64: HostCell = HostCell {
    os: "windows",
    arch: "aarch64",
    pid_lib: "kernel32.dll",
    pid_symbol: "GetCurrentProcessId",
    pid_ret_type: "u32",
    size_probe: SizeProbe::GetConsoleScreenBufferInfo {
        lib: "kernel32.dll",
        symbol: "GetConsoleScreenBufferInfo",
    },
    secondary_probe: SecondaryProbe::Native {
        lib: "kernel32.dll",
        symbol: "GetCurrentThreadId",
        ret_type: "u32",
    },
};

/// All six cells — always available as compile-time data.
pub const ALL_CELLS: [HostCell; 6] = [
    LINUX_X86_64,
    LINUX_AARCH64,
    MACOS_X86_64,
    MACOS_AARCH64,
    WINDOWS_X86_64,
    WINDOWS_AARCH64,
];

/// Return the cell matching the **current** compile target, if it is one of the
/// six supported cells.
pub fn live_cell() -> Option<&'static HostCell> {
    Some(match () {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        () => &LINUX_X86_64,
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        () => &LINUX_AARCH64,
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        () => &MACOS_X86_64,
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        () => &MACOS_AARCH64,
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        () => &WINDOWS_X86_64,
        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        () => &WINDOWS_AARCH64,
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "aarch64"),
        )))]
        () => return None,
    })
}

/// Look up a cell by `(os, arch)` name — used by matrix completeness tests.
pub fn cell(os: &str, arch: &str) -> Option<&'static HostCell> {
    ALL_CELLS.iter().find(|c| c.os == os && c.arch == arch)
}
