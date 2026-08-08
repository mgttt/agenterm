//! rh language + pipeline regression (fast, no AOT compile unless noted).

use agenterm_rh::{RH_HOST_API_VERSION, check, transpile_cdylib};

#[test]
fn rh_host_api_version_is_ten() {
    assert_eq!(RH_HOST_API_VERSION, 10);
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
        (
            "json-array-walk",
            include_str!("../fixtures/rh/json-array-walk.rh"),
        ),
        (
            "json-type-string",
            include_str!("../fixtures/rh/json-type-string.rh"),
        ),
        (
            "string-validate",
            include_str!("../fixtures/rh/string-validate.rh"),
        ),
        (
            "fail-dynamic",
            include_str!("../fixtures/rh/fail-dynamic.rh"),
        ),
        (
            "map-set-membership",
            include_str!("../fixtures/rh/map-set-membership.rh"),
        ),
        (
            "path-metadata-probe",
            include_str!("../fixtures/rh/path-metadata-probe.rh"),
        ),
        (
            "import-bundle-probe",
            include_str!("../fixtures/rh/import-bundle-probe.rh"),
        ),
        (
            "string-fn-bundle",
            include_str!("../fixtures/rh/string-fn-bundle.rh"),
        ),
        (
            "fs-mutation-probe",
            include_str!("../fixtures/rh/fs-mutation-probe.rh"),
        ),
        (
            "json-array-index-assign-probe",
            include_str!("../fixtures/rh/json-array-index-assign-probe.rh"),
        ),
        (
            "json-array-index-map-return-probe",
            include_str!("../fixtures/rh/json-array-index-map-return-probe.rh"),
        ),
        (
            "json-param-index-assign-probe",
            include_str!("../fixtures/rh/json-param-index-assign-probe.rh"),
        ),
        (
            "set-map-loop-assign-probe",
            include_str!("../fixtures/rh/set-map-loop-assign-probe.rh"),
        ),
        (
            "set-map-value-assign-probe",
            include_str!("../fixtures/rh/set-map-value-assign-probe.rh"),
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
    check("import \"scripts/rh/lib/build_identity\" as build_identity; fn entry() { 1 }")
        .expect("import script");
}

#[test]
fn build_rh_transpiles_native_without_compat_delegation() {
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(repo.join("scripts/rh/build.rh")).expect("read");
    let output =
        agenterm_rh::transpile_cdylib_with_project(&repo, &source).expect("transpile");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(!output.rust.contains("compat delegating"));
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
}

#[test]
fn cdylib_transpile_emits_host_runtime_and_entry() {
    let source = include_str!("../fixtures/rh/entry.rh");
    let rust = transpile_cdylib(source).expect("transpile");
    assert!(rust.contains("rh_entry"));
    assert!(rust.contains("rh_host_api_version"));
    assert!(rust.contains("rh_register_host_v10"));
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
            .contains("rh_json_int_path(&document, &[\"schema_version\"])")
    );
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
}

#[test]
fn json_array_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/json-array-walk.rh");
    let output = agenterm_rh::transpile_cdylib_with_mode(source).expect("transpile JSON array");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output
            .rust
            .contains("rh_json_array_len(&document, &[\"executables\"])")
    );
    assert!(
        output
            .rust
            .contains("for executable in rh_json_array_items(&document, &[\"executables\"])")
    );
    assert!(
        output
            .rust
            .contains("rh_json_int_path(&executable, &[\"release_budget_bytes\"])")
    );
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let dir = std::env::temp_dir().join(format!("agenterm-rh-json-array-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify JSON array pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 11);
}

#[test]
fn json_schema_native_pack_executes_without_interpreter() {
    let source = r#"fn entry() {
        let document = rh::json::parse("{\"schema_version\":2}");
        document.schema_version
    }"#;
    let dir = std::env::temp_dir().join(format!("agenterm-rh-json-schema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify JSON native pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 2);
}

#[test]
fn json_type_string_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/json-type-string.rh");
    let output = agenterm_rh::transpile_cdylib_with_mode(source).expect("transpile type/string");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output
            .rust
            .contains("rh_json_type_name(&document, &[\"executables\"])")
    );
    assert!(
        output
            .rust
            .contains("rh_json_get_path(&entry, &[\"name\"])")
    );
    assert!(output.rust.contains("rh_json_as_str(&name)"));
    assert!(output.rust.contains("format!(\"{}{}\""));
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-json-type-string-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify type/string pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 4);
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
fn const_for_span_overflow_transpile_rejects_or_native_bounds() {
    let source = include_str!("../fixtures/rh/for-span-overflow.rh");
    match agenterm_rh::transpile_cdylib_with_mode(source) {
        Err(error) => {
            let detail = error.to_string();
            assert!(
                detail.contains("4096")
                    || detail.contains("4097")
                    || detail.contains("span")
                    || detail.contains("for"),
                "expected bounded-for transpile error, got: {detail}"
            );
        }
        Ok(output) => {
            assert_eq!(
                output.execution_mode,
                agenterm_rh::CdylibExecutionMode::Native,
                "{}",
                output.rust
            );
            assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
            assert!(!output.rust.contains("compat delegating"));
            assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
            assert!(
                !output
                    .rust
                    .lines()
                    .any(|line| line.trim_start().starts_with("for value in 0..4097")),
                "span above MAX_NATIVE_FOR_SPAN must not emit an unbounded native loop:\n{}",
                output.rust
            );
            assert!(
                output.rust.contains("4096")
                    || output.rust.contains("4097")
                    || output.rust.contains("MAX_NATIVE_FOR_SPAN")
                    || output.rust.contains("rh_fail")
                    || output.rust.contains("return Err("),
                "expected native bounded-error lowering:\n{}",
                output.rust
            );
        }
    }
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

#[test]
fn string_validate_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/string-validate.rh");
    let output =
        agenterm_rh::transpile_cdylib_with_mode(source).expect("transpile string validate");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(output.rust.contains(".starts_with("));
    assert!(output.rust.contains(".ends_with("));
    assert!(output.rust.contains(".contains("));
    assert!(output.rust.contains(".replace("));
    assert!(output.rust.contains(".trim().to_string()"));
    assert!(output.rust.contains("for character in role.chars()"));
    assert!(output.rust.contains("character.to_string()"));
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-string-validate-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt =
        agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify string validate pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 111);
}

#[test]
fn fail_dynamic_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/fail-dynamic.rh");
    let output = agenterm_rh::transpile_cdylib_with_mode(source).expect("transpile fail-dynamic");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("rh_fail(&format!(\"{}{}\""),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("String::from(\"empty:\")"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("return rh_fail") && output.rust.contains("== 0"),
        "{}",
        output.rust
    );
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let dir = std::env::temp_dir().join(format!("agenterm-rh-fail-dynamic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify fail-dynamic pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 1);
}

#[test]
fn map_set_membership_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/map-set-membership.rh");
    let output =
        agenterm_rh::transpile_cdylib_with_mode(source).expect("transpile map-set-membership");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("HashSet::<String>::new()"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains(".insert("), "{}", output.rust);
    assert!(output.rust.contains(".contains("), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-map-set-membership-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt =
        agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify map-set-membership pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 3);
}

#[test]
fn path_metadata_probe_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/path-metadata-probe.rh");
    let output =
        agenterm_rh::transpile_cdylib_with_mode(source).expect("transpile path-metadata-probe");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(output.rust.contains("rh_path_absolute("), "{}", output.rust);
    assert!(
        output.rust.contains("rh_symlink_metadata("),
        "{}",
        output.rust
    );
    assert!(output.rust.contains(".is_file"), "{}", output.rust);
    assert!(output.rust.contains(".is_symlink"), "{}", output.rust);
    assert!(output.rust.contains(".is_reparse_point"), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let previous = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(&repo).expect("chdir repo root for Cargo.toml probe");
    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-path-metadata-probe-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_current_dir(previous).expect("restore cwd");
    let receipt = receipt.expect("qualify path-metadata-probe pack");
    assert_eq!(receipt.entry_value, 111);
}

#[test]
fn import_bundle_probe_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/import-bundle-probe.rh");
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = agenterm_rh::transpile_cdylib_with_project(&repo, source)
        .expect("transpile import-bundle-probe");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("pub fn helper__add("),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("helper__add(40, 2)"),
        "{}",
        output.rust
    );
    assert!(!output.rust.contains("helper::"), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let bundled =
        agenterm_rh::bundle_project_source(&repo, source).expect("bundle import-bundle-probe");
    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-import-bundle-probe-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt =
        agenterm_rh::qualify_pack_dir(&bundled, &dir).expect("qualify import-bundle-probe pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 42);
}

#[test]
fn string_fn_bundle_fixture_executes_natively_without_interpreter() {
    let source = include_str!("../fixtures/rh/string-fn-bundle.rh");
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = agenterm_rh::transpile_cdylib_with_project(&repo, source)
        .expect("transpile string-fn-bundle");
    assert_eq!(
        output.execution_mode,
        agenterm_rh::CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("is_artifact_name(name: String)"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("rh_print(&"), "{}", output.rust);
    assert!(output.rust.contains("rh_json_as_str(&"), "{}", output.rust);
    assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);

    let bundled =
        agenterm_rh::bundle_project_source(&repo, source).expect("bundle string-fn-bundle");
    let dir = std::env::temp_dir().join(format!(
        "agenterm-rh-string-fn-bundle-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let receipt =
        agenterm_rh::qualify_pack_dir(&bundled, &dir).expect("qualify string-fn-bundle pack");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(receipt.entry_value, 1);
}
