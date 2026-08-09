//! Regression test: Lua task entry execution through the script backend.
//!
//! Verifies that `.lua` entries in the task manifest are recognized,
//! that the `Lua` backend is selectable, and that lua evaluation works.
//!
//! History: originally written against `try_execute_lua_invocation` +
//! `LuaInvocationOptions`; those were folded into `LuaEngineBackend`
//! (Trait-M4, see `plan/design-script-engine-trait.md` §4), so the same
//! scenarios now run through the `ScriptEngineBackend` trait. The
//! `ScriptBackend::Rhai` variant assertions were dropped when that backend
//! was retired (`AGENTERM_SCRIPT_BACKEND=rhai` is now a compat alias for
//! Rh, covered in `tests/rh_backend.rs`).

use agenterm::script_backend::ScriptBackend;
use agenterm::script_engine::{LuaEngineBackend, ScriptEngineBackend, ScriptInvocationOptions};
use serde_json::json;
use std::sync::Mutex;

/// Serializes the env-var-touching tests in this file (integration tests
/// within one target share a process; `AGENTERM_SCRIPT_BACKEND` is global).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_lua_backend<T>(body: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
    }
    let result = body();
    match prior {
        Some(value) => unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
        },
        None => unsafe {
            std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
        },
    }
    result
}

#[test]
fn lua_task_entry_backend_selection() {
    // Verify path-based backend selection.
    assert_eq!(
        ScriptBackend::from_entry_path("scripts/lua/test.lua"),
        ScriptBackend::Lua
    );
    assert_eq!(ScriptBackend::from_entry_path("test.rh"), ScriptBackend::Rh);
    // Retired-rhai entry paths route to Rh.
    assert_eq!(
        ScriptBackend::from_entry_path("test.rhai"),
        ScriptBackend::Rh
    );
    // Unknown extensions default to Rh.
    assert_eq!(
        ScriptBackend::from_entry_path("test.txt"),
        ScriptBackend::Rh
    );
}

#[test]
fn lua_backend_str() {
    assert_eq!(ScriptBackend::Lua.as_str(), "lua");
    assert_eq!(ScriptBackend::Rh.as_str(), "rh");
    assert_eq!(ScriptBackend::Qjs.as_str(), "qjs");
}

#[test]
fn lua_invocation_eval_returns_value() {
    with_lua_backend(|| {
        let result = LuaEngineBackend
            .execute("return 42", &ScriptInvocationOptions::default(), None)
            .expect("lua eval");
        assert_eq!(result.value, Some(json!(42)));
    });
}

#[test]
fn lua_invocation_check_valid_source() {
    with_lua_backend(|| {
        LuaEngineBackend
            .check("return 0", &ScriptInvocationOptions::default())
            .expect("lua check");
    });
}

#[test]
fn lua_invocation_eval_with_print_captures_stdout() {
    with_lua_backend(|| {
        let result = LuaEngineBackend
            .execute(
                "print('hello') return 7",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect("lua eval");
        assert_eq!(result.value, Some(json!(7)));
        assert!(result.stdout.contains("hello"), "stdout: {}", result.stdout);
    });
}

#[test]
fn lua_backend_not_enabled_without_env() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }

    // Under the trait contract, "not selected" is an `enabled()` query, not
    // an `Ok(None)` return (the old try_execute_lua_invocation shape).
    assert!(!LuaEngineBackend.enabled());

    match prior {
        Some(value) => unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
        },
        None => unsafe {
            std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
        },
    }
}
