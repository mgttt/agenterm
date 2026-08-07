//! Regression test: Lua task entry execution through ScriptBackend.
//!
//! Verifies that `.lua` entries in the task manifest are recognized,
//! that the `Lua` backend is selectable, and that lua evaluation works.

#[test]
fn lua_task_entry_backend_selection() {
    // Verify path-based backend selection.
    assert_eq!(
        agenterm::script_backend::ScriptBackend::from_entry_path("scripts/lua/test.lua"),
        agenterm::script_backend::ScriptBackend::Lua
    );
    assert_eq!(
        agenterm::script_backend::ScriptBackend::from_entry_path("test.rh"),
        agenterm::script_backend::ScriptBackend::Rh
    );
    assert_eq!(
        agenterm::script_backend::ScriptBackend::from_entry_path("test.rhai"),
        agenterm::script_backend::ScriptBackend::Rhai
    );
    // Unknown extensions default to Rh.
    assert_eq!(
        agenterm::script_backend::ScriptBackend::from_entry_path("test.txt"),
        agenterm::script_backend::ScriptBackend::Rh
    );
}

#[test]
fn lua_backend_str() {
    assert_eq!(agenterm::script_backend::ScriptBackend::Lua.as_str(), "lua");
    assert_eq!(agenterm::script_backend::ScriptBackend::Rh.as_str(), "rh");
    assert_eq!(agenterm::script_backend::ScriptBackend::Rhai.as_str(), "rhai");
}

#[test]
fn lua_invocation_eval_returns_value() {
    // Set env to force Lua backend, then evaluate.
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
    }

    let result = agenterm::script_backend::try_execute_lua_invocation(
        agenterm::script_protocol::ScriptOperation::Eval,
        "return 42",
        agenterm::script_backend::LuaInvocationOptions::default(),
        None,
    )
    .expect("probe")
    .expect("lua result");

    assert_eq!(result.value, Some(42));

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
fn lua_invocation_check_valid_source() {
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
    }

    let result = agenterm::script_backend::try_execute_lua_invocation(
        agenterm::script_protocol::ScriptOperation::Check,
        "return 0",
        agenterm::script_backend::LuaInvocationOptions::default(),
        None,
    )
    .expect("probe")
    .expect("lua result");

    assert!(result.value.is_none());

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
fn lua_invocation_eval_with_print_captures_stdout() {
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
    }

    let result = agenterm::script_backend::try_execute_lua_invocation(
        agenterm::script_protocol::ScriptOperation::Eval,
        "print('hello') return 7",
        agenterm::script_backend::LuaInvocationOptions::default(),
        None,
    )
    .expect("probe")
    .expect("lua result");

    assert_eq!(result.value, Some(7));
    assert!(result.stdout.contains("hello"), "stdout: {}", result.stdout);

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
fn lua_invocation_not_enabled_without_env() {
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
    }

    let result = agenterm::script_backend::try_execute_lua_invocation(
        agenterm::script_protocol::ScriptOperation::Eval,
        "return 99",
        agenterm::script_backend::LuaInvocationOptions::default(),
        None,
    )
    .expect("probe");

    assert!(result.is_none(), "should return None when Lua backend not enabled");

    match prior {
        Some(value) => unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
        },
        None => unsafe {
            std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
        },
    }
}
