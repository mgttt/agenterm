//! Host script data for ISA×2 / OS×3 cells.
//!
//! **PLATFORM-CANDIDATE module.** This file is typed OS/host contract data:
//! default library paths, PID symbols, `TIOCGWINSZ` request codes,
//! `GetConsoleScreenBufferInfo`, and secondary probe names. When
//! `agenterm-platform` grows an equivalent host-facts table, move these rows
//! there and keep `agenterm-dyn` as the eval + libffi `dlcall` door only.
//! `Dyn::eval` must still accept OS-specific strings as opaque script data at
//! the boundary — only this catalog of known rows is a platform concern.
//! Search for `PLATFORM-CANDIDATE` in this crate for the full list.
//!
//! Every cell is written explicitly so the full matrix compiles on any host;
//! `live_cell()` selects the row matching `cfg(target_os)` × `cfg(target_arch)`.

/// Names of items marked `PLATFORM-CANDIDATE` in this module (migration index).
pub const PLATFORM_CANDIDATES: &[&str] = &[
    "HostCell",
    "SizeProbe",
    "SecondaryProbe",
    "LINUX_X86_64",
    "LINUX_AARCH64",
    "MACOS_X86_64",
    "MACOS_AARCH64",
    "WINDOWS_X86_64",
    "WINDOWS_AARCH64",
    "ALL_CELLS",
    "live_cell",
    "cell",
];

// PLATFORM-CANDIDATE: one OS×ISA row — default libs, symbols, and probe script data.
/// One OS×ISA cell: default libraries and probe symbols for native smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(alias = "platform-candidate")]
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

// PLATFORM-CANDIDATE: terminal/console size probe contract per OS.
/// Planned terminal / console size probe for a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(alias = "platform-candidate")]
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

// PLATFORM-CANDIDATE: secondary native probe contract per OS.
/// Second real native call for cross-check smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(alias = "platform-candidate")]
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

// PLATFORM-CANDIDATE: linux × x86_64 host row.
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

// PLATFORM-CANDIDATE: linux × aarch64 host row.
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

// PLATFORM-CANDIDATE: macos × x86_64 host row.
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

// PLATFORM-CANDIDATE: macos × aarch64 host row.
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

// PLATFORM-CANDIDATE: windows × x86_64 host row.
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

// PLATFORM-CANDIDATE: windows × aarch64 host row.
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

// PLATFORM-CANDIDATE: full six-cell matrix.
/// All six cells — always available as compile-time data.
pub const ALL_CELLS: [HostCell; 6] = [
    LINUX_X86_64,
    LINUX_AARCH64,
    MACOS_X86_64,
    MACOS_AARCH64,
    WINDOWS_X86_64,
    WINDOWS_AARCH64,
];

// PLATFORM-CANDIDATE: cfg-selected live row lookup.
/// Return the cell matching the **current** compile target, if it is one of the
/// six supported cells.
#[doc(alias = "platform-candidate")]
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

// PLATFORM-CANDIDATE: named row lookup into the host matrix.
/// Look up a cell by `(os, arch)` name — used by matrix completeness tests.
#[doc(alias = "platform-candidate")]
pub fn cell(os: &str, arch: &str) -> Option<&'static HostCell> {
    ALL_CELLS.iter().find(|c| c.os == os && c.arch == arch)
}
