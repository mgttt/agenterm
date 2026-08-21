//! AOT pack regression: requires `feature = "compile"`.
#![cfg(feature = "compile")]

//! Pack qualification regression for crate-local codegen fixtures.

use std::path::PathBuf;

use agenterm_rh::{CdylibExecutionMode, check, qualify_pack_dir, transpile_cdylib_with_mode};

const NATIVE_PACK_FIXTURES: [&str; 4] = [
    "rh_empty_map_fn_return.rh",
    "rh_json_map_key_chain_via_locals.rh",
    "rh_array_index_property_via_local.rh",
    "rh_null_unit_compare.rh",
];

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(name))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn assert_native_pack_fixture(name: &str) {
    let source = read_fixture(name);
    check(&source).unwrap_or_else(|error| panic!("check {name}: {error}"));
    let output = transpile_cdylib_with_mode(&source)
        .unwrap_or_else(|error| panic!("transpile {name}: {error}"));
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{name}: {}",
        output.rust
    );
    assert!(
        !output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"),
        "{name}: {}",
        output.rust
    );

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-crate-fixture-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt =
        qualify_pack_dir(&source, &dir).unwrap_or_else(|error| panic!("qualify {name}: {error}"));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 0, "{name}");
}

#[test]
fn empty_map_fn_return_pack_qualifies_native() {
    assert_native_pack_fixture(NATIVE_PACK_FIXTURES[0]);
}

#[test]
fn json_map_key_chain_via_locals_pack_qualifies_native() {
    assert_native_pack_fixture(NATIVE_PACK_FIXTURES[1]);
}

#[test]
fn array_index_property_via_local_pack_qualifies_native() {
    assert_native_pack_fixture(NATIVE_PACK_FIXTURES[2]);
}

#[test]
fn null_unit_compare_pack_qualifies_native() {
    assert_native_pack_fixture(NATIVE_PACK_FIXTURES[3]);
}
