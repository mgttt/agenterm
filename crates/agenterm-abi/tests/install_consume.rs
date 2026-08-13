//! Milestone 61 end-to-end gate: install libagenterm into a fresh prefix
//! with the REAL `packaging/install.sh`, then consume ONLY the installed
//! tree — never the build tree.
//!
//! `pkgconfig_consume.rs` (milestone 52) staged a hand-built tree (copied
//! `.a` + header) and proved a STATIC consumer works. Milestone 56 then
//! proved the install layout is non-trivial (DT_SONAME means the consumer's
//! DT_NEEDED records `libagenterm.so.1` and the versioned file must exist
//! on disk; `libagenterm.so` is only the link-time symlink), but nothing
//! ever verified "after installing per our layout, can a consumer use it".
//! This test is that verification:
//!
//!   1. build a fresh prefix under the system temp dir (never the repo tree);
//!   2. run the repository's REAL `packaging/install.sh` into it
//!      (--version left unset so the script's own Cargo.toml read is
//!      exercised, not a test-supplied copy);
//!   3. `PKG_CONFIG_PATH=$prefix/lib/pkgconfig`, ask
//!      `pkg-config --cflags --libs libagenterm` (DYNAMIC, not --static)
//!      for the link line;
//!   4. compile and link `examples/c/agenterm_probe.c` with it, run the
//!      binary, exit code must be 0;
//!   5. same prefix again with `pkg-config --cflags --libs --static`, link
//!      `$libdir/libagenterm.a`, run, exit code 0.
//!
//! The runtime library search path is set ONLY to the installed `$prefix/lib`
//! (LD_LIBRARY_PATH / DYLD_LIBRARY_PATH) — the install directory, never
//! `target/`. That is the one place this round can prove the INSTALLED
//! artifacts work.
//!
//! SKIP policy is STRICT by design (counted in CI logs): Windows -> SKIP
//! (`install.sh` + `.pc` are the Unix delivery channel); no `pkg-config`
//! executable -> SKIP; no `sh` interpreter -> SKIP. Everything else MUST
//! run — a missing C compiler is a hard failure, never a skip.

mod common;

use common::toolchain::{
    Cleanup, DIR_SEQ, find_c_compiler, locate_cdylib, repo_root, run_or_panic,
};
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;

/// The `libagenterm.so.N` soname build.rs passes to the linker, read back
/// from `<OUT_DIR>/soname.txt` (the same single source the tests'
/// `ensure_soname_alias` uses — never a hard-coded major here). Only used on
/// Linux to check the installed layout matches the soname.
#[cfg(target_os = "linux")]
fn recorded_soname() -> String {
    let recorded = Path::new(env!("OUT_DIR")).join("soname.txt");
    let soname = std::fs::read_to_string(&recorded)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", recorded.display()))
        .trim()
        .to_string();
    assert!(!soname.is_empty(), "{} is empty", recorded.display());
    soname
}

#[test]
fn install_then_consume_dynamic_and_static() {
    // Windows: install.sh + .pc are the Unix delivery channel — explicit
    // SKIP, never a run.
    if cfg!(windows) {
        eprintln!("SKIP: install.sh and .pc are the Unix delivery channel (this is Windows)");
        return;
    }

    // `sh` must actually exist to run install.sh / generate-pc.sh.
    let sh_ok = Command::new("sh")
        .arg("-c")
        .arg("exit 0")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !sh_ok {
        eprintln!("SKIP: no `sh` interpreter on PATH (install.sh needs one)");
        return;
    }

    // pkg-config must actually exist. Missing executable = explicit SKIP.
    let pkg_config_ok = Command::new("pkg-config")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !pkg_config_ok {
        eprintln!("SKIP: no pkg-config executable on PATH");
        return;
    }

    // Everything past this point MUST run: a missing C compiler is a hard
    // failure, not a SKIP (strict SKIP policy, milestone 61).
    let Some(compiler) = find_c_compiler("install_consume") else {
        panic!(
            "install_consume: no C compiler found on PATH (strict SKIP policy: \
             a Unix consumer needs a C toolchain — do not skip this gate)"
        );
    };

    let root = repo_root();
    let c_file = root.join("examples/c/agenterm_probe.c");
    assert!(c_file.is_file(), "missing {}", c_file.display());

    // ---- fresh prefix under the system temp dir (never the repo tree) ----
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let prefix =
        std::env::temp_dir().join(format!("agenterm-install-{}-{seq}", std::process::id()));
    let libdir = prefix.join("lib");
    let pc_dir = libdir.join("pkgconfig");
    let _cleanup = Cleanup(prefix.clone());

    // ---- run the REAL install script into the prefix ----------------------
    // --version is deliberately left unset: the script's own Cargo.toml read
    // is part of what is being tested. The artifacts dir is pointed at the
    // profile that was actually built (AGENTERM_ABI_PROFILE_DIR on CI, the
    // test binary's own profile tree locally).
    let install_sh = root.join("packaging/install.sh");
    let cdylib = locate_cdylib();
    let artifacts = cdylib
        .parent()
        .expect("cdylib has a parent dir")
        .to_path_buf();
    eprintln!(
        "install_consume: installing artifacts from {} into {}",
        artifacts.display(),
        prefix.display()
    );
    run_or_panic(
        "install.sh",
        Command::new("sh")
            .arg(&install_sh)
            .arg("--prefix")
            .arg(&prefix)
            .arg("--artifacts")
            .arg(&artifacts),
    );

    // ---- the installed layout must match the build's install identity ----
    assert!(
        libdir.join("libagenterm.a").is_file(),
        "installed tree missing static library"
    );
    assert!(
        prefix.join("include/agenterm.h").is_file(),
        "installed tree missing agenterm.h"
    );
    assert!(
        pc_dir.join("libagenterm.pc").is_file(),
        "installed tree missing libagenterm.pc"
    );
    #[cfg(target_os = "linux")]
    {
        let soname = recorded_soname();
        assert!(
            libdir.join(&soname).is_file(),
            "installed tree missing the versioned real file {soname} (the soname \
             build.rs passes to the linker)",
        );
        let symlink = libdir.join("libagenterm.so");
        let meta = std::fs::symlink_metadata(&symlink)
            .unwrap_or_else(|e| panic!("cannot stat installed {}: {e}", symlink.display()));
        assert!(
            meta.file_type().is_symlink(),
            "installed {} must be the link-time symlink, got {:?}",
            symlink.display(),
            meta.file_type()
        );
        let target = std::fs::read_link(&symlink)
            .unwrap_or_else(|e| panic!("cannot readlink {}: {e}", symlink.display()));
        assert_eq!(
            target,
            PathBuf::from(&soname),
            "installed symlink must point at {soname}"
        );
    }
    #[cfg(target_os = "macos")]
    {
        assert!(
            libdir.join("libagenterm.dylib").is_file(),
            "installed tree missing libagenterm.dylib"
        );
    }

    // ---- DYNAMIC consumption: pkg-config WITHOUT --static -----------------
    let dyn_pc = Command::new("pkg-config")
        .env("PKG_CONFIG_PATH", &pc_dir)
        .args(["--cflags", "--libs", "libagenterm"])
        .output()
        .expect("spawn pkg-config (dynamic)");
    assert!(
        dyn_pc.status.success(),
        "pkg-config --cflags --libs libagenterm failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&dyn_pc.stdout),
        String::from_utf8_lossy(&dyn_pc.stderr),
    );
    let dyn_line = String::from_utf8_lossy(&dyn_pc.stdout).trim().to_string();
    eprintln!("install_consume: dynamic pkg-config line: {dyn_line}");
    assert!(
        dyn_line.contains("-lagenterm"),
        "dynamic pkg-config output must reference -lagenterm, got: {dyn_line:?}"
    );
    assert!(
        !dyn_line.contains('@'),
        "dynamic pkg-config output still carries a raw '@' (unsubstituted \
         placeholder in the generated .pc): {dyn_line:?}"
    );

    let dyn_args: Vec<&str> = dyn_line.split_whitespace().collect();
    let dyn_exe = prefix.join("agenterm_probe_dynamic");
    let mut cc = Command::new(&compiler.path);
    for (k, v) in &compiler.env {
        cc.env(k, v);
    }
    cc.arg("-Wall").arg("-Wextra").arg("-Werror");
    cc.arg(&c_file);
    cc.args(&dyn_args);
    cc.arg("-o").arg(&dyn_exe);
    run_or_panic("install_consume dynamic compile/link", &mut cc);

    // Run with the library search path pointing ONLY at the installed libdir
    // (the install directory — never the build tree). This is the one proof
    // that the INSTALLED versioned library is actually resolvable.
    let mut dyn_run = Command::new(&dyn_exe);
    #[cfg(target_os = "macos")]
    dyn_run.env("DYLD_LIBRARY_PATH", &libdir);
    #[cfg(target_os = "linux")]
    dyn_run.env("LD_LIBRARY_PATH", &libdir);
    let out = run_or_panic("install_consume dynamic run", &mut dyn_run);
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));

    // ---- STATIC consumption: pkg-config WITH --static ----------------------
    let st_pc = Command::new("pkg-config")
        .env("PKG_CONFIG_PATH", &pc_dir)
        .args(["--cflags", "--libs", "--static", "libagenterm"])
        .output()
        .expect("spawn pkg-config (static)");
    assert!(
        st_pc.status.success(),
        "pkg-config --cflags --libs --static libagenterm failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&st_pc.stdout),
        String::from_utf8_lossy(&st_pc.stderr),
    );
    let st_line = String::from_utf8_lossy(&st_pc.stdout).trim().to_string();
    eprintln!("install_consume: static pkg-config line: {st_line}");
    assert!(
        st_line.contains("-lagenterm"),
        "static pkg-config output must reference -lagenterm, got: {st_line:?}"
    );

    let st_args: Vec<&str> = st_line.split_whitespace().collect();
    let st_exe = prefix.join("agenterm_probe_static");
    let mut cc2 = Command::new(&compiler.path);
    for (k, v) in &compiler.env {
        cc2.env(k, v);
    }
    cc2.arg("-Wall").arg("-Wextra").arg("-Werror");
    cc2.arg(&c_file);
    cc2.args(&st_args);
    cc2.arg("-o").arg(&st_exe);
    run_or_panic("install_consume static compile/link", &mut cc2);

    // Static binary is self-contained: no library search path at all.
    let out = run_or_panic("install_consume static run", &mut Command::new(&st_exe));
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
}
