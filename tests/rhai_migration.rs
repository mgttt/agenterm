use std::fs;
use std::path::Path;

#[test]
fn check_task_is_rh_and_archives_interpreted_source() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(repo.join("agenterm.tasks.json")).expect("manifest");
    assert!(manifest.contains("\"entry\": \"scripts/rh/check.rh\""));
    assert!(!manifest.contains("\"entry\": \"scripts/rhai/check.rhai\""));
    assert!(repo.join("scripts/archive/rhai/check.rhai").is_file());
    assert!(!repo.join("scripts/rhai/check.rhai").exists());
    assert!(
        fs::read_to_string(repo.join("scripts/rh/check.rh"))
            .expect("native check task")
            .contains("native public catalog clients")
    );
}
