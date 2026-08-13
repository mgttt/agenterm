//! Milestone 55: remove the 4 ObjC class-registration symbols from the
//! macOS dylib's EXPORT surface, so the delivered export set is EXACTLY the
//! promised 55 on every platform.
//!
//! The 4 symbols (`___CLASS_SoftbufferObserver`,
//! `___DROP_FLAG_OFFSET_SoftbufferObserver`, `___IVAR_OFFSET_SoftbufferObserver`,
//! `___REGISTER_CLASS_SoftbufferObserver` — raw Mach-O names with the C `_`
//! prefix) are emitted into the export table by softbuffer's vendored objc2
//! `declare_class!` macro: they are internal static variables (the class
//! object cache, the ivar offset, the drop-flag offset, and the registration
//! Once). They are referenced only from inside the dylib, ObjC classes are
//! registered lazily, and nothing resolves them at load time, so hiding them
//! does not affect the window/draw paths — the risk note is in the milestone
//! brief and is validated by CI (`export_exactness`, `symbol_presence`,
//! `c_consumer`, `c_window`, `dylib_load`).
//!
//! WHY `-unexported_symbols_list` AND NOT `-exported_symbols_list`:
//! `--version-script` is a GNU ld ELF feature — Mach-O has no equivalent.
//! A standalone `-exported_symbols_list` is INERT here: rustc already
//! generates and passes its own list for macOS cdylibs, and these 4 are
//! `#[export_name]` / `SymbolExportLevel::C` symbols that are already IN that
//! automatic list; ld64 takes the UNION of multiple lists, so adding a
//! second list only widens the surface back to 59. The effective tool is
//! `-unexported_symbols_list`, which SUBTRACTS from the export set.
//!
//! The reverse side of this pair is `tests/export_exactness.rs`'s
//! `allowed_non_agt()`: this file ELIMINATES the 4 symbols, that test
//! DISCOVERS any future leak (its macOS allowlist is now empty, so a fifth
//! leak turns the exactness gate red). Keep the two name lists in sync.
//!
//! IMPORTANT: this file runs on the BUILD HOST for the TARGET. Do not use
//! `#[cfg(target_os)]` here — that reflects the HOST; the target OS comes
//! from the `CARGO_CFG_TARGET_OS` environment variable. Non-macOS targets
//! must be completely side-effect free (no link-arg emitted), which is also
//! what the Windows CI/build gate verifies.

use std::path::PathBuf;

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        // Non-macOS: deliberately NOTHING. No file, no link-arg. The
        // `-unexported_symbols_list` flag is ld64-only; emitting it for
        // ELF/COFF would break the link.
        return;
    }

    // Linker-namespace names (Mach-O prefixes C symbols with one `_`, so the
    // raw symbol `___CLASS_SoftbufferObserver` has THREE leading underscores).
    // Same 4 as the (now empty) macOS allowlist in tests/export_exactness.rs.
    const UNEXPORTED: [&str; 4] = [
        "___CLASS_SoftbufferObserver",
        "___DROP_FLAG_OFFSET_SoftbufferObserver",
        "___IVAR_OFFSET_SoftbufferObserver",
        "___REGISTER_CLASS_SoftbufferObserver",
    ];

    // <OUT_DIR> is absolute, so the linker flag below never drifts with the
    // build CWD. Do not "simplify" this to a repo-relative path (e.g. via
    // .cargo/config.toml): relative paths are resolved against the process
    // CWD and silently miss the file in nested builds.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let list_path = out_dir.join("unexported_symbols.txt");
    std::fs::write(&list_path, UNEXPORTED.join("\n") + "\n")
        .expect("write <OUT_DIR>/unexported_symbols.txt");
    // cdylib-only on purpose. `cargo:rustc-link-arg` would also reach every
    // test and example binary this crate builds, and subtracting from an
    // export set is meaningless anywhere but the cdylib — the one artifact
    // that has one.
    println!(
        "cargo:rustc-cdylib-link-arg=-Wl,-unexported_symbols_list,{}",
        list_path.display()
    );
}
