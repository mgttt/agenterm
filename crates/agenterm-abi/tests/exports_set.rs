//! Milestone 1 boundary gate: the `#[no_mangle]` export set in `src/lib.rs`
//! must exactly match `exports.txt` (the single source of truth for generated
//! `.def` / version scripts / `-exported_symbols_list`). One extra or one
//! missing symbol fails this test.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn expected_exports() -> BTreeSet<String> {
    let raw = fs::read_to_string(manifest().join("exports.txt"))
        .expect("exports.txt must exist next to the crate root");
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Extract the set of `#[unsafe(no_mangle)] pub extern "C" fn NAME` declarations
/// from `src/lib.rs`.
fn actual_no_mangle_exports() -> BTreeSet<String> {
    let src = fs::read_to_string(manifest().join("src/lib.rs"))
        .expect("src/lib.rs must exist in the crate");
    let lines: Vec<&str> = src.lines().collect();
    let mut set = BTreeSet::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line == "#[unsafe(no_mangle)]" || line == "#[no_mangle]" {
            // Consume any attribute lines before the fn keyword.
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().starts_with('#') {
                j += 1;
            }
            let decl = lines[j].trim();
            if let Some(rest) = decl.strip_prefix("pub extern \"C\" fn ")
                && let Some(name) = rest.split('(').next()
            {
                let name = name.trim();
                assert!(
                    name.starts_with("agt_"),
                    "exported symbol must be agt_-prefixed: {name}"
                );
                set.insert(name.to_owned());
            }
            i = j;
        }
        i += 1;
    }
    set
}

#[test]
fn exports_set_matches_exports_txt() {
    let expected = expected_exports();
    let actual = actual_no_mangle_exports();
    assert_eq!(
        expected, actual,
        "export set mismatch: expected={expected:?} actual={actual:?}"
    );
}

#[test]
fn exports_txt_is_not_empty() {
    let exports = expected_exports();
    assert!(
        !exports.is_empty(),
        "exports.txt must list at least one symbol"
    );
}

/// Extract the set of C function declarations from `include/agenterm.h`:
/// every occurrence of `agt_<lowercase...>(` on a non-comment line. Type
/// names (`agt_pty_t`, `agt_status`, `agt_pty_spawn`) never match because they
/// are not immediately followed by `(`.
fn declared_header_functions(header_text: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in header_text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("/*") || t.starts_with('*') || t.starts_with("//") {
            continue;
        }
        let bytes = t.as_bytes();
        let mut i = 0;
        while i + 4 <= bytes.len() {
            if bytes[i..].starts_with(b"agt_") {
                let mut j = i + 4;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                let name = &t[i..j];
                // Only lowercase tails (excludes AGT_CAP_* constants) directly
                // followed by `(` (excludes type names like `agt_pty_t`).
                let tail_starts_lower = name.as_bytes().get(4).is_some_and(u8::is_ascii_lowercase);
                let followed_by_paren = t[j..].trim_start().starts_with('(');
                if tail_starts_lower && followed_by_paren {
                    set.insert(name.to_owned());
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
    set
}

/// Boundary gate 2 (§14.5): `include/agenterm.h` must declare exactly the
/// exported symbol set — every export is declared, and no extra agt_ function
/// is declared that the library does not export.
#[test]
fn header_declares_exactly_the_exported_symbols() {
    let manifest = manifest();
    let repo_root = manifest.parent().unwrap().parent().unwrap();
    let header = repo_root.join("include/agenterm.h");
    let text = fs::read_to_string(&header)
        .unwrap_or_else(|e| panic!("include/agenterm.h not found at {}: {e}", header.display()));
    let declared = declared_header_functions(&text);
    let expected = expected_exports();
    assert_eq!(
        expected, declared,
        "header/export set mismatch: exports.txt={expected:?} header={declared:?}"
    );
}
