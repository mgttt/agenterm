//! Single source of truth for the per-platform system-library lists a C
//! consumer must supply when STATICALLY linking libagenterm.
//!
//! Shared by two integration tests:
//! - `c_static_link.rs` consumes these constants to build its real link
//!   command (the milestone 18 gate);
//! - `pkgconfig_libs.rs` (milestone 42 anti-drift gate) compares them against
//!   the values recorded in `packaging/pkgconfig/README.md`, so the
//!   pkg-config `Libs.private` documentation can never silently drift from
//!   the lists that are actually linked with.
//!
//! The `toolchain` module below is the shared C-toolchain discovery, ABI
//! artifact location (`AGENTERM_ABI_PROFILE_DIR` override) and
//! run-or-panic plumbing originally written for `c_static_link.rs`; milestone
//! 51 (`symbol_presence.rs`) reuses the same helpers so the "compiler found or
//! SKIP" and "artifact lookup" decisions are never written a second time.
//!
//! Integration-test files are compiled as independent crates (each
//! `tests/*.rs` is its own crate), so the lists live here in the `common`
//! module instead of being duplicated per test file. Cargo does not build
//! `tests/common/` as a test target, so this adds no extra binary.
//!
//! **These lists are measured, never guessed** — see the per-platform notes
//! below. Stored in pkg-config `Libs.private` token form: bare names for
//! MSVC (the `.lib` suffix is appended where a real link command is built),
//! full `-l` / `-framework` argument pairs for Unix. Order matters on macOS
//! (the linker resolves in order); keep it in sync with the README table.

/// Per-platform lists live in one `system_libs` module shared by every
/// integration-test crate. `#[allow(dead_code)]` is required because each
/// `tests/*.rs` compiles this module independently: on a given platform
/// `c_static_link.rs` only references its own platform's constant, so the
/// other two look dead there — but `pkgconfig_libs.rs` always reads all
/// three, so they are never genuinely unused.
#[allow(dead_code)]
pub mod system_libs {
    /// Windows/MSVC, bare names (no `.lib`): `ws2_32 ntdll ole32 user32
    /// uxtheme dwmapi`. Measured at milestone 18 — linked with no system
    /// libs and added exactly what the MSVC linker reported unresolved.
    /// `kernel32` links by default and needs no entry.
    pub const MSVC: &[&str] = &["ws2_32", "ntdll", "ole32", "user32", "uxtheme", "dwmapi"];

    /// Linux: `-ldl -lpthread -lm` (argument form, usable as-is both in the
    /// link command and in pkg-config `Libs.private`). Measured by CI on the
    /// milestone 18 link gate.
    pub const LINUX: &[&str] = &["-ldl", "-lpthread", "-lm"];

    /// macOS: the Apple frameworks as `-framework X` argument pairs, then
    /// the shared Unix `-ldl -lpthread -lm`. CI-calibrated over two rounds:
    /// milestone 21b added the first set, milestone 21c added Carbon.
    ///
    /// Milestone 21b (first macOS CI run failed with unresolved `_CF*` / CG
    /// / NS symbols pulled in via winit / core-*; the linker errors named):
    ///   CoreFoundation: _CF* 一族 —— _CFAbsoluteTimeGetCurrent
    ///     (winit EventLoopWaker::start_at), _CFArrayGetCount /
    ///     _CFArrayGetValueAtIndex (agenterm_platform search_windows,
    ///     winit MonitorHandle::video_modes, core_graphics CFArray::len),
    ///     _CFAttributedStringCreateMutable (core_foundation),
    ///     _CFBundleCopyBundleURL / _CFBundleCopyExecutableURL /
    ///     _CFBundleCopyPrivateFrameworksURL /
    ///     _CFBundleCopyResourcesDirectoryURL;
    ///   CoreGraphics: the core_graphics adapter (CG* display / window
    ///     geometry symbols);
    ///   AppKit + Foundation: winit platform_impl::macos (NS* application /
    ///     run-loop symbols);
    ///   QuartzCore + Metal + IOKit: winit's macOS backend commonly pulls
    ///     these in too — extra frameworks never fail the link, missing
    ///     ones do, so they are listed preemptively.
    /// Milestone 21c (round two: exactly three unresolved symbols left, all
    /// from winit::platform_impl::macos::event::get_modifierless_char —
    /// _LMGetKbdType (HIToolbox legacy Menu Manager compat) and
    /// _TISCopyCurrentKeyboardLayoutInputSource / _TISGetInputSourceProperty
    /// (Text Input Sources) — all provided by the Carbon framework).
    ///
    /// `-framework X` is TWO separate arguments (`-framework`, then the
    /// name) — never `-framework=X`. The Unix `-ldl -lpthread -lm` are
    /// harmless on macOS and kept for the shared runtime deps. Still a
    /// CI-calibrated set: if the next CI run reports more unresolved
    /// symbols, add exactly those frameworks here, then mirror them in
    /// `packaging/pkgconfig/README.md` (the anti-drift gate compares them).
    pub const MACOS: &[&str] = &[
        "-framework",
        "CoreFoundation",
        "-framework",
        "CoreGraphics",
        "-framework",
        "AppKit",
        "-framework",
        "Foundation",
        "-framework",
        "QuartzCore",
        "-framework",
        "Metal",
        "-framework",
        "IOKit",
        "-framework",
        "Carbon",
        "-ldl",
        "-lpthread",
        "-lm",
    ];
}

/// Shared C-toolchain discovery, ABI artifact location and run-or-panic
/// plumbing. Extracted verbatim from `c_static_link.rs` (the milestone 18
/// gate) plus the cdylib lookup from `c_consumer.rs`; milestone 51's
/// `symbol_presence.rs` reuses these so no second copy of the "compiler found
/// or SKIP" / `AGENTERM_ABI_PROFILE_DIR` decision exists. Keep `c_static_link`
/// / `c_consumer` / `c_window` test bodies and their assertions unchanged.
///
/// `#[allow(dead_code)]` is required because each `tests/*.rs` compiles this
/// module independently: `pkgconfig_libs.rs` only reads `system_libs`, and on
/// a given platform the other tests only reference part of the helper set, so
/// the rest looks dead there — same reasoning as the `system_libs` module.
#[allow(dead_code)]
pub mod toolchain {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::AtomicU64;

    /// Unique temp-dir suffix so parallel test processes never collide.
    pub static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Repository root: this crate lives at <root>/crates/agenterm-abi.
    pub fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ dir missing")
            .parent()
            .expect("repo root missing")
            .to_path_buf()
    }

    /// First candidate that exists as a file on PATH, or None.
    fn find_on_path(candidates: &[&str]) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            for name in candidates {
                let bare = dir.join(name);
                if bare.is_file() {
                    return Some(bare);
                }
                if cfg!(windows) && !name.ends_with(".exe") {
                    let exe = dir.join(format!("{name}.exe"));
                    if exe.is_file() {
                        return Some(exe);
                    }
                }
            }
        }
        None
    }

    /// A located C compiler: the executable path plus (MSVC only) the
    /// toolchain environment (INCLUDE, LIB, PATH, ...) the cl.exe needs to
    /// run. The env must be applied verbatim to every spawned process —
    /// without INCLUDE, cl fails with C1083 (cannot open include file
    /// 'stddef.h').
    pub struct CCompiler {
        pub path: PathBuf,
        pub env: Vec<(OsString, OsString)>,
    }

    /// Human-readable `target_env` for the printed decision line, derived
    /// from the same `cfg!(target_env)` checks that drive the toolchain
    /// branch.
    pub fn target_env_name() -> &'static str {
        if cfg!(target_env = "msvc") {
            "msvc"
        } else if cfg!(target_env = "gnu") {
            "gnu"
        } else if cfg!(target_env = "musl") {
            "musl"
        } else {
            "other"
        }
    }

    /// Where the toolchain puts debug info — appended to the size printout so
    /// CI log readers do not compare sizes across families: MSVC keeps it in
    /// a separate `.pdb`, Unix embeds DWARF in the binary itself, so a
    /// debug-profile probe is much smaller on Windows for that reason alone.
    /// MSVC vs everything else is the only split that matters here
    /// (mingw/Unix both embed).
    pub fn debug_info_note() -> &'static str {
        if cfg!(target_env = "msvc") {
            "debug info separate .pdb on MSVC"
        } else {
            "DWARF embedded"
        }
    }

    /// Raw `AGENTERM_ABI_PROFILE_DIR` value, if set. An explicit override
    /// names the artifact tree to measure (CI sets `target/abi-release`); the
    /// raw value is kept for the size printout so the log shows exactly what
    /// was configured.
    fn profile_dir_override_raw() -> Option<String> {
        std::env::var("AGENTERM_ABI_PROFILE_DIR").ok()
    }

    /// Resolved override directory. Relative values resolve against the repo
    /// root: the test process CWD is the crate dir, while CI passes
    /// `target/abi-release` relative to the workspace root.
    fn profile_dir_override() -> Option<PathBuf> {
        let raw = profile_dir_override_raw()?;
        let p = PathBuf::from(&raw);
        Some(if p.is_absolute() {
            p
        } else {
            repo_root().join(p)
        })
    }

    /// Directory the ABI artifact is looked up in: the
    /// `AGENTERM_ABI_PROFILE_DIR` override when set, otherwise the profile
    /// dir derived from the test binary's own path
    /// (target/<profile>/deps/ -> target/<profile>/).
    fn active_profile_dir() -> PathBuf {
        if let Some(dir) = profile_dir_override() {
            return dir;
        }
        let exe = std::env::current_exe().expect("current_exe()");
        let deps = exe.parent().expect("test binary has a parent dir");
        deps.parent()
            .expect("deps dir has a parent dir")
            .to_path_buf()
    }

    /// Active Cargo profile name for the printout. An explicit
    /// `AGENTERM_ABI_PROFILE_DIR` wins (its raw value is printed, e.g.
    /// `target/abi-release`); otherwise the profile name is derived from the
    /// test binary's own path: it lives in `target/<profile>/deps/` (the same
    /// layout `locate_staticlib` relies on), so the `deps` parent's name is
    /// the profile, e.g. "abi-dev". Avoids the compile-time `env!("PROFILE")`,
    /// which current Cargo no longer provides to rustc, and the run-time
    /// `PROFILE` var, which Cargo does not set in the test process.
    pub fn profile_name() -> String {
        if let Some(raw) = profile_dir_override_raw() {
            return raw;
        }
        let Ok(exe) = std::env::current_exe() else {
            return "unknown".to_string();
        };
        let Some(profile_dir) = exe
            .parent()
            .and_then(|deps| deps.parent())
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
        else {
            return "unknown".to_string();
        };
        profile_dir.to_string()
    }

    /// Locate a C compiler whose ABI matches the Rust target this test was
    /// compiled for. The staticlib ships the Rust target's ABI, so the C
    /// toolchain must agree with it:
    ///
    /// - `target_env = "msvc"`: ONLY the MSVC toolchain (found through
    ///   `cc::windows_registry`, so vcvarsall need not have been sourced into
    ///   PATH). PATH `gcc`/`clang` are deliberately not consulted — a mingw
    ///   ld cannot link MSVC-ABI objects (undefined `__chkstk` /
    ///   `??_7type_info@@6B@` / `__imp_*`), which is the exact CI failure
    ///   this selection exists to prevent.
    /// - `target_env = "gnu"`: the GNU C ABI — PATH `cc`/`gcc`/`clang`, never
    ///   MSVC. Covers mingw on Windows and the ordinary libc toolchain on
    ///   Linux/macOS (unchanged non-Windows behavior).
    /// - anything else (e.g. macOS): unchanged PATH `cc`/`gcc`/`clang`.
    ///
    /// `label` prefixes the eprintln decision line (e.g. "c_static_link") so
    /// a wrong selection is visible at a glance in CI logs.
    pub fn find_c_compiler(label: &str) -> Option<CCompiler> {
        let target_env = target_env_name();
        if cfg!(target_env = "msvc") {
            #[cfg(windows)]
            {
                match find_msvc_tool() {
                    Some(tool) => {
                        eprintln!(
                            "{label}: target_env={target_env} -> using MSVC cl.exe at {}",
                            tool.path().display()
                        );
                        Some(CCompiler {
                            path: tool.path().to_path_buf(),
                            env: tool.env().to_vec(),
                        })
                    }
                    None => {
                        eprintln!(
                            "{label}: target_env={target_env} -> no MSVC toolchain \
                             found via cc::windows_registry -> SKIP (only MSVC \
                             matches the ABI of this target)"
                        );
                        None
                    }
                }
            }
            #[cfg(not(windows))]
            {
                // An msvc target_env exists only on Windows targets; this arm
                // is present solely so the branch compiles on non-Windows
                // hosts.
                path_compiler(label, target_env, &["cc", "gcc", "clang"])
            }
        } else if cfg!(target_env = "gnu") {
            path_compiler(label, target_env, &["cc", "gcc", "clang"])
        } else {
            // Non-Windows / other environments (e.g. macOS): unchanged
            // behavior.
            let mut candidates = vec!["cc", "gcc", "clang"];
            if cfg!(windows) {
                candidates.push("cl.exe");
            }
            path_compiler(label, target_env, &candidates)
        }
    }

    /// PATH lookup with a printed decision line: `label: target_env=... ->
    /// using <name> at <path>` on success, `... -> SKIP` when nothing
    /// matches.
    fn path_compiler(label: &str, target_env: &str, candidates: &[&str]) -> Option<CCompiler> {
        if let Some(path) = find_on_path(candidates) {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            eprintln!(
                "{label}: target_env={target_env} -> using {name} at {}",
                path.display()
            );
            Some(CCompiler {
                path,
                env: Vec::new(),
            })
        } else {
            eprintln!(
                "{label}: target_env={target_env} -> none of [{}] on PATH -> SKIP",
                candidates.join(", ")
            );
            None
        }
    }

    /// Locate cl.exe through the cc crate's Windows registry API, which knows
    /// how to find the installed Visual Studio toolchain without vcvarsall on
    /// PATH.
    #[cfg(windows)]
    fn find_msvc_tool() -> Option<cc::Tool> {
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            "x86" => "i686",
            other => other,
        };
        cc::windows_registry::find_tool(&format!("{arch}-pc-windows-msvc"), "cl.exe")
    }

    /// Locate the cdylib. An explicit `AGENTERM_ABI_PROFILE_DIR` wins
    /// (relative values resolve against the repo root, matching how CI passes
    /// `target/abi-release`) and is searched directly; otherwise the default
    /// layout is used: the test binary sits in target/<profile>/deps/, the
    /// cdylib in target/<profile>/. Missing cdylib = hard test failure, never
    /// a silent skip.
    pub fn locate_cdylib() -> PathBuf {
        const CANDIDATES: [&str; 3] = [
            "agenterm.dll",      // Windows
            "libagenterm.so",    // Linux
            "libagenterm.dylib", // macOS
        ];
        let profile_dir = active_profile_dir();
        // The deps/ fallback is only meaningful for the test binary's own
        // profile tree; an explicit override looks only in the configured
        // directory.
        let mut dirs = vec![profile_dir.clone()];
        if profile_dir_override().is_none() {
            let exe = std::env::current_exe().expect("current_exe()");
            let deps = exe.parent().expect("test binary has a parent dir");
            dirs.push(deps.to_path_buf());
        }
        for dir in &dirs {
            for name in CANDIDATES {
                let p = dir.join(name);
                if p.exists() {
                    return p;
                }
            }
        }
        panic!(
            "agenterm-abi cdylib not found under {} (candidates: {CANDIDATES:?}). \
             Build it with an unwind profile first, e.g. \
             `cargo build -p agenterm-abi --profile abi-dev`",
            profile_dir.display()
        );
    }

    /// Locate the staticlib. An explicit `AGENTERM_ABI_PROFILE_DIR` wins
    /// (relative values resolve against the repo root, matching how CI passes
    /// `target/abi-release`) and is searched directly; otherwise the default
    /// layout (same as tests/artifacts.rs) is used: the test binary sits in
    /// target/<profile>/deps/, the staticlib in target/<profile>/. Missing
    /// staticlib = hard test failure, never a silent skip.
    pub fn locate_staticlib() -> PathBuf {
        const CANDIDATES: [&str; 2] = [
            "agenterm.lib",  // Windows staticlib
            "libagenterm.a", // Unix staticlib
        ];
        let profile_dir = active_profile_dir();
        // The deps/ fallback is only meaningful for the test binary's own
        // profile tree; an explicit override looks only in the configured
        // directory.
        let mut dirs = vec![profile_dir.clone()];
        if profile_dir_override().is_none() {
            let exe = std::env::current_exe().expect("current_exe()");
            let deps = exe.parent().expect("test binary has a parent dir");
            dirs.push(deps.to_path_buf());
        }
        for dir in &dirs {
            for name in CANDIDATES {
                let p = dir.join(name);
                if p.exists() {
                    return p;
                }
            }
        }
        panic!(
            "agenterm-abi staticlib not found under {} (candidates: {CANDIDATES:?}). \
             Build it with an unwind profile first, e.g. \
             `cargo build -p agenterm-abi --profile abi-dev`",
            profile_dir.display()
        );
    }

    /// The Rust runtime's system-library dependencies that a C program must
    /// supply when linking the staticlib. The per-platform lists are the
    /// shared `common::system_libs` constants — the single source of truth
    /// also consumed by the milestone 42 pkg-config anti-drift gate
    /// (`pkgconfig_libs.rs` compares the README record against exactly
    /// these), so the documented set can never silently drift from the linked
    /// set. **Windows list is measured** (milestone 18): linked with no system
    /// libs, the MSVC linker reported exactly the symbols these six libraries
    /// resolve (ws2_32: Winsock2; ntdll: Nt* / RtlGetVersion; ole32: COM +
    /// drag-drop; user32: touch input; uxtheme: SetWindowTheme; dwmapi: DWM).
    /// kernel32 needs no explicit entry — MSVC links it by default. Linux
    /// list is measured by CI. **macOS list is CI-calibrated (milestones 21b + 21c)** —
    /// the detailed symbol record lives next to the constants in
    /// `tests/common/mod.rs` (never split "21b + 21c" across lines: a line
    /// starting with `+` reads as a markdown list item to clippy). It is
    /// deliberately *not* verified locally — it awaits the next CI run to
    /// confirm, and further unresolved symbols get added the same way.
    pub fn system_libs(msvc: bool) -> Vec<String> {
        if msvc {
            // MSVC link args resolve system libs by file name against the LIB
            // search path; pkg-config form stores bare names, so append
            // `.lib`.
            super::system_libs::MSVC
                .iter()
                .map(|name| format!("{name}.lib"))
                .collect()
        } else {
            #[cfg(target_os = "linux")]
            {
                super::system_libs::LINUX
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            }
            #[cfg(target_os = "macos")]
            {
                super::system_libs::MACOS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            }
            // Non-MSVC Windows (mingw gcc/clang) cannot link an MSVC-target
            // staticlib anyway (incompatible COFF/object format), so no list
            // is guessed here: the link would fail loudly, which is the
            // honest result for a mismatched toolchain.
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                Vec::new()
            }
        }
    }

    /// Remove the temp dir on drop (also on panic unwinding).
    pub struct Cleanup(pub PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Run `cmd`, capture output, and panic with it echoed when it fails.
    pub fn run_or_panic(label: &str, cmd: &mut Command) -> std::process::Output {
        let out = cmd
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn {label} ({cmd:?}): {e}"));
        if !out.status.success() {
            let code = out.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            panic!(
                "{label} failed with exit code {code}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
            );
        }
        out
    }
}
