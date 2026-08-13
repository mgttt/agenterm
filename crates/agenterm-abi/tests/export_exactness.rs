//! Milestone 54 gate: the built cdylib's EXPORT SET is EXACTLY the
//! `exports.txt` list ∪ a per-platform allowlist — not merely a superset.
//!
//! Milestone 51's `symbol_presence.rs` proved every promised symbol is
//! present in both delivered artifacts, but never proved NOTHING ELSE leaks:
//! macOS shipped 4 undeclared Objective-C class registration symbols
//! (`___CLASS_SoftbufferObserver` etc., emitted by softbuffer's objc2
//! `declare_class!`), so the promised "55 exports" surface was really 59 on
//! one of three platforms and nobody noticed. This gate is the exactness half
//! of that promise: it parses the BUILD ARTIFACT's export table and asserts
//! the bidirectional identity `exports == exports.txt ∪ allowlist`.
//!
//! Parsing uses the pure-Rust `object` crate — one code path across the three
//! formats, never shelling out to nm/dumpbin:
//! - ELF (`.so`): defined symbols in the DYNAMIC symbol table (`File::exports`
//!   iterates dynsym and keeps `is_definition`; the static symbol table of a
//!   release `.so` is nearly empty — milestone 40 learned that the hard way);
//! - Mach-O (`.dylib`): external-DEFINED symbols (dysymtab
//!   `iextdefsym`/`nextdefsym`, the same table `nm -gU` reads). Mach-O adds a
//!   leading `_` to C symbol names (`_agt_pty_open`), stripped before
//!   comparison so all three platforms compare in the same name space;
//! - PE (`.dll`): the named entries of the export directory.
//!
//! Both directions are asserted and reported separately: declared-but-missing
//! and undeclared-but-present. The printout (`total=N agt=N extra=[...]`)
//! preserves the old `Symbol surface (nm)` step's log readability — that step
//! was deleted from CI because it asserted nothing (`|| true` everywhere) and
//! printed a wrong number on macOS (`agt=0` for the static archive); the
//! correct numbers now come from HERE, with assertions behind them.

use common::toolchain::{locate_cdylib, profile_name};
use object::{File, Object};
use std::collections::BTreeSet;
use std::path::Path;

mod common;

/// The expected export list, read straight from `exports.txt` — the single
/// source of truth. Never a hand-typed 55-line list: a hand-written list is
/// exactly the drift this gate exists to catch.
fn expected_exports() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("exports.txt");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("exports.txt not found at {}: {e}", path.display()));
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Platform allowlist of exports that are NOT part of the promised C ABI.
///
/// Must be EMPTY on ALL THREE platforms: CI measured zero extra symbols
/// there, so leaving the list empty is what makes a FUTURE leak turn this
/// gate red instead of silently widening the surface. Names live in the
/// stripped name space this test compares in (see `exported_names`).
///
/// macOS specifics: milestone 55 removed the 4 softbuffer objc2
/// `declare_class!` registration symbols
/// (`___CLASS_SoftbufferObserver` / `___DROP_FLAG_OFFSET_SoftbufferObserver`
/// / `___IVAR_OFFSET_SoftbufferObserver` / `___REGISTER_CLASS_SoftbufferObserver`)
/// from the dylib export surface with `-unexported_symbols_list` — see
/// `crates/agenterm-abi/build.rs` (this file DISCOVERS a leak, that file
/// ELIMINATES these 4; keep the two name lists in sync). The build script
/// comment explains why `-exported_symbols_list` alone cannot work (ld64
/// takes the union of multiple lists). With the symbols gone the macOS
/// allowlist is empty like the others; if CI ever reports macOS `total=59`,
/// the `-unexported_symbols_list` did not take effect — report it, do NOT
/// re-add names here.
fn allowed_non_agt() -> &'static [&'static str] {
    &[]
}

/// Parse the exported symbol NAMES of the built cdylib with the `object`
/// crate — one code path across ELF / Mach-O / PE, no shelling out to
/// nm/dumpbin. Names are deduplicated and sorted so the printed set and the
/// diff against `exports.txt` are deterministic.
fn exported_names(lib_path: &Path) -> Vec<String> {
    let data = std::fs::read(lib_path)
        .unwrap_or_else(|e| panic!("failed to read cdylib {}: {e}", lib_path.display()));
    let file = File::parse(&*data)
        .unwrap_or_else(|e| panic!("object::File::parse({}) failed: {e}", lib_path.display()));
    let mut names: Vec<String> = file
        .exports()
        .unwrap_or_else(|e| panic!("failed to read export table of {}: {e}", lib_path.display()))
        .iter()
        .map(|e| String::from_utf8_lossy(e.name()).into_owned())
        .collect();
    if cfg!(target_os = "macos") {
        // Mach-O prefixes C symbols with `_` (`_agt_pty_open`); strip exactly
        // one leading underscore so all platforms compare in the same name
        // space. The allowlist entries above are written in this stripped
        // form too.
        for name in &mut names {
            if let Some(stripped) = name.strip_prefix('_') {
                *name = stripped.to_owned();
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The exactness gate itself. `locate_cdylib` honours
/// `AGENTERM_ABI_PROFILE_DIR` (CI sets `target/abi-release`), so both
/// delivered profiles are verified by the same test.
#[test]
fn export_surface_is_exact() {
    let lib_path = locate_cdylib();
    let expected: BTreeSet<String> = expected_exports().into_iter().collect();
    let exports: BTreeSet<String> = exported_names(&lib_path).into_iter().collect();
    let allowed: BTreeSet<&str> = allowed_non_agt().iter().copied().collect();

    let mut missing: Vec<&str> = expected
        .iter()
        .map(String::as_str)
        .filter(|n| !exports.contains(*n))
        .collect();
    missing.sort_unstable();
    let mut extra: Vec<&str> = exports
        .iter()
        .map(String::as_str)
        .filter(|n| !expected.contains(*n) && !allowed.contains(*n))
        .collect();
    extra.sort_unstable();
    let mut allowed_hit: Vec<&str> = exports
        .iter()
        .map(String::as_str)
        .filter(|n| allowed.contains(*n))
        .collect();
    allowed_hit.sort_unstable();

    let agt = exports.iter().filter(|n| n.starts_with("agt_")).count();
    eprintln!(
        "export_exactness: total={} agt={} extra={:?} allowed={:?} ({}, profile={})",
        exports.len(),
        agt,
        extra,
        allowed_hit,
        lib_path.display(),
        profile_name()
    );

    // Both directions are asserted separately so a CI log names exactly what
    // drifted, never just a count.
    assert!(
        missing.is_empty(),
        "export_exactness: {} export(s) DECLARED in exports.txt are MISSING from {}:\n  {}",
        missing.len(),
        lib_path.display(),
        missing.join("\n  ")
    );
    assert!(
        extra.is_empty(),
        "export_exactness: {} UNDECLARED export(s) leak the surface of {} (not in \
         exports.txt and not on the platform allowlist):\n  {}",
        extra.len(),
        lib_path.display(),
        extra.join("\n  ")
    );
}
