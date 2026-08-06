//! rh backend switch (`AGENTERM_SCRIPT_BACKEND=rh`) replacement trial.

use std::sync::Mutex;

use agenterm::script_backend::{ScriptBackend, try_execute_rh_invocation};
use agenterm::script_protocol::ScriptOperation;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_rh_backend<T>(run: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "rh");
    }
    let out = run();
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }
    out
}

#[test]
fn rh_backend_enabled_only_when_env_set() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }
    assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rhai);
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "rh");
    }
    assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rh);
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }
}

#[test]
fn rh_backend_check_accepts_entry_fixture() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/entry.rh");
        let result = try_execute_rh_invocation(ScriptOperation::Check, source, None)
            .expect("check")
            .expect("rh handled");
        assert!(result.value.is_none());
    });
}

#[test]
fn rh_backend_eval_runs_source_without_prebuilt_pack() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/entry.rh");
        let result = try_execute_rh_invocation(ScriptOperation::Eval, source, None)
            .expect("eval")
            .expect("rh handled");
        assert_eq!(result.value, Some(serde_json::json!(42)));
        assert!(result.stdout.contains("rh-aot fixture"));
    });
}

#[test]
fn rh_backend_eval_stdlib_fixture_via_host_eval() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/stdlib.rh");
        let result = try_execute_rh_invocation(ScriptOperation::Eval, source, None)
            .expect("eval")
            .expect("rh handled");
        assert_eq!(result.value, Some(serde_json::json!(42)));
    });
}

#[test]
fn rh_backend_run_matches_eval_for_entry_fixture() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/entry.rh");
        let eval = try_execute_rh_invocation(ScriptOperation::Eval, source, None)
            .expect("eval")
            .expect("rh handled");
        let run = try_execute_rh_invocation(ScriptOperation::Run, source, None)
            .expect("run")
            .expect("rh handled");
        assert_eq!(run.value, eval.value);
        assert_eq!(run.stdout, eval.stdout);
    });
}

#[test]
fn rh_backend_run_while_count_fixture() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/while-count.rh");
        let result = try_execute_rh_invocation(ScriptOperation::Run, source, None)
            .expect("run")
            .expect("rh handled");
        assert_eq!(result.value, Some(serde_json::json!(0)));
    });
}

#[test]
fn rhai_backend_returns_none_for_check() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }
    let probe =
        try_execute_rh_invocation(ScriptOperation::Check, "fn entry() { 1 }", None).expect("probe");
    assert!(probe.is_none());
}
