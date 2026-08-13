//! Milestone 53 gate: the AGT_CAP_* discriminant numbers are copied in three
//! places, and until now nothing compared them:
//!
//! 1. `crates/agenterm-abi/src/lib.rs` — `pub enum agt_capability` (16
//!    variants, discriminants 1..=16);
//! 2. `include/agenterm.h` — the C `agt_capability` enum (`AGT_CAP_*`);
//! 3. `crates/agenterm-abi/tests/common/mod.rs` — `common::capabilities`, the
//!    single hand-written test-side copy consumed by the black-box dlopen
//!    tests (`dylib_load.rs`).
//!
//! This gate asserts all three agree on BOTH the name sequence AND the
//! numeric value of every variant — comparing only the name sets would miss
//! an insert that shifts everything after it. Any drift (an insert, a rename,
//! a reorder, a revalue) fails loudly instead of silently pointing the tests
//! or the C consumers at the wrong capability.
//!
//! The Rust side is parsed from `src/lib.rs` source text (same approach as
//! `actual_no_mangle_exports()` in `exports_set.rs`) — never a hand-written
//! 16-row expected table, which is exactly the drift this gate exists to
//! catch. The header side follows C enum semantics: an explicit decimal
//! value wins, otherwise the value is the previous member's value + 1 (the
//! first member without an explicit value starts at 0). Both files only use
//! decimal literals today; an expression like `1 << 4` would make the parser
//! panic (fail loud) rather than guess.

mod common;

use std::fs;
use std::path::PathBuf;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest()
        .parent()
        .expect("crates/ dir missing")
        .parent()
        .expect("repo root missing")
        .to_path_buf()
}

/// Parse the `agt_capability` enum out of `src/lib.rs` source text, returning
/// `(name, discriminant)` pairs in declaration order. Supports explicit
/// decimal discriminants (`NAME = N,`) and implicit continuation (previous
/// value + 1, first member starting at 0).
fn rust_capability_enum() -> Vec<(String, u64)> {
    let src = fs::read_to_string(manifest().join("src/lib.rs"))
        .expect("src/lib.rs must exist in the crate");
    let lines: Vec<&str> = src.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim() == "pub enum agt_capability {")
        .unwrap_or_else(|| panic!("pub enum agt_capability {{ not found in src/lib.rs"));
    let mut members: Vec<(String, u64)> = Vec::new();
    let mut next: u64 = 0;
    for line in &lines[start + 1..] {
        let t = line.trim();
        if t.starts_with('}') {
            break;
        }
        if t.is_empty() || t.starts_with("//") || t.starts_with("///") || t.starts_with('#') {
            continue;
        }
        let body = t.strip_suffix(',').unwrap_or(t).trim();
        let (name, value) = match body.split_once('=') {
            Some((n, v)) => {
                let value = v.trim().parse::<u64>().unwrap_or_else(|e| {
                    panic!("enum member {n:?} has a non-decimal discriminant {v:?}: {e}")
                });
                (n.trim(), value)
            }
            None => (body, next),
        };
        assert!(
            name.starts_with("AGT_CAP_"),
            "unexpected member in agt_capability enum: {name:?}"
        );
        members.push((name.to_owned(), value));
        next = value + 1;
    }
    assert!(
        !members.is_empty(),
        "agt_capability enum in src/lib.rs parsed as empty"
    );
    members
}

/// Parse the C `agt_capability` enum out of `include/agenterm.h`, returning
/// `(name, value)` pairs in declaration order.
///
/// Implicit-numbering assumption (documented per the milestone 53 brief): the
/// header uses C enum semantics — an explicit decimal value wins, otherwise
/// the value is the previous member's value + 1, and the first member without
/// an explicit value starts at 0. Only decimal literals are supported; any
/// other expression panics (fail loud) instead of being guessed.
fn header_capability_enum() -> Vec<(String, u64)> {
    let header = repo_root().join("include/agenterm.h");
    let text = fs::read_to_string(&header)
        .unwrap_or_else(|e| panic!("include/agenterm.h not found at {}: {e}", header.display()));
    let mut members: Vec<(String, u64)> = Vec::new();
    let mut in_enum = false;
    let mut next: u64 = 0;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("/*") || t.starts_with('*') || t.starts_with("//") {
            continue;
        }
        if !in_enum {
            if t == "typedef enum {" {
                in_enum = true;
                members.clear();
                next = 0;
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix('}') {
            let name = rest.trim().trim_end_matches(';').trim();
            if name == "agt_capability" {
                assert!(
                    !members.is_empty(),
                    "agt_capability enum in include/agenterm.h parsed as empty"
                );
                // Only the target enum is asserted for the AGT_CAP_ prefix —
                // the scanner also walks agt_status / agt_event_kind while
                // looking for the agt_capability block, and their members are
                // legitimately named differently.
                for (member, _) in &members {
                    assert!(
                        member.starts_with("AGT_CAP_"),
                        "unexpected member in agt_capability enum: {member:?}"
                    );
                }
                return members;
            }
            // Some other typedef enum (agt_status, agt_event_kind, ...):
            // drop it and keep scanning.
            in_enum = false;
            continue;
        }
        // Strip a trailing line comment before parsing the member (e.g.
        // `AGT_UNSUPPORTED = 1, /* capability/mechanism absent */`).
        let mut code = t;
        if let Some((head, _)) = code.split_once("/*") {
            code = head;
        }
        if let Some((head, _)) = code.split_once("//") {
            code = head;
        }
        let body = code.trim().strip_suffix(',').unwrap_or(code.trim()).trim();
        let (name, value) = match body.split_once('=') {
            Some((n, v)) => {
                let value = v.trim().parse::<u64>().unwrap_or_else(|e| {
                    panic!("C enum member {n:?} has a non-decimal value {v:?}: {e}")
                });
                (n.trim(), value)
            }
            None => (body, next),
        };
        members.push((name.to_owned(), value));
        next = value + 1;
    }
    panic!("agt_capability enum not found in include/agenterm.h");
}

/// The test-side table, assembled by NAME from `common::capabilities` — the
/// values come from the gated constants, the names are string literals, so a
/// copy-paste mismatch between a name and its constant (or a revalue) shows
/// up as a name/value disagreement with the two authoritative sources.
fn test_capability_table() -> Vec<(&'static str, u64)> {
    use common::capabilities as c;
    vec![
        ("AGT_CAP_PTY", c::AGT_CAP_PTY as u64),
        ("AGT_CAP_PROCESS_SPAWN", c::AGT_CAP_PROCESS_SPAWN as u64),
        ("AGT_CAP_PROCESS_OBSERVE", c::AGT_CAP_PROCESS_OBSERVE as u64),
        ("AGT_CAP_WINDOW_HOST", c::AGT_CAP_WINDOW_HOST as u64),
        (
            "AGT_CAP_WINDOW_ENUMERATE",
            c::AGT_CAP_WINDOW_ENUMERATE as u64,
        ),
        ("AGT_CAP_WINDOW_OP", c::AGT_CAP_WINDOW_OP as u64),
        ("AGT_CAP_SCREENSHOT", c::AGT_CAP_SCREENSHOT as u64),
        ("AGT_CAP_CLIPBOARD", c::AGT_CAP_CLIPBOARD as u64),
        ("AGT_CAP_IME", c::AGT_CAP_IME as u64),
        ("AGT_CAP_INPUT_INJECT", c::AGT_CAP_INPUT_INJECT as u64),
        ("AGT_CAP_IPC", c::AGT_CAP_IPC as u64),
        ("AGT_CAP_FONT_RASTER", c::AGT_CAP_FONT_RASTER as u64),
        (
            "AGT_CAP_FILESYSTEM_PUBLISH",
            c::AGT_CAP_FILESYSTEM_PUBLISH as u64,
        ),
        ("AGT_CAP_SHARED_MEMORY", c::AGT_CAP_SHARED_MEMORY as u64),
        ("AGT_CAP_PARENT_CONSOLE", c::AGT_CAP_PARENT_CONSOLE as u64),
        (
            "AGT_CAP_ACCESSIBILITY_TREE",
            c::AGT_CAP_ACCESSIBILITY_TREE as u64,
        ),
    ]
}

/// Gate 1: the C header and the Rust enum must agree on name sequence AND
/// values. Catches an insert/rename/reorder/revalue in either file.
#[test]
fn header_matches_rust_enum_names_and_values() {
    let header = header_capability_enum();
    let rust = rust_capability_enum();
    assert_eq!(
        header, rust,
        "include/agenterm.h and src/lib.rs agt_capability disagree (names and/or \
         values must match exactly, in declaration order)"
    );
}

/// Gate 2: the test-side table (`common::capabilities`, consumed by the
/// dlopen tests) must agree with the Rust enum on name sequence AND values.
#[test]
fn test_table_matches_rust_enum_names_and_values() {
    let test: Vec<(String, u64)> = test_capability_table()
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    let rust = rust_capability_enum();
    assert_eq!(
        test, rust,
        "tests/common/mod.rs capabilities table and src/lib.rs agt_capability \
         disagree (names and/or values must match exactly, in declaration order)"
    );
}

/// Gate 3: `common::capabilities::ALL` (which drives the valid-discriminant
/// black-box loop in `dylib_load.rs`) must stay equal to the constants it is
/// documented to be assembled from — it must not silently become a second
/// hand-written list.
#[test]
fn all_array_matches_capability_constants() {
    use common::capabilities as c;
    let from_constants: Vec<i32> = test_capability_table()
        .into_iter()
        .map(|(_, v)| v as i32)
        .collect();
    assert_eq!(
        c::ALL.to_vec(),
        from_constants,
        "common::capabilities::ALL must equal the AGT_CAP_* constants in declaration order"
    );
}
