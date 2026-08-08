//! Native / host-eval locks for crate-local Phase B codegen fixtures.

use std::path::PathBuf;

use agenterm_rh::{CdylibExecutionMode, RH_CODEGEN_REVISION, check, transpile_cdylib_with_mode};

const FIXTURES: [(&str, &[&str], &[&str]); 9] = [
    (
        "rh_empty_map_fn_return.rh",
        &["rh_json_set_path_key(&mut env"],
        &["HashSet::<String>::new()"],
    ),
    (
        "rh_json_map_key_chain_via_locals.rh",
        &["rh_json_set_path_key(&mut m, &[], "],
        &[],
    ),
    (
        "rh_array_index_property_via_local.rh",
        &[
            "rh_json_array_get(&ordered, ",
            "rh_json_int_path(&item, &[\"id\"])",
        ],
        &["rh_json_get_path_key(&ordered"],
    ),
    (
        "rh_null_unit_compare.rh",
        &[".is_null()"],
        &["rh_host_run_script(RH_SCRIPT_SOURCE)"],
    ),
    (
        "rh_snapshot_tabs_index_return.rh",
        &["rh_json_get_path_index(&snapshot, &[\"tabs\"], index)"],
        &["rh_json_string_path_index(&snapshot, &[\"tabs\"]"],
    ),
    (
        "rh_working_context_dot_chain_via_locals.rh",
        &[
            "rh_json_get_path(&tab, &[\"working_context\"])",
            "rh_json_get_path(&wc, &[\"proxy\"])",
            "rh_json_string_path(&proxy, &[\"source\"])",
        ],
        &[],
    ),
    (
        "rh_tab_active_map_key_vs_dot.rh",
        &[
            "rh_json_get_path(&tab, &[\"active\"])",
            "rh_json_get_path_key(&tab, &[], &String::from(\"active\"))",
        ],
        &[],
    ),
    (
        "rh_snapshot_tabs_map_key_active.rh",
        &[
            "rh_json_array_get(&tabs, ",
            "rh_json_get_path_key(&tab, &[], &String::from(\"active\"))",
        ],
        &["rh_json_string_path_index(&snapshot, &[\"tabs\"]"],
    ),
    (
        "rh_gui_window_control_visible_click.rh",
        &[
            "rh_child_window_control(&mut gui, 2104)",
            "rh_window_control_visible(&mut tabs_button)",
            "rh_window_control_click(&mut tabs_button)",
            "rh_child_window_message(&mut gui, ",
            "rh_child_window_pointer(&mut gui, ",
            "rh_child_window_rect(&mut gui, true)",
        ],
        &[
            "rh_host_eval_int(\"tabs_button.visible",
            "rh_host_eval_int(\"settings_button.click",
        ],
    ),
];

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(name))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn assert_native_or_host_eval(name: &str, needles: &[&str], anti_needles: &[&str]) {
    let source = read_fixture(name);
    check(&source).unwrap_or_else(|error| panic!("check {name}: {error}"));
    let output = transpile_cdylib_with_mode(&source)
        .unwrap_or_else(|error| panic!("transpile {name}: {error}"));
    assert!(
        matches!(
            output.execution_mode,
            CdylibExecutionMode::Native | CdylibExecutionMode::HostEval
        ),
        "{name}: expected native or host-eval, got {:?}\n{}",
        output.execution_mode,
        output.rust
    );
    assert!(
        output.execution_mode != CdylibExecutionMode::CompatDelegating,
        "{name} must not compat-delegate: {}",
        output.rust
    );
    for needle in needles {
        assert!(
            output.rust.contains(needle),
            "{name} missing {needle:?}: {}",
            output.rust
        );
    }
    for needle in anti_needles {
        assert!(
            !output.rust.contains(needle),
            "{name} must not contain {needle:?}: {}",
            output.rust
        );
    }
    assert!(
        !output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"),
        "{name}: {}",
        output.rust
    );
}

#[test]
fn codegen_revision_is_seventy_eight() {
    assert_eq!(RH_CODEGEN_REVISION, 78);
}

#[test]
fn crate_fixtures_compile() {
    for (name, _, _) in FIXTURES {
        let source = read_fixture(name);
        check(&source).unwrap_or_else(|error| panic!("check {name}: {error}"));
    }
}

#[test]
fn empty_map_fn_return_emits_native_json_not_set() {
    assert_native_or_host_eval(
        FIXTURES[0].0,
        FIXTURES[0].1,
        FIXTURES[0].2,
    );
}

#[test]
fn json_map_key_chain_via_locals_emits_native_path_key_writes() {
    assert_native_or_host_eval(
        FIXTURES[1].0,
        FIXTURES[1].1,
        FIXTURES[1].2,
    );
}

#[test]
fn array_index_property_via_local_emits_index_not_key_path() {
    assert_native_or_host_eval(
        FIXTURES[2].0,
        FIXTURES[2].1,
        FIXTURES[2].2,
    );
}

#[test]
fn null_unit_compare_emits_native_json_null_check() {
    assert_native_or_host_eval(
        FIXTURES[3].0,
        FIXTURES[3].1,
        FIXTURES[3].2,
    );
}

#[test]
fn snapshot_tabs_index_return_emits_path_index_not_string_index() {
    assert_native_or_host_eval(FIXTURES[4].0, FIXTURES[4].1, FIXTURES[4].2);
}

#[test]
fn working_context_dot_chain_via_locals_stays_native_without_extra_host_eval() {
    assert_native_or_host_eval(FIXTURES[5].0, FIXTURES[5].1, FIXTURES[5].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[5].0))
        .expect("transpile working-context dot chain");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}

#[test]
fn tab_active_map_key_vs_dot_emits_native_key_and_field_reads() {
    assert_native_or_host_eval(FIXTURES[6].0, FIXTURES[6].1, FIXTURES[6].2);
}

#[test]
fn snapshot_tabs_map_key_active_emits_array_get_and_path_key_not_string_index() {
    assert_native_or_host_eval(FIXTURES[7].0, FIXTURES[7].1, FIXTURES[7].2);
}

#[test]
fn gui_window_control_visible_click_emits_native_gui_host_calls() {
    assert_native_or_host_eval(FIXTURES[8].0, FIXTURES[8].1, FIXTURES[8].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[8].0))
        .expect("transpile gui window-control fixture");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}
