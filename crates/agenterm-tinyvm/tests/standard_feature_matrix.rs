#![cfg(feature = "simd")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use agenterm_tinyvm::{WasmError, WasmFeatureUsage, WasmModule};

const EXPECTED_FEATURES: [&str; 10] = [
    "bulk-memory",
    "extended-const",
    "multi-value",
    "multiple-memories",
    "multiple-tables",
    "nontrapping-float-to-int",
    "reference-types",
    "sign-extension",
    "simd",
    "tail-call",
];

struct Row<'a> {
    feature: &'a str,
    fixture: &'a str,
    gate: &'a str,
    oracle: &'a str,
    size_profile: &'a str,
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rows(text: &str) -> Vec<Row<'_>> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            assert_eq!(
                columns.len(),
                5,
                "matrix row must have five columns: {line}"
            );
            Row {
                feature: columns[0],
                fixture: columns[1],
                gate: columns[2],
                oracle: columns[3],
                size_profile: columns[4],
            }
        })
        .collect()
}

fn feature_is_used(usage: WasmFeatureUsage, feature: &str) -> bool {
    match feature {
        "bulk-memory" => usage.bulk_memory,
        "sign-extension" => usage.sign_extension,
        "nontrapping-float-to-int" => usage.nontrapping_float_to_int,
        "multi-value" => usage.multi_value,
        "reference-types" => usage.reference_types,
        "multiple-tables" => usage.multiple_tables,
        "multiple-memories" => usage.multiple_memories,
        "extended-const" => usage.extended_const,
        "tail-call" => usage.tail_call,
        "simd" => usage.simd,
        other => panic!("unknown feature row {other}"),
    }
}

fn must<T>(result: Result<T, WasmError>, context: &str) -> T {
    result.unwrap_or_else(|error| panic!("{context}: {}", error.message()))
}

#[test]
fn every_reported_standard_feature_has_an_independent_executable_matrix_edge() {
    let root = crate_dir();
    let matrix = std::fs::read_to_string(root.join("tests/fixtures/standard-feature-matrix.tsv"))
        .expect("read standard feature matrix");
    let rows = rows(&matrix);
    let features: BTreeSet<_> = rows.iter().map(|row| row.feature).collect();
    assert_eq!(features, BTreeSet::from(EXPECTED_FEATURES));

    let mut reference_fixtures = BTreeSet::new();
    let mut gates: BTreeMap<&str, &str> = BTreeMap::new();
    for row in &rows {
        assert!(matches!(row.size_profile, "default" | "simd"));
        assert!(row.oracle.starts_with("WABT+JavaScriptCore"));
        if row.feature == "simd" {
            assert!(row.oracle.ends_with("+H5"));
            assert_eq!(row.size_profile, "simd");
        }
        if row.feature == "reference-types" {
            reference_fixtures.insert(row.fixture);
        }

        let fixture = root.join("tests/fixtures").join(row.fixture);
        let bytes = wat::parse_file(&fixture)
            .unwrap_or_else(|error| panic!("compile {}: {error}", fixture.display()));
        let usage = must(WasmModule::from_bytes(&bytes), "decode matrix fixture").feature_usage();
        assert!(
            feature_is_used(usage, row.feature),
            "{} does not exercise {}",
            row.fixture,
            row.feature
        );

        assert!(!row.gate.contains('/'));
        let gate = root.join(row.gate);
        assert!(gate.is_file(), "missing gate {}", gate.display());
        let script = std::fs::read_to_string(&gate).expect("read matrix gate");
        assert!(script.contains("wat2wasm"));
        assert!(script.contains("wasm-validate"));
        assert!(script.contains("JavaScriptCore"));
        gates.entry(row.gate).or_insert(row.oracle);
    }
    assert_eq!(
        reference_fixtures,
        BTreeSet::from(["externref-v1.wat", "funcref-v1.wat"])
    );
    assert_eq!(gates.len(), 10);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "full proposal matrix; run smoke-standard-feature-matrix.sh"]
fn accepted_standard_feature_matrix_executes_all_oracles_and_budgets() {
    let status = std::process::Command::new(crate_dir().join("smoke-standard-feature-matrix.sh"))
        .current_dir(crate_dir().join("../.."))
        .env("CARGO", env!("CARGO"))
        .status()
        .expect("run standard feature matrix");
    assert!(status.success(), "standard feature matrix failed");
}

#[test]
fn matrix_paths_are_repo_local_basenames() {
    let root = crate_dir();
    let matrix = std::fs::read_to_string(root.join("tests/fixtures/standard-feature-matrix.tsv"))
        .expect("read standard feature matrix");
    for row in rows(&matrix) {
        assert_eq!(Path::new(row.fixture).components().count(), 1);
        assert_eq!(Path::new(row.gate).components().count(), 1);
    }
}
