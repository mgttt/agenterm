use std::path::PathBuf;

use agenterm_tinyvm::{WasmError, WasmFeatureUsage, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn fixture(name: &str) -> WasmFeatureUsage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let bytes = wat::parse_file(&path)
        .unwrap_or_else(|error| panic!("compile feature fixture {}: {error}", path.display()));
    must_ok(WasmModule::from_bytes(&bytes), "load feature fixture").feature_usage()
}

#[test]
fn minimal_scalar_module_reports_no_post_mvp_features() {
    let bytes = wat::parse_str("(module (func (export \"run\") (result i32) i32.const 42))")
        .expect("compile minimal module");
    let usage = must_ok(WasmModule::from_bytes(&bytes), "load minimal module").feature_usage();
    assert!(usage == WasmFeatureUsage::default());
}

#[test]
fn independent_standard_fixtures_cover_every_reported_feature_family() {
    let bulk = fixture("bulk-memory-v1.wat");
    assert!(bulk.bulk_memory);

    let scalar = fixture("scalar-proposals-v1.wat");
    assert!(scalar.sign_extension);
    assert!(scalar.nontrapping_float_to_int);

    assert!(fixture("multi-value-v1.wat").multi_value);
    assert!(fixture("funcref-v1.wat").reference_types);
    assert!(fixture("multi-table-v1.wat").multiple_tables);
    assert!(fixture("multi-memory-v1.wat").multiple_memories);
    assert!(fixture("extended-const-v1.wat").extended_const);
    assert!(fixture("tail-call-v1.wat").tail_call);
    #[cfg(feature = "simd")]
    assert!(fixture("simd-audio-mix-v1.wat").simd);
}

#[cfg(not(feature = "simd"))]
#[test]
fn simd_module_fails_explicitly_when_optional_profile_is_disabled() {
    let bytes = wat::parse_file(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simd-audio-mix-v1.wat"),
    )
    .expect("compile SIMD feature fixture");
    let error = match WasmModule::from_bytes(&bytes) {
        Err(error) => error,
        Ok(_) => panic!("default profile must not silently accept SIMD"),
    };
    assert_eq!(error.message(), "SIMD feature is disabled");
}
