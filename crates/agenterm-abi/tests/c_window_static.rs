//! Link-time + run-time regression for the WINDOW/FRAME rendezvous driven by
//! a real C consumer STATICALLY linked against libagenterm:
//! examples/c/agenterm_window.c — the same source file as the dynamic
//! c_window.rs test — must compile, link and run against the `staticlib`
//! this crate builds.
//!
//! This is the milestone 58 gate. The window + frame "control-inversion
//! rendezvous" is the most delicate part of the ABI (the platform is a
//! blocking callback loop hosted on a library-private thread, and control
//! comes back to the caller through agt_frame_begin / agt_frame_commit),
//! and until now it had only ever been driven through the cdylib. Static
//! linking changes the premises the rendezvous depends on:
//!
//! - no shared-library constructor/destructor ordering (the .so init/fini
//!   segments vs the executable's own);
//! - a different TLS model (shared libraries use global-dynamic, an
//!   executable commonly initial-exec);
//! - the runtime (including Rust std's thread/panic machinery) is linked
//!   into the executable instead of living in a separate module.
//!
//! Any one of those breaking means a static consumer hangs or dies at the
//! rendezvous, and previously NO test would have caught it. On Windows CI
//! this probe really opens a window (320x200, no_activate) and drives the
//! full open -> 3 frames -> poll -> metrics -> close sequence; on headless
//! Linux/macOS it follows the existing convention (agt_window_open returns
//! AGT_UNSUPPORTED, the probe prints the reason and exits 0) — which still
//! proves the statically linked artifact starts, calls into the library
//! and gets a UNSUPPORTED instead of crashing.
//!
//! Structure mirrors c_static_link.rs / c_window.rs: the C toolchain is
//! selected from the Rust target's own ABI (never from what happens to sit
//! on PATH), artifacts go under the system temp directory, and
//! compile/link/run failures all fail the test with the compiler's/linker's
//! stderr echoed verbatim in the panic. The ONLY allowed skip is "no C
//! compiler matching the target's ABI found" — printed as `SKIP: <reason>`
//! so CI logs can be counted. Static linking needs no `ensure_soname_alias`
//! (that is the dynamic loader's business); no DLL is ever placed next to
//! the probe, so a successful run with no agenterm DLL present is what
//! proves the link is really static.

mod common;

use common::toolchain::{
    Cleanup, DIR_SEQ, find_c_compiler, locate_staticlib, repo_root, run_or_panic, system_libs,
    target_env_name,
};
use std::process::Command;
use std::sync::atomic::Ordering;

#[test]
fn c_window_static_probe_compiles_links_and_runs() {
    let Some(compiler) = find_c_compiler("c_window_static") else {
        eprintln!(
            "SKIP: no C compiler matching target_env={} was found (see the \
             target_env= decision line above) — cannot prove C-driven \
             window/frame rendezvous under static linking on this machine",
            target_env_name()
        );
        return;
    };

    let root = repo_root();
    let include = root.join("include");
    let c_file = root.join("examples/c/agenterm_window.c");
    assert!(
        c_file.is_file(),
        "missing {} (expected next to this test)",
        c_file.display()
    );

    let staticlib = locate_staticlib();
    eprintln!(
        "c_window_static: linking static library {}",
        staticlib.display()
    );

    // Isolated scratch dir under the system temp dir — never the repo tree.
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!(
        "agenterm-c-window-static-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let _cleanup = Cleanup(scratch.clone());

    let exe_name = if cfg!(windows) {
        "agenterm_window_static.exe"
    } else {
        "agenterm_window_static"
    };
    let exe = scratch.join(exe_name);

    // ---- compile + link (same shape as c_static_link.rs) ---------------
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
        // a single argument with no hand-escaped quotes. CWD = scratch keeps
        // the .obj and .exe out of the repo tree.
        cc.current_dir(&scratch);
        cc.arg("/nologo").arg("/W4").arg("/WX");
        cc.arg(format!("/I{}", include.display()));
        cc.arg(&c_file);
        cc.arg("/Foagenterm_window_static.obj");
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
    run_or_panic("C window static compile/link", &mut cc);

    // ---- run (no DLL, no LD_LIBRARY_PATH: the probe is self-contained) ---
    // Exit code 0 is the contract in BOTH outcomes: a real successful
    // rendezvous (window opened, 3 frames committed, closed) and an explicit
    // skip (agt_window_open returned AGT_UNSUPPORTED on a headless host /
    // macOS — the probe prints the reason and exits 0). Anything else is red
    // here, with the probe's stdout+stderr echoed verbatim in the panic.
    let out = run_or_panic("C window static probe run", &mut Command::new(&exe));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    print!("{stdout}");
    eprint!("{stderr}");

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
