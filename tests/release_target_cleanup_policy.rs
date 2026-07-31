const BOOTSTRAP: &str = include_str!("../scripts/bootstrap.cmd");
const BUILD: &str = include_str!("../scripts/rhai/build.rhai");

#[test]
fn release_cleanup_reclaims_both_repo_targets_after_staging() {
    let release_cleanup = BUILD
        .split("if profile == \"release\" && !external_target {")
        .nth(1)
        .expect("release cleanup branch");
    assert!(release_cleanup.contains("build_target_clean"));
    assert!(release_cleanup.contains("build_development_target_clean"));
    assert!(release_cleanup.contains("development_target"));
    assert!(release_cleanup.contains("prepare-target-clean"));
    assert!(release_cleanup.contains("[\"clean\", \"--target-dir\", development_target]"));
}

#[test]
fn bootstrap_worker_never_executes_from_a_repo_cargo_target() {
    assert!(BOOTSTRAP.contains("AGENTERM_BOOTSTRAP_CACHE_ROOT"));
    assert!(BOOTSTRAP.contains("%LOCALAPPDATA%\\AgenTerm\\build-cache"));
    assert!(BOOTSTRAP.contains("%TEMP%\\AgenTerm-build-cache"));
    assert!(BOOTSTRAP.contains("AGENTERM_BOOTSTRAP_DIR=%AGENTERM_BOOTSTRAP_CACHE_DIR%\\task-"));
    assert!(!BOOTSTRAP.contains("AGENTERM_BOOTSTRAP_DIR=%AGENTERM_BOOTSTRAP_TARGET%\\task-"));
}
