//! Darwin resource-ownership catalogue guards.
//!
//! `mach_host_self` creates a Mach send right.  The generic `dlcall` door has
//! no ownership-aware release operation, so the catalogue must retain the
//! honest placeholder instead of treating allocation as a harmless probe.

#![cfg(target_os = "macos")]

use std::fs;
use std::path::Path;

use agenterm_dyn::{live_cell, SystemProbeStatus, ALL_CELLS};

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
    for rel in ["src", "tests"] {
        assert_no_dlcall_of_mach_host_self(&root.join(rel));
    }
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
        !text.contains("```lisp"),
        "honesty example must not ship a live-call lisp fence"
    );
}

fn assert_no_dlcall_of_mach_host_self(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display())) {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            assert_no_dlcall_of_mach_host_self(&path);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        for (idx, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            assert!(
                !(line.contains("dlcall") && line.contains("\"mach_host_self\"")),
                "{}:{} must not live-call mach_host_self",
                path.file_name().unwrap_or_default().to_string_lossy(),
                idx + 1
            );
        }
    }
}
