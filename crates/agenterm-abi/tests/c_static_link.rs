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

/// Shared per-platform system-library lists (single source of truth, also
/// consumed by the milestone 42 pkg-config anti-drift gate). Integration
/// tests are independent crates, so the module lives in `tests/common/`.
mod common;

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

/// Raw `AGENTERM_ABI_PROFILE_DIR` value, if set. An explicit override names
/// the artifact tree to measure (CI sets `target/abi-release`); the raw value
/// is kept for the size printout so the log shows exactly what was
/// configured.
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

/// Directory the ABI artifact is looked up in: the `AGENTERM_ABI_PROFILE_DIR`
/// override when set, otherwise the profile dir derived from the test
/// binary's own path (target/<profile>/deps/ -> target/<profile>/).
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

/// Active Cargo profile name for the size printout. An explicit
/// `AGENTERM_ABI_PROFILE_DIR` wins (its raw value is printed, e.g.
/// `target/abi-release`); otherwise the profile name is derived from the
/// test binary's own path: it lives in `target/<profile>/deps/` (the same
/// layout `locate_staticlib` relies on), so the `deps` parent's name is the
/// profile, e.g. "abi-dev". Avoids the compile-time `env!("PROFILE")`, which
/// current Cargo no longer provides to rustc, and the run-time `PROFILE` var,
/// which Cargo does not set in the test process.
fn profile_name() -> String {
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

/// Locate the staticlib. An explicit `AGENTERM_ABI_PROFILE_DIR` wins (relative
/// values resolve against the repo root, matching how CI passes
/// `target/abi-release`) and is searched directly; otherwise the default
/// layout (same as tests/artifacts.rs) is used: the test binary sits in
/// target/<profile>/deps/, the staticlib in target/<profile>/. Missing
/// staticlib = hard test failure, never a silent skip.
fn locate_staticlib() -> PathBuf {
    const CANDIDATES: [&str; 2] = [
        "agenterm.lib",  // Windows staticlib
        "libagenterm.a", // Unix staticlib
    ];
    let profile_dir = active_profile_dir();
    // The deps/ fallback is only meaningful for the test binary's own profile
    // tree; an explicit override looks only in the configured directory.
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
/// supply when linking the staticlib. The per-platform lists are the shared
/// `common::system_libs` constants — the single source of truth also
/// consumed by the milestone 42 pkg-config anti-drift gate
/// (`pkgconfig_libs.rs` compares the README record against exactly these),
/// so the documented set can never silently drift from the linked set.
/// **Windows list is measured** (milestone 18): linked with no system libs,
/// the MSVC linker reported exactly the symbols these six libraries resolve
/// (ws2_32: Winsock2; ntdll: Nt* / RtlGetVersion; ole32: COM + drag-drop;
/// user32: touch input; uxtheme: SetWindowTheme; dwmapi: DWM). kernel32
/// needs no explicit entry — MSVC links it by default. Linux list is
/// measured by CI. **macOS list is CI-calibrated (milestones 21b + 21c)** —
/// the detailed symbol record lives next to the constants in
/// `tests/common/mod.rs`. It is deliberately *not* verified locally — it
/// awaits the next CI run to confirm, and further unresolved symbols get
/// added the same way.
fn system_libs(msvc: bool) -> Vec<String> {
    if msvc {
        // MSVC link args resolve system libs by file name against the LIB
        // search path; pkg-config form stores bare names, so append `.lib`.
        common::system_libs::MSVC
            .iter()
            .map(|name| format!("{name}.lib"))
            .collect()
    } else {
        #[cfg(target_os = "linux")]
        {
            common::system_libs::LINUX
                .iter()
                .map(|s| s.to_string())
                .collect()
        }
        #[cfg(target_os = "macos")]
        {
            common::system_libs::MACOS
                .iter()
                .map(|s| s.to_string())
                .collect()
        }
        // Non-MSVC Windows (mingw gcc/clang) cannot link an MSVC-target
        // staticlib anyway (incompatible COFF/object format), so no list is
        // guessed here: the link would fail loudly, which is the honest
        // result for a mismatched toolchain.
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Vec::new()
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

/// Unix-only: strip a COPY of the probe and print the before/after sizes.
/// Never strips the real probe — it still has to run as the static-link
/// proof. A missing `strip` on PATH is not a failure (some minimal images
/// omit it): print a note and skip. No size assertion — sizes vary too much
/// across toolchains to pin a number.
#[cfg(unix)]
fn strip_report(exe: &Path) {
    let before = std::fs::metadata(exe).map(|m| m.len()).unwrap_or(0);
    let stripped = exe.with_extension("stripped");
    if let Err(e) = std::fs::copy(exe, &stripped) {
        eprintln!("c_static_link: cannot copy probe for strip: {e}");
        return;
    }
    match Command::new("strip").arg(&stripped).output() {
        Ok(out) if out.status.success() => {
            let after = std::fs::metadata(&stripped).map(|m| m.len()).unwrap_or(0);
            eprintln!(
                "c_static_link: stripped probe = {after} bytes \
                 (was {before} before strip, {})",
                debug_info_note()
            );
        }
        Ok(out) => {
            eprintln!(
                "c_static_link: `strip` failed with {}: {} — skipping \
                 stripped-size measurement",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Err(e) => {
            eprintln!(
                "c_static_link: `strip` not available on PATH ({e}) — skipping \
                 stripped-size measurement"
            );
        }
    }
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

    // Unix-only: measure the stripped size of a COPY of the probe — the real
    // one still has to run below, and the copy is what the released binary
    // would look like after the toolchain strips DWARF. Windows is skipped:
    // debug info lives in a separate .pdb there, so there is nothing to
    // strip from the binary itself.
    #[cfg(unix)]
    strip_report(&exe);

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
