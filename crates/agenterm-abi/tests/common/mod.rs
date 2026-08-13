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
