const BOOTSTRAP: &str = include_str!("../scripts/bootstrap.cmd");
const UNIX_BOOTSTRAP: &str = include_str!("../scripts/bootstrap.sh");
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

#[test]
fn bootstrap_exposes_one_stable_cross_platform_rustc_wrapper_path() {
    assert!(BOOTSTRAP.contains("set \"AGENTERM_BOOTSTRAP_CACHE_WORKER="));
    assert!(UNIX_BOOTSTRAP.contains("AGENTERM_BOOTSTRAP_CACHE_WORKER=\"$CACHE_WORKER\""));
    assert!(
        UNIX_BOOTSTRAP.contains("export AGENTERM_BOOTSTRAP_WORKER AGENTERM_BOOTSTRAP_CACHE_WORKER")
    );
}

#[test]
fn bootstrap_prefers_rh_task_front_door_with_explicit_compat_binary() {
    assert!(BOOTSTRAP.contains("set \"AGENTERM_RHAI_COMPAT_CLI=%AGENTERM_BOOTSTRAP_WORKER%\""));
    assert!(BOOTSTRAP.contains(
        "if defined AGENTERM_BOOTSTRAP_RH_CLI set \"AGENTERM_BOOTSTRAP_TASK_CLI=%AGENTERM_BOOTSTRAP_RH_CLI%\""
    ));
    assert!(BOOTSTRAP.contains("\"%AGENTERM_BOOTSTRAP_TASK_CLI%\" task run"));

    assert!(UNIX_BOOTSTRAP.contains("AGENTERM_RHAI_COMPAT_CLI=\"$WORKER\""));
    assert!(UNIX_BOOTSTRAP.contains("TASK_CLI=\"$AGENTERM_BOOTSTRAP_RH_CLI\""));
    assert!(UNIX_BOOTSTRAP.contains("\"$TASK_CLI\" task run"));
}
