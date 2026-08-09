//! Cross-host `ui-snapshot` key-vocabulary parity gate.
//!
//! The two snapshot producers — `ui_snapshot_json` in
//! `src/platform/adapters/windows/remote_frontend.rs` and
//! `build_ui_snapshot_json` in `src/platform/adapters/unix/frontend/mod.rs`
//! — are the largest remaining frontend duplication
//! (`plan/design-frontend-shared-core.md` §1 #1, ~600 dual-written lines).
//! Until that assembly is extracted behind a shared builder, nothing
//! structural stops one host from adding a JSON key the other never emits;
//! that drift IS the parity defect class automation trips over (F7 in
//! `plan/agent-human-parity-audit.md`: unix emits `caret`/`anchor`/
//! `draft_length`, windows does not).
//!
//! Technique follows `src/frontend/ui_action_catalog.rs`: `include_str!`
//! both surfaces, scan string literals, compare sets against explicit
//! per-host allowlists. Each host's haystack is its snapshot-assembly
//! slice PLUS the shared `src/ui_snapshot.rs` builders both hosts call, so
//! keys emitted via the shared module never read as host-only. Source
//! scanning proves vocabulary, not call paths — a key in the haystack is
//! "this host's snapshot code can spell it", which is exactly the cheap
//! invariant worth pinning before the real extraction lands.
//!
//! When extraction candidate #1 lands, the allowlists below should shrink
//! toward empty; deleting an entry requires the key to genuinely appear on
//! both hosts (or disappear from both).

use std::collections::BTreeSet;

/// Keys today emitted only by the Windows remote client. Mostly
/// remote-protocol and native-control machinery (control bounds/visibility
/// reconciliation, render activity, parent paints), plus the sidebar clock
/// and selection-highlight publication detail.
const WINDOWS_ONLY_SNAPSHOT_KEYS: &[&str] = &[
    "capture_owned",
    "classification",
    "control_bounds_skips",
    "control_bounds_updates",
    "control_visibility_skips",
    "control_visibility_updates",
    "copyable",
    "date",
    "desired_cols",
    "desired_rows",
    "endpoint",
    "highlight",
    "instance_label",
    "open_instance",
    "parent_paints",
    "placeholder",
    "redraw_requests",
    "render_activity",
    "rendered",
    "resize_pending",
    "selected",
    "time",
];

/// Keys today emitted only by the Unix embedded frontend. `caret`,
/// `anchor`, `draft_length` are the open F7 parity gap (windows should
/// gain them, not unix lose them); the rest are embedded-window/session
/// facts the remote client has no analog for yet.
const UNIX_ONLY_SNAPSHOT_KEYS: &[&str] = &[
    "active_window_id",
    "add",
    "anchor",
    "as_window",
    "caret",
    "draft_length",
    "focused",
    "menu",
    "session",
    "tab_count",
];

/// Slice `source` from the line containing `start_marker` up to
/// `end_marker`. Panics loudly if either marker vanishes so a refactor
/// that renames the functions fails this gate visibly instead of silently
/// scanning nothing.
fn slice_between<'a>(source: &'a str, start_marker: &str, end_marker: &str) -> &'a str {
    let start = source
        .find(start_marker)
        .unwrap_or_else(|| panic!("marker `{start_marker}` not found — update snapshot_key_parity"));
    let end = source[start..]
        .find(end_marker)
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("marker `{end_marker}` not found — update snapshot_key_parity"));
    &source[start..end]
}

/// Extract every `"key":`-shaped string literal — the JSON object keys
/// spelled by `json!`/`Value` construction. Values, evidence labels, and
/// non-key literals are excluded by requiring the trailing colon.
fn json_keys(haystack: &str) -> BTreeSet<String> {
    let bytes = haystack.as_bytes();
    let mut keys = BTreeSet::new();
    let mut index = 0;
    while let Some(open) = haystack[index..].find('"').map(|o| index + o) {
        let Some(close) = haystack[open + 1..].find('"').map(|o| open + 1 + o) else {
            break;
        };
        let key = &haystack[open + 1..close];
        let mut after = close + 1;
        while after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\n' || bytes[after] == b'\r') {
            after += 1;
        }
        let is_key_shape = !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.' || c == '-');
        if is_key_shape && after < bytes.len() && bytes[after] == b':' {
            keys.insert(key.to_owned());
        }
        index = close + 1;
    }
    keys
}

fn windows_snapshot_keys() -> BTreeSet<String> {
    let remote = include_str!("../src/platform/adapters/windows/remote_frontend.rs");
    let shared = include_str!("../src/ui_snapshot.rs");
    let mut keys = json_keys(slice_between(
        remote,
        "fn ui_snapshot_json",
        "fn publish_ui_snapshot",
    ));
    keys.extend(json_keys(shared));
    keys
}

fn unix_snapshot_keys() -> BTreeSet<String> {
    let unix = include_str!("../src/platform/adapters/unix/frontend/mod.rs");
    let shared = include_str!("../src/ui_snapshot.rs");
    let mut keys = json_keys(slice_between(
        unix,
        "fn build_ui_snapshot_json",
        "\n    fn ",
    ));
    keys.extend(json_keys(shared));
    keys
}

#[test]
fn snapshot_key_vocabulary_matches_across_hosts_modulo_allowlists() {
    let windows = windows_snapshot_keys();
    let unix = unix_snapshot_keys();

    let windows_shared: BTreeSet<_> = windows
        .iter()
        .filter(|key| !WINDOWS_ONLY_SNAPSHOT_KEYS.contains(&key.as_str()))
        .cloned()
        .collect();
    let unix_shared: BTreeSet<_> = unix
        .iter()
        .filter(|key| !UNIX_ONLY_SNAPSHOT_KEYS.contains(&key.as_str()))
        .cloned()
        .collect();

    let missing_on_unix: Vec<_> = windows_shared.difference(&unix_shared).collect();
    let missing_on_windows: Vec<_> = unix_shared.difference(&windows_shared).collect();
    assert!(
        missing_on_unix.is_empty() && missing_on_windows.is_empty(),
        "ui-snapshot key vocabulary drifted between hosts.\n\
         keys only in windows (add to unix, or to WINDOWS_ONLY_SNAPSHOT_KEYS with a reason): {missing_on_unix:?}\n\
         keys only in unix (add to windows, or to UNIX_ONLY_SNAPSHOT_KEYS with a reason): {missing_on_windows:?}\n\
         see plan/design-frontend-shared-core.md §1 #1"
    );
}

#[test]
fn allowlists_are_live_and_disjoint() {
    let windows = windows_snapshot_keys();
    let unix = unix_snapshot_keys();
    for key in WINDOWS_ONLY_SNAPSHOT_KEYS {
        assert!(
            windows.contains(*key),
            "stale allowlist entry: `{key}` is no longer in the windows snapshot vocabulary — delete it"
        );
        assert!(
            !unix.contains(*key),
            "`{key}` is allowlisted windows-only but unix now emits it — parity improved, delete the entry"
        );
    }
    for key in UNIX_ONLY_SNAPSHOT_KEYS {
        assert!(
            unix.contains(*key),
            "stale allowlist entry: `{key}` is no longer in the unix snapshot vocabulary — delete it"
        );
        assert!(
            !windows.contains(*key),
            "`{key}` is allowlisted unix-only but windows now emits it — parity improved, delete the entry"
        );
    }
}
