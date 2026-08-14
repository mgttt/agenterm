//! Milestone 71 end-to-end gate (Windows delivery): install libagenterm
//! into a fresh prefix with the REAL `install-libagenterm` Rh task, then consume
//! ONLY the installed tree -- never the build tree. Windows counterpart of
//! `install_consume.rs` (which serves the Unix channel via install.sh +
//! pkg-config).
//!
//! What this gate proves, in order:
//!
//!   1. the install task lays out exactly the four MSVC/vcpkg files
//!      (include\agenterm.h, lib\agenterm.lib, lib\agenterm.dll.lib,
//!      bin\agenterm.dll) and NOT the .exp/.pdb byproducts;
//!   2. STATIC consumption: link examples/c/agenterm_probe.c against
//!      <libdir>\agenterm.lib + system_libs(true), run, exit 0;
//!   3. DYNAMIC consumption: link the same probe against
//!      <libdir>\agenterm.dll.lib (the import library -- the only thing the
//!      linker sees; no system_libs, the DLL carries its own dependencies),
//!      run with <bindir> on PATH, exit 0;
//!   4. NEGATIVE CONTROL (the decisive evidence, not optional):
//!      - the dynamic exe run in an environment where <bindir> is NOT
//!        reachable must FAIL (agenterm.dll not found) -- proves it is
//!        really dynamically linked;
//!      - the static exe run in the SAME DLL-less environment must SUCCEED
//!        -- proves it really does not depend on the DLL.
//!
//! Before the negative control, agenterm.dll is confirmed absent from every
//! location the loader can reach: the exe directory, the CWD, and every
//! PATH entry (the PATH is rebuilt to exclude <bindir> and any target\
//! directory; a stray DLL anywhere reachable would make the control fake),
//! and the checked locations are eprintln'd so the evidence is auditable in
//! CI logs. Every compile/link/run path points ONLY at the install prefix,
//! and every full command line is eprintln'd.
//!
//! SKIP policy is STRICT by design: non-Windows -> SKIP with a reason
//! (the Rh task is the Windows delivery path); Windows MUST run for real
//! unless `find_c_compiler` finds no MSVC toolchain -- then SKIP with a
//! reason, exactly the c_static_link.rs policy (a mingw gcc on PATH must
//! never be chosen to link an MSVC-ABI .lib).

mod common;

use common::toolchain::{
    Cleanup, DIR_SEQ, find_c_compiler, locate_cdylib, repo_root, run_or_panic, system_libs,
    target_env_name,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;

/// eprintln the full command line (program + args) so CI logs show exactly
/// what was invoked -- every path must point at the install prefix.
fn eprint_cmd(label: &str, cmd: &Command) {
    let mut line = format!("{label}: {}", cmd.get_program().to_string_lossy());
    for a in cmd.get_args() {
        line.push(' ');
        line.push_str(&a.to_string_lossy());
    }
    eprintln!("{line}");
}

/// eprintln + assert that `dir` contains no agenterm.dll (negative-control
/// hygiene: a stray DLL anywhere reachable would make the control fake).
fn assert_no_dll(dir: &Path, label: &str) {
    let p = dir.join("agenterm.dll");
    eprintln!(
        "install_consume_windows: negative-control hygiene: {label} = {} -> has agenterm.dll: {}",
        dir.display(),
        p.exists()
    );
    assert!(
        !p.exists(),
        "negative-control hygiene violated: {label} contains agenterm.dll at {} \
         -- the DLL-less run would not be DLL-less",
        p.display()
    );
}

/// Walk `dir` recursively collecting any .exp / .pdb file (the build
/// byproducts the install task must NOT install).
fn collect_uninstalled_byproducts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_uninstalled_byproducts(&p, out);
        } else if let Some(ext) = p.extension().and_then(|e| e.to_str())
            && (ext.eq_ignore_ascii_case("exp") || ext.eq_ignore_ascii_case("pdb"))
        {
            out.push(p);
        }
    }
}

#[test]
fn install_then_consume_dynamic_and_static_windows() {
    // Non-Windows: the Rh task is the Windows delivery path -- explicit
    // SKIP with a reason, never a run (green must not look like a run).
    if !cfg!(windows) {
        eprintln!(
            "SKIP: install-libagenterm is the Windows delivery path; target_os={}",
            std::env::consts::OS
        );
        return;
    }

    // Windows MUST run for real. The only allowed skip is a missing MSVC
    // toolchain, with a printed reason (c_static_link.rs policy).
    let Some(compiler) = find_c_compiler("install_consume_windows") else {
        eprintln!(
            "SKIP: no C compiler matching target_env={} was found (see the \
             target_env= decision line above) -- cannot prove install-tree \
             usability of the C ABI on this machine",
            target_env_name()
        );
        return;
    };

    let root = repo_root();
    let c_file = root.join("examples/c/agenterm_probe.c");
    assert!(c_file.is_file(), "missing {}", c_file.display());

    // ---- fresh prefix under the system temp dir (never the repo tree) ----
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let prefix =
        std::env::temp_dir().join(format!("agenterm-install-win-{}-{seq}", std::process::id()));
    let libdir = prefix.join("lib");
    let includedir = prefix.join("include");
    let bindir = prefix.join("bin");
    let _cleanup = Cleanup(prefix.clone());

    // ---- run the REAL install-libagenterm task into the prefix ------------
    // ARTIFACTS is pointed at the profile that was actually built
    // (AGENTERM_ABI_PROFILE_DIR on CI, the test binary's own profile tree
    // locally) -- the same decision install_consume.rs makes via
    // locate_cdylib().
    let cdylib = locate_cdylib();
    let artifacts = cdylib
        .parent()
        .expect("cdylib has a parent dir")
        .to_path_buf();
    eprintln!(
        "install_consume_windows: installing artifacts from {} into {}",
        artifacts.display(),
        prefix.display()
    );
    let configured_task_host = std::env::var_os("AGENTERM_INSTALL_TASK_HOST")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/debug/agenterm.exe"));
    let task_host = if configured_task_host.is_absolute() {
        configured_task_host
    } else {
        root.join(configured_task_host)
    };
    assert!(
        task_host.is_file(),
        "install-libagenterm task host missing at {}",
        task_host.display()
    );
    let mut install = Command::new(&task_host);
    install
        .args(["__agenterm-internal-engine", "rh"])
        .args(["task", "run", "install-libagenterm", "--manifest"])
        .arg(root.join("agenterm.tasks.json"))
        .args([
            "--timeout-ms",
            "30000",
            "--max-operations",
            "10000000",
            "--",
        ])
        .arg(&root)
        .arg(&prefix)
        .arg(&artifacts);
    install.env("AGENTERM_NO_ACTIVATE", "1");
    eprint_cmd("install-libagenterm", &install);
    run_or_panic("install-libagenterm", &mut install);

    // ---- the installed layout must be exactly the four promised files ----
    let expected = [
        (libdir.join("agenterm.lib"), "static library agenterm.lib"),
        (
            libdir.join("agenterm.dll.lib"),
            "DLL import library agenterm.dll.lib",
        ),
        (bindir.join("agenterm.dll"), "runtime DLL agenterm.dll"),
        (includedir.join("agenterm.h"), "header agenterm.h"),
    ];
    for (p, what) in &expected {
        let meta = std::fs::metadata(p)
            .unwrap_or_else(|e| panic!("installed tree missing {what} at {}: {e}", p.display()));
        assert!(
            meta.len() > 0,
            "installed {what} at {} is empty",
            p.display()
        );
        eprintln!(
            "install_consume_windows: installed {} = {} bytes",
            p.display(),
            meta.len()
        );
    }

    // The .exp / .pdb byproducts must NOT be installed anywhere in the tree.
    let mut byproducts = Vec::new();
    collect_uninstalled_byproducts(&prefix, &mut byproducts);
    assert!(
        byproducts.is_empty(),
        "install-libagenterm must not install byproducts, found: {:?}",
        byproducts
    );
    eprintln!("install_consume_windows: no .exp/.pdb byproducts in the installed tree");

    // ---- STATIC consumption: <libdir>\agenterm.lib + system_libs(true) ---
    let is_msvc = compiler
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cl.exe") || s.eq_ignore_ascii_case("cl"))
        .unwrap_or(false);

    let static_exe = prefix.join("agenterm_probe_static.exe");
    let mut cc = Command::new(&compiler.path);
    // MSVC toolchain env (INCLUDE/LIB/PATH/...) must be applied verbatim,
    // otherwise cl fails with C1083 (cannot open include file 'stddef.h').
    for (k, v) in &compiler.env {
        cc.env(k, v);
    }
    if is_msvc {
        // cl re-parses the raw command line (not CommandLineToArgvW rules),
        // so each path-bearing option must be a single argument. CWD =
        // prefix keeps the .obj/.exe inside the install tree being tested.
        cc.current_dir(&prefix);
        cc.arg("/nologo").arg("/W4").arg("/WX");
        cc.arg(format!("/I{}", includedir.display()));
        cc.arg(&c_file);
        cc.arg("/Foagenterm_probe_static.obj");
        cc.arg(libdir.join("agenterm.lib"));
        // Rust runtime system libs, resolved by file name against the LIB
        // search path from the toolchain env.
        for lib in system_libs(true) {
            cc.arg(lib);
        }
        cc.arg("/Feagenterm_probe_static.exe");
    } else {
        cc.arg("-Wall").arg("-Wextra").arg("-Werror");
        cc.arg("-I").arg(&includedir);
        cc.arg(&c_file);
        cc.arg(libdir.join("agenterm.lib"));
        cc.arg("-o").arg(&static_exe);
        for lib in system_libs(false) {
            cc.arg(lib);
        }
    }
    eprint_cmd("install_consume_windows static compile/link", &cc);
    run_or_panic("install_consume_windows static compile/link", &mut cc);

    // ---- DYNAMIC consumption: <libdir>\agenterm.dll.lib ------------------
    // No system_libs: the DLL carries its own resolved dependencies; the
    // import library only supplies the __imp_* thunks. Linking against
    // agenterm.dll.lib (NOT agenterm.lib) is what makes the exe depend on
    // agenterm.dll at runtime -- the two libs differ by one .dll infix and
    // picking the wrong one silently changes the linkage form.
    let dynamic_exe = prefix.join("agenterm_probe_dynamic.exe");
    let mut cc_dyn = Command::new(&compiler.path);
    for (k, v) in &compiler.env {
        cc_dyn.env(k, v);
    }
    if is_msvc {
        cc_dyn.current_dir(&prefix);
        cc_dyn.arg("/nologo").arg("/W4").arg("/WX");
        cc_dyn.arg(format!("/I{}", includedir.display()));
        cc_dyn.arg(&c_file);
        cc_dyn.arg("/Foagenterm_probe_dynamic.obj");
        cc_dyn.arg(libdir.join("agenterm.dll.lib"));
        cc_dyn.arg("/Feagenterm_probe_dynamic.exe");
    } else {
        cc_dyn.arg("-Wall").arg("-Wextra").arg("-Werror");
        cc_dyn.arg("-I").arg(&includedir);
        cc_dyn.arg(&c_file);
        cc_dyn.arg(libdir.join("agenterm.dll.lib"));
        cc_dyn.arg("-o").arg(&dynamic_exe);
    }
    eprint_cmd("install_consume_windows dynamic compile/link", &cc_dyn);
    run_or_panic("install_consume_windows dynamic compile/link", &mut cc_dyn);

    // ---- DYNAMIC run with <bindir> on PATH: must succeed -----------------
    // This also exercises the install layout's promise: the runtime DLL is
    // found via PATH (the app dir / CWD have none), so bin\ is the
    // delivery location that matters at run time.
    let mut dyn_run = Command::new(&dynamic_exe);
    let mut path_entries = vec![bindir.clone()];
    if let Some(orig) = std::env::var_os("PATH") {
        path_entries.extend(std::env::split_paths(&orig));
    }
    let dyn_path = std::env::join_paths(path_entries)
        .unwrap_or_else(|e| panic!("cannot build PATH with <bindir> first: {e}"));
    eprintln!(
        "install_consume_windows: dynamic run PATH starts with bindir {}",
        bindir.display()
    );
    dyn_run.env("PATH", &dyn_path);
    dyn_run.current_dir(&prefix);
    eprint_cmd("install_consume_windows dynamic run", &dyn_run);
    let out = run_or_panic("install_consume_windows dynamic run", &mut dyn_run);
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));

    // ---- NEGATIVE CONTROL (the decisive evidence, must not be cut) -------
    // The dynamic exe must FAIL when <bindir> is not reachable, and the
    // static exe must still SUCCEED in that same DLL-less environment.
    //
    // Hygiene first: rebuild PATH without <bindir> and without any target\
    // directory -- keep only the system dirs (System32 + SystemRoot) -- and
    // verify agenterm.dll is absent from every location the loader can
    // reach: the exe dir, the CWD, and each PATH entry. Only after all
    // checks pass is the run meaningful.
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let clean_path = format!(
        "{}\\System32;{}",
        system_root.to_string_lossy(),
        system_root.to_string_lossy()
    );
    eprintln!(
        "install_consume_windows: negative control PATH rebuilt (no <bindir>, no target\\ dirs): {}",
        clean_path
    );
    let clean_dirs: Vec<PathBuf> = std::env::split_paths(&clean_path).collect();
    assert!(!clean_dirs.is_empty(), "empty negative-control PATH");
    assert_no_dll(&prefix, "exe directory (prefix root)");
    assert_no_dll(&prefix, "negative-control CWD");
    for d in &clean_dirs {
        assert_no_dll(d, "negative-control PATH entry");
    }

    // Dynamic exe without the DLL: must FAIL (proves the link is dynamic).
    let mut neg_dyn = Command::new(&dynamic_exe);
    neg_dyn.env("PATH", &clean_path);
    neg_dyn.current_dir(&prefix);
    eprint_cmd(
        "install_consume_windows negative control (dynamic, must fail)",
        &neg_dyn,
    );
    let neg_out = neg_dyn
        .output()
        .expect("spawn dynamic probe for negative control");
    let neg_code = neg_out.status.code();
    eprintln!(
        "install_consume_windows: negative control dynamic exit code = {neg_code:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&neg_out.stdout),
        String::from_utf8_lossy(&neg_out.stderr),
    );
    assert!(
        !neg_out.status.success(),
        "negative control violated: the DYNAMIC probe ran successfully without \
         agenterm.dll reachable (exit {neg_code:?}) -- it is not dynamically \
         linked as claimed"
    );

    // Static exe in the SAME DLL-less environment: must SUCCEED (proves the
    // link is static and the exe is self-contained).
    let mut neg_static = Command::new(&static_exe);
    neg_static.env("PATH", &clean_path);
    neg_static.current_dir(&prefix);
    eprint_cmd(
        "install_consume_windows negative control (static, must run)",
        &neg_static,
    );
    let out = run_or_panic(
        "install_consume_windows negative control static run",
        &mut neg_static,
    );
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
}
