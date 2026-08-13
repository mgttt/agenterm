//! Milestone 51 gate: prove the exports listed in `exports.txt` REALLY exist
//! in BOTH delivered artifacts, not just in the Rust source and the header.
//! `exports_set.rs` compares three source texts (`exports.txt` ↔
//! `include/agenterm.h` ↔ `#[no_mangle]` in `src/lib.rs`); nothing asked the
//! built artifacts whether they actually carry all 54 symbols. A missing
//! symbol in the staticlib (dead-code elimination, a cfg branch, a symbol
//! that only reached the cdylib) blows up at the consumer's link time with CI
//! fully green.
//!
//! Two halves:
//! 1. **Dynamic** — open the built cdylib with `libloading` and resolve EVERY
//!    export via `Library::get`. All must succeed; failures are collected and
//!    reported together (each named), never just the first. This half does
//!    not depend on a C compiler, so it always runs — no SKIP path.
//! 2. **Static** — generate a C program (into the system temp dir, never the
//!    repo tree) whose only job is to take the ADDRESS of every exported
//!    function, link it against the staticlib with the real C toolchain, and
//!    run it. Linking succeeding = every symbol in the archive resolved; this
//!    is more reliable than parsing `nm` output and uses the same code for
//!    MSVC / gcc / clang. No C compiler matching the target ABI -> SKIP
//!    (same convention as `c_static_link.rs`).
//!
//! SAFETY BOUNDARY: the generated C program only takes function addresses and
//! compares pointers — it NEVER calls any export. Several of these 54
//! (`agt_input_pointer_click`, `agt_input_type_text`, `agt_native_window_close`,
//! `agt_process_kill`, ...) would really click, really type, really close the
//! user's window, really kill a process. The address table iterates every
//! element (not just table[0]) on purpose: a compiler that can prove only
//! table[0] is observed would be free to drop the other initializers, which
//! would silently remove the references the linker needs to prove presence.
//!
//! C-toolchain discovery, artifact lookup and run-or-panic plumbing are
//! shared from `common::toolchain` (`c_static_link.rs`'s helpers, extracted
//! for milestone 51) — no second copy of the "compiler found or SKIP" or
//! `AGENTERM_ABI_PROFILE_DIR` decision exists.

use common::toolchain::{
    Cleanup, DIR_SEQ, find_c_compiler, locate_cdylib, locate_staticlib, profile_name, repo_root,
    run_or_panic, system_libs, target_env_name,
};
use libloading::Library;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;

/// Shared per-platform system-library lists and the C-toolchain helpers
/// (single source of truth, also consumed by the milestone 18 / 42 / 51
/// gates). Integration tests are independent crates, so the module lives in
/// `tests/common/`.
mod common;

/// The expected export list, read straight from `exports.txt` — the single
/// source of truth. Never a hand-typed 54-line list: a hand-written list is
/// exactly the drift this gate exists to catch.
fn expected_exports() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("exports.txt");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("exports.txt not found at {}: {e}", path.display()));
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Milestone 51, half 1 (dynamic): every export must resolve in the built
/// cdylib. No C compiler involved, so this test ALWAYS runs — a missing
/// cdylib is a hard failure, never a skip. Failures are accumulated and
/// reported together so one round of fixes can close them all.
#[test]
fn every_export_resolves_in_cdylib() {
    let lib_path = locate_cdylib();
    eprintln!("symbol_presence: opening cdylib {}", lib_path.display());
    let lib = unsafe { Library::new(&lib_path) }
        .unwrap_or_else(|e| panic!("dlopen/LoadLibrary({lib_path:?}) failed: {e}"));

    let exports = expected_exports();
    let mut missing: Vec<String> = Vec::new();
    for name in &exports {
        // Take the ADDRESS only, typed as a generic extern "C" fn() — never
        // called, so the real signature is irrelevant here.
        let got: Option<libloading::Symbol<'_, unsafe extern "C" fn()>> =
            unsafe { lib.get(name.as_bytes()) }.ok();
        if got.is_none() {
            missing.push(name.clone());
        }
    }

    // Report what actually resolved, not the list length: printing "54
    // resolved" on the line above an assertion that fails because three are
    // missing reads as a contradiction in the CI log.
    eprintln!(
        "symbol_presence: {}/{} dynamic symbols resolved ({}, profile={})",
        exports.len() - missing.len(),
        exports.len(),
        lib_path.display(),
        profile_name()
    );
    assert!(
        missing.is_empty(),
        "{} of {} exports are MISSING from the cdylib at {}:\n  {}",
        missing.len(),
        exports.len(),
        lib_path.display(),
        missing.join("\n  ")
    );
}

/// Milestone 51, half 2 (static): generate a C program that takes the address
/// of every export, link it against the staticlib with the real C toolchain
/// and run it. Linking succeeding = every symbol in the archive is present;
/// the program exits 0 only if every address is non-NULL. No C compiler
/// matching the target's ABI -> SKIP (same convention as `c_static_link.rs`).
#[test]
fn every_export_links_into_staticlib() {
    let Some(compiler) = find_c_compiler("symbol_presence") else {
        eprintln!(
            "SKIP: no C compiler matching target_env={} was found (see the \
             target_env= decision line above) — cannot prove the staticlib \
             carries all exports on this machine",
            target_env_name()
        );
        return;
    };

    let exports = expected_exports();
    let root = repo_root();
    let include = root.join("include");
    assert!(
        include.join("agenterm.h").is_file(),
        "missing {} (the generated C program includes it as the declaration \
         reference)",
        include.join("agenterm.h").display()
    );
    let staticlib = locate_staticlib();
    eprintln!(
        "symbol_presence: linking static library {}",
        staticlib.display()
    );

    // Isolated scratch dir under the system temp dir — never the repo tree,
    // never in `git status`.
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!(
        "agenterm-symbol-presence-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let _cleanup = Cleanup(scratch.clone());

    // ---- 1. generate the address-table C program from exports.txt ---------
    // Table content is GENERATED from exports.txt (the SSOT) — never
    // hand-written, or it would drift exactly like the 54-line lists this
    // gate replaces.
    let c_file = scratch.join("symbol_presence.c");
    let mut src = String::new();
    src.push_str("#include <stddef.h>\n");
    src.push_str("#include <agenterm.h>\n");
    src.push_str("/* Generated from exports.txt by symbol_presence.rs: takes the\n");
    src.push_str("   ADDRESS of every export, never calls one. */\n");
    src.push_str("static void (*const table[])(void) = {\n");
    for name in &exports {
        src.push_str(&format!("    (void (*)(void)){name},\n"));
    }
    src.push_str("};\n");
    src.push_str("int main(void) {\n");
    src.push_str("    size_t i;\n");
    src.push_str("    for (i = 0; i < sizeof(table) / sizeof(table[0]); i++) {\n");
    src.push_str("        if (table[i] == 0) {\n");
    src.push_str("            return 1;\n");
    src.push_str("        }\n");
    src.push_str("    }\n");
    src.push_str("    return 0;\n");
    src.push_str("}\n");
    std::fs::write(&c_file, &src).expect("write generated C program");

    // ---- 2. compile + link against the staticlib --------------------------
    let exe_name = if cfg!(windows) {
        "symbol_presence.exe"
    } else {
        "symbol_presence"
    };
    let exe = scratch.join(exe_name);
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
        // cl re-parses the raw command line (not CommandLineToArgvW rules), so
        // each path-bearing option is a single argument; CWD = scratch keeps
        // the .obj and .exe out of the repo tree.
        cc.current_dir(&scratch);
        cc.arg("/nologo").arg("/W4").arg("/WX");
        cc.arg(format!("/I{}", include.display()));
        cc.arg(&c_file);
        cc.arg("/Fosymbol_presence.obj");
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
    // A missing export makes the LINKER fail (undefined reference to the
    // symbol); `run_or_panic` echoes the linker's stderr verbatim, which
    // names the offending symbol.
    run_or_panic("symbol_presence static compile/link", &mut cc);

    // ---- 3. run it: exit 0 means every address was non-NULL ---------------
    run_or_panic("symbol_presence static probe run", &mut Command::new(&exe));

    eprintln!(
        "symbol_presence: {} static symbols linked ({}, profile={})",
        exports.len(),
        staticlib.display(),
        profile_name()
    );
}
