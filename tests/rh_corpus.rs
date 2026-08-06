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
    assert!(report.failed > 0);
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
    assert!(report.passed >= 1);
    assert!(report.failed > 0);
}

#[test]
fn caller_inventory_lists_bootstrap_and_ci() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report = agenterm_rh::scan_caller_inventory(agenterm_rh::CallerInventoryOptions {
        project_root: repo,
    })
    .expect("inventory");
    assert!(report.hit_count >= 40);
    assert!(report.categories.get("bootstrap").copied().unwrap_or(0) >= 1);
    assert!(report.categories.get("ci").copied().unwrap_or(0) >= 5);
}

#[test]
fn fixture_check_many_manifest_is_valid() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = repo.join("fixtures/rh/check-many.json");
    let manifest = agenterm_rh::read_manifest(&manifest_path).expect("manifest");
    let report = agenterm_rh::run_check_many(
        manifest,
        agenterm_rh::CheckManyOptions {
            project_root: repo,
            ..agenterm_rh::CheckManyOptions::default()
        },
    );
    assert!(report.ok, "failures: {:?}", report.failures);
    assert_eq!(report.checked_files, 7);
}

#[test]
fn while_count_fixture_qualifies_to_zero() {
    let dir =
        std::env::temp_dir().join(format!("agenterm-rh-while-count-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/while-count.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 0);
    let _ = std::fs::remove_dir_all(&dir);
}
