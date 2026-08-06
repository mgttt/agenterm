//! rh language + pipeline regression (fast, no AOT compile unless noted).

use agenterm_rh::{RH_HOST_API_VERSION, check, transpile_cdylib};

#[test]
fn rh_host_api_version_is_two() {
    assert_eq!(RH_HOST_API_VERSION, 2);
}

#[test]
fn check_accepts_all_fixtures() {
    for (name, source) in [
        ("entry", include_str!("../fixtures/rh/entry.rh")),
        ("fleet", include_str!("../fixtures/rh/fleet.rh")),
        ("stdlib", include_str!("../fixtures/rh/stdlib.rh")),
        ("while", include_str!("../fixtures/rh/while.rh")),
        ("try-catch", include_str!("../fixtures/rh/try-catch.rh")),
        ("try-ok", include_str!("../fixtures/rh/try-ok.rh")),
    ] {
        check(source).unwrap_or_else(|error| panic!("check failed for {name}: {error}"));
    }
}

#[test]
fn check_rejects_eval_and_import() {
    assert!(check("eval(\"1\");").is_err());
    assert!(check("import \"x\" as y;").is_err());
}

#[test]
fn cdylib_transpile_emits_host_runtime_and_entry() {
    let source = include_str!("../fixtures/rh/entry.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("rh_entry"));
    assert!(rust.contains("rh_host_api_version"));
    assert!(rust.contains("rh_register_host_v2"));
}

#[test]
fn stdlib_fixture_transpile_uses_host_eval() {
    let source = include_str!("../fixtures/rh/stdlib.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("rh_host_eval_int"));
    assert!(rust.contains("std::fs::exists"));
}

#[test]
fn try_catch_fixture_transpile_uses_result() {
    let source = include_str!("../fixtures/rh/try-catch.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("Result<INT, INT>"));
    assert!(rust.contains("return Err("));
}

#[test]
fn while_fixture_transpile_emits_native_loop() {
    let source = include_str!("../fixtures/rh/while.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("while "));
    assert!(!rust.contains("rh_host_eval_int(\"while"));
}

#[test]
fn fleet_fixture_transpile_uses_fleet_call() {
    let source = include_str!("../fixtures/rh/fleet.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("rh_fleet_call"));
    assert!(rust.contains("protocol.info"));
}
