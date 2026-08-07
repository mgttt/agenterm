//! rh language + pipeline regression (fast, no AOT compile unless noted).

use agenterm_rh::{RH_HOST_API_VERSION, check, transpile_cdylib};

#[test]
fn rh_host_api_version_is_nine() {
    assert_eq!(RH_HOST_API_VERSION, 9);
}

#[test]
fn check_accepts_all_fixtures() {
    for (name, source) in [
        ("entry", include_str!("../fixtures/rh/entry.rh")),
        ("fleet", include_str!("../fixtures/rh/fleet.rh")),
        ("stdlib", include_str!("../fixtures/rh/stdlib.rh")),
        ("while", include_str!("../fixtures/rh/while.rh")),
        ("while-count", include_str!("../fixtures/rh/while-count.rh")),
        ("try-catch", include_str!("../fixtures/rh/try-catch.rh")),
        ("try-ok", include_str!("../fixtures/rh/try-ok.rh")),
        ("for-range", include_str!("../fixtures/rh/for-range.rh")),
        (
            "for-dyn-range",
            include_str!("../fixtures/rh/for-dyn-range.rh"),
        ),
        (
            "break-continue",
            include_str!("../fixtures/rh/break-continue.rh"),
        ),
        (
            "for-span-overflow",
            include_str!("../fixtures/rh/for-span-overflow.rh"),
        ),
        (
            "json-parse-schema",
            include_str!("../fixtures/rh/json-parse-schema.rh"),
        ),
    ] {
        check(source).unwrap_or_else(|error| panic!("check failed for {name}: {error}"));
    }
}

#[test]
fn check_rejects_eval_only() {
    assert!(check("eval(\"1\");").is_err());
}

#[test]
fn check_accepts_import_via_compat() {
    check("import \"scripts/rhai/lib/build_identity\" as build_identity; fn entry() { 1 }")
        .expect("import script");
}

#[test]
fn build_rhai_transpiles_compat_delegating() {
    let source = std::fs::read_to_string("scripts/rhai/build.rhai").expect("read");
    let rust = transpile_cdylib(&source).expect("transpile");
    assert!(rust.contains("compat delegating"));
    assert!(rust.contains("rh_host_run_script"));
}

#[test]
fn cdylib_transpile_emits_host_runtime_and_entry() {
    let source = include_str!("../fixtures/rh/entry.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("rh_entry"));
    assert!(rust.contains("rh_host_api_version"));
    assert!(rust.contains("rh_register_host_v9"));
}

#[test]
fn stdlib_fixture_transpile_uses_std_exists_fast_path() {
    let source = include_str!("../fixtures/rh/stdlib.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("rh_std_fs_exists(\"/tmp\")"));
    assert!(!rust.contains("rh_host_eval_int(\"std::fs::exists"));
}

#[test]
fn json_schema_fixture_transpiles_without_interpreter_fallback() {
    let source = include_str!("../fixtures/rh/json-parse-schema.rh");
    let output = agenterm_rh::transpile_cdylib_with_mode(source).expect("transpile JSON fixture");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(output.rust.contains("rh_json_parse("));
    assert!(
        output
            .rust
            .contains("rh_json_int_property(&document, \"schema_version\")")
    );
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn json_schema_native_pack_executes_without_interpreter() {
    let source = r#"fn entry() {
        let document = rhai::json::parse("{\"schema_version\":2}");
        document.schema_version
    }"#;
    let dir = std::env::temp_dir().join(format!("agenterm-rh-json-schema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify JSON native pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 2);
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
fn for_dyn_range_fixture_transpile_emits_native_loop() {
    let source = include_str!("../fixtures/rh/for-dyn-range.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("for value in 1..limit"));
    assert!(!rust.contains("rh_host_eval_int(\"for"));
}

#[test]
fn for_range_fixture_transpile_emits_native_loop() {
    let source = include_str!("../fixtures/rh/for-range.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("for value in 1..5"));
    assert!(!rust.contains("rh_host_eval_int(\"for"));
}

#[test]
fn const_for_span_overflow_transpile_uses_host_eval_fallback() {
    let source = include_str!("../fixtures/rh/for-span-overflow.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(
        rust.lines().any(|line| {
            line.contains("let _for = rh_host_eval_int(") && line.contains("for value in 0..4097")
        }),
        "expected localized host-eval for fallback in:\n{rust}"
    );
    assert!(
        !rust
            .lines()
            .any(|line| line.trim_start().starts_with("for value in 0..4097")),
        "span above MAX_NATIVE_FOR_SPAN must not emit a native loop:\n{rust}"
    );
    assert!(!rust.contains("compat delegating"));
}

#[test]
fn break_continue_fixture_transpile_emits_native_control_flow() {
    let source = include_str!("../fixtures/rh/break-continue.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("continue;"));
    assert!(rust.contains("break;"));
    assert!(!rust.contains("rh_host_eval_int(\"break"));
}

#[test]
fn fleet_fixture_transpile_uses_fleet_call() {
    let source = include_str!("../fixtures/rh/fleet.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("rh_fleet_call"));
    assert!(rust.contains("protocol.info"));
}
