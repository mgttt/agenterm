//! Native emit locks for Phase B codegen fixtures (rev 74 map/index/field wins).

use agenterm_rh::{CdylibExecutionMode, RH_CODEGEN_REVISION, transpile_cdylib_with_mode};

const CRATE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/agenterm-rh/tests/fixtures/"
);

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("fixtures/rh/{name}"))
        .unwrap_or_else(|error| panic!("read fixtures/rh/{name}: {error}"))
}

fn crate_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{CRATE_FIXTURE}{name}"))
        .unwrap_or_else(|error| panic!("read crate fixture {name}: {error}"))
}

/// `expected_host_evals` locks how many expressions still escape to the
/// host-eval fallback. Rev 84 emits `.len` comparisons natively
/// (`rh_json_array_len`), so most probes are now fully native (0); a probe
/// that still carries exactly one deliberate escape locks 1.
fn assert_native_fixture(
    name: &str,
    needles: &[&str],
    anti_needles: &[&str],
    expected_host_evals: usize,
) {
    let source = if name.starts_with("rh_") {
        crate_fixture(name)
    } else {
        fixture(name)
    };
    agenterm_rh::check(&source).unwrap_or_else(|error| panic!("check {name}: {error}"));
    let output = transpile_cdylib_with_mode(&source)
        .unwrap_or_else(|error| panic!("transpile {name}: {error}"));
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{name}: {}",
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
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        expected_host_evals,
        "{name}: {}",
        output.rust
    );
}

#[test]
fn codegen_revision_is_eighty_nine() {
    assert_eq!(RH_CODEGEN_REVISION, 100);
}

#[test]
fn set_map_value_assign_fixture_emits_native_map_key_writes() {
    assert_native_fixture(
        "set-map-value-assign-probe.rh",
        &[
            "rh_json_set_path_key(&mut identities",
            "rh_json_set_path_key(&mut unique",
            "rh_json_set_path_key(&mut owned_ids",
        ],
        &["HashSet::<String>::new()"],
        0,
    );
}

#[test]
fn json_param_index_assign_fixture_emits_native_map_key_writes() {
    assert_native_fixture(
        "json-param-index-assign-probe.rh",
        &["rh_json_set_path_key(&mut states, &[], "],
        &[],
        0,
    );

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-json-param-index-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(&fixture("json-param-index-assign-probe.rh"), &dir)
        .expect("qualify json-param-index-assign-probe");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 0);
}

#[test]
fn json_array_index_assign_fixture_emits_native_index_writes() {
    assert_native_fixture(
        "json-array-index-assign-probe.rh",
        &["rh_json_set_path_index(&mut safe, &[], "],
        &["rh_json_set_path_key(&mut safe"],
        0,
    );
}

#[test]
fn json_array_index_map_return_fixture_emits_index_not_key_path() {
    assert_native_fixture(
        "json-array-index-map-return-probe.rh",
        &["rh_json_array_get(&matches, "],
        &["rh_json_get_path_key(&matches"],
        0,
    );

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-json-array-index-map-return-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt =
        agenterm_rh::qualify_pack_dir(&fixture("json-array-index-map-return-probe.rh"), &dir)
            .expect("qualify json-array-index-map-return-probe");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 0);
}

#[test]
fn set_map_loop_assign_fixture_emits_native_map_key_reads_in_loop() {
    assert_native_fixture(
        "set-map-loop-assign-probe.rh",
        &["names.insert(rh_json_as_str(&name))"],
        &[],
        0,
    );

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-set-map-loop-assign-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(&fixture("set-map-loop-assign-probe.rh"), &dir)
        .expect("qualify set-map-loop-assign-probe");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 0);
}

#[test]
fn rhai_array_index_property_misparse_emits_parenthesized_index_access() {
    let source = r#"fn entry() {
    let ordered = [#{ id: 1 }, #{ id: 3 }];
    let order_index = 0;
    let process = #{ id: 2 };
    if process.id < ordered[order_index].id {
        return 1;
    }
    0
}"#;
    let output = transpile_cdylib_with_mode(source).expect("transpile ordered[index].field");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("rh_json_get_path_index(&ordered"),
        "{}",
        output.rust
    );
    assert!(
        !output.rust.contains("rh_json_get_path_key(&ordered"),
        "{}",
        output.rust
    );
}

#[test]
fn snapshot_tabs_index_return_fixture_emits_path_index_not_string_index() {
    let source = crate_fixture("rh_snapshot_tabs_index_return.rh");
    agenterm_rh::check(&source).expect("check rh_snapshot_tabs_index_return");
    let output =
        transpile_cdylib_with_mode(&source).expect("transpile rh_snapshot_tabs_index_return");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output
            .rust
            .contains("rh_json_get_path_index(&snapshot, &[\"tabs\"], index)"),
        "{}",
        output.rust
    );
    assert!(
        !output
            .rust
            .contains("rh_json_string_path_index(&snapshot, &[\"tabs\"]"),
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        0,
        "{}",
        output.rust
    );

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-snapshot-tabs-index-return-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(&source, &dir)
        .expect("qualify rh_snapshot_tabs_index_return");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 0);
}

#[test]
fn working_context_dot_chain_fixture_stays_native_without_extra_host_eval() {
    assert_native_fixture(
        "rh_working_context_dot_chain_via_locals.rh",
        &[
            "rh_json_get_path(&tab, &[\"working_context\"])",
            "rh_json_get_path(&wc, &[\"proxy\"])",
            "rh_json_string_path(&proxy, &[\"source\"])",
        ],
        &[],
        0,
    );
}

#[test]
fn tab_active_map_key_vs_dot_fixture_emits_native_key_and_field_reads() {
    assert_native_fixture(
        "rh_tab_active_map_key_vs_dot.rh",
        &[
            "rh_json_get_path(&tab, &[\"active\"])",
            "rh_json_get_path_key(&tab, &[], &String::from(\"active\"))",
        ],
        &[],
        0,
    );
}

#[test]
fn clipboard_get_set_text_fixture_emits_native_host_json_calls() {
    assert_native_fixture(
        "rh_clipboard_get_set_text.rh",
        &[
            "rh_clipboard_get_text()",
            "rh_clipboard_set_text(&String::from(\"native-clipboard-probe\"))",
        ],
        &["rh_host_eval_int(\"rhai::clipboard::"],
        0,
    );
}
