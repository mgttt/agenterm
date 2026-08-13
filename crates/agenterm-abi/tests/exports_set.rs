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

/// Boundary gate: `include/agenterm.h` is a public C header compiled by
/// external consumers, so it must be pure ASCII. A non-ASCII byte (e.g. an
/// em dash, arrow, or section sign) triggers MSVC C4819 under CJK code pages
/// and breaks `/WX` builds. Fails on the first offending byte, naming its
/// line number and the line content for easy location.
#[test]
fn header_is_pure_ascii() {
    let manifest = manifest();
    let repo_root = manifest.parent().unwrap().parent().unwrap();
    let header = repo_root.join("include/agenterm.h");
    let bytes = fs::read(&header)
        .unwrap_or_else(|e| panic!("include/agenterm.h not found at {}: {e}", header.display()));
    let mut line_no = 1usize;
    let mut line_start = 0usize;
    for (idx, &b) in bytes.iter().enumerate() {
        if b < 0x80 {
            if b == b'\n' {
                line_no += 1;
                line_start = idx + 1;
            }
            continue;
        }
        let line_end = bytes[idx..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(bytes.len(), |p| idx + p);
        let line = String::from_utf8_lossy(&bytes[line_start..line_end]);
        panic!("include/agenterm.h contains non-ASCII byte 0x{b:02x} at line {line_no}: {line}");
    }
}

/// The third boundary gate from plan §14 ("产品名闸"): an export must carry
/// the `agt_` prefix and must name an OS MECHANISM, never a product concept.
///
/// The mechanism layer's whole premise is that it knows nothing about the
/// products above it, and export names are where that leaks first: once
/// `agt_tab_activate` exists, the boundary has already moved and every
/// consumer inherits agenterm-con's vocabulary. The other two gates
/// (exports.txt as the single source of truth, header/implementation drift)
/// were built long ago; this one had not been.
///
/// Today's 55 exports are all mechanisms -- pty, window, screenshot, process,
/// a11y, clipboard, parent_console, runtime, input, screen, native_window --
/// so this gate is green on arrival. That is the point: it pins a discipline
/// that currently holds, so the first violation is the one that turns red.
///
/// Matching is per underscore-separated SEGMENT, never substring. A substring
/// check would reject `agt_parent_console_write_stdout` for containing "con",
/// which is exactly the sort of false positive that gets a gate deleted.
#[test]
fn exports_name_mechanisms_not_products() {
    /// Product names and product-layer vocabulary. Each entry is a concept
    /// that belongs to a consumer, not to an OS mechanism.
    const PRODUCT_WORDS: &[&str] = &[
        // Product names.
        "agenterm",
        "con",
        "cu",
        // agenterm-con's own vocabulary: a terminal multiplexer's concepts,
        // not something an operating system offers.
        "tab",
        "session",
        "workspace",
        "pane",
        "split",
        // Presentation choices that belong to a product's UI.
        "theme",
        "palette",
        "layout",
        "profile",
        "prompt",
        // "terminal" is deliberately here: the mechanism the OS provides is a
        // PTY, and `agt_pty_*` already names it. An `agt_terminal_*` export
        // would mean a product concept had been pushed down.
        "terminal",
        // The window-placement catalog is a cu-level concept; the mechanism
        // is agt_native_window_move / _rect.
        "spectacle",
    ];

    let mut violations: Vec<String> = Vec::new();
    for name in expected_exports() {
        let Some(rest) = name.strip_prefix("agt_") else {
            violations.push(format!("{name}: missing the agt_ prefix"));
            continue;
        };
        for segment in rest.split('_') {
            if PRODUCT_WORDS.contains(&segment) {
                violations.push(format!(
                    "{name}: segment {segment:?} is a product concept, not an OS mechanism"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{} export name(s) carry product vocabulary across the mechanism \
         boundary:\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}
