//! Milestone 59 measurement: a real C consumer
//! (examples/c/agenterm_mixed_linkage.c) that links libagenterm STATICALLY
//! while ALSO loading the dynamic library (LoadLibrary / dlopen) into the
//! same process, then reports whether the two copies' per-process state
//! (the thread-local LAST_ERROR / MSG_BUF / A11Y_SNAPSHOT in src/lib.rs) is
//! independent.
//!
//! The probe is a measuring instrument, not an assertion engine: it always
//! exits 0 and prints the observed facts to stdout. The cross-copy
//! last_error / A11Y_SNAPSHOT behavior is the unknown quantity this
//! milestone exists to measure, so the test asserts only what is CERTAIN:
//! the compile/link succeeds, the run exits 0, and both copies report the
//! same `agt_abi_version()` and the same `agt_process_self()` (same ABI,
//! same process — the experiment's own validity check). The ONLY allowed
//! skip is "no C compiler matching the target ABI found"; `[fatal]` output
//! from the probe (dynamic load / symbol resolution failed) is a hard
//! failure, because a broken dynamic half invalidates the whole experiment.
//!
//! Artifacts go under the system temp directory and are cleaned up; nothing
//! is written into the repository tree. The dynamic copy is located via the
//! shared `common::toolchain` lookup and passed to the probe as argv[1], so
//! no DLL is ever placed next to the probe by this test.

use agenterm::{ABI_MAJOR, ABI_MINOR};
use common::toolchain::{
    Cleanup, DIR_SEQ, debug_info_note, find_c_compiler, locate_cdylib, locate_staticlib,
    profile_name, repo_root, run_or_panic, system_libs, target_env_name,
};
use std::process::Command;
use std::sync::atomic::Ordering;

mod common;

/// Value of the stdout line `prefix<value>`, e.g. `static_process_self=1234`
/// with prefix `"static_process_self="` returns `"1234"`. A missing line
/// panics with the full stdout, so a probe that never reached a step
/// (dynamic load failed, symbol missing, ...) fails the test loudly instead
/// of silently skipping.
fn field<'a>(stdout: &'a str, prefix: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| {
            panic!("probe stdout must contain a line starting with {prefix:?}:\n{stdout}")
        })
}

#[test]
fn mixed_static_and_dynamic_linkage_probe() {
    let Some(compiler) = find_c_compiler("mixed_linkage") else {
        eprintln!(
            "SKIP: no C compiler matching target_env={} was found (see the \
             target_env= decision line above) — cannot build the mixed \
             static+dynamic linkage probe on this machine",
            target_env_name()
        );
        return;
    };

    let root = repo_root();
    let include = root.join("include");
    let c_file = root.join("examples/c/agenterm_mixed_linkage.c");
    assert!(
        c_file.is_file(),
        "missing {} (expected next to this test)",
        c_file.display()
    );

    let staticlib = locate_staticlib();
    let cdylib = locate_cdylib();
    eprintln!("mixed_linkage: static archive {}", staticlib.display());
    eprintln!("mixed_linkage: dynamic library {}", cdylib.display());

    // Isolated scratch dir under the system temp dir — never the repo tree.
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!(
        "agenterm-mixed-linkage-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let _cleanup = Cleanup(scratch.clone());

    let exe_name = if cfg!(windows) {
        "agenterm_mixed_linkage.exe"
    } else {
        "agenterm_mixed_linkage"
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
        cc.current_dir(&scratch);
        cc.arg("/nologo").arg("/W4").arg("/WX");
        cc.arg(format!("/I{}", include.display()));
        cc.arg(&c_file);
        cc.arg("/Foagenterm_mixed_linkage.obj");
        cc.arg(&staticlib);
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
    run_or_panic("C mixed-linkage compile/link", &mut cc);

    // ---- report size (print only, no assertion: it varies too much across
    // platforms/toolchains to pin a number) --------------------------------
    let probe_size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "mixed_linkage: probe = {probe_size} bytes (profile={}, {})",
        profile_name(),
        debug_info_note()
    );

    // ---- run ------------------------------------------------------------
    // The probe LoadLibrary/dlopens the cdylib itself via argv[1], so no
    // copy, no LD_LIBRARY_PATH / DYLD_LIBRARY_PATH is needed here.
    let out = run_or_panic("C mixed-linkage probe run", Command::new(&exe).arg(&cdylib));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    print!("{stdout}");
    eprint!("{stderr}");

    // [fatal] = the probe could not load/resolve the dynamic copy; that is an
    // environment failure, not a measured fact, so it fails the test.
    assert!(
        !stdout.contains("[fatal]"),
        "probe reported a fatal error (dynamic copy unusable):\n{stdout}"
    );

    // The two CERTAIN facts: both copies are the same ABI and run in the same
    // process. Cross-copy last_error / A11Y_SNAPSHOT behavior is the measured
    // unknown and is deliberately NOT asserted — that is the milestone's
    // whole point, and the assertions only become possible once all three
    // platforms' data is in.
    let abi_version = ((ABI_MAJOR as u32) << 16) | ABI_MINOR as u32;
    let expected_abi = format!("0x{abi_version:08x}");
    let static_abi = field(&stdout, "static_abi_version=");
    let dynamic_abi = field(&stdout, "dynamic_abi_version=");
    assert_eq!(
        static_abi, expected_abi,
        "static copy must report the ABI version this crate was built with"
    );
    assert_eq!(
        dynamic_abi, expected_abi,
        "dynamic copy must report the ABI version this crate was built with"
    );
    assert_eq!(
        dynamic_abi, static_abi,
        "both copies must report the same ABI version"
    );
    let static_pid = field(&stdout, "static_process_self=");
    let dynamic_pid = field(&stdout, "dynamic_process_self=");
    assert_eq!(
        static_pid, dynamic_pid,
        "both copies run in the SAME process, so agt_process_self() must agree \
         (this is the experiment's own validity check)"
    );
    assert_ne!(
        static_pid, "0",
        "agt_process_self() must report a real pid, got 0"
    );
}
