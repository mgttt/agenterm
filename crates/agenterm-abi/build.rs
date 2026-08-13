//! Milestone 56: give the shared library an INSTALL IDENTITY.
//!
//! Milestones 51–55 pinned the symbol surface; this file pins the two fields
//! that let a shared library be INSTALLED and located by consumers:
//!
//! - Linux: `DT_SONAME` (ELF dynamic segment). Without it a consumer's
//!   `DT_NEEDED` records whatever filename the linker was given at build
//!   time, so versioned coexistence (`libagenterm.so.1` / `.so.2`) is
//!   impossible. We emit `-soname libagenterm.so.1`.
//! - macOS: `LC_ID_DYLIB` install name. Cargo builds default to writing the
//!   BUILD-TIME ABSOLUTE PATH (e.g.
//!   `<build-tree>/target/abi-release/libagenterm.dylib`),
//!   which consumers then record and cannot resolve after install. We emit
//!   `-install_name @rpath/libagenterm.dylib`, the conventional rpath-relative
//!   install name.
//! - Windows: neither concept exists; the PE branch below emits NOTHING.
//!
//! Measured/asserted by `tests/install_identity.rs` (milestone 56), which
//! first prints the real values and then gates: ELF `DT_SONAME` must exist
//! and be non-empty; Mach-O install name must not leak the build tree and
//! must start with `@rpath/`.
//!
//! # Why `cargo:rustc-cdylib-link-arg` and not `cargo:rustc-link-arg`
//! `cargo:rustc-cdylib-link-arg` reaches ONLY the cdylib artifact; plain
//! `cargo:rustc-link-arg` would also apply to every test/example binary this
//! crate builds, and an install name / soname is meaningless on an executable.
//!
//! # Milestone 55 lesson: the mechanism works, the flag was the problem
//! Milestone 55 briefly shipped a build.rs that passed
//! `-Wl,-unexported_symbols_list,<file>`; the link-arg mechanism itself
//! worked (the flag demonstrably reached ld64), but ld64 rejected the link
//! because rustc always passes its own `-exported_symbols_list` for a macOS
//! cdylib and "-exported_symbol*, -unexported_symbol* and -no_exported_symbols
//! cannot be used together". That conflict is specific to EXPORT-list flags.
//! The flags used here (`-install_name`, `-soname`) are NOT export-list
//! flags: they set a header/segment field and do not interact with any
//! export-list ld64/rustc machinery, so the milestone 55 conflict cannot
//! recur. The build.rs mechanism is the same one proven by 55 — only the
//! flag set is different.
//!
//! # Host vs target
//! This file runs on the BUILD HOST for the TARGET. `#[cfg(target_os)]` here
//! would reflect the HOST, not the target — the target OS comes from the
//! `CARGO_CFG_TARGET_OS` environment variable. Non-Linux/non-macOS targets
//! must be completely side-effect free (no link-arg emitted, no file
//! written), which is exactly what the Windows CI/build gate verifies.

use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "macos" => {
            // Conventional rpath-relative install name. Consumers must set
            // their own rpath (e.g. `-Wl,-rpath,@loader_path` or the install
            // libdir) to locate the dylib at runtime; the .pc template does
            // NOT add an rpath today (out-of-scope contract, see
            // packaging/pkgconfig/README.md milestone 56 note).
            println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libagenterm.dylib");
        }
        "linux" => {
            // SONAME version choice: the ELF soname tracks the ABI MAJOR, not
            // the crate version. The crate is 0.1.16, but a soname is a
            // linker-level ABI identity: consumers link against
            // `libagenterm.so.1` and the dynamic loader resolves that name at
            // runtime, so bumping it must mean "ABI breaking change" — exactly
            // the contract of ABI_MAJOR (see `abi_version!(1, 6)` and its docs
            // in src/lib.rs: major only moves on breaking changes, consumers
            // must reject a mismatched major). Crate version bumps (0.1.16 ->
            // 0.1.17) are additive and must NOT change the soname, or every
            // consumer would need a rebuild. Hence libagenterm.so.1.
            //
            // Drift guard: the `1` below is ABI_MAJOR from src/lib.rs. A
            // build.rs cannot read the crate's compiled constants (it runs
            // before compilation), so pin the source text instead: if the
            // major ever changes, this build fails loudly instead of silently
            // shipping a stale soname.
            let lib_src = std::fs::read_to_string(manifest_dir.join("src/lib.rs"))
                .expect("read crates/agenterm-abi/src/lib.rs for the ABI major drift guard");
            assert!(
                lib_src.contains("abi_version!(1,"),
                "ABI major drift: src/lib.rs no longer declares abi_version!(1, ...) but this \
                 build.rs pins the ELF soname to libagenterm.so.1. Update the SONAME here to \
                 match the new ABI major in src/lib.rs"
            );
            println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libagenterm.so.1");
        }
        // Windows (and any other target): PE has no SONAME / LC_ID_DYLIB
        // concept — deliberately NOTHING, no link-arg, no file.
        _ => {}
    }
}
