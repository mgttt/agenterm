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
//!
//! C-toolchain discovery, artifact lookup and run-or-panic plumbing are
//! shared via `common::toolchain` (milestone 51) so `symbol_presence.rs`
//! reuses the exact same "compiler found or SKIP" and
//! `AGENTERM_ABI_PROFILE_DIR` decisions — no second copy exists.

use agenterm::{ABI_MAJOR, ABI_MINOR};
use common::toolchain::{
    Cleanup, DIR_SEQ, debug_info_note, find_c_compiler, locate_staticlib, profile_name, repo_root,
    run_or_panic, system_libs, target_env_name,
};
use std::process::Command;
use std::sync::atomic::Ordering;

/// Shared per-platform system-library lists and the C-toolchain helpers
/// (single source of truth, also consumed by the milestone 42 pkg-config
/// anti-drift gate and the milestone 51 symbol-presence gate).
/// Integration tests are independent crates, so the module lives in
/// `tests/common/`.
mod common;

/// Unix-only: strip a COPY of the probe and print the before/after sizes.
/// Never strips the real probe — it still has to run as the static-link
/// proof. A missing `strip` on PATH is not a failure (some minimal images
/// omit it): print a note and skip. No size assertion — sizes vary too much
/// across toolchains to pin a number.
#[cfg(unix)]
fn strip_report(exe: &std::path::Path) {
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
