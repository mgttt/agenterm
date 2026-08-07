//! rh fixture corpus and check-many manifest validation.

use std::path::PathBuf;

#[test]
fn scripts_rhai_corpus_scan_from_integration_test() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report = agenterm_rh::scan_rhai_directory(agenterm_rh::CorpusScanOptions {
        project_root: repo.clone(),
        relative_dir: "scripts/rhai".to_owned(),
        ..Default::default()
    })
    .expect("scan");
    assert!(report.scanned >= 50);
    assert_eq!(report.failed, 0);
    assert_eq!(report.passed, report.scanned);
}

#[test]
fn task_manifest_corpus_scan_from_integration_test() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report = agenterm_rh::scan_task_manifest(agenterm_rh::CorpusScanOptions {
        project_root: repo,
        ..Default::default()
    })
    .expect("scan tasks");
    assert_eq!(report.kind, "agenterm-rh-corpus-scan-tasks");
    assert!(report.scanned >= 50);
    assert_eq!(report.failed, 0);
    assert_eq!(report.passed, report.scanned);
}

#[test]
fn caller_inventory_hit_count_baseline_guard() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let baseline_path = repo.join("fixtures/rh/caller-inventory-baseline.json");
    let baseline_raw = std::fs::read_to_string(&baseline_path).expect("baseline fixture");
    let baseline: serde_json::Value = serde_json::from_str(&baseline_raw).expect("baseline json");
    let min_hit_count = baseline["min_hit_count"].as_u64().expect("min_hit_count") as usize;
    let min_categories = baseline["min_categories"]
        .as_object()
        .expect("min_categories");
    let max_categories = baseline["max_categories"]
        .as_object()
        .expect("max_categories");

    let report = agenterm_rh::scan_caller_inventory(agenterm_rh::CallerInventoryOptions {
        project_root: repo,
    })
    .expect("inventory");

    assert!(
        report.hit_count >= min_hit_count,
        "hit_count {} below baseline {}",
        report.hit_count,
        min_hit_count
    );
    for (category, min) in min_categories {
        let min = min.as_u64().expect("category min") as usize;
        let actual = report
            .categories
            .get(category.as_str())
            .copied()
            .unwrap_or(0);
        assert!(
            actual >= min,
            "category {category}: {actual} below baseline {min}"
        );
    }
    for (category, max) in max_categories {
        let max = max.as_u64().expect("category max") as usize;
        let actual = report
            .categories
            .get(category.as_str())
            .copied()
            .unwrap_or(0);
        assert!(
            actual <= max,
            "category {category}: {actual} above migration ceiling {max}"
        );
    }
    assert!(report.categories.contains_key("bootstrap"));
    assert!(!report.categories.contains_key("ci"));
}

#[test]
fn fixture_check_many_manifest_is_valid() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = repo.join("fixtures/rh/check-many.json");
    let manifest = agenterm_rh::read_manifest(&manifest_path).expect("manifest");
    let expected_files = manifest.files.len();
    let report = agenterm_rh::run_check_many(
        manifest,
        agenterm_rh::CheckManyOptions {
            project_root: repo,
            ..agenterm_rh::CheckManyOptions::default()
        },
    );
    assert!(report.ok, "failures: {:?}", report.failures);
    assert_eq!(report.checked_files, expected_files);
}

#[test]
fn while_count_fixture_qualifies_to_zero() {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-while-count-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/while-count.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 0);
    let _ = std::fs::remove_dir_all(&dir);
}
