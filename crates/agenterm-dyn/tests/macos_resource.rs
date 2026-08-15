//! Darwin resource-ownership catalogue guards.
//!
//! `mach_host_self` creates a Mach send right.  The generic `dlcall` door has
//! no ownership-aware release operation, so the catalogue must retain the
//! honest placeholder instead of treating allocation as a harmless probe.

#![cfg(target_os = "macos")]

use std::fs;
use std::path::Path;

use agenterm_dyn::{ALL_CELLS, SystemProbeStatus, live_cell};

#[test]
fn mach_host_self_is_catalogued_but_never_callable_without_a_release_owner() {
    let probe = live_cell()
        .expect("macOS host cell")
        .system_probes
        .iter()
        .find(|probe| probe.name == "mach_host_self")
        .expect("Mach host probe is catalogued");

    assert!(matches!(probe.status, SystemProbeStatus::Placeholder));
}

#[test]
fn mach_host_self_is_placeholder_on_every_six_cell_row() {
    for cell in ALL_CELLS {
        let probe = cell
            .system_probes
            .iter()
            .find(|probe| probe.name == "mach_host_self")
            .unwrap_or_else(|| panic!("{} × {} catalogues mach_host_self", cell.os, cell.arch));
        assert!(
            matches!(probe.status, SystemProbeStatus::Placeholder),
            "{} × {} must not mark mach_host_self live",
            cell.os,
            cell.arch
        );
    }
}

#[test]
fn crate_sources_do_not_dlcall_mach_host_self() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_mach_host_self_has_only_placeholder_catalogue_entries(&root.join("src"));
}

#[test]
fn mach_host_self_example_is_honesty_only() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/mach-host-self.md");
    let text = fs::read_to_string(&path).expect("honesty example exists");
    assert!(
        text.contains("send right"),
        "honesty note must name the unreaped send right"
    );
    assert!(
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("```")),
        "honesty example must not ship a code fence that could be mistaken for a live call"
    );
}

fn assert_mach_host_self_has_only_placeholder_catalogue_entries(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display())) {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            assert_mach_host_self_has_only_placeholder_catalogue_entries(&path);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let code = strip_rust_comments(&source);
        for (idx, line) in code.lines().enumerate() {
            if !line.contains("\"mach_host_self\"") {
                continue;
            }
            assert!(
                line.contains("placeholder(\"mach_host_self\")"),
                "{}:{} must keep mach_host_self as a placeholder, never a dlcall target",
                path.file_name().unwrap_or_default().to_string_lossy(),
                idx + 1
            );
        }
    }
}

/// Preserve line positions while excluding Rust line and block comments from the
/// source-level catalogue guard. String literals remain visible for inspection.
fn strip_rust_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    let mut block_depth = 0usize;

    while index < bytes.len() {
        if block_depth > 0 {
            if bytes[index..].starts_with(b"/*") {
                block_depth += 1;
                output.push_str("  ");
                index += 2;
            } else if bytes[index..].starts_with(b"*/") {
                block_depth -= 1;
                output.push_str("  ");
                index += 2;
            } else {
                output.push(if bytes[index] == b'\n' { '\n' } else { ' ' });
                index += 1;
            }
            continue;
        }

        if bytes[index..].starts_with(b"//") {
            while index < bytes.len() && bytes[index] != b'\n' {
                output.push(' ');
                index += 1;
            }
        } else if bytes[index..].starts_with(b"/*") {
            block_depth = 1;
            output.push_str("  ");
            index += 2;
        } else {
            output.push(bytes[index] as char);
            index += 1;
        }
    }

    output
}
