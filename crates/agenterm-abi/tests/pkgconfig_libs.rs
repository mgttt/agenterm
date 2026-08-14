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
//!
//! Milestone 52 added the third leg: the `SYSTEM_LIBS_*` sets baked into
//! `packaging/pkgconfig/generate-pc.sh` (what a real `pkg-config --static`
//! consumer actually receives as `Libs.private`) must match too, closing
//! the three-way gate README table ↔ common::system_libs ↔ generate-pc.sh.

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

/// The `Libs.private` value embedded in `generate-pc.sh` for one platform:
/// the whitespace-split tokens of the single-quoted `SYSTEM_LIBS_<OS>='...'`
/// assignment (the script's own selection data, shipped to consumers as-is).
/// The assignment line format is the parse anchor — mirroring the README
/// table rows — so keep it single-line and single-quoted.
fn script_libs(script: &str, var: &str) -> Vec<String> {
    let prefix = format!("{var}=");
    let line = script
        .lines()
        .find(|l| l.trim_start().starts_with(&prefix))
        .unwrap_or_else(|| {
            panic!(
                "generate-pc.sh has no {var} assignment (the three-way gate \
                 parse anchor — keep the `SYSTEM_LIBS_<OS>='...'` line intact)"
            )
        });
    let value = line.split('\'').nth(1).unwrap_or_else(|| {
        panic!("generate-pc.sh {var} assignment has no single-quoted value: {line:?}")
    });
    value.split_whitespace().map(str::to_string).collect()
}

/// Milestone 52 third leg: the `SYSTEM_LIBS` sets baked into
/// `generate-pc.sh` must match the linked lists too, so the pkg-config
/// metadata a real consumer receives (`Libs.private`) can never silently
/// drift from what `c_static_link.rs` actually links with. With the other
/// two legs (README table <-> common, script <-> common) this closes the
/// three-way gate: README table ↔ common::system_libs ↔ generate-pc.sh.
#[test]
fn script_system_libs_match_link_system_libs() {
    let script = std::fs::read_to_string(repo_root().join("packaging/pkgconfig/generate-pc.sh"))
        .expect("packaging/pkgconfig/generate-pc.sh must exist (milestone 52 deliverable)");

    let linux = script_libs(&script, "SYSTEM_LIBS_LINUX");
    let macos = script_libs(&script, "SYSTEM_LIBS_DARWIN");
    let code_linux: Vec<String> = common::system_libs::LINUX
        .iter()
        .map(|s| s.to_string())
        .collect();
    let code_macos: Vec<String> = common::system_libs::MACOS
        .iter()
        .map(|s| s.to_string())
        .collect();

    assert_eq!(
        linux, code_linux,
        "generate-pc.sh embedded SYSTEM_LIBS_LINUX drifted from \
         common::system_libs::LINUX (the list c_static_link.rs actually links \
         with). Single source of truth: tests/common/mod.rs::system_libs. \
         Update the constant, the README table AND the script in the same \
         change.",
    );
    assert_eq!(
        macos, code_macos,
        "generate-pc.sh embedded SYSTEM_LIBS_DARWIN drifted from \
         common::system_libs::MACOS (the list c_static_link.rs actually links \
         with). Single source of truth: tests/common/mod.rs::system_libs. \
         Update the constant, the README table AND the script in the same \
         change.",
    );
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

/// The fourth copy of the MSVC system-library list: the copy-pasteable `cl`
/// command line in `crates/agenterm-abi/README.md`.
///
/// `packaging/pkgconfig/README.md` sends MSVC users there by name, because
/// pkg-config is not how anyone links on Windows, so that command line is the
/// list a Windows consumer actually reads -- and until now nothing compared
/// it to anything. The other three copies have been gated against each other
/// since milestone 42; this one drifted freely.
///
/// Milestone 71b (anchor uniqueness): the parse anchor is NOT "the first `cl`
/// line in the file" anymore. A Windows-install section added above the
/// original command line silently migrated the gated object to a new example
/// line whose 6 system libraries happened to match -- the gate stayed green
/// while the line it was meant to watch drifted freely. The anchor is now
/// explicit and must be UNIQUE:
///
/// - The gated command line is the one followed (after only blank/comment
///   lines) by the marker `# AGENTERM_MSVC_SYSTEM_LIBS_ANCHOR`. The marker
///   comment sits inside the README's fenced block, directly below the
///   command -- read it before editing the anchored line.
/// - The gate collects EVERY `cl ... .lib ...` line in the README and asserts
///   exactly ONE of them is anchored. Zero anchors or several anchors both
///   fail on purpose, printing every candidate line with its 1-based line
///   number, so a second `cl` example can never again silently steal or split
///   the gate -- whatever the section order.
///
/// Keep the anchored command line on one line, with the marker line directly
/// below it inside the same fenced block. The documented list is every
/// whitespace token ending in `.lib` that is not the library's own
/// `agenterm.lib`.
#[test]
fn abi_readme_msvc_command_line_matches_link_system_libs() {
    const ANCHOR_MARKER: &str = "# AGENTERM_MSVC_SYSTEM_LIBS_ANCHOR";
    let readme = std::fs::read_to_string(repo_root().join("crates/agenterm-abi/README.md"))
        .expect("crates/agenterm-abi/README.md must exist");
    let lines: Vec<&str> = readme.lines().map(str::trim).collect();

    fn render_candidates(cands: &[(usize, &str)]) -> String {
        cands
            .iter()
            .map(|(n, l)| format!("  line {n}: {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // Every `cl ... .lib ...` command line in the file (1-based line number).
    // All are candidates; exactly one may be the anchor.
    let candidates: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("cl ") && l.contains(".lib"))
        .map(|(i, l)| (i + 1, *l))
        .collect();
    if candidates.is_empty() {
        panic!(
            "crates/agenterm-abi/README.md has no `cl ... .lib ...` command line \
             (that command is the anti-drift parse anchor for the MSVC system \
             libraries -- keep it on a single line inside its fenced block)"
        );
    }

    // The anchored candidate: the one whose directly following lines (only
    // blank/comment lines in between) contain the anchor marker. Skipping
    // blank/comment lines lets the marker block reflow without dropping the
    // anchor, while a different command line in between still breaks it.
    let anchored: Vec<(usize, &str)> = candidates
        .iter()
        .copied()
        .filter(|&(lineno, _)| {
            lines[lineno..]
                .iter()
                .take_while(|l| l.is_empty() || l.starts_with('#'))
                .any(|l| l.starts_with(ANCHOR_MARKER))
        })
        .collect();
    if anchored.is_empty() {
        panic!(
            "crates/agenterm-abi/README.md has {} `cl ... .lib ...` line(s) but none \
             carries the anti-drift anchor marker {ANCHOR_MARKER:?}. The gated command \
             line is the one followed by that marker (see the marker's comment in the \
             README -- read it before editing the anchored line). A missing anchor is \
             a hard failure, never a silent pass. All `cl` candidate lines found:\n{}",
            candidates.len(),
            render_candidates(&candidates),
        );
    }
    if anchored.len() > 1 {
        panic!(
            "crates/agenterm-abi/README.md has {} anchored `cl ... .lib ...` line(s) but \
             the anti-drift gate requires EXACTLY ONE. The anchor is unique by design: \
             only the command line followed by the marker {ANCHOR_MARKER:?} is watched, \
             so a second `cl` example that also carries the marker steals or splits the \
             gate. New `cl` examples are welcome but must NOT be followed by the anchor \
             marker line -- see the marker's comment in the README. Anchored lines:\n{}\n\
             All `cl` candidate lines found:\n{}",
            anchored.len(),
            render_candidates(&anchored),
            render_candidates(&candidates),
        );
    }
    let line = anchored[0].1;
    let documented: Vec<String> = line
        .split_whitespace()
        .filter(|t| t.ends_with(".lib"))
        .filter(|t| !t.ends_with("agenterm.lib"))
        .map(|t| t.trim_end_matches(".lib").to_owned())
        .collect();
    let linked: Vec<String> = common::system_libs::MSVC
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(
        documented, linked,
        "the anchored `cl` command line in crates/agenterm-abi/README.md (the one \
         followed by {ANCHOR_MARKER:?}) drifted from common::system_libs::MSVC (the \
         list c_static_link.rs actually links with). Single source of truth: \
         tests/common/mod.rs::system_libs. Update the constant, the packaging README \
         table, generate-pc.sh AND this command line in the same change."
    );
}
