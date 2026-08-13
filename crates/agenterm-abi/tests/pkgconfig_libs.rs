//! Milestone 42 anti-drift gate: the per-platform system-library lists
//! documented for pkg-config consumers (`packaging/pkgconfig/README.md`,
//! which feeds `Libs.private` in `packaging/pkgconfig/libagenterm.pc.in`)
//! must EXACTLY match the lists `tests/c_static_link.rs` actually links
//! with. Those lists were measured empirically (milestone 18 / 21b / 21c);
//! if someone later changes the link arguments (e.g. a new dependency pulls
//! in a new framework) without updating the pkg-config record, consumers
//! would fail to link with a silently stale list. This gate fails the test
//! on the spot.
//!
//! Code side is the shared `common::system_libs` constants — the same ones
//! `c_static_link.rs` builds its link command from, so there is only one
//! list in code. Doc side is the machine-readable table in
//! `packaging/pkgconfig/README.md`; the table row format is the parse
//! anchor, so keep the `| <platform> |` rows and their backtick cells
//! intact.

mod common;

use std::path::{Path, PathBuf};

/// Repository root: this crate lives at <root>/crates/agenterm-abi.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ dir missing")
        .parent()
        .expect("repo root missing")
        .to_path_buf()
}

/// The `Libs.private` value recorded for one platform in the README table:
/// the whitespace-split tokens of the backtick cell of the `| <platform> |`
/// row. A missing/restructured row is a hard failure, never a silent pass.
fn doc_libs(readme: &str, platform_marker: &str) -> Vec<String> {
    let row = readme
        .lines()
        .find(|l| l.trim_start().starts_with(platform_marker))
        .unwrap_or_else(|| {
            panic!(
                "packaging/pkgconfig/README.md has no row starting \
                 {platform_marker:?} (the table is the anti-drift parse \
                 anchor — keep the `| <platform> |` rows intact)"
            )
        });
    let cell = row
        .split('`')
        .nth(1)
        .unwrap_or_else(|| panic!("README row {row:?} has no backtick cell"));
    cell.split_whitespace().map(str::to_string).collect()
}

/// Assert one platform's documented list equals the linked list. On
/// mismatch, `assert_eq!`'s diff prints both token sequences — that is the
/// pointer to exactly what differs.
fn assert_platform_lists_match(readme: &str, platform: &str, marker: &str, linked: &[&str]) {
    let doc = doc_libs(readme, marker);
    let code: Vec<String> = linked.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        doc, code,
        "pkg-config `Libs.private` record for {platform} drifted from the lists \
         c_static_link.rs actually links with. Single source of truth: \
         tests/common/mod.rs::system_libs. Update the constant AND the README \
         table in the same change.",
    );
}

/// Core anti-drift check: every platform list the README records for
/// pkg-config consumers is byte-identical (token-for-token, order included —
/// order matters on macOS) to the lists the static-link test links with.
#[test]
fn doc_system_libs_match_link_system_libs() {
    let readme = std::fs::read_to_string(repo_root().join("packaging/pkgconfig/README.md"))
        .expect("packaging/pkgconfig/README.md must exist (milestone 42 deliverable)");

    assert_platform_lists_match(
        &readme,
        "Windows/MSVC",
        "| Windows/MSVC |",
        common::system_libs::MSVC,
    );
    assert_platform_lists_match(&readme, "Linux", "| Linux |", common::system_libs::LINUX);
    assert_platform_lists_match(&readme, "macOS", "| macOS |", common::system_libs::MACOS);
}

/// The `.pc.in` template keeps its pkg-config shape: the standard fields
/// (`Name` / `Description` / `Version` / `Cflags` / `Libs` / `Libs.private`)
/// and the `-lagenterm` link name (the artifact is `libagenterm.{a,so,dylib}`
/// since milestone 17 — a rename back to another lib name would silently
/// break consumers, so the template shape is part of the deliverable).
#[test]
fn pc_in_template_shape_ok() {
    let pc_in = std::fs::read_to_string(repo_root().join("packaging/pkgconfig/libagenterm.pc.in"))
        .expect("packaging/pkgconfig/libagenterm.pc.in must exist (milestone 42 deliverable)");
    for required in [
        "prefix=@PREFIX@",
        "libdir=@LIBDIR@",
        "includedir=@INCLUDEDIR@",
        "Name: libagenterm",
        "Description: agenterm mechanism ABI (C boundary to OS terminal/window/process mechanisms)",
        "Version: @VERSION@",
        "Cflags: -I${includedir}",
        "Libs: -L${libdir} -lagenterm",
        "Libs.private: @SYSTEM_LIBS@",
    ] {
        assert!(
            pc_in.contains(required),
            "libagenterm.pc.in must contain {required:?} — the pkg-config template \
             shape is part of the deliverable; keep the fields intact"
        );
    }
}
