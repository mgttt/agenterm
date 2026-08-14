//! Host script data for ISA×2 / OS×3 cells.
//!
//! **PLATFORM-CANDIDATE module.** This file is typed OS/host contract data:
//! default library paths, PID symbols, `TIOCGWINSZ` request codes,
//! `GetConsoleScreenBufferInfo`, and secondary probe names. When
//! `agenterm-platform` grows an equivalent host-facts table, move these rows
//! there and keep `agenterm-dyn` as the eval + bounded native `dlcall` door only.
//! `Dyn::eval` must still accept OS-specific strings as opaque script data at
//! the boundary — only this catalog of known rows is a platform concern.
//! Search for `PLATFORM-CANDIDATE` in this crate for the full list.
//!
//! Every cell is written explicitly so the full matrix compiles on any host;
//! `live_cell()` selects the row matching `cfg(target_os)` × `cfg(target_arch)`.
//!
//! A parallel **CU-ADJACENT** catalog (`CU_ADJACENT_PROBE_CATALOG`) names libs/symbols/bus
//! facts for `agenterm-cu` hands — script data only, no AT-SPI/UIA/AX wiring.

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
    "CU_ADJACENT_PROBE_CATALOG",
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

// --- CU-ADJACENT probe catalog (PLATFORM-CANDIDATE) ---------------------------

/// Names of LAYER3-CANDIDATE markers in this crate (grep / registry hook). No SLJIT/DynASM
/// dependency is linked yet; see README for portable-backend preference and W^X notes.
pub const LAYER3_CANDIDATES: &[&str] = ["eval_special_form_match", "dlcall_rust_dispatch"];

/// Host OS facet for a catalog cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostOs {
    Linux,
    Macos,
    Windows,
}

impl HostOs {
    pub const ALL: [Self; 3] = [Self::Linux, Self::Macos, Self::Windows];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }
}

/// Host ISA facet for a catalog cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostArch {
    X86_64,
    Aarch64,
}

impl HostArch {
    pub const ALL: [Self; 2] = [Self::X86_64, Self::Aarch64];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

/// One dlcall-oriented or protocol note for a cu hand concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeFact {
    /// Dynamic library when dlcall applies; empty for pure bus/protocol rows.
    pub lib: &'static str,
    /// Exported symbol when dlcall applies; empty for pure bus/protocol rows.
    pub symbol: &'static str,
    /// Protocol / bus / pattern note for script planners and future wiring.
    pub note: &'static str,
}

/// CU-ADJACENT probe row for one `{os, arch}` cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CuAdjacentProbeCell {
    pub os: HostOs,
    pub arch: HostArch,
    pub window_list: ProbeFact,
    pub focus: ProbeFact,
    pub get_text: ProbeFact,
}

impl CuAdjacentProbeCell {
    pub const fn cell_id(self) -> (&'static str, &'static str) {
        (self.os.as_str(), self.arch.as_str())
    }
}

// CU-ADJACENT · PLATFORM-CANDIDATE — per-OS probe facts (arch shares the same script data).

const LINUX_WINDOW_LIST: ProbeFact = ProbeFact {
    lib: "libX11.so.6",
    symbol: "XOpenDisplay",
    note: "X11 _NET_CLIENT_LIST (cu live hand via libagenterm; dyn names lib/symbol only)",
};

const LINUX_FOCUS: ProbeFact = ProbeFact {
    lib: "libatspi.so.0",
    symbol: "",
    note: "AT-SPI2 org.a11y.atspi.Component::grab_focus on session D-Bus (org.a11y.atspi.*)",
};

const LINUX_GET_TEXT: ProbeFact = ProbeFact {
    lib: "",
    symbol: "",
    note: "AT-SPI2 org.a11y.atspi.Text.GetText on session D-Bus",
};

const WINDOWS_WINDOW_LIST: ProbeFact = ProbeFact {
    lib: "user32.dll",
    symbol: "EnumWindows",
    note: "Win32 top-level window enumeration (cu live hand)",
};

const WINDOWS_FOCUS: ProbeFact = ProbeFact {
    lib: "UIAutomationCore.dll",
    symbol: "",
    note: "UIA IUIAutomation::GetFocusedElement",
};

const WINDOWS_GET_TEXT: ProbeFact = ProbeFact {
    lib: "UIAutomationCore.dll",
    symbol: "",
    note: "UIA ValuePattern / LegacyIAccessible for readable text",
};

const MACOS_WINDOW_LIST: ProbeFact = ProbeFact {
    lib: "ApplicationServices",
    symbol: "AXUIElementCreateApplication",
    note: "macOS AX application windows (cu planned hand; PLATFORM-CANDIDATE)",
};

const MACOS_FOCUS: ProbeFact = ProbeFact {
    lib: "ApplicationServices",
    symbol: "",
    note: "AX kAXFocusedAttribute / AXUIElementSetAttributeValue (PLATFORM-CANDIDATE)",
};

const MACOS_GET_TEXT: ProbeFact = ProbeFact {
    lib: "ApplicationServices",
    symbol: "",
    note: "AX kAXValueAttribute / AXUIElementCopyAttributeValue (PLATFORM-CANDIDATE)",
};

const fn linux_cell(arch: HostArch) -> CuAdjacentProbeCell {
    CuAdjacentProbeCell {
        os: HostOs::Linux,
        arch,
        window_list: LINUX_WINDOW_LIST,
        focus: LINUX_FOCUS,
        get_text: LINUX_GET_TEXT,
    }
}

const fn windows_cell(arch: HostArch) -> CuAdjacentProbeCell {
    CuAdjacentProbeCell {
        os: HostOs::Windows,
        arch,
        window_list: WINDOWS_WINDOW_LIST,
        focus: WINDOWS_FOCUS,
        get_text: WINDOWS_GET_TEXT,
    }
}

const fn macos_cell(arch: HostArch) -> CuAdjacentProbeCell {
    CuAdjacentProbeCell {
        os: HostOs::Macos,
        arch,
        window_list: MACOS_WINDOW_LIST,
        focus: MACOS_FOCUS,
        get_text: MACOS_GET_TEXT,
    }
}

/// Six-cell CU-ADJACENT probe catalog — PLATFORM-CANDIDATE script data.
pub const CU_ADJACENT_PROBE_CATALOG: [CuAdjacentProbeCell; 6] = [
    linux_cell(HostArch::X86_64),
    linux_cell(HostArch::Aarch64),
    macos_cell(HostArch::X86_64),
    macos_cell(HostArch::Aarch64),
    windows_cell(HostArch::X86_64),
    windows_cell(HostArch::Aarch64),
];

/// Look up a catalog cell by `{os, arch}`.
pub fn cu_adjacent_probe(os: HostOs, arch: HostArch) -> Option<&'static CuAdjacentProbeCell> {
    CU_ADJACENT_PROBE_CATALOG
        .iter()
        .find(|cell| cell.os == os && cell.arch == arch)
}

/// Candidate dynamic-library names for an AT-SPI existence probe on Linux hosts.
pub const LINUX_ATSPI_EXISTENCE_LIBS: &[&str] = &["libatspi.so.0", "libatspi.so"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn platform_candidates_lists_catalog() {
        assert!(PLATFORM_CANDIDATES.contains(&"ALL_CELLS"));
        assert!(PLATFORM_CANDIDATES.contains(&"CU_ADJACENT_PROBE_CATALOG"));
    }

    #[test]
    fn catalog_has_six_unique_cells() {
        assert_eq!(CU_ADJACENT_PROBE_CATALOG.len(), 6);
        let mut seen = HashSet::new();
        for cell in &CU_ADJACENT_PROBE_CATALOG {
            assert!(seen.insert(cell.cell_id()));
            assert!(!cell.window_list.note.is_empty());
            assert!(!cell.focus.note.is_empty());
            assert!(!cell.get_text.note.is_empty());
        }
        for os in HostOs::ALL {
            for arch in HostArch::ALL {
                assert!(cu_adjacent_probe(os, arch).is_some());
            }
        }
    }

    #[test]
    fn linux_rows_name_x11_and_atspi_facts() {
        for arch in HostArch::ALL {
            let cell = cu_adjacent_probe(HostOs::Linux, arch).expect("linux cell");
            assert_eq!(cell.window_list.lib, "libX11.so.6");
            assert_eq!(cell.window_list.symbol, "XOpenDisplay");
            assert!(cell.focus.note.contains("AT-SPI2"));
            assert!(cell.get_text.note.contains("Text.GetText"));
        }
    }
}
