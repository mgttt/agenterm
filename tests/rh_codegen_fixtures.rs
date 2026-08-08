//! Native emit locks for Phase B codegen fixtures (rev 74 map/index/field wins).

use agenterm_rh::{CdylibExecutionMode, RH_CODEGEN_REVISION, transpile_cdylib_with_mode};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("fixtures/rh/{name}"))
        .unwrap_or_else(|error| panic!("read fixtures/rh/{name}: {error}"))
}

fn assert_native_fixture(name: &str, needles: &[&str], anti_needles: &[&str]) {
    let source = fixture(name);
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
        1,
        "{name}: {}",
        output.rust
    );
}

#[test]
fn codegen_revision_is_seventy_six() {
    assert_eq!(RH_CODEGEN_REVISION, 76);
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
    );
}

#[test]
fn json_param_index_assign_fixture_emits_native_map_key_writes() {
    assert_native_fixture(
        "json-param-index-assign-probe.rh",
        &["rh_json_set_path_key(&mut states, &[], "],
        &[],
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
    );
}

#[test]
fn json_array_index_map_return_fixture_emits_index_not_key_path() {
    assert_native_fixture(
        "json-array-index-map-return-probe.rh",
        &["rh_json_array_get(&matches, "],
        &["rh_json_get_path_key(&matches"],
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
