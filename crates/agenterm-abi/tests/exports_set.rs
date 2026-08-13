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
            if let Some(rest) = decl.strip_prefix("pub extern \"C\" fn ") {
                if let Some(name) = rest.split('(').next() {
                    let name = name.trim();
                    assert!(
                        name.starts_with("agt_"),
                        "exported symbol must be agt_-prefixed: {name}"
                    );
                    set.insert(name.to_owned());
                }
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
