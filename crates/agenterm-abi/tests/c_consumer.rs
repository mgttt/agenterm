//! Link-time regression: a real C consumer (examples/c/agenterm_probe.c) must
//! compile, link and run against the cdylib this crate builds. The
//! dlopen-based suite (tests/dylib_load.rs) proves the FFI contract at runtime
//! only; this test proves the exported symbols are actually usable at *link*
//! time and that `include/agenterm.h` is consumable by a real C compiler.
//!
//! Compile/link/run failures all fail the test, with the compiler's stderr
//! echoed verbatim in the panic. The ONLY allowed skip is "no C compiler
//! matching the target's ABI found": the C toolchain is selected from the
//! Rust target's own ABI (`cfg!(target_env)`), never from what happens to
//! sit on PATH — an MSVC target links with the MSVC toolchain only (via
//! cc::windows_registry, which finds cl.exe without vcvarsall on PATH), a
//! GNU target with PATH cc/gcc/clang, so a mingw ld can never be chosen to
//! consume an MSVC-ABI import library (undefined `__chkstk` /
//! `??_7type_info@@6B@` / `__imp_*`). Artifacts go under the system temp
//! directory and are cleaned up; nothing is written into the repository
//! tree.

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
/// debug-profile probe is much smaller on Windows for that reason alone.
/// MSVC vs everything else is the only split that matters here
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
/// `locate_cdylib` relies on), so the `deps` parent's name is the profile,
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
/// compiled for. The cdylib ships the Rust target's ABI, so the C toolchain
/// must agree with it:
///
/// - `target_env = "msvc"`: ONLY the MSVC toolchain (found through
///   `cc::windows_registry`, so vcvarsall need not have been sourced into
///   PATH). PATH `gcc`/`clang` are deliberately not consulted — a mingw ld
///   cannot consume MSVC-ABI objects (undefined `__chkstk` /
///   `??_7type_info@@6B@` / `__imp_*`), which is the exact CI failure this
///   selection exists to prevent.
/// - `target_env = "gnu"`: the GNU C ABI — PATH `cc`/`gcc`/`clang`, never
///   MSVC. Covers mingw on Windows and the ordinary libc toolchain on
///   Linux/macOS (unchanged non-Windows behavior).
/// - anything else (e.g. macOS): unchanged PATH `cc`/`gcc`/`clang`.
///
/// `label` prefixes the eprintln decision line (e.g. "c_consumer") so a
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

/// Locate the cdylib built under the active profile (same layout as
/// tests/dylib_load.rs): the test binary sits in target/<profile>/deps/, the
/// cdylib in target/<profile>/. Missing cdylib = hard test failure, never a
/// silent skip.
fn locate_cdylib() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe()");
    let deps = exe.parent().expect("test binary has a parent dir");
    let profile_dir = deps.parent().expect("deps dir has a parent dir");
    const CANDIDATES: [&str; 3] = [
        "agenterm.dll",      // Windows
        "libagenterm.so",    // Linux
        "libagenterm.dylib", // macOS
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
        "agenterm-abi cdylib not found under {} (candidates: {CANDIDATES:?}). \
         Build it with an unwind profile first, e.g. \
         `cargo build -p agenterm-abi --profile abi-dev`",
        profile_dir.display()
    );
}

/// Windows-only link input: the MSVC import library next to the cdylib.
/// Unix links the shared object directly with -lagenterm.
fn locate_import_lib(cdylib: &Path) -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let p = cdylib.with_file_name("agenterm.dll.lib");
    if p.is_file() { Some(p) } else { None }
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
fn c_consumer_compiles_links_and_runs() {
    let Some(compiler) = find_c_compiler("c_consumer") else {
        eprintln!(
            "SKIP: no C compiler matching target_env={} was found (see the \
             target_env= decision line above) — cannot prove link-time \
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

    let cdylib = locate_cdylib();
    let import_lib = locate_import_lib(&cdylib);

    // Isolated scratch dir under the system temp dir — never the repo tree.
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch =
        std::env::temp_dir().join(format!("agenterm-c-consumer-{}-{seq}", std::process::id()));
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
    // otherwise cl fails with C1083 (cannot open include file 'stddef.h').
    for (k, v) in &compiler.env {
        cc.env(k, v);
    }
    if is_msvc {
        // /W4 /WX = warnings are errors. cl re-parses the raw command line
        // (not CommandLineToArgvW rules), so each path-bearing option must be
        // a single argument with no hand-escaped quotes; a bare
        // "/Fe <path>" split across two arguments makes cl treat the path as
        // an extra source file. A path containing spaces gets the standard
        // quoting that MSVC's parser understands. CWD = scratch keeps the
        // .obj and .exe out of the repo tree.
        cc.current_dir(&scratch);
        cc.arg("/nologo").arg("/W4").arg("/WX");
        cc.arg(format!("/I{}", include.display()));
        cc.arg(&c_file);
        cc.arg("/Foagenterm_probe.obj");
        let lib = import_lib.as_ref().unwrap_or_else(|| {
            panic!(
                "MSVC import library missing next to {} — build with \
                 `cargo build -p agenterm-abi --profile abi-dev`",
                cdylib.display()
            )
        });
        cc.arg(lib);
        cc.arg(format!("/Fe{exe_name}"));
    } else {
        cc.arg("-Wall").arg("-Wextra").arg("-Werror");
        cc.arg("-I").arg(&include);
        cc.arg(&c_file);
        cc.arg("-o").arg(&exe);
        if let Some(lib) = &import_lib {
            // Windows gcc/clang: link the MSVC import library directly.
            cc.arg(lib);
        } else {
            let lib_dir = cdylib.parent().expect("cdylib has a parent dir");
            cc.arg("-L").arg(lib_dir).arg("-lagenterm");
        }
    }
    run_or_panic("C compile/link", &mut cc);

    // ---- report size (print only, no assertion: it varies too much across
    // platforms/toolchains to pin a number) --------------------------------
    let probe_size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "c_consumer: dynamically linked probe = {probe_size} bytes \
         (profile={}, {})",
        profile_name(),
        debug_info_note()
    );

    // ---- run ------------------------------------------------------------
    if cfg!(windows) {
        // Windows loads the DLL from the executable's own directory.
        std::fs::copy(&cdylib, scratch.join(cdylib.file_name().unwrap()))
            .expect("copy cdylib next to the probe executable");
    }
    let mut run = Command::new(&exe);
    if cfg!(target_os = "macos") {
        run.env("DYLD_LIBRARY_PATH", cdylib.parent().unwrap());
    } else if !cfg!(windows) {
        run.env("LD_LIBRARY_PATH", cdylib.parent().unwrap());
    }
    let out = run_or_panic("C probe run", &mut run);

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
}
