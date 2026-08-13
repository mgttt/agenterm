//! Link-time regression for the STATIC library: a real C consumer
//! (examples/c/agenterm_probe.c — the same source file as the dynamic
//! c_consumer.rs test, so both paths prove the same ABI) must compile, link
//! and run against the `staticlib` this crate builds.
//!
//! This is the milestone 18 gate: a staticlib that exists is not a
//! deliverable — it must actually link into a C program. Rust `staticlib`
//! statically linked into C requires the C side to supply the Rust runtime's
//! system-library dependencies; the MSVC list below was measured empirically
//! (link once with no system libs, then add exactly what the linker
//! reported unresolved). Missing staticlib, compile failure, link failure
//! and non-zero exit all fail the test, with the linker's stderr echoed
//! verbatim in the panic. The ONLY allowed skip is "no C compiler matching
//! the target's ABI found": the C toolchain is selected from the Rust
//! target's own ABI (`cfg!(target_env)`), never from what happens to sit on
//! PATH — an MSVC target links with the MSVC toolchain only, a GNU target
//! with PATH cc/gcc/clang, so a mingw ld can never be chosen to link an
//! MSVC-ABI staticlib (undefined `__chkstk` / `??_7type_info@@6B@` /
//! `__imp_*`). Artifacts go under the system temp directory and are cleaned
//! up; nothing
//! is written into the repository tree, and — unlike the dynamic path — no
//! DLL is ever placed next to the probe executable: the run succeeding with
//! no agenterm DLL present is what proves the link is really static.

use agenterm::{ABI_MAJOR, ABI_MINOR};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique temp-dir suffix so parallel test processes never collide.
static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

/// Repository root: this crate lives at <root>/crates/agenterm-abi.
fn repo_root() -> PathBuf {
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

/// A located C compiler: the executable path plus (MSVC only) the toolchain
/// environment (INCLUDE, LIB, PATH, ...) the cl.exe needs to run. The env
/// must be applied verbatim to every spawned process — without INCLUDE, cl
/// fails with C1083 (cannot open include file 'stddef.h').
struct CCompiler {
    path: PathBuf,
    env: Vec<(OsString, OsString)>,
}

/// Human-readable `target_env` for the printed decision line, derived from
/// the same `cfg!(target_env)` checks that drive the toolchain branch.
fn target_env_name() -> &'static str {
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

/// Where the toolchain puts debug info — appended to the size printout so CI
/// log readers do not compare sizes across families: MSVC keeps it in a
/// separate `.pdb`, Unix embeds DWARF in the binary itself, so a
/// debug-profile static probe is much smaller on Windows for that reason
/// alone. MSVC vs everything else is the only split that matters here
/// (mingw/Unix both embed).
fn debug_info_note() -> &'static str {
    if cfg!(target_env = "msvc") {
        "debug info separate .pdb on MSVC"
    } else {
        "DWARF embedded"
    }
}

/// Active Cargo profile name for the size printout, derived from the test
/// binary's own path: it lives in `target/<profile>/deps/` (the same layout
/// `locate_staticlib` relies on), so the `deps` parent's name is the profile,
/// e.g. "abi-dev". Avoids the compile-time `env!("PROFILE")`, which current
/// Cargo no longer provides to rustc, and the run-time `PROFILE` var, which
/// Cargo does not set in the test process.
fn profile_name() -> String {
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
///   PATH). PATH `gcc`/`clang` are deliberately not consulted — a mingw ld
///   cannot link MSVC-ABI objects (undefined `__chkstk` /
///   `??_7type_info@@6B@` / `__imp_*`), which is the exact CI failure this
///   selection exists to prevent.
/// - `target_env = "gnu"`: the GNU C ABI — PATH `cc`/`gcc`/`clang`, never
///   MSVC. Covers mingw on Windows and the ordinary libc toolchain on
///   Linux/macOS (unchanged non-Windows behavior).
/// - anything else (e.g. macOS): unchanged PATH `cc`/`gcc`/`clang`.
///
/// `label` prefixes the eprintln decision line (e.g. "c_static_link") so a
/// wrong selection is visible at a glance in CI logs.
fn find_c_compiler(label: &str) -> Option<CCompiler> {
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
            // An msvc target_env exists only on Windows targets; this arm is
            // present solely so the branch compiles on non-Windows hosts.
            path_compiler(label, target_env, &["cc", "gcc", "clang"])
        }
    } else if cfg!(target_env = "gnu") {
        path_compiler(label, target_env, &["cc", "gcc", "clang"])
    } else {
        // Non-Windows / other environments (e.g. macOS): unchanged behavior.
        let mut candidates = vec!["cc", "gcc", "clang"];
        if cfg!(windows) {
            candidates.push("cl.exe");
        }
        path_compiler(label, target_env, &candidates)
    }
}

/// PATH lookup with a printed decision line: `label: target_env=... ->
/// using <name> at <path>` on success, `... -> SKIP` when nothing matches.
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

/// Locate cl.exe through the cc crate's Windows registry API, which knows how
/// to find the installed Visual Studio toolchain without vcvarsall on PATH.
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

/// Locate the staticlib built under the active profile (same layout as
/// tests/artifacts.rs): the test binary sits in target/<profile>/deps/, the
/// staticlib in target/<profile>/. Missing staticlib = hard test failure,
/// never a silent skip.
fn locate_staticlib() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe()");
    let deps = exe.parent().expect("test binary has a parent dir");
    let profile_dir = deps.parent().expect("deps dir has a parent dir");
    const CANDIDATES: [&str; 2] = [
        "agenterm.lib",  // Windows staticlib
        "libagenterm.a", // Unix staticlib
    ];
    for dir in [profile_dir, deps] {
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
/// supply when linking the staticlib. **Windows list is measured** (milestone
/// 18): linked with no system libs, the MSVC linker reported exactly the
/// symbols these six libraries resolve (ws2_32: Winsock2; ntdll: Nt* /
/// RtlGetVersion; ole32: COM + drag-drop; user32: touch input; uxtheme:
/// SetWindowTheme; dwmapi: DWM). kernel32 needs no explicit entry — MSVC
/// links it by default. Linux list is measured by CI. **macOS list is
/// CI-calibrated (milestones 21b + 21c)**: the first macOS CI run failed
/// with unresolved `_CF*` / CG / NS symbols pulled in via winit / core-*, so
/// the C side must add the Apple frameworks (`-framework` flags, see the
/// branch below) on top of the Unix `-ldl -lpthread -lm`. Round two (21c)
/// added Carbon for the last three winit `get_modifierless_char` Text Input
/// Services symbols. It is deliberately *not* verified locally — it awaits
/// the next CI run to confirm, and further unresolved symbols get added the
/// same way.
fn system_libs(msvc: bool) -> Vec<&'static str> {
    if msvc {
        vec![
            "ws2_32.lib",
            "ntdll.lib",
            "ole32.lib",
            "user32.lib",
            "uxtheme.lib",
            "dwmapi.lib",
        ]
    } else {
        #[cfg(target_os = "linux")]
        {
            vec!["-ldl", "-lpthread", "-lm"]
        }
        #[cfg(target_os = "macos")]
        {
            // macOS: first-CI-calibrated initial set (milestone 21b). Rust
            // staticlib statically linked into a C program pulls in the
            // Apple frameworks through winit / core-*; the first macOS CI
            // run's linker errors named these symbols:
            //   CoreFoundation: _CF* 一族 —— _CFAbsoluteTimeGetCurrent
            //     (winit EventLoopWaker::start_at), _CFArrayGetCount /
            //     _CFArrayGetValueAtIndex (agenterm_platform search_windows,
            //     winit MonitorHandle::video_modes, core_graphics
            //     CFArray::len), _CFAttributedStringCreateMutable
            //     (core_foundation), _CFBundleCopyBundleURL /
            //     _CFBundleCopyExecutableURL /
            //     _CFBundleCopyPrivateFrameworksURL /
            //     _CFBundleCopyResourcesDirectoryURL;
            //   CoreGraphics: the core_graphics adapter (CG* display /
            //     window geometry symbols);
            //   AppKit + Foundation: winit platform_impl::macos (NS*
            //     application / run-loop symbols);
            //   QuartzCore + Metal + IOKit: winit's macOS backend commonly
            //     pulls these in too — extra frameworks never fail the link,
            //     missing ones do, so they are listed preemptively.
            //   Carbon (round two, milestone 21c): the second macOS CI run
            //     was down to exactly three unresolved symbols, all from
            //     winit::platform_impl::macos::event::get_modifierless_char:
            //     _LMGetKbdType (HIToolbox legacy Menu Manager compat) and
            //     _TISCopyCurrentKeyboardLayoutInputSource /
            //     _TISGetInputSourceProperty (Text Input Sources) — all
            //     provided by the Carbon framework.
            // `-framework X` is TWO separate arguments (`-framework`, then
            // the name) — never `-framework=X`. The Unix `-ldl -lpthread
            // -lm` are harmless on macOS and kept for the shared runtime
            // deps. Still a CI-calibrated set: if the next CI run reports
            // more unresolved symbols, add exactly those frameworks.
            vec![
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
            ]
        }
        // Non-MSVC Windows (mingw gcc/clang) cannot link an MSVC-target
        // staticlib anyway (incompatible COFF/object format), so no list is
        // guessed here: the link would fail loudly, which is the honest
        // result for a mismatched toolchain.
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            vec![]
        }
    }
}

/// Remove the temp dir on drop (also on panic unwinding).
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `cmd`, capture output, and panic with it echoed when it fails.
fn run_or_panic(label: &str, cmd: &mut Command) -> std::process::Output {
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

#[test]
fn c_consumer_static_links_and_runs() {
    let Some(compiler) = find_c_compiler("c_static_link") else {
        eprintln!(
            "SKIP: no C compiler matching target_env={} was found (see the \
             target_env= decision line above) — cannot prove static link-time \
             usability of the C ABI on this machine",
            target_env_name()
        );
        return;
    };

    let root = repo_root();
    let include = root.join("include");
    let c_file = root.join("examples/c/agenterm_probe.c");
    assert!(
        c_file.is_file(),
        "missing {} (expected next to this test)",
        c_file.display()
    );

    let staticlib = locate_staticlib();
    eprintln!(
        "c_static_link: linking static library {}",
        staticlib.display()
    );

    // Isolated scratch dir under the system temp dir — never the repo tree.
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch =
        std::env::temp_dir().join(format!("agenterm-c-static-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let _cleanup = Cleanup(scratch.clone());

    let exe_name = if cfg!(windows) {
        "agenterm_probe.exe"
    } else {
        "agenterm_probe"
    };
    let exe = scratch.join(exe_name);

    // ---- compile + link -------------------------------------------------
    let is_msvc = compiler
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cl.exe") || s.eq_ignore_ascii_case("cl"))
        .unwrap_or(false);

    let mut cc = Command::new(&compiler.path);
    // MSVC toolchain env (INCLUDE/LIB/PATH/...) must be applied verbatim,
    // otherwise cl fails with C1083 (cannot open include file 'stddef.h')
    // and the system .lib files are not found on the LIB search path.
    for (k, v) in &compiler.env {
        cc.env(k, v);
    }
    if is_msvc {
        // /W4 /WX = warnings are errors. cl re-parses the raw command line
        // (not CommandLineToArgvW rules), so each path-bearing option must be
        // a single argument with no hand-escaped quotes; a bare
        // "/Fe <path>" split across two arguments makes cl treat the path as
        // an extra source file. CWD = scratch keeps the .obj and .exe out of
        // the repo tree.
        cc.current_dir(&scratch);
        cc.arg("/nologo").arg("/W4").arg("/WX");
        cc.arg(format!("/I{}", include.display()));
        cc.arg(&c_file);
        cc.arg("/Foagenterm_probe.obj");
        cc.arg(&staticlib);
        // Rust runtime system libs, resolved by file name against the LIB
        // search path from the toolchain env.
        for lib in system_libs(true) {
            cc.arg(lib);
        }
        cc.arg(format!("/Fe{exe_name}"));
    } else {
        cc.arg("-Wall").arg("-Wextra").arg("-Werror");
        cc.arg("-I").arg(&include);
        cc.arg(&c_file);
        cc.arg(&staticlib);
        cc.arg("-o").arg(&exe);
        for lib in system_libs(false) {
            cc.arg(lib);
        }
    }
    run_or_panic("C static compile/link", &mut cc);

    // ---- report sizes (print only, no assertion: they vary too much
    // across platforms/toolchains to pin a number) -------------------------
    // The staticlib's own size is printed next to the final binary so the
    // "archive vs linked output" gap is visible at a glance — archives are
    // usually much larger because the linker keeps only the members that
    // are actually referenced.
    let probe_size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    let lib_size = std::fs::metadata(&staticlib).map(|m| m.len()).unwrap_or(0);
    let lib_name = staticlib
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?");
    eprintln!(
        "c_static_link: statically linked probe = {probe_size} bytes \
         (profile={}, {}) \
         ({lib_name} = {lib_size} bytes)",
        profile_name(),
        debug_info_note()
    );

    // ---- run (no DLL, no LD_LIBRARY_PATH: the probe is self-contained) ---
    // The static link is only proven by the probe running with NO
    // agenterm.dll / libagenterm.so next to it and no loader env pointing at
    // one — so unlike c_consumer.rs nothing is copied here.
    let out = run_or_panic("C static probe run", &mut Command::new(&exe));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    print!("{stdout}");
    eprint!("{stderr}");

    // Anti-drift gate: expect the line the C probe must print for the ABI
    // version, computed from the crate's own constants — never a hard-coded
    // literal that would go stale on the next ABI bump.
    let abi_version = ((ABI_MAJOR as u32) << 16) | ABI_MINOR as u32;
    let expected_line = format!("abi_version=0x{abi_version:08x}");
    assert!(
        stdout.contains(&expected_line),
        "probe stdout must contain {expected_line}, got:\n{stdout}"
    );

    // Static-link proof: no agenterm dynamic library may exist anywhere in
    // the scratch dir (it never does — nothing copies one — and the run
    // already succeeded without one).
    for stray in ["agenterm.dll", "libagenterm.so", "libagenterm.dylib"] {
        assert!(
            !scratch.join(stray).exists(),
            "static-link gate violated: {stray} was placed next to the probe"
        );
    }
}
