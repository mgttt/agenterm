//! rh backend switch (`AGENTERM_SCRIPT_BACKEND=rh`) replacement trial.

use std::sync::Mutex;

use agenterm::script_backend::{RhInvocationOptions, ScriptBackend, try_execute_rh_invocation};
use agenterm::script_protocol::ScriptOperation;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_rh_backend<T>(run: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
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
fn rh_backend_defaults_to_rh_without_env() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }
    assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rh);
    assert!(agenterm::script_backend::rh_backend_enabled());
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "rhai");
    }
    // `rhai` is a retired backend; the env value survives as a compat alias
    // routed to Rh (see ScriptBackend::from_env).
    assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rh);
    match prior {
        Some(value) => unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
        },
        None => unsafe {
            std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
        },
    }
}

#[test]
fn rh_backend_check_rejects_unknown_api_like_rhai_path() {
    with_rh_backend(|| {
        let result = try_execute_rh_invocation(
            ScriptOperation::Check,
            "fn entry() { std::fs::not_shipped(`x`) }",
            RhInvocationOptions::default(),
            None,
        );
        assert!(result.is_err());
    });
}

#[test]
fn rh_backend_check_accepts_entry_fixture() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/entry.rh");
        let result = try_execute_rh_invocation(
            ScriptOperation::Check,
            source,
            RhInvocationOptions::default(),
            None,
        )
        .expect("check")
        .expect("rh handled");
        assert!(result.value.is_none());
    });
}

#[test]
fn rh_backend_eval_runs_source_without_prebuilt_pack() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/entry.rh");
        let result = try_execute_rh_invocation(
            ScriptOperation::Eval,
            source,
            RhInvocationOptions::default(),
            None,
        )
        .expect("eval")
        .expect("rh handled");
        assert_eq!(result.value, Some(serde_json::json!(42)));
        assert!(result.stdout.contains("rh-aot fixture"));
    });
}

#[test]
fn rh_backend_eval_stdlib_fixture_via_std_exists_fast_path() {
    with_rh_backend(|| {
        // `/tmp` is not a portable existence witness once the native host
        // callback is installed: Windows correctly resolves it on the
        // current drive, where that directory need not exist. Bind the
        // fixture probe to this host's actual temporary directory.
        let source = include_str!("../fixtures/rh/stdlib.rh")
            .replace("`/tmp`", &format!("`{}`", std::env::temp_dir().display()));
        let result = try_execute_rh_invocation(
            ScriptOperation::Eval,
            &source,
            RhInvocationOptions::default(),
            None,
        )
        .expect("eval")
        .expect("rh handled");
        assert_eq!(result.value, Some(serde_json::json!(42)));
        assert!(result.stdout.contains("std-fast-path"));
    });
}

#[test]
fn rh_backend_run_matches_eval_for_entry_fixture() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/entry.rh");
        let eval = try_execute_rh_invocation(
            ScriptOperation::Eval,
            source,
            RhInvocationOptions::default(),
            None,
        )
        .expect("eval")
        .expect("rh handled");
        let run = try_execute_rh_invocation(
            ScriptOperation::Run,
            source,
            RhInvocationOptions::default(),
            None,
        )
        .expect("run")
        .expect("rh handled");
        assert_eq!(run.value, eval.value);
        assert_eq!(run.stdout, eval.stdout);
    });
}

#[test]
fn rh_backend_native_throw_surfaces_host_error() {
    with_rh_backend(|| {
        let source = r#"fn entry() { throw "native boom"; }"#;
        let result = try_execute_rh_invocation(
            ScriptOperation::Run,
            source,
            RhInvocationOptions {
                project_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
                ..RhInvocationOptions::default()
            },
            None,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("native throw must fail"),
        };
        let detail = error.to_string();
        assert!(
            detail.contains("rh_fail:") && detail.contains("native boom"),
            "missing stable native throw host error detail: {error}"
        );

        let healthy = try_execute_rh_invocation(
            ScriptOperation::Run,
            "fn entry() { 42 }",
            RhInvocationOptions {
                project_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
                ..RhInvocationOptions::default()
            },
            None,
        )
        .expect("post-error run")
        .expect("rh handled");
        assert_eq!(healthy.value, Some(serde_json::json!(42)));
    });
}

#[test]
fn rh_backend_native_file_read_failure_is_an_error() {
    with_rh_backend(|| {
        let missing = std::env::temp_dir().join(format!(
            "agenterm-rh-host-eval-missing-{}/file.txt",
            std::process::id()
        ));
        let source = format!(
            "fn entry() {{ std::fs::read_to_string({}); 42 }}",
            serde_json::to_string(&missing.to_string_lossy()).expect("path json")
        );
        let result = try_execute_rh_invocation(
            ScriptOperation::Eval,
            &source,
            RhInvocationOptions {
                project_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
                ..RhInvocationOptions::default()
            },
            None,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("native file-read failure must fail the entry"),
        };
        assert!(
            error.to_string().contains("rh_std_fs_read_to_string:"),
            "missing stable host error detail: {error}"
        );
    });
}

#[test]
fn rh_backend_run_while_count_fixture() {
    with_rh_backend(|| {
        let source = include_str!("../fixtures/rh/while-count.rh");
        let result = try_execute_rh_invocation(
            ScriptOperation::Run,
            source,
            RhInvocationOptions::default(),
            None,
        )
        .expect("run")
        .expect("rh handled");
        assert_eq!(result.value, Some(serde_json::json!(0)));
    });
}

#[test]
fn rhai_env_value_routes_check_to_rh_backend() {
    // Pre-retirement, `AGENTERM_SCRIPT_BACKEND=rhai` selected a distinct
    // rhai backend and this probe returned None (rh not selected). After
    // the rhai retirement (Wave 4.5-A/B), `rhai` is a compat alias for Rh,
    // so the same env value now routes the check to the rh backend — this
    // test locks the alias behavior.
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "rhai");
    }
    let probe = try_execute_rh_invocation(
        ScriptOperation::Check,
        "fn entry() { 1 }",
        RhInvocationOptions::default(),
        None,
    )
    .expect("probe");
    assert!(
        probe.is_some(),
        "rhai env value should alias to the rh backend"
    );
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }
}

#[test]
fn rh_backend_run_honors_task_arguments() {
    with_rh_backend(|| {
        let source = "fn entry() { args.len() }";
        let result = try_execute_rh_invocation(
            ScriptOperation::Run,
            source,
            RhInvocationOptions {
                project_root: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
                arguments: Some(serde_json::json!(["alpha", "beta"])),
                ..RhInvocationOptions::default()
            },
            None,
        )
        .expect("run")
        .expect("rh handled");
        assert_eq!(result.value, Some(serde_json::json!(2)));
    });
}
