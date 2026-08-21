//! Parity checks for agenterm's script-engine "L2 facade" — the fleet.*
//! scripting API surface that every engine exposes over
//! `__host.fleet_call(operation_id, params_json)` (lua/qjs) or the
//! equivalent compiled-in path (rh).
//!
//! Nothing else in the tree asserts these surfaces stay aligned, so drift
//! between engines would otherwise be silent. This file parses the actual
//! source artifacts (plain text/regex, no engine execution) and compares
//! the extracted (function/path -> operation_id) catalogs:
//!
//! 1. lua vs qjs: `scripts/lua/lib/fleet.lua` vs `scripts/qjs/lib/fleet.js`
//! 2. rh vs lua/qjs: `crates/agenterm-rh/src/shipped_surfaces.rs` vs the above
//! 3. every facade vs host: `src/operations.rs` `OPERATION_CATALOG` (the
//!    authoritative, dispatchable operation-id list)
//!
//! Investigation findings baked into this file:
//!
//! - lua and qjs are IN SYNC today: identical 29-entry (function_name ->
//!   operation_id) maps. Test 1 locks full equality.
//! - rh's `SHIPPED_SURFACE_PATHS` declares 76 `fleet.*` surface paths — a
//!   strict superset of lua/qjs's 29. rh's surface legitimately being wider
//!   (compiled-in, not hand-ported) is expected; this is documented and
//!   pinned rather than silently ignored. See `rh_only_operation_ids()` below.
//! - `src/operations.rs::OPERATION_CATALOG` is the authoritative,
//!   *dispatchable* operation-id list (`operation_by_id` looks entries up
//!   there; see `crates/agenterm-rh/src/fleet.rs`/`src/script_fleet.rs`
//!   callers). It has 44 entries. Every lua/qjs operation_id is present in
//!   it (clean subset). BUT 32 of rh's 76 declared `fleet.*` surfaces are
//!   **not** in `OPERATION_CATALOG` at all (e.g. `ui.settings.*`,
//!   `ui.modal.*`, `ui.font.*`, `ui.window.maximize/minimize/restore/close`,
//!   `ui.instance-picker.{cancel,confirm,next,prev}`, `ui.tab.new`,
//!   `ui.tab.editor.{save,cancel}`, `terminal.copy-selection`,
//!   `ui.locale.toggle`, `ui.new-terminal.open`,
//!   `ui.window-close.keep-server-running`). rh's shipped-surface catalog
//!   claims support for operations the host's authoritative catalog does
//!   not (yet) implement/dispatch. That asymmetry is real and is pinned by
//!   `rh_surfaces_missing_from_host_catalog()` below rather than swept under
//!   an intersection.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path: PathBuf = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// Extracts `(function_name, operation_id)` pairs from a lua/qjs fleet
/// wrapper file. Both files share the same shape: each exported leaf
/// function's body contains exactly one `call("<operation_id>"` (or
/// `call("<operation_id>", ...`) invocation, and is declared as either
/// `function fleet.a.b.c(...)` (lua) or `fleet.a.b.c = function (...)`
/// (qjs). We key on the *fully-qualified* dotted path (`a.b.c`) rather than
/// just the leaf name so that e.g. `tabs.list` and (hypothetically)
/// `events.list` can't collide.
fn extract_script_facade(source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut pending_path: Option<String> = None;
    for raw_line in source.lines() {
        let line = raw_line.trim();

        if let Some(rest) = line.strip_prefix("function fleet.") {
            // lua: `function fleet.tabs.set_note(tab_id, note)`
            if let Some(paren) = rest.find('(') {
                pending_path = Some(rest[..paren].to_owned());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("fleet.") {
            // qjs: `fleet.tabs.set_note = function (tabId, note) {`
            if let Some(eq) = rest.find(" = function") {
                pending_path = Some(rest[..eq].to_owned());
                continue;
            }
        }

        if let Some(call_start) = line.find("call(\"") {
            let after = &line[call_start + "call(\"".len()..];
            if let Some(end) = after.find('"') {
                let operation_id = after[..end].to_owned();
                if let Some(path) = pending_path.take() {
                    out.push((path, operation_id));
                }
            }
        }
    }
    out
}

fn lua_facade() -> Vec<(String, String)> {
    extract_script_facade(&read("scripts/lua/lib/fleet.lua"))
}

fn qjs_facade() -> Vec<(String, String)> {
    extract_script_facade(&read("scripts/qjs/lib/fleet.js"))
}

/// Parses the `pub const NAME: &str = "value";` declarations near the top
/// of `src/operations.rs` into a name -> literal-value lookup, so catalog
/// entries written as `id: SOME_CONST` can be resolved to their string.
fn extract_str_consts(source: &str) -> std::collections::BTreeMap<String, String> {
    let mut consts = std::collections::BTreeMap::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let name = rest[..colon].trim();
        if !rest[colon..].contains("&str") {
            continue;
        }
        let Some(eq) = rest.find('=') else { continue };
        let value_part = rest[eq + 1..].trim();
        let Some(open) = value_part.find('"') else {
            continue;
        };
        let after_open = &value_part[open + 1..];
        let Some(close) = after_open.find('"') else {
            continue;
        };
        consts.insert(name.to_owned(), after_open[..close].to_owned());
    }
    consts
}

/// Extracts the authoritative, host-dispatchable operation-id set from
/// `src/operations.rs`'s `pub const OPERATION_CATALOG: &[OperationSpec] = &[ ... ];`.
/// Each entry's `id:` field is either a string literal or a reference to one
/// of the `pub const X: &str = "...";` declarations earlier in the file.
fn host_operation_catalog_ids() -> BTreeSet<String> {
    let source = read("src/operations.rs");
    let consts = extract_str_consts(&source);

    // NB: must include the trailing `:` — `pub const OPERATION_CATALOG` alone
    // also matches `pub const OPERATION_CATALOG_SCHEMA_VERSION`, which sits
    // earlier in the file and would make `find` latch onto the wrong array.
    let start_marker = "pub const OPERATION_CATALOG:";
    let start = source
        .find(start_marker)
        .expect("OPERATION_CATALOG declaration not found in src/operations.rs");
    let body = &source[start..];
    let array_open = body
        .find("= &[")
        .expect("OPERATION_CATALOG array opener `= &[` not found");
    let after_open = &body[array_open + "= &[".len()..];
    let array_close = after_open
        .find("\n];")
        .expect("OPERATION_CATALOG array closer `\\n];` not found");
    let catalog_block = &after_open[..array_close];

    let mut ids = BTreeSet::new();
    for line in catalog_block.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("id:") else {
            continue;
        };
        let rest = rest.trim().trim_end_matches(',');
        if let Some(stripped) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            ids.insert(stripped.to_owned());
        } else if let Some(value) = consts.get(rest) {
            ids.insert(value.clone());
        } else {
            panic!("OPERATION_CATALOG id `{rest}` did not resolve to a literal or known const");
        }
    }
    assert!(
        ids.len() >= 40,
        "sanity check: expected >=40 entries in OPERATION_CATALOG, found {} \
         (extraction likely broke — did operations.rs's array formatting change?)",
        ids.len()
    );
    ids
}

/// Extracts the `fleet.*` entries of rh's
/// `crates/agenterm-rh/src/shipped_surfaces.rs::SHIPPED_SURFACE_PATHS` and
/// converts each surface path to the operation_id it names. The catalog
/// uses `snake_case` path segments (matching Rust identifier conventions,
/// e.g. `fleet.ui.tabs.set_width`, `fleet.ui.cwd_editor.open`) while
/// operation ids use `kebab-case` segments (e.g. `ui.tabs.set-width`,
/// `ui.cwd-editor.open`) — confirmed against `src/operations.rs`'s
/// `script_surface` <-> `id` pairs (e.g. `TABS_SET_NOTE` has
/// `script_surface: "fleet.tabs.set_note"` and `id: "tabs.set-note"`).
/// So: strip the `fleet.` prefix, then replace `_` with `-` within each
/// dot-separated segment.
fn rh_shipped_fleet_operation_ids() -> BTreeSet<String> {
    let source = read("crates/agenterm-rh/src/shipped_surfaces.rs");
    let start_marker = "SHIPPED_SURFACE_PATHS";
    let start = source
        .find(start_marker)
        .expect("SHIPPED_SURFACE_PATHS declaration not found");
    let body = &source[start..];
    let array_open = body
        .find("= &[")
        .expect("SHIPPED_SURFACE_PATHS array opener `= &[` not found");
    let after_open = &body[array_open + "= &[".len()..];
    let array_close = after_open
        .find("\n];")
        .expect("SHIPPED_SURFACE_PATHS array closer `\\n];` not found");
    let block = &after_open[..array_close];

    let mut ids = BTreeSet::new();
    for line in block.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        let path = &rest[..end];
        let Some(suffix) = path.strip_prefix("fleet.") else {
            continue;
        };
        let operation_id = suffix
            .split('.')
            .map(|segment| segment.replace('_', "-"))
            .collect::<Vec<_>>()
            .join(".");
        ids.insert(operation_id);
    }
    assert!(
        ids.len() >= 60,
        "sanity check: expected >=60 `fleet.*` entries in SHIPPED_SURFACE_PATHS, found {} \
         (extraction likely broke — did shipped_surfaces.rs's array formatting change?)",
        ids.len()
    );
    ids
}

// ── Test 1: lua vs qjs — the strong parity check ───────────────────────
//
// Investigation result: the two facades are IN SYNC today (29/29 identical
// function-path -> operation_id pairs). This locks full equality; any
// future edit to either file that adds, removes, or renames an operation
// without mirroring it in the other engine will fail this test.

#[test]
fn lua_and_qjs_fleet_facades_expose_identical_operation_catalogs() {
    let lua = lua_facade();
    let qjs = qjs_facade();

    assert!(
        !lua.is_empty(),
        "lua facade extraction found zero entries — parser likely broken"
    );
    assert!(
        !qjs.is_empty(),
        "qjs facade extraction found zero entries — parser likely broken"
    );

    let lua_map: std::collections::BTreeMap<_, _> = lua.into_iter().collect();
    let qjs_map: std::collections::BTreeMap<_, _> = qjs.into_iter().collect();

    assert_eq!(
        lua_map, qjs_map,
        "lua and qjs fleet facades have drifted: expected identical \
         (function_path -> operation_id) maps. Diff the two sides above."
    );

    // Sanity-anchor the count so a silent parser regression that still
    // happens to produce equal (but empty, or partial) maps is caught too.
    assert_eq!(
        lua_map.len(),
        29,
        "expected the known-good count of 29 fleet operations in lua/qjs; \
         if this changed intentionally, update this anchor alongside the \
         module doc comment counts"
    );
}

// ── Test 2: rh vs lua/qjs ───────────────────────────────────────────────
//
// Investigation result: rh's declared surface (`SHIPPED_SURFACE_PATHS`,
// 76 `fleet.*` entries) is a STRICT SUPERSET of lua/qjs's 29-entry set —
// every lua/qjs operation_id is present in rh's declared surface, plus 47
// rh-only entries. rh being compiled-in and wider than the hand-ported
// script facades is plausible/legitimate (it may expose operations that
// haven't been ported to lua/qjs yet), so this test locks the *intersection*
// (must equal all of lua/qjs — zero missing) and pins the extra/known-divergent
// entries explicitly rather than silently ignoring them.

#[test]
fn rh_shipped_surface_is_a_superset_of_the_lua_qjs_fleet_facade() {
    let lua_ids: BTreeSet<String> = lua_facade().into_iter().map(|(_, id)| id).collect();
    let rh_ids = rh_shipped_fleet_operation_ids();

    let missing_from_rh: Vec<&String> = lua_ids.difference(&rh_ids).collect();
    assert!(
        missing_from_rh.is_empty(),
        "lua/qjs reference operation_ids that rh's shipped_surfaces.rs does \
         not declare: {missing_from_rh:?}"
    );

    // Pin today's known rh-only surplus so an *unexpected* future shrink or
    // grow of the gap is caught. Growth here is expected as rh ships more
    // fleet.* coverage than the script facades; shrinkage would mean rh
    // removed a declared surface, which is worth a human look either way.
    let rh_only: BTreeSet<String> = rh_ids.difference(&lua_ids).cloned().collect();
    assert_eq!(
        rh_only,
        rh_only_operation_ids(),
        "rh's fleet.* surplus over lua/qjs changed. If this is an \
         intentional addition/removal to rh's shipped_surfaces.rs, update \
         `rh_only_operation_ids()` in this test to match (and consider \
         whether lua/qjs should gain the same operation)."
    );
}

/// Documented, explicit allowlist of operation ids that rh's compiled-in
/// `shipped_surfaces.rs` declares but that lua/qjs's fleet.* wrappers do not
/// (yet) expose. Extracted from the investigation run on 2026-08-09;
/// 47 entries.
fn rh_only_operation_ids() -> BTreeSet<String> {
    [
        "terminal.copy-selection",
        "terminal.mouse",
        "ui.cwd-editor.open",
        "ui.cwd-editor.prepare",
        "ui.cwd-editor.prepare-append",
        "ui.cwd-editor.prepare-replace",
        "ui.cwd-editor.send-now",
        "ui.font.decrease",
        "ui.font.increase",
        "ui.input.key",
        "ui.instance-picker.cancel",
        "ui.instance-picker.confirm",
        "ui.instance-picker.next",
        "ui.instance-picker.open",
        "ui.instance-picker.prev",
        "ui.instance-picker.select",
        "ui.locale.toggle",
        "ui.modal.cancel",
        "ui.modal.confirm",
        "ui.new-terminal.open",
        "ui.server-strip.select",
        "ui.settings.apply",
        "ui.settings.inherit.font",
        "ui.settings.inherit.size",
        "ui.settings.inherit.theme",
        "ui.settings.open",
        "ui.settings.preset.classic-day",
        "ui.settings.preset.classic-night",
        "ui.settings.preset.fancy-day",
        "ui.settings.preset.fancy-night",
        "ui.settings.reset-overrides",
        "ui.settings.scope.current",
        "ui.settings.scope.defaults",
        "ui.settings.theme.dark",
        "ui.settings.theme.light",
        "ui.tab.close",
        "ui.tab.edit",
        "ui.tab.editor.cancel",
        "ui.tab.editor.save",
        "ui.tab.new",
        "ui.window-close.keep-server-running",
        "ui.window-close.stop-server-and-exit",
        "ui.window.close",
        "ui.window.maximize",
        "ui.window.minimize",
        "ui.window.resize",
        "ui.window.restore",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

// ── Test 3: every facade's operation_ids vs the host-authoritative list ──
//
// `src/operations.rs`'s `OPERATION_CATALOG` is the authoritative,
// *dispatchable* operation-id list: `operation_by_id()` looks entries up
// there, and both `src/script_fleet.rs` (rhai-engine binding) and
// `crates/agenterm-rh/src/fleet.rs` (rh's AST-level transpiler, indirectly
// via the runtime host bridge) reject any operation_id that isn't in it.
//
// Investigation result:
//  - lua/qjs (29 operation_ids): a CLEAN SUBSET of the 44-entry
//    OPERATION_CATALOG. Nothing referenced by the script facades is
//    missing from the host's authoritative list. This is asserted as a
//    strict subset check (any new lua/qjs operation_id that isn't wired
//    into OPERATION_CATALOG will fail loudly).
//  - rh's declared `SHIPPED_SURFACE_PATHS` (76 fleet.* entries) is NOT a
//    subset: 32 of its declared operation ids have no entry in
//    OPERATION_CATALOG at all (e.g. `ui.settings.*`, `ui.modal.*`,
//    `ui.font.*`, `ui.window.{maximize,minimize,restore,close}`,
//    `ui.instance-picker.{cancel,confirm,next,prev}`, `ui.tab.new`,
//    `ui.tab.editor.{save,cancel}`, `terminal.copy-selection`,
//    `ui.locale.toggle`, `ui.new-terminal.open`,
//    `ui.window-close.keep-server-running`). rh's shipped-surface catalog
//    claims support for operations the host does not (yet) implement /
//    make dispatchable. This is real drift between "what rh's docs say it
//    ships" and "what the host can actually execute" — pinned explicitly
//    below rather than silently intersected away.
//
// (Aside, not asserted here: `OPERATION_CATALOG` also has `pane.capture`,
// which has no `fleet.*`-prefixed shipped_surfaces entry because it's
// listed there as `"FleetTerminal.capture"` instead — a naming
// convention difference, not a missing surface.)

#[test]
fn lua_qjs_operation_ids_are_a_subset_of_the_host_operation_catalog() {
    let catalog = host_operation_catalog_ids();
    let lua_ids: BTreeSet<String> = lua_facade().into_iter().map(|(_, id)| id).collect();

    let missing: Vec<&String> = lua_ids.difference(&catalog).collect();
    assert!(
        missing.is_empty(),
        "lua/qjs fleet facade references operation_ids the host's \
         OPERATION_CATALOG (src/operations.rs) does not implement: {missing:?}"
    );
}

#[test]
fn rh_shipped_surface_operation_ids_not_in_host_catalog_match_documented_gap() {
    let catalog = host_operation_catalog_ids();
    let rh_ids = rh_shipped_fleet_operation_ids();

    let undispatchable: BTreeSet<String> = rh_ids.difference(&catalog).cloned().collect();

    assert_eq!(
        undispatchable,
        rh_surfaces_missing_from_host_catalog(),
        "the set of rh-declared fleet.* operation_ids that are absent from \
         src/operations.rs's OPERATION_CATALOG changed. If OPERATION_CATALOG \
         gained one of these (the host now implements it), remove it from \
         `rh_surfaces_missing_from_host_catalog()`. If rh's \
         shipped_surfaces.rs gained a new undispatchable entry, that's new \
         drift worth a second look before updating the allowlist."
    );
}

/// Documented, explicit allowlist of operation ids that rh's
/// `shipped_surfaces.rs` declares as supported but that
/// `src/operations.rs::OPERATION_CATALOG` does not implement/dispatch.
/// Extracted from the investigation run on 2026-08-09; 33 entries.
/// (The comment said 32 while the array held 33, which is how that figure
/// got copied into the PRD.)
fn rh_surfaces_missing_from_host_catalog() -> BTreeSet<String> {
    [
        "terminal.copy-selection",
        "ui.font.decrease",
        "ui.font.increase",
        "ui.instance-picker.cancel",
        "ui.instance-picker.confirm",
        "ui.instance-picker.next",
        "ui.instance-picker.prev",
        "ui.locale.toggle",
        "ui.modal.cancel",
        "ui.modal.confirm",
        "ui.new-terminal.open",
        "ui.settings.apply",
        "ui.settings.inherit.font",
        "ui.settings.inherit.size",
        "ui.settings.inherit.theme",
        "ui.settings.open",
        "ui.settings.preset.classic-day",
        "ui.settings.preset.classic-night",
        "ui.settings.preset.fancy-day",
        "ui.settings.preset.fancy-night",
        "ui.settings.reset-overrides",
        "ui.settings.scope.current",
        "ui.settings.scope.defaults",
        "ui.settings.theme.dark",
        "ui.settings.theme.light",
        "ui.tab.editor.cancel",
        "ui.tab.editor.save",
        "ui.tab.new",
        "ui.window-close.keep-server-running",
        "ui.window.close",
        "ui.window.maximize",
        "ui.window.minimize",
        "ui.window.restore",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}
