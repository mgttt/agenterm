//! rh fixture corpus and check-many manifest validation.

use std::path::PathBuf;

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
    assert_eq!(report.checked_files, 5);
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
